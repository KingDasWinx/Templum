//! The Templum API, served (docs/specs/api.md): the server's messages answered on the UI thread
//! with the token's scope and workspaces checked, the store and the server's life, and the tick
//! that hands the state to the event streams while any is open.

use std::time::{Duration, Instant, SystemTime};

use gpui_kit::component::notification::Notification;
use gpui_kit::{AppContext as _, Context, Window};
use serde_json::{Value, json};
use templum_agent::Target;
use templum_api::registry::{self, Answer as Picked, Input, Own};
use templum_api::store::{self, Scope, Settings, Share, Tls, Way};
use templum_api::{Caller, Notice, Server, Store, TerminalRef, codes};
use templum_mcp::control;
use templum_mcp::{Hello, Incoming, Replier, Reply, rpc, tools};

use super::bus::Outcome;
use super::control::Actor;
use super::waits::{Asked, Waiting};
use super::{Drag, Space, Workspace, show_notification};
use crate::automations::{Automation, Do};
use crate::chat::Answer as Approve;

/// How often the state is handed to the streams, and the terminals lent to the terminal streams,
/// while any is open.
const TICK: Duration = Duration::from_millis(250);
/// The tick while the pointer moves a block or the canvas, or the camera moved a moment ago (a
/// touchpad pan, the wheel, a pinch), so a mirror follows.
const FAST_TICK: Duration = Duration::from_millis(100);
/// How long after the camera moved the tick stays fast.
pub(super) const MOVED_FOR: Duration = Duration::from_millis(250);
/// How often the tokens' last uses are written.
const USES_EVERY: Duration = Duration::from_secs(60);

/// The API's state in the workspace: its file, its server, and what Settings shows.
#[derive(Default)]
pub(crate) struct Api {
    /// The settings and tokens, as the file keeps them; None without a data folder.
    pub store: Option<Store>,
    /// Listening, while it is on and could start.
    pub server: Option<Server>,
    /// Why it isn't listening, when it is on.
    pub error: Option<String>,
    /// A token just made, shown once in the dialog (`token_seen` clears it).
    pub shown: Option<String>,
    /// The bus's channel, where the server's messages go.
    pub bus: Option<super::bus::Sender>,
    /// Templum's certificate's fingerprint, read when the server starts with it.
    pub fingerprint: Option<String>,
    /// How many clients are connected, as the server last said.
    pub clients: usize,
    /// The token uses not in the file yet, and when the file was last written (the executor's clock).
    pub(super) dirty: bool,
    saved: Option<std::time::Instant>,
    /// The tick that hands the state to the streams is running.
    pub(crate) ticking: bool,
    /// The canvas's size in screen pixels at the last paint (zero before the first): with the
    /// camera it makes the state's `view`.
    pub(super) viewport: templum_canvas::Size,
    /// When the camera last moved without a drag (`pan_by`, `fly`, a marquee's edge scroll): the
    /// tick follows it.
    pub(super) moved_at: Option<std::time::Instant>,
    /// Calls to `delegate` with `wait` still waiting, by connection: if their client leaves, the
    /// task is no longer handed to it.
    delegated: Vec<u64>,
    /// The `git` calls waiting their turn on each repository (by its root); the first of a root
    /// that has an entry is running.
    pub(super) git: std::collections::HashMap<
        std::path::PathBuf,
        std::collections::VecDeque<super::api_git::GitTurn>,
    >,
    /// What `api_notice` sent, for the tests (no server listens in them).
    #[cfg(test)]
    pub notices: Vec<Notice>,
    /// What `api_say` told the person, for the tests.
    #[cfg(test)]
    pub said: Vec<String>,
}

impl Api {
    pub(super) fn new(bus: Option<super::bus::Sender>) -> Api {
        Api {
            bus,
            ..Default::default()
        }
    }
}

fn now_secs() -> u64 {
    super::automations::unix(SystemTime::now()).max(0) as u64
}

/// An error line for the server: its code and words.
pub(super) type Refusal = (i32, String);

pub(super) fn not_found(word: &str, id: &str) -> Refusal {
    (codes::NOT_FOUND, registry::no(word, id))
}

fn params(why: impl Into<String>) -> Refusal {
    (rpc::PARAMS, why.into())
}

/// What the Share… dialog and `share_workspace` ask of `Workspace::share_workspace`.
pub(crate) struct ShareDraft {
    /// A local workspace, by its id or name.
    pub workspace: String,
    pub name: String,
    pub scope: Scope,
    /// Unix seconds; None: never.
    pub expires: Option<u64>,
    pub way: Way,
    pub address: String,
}

/// A `tools/call` answer for the API: the text, and what it touched.
pub(super) fn answer_line(id: &Value, reply: &Reply, target: Option<&Target>) -> String {
    let mut result = json!({ "content": [{ "type": "text", "text": reply.text }] });
    if reply.is_error {
        result["isError"] = json!(true);
    }
    if let Some(target) = target {
        result["target"] = json!({ "workspace": target.workspace, "block": target.block });
    }
    rpc::result(id, result)
}

/// `recent_folders`' answer: a workspace's recent folders, newest first, a folder a line. For a
/// token limited to `roots`, only those inside them, as their real paths (a link's own path may
/// lie outside), each once. A folder that isn't one line is left out.
fn recent_lines(recent: &[std::path::PathBuf], roots: Option<&[std::path::PathBuf]>) -> String {
    let mut lines: Vec<String> = Vec::new();
    for folder in recent {
        let folder = match roots {
            Some(roots) => match super::api_git::inside(folder, roots) {
                Some(real) => real,
                None => continue,
            },
            None => folder.clone(),
        };
        let line = folder.display().to_string();
        if !line.is_empty() && !line.chars().any(char::is_control) && !lines.contains(&line) {
            lines.push(line);
        }
    }
    lines.join("\n")
}

impl Workspace {
    /// The token whose id is `terminal` (a hello's), if it is live: who is asking.
    fn api_caller(&self, terminal: &str) -> Option<Caller> {
        let id: u64 = terminal.parse().ok()?;
        let now = now_secs();
        self.api
            .store
            .as_ref()?
            .tokens
            .iter()
            .find(|t| t.id == id && t.live(now))
            .map(Caller::of)
    }

    /// Answers one message of the API's: the state, a tool call, or the end of a client.
    pub(super) fn serve_api(
        &mut self,
        hello: Hello,
        conn: u64,
        incoming: Incoming,
        reply: Replier,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (id, method, params) = match incoming {
            Incoming::Request { id, method, params } => (id, method, params),
            Incoming::Notification { method, params } => {
                match method.as_str() {
                    "notifications/cancelled" => self.cancel(conn, &params["requestId"], cx),
                    templum_mcp::bus::CLOSED => self.api_closed(conn, cx),
                    templum_api::WATCH => self.start_api_tick(cx),
                    templum_api::CLIENTS => self.api_clients(cx),
                    _ => {}
                }
                return;
            }
            Incoming::Response => return,
        };
        let Some(caller) = self.api_caller(&hello.terminal) else {
            let _ = reply.send(rpc::error(&id, codes::AUTH, "This token was revoked."));
            return;
        };
        let line = match method.as_str() {
            templum_api::STATE => {
                let state = self.api_state(cx).visible(&caller);
                rpc::result(&id, serde_json::to_value(state).unwrap_or(Value::Null))
            }
            "tools/call" => {
                let name = params["name"].as_str().unwrap_or_default().to_string();
                match self.api_call(
                    caller,
                    conn,
                    &id,
                    &reply,
                    &name,
                    &params["arguments"],
                    window,
                    cx,
                ) {
                    Ok(None) => return,
                    Ok(Some(line)) => line,
                    Err((code, why)) => rpc::error(&id, code, &why),
                }
            }
            templum_api::TERMINAL => {
                let block = params["block"].as_str().unwrap_or_default();
                match self.api_terminal(&caller, block, cx) {
                    Ok(done) => answer_line(&id, &done, None),
                    Err((code, why)) => rpc::error(&id, code, &why),
                }
            }
            _ => rpc::error(&id, rpc::METHOD, &format!("Method not found: {method}")),
        };
        let _ = reply.send(line);
    }

    /// Every terminal block of every workspace, as the server is lent them for the terminal
    /// streams: its uid, its workspace's id and its grid.
    pub(crate) fn api_terminals(&self) -> Vec<TerminalRef> {
        self.order()
            .into_iter()
            .flat_map(|space| {
                space.terminals().filter_map(move |(id, pane)| {
                    Some(TerminalRef {
                        block: space.uids.get(&id)?.clone(),
                        workspace: space.id.clone(),
                        term: pane.term.shared().clone(),
                    })
                })
            })
            .collect()
    }

    /// A terminal stream opens: the block is the token's to see and a terminal, or it is refused
    /// as a tool refuses (the server answers `404`, `422`); the terminals are lent, and the tick
    /// lends them again while the stream is open, so a closed or moved block ends it.
    fn api_terminal(
        &mut self,
        caller: &Caller,
        block: &str,
        cx: &mut Context<Self>,
    ) -> Result<Reply, Refusal> {
        self.api_allows(
            caller,
            templum_api::TERMINAL,
            &json!({ "block": block }),
            cx,
        )?;
        let Some((space, id)) = self.find_block(block) else {
            return Err(not_found("block", block));
        };
        if self
            .space_named(&space)
            .and_then(|s| s.terminal(id))
            .is_none()
        {
            return Ok(Reply::err(format!("{block} isn't a terminal.")));
        }
        if let Some(server) = &self.api.server {
            server.set_terminals(self.api_terminals());
        }
        self.start_api_tick(cx);
        Ok(Reply::ok(format!("Streaming {block}.")))
    }

    /// A tool call: its scope and targets checked, then run. Ok(None): it waits, and its reply
    /// goes later.
    #[allow(clippy::too_many_arguments)]
    fn api_call(
        &mut self,
        caller: Caller,
        conn: u64,
        id: &Value,
        reply: &Replier,
        name: &str,
        args: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Option<String>, Refusal> {
        let needs = registry::scope(name, args).map_err(params)?;
        if needs > caller.scope {
            return Err((codes::SCOPE, registry::needs_words(name, args, needs)));
        }
        let args = &self.api_allows(&caller, name, args, cx)?;
        if registry::own(name) {
            let own = registry::parse_own(name, args).map_err(params)?;
            // Switching, running or replacing a rule that runs a command is typing for the person.
            if let Own::SetAutomation { id, .. }
            | Own::RunAutomation { id }
            | Own::SaveAutomation { id: Some(id), .. } = &own
                && caller.scope < Scope::Full
                && matches!(
                    self.automations.get(*id).map(|a| &a.act),
                    Some(Do::Command { .. })
                )
            {
                return Err((codes::SCOPE, format!("{name} needs a Full token.")));
            }
            // Sharing hands over the connection code, which only its answer carries.
            if let Own::ShareWorkspace {
                workspace,
                name,
                scope,
                expires,
                way,
                address,
            } = own
            {
                if caller.workspaces.is_some() {
                    return Err((
                        codes::SCOPE,
                        "share_workspace needs a Full token for all workspaces.".into(),
                    ));
                }
                let Ok(space) = self.space_of(&workspace, true) else {
                    return Err(
                        match self.space_called(&workspace).and_then(Space::mirror_words) {
                            Some(words) => params(words),
                            None => not_found("workspace", &workspace),
                        },
                    );
                };
                let draft = ShareDraft {
                    workspace: space.clone(),
                    name,
                    scope,
                    expires,
                    way,
                    address,
                };
                let line = match self.share_workspace(draft, cx) {
                    Ok((share, code)) => {
                        let workspace = self
                            .space_named(&space)
                            .map_or(space.clone(), |s| s.name.clone());
                        let mut result = json!({
                            "content": [{ "type": "text", "text": format!("Shared {workspace} as {share}.") }],
                            "target": { "workspace": space, "block": null },
                        });
                        result["code"] = json!(code);
                        rpc::result(id, result)
                    }
                    Err(why) => answer_line(id, &Reply::err(why), None),
                };
                return Ok(Some(line));
            }
            // A repository's operations come one at a time, and answer later.
            if let Own::Git {
                workspace,
                folder,
                op,
            } = own
            {
                return self.api_git(&caller, id, reply, workspace, folder, op, cx);
            }
            // The person's keyboard, not an agent's: no task check, no screen read back.
            if let Own::TerminalInput { block, input } = own {
                let outcome = self.api_input(&caller, &block, input, cx)?;
                return Ok(self.api_answer(outcome, None, conn, id, reply, name, window, cx));
            }
            if matches!(
                own,
                Own::EditNote { .. }
                    | Own::NewNote { .. }
                    | Own::OpenNote { .. }
                    | Own::CloseNote { .. }
                    | Own::DeleteNote { .. }
            ) {
                let (done, target) = self.api_note(own, window, cx)?;
                return Ok(Some(answer_line(id, &done, target.as_ref())));
            }
            let done = self.api_own(&caller, own, window, cx)?;
            return Ok(Some(answer_line(id, &done, None)));
        }
        let parsed = control::parse(name, args).map_err(params)?;
        let actor = Actor::Api(caller);
        let (outcome, target) = self.control_call(&actor, parsed, args, window, cx);
        Ok(self.api_answer(outcome, target, conn, id, reply, name, window, cx))
    }

    /// The reply line of a call's outcome: now, or None while it waits and replies later.
    #[allow(clippy::too_many_arguments)]
    fn api_answer(
        &mut self,
        outcome: Outcome,
        target: Option<Target>,
        conn: u64,
        id: &Value,
        reply: &Replier,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        match outcome {
            Outcome::Now(done) => Some(answer_line(id, &done, target.as_ref())),
            Outcome::Later(job) => {
                let (id, reply) = (id.clone(), reply.clone());
                cx.spawn(async move |_, _| {
                    let done = job.await;
                    // A worktree outside a limited token's folders is a refusal of scope, as any
                    // other folder outside them is, not a tool's failure.
                    let line = if done.is_error && done.text == WORKTREE_OUTSIDE {
                        rpc::error(&id, codes::SCOPE, WORKTREE_OUTSIDE)
                    } else {
                        tools::tool_result(&id, &done)
                    };
                    let _ = reply.send(line);
                })
                .detach();
                None
            }
            Outcome::Wait(what, seconds) => {
                if name == "delegate" {
                    let waiting: Vec<u64> = self.waits.iter().map(|w| w.conn).collect();
                    self.api.delegated.retain(|c| waiting.contains(c));
                    self.api.delegated.push(conn);
                }
                let asked = Asked {
                    conn,
                    id: id.clone(),
                    reply: reply.clone(),
                    from: templum_api::ASKER.to_string(),
                };
                self.wait(asked, *what, seconds, window, cx);
                None
            }
        }
    }

    /// `terminal_input`: `input` typed into the terminal `block` as the person's keyboard does.
    /// Text is one paste, answered now; keys are pressed KEY_GAP apart and the answer comes after
    /// the last, so the client's next input cannot overtake them.
    fn api_input(
        &mut self,
        caller: &Caller,
        block: &str,
        input: Input,
        cx: &mut Context<Self>,
    ) -> Result<Outcome, Refusal> {
        let Some((space, id)) = self.find_block(block) else {
            return Err(not_found("block", block));
        };
        let Some(pane) = self.space_named(&space).and_then(|s| s.terminal(id)) else {
            return Ok(Outcome::Now(Reply::err(format!(
                "{block} isn't a terminal."
            ))));
        };
        if let Err(refused) = pane.read_only(block) {
            return Ok(Outcome::Now(refused));
        }
        if pane.term.exited() {
            return Ok(Outcome::Now(Reply::err(format!(
                "{block}'s shell has ended: nothing typed reaches it."
            ))));
        }
        let worker = pane.term.id().to_string();
        let by = format!("API · {}", caller.name);
        Ok(match input {
            Input::Text(text) => {
                if let Some(pane) = self.pane_mut(&worker) {
                    pane.person_wrote();
                    pane.keys_from = Some((by, Instant::now()));
                    pane.term.paste(&text);
                }
                self.repaint_after_answered(cx);
                cx.notify();
                Outcome::Now(Reply::ok(format!("Pasted into {block}.")))
            }
            Input::Mouse(events) => {
                // Home encodes each in the program's mode now; the ones it didn't ask for are
                // dropped, and the answer counts those taken.
                let mut taken = 0;
                if let Some(pane) = self.pane_mut(&worker) {
                    taken = events.iter().filter(|e| pane.term.mouse(e)).count();
                    if taken > 0 {
                        pane.scrolled.set(true);
                        pane.keys_from = Some((by, Instant::now()));
                    }
                }
                self.repaint_after_answered(cx);
                cx.notify();
                Outcome::Now(Reply::ok(match taken {
                    1 => format!("Sent 1 mouse event to {block}."),
                    n => format!("Sent {n} mouse events to {block}."),
                }))
            }
            Input::Keys(keys) => {
                let said = match keys.len() {
                    1 => format!("Pressed 1 key in {block}."),
                    n => format!("Pressed {n} keys in {block}."),
                };
                let pressed = self.press_each(worker, by, keys, true, cx);
                Outcome::Later(cx.spawn(async move |_, _| {
                    if pressed.await {
                        Reply::ok(said)
                    } else {
                        Reply::err("Its block was closed before every key was pressed.")
                    }
                }))
            }
        })
    }

    /// The client of a call left: what it waited for is dropped as an agent's closed connection
    /// drops it, and a task it handed over and waited for is no longer handed to it.
    fn api_closed(&mut self, conn: u64, cx: &mut Context<Self>) {
        let handed: Vec<String> = if self.api.delegated.contains(&conn) {
            self.waits
                .iter()
                .filter(|w| w.conn == conn)
                .filter_map(|w| match &w.what {
                    Waiting::Turn { of, .. } => Some(of.clone()),
                    _ => None,
                })
                .collect()
        } else {
            Vec::new()
        };
        self.api.delegated.retain(|c| *c != conn);
        self.closed(conn, cx);
        for terminal in handed {
            if let Some(pane) = self.pane_mut(&terminal)
                && pane
                    .handed
                    .as_ref()
                    .is_some_and(|h| h.by == templum_api::ASKER)
            {
                pane.handed = None;
            }
        }
    }

    /// Whether `caller` may name everything `args` names: for a token limited to some workspaces,
    /// every block, workspace, board, card, request and automation must be one it sees, and is
    /// otherwise answered as one that doesn't exist (docs/specs/api.md, Workspaces a token may see).
    /// A workspace or a board is resolved here as the tool resolves it (an id first, then a name in
    /// either case) and the arguments come back with the id, so what is checked is what is touched.
    fn api_allows(
        &self,
        caller: &Caller,
        name: &str,
        args: &Value,
        cx: &gpui_kit::App,
    ) -> Result<Value, Refusal> {
        if caller.workspaces.is_none() {
            return Ok(args.clone());
        }
        if name == "create_workspace" {
            return Err((
                codes::SCOPE,
                "A token limited to some workspaces can't make a workspace.".into(),
            ));
        }
        if name == "overview" {
            return Err((
                codes::SCOPE,
                "overview lists every workspace: a token limited to some workspaces reads GET \
                 /v1/state."
                    .into(),
            ));
        }
        // Without a board these would pick one, or list them all.
        if (name == "create_card" || name == "list_cards")
            && args["board"].is_null()
            && (name == "create_card" || args["card"].is_null())
        {
            return Err(params(format!(
                "A token limited to some workspaces names the board: {name} needs \"board\"."
            )));
        }
        let seen = self.api_scope(cx).visible(caller);
        let mut args = args.clone();
        let text = |value: &Value| match value {
            Value::String(s) => Some(s.clone()),
            Value::Number(n) => Some(n.to_string()),
            _ => None,
        };
        let block = |uid: &str| seen.blocks.iter().any(|b| b.id == uid);
        for key in registry::TARGETS {
            let values: Vec<String> = match &args[key] {
                Value::Array(items) => items.iter().filter_map(text).collect(),
                other => text(other).into_iter().collect(),
            };
            for value in values {
                let found = match key {
                    "block" | "a" | "b" | "near" | "connect" => {
                        block(&value).then_some(()).ok_or("block")
                    }
                    // Who a card is for: anyone, an agent, or a block of its own.
                    "for" => (value == "anyone"
                        || block(&value)
                        || self.agents.iter().any(|a| a.id == value))
                    .then_some(())
                    .ok_or("block"),
                    "workspace" => match self.space_of(&value, true) {
                        Ok(id) if seen.workspaces.iter().any(|w| w.id == id) => {
                            args[key] = json!(id);
                            Ok(())
                        }
                        _ => Err("workspace"),
                    },
                    "board" => match self.board_named(&value) {
                        Ok(id) if seen.boards.iter().any(|b| b.id == id) => {
                            args[key] = json!(id.to_string());
                            Ok(())
                        }
                        _ => Err("board"),
                    },
                    "card" => seen
                        .cards
                        .iter()
                        .any(|c| c.id.to_string() == value)
                        .then_some(())
                        .ok_or("card"),
                    "request" => seen
                        .requests
                        .iter()
                        .any(|r| r.id == value)
                        .then_some(())
                        .ok_or("request"),
                    "automation" => seen
                        .automations
                        .iter()
                        .any(|a| a.id.to_string() == value)
                        .then_some(())
                        .ok_or("automation"),
                    "schedule" => {
                        let id = format!("s{}", value.trim().trim_start_matches('s'));
                        if !seen.schedules.iter().any(|s| s.id == id) {
                            return Err(not_found("schedule", &id));
                        }
                        Ok(())
                    }
                    _ => Ok(()),
                };
                if let Err(word) = found {
                    return Err(not_found(word, &value));
                }
            }
        }
        // A note is the token's when a block it sees shows it, or it was last open in a workspace
        // it sees: never home's other notes (they are app-wide).
        if matches!(name, "open_note" | "delete_note")
            && let Some(note) = registry::whole(&args["note"])
            && !self.note_seen(&seen, note)
        {
            return Err(not_found("note", &note.to_string()));
        }
        // What it opens starts inside the folders of its workspaces, symlinks and `..` followed.
        if matches!(name, "add_block" | "open_agent" | "git") {
            let kind = args["kind"].as_str().unwrap_or_default();
            let opens =
                matches!(name, "open_agent" | "git") || matches!(kind, "terminal" | "editor");
            let target = args["workspace"].as_str().unwrap_or_default().to_string();
            let target = target.as_str();
            // What is checked is what is used: the real path replaces the one named.
            // A worktree is made in the repository's main tree, which may lie outside its folders:
            // `in_worktree` asks git where it is, and refuses (`main_tree_inside`).
            match args["folder"].as_str() {
                Some(folder) => {
                    let real = self.confined(&seen, target, folder)?;
                    args["folder"] = json!(real.to_string_lossy());
                }
                None if opens && matches!(name, "add_block" | "git") => {
                    self.confined_default(target)?
                }
                None => {}
            }
        }
        if name == "save_automation" {
            // The rule it replaces, the workspace it is for and the blocks it types into are the
            // token's to see, or it would reach what the token can't.
            if let Some(id) = args["id"].as_u64()
                && !seen.automations.iter().any(|a| a.id == id)
            {
                return Err(not_found("automation", &id.to_string()));
            }
            let rule = args["automation"].clone();
            let workspace = match rule["workspace"].as_str() {
                Some(workspace) if seen.workspaces.iter().any(|w| w.id == workspace) => workspace,
                Some(workspace) => return Err(not_found("workspace", workspace)),
                None => {
                    return Err((
                        codes::SCOPE,
                        "A token limited to some workspaces saves automations for them: name a \
                         workspace."
                            .into(),
                    ));
                }
            };
            for uid in [
                rule["do"]["prompt"]["block"].as_str(),
                rule["do"]["command"]["terminal"].as_str(),
            ]
            .into_iter()
            .flatten()
            {
                if !block(uid) {
                    return Err(not_found("block", uid));
                }
            }
            // Where it opens things is checked now, written down as the real path, and checked again
            // each time it runs (`automation_target`).
            for key in ["agent", "command"] {
                let act = &rule["do"][key];
                let opens = key == "agent" || act["terminal"].is_null();
                if act.is_null() {
                    continue;
                }
                match act["folder"]
                    .as_str()
                    .map(str::trim)
                    .filter(|f| !f.is_empty())
                {
                    Some(folder) => {
                        let real = self.confined(&seen, workspace, folder)?;
                        args["automation"]["do"][key]["folder"] = json!(real.to_string_lossy());
                    }
                    None if opens => self.confined_default(workspace)?,
                    None => {}
                }
            }
        }
        Ok(args)
    }

    /// Whether note `id` is one `seen` reaches: shown by a block `seen` has, or open nowhere and
    /// last open in a workspace `seen` has.
    fn note_seen(&self, seen: &templum_api::State, id: u64) -> bool {
        let sees = |workspace: &str| seen.workspaces.iter().any(|w| w.id == workspace);
        match self.note_open_at(id) {
            Some((space, block)) => {
                sees(&space.id)
                    && space
                        .uids
                        .get(&block)
                        .is_some_and(|uid| seen.blocks.iter().any(|b| &b.id == uid))
            }
            None => self
                .notes
                .get(id)
                .and_then(|n| n.closed_in.as_deref())
                .is_some_and(sees),
        }
    }

    /// The real folders of the workspaces `seen` has.
    pub(super) fn confined_roots(&self, seen: &templum_api::State) -> Vec<std::path::PathBuf> {
        seen.workspaces
            .iter()
            .filter_map(|w| dunce::canonicalize(self.space_named(&w.id)?.folder.as_ref()?).ok())
            .collect()
    }

    /// `folder` (as a tool takes it: `~`, or relative to the workspace `target`'s folder) lies inside
    /// the folder of a workspace the token sees. It must exist: it is resolved to its real path,
    /// which is what comes back.
    fn confined(
        &self,
        seen: &templum_api::State,
        target: &str,
        folder: &str,
    ) -> Result<std::path::PathBuf, Refusal> {
        let outside = || {
            (
                codes::SCOPE,
                format!(
                    "A token limited to some workspaces opens things inside their folders: \
                     {folder} is not in one."
                ),
            )
        };
        let mut path = crate::launcher::expand(folder);
        if path.is_relative() {
            let home = self.space_named(target).and_then(|s| s.folder.clone());
            path = home.ok_or_else(outside)?.join(path);
        }
        let roots = self.confined_roots(seen);
        // Before the filesystem is asked about it: a share of another computer is asked for the
        // person's credentials.
        if refuses_share(
            prefix_of(&path),
            &roots.iter().map(|r| prefix_of(r)).collect::<Vec<_>>(),
        ) {
            return Err(outside());
        }
        let real = dunce::canonicalize(&path).map_err(|_| outside())?;
        let inside = roots.iter().any(|root| real.starts_with(root));
        inside.then_some(real).ok_or_else(outside)
    }

    /// With no folder named, a terminal or an editor starts in the workspace's: it needs one.
    fn confined_default(&self, target: &str) -> Result<(), Refusal> {
        match self.space_named(target) {
            Some(space) if space.folder.is_none() => Err((
                codes::SCOPE,
                "A token limited to some workspaces opens things inside their folders: this \
                 workspace has none."
                    .into(),
            )),
            _ => Ok(()),
        }
    }

    /// The person edited or deleted the automation `id`: it is theirs now, no longer held to the
    /// folders a limited token saved it for, and no longer the token's to answer for.
    pub(super) fn api_unconfine(&mut self, id: u64) {
        if self
            .api
            .store
            .as_mut()
            .is_some_and(|s| s.confined.remove(&id).is_some() | s.saved_by.remove(&id).is_some())
        {
            self.save_api();
        }
    }

    /// Why the automation `a`, saved by a limited token, can't open where it would: that place
    /// is not inside the folders it was saved for any more (a link was swapped, a folder moved).
    pub(super) fn api_folder_problem(
        &self,
        a: &Automation,
        place: &std::path::Path,
    ) -> Option<String> {
        let roots = self.api.store.as_ref()?.confined.get(&a.id)?;
        let inside = dunce::canonicalize(place)
            .ok()
            .is_some_and(|real| roots.iter().any(|root| real.starts_with(root)));
        (!inside).then(|| {
            format!(
                "{} is outside the workspaces it was saved for",
                place.display()
            )
        })
    }

    /// Whether the automation `a` was saved through the API by a token that is revoked or expired
    /// now: it must not run before the next sweep turns it off.
    pub(super) fn api_token_is_gone(&self, a: &Automation) -> bool {
        let now = now_secs();
        self.api.store.as_ref().is_some_and(|store| {
            store
                .saved_by
                .get(&a.id)
                .is_some_and(|token| !store.tokens.iter().any(|t| t.id == *token && t.live(now)))
        })
    }

    /// The API's own tools.
    fn api_own(
        &mut self,
        caller: &Caller,
        own: Own,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Reply, Refusal> {
        let known = |ws: &Self, id: u64| {
            ws.automations
                .get(id)
                .map(|_| ())
                .ok_or_else(|| not_found("automation", &id.to_string()))
        };
        Ok(match own {
            Own::AnswerRequest {
                request,
                answer,
                note,
            } => {
                return self.answer_request(caller, &request, answer, note, window, cx);
            }
            Own::SaveAutomation { id, automation } => {
                if let Some(id) = id {
                    known(self, id)?;
                }
                let mut value = automation;
                value["id"] = json!(id.unwrap_or(super::automations::NEW));
                let mut rule: Automation = serde_json::from_value(value)
                    .map_err(|e| params(format!("The automation isn't valid: {e}.")))?;
                // What the editor doesn't set is the app's: a run count, a start, a last run.
                (rule.runs, rule.since, rule.last, rule.opened) = (0, 0, None, None);
                let saved = self.save_rule(rule.id, rule, cx).map_err(params)?;
                // A limited token's rule is held to its folders each time it runs; one saved by a
                // token that isn't limited is the person's own.
                let roots = caller.workspaces.is_some().then(|| {
                    let seen = self.api_scope(cx).visible(caller);
                    self.confined_roots(&seen)
                });
                if let Some(store) = self.api.store.as_mut() {
                    match roots {
                        Some(roots) => store.confined.insert(saved, roots),
                        None => store.confined.remove(&saved),
                    };
                    // Whoever saved it answers for it: it stops with the token.
                    store.saved_by.insert(saved, caller.token);
                }
                self.save_api();
                Reply::ok(format!("Saved automation {saved}."))
            }
            Own::SetAutomation { id, on } => {
                known(self, id)?;
                self.set_automation_on(id, on, cx);
                Reply::ok(format!(
                    "Turned automation {id} {}.",
                    if on { "on" } else { "off" }
                ))
            }
            Own::RunAutomation { id } => {
                known(self, id)?;
                self.run_automation_now(id, window, cx);
                match self.automations.get(id).and_then(|a| a.last.clone()) {
                    Some(last) if last.went != crate::automations::Went::Ran => {
                        Reply::err(format!("Automation {id} didn't run: {}", last.about))
                    }
                    _ => Reply::ok(format!("Ran automation {id}.")),
                }
            }
            Own::DeleteAutomation { id } => {
                known(self, id)?;
                self.delete_automation(id, cx);
                Reply::ok(format!("Deleted automation {id}."))
            }
            Own::CancelSchedule { schedule } => {
                let n = schedule.trim().trim_start_matches('s');
                match n.parse::<u64>() {
                    Ok(id) if self.schedules.iter().any(|s| s.id == id) => {
                        self.cancel_schedule_now(id, cx);
                        Reply::ok(format!("Cancelled s{id}."))
                    }
                    _ => return Err(not_found("schedule", &format!("s{n}"))),
                }
            }
            // Typed on the person's keyboard's path (`api_input`), which `api_call` takes first.
            Own::TerminalInput { block, .. } => Reply::err(format!(
                "terminal_input to {block} must go through api_input."
            )),
            // Answered later by `api_git`, which `api_call` takes first.
            Own::Git { .. } => Reply::err("git must go through api_git."),
            // Answered with its target by `api_call`, which takes them first (`api_note`).
            Own::EditNote { .. }
            | Own::NewNote { .. }
            | Own::OpenNote { .. }
            | Own::CloseNote { .. }
            | Own::DeleteNote { .. } => Reply::err("The note tools must go through api_note."),
            Own::RecentFolders { workspace } => {
                // A mirror's folders are home's: it has none here.
                let space = match self.space_of(&workspace, true) {
                    Ok(space) => space,
                    Err(refused) => {
                        return match self.space_called(&workspace).and_then(Space::mirror_words) {
                            Some(_) => Ok(refused),
                            None => Err(not_found("workspace", &workspace)),
                        };
                    }
                };
                let recent = (self.space_named(&space)).map_or_else(Vec::new, |s| s.recent.clone());
                let roots = caller.workspaces.is_some().then(|| {
                    let seen = self.api_scope(cx).visible(caller);
                    self.confined_roots(&seen)
                });
                Reply::ok(recent_lines(&recent, roots.as_deref()))
            }
            // Answered with its code by `api_call`, which takes it first.
            Own::ShareWorkspace { workspace, .. } => Reply::err(format!(
                "share_workspace of {workspace} must go through api_call."
            )),
            Own::RevokeShare { share } => {
                let own = format!("s{}", caller.token);
                let shares = || self.api.store.iter().flat_map(|s| s.tokens.iter());
                let share = match share {
                    None => shares()
                        .find(|t| t.id == caller.token && t.share.is_some())
                        .and_then(|t| t.share_id())
                        .ok_or_else(|| {
                            params("revoke_share needs \"share\": this token isn't a share.")
                        })?,
                    Some(share) => share,
                };
                // Any share may revoke itself; revoking another takes a Full token for everything.
                if share != own && (caller.scope < Scope::Full || caller.workspaces.is_some()) {
                    return Err((
                        codes::SCOPE,
                        "revoke_share needs a Full token for all workspaces.".into(),
                    ));
                }
                let id = shares()
                    .find(|t| t.share_id().as_deref() == Some(share.as_str()))
                    .map(|t| t.id)
                    .ok_or_else(|| not_found("share", &share))?;
                self.revoke_token(id, cx);
                Reply::ok(format!("Revoked {share}."))
            }
        })
    }

    /// The shares of the workspace `space`, oldest first: the Edit… dialog's rows.
    pub(crate) fn share_rows(&self, space: &str) -> Vec<templum_api::model::ShareView> {
        let tokens = self.api.store.iter().flat_map(|s| s.tokens.iter());
        let ours: Vec<String> = tokens
            .filter(|t| t.shares(space))
            .filter_map(|t| t.share_id())
            .collect();
        let mut rows: Vec<_> = self
            .share_views()
            .into_iter()
            .filter(|share| ours.contains(&share.id))
            .collect();
        rows.sort_by_key(|share| (share.created, share.id[1..].parse::<u64>().unwrap_or(0)));
        rows
    }

    /// The names and addresses of the shares of the workspace `space` that are mirrored now: an
    /// event stream of their token is open. Asked of the server, no state built, for the places
    /// that draw every frame (the sidebar's tiles).
    pub(crate) fn mirrored_now(&self, space: &str) -> Vec<(String, String)> {
        let (Some(store), Some(server)) = (self.api.store.as_ref(), self.api.server.as_ref())
        else {
            return Vec::new();
        };
        let open = server.mirrored();
        store
            .tokens
            .iter()
            .filter(|t| t.shares(space))
            .filter_map(|t| {
                let (_, addr) = open.iter().find(|(id, _)| *id == t.id)?;
                Some((t.name.clone(), addr.clone()))
            })
            .collect()
    }

    /// The address of the event stream of the share `share` (`s<id>`), when it is mirrored now.
    pub(crate) fn mirrored_from(&self, share: &str) -> Option<String> {
        let server = self.api.server.as_ref()?;
        server
            .mirrored()
            .into_iter()
            .find(|(id, _)| format!("s{id}") == share)
            .map(|(_, addr)| addr)
    }

    /// The Share… dialog's one button: the API on, and for a direct share on every network with
    /// Templum's certificate.
    pub(crate) fn fit_api(&mut self, way: Way, cx: &mut Context<Self>) {
        self.api_edit(
            |settings| {
                settings.enabled = true;
                if way == Way::Pinned {
                    settings.listen = store::Listen::All;
                    settings.tls = Tls::Own;
                }
            },
            cx,
        );
    }

    /// Shares a workspace with another Templum: a token limited to it, kept with the share's
    /// details, and its connection code, which is made from the token's text here and kept
    /// nowhere. Ok: the share's id and its code. Err: the sentence that says what to fix.
    pub(crate) fn share_workspace(
        &mut self,
        draft: ShareDraft,
        cx: &mut Context<Self>,
    ) -> Result<(String, String), String> {
        use templum_api::code::{self, Key};
        let ShareDraft {
            workspace,
            name,
            scope,
            expires,
            way,
            address,
        } = draft;
        let space = self.space_of(&workspace, true).map_err(|reply| {
            match self.space_called(&workspace).and_then(Space::mirror_words) {
                Some(_) => reply.text,
                None => registry::no("workspace", &workspace),
            }
        })?;
        let (Some(store), Some(data)) = (self.api.store.as_ref(), self.data_dir.clone()) else {
            return Err("There is no data folder.".into());
        };
        if let Some(problem) = store::share_problem(&store.settings, way, &address) {
            return Err(problem.into());
        }
        let (url, pin) = match way {
            Way::Public => (address.trim().trim_end_matches('/').to_string(), None),
            Way::Pinned => {
                let host = address.trim();
                if host.is_empty() || host.contains("://") {
                    return Err("Give Directly's address as host:port.".into());
                }
                let cert = templum_api::tls::own(&data)?;
                (
                    format!("https://{host}"),
                    Some(templum_api::tls::pin(&cert.chain[0])),
                )
            }
        };
        // A code the other Templum could not read is not made: tried before the token is.
        code::decode(&code::encode(
            &url,
            &Key::new("tpl_".into()),
            pin.as_deref(),
        ))
        // Whatever is wrong is this address's: the mirror's sentences are for a code pasted there.
        .map_err(|_| "Templum can't make a code for this address.".to_string())?;
        let store = self.api.store.as_mut().ok_or("There is no data folder.")?;
        let share = Share {
            workspace: space,
            way,
            address: address.trim().to_string(),
        };
        let (token, text) = store.mint_share(share, &name, scope, expires, now_secs())?;
        let code = code::encode(&url, &Key::new(text), pin.as_deref());
        self.tokens_changed(cx);
        Ok((format!("s{token}"), code))
    }

    /// `answer_request`: a request answered as the inbox's buttons answer it, if `caller` sees it
    /// and its scope reaches it.
    fn answer_request(
        &mut self,
        caller: &Caller,
        request: &str,
        answer: Picked,
        note: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Reply, Refusal> {
        let view = self
            .api_state(cx)
            .visible(caller)
            .requests
            .into_iter()
            .find(|r| r.id == request)
            .ok_or_else(|| not_found("request", request))?;
        if view.scope > caller.scope {
            return Err((
                codes::SCOPE,
                format!(
                    "answer_request needs a {} token for this request.",
                    view.scope.word()
                ),
            ));
        }
        let number: u64 = view.id[1..]
            .parse()
            .map_err(|_| not_found("request", request))?;
        match (view.kind.as_str(), answer) {
            ("approval", Picked::Allow(allow)) => {
                let answer = if allow {
                    Approve::Once
                } else {
                    Approve::Deny(note.filter(|n| !n.trim().is_empty()))
                };
                self.answer_approval(number, answer, window, cx);
                // An Allow after what it is about changed is asked again, not given.
                if allow && self.templum_agent.approvals.iter().any(|a| a.id == number) {
                    return Ok(Reply::err(
                        "What it is about changed since it was shown: it is asked again. Read it, then answer.",
                    ));
                }
            }
            ("approval", _) => return Err(params("An approval takes allow.")),
            ("question" | "plan", Picked::Picked(answers)) => {
                let wait = self.waits.iter().find(|w| w.key == number);
                let Some(Waiting::Question { ask, .. }) = wait.map(|w| &w.what) else {
                    return Err(not_found("request", request));
                };
                ask.checked(&answers).map_err(params)?;
                self.answer_question(
                    number,
                    answers,
                    note.as_deref().unwrap_or_default(),
                    window,
                    cx,
                );
            }
            ("question" | "plan", Picked::Dismiss) => {
                self.dismiss_now(number, window, cx);
            }
            ("question" | "plan", Picked::Allow(_)) => {
                return Err(params("A question takes answers or dismiss."));
            }
            (_, Picked::Allow(yes)) => self.answer_allow(number, yes, cx),
            (_, _) => return Err(params("This request takes allow.")),
        }
        Ok(Reply::ok("Answered."))
    }

    /// Tells the server of something that isn't state: a task the API handed ended, or its agent
    /// waits for its person.
    pub(super) fn api_notice(&mut self, notice: Notice) {
        #[cfg(test)]
        self.api.notices.push(notice.clone());
        if let Some(server) = &self.api.server {
            server.notice(notice);
        }
    }

    /// A notice for the API's asker, with the block and workspace of the `terminal` it is about
    /// (a task's worker, or the terminal a watch is on).
    pub(super) fn api_notice_about(&mut self, text: String, terminal: Option<&str>) {
        let place = terminal
            .and_then(|terminal| self.caller(terminal))
            .map(|(space, id)| {
                let uid = self
                    .space_named(&space)
                    .and_then(|s| s.uids.get(&id).cloned());
                (uid, space)
            });
        let (block, workspace) = match place {
            Some((uid, space)) => (uid, Some(space)),
            None => (None, None),
        };
        self.api_notice(Notice {
            text,
            block,
            workspace,
            automation: None,
        });
    }

    // The store, the server and the tick.

    /// Reads the API's file at start; on, it listens. A file that can't be read is set aside, the
    /// API stays off, and a notification says so.
    pub(super) fn load_api(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = self.data_dir.clone() else {
            return;
        };
        match store::load(&data) {
            Ok(mut store) => {
                // Unreadable automations aren't gone, only not loaded: their entries stay.
                let stale = !self.automations_frozen
                    && store.forget_automations(|id| self.automations.get(id).is_some());
                self.api.store = Some(store);
                if stale {
                    self.save_api();
                }
            }
            Err(why) => {
                eprintln!("templum: {why}");
                show_notification(window, cx, Notification::error(why));
                self.api.store = Some(Store::default());
            }
        }
        self.start_api();
    }

    /// Starts the server if the settings say it is on, replacing one that runs. The reason it
    /// can't is kept for Settings and the title bar.
    pub(crate) fn start_api(&mut self) {
        // The old server's uses, before it is dropped with them.
        self.drain_api_uses();
        self.api.clients = 0;
        self.api.server = None;
        self.api.error = None;
        let (Some(store), Some(data), Some(bus)) = (
            self.api.store.as_ref(),
            self.data_dir.as_deref(),
            self.api.bus.clone(),
        ) else {
            return;
        };
        if !store.settings.enabled {
            return;
        }
        match templum_api::Options::from(&store.settings, data)
            .and_then(|options| templum_api::start(options, store.tokens.clone(), bus))
        {
            Ok(server) => self.api.server = Some(server),
            Err(why) => self.api.error = Some(why),
        }
        self.api.fingerprint = match store.settings.tls {
            Tls::Own => templum_api::tls::own(data)
                .ok()
                .map(|c| templum_api::tls::fingerprint(&c.chain[0])),
            _ => None,
        };
    }

    /// The server speaks TLS (it listens, or would).
    pub(crate) fn api_tls(&self) -> bool {
        self.api
            .store
            .as_ref()
            .is_some_and(|s| s.settings.tls != Tls::Off)
    }

    /// Settings ▸ API ▸ Save certificate…: Templum's certificate as a `.pem` the person picks the
    /// place of, to install on a phone.
    pub(crate) fn save_certificate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(data) = self.data_dir.clone() else {
            return;
        };
        let folder = std::env::home_dir().unwrap_or_else(std::env::temp_dir);
        let picked = cx.prompt_for_new_path(&folder, Some("templum.pem"));
        cx.spawn_in(window, async move |ws, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let pem = std::fs::read_to_string(store::folder(&data).join("cert.pem"));
            let wrote = pem.and_then(|pem| std::fs::write(&path, pem));
            ws.update_in(cx, |_, window, cx| {
                if let Err(error) = wrote {
                    let message = format!("Couldn't save the certificate: {error}");
                    show_notification(window, cx, Notification::warning(message));
                }
            })
            .ok();
        })
        .detach();
    }

    /// Writes the API's file. A failure is said and the session goes on.
    fn save_api(&mut self) {
        self.api.dirty = false;
        if let (Some(store), Some(data)) = (&self.api.store, &self.data_dir)
            && let Err(error) = store::save(data, store)
        {
            eprintln!("templum: couldn't save the API's settings: {error}");
        }
    }

    /// Changes the settings: off a loopback address HTTPS is never off, the file is written, and
    /// the server restarts (or stops) to match.
    pub(crate) fn api_edit(&mut self, edit: impl FnOnce(&mut Settings), cx: &mut Context<Self>) {
        let Some(store) = self.api.store.as_mut() else {
            return;
        };
        let before = store.settings.clone();
        edit(&mut store.settings);
        let loopback = match &store.settings.listen {
            store::Listen::Loopback => true,
            store::Listen::All => false,
            store::Listen::Address(a) => a
                .trim()
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback()),
        };
        if !loopback && store.settings.tls == Tls::Off {
            store.settings.tls = Tls::Own;
        }
        // Nothing changed (a field left as it was): nothing to write, nothing to restart.
        if store.settings == before {
            return;
        }
        let only_origins = Settings {
            origins: before.origins.clone(),
            ..store.settings.clone()
        } == before;
        self.save_api();
        // The web pages allowed change on a running server; anything else starts it again.
        match (&self.api.server, only_origins, self.api.store.as_ref()) {
            (Some(server), true, Some(store)) => server.set_origins(store.settings.origins.clone()),
            _ => self.start_api(),
        }
        cx.notify();
    }

    /// Makes a token: its text is kept in `Api::shown` until `token_seen`, the file keeps its
    /// hash, and the server knows it.
    pub(crate) fn mint_token(
        &mut self,
        name: &str,
        scope: Scope,
        expires: Option<u64>,
        workspaces: Option<Vec<String>>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let store = self.api.store.as_mut().ok_or("There is no data folder.")?;
        let text = store.mint(name, scope, expires, workspaces, now_secs())?;
        self.api.shown = Some(text);
        self.tokens_changed(cx);
        Ok(())
    }

    /// The dialog that showed a token closed: its text is gone from memory.
    pub(crate) fn token_seen(&mut self) {
        self.api.shown = None;
    }

    pub(crate) fn revoke_token(&mut self, id: u64, cx: &mut Context<Self>) {
        let names = self.token_names();
        if self.api.store.as_mut().is_some_and(|s| s.revoke(id)) {
            self.tokens_changed(cx);
            self.api_cut_rules(&names, cx);
        }
    }

    pub(crate) fn revoke_all_tokens(&mut self, cx: &mut Context<Self>) {
        let names = self.token_names();
        if let Some(store) = self.api.store.as_mut() {
            store.revoke_all();
            self.tokens_changed(cx);
            self.api_cut_rules(&names, cx);
        }
    }

    /// The tokens' names by id, for what is said of a token once it is gone.
    pub(super) fn token_names(&self) -> std::collections::BTreeMap<u64, String> {
        let tokens = self.api.store.iter().flat_map(|s| s.tokens.iter());
        tokens.map(|t| (t.id, t.name.clone())).collect()
    }

    /// The automations saved through the API by a token that is revoked or expired are turned off,
    /// and the person is told: a leaked token's rule must not outlive it. `names` is the tokens'
    /// names before the change (`token_names`).
    pub(super) fn api_cut_rules(
        &mut self,
        names: &std::collections::BTreeMap<u64, String>,
        cx: &mut Context<Self>,
    ) {
        let now = now_secs();
        let Some(store) = self.api.store.as_mut() else {
            return;
        };
        let gone: Vec<(u64, u64)> = store
            .saved_by
            .iter()
            .filter(|(_, token)| !store.tokens.iter().any(|t| t.id == **token && t.live(now)))
            .map(|(rule, token)| (*rule, *token))
            .collect();
        if gone.is_empty() {
            return;
        }
        for (rule, _) in &gone {
            store.saved_by.remove(rule);
        }
        self.save_api();
        for (id, token) in gone {
            let Some(rule) = self.automations.get(id) else {
                continue;
            };
            let on = rule.on;
            let rule = match rule.name.as_str() {
                "" => format!("Automation {id}"),
                name => format!("Automation {name}"),
            };
            let token = names.get(&token).cloned().unwrap_or_default();
            // One the token had turned off itself is told of too, and stays off.
            let said = if on {
                self.set_automation_on(id, false, cx);
                "was turned off"
            } else {
                "stays off"
            };
            self.api_say(format!("{rule}, saved by token '{token}', {said}."), cx);
        }
    }

    /// Tells the person, in a notification, once the window is free to take it.
    fn api_say(&mut self, text: String, cx: &mut Context<Self>) {
        #[cfg(test)]
        self.api.said.push(text.clone());
        let window = self.window_handle;
        cx.spawn(async move |_, cx| {
            cx.update_window(window, |_, window, cx| {
                show_notification(window, cx, Notification::warning(text));
            })
            .ok();
        })
        .detach();
    }

    /// A workspace was deleted: its id goes from every token's list, or a new workspace that takes
    /// the id would be the token's.
    pub(super) fn api_forget_workspace(&mut self, id: &str, cx: &mut Context<Self>) {
        let mut changed = false;
        for token in self.api.store.iter_mut().flat_map(|s| s.tokens.iter_mut()) {
            if let Some(list) = token.workspaces.as_mut()
                && list.iter().any(|w| w == id)
            {
                list.retain(|w| w != id);
                changed = true;
            }
        }
        if changed {
            self.tokens_changed(cx);
        }
    }

    fn tokens_changed(&mut self, cx: &mut Context<Self>) {
        self.save_api();
        if let (Some(server), Some(store)) = (&self.api.server, &self.api.store) {
            server.set_tokens(store.tokens.clone());
        }
        cx.notify();
    }

    pub(crate) fn disconnect_client(&mut self, id: u64) {
        if let Some(server) = &self.api.server {
            server.disconnect(id);
        }
    }

    /// New files for Templum's certificate: clients that pinned the old one stop connecting.
    pub(crate) fn renew_certificate(&mut self, cx: &mut Context<Self>) {
        if let Some(data) = self.data_dir.as_deref()
            && let Err(why) = templum_api::tls::renew(data)
        {
            self.api.error = Some(why);
        } else {
            self.start_api();
        }
        cx.notify();
    }

    /// The tokens' newest uses the server holds go into the store (and mark it for writing).
    fn drain_api_uses(&mut self) {
        let (Some(server), Some(store)) = (&self.api.server, self.api.store.as_mut()) else {
            return;
        };
        for (id, used) in server.uses() {
            if let Some(token) = store.tokens.iter_mut().find(|t| t.id == id) {
                token.last = Some(used);
                self.api.dirty = true;
            }
        }
    }

    /// Each token's newest use since the last snapshot is kept; the file takes it at most once a
    /// minute (it is synced to disk), and at quit (`flush_api`).
    pub(super) fn keep_api_uses(&mut self, now: std::time::Instant) {
        self.drain_api_uses();
        if self.api.dirty
            && self
                .api
                .saved
                .is_none_or(|at| now.saturating_duration_since(at) >= USES_EVERY)
        {
            self.save_api();
            self.api.saved = Some(now);
        }
    }

    /// At quit: whatever the file hasn't taken yet, including the uses since the last snapshot.
    pub(super) fn flush_api(&mut self) {
        self.drain_api_uses();
        if self.api.dirty {
            self.save_api();
        }
    }

    /// A client connected or left: the pill counts them, and Settings lists them, at the next draw.
    fn api_clients(&mut self, cx: &mut Context<Self>) {
        let count = self.api.server.as_ref().map_or(0, |s| s.client_count());
        self.api.clients = count;
        // Also when only a stream opened or ended: who mirrors a workspace is drawn from the server.
        cx.notify();
    }

    /// One round of the tick: hands the event streams the state, and the terminal streams the
    /// terminals as they are now. False (and the tick over) when no stream of either kind is open.
    pub(crate) fn api_tick_once(&mut self, cx: &mut Context<Self>) -> bool {
        let (events, terminals) = self.api.server.as_ref().map_or((false, false), |s| {
            (s.streams() > 0, s.terminal_streams() > 0)
        });
        if events {
            // A share's last use is in its state.
            self.drain_api_uses();
            let state = self.api_state(cx);
            if let Some(server) = &self.api.server {
                server.publish(state);
            }
        }
        if terminals && let Some(server) = &self.api.server {
            server.set_terminals(self.api_terminals());
        }
        if !events && !terminals {
            self.api.ticking = false;
        }
        events || terminals
    }

    /// How long the tick waits: FAST_TICK while a block is dragged or resized, the canvas panned
    /// by the pointer, or the camera moved in the last `MOVED_FOR`, else TICK.
    pub(super) fn api_tick(&self, now: Instant) -> Duration {
        use templum_canvas::Gesture;
        let moving = match &self.drag {
            Some(Drag::Canvas(gesture)) | Some(Drag::Held { gesture, .. }) => matches!(
                gesture,
                Gesture::Move { .. }
                    | Gesture::MoveGroup { .. }
                    | Gesture::Resize { .. }
                    | Gesture::Pan { .. }
            ),
            _ => false,
        };
        let moved = self
            .api
            .moved_at
            .is_some_and(|at| now.saturating_duration_since(at) < MOVED_FOR);
        if moving || moved { FAST_TICK } else { TICK }
    }

    /// While at least one stream is open, event or terminal, runs `api_tick_once` every
    /// `api_tick()`; ends at the first look with none (the server asks again when one opens).
    fn start_api_tick(&mut self, cx: &mut Context<Self>) {
        if self.api.ticking || self.api.server.is_none() {
            return;
        }
        self.api.ticking = true;
        cx.spawn(async move |ws, cx| {
            loop {
                let wait = ws
                    .update(cx, |ws, _| ws.api_tick(Instant::now()))
                    .unwrap_or(TICK);
                cx.background_executor().timer(wait).await;
                let more = ws
                    .update(cx, |ws, cx| ws.api_tick_once(cx))
                    .unwrap_or(false);
                if !more {
                    break;
                }
            }
        })
        .detach();
    }
}

/// A path's Windows prefix (`C:`, `\\server\share`, `\\?\UNC\…`); None on any other system.
pub(super) fn prefix_of(path: &std::path::Path) -> Option<std::path::Prefix<'_>> {
    match path.components().next() {
        Some(std::path::Component::Prefix(prefix)) => Some(prefix.kind()),
        _ => None,
    }
}

/// The share of another computer (or the device namespace) a prefix reaches, lowercased: what
/// Windows would open a session to, offering the person's credentials. Fails closed: only a drive
/// is the computer's own. std matches `\\?\UNC\` case-sensitively, so `\\?\unc\x\y` and
/// `\\?\GLOBALROOT\Device\Mup\…` come as a plain `Verbatim`, and reach a share all the same.
fn share_of(prefix: std::path::Prefix<'_>) -> Option<String> {
    use std::path::Prefix::{DeviceNS, Disk, UNC, Verbatim, VerbatimDisk, VerbatimUNC};
    match prefix {
        Disk(_) | VerbatimDisk(_) => None,
        Verbatim(name) => Some(name.to_string_lossy().to_lowercase()),
        UNC(server, share) | VerbatimUNC(server, share) => Some(format!(
            "{}\\{}",
            server.to_string_lossy().to_lowercase(),
            share.to_string_lossy().to_lowercase()
        )),
        DeviceNS(name) => Some(name.to_string_lossy().to_lowercase()),
    }
}

/// Whether a path with prefix `path` mustn't be touched: it is on a share (or a device namespace),
/// and none of the token's folders (`roots`' prefixes) is on that share.
pub(super) fn refuses_share(
    path: Option<std::path::Prefix<'_>>,
    roots: &[Option<std::path::Prefix<'_>>],
) -> bool {
    let Some(share) = path.and_then(share_of) else {
        return false;
    };
    !roots
        .iter()
        .any(|root| root.and_then(share_of).as_ref() == Some(&share))
}

/// Why a limited token's worktree is refused (`main_tree_inside`).
pub(super) const WORKTREE_OUTSIDE: &str = "A token limited to some workspaces makes worktrees \
     inside their folders: this repository's main tree is not in one.";

/// Whether the main working tree of `repo`, as git reports it (it obeys `commondir`,
/// `core.worktree` and the like, which a `.git` file planted in a folder can use), is inside one
/// of `roots`. Err: what to say.
pub(super) fn main_tree_inside(
    repo: &templum_git::Repo,
    roots: &[std::path::PathBuf],
) -> Result<(), String> {
    let inside = repo
        .main_tree()
        .ok()
        .flatten()
        .and_then(|main| dunce::canonicalize(main).ok())
        .is_some_and(|main| roots.iter().any(|root| main.starts_with(root)));
    if inside {
        Ok(())
    } else {
        Err(WORKTREE_OUTSIDE.into())
    }
}
