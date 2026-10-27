//! Where a terminal's view is in its history: what its scrollbar draws and where a drag of its
//! thumb puts the view. Pure: the app's scrollbar takes these shares and lines.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Term, TermMode};

/// A terminal's history, its screen and its view of them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scrollback {
    /// Lines above the screen: none in the alternate screen, nor before any scrolled off.
    pub history: usize,
    /// Lines under the view: how far it is scrolled back from the live screen.
    pub below: usize,
    /// The screen's lines.
    pub lines: usize,
    /// Its program asked for the mouse.
    pub mouse: bool,
}

impl Scrollback {
    pub(crate) fn of<T>(term: &Term<T>) -> Self {
        let grid = term.grid();
        Self {
            history: grid.history_size(),
            below: grid.display_offset(),
            lines: grid.screen_lines(),
            mouse: term.mode().intersects(TermMode::MOUSE_MODE),
        }
    }

    /// The scrollbar is drawn: there is a history to go through, and the press on its strip
    /// isn't a program's (one that asked for the mouse keeps its clicks, unless the person went
    /// back in the history with Shift and the wheel).
    pub fn bar(&self) -> bool {
        self.history > 0 && (!self.mouse || self.below > 0)
    }

    /// The view as shares of the history and the screen together: where its top is, and how
    /// long it is (the thumb on its track).
    pub fn thumb(&self) -> (f32, f32) {
        let total = (self.history + self.lines).max(1) as f32;
        let top = self.history.saturating_sub(self.below);
        (top as f32 / total, self.lines as f32 / total)
    }

    /// How far back a view whose top is at `top` (a share, as `thumb` gives it) is: the nearest
    /// line, within the history.
    pub fn below_at(&self, top: f32) -> usize {
        let line = (top * (self.history + self.lines) as f32).round().max(0.0) as usize;
        self.history - line.min(self.history)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACK: Scrollback = Scrollback {
        history: 76,
        below: 10,
        lines: 24,
        mouse: false,
    };

    #[test]
    fn the_thumb_is_the_screen_s_share_where_the_view_is() {
        assert_eq!(BACK.thumb(), (0.66, 0.24));
        let bottom = Scrollback { below: 0, ..BACK };
        assert_eq!(bottom.thumb(), (0.76, 0.24), "at the end of the track");
        let top = Scrollback { below: 76, ..BACK };
        assert_eq!(top.thumb(), (0.0, 0.24));
        let fresh = Scrollback {
            history: 0,
            below: 0,
            ..BACK
        };
        assert_eq!(fresh.thumb(), (0.0, 1.0), "the whole track");
    }

    #[test]
    fn a_thumb_dragged_to_a_share_puts_the_view_on_the_nearest_line() {
        assert_eq!(BACK.below_at(BACK.thumb().0), 10, "where it is");
        assert_eq!(BACK.below_at(0.0), 76, "the history's first line");
        assert_eq!(BACK.below_at(0.76), 0, "the live screen");
        assert_eq!(BACK.below_at(0.5), 26);
        assert_eq!(BACK.below_at(0.504), 26, "the nearest line");
        assert_eq!(BACK.below_at(-0.2), 76, "above the track");
        assert_eq!(BACK.below_at(1.5), 0, "under it");
    }

    #[test]
    fn the_bar_is_drawn_over_a_history_the_program_leaves_the_mouse_to() {
        assert!(BACK.bar());
        assert!(Scrollback { below: 0, ..BACK }.bar());
        let fresh = Scrollback {
            history: 0,
            below: 0,
            ..BACK
        };
        assert!(!fresh.bar(), "nothing to scroll");
        let mouse = Scrollback {
            below: 0,
            mouse: true,
            ..BACK
        };
        assert!(!mouse.bar(), "its clicks are the program's");
        assert!(
            Scrollback { below: 3, ..mouse }.bar(),
            "back in the history"
        );
    }
}
