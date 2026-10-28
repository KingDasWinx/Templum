//! The Templum Agent (docs/specs/templum-agent.md): the chat the person has with an agent that
//! builds and runs the canvas, and its state on the workspace.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, FocusHandle, Focusable as _, FollowMode, KeyDownEvent, ListAlignment,
    ListState, ScrollHandle, Subscription, Task, Window, px,
};
use serde_json::Value;
use templum_agent::{Conversation, Engine};
use templum_canvas::BlockId;
use templum_mcp::Replier;
use templum_mcp::control::{Level, Parsed};

use crate::keys::Command;
use crate::workspace::Workspace;

pub(crate) mod approval;
pub(crate) mod look;
pub(crate) mod panel;
pub(crate) mod pill;
mod rows;

/// The Templum Agent's state: one per Templum.
pub(crate) struct TemplumAgent {
    pub conversation: Conversation,
    /// The secret its engine's process says hello with (`TEMPLUM_CONTROL`): new at each start
    /// of the engine, so no older hello matches.
    pub secret: String,
    /// The tools "Allow for this conversation" let through, until New chat or quit.
    pub grants: BTreeSet<String>,
    /// The dangerous calls waiting for the person, oldest first.
    pub approvals: Vec<Approval>,
    pub next_approval: u64,
    /// It wrote something, or asks something, since the chat was last open.
    pub unread: bool,
    /// The chat is open.
    pub open: bool,
    /// The chat fills the canvas instead of its card: kept while Templum runs, closed or not.
    pub full: bool,
    /// Its engine's process, while it runs.
    pub engine: Option<Engine>,
    /// Counts the engine's starts: lines of an engine already gone are dropped.
    pub engine_gen: u64,
    /// Reads the engine's lines on the UI thread.
    pub lines: Option<Task<()>>,
    /// The engine's last lines on stderr, for the failure row when it ends.
    pub stderr: VecDeque<String>,
    /// The person pressed Stop: the turn's end is theirs, not a failure.
    pub stopping: bool,
    /// The settings changed while it worked: its process ends at the turn's end.
    pub restart: bool,
    /// The model the engine said it runs on.
    pub model: Option<String>,
    /// This start resumes a session and no turn has started in it yet: the messages it was
    /// given, for a new session when that one is gone.
    pub resuming: Option<Vec<String>>,
    /// The pill's field, made the first time it takes the keyboard.
    pub input: Option<Entity<TextareaState>>,
    _enter: Option<Subscription>,
    /// The block that had the keyboard before the pill took it.
    pub back: Option<BlockId>,
    /// The chat's rows, only those in view drawn: it follows its end while it is there.
    pub list: ListState,
    /// The rows the chat drew last, and whether some came below the view since.
    pub seen: Cell<usize>,
    pub unseen: Cell<bool>,
    /// The first row of the last arrival while the chat was open, and when: those rows fade in.
    pub arrival: Cell<Option<(usize, Instant)>>,
    /// The chat was drawn open last frame: rows there when it opens show at once.
    pub drawn: Cell<bool>,
    /// The run meter's list of its tasks is open.
    pub tasks_open: bool,
    /// The height the chat's body has grown to for its rows: it only goes up, and New chat starts
    /// it again.
    pub rows_h: Rc<Cell<f32>>,
    /// The rows opened to show their call, by number, and when: New chat closes them all.
    pub expanded: BTreeMap<usize, Instant>,
    /// The composer's model menu is open: the composer stays as it is while the menu has the
    /// keyboard.
    pub model_menu: bool,
    /// The blocks it just touched, by id, and when: their border glows for GLOW.
    pub glow: HashMap<String, Instant>,
    /// The approval's keyboard, and its field for the person's words, made with the first one.
    pub approval_focus: Option<FocusHandle>,
    pub words: Option<Entity<InputState>>,
    _words: Option<Subscription>,
    /// Whether Claude Code is installed, as the tests say it: yes unless they say otherwise,
    /// whatever this computer has.
    #[cfg(test)]
    pub installed: bool,
    /// The fake engine the tests run instead of Claude Code.
    #[cfg(test)]
    pub launch: Option<templum_agent::Launch>,
    /// The words of each start of the engine, as Claude Code would have got them.
    #[cfg(test)]
    pub launched: Vec<Vec<String>>,
    /// The messages the engine was given.
    #[cfg(test)]
    pub sent: Vec<String>,
}

impl Default for TemplumAgent {
    fn default() -> Self {
        Self {
            conversation: Conversation::new("claude"),
            secret: uuid::Uuid::new_v4().simple().to_string(),
            grants: BTreeSet::new(),
            approvals: Vec::new(),
            next_approval: 0,
            unread: false,
            open: false,
            full: false,
            engine: None,
            engine_gen: 0,
            lines: None,
            stderr: VecDeque::new(),
            stopping: false,
            restart: false,
            model: None,
            resuming: None,
            input: None,
            _enter: None,
            back: None,
            list: {
                let list = ListState::new(0, ListAlignment::Bottom, px(400.0));
                list.set_follow_mode(FollowMode::Tail);
                list
            },
            seen: Cell::new(0),
            unseen: Cell::new(false),
            arrival: Cell::new(None),
            drawn: Cell::new(false),
            tasks_open: false,
            rows_h: Rc::new(Cell::new(0.0)),
            expanded: BTreeMap::new(),
            model_menu: false,
            glow: HashMap::new(),
            approval_focus: None,
            words: None,
            _words: None,
            #[cfg(test)]
            installed: true,
            #[cfg(test)]
            launch: None,
            #[cfg(test)]
            launched: Vec::new(),
            #[cfg(test)]
            sent: Vec::new(),
        }
    }
}

/// A dangerous call held until the person answers (docs/specs/templum-agent.md#approvals): its
/// connection and request, to answer it; the call; and what the popup says of it, written by
/// Templum from the call.
pub(crate) struct Approval {
    pub id: u64,
    pub conn: u64,
    pub request: Value,
    pub reply: Replier,
    pub parsed: Parsed,
    /// The call's arguments as the engine sent them: its row in the chat.
    pub args: Value,
    pub tool: String,
    /// Ask, or Ask each time (no "Allow for this conversation").
    pub level: Level,
    /// When it first showed at the front, by the executor's clock: clicks before KEYS_AFTER are
    /// ignored.
    pub shown: Cell<Option<Instant>>,
    /// When it took the keyboard: keys before KEYS_AFTER are ignored.
    pub keys: Cell<Option<Instant>>,
    /// The action in one sentence: `Close "Codex · api"`.
    pub said: String,
    /// Exactly what will happen: the text to type, the keys, what stops.
    pub details: String,
    /// What the person saw of its block, written with its words: an Allow after it changed is
    /// asked again.
    pub facts: String,
    /// It was asked again: what it is about changed since the person looked.
    pub changed: bool,
    /// Its box of details, and whether it was seen to its end: until then, nothing allows it.
    pub scroll: ScrollHandle,
    pub read: Cell<bool>,
}

impl Approval {
    /// Its box of details was seen to its end (laid out, scrolled to its end), or it has none:
    /// until then nothing allows it.
    pub fn seen(&self) -> bool {
        let scroll = &self.scroll;
        let laid_out = scroll.bounds().size.height > px(0.0);
        if self.details.is_empty()
            || laid_out && -scroll.offset().y >= scroll.max_offset().y - px(1.0)
        {
            self.read.set(true);
        }
        self.read.get()
    }
}

/// The person's answer to an approval.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Answer {
    Once,
    Conversation,
    /// Denied, with their words when they wrote some.
    Deny(Option<String>),
}

impl TemplumAgent {
    /// Whether the engine's program can be run.
    pub fn installed(&self) -> bool {
        #[cfg(test)]
        let installed = self.installed;
        #[cfg(not(test))]
        let installed = templum_terminal::installed("claude");
        installed
    }
}

/// How long a block the Templum Agent touched glows.
pub(crate) const GLOW: Duration = Duration::from_millis(1200);

/// How lit a glow that started `at` is `now`, 0 to 1: its border goes to the accent twice over
/// GLOW; None once it is over.
pub(crate) fn glow(at: Instant, now: Instant) -> Option<f32> {
    let t = now.checked_duration_since(at)?.as_secs_f32() / GLOW.as_secs_f32();
    (t < 1.0).then(|| (1.0 - (t * 4.0 * std::f32::consts::PI).cos()) / 2.0)
}

/// The placeholder of the pill's field.
const PLACEHOLDER: &str = "Ask Templum to build, run or tidy your canvas…";

impl Workspace {
    /// The `agent` shortcut: the pill takes the keyboard (the chat opens when the conversation
    /// has rows); pressed again while the pill has it, the chat closes and the keyboard goes back.
    pub(crate) fn toggle_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.agent_has_keys(window, cx) {
            self.leave_agent(window, cx);
            return;
        }
        if !self.templum_agent.installed() {
            self.open_agent_settings(window, cx);
            return;
        }
        // The field, even while an approval waits: Enter there denies it with the words. Only a
        // click on the approval, or Review, gives it the keyboard, so no digit meant for the field
        // answers it.
        let input = self.agent_input(window, cx);
        self.templum_agent.back = self.space.focused;
        input.update(cx, |input, cx| input.focus(window, cx));
        let agent = &mut self.templum_agent;
        if !agent.conversation.rows.is_empty() {
            agent.open = true;
            agent.unread = false;
        }
        cx.notify();
    }

    /// Review, on the pill that asks: the pill takes the keyboard and the chat opens at the
    /// approval.
    pub(crate) fn review_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The chat is drawn on a canvas: from the Home, the workspace held under it is shown.
        if self.home_up() {
            self.switch_to(self.active, window, cx);
        }
        if !self.agent_has_keys(window, cx) {
            let input = self.agent_input(window, cx);
            self.templum_agent.back = self.space.focused;
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        self.focus_approval(window, cx);
        self.templum_agent.open = true;
        self.templum_agent.unread = false;
        cx.notify();
    }

    /// Esc in the pill: the chat closes and the block that had the keyboard has it again. The
    /// text stays.
    pub(crate) fn leave_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.templum_agent.open = false;
        match self
            .templum_agent
            .back
            .take()
            .filter(|id| self.space.canvas.get(*id).is_some())
        {
            Some(id) => self.focus_block(id, window, cx),
            None => window.focus(&self.focus_handle, cx),
        }
        cx.notify();
    }

    /// Whether the pill's field, or an approval of the Templum Agent, has the keyboard.
    pub(crate) fn agent_has_keys(&self, window: &Window, cx: &gpui_kit::App) -> bool {
        let agent = &self.templum_agent;
        agent
            .input
            .as_ref()
            .is_some_and(|i| i.read(cx).focus_handle(cx).is_focused(window))
            || agent
                .approval_focus
                .as_ref()
                .is_some_and(|f| f.contains_focused(window, cx))
    }

    /// The front approval takes the keyboard, when one waits: its keys count from now, unless
    /// it already had it.
    pub(crate) fn focus_approval(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.templum_agent.approvals.is_empty() {
            return;
        }
        let focus = self.approval_ui(window, cx);
        let front = &self.templum_agent.approvals[0];
        if !focus.is_focused(window) || front.keys.get().is_none() {
            front.keys.set(Some(cx.background_executor().now()));
        }
        window.focus(&focus, cx);
    }

    /// The front approval showed or took the keyboard under KEYS_AFTER ago: a press on it, or a
    /// key (`keys`), is ignored.
    pub(crate) fn approval_too_soon(&self, keys: bool, cx: &gpui_kit::App) -> bool {
        let Some(front) = self.templum_agent.approvals.first() else {
            return true;
        };
        let now = cx.background_executor().now();
        let soon = |at: &Cell<Option<Instant>>| {
            at.get()
                .is_none_or(|at| now.duration_since(at) < approval::KEYS_AFTER)
        };
        soon(&front.shown) || (keys && soon(&front.keys))
    }

    /// The approval's focus and its field, made the first time one shows.
    pub(crate) fn approval_ui(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FocusHandle {
        if let Some(focus) = &self.templum_agent.approval_focus {
            return focus.clone();
        }
        let focus = cx.focus_handle();
        let words =
            cx.new(|cx| InputState::new(window, cx).placeholder("Or tell it what to do instead"));
        let enter = cx.subscribe_in(
            &words,
            window,
            |ws, words, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    let text = words.read(cx).value().to_string();
                    words.update(cx, |w, cx| w.set_value("", window, cx));
                    if let Some(id) = ws.templum_agent.approvals.first().map(|a| a.id) {
                        ws.answer_approval(id, Answer::Deny(Some(text)), window, cx);
                        ws.after_approval(window, cx);
                    }
                }
            },
        );
        let agent = &mut self.templum_agent;
        agent.approval_focus = Some(focus.clone());
        agent.words = Some(words);
        agent._words = Some(enter);
        focus
    }

    /// An approval was answered from its popup: the next one keeps the keyboard, else the pill
    /// has it again.
    pub(crate) fn after_approval(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.templum_agent.approvals.is_empty() {
            self.focus_approval(window, cx);
        } else if let Some(input) = self.templum_agent.input.clone() {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    /// A button of the popup: the front approval answered; none within KEYS_AFTER of it
    /// showing, as the balloon's.
    pub(crate) fn answer_front(
        &mut self,
        answer: Answer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.approval_too_soon(false, cx) {
            return;
        }
        let Some(front) = self.templum_agent.approvals.first() else {
            return;
        };
        if answer != Answer::Deny(None) && !front.seen() {
            return;
        }
        if let Some(id) = self.templum_agent.approvals.first().map(|a| a.id) {
            self.answer_approval(id, answer, window, cx);
            self.after_approval(window, cx);
        }
    }

    /// Keys while the approval itself has the keyboard: `1` Allow once, `2` Allow for this
    /// conversation (not for one that asks each time), `3` or Esc Deny; none within KEYS_AFTER
    /// of it showing or taking the keyboard. Templum's shortcuts run.
    pub(crate) fn on_approval_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focused = self
            .templum_agent
            .approval_focus
            .as_ref()
            .is_some_and(|f| f.is_focused(window));
        let Some(front) = self.templum_agent.approvals.first() else {
            return;
        };
        let k = &e.keystroke;
        if !focused || k.modifiers.modified() {
            return;
        }
        let answer = match k.key.as_str() {
            "1" => Some(Answer::Once),
            "2" if front.level == Level::Ask => Some(Answer::Conversation),
            "3" | "escape" => Some(Answer::Deny(None)),
            _ => None,
        };
        let Some(answer) = answer else {
            return;
        };
        cx.stop_propagation();
        if !self.approval_too_soon(true, cx) {
            self.answer_front(answer, window, cx);
        }
    }

    /// The pill's field: 1 to 8 lines, Enter sends, Shift+Enter is a new line.
    fn agent_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<TextareaState> {
        if let Some(input) = &self.templum_agent.input {
            return input.clone();
        }
        let input = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .submit_on_enter(true)
                .placeholder(PLACEHOLDER)
        });
        let enter = cx.subscribe_in(&input, window, |ws, _, event: &InputEvent, window, cx| {
            if let InputEvent::PressEnter { shift: false, .. } = event {
                ws.submit_to_agent(window, cx);
            }
        });
        self.templum_agent.input = Some(input.clone());
        self.templum_agent._enter = Some(enter);
        input
    }

    /// Enter in the pill: its text goes to the Templum Agent, the field empties, the chat opens.
    pub(crate) fn submit_to_agent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.templum_agent.installed() {
            self.open_agent_settings(window, cx);
            return;
        }
        let Some(input) = self.templum_agent.input.clone() else {
            return;
        };
        let text = input.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        input.update(cx, |input, cx| input.set_value("", window, cx));
        self.templum_agent.open = true;
        self.templum_agent.unread = false;
        self.send_to_agent(text, window, cx);
    }

    /// A suggestion of the empty chat, clicked: its text in the field, which takes the keyboard.
    pub(crate) fn suggest_to_agent(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Catch me up is sent at once.
        if text == panel::CATCH_UP {
            return self.send_to_agent(text.to_string(), window, cx);
        }
        let input = self.agent_input(window, cx);
        if !self.agent_has_keys(window, cx) {
            self.templum_agent.back = self.space.focused;
        }
        input.update(cx, |input, cx| {
            input.set_value(text.to_string(), window, cx);
            input.focus(window, cx);
        });
        cx.notify();
    }

    /// A click on row `n`: what its action touched is shown, its workspace first.
    pub(crate) fn show_row(&mut self, n: usize, window: &mut Window, cx: &mut Context<Self>) {
        let target = match self.templum_agent.conversation.rows.get(n).cloned() {
            Some(
                templum_agent::Row::Action {
                    target: Some(target),
                    ..
                }
                | templum_agent::Row::Notice {
                    target: Some(target),
                    ..
                },
            ) => target,
            _ => return,
        };
        let block = target.block.as_deref().and_then(|uid| self.find_block(uid));
        match (block, target.workspace) {
            (Some((space, id)), _) => self.reveal(&space, id, window, cx),
            (None, Some(space)) => {
                if let Some(to) = self.order().iter().position(|s| s.id == space) {
                    self.switch_to(to, window, cx);
                }
            }
            (None, None) => {}
        }
    }

    /// Row `n` opens to show its call, or closes; the rows under it move.
    pub(crate) fn toggle_row(&mut self, n: usize, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let agent = &mut self.templum_agent;
        if agent.expanded.remove(&n).is_none() {
            agent.expanded.insert(n, now);
        }
        agent.list.remeasure_items(n..n + 1);
        cx.notify();
    }

    /// The models its agent offers (Claude Code: haiku, sonnet, opus) that Templum can type to
    /// start it; none for an engine with no list.
    pub(crate) fn engine_models(&self) -> Vec<String> {
        let id = &self.config.templum_agent.agent;
        let engine = templum_terminal::Agent::from_name(id);
        self.agents
            .iter()
            .find(|a| a.id == *id)
            .into_iter()
            .flat_map(|a| &a.models)
            .filter(|m| engine.is_some_and(|e| e.launch_args(Some(m), None).is_ok()))
            .cloned()
            .collect()
    }

    /// Settings ▸ Templum Agent.
    pub(crate) fn open_agent_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.as_ref().is_some_and(|p| !p.closing) {
            self.toggle_settings(window, cx);
        }
        self.set_section(crate::settings::Section::TemplumAgent, cx);
    }

    /// Keys while the pill or the chat has the keyboard: Esc gives it back; Templum's shortcuts
    /// run, as with the settings panel focused; nothing reaches a terminal.
    pub(crate) fn on_agent_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &e.keystroke;
        if k.key == "escape" && !k.modifiers.modified() {
            self.leave_agent(window, cx);
            cx.stop_propagation();
            return;
        }
        // Copy, paste and close act on the focused terminal, which doesn't have the keyboard now.
        let command = self.keymap.command(k).filter(|c| {
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
}
