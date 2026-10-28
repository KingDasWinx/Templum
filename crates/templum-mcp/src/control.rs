//! The control server (docs/specs/templum-agent.md#the-control-server): the tools only the
//! Templum Agent's process gets, the calls parsed from them, their running titles, and which of
//! them wait for the person.

use serde_json::{Value, json};

use crate::tools::{self, Call, Spawn};

/// The control server's name.
pub const SERVER: &str = "templum-control";

/// The block server's tools the control server serves as they are, on any block or board.
pub const SHARED: [&str; 22] = [
    "read_terminal",
    "list_cards",
    "read_note",
    "read_checklist",
    "create_card",
    "edit_card",
    "move_card",
    "comment_card",
    "edit_comment",
    "delete_comment",
    "append_note",
    "add_items",
    "check_item",
    "edit_item",
    "delete_item",
    "send_to_terminal",
    "send_keys",
    "notify",
    "delegate",
    "wait_for",
    "interrupt",
    "stop_agent",
];

/// A block `add_block` opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Terminal,
    Note,
    Checklist,
    Kanban,
    Editor,
}

/// What `add_block` opens: a kanban's `board` (made with `columns` when there is none), a
/// note's or a checklist's `text`.
#[derive(Clone, Debug, PartialEq)]
pub struct AddBlock {
    pub workspace: String,
    pub kind: Kind,
    pub folder: Option<String>,
    pub board: Option<String>,
    pub columns: Vec<String>,
    pub text: Option<String>,
    /// A terminal's shell, by its name in `list_shells`; the default when None.
    pub shell: Option<String>,
    pub near: Option<String>,
    /// Its top-left corner on the canvas; not with `near`.
    pub at: Option<(f32, f32)>,
}

/// What `open_agent` opens: `spawn_agent`'s agent in a workspace, wired to `connect`.
#[derive(Clone, Debug, PartialEq)]
pub struct Opening {
    pub workspace: String,
    pub spawn: Spawn,
    pub connect: Vec<String>,
    pub near: Option<String>,
    /// Its top-left corner on the canvas; not with `near`.
    pub at: Option<(f32, f32)>,
}

/// Where `edit_column` puts a column, or `edit_checklist` an item: before another one of its
/// board or checklist (its id), or at the end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Before {
    Id(u64),
    End,
}

/// A control tool's call, its arguments checked.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlCall {
    /// One of [`SHARED`], as the block server parses it.
    Block(Call),
    Overview,
    ListAgents,
    ListShells,
    CreateWorkspace {
        name: String,
        folder: Option<String>,
        color: Option<String>,
    },
    Show {
        workspace: Option<String>,
        block: Option<String>,
    },
    AddBlock(AddBlock),
    OpenAgent(Opening),
    /// Wires two blocks; `label` names the wire (blank takes the name off), left out keeps it.
    Connect {
        a: String,
        b: String,
        label: Option<String>,
    },
    Disconnect {
        a: String,
        b: String,
    },
    CloseBlock {
        block: String,
    },
    /// Deletes a card and its comments.
    DeleteCard {
        card: u64,
    },
    /// Renames a board (`name`), or deletes it with its cards (`delete`): exactly one.
    EditBoard {
        board: String,
        name: Option<String>,
        delete: bool,
    },
    /// No `column`: a new column (at the end, or `before`; with `title` when given). A `column`:
    /// renamed (`title`), moved (`before`) or both; `delete` goes alone.
    EditColumn {
        board: String,
        column: Option<u64>,
        title: Option<String>,
        before: Option<Before>,
        delete: bool,
    },
    /// Renames the checklist the checklist block `block` shows (`name`; blank reads "Checklist"),
    /// moves its item `item` (`before`), or both.
    EditChecklist {
        block: String,
        name: Option<String>,
        moved: Option<(u64, Before)>,
    },
    /// The checklist block `block`, showing none, shows a new checklist named `name` (none:
    /// "Checklist").
    NewChecklist {
        block: String,
        name: Option<String>,
    },
    /// The kanban `block` shows `board`, or a new board named `new` (blank: "Board n") with
    /// `columns` (To do, Doing and Done without): exactly one of `board` and `new`.
    ShowBoard {
        block: String,
        board: Option<String>,
        new: Option<String>,
        columns: Vec<String>,
    },
    /// Moves or resizes a block; what is left out keeps. Sizes are over 0.
    MoveBlock {
        block: String,
        x: Option<f32>,
        y: Option<f32>,
        w: Option<f32>,
        h: Option<f32>,
    },
}

/// A call and the reason the model gave for it, shown apart in the approval.
#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub call: ControlCall,
    pub reason: Option<String>,
}

/// How dangerous a call is, from its tool alone; [`Rule::level`] adds its target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rule {
    /// Runs at once.
    Free,
    /// Waits for the person.
    Ask,
    /// Waits unless the target is its own, or nothing would be lost.
    AskOwnWhenLosing,
    /// Waits unless the target is its own.
    AskOwn,
    /// Waits each time, without "Allow for this conversation".
    AskEach,
}

/// What the approval offers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Ask,
    AskEach,
}

/// What the block server's descriptions say of the caller, said for the Templum Agent: it acts
/// on any block and any board.
const SHARED_WORDS: [(&str, &str); 18] = [
    (" connected to you", ""),
    (" shown by a kanban", ""),
    ("; may be left out when one kanban shows a board", ""),
    ("as list_blocks shows it", "as overview shows it"),
    (
        "signed with your block's id",
        "signed \\\"Templum Agent\\\"",
    ),
    (
        "a notification from your block",
        "a notification from the Templum Agent; a click opens the chat",
    ),
    (
        " Done with your card: comment on it first what you did (comment_card), then move it to Review or Done.",
        "",
    ),
    (
        ": what you did on it, and what the next one needs to know, when you finish it.",
        ".",
    ),
    (
        " Not your own block, nor an agent on another agent's task unless it waits for its person.",
        "",
    ),
    (
        "in a terminal you opened (spawn_agent, create_block)",
        "in a terminal",
    ),
    (
        "Only the person stops a block you didn't open.",
        "The person allows it first, unless you opened it in this conversation.",
    ),
    (
        "waits for its person or is on another agent's task.",
        "waits for its person or is on another agent's task. The person allows it first, unless \
         you opened it in this conversation.",
    ),
    (
        "Refused while it is idle or on another agent's task.",
        "Refused while it is idle or on another agent's task. The person allows it first, unless \
         you opened it in this conversation.",
    ),
    // Its tasks' ends and watches come back into its chat; it has no reply of its own.
    (
        "what it sends with reply (Templum asks it to)",
        "what it sends with its reply tool (Templum asks it to)",
    ),
    ("back typed into your terminal as", "back into this chat as"),
    (
        "is typed into your terminal while you work",
        "comes back into this chat while you work",
    ),
    (" Left out with fork.", ""),
    (" ((you) on yours)", ""),
];

/// `tool` as the Templum Agent reads it: SHARED_WORDS said for it.
fn for_it(tool: &Value) -> Value {
    let mut text = tool.to_string();
    for (from, to) in SHARED_WORDS {
        text = text.replace(from, to);
    }
    serde_json::from_str(&text).expect("JSON")
}

/// The farthest a block may be put from the canvas's origin, in canvas units.
pub const PLACE_MOST: f32 = 10_000_000.0;

/// The longest name of a wire, in characters.
pub const LABEL_MOST: usize = 60;

/// The longest `reason`, in bytes: the approval draws it whole above its buttons.
const MAX_REASON: usize = 500;

/// `reason`, on every tool that may wait for the person.
fn reason() -> Value {
    json!({ "type": "string", "description": "Why, in a few words: the person sees it in the approval." })
}

/// The number under `key`: null is none, a finite number is some, else Err.
fn number(args: &Value, key: &str) -> Result<Option<f32>, String> {
    match &args[key] {
        Value::Null => Ok(None),
        value => value
            .as_f64()
            .map(|n| n as f32)
            .filter(|n| n.is_finite())
            .map(Some)
            .ok_or_else(|| format!("\"{key}\" is a number")),
    }
}

/// A place no canvas reaches is a typo, and would be stored as it is.
fn within(places: &[(&str, Option<f32>)]) -> Result<(), String> {
    match places
        .iter()
        .find(|(_, place)| place.is_some_and(|p| p.abs() > PLACE_MOST))
    {
        Some((key, _)) => Err(format!("\"{key}\" is within ±10000000")),
        None => Ok(()),
    }
}

/// `before` of `edit_column` and `edit_checklist`: an id, or `"end"`; Err when it is neither.
fn before(args: &Value) -> Result<Option<Before>, ()> {
    match &args["before"] {
        Value::Null => Ok(None),
        Value::String(s) if s.trim().eq_ignore_ascii_case("end") => Ok(Some(Before::End)),
        Value::String(s) => s
            .trim()
            .parse()
            .map(|id| Some(Before::Id(id)))
            .map_err(|_| ()),
        Value::Number(n) => n.as_u64().map(|id| Some(Before::Id(id))).ok_or(()),
        _ => Err(()),
    }
}

/// `x` and `y` of `add_block` and `open_agent`: both or neither, and not with `near`.
fn place(args: &Value) -> Result<Option<(f32, f32)>, String> {
    let (x, y) = (number(args, "x")?, number(args, "y")?);
    within(&[("x", x), ("y", y)])?;
    let Some((x, y)) = x.zip(y) else {
        return if x.is_some() || y.is_some() {
            Err("\"x\" and \"y\" go together".into())
        } else {
            Ok(None)
        };
    };
    if args["near"].as_str().is_some_and(|n| !n.trim().is_empty()) {
        return Err("Give near, or x and y: not both.".into());
    }
    Ok(Some((x, y)))
}

/// `tools/list`'s result.
pub fn list() -> Value {
    let block = json!({ "type": "string", "description": "A block's id, as overview shows it." });
    let workspace = json!({ "type": "string", "description": "A workspace's name or id (w3), as overview shows it." });
    let none = json!({ "type": "object", "properties": {} });
    let board =
        json!({ "type": "string", "description": "A board's name or id, as overview shows it." });
    let card = json!({ "type": "integer", "description": "A card's id, as list_cards shows it." });
    let at = |key: &str| {
        json!({
            "type": "number",
            "description": format!("Its top-left corner on the canvas, with {} (canvas units): it goes there, over what is there.",
                if key == "x" { "y" } else { "x" }),
        })
    };
    let blocks = tools::list();
    let blocks = blocks["tools"].as_array().expect("tools");
    let shared = blocks
        .iter()
        .filter(|t| SHARED.iter().any(|name| t["name"] == *name))
        .map(|t| {
            let mut tool = for_it(t);
            if [
                "send_to_terminal",
                "send_keys",
                "delegate",
                "interrupt",
                "stop_agent",
            ]
            .iter()
            .any(|name| t["name"] == *name)
            {
                tool["inputSchema"]["properties"]["reason"] = reason();
            }
            tool
        });
    let spawn = &blocks
        .iter()
        .find(|t| t["name"] == "spawn_agent")
        .expect("spawn_agent")["inputSchema"]["properties"];
    let mut opening = for_it(spawn);
    // The Templum Agent has no block session to fork, and opens no role.
    for key in ["fork", "role"] {
        opening.as_object_mut().expect("properties").remove(key);
    }
    opening["folder"]["description"] = json!(
        "Where it starts, relative to the workspace's folder; the workspace's when left out."
    );
    opening["workspace"] = workspace.clone();
    opening["connect"] = json!({
        "type": "array",
        "items": { "type": "string" },
        "description": "Blocks of that workspace to wire it to (a kanban, a note), by id.",
    });
    opening["near"] = json!({ "type": "string", "description": "A block to open it beside; else the workspace's free spot." });
    opening["x"] = at("x");
    opening["y"] = at("y");
    let own = json!([
        {
            "name": "overview",
            "description": "Every workspace: its id, name, folder, whether it is shown or stopped; each block's \
                            id, kind, title, folder, place, agent and its state (with what it asks), who opened \
                            it and whether it is your own; the wires; the boards each kanban shows; what waits \
                            for the person.",
            "inputSchema": none,
        },
        {
            "name": "list_agents",
            "description": "The agents Templum can open (open_agent's agent): id, title, whether it is \
                            installed, the efforts it takes and whether it takes a model, and the accounts; \
                            one the person hid in the menus ends its line with \"hidden\".",
            "inputSchema": none,
        },
        {
            "name": "list_shells",
            "description": "The shells a terminal can run (add_block's shell), a name a line: the default first, \
                            its line ending with \"  default\", then each other shell found.",
            "inputSchema": none,
        },
        {
            "name": "create_workspace",
            "description": "Makes a workspace, not shown. Says its id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string" },
                    "folder": { "type": "string", "description": "Its project's folder: its blocks start there." },
                    "color": {
                        "type": "string",
                        "enum": ["red", "yellow", "green", "cyan", "blue", "magenta", "accent"],
                    },
                },
                "required": ["name"],
            },
        },
        {
            "name": "show",
            "description": "Shows a workspace on the canvas, and flies the camera to a block of it. Only when \
                            the person asks to see something: it moves their view.",
            "inputSchema": {
                "type": "object",
                "properties": { "workspace": workspace, "block": block },
            },
        },
        {
            "name": "add_block",
            "description": "Opens a block in a workspace where it covers nothing: beside near, else at the \
                            workspace's free spot; at x and y when given. A terminal runs no command. Says its id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "workspace": workspace,
                    "kind": { "type": "string", "enum": ["terminal", "note", "checklist", "kanban", "editor"] },
                    "folder": { "type": "string", "description": "A terminal's or an editor's folder; the workspace's when left out." },
                    "board": { "type": "string", "description": "A kanban's board, by name or id: made with columns when there is none." },
                    "columns": { "type": "array", "items": { "type": "string" }, "description": "A new board's columns." },
                    "text": { "type": "string", "description": "A note's text; a checklist's name on its first line, then an item per line." },
                    "shell": { "type": "string", "description": "A terminal's shell, by its name in list_shells; the default when left out." },
                    "near": block,
                    "x": at("x"),
                    "y": at("y"),
                },
                "required": ["workspace", "kind"],
            },
        },
        {
            "name": "open_agent",
            "description": "Opens an agent (an id of list_agents) in a block of a workspace, as spawn_agent does: \
                            in the workspace's folder, the one you name, or a new Git worktree of that folder's \
                            repository (.worktrees/<worktree>, on a new branch of that name); wired to connect; \
                            its prompt typed once it is ready. With model and effort, it starts on them. Says \
                            the new block's id.",
            "inputSchema": { "type": "object", "properties": opening, "required": ["agent", "workspace"] },
        },
        {
            "name": "connect",
            "description": "Wires two blocks of one workspace (the person takes it back with Ctrl+Z); with label, \
                            names it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "a": block,
                    "b": block,
                    "label": {
                        "type": "string",
                        "description": "The wire's name, one line, at most 60 characters; blank takes it off. \
                                        On two blocks already wired it renames their wire.",
                    },
                },
                "required": ["a", "b"],
            },
        },
        {
            "name": "disconnect",
            "description": "Unwires two blocks (the person takes it back with Ctrl+Z).",
            "inputSchema": {
                "type": "object",
                "properties": { "a": block, "b": block },
                "required": ["a", "b"],
            },
        },
        {
            "name": "move_block",
            "description": "Moves or resizes a block: x and y its top-left corner, w and h its size, in canvas \
                            units; what you leave out keeps (the person takes it back with Ctrl+Z). Says where \
                            it is now.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "x": { "type": "number" },
                    "y": { "type": "number" },
                    "w": { "type": "number", "description": "Its width, over 0 (a block never gets smaller than its kind's least)." },
                    "h": { "type": "number", "description": "Its height, over 0 (a block never gets smaller than its kind's least)." },
                },
                "required": ["block"],
            },
        },
        {
            "name": "delete_card",
            "description": "Deletes a card and its comments (Ctrl+Z on a kanban brings it back). Asks the \
                            person first when it isn't a card you made.",
            "inputSchema": {
                "type": "object",
                "properties": { "card": card, "reason": reason() },
                "required": ["card"],
            },
        },
        {
            "name": "edit_board",
            "description": "Renames a board (name), or deletes it with its cards (delete: true; the kanbans \
                            that showed it show none; Ctrl+Z on a kanban brings it back). Deleting a board \
                            asks the person first when it holds a card you didn't make, or a board you \
                            didn't make.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "board": board,
                    "name": { "type": "string" },
                    "delete": { "type": "boolean" },
                    "reason": reason(),
                },
                "required": ["board"],
            },
        },
        {
            "name": "edit_column",
            "description": "Adds a column to a board (column left out: at the end, or before another), renames \
                            one (title), moves one (before: a column of the board, or \"end\") or deletes one \
                            with its cards (delete: true). Deleting a column \
                            asks the person first when it holds a card you didn't make, or its board \
                            isn't one you made.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "board": board,
                    "column": { "type": "integer", "description": "A column's id, as list_cards shows it." },
                    "title": { "type": "string" },
                    "before": {
                        "type": "string",
                        "description": "A column of the board (its id) to go before, or \"end\".",
                    },
                    "delete": { "type": "boolean" },
                    "reason": reason(),
                },
                "required": ["board"],
            },
        },
        {
            "name": "show_board",
            "description": "Shows a board in a kanban block (board), or a new one (new: its name, blank for \
                            \"Board n\"; columns: its columns, To do, Doing and Done when left out).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "board": board,
                    "new": { "type": "string" },
                    "columns": { "type": "array", "items": { "type": "string" } },
                },
                "required": ["block"],
            },
        },
        {
            "name": "edit_checklist",
            "description": "Renames the checklist a checklist block shows (name; blank reads \"Checklist\"), \
                            moves one of its items (item, before: an item of the checklist it goes just \
                            before, or \"end\"), or both: one step of the checklist's Ctrl+Z.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "block": block,
                    "name": { "type": "string" },
                    "item": { "type": "integer", "description": "An item's id, as read_checklist shows it." },
                    "before": {
                        "type": "string",
                        "description": "An item of the checklist (its id) to go before, or \"end\".",
                    },
                },
                "required": ["block"],
            },
        },
        {
            "name": "new_checklist",
            "description": "Shows a new checklist, named name (\"Checklist\" when left out), in a checklist \
                            block that shows none. A block that shows one keeps it: add_block opens a block \
                            with a new one.",
            "inputSchema": {
                "type": "object",
                "properties": { "block": block, "name": { "type": "string" } },
                "required": ["block"],
            },
        },
        {
            "name": "close_block",
            "description": "Closes a block (the person brings it back with Ctrl+Z). Asks the person first when a \
                            program runs in a block you didn't open.",
            "inputSchema": {
                "type": "object",
                "properties": { "block": block, "reason": reason() },
                "required": ["block"],
            },
        },
    ]);
    let mut all: Vec<Value> = own.as_array().expect("tools").clone();
    all.extend(shared);
    json!({ "tools": all })
}

/// Whether the tool `name` only reads: the API's Read tokens may call it.
pub fn reads(name: &str) -> bool {
    matches!(
        name,
        "overview"
            | "list_agents"
            | "list_shells"
            | "read_terminal"
            | "list_cards"
            | "read_note"
            | "read_checklist"
            | "wait_for"
    )
}

/// `name` is one of the control tools.
pub fn known(name: &str) -> bool {
    list()["tools"]
        .as_array()
        .is_some_and(|tools| tools.iter().any(|t| t["name"] == name))
}

/// The call `name` with its arguments `args`; Err says what is wrong with them.
pub fn parse(name: &str, args: &Value) -> Result<Parsed, String> {
    let text = |key: &str| {
        args[key]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| format!("{name} needs \"{key}\", a string"))
    };
    // A blank string is no value, as the block server takes it.
    let optional = |key: &str| match &args[key] {
        Value::Null => Ok(None),
        value => value
            .as_str()
            .map(|s| Some(s.to_string()).filter(|s| !s.trim().is_empty()))
            .ok_or_else(|| format!("\"{key}\" is a string")),
    };
    // A string kept as it is, blank included: a caller's blank name is for the glue to refuse.
    let string = |key: &str| match &args[key] {
        Value::Null => Ok(None),
        value => value
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| format!("\"{key}\" is a string")),
    };
    let flag = |key: &str| match &args[key] {
        Value::Null => Ok(false),
        value => value
            .as_bool()
            .ok_or_else(|| format!("\"{key}\" is true or false")),
    };
    // A board comes as its id or its name.
    let named = |key: &str| match &args[key] {
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) if !s.trim().is_empty() => Ok(s.trim().to_string()),
        _ => Err(format!("{name} needs \"{key}\", a board's name or id")),
    };
    let id = |key: &str, what: &str| -> Result<Option<u64>, String> {
        let bad = || format!("\"{key}\" is {what}, as list_cards shows it");
        match &args[key] {
            Value::Null => Ok(None),
            Value::Number(n) => n.as_u64().map(Some).ok_or_else(bad),
            Value::String(s) => s.trim().parse().map(Some).map_err(|_| bad()),
            _ => Err(bad()),
        }
    };
    let strings = |key: &str| match &args[key] {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => items
            .iter()
            .map(|v| v.as_str().map(str::to_string))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("\"{key}\" is a list of strings")),
        _ => Err(format!("\"{key}\" is a list of strings")),
    };
    let call = match name {
        "overview" => ControlCall::Overview,
        "list_agents" => ControlCall::ListAgents,
        "list_shells" => ControlCall::ListShells,
        "create_workspace" => ControlCall::CreateWorkspace {
            name: text("name")?,
            folder: optional("folder")?,
            color: optional("color")?,
        },
        "show" => {
            let (workspace, block) = (optional("workspace")?, optional("block")?);
            if workspace.is_none() && block.is_none() {
                return Err("show needs \"workspace\" or \"block\"".into());
            }
            ControlCall::Show { workspace, block }
        }
        "add_block" => ControlCall::AddBlock(AddBlock {
            workspace: text("workspace")?,
            shell: match (optional("shell")?, args["kind"].as_str()) {
                (Some(_), Some(kind)) if kind != "terminal" => {
                    return Err("\"shell\" is a terminal's".into());
                }
                (shell, _) => shell,
            },
            kind: match args["kind"].as_str() {
                Some("terminal") => Kind::Terminal,
                Some("note") => Kind::Note,
                Some("checklist") => Kind::Checklist,
                Some("kanban") => Kind::Kanban,
                Some("editor") => Kind::Editor,
                _ => {
                    return Err(
                        "add_block needs \"kind\": terminal, note, checklist, kanban or editor"
                            .into(),
                    );
                }
            },
            folder: optional("folder")?,
            board: match &args["board"] {
                Value::Number(n) => Some(n.to_string()),
                _ => optional("board")?,
            },
            columns: strings("columns")?,
            text: optional("text")?,
            near: optional("near")?,
            at: place(args)?,
        }),
        "open_agent" => {
            text("agent")?;
            let Call::SpawnAgent(spawn) = tools::parse("spawn_agent", args)
                .map_err(|e| e.replace("spawn_agent", "open_agent"))?
            else {
                unreachable!("spawn_agent parses into SpawnAgent")
            };
            if spawn.fork || spawn.role.is_some() {
                return Err("open_agent opens an agent of list_agents: no fork, no role".into());
            }
            ControlCall::OpenAgent(Opening {
                workspace: text("workspace")?,
                spawn,
                connect: strings("connect")?,
                near: optional("near")?,
                at: place(args)?,
            })
        }
        "connect" => ControlCall::Connect {
            a: text("a")?,
            b: text("b")?,
            label: match &args["label"] {
                Value::Null => None,
                Value::String(label) => {
                    let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
                    if label.chars().count() > LABEL_MOST {
                        return Err(format!(
                            "\"label\" is at most {LABEL_MOST} characters: a few words."
                        ));
                    }
                    Some(label)
                }
                _ => return Err("\"label\" is a string".into()),
            },
        },
        "disconnect" => ControlCall::Disconnect {
            a: text("a")?,
            b: text("b")?,
        },
        "close_block" => ControlCall::CloseBlock {
            block: text("block")?,
        },
        "delete_card" => ControlCall::DeleteCard {
            card: id("card", "a card's id")?
                .ok_or_else(|| format!("{name} needs \"card\", a card's id"))?,
        },
        "edit_board" => {
            let (new_name, delete) = (string("name")?, flag("delete")?);
            if new_name.is_some() == delete {
                return Err("edit_board takes name or delete: true".into());
            }
            if new_name.as_deref().is_some_and(|n| n.trim().is_empty()) {
                return Err("A board needs a name.".into());
            }
            ControlCall::EditBoard {
                board: named("board")?,
                name: new_name.map(|n| n.trim().to_string()),
                delete,
            }
        }
        "edit_column" => {
            let (column, title, delete) = (
                id("column", "a column's id")?,
                string("title")?,
                flag("delete")?,
            );
            let before = before(args)
                .map_err(|()| "\"before\" is a column of the board, or \"end\"".to_string())?;
            if delete {
                if title.is_some() || before.is_some() {
                    return Err("delete goes alone: it takes the column and its cards away".into());
                }
                if column.is_none() {
                    return Err("delete needs \"column\"".into());
                }
            } else if column.is_some() && title.is_none() && before.is_none() {
                return Err("edit_column needs title, before or delete for a column".into());
            }
            if title.as_deref().is_some_and(|t| t.trim().is_empty()) {
                return Err("A column needs a title.".into());
            }
            ControlCall::EditColumn {
                board: named("board")?,
                column,
                title: title.map(|t| t.trim().to_string()),
                before,
                delete,
            }
        }
        "edit_checklist" => {
            let item = match &args["item"] {
                Value::Null => None,
                value => Some(
                    value
                        .as_u64()
                        .or_else(|| value.as_str()?.trim().parse().ok())
                        .ok_or("\"item\" is an item's id, as read_checklist shows it")?,
                ),
            };
            let new_name = string("name")?;
            let before = before(args)
                .map_err(|()| "\"before\" is an item of the checklist, or \"end\"".to_string())?;
            if new_name.is_none() && item.is_none() && before.is_none() {
                return Err("edit_checklist takes name, or item with before".into());
            }
            let moved = match (item, before) {
                (Some(item), Some(before)) if before == Before::Id(item) => {
                    return Err("\"before\" is another item than \"item\"".into());
                }
                (Some(item), Some(before)) => Some((item, before)),
                (None, None) => None,
                _ => return Err("\"item\" and \"before\" go together".into()),
            };
            ControlCall::EditChecklist {
                block: text("block")?,
                name: new_name.map(|n| n.trim().to_string()),
                moved,
            }
        }
        "new_checklist" => ControlCall::NewChecklist {
            block: text("block")?,
            name: optional("name")?.map(|n| n.trim().to_string()),
        },
        "show_board" => {
            let (board, new) = (
                match &args["board"] {
                    Value::Number(n) => Some(n.to_string()),
                    _ => optional("board")?,
                },
                string("new")?,
            );
            if board.is_some() == new.is_some() {
                return Err("show_board takes board or new".into());
            }
            let columns = strings("columns")?;
            if new.is_none() && !columns.is_empty() {
                return Err("\"columns\" go with new".into());
            }
            ControlCall::ShowBoard {
                block: text("block")?,
                board,
                new: new.map(|n| n.trim().to_string()),
                columns,
            }
        }
        "move_block" => {
            let (x, y, w, h) = (
                number(args, "x")?,
                number(args, "y")?,
                number(args, "w")?,
                number(args, "h")?,
            );
            within(&[("x", x), ("y", y)])?;
            if [x, y, w, h].iter().all(Option::is_none) {
                return Err("move_block needs x, y, w or h".into());
            }
            for (key, size) in [("w", w), ("h", h)] {
                if size.is_some_and(|size| size <= 0.0) {
                    return Err(format!("\"{key}\" is a size over 0"));
                }
            }
            ControlCall::MoveBlock {
                block: text("block")?,
                x,
                y,
                w,
                h,
            }
        }
        _ if SHARED.contains(&name) => {
            let call = tools::parse(name, args)?;
            // The approval shows it whole: a longer one would stall each frame that draws it.
            if let Call::SendToTerminal { text, .. } | Call::Delegate { task: text, .. } = &call
                && text.len() > tools::MAX_TEXT
            {
                let key = if name == "delegate" { "task" } else { "text" };
                return Err(format!(
                    "\"{key}\" is at most {} bytes: send it in parts.",
                    tools::MAX_TEXT
                ));
            }
            ControlCall::Block(call)
        }
        _ => return Err(format!("Unknown tool: {name}")),
    };
    let reason = optional("reason")?;
    if reason.as_ref().is_some_and(|r| r.len() > MAX_REASON) {
        return Err(format!(
            "\"reason\" is at most {MAX_REASON} bytes: a few words."
        ));
    }
    Ok(Parsed { call, reason })
}

/// What a tool does, as its action row says while it runs (followed by `…`).
pub fn title(name: &str) -> &'static str {
    match name {
        "overview" => "Looking at the canvas",
        "list_agents" => "Looking at the agents",
        "list_shells" => "Looking at the shells",
        "read_terminal" => "Reading a terminal",
        "list_cards" => "Reading a board",
        "read_note" => "Reading a note",
        "read_checklist" => "Reading a checklist",
        "create_workspace" => "Creating a workspace",
        "show" => "Showing",
        "add_block" => "Adding a block",
        "create_card" => "Adding a card",
        "edit_card" => "Editing a card",
        "move_card" => "Moving a card",
        "comment_card" => "Commenting on a card",
        "edit_comment" => "Editing a comment",
        "delete_comment" => "Deleting a comment",
        "append_note" => "Writing in a note",
        "add_items" => "Adding items",
        "check_item" => "Checking an item",
        "edit_item" => "Editing an item",
        "delete_item" => "Deleting an item",
        "open_agent" => "Opening an agent",
        "connect" => "Connecting blocks",
        "disconnect" => "Disconnecting blocks",
        "close_block" => "Closing a block",
        "delete_card" => "Deleting a card",
        "edit_board" => "Editing a board",
        "edit_column" => "Editing a column",
        "show_board" => "Showing a board",
        "edit_checklist" => "Editing a checklist",
        "new_checklist" => "Making a checklist",
        "move_block" => "Moving a block",
        "send_to_terminal" => "Typing into a terminal",
        "send_keys" => "Pressing keys",
        "notify" => "Notifying you",
        "delegate" => "Handing a task",
        "wait_for" => "Waiting for an agent",
        "interrupt" => "Interrupting an agent",
        "stop_agent" => "Stopping a terminal's program",
        _ => "",
    }
}

/// A call's danger, decided from its tool: the model's arguments can't lower it.
pub fn rule(call: &ControlCall) -> Rule {
    match call {
        ControlCall::Block(Call::SendToTerminal { .. } | Call::SendKeys { .. }) => Rule::Ask,
        ControlCall::CloseBlock { .. }
        | ControlCall::EditBoard { delete: true, .. }
        | ControlCall::EditColumn { delete: true, .. } => Rule::AskOwnWhenLosing,
        // A wire lets each terminal on it type into the other and answer its dialogs: Templum
        // asks when it would reach a terminal that isn't the Templum Agent's own. A task runs as
        // the person's agent's mode lets it: in bypass or auto, whatever the task says.
        ControlCall::Block(
            Call::Interrupt { .. }
            | Call::StopAgent { .. }
            | Call::Delegate { .. }
            | Call::DeleteItem { .. },
        )
        | ControlCall::Connect { .. }
        | ControlCall::DeleteCard { .. }
        | ControlCall::OpenAgent(_) => Rule::AskOwn,
        _ => Rule::Free,
    }
}

impl Rule {
    /// Whether a call waits, and what it offers: `own` when its target is the Templum Agent's
    /// own, `losing` when it would end a program that runs.
    pub fn level(self, own: bool, losing: bool) -> Option<Level> {
        match self {
            Rule::Free => None,
            Rule::Ask => Some(Level::Ask),
            Rule::AskOwnWhenLosing => (!own && losing).then_some(Level::Ask),
            Rule::AskOwn => (!own).then_some(Level::Ask),
            Rule::AskEach => Some(Level::AskEach),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_names_the_tools_that_only_read() {
        let tools = list();
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        let reading: Vec<&str> = names.iter().copied().filter(|n| reads(n)).collect();
        assert_eq!(
            reading,
            [
                "overview",
                "list_agents",
                "list_shells",
                "read_terminal",
                "wait_for",
                "list_cards",
                "read_note",
                "read_checklist",
            ]
        ); // in list()'s order
        assert!(!reads("send_to_terminal") && !reads("whatever"));
    }

    #[test]
    fn move_block_takes_a_place_or_a_size_and_never_asks() {
        let err = |args: Value| parse("move_block", &args).unwrap_err();
        assert_eq!(err(json!({"block": "b"})), "move_block needs x, y, w or h");
        assert_eq!(err(json!({"block": "b", "h": 0})), "\"h\" is a size over 0");
        assert_eq!(
            err(json!({"block": "b", "w": -4})),
            "\"w\" is a size over 0"
        );
        assert_eq!(err(json!({"block": "b", "x": "left"})), "\"x\" is a number");
        assert!(err(json!({"x": 1})).contains("needs \"block\""));
        assert_eq!(
            err(json!({"block": "b", "x": 1e12})),
            "\"x\" is within ±10000000"
        );
        assert_eq!(
            err(json!({"block": "b", "y": -1e8})),
            "\"y\" is within ±10000000"
        );
        assert!(
            parse(
                "move_block",
                &json!({"block": "b", "x": -9_999_999, "y": 1e7})
            )
            .is_ok()
        );
        let parsed = parse("move_block", &json!({"block": "b", "x": -10.5, "w": 300})).unwrap();
        assert_eq!(
            parsed.call,
            ControlCall::MoveBlock {
                block: "b".into(),
                x: Some(-10.5),
                y: None,
                w: Some(300.0),
                h: None
            }
        );
        assert_eq!(rule(&parsed.call), Rule::Free);
        assert_eq!(title("move_block"), "Moving a block");
    }

    #[test]
    fn add_block_and_open_agent_take_a_place() {
        let err = |name: &str, args: Value| parse(name, &args).unwrap_err();
        let base = json!({"workspace": "w3", "kind": "terminal"});
        let with = |extra: Value| {
            let mut a = base.clone();
            a.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            a
        };
        assert_eq!(
            err("add_block", with(json!({"x": 10}))),
            "\"x\" and \"y\" go together"
        );
        assert_eq!(
            err("add_block", with(json!({"x": 1e8, "y": 0}))),
            "\"x\" is within ±10000000"
        );
        assert_eq!(
            err("add_block", with(json!({"x": "left", "y": 0}))),
            "\"x\" is a number"
        );
        assert_eq!(
            err("add_block", with(json!({"x": 1, "y": 2, "near": "b7k2m"}))),
            "Give near, or x and y: not both."
        );
        let ControlCall::AddBlock(add) = parse("add_block", &with(json!({"x": -12.5, "y": 40})))
            .unwrap()
            .call
        else {
            panic!()
        };
        assert_eq!(add.at, Some((-12.5, 40.0)));
        assert_eq!(add.shell, None);
        let ControlCall::AddBlock(add) = parse("add_block", &with(json!({"shell": "fish"})))
            .unwrap()
            .call
        else {
            panic!()
        };
        assert_eq!(add.shell.as_deref(), Some("fish"));
        assert_eq!(
            err(
                "add_block",
                json!({"workspace": "w3", "kind": "kanban", "shell": "fish"})
            ),
            "\"shell\" is a terminal's"
        );
        assert_eq!(
            err("add_block", with(json!({"shell": 5}))),
            "\"shell\" is a string"
        );
        let opening = json!({"agent": "codex", "workspace": "w3", "x": 1e7, "y": -1e7});
        let ControlCall::OpenAgent(open) = parse("open_agent", &opening).unwrap().call else {
            panic!()
        };
        assert_eq!(open.at, Some((1e7, -1e7)));
        assert_eq!(
            err(
                "open_agent",
                json!({"agent": "codex", "workspace": "w3", "y": 3})
            ),
            "\"x\" and \"y\" go together"
        );
    }

    #[test]
    fn the_checklist_tools_parse_and_say_what_is_missing() {
        let err = |name: &str, args: Value| parse(name, &args).unwrap_err();
        assert_eq!(
            err("edit_checklist", json!({"block": "c"})),
            "edit_checklist takes name, or item with before"
        );
        assert_eq!(
            err("edit_checklist", json!({"block": "c", "item": 13})),
            "\"item\" and \"before\" go together"
        );
        assert_eq!(
            err("edit_checklist", json!({"block": "c", "before": "end"})),
            "\"item\" and \"before\" go together"
        );
        assert_eq!(
            err(
                "edit_checklist",
                json!({"block": "c", "item": 13, "before": 13})
            ),
            "\"before\" is another item than \"item\""
        );
        assert_eq!(
            err(
                "edit_checklist",
                json!({"block": "c", "item": 13, "before": "first"})
            ),
            "\"before\" is an item of the checklist, or \"end\""
        );
        assert_eq!(
            err(
                "edit_checklist",
                json!({"block": "c", "item": "x", "before": 1})
            ),
            "\"item\" is an item's id, as read_checklist shows it"
        );
        assert!(err("new_checklist", json!({})).contains("needs \"block\""));
        let call = |name: &str, args: Value| parse(name, &args).unwrap().call;
        assert_eq!(
            call(
                "edit_checklist",
                json!({"block": "c", "name": " Release ", "item": "13", "before": "End"})
            ),
            ControlCall::EditChecklist {
                block: "c".into(),
                name: Some("Release".into()),
                moved: Some((13, Before::End)),
            }
        );
        assert_eq!(
            call("edit_checklist", json!({"block": "c", "name": ""})),
            ControlCall::EditChecklist {
                block: "c".into(),
                name: Some(String::new()),
                moved: None,
            },
            "a blank name reads \"Checklist\""
        );
        assert_eq!(
            call(
                "edit_checklist",
                json!({"block": "c", "item": 13, "before": 14})
            ),
            ControlCall::EditChecklist {
                block: "c".into(),
                name: None,
                moved: Some((13, Before::Id(14))),
            }
        );
        assert_eq!(
            call("new_checklist", json!({"block": "c", "name": " "})),
            ControlCall::NewChecklist {
                block: "c".into(),
                name: None,
            }
        );
    }

    #[test]
    fn the_board_tools_parse_and_say_what_is_missing() {
        let err = |name: &str, args: Value| parse(name, &args).unwrap_err();
        assert_eq!(
            err("edit_board", json!({"board": 1})),
            "edit_board takes name or delete: true"
        );
        assert_eq!(
            err(
                "edit_board",
                json!({"board": 1, "name": "x", "delete": true})
            ),
            "edit_board takes name or delete: true"
        );
        assert_eq!(
            err("edit_board", json!({"board": 1, "name": " "})),
            "A board needs a name."
        );
        assert_eq!(
            err("edit_column", json!({"board": 1, "column": 3})),
            "edit_column needs title, before or delete for a column"
        );
        assert_eq!(
            err(
                "edit_column",
                json!({"board": 1, "column": 3, "title": "x", "delete": true})
            ),
            "delete goes alone: it takes the column and its cards away"
        );
        assert_eq!(
            err("edit_column", json!({"board": 1, "delete": true})),
            "delete needs \"column\""
        );
        assert_eq!(
            err("edit_column", json!({"board": 1, "before": "first"})),
            "\"before\" is a column of the board, or \"end\""
        );
        assert_eq!(
            err("edit_column", json!({"board": 1, "title": ""})),
            "A column needs a title."
        );
        assert_eq!(
            err("show_board", json!({"block": "k"})),
            "show_board takes board or new"
        );
        assert_eq!(
            err(
                "show_board",
                json!({"block": "k", "board": "1", "new": "x"})
            ),
            "show_board takes board or new"
        );
        assert_eq!(
            err(
                "show_board",
                json!({"block": "k", "board": "1", "columns": ["a"]})
            ),
            "\"columns\" go with new"
        );
        assert_eq!(
            err("delete_card", json!({})),
            "delete_card needs \"card\", a card's id"
        );
        let call = |name: &str, args: Value| parse(name, &args).unwrap().call;
        assert_eq!(
            call("edit_column", json!({"board": 1, "before": "end"})),
            ControlCall::EditColumn {
                board: "1".into(),
                column: None,
                title: None,
                before: Some(Before::End),
                delete: false
            }
        );
        assert_eq!(
            call(
                "edit_column",
                json!({"board": "Sprint", "column": "3", "before": 5, "title": " QA "})
            ),
            ControlCall::EditColumn {
                board: "Sprint".into(),
                column: Some(3),
                title: Some("QA".into()),
                before: Some(Before::Id(5)),
                delete: false
            }
        );
        assert_eq!(
            call("show_board", json!({"block": "k", "new": ""})),
            ControlCall::ShowBoard {
                block: "k".into(),
                board: None,
                new: Some(String::new()),
                columns: vec![]
            },
            "a blank name is \"Board n\""
        );
    }

    #[test]
    fn the_two_servers_share_the_comment_and_item_tools() {
        assert_eq!(SHARED.len(), 22);
        for (name, args, said, want) in [
            (
                "edit_comment",
                json!({"card": 4, "comment": 9, "text": "x"}),
                "Editing a comment",
                Rule::Free,
            ),
            (
                "delete_comment",
                json!({"card": 4, "comment": 9}),
                "Deleting a comment",
                Rule::Free,
            ),
            (
                "edit_item",
                json!({"block": "c1", "item": 13, "text": "x"}),
                "Editing an item",
                Rule::Free,
            ),
            (
                "delete_item",
                json!({"block": "c1", "item": 13}),
                "Deleting an item",
                Rule::AskOwn,
            ),
        ] {
            assert!(known(name) && crate::tools::known(name), "{name}");
            let parsed = parse(name, &args).unwrap();
            assert!(matches!(parsed.call, ControlCall::Block(_)), "{name}");
            assert_eq!(rule(&parsed.call), want, "{name}");
            assert_eq!(title(name), said);
        }
    }

    #[test]
    fn connect_takes_a_label_of_one_line() {
        let label = |l: Value| match parse("connect", &json!({"a": "a1", "b": "b1", "label": l})) {
            Ok(Parsed {
                call: ControlCall::Connect { label, .. },
                ..
            }) => Ok(label),
            Ok(other) => panic!("{other:?}"),
            Err(e) => Err(e),
        };
        assert_eq!(
            label(json!("  two \n words ")),
            Ok(Some("two words".into()))
        );
        assert_eq!(label(json!("  ")), Ok(Some(String::new())));
        assert_eq!(label(json!(5)), Err("\"label\" is a string".into()));
        assert_eq!(
            label(json!("x".repeat(61))),
            Err("\"label\" is at most 60 characters: a few words.".into())
        );
        assert!(matches!(
            parse("connect", &json!({"a": "a1", "b": "b1"}))
                .unwrap()
                .call,
            ControlCall::Connect { label: None, .. }
        ));
    }

    #[test]
    fn every_tool_parses_its_example_and_names_what_is_missing() {
        let examples = [
            ("overview", json!({})),
            ("list_agents", json!({})),
            ("list_shells", json!({})),
            (
                "create_workspace",
                json!({"name": "API", "folder": "~/proj"}),
            ),
            ("show", json!({"workspace": "API", "block": "b7k2m"})),
            (
                "add_block",
                json!({"workspace": "w3", "kind": "kanban", "board": "Sprint", "columns": ["To do", "Done"]}),
            ),
            (
                "open_agent",
                json!({"agent": "codex", "workspace": "w3", "folder": "~/proj", "prompt": "Take card 4",
                    "connect": ["k9x1d"], "model": "gpt-5.6", "effort": "high"}),
            ),
            ("connect", json!({"a": "b7k2m", "b": "k9x1d"})),
            ("disconnect", json!({"a": "b7k2m", "b": "k9x1d"})),
            ("close_block", json!({"block": "b7k2m", "reason": "done"})),
            ("move_block", json!({"block": "b7k2m", "x": 10, "w": 300})),
            ("delete_card", json!({"card": 4, "reason": "done"})),
            ("edit_board", json!({"board": "Sprint", "name": "Sprint 2"})),
            (
                "edit_column",
                json!({"board": "Sprint", "column": 3, "title": "Review", "before": "end"}),
            ),
            (
                "show_board",
                json!({"block": "k9x1d", "new": "Ops", "columns": ["A", "B"]}),
            ),
            (
                "edit_checklist",
                json!({"block": "c1", "name": "Release", "item": 13, "before": "end"}),
            ),
            ("new_checklist", json!({"block": "c1", "name": "Plan"})),
            (
                "send_to_terminal",
                json!({"block": "b7k2m", "text": "ls", "enter": true, "reason": "look"}),
            ),
            ("send_keys", json!({"block": "b7k2m", "keys": ["2"]})),
            ("create_card", json!({"board": "Sprint", "title": "Login"})),
            ("read_terminal", json!({"block": "b7k2m"})),
            ("list_cards", json!({"board": "Sprint"})),
            ("read_note", json!({"block": "n1"})),
            ("read_checklist", json!({"block": "c1"})),
            ("edit_card", json!({"card": 4, "priority": "high"})),
            ("move_card", json!({"card": 4, "column": "Done"})),
            ("comment_card", json!({"card": 4, "text": "ok"})),
            (
                "edit_comment",
                json!({"card": 4, "comment": 9, "text": "x"}),
            ),
            ("delete_comment", json!({"card": 4, "comment": 9})),
            ("append_note", json!({"block": "n1", "text": "ok"})),
            ("add_items", json!({"block": "c1", "text": "a\nb"})),
            ("check_item", json!({"block": "c1", "item": 2})),
            ("edit_item", json!({"block": "c1", "item": 13, "text": "x"})),
            ("delete_item", json!({"block": "c1", "item": 13})),
            ("notify", json!({"text": "done"})),
            ("delegate", json!({"block": "b7k2m", "task": "Take card 4"})),
            ("wait_for", json!({"block": "b7k2m", "event": "idle"})),
            ("interrupt", json!({"block": "b7k2m", "reason": "stuck"})),
            ("stop_agent", json!({"block": "b7k2m", "reason": "done"})),
        ];
        let listed = list()["tools"].as_array().unwrap().len();
        assert_eq!(listed, examples.len(), "every listed tool has an example");
        for (name, args) in examples {
            assert!(known(name), "{name}");
            assert!(!title(name).is_empty(), "{name}");
            parse(name, &args).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
        assert!(
            parse("add_block", &json!({"kind": "note"}))
                .unwrap_err()
                .contains("\"workspace\"")
        );
        assert!(
            parse("add_block", &json!({"workspace": "w3", "kind": "drawing"}))
                .unwrap_err()
                .contains("terminal, note, checklist, kanban or editor")
        );
        assert!(
            parse("open_agent", &json!({"workspace": "w3"}))
                .unwrap_err()
                .contains("open_agent needs \"agent\"")
        );
        assert_eq!(
            parse(
                "send_to_terminal",
                &json!({"block": "b", "text": "x", "reason": "r"})
            )
            .unwrap()
            .reason
            .as_deref(),
            Some("r")
        );
        let reasoned =
            |reason: String| parse("close_block", &json!({"block": "b", "reason": reason}));
        assert!(reasoned("x".repeat(MAX_REASON)).is_ok());
        assert!(
            reasoned("x".repeat(MAX_REASON + 1))
                .unwrap_err()
                .contains("\"reason\" is at most 500 bytes")
        );
        assert_eq!(
            parse(
                "open_agent",
                &json!({"agent": "claude", "workspace": "w3", "connect": ["k1", "n2"]})
            )
            .unwrap()
            .call,
            ControlCall::OpenAgent(Opening {
                workspace: "w3".into(),
                spawn: Spawn {
                    agent: Some("claude".into()),
                    prompt: None,
                    folder: None,
                    worktree: None,
                    model: None,
                    effort: None,
                    fork: false,
                    role: None,
                    args: None,
                },
                connect: vec!["k1".into(), "n2".into()],
                near: None,
                at: None,
            })
        );
        for refused in [
            json!({"agent": "claude", "fork": true, "prompt": "p"}),
            json!({"agent": "claude", "role": "reviewer"}),
        ] {
            let mut args = refused;
            args["workspace"] = json!("w3");
            assert!(parse("open_agent", &args).is_err(), "{args}");
        }
        assert!(
            parse("whoami", &json!({}))
                .unwrap_err()
                .contains("Unknown tool")
        );
        let long = "x".repeat(tools::MAX_TEXT);
        let typing = |text: String| parse("send_to_terminal", &json!({"block": "b", "text": text}));
        assert!(typing(long.clone()).is_ok());
        assert!(
            typing(long.clone() + "x")
                .unwrap_err()
                .contains("\"text\" is at most 262144 bytes")
        );
        let task = parse("delegate", &json!({"block": "b", "task": long + "x"}));
        assert!(task.unwrap_err().contains("\"task\" is at most"));
    }

    #[test]
    fn the_two_servers_dont_list_each_others_own_tools() {
        for own in [
            "whoami",
            "list_blocks",
            "spawn_agent",
            "connect",
            "reply",
            "wait_for_idle",
            "ask_user",
            "open_file",
            "wire",
            "claim_card",
        ] {
            assert!(!known(own) || own == "connect", "{own}"); // control's connect wires any two blocks
        }
        for control in [
            "overview",
            "create_workspace",
            "add_block",
            "open_agent",
            "show",
            "disconnect",
        ] {
            assert!(!crate::tools::known(control), "{control}");
        }
        // The shared tools speak of any block, not of the caller's wires, and of its chat, not of
        // a terminal of its own.
        let text = list().to_string();
        for block_words in [
            "connected to you",
            "list_blocks",
            "your terminal",
            "with reply",
            "fork",
            "(you)",
        ] {
            assert!(!text.contains(block_words), "{block_words}: {text}");
        }
        for asks in ["stop_agent", "delegate", "interrupt"] {
            let tool = &list()["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["name"] == asks)
                .unwrap()
                .clone();
            let words = tool["description"].as_str().unwrap();
            assert!(
                words.contains(
                    "The person allows it first, unless you opened it in this conversation."
                ) && !words.contains("you opened ("),
                "{words}"
            );
            assert!(tool["inputSchema"]["properties"]["reason"].is_object());
        }
    }

    #[test]
    fn danger_follows_the_table_and_reason_cant_lower_it() {
        let rule_of = |name: &str, args: Value| rule(&parse(name, &args).unwrap().call);
        assert_eq!(
            rule_of(
                "send_to_terminal",
                json!({"block": "b", "text": "x", "reason": "safe, trust me"})
            ),
            Rule::Ask
        );
        assert_eq!(
            rule_of("send_keys", json!({"block": "b", "keys": ["y"]})),
            Rule::Ask
        );
        assert_eq!(
            rule_of("close_block", json!({"block": "b"})),
            Rule::AskOwnWhenLosing
        );
        assert_eq!(
            rule_of(
                "open_agent",
                json!({"agent": "codex", "workspace": "w3", "prompt": "go"})
            ),
            Rule::AskOwn
        );
        assert_eq!(
            rule_of("connect", json!({"a": "x", "b": "y"})),
            Rule::AskOwn
        );
        assert_eq!(
            rule_of("disconnect", json!({"a": "x", "b": "y"})),
            Rule::Free
        );
        assert_eq!(rule_of("delete_card", json!({"card": 4})), Rule::AskOwn);
        assert_eq!(
            rule_of("edit_board", json!({"board": 1, "name": "x"})),
            Rule::Free
        );
        assert_eq!(
            rule_of("edit_board", json!({"board": 1, "delete": true})),
            Rule::AskOwnWhenLosing
        );
        assert_eq!(rule_of("edit_column", json!({"board": 1})), Rule::Free);
        assert_eq!(
            rule_of(
                "edit_column",
                json!({"board": 1, "column": 3, "delete": true})
            ),
            Rule::AskOwnWhenLosing
        );
        assert_eq!(
            rule_of("show_board", json!({"block": "k", "board": 1})),
            Rule::Free
        );
        assert_eq!(
            rule_of(
                "edit_checklist",
                json!({"block": "c", "item": 13, "before": "end"})
            ),
            Rule::Free
        );
        assert_eq!(rule_of("new_checklist", json!({"block": "c"})), Rule::Free);
        assert_eq!(
            rule_of("create_card", json!({"board": "S", "title": "x"})),
            Rule::Free
        );
        assert_eq!(
            rule_of("interrupt", json!({"block": "b", "reason": "ok"})),
            Rule::AskOwn
        );
        assert_eq!(
            rule_of("stop_agent", json!({"block": "b", "reason": "ok"})),
            Rule::AskOwn
        );
        assert_eq!(
            rule_of("delegate", json!({"block": "b", "task": "t"})),
            Rule::AskOwn
        );
        assert_eq!(Rule::AskOwnWhenLosing.level(false, true), Some(Level::Ask));
        assert_eq!(Rule::AskOwnWhenLosing.level(true, true), None);
        assert_eq!(Rule::AskOwnWhenLosing.level(false, false), None);
        assert_eq!(Rule::Ask.level(true, false), Some(Level::Ask));
        assert_eq!(Rule::AskEach.level(false, false), Some(Level::AskEach));
        assert_eq!(Rule::AskOwn.level(true, true), None);
        assert_eq!(Rule::AskOwn.level(false, false), Some(Level::Ask));
        assert_eq!(Rule::Free.level(false, true), None);
    }
}
