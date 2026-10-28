//! The requests inbox's model (docs/specs/ask-inbox.md): the requests of the workspace shown,
//! the one selected, and why each is asked.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use gpui_kit::{App, Context, KeyDownEvent, Keystroke, Window};

use super::Workspace;
use super::waits::{Wait, Waiting, field_focused};
use crate::board::Assignee;
use crate::keys::Command;

/// How long the sidebar's button pulses when a request arrives.
pub(crate) const PULSE: Duration = Duration::from_secs(3);
/// The lines of a handed task the inbox keeps.
const TASK_LINES: usize = 3;

/// What is known of why an agent asks: each only when known.
#[derive(Debug, PartialEq)]
pub(crate) struct Why {
    /// The kanban card it works on: its id and title.
    pub card: Option<(u64, String)>,
    /// The first lines of the task it was handed.
    pub task: Option<String>,
    /// What the agent said with `why`.
    pub agent: Option<String>,
}

/// The card each block works on in `boards`, by the block's id: its newest card not yet in its
/// board's last column.
pub(crate) fn cards_of(boards: &crate::board::Boards) -> HashMap<String, (u64, String)> {
    let mut cards: HashMap<String, (u64, String)> = HashMap::new();
    for board in &boards.boards {
        let open = board.columns.len().saturating_sub(1);
        for card in board.columns[..open].iter().flat_map(|c| &c.cards) {
            if let Some(Assignee::Block(uid)) = &card.assignee
                && cards.get(uid).is_none_or(|(id, _)| card.id > *id)
            {
                cards.insert(uid.clone(), (card.id, card.title.clone()));
            }
        }
    }
    cards
}

impl Workspace {
    /// The requests waiting for the person in the workspace shown (questions, plans and yes/no
    /// requests whose terminal is here), oldest first.
    pub(crate) fn inbox(&self) -> Vec<&Wait> {
        (self.requests_waiting().into_iter())
            .filter(|(_, space)| *space == self.space.id)
            .map(|(w, _)| w)
            .collect()
    }

    /// The requests waiting for the person in every workspace, with their workspace's id, oldest
    /// first: the Home counts them.
    pub(crate) fn requests_waiting(&self) -> Vec<(&Wait, String)> {
        self.waits
            .iter()
            .filter(|w| matches!(w.what, Waiting::Question { .. } | Waiting::Allow { .. }))
            // An answer sent to another computer leaves the list until its state drops it.
            .filter(|w| !w.remote.as_ref().is_some_and(|r| r.sent))
            .filter_map(|w| Some((w, self.request_space(w)?)))
            .collect()
    }

    /// What the inbox says of the requests of the mirrored workspace shown that it doesn't list:
    /// "And 3 more on winx-desktop."
    pub(crate) fn inbox_more(&self) -> Option<String> {
        let remote = self.space.remote.as_deref()?;
        let more = remote.requests_more;
        (more > 0).then(|| format!("And {more} more on {}.", crate::mirror::computer_of(remote)))
    }

    /// The request the inbox has selected: the one `inbox_at` names if it is listed, else the
    /// first.
    pub(crate) fn inbox_selected(&self) -> Option<&Wait> {
        let inbox = self.inbox();
        let at = inbox.iter().find(|w| Some(w.key) == self.inbox_at);
        at.or(inbox.first()).copied()
    }

    /// Opens the inbox with the keyboard, on the request `at` when it is listed; the Git popup
    /// closes.
    pub(crate) fn open_inbox(
        &mut self,
        at: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The requests are a workspace's: from the Home, the oldest one's is shown, else the one
        // held under it.
        if self.home_up() {
            let oldest = self.requests_waiting().first().map(|(_, s)| s.clone());
            let to = oldest.and_then(|s| self.order().iter().position(|o| o.id == s));
            self.switch_to(to.unwrap_or(self.active), window, cx);
        }
        self.inbox_open = true;
        self.git_open = false;
        self.agents_open = false;
        self.automations_open = false;
        if let Some(at) = at
            && self.inbox().iter().any(|w| w.key == at)
        {
            self.inbox_at = Some(at);
        }
        self.show_selected();
        self.inbox_focus.focus(window, cx);
        cx.notify();
    }

    /// Closes the inbox; the keyboard goes back to the canvas only if the inbox had it.
    pub(crate) fn close_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.inbox_open = false;
        if self.inbox_focus.contains_focused(window, cx) {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    /// The sidebar's button.
    pub(crate) fn toggle_inbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inbox_open {
            self.close_inbox(window, cx);
        } else {
            self.open_inbox(None, window, cx);
        }
    }

    /// The key of the selected request while the inbox has the keyboard: its popup or one of its
    /// fields holds it.
    pub(crate) fn inbox_keys(&self, window: &Window, cx: &App) -> Option<u64> {
        if !self.inbox_open || !self.inbox_focus.contains_focused(window, cx) {
            return None;
        }
        self.inbox_selected().map(|w| w.key)
    }

    /// Selects the request `key`, and gives the inbox the keyboard.
    pub(crate) fn select_request(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.inbox_at = Some(key);
        self.show_selected();
        self.inbox_focus.focus(window, cx);
        cx.notify();
    }

    /// The list scrolls, once drawn and laid out, to show the selected row.
    fn show_selected(&self) {
        self.inbox_reveal.set(true);
    }

    /// A request arrived in the workspace shown: the button pulses for a while, unless motion is
    /// reduced.
    pub(super) fn inbox_arrived(&mut self, cx: &App) {
        if !cx.reduce_motion() {
            self.inbox_pulse = Some(cx.background_executor().now());
        }
    }

    /// The button pulses at `now`: a request arrived less than `PULSE` ago.
    pub(crate) fn inbox_pulsing(&self, now: Instant) -> bool {
        self.inbox_pulse
            .is_some_and(|at| now.saturating_duration_since(at) < PULSE)
    }

    /// A click on a notification of the request `key`, which came from the workspace `space`: the
    /// workspace its block is in now is shown (`space` if the request is gone), with its inbox
    /// open at the request.
    pub(crate) fn show_request(
        &mut self,
        space: &str,
        key: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let now = self
            .waits
            .iter()
            .find(|w| w.key == key)
            .and_then(|w| self.request_space(w))
            .unwrap_or_else(|| space.to_string());
        let Some(to) = self.order().iter().position(|s| s.id == now) else {
            return;
        };
        self.close_welcome(window, cx);
        self.switch_to(to, window, cx);
        self.open_inbox(Some(key), window, cx);
    }

    /// Go to terminal: the block of the request `key` is revealed, and the inbox closes.
    pub(crate) fn go_to_request(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((space, id)) = self
            .waits
            .iter()
            .find(|w| w.key == key)
            .and_then(|w| self.asker(w))
        else {
            return;
        };
        self.close_inbox(window, cx);
        self.reveal(&space, id, window, cx);
    }

    /// Alt+↑ / Alt+↓: the previous or next request of the list is selected (none past its ends).
    fn inbox_step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let keys: Vec<u64> = self.inbox().iter().map(|w| w.key).collect();
        let at = self
            .inbox_selected()
            .and_then(|w| keys.iter().position(|k| *k == w.key));
        if let Some(at) = at
            && let Some(next) = at.checked_add_signed(by).and_then(|i| keys.get(i))
        {
            self.select_request(*next, window, cx);
        }
    }

    /// The popup's keys, with the keyboard in it: Esc closes it, Alt+↑ / Alt+↓ move in the list,
    /// the request's own keys (`question_key`); the workspace's shortcuts go on, but those that
    /// act on the terminal in front. While the theme picker is open the keys are its.
    pub(crate) fn on_inbox_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k: &Keystroke = &e.keystroke;
        let m = &k.modifiers;
        // As the workspace's own keys: no editor has the keyboard, so an editor's commands are
        // nothing here.
        let command = super::effective(
            self.keymap.command(k),
            false,
            self.space.grid.is_some(),
            false,
        );
        // The theme picker takes no focus: while it is open its keys are its own.
        if self.picker.is_some() {
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
            return;
        }
        let handled = if k.key == "escape" && !m.modified() {
            self.close_inbox(window, cx);
            true
        } else if m.alt
            && !m.control
            && !m.platform
            && !m.shift
            && matches!(k.key.as_str(), "up" | "down")
        {
            self.inbox_step(if k.key == "up" { -1 } else { 1 }, window, cx);
            true
        } else if let Some(key) = self.inbox_keys(window, cx) {
            self.question_key(key, k, window, cx)
        } else {
            false
        };
        if handled {
            cx.stop_propagation();
            return;
        }
        let command = command.filter(|c| {
            !matches!(
                c,
                Command::Copy
                    | Command::Paste
                    | Command::CloseTerminal
                    | Command::PreviousPrompt
                    | Command::NextPrompt
                    | Command::Find
            )
        });
        if command.is_some() {
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
        }
    }

    /// The next frame, after a request ended: the inbox gets the keyboard back if one of the
    /// fields that went with it had it (its element is gone, and keys would go to the window).
    /// A field that is still there keeps it.
    pub(super) fn inbox_keeps_keys(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inbox_open
            && std::mem::take(&mut self.inbox_refocus)
            && (window.focused(cx).is_none()
                || (self.inbox_focus.contains_focused(window, cx)
                    && !self.inbox_focus.is_focused(window)
                    && !self.waits.iter().any(|w| field_focused(w, window, cx))))
        {
            window.focus(&self.inbox_focus, cx);
        }
    }

    /// The requests `gone` (keys) ended: if the selected one is among them, the next is selected,
    /// or with none after it the first. Only a question or a yes/no request is selected, and
    /// those end only in the workspace shown. Called wherever requests end.
    pub(super) fn inbox_settle(&mut self, gone: &[u64]) {
        self.inbox_refocus |= self.inbox_open && !gone.is_empty();
        let Some(at) = self.inbox_at.filter(|at| gone.contains(at)) else {
            return;
        };
        self.inbox_at = self.inbox().iter().map(|w| w.key).find(|&key| key > at);
        self.show_selected();
    }

    /// The card each block works on, by the block's id: its newest card not yet in its board's
    /// last column, of any board.
    pub(crate) fn cards_by_block(&self) -> HashMap<String, (u64, String)> {
        cards_of(&self.boards)
    }

    /// Why the agent that made `wait` asks, as far as Templum knows and the agent said.
    pub(crate) fn why(&self, wait: &Wait) -> Why {
        let card = self.cards_by_block().remove(&self.uid_of(&wait.from));
        let task = self
            .pane(&wait.from)
            .and_then(|pane| pane.handed.as_ref())
            .map(|handed| {
                let lines: Vec<&str> = handed.text.lines().take(TASK_LINES).collect();
                lines.join("\n")
            })
            .filter(|task| !task.is_empty());
        let agent = match &wait.what {
            Waiting::Question { ask, .. } => ask.why.clone(),
            _ => None,
        };
        Why { card, task, agent }
    }
}
