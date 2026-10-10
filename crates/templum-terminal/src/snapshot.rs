//! Cells copied out of a terminal for painting. Painting never waits on the PTY thread:
//! if it holds the lock (heavy output), the previous snapshot is reused for up to MAX_STALE.

use std::time::{Duration, Instant};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::vte::ansi::CursorShape;

use crate::palette::{self, Palette};
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Row {
    /// One char per column (wide-char spacers become blanks), trailing blanks trimmed.
    pub text: String,
    pub runs: Vec<Run>,
    /// Backgrounds other than the default one.
    pub bgs: Vec<(Span, Rgb)>,
    pub selected: Vec<Span>,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Default)]
pub struct Snapshot {
    /// One row per visible line.
    pub rows: Vec<Row>,
    /// None when hidden or scrolled out of view.
    pub cursor: Option<Cursor>,
    taken: Option<Instant>,
}

impl Snapshot {
    /// Re-reads the visible grid, resolving colors with `palette`, unless the PTY thread holds the
    /// lock, in which case the previous cells are kept (past MAX_STALE it waits). Returns false
    /// when it kept them.
    pub fn refresh(&mut self, term: &SharedTerm, palette: &Palette) -> bool {
        let stale = self.taken.is_none_or(|t| t.elapsed() > MAX_STALE);
        let term = match term.0.try_lock_unfair() {
            Some(term) => term,
            None if stale => term.0.lock(),
            None => return false,
        };
        let screen_rows = term.screen_lines();
        let content = term.renderable_content();
        let offset = content.display_offset as i32;
        self.rows.clear();
        self.rows.resize_with(screen_rows, Row::default);
        self.cursor = None;
        if content.cursor.shape != CursorShape::Hidden {
            let row = content.cursor.point.line.0 + offset;
            if (0..screen_rows as i32).contains(&row) {
                self.cursor = Some(Cursor {
                    row: row as usize,
                    col: content.cursor.point.column.0,
                });
            }
        }
        let selection = content.selection;
        for item in content.display_iter {
            let Some(row) = self.rows.get_mut((item.point.line.0 + offset) as usize) else {
                continue;
            };
            let cell = item.cell;
            let mut fg = palette::resolve(cell.fg, content.colors, palette);
            let mut bg = palette::resolve(cell.bg, content.colors, palette);
            if cell.flags.contains(Flags::INVERSE) {
                std::mem::swap(&mut fg, &mut bg);
            }
            if cell.flags.contains(Flags::HIDDEN) {
                fg = bg;
            }
            let col = item.point.column.0;
            if bg != palette.background {
                row.push_bg(col, bg);
            }
            if selection.as_ref().is_some_and(|s| s.contains(item.point)) {
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
        drop(term);
        self.rows.iter_mut().for_each(Row::trim);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{feed, headless};
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
        assert_eq!(s.cursor, Some(Cursor { row: 1, col: 2 }));
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
}
