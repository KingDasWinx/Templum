//! An agent's state from its screen, where its hooks say nothing (docs/specs/connections.md, An
//! agent's state): a screen that keeps changing (a spinner, a timer) is an agent at work, one
//! that stays still is not. Claude Code says nothing when Esc stops it while it writes, nor when
//! a tool the person allowed starts. An agent whose hooks speak would have said its turn ended:
//! still for 30 looks while it works, it seems stuck (a dialog no hook tells of, a hang).

use std::hash::{DefaultHasher, Hash as _, Hasher as _};
use std::time::Duration;

use gpui_kit::Context;

use super::{AgentState, Body, Workspace};

/// How often Templum looks at the agents' screens.
pub(super) const LOOK: Duration = Duration::from_secs(1);
/// Of the last 4 looks, how many saw the screen change for its agent to be at work: Codex's
/// "Working (5s)" changes once a second, and a look may fall between two changes.
const CHANGING: u32 = 3;
/// How many looks in a row saw the screen still for its agent to be at rest.
pub(super) const STILL: u32 = 5;
/// How many looks in a row saw the screen still for an agent whose hooks speak, at work, to
/// seem stuck: its hooks would have said its turn ended.
pub(super) const STUCK: u32 = 30;
/// What an agent that seems stuck waits for its person with.
pub(crate) const STUCK_TEXT: &str = "it seems stuck, its screen still for 30 seconds";

/// The looks at an agent's screen since its last hook: its content's hash, whether each of the
/// last 8 saw it change (the newest in the lowest bit), how many in a row saw it still, and
/// whether the person typed into it (they answered the dialog it showed).
#[derive(Default)]
pub(crate) struct Looks {
    hash: u64,
    changes: u8,
    still: u8,
    answered: bool,
}

impl Looks {
    /// A look that found the screen's content hashing to `hash`.
    fn see(&mut self, hash: u64) {
        let changed = hash != self.hash;
        self.changes = (self.changes << 1) | u8::from(changed);
        self.hash = hash;
        self.still = if changed {
            0
        } else {
            self.still.saturating_add(1)
        };
    }

    /// A look right after the person's input, finding `hash`: it tells nothing, and the looks
    /// before it count no more (the echoes of keys typed in pauses don't add up). Keys they
    /// `typed` answer a dialog.
    fn skip(&mut self, hash: u64, typed: bool) {
        *self = Looks {
            hash,
            answered: self.answered || typed,
            ..Looks::default()
        };
    }

    /// Its agent's hook spoke: the looks before it count no more (the spinner that ran until a
    /// turn ended or a dialog opened says nothing after).
    pub(super) fn heard(&mut self) {
        *self = Looks {
            hash: self.hash,
            ..Looks::default()
        };
    }

    /// The state these looks give an agent in `state` (`speaks`: its hooks say when a turn
    /// ends); None when they say nothing new. A changing screen is at work, also after a dialog
    /// once the person typed into it (a subagent may draw under one still open), or after it
    /// seemed stuck; a still one ends only work, never a wait for the person. An agent whose hooks
    /// speak, still for STUCK looks with no key of its person's since its last hook, seems stuck:
    /// their Esc while Claude Code writes says nothing, and rests it after STILL looks as one
    /// without hooks.
    fn state(&self, state: &Option<AgentState>, speaks: bool) -> Option<AgentState> {
        let changing = (self.changes & 0b1111).count_ones() >= CHANGING;
        let still = u32::from(self.still);
        let heard = speaks && !self.answered;
        match state {
            Some(AgentState::Working) if heard && still >= STUCK => {
                Some(AgentState::NeedsYou(Some(STUCK_TEXT.into())))
            }
            Some(AgentState::Working) if !heard && still >= STILL => Some(AgentState::Idle),
            None | Some(AgentState::Idle) if changing => Some(AgentState::Working),
            Some(AgentState::NeedsYou(Some(m))) if changing && m == STUCK_TEXT => {
                Some(AgentState::Working)
            }
            Some(AgentState::NeedsYou(_)) if changing && self.answered => Some(AgentState::Working),
            _ => None,
        }
    }
}

impl Workspace {
    /// The loop's look: at the screens, at what the agents' states became (no window is at hand
    /// to say whether one is looked at: the next frame does), and the notices due.
    pub(super) fn look_around(&mut self, cx: &mut Context<Self>) {
        self.look_at_agents(cx);
        self.watch_agents(false, cx.background_executor().now());
        self.deliver_notices(cx);
    }

    /// A look at every agent's screen, in every workspace: the state it gives, where that is new.
    /// A look after the person typed, clicked or scrolled in it tells nothing (they change it).
    /// An agent at work whose screen went still ends the task it was on, with its last lines: no
    /// event will. Unless it has Templum's tools, which it answers its task with (`reply`): its
    /// master is only told once that it went quiet. One whose hooks speak, still for 30 looks,
    /// seems stuck: it keeps its task, and waits for its person.
    pub(super) fn look_at_agents(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        let mut still = Vec::new();
        let mut stuck = Vec::new();
        let spaces = std::iter::once(&mut self.space).chain(&mut self.others);
        for pane in spaces
            .flat_map(|s| s.blocks.values_mut())
            .filter_map(Body::terminal_mut)
        {
            // A spawn's prompt still waiting: its screen is its start and its first questions,
            // which `look_ready` watches, and the minute ends its task. A turn that ended while
            // its subagents work: their hooks tell, its prompt sits still.
            if pane.seen.agent.is_none() || pane.prompt.is_some() || pane.awaits_subagents {
                pane.looks = Looks::default();
                continue;
            }
            let mut hasher = DefaultHasher::new();
            pane.screen().hash(&mut hasher);
            let hash = hasher.finish();
            let typed = pane.typed.replace(false);
            if pane.scrolled.replace(false) || typed {
                pane.looks.skip(hash, typed);
                continue;
            }
            pane.looks.see(hash);
            let Some(state) = pane.looks.state(&pane.state, pane.speaks) else {
                continue;
            };
            match &state {
                AgentState::Idle => still.push(pane.term.id().to_string()),
                AgentState::NeedsYou(_) => {
                    pane.dialogs += 1;
                    stuck.push(pane.term.id().to_string());
                }
                AgentState::Working => {}
            }
            pane.state = Some(state);
            pane.asked_by = None;
            changed = true;
        }
        for terminal in still {
            // Waiting for a task it handed out itself: its screen tells nothing about its own
            // task, whatever it has, but the others waiting for its turn hear it ended
            // (docs/specs/agent-protocol.md, 4).
            if self.waits_on_its_own(&terminal) {
                let reply = self.still_screen(&terminal);
                self.answer_others(&terminal, reply, cx);
                continue;
            }
            // An agent on a task with Templum's tools and no hooks can answer it with `reply`: a
            // still screen only tells its master it went quiet. Any other has no other way to say
            // it is done.
            let quiet = self
                .pane(&terminal)
                .is_some_and(|p| !p.speaks && p.handed.is_some())
                && self.has_tools(&terminal);
            if quiet {
                self.went_quiet(&terminal, cx);
            } else {
                let reply = self.still_screen(&terminal);
                self.end_turn(&terminal, reply, cx);
            }
        }
        for terminal in stuck {
            self.needs_you(&terminal, Some(STUCK_TEXT), true, cx);
        }
        if changed {
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The looks after a first one, each seeing the screen change or not.
    fn saw(changes: &[bool]) -> Looks {
        let mut looks = Looks::default();
        looks.see(1);
        more(&mut looks, changes);
        looks
    }

    /// More looks, each seeing the screen change or not.
    fn more(looks: &mut Looks, changes: &[bool]) {
        for &changed in changes {
            looks.see(looks.hash + u64::from(changed));
        }
    }

    #[test]
    fn a_screen_that_keeps_changing_is_at_work() {
        // Codex's "Working (5s)" changes once a second: a look may fall between two changes.
        let looks = saw(&[true, false, true, true]);
        assert_eq!(looks.state(&None, false), Some(AgentState::Working));
        assert_eq!(
            looks.state(&Some(AgentState::Idle), false),
            Some(AgentState::Working)
        );
        assert_eq!(
            looks.state(&Some(AgentState::Working), false),
            None,
            "nothing new"
        );
        assert_eq!(
            saw(&[true, false, false, true]).state(&None, false),
            None,
            "twice in four looks: a redraw"
        );
    }

    #[test]
    fn a_screen_still_for_five_looks_is_at_rest() {
        let working = Some(AgentState::Working);
        assert_eq!(
            saw(&[true, false, false, false, false, false]).state(&working, false),
            Some(AgentState::Idle)
        );
        assert_eq!(
            saw(&[true, false, false, false, false]).state(&working, false),
            None,
            "four"
        );
        assert_eq!(
            saw(&[false; 6]).state(&Some(AgentState::NeedsYou(None)), false),
            None,
            "a dialog waits for its person, still: it stays yellow"
        );
        assert_eq!(saw(&[false; 6]).state(&None, false), None);
        assert_eq!(saw(&[false; 6]).state(&Some(AgentState::Idle), false), None);
    }

    #[test]
    fn a_hooks_event_starts_the_looks_over() {
        // The spinner ran until the turn ended or a dialog opened: it says nothing after.
        let mut looks = saw(&[true; 4]);
        looks.heard();
        more(&mut looks, &[false]);
        assert_eq!(
            looks.state(
                &Some(AgentState::NeedsYou(Some("Allow Bash?".into()))),
                true
            ),
            None
        );
        assert_eq!(looks.state(&Some(AgentState::Idle), true), None);
    }

    #[test]
    fn a_look_after_typing_starts_the_looks_over() {
        // The echoes of keys typed in pauses don't add up to a screen at work.
        let mut looks = saw(&[true, true]);
        looks.skip(looks.hash + 1, true);
        more(&mut looks, &[true]);
        assert_eq!(looks.state(&None, false), None);
    }

    #[test]
    fn a_dialog_ends_only_once_its_person_typed() {
        let asking = Some(AgentState::NeedsYou(Some("Allow Bash?".into())));
        assert_eq!(
            saw(&[true; 4]).state(&asking, true),
            None,
            "a subagent draws under the dialog"
        );
        // The person answers it, and the tool they allowed runs.
        let mut looks = saw(&[]);
        looks.skip(looks.hash + 1, true);
        more(&mut looks, &[true; 3]);
        assert_eq!(looks.state(&asking, true), Some(AgentState::Working));
        // Its next dialog waits for its person again.
        looks.heard();
        more(&mut looks, &[true; 3]);
        assert_eq!(looks.state(&asking, true), None);
    }

    #[test]
    fn a_hooked_agent_still_for_thirty_looks_seems_stuck() {
        let working = Some(AgentState::Working);
        let stuck = Some(AgentState::NeedsYou(Some(STUCK_TEXT.into())));
        let mut looks = saw(&[true]);
        more(&mut looks, &[false; 29]);
        assert_eq!(
            looks.state(&working, true),
            None,
            "its hooks spoke: at work for 29"
        );
        more(&mut looks, &[false]);
        assert_eq!(looks.state(&working, true), stuck);
        assert_eq!(
            saw(&[true, false, false, false, false, false]).state(&working, false),
            Some(AgentState::Idle)
        );
        // The person typed since its last hook (an Esc while Claude Code writes): at rest after 5.
        let mut looks = saw(&[]);
        looks.skip(looks.hash + 1, true);
        more(&mut looks, &[false; 5]);
        assert_eq!(looks.state(&working, true), Some(AgentState::Idle));
        // Moving again: at work, without the person's keys.
        assert_eq!(saw(&[true, true, true]).state(&stuck, true), working);
    }
}
