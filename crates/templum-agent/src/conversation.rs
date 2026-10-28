//! The conversation (docs/specs/templum-agent.md#rows): the rows the chat shows, folded from the
//! engine's events, what the Templum Agent made in it, and its file.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::ChatEvent;
use crate::claude::tool_name;

/// The conversation file's version this Templum writes and reads.
const VERSION: u32 = 1;

/// One row of the chat.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Row {
    /// The person's message; queued until the engine takes it.
    Person {
        text: String,
        #[serde(default)]
        queued: bool,
    },
    /// The agent's text, Markdown.
    Text { text: String },
    /// The engine thinks; never saved.
    Thinking,
    /// A control tool's call: its tool (without the prefix), its call, its state, the first line
    /// of its answer, and what it touched.
    Action {
        id: String,
        tool: String,
        #[serde(default)]
        input: Value,
        state: ActionState,
        #[serde(default)]
        said: String,
        #[serde(default)]
        target: Option<Target>,
    },
    /// Consecutive reads, folded.
    Reads { count: u32 },
    /// An approval, once answered: its sentence and the answer.
    Approval { said: String, answer: String },
    /// A turn's end: its time and cost when the engine says them.
    End {
        #[serde(default)]
        ms: Option<u64>,
        #[serde(default)]
        cost_usd: Option<f64>,
    },
    /// A failed turn or an engine that stopped.
    Failure { text: String },
    /// "Stopped by you.", "New conversation."
    Note { text: String },
    /// A task it handed ended, or its agent waits for its person: the notice's first line, and
    /// the block it is about.
    Notice {
        text: String,
        level: NoticeLevel,
        #[serde(default)]
        target: Option<Target>,
    },
}

/// What a notice tells: a task done, failed, or its agent waiting for its person.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeLevel {
    Done,
    Failed,
    Waiting,
}

/// Where an action is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Running,
    Done,
    Failed,
    Denied,
}

/// What an action touched: a click on its row shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Target {
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub block: Option<String>,
}

/// What the Templum Agent made in this conversation: its own, closed and deleted without asking.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Own {
    pub workspaces: Vec<String>,
    pub blocks: Vec<String>,
    pub boards: Vec<u64>,
    pub cards: Vec<u64>,
    pub notes: Vec<u64>,
    pub checklists: Vec<u64>,
}

/// One chat with the engine.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Conversation {
    pub version: u32,
    pub engine: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub rows: Vec<Row>,
    #[serde(default)]
    pub own: Own,
    /// What the session had spent at its last turn's end, in US dollars: Claude Code's
    /// `total_cost_usd` is the session's running total (a resumed one's included).
    #[serde(default)]
    pub spent: f64,
    /// A turn runs, or a message waits for one.
    #[serde(skip)]
    pub working: bool,
    /// The messages the engine was given that no turn has taken yet.
    #[serde(skip)]
    pending: u32,
    /// The turn under way took a message (its replay came).
    #[serde(skip)]
    took: bool,
    /// The last row is the text the engine is writing: its next delta grows it.
    #[serde(skip)]
    open_text: bool,
}

/// What reading the conversation file found.
#[derive(Debug)]
pub enum Load {
    /// No file.
    None,
    Some(Box<Conversation>),
    /// A file that doesn't read (half-written, or from a newer Templum), now set aside as
    /// `conversation.json.bak`.
    Unreadable,
}

/// Whether a tool only reads: its rows fold into one.
pub fn is_read(tool: &str) -> bool {
    matches!(
        tool_name(tool),
        "overview"
            | "list_agents"
            | "list_shells"
            | "read_terminal"
            | "list_cards"
            | "read_note"
            | "read_checklist"
            | "Read"
            | "Glob"
            | "Grep"
    )
}

/// What a notice's first line says its task spent: `…: done in 31 s, $0.31, transcript …]`.
fn notice_cost(line: &str) -> Option<f64> {
    let head = line.split(']').next()?;
    let at = head.find(", $")? + 3;
    let digits: String = head[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse().ok()
}

impl Conversation {
    pub fn new(engine: &str) -> Self {
        Self {
            version: VERSION,
            engine: engine.to_owned(),
            session: None,
            rows: Vec::new(),
            own: Own::default(),
            spent: 0.0,
            working: false,
            pending: 0,
            took: false,
            open_text: false,
        }
    }

    /// The last row is the text the engine is writing: it grows with each delta.
    pub fn streaming(&self) -> bool {
        self.open_text
    }

    /// The person sent a message: queued when a turn runs.
    pub fn person(&mut self, text: &str) {
        self.push(Row::Person {
            text: text.to_owned(),
            queued: self.working,
        });
        self.pending += 1;
        self.working = true;
    }

    /// Folds one of the engine's events into the rows.
    pub fn apply(&mut self, e: ChatEvent) {
        if e != ChatEvent::Thinking && self.rows.last() == Some(&Row::Thinking) {
            self.rows.pop();
        }
        // Only a delta keeps the turn's text open; `Text` ends the block that streamed.
        let open_text = std::mem::take(&mut self.open_text);
        match e {
            ChatEvent::Started { session, .. } => {
                self.session = Some(session);
                self.working = true;
            }
            ChatEvent::Thinking => {
                if self.rows.last() != Some(&Row::Thinking) {
                    self.rows.push(Row::Thinking);
                }
            }
            ChatEvent::TextDelta(delta) => {
                match self.rows.last_mut() {
                    Some(Row::Text { text }) if open_text => text.push_str(&delta),
                    _ => self.rows.push(Row::Text { text: delta }),
                }
                self.open_text = true;
            }
            ChatEvent::Text(whole) => match self.rows.last_mut() {
                Some(Row::Text { text }) if open_text => *text = whole,
                _ => self.rows.push(Row::Text { text: whole }),
            },
            ChatEvent::ToolStarted { id, name } => {
                if is_read(&name) {
                    match self.rows.last_mut() {
                        Some(Row::Reads { count }) => *count += 1,
                        _ => self.rows.push(Row::Reads { count: 1 }),
                    }
                } else {
                    self.rows.push(Row::Action {
                        id,
                        tool: tool_name(&name).to_owned(),
                        input: Value::Null,
                        state: ActionState::Running,
                        said: String::new(),
                        target: None,
                    });
                }
            }
            ChatEvent::ToolCall {
                id, input: call, ..
            } => {
                if let Some(Row::Action { input, .. }) = self.action(&id) {
                    *input = call;
                }
            }
            ChatEvent::ToolResult { id, text, is_error } => {
                if let Some(Row::Action { state, said, .. }) = self.action(&id).filter(|r| {
                    !matches!(
                        r,
                        Row::Action {
                            state: ActionState::Denied,
                            ..
                        }
                    )
                }) {
                    *state = if is_error {
                        ActionState::Failed
                    } else {
                        ActionState::Done
                    };
                    *said = text.lines().next().unwrap_or_default().to_owned();
                }
            }
            // Each message is replayed once, when a turn takes it: at its start, or at a tool's
            // answer for one written while it runs.
            ChatEvent::Taken(taken) => {
                self.pending = self.pending.saturating_sub(1);
                self.took = true;
                if let Some(Row::Person { queued, .. }) = self
                    .rows
                    .iter_mut()
                    .find(|r| matches!(r, Row::Person { text, queued: true } if *text == taken))
                {
                    *queued = false;
                }
            }
            ChatEvent::TurnEnded {
                ms,
                cost_usd,
                error,
            } => {
                // The session's running total: this turn spent what it added.
                let cost_usd = cost_usd.map(|total| {
                    let turn = if total >= self.spent {
                        total - self.spent
                    } else {
                        total
                    };
                    self.spent = total;
                    turn
                });
                self.rows.push(Row::End { ms, cost_usd });
                if let Some(error) = error {
                    self.rows.push(Row::Failure {
                        text: format!("The turn failed: {error}"),
                    });
                }
                // Nothing it called answers after its end.
                self.fail_running();
                // A turn that failed before its replay still answered its message.
                if !self.took {
                    self.pending = self.pending.saturating_sub(1);
                }
                self.took = false;
                self.working = self.pending > 0;
            }
        }
    }

    /// The action row of a tool call.
    fn action(&mut self, call: &str) -> Option<&mut Row> {
        self.rows
            .iter_mut()
            .rev()
            .find(|r| matches!(r, Row::Action { id, .. } if id == call))
    }

    /// Templum says what an action touched: the oldest action of that tool and call without a
    /// target gets it (one whose call the stream hasn't given yet counts).
    pub fn touched(&mut self, tool: &str, input: &Value, target: Target) {
        if let Some(Row::Action { target: t, .. }) = self.rows.iter_mut().find(|r| {
            matches!(r, Row::Action { tool: t, input: i, target: None, .. }
                if t == tool && (i == input || i.is_null()))
        }) {
            *t = Some(target);
        }
    }

    /// The person denied a call: its action row says so, whatever its refusal says after.
    pub fn denied(&mut self, tool: &str, input: &Value) {
        if let Some(Row::Action { state, said, .. }) = self.rows.iter_mut().find(|r| {
            matches!(r, Row::Action { tool: t, input: i, state: ActionState::Running, .. }
                if t == tool && (i == input || i.is_null()))
        }) {
            *state = ActionState::Denied;
            *said = "You denied this.".into();
        }
    }

    /// The engine is gone: what runs failed, and a failure row says why.
    pub fn ended(&mut self, why: &str) {
        self.end_running();
        self.rows.push(Row::Failure {
            text: why.to_owned(),
        });
    }

    /// The person stopped the turn: what runs failed, and a note says so.
    pub fn stopped(&mut self) {
        self.end_running();
        self.rows.push(Row::Note {
            text: "Stopped by you.".into(),
        });
    }

    /// No turn runs any more: no Thinking, no action running, nothing working.
    fn end_running(&mut self) {
        if self.rows.last() == Some(&Row::Thinking) {
            self.rows.pop();
        }
        self.fail_running();
        // What it was given and didn't take is lost with it.
        for row in &mut self.rows {
            if let Row::Person { queued, .. } = row {
                *queued = false;
            }
        }
        self.open_text = false;
        self.pending = 0;
        self.took = false;
        self.working = false;
    }

    /// An action still running failed: no answer comes for it any more.
    fn fail_running(&mut self) {
        for row in &mut self.rows {
            if let Row::Action {
                state: state @ ActionState::Running,
                ..
            } = row
            {
                *state = ActionState::Failed;
            }
        }
    }

    /// A row of Templum's: above the engine's Thinking, which stays last for its next event to
    /// take away.
    fn push(&mut self, row: Row) {
        self.open_text = false;
        let at = self.rows.len() - usize::from(self.rows.last() == Some(&Row::Thinking));
        self.rows.insert(at, row);
    }

    /// A notice from one of its tasks, given to the engine as a message: a turn answers it.
    pub fn notice(&mut self, text: &str, level: NoticeLevel, target: Option<Target>) {
        let first = text.lines().next().unwrap_or_default();
        self.push(Row::Notice {
            text: first.to_owned(),
            level,
            target,
        });
        self.pending += 1;
        self.working = true;
    }

    pub fn note(&mut self, text: &str) {
        self.push(Row::Note {
            text: text.to_owned(),
        });
    }

    /// What it spent, in US dollars: its turns' cost, and its tasks' as their notices say.
    pub fn cost(&self) -> f64 {
        self.rows
            .iter()
            .filter_map(|r| match r {
                Row::End { cost_usd, .. } => *cost_usd,
                Row::Notice { text, .. } => notice_cost(text),
                _ => None,
            })
            .sum()
    }

    pub fn load(path: &Path) -> Load {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Load::None,
            Err(_) => String::new(),
        };
        match serde_json::from_str::<Conversation>(&text) {
            Ok(mut c) if c.version == VERSION => {
                // A message queued when Templum quit went with its engine.
                for row in &mut c.rows {
                    if let Row::Person { queued, .. } = row {
                        *queued = false;
                    }
                }
                Load::Some(Box::new(c))
            }
            _ => {
                let _ = std::fs::rename(path, path.with_extension("json.bak"));
                Load::Unreadable
            }
        }
    }

    /// Writes the file as the chat shows it: no Thinking, and an action still running failed
    /// with "Templum quit.".
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let mut c = self.clone();
        c.version = VERSION;
        c.rows.retain(|r| *r != Row::Thinking);
        for row in &mut c.rows {
            if let Row::Action {
                state: state @ ActionState::Running,
                said,
                ..
            } = row
            {
                *state = ActionState::Failed;
                *said = "Templum quit.".into();
            }
        }
        crate::write_atomic(path, &serde_json::to_vec_pretty(&c).expect("JSON"))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn turn(lines: &str) -> Conversation {
        let mut c = Conversation::new("claude");
        c.person("hi");
        for e in lines.lines().flat_map(crate::claude::parse) {
            c.apply(e);
        }
        c
    }

    fn started(id: &str, name: &str) -> ChatEvent {
        ChatEvent::ToolStarted {
            id: id.into(),
            name: name.into(),
        }
    }

    #[test]
    fn deltas_then_the_whole_text_make_one_text_row() {
        let mut c = Conversation::new("claude");
        c.person("hi");
        assert!(c.working);
        for e in [
            ChatEvent::Thinking,
            ChatEvent::TextDelta("Hel".into()),
            ChatEvent::TextDelta("lo".into()),
            ChatEvent::Text("Hello!".into()),
            ChatEvent::TurnEnded {
                ms: Some(1200),
                cost_usd: Some(0.04),
                error: None,
            },
        ] {
            c.apply(e);
        }
        assert_eq!(
            c.rows,
            vec![
                Row::Person {
                    text: "hi".into(),
                    queued: false
                },
                Row::Text {
                    text: "Hello!".into()
                },
                Row::End {
                    ms: Some(1200),
                    cost_usd: Some(0.04)
                },
            ]
        );
        assert!(!c.working);
        assert_eq!(c.cost(), 0.04);
    }

    #[test]
    fn a_recorded_turn_folds_into_its_rows() {
        let c = turn(include_str!("../tests/fixtures/claude-tools.jsonl"));
        assert!(
            matches!(
                &c.rows[..],
                [Row::Person { .. }, Row::Reads { count: 1 }, Row::Text { text }, Row::End { cost_usd: Some(_), .. }]
                    if text.contains("hello-fixture")
            ),
            "{:?}",
            c.rows
        );
        assert!(c.session.is_some());
    }

    #[test]
    fn an_action_runs_then_says_the_first_line_of_its_answer() {
        let mut c = Conversation::new("claude");
        c.person("make it");
        c.apply(started("t1", "mcp__templum-control__create_workspace"));
        c.apply(ChatEvent::ToolCall {
            id: "t1".into(),
            name: "mcp__templum-control__create_workspace".into(),
            input: json!({"name": "API"}),
        });
        assert!(
            matches!(&c.rows[1], Row::Action { state: ActionState::Running, tool, input, .. }
                if tool == "create_workspace" && input["name"] == "API")
        );
        c.touched(
            "create_workspace",
            &json!({"name": "API"}),
            Target {
                workspace: Some("w3".into()),
                block: None,
            },
        );
        c.apply(ChatEvent::ToolResult {
            id: "t1".into(),
            text: "Created workspace API (w3).\nmore".into(),
            is_error: false,
        });
        assert!(
            matches!(&c.rows[1], Row::Action { state: ActionState::Done, said, target: Some(t), .. }
            if said == "Created workspace API (w3)." && t.workspace.as_deref() == Some("w3"))
        );
        c.apply(started("t2", "mcp__templum-control__close_block"));
        c.apply(ChatEvent::ToolResult {
            id: "t2".into(),
            text: "No block x.".into(),
            is_error: true,
        });
        assert!(
            matches!(&c.rows[2], Row::Action { state: ActionState::Failed, said, .. } if said == "No block x.")
        );
    }

    #[test]
    fn reads_in_a_row_fold_into_one() {
        let mut c = Conversation::new("claude");
        c.person("look");
        for e in [
            started("a", "Read"),
            started("b", "mcp__templum-control__overview"),
            started("c", "Glob"),
        ] {
            c.apply(e);
        }
        assert_eq!(c.rows[1..], [Row::Reads { count: 3 }]);
        c.apply(ChatEvent::TextDelta("So.".into()));
        c.apply(started("d", "Read"));
        assert_eq!(
            c.rows[1..],
            [
                Row::Reads { count: 3 },
                Row::Text { text: "So.".into() },
                Row::Reads { count: 1 }
            ]
        );
    }

    #[test]
    fn a_message_while_it_works_is_queued_until_taken() {
        let mut c = Conversation::new("claude");
        c.person("first");
        c.apply(ChatEvent::Taken("first".into()));
        c.person("second");
        assert_eq!(
            c.rows[1],
            Row::Person {
                text: "second".into(),
                queued: true
            }
        );
        c.apply(ChatEvent::TurnEnded {
            ms: None,
            cost_usd: None,
            error: None,
        });
        assert!(c.working, "the queued message's turn comes next");
        c.apply(ChatEvent::Taken("second".into()));
        assert_eq!(
            c.rows[1],
            Row::Person {
                text: "second".into(),
                queued: false
            }
        );
    }

    #[test]
    fn a_denied_action_stays_denied_when_its_refusal_comes() {
        let mut c = Conversation::new("claude");
        c.person("close it");
        c.apply(started("t1", "mcp__templum-control__close_block"));
        c.apply(ChatEvent::ToolCall {
            id: "t1".into(),
            name: "mcp__templum-control__close_block".into(),
            input: json!({"block": "b1"}),
        });
        c.denied("close_block", &json!({"block": "b1"}));
        c.apply(ChatEvent::ToolResult {
            id: "t1".into(),
            text: "The person denied this.".into(),
            is_error: true,
        });
        assert!(
            matches!(&c.rows[1], Row::Action { state: ActionState::Denied, said, .. }
            if said == "You denied this.")
        );
    }

    #[test]
    fn a_notice_is_its_first_line_and_starts_a_turn() {
        let mut c = Conversation::new("claude");
        let target = Target {
            workspace: Some("w2".into()),
            block: Some("b1".into()),
        };
        c.notice(
            "[task t1 from Codex (b1): done in 3 s] Its answer.\nAll good.",
            NoticeLevel::Done,
            Some(target.clone()),
        );
        assert_eq!(
            c.rows,
            [Row::Notice {
                text: "[task t1 from Codex (b1): done in 3 s] Its answer.".into(),
                level: NoticeLevel::Done,
                target: Some(target),
            }]
        );
        assert!(c.working);
    }

    #[test]
    fn the_cost_is_its_turns_and_its_tasks() {
        let mut c = Conversation::new("claude");
        c.person("go");
        c.apply(ChatEvent::TurnEnded {
            ms: None,
            cost_usd: Some(0.04),
            error: None,
        });
        c.notice(
            "[task t1 from Codex (b1): done in 31 s, $0.31, transcript /t.jsonl] Its answer.",
            NoticeLevel::Done,
            None,
        );
        c.notice(
            "[task t2 from Gemini (b2): done in 3 s, 1200 tokens] Its answer.",
            NoticeLevel::Done,
            None,
        );
        assert!((c.cost() - 0.35).abs() < 1e-9, "{}", c.cost());
    }

    #[test]
    fn a_notice_during_a_turn_keeps_it_working_until_its_own_turn_ends() {
        let mut c = Conversation::new("claude");
        let started = || ChatEvent::Started {
            session: "s".into(),
            model: None,
        };
        let ended = || ChatEvent::TurnEnded {
            ms: None,
            cost_usd: None,
            error: None,
        };
        c.person("go");
        c.apply(started());
        c.notice(
            "[task t1 from Codex (b1): done in 3 s] Its answer.",
            NoticeLevel::Done,
            None,
        );
        c.apply(ended());
        assert!(c.working, "the notice's turn comes next");
        c.apply(started());
        c.apply(ended());
        assert!(!c.working);
    }

    #[test]
    fn messages_taken_into_the_turn_under_way_leave_none_waiting() {
        // Claude Code takes the two written during this turn into it, at a tool's answer: one
        // init and one result for three messages, each replayed.
        let mut c = Conversation::new("claude");
        c.person(
            "Read each of the files f1.txt to f6.txt in this folder, one Read call at a time, \
             then tell me how many words in total.",
        );
        c.person("Also, after that, say the word banana.");
        c.person("And then say the word cherry.");
        for e in include_str!("../tests/fixtures/claude-taken.jsonl")
            .lines()
            .flat_map(crate::claude::parse)
        {
            c.apply(e);
        }
        assert!(!c.working, "no turn comes for what this one took");
        assert!(
            !c.rows
                .iter()
                .any(|r| matches!(r, Row::Person { queued: true, .. }))
        );
    }

    #[test]
    fn a_message_while_it_thinks_stays_above_thinking() {
        let mut c = Conversation::new("claude");
        c.person("go");
        c.apply(ChatEvent::Thinking);
        c.person("and this");
        c.notice(
            "[task t1 from Codex (b1): done in 3 s] Its answer.",
            NoticeLevel::Done,
            None,
        );
        assert_eq!(c.rows.last(), Some(&Row::Thinking));
        c.apply(started("t1", "mcp__templum-control__add_block"));
        assert!(!c.rows.contains(&Row::Thinking), "{:?}", c.rows);
    }

    #[test]
    fn a_turns_end_fails_an_action_it_never_answered() {
        let mut c = Conversation::new("claude");
        c.person("go");
        c.apply(started("t1", "mcp__templum-control__add_block"));
        c.apply(ChatEvent::TurnEnded {
            ms: None,
            cost_usd: None,
            error: Some("Overloaded".into()),
        });
        assert!(matches!(
            &c.rows[1],
            Row::Action {
                state: ActionState::Failed,
                ..
            }
        ));
    }

    #[test]
    fn a_message_lost_with_the_engine_is_no_longer_queued() {
        let mut c = Conversation::new("claude");
        c.person("go");
        c.apply(ChatEvent::Started {
            session: "s".into(),
            model: None,
        });
        c.person("and then this");
        c.ended("Claude Code stopped: gone. Your next message starts it again.");
        assert!(!c.working);
        assert!(
            !c.rows
                .iter()
                .any(|r| matches!(r, Row::Person { queued: true, .. }))
        );
        c.person("again");
        c.apply(ChatEvent::Started {
            session: "s".into(),
            model: None,
        });
        c.apply(ChatEvent::TurnEnded {
            ms: None,
            cost_usd: None,
            error: None,
        });
        assert!(!c.working, "the lost message waits for no turn");
    }

    #[test]
    fn each_end_says_its_own_turns_cost_from_the_sessions_running_total() {
        let c = turn(include_str!("../tests/fixtures/claude-queued.jsonl"));
        let costs: Vec<f64> = c
            .rows
            .iter()
            .filter_map(|r| match r {
                Row::End { cost_usd, .. } => *cost_usd,
                _ => None,
            })
            .collect();
        // The stream says 0.0259058, then 0.0372116 for the whole process.
        assert_eq!(costs.len(), 2);
        assert!(
            (costs[0] - 0.0259058).abs() < 1e-9 && (costs[1] - 0.0113058).abs() < 1e-9,
            "{costs:?}"
        );
        assert!((c.cost() - 0.0372116).abs() < 1e-9);
    }

    #[test]
    fn a_failed_turn_says_why() {
        let mut c = Conversation::new("claude");
        c.person("hi");
        c.apply(ChatEvent::TurnEnded {
            ms: Some(5),
            cost_usd: None,
            error: Some("Overloaded".into()),
        });
        assert_eq!(
            c.rows[1..],
            [
                Row::End {
                    ms: Some(5),
                    cost_usd: None
                },
                Row::Failure {
                    text: "The turn failed: Overloaded".into()
                }
            ]
        );
    }

    #[test]
    fn the_engine_ending_fails_what_runs() {
        let mut c = Conversation::new("claude");
        c.person("go");
        c.apply(started("t1", "mcp__templum-control__add_block"));
        c.apply(ChatEvent::Thinking);
        let why = "Claude Code stopped: x. Your next message starts it again.";
        c.ended(why);
        assert!(matches!(
            &c.rows[1],
            Row::Action {
                state: ActionState::Failed,
                ..
            }
        ));
        assert_eq!(c.rows.last(), Some(&Row::Failure { text: why.into() }));
        assert!(!c.rows.contains(&Row::Thinking));
        assert!(!c.working);
    }

    #[test]
    fn a_stop_fails_what_runs_and_says_so_quietly() {
        let mut c = Conversation::new("claude");
        c.person("go");
        c.apply(started("t1", "mcp__templum-control__add_block"));
        c.stopped();
        assert!(matches!(
            &c.rows[1],
            Row::Action {
                state: ActionState::Failed,
                ..
            }
        ));
        assert_eq!(
            c.rows.last(),
            Some(&Row::Note {
                text: "Stopped by you.".into()
            })
        );
        assert!(!c.working);
    }

    #[test]
    fn the_file_round_trips_without_thinking_and_with_running_actions_failed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversation.json");
        let mut c = turn(include_str!("../tests/fixtures/claude-tools.jsonl"));
        c.own.blocks.push("b7k2m".into());
        c.session = Some("u1".into());
        c.person("more");
        c.apply(started("t9", "mcp__templum-control__add_block"));
        c.apply(ChatEvent::Thinking);
        c.save(&path).unwrap();
        let Load::Some(back) = Conversation::load(&path) else {
            panic!()
        };
        assert_eq!(
            (back.session.as_deref(), back.own.blocks.clone()),
            (Some("u1"), vec!["b7k2m".to_string()])
        );
        assert!(!back.rows.contains(&Row::Thinking));
        assert!(
            matches!(back.rows.last(), Some(Row::Action { state: ActionState::Failed, said, .. })
            if said == "Templum quit.")
        );
        assert_eq!(back.rows[..4], c.rows[..4]);
        assert!(!back.working);
    }

    #[test]
    fn an_unreadable_or_newer_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conversation.json");
        for text in [
            "{\"version\":1,\"rows\":[",
            "{\"version\":2,\"engine\":\"claude\",\"rows\":[]}",
        ] {
            std::fs::write(&path, text).unwrap();
            assert!(matches!(Conversation::load(&path), Load::Unreadable));
            assert!(path.with_extension("json.bak").exists() && !path.exists());
        }
        assert!(matches!(Conversation::load(&path), Load::None));
    }
}
