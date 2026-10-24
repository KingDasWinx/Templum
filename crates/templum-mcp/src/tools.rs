//! The MCP tools Templum serves (docs/specs/connections.md, Tools): their schemas, the calls
//! parsed from `tools/call`, and the replies.

use serde_json::{Value, json};

/// The MCP versions Templum speaks, newest first: it serves tools only, whose messages none of
/// these changed.
const VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// The most lines `read_terminal` returns.
pub const MAX_LINES: usize = 2000;
/// The most bytes of text a tool returns: its end is kept.
pub const MAX_TEXT: usize = 256 * 1024;
/// The longest `timeout` a call may ask, seconds. A question asked without one may wait longer:
/// the agents Templum opens wait a week for a tool (`codex_flags`, templum-usage's
/// `MCP_TIMEOUT_MS`).
pub const MAX_WAIT: u64 = 3600;
/// How long a request to connect waits for the person, seconds, unless asked. A question waits
/// until it is answered (docs/specs/ask-user.md, How long it waits).
pub const ASK_WAIT: u64 = 1800;
/// How long `delegate` waits for the answer, and how long a task stays handed over, seconds,
/// unless asked.
pub const TASK_WAIT: u64 = 600;
/// How long `wait_for_idle` and `wait_for` wait, seconds, unless asked.
pub const IDLE_WAIT: u64 = 300;
/// The most keys `send_keys` presses in one call.
pub const MAX_KEYS: usize = 16;

/// A tool call, its arguments checked.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    Whoami,
    ListBlocks,
    ReadTerminal {
        block: String,
        lines: usize,
        last_command: bool,
    },
    SendToTerminal {
        block: String,
        text: String,
        enter: bool,
    },
    Notify {
        text: String,
    },
    /// `timeout`: the call's own, if it gave one.
    AskUser {
        ask: crate::ask::Ask,
        timeout: Option<u64>,
    },
    Connect {
        block: String,
        timeout: u64,
    },
    Delegate {
        block: String,
        task: String,
        wait: bool,
        timeout: u64,
    },
    /// Without a `block`, the task the caller is on is answered.
    ReplyTo {
        block: Option<String>,
        text: String,
    },
    WaitForIdle {
        block: String,
        timeout: u64,
    },
    /// `pattern` goes with `Event::Output` alone; `lines` (0 unless given) with a pattern; and
    /// `wait` false (a watch, its match typed back) too.
    WaitFor {
        block: String,
        event: Event,
        timeout: u64,
        pattern: Option<String>,
        lines: usize,
        wait: bool,
    },
    SpawnAgent(Spawn),
    ListCards {
        board: Option<String>,
        card: Option<u64>,
    },
    CreateCard(NewCard),
    /// A note, a checklist or a terminal beside the caller, wired to it.
    CreateBlock(NewBlockArgs),
    EditCard(CardEdit),
    MoveCard {
        card: u64,
        column: String,
    },
    CommentCard {
        card: u64,
        text: String,
    },
    /// The caller's own comment on a card gets a new text.
    EditComment {
        card: u64,
        comment: u64,
        text: String,
    },
    /// The caller's own comment on a card goes.
    DeleteComment {
        card: u64,
        comment: u64,
    },
    ClaimCard {
        card: u64,
    },
    ReadNote {
        block: String,
    },
    AppendNote {
        block: String,
        text: String,
    },
    /// The note's whole text, or its one passage `old`, becomes `text`.
    ReplaceNote {
        block: String,
        text: String,
        old: Option<String>,
    },
    ReadChecklist {
        block: String,
    },
    CheckItem {
        block: String,
        item: u64,
        done: bool,
    },
    AddItems {
        block: String,
        text: String,
    },
    EditItem {
        block: String,
        item: u64,
        text: String,
    },
    DeleteItem {
        block: String,
        item: u64,
    },
    OpenFile {
        path: String,
        line: Option<u32>,
    },
    /// Presses keys in a terminal, by name (`templum_terminal::KEY_NAMES`, checked by the app).
    SendKeys {
        block: String,
        keys: Vec<String>,
    },
    /// Esc in an agent; ends the task the caller handed it.
    Interrupt {
        block: String,
    },
    /// A plan the person approves or rejects in the caller's balloon.
    ProposePlan {
        plan: String,
        title: Option<String>,
        timeout: Option<u64>,
    },
    /// `text` delivered to `block` (or the caller) later: at a time, after some minutes, and
    /// again every `every` minutes.
    Schedule {
        block: String,
        text: String,
        when: When,
        every: Option<u64>,
    },
    /// Cancels one of the caller's schedules, `s3` or `3`.
    CancelSchedule {
        schedule: String,
    },
    /// Removes a worktree `spawn_agent` made: at once when it is clean and merged; with
    /// `discard`, after the person allows losing its work.
    RemoveWorktree {
        worktree: String,
        folder: Option<String>,
        discard: bool,
    },
    /// Merges a worktree's branch back, once the person allows it.
    MergeWorktree {
        worktree: String,
        folder: Option<String>,
    },
    /// Ends what runs in a terminal the caller opened, and what it left running.
    StopAgent {
        block: String,
    },
    /// Closes a block the caller opened with `spawn_agent`.
    CloseBlock {
        block: String,
    },
    /// Wires two blocks: `templum wire`'s alone, never listed to agents.
    Wire {
        a: String,
        b: String,
    },
}

/// What `wait_for` waits for in a block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    /// The agent finishes its turn; at once when it is idle.
    Idle,
    /// The agent waits for its person (a permission, a question); at once when it already does.
    NeedsYou,
    /// The next command that finishes in the terminal (needs the shell's integration).
    CommandFinished,
    /// A line printed in the terminal that matches a pattern.
    Output,
    /// The terminal rings or sends a notification.
    Attention,
}

/// What `spawn_agent` opens: the agent (an id of Settings ▸ Agents), its prompt, its folder (the
/// caller's when left out), and the worktree it makes first.
#[derive(Clone, Debug, PartialEq)]
pub struct Spawn {
    /// None only for a fork, which is always of the caller's own agent.
    pub agent: Option<String>,
    pub prompt: Option<String>,
    pub folder: Option<String>,
    pub worktree: Option<String>,
    /// The model and the reasoning effort its agent starts on, checked by the app.
    pub model: Option<String>,
    pub effort: Option<String>,
    /// A copy of the caller's own agent, started on its conversation so far.
    pub fork: bool,
    /// A role of `[roles]` in config.toml: its agent, model, effort, words and instructions.
    pub role: Option<String>,
    /// The role's words for its agent's command line; set by the app from the role, never by a
    /// call.
    pub args: Option<String>,
}

/// When a schedule first goes: at a time of the computer's clock (`HH:MM`, the next time it
/// shows it), after some minutes, or (`None`) `every` minutes from now.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum When {
    At { hour: u8, minute: u8 },
    After { minutes: u64 },
    None,
}

/// The most minutes `schedule`'s `after` and `every` take: a week.
pub const MAX_MINUTES: u64 = 10080;

/// The kinds of block `create_block` opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BlockKind {
    Note,
    Checklist,
    Terminal,
}

/// What `create_block` opens: a note's `text`; a checklist's `name` and `items` (an item per line,
/// as `add_items` reads them); a terminal's `folder` (the caller's when left out, relative to it)
/// and the `command` typed and run in it.
#[derive(Clone, Debug, PartialEq)]
pub struct NewBlockArgs {
    pub kind: BlockKind,
    pub text: Option<String>,
    pub name: Option<String>,
    pub items: Option<String>,
    pub folder: Option<String>,
    pub command: Option<String>,
}

/// The longest command `create_block` types into a terminal, in bytes.
pub const MAX_COMMAND: usize = 4096;

/// What `create_card` adds. `priority` is lowercased, one of low, medium, high or none; `who`
/// (the key `for`) is checked by the app; `checklist` is items' text, a line each.
#[derive(Clone, Debug, PartialEq)]
pub struct NewCard {
    pub board: Option<String>,
    pub column: Option<String>,
    pub title: String,
    pub description: Option<String>,
    pub priority: Option<String>,
    pub who: Option<String>,
    pub checklist: Option<String>,
}

/// What `edit_card` changes on `card`: at least one of the others.
#[derive(Clone, Debug, PartialEq)]
pub struct CardEdit {
    pub card: u64,
    pub priority: Option<String>,
    pub who: Option<String>,
    pub add_items: Option<String>,
    pub check: Vec<u64>,
    pub uncheck: Vec<u64>,
}

/// What a tool answers: its text, and whether it is a refusal or a failure.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub text: String,
    pub is_error: bool,
}

impl Reply {
    pub fn ok(text: impl Into<String>) -> Reply {
        Reply {
            text: text.into(),
            is_error: false,
        }
    }

    pub fn err(text: impl Into<String>) -> Reply {
        Reply {
            text: text.into(),
            is_error: true,
        }
    }
}

/// `initialize`'s result: the client's version when Templum speaks it, else its newest.
pub fn initialize_result(params: &Value, instructions: &str) -> Value {
    let asked = params["protocolVersion"].as_str();
    let version = VERSIONS
        .iter()
        .find(|v| Some(**v) == asked)
        .unwrap_or(&VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": "templum", "version": env!("CARGO_PKG_VERSION") },
        "instructions": instructions,
    })
}

/// `tools/list`'s result.
pub fn list() -> Value {
    let block =
        json!({ "type": "string", "description": "A block's id, as list_blocks shows it." });
    let board = json!({
        "type": "string",
        "description": "A board's name or id; may be left out when one kanban connected to you shows a board.",
    });
    let card = json!({ "type": "integer", "description": "A card's id, as list_cards shows it." });
    let column =
        json!({ "type": "string", "description": "A column's title (either case) or id." });
    let none = json!({ "type": "object", "properties": {} });
    let item =
        json!({ "type": "integer", "description": "An item's id, as read_checklist shows it." });
    let minutes = json!({ "type": "integer", "minimum": 1, "maximum": MAX_MINUTES });

    let items_text = json!({
        "type": "string",
        "description": "A checklist's items, one per line; a leading - or *, then [ ] or [x], is dropped.",
    });
    let command = json!({
        "type": "string",
        "maxLength": MAX_COMMAND,
        "description": "One line a terminal's shell runs, as the person would type it.",
    });
    let pattern = json!({
        "type": "string",
        "description": "A regular expression (Rust's regex syntax, as grep -E writes most of it): Ready in, \
                        ERROR|Traceback, (?i)listening on. Each line printed from now on is tested; the first \
                        match answers.",
    });
    let lines = json!({
        "type": "integer",
        "minimum": 0,
        "maximum": MAX_LINES,
        "description": "With a pattern: how many of the lines already in the terminal are tested first, \
                        oldest first (0 unless given), for a command that may have printed its line already.",
    });
    let priority = json!({ "type": "string", "enum": ["low", "medium", "high", "none"] });
    let who = json!({
        "type": "string",
        "description": "Who it is for: anyone, an agent's id (claude, codex, …) for any of its sessions, or a \
                        terminal block's id for that session.",
    });
    let items = json!({
        "type": "string",
        "description": "Checklist items, one per line; a leading - or *, then [ ] or [x], is dropped ([x] \
                        adds it checked).",
    });
    // A list only in the schema (no `anyOf`, which not every client's model API takes); one id
    // is taken all the same.
    let item_ids = json!({
        "type": "array",
        "items": { "type": "integer" },
        "description": "Items' ids, as list_cards with card shows them.",
    });
    // Claude Code shows these whole from the start, beside its own Agent tool, where the others
    // wait behind its tool search by name only.
    let always_load = json!({ "anthropic/alwaysLoad": true });
    let seconds = |default: u64| {
        json!({
            "type": "integer",
            "minimum": 1,
            "maximum": MAX_WAIT,
            "default": default,
            "description": "Seconds to wait before giving up.",
        })
    };
    // ask_user's, apart: one json! of every tool reaches the macro's recursion limit.
    let choice = json!({
        "type": "object",
        "properties": {
            "label": { "type": "string" },
            "description": { "type": "string" },
            "preview": {
                "type": "string",
                "description": "Shown beside the options, in a monospace frame.",
            },
        },
        "required": ["label"],
    });
    let question = json!({
        "type": "object",
        "properties": {
            "question": { "type": "string" },
            "header": {
                "type": "string",
                "description": "A short label, shown up to 12 characters.",
            },
            "options": { "type": "array", "items": choice },
            "multiSelect": { "type": "boolean", "default": false },
        },
        "required": ["question"],
    });
    let stop_agent = json!({
        "name": "stop_agent",
        "description": "Ends the agent, or the command, that runs in a terminal you opened (spawn_agent, \
                        create_block), and every process it started, background ones included; its shell \
                        stays. Only the person stops a block you didn't open.",
        "inputSchema": { "type": "object", "properties": { "block": block }, "required": ["block"] },
    });
    let create_block = json!({
        "name": "create_block",
        "description": "Opens a block beside yours, connected to you: a note with text, a checklist (your own \
                        plan for work of several steps: check its items as you go), or a terminal in a folder \
                        (yours when left out) with a command typed and run in it, such as a dev server; \
                        wait_for with a pattern waits for its line, stop_agent ends it. The person closes it \
                        with Ctrl+Z.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "kind": { "type": "string", "enum": ["note", "checklist", "terminal"] },
                "text": { "type": "string", "description": "A note's text." },
                "name": { "type": "string", "description": "A checklist's name." },
                "items": items_text,
                "folder": { "type": "string", "description": "A terminal's folder, relative to yours." },
                "command": command,
            },
            "required": ["kind"],
        },
    });
    let propose_plan = json!({
        "name": "propose_plan",
        "description": "Shows the person a plan in Markdown, in a balloon on your block, with Approve, Reject and \
                        a comment; waits for their decision (it may take minutes). Approved is a result, \
                        Rejected an error, each with their comment when they wrote one. Use it before work \
                        the person should agree to.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "plan": { "type": "string", "description": "The plan, in Markdown (at most 256 KB)." },
                "title": { "type": "string", "description": "A short title, shown up to 60 characters." },
                "timeout": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_WAIT,
                    "description": "Seconds to wait before giving up; none waits until the person answers.",
                },
            },
            "required": ["plan"],
        },
    });
    let replace_note = json!({
        "name": "replace_note",
        "description": "Replaces the text of the note a note block connected to you shows; with old, only \
                        that passage, which must be in the note once (blank text deletes it). The person sees \
                        it at once and can take it back.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "block": block,
                "text": { "type": "string" },
                "old": { "type": "string", "description": "The one passage to replace, exactly as in the note." },
            },
            "required": ["block", "text"],
        },
    });
    let edit_item = json!({
        "name": "edit_item",
        "description": "Changes the text of an item of the checklist a checklist block connected to you \
                        shows. The person sees it at once and can take it back.",
        "inputSchema": {
            "type": "object",
            "properties": { "block": block, "item": item, "text": { "type": "string" } },
            "required": ["block", "item", "text"],
        },
    });
    let delete_item = json!({
        "name": "delete_item",
        "description": "Deletes an item of the checklist a checklist block connected to you shows. The \
                        person sees it at once and can take it back.",
        "inputSchema": {
            "type": "object",
            "properties": { "block": block, "item": item },
            "required": ["block", "item"],
        },
    });
    let comment = json!({
        "type": "integer",
        "description": "A comment's number, as list_cards with card shows it or comment_card says it.",
    });
    let edit_comment = json!({
        "name": "edit_comment",
        "description": "Changes the text of a comment you wrote on a card (to fix it); the card shows it \
                        as edited. Not the person's or another agent's. One step of the person's Ctrl+Z.",
        "inputSchema": {
            "type": "object",
            "properties": { "card": card, "comment": comment, "text": { "type": "string" } },
            "required": ["card", "comment", "text"],
        },
    });
    let delete_comment = json!({
        "name": "delete_comment",
        "description": "Deletes a comment you wrote on a card. Not the person's or another agent's. One \
                        step of the person's Ctrl+Z.",
        "inputSchema": {
            "type": "object",
            "properties": { "card": card, "comment": comment },
            "required": ["card", "comment"],
        },
    });
    let schedule = json!({
        "name": "schedule",
        "description": "Delivers a text later: to an agent connected to you as a task (its end comes back to \
                        you), or to yourself (your own block) as a reminder typed into your terminal; never to \
                        a terminal running no agent. at (HH:MM, the computer's local time, \
                        the next time the clock shows it), after (minutes), or every (minutes, again until \
                        cancelled, at most 7 days). Says its id; the person sees your schedules on your block.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "block": block,
                "text": { "type": "string" },
                "at": { "type": "string", "description": "HH:MM, 24-hour." },
                "after": minutes,
                "every": minutes,
            },
            "required": ["block", "text"],
        },
    });
    let cancel_schedule = json!({
        "name": "cancel_schedule",
        "description": "Cancels one of your schedules, by its id (s3).",
        "inputSchema": {
            "type": "object",
            "properties": { "schedule": { "type": "string" } },
            "required": ["schedule"],
        },
    });
    let worktree = json!({
        "type": "string",
        "description": "The name spawn_agent's worktree was given: .worktrees/<worktree> of the repository.",
    });
    let repository = json!({
        "type": "string",
        "description": "A folder of the repository, relative to yours; yours when left out. It merges into the branch \
                        checked out there.",
    });
    let remove_worktree = json!({
        "name": "remove_worktree",
        "description": "Removes a worktree spawn_agent made, and its branch: at once when it has no changes and its \
                        commits are merged; with discard, after the person allows losing them. Refused while a block \
                        works in it.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "worktree": worktree,
                "folder": repository,
                "discard": { "type": "boolean" },
            },
            "required": ["worktree"],
        },
    });
    let merge_worktree = json!({
        "name": "merge_worktree",
        "description": "Merges a worktree spawn_agent made back into the branch of your folder (or folder's), once the \
                        person allows it; a conflict is aborted, naming the files. Refused while a block works in it \
                        or with changes not committed.",
        "inputSchema": {
            "type": "object",
            "properties": { "worktree": worktree, "folder": repository },
            "required": ["worktree"],
        },
    });
    json!({ "tools": [
        {
            "name": "whoami",
            "description": "Your block on Templum's canvas: its id, kind, title, folder and workspace.",
            "inputSchema": none,
        },
        {
            "name": "list_blocks",
            "description": "Every block in your workspace: its id, kind, title and folder, whether it is \
                            connected to you, and an agent's state.",
            "inputSchema": none,
        },
        {
            "name": "read_terminal",
            "description": "The last lines of a terminal connected to you (its history and screen), or its \
                            last finished command and that command's output.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "lines": { "type": "integer", "minimum": 1, "maximum": MAX_LINES, "default": 200 },
                    "last_command": {
                        "type": "boolean",
                        "default": false,
                        "description": "Only the last finished command and its output (needs the shell's integration).",
                    },
                },
                "required": ["block"],
            },
        },
        {
            "name": "send_to_terminal",
            "description": "Types text into a terminal connected to you, as a paste; Enter after it when asked.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "text": { "type": "string" },
                    "enter": { "type": "boolean", "default": false },
                },
                "required": ["block", "text"],
            },
        },
        {
            "name": "notify",
            "description": "Shows the person a notification from your block.",
            "inputSchema": {
                "type": "object",
                "properties": { "text": { "type": "string" } },
                "required": ["text"],
            },
        },
        {
            "name": "ask_user",
            "description": "Asks the person in a dialog on your block, as AskUserQuestion does: questions, \
                            each with a header, options (label, description, preview) and multiSelect; every \
                            question also gets an Other field, and the person may add a note. Returns \
                            {\"answers\": {question: answer, or a list for multiSelect}, \"note\": …}. Waits \
                            until the person answers: no limit unless you give timeout (seconds) or the \
                            person set one; if your client stops waiting, the answer is typed into your \
                            terminal later. The older question with options (strings) still works and \
                            returns the answer as text. Give why: what you are doing and what the answer \
                            decides, so the person can answer without reading your terminal.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "questions": { "type": "array", "minItems": 1, "items": question },
                    "question": {
                        "type": "string",
                        "description": "The older shape: one question, without questions.",
                    },
                    "options": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "The older shape's answers to offer; none for a text field.",
                    },
                    "why": { "type": "string" },
                    "timeout": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": MAX_WAIT,
                        "description": "Seconds to wait before giving up; none waits until the person answers.",
                    },
                },
            },
        },
        {
            "name": "connect",
            "description": "Asks the person to connect your block to another block of your workspace (list_blocks \
                            shows which aren't), in a balloon on your block. Waits for Allow or Deny; it may take \
                            minutes.",
            "inputSchema": {
                "type": "object",
                "properties": { "block": block, "timeout": seconds(ASK_WAIT) },
                "required": ["block"],
            },
        },
        {
            "name": "delegate",
            "description": "Hands a task to an agent connected to you: Templum types it into its terminal, after \
                            a line saying it is from you and its id (t7), and sends it. Its answer is what it \
                            sends with reply (Templum asks it to), else its last message when its turn ends. With wait, waits \
                            for that answer (it may take minutes); without, says its id once the agent took it \
                            (a few seconds; one still starting gets it once ready), or why it didn't, and its end \
                            comes back typed into your terminal as [task t7 …] with its time, what it spent and \
                            its transcript, as does a dialog it waits on. To the agent on your task, it is a \
                            change to that task, typed at once. Refused while the agent waits for its person or \
                            is on another agent's task.",
            "_meta": always_load,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "task": { "type": "string" },
                    "wait": { "type": "boolean", "default": false },
                    "timeout": seconds(TASK_WAIT),
                },
                "required": ["block", "task"],
            },
        },
        {
            "name": "reply",
            "description": "Answers the task you are on (it began [from <title> (<id>), task t7]): call it once, \
                            when you are done, with your whole answer as text. It ends the task, and the agent that \
                            handed it gets your text now. With block, writes to that connected agent instead: typed \
                            into its terminal after a line saying it is from you.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": {
                        "type": "string",
                        "description": "The agent to write to; leave it out to answer the task you are on.",
                    },
                    "text": { "type": "string" },
                },
                "required": ["text"],
            },
        },
        {
            "name": "wait_for_idle",
            "description": "Waits until an agent connected to you finishes its turn (or the task you handed it), \
                            and returns its last answer; at once when it is idle. It may take minutes.",
            "inputSchema": {
                "type": "object",
                "properties": { "block": block, "timeout": seconds(IDLE_WAIT) },
                "required": ["block"],
            },
        },
        {
            "name": "wait_for",
            "description": "Waits for an event in a block connected to you. idle: the agent finishes its turn \
                            (at once when it is idle), with its last answer. needs_you: the agent waits for its \
                            person, at a permission prompt or a question (at once when it already does). \
                            command_finished: the next command that finishes in the terminal, with its output \
                            (needs the shell's integration). output (with a pattern, which implies it): a line \
                            printed in the terminal that matches the pattern, as \"Ready in\" or \"ERROR|Traceback\", \
                            with the line. attention: the terminal rings or sends a notification. It may take \
                            minutes.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "event": {
                        "type": "string",
                        "enum": ["idle", "needs_you", "command_finished", "output", "attention"],
                    },
                    "pattern": pattern,
                    "lines": lines,
                    "wait": {
                        "type": "boolean",
                        "description": "With a pattern, false answers at once and keeps watching: the first \
                                        match, or the timeout, is typed into your terminal while you work.",
                    },
                    "timeout": seconds(IDLE_WAIT),
                },
                "required": ["block"],
            },
        },
        {
            "name": "spawn_agent",
            "description": "Opens a new agent block beside yours, connected to you: in your folder, the one you \
                            name, or a new Git worktree of that folder's repository (.worktrees/<worktree>, on a \
                            new branch of that name). A prompt is typed once the agent is ready, as a task you \
                            handed it (the result says its id): its end comes back as delegate's does, or \
                            wait_for_idle gets it. Says the new block's id and folder. \
                            With model and effort, its command line starts it on them, and so does every resume.",
            "_meta": always_load,
            "inputSchema": {
                "type": "object",
                "properties": {
                    "agent": {
                        "type": "string",
                        "description": "Which agent: claude, codex, gemini, … or an account's name (Settings ▸ Agents ▸ Accounts). Left out with fork.",
                    },
                    "role": {
                        "type": "string",
                        "description": "A role saved in config.toml ([roles]): its agent, model, effort, flags and instructions; your agent, model and effort win.",
                    },
                    "fork": {
                        "type": "boolean",
                        "description": "A copy of your own agent (Claude Code, Codex), started on your conversation so far; needs a prompt telling it what to do.",
                    },
                    "prompt": { "type": "string" },
                    "folder": {
                        "type": "string",
                        "description": "Where it starts, relative to your folder; yours when left out.",
                    },
                    "worktree": {
                        "type": "string",
                        "description": "A new branch's and worktree's name: letters, digits, - _ . and /.",
                    },
                    "model": {
                        "type": "string",
                        "description": "Its model, as its own --model or -m takes it (provider/model for OpenCode, \
                                        Kilo Code and Pi). Letters, digits, . - _ : and /.",
                    },
                    "effort": {
                        "type": "string",
                        "description": "Its reasoning effort. claude: low, medium, high, xhigh, max. codex: low, medium, \
                                        high, xhigh, max, ultra. copilot and grok: none, minimal, low, medium, high, \
                                        xhigh, max. pi: off, minimal, low, medium, high, xhigh, max. kiro: low, medium, \
                                        high, xhigh, max. cline: none, low, medium, high, xhigh. aider: low, medium, \
                                        high. The others take none.",
                    },
                },
            },
        },
        {
            "name": "list_cards",
            "description": "A board shown by a kanban connected to you: its columns, and each card's id, title, \
                            labels, priority, assignee ((you) on yours), checklist progress, comment and attachment counts. \
                            With card, that card whole: its description, checklist items, attachments (each \
                            one's name, size and path) and comments too.",
            "inputSchema": { "type": "object", "properties": { "board": board, "card": card } },
        },
        {
            "name": "create_card",
            "description": "Adds a card at the end of a column (the board's first when left out) of a board shown \
                            by a kanban connected to you. Says its id. A review's findings: one card each, its \
                            priority by severity, file:line and what fails in its description.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "board": board,
                    "column": column,
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "priority": priority,
                    "for": who,
                    "checklist": items,
                },
                "required": ["title"],
            },
        },
        {
            "name": "edit_card",
            "description": "Sets a card's priority and who it is for, adds checklist items, checks and unchecks \
                            them: one step of the person's Ctrl+Z. Check an item as you finish it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "card": card,
                    "priority": priority,
                    "for": who,
                    "add_items": items,
                    "check": item_ids,
                    "uncheck": item_ids,
                },
                "required": ["card"],
            },
        },
        {
            "name": "move_card",
            "description": "Moves a card to the end of a column of its board. Done with your card: comment \
                            on it first what you did (comment_card), then move it to Review or Done.",
            "inputSchema": {
                "type": "object",
                "properties": { "card": card, "column": column },
                "required": ["card", "column"],
            },
        },
        {
            "name": "comment_card",
            "description": "Adds a comment to a card, signed with your block's id: what you did on it, \
                            and what the next one needs to know, when you finish it.",
            "inputSchema": {
                "type": "object",
                "properties": { "card": card, "text": { "type": "string" } },
                "required": ["card", "text"],
            },
        },
        edit_comment,
        delete_comment,
        {
            "name": "claim_card",
            "description": "Makes you the card's assignee. Refused when another block holds it, or when it is \
                            for another agent.",
            "inputSchema": { "type": "object", "properties": { "card": card }, "required": ["card"] },
        },
        {
            "name": "read_note",
            "description": "The text of the note a note block connected to you shows.",
            "inputSchema": { "type": "object", "properties": { "block": block }, "required": ["block"] },
        },
        {
            "name": "append_note",
            "description": "Adds text at the end of the note a note block connected to you shows, after a blank \
                            line. The person sees it at once and can take it back.",
            "inputSchema": {
                "type": "object",
                "properties": { "block": block, "text": { "type": "string" } },
                "required": ["block", "text"],
            },
        },
        replace_note,
        {
            "name": "read_checklist",
            "description": "The checklist a checklist block connected to you shows: its name and progress, \
                            then each item's id, [x] or [ ], and text.",
            "inputSchema": { "type": "object", "properties": { "block": block }, "required": ["block"] },
        },
        {
            "name": "check_item",
            "description": "Checks an item of the checklist a checklist block connected to you shows; \
                            unchecks it with done false.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "item": { "type": "integer", "description": "An item's id, as read_checklist shows it." },
                    "done": { "type": "boolean", "default": true },
                },
                "required": ["block", "item"],
            },
        },
        {
            "name": "add_items",
            "description": "Adds items at the end of the checklist a checklist block connected to you shows: \
                            one per line with text; a leading - or *, then [ ] or [x], is dropped ([x] adds it \
                            checked). The person sees them at once and can take them back.",
            "inputSchema": {
                "type": "object",
                "properties": { "block": block, "text": { "type": "string" } },
                "required": ["block", "text"],
            },
        },
        edit_item,
        delete_item,
        create_block,
        {
            "name": "open_file",
            "description": "Shows a file, at a line, in the editor connected to you whose folder holds it; with \
                            none, in a new editor beside your block, connected to you. The person's keyboard and \
                            view stay where they are.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "The file, relative to your folder or whole." },
                    "line": { "type": "integer", "minimum": 1, "description": "The line, from 1." },
                },
                "required": ["path"],
            },
        },
        {
            "name": "send_keys",
            "description": "Presses keys in a terminal connected to you, one at a time, as a keyboard does (never \
                            as a paste): to answer an agent's dialog (a permission, a trust question) or drive a \
                            program. Keys: enter, escape, tab, shift+tab, backspace, delete, up, down, left, right, \
                            home, end, page_up, page_down, space, ctrl+<letter>, or one character (2, y). Returns \
                            the last 20 lines of its screen after them. Not your own block, nor an agent on another \
                            agent's task unless it waits for its person.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "keys": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "maxItems": MAX_KEYS,
                    },
                },
                "required": ["block", "keys"],
            },
        },
        propose_plan,
        schedule,
        cancel_schedule,
        remove_worktree,
        merge_worktree,
        {
            "name": "interrupt",
            "description": "Presses Esc once in an agent connected to you while it works or waits for its person, \
                            and ends the task you handed it: its waits get \"Interrupted by you.\" Returns its \
                            screen after. Refused while it is idle or on another agent's task.",
            "inputSchema": { "type": "object", "properties": { "block": block }, "required": ["block"] },
        },
        stop_agent,
        {
            "name": "close_block",
            "description": "Closes a block you opened with spawn_agent and are still connected to, without asking \
                            the person (they bring it back with Ctrl+Z, its agent resumed). Only the person closes \
                            any other block.",
            "inputSchema": { "type": "object", "properties": { "block": block }, "required": ["block"] },
        },
    ]})
}

/// `name` is one of the tools.
pub fn known(name: &str) -> bool {
    list()["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|t| t["name"] == name))
}

/// `name` is a call only the command line makes (`templum wire`): not listed, so agents never
/// see it.
pub fn cli_only(name: &str) -> bool {
    name == "wire"
}

/// The call `name` with its arguments `args`; Err says what is wrong with them.
pub fn parse(name: &str, args: &Value) -> Result<Call, String> {
    let text = |key: &str| {
        args[key]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{name} needs \"{key}\", a string"))
    };
    let flag = |key: &str| match &args[key] {
        Value::Null => Ok(false),
        value => value
            .as_bool()
            .ok_or_else(|| format!("\"{key}\" is true or false")),
    };
    let timeout = || match &args["timeout"] {
        Value::Null => Ok(None),
        value => value
            .as_u64()
            .filter(|n| *n >= 1)
            .map(|n| Some(n.min(MAX_WAIT)))
            .ok_or_else(|| format!("\"timeout\" is a number of seconds from 1 to {MAX_WAIT}")),
    };
    let seconds = |default: u64| timeout().map(|t| t.unwrap_or(default));
    // A blank string is no value: `"worktree": ""` names no worktree, `"prompt": " "` no task.
    let optional = |key: &str| match &args[key] {
        Value::Null => Ok(None),
        value => value
            .as_str()
            .map(|s| Some(s.to_string()).filter(|s| !s.trim().is_empty()))
            .ok_or_else(|| format!("\"{key}\" is a string")),
    };
    // Boards, columns and cards may come as numbers or as strings.
    let named = |key: &str| match &args[key] {
        Value::Null => Ok(None),
        Value::Number(n) => Ok(Some(n.to_string())),
        Value::String(s) => Ok(Some(s.trim().to_string()).filter(|s| !s.is_empty())),
        _ => Err(format!("\"{key}\" is a name or an id")),
    };
    // `shown_by`: the tool that lists them.
    let id = |key: &str, shown_by: &str| -> Result<Option<u64>, String> {
        let bad = || format!("\"{key}\" is an id, as {shown_by} shows it");
        match &args[key] {
            Value::Null => Ok(None),
            Value::Number(n) => n.as_u64().map(Some).ok_or_else(bad),
            Value::String(s) => s.trim().parse().map(Some).map_err(|_| bad()),
            _ => Err(bad()),
        }
    };
    let card =
        || id("card", "list_cards")?.ok_or_else(|| format!("{name} needs \"card\", a card's id"));
    let comment = || {
        id("comment", "list_cards with card")?
            .ok_or_else(|| format!("{name} needs \"comment\", a comment's number"))
    };
    let priority = || match optional("priority")? {
        Some(p) => {
            let p = p.trim().to_lowercase();
            ["low", "medium", "high", "none"]
                .contains(&p.as_str())
                .then_some(Some(p))
                .ok_or_else(|| "\"priority\" is low, medium, high or none".to_string())
        }
        None => Ok(None),
    };
    // One item's id (a number or its text), or a list of them.
    let item_ids = |key: &str| {
        let one = |v: &Value| match v {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.trim().parse().ok(),
            _ => None,
        };
        match &args[key] {
            Value::Null => Some(Vec::new()),
            Value::Array(ids) => ids.iter().map(one).collect(),
            v => one(v).map(|id| vec![id]),
        }
        .ok_or_else(|| {
            format!("\"{key}\" is an item's id, or a list of them, as list_cards shows them")
        })
    };
    Ok(match name {
        "whoami" => Call::Whoami,
        "list_blocks" => Call::ListBlocks,
        "read_terminal" => Call::ReadTerminal {
            block: text("block")?,
            lines: match &args["lines"] {
                Value::Null => 200,
                value => value
                    .as_u64()
                    .filter(|n| *n >= 1)
                    .map(|n| (n as usize).min(MAX_LINES))
                    .ok_or_else(|| format!("\"lines\" is a number from 1 to {MAX_LINES}"))?,
            },
            last_command: flag("last_command")?,
        },
        "send_to_terminal" => Call::SendToTerminal {
            block: text("block")?,
            text: text("text")?,
            enter: flag("enter")?,
        },
        "notify" => Call::Notify {
            text: text("text")?,
        },
        "ask_user" => Call::AskUser {
            ask: crate::ask::parse(args)?,
            timeout: timeout()?,
        },
        "connect" => Call::Connect {
            block: text("block")?,
            timeout: seconds(ASK_WAIT)?,
        },
        "delegate" => Call::Delegate {
            block: text("block")?,
            task: text("task")?,
            wait: flag("wait")?,
            timeout: seconds(TASK_WAIT)?,
        },
        "reply" => Call::ReplyTo {
            block: optional("block")?,
            text: text("text")?,
        },
        "wait_for_idle" => Call::WaitForIdle {
            block: text("block")?,
            timeout: seconds(IDLE_WAIT)?,
        },
        "wait_for" => {
            // A blank pattern goes on, for the app to say so.
            let pattern = match &args["pattern"] {
                Value::Null => None,
                value => Some(
                    value
                        .as_str()
                        .ok_or("\"pattern\" is a string, a regular expression")?
                        .to_string(),
                ),
            };
            let lines = match &args["lines"] {
                Value::Null => None,
                value => Some(
                    value
                        .as_u64()
                        .map(|n| (n as usize).min(MAX_LINES))
                        .ok_or_else(|| format!("\"lines\" is a number from 0 to {MAX_LINES}"))?,
                ),
            };
            if lines.is_some() && pattern.is_none() {
                return Err("\"lines\" goes with a pattern".into());
            }
            let event = match (args["event"].as_str(), &args["event"]) {
                (_, Value::Null) if pattern.is_some() => Event::Output,
                (Some("output"), _) if pattern.is_some() => Event::Output,
                (Some("idle"), _) => Event::Idle,
                (Some("needs_you"), _) => Event::NeedsYou,
                (Some("command_finished"), _) => Event::CommandFinished,
                (Some("attention"), _) => Event::Attention,
                _ => {
                    return Err(format!(
                        "{name} needs \"event\": idle, needs_you, command_finished, output (with a \
                         pattern) or attention"
                    ));
                }
            };
            if pattern.is_some() && event != Event::Output {
                return Err("\"pattern\" goes with event output".into());
            }
            let wait = match &args["wait"] {
                Value::Null => true,
                value => value.as_bool().ok_or("\"wait\" is true or false")?,
            };
            if !wait && pattern.is_none() {
                return Err("\"wait\" goes with a pattern".into());
            }
            Call::WaitFor {
                block: text("block")?,
                event,
                timeout: seconds(IDLE_WAIT)?,
                pattern,
                lines: lines.unwrap_or(0),
                wait,
            }
        }
        "spawn_agent" => {
            let fork = flag("fork")?;
            let agent = optional("agent")?;
            let role = optional("role")?;
            if agent.is_none() && role.is_none() && !fork {
                return Err(format!("{name} needs \"agent\", a role, or fork"));
            }
            let prompt = optional("prompt")?;
            if fork && prompt.is_none() {
                return Err("A fork needs a prompt: it tells the copy what to do.".into());
            }
            Call::SpawnAgent(Spawn {
                agent,
                prompt,
                folder: optional("folder")?,
                worktree: optional("worktree")?,
                model: optional("model")?,
                effort: optional("effort")?,
                fork,
                role,
                args: None,
            })
        }
        "list_cards" => Call::ListCards {
            board: named("board")?,
            card: id("card", "list_cards")?,
        },
        "create_block" => {
            let kind = match args["kind"].as_str() {
                Some("note") => BlockKind::Note,
                Some("checklist") => BlockKind::Checklist,
                Some("terminal") => BlockKind::Terminal,
                _ => return Err("kind is note, checklist or terminal".into()),
            };
            let given = |keys: &[&str]| keys.iter().any(|k| !args[*k].is_null());
            if kind != BlockKind::Note && given(&["text"]) {
                return Err("\"text\" is for a note".into());
            }
            if kind != BlockKind::Checklist && given(&["name", "items"]) {
                return Err("\"name\" and \"items\" are for a checklist".into());
            }
            if kind != BlockKind::Terminal && given(&["folder", "command"]) {
                return Err("\"folder\" and \"command\" are for a terminal".into());
            }
            let command = optional("command")?;
            if command
                .as_ref()
                .is_some_and(|c| c.len() > MAX_COMMAND || c.contains(['\n', '\r']))
            {
                return Err(format!(
                    "\"command\" is one line of at most {MAX_COMMAND} bytes"
                ));
            }
            Call::CreateBlock(NewBlockArgs {
                kind,
                text: optional("text")?,
                name: optional("name")?,
                items: optional("items")?,
                folder: optional("folder")?,
                command,
            })
        }
        "create_card" => Call::CreateCard(NewCard {
            board: named("board")?,
            column: named("column")?,
            title: text("title")?,
            description: optional("description")?,
            priority: priority()?,
            who: optional("for")?,
            checklist: optional("checklist")?,
        }),
        "edit_card" => {
            let edit = CardEdit {
                card: card()?,
                priority: priority()?,
                who: optional("for")?,
                add_items: optional("add_items")?,
                check: item_ids("check")?,
                uncheck: item_ids("uncheck")?,
            };
            if edit.priority.is_none()
                && edit.who.is_none()
                && edit.add_items.is_none()
                && edit.check.is_empty()
                && edit.uncheck.is_empty()
            {
                return Err(
                    "edit_card needs something to change: priority, for, add_items, check or uncheck"
                        .into(),
                );
            }
            Call::EditCard(edit)
        }
        "move_card" => Call::MoveCard {
            card: card()?,
            column: named("column")?
                .ok_or_else(|| format!("{name} needs \"column\", a title or an id"))?,
        },
        "comment_card" => Call::CommentCard {
            card: card()?,
            text: text("text")?,
        },
        "edit_comment" => Call::EditComment {
            card: card()?,
            comment: comment()?,
            text: text("text")?,
        },
        "delete_comment" => Call::DeleteComment {
            card: card()?,
            comment: comment()?,
        },
        "claim_card" => Call::ClaimCard { card: card()? },
        "read_note" => Call::ReadNote {
            block: text("block")?,
        },
        "append_note" => Call::AppendNote {
            block: text("block")?,
            text: text("text")?,
        },
        "replace_note" => Call::ReplaceNote {
            block: text("block")?,
            text: text("text")?,
            old: match &args["old"] {
                Value::Null => None,
                value => Some(
                    value
                        .as_str()
                        .filter(|o| !o.is_empty())
                        .ok_or("\"old\" is the passage to replace, as in the note")?
                        .to_string(),
                ),
            },
        },
        "read_checklist" => Call::ReadChecklist {
            block: text("block")?,
        },
        "check_item" => Call::CheckItem {
            block: text("block")?,
            item: id("item", "read_checklist")?
                .ok_or_else(|| format!("{name} needs \"item\", an item's id"))?,
            done: match &args["done"] {
                Value::Null => true,
                value => value
                    .as_bool()
                    .ok_or_else(|| "\"done\" is true or false".to_string())?,
            },
        },
        "add_items" => Call::AddItems {
            block: text("block")?,
            text: text("text")?,
        },
        "edit_item" => Call::EditItem {
            block: text("block")?,
            item: id("item", "read_checklist")?
                .ok_or_else(|| format!("{name} needs \"item\", an item's id"))?,
            text: text("text")?,
        },
        "delete_item" => Call::DeleteItem {
            block: text("block")?,
            item: id("item", "read_checklist")?
                .ok_or_else(|| format!("{name} needs \"item\", an item's id"))?,
        },
        "open_file" => Call::OpenFile {
            path: text("path")?,
            line: match &args["line"] {
                Value::Null => None,
                value => Some(
                    value
                        .as_u64()
                        .filter(|n| *n >= 1)
                        .map(|n| n.min(u64::from(u32::MAX)) as u32)
                        .ok_or_else(|| "\"line\" is a line number, from 1".to_string())?,
                ),
            },
        },
        "send_keys" => Call::SendKeys {
            block: text("block")?,
            keys: match &args["keys"] {
                Value::Array(keys) if (1..=MAX_KEYS).contains(&keys.len()) => keys
                    .iter()
                    .map(|k| k.as_str().map(str::to_string))
                    .collect::<Option<_>>(),
                _ => None,
            }
            .ok_or_else(|| {
                format!(
                    "{name} needs \"keys\", a list of 1 to {MAX_KEYS} key names (enter, escape, ctrl+c, 2…)"
                )
            })?,
        },
        "interrupt" => Call::Interrupt {
            block: text("block")?,
        },
        "propose_plan" => {
            let plan = text("plan")?;
            if plan.trim().is_empty() {
                return Err("\"plan\" is blank".into());
            }
            if plan.len() > crate::ask::PLAN_MAX {
                return Err("\"plan\" is longer than 256 KB".into());
            }
            Call::ProposePlan {
                plan,
                title: optional("title")?,
                timeout: timeout()?,
            }
        }
        "schedule" => {
            let minutes = |key: &str| match &args[key] {
                Value::Null => Ok(None),
                value => value
                    .as_u64()
                    .filter(|m| (1..=MAX_MINUTES).contains(m))
                    .map(Some)
                    .ok_or_else(|| {
                        format!("\"after\" and \"every\" are minutes from 1 to {MAX_MINUTES}")
                    }),
            };
            let at = match &args["at"] {
                Value::Null => None,
                value => Some(
                    value
                        .as_str()
                        .and_then(|t| {
                            let (h, m) = t.trim().split_once(':')?;
                            let (hour, minute) = (h.parse::<u8>().ok()?, m.parse::<u8>().ok()?);
                            (hour < 24 && minute < 60 && m.len() == 2).then_some((hour, minute))
                        })
                        .ok_or("\"at\" is HH:MM, 00:00 to 23:59")?,
                ),
            };
            let (after, every) = (minutes("after")?, minutes("every")?);
            let when = match (at, after) {
                (Some(_), Some(_)) => return Err("\"at\" and \"after\" go apart".into()),
                (Some((hour, minute)), None) => When::At { hour, minute },
                (None, Some(minutes)) => When::After { minutes },
                (None, None) if every.is_some() => When::None,
                (None, None) => return Err(format!("{name} needs at, after or every")),
            };
            let (block, said) = (text("block")?, text("text")?);
            if said.trim().is_empty() {
                return Err("\"text\" is blank".into());
            }
            Call::Schedule {
                block,
                text: said,
                when,
                every,
            }
        }
        "cancel_schedule" => Call::CancelSchedule {
            schedule: named("schedule")?
                .ok_or_else(|| format!("{name} needs \"schedule\", its id (s3)"))?,
        },
        "remove_worktree" => Call::RemoveWorktree {
            worktree: text("worktree")?,
            folder: optional("folder")?,
            discard: flag("discard")?,
        },
        "merge_worktree" => Call::MergeWorktree {
            worktree: text("worktree")?,
            folder: optional("folder")?,
        },
        "stop_agent" => Call::StopAgent {
            block: text("block")?,
        },
        "close_block" => Call::CloseBlock {
            block: text("block")?,
        },
        "wire" => Call::Wire {
            a: text("a")?,
            b: text("b")?,
        },
        _ => return Err(format!("Unknown tool: {name}")),
    })
}

/// A `tools/call` answer, as a line.
pub fn tool_result(id: &Value, reply: &Reply) -> String {
    let mut result = json!({ "content": [{ "type": "text", "text": reply.text }] });
    if reply.is_error {
        result["isError"] = json!(true);
    }
    crate::rpc::result(id, result)
}

/// The end of `text`, at most `max` bytes, cut where a character starts.
pub fn tail(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_listed_parses_its_example() {
        let examples = [
            ("whoami", json!({})),
            ("list_blocks", json!({})),
            (
                "read_terminal",
                json!({ "block": "b7k2m", "lines": 10, "last_command": true }),
            ),
            (
                "send_to_terminal",
                json!({ "block": "b7k2m", "text": "ls", "enter": true }),
            ),
            ("notify", json!({ "text": "done" })),
            (
                "ask_user",
                json!({
                    "questions": [
                        {
                            "question": "Which database should the service use?",
                            "header": "Database",
                            "options": [
                                { "label": "Postgres", "description": "What production runs",
                                  "preview": "DATABASE_URL=postgres://…\npool = 10" },
                                { "label": "SQLite", "description": "One file, no server" }
                            ],
                            "multiSelect": false
                        },
                        {
                            "question": "Which features go in the first release?",
                            "header": "Features",
                            "options": [{ "label": "Auth" }, { "label": "Search" }, { "label": "Export" }],
                            "multiSelect": true
                        }
                    ],
                    "timeout": 600
                }),
            ),
            ("connect", json!({ "block": "k9x1d", "timeout": 60 })),
            (
                "delegate",
                json!({ "block": "k9x1d", "task": "fix the tests", "wait": true, "timeout": 900 }),
            ),
            ("reply", json!({ "block": "b7k2m", "text": "Done." })),
            ("wait_for_idle", json!({ "block": "k9x1d", "timeout": 60 })),
            (
                "wait_for",
                json!({ "block": "k9x1d", "event": "needs_you", "timeout": 60 }),
            ),
            (
                "spawn_agent",
                json!({ "agent": "codex", "prompt": "fix the tests", "folder": "../api", "worktree": "fix-tests",
                        "model": "gpt-5.6-luna", "effort": "high" }),
            ),
            ("list_cards", json!({ "board": "Sprint", "card": 12 })),
            (
                "create_card",
                json!({ "board": "1", "column": "To do", "title": "Fix login", "description": "It 500s.",
                        "priority": "high", "for": "codex", "checklist": "- [ ] Build" }),
            ),
            (
                "edit_card",
                json!({ "card": 12, "priority": "low", "for": "anyone", "add_items": "Docs", "check": 13,
                        "uncheck": [14] }),
            ),
            ("move_card", json!({ "card": 12, "column": "Doing" })),
            ("comment_card", json!({ "card": 12, "text": "On it." })),
            (
                "edit_comment",
                json!({ "card": 12, "comment": 41, "text": "Fixed the parser." }),
            ),
            ("delete_comment", json!({ "card": 12, "comment": "41" })),
            ("claim_card", json!({ "card": "12" })),
            ("read_note", json!({ "block": "4qm2a" })),
            (
                "append_note",
                json!({ "block": "4qm2a", "text": "Next: the tests." }),
            ),
            (
                "replace_note",
                json!({ "block": "4qm2a", "old": "- ship", "text": "- ship it" }),
            ),
            ("read_checklist", json!({ "block": "4qm2a" })),
            (
                "check_item",
                json!({ "block": "4qm2a", "item": 13, "done": false }),
            ),
            (
                "add_items",
                json!({ "block": "4qm2a", "text": "- [ ] Build\n- [ ] Test" }),
            ),
            (
                "edit_item",
                json!({ "block": "4qm2a", "item": 41, "text": "Build" }),
            ),
            ("delete_item", json!({ "block": "4qm2a", "item": "41" })),
            (
                "create_block",
                json!({ "kind": "checklist", "name": "Plan", "items": "- [ ] Read\n- [ ] Build" }),
            ),
            ("open_file", json!({ "path": "src/main.rs", "line": 12 })),
            (
                "send_keys",
                json!({ "block": "k9x1d", "keys": ["2", "enter"] }),
            ),
            (
                "propose_plan",
                json!({ "plan": "## Steps\n1. Replace the reader threads", "title": "Move the bus to tokio" }),
            ),
            (
                "schedule",
                json!({ "block": "k9x1d", "text": "check the build", "at": "15:00", "every": 10 }),
            ),
            ("cancel_schedule", json!({ "schedule": "s3" })),
            (
                "remove_worktree",
                json!({ "worktree": "fix-login", "folder": "../api", "discard": true }),
            ),
            ("merge_worktree", json!({ "worktree": "fix-login" })),
            ("interrupt", json!({ "block": "k9x1d" })),
            ("stop_agent", json!({ "block": "k9x1d" })),
            ("close_block", json!({ "block": "k9x1d" })),
        ];
        let listed: Vec<String> = list()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            listed,
            examples
                .iter()
                .map(|(n, _)| n.to_string())
                .collect::<Vec<_>>()
        );
        for (name, args) in &examples {
            assert!(known(name));
            assert!(parse(name, args).is_ok(), "{name}");
        }
        assert_eq!(
            parse("read_terminal", &json!({ "block": "b" })),
            Ok(Call::ReadTerminal {
                block: "b".into(),
                lines: 200,
                last_command: false
            })
        );
        assert_eq!(
            parse("read_terminal", &json!({ "block": "b", "lines": 99_999 })),
            Ok(Call::ReadTerminal {
                block: "b".into(),
                lines: MAX_LINES,
                last_command: false
            })
        );
        assert_eq!(
            parse("send_to_terminal", &json!({ "block": "b", "text": "x" })),
            Ok(Call::SendToTerminal {
                block: "b".into(),
                text: "x".into(),
                enter: false
            })
        );
    }

    /// `reply` without a block answers the task its caller is on.
    #[test]
    fn reply_needs_only_its_text() {
        assert_eq!(
            parse("reply", &json!({ "text": "Line 42." })),
            Ok(Call::ReplyTo {
                block: None,
                text: "Line 42.".into()
            })
        );
        assert_eq!(
            parse("reply", &json!({ "block": "k9x1d", "text": "Line 42." })),
            Ok(Call::ReplyTo {
                block: Some("k9x1d".into()),
                text: "Line 42.".into()
            })
        );
        let tools = list();
        let reply = tools["tools"]
            .as_array()
            .and_then(|t| t.iter().find(|t| t["name"] == "reply"))
            .expect("reply is listed");
        assert_eq!(reply["inputSchema"]["required"], json!(["text"]));
    }

    #[test]
    fn the_tools_that_wait_have_their_defaults_and_limits() {
        let why = crate::ask::parse(&json!({ "question": "Why?" })).unwrap();
        assert_eq!(
            parse("ask_user", &json!({ "question": "Why?" })),
            Ok(Call::AskUser {
                ask: why.clone(),
                timeout: None
            }),
            "a question waits until it is answered"
        );
        assert_eq!(
            parse(
                "ask_user",
                &json!({ "question": "Why?", "timeout": 99_999 })
            ),
            Ok(Call::AskUser {
                ask: why,
                timeout: Some(MAX_WAIT)
            })
        );
        assert_eq!(
            parse("connect", &json!({ "block": "b" })),
            Ok(Call::Connect {
                block: "b".into(),
                timeout: ASK_WAIT
            })
        );
        assert_eq!(
            parse("delegate", &json!({ "block": "b", "task": "t" })),
            Ok(Call::Delegate {
                block: "b".into(),
                task: "t".into(),
                wait: false,
                timeout: TASK_WAIT
            })
        );
        assert_eq!(
            parse("wait_for_idle", &json!({ "block": "b" })),
            Ok(Call::WaitForIdle {
                block: "b".into(),
                timeout: IDLE_WAIT
            })
        );
        assert_eq!(
            parse(
                "wait_for",
                &json!({ "block": "b", "event": "command_finished" })
            ),
            Ok(Call::WaitFor {
                block: "b".into(),
                event: Event::CommandFinished,
                timeout: IDLE_WAIT,
                pattern: None,
                lines: 0,
                wait: true
            })
        );
        assert_eq!(
            parse(
                "wait_for",
                &json!({ "block": "b", "event": "idle", "timeout": 99_999 })
            ),
            Ok(Call::WaitFor {
                block: "b".into(),
                event: Event::Idle,
                timeout: MAX_WAIT,
                pattern: None,
                lines: 0,
                wait: true
            })
        );
        assert!(parse("reply", &json!({ "block": "b" })).is_err(), "no text");
        assert_eq!(
            parse("spawn_agent", &json!({ "agent": "claude" })),
            Ok(Call::SpawnAgent(Spawn {
                agent: Some("claude".into()),
                prompt: None,
                folder: None,
                worktree: None,
                model: None,
                effort: None,
                fork: false,
                role: None,
                args: None
            }))
        );
        assert_eq!(
            parse("spawn_agent", &json!({ "fork": true, "prompt": "x" })),
            Ok(Call::SpawnAgent(Spawn {
                agent: None,
                prompt: Some("x".into()),
                folder: None,
                worktree: None,
                model: None,
                effort: None,
                fork: true,
                role: None,
                args: None
            }))
        );
        assert_eq!(
            parse("spawn_agent", &json!({})),
            Err("spawn_agent needs \"agent\", a role, or fork".into())
        );
        let Ok(Call::SpawnAgent(by_role)) =
            parse("spawn_agent", &json!({ "role": "reviewer", "prompt": "x" }))
        else {
            panic!("a role is enough")
        };
        assert_eq!(
            (by_role.agent, by_role.role.as_deref(), by_role.args),
            (None, Some("reviewer"), None)
        );
        assert_eq!(
            parse("spawn_agent", &json!({ "fork": true })),
            Err("A fork needs a prompt: it tells the copy what to do.".into())
        );
        assert_eq!(
            parse(
                "spawn_agent",
                &json!({ "agent": "claude", "prompt": "", "folder": "", "worktree": "", "model": "", "effort": "" })
            ),
            parse("spawn_agent", &json!({ "agent": "claude" })),
            "an empty string is no value"
        );
        assert_eq!(
            parse(
                "spawn_agent",
                &json!({ "agent": "claude", "prompt": " \n", "folder": " ", "worktree": "\t", "model": " ", "effort": "\t" })
            ),
            parse("spawn_agent", &json!({ "agent": "claude" })),
            "nor is a blank one: a blank prompt typed is no task"
        );
        assert!(parse("spawn_agent", &json!({ "agent": "claude", "prompt": 3 })).is_err());
        for bad in [
            json!({}),
            json!({ "question": "Why?", "options": "yes" }),
            json!({ "question": "Why?", "options": [1] }),
            json!({ "question": "Why?", "timeout": 0 }),
        ] {
            assert!(parse("ask_user", &bad).is_err(), "{bad}");
        }
    }

    /// Both shapes, neither required: a client that checks a call against the schema still sends
    /// today's. Options are objects only (no `anyOf`), though strings are taken.
    #[test]
    fn ask_users_schema_lists_both_shapes() {
        let tools = list();
        let ask = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "ask_user")
            .unwrap();
        let schema = &ask["inputSchema"];
        assert_eq!(schema["required"], Value::Null);
        for key in ["questions", "question", "options", "why", "timeout"] {
            assert!(schema["properties"][key].is_object(), "{key}");
        }
        let question = &schema["properties"]["questions"]["items"]["properties"];
        for key in ["question", "header", "options", "multiSelect"] {
            assert!(question[key].is_object(), "{key}");
        }
        assert_eq!(question["options"]["items"]["type"], "object");
        assert!(
            ask["description"]
                .as_str()
                .unwrap()
                .starts_with("Asks the person in a dialog on your block, as AskUserQuestion does")
        );
    }

    /// No schema has an `anyOf`, which not every client's model API takes.
    #[test]
    fn no_tool_schema_has_an_any_of() {
        let tools = list().to_string();
        assert!(!tools.contains("anyOf"), "{tools}");
    }

    /// The events tools/list offers are the ones wait_for takes; any other is refused, naming them.
    #[test]
    fn wait_for_takes_the_five_events_it_lists() {
        let tools = list();
        let listed = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "wait_for")
            .unwrap();
        assert_eq!(listed["inputSchema"]["required"], json!(["block"]));
        let events: Vec<Event> = listed["inputSchema"]["properties"]["event"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| {
                let mut args = json!({ "block": "b", "event": name });
                if name == "output" {
                    args["pattern"] = json!("x");
                }
                match parse("wait_for", &args) {
                    Ok(Call::WaitFor { event, .. }) => event,
                    other => panic!("{name}: {other:?}"),
                }
            })
            .collect();
        assert_eq!(
            events,
            [
                Event::Idle,
                Event::NeedsYou,
                Event::CommandFinished,
                Event::Output,
                Event::Attention
            ]
        );
        assert_eq!(
            parse(
                "wait_for",
                &json!({ "block": "t4q8p", "pattern": "Ready in", "lines": 20 })
            ),
            Ok(Call::WaitFor {
                block: "t4q8p".into(),
                event: Event::Output,
                timeout: IDLE_WAIT,
                pattern: Some("Ready in".into()),
                lines: 20,
                wait: true
            })
        );
        assert!(matches!(
            parse(
                "wait_for",
                &json!({ "block": "t4q8p", "pattern": "ERROR", "wait": false })
            ),
            Ok(Call::WaitFor { wait: false, .. })
        ));
        assert_eq!(
            parse(
                "wait_for",
                &json!({ "block": "t4q8p", "event": "idle", "wait": false })
            ),
            Err("\"wait\" goes with a pattern".into())
        );
        for (bad, why) in [
            (
                json!({ "block": "t4q8p", "event": "idle", "pattern": "x" }),
                "\"pattern\" goes with event output",
            ),
            (
                json!({ "block": "t4q8p", "event": "output" }),
                "wait_for needs \"event\": idle, needs_you, command_finished, output (with a pattern) or attention",
            ),
            (
                json!({ "block": "t4q8p", "lines": 5 }),
                "\"lines\" goes with a pattern",
            ),
        ] {
            assert_eq!(parse("wait_for", &bad), Err(why.to_string()), "{bad}");
        }
        for bad in [
            json!({ "block": "b" }),
            json!({ "block": "b", "event": "done" }),
            json!({ "block": "b", "event": 1 }),
        ] {
            let err = parse("wait_for", &bad).unwrap_err();
            assert!(
                err.contains("\"event\"") && err.contains("needs_you") && err.contains("attention"),
                "{err}"
            );
        }
        assert!(
            parse(
                "wait_for",
                &json!({ "block": "b", "event": "idle", "timeout": 0 })
            )
            .is_err()
        );
    }

    #[test]
    fn a_comment_is_edited_and_deleted_by_its_number() {
        assert_eq!(
            parse(
                "edit_comment",
                &json!({ "card": "12", "comment": 41, "text": "Fixed." })
            ),
            Ok(Call::EditComment {
                card: 12,
                comment: 41,
                text: "Fixed.".into()
            })
        );
        assert_eq!(
            parse("delete_comment", &json!({ "card": 12, "comment": " 41 " })),
            Ok(Call::DeleteComment {
                card: 12,
                comment: 41
            })
        );
        for (tool, bad) in [
            ("edit_comment", json!({ "card": 12, "text": "x" })),
            ("edit_comment", json!({ "card": 12, "comment": 41 })),
            ("edit_comment", json!({ "comment": 41, "text": "x" })),
            (
                "edit_comment",
                json!({ "card": 12, "comment": "last", "text": "x" }),
            ),
            ("delete_comment", json!({ "card": 12 })),
            ("delete_comment", json!({ "card": 12, "comment": -1 })),
        ] {
            assert!(parse(tool, &bad).is_err(), "{tool} {bad}");
        }
        let err = parse("delete_comment", &json!({ "card": 12 })).unwrap_err();
        assert!(err.contains("\"comment\""), "{err}");
    }

    #[test]
    fn the_note_and_checklist_fixes_take_their_arguments() {
        assert_eq!(
            parse("replace_note", &json!({ "block": "b", "text": "All new" })),
            Ok(Call::ReplaceNote {
                block: "b".into(),
                text: "All new".into(),
                old: None
            })
        );
        assert_eq!(
            parse(
                "edit_item",
                &json!({ "block": "b", "item": "41", "text": "Build" })
            ),
            Ok(Call::EditItem {
                block: "b".into(),
                item: 41,
                text: "Build".into()
            })
        );
        assert_eq!(
            parse("delete_item", &json!({ "block": "b", "item": 41 })),
            Ok(Call::DeleteItem {
                block: "b".into(),
                item: 41
            })
        );
        assert!(parse("replace_note", &json!({ "block": "b" })).is_err());
        assert!(parse("edit_item", &json!({ "block": "b", "text": "x" })).is_err());
        assert!(parse("delete_item", &json!({ "block": "b" })).is_err());
    }

    #[test]
    fn propose_plan_takes_a_plan_and_refuses_a_blank_or_huge_one() {
        assert_eq!(
            parse(
                "propose_plan",
                &json!({ "plan": "## Steps", "title": "Move the bus" })
            ),
            Ok(Call::ProposePlan {
                plan: "## Steps".into(),
                title: Some("Move the bus".into()),
                timeout: None
            })
        );
        assert_eq!(
            parse("propose_plan", &json!({ "plan": " \n" })),
            Err("\"plan\" is blank".into())
        );
        assert_eq!(
            parse(
                "propose_plan",
                &json!({ "plan": "x".repeat(256 * 1024 + 1) })
            ),
            Err("\"plan\" is longer than 256 KB".into())
        );
    }

    /// schedule takes at, after or every, at with every or after with every, and names a bad one.
    #[test]
    fn schedule_takes_a_time_and_refuses_a_bad_one() {
        let at = |when: Value| {
            let mut args = json!({ "block": "b", "text": "go" });
            for (k, v) in when.as_object().unwrap() {
                args[k] = v.clone();
            }
            parse("schedule", &args)
        };
        let ok = |when, every| {
            Ok(Call::Schedule {
                block: "b".into(),
                text: "go".into(),
                when,
                every,
            })
        };
        assert_eq!(
            at(json!({ "at": "09:05" })),
            ok(When::At { hour: 9, minute: 5 }, None)
        );
        assert_eq!(
            at(json!({ "after": 30, "every": 10 })),
            ok(When::After { minutes: 30 }, Some(10))
        );
        assert_eq!(at(json!({ "every": 1 })), ok(When::None, Some(1)));
        for (when, why) in [
            (json!({}), "schedule needs at, after or every"),
            (
                json!({ "at": "09:05", "after": 3 }),
                "\"at\" and \"after\" go apart",
            ),
            (json!({ "at": "24:00" }), "\"at\" is HH:MM, 00:00 to 23:59"),
            (json!({ "at": "9am" }), "\"at\" is HH:MM, 00:00 to 23:59"),
            (
                json!({ "after": 0 }),
                "\"after\" and \"every\" are minutes from 1 to 10080",
            ),
            (
                json!({ "every": 10081 }),
                "\"after\" and \"every\" are minutes from 1 to 10080",
            ),
        ] {
            assert_eq!(at(when.clone()), Err(why.to_string()), "{when}");
        }
        assert_eq!(
            parse(
                "schedule",
                &json!({ "block": "b", "text": " ", "after": 1 })
            ),
            Err("\"text\" is blank".into())
        );
        assert_eq!(
            parse("cancel_schedule", &json!({ "schedule": 3 })),
            Ok(Call::CancelSchedule {
                schedule: "3".into()
            })
        );
    }

    #[test]
    fn the_worktree_tools_take_a_name_and_a_folder() {
        assert_eq!(
            parse("remove_worktree", &json!({ "worktree": "w" })),
            Ok(Call::RemoveWorktree {
                worktree: "w".into(),
                folder: None,
                discard: false
            })
        );
        assert_eq!(
            parse(
                "merge_worktree",
                &json!({ "worktree": "w", "folder": "api" })
            ),
            Ok(Call::MergeWorktree {
                worktree: "w".into(),
                folder: Some("api".into())
            })
        );
        assert!(parse("merge_worktree", &json!({})).is_err());
    }

    /// create_block takes what its kind does, and names what it doesn't.
    #[test]
    fn create_block_takes_what_its_kind_does() {
        let none = NewBlockArgs {
            kind: BlockKind::Note,
            text: None,
            name: None,
            items: None,
            folder: None,
            command: None,
        };
        assert_eq!(
            parse("create_block", &json!({ "kind": "note", "text": "notes" })),
            Ok(Call::CreateBlock(NewBlockArgs {
                text: Some("notes".into()),
                ..none.clone()
            }))
        );
        assert_eq!(
            parse(
                "create_block",
                &json!({ "kind": "checklist", "name": "Plan", "items": "- [ ] Read" })
            ),
            Ok(Call::CreateBlock(NewBlockArgs {
                kind: BlockKind::Checklist,
                name: Some("Plan".into()),
                items: Some("- [ ] Read".into()),
                ..none.clone()
            }))
        );
        assert_eq!(
            parse(
                "create_block",
                &json!({ "kind": "terminal", "folder": "web", "command": "npm run dev" })
            ),
            Ok(Call::CreateBlock(NewBlockArgs {
                kind: BlockKind::Terminal,
                folder: Some("web".into()),
                command: Some("npm run dev".into()),
                ..none
            }))
        );
        let kinds = "kind is note, checklist or terminal";
        for (args, why) in [
            (json!({}), kinds),
            (json!({ "kind": "kanban" }), kinds),
            (
                json!({ "kind": "checklist", "text": "x" }),
                "\"text\" is for a note",
            ),
            (
                json!({ "kind": "note", "items": "x" }),
                "\"name\" and \"items\" are for a checklist",
            ),
            (
                json!({ "kind": "terminal", "name": "x" }),
                "\"name\" and \"items\" are for a checklist",
            ),
            (
                json!({ "kind": "note", "command": "x" }),
                "\"folder\" and \"command\" are for a terminal",
            ),
            (
                json!({ "kind": "terminal", "command": "a\nb" }),
                "\"command\" is one line of at most 4096 bytes",
            ),
            (
                json!({ "kind": "terminal", "command": "a".repeat(4097) }),
                "\"command\" is one line of at most 4096 bytes",
            ),
        ] {
            assert_eq!(parse("create_block", &args), Err(why.to_string()), "{args}");
        }
    }

    /// send_keys takes 1 to 16 keys, as strings; their names are the app's to check.
    #[test]
    fn send_keys_takes_one_to_sixteen_keys() {
        assert_eq!(
            parse("send_keys", &json!({ "block": "b", "keys": ["ctrl+c"] })),
            Ok(Call::SendKeys {
                block: "b".into(),
                keys: vec!["ctrl+c".into()]
            })
        );
        let (full, over) = (vec!["y"; MAX_KEYS], vec!["y"; MAX_KEYS + 1]);
        assert!(parse("send_keys", &json!({ "block": "b", "keys": full })).is_ok());
        for bad in [
            json!({ "block": "b" }),
            json!({ "block": "b", "keys": [] }),
            json!({ "block": "b", "keys": over }),
            json!({ "block": "b", "keys": "enter" }),
            json!({ "block": "b", "keys": [13] }),
        ] {
            let err = parse("send_keys", &bad).unwrap_err();
            assert!(
                err.contains("\"keys\"") && err.contains("16"),
                "{bad}: {err}"
            );
        }
        assert_eq!(
            parse("interrupt", &json!({ "block": "b" })),
            Ok(Call::Interrupt { block: "b".into() })
        );
        assert_eq!(
            parse("close_block", &json!({ "block": "b" })),
            Ok(Call::CloseBlock { block: "b".into() })
        );
        assert!(parse("close_block", &json!({})).is_err());
    }

    /// `wire` is the command line's: agents never see it listed, and only `templum wire` makes it.
    #[test]
    fn wire_is_the_command_lines_alone() {
        assert!(!known("wire") && cli_only("wire"));
        assert!(!cli_only("wait_for") && !cli_only("whoami"));
        assert_eq!(
            parse("wire", &json!({ "a": "b7k2m", "b": "k9x1d" })),
            Ok(Call::Wire {
                a: "b7k2m".into(),
                b: "k9x1d".into()
            })
        );
        let err = parse("wire", &json!({ "a": "b7k2m" })).unwrap_err();
        assert!(err.contains("\"b\""), "{err}");
    }

    #[test]
    fn bad_arguments_say_which() {
        let err = parse("read_terminal", &json!({})).unwrap_err();
        assert!(err.contains("\"block\""), "{err}");
        assert!(parse("read_terminal", &json!({ "block": "b", "lines": 0 })).is_err());
        assert!(
            parse(
                "send_to_terminal",
                &json!({ "block": "b", "text": "x", "enter": "yes" })
            )
            .is_err()
        );
        assert_eq!(
            parse("stop_agent", &json!({})),
            Err("stop_agent needs \"block\", a string".into())
        );
        assert!(!known("rm_rf"));
        assert!(parse("rm_rf", &json!({})).is_err());
    }

    /// Starting and handing work to agents is listed whole in Claude Code, beside its Agent tool.
    #[test]
    fn the_agent_tools_alone_load_upfront() {
        let upfront: Vec<_> = list()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|t| t["_meta"]["anthropic/alwaysLoad"] == true)
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(upfront, ["delegate", "spawn_agent"]);
    }

    #[test]
    fn initialize_speaks_the_clients_version_when_it_can() {
        let answer = initialize_result(&json!({ "protocolVersion": "2025-06-18" }), "hi");
        assert_eq!(answer["protocolVersion"], "2025-06-18");
        assert_eq!(answer["serverInfo"]["name"], "templum");
        assert_eq!(answer["instructions"], "hi");
        assert!(answer["capabilities"]["tools"].is_object());
        let newer = initialize_result(&json!({ "protocolVersion": "2099-01-01" }), "");
        assert_eq!(newer["protocolVersion"], VERSIONS[0]);
    }

    #[test]
    fn a_refusal_is_a_result_marked_as_an_error() {
        let refused: Value =
            serde_json::from_str(&tool_result(&json!(3), &Reply::err("no"))).unwrap();
        assert_eq!(refused["result"]["isError"], true);
        assert_eq!(refused["result"]["content"][0]["text"], "no");
        let done: Value = serde_json::from_str(&tool_result(&json!(4), &Reply::ok("yes"))).unwrap();
        assert!(done["result"].get("isError").is_none());
    }

    #[test]
    fn a_long_text_keeps_its_end() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("abc", 10), "abc");
        // Cut inside "é" (2 bytes): starts after it.
        assert_eq!(tail("abcé", 1), "");
        assert_eq!(tail("abcé", 2), "é");
    }

    #[test]
    fn create_and_edit_card_take_a_priority_who_and_checklist_items() {
        assert_eq!(
            parse(
                "create_card",
                &json!({ "title": "v2", "priority": " High", "for": "codex", "checklist": "- a" })
            ),
            Ok(Call::CreateCard(NewCard {
                board: None,
                column: None,
                title: "v2".into(),
                description: None,
                priority: Some("high".into()),
                who: Some("codex".into()),
                checklist: Some("- a".into()),
            }))
        );
        let edit = |args: Value| parse("edit_card", &args);
        let only = |check: Vec<u64>, uncheck: Vec<u64>| {
            Ok(Call::EditCard(CardEdit {
                card: 12,
                priority: None,
                who: None,
                add_items: None,
                check,
                uncheck,
            }))
        };
        assert_eq!(
            edit(json!({ "card": 12, "check": 3 })),
            only(vec![3], vec![])
        );
        assert_eq!(
            edit(json!({ "card": "12", "check": ["3", 4] })),
            only(vec![3, 4], vec![])
        );
        assert_eq!(
            edit(json!({ "card": 12, "uncheck": [5] })),
            only(vec![], vec![5])
        );
        assert_eq!(
            edit(json!({ "card": 12, "priority": "NONE", "for": "anyone", "add_items": "x" })),
            Ok(Call::EditCard(CardEdit {
                card: 12,
                priority: Some("none".into()),
                who: Some("anyone".into()),
                add_items: Some("x".into()),
                check: vec![],
                uncheck: vec![],
            }))
        );
        for (tool, args, says) in [
            (
                "create_card",
                json!({ "title": "x", "priority": "urgent" }),
                "\"priority\" is low, medium, high or none",
            ),
            (
                "edit_card",
                json!({ "card": 1, "check": "x" }),
                "\"check\" is an item's id, or a list of them, as list_cards shows them",
            ),
            (
                "edit_card",
                json!({ "card": 1, "uncheck": [true] }),
                "\"uncheck\" is an item's id, or a list of them, as list_cards shows them",
            ),
            (
                "edit_card",
                json!({ "card": 1 }),
                "edit_card needs something to change: priority, for, add_items, check or uncheck",
            ),
        ] {
            assert_eq!(parse(tool, &args), Err(says.to_string()), "{args}");
        }
        let tools = list();
        let edit_card = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "edit_card")
            .unwrap();
        assert_eq!(edit_card["inputSchema"]["required"], json!(["card"]));
    }

    #[test]
    fn the_content_tools_take_names_or_ids_and_leave_out_what_they_may() {
        assert_eq!(
            parse("list_cards", &json!({})),
            Ok(Call::ListCards {
                board: None,
                card: None
            })
        );
        assert_eq!(
            parse("list_cards", &json!({ "board": 1, "card": "7" })),
            Ok(Call::ListCards {
                board: Some("1".into()),
                card: Some(7)
            })
        );
        assert_eq!(
            parse("create_card", &json!({ "title": "x" })),
            Ok(Call::CreateCard(NewCard {
                board: None,
                column: None,
                title: "x".into(),
                description: None,
                priority: None,
                who: None,
                checklist: None,
            }))
        );
        assert_eq!(
            parse("move_card", &json!({ "card": 3, "column": 4 })),
            Ok(Call::MoveCard {
                card: 3,
                column: "4".into()
            })
        );
        assert_eq!(
            parse("claim_card", &json!({ "card": " 9 " })),
            Ok(Call::ClaimCard { card: 9 })
        );
        assert_eq!(
            parse("open_file", &json!({ "path": "a.rs" })),
            Ok(Call::OpenFile {
                path: "a.rs".into(),
                line: None
            })
        );
        for (tool, bad) in [
            ("move_card", json!({ "column": "Doing" })),
            ("move_card", json!({ "card": "x", "column": "Doing" })),
            ("move_card", json!({ "card": 1 })),
            ("comment_card", json!({ "card": 1 })),
            ("claim_card", json!({ "card": -1 })),
            ("create_card", json!({})),
            ("open_file", json!({ "path": "a.rs", "line": 0 })),
            ("open_file", json!({})),
            ("append_note", json!({ "block": "n" })),
        ] {
            assert!(parse(tool, &bad).is_err(), "{tool} {bad}");
        }
        let err = parse("move_card", &json!({ "column": "Doing" })).unwrap_err();
        assert!(err.contains("\"card\""), "{err}");
        assert_eq!(
            parse("check_item", &json!({ "block": "c", "item": "13" })),
            Ok(Call::CheckItem {
                block: "c".into(),
                item: 13,
                done: true
            })
        );
        for bad in [
            json!({ "block": "c" }),
            json!({ "block": "c", "item": "x" }),
            json!({ "item": 1 }),
            json!({ "block": "c", "item": 1, "done": "no" }),
        ] {
            assert!(parse("check_item", &bad).is_err(), "{bad}");
        }
        let err = parse("check_item", &json!({ "block": "c", "item": "x" })).unwrap_err();
        assert!(err.contains("read_checklist"), "{err}");
        assert!(parse("add_items", &json!({ "block": "c" })).is_err());
    }
}
