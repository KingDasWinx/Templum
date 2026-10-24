//! Tasks handed between agents (docs/specs/connections.md, Delegating): typed into an agent wired
//! to the one handing it over; its answer is the end of that agent's turn, or its `reply`.

use std::time::{Duration, Instant};

use futures::channel::oneshot;
use gpui_kit::{App, Context, Task};
use templum_agent::NoticeLevel;
use templum_canvas::BlockId;
use templum_mcp::{Reply, tools};
use templum_terminal::{KEY_NAMES, KeyInput, named_key};
use templum_usage::Session;

use super::activity::STILL;
use super::bus::{Outcome, clean_title, flat, target};
use super::waits::{LAST_LINES, Waiting};
use super::{AgentState, Handed, Pane, Prompt, Space, Workspace};

/// How long apart Templum looks at an agent's screen while a typed task waits for its Enter, and
/// after it: an agent may take keys that come in one burst for a paste, where Enter is a new line.
pub(super) const ENTER_AFTER: Duration = Duration::from_millis(150);
/// How many times, ENTER_AFTER apart, Templum looks for the typed task on the agent's screen
/// before it gives up on its Enter: about 2 s, for an agent slow to draw.
const ENTER_LOOKS: u32 = 13;
/// The lines at the end of a terminal compared to tell whether its screen changed.
const SCREEN_LINES: usize = 100;
/// Looks in a row the screen must stay the same after the agent's terminal is resized: it has
/// redrawn at its new size, and read what was typed meanwhile.
const SETTLED: u32 = 2;
/// How long after a task's Enter an agent whose hooks speak must say it took it (`Submitted`), or
/// open a dialog: only a prompt taken confirms it. Another's screen must have moved by then.
pub(super) const CONFIRM: Duration = Duration::from_secs(3);
/// The looks after a task's Enter, ENTER_AFTER apart, that tell whether its agent took it; the
/// first only reads the screen the next ones are compared to.
const CHECKS: u32 = (CONFIRM.as_millis() / ENTER_AFTER.as_millis()) as u32 - 1;
/// How long after Templum typed into an agent a notice waits: that text's first Enter may come
/// ENTER_LOOKS looks after it was typed, its second CONFIRM after that, and the looks after the
/// second go on CONFIRM more; a notice typed before then would go with it, or move the screen
/// they read.
pub(super) const NOTICE_GAP: Duration = CONFIRM
    .saturating_mul(2)
    .saturating_add(ENTER_AFTER.saturating_mul(ENTER_LOOKS + 1));
/// The person typed into an agent this recently: they are still typing, and a second Enter for a
/// task would send their half-written line.
pub(super) const TYPING: Duration = Duration::from_secs(3);
/// Between two keys `send_keys` presses: an Esc and the key after it in one burst read as
/// Alt+key.
const KEY_GAP: Duration = Duration::from_millis(50);
/// How long after its last key `send_keys` reads the screen it answers with.
const KEYS_SETTLE: Duration = Duration::from_millis(300);
/// The last lines of a screen Templum answers with (`send_keys`, a task not taken).
pub(super) const TAIL_LINES: usize = 20;
/// What a task's first line asks, after who hands it over: Claude Code takes a long paste alone
/// for data it was shown, and does what typed text with it asks. It asks for the answer through
/// `reply` (docs/specs/agent-protocol.md, 1).
pub(super) const TASK_LEAD: &str =
    "Your task is in the pasted text: do it, then answer it with reply.";
/// What a change's first line asks, after who changes which task.
pub(super) const CHANGE_LEAD: &str =
    "The change is in the pasted text: do it, then answer the task with reply.";

/// What ends a task's pasted text (a change's too): how task `id` is answered, with `reply`, or
/// from a shell where the agent has no Templum tools.
pub(super) fn answer_how(id: u64) -> String {
    format!(
        "\n\n(When task t{id} is done, call Templum's reply tool once, with your whole answer as \
         its text: that ends the task and hands your answer over. Without Templum's tools, run \
         `templum reply -` in your shell, the answer on its standard input.)"
    )
}
/// What the first line of a task's end says follows it: its answer, or why it failed.
pub(super) const DONE_LEAD: &str = "Its answer is in the pasted text.";
pub(super) const FAILED_LEAD: &str = "What happened is in the pasted text.";
/// What the first line of a notice of a task's dialog says follows it.
pub(super) const SCREEN_LEAD: &str = "Its screen is in the pasted text.";
/// How long after a task's end what it spent is read: Claude Code's status line gives its last
/// cost about 0.3 s after its `Stop`.
pub(super) const SPENT_AFTER: Duration = Duration::from_secs(1);

/// What a task spent between its agent's usage `from` (when it was handed over) and `to`, as its
/// usage chip shows it: its cost when `to` has one, else its tokens; None when unknown or when
/// nothing new was read (a timeout's notice before its agent wrote a figure). Cost first: Claude
/// Code's token totals are its last call's context, not what it used (batch 8's spike S8-1).
/// Nothing read at the hand-over is nothing spent: a session's first task counts it whole.
// ponytail: a resumed session handed a task before Templum's first read of it (session::EVERY)
// counts its earlier turns too; read its usage at the hand-over if that bites.
fn spent(from: Option<&Session>, to: Option<&Session>) -> Option<String> {
    let to = to?;
    if let Some(b) = to.cost_usd {
        let a = from.and_then(|f| f.cost_usd).unwrap_or(0.0);
        return (b > a).then(|| format!("${:.2}", b - a));
    }
    let b = to.tokens?;
    let a = from.and_then(|f| f.tokens).unwrap_or(0);
    (b > a).then(|| format!("{} tokens", b - a))
}
/// How many rows at the bottom of a screen an agent's input and its footer take at most: a row
/// with a prompt marker higher up is output.
const INPUT_ROWS: usize = 6;

/// The input of the agent whose screen is `screen` holds no text: its last line that opens with
/// a prompt marker (Codex's `›`, Claude Code's `❯` or `>`, a plain agent's `>`), inside the box
/// some draw around it, has nothing after the marker. Its faint suggestion isn't on `screen`
/// (`SharedTerm::lines`). False when none is found in the last `INPUT_ROWS` rows: a dialog, or an
/// input of another shape (Claude Code's shell mode has none).
fn input_empty(screen: &str) -> bool {
    screen
        .lines()
        .rev()
        .take(INPUT_ROWS)
        .map(|line| line.trim_matches(|c: char| c.is_whitespace() || matches!(c, '│' | '┃')))
        .find_map(|line| line.strip_prefix(['›', '❯', '>']))
        .is_some_and(|rest| rest.trim().is_empty())
}
/// How long a terminal says which block pressed keys in it ("answered by").
pub(crate) const ANSWERED_FOR: Duration = Duration::from_secs(10);

/// How long a notice about a task waits for others to the same agent, from the newest one.
pub(super) const BATCH_WAIT: Duration = Duration::from_secs(15);
/// The most a notice about a task waits, however many come after it.
pub(super) const BATCH_MOST: Duration = Duration::from_secs(60);
/// What the first line of a batch of notices says follows it.
const BATCH_LEAD: &str = "They are in the pasted text.";

/// What a notice is about, and so whether it waits for others (docs/specs/notice-batching.md).
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum NoticeKind {
    /// A task Templum types into the agent that handed it: its end, stuck, quiet, or waits for its
    /// person. These go in batches (`deliver_notices`).
    Task(NoticeLevel),
    /// Anything else (a watch that matched, a schedule's reminder, a question's answer): as soon
    /// as its agent takes it.
    Other,
}

/// What a task's agent did, to type into the agent that handed it (its terminal `to`).
pub(crate) struct Notice {
    pub to: String,
    pub text: String,
    pub kind: NoticeKind,
    /// The terminal of the agent it is about and the task's id, for a notice about a task.
    pub about: Option<(String, u64)>,
    /// When it was queued, by the executor's clock.
    pub at: Instant,
}

/// What a look at an agent's screen, while a typed task waits for its Enter, says to do.
#[derive(Debug, PartialEq)]
enum Step {
    Wait,
    Enter,
    GiveUp,
    /// Its person typed into it since the paste: an Enter would send their text with the task.
    Typing,
}

/// The wait for a typed task's Enter (`send_task`): the agent's screen must show the paste, then
/// stand still a look. A resize meanwhile redraws it at its new size, which shows nothing of the
/// paste.
struct EnterWait {
    /// The screen the paste must change.
    before: String,
    size: (u16, u16),
    /// Since a resize: the last screen, and how many looks in a row saw it.
    settling: Option<(String, u32)>,
    /// The screen the last look saw showing the paste.
    shown: Option<String>,
    looks: u32,
}

impl EnterWait {
    fn new(before: String, size: (u16, u16)) -> Self {
        Self {
            before,
            size,
            settling: None,
            shown: None,
            looks: 0,
        }
    }

    /// A look at the agent's `screen`, its terminal now `size`; `takes`: Templum may still type
    /// into it; `speaks`: its hooks say when it starts a turn or asks its person, so a dialog of
    /// it would be heard, and its Enter is confirmed; `typed`: its person typed into it since the
    /// paste. After a resize, the screen settling shows such an agent read the paste; for another
    /// (Codex, whose approval Templum doesn't hear) it is the new start, and only a change after
    /// it is the paste.
    fn look(
        &mut self,
        screen: String,
        size: (u16, u16),
        (takes, speaks, typed): (bool, bool, bool),
    ) -> Step {
        if !takes {
            return Step::GiveUp;
        }
        if typed {
            return Step::Typing;
        }
        self.looks += 1;
        if size != self.size {
            self.size = size;
            self.settling = Some((screen, 0));
            self.looks = 0;
            return Step::Wait;
        }
        if let Some((last, still)) = self.settling.take() {
            let still = if screen == last { still + 1 } else { 0 };
            if still >= SETTLED {
                if speaks {
                    return Step::Enter;
                }
                self.before = screen;
                self.shown = None;
                self.looks = 0;
                return Step::Wait;
            }
            self.settling = Some((screen, still));
            return match (self.looks < ENTER_LOOKS, speaks) {
                (true, _) => Step::Wait,
                // Moving all along: alive, and reading what it is typed.
                (false, true) => Step::Enter,
                (false, false) => Step::GiveUp,
            };
        }
        if screen == self.before {
            return if self.looks < ENTER_LOOKS {
                Step::Wait
            } else {
                Step::GiveUp
            };
        }
        // The paste shows. An agent that reads it as keys (Codex and Copilot CLI on Windows,
        // whose console passes no bracketed paste on) takes an Enter that comes while they still
        // arrive, or just after, for a new line in it: the Enter waits until the screen stood
        // still a look. One that keeps moving (at work) gets it after ENTER_LOOKS.
        if self.shown.as_ref() == Some(&screen) || self.looks >= ENTER_LOOKS {
            return Step::Enter;
        }
        self.shown = Some(screen);
        Step::Wait
    }
}

/// Why an agent didn't take the task typed into it, as its asker is told.
#[derive(Debug, PartialEq)]
enum NotTaken {
    /// Its screen never showed the paste: no Enter went.
    Unseen,
    /// Its hooks speak, and none said it took the task, though its screen changed.
    Unheard,
    /// Two Enters changed nothing on its screen.
    Unsent,
    /// Its person typed into it before its Enter: no Enter went.
    Typing,
}

impl NotTaken {
    fn why(&self) -> &'static str {
        match self {
            NotTaken::Unseen => {
                "its screen never showed it: it may be showing a dialog, or its input wasn't ready"
            }
            NotTaken::Unheard => "it may be showing a dialog",
            NotTaken::Unsent => {
                "typed but not submitted: two Enters changed nothing on its screen, and its input \
                 may still hold it"
            }
            NotTaken::Typing => {
                "its person is typing in it: an Enter would have sent their text with it, and its \
                 input may hold both"
            }
        }
    }

    /// What its asker should do next.
    fn then(&self) -> &'static str {
        match self {
            NotTaken::Unseen | NotTaken::Unheard => "Look at its block, then try again.",
            NotTaken::Unsent => {
                "Look at its block, and clear its input (send_keys) before you try again: else the \
                 task goes in twice."
            }
            NotTaken::Typing => "Try again once its person is done.",
        }
    }
}

/// What a look at an agent's screen after a typed task's Enter says.
#[derive(Debug, PartialEq)]
enum Check {
    Wait,
    Taken,
    /// Press Enter once more: the first may have joined the paste as a new line, or done nothing.
    Again,
    Not(NotTaken),
}

/// The looks after a typed task's Enter (`send_task`), ENTER_AFTER apart for CONFIRM: did its
/// agent take it? One whose hooks speak says so (`Submitted`); another's screen moves (a spinner,
/// a timer). A screen that didn't change since just after the Enter gets the Enter once more
/// where that Enter can't answer a dialog: it changed nothing (the text still sits in the input),
/// or the paste went as keys, which an agent may take an Enter among for a new line in it. A
/// screen the Enter changed otherwise may be a dialog drawn at once (Aider's "Add file to the
/// chat? [Yes]"), and gets nothing more.
struct EnterCheck {
    /// Its hooks spoke before the Enter.
    spoke: bool,
    /// The prompts it had taken at the Enter.
    submitted: u64,
    /// Its screen when the Enter went.
    before: String,
    /// The paste went as keys (Windows' console, a program that asked for no bracketed paste).
    keys: bool,
    /// An Enter changed it: a new line in its input, the task taken, or a dialog.
    moved: bool,
    /// Its screen ENTER_AFTER after the Enter, once read, and at the last look.
    after: Option<String>,
    last: String,
    /// Looks in a row that saw the screen change.
    moving: u32,
    looks: u32,
    /// The Enter was the second.
    second: bool,
}

impl EnterCheck {
    fn new(spoke: bool, submitted: u64, before: String, keys: bool) -> Self {
        Self {
            spoke,
            submitted,
            before,
            keys,
            moved: false,
            after: None,
            last: String::new(),
            moving: 0,
            looks: 0,
            second: false,
        }
    }

    /// A look at the agent's `screen`; `submitted`: the prompts it has taken now; `takes`:
    /// Templum may still type into it (no dialog its hooks told of, its shell not back in front);
    /// `typing`: its person typed into it lately.
    fn look(&mut self, screen: String, submitted: u64, takes: bool, typing: bool) -> Check {
        // A prompt taken, or a dialog it opened: what comes next tells the rest.
        if submitted != self.submitted || !takes {
            return Check::Taken;
        }
        let Some(after) = &self.after else {
            self.moved |= screen != self.before;
            self.after = Some(screen.clone());
            self.last = screen;
            return Check::Wait;
        };
        let still = screen == *after;
        self.moving = if screen != self.last {
            self.moving + 1
        } else {
            0
        };
        self.last = screen;
        self.looks += 1;
        // At work: no hook says so, its screen moving does.
        if !self.spoke && self.moving >= 2 {
            return Check::Taken;
        }
        if self.looks < CHECKS {
            return Check::Wait;
        }
        match (still && !typing, self.second, self.spoke) {
            (true, false, _) if !self.moved || self.keys => Check::Again,
            // An Enter did something, then nothing moved: it took the task, and works drawing
            // nothing, or it drew a dialog no hook tells of. Templum can't tell, and presses
            // nothing more into it.
            (true, _, false) if self.moved => Check::Taken,
            (true, true, _) => Check::Not(NotTaken::Unsent),
            // It changed, then stopped: the same.
            (_, _, false) => Check::Taken,
            (_, _, true) => Check::Not(NotTaken::Unheard),
        }
    }

    /// The second Enter went, into the screen the last look saw: the looks start over.
    fn again(&mut self) {
        let before = std::mem::take(&mut self.last);
        *self = Self {
            second: true,
            moved: self.moved,
            ..Self::new(self.spoke, self.submitted, before, self.keys)
        };
    }
}

impl Pane {
    /// Who pressed keys in it (`send_keys`, `interrupt`), while ANSWERED_FOR hasn't passed.
    pub fn answered_by(&self) -> Option<&str> {
        self.keys_from
            .as_ref()
            .filter(|(_, at)| at.elapsed() < ANSWERED_FOR)
            .map(|(by, _)| by.as_str())
    }

    /// The person typed into it: a key, a paste, text.
    pub(super) fn person_typed(&self) {
        self.typed.set(true);
        self.keyed.set(Some(Instant::now()));
    }

    /// The person typed or pasted text into it: its agent's input holds a draft, unless the
    /// agent waits for its person (a dialog takes the text as its answer).
    pub(super) fn person_wrote(&self) {
        self.person_typed();
        if !matches!(self.state, Some(AgentState::NeedsYou(_))) {
            self.draft.set(true);
        }
    }

    /// The person pressed `key` (not text: that is `person_wrote`) in it, and what it does to its
    /// agent's input, as far as keys tell. Shift+Enter and Alt+Enter add a line to the draft; so
    /// does Claude Code's `\` and Enter, which is why its Enter ends nothing: its `Submitted`
    /// does (`bus.rs`). Another agent's Enter sends the draft. Tab takes the suggestion the input
    /// shows, Up and Down recall history into it, Ctrl+Y yanks back what was deleted; Ctrl+C
    /// empties it. Backspace, Esc and Ctrl+U (to the start of the line only) say nothing: the
    /// screen does (`settle_draft`). Keys that answer a dialog change nothing in the input.
    pub(super) fn person_keyed(&self, key: &KeyInput) {
        self.person_typed();
        if matches!(self.state, Some(AgentState::NeedsYou(_))) {
            return;
        }
        // Claude Code whose hooks were heard says `Submitted` at its Enter; without them (no bus,
        // `disableAllHooks`) only the screen can end a draft, so its Enter does.
        let claude = self.seen.agent.as_deref() == Some("claude");
        match (key.key, key.ctrl) {
            ("enter", false) if key.shift || key.alt => self.draft.set(true),
            ("enter", false) if !(claude && self.speaks) => self.draft.set(false),
            ("c", true) => self.draft.set(false),
            ("tab", false) if !key.shift => self.draft.set(true),
            ("up" | "down", false) | ("y", true) => self.draft.set(true),
            _ => {}
        }
    }

    /// A draft its person's last key is not recent for (TYPING) ends when the screen shows its
    /// agent's input empty: Backspace to empty, an Esc that cleared it, or a send Templum never
    /// heard. An input Templum can't read stays a draft: it waits.
    /// True when it did.
    pub(super) fn settle_draft(&self) -> bool {
        let still_typing = self.keyed.get().is_some_and(|at| at.elapsed() < TYPING);
        let ends = self.has_draft() && !still_typing && input_empty(&self.screen());
        if ends {
            self.draft.set(false);
        }
        ends
    }

    /// Its agent's input holds text its person typed and didn't send.
    pub fn has_draft(&self) -> bool {
        self.draft.get()
    }

    /// The last `TAIL_LINES` lines its terminal shows, as an agent is answered with them.
    pub(super) fn tail(&self) -> String {
        self.term.shared().lines(TAIL_LINES).join("\n")
    }

    /// The end of what its terminal shows, to tell whether it changed.
    pub fn screen(&self) -> String {
        self.term.shared().lines(SCREEN_LINES).join("\n")
    }

    /// The task its agent works on, while its answer is awaited.
    pub fn task(&self, now: Instant) -> Option<&Handed> {
        self.handed.as_ref().filter(|h| now < h.until)
    }

    /// Its agent is at work: on a task, on a turn, on a question to its person, or about to get
    /// its prompt.
    pub fn busy(&self, now: Instant) -> bool {
        self.task(now).is_some()
            || self.prompt.is_some()
            || matches!(
                self.state,
                Some(AgentState::Working | AgentState::NeedsYou(_))
            )
    }

    /// Its shell, not the agent, is in front: an agent seen there quit, though the last snapshot
    /// (up to 5 s old) still has it. Its process group, found in front by a snapshot, tells (on
    /// macOS the shell isn't the terminal's process, `login` is); before one found it, its shell
    /// being in front does. False when the OS can't tell (Windows).
    pub(super) fn shell_in_front(&self) -> bool {
        let p = self.term.processes();
        match (p.foreground, self.seen.pid) {
            (None, _) => false,
            (front, Some(agent)) => front != Some(agent),
            (front, None) => front == p.shell,
        }
    }

    /// Templum may type into it (its block id `uid`): an agent runs there, not waiting for its
    /// person (a permission dialog would take the text as its answer). Never a shell, where the
    /// text would run as a command.
    fn takes_typing(&self, uid: &str) -> Result<(), Reply> {
        self.read_only(uid)?;
        if self.seen.agent.is_none() || self.shell_in_front() {
            return Err(Reply::err(format!(
                "{uid} runs no agent: send_to_terminal types into a terminal."
            )));
        }
        if let Some(AgentState::NeedsYou(what)) = &self.state {
            let asks = match what.as_deref().filter(|w| !w.is_empty()) {
                Some(what) => format!(": {what}"),
                None => String::new(),
            };
            return Err(Reply::err(format!(
                "{uid} is waiting for its person{asks}. Templum won't type into it now; send_keys \
                 answers its dialog. Its screen:\n{}",
                tools::tail(&self.tail(), tools::MAX_TEXT)
            )));
        }
        Ok(())
    }

    /// Templum may type a notice into it now (docs/specs/connections.md, Delegating 10). None:
    /// never, no agent runs there (a shell would run the text). Some(false): not now, while its
    /// agent is starting (no snapshot found it yet: macOS can't tell its shell is in front before
    /// one did), waits for its person, its person typed into it in the last TYPING, or Templum did
    /// in the last NOTICE_GAP (that text's Enter, and a second one, may still be due).
    pub(super) fn takes_notice(&self) -> Option<bool> {
        self.seen.agent.as_ref()?;
        if self.seen.pid.is_none() {
            return Some(false);
        }
        if self.shell_in_front() {
            return None;
        }
        let recent = |at: Option<Instant>, within| at.is_some_and(|at| at.elapsed() < within);
        Some(
            !matches!(self.state, Some(AgentState::NeedsYou(_)))
                && !self.has_draft()
                && !recent(self.keyed.get(), TYPING)
                && !recent(self.sent, NOTICE_GAP),
        )
    }

    /// `takes_typing`, for what another agent hands it (`delegate`, `reply`): only once a snapshot
    /// found its agent. One Templum typed there may never have started (its folder's trust
    /// declined), and before a snapshot found it, macOS can't tell its shell is in front.
    fn takes_handing(&self, uid: &str) -> Result<(), Reply> {
        if self.seen.agent.is_some() && self.seen.pid.is_none() {
            return Err(Reply::err(format!(
                "{uid} is still starting: try again in a few seconds."
            )));
        }
        self.takes_typing(uid)
    }

    /// The agent Templum started in it may not take typing yet: its screen hasn't shown it (its
    /// cover is still on), or no snapshot found it yet, though STARTS_WITHIN hasn't passed since
    /// its command was typed (after that, it never started). A task handed to it waits, as a
    /// spawn's prompt does (docs/specs/connections.md, Delegating 5).
    fn agent_starting(&self) -> bool {
        let young = self
            .typed_agent
            .is_some_and(|at| at.elapsed() < super::spawn::STARTS_WITHIN);
        self.agent().is_some() && (self.starting.is_some() || self.seen.pid.is_none() && young)
    }
}

impl Workspace {
    /// The block `id` of `space` as an agent reads who wrote to it: `Claude Code (b7k2m)`, its
    /// title as `about` gives it.
    pub(super) fn signature(&self, space: &Space, id: BlockId, cx: &App) -> String {
        let (_, title, _) = self.about(&space.blocks[&id], cx);
        let uid = space.uids.get(&id).cloned().unwrap_or_default();
        format!("{title} ({uid})")
    }

    /// `send_keys` from the terminal `from`: `keys` pressed in the terminal `block`, also into
    /// an agent waiting for its person (the wire is the person's consent that the agents on it
    /// answer each other's dialogs), never into one on another agent's task while it works.
    pub(super) fn send_keys(
        &mut self,
        from: &str,
        block: &str,
        keys: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = match target(space, me, block) {
            Ok(to) => to,
            Err(refused) => return Outcome::Now(refused),
        };
        if to == me {
            return Outcome::Now(Reply::err(
                "This block is yours: send_keys presses keys in another block.",
            ));
        }
        let by = self.signature(space, me, cx);
        self.keys_in(from, &by, (&space_id, to), block, keys, cx)
    }

    /// The body of `send_keys`, for any asker (a terminal's `TEMPLUM_TERMINAL`, or the Templum
    /// Agent's word): `keys` pressed as `by` in the terminal block `to` of `space`, never into an
    /// agent on another agent's task while it works.
    pub(super) fn keys_in(
        &mut self,
        asker: &str,
        by: &str,
        (space_id, to): (&str, BlockId),
        block: &str,
        keys: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let Some(pane) = self.space_named(space_id).and_then(|s| s.terminal(to)) else {
            return Outcome::Now(Reply::err(format!("{block} isn't a terminal.")));
        };
        if let Err(refused) = pane.read_only(block) {
            return Outcome::Now(refused);
        }
        if let Some(bad) = keys.iter().find(|k| named_key(k).is_none()) {
            return Outcome::Now(Reply::err(format!(
                "{bad:?} isn't a key Templum knows. Keys: {KEY_NAMES}."
            )));
        }
        let asks = matches!(pane.state, Some(AgentState::NeedsYou(_)));
        if pane.task(Instant::now()).is_some_and(|h| h.by != asker) && !asks {
            return Outcome::Now(Reply::err(format!(
                "{block} is on a task another agent handed it: only that agent presses keys in it \
                 while it works. wait_for_idle(\"{block}\") waits for its end."
            )));
        }
        let worker = pane.term.id().to_string();
        self.asker_wrote(&worker, asker);
        let said = match keys.len() {
            1 => format!("Pressed 1 key in {block}."),
            n => format!("Pressed {n} keys in {block}."),
        };
        Outcome::Later(self.press_keys(worker, by.to_string(), keys, said, cx))
    }

    /// `asker` wrote into the agent in `terminal` (keys, typed text, a reply): that is how it
    /// answers a dialog of an agent that went quiet on the task `asker` handed it, so the agent
    /// waits for the task again, and a screen that stays still after the writing tells `asker`
    /// once more (docs/specs/agent-protocol.md, 5). Anyone else's writing changes nothing.
    pub(super) fn asker_wrote(&mut self, terminal: &str, asker: &str) {
        if let Some(pane) = self.pane_mut(terminal)
            && let Some(handed) = pane.handed.as_mut().filter(|h| h.by == asker && h.quiet)
        {
            handed.quiet = false;
            pane.state = Some(AgentState::Working);
            pane.looks.heard();
        }
    }

    /// `interrupt` from the terminal `from`: Esc once in the agent `block` while it works or waits
    /// for its person. The task the caller handed it ends first, its waits hearing so at once,
    /// with the prompt a spawn still keeps for it: typed later, it would start work no one awaits.
    /// The caller isn't told of that end: it knows (docs/specs/connections.md, Delegating 12).
    pub(super) fn interrupt(&mut self, from: &str, block: &str, cx: &mut Context<Self>) -> Outcome {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = match target(space, me, block) {
            Ok(to) => to,
            Err(refused) => return Outcome::Now(refused),
        };
        if to == me {
            return Outcome::Now(Reply::err(
                "This block is yours: interrupt stops another agent.",
            ));
        }
        let by = self.signature(space, me, cx);
        self.interrupt_in(from, &by, (&space_id, to), block, cx)
    }

    /// The body of `interrupt`, for any asker: Esc once in the agent of block `to` of `space`,
    /// pressed as `by`; `asker`'s task there ends first.
    pub(super) fn interrupt_in(
        &mut self,
        asker: &str,
        by: &str,
        (space_id, to): (&str, BlockId),
        block: &str,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let now = Instant::now();
        let from = asker;
        let Some(pane) = self.space_named(space_id).and_then(|s| s.terminal(to)) else {
            return Outcome::Now(Reply::err(format!("{block} isn't a terminal.")));
        };
        if let Err(refused) = pane.read_only(block) {
            return Outcome::Now(refused);
        }
        if pane.seen.agent.is_none() || pane.shell_in_front() {
            return Outcome::Now(Reply::err(format!(
                "{block} runs no agent: send_keys presses keys in a terminal."
            )));
        }
        // Past its timeout too, as `wait_for_idle` waits for it.
        let mine = pane.handed.as_ref().is_some_and(|h| h.by == from);
        if !mine && pane.task(now).is_some() {
            return Outcome::Now(Reply::err(format!(
                "{block} is on a task another agent handed it: only that agent interrupts it."
            )));
        }
        if !mine && !pane.busy(now) {
            return Outcome::Now(Reply::err(format!(
                "{block} is idle: there's nothing to interrupt."
            )));
        }
        let worker = pane.term.id().to_string();
        let by = by.to_string();
        let said = if mine {
            if let Some(pane) = self.pane_mut(&worker) {
                pane.handed = None;
                pane.prompt = None;
            }
            self.end_turn(&worker, Reply::err("Interrupted by you."), cx);
            format!("Pressed Esc in {block}, and ended the task you handed it.")
        } else {
            format!("Pressed Esc in {block}.")
        };
        Outcome::Later(self.press_keys(worker, by, vec!["escape".into()], said, cx))
    }

    /// `stop_agent` from the terminal `from`: ends what runs in the terminal `block`, which the
    /// caller opened, and what it left running (docs/specs/agent-tools.md#stop_agent).
    pub(super) fn stop_agent(&mut self, from: &str, block: &str, cx: &mut Context<Self>) -> Reply {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = match target(space, me, block) {
            Ok(to) => to,
            Err(refused) => return refused,
        };
        if to == me {
            return Reply::err("This block is yours: stop_agent stops another block.");
        }
        let Some(pane) = space.terminal(to) else {
            return Reply::err(format!("{block} isn't a terminal."));
        };
        if pane.opened_by.is_none() || pane.opened_by.as_ref() != space.uids.get(&me) {
            return Reply::err(format!(
                "Only the person stops {block}: you didn't open it. interrupt presses Esc in it."
            ));
        }
        self.stop_agent_in(from, (&space_id, to), block, cx)
    }

    /// The body of `stop_agent`, for any asker (a terminal's `TEMPLUM_TERMINAL`, or the Templum
    /// Agent's word): ends what runs in the terminal block `to` of `space`. The task `asker` handed
    /// it ends, its waits hearing so at once, with the prompt a spawn still keeps for it; the asker
    /// isn't told of that end: it knows. Another agent's task there ends as when its agent leaves,
    /// once a snapshot no longer finds it.
    pub(super) fn stop_agent_in(
        &mut self,
        asker: &str,
        (space_id, to): (&str, BlockId),
        block: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        let from = asker;
        let Some(pane) = self.space_named(space_id).and_then(|s| s.terminal(to)) else {
            return Reply::err(format!("{block} isn't a terminal."));
        };
        if let Err(refused) = pane.read_only(block) {
            return refused;
        }
        let worker = pane.term.id().to_string();
        let stopped = match self.stop_in(space_id, to, cx) {
            Ok(stopped) => stopped,
            Err(refused) => return Reply::err(refused),
        };
        if let Some(pane) = self.pane_mut(&worker) {
            pane.prompt = None;
            if pane.handed.as_ref().is_some_and(|h| h.by == from) {
                pane.handed = None;
                self.end_turn(&worker, Reply::err("Stopped by you."), cx);
            }
        }
        let count = |n| match n {
            1 => "1 process".to_string(),
            n => format!("{n} processes"),
        };
        let ended = match (stopped.program.as_str(), stopped.processes) {
            ("", n) => format!("{} it left running", count(n)),
            (program, 0) => format!("{program} and nothing else"),
            (program, n) => format!("{program} and {} it started", count(n)),
        };
        Reply::ok(format!(
            "Stopped {block}: ended {ended}. Its shell is at its prompt."
        ))
    }

    /// Presses `keys` in the terminal `worker`, KEY_GAP apart, as keys `by` pressed (the person's,
    /// for the screen's looks); then `said` and its screen, KEYS_SETTLE after the last. The
    /// terminal is found again before each key: one closed meanwhile ends the call there.
    pub(super) fn press_keys(
        &self,
        worker: String,
        by: String,
        keys: Vec<String>,
        said: String,
        cx: &mut Context<Self>,
    ) -> Task<Reply> {
        cx.spawn(async move |ws, cx| {
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    cx.background_executor().timer(KEY_GAP).await;
                }
                let pressed = ws.update(cx, |ws, cx| {
                    let (Some(pane), Some(key)) = (ws.pane_mut(&worker), named_key(key)) else {
                        return false;
                    };
                    // The person's keys, for the screen's looks: a dialog they answer ends.
                    pane.typed.set(true);
                    pane.keys_from = Some((by.clone(), Instant::now()));
                    pane.term.press(&key);
                    if i + 1 == keys.len() {
                        // "answered by" goes with no other frame to take it.
                        cx.spawn(async move |ws, cx| {
                            cx.background_executor().timer(ANSWERED_FOR).await;
                            let _ = ws.update(cx, |_, cx| cx.notify());
                        })
                        .detach();
                    }
                    cx.notify();
                    true
                });
                if !pressed.unwrap_or(false) {
                    return Reply::err("Its block was closed before every key was pressed.");
                }
            }
            cx.background_executor().timer(KEYS_SETTLE).await;
            let lines = ws
                .update(cx, |ws, _| ws.pane(&worker).map(Pane::tail))
                .ok()
                .flatten()
                .unwrap_or_default();
            Reply::ok(format!(
                "{said} Its screen now:\n{}",
                tools::tail(&lines, tools::MAX_TEXT)
            ))
        })
    }

    /// Types `text` into the agent in terminal `terminal`, its first line and the rest as two
    /// pastes, then Enter once its screen shows the paste and stood still a look (`EnterWait`), if
    /// Templum still may type into it. A screen that doesn't (a dialog that ignores pastes, as
    /// Codex's approval does, which Templum hears nothing of), or an agent that started asking its
    /// person meanwhile (a working agent's screen changes all along), gets no Enter: it would pick
    /// the dialog's answer for the person, as would one its person typed into since. Then its
    /// looks tell whether it took it (`EnterCheck`), with a second Enter when it can't answer a
    /// dialog; an agent at work when it was typed queues it. The `task` of that id (not a reply)
    /// not taken ends at once; once it ended, or another took its place, its looks stop, pressing
    /// nothing more. The answer says how it went, once known: Ok, or why not, the task then ended
    /// without a notice besides (`not_taken`). A notice waits for its Enters (`Pane::sent`).
    pub(super) fn send_task(
        &mut self,
        terminal: &str,
        text: &str,
        task: Option<u64>,
        cx: &mut Context<Self>,
    ) -> oneshot::Receiver<Result<(), Reply>> {
        let (told, answer) = oneshot::channel();
        let Some(pane) = self.pane_mut(terminal) else {
            return answer;
        };
        let typed_at = Instant::now();
        pane.sent = Some(typed_at);
        // At work, it queues the task: Codex says it took it only once a turn takes it in.
        let queued = pane.state == Some(AgentState::Working);
        let mut wait = EnterWait::new(pane.screen(), pane.term.grid_size());
        // Windows' console hands what it is typed to its program as keys, bracketed or not.
        let keys = cfg!(windows) || !pane.term.bracketed();
        // Claude Code shows a paste of 4 lines or more than 800 characters as "[Pasted text]",
        // which it takes for data unless typed text comes with it: the first line is that text.
        match text.find('\n') {
            Some(at) => {
                pane.term.type_in(&text[..at]);
                pane.term.type_in(&text[at..]);
            }
            None => pane.term.type_in(text),
        }
        let terminal = terminal.to_string();
        cx.spawn(async move |ws, cx| {
            let mut told = Some(told);
            // Once its Enter went.
            let mut check: Option<EnterCheck> = None;
            loop {
                cx.background_executor().timer(ENTER_AFTER).await;
                let done = ws.update(cx, |ws, cx| {
                    let Some(pane) = ws.pane(&terminal) else {
                        return true;
                    };
                    // Its task ended (interrupted, answered), or another came after it.
                    if task.is_some() && pane.handed.as_ref().map(|h| h.id) != task {
                        return true;
                    }
                    let takes = pane.takes_typing("").is_ok();
                    let screen = pane.screen();
                    let took = match check.as_mut() {
                        None => {
                            let size = pane.term.grid_size();
                            let typed = pane.keyed.get().is_some_and(|at| at > typed_at);
                            match wait.look(screen.clone(), size, (takes, pane.speaks, typed)) {
                                Step::Wait => return false,
                                Step::GiveUp => Err(NotTaken::Unseen),
                                Step::Typing => Err(NotTaken::Typing),
                                Step::Enter if queued => {
                                    pane.term.write(&b"\r"[..]);
                                    Ok(())
                                }
                                Step::Enter => {
                                    pane.term.write(&b"\r"[..]);
                                    let (speaks, submitted) = (pane.speaks, pane.submitted);
                                    check = Some(EnterCheck::new(speaks, submitted, screen, keys));
                                    return false;
                                }
                            }
                        }
                        Some(check) => {
                            let typing = pane.keyed.get().is_some_and(|at| at.elapsed() < TYPING);
                            match check.look(screen, pane.submitted, takes, typing) {
                                Check::Wait => return false,
                                Check::Again => {
                                    pane.term.write(&b"\r"[..]);
                                    check.again();
                                    return false;
                                }
                                Check::Taken => Ok(()),
                                Check::Not(why) => Err(why),
                            }
                        }
                    };
                    match took {
                        Ok(()) => {
                            if let Some(told) = told.take() {
                                let _ = told.send(Ok(()));
                            }
                        }
                        Err(why) if task.is_some() => ws.not_taken(&terminal, why, told.take(), cx),
                        Err(_) => {}
                    }
                    true
                });
                if done.unwrap_or(true) {
                    return;
                }
            }
        })
        .detach();
        answer
    }

    /// The block id of the terminal `terminal`, as agents name it; empty when it is gone.
    pub(super) fn uid_of(&self, terminal: &str) -> String {
        self.caller(terminal)
            .and_then(|(space, id)| self.space_named(&space)?.uids.get(&id).cloned())
            .unwrap_or_default()
    }

    /// The `signature` of the block of the terminal `terminal`; empty when it is gone.
    pub(super) fn signature_of(&self, terminal: &str, cx: &App) -> String {
        self.caller(terminal)
            .and_then(|(space, id)| Some(self.signature(self.space_named(&space)?, id, cx)))
            .unwrap_or_default()
    }

    /// The agent in the terminal `terminal` didn't take the task typed into it (`why`): the task,
    /// if it is still handed over, ends, and the calls waiting on it hear so, with its screen. The
    /// call that handed it over and still waits to say how that went (`told`) says so instead of
    /// a notice.
    fn not_taken(
        &mut self,
        terminal: &str,
        why: NotTaken,
        told: Option<oneshot::Sender<Result<(), Reply>>>,
        cx: &mut Context<Self>,
    ) {
        let Some(tail) = self
            .pane(terminal)
            .filter(|p| p.handed.is_some())
            .map(Pane::tail)
        else {
            return;
        };
        let uid = self.uid_of(terminal);
        let reply = Reply::err(format!(
            "{uid} didn't take the task: {}. {} The last lines of its screen:\n{}",
            why.why(),
            why.then(),
            tools::tail(&tail, tools::MAX_TEXT)
        ));
        match told.filter(|t| !t.is_canceled()) {
            Some(told) => {
                if let Some(pane) = self.pane_mut(terminal) {
                    pane.handed = None;
                }
                self.end_turn(terminal, reply.clone(), cx);
                let _ = told.send(Err(reply));
            }
            None => self.end_turn(terminal, reply, cx),
        }
    }

    /// `delegate` from the terminal `from`: `task` typed into the agent `block` under a line
    /// saying who hands it over; with `wait`, the call waits for its answer. Into an agent on the
    /// caller's own task (also past its deadline), it is a change to that task, typed at once: the
    /// end of the turn that takes it answers the task, whose timeout starts again
    /// (docs/specs/connections.md, Delegating 11). `scheduled`: the schedule that hands it, which
    /// its line names.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn delegate(
        &mut self,
        from: &str,
        block: &str,
        task: &str,
        wait: bool,
        timeout: u64,
        scheduled: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = match target(space, me, block) {
            Ok(to) => to,
            Err(refused) => return Outcome::Now(refused),
        };
        if to == me {
            return Outcome::Now(Reply::err(
                "This block is yours: hand the task to another agent.",
            ));
        }
        let by = self.signature(space, me, cx);
        let at = (space_id.as_str(), to);
        self.hand_task(from, &by, at, block, task, wait, timeout, scheduled, cx)
    }

    /// The body of `delegate`, for any asker: the task typed into the agent of block `to` of
    /// `space` under a line signed `by`, handed over by `asker` (a terminal's `TEMPLUM_TERMINAL`,
    /// or the Templum Agent's word), who hears its end; `scheduled` as `delegate` takes it.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn hand_task(
        &mut self,
        asker: &str,
        by: &str,
        (space_id, to): (&str, BlockId),
        block: &str,
        task: &str,
        wait: bool,
        timeout: u64,
        scheduled: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let now = Instant::now();
        let from = asker;
        let Some(pane) = self.space_named(space_id).and_then(|s| s.terminal(to)) else {
            return Outcome::Now(Reply::err(format!("{block} isn't a terminal.")));
        };
        let starting = pane.agent_starting();
        if !starting && let Err(refused) = pane.takes_handing(block) {
            return Outcome::Now(refused);
        }
        let mine = pane.handed.as_ref().filter(|h| h.by == from).map(|h| h.id);
        if mine.is_none() && pane.task(now).is_some() {
            return Outcome::Now(Reply::err(format!(
                "{block} is already on a task another agent handed it: wait_for_idle(\"{block}\") \
                 waits for its end."
            )));
        }
        // A spawn's prompt still waits: a change typed now would come before it.
        if let Some(id) = mine
            && pane.prompt.is_some()
        {
            return Outcome::Now(Reply::err(format!(
                "{block} hasn't got task t{id} yet: it is starting. Change it once it has."
            )));
        }
        let worker = pane.term.id().to_string();
        let usage = pane.usage.clone();
        // A task for an agent without Templum's tools says why, and what else ends it. One
        // starting has none yet.
        let no_tools = (mine.is_none() && !starting && !self.has_tools(&worker)).then(|| {
            let shell = if self.replies_from_shell(pane) {
                "it can still answer with `templum reply -` in its shell; else "
            } else {
                ""
            };
            format!(
                " {block} has no Templum tools ({}): {shell}its turn's end or its screen going \
                 still ends the task.",
                self.no_tools_why(pane)
            )
        });
        let id = mine.unwrap_or_else(|| {
            self.next_task += 1;
            self.next_task
        });
        let line = match mine {
            Some(_) => format!(
                "[from {by}, a change to task t{id}] {CHANGE_LEAD}\n{task}{}",
                answer_how(id)
            ),
            None => {
                let from = scheduled.map_or(String::new(), |s| format!(", scheduled s{s}"));
                format!(
                    "[from {by}, task t{id}{from}] {TASK_LEAD}\n{task}{}",
                    answer_how(id)
                )
            }
        };
        // Starting, it gets the task once it is ready, as a spawn's prompt.
        let sent = if starting {
            let builtin = self
                .pane(&worker)
                .and_then(Pane::agent)
                .and_then(|id| self.agents.iter().find(|a| a.id == id))
                .and_then(|a| a.builtin);
            if let Some(pane) = self.pane_mut(&worker) {
                pane.prompt = Some(Prompt {
                    delegated: true,
                    ..Prompt::new(line, builtin)
                });
            }
            self.watch_ready(worker.clone(), cx);
            None
        } else {
            Some(self.send_task(&worker, &line, Some(id), cx))
        };
        let until = Instant::now() + Duration::from_secs(timeout);
        if let Some(pane) = self.pane_mut(&worker) {
            match (mine, pane.handed.as_mut()) {
                // The turn that takes the change answers it.
                (Some(_), Some(handed)) => {
                    handed.taken = false;
                    handed.behind = false;
                    handed.quiet = false;
                    handed.until = until;
                }
                _ => {
                    pane.handed = Some(Handed {
                        id,
                        usage,
                        until,
                        text: Handed::text_of(task),
                        ..Handed::new(from, Duration::from_secs(timeout))
                    })
                }
            }
        }
        cx.notify();
        if wait {
            // Its call's own deadline tells its timeout.
            let what = Waiting::Turn {
                of: worker,
                uid: block.to_string(),
            };
            return Outcome::Wait(Box::new(what), Some(timeout));
        }
        let said = match mine {
            Some(_) => format!("Typed into {block} as a change to task t{id}."),
            None => format!(
                "Handed to {block} as task t{id}.{}",
                no_tools.unwrap_or_default()
            ),
        };
        let Some(sent) = sent else {
            return Outcome::Now(Reply::ok(match mine {
                Some(_) => format!(
                    "{block} is starting: the change to task t{id} is typed once it is ready."
                ),
                None => format!(
                    "Handed to {block} as task t{id}. It is starting: the task is typed once it \
                     is ready."
                ),
            }));
        };
        // Answered once the agent took the task, or with why not.
        let gone = format!("{block} closed, or its task ended, before it took the task.");
        Outcome::Later(cx.spawn(async move |_, _| match sent.await {
            Ok(Ok(())) => Reply::ok(said),
            Ok(Err(why)) => why,
            Err(_) => Reply::err(gone),
        }))
    }

    /// `reply` from the terminal `from`: without a `block`, or to the agent that handed the caller
    /// its task, the caller's answer, which ends that task (`finish_task`). To another connected
    /// agent `block`, a message: its calls waiting on the caller (`delegate` with `wait`,
    /// `wait_for_idle`) get `text`; with none, the text is typed into it. No task ends then
    /// (docs/specs/agent-protocol.md, 2).
    pub(super) fn reply_to(
        &mut self,
        from: &str,
        block: Option<&str>,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        if text.trim().is_empty() {
            return Reply::err("reply needs your answer as text.");
        }
        let Some(block) = block else {
            return self.finish_task(from, text, cx);
        };
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = match target(space, me, block) {
            Ok(to) => to,
            Err(refused) => return refused,
        };
        if to == me {
            return Reply::err(
                "This block is yours: leave block out to answer the task you are on.",
            );
        }
        let Some(pane) = space.terminal(to) else {
            return Reply::err(format!("{block} isn't a terminal."));
        };
        let asker = pane.term.id().to_string();
        if self
            .pane(from)
            .and_then(|p| p.handed.as_ref())
            .is_some_and(|h| h.by == asker)
        {
            return self.finish_task(from, text, cx);
        }
        let waiting: Vec<u64> = self
            .waits
            .iter()
            .filter(|w| {
                w.from == asker && matches!(&w.what, Waiting::Turn { of, .. } if of == from)
            })
            .map(|w| w.key)
            .collect();
        if waiting.is_empty() {
            if let Err(refused) = pane.takes_handing(block) {
                return refused;
            }
            let reply = format!(
                "[reply from {}] Its reply is in the pasted text.\n{text}",
                self.signature(space, me, cx)
            );
            drop(self.send_task(&asker, &reply, None, cx));
            self.asker_wrote(&asker, from);
        }
        if let Some(mine) = self.pane_mut(from) {
            mine.answer = Some(text.to_string());
        }
        let answered = !waiting.is_empty();
        for key in waiting {
            self.answer(key, Reply::ok(text), cx);
        }
        cx.notify();
        Reply::ok(if answered {
            format!("Answered {block}.")
        } else {
            format!("Typed into {block}.")
        })
    }

    /// The agent in the terminal `terminal` answers the task it is on with `text`, whole: the task
    /// ends. The calls of the agent that handed it waiting for it (`delegate` with `wait`,
    /// `wait_for_idle`) get `text`; with none, that agent hears the end as any end is told
    /// (`tell_end`). Other agents' calls waiting on its turn stay for the turn's end
    /// (docs/specs/agent-protocol.md, 2).
    pub(super) fn finish_task(
        &mut self,
        terminal: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        let Some(handed) = self.pane_mut(terminal).and_then(|p| p.handed.take()) else {
            return Reply::err(
                "You are on no task: reply answers the task an agent handed you; name a block to \
                 write to one.",
            );
        };
        if let Some(pane) = self.pane_mut(terminal) {
            pane.answer = Some(text.to_string());
        }
        let reply = Reply::ok(text);
        let waiting: Vec<u64> = self
            .waits
            .iter()
            .filter(|w| {
                w.from == handed.by && matches!(&w.what, Waiting::Turn { of, .. } if of == terminal)
            })
            .map(|w| w.key)
            .collect();
        if waiting.is_empty() {
            self.tell_end(terminal, &handed, &reply, cx);
        }
        for key in waiting {
            self.answer(key, reply.clone(), cx);
        }
        cx.notify();
        Reply::ok(format!("Answered task t{}.", handed.id))
    }

    /// `wait_for_idle` from the terminal `from` on the agent `block`: its last answer now when it
    /// is idle, else once its turn, or the task it is on, ends. A task the caller handed it is
    /// waited for past its deadline while unanswered: Codex never says it works.
    pub(super) fn wait_for_idle(&self, from: &str, block: &str, timeout: u64) -> Outcome {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = match target(space, me, block) {
            Ok(to) => to,
            Err(refused) => return Outcome::Now(refused),
        };
        if to == me {
            return Outcome::Now(Reply::err("This block is yours: wait for another agent."));
        }
        self.idle_wait(from, (&space_id, to), block, timeout)
    }

    /// The body of `wait_for_idle`, for any asker: the turn of the agent in block `to` of
    /// `space`, waited for when it works or runs `asker`'s task.
    pub(super) fn idle_wait(
        &self,
        asker: &str,
        (space_id, to): (&str, BlockId),
        block: &str,
        timeout: u64,
    ) -> Outcome {
        let from = asker;
        let Some(pane) = self.space_named(space_id).and_then(|s| s.terminal(to)) else {
            return Outcome::Now(Reply::err(format!("{block} isn't a terminal.")));
        };
        if pane.seen.agent.is_none() {
            return Outcome::Now(Reply::err(format!(
                "{block} runs no agent: read_terminal reads its screen."
            )));
        }
        let worker = pane.term.id();
        let mine = pane.handed.as_ref().is_some_and(|h| h.by == from);
        // Another agent waits for its turn, not its task: a turn that ended while the task stays
        // open (it waits on a task of its own, or went quiet) answers it now
        // (docs/specs/agent-protocol.md, 4 and 5). Its last answer is its turn's only where its
        // hooks refresh it at each turn's end: an agent without them may hold one of an earlier task.
        let quiet = pane.handed.as_ref().is_some_and(|h| h.quiet);
        let rests = !mine
            && pane.state == Some(AgentState::Idle)
            && (quiet || self.waits_on_its_own(worker));
        if rests {
            return Outcome::Now(match &pane.answer {
                Some(answer) if !quiet && pane.speaks => Reply::ok(answer),
                _ => self.still_screen(worker),
            });
        }
        if mine || pane.busy(Instant::now()) {
            let what = Waiting::Turn {
                of: worker.to_string(),
                uid: block.to_string(),
            };
            return Outcome::Wait(Box::new(what), Some(timeout));
        }
        Outcome::Now(Reply::ok(pane.answer.clone().unwrap_or_else(|| {
            format!("{block} is idle; Templum has heard no answer from it.")
        })))
    }

    /// The agent in the terminal `terminal` is gone: its session ended, a snapshot no longer
    /// finds its process (it crashed, or Codex quit, which says nothing), or one spawned with a
    /// prompt no snapshot found within `STARTS_WITHIN` never started. Its transcript goes with it,
    /// once its task's end named it: an agent without hooks started next there names none.
    pub(super) fn agent_left(&mut self, terminal: &str, cx: &mut Context<Self>) {
        self.end_turn(
            terminal,
            Reply::err("Its agent ended before it answered."),
            cx,
        );
        if let Some(pane) = self.pane_mut(terminal) {
            pane.transcript = None;
            pane.keep_subagents(|_| false);
            pane.awaits_subagents = false;
            pane.settles = None;
        }
    }

    /// Some pane, in any workspace, holds a task the agent in `terminal` handed over (`delegate`,
    /// or a spawn's prompt) since `since`, when its own task was handed to it, also past the
    /// timeout of that one: the agent waits for it. One handed before its own task isn't what
    /// this task waits for, nor one whose agent went quiet: the agent was told, and reads its
    /// screen (docs/specs/agent-protocol.md, 4).
    pub(super) fn hands_out(&self, terminal: &str, since: Instant) -> bool {
        self.spaces().any(|space| {
            space.terminals().any(|(_, pane)| {
                pane.handed
                    .as_ref()
                    .is_some_and(|h| h.by == terminal && h.since >= since && !h.quiet)
            })
        })
    }

    /// The agent in `terminal` is on a task and waits for one it handed out for it.
    pub(super) fn waits_on_its_own(&self, terminal: &str) -> bool {
        self.pane(terminal)
            .and_then(|p| p.handed.as_ref())
            .is_some_and(|h| self.hands_out(terminal, h.since))
    }

    /// The agent in the terminal `terminal` ended its turn, or its session: the task it was on is
    /// no longer handed over, and the calls waiting on it get `reply`; the agent that handed the
    /// task over hears so, typed, when no call of its waits for it (docs/specs/connections.md,
    /// Delegating 10).
    pub(super) fn end_turn(&mut self, terminal: &str, reply: Reply, cx: &mut Context<Self>) {
        let handed = self.pane_mut(terminal).and_then(|p| p.handed.take());
        let waiting: Vec<u64> = self
            .waits
            .iter()
            .filter(|w| matches!(&w.what, Waiting::Turn { of, .. } if of == terminal))
            .map(|w| w.key)
            .collect();
        if let Some(handed) = handed.filter(|h| !self.waits_on(&h.by, terminal)) {
            self.tell_end(terminal, &handed, &reply, cx);
        }
        for key in waiting {
            self.answer(key, reply.clone(), cx);
        }
    }

    /// What the calls waiting on the turn of the agent in `terminal` get when its screen has been
    /// still: it may be done, but nothing says so.
    pub(super) fn still_screen(&self, terminal: &str) -> Reply {
        let lines = self
            .pane(terminal)
            .map(|pane| pane.term.shared().lines(LAST_LINES).join("\n"))
            .unwrap_or_default();
        Reply::ok(format!(
            "Its screen has been still for {STILL} seconds, and Templum heard no answer: it may \
             be done, stopped, or asking its person. The last lines of its screen:\n{}",
            tools::tail(&lines, tools::MAX_TEXT)
        ))
    }

    /// The turn of the agent in `terminal`, which is on a task, ended or went still while the task
    /// stays open: the calls of agents other than the task's asker that wait for it get `reply`.
    /// For them the turn ended; the asker's own calls wait for the task's end
    /// (docs/specs/agent-protocol.md, 4 and 5).
    pub(super) fn answer_others(&mut self, terminal: &str, reply: Reply, cx: &mut Context<Self>) {
        let Some(by) = self
            .pane(terminal)
            .and_then(|p| p.handed.as_ref())
            .map(|h| h.by.clone())
        else {
            return;
        };
        let others: Vec<u64> = self
            .waits
            .iter()
            .filter(|w| w.from != by)
            .filter(|w| matches!(&w.what, Waiting::Turn { of, .. } if of == terminal))
            .map(|w| w.key)
            .collect();
        for key in others {
            self.answer(key, reply.clone(), cx);
        }
    }

    /// The screen of the agent in `terminal`, which has Templum's tools and no hooks, went still
    /// while it is on a task: it may be done without replying, stopped, or asking its person. The
    /// task stays open (its `reply` still ends it) and its master is told once: a call of its
    /// waiting for the task gets an error, else a notice is typed as a dialog's is. Other agents'
    /// calls waiting for its turn get `still_screen`: for them its turn ended
    /// (docs/specs/agent-protocol.md, 5).
    pub(super) fn went_quiet(&mut self, terminal: &str, cx: &mut Context<Self>) {
        let Some(handed) = self.pane(terminal).and_then(|p| p.handed.as_ref()) else {
            return;
        };
        let (id, by, told) = (handed.id, handed.by.clone(), handed.quiet);
        let reply = self.still_screen(terminal);
        self.answer_others(terminal, reply, cx);
        // What still waits for its turn is its asker's.
        let asked: Vec<u64> = self
            .waits
            .iter()
            .filter(|w| matches!(&w.what, Waiting::Turn { of, .. } if of == terminal))
            .map(|w| w.key)
            .collect();
        if told {
            return;
        }
        if let Some(handed) = self.pane_mut(terminal).and_then(|p| p.handed.as_mut()) {
            handed.quiet = true;
        }
        if asked.is_empty() {
            let text = format!(
                "[task t{id} from {} hasn't answered: its screen is still] {SCREEN_LEAD}\n{}",
                self.signature_of(terminal, cx),
                self.pane(terminal).map(Pane::tail).unwrap_or_default()
            );
            self.notice_to(by, text, terminal, NoticeLevel::Waiting, Some(id), cx);
        }
        for key in asked {
            let lines = self
                .pane(terminal)
                .map(|pane| pane.term.shared().lines(LAST_LINES).join("\n"))
                .unwrap_or_default();
            let reply = Reply::err(format!(
                "{} hasn't answered task t{id}: its screen has been still for {STILL} seconds. It \
                 may be done without replying, stopped, or asking its person. The task stays open, \
                 and its reply still ends it: wait_for_idle waits for it, send_keys answers a \
                 dialog, delegate to it again asks it for its answer, and interrupt ends it. The \
                 last lines of its screen:\n{}",
                self.uid_of(terminal),
                tools::tail(&lines, tools::MAX_TEXT)
            ));
            self.answer(key, reply, cx);
        }
    }

    /// The task `handed` to the agent in `terminal` ended with `reply`: its asker hears so,
    /// SPENT_AFTER later, with what it spent then (docs/specs/connections.md, Delegating 10). Its
    /// time and transcript are the end's.
    pub(super) fn tell_end(
        &mut self,
        terminal: &str,
        handed: &Handed,
        reply: &Reply,
        cx: &mut Context<Self>,
    ) {
        let pane = self.pane(terminal);
        let (word, lead, level) = if reply.is_error {
            ("failed", FAILED_LEAD, NoticeLevel::Failed)
        } else {
            ("done", DONE_LEAD, NoticeLevel::Done)
        };
        let (id, since) = (handed.id, handed.since.elapsed().as_secs());
        let head = format!(
            "[task t{} from {}: {word} in {since} s",
            handed.id,
            self.signature_of(terminal, cx)
        );
        // On one line: a path can't break it, nor end the paste.
        let transcript = pane
            .and_then(|p| p.transcript.as_deref())
            .map(|t| format!(", transcript {}", flat(t)))
            .unwrap_or_default();
        let proc = pane.and_then(|p| self.proc_of(p));
        let (data, from, to, body) = (
            self.data_dir.clone(),
            handed.usage.clone(),
            handed.by.clone(),
            reply.text.clone(),
        );
        let terminal = terminal.to_string();
        // Still running, to the agent that handed it, until its notice is queued.
        *self.ending.entry(to.clone()).or_default() += 1;
        cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(SPENT_AFTER).await;
            let read = match (data, proc) {
                (Some(data), Some(proc)) => {
                    let dirs = crate::agents::dirs(&data);
                    cx.background_executor()
                        .spawn(async move {
                            templum_usage::read(&[proc], &dirs, &[], crate::notes::now())
                                .0
                                .pop()
                                .flatten()
                        })
                        .await
                }
                _ => None,
            };
            let _ = ws.update(cx, |ws, cx| {
                ws.ended(&to);
                // The next task handed before a snapshot reads it again starts from this read.
                if let (Some(read), Some(pane)) = (&read, ws.pane_mut(&terminal)) {
                    pane.usage = Some(read.clone());
                }
                // Its block may be gone (closed): what it spent then is unknown.
                let now = read.or_else(|| ws.pane(&terminal).and_then(|p| p.usage.clone()));
                let spent = spent(from.as_ref(), now.as_ref())
                    .map(|s| format!(", {s}"))
                    .unwrap_or_default();
                let text = format!("{head}{spent}{transcript}] {lead}\n{body}");
                ws.notice_to(to, text, &terminal, level, Some(id), cx);
            });
        })
        .detach();
    }

    /// The end of a task `asker` handed is queued, or lost: it is no longer on its way.
    fn ended(&mut self, asker: &str) {
        if let Some(n) = self.ending.get_mut(asker) {
            *n -= 1;
            if *n == 0 {
                self.ending.remove(asker);
            }
        }
    }

    /// The agent in `terminal` started waiting for its person, asking `what`, its `screen` as
    /// drawn: the agent that handed it its task hears so, unless a call of its waits for the
    /// task's end (docs/specs/connections.md, Delegating 10).
    pub(super) fn tell_waiting(
        &mut self,
        terminal: &str,
        what: Option<&str>,
        screen: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(handed) = self.pane(terminal).and_then(|p| p.handed.as_ref()) else {
            return;
        };
        if self.waits_on(&handed.by, terminal) {
            return;
        }
        let (id, by) = (handed.id, handed.by.clone());
        let asks = what
            .map(clean_title)
            .filter(|w| !w.is_empty())
            .map(|w| format!(": {w}"))
            .unwrap_or_default();
        let text = format!(
            "[task t{id} from {} waits for its person{asks}] {SCREEN_LEAD}\n{screen}",
            self.signature_of(terminal, cx)
        );
        self.notice_to(by, text, terminal, NoticeLevel::Waiting, Some(id), cx);
    }

    /// How many notices wait in the queue for the message its person is writing to the agent in
    /// `terminal`; none without a draft (its chip, `block.rs`).
    pub(crate) fn answers_waiting(&self, terminal: &str) -> usize {
        if !self.pane(terminal).is_some_and(Pane::has_draft) {
            return 0;
        }
        self.notices.iter().filter(|n| n.to == terminal).count()
    }

    /// Queues the notice `text` for the agent in the terminal `to`, then types what may go now.
    pub(super) fn queue_notice(
        &mut self,
        to: String,
        text: String,
        kind: NoticeKind,
        cx: &mut Context<Self>,
    ) {
        self.push_notice(to, text, kind, None, cx);
    }

    /// `queue_notice`, for a notice `about` a task: its worker's terminal and its id.
    pub(super) fn push_notice(
        &mut self,
        to: String,
        text: String,
        kind: NoticeKind,
        about: Option<(String, u64)>,
        cx: &mut Context<Self>,
    ) {
        let at = cx.background_executor().now();
        self.notices.push(Notice {
            to,
            text,
            kind,
            about,
            at,
        });
        // Its chip, if a draft holds it (`answers_waiting`).
        cx.notify();
        self.deliver_notices(cx);
    }

    /// A notice that a task waits for its person, or went quiet, tells nothing once its worker
    /// doesn't any more: its person answered the dialog, or the task ended or was replaced while
    /// the notice waited for the others. The asker would act on a screen that is gone.
    fn stale(&self, notice: &Notice) -> bool {
        let NoticeKind::Task(NoticeLevel::Waiting) = notice.kind else {
            return false;
        };
        let Some((worker, id)) = &notice.about else {
            return false;
        };
        !self
            .pane(worker)
            .and_then(|pane| Some((pane, pane.handed.as_ref().filter(|h| h.id == *id)?)))
            .is_some_and(|(pane, handed)| {
                handed.quiet || matches!(pane.state, Some(AgentState::NeedsYou(_)))
            })
    }

    /// Some task the agent in the terminal `asker` handed is still running: awaited and not stuck
    /// (waiting for its person, or gone quiet: its notice is on its way or was told), or its end is
    /// on its way (`tell_end`).
    pub(super) fn still_running(&self, asker: &str) -> bool {
        let now = Instant::now();
        self.ending.contains_key(asker)
            || self.spaces().any(|space| {
                space.terminals().any(|(_, pane)| {
                    pane.task(now).is_some_and(|h| h.by == asker && !h.quiet)
                        && !matches!(pane.state, Some(AgentState::NeedsYou(_)))
                })
            })
    }

    /// The notices about tasks waiting for the agent in the terminal `to` go now: the newest came
    /// BATCH_WAIT ago, or the oldest BATCH_MOST ago, or no other task of its agent is running.
    fn batch_due(&self, to: &str, now: Instant) -> bool {
        let tasks: Vec<&Notice> = self
            .notices
            .iter()
            .filter(|n| n.to == to && matches!(n.kind, NoticeKind::Task(_)))
            .collect();
        let (Some(oldest), Some(newest)) = (tasks.first(), tasks.last()) else {
            return false;
        };
        now.saturating_duration_since(newest.at) >= BATCH_WAIT
            || now.saturating_duration_since(oldest.at) >= BATCH_MOST
            || !self.still_running(to)
    }

    /// Types the notices whose agents take them now; drops those whose terminal runs no agent any
    /// more (docs/specs/connections.md, Delegating 10). The notices about tasks go together, as
    /// one paste, once `batch_due`; the others one at a time, oldest first. The Templum Agent's are
    /// told to its engine, in the same batches.
    pub(super) fn deliver_notices(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let before = self.notices.len();
        let mut freed = false;
        let stale: Vec<bool> = self.notices.iter().map(|n| self.stale(n)).collect();
        let mut stale = stale.into_iter();
        self.notices.retain(|_| !stale.next().unwrap_or(false));
        let mut i = 0;
        while i < self.notices.len() {
            let to = self.notices[i].to.clone();
            let task = matches!(self.notices[i].kind, NoticeKind::Task(_));
            // The Templum Agent's engine takes a message at any time, and no person writes to it.
            let engine = to == templum_agent::ASKER;
            let takes = if engine {
                Some(true)
            } else {
                freed |= self.pane(&to).is_some_and(Pane::settle_draft);
                self.pane(&to).and_then(Pane::takes_notice)
            };
            match takes {
                None => {
                    self.notices.remove(i);
                }
                Some(false) => i += 1,
                Some(true) if task && !self.batch_due(&to, now) => i += 1,
                Some(true) => {
                    let batch = if task {
                        let (batch, rest) = std::mem::take(&mut self.notices)
                            .into_iter()
                            .partition(|n| n.to == to && matches!(n.kind, NoticeKind::Task(_)));
                        self.notices = rest;
                        batch
                    } else {
                        vec![self.notices.remove(i)]
                    };
                    let text = batch_text(&batch);
                    if engine {
                        self.tell_engine(&text, cx);
                    } else {
                        // Its next notice waits, also in this pass: two never share an Enter.
                        drop(self.send_task(&to, &text, None, cx));
                    }
                }
            }
        }
        // The chips of those that wait (`answers_waiting`) follow the person's drafts.
        if freed || self.notices.len() != before {
            cx.notify();
        }
    }
}

/// What is typed for `batch`, the notices of one agent, oldest first: one as it is; several as one
/// text, a title line (what they are, and where their words are) and each of them, a blank line
/// between. A notice that isn't an end (`Waiting`: a dialog, or a screen gone still) "needs a
/// look".
fn batch_text(batch: &[Notice]) -> String {
    if let [only] = batch {
        return only.text.clone();
    }
    let ended = batch
        .iter()
        .filter(|n| !matches!(n.kind, NoticeKind::Task(NoticeLevel::Waiting)))
        .count();
    let waiting = batch.len() - ended;
    let title = match (ended, waiting) {
        (n, 0) => format!("{n} tasks ended"),
        (0, n) => format!("{n} tasks need a look"),
        (a, b) => {
            let ended = if a == 1 {
                "1 task ended".into()
            } else {
                format!("{a} tasks ended")
            };
            let waits = if b == 1 {
                "1 needs a look".into()
            } else {
                format!("{b} need a look")
            };
            format!("{} notices: {ended}, {waits}", batch.len())
        }
    };
    let texts: Vec<&str> = batch.iter().map(|n| n.text.as_str()).collect();
    format!("[{title}] {BATCH_LEAD}\n\n{}", texts.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u16, u16) = (80, 24);
    const BIG: (u16, u16) = (100, 30);

    fn wait() -> EnterWait {
        EnterWait::new("› ".into(), SIZE)
    }

    /// The bare prompt of each agent family is an empty input; text after it, or a screen with no
    /// prompt Templum knows (a dialog), is not.
    #[test]
    fn an_input_is_empty_when_its_prompt_has_nothing_after_it() {
        // Claude Code, in a box (older) and with rules (newer).
        let boxed = "╭────────────────╮\n│ >              │\n╰────────────────╯\n  ? for shortcuts";
        let ruled = "● Done.\n\n────────────────\n❯\n────────────────\n  ? for shortcuts";
        assert!(input_empty(boxed) && input_empty(ruled));
        assert!(!input_empty(&boxed.replace("│ >  ", "│ > hi")));
        assert!(!input_empty(&ruled.replace('❯', "❯ fix the")));
        // Codex.
        assert!(input_empty(
            "• Ran cargo test\n\n›\n\n  ⏎ send   ⌃J newline"
        ));
        assert!(!input_empty(
            "• Ran cargo test\n\n› fix the tests\n\n  ⏎ send"
        ));
        // A plain agent: an earlier prompt with text above the bare one is history.
        assert!(input_empty("> run it\nran it\n> "));
        assert!(!input_empty("ran it\n> run it"));
        // A dialog's choice is not an input; nothing known is no answer.
        assert!(!input_empty("Allow Edit?\n❯ 1. Yes\n  2. No"));
        assert!(!input_empty("compiling…"));
        assert!(!input_empty(""));
        // Claude Code's shell mode has no marker: a bare one far up is output, not its input.
        let shell = "❯\nran it\n1\n2\n3\n4\n5\n6\n! git sta";
        assert!(!input_empty(shell));
    }

    /// A batch of one is the notice as it is; several have a title that says what they are.
    #[test]
    fn a_batch_has_a_title_that_names_what_it_holds() {
        let notice = |level, text: &str| Notice {
            to: "t".into(),
            text: text.into(),
            kind: NoticeKind::Task(level),
            about: None,
            at: Instant::now(),
        };
        let (done, failed, waits) = (NoticeLevel::Done, NoticeLevel::Failed, NoticeLevel::Waiting);
        let title = |levels: &[NoticeLevel]| {
            let batch: Vec<Notice> = levels.iter().map(|l| notice(*l, "x")).collect();
            batch_text(&batch).lines().next().unwrap().to_string()
        };
        assert_eq!(
            batch_text(&[notice(done, "[task t1] one\nbody")]),
            "[task t1] one\nbody"
        );
        assert_eq!(
            title(&[done, failed]),
            "[2 tasks ended] They are in the pasted text."
        );
        assert_eq!(
            title(&[waits, waits, waits]),
            "[3 tasks need a look] They are in the pasted text."
        );
        assert_eq!(
            title(&[done, done, waits]),
            "[3 notices: 2 tasks ended, 1 needs a look] They are in the pasted text."
        );
        assert_eq!(
            title(&[done, waits, waits]),
            "[3 notices: 1 task ended, 2 need a look] They are in the pasted text."
        );
        // Each notice as it is, a blank line between, oldest first.
        let batch = [
            notice(done, "[task t1] a\nA"),
            notice(failed, "[task t2] b\nB"),
        ];
        assert_eq!(
            batch_text(&batch),
            "[2 tasks ended] They are in the pasted text.\n\n[task t1] a\nA\n\n[task t2] b\nB"
        );
    }

    #[test]
    fn what_a_task_spent_is_its_cost_else_its_tokens() {
        use templum_usage::Session;
        let s = |cost_usd, tokens| Session {
            cost_usd,
            tokens,
            ..Default::default()
        };
        // Claude Code's status line: its cost; its token totals are its last call's context.
        let (claude, later) = (s(Some(0.10), Some(38069)), s(Some(0.52), Some(38279)));
        assert_eq!(spent(Some(&claude), Some(&later)).as_deref(), Some("$0.42"));
        // Codex: its tokens.
        let (codex, later) = (s(None, Some(1000)), s(None, Some(61231)));
        assert_eq!(
            spent(Some(&codex), Some(&later)).as_deref(),
            Some("60231 tokens")
        );
        // Nothing read at the hand-over (a session's first task): all of it.
        assert_eq!(spent(None, Some(&later)).as_deref(), Some("61231 tokens"));
        assert_eq!(
            spent(Some(&s(None, None)), Some(&s(Some(0.52), None))).as_deref(),
            Some("$0.52")
        );
        // Unknown at the end: nothing.
        assert_eq!(spent(Some(&codex), None), None);
        assert_eq!(spent(Some(&s(None, None)), Some(&s(None, None))), None);
        // Nothing new (a timeout before its agent wrote a figure), or less: nothing, never "0".
        assert_eq!(spent(Some(&codex), Some(&codex)), None);
        assert_eq!(spent(Some(&later), Some(&codex)), None);
        assert_eq!(spent(Some(&claude), Some(&claude)), None);
    }

    #[test]
    fn enter_goes_once_the_screen_shows_the_paste_and_stood_still() {
        let mut w = wait();
        assert_eq!(w.look("› ".into(), SIZE, (true, false, false)), Step::Wait);
        // Codex and Copilot CLI on Windows read the paste as keys: an Enter among them, or right
        // after, is a new line in it.
        assert_eq!(
            w.look("› fix the".into(), SIZE, (true, false, false)),
            Step::Wait
        );
        assert_eq!(
            w.look("› fix the tests".into(), SIZE, (true, false, false)),
            Step::Wait,
            "still arriving"
        );
        assert_eq!(
            w.look("› fix the tests".into(), SIZE, (true, false, false)),
            Step::Enter
        );
    }

    /// An agent at work (a spinner) never stands still: it gets its Enter once Templum stopped
    /// looking, as it queues the task.
    #[test]
    fn a_moving_screen_that_shows_the_paste_gets_its_enter_at_last() {
        let mut w = wait();
        let mut step = Step::Wait;
        let mut looks = 0;
        while step == Step::Wait {
            looks += 1;
            step = w.look(format!("› fix ⠋{looks}"), SIZE, (true, true, false));
        }
        assert_eq!((step, looks), (Step::Enter, ENTER_LOOKS));
    }

    #[test]
    fn no_enter_for_a_screen_that_never_shows_the_paste_nor_into_a_dialog() {
        let mut w = wait();
        for _ in 1..ENTER_LOOKS {
            assert_eq!(w.look("› ".into(), SIZE, (true, false, false)), Step::Wait);
        }
        assert_eq!(
            w.look("› ".into(), SIZE, (true, false, false)),
            Step::GiveUp
        );
        assert_eq!(
            wait().look("› fix".into(), SIZE, (false, true, false)),
            Step::GiveUp,
            "a dialog opened: Templum may not type into it"
        );
    }

    /// The person resizes the agent's block: it redraws at its new size, which is no sign that it
    /// read the paste. Its Enter waits until its screen settles.
    #[test]
    fn a_resize_isnt_the_paste() {
        let mut w = wait();
        assert_eq!(
            w.look("redrawn".into(), BIG, (true, true, false)),
            Step::Wait
        );
        assert_eq!(
            w.look("redrawn › fix".into(), BIG, (true, true, false)),
            Step::Wait
        );
        assert_eq!(
            w.look("redrawn › fix".into(), BIG, (true, true, false)),
            Step::Wait
        );
        assert_eq!(
            w.look("redrawn › fix".into(), BIG, (true, true, false)),
            Step::Enter,
            "settled: an agent whose dialogs are heard gets its Enter, and confirms it"
        );
        // One whose dialogs Templum doesn't hear (Codex's approval): the settled screen is the
        // new start, and only a change after it is the paste.
        let mut w = wait();
        for _ in 0..3 {
            assert_eq!(
                w.look("redrawn".into(), BIG, (true, false, false)),
                Step::Wait
            );
        }
        assert_eq!(
            w.look("redrawn".into(), BIG, (true, false, false)),
            Step::Wait
        );
        assert_eq!(
            w.look("redrawn › fix".into(), BIG, (true, false, false)),
            Step::Wait
        );
        assert_eq!(
            w.look("redrawn › fix".into(), BIG, (true, false, false)),
            Step::Enter
        );
        let mut w = wait();
        for _ in 0..3 {
            w.look("redrawn".into(), BIG, (true, false, false));
        }
        for _ in 1..ENTER_LOOKS {
            assert_eq!(
                w.look("redrawn".into(), BIG, (true, false, false)),
                Step::Wait
            );
        }
        assert_eq!(
            w.look("redrawn".into(), BIG, (true, false, false)),
            Step::GiveUp,
            "the paste may have been drawn with the redraw: no Enter it can't tell is safe"
        );
    }

    #[test]
    fn a_screen_that_keeps_moving_after_a_resize() {
        let moving = |w: &mut EnterWait, speaks: bool| {
            let mut step = Step::Wait;
            for n in 0..=ENTER_LOOKS {
                step = w.look(format!("frame {n}"), BIG, (true, speaks, false));
                if step != Step::Wait {
                    break;
                }
            }
            step
        };
        assert_eq!(
            moving(&mut wait(), true),
            Step::Enter,
            "alive, and its Enter is confirmed"
        );
        assert_eq!(moving(&mut wait(), false), Step::GiveUp);
    }

    /// The looks after an Enter, the screen ENTER_AFTER after it first, until one says more than
    /// Wait: what it says, and after how many looks.
    fn checked(
        c: &mut EnterCheck,
        screens: impl Fn(u32) -> String,
        submitted: u64,
    ) -> (Check, u32) {
        for n in 0..=CHECKS + 1 {
            let check = c.look(screens(n), submitted, true, false);
            if check != Check::Wait {
                return (check, n);
            }
        }
        (Check::Wait, CHECKS + 1)
    }

    /// Codex and Copilot CLI took the first Enter for a new line in the paste: its screen moved
    /// no more after it. A second Enter goes, and takes it.
    #[test]
    fn an_enter_taken_for_a_new_line_goes_again() {
        let mut c = EnterCheck::new(false, 0, "› fix".into(), true);
        assert_eq!(
            checked(&mut c, |_| "› fix\n".into(), 0),
            (Check::Again, CHECKS)
        );
        c.again();
        assert_eq!(
            checked(&mut c, |n| format!("Working ⠋{n}"), 0),
            (Check::Taken, 2)
        );
    }

    /// Two Enters that changed nothing at all (an input not ready, a task lost): typed but not
    /// submitted.
    #[test]
    fn two_enters_that_changed_nothing_dont_take_the_task() {
        for spoke in [false, true] {
            let mut c = EnterCheck::new(spoke, 4, "› fix".into(), false);
            assert_eq!(
                checked(&mut c, |_| "› fix".into(), 4),
                (Check::Again, CHECKS)
            );
            c.again();
            let not = Check::Not(NotTaken::Unsent);
            assert_eq!(checked(&mut c, |_| "› fix".into(), 4), (not, CHECKS));
        }
        // Its first Enter did something to a paste that went as keys (a new line in it, or the
        // task taken by an agent that draws nothing while it works): only its hooks, when they
        // speak, would say it didn't take it.
        let mut c = EnterCheck::new(false, 0, "› fix".into(), true);
        assert_eq!(
            checked(&mut c, |_| "> fix".into(), 0),
            (Check::Again, CHECKS)
        );
        c.again();
        assert_eq!(
            checked(&mut c, |_| "> fix".into(), 0),
            (Check::Taken, CHECKS)
        );
        let mut c = EnterCheck::new(true, 0, "› fix".into(), true);
        checked(&mut c, |_| "> fix".into(), 0);
        c.again();
        let not = Check::Not(NotTaken::Unsent);
        assert_eq!(checked(&mut c, |_| "> fix".into(), 0), (not, CHECKS));
        // Its second did, at once, and nothing moved after.
        let mut c = EnterCheck::new(false, 0, "› fix".into(), false);
        checked(&mut c, |_| "› fix".into(), 0);
        c.again();
        assert_eq!(
            checked(&mut c, |_| "Sent.".into(), 0),
            (Check::Taken, CHECKS)
        );
    }

    /// An agent no hook speaks for took it: its screen moves. One whose hooks speak says so.
    #[test]
    fn a_task_is_taken_when_its_screen_moves_or_its_hooks_say_so() {
        let mut c = EnterCheck::new(false, 0, "› fix".into(), false);
        assert_eq!(
            checked(&mut c, |n| format!("Working ⠋{n}"), 0),
            (Check::Taken, 2)
        );
        // A moving screen isn't a prompt taken for an agent whose hooks speak.
        let mut c = EnterCheck::new(true, 3, "› fix".into(), false);
        let unheard = Check::Not(NotTaken::Unheard);
        assert_eq!(
            checked(&mut c, |n| format!("Working ⠋{n}"), 3),
            (unheard, CHECKS)
        );
        let mut c = EnterCheck::new(true, 3, "› fix".into(), false);
        assert_eq!(c.look("› fix".into(), 4, true, false), Check::Taken);
        // A dialog its hooks told of: Templum may not type into it, and what comes next tells.
        let mut c = EnterCheck::new(false, 0, "› fix".into(), false);
        assert_eq!(c.look("Allow?".into(), 0, false, false), Check::Taken);
    }

    /// A screen that changed once and stood still (a dialog no hook tells of, or work that draws
    /// nothing more) gets no Enter that would answer the dialog; nor one the person types into.
    #[test]
    fn no_second_enter_into_a_changed_screen_or_while_the_person_types() {
        let changed = |n: u32| {
            if n < 3 {
                "› fix"
            } else {
                "Trust this folder? › Yes"
            }
            .to_string()
        };
        let mut c = EnterCheck::new(false, 0, "› fix".into(), false);
        assert_eq!(checked(&mut c, changed, 0), (Check::Taken, CHECKS));
        let mut c = EnterCheck::new(true, 0, "› fix".into(), false);
        let unheard = Check::Not(NotTaken::Unheard);
        assert_eq!(checked(&mut c, changed, 0), (unheard, CHECKS));
        let mut c = EnterCheck::new(false, 0, "› fix".into(), false);
        for _ in 0..CHECKS {
            assert_eq!(c.look("› fix".into(), 0, true, true), Check::Wait);
        }
        assert_eq!(c.look("› fix".into(), 0, true, true), Check::Taken);
    }

    /// A bracketed paste takes no Enter for a new line: a screen its Enter changed at once, then
    /// still, is the task taken or a dialog drawn at once (Aider's, for a task naming a file),
    /// which a second Enter would answer for the person. None goes.
    #[test]
    fn no_second_enter_into_a_dialog_drawn_at_once() {
        let dialog = |_| "Add file to the chat? [Yes]".to_string();
        let mut c = EnterCheck::new(false, 0, "› fix".into(), false);
        assert_eq!(checked(&mut c, dialog, 0), (Check::Taken, CHECKS));
        let mut c = EnterCheck::new(true, 0, "› fix".into(), false);
        let unheard = Check::Not(NotTaken::Unheard);
        assert_eq!(checked(&mut c, dialog, 0), (unheard, CHECKS));
    }

    /// The person types into the agent before its Enter: the Enter would send their text too.
    #[test]
    fn no_enter_once_the_person_typed() {
        let mut w = wait();
        assert_eq!(
            w.look("› fix".into(), SIZE, (true, false, false)),
            Step::Wait
        );
        assert_eq!(
            w.look("› fix".into(), SIZE, (true, false, true)),
            Step::Typing
        );
    }

    /// A notice waits until the looks after a task's second Enter are over: its first Enter, the
    /// looks after it (the first reads the screen), the second's.
    #[test]
    fn a_notice_waits_for_the_looks_after_both_enters() {
        let looks = ENTER_AFTER * (CHECKS + 1);
        assert!(NOTICE_GAP >= ENTER_AFTER * (ENTER_LOOKS + 1) + looks * 2);
    }
}
