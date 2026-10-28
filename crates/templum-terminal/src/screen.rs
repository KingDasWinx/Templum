//! A terminal's live screen as runs of styled text, for a client that draws it itself (the
//! API's terminal stream). Read without waiting: painting and the PTY thread come first.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::TermMode;
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};

use crate::snapshot::Cursor;
use crate::terminal::Modes;
use crate::{Rgb, SharedTerm};

/// A color as the program asked for it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Paint {
    /// The theme's foreground or background.
    Default,
    /// A palette slot 0–255: the viewer's palette draws it.
    Index(u8),
    Rgb(Rgb),
}

/// Text of one style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Styled {
    pub text: String,
    pub fg: Paint,
    pub bg: Paint,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    /// Kept as a flag: the viewer swaps the colors, not the reader.
    pub inverse: bool,
}

impl Styled {
    /// Whether `other` is drawn alike, its text aside.
    fn same_style(&self, other: &Styled) -> bool {
        (
            self.fg,
            self.bg,
            self.bold,
            self.dim,
            self.italic,
            self.underline,
            self.inverse,
        ) == (
            other.fg,
            other.bg,
            other.bold,
            other.dim,
            other.italic,
            other.underline,
            other.inverse,
        )
    }

    /// Blank space the viewer need not draw: nothing shows it.
    fn invisible_blank(&self) -> bool {
        self.bg == Paint::Default && !self.inverse && !self.underline
    }
}

/// What a terminal shows now (the live screen, never the view scrolled back to).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Screen {
    pub cols: usize,
    /// One entry per screen line: its runs, trailing blanks trimmed (an empty row is `vec![]`).
    pub rows: Vec<Vec<Styled>>,
    /// None while the program hides it.
    pub cursor: Option<Cursor>,
    /// The program's own title (OSC 0/2); None without one.
    pub title: Option<String>,
    /// What the program took, read with the rows. `typing` is always false: a mirror doesn't
    /// need it, and a change of it alone must not send a frame.
    pub modes: Modes,
}

impl SharedTerm {
    /// The live screen, or None at once when the PTY thread (or anyone) holds the grid: the
    /// caller looks again later. No selection, no scrolled view.
    pub fn screen(&self) -> Option<Screen> {
        let term = self.0.try_lock_unfair()?;
        let grid = term.grid();
        let colors = term.colors();
        let cols = grid.columns();
        let rows = (0..grid.screen_lines())
            .map(|y| runs(&grid[Line(y as i32)], cols, colors))
            .collect();
        let modes = Modes {
            typing: false,
            ..Modes::of(*term.mode())
        };
        let shape = term.cursor_style();
        let cursor = (term.mode().contains(TermMode::SHOW_CURSOR)
            && shape.shape != CursorShape::Hidden)
            .then(|| Cursor {
                row: grid.cursor.point.line.0.max(0) as usize,
                col: grid.cursor.point.column.0,
                shape: match shape.shape {
                    CursorShape::HollowBlock => CursorShape::Block,
                    other => other,
                },
                blinking: shape.blinking,
            });
        drop(term);
        Some(Screen {
            cols,
            rows,
            cursor,
            title: self.1.title(),
            modes,
        })
    }
}

fn runs(cells: &alacritty_terminal::grid::Row<Cell>, cols: usize, colors: &Colors) -> Vec<Styled> {
    let mut runs: Vec<Styled> = Vec::new();
    for col in 0..cols {
        let cell = &cells[Column(col)];
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let hidden = cell.flags.contains(Flags::HIDDEN);
        let c = if hidden || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) || cell.c < ' ' {
            ' '
        } else {
            cell.c
        };
        let style = Styled {
            text: String::new(),
            fg: paint(cell.fg, colors),
            bg: paint(cell.bg, colors),
            bold: cell.flags.contains(Flags::BOLD),
            dim: cell.flags.contains(Flags::DIM),
            italic: cell.flags.contains(Flags::ITALIC),
            underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
            inverse: cell.flags.contains(Flags::INVERSE),
        };
        let run = match runs.last_mut() {
            Some(last) if last.same_style(&style) => last,
            _ => {
                runs.push(style);
                runs.last_mut().unwrap()
            }
        };
        run.text.push(c);
        if let Some(marks) = cell.zerowidth().filter(|_| !hidden) {
            run.text.extend(marks);
        }
    }
    // Trailing blanks nothing shows go, and a run left empty with them.
    while let Some(last) = runs.last_mut() {
        if last.invisible_blank() {
            let kept = last.text.trim_end_matches(' ').len();
            last.text.truncate(kept);
        }
        if last.text.is_empty() {
            runs.pop();
        } else {
            break;
        }
    }
    runs
}

fn paint(color: Color, colors: &Colors) -> Paint {
    let set = match color {
        Color::Spec(rgb) => Some(rgb),
        Color::Indexed(i) => colors[i as usize],
        Color::Named(n) => colors[n],
    };
    if let Some(rgb) = set {
        return Paint::Rgb(rgb);
    }
    match color {
        Color::Spec(_) => unreachable!("a spec color is always set"),
        Color::Indexed(i) => Paint::Index(i),
        Color::Named(n) => match n as usize {
            i @ 0..16 => Paint::Index(i as u8),
            i if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&i) => {
                Paint::Index((i - NamedColor::DimBlack as usize) as u8)
            }
            _ => Paint::Default,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{feed, headless};
    use crate::{Palette, TermOptions, Terminal};
    use alacritty_terminal::grid::Scroll;
    use std::sync::Arc;

    fn styled(text: &str) -> Styled {
        Styled {
            text: text.into(),
            fg: Paint::Default,
            bg: Paint::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
        }
    }

    fn viewer(cols: u16, rows: u16) -> Terminal {
        Terminal::viewer(
            "v",
            (cols, rows),
            Palette::default(),
            TermOptions::default(),
            Arc::new(|| {}),
        )
    }

    #[test]
    fn styles_come_as_the_program_asked() {
        let (term, _) = headless(20, 2);
        feed(
            &term,
            b"\x1b[1;31mA\x1b[0m\x1b[38;5;200mB\x1b[38;2;1;2;3mC\x1b[0m\x1b[7;4;3;2mD\x1b[0m",
        );
        let row = &term.screen().unwrap().rows[0];
        assert_eq!(
            row[0],
            Styled {
                fg: Paint::Index(1),
                bold: true,
                ..styled("A")
            }
        );
        assert_eq!(
            row[1],
            Styled {
                fg: Paint::Index(200),
                ..styled("B")
            }
        );
        assert_eq!(
            row[2],
            Styled {
                fg: Paint::Rgb(Rgb { r: 1, g: 2, b: 3 }),
                ..styled("C")
            }
        );
        assert_eq!(
            row[3],
            Styled {
                inverse: true,
                underline: true,
                italic: true,
                dim: true,
                ..styled("D")
            }
        );
    }

    #[test]
    fn the_dim_colors_map_to_their_palette_slots() {
        let colors = Colors::default();
        for (named, slot) in [
            (NamedColor::DimBlack, 0),
            (NamedColor::DimRed, 1),
            (NamedColor::DimWhite, 7),
        ] {
            assert_eq!(paint(Color::Named(named), &colors), Paint::Index(slot));
        }
        assert_eq!(
            paint(Color::Named(NamedColor::Foreground), &colors),
            Paint::Default
        );
    }

    #[test]
    fn a_color_the_program_set_comes_as_rgb() {
        let (term, _) = headless(20, 2);
        feed(&term, b"\x1b]4;1;rgb:ff/00/00\x07\x1b[31mX");
        let row = &term.screen().unwrap().rows[0];
        assert_eq!(
            row[0],
            Styled {
                fg: Paint::Rgb(Rgb { r: 255, g: 0, b: 0 }),
                ..styled("X")
            }
        );
    }

    #[test]
    fn hidden_text_is_blank_and_trailing_blanks_go() {
        let (term, _) = headless(20, 2);
        feed(&term, b"ab\x1b[8mpass\x1b[0m   ");
        assert_eq!(term.screen().unwrap().rows[0], vec![styled("ab")]);
        let (term, _) = headless(20, 2);
        feed(&term, b"\x1b[44m  \x1b[0m");
        assert_eq!(
            term.screen().unwrap().rows[0],
            vec![Styled {
                bg: Paint::Index(4),
                ..styled("  ")
            }]
        );
    }

    #[test]
    fn wide_and_combining_characters_keep_their_cells() {
        let (term, _) = headless(20, 2);
        feed(&term, "你x".as_bytes());
        assert_eq!(term.screen().unwrap().rows[0], vec![styled("你x")]);
        let (term, _) = headless(20, 2);
        feed(&term, "e\u{301}!".as_bytes());
        assert_eq!(term.screen().unwrap().rows[0], vec![styled("e\u{301}!")]);
    }

    #[test]
    fn the_cursor_and_the_title() {
        let v = viewer(20, 2);
        v.feed(b"\x1b]2;vim\x07ab\x1b[4 q");
        let s = v.shared().screen().unwrap();
        assert_eq!(s.title.as_deref(), Some("vim"));
        assert_eq!(
            s.cursor,
            Some(Cursor {
                row: 0,
                col: 2,
                shape: CursorShape::Underline,
                blinking: false
            })
        );
        v.feed(b"\x1b[?25l");
        assert_eq!(v.shared().screen().unwrap().cursor, None);
        v.feed(b"\x1b]2; \x07");
        assert_eq!(v.shared().screen().unwrap().title, None);
    }

    #[test]
    fn the_screen_says_what_the_program_took() {
        let v = viewer(20, 4);
        assert_eq!(v.shared().screen().unwrap().modes, Modes::default());
        v.feed(b"\x1b[?1049h\x1b[?1000h\x1b[>1u");
        assert_eq!(
            v.shared().screen().unwrap().modes,
            Modes {
                full_screen: true,
                mouse: true,
                kitty_keys: true,
                ..Modes::default()
            }
        );
        // Bracketed paste with the cursor shown is `typing` for Terminal::modes, never for a frame.
        v.feed(b"\x1b[?2004h");
        assert!(!v.shared().screen().unwrap().modes.typing);
    }

    #[test]
    fn the_live_screen_not_the_scrolled_view() {
        let (term, _) = headless(20, 4);
        feed(&term, b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n9");
        term.0.lock().scroll_display(Scroll::Delta(3));
        let texts: Vec<String> = term
            .screen()
            .unwrap()
            .rows
            .iter()
            .map(|r| r.iter().map(|s| s.text.as_str()).collect())
            .collect();
        assert_eq!(texts, ["6", "7", "8", "9"]);
    }

    #[test]
    fn a_held_lock_reads_nothing_at_once() {
        let (term, _) = headless(20, 2);
        let _held = term.0.lock();
        assert!(term.screen().is_none());
    }

    #[test]
    fn changes_count_what_may_have_changed() {
        let v = viewer(20, 2);
        let c = v.shared().changes();
        v.feed(b"x");
        assert!(v.shared().changes() > c);
        assert!(v.shared().same(&v.shared().clone()));
        assert!(!v.shared().same(viewer(20, 2).shared()));
        let (term, _) = headless(20, 2);
        let c = term.changes();
        term.touched();
        assert!(term.changes() > c);
    }
}
