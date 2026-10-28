//! A terminal stream's frames become the bytes a viewer is fed, as a PTY reader would feed it
//! (docs/specs/templum-to-templum.md, Terminals). What home sends is drawn, never run: control
//! characters are dropped before a byte is made, the title is cut, colors are read strictly and a
//! frame past home's largest grid is refused.

use crate::model::{Color, CursorView, FrameView, ModesView, RunView, SizeView};
use templum_terminal::Feeder;

/// The most columns and rows a frame may have: home's own largest grid.
pub(crate) const MOST: (usize, usize) = (2000, 1000);

/// The longest title that is shown, in characters.
const TITLE_MOST: usize = 256;

/// One stream's memory of what its viewer was last told, so a frame sends only what changed.
#[derive(Default)]
pub(crate) struct Painter {
    modes: Option<ModesView>,
    title: Option<String>,
    /// The first frame says the title even when there is none, so a stream that reopens drops
    /// the old one.
    titled: bool,
}

impl Painter {
    /// Shows `frame` in `into`. Nothing is fed when its size is past [`MOST`] or empty.
    pub(crate) fn paint(&mut self, frame: &FrameView, into: &Feeder) -> Result<(), &'static str> {
        let (cols, rows) = (frame.size.cols, frame.size.rows);
        if cols > MOST.0 || rows > MOST.1 {
            return Err("too large");
        }
        if cols == 0 || rows == 0 {
            return Err("empty");
        }
        if frame.whole {
            into.resize(cols as u16, rows as u16);
        }
        into.feed(&self.bytes(frame));
        Ok(())
    }

    /// The bytes that make a viewer show `frame`: the modes that changed (the first frame says
    /// all three, so a stream that reopens leaves nothing of the old one behind), the rows, the
    /// cursor and the title when it changed. A row longer than the columns is clipped by the viewer
    /// (no autowrap, set on the first frame), so it never wraps onto the next.
    pub(crate) fn bytes(&mut self, frame: &FrameView) -> Vec<u8> {
        let mut out = String::new();
        self.modes_into(&frame.modes, &mut out);
        if frame.whole {
            // Not `CSI 2J`: on the main screen it pushes the old screen into the history a viewer
            // may hold; from the top left, `CSI J` clears it in place.
            out.push_str("\x1b[H\x1b[J");
        }
        for row in frame.rows.iter().filter(|row| row.y < frame.size.rows) {
            out.push_str(&format!("\x1b[{};1H\x1b[0m\x1b[2K", row.y + 1));
            for run in &row.runs {
                run_into(run, &mut out);
            }
            out.push_str("\x1b[0m");
        }
        cursor_into(frame.cursor.as_ref(), &frame.size, &mut out);
        self.title_into(frame.title.as_deref(), &mut out);
        out.into_bytes()
    }

    fn modes_into(&mut self, now: &ModesView, out: &mut String) {
        let before = self.modes.replace(now.clone());
        // Only the painter writes to a viewer, and a row longer than the grid must be clipped at
        // the right margin, not wrapped onto the next row.
        if before.is_none() {
            out.push_str("\x1b[?7l");
        }
        // Nothing is known before the first frame, so every mode is said.
        let changed =
            |was: fn(&ModesView) -> bool| before.as_ref().is_none_or(|b| was(b) != was(now));
        if changed(|m| m.full_screen) {
            out.push_str(if now.full_screen {
                "\x1b[?1049h"
            } else {
                "\x1b[?1049l"
            });
        }
        // alacritty's mouse modes replace each other (`?1000h` removes `?1003` and the other way
        // round), so the two are written as one pair.
        if changed(|m| m.mouse) || changed(|m| m.mouse_moves) {
            out.push_str("\x1b[?1000l\x1b[?1003l");
            if now.mouse_moves {
                out.push_str("\x1b[?1003h");
            } else if now.mouse {
                out.push_str("\x1b[?1000h");
            }
        }
        // alacritty keeps a kitty stack per screen, and a switch of screens swaps them: a push
        // and a pop would drift from home's. So the active stack is emptied (`CSI < 99 u`) and
        // filled again whenever the screen or the mode changes, and on the first frame.
        if before.is_none() || changed(|m| m.full_screen) || changed(|m| m.kitty_keys) {
            out.push_str("\x1b[<99u");
            if now.kitty_keys {
                out.push_str("\x1b[>1u");
            }
        }
    }

    fn title_into(&mut self, title: Option<&str>, out: &mut String) {
        let title = title
            .map(|t| {
                t.chars()
                    .filter(|c| !c.is_control())
                    .take(TITLE_MOST)
                    .collect::<String>()
            })
            .filter(|t| !t.trim().is_empty());
        if self.titled && title == self.title {
            return;
        }
        out.push_str(&format!("\x1b]2;{}\x07", title.as_deref().unwrap_or("")));
        self.title = title;
        self.titled = true;
    }
}

/// `CSI 0m`, the run's style and its text without a control character. The text is not cut here:
/// a row is clipped by the viewer's own right margin (autowrap is off, see `modes_into`), because
/// counting characters would miscount the zero-width marks home appends to a cell's text.
fn run_into(run: &RunView, out: &mut String) {
    out.push_str("\x1b[0");
    for (on, code) in [
        (run.bold, 1),
        (run.dim, 2),
        (run.italic, 3),
        (run.underline, 4),
        (run.inverse, 7),
    ] {
        if on {
            out.push_str(&format!(";{code}"));
        }
    }
    color_into(run.fg.as_ref(), 38, out);
    color_into(run.bg.as_ref(), 48, out);
    out.push('m');
    out.extend(run.text.chars().filter(|c| !c.is_control()));
}

/// An index, or `#rrggbb` of six hex digits; anything else is the default (nothing).
fn color_into(color: Option<&Color>, base: u8, out: &mut String) {
    match color {
        Some(Color::Index(i)) => out.push_str(&format!(";{base};5;{i}")),
        Some(Color::Rgb(text)) => {
            let hex = text
                .strip_prefix('#')
                .filter(|h| h.len() == 6 && h.is_ascii());
            let channel = |at: usize| hex.and_then(|h| u8::from_str_radix(&h[at..at + 2], 16).ok());
            if let (Some(r), Some(g), Some(b)) = (channel(0), channel(2), channel(4)) {
                out.push_str(&format!(";{base};2;{r};{g};{b}"));
            }
        }
        None => {}
    }
}

fn cursor_into(cursor: Option<&CursorView>, size: &SizeView, out: &mut String) {
    let Some(c) = cursor else {
        out.push_str("\x1b[?25l");
        return;
    };
    let n = match (c.shape.as_str(), c.blinking) {
        ("underline", true) => 3,
        ("underline", false) => 4,
        ("beam", true) => 5,
        ("beam", false) => 6,
        (_, true) => 1,
        (_, false) => 2,
    };
    // Home says where; the grid says how far it can go.
    let (row, col) = (
        c.row.min(size.rows.saturating_sub(1)),
        c.col.min(size.cols.saturating_sub(1)),
    );
    out.push_str(&format!("\x1b[{};{}H\x1b[{n} q\x1b[?25h", row + 1, col + 1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminals::{frame, whole};
    use std::sync::Arc;
    use templum_terminal::{Palette, Screen, TermOptions, Terminal};

    fn viewer(cols: u16, rows: u16) -> Terminal {
        Terminal::viewer(
            "v",
            (cols, rows),
            Palette::default(),
            TermOptions::default(),
            Arc::new(|| {}),
        )
    }

    fn screen_of(v: &Terminal) -> Screen {
        v.shared().screen().unwrap()
    }

    fn run(text: &str) -> RunView {
        RunView {
            text: text.into(),
            ..Default::default()
        }
    }

    /// A whole frame of `cols` × `rows` whose rows hold the given runs.
    fn frame_of(cols: usize, rows: usize, runs: Vec<RunView>) -> FrameView {
        FrameView {
            size: SizeView { cols, rows },
            whole: true,
            rows: vec![crate::model::RowView { y: 0, runs }],
            ..Default::default()
        }
    }

    fn escapes(bytes: &[u8]) -> usize {
        bytes.iter().filter(|b| **b == 0x1b).count()
    }

    #[test]
    fn a_frame_painted_into_a_viewer_reads_back_the_same_screen() {
        let home = viewer(30, 7);
        home.feed(
            "\x1b[?1049h\x1b[>1u\x1b]2;title\x07\x1b[1;31mred\x1b[0m \x1b[38;5;200mix\x1b[38;2;9;8;7m rgb\x1b[0m\r\n\
             \x1b[48;5;4m  \x1b[48;2;1;2;3mblue\x1b[0m\x1b[7mrev\x1b[0m\x1b[4;3;2mstyles\x1b[0m\r\n\
             ab\x1b[8msecret\x1b[0m 你好 e\u{301}!\r\n\
             \x1b[44m  \x1b[0m\x1b[4m  \x1b[0m\r\n\
             012345678901234567890123456789\r\n\
             e\u{301}12345678901234567890123456789\r\n\
             \u{26a0}\u{fe0f}\u{2714}\u{fe0f}5678901234567890123456789012\x1b[2 q"
                .as_bytes(),
        );
        let before = screen_of(&home);
        assert!(before.modes.full_screen && before.modes.kitty_keys);
        let mirror = viewer(30, 7);
        let mut painter = Painter::default();
        painter.paint(&whole(&before), &mirror.feeder()).unwrap();
        assert_eq!(screen_of(&mirror), before);

        home.feed(b"\r\nmore");
        let after = screen_of(&home);
        let changed = frame(Some(&before), &after).unwrap();
        assert!(!changed.whole);
        painter.paint(&changed, &mirror.feeder()).unwrap();
        assert_eq!(screen_of(&mirror), after);
    }

    #[test]
    fn what_home_sends_is_drawn_never_run() {
        let hostile = RunView {
            fg: Some(Color::Rgb("red".into())),
            ..run("a\x1b]2;pwn\x07b\u{9b}31mc")
        };
        let mut frame = frame_of(20, 2, vec![hostile]);
        frame.title = Some(format!("t\x1b[2J{}", "x".repeat(300)));

        let mut clean = frame_of(10, 2, vec![run("abc")]);
        clean.title = Some("t".into());
        assert_eq!(
            escapes(&Painter::default().bytes(&frame)),
            escapes(&Painter::default().bytes(&clean)),
            "only the painter's own sequences carry an ESC"
        );

        let mirror = viewer(20, 2);
        Painter::default().paint(&frame, &mirror.feeder()).unwrap();
        let screen = screen_of(&mirror);
        assert_eq!(screen.rows[0].len(), 1, "no color: one run");
        assert_eq!(screen.rows[0][0].text, "a]2;pwnb31mc");
        assert_eq!(screen.rows[0][0].fg, templum_terminal::Paint::Default);
        let title = screen.title.unwrap();
        assert_eq!(title, format!("t[2J{}", "x".repeat(252)));
        assert_eq!(title.chars().count(), 256);

        let huge = FrameView {
            size: SizeView {
                cols: 5000,
                rows: 10,
            },
            ..frame_of(5000, 10, vec![run("zzz")])
        };
        let untouched = viewer(10, 2);
        let seen = screen_of(&untouched);
        assert_eq!(
            Painter::default().paint(&huge, &untouched.feeder()),
            Err("too large")
        );
        assert_eq!(screen_of(&untouched), seen);
        assert_eq!(untouched.shared().grid_size(), (10, 2));
    }

    #[test]
    fn colors_are_indexes_or_six_hex_digits_and_nothing_else() {
        let colored = |fg: Option<Color>| {
            let mut out = String::new();
            run_into(&RunView { fg, ..run("x") }, &mut out);
            out
        };
        assert_eq!(colored(Some(Color::Index(200))), "\x1b[0;38;5;200mx");
        assert_eq!(
            colored(Some(Color::Rgb("#0a0b0c".into()))),
            "\x1b[0;38;2;10;11;12mx"
        );
        for wrong in ["red", "#12345", "#1234567", "0a0b0c", "#gg0000", "#é1234"] {
            assert_eq!(
                colored(Some(Color::Rgb(wrong.into()))),
                "\x1b[0mx",
                "{wrong}"
            );
        }
    }

    #[test]
    fn a_whole_frame_of_a_new_size_resizes_the_viewer() {
        let mirror = viewer(10, 3);
        let frame = frame_of(20, 5, vec![run("wider")]);
        Painter::default().paint(&frame, &mirror.feeder()).unwrap();
        assert_eq!(mirror.shared().grid_size(), (20, 5));
        assert_eq!(screen_of(&mirror).rows[0][0].text, "wider");
    }

    #[test]
    fn modes_are_sent_only_when_they_change() {
        let mirror = viewer(10, 2);
        let mut painter = Painter::default();
        let mut frame = frame_of(10, 2, vec![run("x")]);
        frame.modes.kitty_keys = true;
        let has = |bytes: &[u8], what: &str| String::from_utf8_lossy(bytes).contains(what);
        let first = painter.bytes(&frame);
        assert!(has(&first, "\x1b[>1u") && has(&first, "\x1b[?1049l"));
        assert!(!has(&painter.bytes(&frame), "\x1b[>1u"));
        let mut painter = Painter::default();
        painter.paint(&frame, &mirror.feeder()).unwrap();
        assert!(mirror.shared().modes().kitty_keys);
        frame.modes.kitty_keys = false;
        let off = painter.bytes(&frame);
        assert!(has(&off, "\x1b[<99u") && !has(&off, "\x1b[>1u"));
        mirror.feed(&off);
        assert!(!mirror.shared().modes().kitty_keys);
    }

    #[test]
    fn a_whole_frame_keeps_the_viewer_s_history() {
        let mirror = viewer(10, 2);
        let mut painter = Painter::default();
        painter
            .paint(&frame_of(10, 2, vec![run("old")]), &mirror.feeder())
            .unwrap();
        assert!(mirror.feeder().fill_history("h1\nh2\nold"));
        painter
            .paint(&frame_of(10, 2, vec![run("new")]), &mirror.feeder())
            .unwrap();
        assert_eq!(
            mirror.shared().lines(100),
            ["h1", "h2", "new"],
            "the old screen isn't pushed into the history"
        );
    }

    #[test]
    fn the_mouse_modes_reach_the_viewer_as_home_has_them() {
        let mirror = viewer(10, 2);
        let mut painter = Painter::default();
        let mut frame = frame_of(10, 2, vec![run("x")]);
        let mut paint = |mouse, moves| {
            frame.modes.mouse = mouse;
            frame.modes.mouse_moves = moves;
            painter.paint(&frame, &mirror.feeder()).unwrap();
            let m = mirror.shared().modes();
            (m.mouse, m.mouse_moves)
        };
        assert_eq!(paint(true, true), (true, true));
        // alacritty's mouse modes replace each other: back to clicks only is clicks only.
        assert_eq!(paint(true, false), (true, false));
        assert_eq!(paint(false, false), (false, false));
    }

    #[test]
    fn a_title_that_goes_away_is_cleared_and_a_blank_one_reads_as_none() {
        let mirror = viewer(10, 2);
        let mut painter = Painter::default();
        let mut frame = frame_of(10, 2, vec![run("x")]);
        frame.title = Some("vim".into());
        painter.paint(&frame, &mirror.feeder()).unwrap();
        assert_eq!(screen_of(&mirror).title.as_deref(), Some("vim"));
        frame.title = Some("\x1b \u{7}".into());
        painter.paint(&frame, &mirror.feeder()).unwrap();
        assert_eq!(screen_of(&mirror).title, None);
    }

    fn kitty_of(v: &Terminal) -> bool {
        v.shared().modes().kitty_keys
    }

    #[test]
    fn a_reopened_stream_leaves_no_kitty_push_behind() {
        let mirror = viewer(10, 2);
        let mut frame = frame_of(10, 2, vec![run("x")]);
        frame.modes.kitty_keys = true;
        Painter::default().paint(&frame, &mirror.feeder()).unwrap();
        assert!(kitty_of(&mirror));
        // The stream reopened: a new painter pushes again, and home leaving kitty is one pop.
        let mut reopened = Painter::default();
        reopened.paint(&frame, &mirror.feeder()).unwrap();
        frame.modes.kitty_keys = false;
        reopened.paint(&frame, &mirror.feeder()).unwrap();
        assert!(
            !kitty_of(&mirror),
            "the stack was emptied, not pushed twice"
        );
    }

    #[test]
    fn kitty_used_in_the_alternate_screen_does_not_outlive_it() {
        let mirror = viewer(10, 2);
        let mut painter = Painter::default();
        let mut frame = frame_of(10, 2, vec![run("x")]);
        frame.modes.full_screen = true;
        frame.modes.kitty_keys = true;
        painter.paint(&frame, &mirror.feeder()).unwrap();
        assert!(kitty_of(&mirror));
        frame.modes.full_screen = false;
        frame.modes.kitty_keys = false;
        painter.paint(&frame, &mirror.feeder()).unwrap();
        assert!(!kitty_of(&mirror));
        // The next program in the alternate screen finds its stack empty, as home's is.
        frame.modes.full_screen = true;
        painter.paint(&frame, &mirror.feeder()).unwrap();
        assert!(
            !kitty_of(&mirror),
            "the alternate screen's old push is gone"
        );
    }

    #[test]
    fn a_reopened_stream_drops_the_old_title() {
        let mirror = viewer(10, 2);
        let mut frame = frame_of(10, 2, vec![run("x")]);
        frame.title = Some("old".into());
        Painter::default().paint(&frame, &mirror.feeder()).unwrap();
        assert_eq!(screen_of(&mirror).title.as_deref(), Some("old"));
        frame.title = None;
        Painter::default().paint(&frame, &mirror.feeder()).unwrap();
        assert_eq!(screen_of(&mirror).title, None);
    }

    #[test]
    fn a_home_gone_wrong_cannot_overrun_a_row_or_the_cursor() {
        // A cursor at the end of the numbers must not overflow, and stays on the grid.
        let mut frame = frame_of(10, 3, vec![run("x")]);
        frame.cursor = Some(CursorView {
            row: usize::MAX,
            col: usize::MAX,
            shape: "block".into(),
            blinking: false,
        });
        let bytes = String::from_utf8(Painter::default().bytes(&frame)).unwrap();
        assert!(bytes.contains("\x1b[3;10H"), "{bytes:?}");

        // A row is clipped at the columns, across its runs, so it never wraps onto the next.
        let mirror = viewer(10, 3);
        let mut long = frame_of(10, 3, vec![run(&"a".repeat(7)), run(&"b".repeat(100_000))]);
        long.rows.push(crate::model::RowView {
            y: 1,
            runs: vec![run("below")],
        });
        Painter::default().paint(&long, &mirror.feeder()).unwrap();
        let screen = screen_of(&mirror);
        let text = |y: usize| {
            screen.rows[y]
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_string()
        };
        assert_eq!(text(0), "aaaaaaabbb");
        assert_eq!(text(1), "below");
        assert_eq!(text(2), "");
    }
}
