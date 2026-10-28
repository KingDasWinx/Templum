//! The registry (docs/specs/api.md, Tools): every control tool plus the API's own, each tool's
//! scope, the route table that names a tool or a resource, and how a request's path, query and
//! body become a tool's arguments. The OpenAPI document is written from it (`openapi`).

use crate::store::{Scope, Way};
use serde_json::{Map, Value, json};
use templum_git::ops::{OPS, Op};
use templum_mcp::control::{self, Rule};
use templum_mcp::tools::MAX_TEXT;
use templum_terminal::{Button, KEY_NAMES, MouseInput, MouseKind, named_key};

use crate::model::NOTE_TEXT_MOST;

/// The colors a note takes through `edit_note`, and the word that clears it.
const NOTE_COLORS: [&str; 7] = ["red", "yellow", "green", "cyan", "blue", "magenta", "none"];

/// The keys one `terminal_input` call presses (`send_keys` keeps its own, fewer: `MAX_KEYS`).
pub const INPUT_KEYS: usize = 256;

/// The mouse events one `terminal_input` call sends.
pub const INPUT_MOUSE: usize = 64;

/// The tools only the API serves: automations are the person's, not an agent's.
const OWN: [&str; 16] = [
    "answer_request",
    "save_automation",
    "set_automation",
    "run_automation",
    "delete_automation",
    "cancel_schedule",
    "terminal_input",
    "share_workspace",
    "revoke_share",
    "git",
    "recent_folders",
    "edit_note",
    "new_note",
    "open_note",
    "close_note",
    "delete_note",
];

/// The arguments that name something a workspace-limited token must see: a block, a workspace, a
/// board, a card, a request, an automation or a schedule. One list, so a new tool's argument can't slip past
/// the check (`every_argument_is_a_target_or_not`).
pub const TARGETS: [&str; 12] = [
    "block",
    "a",
    "b",
    "near",
    "connect",
    "for",
    "workspace",
    "board",
    "card",
    "request",
    "automation",
    "schedule",
];

/// Every other argument name the tools have today: they name nothing a token could be kept from.
pub const NOT_TARGETS: &[&str] = &[
    "add_items",
    "address",
    "agent",
    "all",
    "allow",
    "amend",
    "answers",
    "base",
    "before",
    "branch",
    "check",
    "checklist",
    "color",
    "column",
    "columns",
    "comment",
    "delete",
    "description",
    "dismiss",
    "done",
    "effort",
    "email",
    "enter",
    "entry",
    "event",
    "expires",
    "folder",
    "force",
    "h",
    "hash",
    "how",
    "id",
    "ignore_whitespace",
    "item",
    "join",
    "keys",
    "kind",
    "label",
    "labels",
    "last_command",
    "leave",
    "lines",
    "message",
    "model",
    "mouse",
    "name",
    "new",
    "note",
    "on",
    "op",
    "patch",
    "path",
    "paths",
    "pattern",
    "priority",
    "prompt",
    "reason",
    "region",
    "remote",
    "remove_items",
    "replaced",
    "rev",
    "scope",
    "search",
    "share",
    "shell",
    "shown",
    "side",
    "skip",
    "stash",
    "summary",
    "take",
    "task",
    "text",
    "then",
    "timeout",
    "title",
    "uncheck",
    "untracked",
    "w",
    "wait",
    "way",
    "whole",
    "worktree",
    "x",
    "y",
];

/// A tool: its name, what it does and the schema of its arguments.
fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

/// Every tool: the control server's, then the API's own.
pub fn tools() -> Vec<Value> {
    let mut all = control::list()["tools"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let automation = json!({ "type": "integer", "description": "An automation's number, as the state shows it." });
    let note_id =
        json!({ "type": "integer", "description": "A note's id, as the state shows it." });
    all.extend([
        tool(
            "answer_request",
            "Answers what waits for the person, as the inbox's buttons do: a question or a plan with \
             answers (one list per question, the labels picked and at most one other text; a plan is \
             Approve or Reject), a yes/no request or an approval with allow (an approval's allow is \
             Allow once), or dismiss. note is a plan's comment or a denial's words.",
            json!({
                "request": { "type": "string", "description": "A request's id, as the state shows it (r12, a3)." },
                "answers": {
                    "type": "array",
                    "items": { "type": "array", "items": { "type": "string" } },
                    "description": "One list per question: the labels picked, then at most one other text.",
                },
                "allow": { "type": "boolean", "description": "Allow (true) or deny (false) a yes/no request or an approval." },
                "dismiss": { "type": "boolean", "description": "Dismiss a question or a plan without answering it." },
                "note": { "type": "string" },
            }),
            &["request"],
        ),
        tool(
            "save_automation",
            "Saves an automation as the editor's Save does, with its checks and words: a new one, or \
             the one numbered id replaced. Says its number.",
            json!({
                "automation": {
                    "type": "object",
                    "description": "As automations.json keeps one: name, on, workspace, when, do, max_runs.",
                },
                "id": automation,
            }),
            &["automation"],
        ),
        tool(
            "set_automation",
            "Turns an automation on or off.",
            json!({ "automation": automation, "on": { "type": "boolean" } }),
            &["automation", "on"],
        ),
        tool(
            "run_automation",
            "Runs an automation now.",
            json!({ "automation": automation }),
            &["automation"],
        ),
        tool(
            "delete_automation",
            "Deletes an automation.",
            json!({ "automation": automation }),
            &["automation"],
        ),
        tool(
            "cancel_schedule",
            "Cancels a delivery an agent asked for later, as the Automations popup's Cancel does.",
            json!({ "schedule": { "type": "string", "description": "A schedule's id, as the state shows it (s3)." } }),
            &["schedule"],
        ),
        tool(
            "terminal_input",
            "Types into a terminal as the person's keyboard does: text as a paste (bracketed when \
             its program asked), or keys by send_keys' names (a character is a key; 1 to 256, \
             back to back, 50 ms after an escape that another key follows), or clicks and scrolls \
             as their mouse does (mouse: only what the program asked for). Answers once they are in.",
            json!({
                "block": { "type": "string", "description": "A terminal block's id, as the state shows it." },
                "text": { "type": "string", "description": "Pasted whole; at most 256 KiB. Give text, keys or mouse." },
                "keys": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "maxItems": INPUT_KEYS,
                    "description": "Key names (enter, escape, ctrl+c, ctrl+shift+p, f5, a character…), 1 to 256.",
                },
                "mouse": {
                    "type": "array",
                    "minItems": 1,
                    "maxItems": INPUT_MOUSE,
                    "items": {
                        "type": "object",
                        "properties": {
                            "kind": { "enum": ["press", "release", "move"] },
                            "button": { "enum": ["left", "middle", "right", "wheel_up", "wheel_down", null] },
                            "row": { "type": "integer", "minimum": 0 },
                            "col": { "type": "integer", "minimum": 0 },
                            "ctrl": { "type": "boolean" },
                            "alt": { "type": "boolean" },
                            "shift": { "type": "boolean" },
                        },
                        "required": ["kind", "row", "col"],
                    },
                    "description": "Mouse events at cells of the grid (clamped), encoded at home in the program's mode; one it didn't ask for is dropped. 1 to 64.",
                },
            }),
            &["block"],
        ),
        tool(
            "share_workspace",
            "Shares a workspace with another Templum, as its Edit… dialog does: a token limited to it \
             and the connection code, given once in the answer's code field.",
            json!({
                "workspace": { "type": "string", "description": "The workspace's id or name." },
                "name": { "type": "string", "description": "What the share is called (at most 60 characters), like \"Another computer\"." },
                "scope": { "type": "string", "enum": ["control", "full"], "description": "Control watches, moves blocks and answers requests; full also types into terminals." },
                "expires": { "type": ["integer", "null"], "description": "Unix seconds; null or left out: never." },
                "way": { "type": "string", "enum": ["public", "pinned"], "description": "public: through a tunnel or proxy with a public certificate; pinned: directly, pinning Templum's certificate." },
                "address": { "type": "string", "description": "public: the https:// address; pinned: host:port." },
            }),
            &["workspace", "name", "scope", "way", "address"],
        ),
        tool(
            "revoke_share",
            "Revokes a share: its mirror is cut at once.",
            json!({
                "share": { "type": "string", "description": "A share's id, as the state shows it (s4). Left out: the calling token's own." },
            }),
            &[],
        ),
        tool(
            "git",
            "Runs one of the Git popup's operations in a repository of a workspace, as the popup \
             does: op names it, its arguments go beside it, paths are relative to the repository's \
             root. Reads take any token; acts take Control; discards, Restore version, deleting a \
             branch, dropping a stash and aborting take Full. Never raw git arguments.",
            json!({
                "workspace": { "type": "string", "description": "The workspace's id or name." },
                "folder": { "type": "string", "description": "A folder at home, whole or relative to the workspace's; its repository is used. Left out: the workspace's folder." },
                "op": { "type": "string", "enum": OPS, "description": "The operation; GET /v1/openapi.json and docs/api.md list each one's arguments." },
                "path": { "type": "string", "description": "A file, relative to the repository's root." },
                "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 10000, "description": "Files, relative to the repository's root." },
                "hash": { "type": "string", "description": "A commit's hash, 4 to 64 hex digits." },
                "base": { "type": "string", "description": "compare: the branch compared with." },
                "branch": { "type": "string", "description": "A branch the repository lists." },
                "name": { "type": "string", "description": "A new branch's or tag's name; set_author's name." },
                "email": { "type": "string", "description": "set_author: the email." },
                "message": { "type": "string", "description": "stash: the stash's name." },
                "summary": { "type": "string", "description": "commit: the first line; left blank, the sentence for the one file staged." },
                "description": { "type": "string", "description": "commit: the body." },
                "amend": { "type": "boolean", "description": "commit: replace the last commit." },
                "all": { "type": "boolean", "description": "commit, message: every change, not only what is staged; history: every branch." },
                "replaced": { "type": "string", "description": "undo_commit: the commit an amend replaced." },
                "how": { "type": "string", "enum": ["fast_forward", "merge", "rebase"], "description": "pull: how the upstream's commits join." },
                "join": { "type": "boolean", "description": "sync: after both sides moved, true rebases and false merges." },
                "remote": { "type": ["string", "boolean"], "description": "publish: the remote's name. switch: true for a remote branch (origin/x)." },
                "leave": { "type": "string", "description": "switch: the name of the stash the changes left behind go to." },
                "entry": { "type": "string", "description": "restore: a discard's id, as trash lists it." },
                "force": { "type": "boolean", "description": "restore, delete_branch: write over files changed since; delete an unmerged branch." },
                "patch": { "type": "string", "description": "A hunk, as the diff panel draws it (hunk_patch of the file shown)." },
                "region": { "type": "integer", "minimum": 0, "description": "resolve_region: the conflict region's number in the file." },
                "shown": { "type": "array", "description": "resolve_region: the region's two parts' lines as shown, [[…], […]]." },
                "side": { "type": "string", "enum": ["ours", "theirs"], "description": "resolve: the version kept." },
                "take": { "type": "string", "enum": ["first", "second", "both"], "description": "resolve_region: the part kept." },
                "then": { "type": "string", "description": "restore_version: the file's path in that commit." },
                "search": { "type": "object", "description": "history: {\"by\": \"message\"|\"author\"|\"code\", \"text\": …}." },
                "skip": { "type": "integer", "minimum": 0, "description": "history: commits to skip (200 come at a time)." },
                "stash": { "type": "object", "description": "A stash as stashes lists it: {\"index\": n, \"hash\": …}." },
                "untracked": { "type": "boolean", "description": "stash: untracked files too." },
                "whole": { "type": "array", "description": "diff: files shown whole, [{\"staged\": bool, \"path\": …}]." },
                "ignore_whitespace": { "type": "boolean", "description": "diff, show, compare, stash_diff: leave whitespace changes out." },
            }),
            &["workspace", "op"],
        ),
        tool(
            "recent_folders",
            "The folders a workspace opened lately, newest first, a folder a line, as its Git \
             popup's folder menu lists them. A token limited to some workspaces gets only those \
             inside their folders, links followed, as their real paths.",
            json!({ "workspace": { "type": "string", "description": "The workspace's id or name." } }),
            &["workspace"],
        ),
        tool(
            "edit_note",
            "Renames a note, tints it, or replaces its whole text, as the person does on a note \
             block: name (one line; blank is Untitled), color (red, yellow, green, cyan, blue, \
             magenta or none), or text (at most 256 KiB) with rev, the note's rev in the state the \
             text was made on. A text is written only while the note's rev is still that one, as \
             one step of its Ctrl+Z with the person's caret kept; else it is refused and the \
             newer text and rev are in the state. The note must be a tab of block.",
            json!({
                "block": { "type": "string", "description": "A note block's id, as the state shows it." },
                "note": note_id.clone(),
                "name": { "type": "string" },
                "color": { "type": "string", "enum": NOTE_COLORS, "description": "none clears it." },
                "text": { "type": "string", "description": "The note's whole new text, at most 256 KiB." },
                "rev": { "type": "integer", "description": "With text: the note's rev in the state, which the text was made on." },
            }),
            &["block", "note"],
        ),
        tool(
            "new_note",
            "Adds a new note as a tab of a note block, shown, as the block's + does; name (one \
             line) and text (at most 256 KiB) are optional. Says its id.",
            json!({
                "block": { "type": "string", "description": "A note block's id, as the state shows it." },
                "name": { "type": "string" },
                "text": { "type": "string", "description": "The new note's text, at most 256 KiB." },
            }),
            &["block"],
        ),
        tool(
            "open_note",
            "Shows a note: where it is open that tab is shown; else it opens as a tab of block, or \
             in a new note block of workspace (at x, y, both or neither). The state lists the \
             notes no block shows that were last open in a workspace.",
            json!({
                "note": note_id.clone(),
                "block": { "type": "string", "description": "A note block's id, as the state shows it." },
                "workspace": { "type": "string", "description": "The workspace's id or name." },
                "x": { "type": "number", "description": "With workspace: where the new block goes." },
                "y": { "type": "number", "description": "With workspace: where the new block goes." },
            }),
            &["note"],
        ),
        tool(
            "close_note",
            "Closes a note's tab as its × does: the note stays (a blank one goes) and the state \
             lists it by the workspace it was open in.",
            json!({
                "block": { "type": "string", "description": "A note block's id, as the state shows it." },
                "note": note_id.clone(),
            }),
            &["block", "note"],
        ),
        tool(
            "delete_note",
            "Deletes a note for good, from every block, as the Notes dialog's trash does.",
            json!({ "note": note_id }),
            &["note"],
        ),
    ]);
    all
}

/// The tools with their scope, as `GET /v1/tools` lists them.
pub fn listed() -> Vec<Value> {
    tools()
        .into_iter()
        .map(|mut tool| {
            let scope = nominal(tool["name"].as_str().unwrap_or_default());
            tool["scope"] = json!(scope);
            tool
        })
        .collect()
}

pub fn known(name: &str) -> bool {
    tools().iter().any(|tool| tool["name"] == name)
}

/// The words of a target that isn't there, or isn't the token's to see: the same for both, so a
/// limited token learns nothing about what lies outside its workspaces.
pub fn no(word: &str, id: &str) -> String {
    let path = match word {
        "block" => "/v1/blocks",
        "workspace" => "/v1/workspaces",
        "board" => "/v1/boards",
        "request" => "/v1/requests",
        "automation" => "/v1/automations",
        "share" => "/v1/shares",
        _ => "/v1/state",
    };
    format!("There's no {word} {id}: GET {path} lists them.")
}

/// One of the API's own tools.
pub fn own(name: &str) -> bool {
    OWN.contains(&name)
}

/// The scope a call of `name` with `args` takes. A control tool is parsed first (its Err is the
/// parser's sentence): Read when it only reads, Full when its rule asks the person each time,
/// else Control.
pub fn scope(name: &str, args: &Value) -> Result<Scope, String> {
    if name == "git" {
        return match args["op"].as_str() {
            None => Err("git needs \"op\": GET /v1/tools lists the operations.".into()),
            Some(op) => git_scope(op)
                .ok_or_else(|| format!("There's no git operation {op}: GET /v1/tools lists them.")),
        };
    }
    if own(name) {
        return Ok(match name {
            "save_automation" if args["automation"]["do"].get("command").is_some() => Scope::Full,
            "delete_automation" | "terminal_input" | "share_workspace" | "delete_note" => {
                Scope::Full
            }
            // Any share may revoke itself; the glue asks Full of one revoking another.
            "revoke_share" => Scope::Read,
            "recent_folders" => Scope::Read,
            _ => Scope::Control,
        });
    }
    let parsed = control::parse(name, args)?;
    Ok(if control::reads(name) {
        Scope::Read
    } else if matches!(control::rule(&parsed.call), Rule::Ask | Rule::AskEach) {
        Scope::Full
    } else {
        Scope::Control
    })
}

/// The git operations that only read.
const GIT_READS: [&str; 16] = [
    "find",
    "look",
    "diff",
    "history",
    "show",
    "blame",
    "branches",
    "unmerged",
    "default_branch",
    "compare",
    "stashes",
    "stash_diff",
    "remotes",
    "last_message",
    "measure",
    "trash",
];

/// The git operations that can lose work: a discard, a version written over a file, a branch
/// or a stash deleted, a merge given up.
const GIT_FULL: [&str; 6] = [
    "discard",
    "discard_hunk",
    "restore_version",
    "delete_branch",
    "stash_drop",
    "abort",
];

/// The scope of a git operation; None for a name that isn't one.
pub fn git_scope(op: &str) -> Option<Scope> {
    if GIT_READS.contains(&op) {
        Some(Scope::Read)
    } else if GIT_FULL.contains(&op) {
        Some(Scope::Full)
    } else if OPS.contains(&op) {
        Some(Scope::Control)
    } else {
        None
    }
}

/// What a call needs and its token lacks: "send_keys needs a Full token.", and for git the
/// operation's own: "git's discard needs a Full token."
pub fn needs_words(name: &str, args: &Value, needs: Scope) -> String {
    match (name, args["op"].as_str()) {
        ("git", Some(op)) => format!("git's {op} needs a {} token.", needs.word()),
        _ => format!("{name} needs a {} token.", needs.word()),
    }
}

/// A tool's scope as the documents say it: "read", "control", "full", or "control or full" for the
/// two whose scope depends on their arguments.
pub fn nominal(name: &str) -> &'static str {
    if matches!(name, "answer_request" | "save_automation") {
        return "control or full";
    }
    if name == "revoke_share" {
        return "full, or the share itself";
    }
    if name == "git" {
        return "read, control or full, by op";
    }
    let Some(tool) = tools().into_iter().find(|tool| tool["name"] == name) else {
        return "control";
    };
    // An example call: what the tool needs, in the least that parses.
    let schema = &tool["inputSchema"];
    let mut args = Map::new();
    for key in schema["required"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        let property = &schema["properties"][key];
        let example = match property["type"].as_str() {
            Some("integer" | "number") => json!(1),
            Some("boolean") => json!(true),
            Some("array") => json!(["x"]),
            Some("object") => json!({}),
            _ => property["enum"][0]
                .clone()
                .as_str()
                .map_or(json!("x"), |e| json!(e)),
        };
        args.insert(key.to_string(), example);
    }
    match scope(name, &Value::Object(args)) {
        Ok(Scope::Read) => "read",
        Ok(Scope::Full) => "full",
        _ => "control",
    }
}

/// What a route does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Does {
    /// Runs a tool; an empty name is `POST /v1/tools/{name}`, where the path names it.
    Tool(&'static str),
    Read(Resource),
    Events,
    /// A terminal's screen, live (`GET /v1/terminals/{block}/stream`).
    Frames,
    Ticket,
    Tools,
    OpenApi,
    /// Who the token is (`GET /v1/me`): answered from the token alone, without the UI thread.
    Me,
}

/// A resource a GET reads from the state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Resource {
    State,
    Workspaces,
    Workspace,
    Blocks,
    Block,
    Agents,
    Requests,
    Request,
    Boards,
    Board,
    Card,
    Automations,
    Automation,
    Shares,
    Usage,
}

#[derive(Debug)]
pub struct Route {
    pub method: &'static str,
    pub path: &'static str,
    pub does: Does,
}

const fn route(method: &'static str, path: &'static str, does: Does) -> Route {
    Route { method, path, does }
}

use Does::{Events, Frames, Me, OpenApi, Read, Ticket, Tool, Tools};
use Resource as R;

/// Every route (docs/specs/api.md, Resources and REST aliases). A path parameter is a tool's
/// argument of the same name.
pub const ROUTES: &[Route] = &[
    route("GET", "/v1/state", Read(R::State)),
    route("GET", "/v1/workspaces", Read(R::Workspaces)),
    route("GET", "/v1/workspaces/{workspace}", Read(R::Workspace)),
    route("GET", "/v1/blocks", Read(R::Blocks)),
    route("GET", "/v1/blocks/{block}", Read(R::Block)),
    route("GET", "/v1/agents", Read(R::Agents)),
    route("GET", "/v1/terminals/{block}/text", Tool("read_terminal")),
    route("GET", "/v1/notes/{block}/text", Tool("read_note")),
    route("GET", "/v1/checklists/{block}", Tool("read_checklist")),
    route("GET", "/v1/requests", Read(R::Requests)),
    route("GET", "/v1/requests/{request}", Read(R::Request)),
    route("GET", "/v1/boards", Read(R::Boards)),
    route("GET", "/v1/boards/{board}", Read(R::Board)),
    route("GET", "/v1/cards/{card}", Read(R::Card)),
    route("GET", "/v1/automations", Read(R::Automations)),
    route("GET", "/v1/automations/{automation}", Read(R::Automation)),
    route("GET", "/v1/shares", Read(R::Shares)),
    route("GET", "/v1/me", Me),
    route("GET", "/v1/usage", Read(R::Usage)),
    route("GET", "/v1/tools", Tools),
    route("GET", "/v1/openapi.json", OpenApi),
    route("GET", "/v1/events", Events),
    route("GET", "/v1/terminals/{block}/stream", Frames),
    route("POST", "/v1/tickets", Ticket),
    route("POST", "/v1/tools/{name}", Tool("")),
    route("POST", "/v1/workspaces", Tool("create_workspace")),
    route("POST", "/v1/blocks", Tool("add_block")),
    route("PATCH", "/v1/blocks/{block}", Tool("move_block")),
    route("DELETE", "/v1/blocks/{block}", Tool("close_block")),
    route("POST", "/v1/show", Tool("show")),
    route("POST", "/v1/wires", Tool("connect")),
    route("DELETE", "/v1/wires/{a}/{b}", Tool("disconnect")),
    route("POST", "/v1/agents", Tool("open_agent")),
    route("POST", "/v1/agents/{block}/task", Tool("delegate")),
    route("POST", "/v1/agents/{block}/interrupt", Tool("interrupt")),
    route("POST", "/v1/agents/{block}/stop", Tool("stop_agent")),
    route(
        "POST",
        "/v1/terminals/{block}/text",
        Tool("send_to_terminal"),
    ),
    route("POST", "/v1/terminals/{block}/keys", Tool("send_keys")),
    route(
        "POST",
        "/v1/terminals/{block}/input",
        Tool("terminal_input"),
    ),
    route(
        "POST",
        "/v1/requests/{request}/answer",
        Tool("answer_request"),
    ),
    route("POST", "/v1/boards/{board}/cards", Tool("create_card")),
    route("PATCH", "/v1/cards/{card}", Tool("edit_card")),
    route("POST", "/v1/cards/{card}/move", Tool("move_card")),
    route("POST", "/v1/cards/{card}/comments", Tool("comment_card")),
    route("POST", "/v1/automations", Tool("save_automation")),
    route(
        "PUT",
        "/v1/automations/{automation}",
        Tool("save_automation"),
    ),
    route(
        "PATCH",
        "/v1/automations/{automation}",
        Tool("set_automation"),
    ),
    route(
        "POST",
        "/v1/automations/{automation}/run",
        Tool("run_automation"),
    ),
    route(
        "DELETE",
        "/v1/automations/{automation}",
        Tool("delete_automation"),
    ),
    route(
        "POST",
        "/v1/workspaces/{workspace}/shares",
        Tool("share_workspace"),
    ),
    route("DELETE", "/v1/shares/{share}", Tool("revoke_share")),
];

/// The route for `method` and `path`, with its path parameters (percent-decoded). None for
/// anything else, a trailing slash and an empty segment included.
pub fn find(method: &str, path: &str) -> Option<(&'static Route, Vec<(&'static str, String)>)> {
    let segments: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    ROUTES
        .iter()
        .filter(|route| route.method == method)
        .find_map(|route| {
            let pattern: Vec<&'static str> = route.path.strip_prefix('/')?.split('/').collect();
            if pattern.len() != segments.len() {
                return None;
            }
            let mut params = Vec::new();
            for (pattern, segment) in pattern.iter().zip(&segments) {
                match pattern.strip_prefix('{').and_then(|p| p.strip_suffix('}')) {
                    Some(name) => {
                        let value = percent_decode(segment).filter(|v| !v.is_empty())?;
                        params.push((name, value));
                    }
                    None if pattern == segment => {}
                    None => return None,
                }
            }
            Some((route, params))
        })
}

/// A path segment with its `%XX` bytes decoded; None when they aren't UTF-8.
fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = segment.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The arguments of `tool` from a request: the body (a JSON object, or none), then the query,
/// then the path parameters (the path wins), each text value typed by the tool's schema.
pub fn arguments(
    tool: &str,
    params: &[(&str, String)],
    query: &[(String, String)],
    body: Option<Value>,
) -> Result<Value, String> {
    let mut args = match body {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(object)) => object,
        Some(_) => return Err("The body is a JSON object.".into()),
    };
    // An automation is saved from its own fields; the number in the path is its id.
    if tool == "save_automation" {
        let rule = if args.contains_key("automation") {
            args.clone()
        } else {
            Map::from_iter([("automation".to_string(), Value::Object(args))])
        };
        args = rule;
    }
    let schema = tools()
        .into_iter()
        .find(|t| t["name"] == tool)
        .ok_or_else(|| format!("Unknown tool: {tool}"))?;
    let properties = &schema["inputSchema"]["properties"];
    for (key, text) in query {
        if properties.get(key.as_str()).is_none() {
            continue;
        }
        let typed = typed(key, text, &properties[key.as_str()])?;
        match (args.get_mut(key), typed) {
            // `?keys=a&keys=b`: one list.
            (Some(Value::Array(list)), Value::Array(more)) => list.extend(more),
            (_, typed) => {
                args.insert(key.clone(), typed);
            }
        }
    }
    for (key, text) in params {
        let key = if tool == "save_automation" && *key == "automation" {
            "id"
        } else {
            key
        };
        args.insert(key.to_string(), typed(key, text, &properties[key])?);
    }
    Ok(Value::Object(args))
}

/// `text` as the schema `property` says its argument is.
fn typed(key: &str, text: &str, property: &Value) -> Result<Value, String> {
    Ok(match property["type"].as_str() {
        Some("integer") => json!(
            text.parse::<i64>()
                .map_err(|_| format!("\"{key}\" is a whole number"))?
        ),
        Some("number") => json!(
            text.parse::<f64>()
                .ok()
                .filter(|n| n.is_finite())
                .ok_or_else(|| format!("\"{key}\" is a number"))?
        ),
        Some("boolean") => match text {
            "true" => json!(true),
            "false" => json!(false),
            _ => return Err(format!("\"{key}\" is true or false")),
        },
        Some("array") => json!([text]),
        _ => json!(text),
    })
}

/// An own tool's call, its arguments checked.
#[derive(Clone, Debug, PartialEq)]
pub enum Own {
    AnswerRequest {
        request: String,
        answer: Answer,
        note: Option<String>,
    },
    SaveAutomation {
        id: Option<u64>,
        automation: Value,
    },
    SetAutomation {
        id: u64,
        on: bool,
    },
    RunAutomation {
        id: u64,
    },
    DeleteAutomation {
        id: u64,
    },
    CancelSchedule {
        schedule: String,
    },
    TerminalInput {
        block: String,
        input: Input,
    },
    ShareWorkspace {
        workspace: String,
        name: String,
        scope: Scope,
        expires: Option<u64>,
        way: Way,
        address: String,
    },
    RevokeShare {
        share: Option<String>,
    },
    Git {
        workspace: String,
        folder: Option<String>,
        op: Op,
    },
    RecentFolders {
        workspace: String,
    },
    EditNote {
        block: String,
        note: u64,
        name: Option<String>,
        /// None: left as it is; Some(None): cleared.
        color: Option<Option<String>>,
        /// The text and the `rev` it was made on.
        text: Option<(String, u64)>,
    },
    NewNote {
        block: String,
        name: Option<String>,
        text: Option<String>,
    },
    OpenNote {
        note: u64,
        into: NoteInto,
    },
    CloseNote {
        block: String,
        note: u64,
    },
    DeleteNote {
        note: u64,
    },
}

/// Where `open_note` opens a note that no block shows.
#[derive(Clone, Debug, PartialEq)]
pub enum NoteInto {
    Block(String),
    Workspace {
        workspace: String,
        at: Option<(f32, f32)>,
    },
}

/// What `terminal_input` types.
#[derive(Clone, PartialEq)]
pub enum Input {
    /// A paste.
    Text(String),
    /// Keys by `send_keys`' names.
    Keys(Vec<String>),
    /// Mouse events at cells of the grid.
    Mouse(Vec<MouseInput>),
}

/// A mouse event as `terminal_input` takes it (`mouse_of` reads it back).
pub fn mouse_value(e: &MouseInput) -> Value {
    json!({
        "kind": match e.kind {
            MouseKind::Press => "press",
            MouseKind::Release => "release",
            MouseKind::Move => "move",
        },
        "button": e.button.map(|b| match b {
            Button::Left => "left",
            Button::Middle => "middle",
            Button::Right => "right",
            Button::WheelUp => "wheel_up",
            Button::WheelDown => "wheel_down",
        }),
        "row": e.row,
        "col": e.col,
        "ctrl": e.ctrl,
        "alt": e.alt,
        "shift": e.shift,
    })
}

/// The mouse event `v` says, or None when it isn't one: a press or release has a button, a cell
/// is a whole number (past `usize`, the last cell: home clamps it), a modifier a boolean.
pub fn mouse_of(v: &Value) -> Option<MouseInput> {
    let kind = match v["kind"].as_str()? {
        "press" => MouseKind::Press,
        "release" => MouseKind::Release,
        "move" => MouseKind::Move,
        _ => return None,
    };
    let button = match v["button"].as_str() {
        Some("left") => Some(Button::Left),
        Some("middle") => Some(Button::Middle),
        Some("right") => Some(Button::Right),
        Some("wheel_up") => Some(Button::WheelUp),
        Some("wheel_down") => Some(Button::WheelDown),
        Some(_) => return None,
        None if v["button"].is_null() && kind == MouseKind::Move => None,
        None => return None,
    };
    let cell = |key: &str| {
        let n = v[key].as_number()?;
        match n.as_u64() {
            Some(n) => Some(usize::try_from(n).unwrap_or(usize::MAX)),
            // A whole number past u64 reads as a float; `as` saturates.
            None => n
                .as_f64()
                .filter(|f| *f >= 0.0 && f.fract() == 0.0)
                .map(|f| f as usize),
        }
    };
    let flag = |key: &str| match &v[key] {
        Value::Null => Some(false),
        Value::Bool(b) => Some(*b),
        _ => None,
    };
    Some(MouseInput {
        button,
        kind,
        row: cell("row")?,
        col: cell("col")?,
        ctrl: flag("ctrl")?,
        alt: flag("alt")?,
        shift: flag("shift")?,
    })
}

/// Never the text or the keys: a password typed through the API must not reach a log by `{:?}`.
impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Input::Text(text) => write!(f, "Text({} bytes)", text.len()),
            Input::Keys(keys) => write!(f, "Keys({})", keys.len()),
            Input::Mouse(events) => write!(f, "Mouse({})", events.len()),
        }
    }
}

/// How a request is answered.
#[derive(Clone, Debug, PartialEq)]
pub enum Answer {
    /// A list per question: the labels picked, then at most one other text.
    Picked(Vec<Vec<String>>),
    Allow(bool),
    Dismiss,
}

/// A whole number given as a number or as a string of digits (a path's parameter comes as one).
pub fn whole(value: &Value) -> Option<u64> {
    value
        .as_u64()
        .or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

/// The note tools' arguments (`parse_own`'s arm for them).
fn parse_note_tool(name: &str, args: &Value) -> Result<Own, String> {
    let note = || {
        whole(&args["note"])
            .ok_or_else(|| "\"note\" is a note's id, as the state shows it".to_string())
    };
    let string = |key: &str| match &args[key] {
        Value::Null => Ok(None),
        Value::String(text) => Ok(Some(text.clone())),
        _ => Err(format!("\"{key}\" is a string")),
    };
    let block = || string("block")?.ok_or_else(|| format!("{name} needs \"block\", a string"));
    let text = |key: &str| -> Result<Option<String>, String> {
        let text = string(key)?;
        match text {
            Some(text) if text.len() > NOTE_TEXT_MOST => {
                Err("A note through the API holds at most 256 KiB.".into())
            }
            text => Ok(text),
        }
    };
    Ok(match name {
        "edit_note" => {
            let (block, note) = (block()?, note()?);
            let name = string("name")?;
            let color = match string("color")? {
                None => None,
                Some(color) if NOTE_COLORS.contains(&color.as_str()) => {
                    Some((color != "none").then_some(color))
                }
                Some(_) => {
                    return Err(
                        "\"color\" is red, yellow, green, cyan, blue, magenta or none".into(),
                    );
                }
            };
            let rev = match &args["rev"] {
                Value::Null => None,
                rev => Some(whole(rev).ok_or("\"rev\" is a whole number")?),
            };
            let text = match (text("text")?, rev) {
                (Some(text), Some(rev)) => Some((text, rev)),
                (None, None) => None,
                _ => {
                    return Err(
                        "edit_note's text and rev go together: rev is the note's rev in \
                                the state, which the text was made on"
                            .into(),
                    );
                }
            };
            if name.is_none() && color.is_none() && text.is_none() {
                return Err("edit_note takes name, color, or text with rev".into());
            }
            Own::EditNote {
                block,
                note,
                name,
                color,
                text,
            }
        }
        "new_note" => Own::NewNote {
            block: block()?,
            name: string("name")?,
            text: text("text")?,
        },
        "open_note" => {
            let note = note()?;
            let (block, workspace) = (string("block")?, string("workspace")?);
            let place = |key: &str| match &args[key] {
                Value::Null => Ok(None),
                value => value
                    .as_f64()
                    .filter(|n| n.is_finite() && n.abs() <= f64::from(control::PLACE_MOST))
                    .map(Some)
                    .ok_or_else(|| format!("\"{key}\" is within ±10000000")),
            };
            let (x, y) = (place("x")?, place("y")?);
            let into = match (block, workspace) {
                (Some(block), None) if x.is_none() && y.is_none() => NoteInto::Block(block),
                (None, Some(workspace)) => NoteInto::Workspace {
                    workspace,
                    at: match (x, y) {
                        (Some(x), Some(y)) => Some((x as f32, y as f32)),
                        (None, None) => None,
                        _ => return Err("x and y go together, with workspace".into()),
                    },
                },
                (Some(_), None) => return Err("x and y go together, with workspace".into()),
                _ => return Err("open_note takes block, or workspace".into()),
            };
            Own::OpenNote { note, into }
        }
        "close_note" => Own::CloseNote {
            block: block()?,
            note: note()?,
        },
        _ => Own::DeleteNote { note: note()? },
    })
}

/// An own tool's arguments; Err says what is wrong with them.
pub fn parse_own(name: &str, args: &Value) -> Result<Own, String> {
    let number = |key: &str| {
        args[key]
            .as_u64()
            .ok_or_else(|| format!("{name} needs \"{key}\", a whole number"))
    };
    Ok(match name {
        "answer_request" => {
            let request = args["request"]
                .as_str()
                .ok_or_else(|| format!("{name} needs \"request\", a string"))?
                .to_string();
            let note = match &args["note"] {
                Value::Null => None,
                Value::String(note) => Some(note.clone()),
                _ => return Err("\"note\" is a string".into()),
            };
            let picked = match &args["answers"] {
                Value::Null => None,
                Value::Array(lists) => Some(
                    lists
                        .iter()
                        .map(|list| {
                            list.as_array()?
                                .iter()
                                .map(|a| a.as_str().map(str::to_string))
                                .collect()
                        })
                        .collect::<Option<Vec<Vec<String>>>>()
                        .ok_or("\"answers\" is a list of lists of strings")?,
                ),
                _ => return Err("\"answers\" is a list of lists of strings".into()),
            };
            let allow = match &args["allow"] {
                Value::Null => None,
                Value::Bool(allow) => Some(*allow),
                _ => return Err("\"allow\" is true or false".into()),
            };
            let dismiss = match &args["dismiss"] {
                Value::Null | Value::Bool(false) => false,
                Value::Bool(true) => true,
                _ => return Err("\"dismiss\" is true".into()),
            };
            let answer = match (picked, allow, dismiss) {
                (Some(picked), None, false) => Answer::Picked(picked),
                (None, Some(allow), false) => Answer::Allow(allow),
                (None, None, true) => Answer::Dismiss,
                _ => return Err("answer_request needs one of answers, allow or dismiss".into()),
            };
            Own::AnswerRequest {
                request,
                answer,
                note,
            }
        }
        "save_automation" => {
            if !args["automation"].is_object() {
                return Err("save_automation needs \"automation\", an object".into());
            }
            let id = match &args["id"] {
                Value::Null => None,
                id => Some(id.as_u64().ok_or("\"id\" is a whole number")?),
            };
            Own::SaveAutomation {
                id,
                automation: args["automation"].clone(),
            }
        }
        "set_automation" => Own::SetAutomation {
            id: number("automation")?,
            on: args["on"]
                .as_bool()
                .ok_or("set_automation needs \"on\", true or false")?,
        },
        "run_automation" => Own::RunAutomation {
            id: number("automation")?,
        },
        "delete_automation" => Own::DeleteAutomation {
            id: number("automation")?,
        },
        "edit_note" | "new_note" | "open_note" | "close_note" | "delete_note" => {
            parse_note_tool(name, args)?
        }
        "recent_folders" => Own::RecentFolders {
            workspace: args["workspace"]
                .as_str()
                .ok_or("recent_folders needs \"workspace\", a string")?
                .to_string(),
        },
        "cancel_schedule" => Own::CancelSchedule {
            schedule: args["schedule"]
                .as_str()
                .ok_or("cancel_schedule needs \"schedule\", a string")?
                .to_string(),
        },
        "terminal_input" => {
            let block = args["block"]
                .as_str()
                .ok_or("terminal_input needs \"block\", a string")?
                .to_string();
            let (text, keys, mouse) = (&args["text"], &args["keys"], &args["mouse"]);
            if [text, keys, mouse].iter().filter(|v| !v.is_null()).count() != 1 {
                return Err("terminal_input takes one of text, keys or mouse.".into());
            }
            let input = if !keys.is_null() {
                let keys = keys.as_array().ok_or("\"keys\" is a list of key names.")?;
                if !(1..=INPUT_KEYS).contains(&keys.len()) {
                    return Err(format!("\"keys\" holds 1 to {INPUT_KEYS} keys."));
                }
                let keys = keys
                    .iter()
                    .map(|k| k.as_str().map(str::to_string))
                    .collect::<Option<Vec<String>>>()
                    .ok_or("\"keys\" is a list of key names.")?;
                // The position, not the key: what was typed may be a secret.
                if let Some(at) = keys.iter().position(|k| named_key(k).is_none()) {
                    return Err(format!(
                        "keys[{at}] isn't a key Templum knows. Keys: {KEY_NAMES}."
                    ));
                }
                Input::Keys(keys)
            } else if !mouse.is_null() {
                let events = mouse
                    .as_array()
                    .ok_or("\"mouse\" is a list of mouse events.")?;
                if !(1..=INPUT_MOUSE).contains(&events.len()) {
                    return Err(format!("\"mouse\" holds 1 to {INPUT_MOUSE} events."));
                }
                let mut parsed = Vec::with_capacity(events.len());
                for (at, event) in events.iter().enumerate() {
                    parsed.push(mouse_of(event).ok_or_else(|| {
                        format!(
                            "mouse[{at}] isn't a mouse event: \"kind\" is press, release or move, \
                             \"button\" left, middle, right, wheel_up, wheel_down or null (a move with \
                             none held), \"row\" and \"col\" whole numbers."
                        )
                    })?);
                }
                Input::Mouse(parsed)
            } else {
                let text = text.as_str().ok_or("\"text\" is a string.")?;
                if text.len() > MAX_TEXT {
                    return Err(format!("\"text\" is at most {} KiB.", MAX_TEXT / 1024));
                }
                Input::Text(text.to_string())
            };
            Own::TerminalInput { block, input }
        }
        "share_workspace" => {
            let text = |key: &str| {
                args[key]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("{name} needs \"{key}\", a string"))
            };
            Own::ShareWorkspace {
                workspace: text("workspace")?,
                name: text("name")?,
                scope: match args["scope"].as_str() {
                    Some("control") => Scope::Control,
                    Some("full") => Scope::Full,
                    _ => return Err("\"scope\" is control or full.".into()),
                },
                expires: match &args["expires"] {
                    Value::Null => None,
                    expires => Some(
                        expires
                            .as_u64()
                            .ok_or("\"expires\" is a whole number or null.")?,
                    ),
                },
                way: match args["way"].as_str() {
                    Some("public") => Way::Public,
                    Some("pinned") => Way::Pinned,
                    _ => return Err("\"way\" is public or pinned.".into()),
                },
                address: text("address")?,
            }
        }
        "revoke_share" => Own::RevokeShare {
            share: match &args["share"] {
                Value::Null => None,
                Value::String(share) => Some(share.clone()),
                _ => return Err("\"share\" is a string".into()),
            },
        },
        "git" => {
            let text = |key: &str| match &args[key] {
                Value::Null => Ok(None),
                Value::String(text) => Ok(Some(text.clone())),
                _ => Err(format!("\"{key}\" is a string")),
            };
            let workspace = text("workspace")?
                .ok_or_else(|| format!("{name} needs \"workspace\", a string"))?;
            let op: Op = serde_json::from_value(args.clone()).map_err(|e| {
                format!(
                    "git's {} is malformed: {e}",
                    args["op"].as_str().unwrap_or("op")
                )
            })?;
            op.check()?;
            Own::Git {
                workspace,
                folder: text("folder")?,
                op,
            }
        }
        _ => return Err(format!("Unknown tool: {name}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_tool_is_known_with_a_schema() {
        let all = tools();
        let names: Vec<&str> = all.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for tool in control::list()["tools"].as_array().unwrap() {
            assert!(names.contains(&tool["name"].as_str().unwrap()));
        }
        for own in OWN {
            assert!(
                names.contains(&own) && known(own) && self::own(own),
                "{own}"
            );
        }
        assert!(
            all.iter()
                .all(|t| t["inputSchema"].is_object() && t["description"].is_string())
        );
        assert_eq!(
            names.iter().collect::<HashSet<_>>().len(),
            names.len(),
            "no name twice"
        );
        assert!(!known("nope") && !own("overview"));
    }

    #[test]
    fn scope_follows_the_registry() {
        for (name, args, want) in [
            ("overview", json!({}), Scope::Read),
            ("read_terminal", json!({"block": "b"}), Scope::Read),
            (
                "wait_for",
                json!({"block": "b", "event": "idle"}),
                Scope::Read,
            ),
            (
                "create_card",
                json!({"board": "S", "title": "x"}),
                Scope::Control,
            ),
            ("move_block", json!({"block": "b", "x": 1}), Scope::Control),
            (
                "open_agent",
                json!({"agent": "claude", "workspace": "w3"}),
                Scope::Control,
            ),
            (
                "delegate",
                json!({"block": "b", "task": "t"}),
                Scope::Control,
            ),
            ("close_block", json!({"block": "b"}), Scope::Control),
            (
                "edit_item",
                json!({"block": "c", "item": 13, "text": "x"}),
                Scope::Control,
            ),
            (
                "delete_item",
                json!({"block": "c", "item": 13}),
                Scope::Control,
            ),
            (
                "send_to_terminal",
                json!({"block": "b", "text": "ls"}),
                Scope::Full,
            ),
            (
                "send_keys",
                json!({"block": "b", "keys": ["y"]}),
                Scope::Full,
            ),
            (
                "answer_request",
                json!({"request": "r1", "allow": true}),
                Scope::Control,
            ),
            (
                "save_automation",
                json!({"automation": {"do": {"notify": {"text": "x"}}}}),
                Scope::Control,
            ),
            (
                "save_automation",
                json!({"automation": {"do": {"command": {"text": "ls"}}}}),
                Scope::Full,
            ),
            (
                "set_automation",
                json!({"automation": 1, "on": false}),
                Scope::Control,
            ),
            ("run_automation", json!({"automation": 1}), Scope::Control),
            ("delete_automation", json!({"automation": 1}), Scope::Full),
            ("cancel_schedule", json!({"schedule": "s3"}), Scope::Control),
            ("recent_folders", json!({"workspace": "w3"}), Scope::Read),
            (
                "edit_note",
                json!({"block": "n1", "note": 7, "name": "x"}),
                Scope::Control,
            ),
            ("new_note", json!({"block": "n1"}), Scope::Control),
            (
                "open_note",
                json!({"note": 7, "block": "n1"}),
                Scope::Control,
            ),
            (
                "close_note",
                json!({"block": "n1", "note": 7}),
                Scope::Control,
            ),
            ("delete_note", json!({"note": 7}), Scope::Full),
        ] {
            assert_eq!(scope(name, &args), Ok(want), "{name}");
        }
        assert!(
            scope("move_card", &json!({"card": 4}))
                .unwrap_err()
                .contains("column")
        );
        assert_eq!(scope("nope", &json!({})), Err("Unknown tool: nope".into()));
    }

    #[test]
    fn the_listed_scopes_are_the_nominal_ones() {
        let by_name = |name: &str| nominal(name);
        assert_eq!(by_name("overview"), "read");
        assert_eq!(by_name("create_card"), "control");
        assert_eq!(by_name("send_keys"), "full");
        assert_eq!(by_name("delete_automation"), "full");
        assert_eq!(by_name("answer_request"), "control or full");
        assert_eq!(by_name("save_automation"), "control or full");
        let listed = listed();
        assert_eq!(listed.len(), tools().len());
        assert!(listed.iter().all(|t| t["scope"].is_string()));
    }

    #[test]
    fn git_takes_the_scope_of_its_operation() {
        assert_eq!(scope("git", &json!({"op": "look"})), Ok(Scope::Read));
        assert_eq!(
            scope("git", &json!({"op": "stage", "paths": ["a"]})),
            Ok(Scope::Control)
        );
        let full = [
            "discard",
            "discard_hunk",
            "restore_version",
            "delete_branch",
            "stash_drop",
            "abort",
        ];
        for op in full {
            assert_eq!(git_scope(op), Some(Scope::Full), "{op}");
        }
        // Every operation has a scope: exactly those six are Full, the 16 reads Read.
        let scopes: Vec<_> = OPS.iter().map(|op| git_scope(op).unwrap()).collect();
        assert_eq!(scopes.iter().filter(|s| **s == Scope::Full).count(), 6);
        assert_eq!(scopes.iter().filter(|s| **s == Scope::Read).count(), 16);
        assert_eq!(scopes.len(), 52);
        assert_eq!(git_scope("rm"), None);
        assert_eq!(nominal("git"), "read, control or full, by op");
    }

    #[test]
    fn an_unknown_git_operation_is_invalid() {
        let words = "There's no git operation rm: GET /v1/tools lists them.";
        assert_eq!(scope("git", &json!({"op": "rm"})), Err(words.into()));
        assert!(
            scope("git", &json!({}))
                .unwrap_err()
                .starts_with("git needs \"op\"")
        );
    }

    #[test]
    fn git_parses_into_its_operation_and_refuses_bad_shapes() {
        assert_eq!(
            parse_own(
                "git",
                &json!({"workspace": "w3", "folder": "src", "op": "stage", "paths": ["a.rs"]})
            ),
            Ok(Own::Git {
                workspace: "w3".into(),
                folder: Some("src".into()),
                op: Op::Stage {
                    paths: vec!["a.rs".into()]
                },
            })
        );
        let bad = parse_own(
            "git",
            &json!({"workspace": "w3", "op": "stage", "paths": ["../x"]}),
        );
        assert!(
            bad.unwrap_err()
                .starts_with("\"../x\" isn't a path inside the repository")
        );
        assert!(
            parse_own("git", &json!({"op": "look"}))
                .unwrap_err()
                .contains("workspace")
        );
        assert!(
            parse_own("git", &json!({"workspace": "w3", "op": "stage"}))
                .unwrap_err()
                .contains("paths")
        );
    }

    #[test]
    fn needs_words_name_the_git_operation() {
        assert_eq!(
            needs_words("git", &json!({"op": "discard"}), Scope::Full),
            "git's discard needs a Full token."
        );
        assert_eq!(
            needs_words("send_keys", &json!({}), Scope::Full),
            "send_keys needs a Full token."
        );
    }

    #[test]
    fn the_share_tools_parse_and_route() {
        let share = json!({"workspace": "w3", "name": "x", "scope": "full", "expires": null,
            "way": "public", "address": "https://t"});
        assert_eq!(
            parse_own("share_workspace", &share),
            Ok(Own::ShareWorkspace {
                workspace: "w3".into(),
                name: "x".into(),
                scope: Scope::Full,
                expires: None,
                way: Way::Public,
                address: "https://t".into()
            })
        );
        let with = |key: &str, value: Value| {
            let mut args = share.clone();
            args[key] = value;
            parse_own("share_workspace", &args)
        };
        assert_eq!(
            with("scope", json!("read")),
            Err("\"scope\" is control or full.".into())
        );
        assert_eq!(
            with("way", json!("lan")),
            Err("\"way\" is public or pinned.".into())
        );
        assert_eq!(
            with("expires", json!("soon")),
            Err("\"expires\" is a whole number or null.".into())
        );
        assert!(matches!(
            with("expires", json!(99)),
            Ok(Own::ShareWorkspace {
                expires: Some(99),
                ..
            })
        ));
        assert_eq!(
            parse_own("revoke_share", &json!({})),
            Ok(Own::RevokeShare { share: None })
        );
        assert_eq!(
            parse_own("revoke_share", &json!({"share": "s4"})),
            Ok(Own::RevokeShare {
                share: Some("s4".into())
            })
        );
        let (route, params) = find("POST", "/v1/workspaces/w3/shares").unwrap();
        assert_eq!(
            (route.does, params),
            (
                Does::Tool("share_workspace"),
                vec![("workspace", "w3".to_string())]
            )
        );
        let (route, params) = find("DELETE", "/v1/shares/s4").unwrap();
        assert_eq!(
            (route.does, params),
            (
                Does::Tool("revoke_share"),
                vec![("share", "s4".to_string())]
            )
        );
        assert_eq!(scope("share_workspace", &share), Ok(Scope::Full));
        assert_eq!(scope("revoke_share", &json!({})), Ok(Scope::Read));
        assert_eq!(nominal("share_workspace"), "full");
        assert_eq!(nominal("revoke_share"), "full, or the share itself");
        assert_eq!(
            no("share", "s999"),
            "There's no share s999: GET /v1/shares lists them."
        );
    }

    #[test]
    fn the_board_and_checklist_tools_take_a_control_token() {
        for (name, args) in [
            ("delete_card", json!({"card": 4})),
            ("edit_board", json!({"board": 1, "name": "x"})),
            ("edit_board", json!({"board": 1, "delete": true})),
            ("edit_column", json!({"board": 1})),
            (
                "edit_column",
                json!({"board": 1, "column": 3, "delete": true}),
            ),
            ("show_board", json!({"block": "k", "board": 1})),
            ("edit_checklist", json!({"block": "c", "name": "x"})),
            (
                "edit_checklist",
                json!({"block": "c", "item": 13, "before": "end"}),
            ),
            ("new_checklist", json!({"block": "c"})),
        ] {
            assert_eq!(scope(name, &args), Ok(Scope::Control), "{name} {args}");
        }
    }

    #[test]
    fn every_argument_is_a_target_or_not() {
        let mut unclassified = Vec::new();
        for tool in tools() {
            for key in tool["inputSchema"]["properties"]
                .as_object()
                .unwrap()
                .keys()
            {
                if !(TARGETS.contains(&key.as_str()) || NOT_TARGETS.contains(&key.as_str())) {
                    unclassified.push(format!("{}.{key}", tool["name"].as_str().unwrap()));
                }
            }
        }
        assert!(
            unclassified.is_empty(),
            "neither a target nor not: {unclassified:?}"
        );
        assert!(
            TARGETS.iter().all(|t| !NOT_TARGETS.contains(t)),
            "no name is both"
        );
        // And no name stays on a list after the last tool that had it.
        let used: HashSet<String> = tools()
            .iter()
            .flat_map(|t| {
                t["inputSchema"]["properties"]
                    .as_object()
                    .unwrap()
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .collect();
        let stale: Vec<_> = TARGETS
            .iter()
            .chain(NOT_TARGETS)
            .filter(|n| !used.contains(**n))
            .collect();
        assert!(stale.is_empty(), "no tool has these arguments: {stale:?}");
    }

    #[test]
    fn routes_find_their_tool_and_parameters() {
        let (route, params) = find("POST", "/v1/cards/40/move").unwrap();
        assert_eq!(
            (route.does, params),
            (Does::Tool("move_card"), vec![("card", "40".to_string())])
        );
        let (route, params) = find("DELETE", "/v1/wires/b1/k1").unwrap();
        assert_eq!(route.does, Does::Tool("disconnect"));
        assert_eq!(params, [("a", "b1".to_string()), ("b", "k1".to_string())]);
        let (route, params) = find("POST", "/v1/tools/move_card").unwrap();
        assert_eq!(
            (route.does, params),
            (Does::Tool(""), vec![("name", "move_card".to_string())])
        );
        let (route, params) = find("GET", "/v1/blocks/b%37k").unwrap();
        assert_eq!(
            (route.does, params),
            (
                Does::Read(Resource::Block),
                vec![("block", "b7k".to_string())]
            )
        );
        assert!(find("GET", "/v2/state").is_none() && find("GET", "/v1/state/").is_none());
        assert!(
            find("GET", "/v1/blocks/").is_none(),
            "an empty segment is no block"
        );
        assert!(find("GET", "/v1/blocks/%ff").is_none(), "not UTF-8");
        assert!(find("GET", "/v1/blocks/%zz").is_none() && find("GET", "/v1/nope").is_none());
        assert!(
            find("PUT", "/v1/state").is_none(),
            "a method the path doesn't take"
        );
        assert_eq!(find("GET", "/v1/events").unwrap().0.does, Does::Events);
        assert_eq!(find("POST", "/v1/tickets").unwrap().0.does, Does::Ticket);
        for route in ROUTES {
            if let Does::Tool(name) = route.does
                && !name.is_empty()
            {
                assert!(known(name), "{} {}", route.method, route.path);
                // Its path parameters are its arguments.
                let schema = tools().into_iter().find(|t| t["name"] == name).unwrap();
                for segment in route
                    .path
                    .split('/')
                    .filter_map(|s| s.strip_prefix('{')?.strip_suffix('}'))
                {
                    let key = if name == "save_automation" {
                        "id"
                    } else {
                        segment
                    };
                    assert!(
                        schema["inputSchema"]["properties"].get(key).is_some(),
                        "{} {segment}",
                        route.path
                    );
                }
            }
        }
        let mut seen = HashSet::new();
        assert!(
            ROUTES.iter().all(|r| seen.insert((r.method, r.path))),
            "no route twice"
        );
    }

    #[test]
    fn arguments_are_typed_by_the_schema_and_the_path_wins() {
        let text = |s: &str| s.to_string();
        assert_eq!(
            arguments(
                "read_terminal",
                &[("block", text("b7k2m"))],
                &[
                    (text("lines"), text("50")),
                    (text("last_command"), text("true"))
                ],
                None
            ),
            Ok(json!({"block": "b7k2m", "lines": 50, "last_command": true}))
        );
        assert_eq!(
            arguments(
                "move_card",
                &[("card", text("40"))],
                &[],
                Some(json!({"card": 99, "column": "Done"}))
            ),
            Ok(json!({"card": 40, "column": "Done"}))
        );
        assert_eq!(
            arguments(
                "read_terminal",
                &[("block", text("b"))],
                &[(text("lines"), text("many"))],
                None
            ),
            Err("\"lines\" is a whole number".into())
        );
        assert_eq!(
            arguments(
                "read_terminal",
                &[("block", text("b"))],
                &[(text("last_command"), text("yes"))],
                None
            ),
            Err("\"last_command\" is true or false".into())
        );
        assert_eq!(
            arguments(
                "send_keys",
                &[("block", text("b"))],
                &[(text("keys"), text("a")), (text("keys"), text("b"))],
                None
            ),
            Ok(json!({"block": "b", "keys": ["a", "b"]}))
        );
        assert_eq!(
            arguments(
                "show",
                &[],
                &[(text("nonsense"), text("1"))],
                Some(json!({"workspace": "w3"}))
            ),
            Ok(json!({"workspace": "w3"})),
            "an unknown query key is ignored"
        );
        assert!(arguments("show", &[], &[], Some(json!([1]))).is_err());
        assert_eq!(arguments("show", &[], &[], None), Ok(json!({})));
        assert!(
            arguments("nope", &[], &[], None)
                .unwrap_err()
                .contains("Unknown tool")
        );
    }

    #[test]
    fn a_saved_automation_comes_from_the_body_and_the_path_gives_its_id() {
        let rule = json!({"name": "x", "do": {"notify": {"text": "t"}}});
        assert_eq!(
            arguments("save_automation", &[], &[], Some(rule.clone())),
            Ok(json!({"automation": rule}))
        );
        assert_eq!(
            arguments(
                "save_automation",
                &[("automation", "3".into())],
                &[],
                Some(rule.clone())
            ),
            Ok(json!({"automation": rule, "id": 3}))
        );
        assert_eq!(
            arguments(
                "save_automation",
                &[],
                &[],
                Some(json!({"automation": rule, "id": 2}))
            ),
            Ok(json!({"automation": rule, "id": 2}))
        );
    }

    #[test]
    fn terminal_input_parses_mouse_events() {
        let press = json!({"kind": "press", "button": "left", "row": 3, "col": 10});
        let Own::TerminalInput {
            input: Input::Mouse(events),
            ..
        } = parse_own(
            "terminal_input",
            &json!({"block": "b", "mouse": [press.clone()]}),
        )
        .unwrap()
        else {
            panic!("mouse")
        };
        assert_eq!(
            events,
            [MouseInput {
                button: Some(Button::Left),
                kind: MouseKind::Press,
                row: 3,
                col: 10,
                ctrl: false,
                alt: false,
                shift: false
            }]
        );
        let hover = json!({"kind": "move", "button": null, "row": 0, "col": 0});
        assert!(
            parse_own(
                "terminal_input",
                &json!({"block": "b", "mouse": vec![hover.clone(); INPUT_MOUSE]})
            )
            .is_ok()
        );
        let err = |args: Value| parse_own("terminal_input", &args).unwrap_err();
        let holds = "\"mouse\" holds 1 to 64 events.";
        assert_eq!(
            err(json!({"block": "b", "mouse": vec![hover; INPUT_MOUSE + 1]})),
            holds
        );
        assert_eq!(err(json!({"block": "b", "mouse": []})), holds);
        assert_eq!(
            err(json!({"block": "b", "mouse": "click"})),
            "\"mouse\" is a list of mouse events."
        );
        let bad = err(
            json!({"block": "b", "mouse": [press.clone(), press.clone(), press.clone(),
            {"kind": "press", "button": null, "row": 0, "col": 0}]}),
        );
        assert!(
            bad.starts_with("mouse[3] isn't a mouse event: \"kind\" is press, release or move"),
            "{bad}"
        );
        assert_eq!(
            err(json!({"block": "b", "keys": ["a"], "mouse": [press.clone()]})),
            "terminal_input takes one of text, keys or mouse."
        );
        assert_eq!(
            scope("terminal_input", &json!({"block": "b", "mouse": [press]})),
            Ok(Scope::Full)
        );
    }

    #[test]
    fn a_mouse_event_reads_back_as_it_was_written() {
        for button in [
            None,
            Some(Button::Left),
            Some(Button::Middle),
            Some(Button::Right),
            Some(Button::WheelUp),
            Some(Button::WheelDown),
        ] {
            let e = MouseInput {
                button,
                kind: if button.is_none() {
                    MouseKind::Move
                } else {
                    MouseKind::Release
                },
                row: 7,
                col: 2000,
                ctrl: true,
                alt: false,
                shift: true,
            };
            assert_eq!(mouse_of(&mouse_value(&e)), Some(e));
        }
        let far = json!({"kind": "move", "row": 1e30, "col": 18446744073709551615u64});
        let e = mouse_of(&far).expect("a cell past usize is the last one");
        assert_eq!((e.row, e.col), (usize::MAX, usize::MAX));
        assert_eq!(
            mouse_of(&json!({"kind": "move", "row": -1, "col": 0})),
            None
        );
        assert_eq!(
            mouse_of(&json!({"kind": "move", "row": 1.5, "col": 0})),
            None
        );
    }

    #[test]
    fn terminal_input_parses_text_or_keys() {
        assert_eq!(
            parse_own("terminal_input", &json!({"block": "b", "text": "ls\n"})),
            Ok(Own::TerminalInput {
                block: "b".into(),
                input: Input::Text("ls\n".into())
            })
        );
        assert_eq!(
            parse_own(
                "terminal_input",
                &json!({"block": "b", "keys": ["escape", "j"]})
            ),
            Ok(Own::TerminalInput {
                block: "b".into(),
                input: Input::Keys(vec!["escape".into(), "j".into()])
            })
        );
        let err = |args: Value| parse_own("terminal_input", &args).unwrap_err();
        let one = "terminal_input takes one of text, keys or mouse.";
        assert_eq!(err(json!({"block": "b"})), one);
        assert_eq!(err(json!({"block": "b", "text": "x", "keys": ["y"]})), one);
        assert_eq!(
            err(json!({"block": "b", "keys": vec!["y"; INPUT_KEYS + 1]})),
            "\"keys\" holds 1 to 256 keys."
        );
        assert!(
            parse_own(
                "terminal_input",
                &json!({"block": "b", "keys": vec!["y"; INPUT_KEYS]})
            )
            .is_ok()
        );
        assert!(
            parse_own(
                "terminal_input",
                &json!({"block": "b", "keys": ["ctrl+shift+f5"]})
            )
            .is_ok()
        );
        assert_eq!(
            err(json!({"block": "b", "keys": []})),
            "\"keys\" holds 1 to 256 keys."
        );
        assert!(
            err(json!({"block": "b", "keys": ["f13"]}))
                .starts_with("keys[0] isn't a key Templum knows. Keys: [ctrl+]")
        );
        // The position is named, never the key: it may be a password.
        let refused = err(json!({"block": "b", "keys": ["enter", "hunter2"]}));
        assert!(refused.starts_with("keys[1] isn't a key Templum knows."));
        assert!(!refused.contains("hunter2"));
        assert_eq!(
            err(json!({"block": "b", "keys": "y"})),
            "\"keys\" is a list of key names."
        );
        assert_eq!(
            err(json!({"block": "b", "keys": [1]})),
            "\"keys\" is a list of key names."
        );
        assert_eq!(
            err(json!({"block": "b", "text": "x".repeat(MAX_TEXT + 1)})),
            "\"text\" is at most 256 KiB."
        );
        assert_eq!(
            err(json!({"block": "b", "text": 3})),
            "\"text\" is a string."
        );
        assert!(
            parse_own(
                "terminal_input",
                &json!({"block": "b", "text": "x".repeat(MAX_TEXT)})
            )
            .is_ok()
        );
        assert!(err(json!({"text": "x"})).contains("\"block\""));
        let typed = parse_own("terminal_input", &json!({"block": "b", "text": "s3cr3t"}));
        assert!(
            !format!("{typed:?}").contains("s3cr3t"),
            "Debug never shows what is typed"
        );
        assert_eq!(scope("terminal_input", &json!({})), Ok(Scope::Full));
        assert_eq!(nominal("terminal_input"), "full");
        let (route, params) = find("POST", "/v1/terminals/b7k2m/input").unwrap();
        assert_eq!(
            (route.does, params),
            (
                Does::Tool("terminal_input"),
                vec![("block", "b7k2m".to_string())]
            )
        );
    }

    #[test]
    fn own_tools_parse_their_arguments() {
        assert_eq!(
            parse_own(
                "answer_request",
                &json!({"request": "r1", "answers": [["A"]]})
            ),
            Ok(Own::AnswerRequest {
                request: "r1".into(),
                answer: Answer::Picked(vec![vec!["A".into()]]),
                note: None
            })
        );
        assert_eq!(
            parse_own(
                "answer_request",
                &json!({"request": "r1", "allow": false, "note": "no"})
            ),
            Ok(Own::AnswerRequest {
                request: "r1".into(),
                answer: Answer::Allow(false),
                note: Some("no".into())
            })
        );
        assert_eq!(
            parse_own("answer_request", &json!({"request": "r1", "dismiss": true})),
            Ok(Own::AnswerRequest {
                request: "r1".into(),
                answer: Answer::Dismiss,
                note: None
            })
        );
        for bad in [
            json!({"request": "r1"}),
            json!({"request": "r1", "allow": true, "dismiss": true}),
            json!({"request": "r1", "allow": true, "answers": []}),
            json!({"request": "r1", "dismiss": false}),
        ] {
            assert_eq!(
                parse_own("answer_request", &bad),
                Err("answer_request needs one of answers, allow or dismiss".into()),
                "{bad}"
            );
        }
        assert!(
            parse_own("answer_request", &json!({"allow": true}))
                .unwrap_err()
                .contains("\"request\"")
        );
        assert!(parse_own("answer_request", &json!({"request": "r", "answers": [1]})).is_err());
        assert_eq!(
            parse_own(
                "save_automation",
                &json!({"automation": {"name": "x"}, "id": 2})
            ),
            Ok(Own::SaveAutomation {
                id: Some(2),
                automation: json!({"name": "x"})
            })
        );
        assert!(parse_own("save_automation", &json!({"automation": 3})).is_err());
        assert_eq!(
            parse_own("set_automation", &json!({"automation": 3, "on": false})),
            Ok(Own::SetAutomation { id: 3, on: false })
        );
        assert!(parse_own("set_automation", &json!({"automation": 3})).is_err());
        assert_eq!(
            parse_own("run_automation", &json!({"automation": 3})),
            Ok(Own::RunAutomation { id: 3 })
        );
        assert_eq!(
            parse_own("delete_automation", &json!({"automation": 3})),
            Ok(Own::DeleteAutomation { id: 3 })
        );
        assert!(parse_own("delete_automation", &json!({"automation": "x"})).is_err());
        assert_eq!(
            parse_own("cancel_schedule", &json!({"schedule": "s3"})),
            Ok(Own::CancelSchedule {
                schedule: "s3".into()
            })
        );
        assert_eq!(
            parse_own("cancel_schedule", &json!({})),
            Err("cancel_schedule needs \"schedule\", a string".into())
        );
        assert!(own("cancel_schedule") && known("cancel_schedule"));
        assert_eq!(
            parse_own("recent_folders", &json!({"workspace": "w3"})),
            Ok(Own::RecentFolders {
                workspace: "w3".into()
            })
        );
        assert_eq!(
            parse_own("recent_folders", &json!({})),
            Err("recent_folders needs \"workspace\", a string".into())
        );
        assert_eq!(nominal("recent_folders"), "read");
        assert_eq!(nominal("delete_note"), "full");
        assert_eq!(nominal("edit_note"), "control");
        assert!(parse_own("overview", &json!({})).is_err());
    }

    #[test]
    fn the_note_tools_parse_and_say_what_is_missing() {
        let edit = |args: Value| parse_own("edit_note", &args);
        assert_eq!(
            edit(
                json!({"block": "n1", "note": 7, "name": "Plan", "color": "blue",
                        "text": "x", "rev": 5})
            ),
            Ok(Own::EditNote {
                block: "n1".into(),
                note: 7,
                name: Some("Plan".into()),
                color: Some(Some("blue".into())),
                text: Some(("x".into(), 5)),
            })
        );
        assert_eq!(
            edit(json!({"block": "n1", "note": "7", "color": "none"})),
            Ok(Own::EditNote {
                block: "n1".into(),
                note: 7,
                name: None,
                color: Some(None),
                text: None,
            }),
            "a numeric string is a number; none clears"
        );
        assert_eq!(
            edit(json!({"block": "n1", "note": 7})),
            Err("edit_note takes name, color, or text with rev".into())
        );
        let together = Err(
            "edit_note's text and rev go together: rev is the note's rev in the \
                            state, which the text was made on"
                .to_string(),
        );
        assert_eq!(
            edit(json!({"block": "n1", "note": 7, "text": "x"})),
            together
        );
        assert_eq!(edit(json!({"block": "n1", "note": 7, "rev": 3})), together);
        assert_eq!(
            edit(json!({"block": "n1", "note": 7, "color": "plaid"})),
            Err("\"color\" is red, yellow, green, cyan, blue, magenta or none".into())
        );
        let long = "x".repeat(NOTE_TEXT_MOST + 1);
        let most = Err("A note through the API holds at most 256 KiB.".to_string());
        assert_eq!(
            edit(json!({"block": "n1", "note": 7, "text": long, "rev": 1})),
            most
        );
        assert_eq!(
            parse_own("new_note", &json!({"block": "n1", "text": long})),
            most
        );
        assert!(
            edit(json!({"block": "n1", "note": 7, "text": "x".repeat(NOTE_TEXT_MOST), "rev": 1}))
                .is_ok()
        );
        assert_eq!(
            edit(json!({"block": "n1", "note": 1.5, "name": "x"})),
            Err("\"note\" is a note's id, as the state shows it".into())
        );
        assert_eq!(
            parse_own("new_note", &json!({"block": "n1", "name": "Plan"})),
            Ok(Own::NewNote {
                block: "n1".into(),
                name: Some("Plan".into()),
                text: None
            })
        );
        assert_eq!(
            parse_own("new_note", &json!({})),
            Err("new_note needs \"block\", a string".into())
        );

        let open = |args: Value| parse_own("open_note", &args);
        assert_eq!(
            open(json!({"note": 5, "block": "n1"})),
            Ok(Own::OpenNote {
                note: 5,
                into: NoteInto::Block("n1".into())
            })
        );
        assert_eq!(
            open(json!({"note": 5, "workspace": "w1", "x": 10, "y": -20.5})),
            Ok(Own::OpenNote {
                note: 5,
                into: NoteInto::Workspace {
                    workspace: "w1".into(),
                    at: Some((10.0, -20.5))
                }
            })
        );
        assert_eq!(
            open(json!({"note": 5, "workspace": "w1"})),
            Ok(Own::OpenNote {
                note: 5,
                into: NoteInto::Workspace {
                    workspace: "w1".into(),
                    at: None
                }
            })
        );
        let either = Err("open_note takes block, or workspace".to_string());
        assert_eq!(open(json!({"note": 5})), either);
        assert_eq!(
            open(json!({"note": 5, "block": "n1", "workspace": "w1"})),
            either
        );
        let pair = Err("x and y go together, with workspace".to_string());
        assert_eq!(
            open(json!({"note": 5, "block": "n1", "x": 1, "y": 2})),
            pair
        );
        assert_eq!(open(json!({"note": 5, "workspace": "w1", "x": 1})), pair);
        assert_eq!(
            open(json!({"note": 5, "workspace": "w1", "x": 1e9, "y": 2})),
            Err("\"x\" is within ±10000000".into())
        );
        assert_eq!(
            parse_own("close_note", &json!({"block": "n1", "note": 7})),
            Ok(Own::CloseNote {
                block: "n1".into(),
                note: 7
            })
        );
        assert_eq!(
            parse_own("delete_note", &json!({"note": 7})),
            Ok(Own::DeleteNote { note: 7 })
        );
        assert_eq!(
            parse_own("delete_note", &json!({})),
            Err("\"note\" is a note's id, as the state shows it".into())
        );
        assert!(
            [
                "edit_note",
                "new_note",
                "open_note",
                "close_note",
                "delete_note"
            ]
            .iter()
            .all(|n| own(n) && known(n))
        );
    }
}
