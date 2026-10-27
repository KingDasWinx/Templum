//! Cells copied out of a terminal for painting. Painting never waits on the PTY thread:
//! if it holds the lock (heavy output), the previous snapshot is reused for up to MAX_STALE.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use alacritty_terminal::grid::{Dimensions, Row as GridRow};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::selection::SelectionRange;
use alacritty_terminal::term::TermDamage;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::CursorShape;

use crate::palette::{self, Palette};
use crate::scrollback::Scrollback;
use crate::tap::{Mark, line_mark, mark_of};
use crate::{Rgb, SharedTerm};

const MAX_STALE: Duration = Duration::from_millis(100);

/// Text with one style; `len` is in bytes of [`Row::text`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
    pub len: usize,
    pub fg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub dim: bool,
}

/// Columns `start..start + len`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub start: usize,
    pub len: usize,
}

/// Text drawn from afar: a run of non-blank cells of one color, columns `cols`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bar {
    pub cols: Span,
    pub fg: Rgb,
    pub dim: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// One char per column (wide-char spacers become blanks), trailing blanks trimmed.
    pub text: String,
    pub runs: Vec<Run>,
    /// Backgrounds other than the default one.
    pub bgs: Vec<(Span, Rgb)>,
    pub selected: Vec<Span>,
    /// A prompt starts here, and the command run from it is in this state (shell integration).
    pub mark: Option<Mark>,
}

impl Row {
    fn push(&mut self, c: char, style: Run) {
        self.text.push(c);
        let n = c.len_utf8();
        match self.runs.last_mut() {
            Some(last)
                if Run {
                    len: last.len,
                    ..style
                } == *last =>
            {
                last.len += n
            }
            _ => self.runs.push(Run { len: n, ..style }),
        }
    }

    fn push_bg(&mut self, col: usize, bg: Rgb) {
        match self.bgs.last_mut() {
            Some((span, color)) if *color == bg && span.start + span.len == col => span.len += 1,
            _ => self.bgs.push((Span { start: col, len: 1 }, bg)),
        }
    }

    fn push_selected(&mut self, col: usize) {
        match self.selected.last_mut() {
            Some(span) if span.start + span.len == col => span.len += 1,
            _ => self.selected.push(Span { start: col, len: 1 }),
        }
    }

    /// Empties the row for a rebuild, keeping its buffers.
    fn clear(&mut self) {
        self.text.clear();
        self.runs.clear();
        self.bgs.clear();
        self.selected.clear();
        self.mark = None;
    }

    /// Trailing blanks are not shaped.
    fn trim(&mut self) {
        let mut extra = self.text.len() - self.text.trim_end_matches(' ').len();
        self.text.truncate(self.text.len() - extra);
        while extra > 0 {
            let Some(last) = self.runs.last_mut() else {
                break;
            };
            let n = last.len.min(extra);
            last.len -= n;
            extra -= n;
            if last.len == 0 {
                self.runs.pop();
            }
        }
    }

    /// The row's text as bars: one per run of non-blank cells of one color (bold and italic don't
    /// count), for a terminal seen from far away. One column per character: a wide character's
    /// spacer is a blank, so its bar is its first column.
    pub fn bars(&self) -> Vec<Bar> {
        let mut bars: Vec<Bar> = Vec::new();
        let (mut byte, mut col) = (0, 0);
        for run in &self.runs {
            // `push` and `trim` keep the runs on `text`'s char boundaries.
            for c in self.text[byte..byte + run.len].chars() {
                if c != ' ' {
                    match bars.last_mut() {
                        Some(b)
                            if b.cols.start + b.cols.len == col
                                && b.fg == run.fg
                                && b.dim == run.dim =>
                        {
                            b.cols.len += 1
                        }
                        _ => bars.push(Bar {
                            cols: Span { start: col, len: 1 },
                            fg: run.fg,
                            dim: run.dim,
                        }),
                    }
                }
                col += 1;
            }
            byte += run.len;
        }
        bars
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    /// As the terminal resolves it: the program's DECSCUSR, else the configured default.
    pub shape: CursorShape,
    pub blinking: bool,
}

#[derive(Debug, Default)]
pub struct Snapshot {
    /// One row per visible line.
    pub rows: Vec<Row>,
    /// None when hidden or scrolled out of view.
    pub cursor: Option<Cursor>,
    /// Where the view was in the history at the last read.
    pub scrollback: Scrollback,
    /// Rows copied by the last `refresh`: every row, the changed ones, or none (nothing changed).
    pub rebuilt: usize,
    taken: Option<Instant>,
    /// The palette the rows were resolved with: another one means every row again.
    last_palette: Option<Palette>,
    /// Each row's prompt number (see tap.rs), kept with the row.
    prompts: Vec<Option<u32>>,
    /// The selection the rows show: alacritty's damage doesn't track it.
    last_selection: Option<SelectionRange>,
}

impl Snapshot {
    /// Re-reads the visible grid, resolving colors with `palette`, once the terminal changed (its
    /// output, a scroll, the selection, a resize) or the palette did; otherwise the cells are
    /// current and nothing is copied. If the PTY thread holds the lock, the previous cells are
    /// kept (past MAX_STALE it waits). Returns false when it kept them.
    pub fn refresh(&mut self, term: &SharedTerm, palette: &Palette) -> bool {
        let heard = &term.1;
        let same_palette = self.last_palette == Some(*palette);
        if self.taken.is_some() && same_palette && !term.dirty() {
            // Still current: it only goes stale while output waits behind a busy lock.
            self.taken = Some(Instant::now());
            self.rebuilt = 0;
            return true;
        }
        let stale = self.taken.is_none_or(|t| t.elapsed() > MAX_STALE);
        let mut term = match term.0.try_lock_unfair() {
            Some(term) => term,
            // Unfair: waits for the parse under way (at most MAX_LOCKED_READ bytes), not for
            // the PTY thread's whole read pass as the fair lock would.
            None if stale => term.0.lock_unfair(),
            None => return false,
        };
        // Cleared under the lock: output parsed after this point sets it again.
        heard.dirty.store(false, Ordering::Release);
        let screen_rows = term.screen_lines();
        // The viewport rows alacritty changed since the last read; None: all of them.
        let damaged: Option<Vec<usize>> = match term.damage() {
            TermDamage::Full => None,
            TermDamage::Partial(lines) => Some(lines.map(|l| l.line).collect()),
        };
        term.reset_damage();
        let blinking = term.cursor_style().blinking;
        self.scrollback = Scrollback::of(&term);
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        let selection = content.selection;
        // Everything: full damage, a resize, another palette, or a selection change (not in the
        // damage).
        let all = damaged.is_none()
            || self.rows.len() != screen_rows
            || !same_palette
            || selection != self.last_selection;
        if all {
            self.rows.resize_with(screen_rows, Row::default);
            self.prompts.resize(screen_rows, None);
        }
        let lines: Vec<usize> = match damaged {
            Some(lines) if !all => lines,
            _ => (0..screen_rows).collect(),
        };
        self.cursor = None;
        if content.cursor.shape != CursorShape::Hidden {
            let row = content.cursor.point.line.0 + offset;
            if (0..screen_rows as i32).contains(&row) {
                self.cursor = Some(Cursor {
                    row: row as usize,
                    col: content.cursor.point.column.0,
                    shape: content.cursor.shape,
                    blinking,
                });
            }
        }
        let grid = term.grid();
        for &r in &lines {
            let Some(row) = self.rows.get_mut(r) else {
                continue;
            };
            let line = Line(r as i32 - offset);
            let cells = &grid[line];
            rebuild(
                row,
                &mut self.prompts[r],
                cells,
                line,
                selection,
                content.colors,
                palette,
            );
        }
        self.rebuilt = lines.len();
        // A prompt starts on the first of its lines: the one above the view may be one of them.
        let above = (offset < grid.history_size() as i32)
            .then(|| line_mark(grid, Line(-offset - 1)))
            .flatten();
        drop(term);
        if self.prompts.iter().any(Option::is_some) {
            let marks = heard.marks.lock().unwrap();
            let mut last = above;
            // Every row, rebuilt or not: a row stops being a prompt's first line when the line
            // above it joins the same prompt.
            for (row, prompt) in self.rows.iter_mut().zip(&self.prompts) {
                row.mark = prompt
                    .filter(|_| *prompt != last)
                    .and_then(|p| marks.get(&p).copied());
                last = *prompt;
            }
        }
        self.last_selection = selection;
        self.last_palette = Some(*palette);
        self.taken = Some(Instant::now());
        true
    }

    /// The visible text, one line per row (tests, debugging).
    pub fn text(&self) -> String {
        self.rows
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Copies one grid line into `row`, with its prompt number if a prompt starts on it.
fn rebuild(
    row: &mut Row,
    prompt: &mut Option<u32>,
    cells: &GridRow<Cell>,
    line: Line,
    selection: Option<SelectionRange>,
    colors: &Colors,
    palette: &Palette,
) {
    row.clear();
    *prompt = None;
    for col in 0..cells.len() {
        let cell = &cells[Column(col)];
        if prompt.is_none() && cell.extra.is_some() {
            *prompt = mark_of(cell);
        }
        let mut fg = palette::resolve(cell.fg, colors, palette);
        let mut bg = palette::resolve(cell.bg, colors, palette);
        if cell.flags.contains(Flags::INVERSE) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if cell.flags.contains(Flags::HIDDEN) {
            fg = bg;
        }
        if bg != palette.background {
            row.push_bg(col, bg);
        }
        if selection.is_some_and(|s| s.contains(Point::new(line, Column(col)))) {
            row.push_selected(col);
        }
        // A wide char's spacer becomes a blank so later glyphs stay on their columns.
        let c = if cell.flags.contains(Flags::WIDE_CHAR_SPACER) || cell.c < ' ' {
            ' '
        } else {
            cell.c
        };
        row.push(
            c,
            Run {
                len: 0,
                fg,
                bold: cell.flags.contains(Flags::BOLD),
                italic: cell.flags.contains(Flags::ITALIC),
                dim: cell.flags.contains(Flags::DIM),
            },
        );
    }
    row.trim();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{feed, feed_heard, headless};
    use alacritty_terminal::term::color::Colors;
    use alacritty_terminal::vte::ansi::{Color, NamedColor};

    fn snap(bytes: &[u8]) -> Snapshot {
        let (term, _) = headless(20, 4);
        feed(&term, bytes);
        let mut s = Snapshot::default();
        assert!(s.refresh(&term, &Palette::default()));
        s
    }

    #[test]
    fn colors_runs_and_trimming() {
        let s = snap(b"\x1b[31mred\x1b[0m ok   ");
        assert_eq!(s.rows.len(), 4);
        assert_eq!(s.rows[0].text, "red ok");
        let red = palette::resolve(
            Color::Named(NamedColor::Red),
            &Colors::default(),
            &Palette::default(),
        );
        assert_eq!(
            s.rows[0].runs[0],
            Run {
                len: 3,
                fg: red,
                bold: false,
                italic: false,
                dim: false
            }
        );
        assert_eq!(
            s.rows[0].runs.iter().map(|r| r.len).sum::<usize>(),
            "red ok".len()
        );
        assert_eq!(s.text(), "red ok\n\n\n");
    }

    #[test]
    fn inverse_and_dim_and_bold() {
        let s = snap(b"\x1b[7mX\x1b[0m\x1b[2;1mY");
        let p = Palette::default();
        assert_eq!(
            s.rows[0].bgs,
            vec![(Span { start: 0, len: 1 }, p.foreground)]
        );
        assert_eq!(s.rows[0].runs[0].fg, p.background);
        assert!(s.rows[0].runs[1].dim && s.rows[0].runs[1].bold);
    }

    #[test]
    fn wide_chars_keep_later_glyphs_on_their_columns() {
        let s = snap("你好x".as_bytes());
        assert_eq!(s.rows[0].text, "你 好 x");
    }

    #[test]
    fn cursor_position() {
        let s = snap(b"abc\r\nde");
        let c = s.cursor.unwrap();
        assert_eq!((c.row, c.col), (1, 2));
    }

    #[test]
    fn cursor_shape_comes_from_options_unless_the_program_asks() {
        use crate::{CursorShape, CursorStyle, TermOptions};
        let (term, _) = headless(20, 4);
        term.set_options(TermOptions {
            cursor: CursorStyle {
                shape: CursorShape::Beam,
                blinking: true,
            },
            ..TermOptions::default()
        });
        let mut s = Snapshot::default();
        s.refresh(&term, &Palette::default());
        let c = s.cursor.unwrap();
        assert_eq!((c.shape, c.blinking), (CursorShape::Beam, true));
        // DECSCUSR 2: a steady block, asked for by the program.
        feed(&term, b"\x1b[2 q");
        s.refresh(&term, &Palette::default());
        let c = s.cursor.unwrap();
        assert_eq!((c.shape, c.blinking), (CursorShape::Block, false));
    }

    #[test]
    fn refresh_resolves_cells_with_the_given_palette() {
        let (term, _) = headless(20, 4);
        // Red text, then one cell with a red background.
        feed(&term, b"\x1b[31mr\x1b[0m\x1b[41m \x1b[0m");
        let mark = Rgb { r: 1, g: 2, b: 3 };
        let p = Palette {
            ansi: [mark; 16],
            ..Palette::default()
        };
        let mut s = Snapshot::default();
        assert!(s.refresh(&term, &p));
        assert_eq!(s.rows[0].runs[0].fg, mark);
        assert_eq!(s.rows[0].bgs, vec![(Span { start: 1, len: 1 }, mark)]);
    }

    #[test]
    fn refresh_copies_only_after_output_or_a_change_from_the_ui() {
        use crate::{SelectionKind, Side};
        let (term, _) = headless(20, 4);
        feed(&term, b"abc");
        let p = Palette::default();
        let mut s = Snapshot::default();
        assert!(s.refresh(&term, &p));
        assert_eq!(s.rebuilt, 4, "the first read copies every row");
        assert!(s.refresh(&term, &p), "nothing happened: still fresh");
        assert_eq!(s.rebuilt, 0, "nothing happened: nothing is copied");
        // Output (feed marks the terminal changed, as the PTY thread's wake-up does).
        feed(&term, b"d");
        s.refresh(&term, &p);
        assert!(s.rebuilt > 0);
        assert_eq!(s.rows[0].text, "abcd");
        s.refresh(&term, &p);
        assert_eq!(s.rebuilt, 0);
        // A selection: alacritty's damage doesn't track it, the UI call does.
        term.select_start(0, 0, Side::Left, SelectionKind::Simple);
        term.select_update(0, 1, Side::Right);
        s.refresh(&term, &p);
        assert_eq!(s.rebuilt, 4);
        assert_eq!(s.rows[0].selected, vec![Span { start: 0, len: 2 }]);
        s.refresh(&term, &p);
        assert_eq!(s.rebuilt, 0);
        // Another palette (a theme change): the colors are resolved again at once.
        let other = Palette {
            ansi: [Rgb { r: 1, g: 2, b: 3 }; 16],
            ..Palette::default()
        };
        feed(&term, b"\x1b[31m!");
        s.refresh(&term, &p);
        s.refresh(&term, &other);
        assert_eq!(s.rebuilt, 4);
        assert_eq!(s.rows[0].runs.last().unwrap().fg, Rgb { r: 1, g: 2, b: 3 });
    }

    #[test]
    fn a_terminal_idle_for_a_while_never_waits_on_its_first_output() {
        use std::sync::mpsc;
        use std::time::Duration;
        let (term, _) = headless(20, 4);
        let p = Palette::default();
        let mut s = Snapshot::default();
        s.refresh(&term, &p);
        // Idle past MAX_STALE, painted every frame: nothing changed, the snapshot is current.
        std::thread::sleep(MAX_STALE + Duration::from_millis(50));
        assert!(s.refresh(&term, &p));
        // Output, while the PTY thread holds the lock for a long parse.
        feed(&term, b"x");
        let (locked, wait) = mpsc::channel();
        let pty = {
            let term = term.clone();
            std::thread::spawn(move || {
                let _held = term.0.lock_unfair();
                locked.send(()).unwrap();
                std::thread::sleep(Duration::from_millis(300));
            })
        };
        wait.recv().unwrap();
        let start = Instant::now();
        assert!(!s.refresh(&term, &p), "the previous cells are drawn");
        assert!(start.elapsed() < MAX_STALE, "waited {:?}", start.elapsed());
        pty.join().unwrap();
    }

    #[test]
    fn only_the_rows_the_terminal_damaged_are_rebuilt() {
        let (term, _) = headless(20, 4);
        feed(&term, b"abc\r\ndef\r\nghi");
        let p = Palette::default();
        let mut s = Snapshot::default();
        s.refresh(&term, &p);
        assert_eq!(s.rebuilt, 4, "a new terminal is fully damaged");
        // Overwrite the second line: alacritty damages the cursor's old line (the third, where
        // it left) and the line written.
        feed(&term, b"\x1b[2;1HXYZ");
        s.refresh(&term, &p);
        assert_eq!(s.text(), "abc\nXYZ\nghi\n");
        assert_eq!(s.rebuilt, 2);
        // A line feed at the bottom scrolls: everything is damaged.
        feed(&term, b"\x1b[4;1H\r\n");
        s.refresh(&term, &p);
        assert_eq!(s.text(), "XYZ\nghi\n\n");
        assert_eq!(s.rebuilt, 4);
    }

    #[test]
    fn a_resize_rebuilds_every_row_at_the_new_size() {
        use alacritty_terminal::term::test::TermSize;
        let (term, _) = headless(20, 4);
        feed(&term, b"one\r\ntwo");
        let mut s = Snapshot::default();
        s.refresh(&term, &Palette::default());
        term.0.lock_unfair().resize(TermSize::new(20, 6));
        term.touched();
        s.refresh(&term, &Palette::default());
        assert_eq!(s.rows.len(), 6);
        assert_eq!(s.rebuilt, 6);
        assert_eq!(s.text(), "one\ntwo\n\n\n\n");
    }

    #[test]
    fn a_prompts_mark_survives_rows_that_are_not_rebuilt() {
        let (term, _) = headless(20, 4);
        // A prompt whose command failed (see tap.rs), then another prompt.
        feed_heard(
            &term,
            b"\x1b]133;A\x07$ false\r\n\x1b]133;C\x07\x1b]133;D;1\x07\x1b]133;A\x07$ ",
        );
        let mut s = Snapshot::default();
        s.refresh(&term, &Palette::default());
        assert_eq!(s.rows[0].mark, Some(Mark::Ended(Some(1))));
        // Typing on the last prompt's line damages that line only: the first keeps its mark.
        feed(&term, b"ls");
        s.refresh(&term, &Palette::default());
        assert_eq!(s.rebuilt, 1);
        assert_eq!(s.rows[0].mark, Some(Mark::Ended(Some(1))));
        assert_eq!(s.rows[1].text, "$ ls");
    }

    #[test]
    fn a_row_that_stops_being_a_prompts_first_line_loses_its_mark() {
        // A prompt on the second line whose command succeeded, the cursor parked below it.
        let (term, _) = headless(20, 4);
        feed_heard(
            &term,
            b"\r\n\x1b]133;A\x07$ \x1b]133;C\x07\x1b]133;D;0\x07\x1b[4;1H",
        );
        let p = Palette::default();
        let mut s = Snapshot::default();
        s.refresh(&term, &p);
        assert_eq!(s.rows[1].mark, Some(Mark::Ended(Some(0))));
        // The shell redraws the line above inside the same prompt (its link), and the cursor goes
        // back below: the prompt now starts on the first line, and the second line isn't damaged.
        feed(
            &term,
            b"\x1b[1;1H\x1b]8;;templum:cmd/0\x1b\\>\x1b]8;;\x1b\\\x1b[4;1H",
        );
        s.refresh(&term, &p);
        let mut fresh = Snapshot::default();
        term.touched();
        fresh.refresh(&term, &p);
        let marks = |s: &Snapshot| s.rows.iter().map(|r| r.mark).collect::<Vec<_>>();
        assert_eq!(marks(&fresh)[..2], [Some(Mark::Ended(Some(0))), None]);
        assert_eq!(marks(&s), marks(&fresh));
    }

    #[test]
    fn selection_spans() {
        use crate::{SelectionKind, Side};
        let (term, _) = headless(20, 4);
        feed(&term, b"hello world");
        term.select_start(0, 0, Side::Left, SelectionKind::Simple);
        term.select_update(0, 4, Side::Right);
        let mut s = Snapshot::default();
        s.refresh(&term, &Palette::default());
        assert_eq!(s.rows[0].selected, vec![Span { start: 0, len: 5 }]);
    }

    fn bar(start: usize, len: usize, fg: Rgb, dim: bool) -> Bar {
        Bar {
            cols: Span { start, len },
            fg,
            dim,
        }
    }

    fn red() -> Rgb {
        palette::resolve(
            Color::Named(NamedColor::Red),
            &Colors::default(),
            &Palette::default(),
        )
    }

    #[test]
    fn row_bars_are_runs_of_non_blank_cells_of_one_color() {
        let fg = Palette::default().foreground;
        let s = snap(b"\x1b[31mred\x1b[0m ok");
        assert_eq!(
            s.rows[0].bars(),
            vec![bar(0, 3, red(), false), bar(4, 2, fg, false)]
        );
        // A colored run with a space inside: two bars.
        let s = snap(b"\x1b[31mgo on");
        assert_eq!(
            s.rows[0].bars(),
            vec![bar(0, 2, red(), false), bar(3, 2, red(), false)]
        );
        // Leading blanks have none; bold and italic don't cut a bar.
        let s = snap(b"   \x1b[1mab\x1b[0;3mcd");
        assert_eq!(s.rows[0].bars(), vec![bar(3, 4, fg, false)]);
        assert!(s.rows[1].bars().is_empty(), "an empty row");
    }

    #[test]
    fn row_bars_count_columns_in_characters() {
        let fg = Palette::default().foreground;
        // é is two bytes, one column.
        let s = snap("é ab".as_bytes());
        assert_eq!(
            s.rows[0].bars(),
            vec![bar(0, 1, fg, false), bar(2, 2, fg, false)]
        );
        // A wide character's bar is its first column: its spacer is a blank.
        let s = snap("你好x".as_bytes());
        assert_eq!(
            s.rows[0].bars(),
            vec![
                bar(0, 1, fg, false),
                bar(2, 1, fg, false),
                bar(4, 1, fg, false)
            ]
        );
    }

    #[test]
    fn a_dim_run_stays_dim() {
        let fg = Palette::default().foreground;
        let s = snap(b"\x1b[2mdim\x1b[0mlit");
        assert_eq!(
            s.rows[0].bars(),
            vec![bar(0, 3, fg, true), bar(3, 3, fg, false)]
        );
    }
}
