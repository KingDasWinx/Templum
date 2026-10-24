//! The cover over an agent's terminal while it starts: its name and a spinner instead of its
//! shell typing the command, until the agent's screen shows.

use std::time::{Duration, Instant};

use gpui_kit::{Context, SharedString};
use templum_canvas::BlockId;
use templum_terminal::Status;

use super::Workspace;

/// How often a starting agent is looked at.
const LOOK: Duration = Duration::from_millis(150);
/// How long its screen stays still, once it printed, before the cover comes off.
const QUIET: Duration = Duration::from_millis(400);
/// How long it may go without word from its shell that it started (no shell integration, or it
/// quit at once).
const NO_WORD: Duration = Duration::from_secs(3);
/// The cover comes off by then whatever happens (a slow agent, a question of its shell).
const GIVE_UP: Duration = Duration::from_secs(12);

pub(crate) struct Starting {
    /// Its agent's name.
    pub title: SharedString,
    since: Instant,
}

impl Starting {
    fn new(title: String, since: Instant) -> Self {
        Self {
            title: title.into(),
            since,
        }
    }

    /// The cover comes off at `now`: its agent printed since it started and has been still for
    /// QUIET (without word from its shell, NO_WORD after the cover went on); its shell ended; or
    /// GIVE_UP passed.
    fn over(&self, status: &Status, exited: bool, now: Instant) -> bool {
        let drew = match status.running {
            Some(start) => status.output.is_some_and(|out| out > start),
            None => now - self.since >= NO_WORD,
        };
        let quiet = status.output.is_none_or(|out| now - out >= QUIET);
        exited || now - self.since >= GIVE_UP || (drew && quiet)
    }
}

impl Workspace {
    /// Covers the terminal `id` of the active workspace with `title`, its agent's name, until the
    /// agent shows. Its command was just typed (`Pane::typed_agent`).
    pub(super) fn cover_start(&mut self, id: BlockId, title: String, cx: &mut Context<Self>) {
        let Some(pane) = self.space.terminal_mut(id) else {
            return;
        };
        pane.starting = Some(Starting::new(title, Instant::now()));
        pane.typed_agent = Some(Instant::now());
        let terminal = pane.term.id().to_string();
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor().timer(LOOK).await;
                let covered = ws
                    .update(cx, |ws, cx| ws.look_start(&terminal, cx))
                    .unwrap_or(false);
                if !covered {
                    break;
                }
            }
        })
        .detach();
    }

    /// One look at the terminal `terminal`: its cover comes off when it is over. False once it
    /// is off.
    fn look_start(&mut self, terminal: &str, cx: &mut Context<Self>) -> bool {
        let Some(pane) = self.pane_mut(terminal) else {
            return false;
        };
        let Some(starting) = &pane.starting else {
            return false;
        };
        if !starting.over(&pane.term.status(), pane.term.exited(), Instant::now()) {
            return true;
        }
        pane.starting = None;
        cx.notify();
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(running: Option<Instant>, output: Option<Instant>) -> Status {
        Status {
            running,
            output,
            ..Status::default()
        }
    }

    #[test]
    fn the_cover_comes_off_once_the_agent_printed_and_went_still() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let s = Starting::new("Claude Code".into(), t0);
        // The shell's prompt and the command, then the agent starts and prints nothing yet.
        assert!(!s.over(&status(None, Some(at(100))), false, at(600)));
        assert!(!s.over(&status(Some(at(500)), Some(at(490))), false, at(1500)));
        // It draws: still while it goes on, off once its screen stood still for QUIET.
        assert!(!s.over(&status(Some(at(500)), Some(at(1400))), false, at(1600)));
        assert!(s.over(&status(Some(at(500)), Some(at(1400))), false, at(1800)));
    }

    #[test]
    fn the_cover_comes_off_without_word_when_the_shell_ends_or_at_last() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let s = Starting::new("Codex".into(), t0);
        // No shell integration: NO_WORD after it went on, once still.
        assert!(!s.over(&status(None, Some(at(2900))), false, at(2950)));
        assert!(s.over(&status(None, Some(at(2900))), false, at(3400)));
        // Its shell ended.
        assert!(s.over(&status(None, None), true, at(10)));
        // An agent that never stops printing.
        let busy = status(Some(at(100)), Some(at(11_990)));
        assert!(!s.over(&busy, false, at(11_999)));
        assert!(s.over(&busy, false, at(12_000)));
    }
}
