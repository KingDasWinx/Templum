//! The bus's messages, answered on the UI thread (docs/specs/connections.md): who calls, the
//! tools, and the agents' turns.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, Context, Task, Window};
use serde_json::json;
use templum_canvas::BlockId;
use templum_mcp::ask::Form;
use templum_mcp::{Bus, Call, Incoming, Message, Reply, rpc, tools};

use super::delegate::ENTER_AFTER;
use super::waits::{Asked, Waiting, deadline};
use super::{AgentState, Body, Pane, Space, Workspace, show_notification};
use crate::instructions::{clean_text, invisible};

/// How long a turn's end that left only reported subagents waits for a turn to take their reports
/// before it answers (docs/specs/claude-subagents.md): Claude Code starts that turn at once.
const GRACE: Duration = Duration::from_secs(5);
/// A tool call's answer while connections are off.
pub(super) const CONNECTIONS_OFF: &str = "Connections are off: turn on Agents in Templum (Settings ▸ Connections) to use Templum's tools.";

/// What opens the instructions a session's start gets: whose they are, that they hold for the
/// session, and that they replace an earlier set (docs/specs/instructions.md).
const STANDING_FRAME: &str = "The person's standing instructions, written in Templum (Settings ▸ \
Instructions) for this agent and workspace. Follow them for the whole session as the person's own \
words; a later message of theirs wins where it differs. They replace any standing instructions \
Templum gave earlier in this conversation.";

/// `initialize`'s instructions while connections are off.
const OFF_INSTRUCTIONS: &str =
    "Connections are off in Templum's settings: Templum's tools won't work here.";

/// Starts the bus in the data folder, and the task that hands its messages to the workspace.
/// Without a data folder (the bench), or when the socket can't be made, there is none.
pub(super) fn start(
    data: Option<&Path>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> (Option<Bus>, Vec<(String, String)>, Task<()>) {
    let Some(data) = data else {
        return (None, Vec::new(), Task::ready(()));
    };
    let (tx, mut rx) = futures::channel::mpsc::unbounded::<Message>();
    let bus = match templum_mcp::bus::start(data, tx) {
        Ok(bus) => bus,
        Err(err) => {
            eprintln!(
                "templum: couldn't start the bus in {}: {err}",
                data.display()
            );
            return (None, Vec::new(), Task::ready(()));
        }
    };
    let env = terminal_env(&bus, data);
    if let Some(gemini) = crate::agents::dirs(data).gemini {
        templum_usage::gemini::refresh(data, &gemini, cfg!(windows));
    }
    let task = cx.spawn_in(window, async move |ws, cx| {
        while let Some(message) = rx.next().await {
            if ws
                .update_in(cx, |ws, window, cx| ws.serve(message, window, cx))
                .is_err()
            {
                break;
            }
        }
    });
    (Some(bus), env, task)
}

/// What a tool call comes to.
pub(super) enum Outcome {
    /// Its answer, now.
    Now(Reply),
    /// It waits for this, at most these many seconds (`waits.rs`); None: until it comes.
    Wait(Box<Waiting>, Option<u64>),
    /// A job answers it when it is done (`spawn_agent` making a worktree).
    Later(Task<Reply>),
}

/// What every terminal gets while the bus runs: its address and Templum's folder first in
/// PATH, where `templum` is this Templum. Taken for each terminal: PATH may have changed since.
pub(super) fn terminal_env(bus: &Bus, data: &Path) -> Vec<(String, String)> {
    let mut env = vec![("TEMPLUM_BUS".to_string(), bus.env.clone())];
    let path = templum_terminal::path_var().unwrap_or_default();
    if let Some(path) = templum_folder(data).and_then(|folder| first_on_path(folder, &path)) {
        env.push(("PATH".to_string(), path));
    }
    env
}

/// `path` (a PATH's value) with `folder` in front, without its empty entries: an empty one is
/// the current folder, where any `templum` or `git` a repository holds would run.
pub(super) fn first_on_path(folder: PathBuf, path: &OsStr) -> Option<String> {
    let rest = std::env::split_paths(path).filter(|p| !p.as_os_str().is_empty());
    std::env::join_paths(std::iter::once(folder).chain(rest))
        .ok()?
        .into_string()
        .ok()
}

/// `<data>/bin`, where `templum` is this Templum: on Unix a link to it; on Windows
/// `templum.exe`, the same file or a copy (its own folder may be Downloads, with no
/// `templum.exe` and any other program in it).
// ponytail: one entry for every Templum sharing the data folder: the last one started wins.
fn templum_folder(data: &Path) -> Option<PathBuf> {
    let exe = super::usage::program()?;
    let bin = data.join("bin");
    std::fs::create_dir_all(&bin).ok()?;
    #[cfg(unix)]
    {
        let link = templum_in(data);
        if std::fs::read_link(&link).ok().as_deref() != Some(exe.as_path()) {
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(&exe, &link).ok()?;
        }
    }
    #[cfg(not(unix))]
    same_or_copy(&exe, &templum_in(data)).ok()?;
    Some(bin)
}

/// `<data>/bin/templum` (`templum.exe` on Windows): this Templum, for what outlives it (another
/// agent's configuration, the Gemini CLI extension).
pub(crate) fn templum_in(data: &Path) -> PathBuf {
    data.join("bin").join(if cfg!(windows) {
        "templum.exe"
    } else {
        "templum"
    })
}

/// Makes `to` a hard link to `exe`, or a copy of it where a link can't be made (another drive),
/// unless its length and modified time are already `exe`'s.
#[cfg_attr(unix, allow(dead_code))] // Used on Windows; tested everywhere.
pub(super) fn same_or_copy(exe: &Path, to: &Path) -> std::io::Result<()> {
    let source = std::fs::metadata(exe)?;
    let modified = source.modified()?;
    if std::fs::metadata(to)
        .is_ok_and(|m| m.len() == source.len() && m.modified().ok() == Some(modified))
    {
        return Ok(());
    }
    let _ = std::fs::remove_file(to);
    if std::fs::hard_link(exe, to).is_err() {
        std::fs::copy(exe, to)?;
        // A copy may get the time it is made (Unix's does): it would be made again at each start.
        let _ = std::fs::File::options()
            .write(true)
            .open(to)
            .and_then(|f| f.set_modified(modified));
    }
    Ok(())
}

impl Workspace {
    /// Answers one message from the bus. A tool call that waits is kept (`waits.rs`) and answered
    /// later; meanwhile the connection's next lines are served.
    pub(super) fn serve(&mut self, message: Message, window: &mut Window, cx: &mut Context<Self>) {
        let Message {
            hello,
            conn,
            incoming,
            reply,
        } = message;
        if hello.client == "control" {
            return self.serve_control(hello, conn, incoming, reply, window, cx);
        }
        // Connections off: an agent's server and hooks get nothing, the command line works.
        let off = !self.config.connections.enabled && hello.client != "cli";
        match incoming {
            Incoming::Request { id, method, params } => {
                let answer = match method.as_str() {
                    "initialize" => {
                        // An agent's Templum server started: its terminal has Templum's tools.
                        if hello.client == "mcp" && !off {
                            self.tool_conns.insert(conn, hello.terminal.clone());
                        }
                        let instructions = if off {
                            OFF_INSTRUCTIONS.to_string()
                        } else {
                            self.instructions(&hello.terminal, cx)
                        };
                        rpc::result(&id, tools::initialize_result(&params, &instructions))
                    }
                    "ping" => rpc::result(&id, json!({})),
                    templum_mcp::hooks::INSTRUCTIONS if hello.client == "hook" => {
                        let agent = params["agent"].as_str().unwrap_or_default();
                        let text = self.standing_for(&hello.terminal, agent);
                        rpc::result(&id, json!({ "text": text }))
                    }
                    "tools/list" => rpc::result(&id, tools::list()),
                    "tools/call" if off => tools::tool_result(&id, &Reply::err(CONNECTIONS_OFF)),
                    "tools/call" => {
                        let name = params["name"].as_str().unwrap_or_default();
                        if hello.client == "mcp" {
                            self.allowed(&hello.terminal, name, cx);
                        }
                        let cli_only = hello.client == "cli" && tools::cli_only(name);
                        if !tools::known(name) && !cli_only {
                            rpc::error(&id, rpc::PARAMS, &format!("Unknown tool: {name}"))
                        } else {
                            let outcome = match tools::parse(name, &params["arguments"]) {
                                Ok(call) => self.call(&hello.terminal, call, window, cx),
                                Err(why) => Outcome::Now(Reply::err(why)),
                            };
                            match outcome {
                                Outcome::Now(done) => tools::tool_result(&id, &done),
                                Outcome::Wait(what, seconds) => {
                                    let asked = Asked {
                                        conn,
                                        id,
                                        reply,
                                        from: hello.terminal,
                                    };
                                    self.wait(asked, *what, seconds, window, cx);
                                    return;
                                }
                                Outcome::Later(job) => {
                                    cx.spawn(async move |_, _| {
                                        let done = job.await;
                                        let _ = reply.send(tools::tool_result(&id, &done));
                                    })
                                    .detach();
                                    return;
                                }
                            }
                        }
                    }
                    _ => rpc::error(&id, rpc::METHOD, &format!("Method not found: {method}")),
                };
                let _ = reply.send(answer);
            }
            Incoming::Notification { method, .. }
                if off && method == templum_mcp::hooks::METHOD => {}
            Incoming::Notification { method, params } if method == templum_mcp::hooks::METHOD => {
                if let Ok(event) = serde_json::from_value(params) {
                    self.agent_event(&hello.terminal, event, window, cx);
                }
            }
            // The client gave up on a call (MCP): no answer goes to it.
            Incoming::Notification { method, params } if method == "notifications/cancelled" => {
                self.cancel(conn, &params["requestId"], cx);
            }
            Incoming::Notification { method, .. } if method == templum_mcp::bus::CLOSED => {
                self.closed(conn, cx);
            }
            // `notifications/initialized` and the rest need nothing.
            Incoming::Notification { .. } | Incoming::Response => {}
        }
    }

    /// An agent's hook says where its turn is, and its screen's looks start over. Its turn's end
    /// answers what waited on it.
    fn agent_event(
        &mut self,
        terminal: &str,
        event: templum_mcp::AgentEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use templum_mcp::AgentEvent;
        let Some(pane) = self.pane_mut(terminal) else {
            return;
        };
        // Keys typed before this event answer nothing it brings (a dialog opening now).
        pane.looks.heard();
        pane.typed.set(false);
        // A prompt taken at its Enter, or an agent that started or quit, leaves nothing the person
        // typed. Another's `Submitted` comes once a turn takes a prompt in: the person may be
        // writing the next meanwhile.
        if matches!(
            event,
            AgentEvent::Submitted { at_enter: true } | AgentEvent::Started | AgentEvent::Ended
        ) {
            pane.draft.set(false);
        }
        if !matches!(event, AgentEvent::Stopped { .. }) {
            pane.speaks = true;
        }
        // A turn under way, or a new session: the reports a turn's end left to come aren't
        // settled (`settle`).
        if matches!(
            event,
            AgentEvent::Submitted { .. }
                | AgentEvent::Reports { .. }
                | AgentEvent::Working
                | AgentEvent::NeedsYou { .. }
                | AgentEvent::Started
                | AgentEvent::Ended
        ) {
            pane.settles = None;
        }
        match event {
            AgentEvent::Submitted { at_enter } => {
                // A turn under way, not one that ended while its subagents work: a prompt then
                // starts the next at once.
                let turn = pane.state == Some(AgentState::Working) && !pane.awaits_subagents;
                pane.awaits_subagents = false;
                pane.state = Some(AgentState::Working);
                pane.submitted += 1;
                // Its task, once typed: a spawn's prompt waits in `prompt` until then.
                let typed = pane.prompt.is_none();
                let earlier = pane.subagents.iter().map(|s| s.id.clone()).collect();
                if let Some(handed) = pane.handed.as_mut().filter(|h| typed && !h.taken) {
                    handed.taken = true;
                    handed.subagents = earlier;
                    // Claude Code says so at its Enter, also for a prompt it queues behind its turn.
                    handed.behind = turn && at_enter;
                }
            }
            AgentEvent::Working => {
                // At work again after a permission. Never a new turn: a command an interrupt left
                // running ends later (Codex). A subagent's dialog stays open.
                if pane.state == Some(AgentState::Working) || pane.asks(None) {
                    pane.state = Some(AgentState::Working);
                }
                // A tool's end takes the prompt Claude Code queued into the turn under way.
                if let Some(handed) = pane.handed.as_mut() {
                    handed.behind = false;
                }
            }
            AgentEvent::Started => {
                pane.state = Some(AgentState::Idle);
                // A new session: the last one's transcript isn't its own, nor its subagents.
                pane.transcript = None;
                pane.keep_subagents(|_| false);
                pane.awaits_subagents = false;
                self.subagents_done(terminal, None, window, cx);
            }
            // Its turn ended while its subagents work: the reports they send start its next
            // turns, and the last one's end is its answer (docs/specs/claude-subagents.md).
            AgentEvent::AwaitsSubagents {
                message,
                transcript,
                subagents,
            } => {
                if transcript.is_some() {
                    pane.transcript = transcript;
                }
                // The list is whole: one gone without a word (stopped from its panel) is gone.
                pane.keep_subagents(|s| s.reported || subagents.contains(&s.id));
                for id in subagents {
                    pane.add_subagent(id);
                }
                self.turn_ended(terminal, message, cx);
            }
            AgentEvent::Stopped {
                message,
                transcript,
            } => {
                if transcript.is_some() {
                    pane.transcript = transcript;
                }
                // Claude Code's `Stop` with its answer names the subagents still at work
                // (`AwaitsSubagents` otherwise): none. One counted as such stopped without a word.
                if message.is_some() {
                    pane.keep_subagents(|s| s.reported);
                }
                self.turn_ended(terminal, message, cx);
            }
            // A wait that says nothing new (Claude Code's notification about the dialog its
            // `PermissionRequest` opened) keeps what it asks: `allowed` looks for it.
            AgentEvent::NeedsYou { message: None }
                if matches!(pane.state, Some(AgentState::NeedsYou(Some(_)))) => {}
            AgentEvent::NeedsYou { message } => {
                // Already waiting: the same dialog (Claude Code's notification about it).
                let new = !matches!(pane.state, Some(AgentState::NeedsYou(_)));
                if new || message.is_some() {
                    pane.dialogs += 1;
                }
                pane.state = Some(AgentState::NeedsYou(message.clone()));
                pane.asked_by = None;
                self.needs_you(terminal, message.as_deref(), new, cx);
            }
            AgentEvent::Ended => {
                pane.state = None;
                pane.speaks = false;
                self.agent_left(terminal, cx);
                self.subagents_done(terminal, None, window, cx);
            }
            // Claude Code's own prompt, its subagents' reports: a turn starts that answers no
            // task and takes none in. A report ends that subagent, also one stopped without a
            // word.
            AgentEvent::Reports { subagents } => {
                pane.keep_subagents(|s| !subagents.contains(&s.id));
                pane.awaits_subagents = false;
                if !matches!(pane.state, Some(AgentState::NeedsYou(_))) {
                    pane.state = Some(AgentState::Working);
                }
            }
            AgentEvent::SubagentStarted {
                id,
                kind,
                transcript,
            } => {
                pane.add_subagent(id.clone());
                self.open_subagent(terminal, (&id, &kind), transcript.as_deref(), window, cx);
            }
            // The dialog it asked its person is over: its tool ran, or it stopped.
            AgentEvent::SubagentWorking { id } => pane.dialog_over(&id),
            AgentEvent::SubagentNeedsYou { id, message } => {
                let new = !matches!(pane.state, Some(AgentState::NeedsYou(_)));
                pane.dialogs += 1;
                pane.state = Some(AgentState::NeedsYou(message.clone()));
                pane.asked_by = Some(id);
                self.needs_you(terminal, message.as_deref(), new, cx);
            }
            AgentEvent::SubagentStopped { id, reports } => {
                pane.dialog_over(&id);
                match pane.subagents.iter_mut().find(|s| s.id == id) {
                    // Its report comes in a turn of its own. Never one not heard to start (a new
                    // session since, `/clear`): nothing would ever take it off.
                    Some(subagent) if reports => subagent.reported = true,
                    _ => pane.keep_subagents(|s| s.id != id),
                }
                self.subagents_done(terminal, Some(&id), window, cx);
            }
        }
        cx.notify();
    }

    /// The turn of the agent in `terminal` ended, with `message` (None: without an answer, or
    /// Claude Code's prompt sat idle). With no subagent of its left, that end answers what waited
    /// on it. While one works, nothing: their reports come in turns of their own, the last one's
    /// end is the answer; but a task taken while only these were there is answered now. With only
    /// reported ones left, their reports may never come as a turn (Claude Code took them inside
    /// this one, at a tool's end; a nested one's went to its own parent; one stopped from Claude
    /// Code's panel): unless a turn starts within GRACE, or at once when its prompt sat idle,
    /// they are forgotten and this end answers (docs/specs/claude-subagents.md).
    fn turn_ended(&mut self, terminal: &str, message: Option<String>, cx: &mut Context<Self>) {
        // Its usage changed with the turn: a Claude Code account's limits are asked again.
        if let Some(agent) = self.pane(terminal).and_then(|p| p.seen.agent.clone()) {
            self.ask_claude(Some(&agent), super::usage::CLAUDE_MIN, cx);
        }
        let Some(pane) = self.pane_mut(terminal) else {
            return;
        };
        if let Some(message) = &message {
            pane.answer = Some(message.clone());
        }
        if pane.subagents.is_empty() {
            pane.state = Some(AgentState::Idle);
            pane.awaits_subagents = false;
            self.answer_turn(terminal, message, cx);
            return;
        }
        let at_work = pane.subagents.iter().any(|s| !s.reported);
        if !at_work && message.is_none() {
            // Its prompt sat idle since the turn that awaited them (its answer is that turn's),
            // or the turn under way ended without one (an error, an Esc).
            let kept = pane.awaits_subagents.then(|| pane.answer.clone()).flatten();
            self.settle(terminal, kept, cx);
            return;
        }
        // A task taken while only these were there: their reports aren't its own, and this end
        // is its answer. The agent still awaits them.
        let earlier = pane.handed.as_ref().is_some_and(|h| {
            h.taken && !h.behind && pane.subagents.iter().all(|s| h.subagents.contains(&s.id))
        });
        pane.await_subagents();
        if earlier {
            self.answer_turn(terminal, message.clone(), cx);
        }
        let Some(pane) = self.pane_mut(terminal) else {
            return;
        };
        if !at_work {
            let at = cx.background_executor().now() + GRACE;
            pane.settles = Some(at);
            let terminal = terminal.to_string();
            cx.spawn(async move |ws, cx| {
                cx.background_executor().timer(GRACE).await;
                ws.update(cx, |ws, cx| {
                    if ws.pane(&terminal).is_some_and(|p| p.settles == Some(at)) {
                        ws.settle(&terminal, message, cx);
                    }
                })
                .ok();
            })
            .detach();
        }
    }

    /// The agent in `terminal` ended a turn, with `message`, leaving only reported subagents, and
    /// no turn took their reports in: they are forgotten, and that end answers as it would have
    /// without them.
    fn settle(&mut self, terminal: &str, message: Option<String>, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_mut(terminal) else {
            return;
        };
        pane.settles = None;
        if pane.subagents.iter().any(|s| !s.reported) {
            return;
        }
        pane.keep_subagents(|_| false);
        pane.state = Some(AgentState::Idle);
        pane.awaits_subagents = false;
        self.answer_turn(terminal, message, cx);
    }

    /// The turn of the agent in `terminal` ended with `message`, and nothing more comes of it:
    /// the task it is on ends with it, and what waits on its turn hears it.
    fn answer_turn(&mut self, terminal: &str, message: Option<String>, cx: &mut Context<Self>) {
        let hands_out = self.waits_on_its_own(terminal);
        let Some(pane) = self.pane_mut(terminal) else {
            return;
        };
        // The turn the task was typed in on, or the one Claude Code queued it behind: the next
        // turn's end is its own.
        if let Some(handed) = pane.handed.as_mut().filter(|h| !h.taken || h.behind) {
            handed.behind = false;
            return;
        }
        let reply = match message {
            Some(message) => Reply::ok(message),
            None => {
                Reply::err("Its turn ended without an answer: an error, or it was interrupted.")
            }
        };
        if hands_out {
            // It handed a task of its own over and waits for it, as card 30 asks: this turn's
            // end isn't its answer, and only the calls of agents that didn't hand it its task
            // hear it. Its `reply`, or the end of a turn with nothing handed out any more, is
            // the answer.
            self.answer_others(terminal, reply, cx);
        } else {
            self.end_turn(terminal, reply, cx);
        }
    }

    /// The agent in `terminal` calls the Templum tool `name`: when it was asking its person to
    /// allow that tool (`hooks::claude`), they did, and it works again. Its next hook may come
    /// only at the tool's end, minutes later. Another tool's dialog may be a subagent's.
    fn allowed(&mut self, terminal: &str, name: &str, cx: &mut Context<Self>) {
        let Some(pane) = self.pane_mut(terminal) else {
            return;
        };
        let asked = format!("Allow mcp__templum__{name}?");
        if pane.state == Some(AgentState::NeedsYou(Some(asked))) {
            pane.state = Some(AgentState::Working);
            pane.looks.heard();
            pane.typed.set(false);
            cx.notify();
        }
    }

    /// Types `text` into the terminal `terminal`, then Enter ENTER_AFTER later, as a task's: an
    /// agent may take keys that come in one burst for a paste, where Enter is a new line. True once
    /// Enter went; false when the terminal closed first.
    pub(super) fn type_then_enter(
        &self,
        terminal: String,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Task<bool> {
        if let Some(pane) = self.pane(&terminal) {
            pane.term.type_in(text);
        }
        cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(ENTER_AFTER).await;
            ws.update(cx, |ws, _| {
                ws.pane(&terminal)
                    .map(|pane| pane.term.write(&b"\r"[..]))
                    .is_some()
            })
            .unwrap_or(false)
        })
    }

    /// The block whose terminal is `terminal` (its `TEMPLUM_TERMINAL`): its workspace's id and
    /// its id. Looked up at each call: a block moved to another workspace keeps its terminal.
    pub(crate) fn caller(&self, terminal: &str) -> Option<(String, BlockId)> {
        self.spaces().find_map(|space| {
            space
                .terminals()
                .find(|(_, pane)| pane.term.id() == terminal)
                .map(|(id, _)| (space.id.clone(), id))
        })
    }

    /// The pane of the terminal `terminal` (its `TEMPLUM_TERMINAL`), in any workspace.
    pub(crate) fn pane(&self, terminal: &str) -> Option<&Pane> {
        self.spaces()
            .flat_map(|s| s.blocks.values())
            .filter_map(Body::terminal)
            .find(|pane| pane.term.id() == terminal)
    }

    pub(super) fn pane_mut(&mut self, terminal: &str) -> Option<&mut Pane> {
        std::iter::once(&mut self.space)
            .chain(&mut self.others)
            .flat_map(|s| s.blocks.values_mut())
            .filter_map(Body::terminal_mut)
            .find(|pane| pane.term.id() == terminal)
    }

    pub(super) fn space_named(&self, id: &str) -> Option<&Space> {
        self.spaces().find(|s| s.id == id)
    }

    /// A tool call from the terminal `terminal`: its answer now, or what it waits for.
    fn call(
        &mut self,
        terminal: &str,
        call: Call,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let Some((space_id, me)) = self.caller(terminal) else {
            return Outcome::Now(Reply::err("This terminal isn't a block of this Templum."));
        };
        let space = self.space_named(&space_id).expect("the caller's workspace");
        Outcome::Now(match call {
            Call::Whoami => {
                let (kind, title, folder) = self.about(&space.blocks[&me], cx);
                let mut text = format!(
                    "id: {}\nkind: {kind}\ntitle: {title}\n",
                    space.uids.get(&me).map_or("", String::as_str)
                );
                if let Some(folder) = folder {
                    text.push_str(&format!("folder: {}\n", folder.display()));
                }
                text.push_str(&format!("workspace: {}", space.name));
                let uid = space.uids.get(&me).cloned().unwrap_or_default();
                for line in self.schedule_lines(terminal, &uid) {
                    text.push('\n');
                    text.push_str(&line);
                }
                Reply::ok(text)
            }
            Call::ListBlocks => Reply::ok(self.block_lines(space, me, cx).join("\n")),
            Call::ReadTerminal {
                block,
                lines,
                last_command,
            } => {
                let id = match target(space, me, &block) {
                    Ok(id) => id,
                    Err(reply) => return Outcome::Now(reply),
                };
                self.terminal_text((&space_id, id), &block, lines, last_command)
            }
            Call::SendToTerminal { block, text, enter } => {
                let id = match target(space, me, &block) {
                    Ok(id) => id,
                    Err(reply) => return Outcome::Now(reply),
                };
                return self.type_text(terminal, (&space_id, id), &block, &text, enter, cx);
            }
            Call::Notify { text } => {
                let (_, title, _) = self.about(&space.blocks[&me], cx);
                self.notify_from(space_id, me, title, text, None, window, cx);
                Reply::ok("Shown.")
            }
            Call::Connect { block, timeout } => {
                return self.ask_to_connect(terminal, &block, timeout);
            }
            Call::Delegate {
                block,
                task,
                wait,
                timeout,
            } => return self.delegate(terminal, &block, &task, wait, timeout, None, cx),
            Call::Schedule {
                block,
                text,
                when,
                every,
            } => self.schedule(terminal, &block, &text, when, every, cx),
            Call::CancelSchedule { schedule } => self.cancel_schedule(terminal, &schedule),
            Call::ReplyTo { block, text } => self.reply_to(terminal, block.as_deref(), &text, cx),
            Call::WaitForIdle { block, timeout } => {
                return self.wait_for_idle(terminal, &block, timeout);
            }
            Call::SpawnAgent(spawn) => return self.spawn_agent(terminal, spawn, window, cx),
            Call::WaitFor {
                block,
                event,
                timeout,
                pattern,
                lines,
                wait,
            } => return self.wait_for(terminal, &block, event, timeout, pattern, lines, wait, cx),
            Call::Wire { a, b } => self.wire_blocks(terminal, &a, &b, cx),
            Call::ListCards { board, card } => {
                self.list_cards(terminal, board.as_deref(), card, cx)
            }
            Call::CreateCard(new) => self.create_card(terminal, new, cx),
            Call::CreateBlock(args) => self.create_block(terminal, args, window, cx),
            Call::EditCard(edit) => self.edit_card(terminal, edit, cx),
            Call::MoveCard { card, column } => self.move_card(terminal, card, &column, cx),
            Call::CommentCard { card, text } => self.comment_card(terminal, card, &text, cx),
            Call::EditComment {
                card,
                comment,
                text,
            } => self.edit_comment(terminal, card, comment, &text, cx),
            Call::DeleteComment { card, comment } => {
                self.delete_comment(terminal, card, comment, cx)
            }
            Call::ClaimCard { card } => self.claim_card(terminal, card, cx),
            Call::ReadNote { block } => self.read_note(terminal, &block, cx),
            Call::AppendNote { block, text } => {
                self.append_note(terminal, &block, &text, window, cx)
            }
            Call::ReadChecklist { block } => self.read_checklist(terminal, &block),
            Call::CheckItem { block, item, done } => {
                self.check_item(terminal, &block, item, done, cx)
            }
            Call::AddItems { block, text } => self.add_items(terminal, &block, &text, cx),
            Call::ReplaceNote { block, text, old } => {
                self.replace_note(terminal, &block, &text, old.as_deref(), window, cx)
            }
            Call::EditItem { block, item, text } => {
                self.edit_item(terminal, &block, item, &text, cx)
            }
            Call::DeleteItem { block, item } => self.remove_item(terminal, &block, item, cx),
            Call::OpenFile { path, line } => self.open_file_for(terminal, &path, line, window, cx),
            Call::SendKeys { block, keys } => return self.send_keys(terminal, &block, keys, cx),
            Call::Interrupt { block } => return self.interrupt(terminal, &block, cx),
            Call::StopAgent { block } => self.stop_agent(terminal, &block, cx),
            Call::RemoveWorktree {
                worktree,
                folder,
                discard,
            } => {
                return self.remove_worktree(terminal, &worktree, folder.as_deref(), discard, cx);
            }
            Call::MergeWorktree { worktree, folder } => {
                return self.merge_worktree(terminal, &worktree, folder.as_deref());
            }
            Call::CloseBlock { block } => self.close_block(terminal, &block, window, cx),
            Call::ProposePlan {
                plan,
                title,
                timeout,
            } => {
                let ask = templum_mcp::ask::plan(title, plan);
                let what = Waiting::Question {
                    form: Form::new(&ask),
                    ask,
                    others: Vec::new(),
                    note: None,
                    stopped: false,
                    scroll: Default::default(),
                };
                let seconds = deadline(timeout, self.config.connections.question_timeout);
                return Outcome::Wait(Box::new(what), seconds);
            }
            Call::AskUser { ask, timeout } => {
                let what = Waiting::Question {
                    form: Form::new(&ask),
                    ask,
                    others: Vec::new(),
                    note: None,
                    stopped: false,
                    scroll: Default::default(),
                };
                let seconds = deadline(timeout, self.config.connections.question_timeout);
                return Outcome::Wait(Box::new(what), seconds);
            }
        })
    }

    /// A notification from block `me` of workspace `space`, titled `title`; clicking it shows the
    /// block, or for the request `key` that workspace with its inbox open at it. In the
    /// background it is a system notification too.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn notify_from(
        &self,
        space: String,
        me: BlockId,
        title: String,
        text: String,
        request: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = cx.entity().downgrade();
        let notification = Notification::new()
            .title(title)
            .message(text)
            .delivery(self.delivery(window))
            .on_click(move |_, window, cx| {
                ws.update(cx, |ws, cx| match request {
                    Some(key) => ws.show_request(&space, key, window, cx),
                    None => ws.reveal(&space, me, window, cx),
                })
                .ok();
            });
        show_notification(window, cx, notification);
    }

    /// A block's kind, title (made safe to hand an agent) and folder, as `whoami` and
    /// `list_blocks` say them.
    pub(super) fn about(&self, body: &Body, cx: &App) -> (&'static str, String, Option<PathBuf>) {
        let title = || crate::block::title(self, body, cx).to_string();
        let (kind, title, folder) = match body {
            Body::Terminal(pane) => (
                "terminal",
                self.agent_name(pane)
                    .map_or_else(|| pane.term.title(), str::to_string),
                pane.seen.folder.clone(),
            ),
            Body::Kanban(_) => ("kanban", title(), None),
            Body::Note(_) => ("note", title(), None),
            Body::Checklist(_) => ("checklist", title(), None),
            Body::Editor(e) => ("editor", title(), Some(e.folder.clone())),
        };
        (kind, clean_title(&title), folder)
    }

    /// The title agents see for the block `uid`, in any workspace; None when none has it.
    pub(crate) fn uid_title(&self, uid: &str, cx: &App) -> Option<String> {
        self.spaces().find_map(|s| {
            let id = s.block_by_uid(uid)?;
            Some(self.about(s.blocks.get(&id)?, cx).1)
        })
    }

    /// One line per block of `space`, back to front: id, kind, title, folder, and its tie to
    /// the caller `me`.
    fn block_lines(&self, space: &Space, me: BlockId, cx: &App) -> Vec<String> {
        space
            .canvas
            .blocks()
            .iter()
            .map(|b| {
                let (kind, title, folder) = self.about(&space.blocks[&b.id], cx);
                let uid = space.uids.get(&b.id).map_or("", String::as_str);
                let folder = folder
                    .map(|f| format!("  {}", f.display()))
                    .unwrap_or_default();
                let tie = if b.id == me {
                    "you"
                } else if space.wired(me, b.id) {
                    "connected"
                } else {
                    "not connected"
                };
                let state = agent_state(space, b.id);
                let tools = match space.terminal(b.id) {
                    Some(pane) if pane.follows.is_some() => {
                        ", a Claude Code subagent's transcript, read-only"
                    }
                    Some(pane) if pane.seen.agent.is_some() && !self.has_tools(pane.term.id()) => {
                        ", no Templum tools"
                    }
                    _ => "",
                };
                format!("{uid}  {kind}  {title}{folder}  {tie}{state}{tools}")
            })
            .collect()
    }

    /// The agent in `terminal` has Templum's tools: its MCP client said `initialize` and is still
    /// connected (docs/specs/agent-protocol.md, 8).
    pub(super) fn has_tools(&self, terminal: &str) -> bool {
        self.tool_conns.values().any(|t| t == terminal)
    }

    /// The terminal of `pane` runs WSL, which gets neither `TEMPLUM_BUS` nor a `templum` it can
    /// run.
    fn pane_in_wsl(&self, pane: &Pane) -> bool {
        self.shell_is(pane.shell.as_ref().map(|s| s.program.as_str()), "wsl")
    }

    /// The agent of `pane`, without Templum's tools, can still run `templum reply -` in its
    /// shell: not in WSL, nor Aider, which has no shell tool.
    pub(super) fn replies_from_shell(&self, pane: &Pane) -> bool {
        !self.pane_in_wsl(pane) && pane.seen.agent.as_deref() != Some("aider")
    }

    /// Why the agent of `pane` has no Templum tools, the first reason that fits, as its master is
    /// told (docs/specs/agent-protocol.md, 8).
    pub(super) fn no_tools_why(&self, pane: &Pane) -> String {
        use templum_terminal::Agent;
        let name = pane.seen.agent.as_deref().unwrap_or_default();
        let agent = Agent::from_name(name);
        if !self.config.connections.enabled {
            return "connections are off (Settings ▸ Connections ▸ Agents in Templum)".into();
        }
        if self.pane_in_wsl(pane) {
            return "it runs in WSL, where Templum can't give it its server".into();
        }
        match agent {
            Some(aider) if aider.name() == "aider" => "Aider has no MCP".into(),
            Some(Agent::GEMINI) => {
                // An account's extensions are its folder's.
                let dir = match pane
                    .account
                    .as_ref()
                    .and_then(|id| self.agents.iter().find(|a| &a.id == id))
                    .and_then(|a| a.account.as_ref())
                {
                    Some(a) => Some(a.kind.state_dir(&a.folder)),
                    None => self
                        .data_dir
                        .as_deref()
                        .and_then(|data| crate::agents::dirs(data).gemini),
                };
                if dir.is_some_and(|dir| templum_usage::gemini::installed(&dir)) {
                    "Gemini CLI starts no server in a folder it doesn't trust".into()
                } else {
                    "Templum's Gemini CLI extension isn't installed (Settings ▸ Connections ▸ \
                     Gemini CLI ▸ Install)"
                        .into()
                }
            }
            Some(Agent::PI) => "Pi has MCP only from 0.99".into(),
            Some(
                agent @ (Agent::CLAUDE
                | Agent::CODEX
                | Agent::COPILOT
                | Agent::OPENCODE
                | Agent::KILO
                | Agent::QWEN),
            ) => format!(
                "its server hasn't connected: it is still starting, its start failed, or it was \
                 started without Templum's words (command {}, a shell without Templum's \
                 integration)",
                agent.command()
            ),
            _ => format!(
                "Templum can't give {} its server for one session: add `templum mcp` to its own \
                 MCP servers (Settings ▸ Connections ▸ Other agents)",
                agent.map_or(name, |a| a.title())
            ),
        }
    }

    /// What the hook of `agent` gives the session starting in `terminal`: the person's
    /// instructions that apply to its workspace and agent, framed (docs/specs/instructions.md).
    /// Empty when none apply, for a terminal that isn't a block, and while connections are off.
    pub(super) fn standing_for(&self, terminal: &str, agent: &str) -> String {
        if !self.config.connections.enabled {
            return String::new();
        }
        let Some((space, pane)) = self.spaces().find_map(|space| {
            let pane = space
                .blocks
                .values()
                .filter_map(Body::terminal)
                .find(|pane| pane.term.id() == terminal)?;
            Some((space, pane))
        }) else {
            return String::new();
        };
        // The agent seen there when it runs the hook's (`codex` typed where Claude Code ran a
        // moment ago is Codex), else the block's account of it, else the hook's own.
        let is_account_of = |id: &str| {
            self.agents
                .iter()
                .find(|a| a.id == id)
                .and_then(|a| a.account.as_ref())
                .is_some_and(|account| account.kind.agent == agent)
        };
        let session = match (pane.agent(), pane.account.as_deref()) {
            (Some(seen), _) if self.runs(Some(seen), agent) => seen,
            (_, Some(account)) if is_account_of(account) => account,
            _ => agent,
        };
        let applying = self
            .standing
            .applying(&space.id, |id| self.runs(Some(session), id));
        if applying.is_empty() {
            return String::new();
        }
        let mut text = STANDING_FRAME.to_string();
        for instruction in applying {
            let name = flat(&instruction.name);
            let name = if name.is_empty() {
                "Instruction"
            } else {
                &name
            };
            text.push_str(&format!(
                "\n\n## {name}\n\n{}",
                clean_text(&instruction.text)
            ));
        }
        text
    }

    /// What `initialize` tells the agent in `terminal` about itself.
    pub(super) fn instructions(&self, terminal: &str, cx: &App) -> String {
        let Some((space_id, me)) = self.caller(terminal) else {
            return "This terminal isn't a block of this Templum: its tools won't work here."
                .into();
        };
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let (_, title, _) = self.about(&space.blocks[&me], cx);
        let uid = space.uids.get(&me).map_or("", String::as_str);
        // The other blocks, split by their wire to the caller.
        let (connected, others): (Vec<_>, Vec<_>) = space
            .canvas
            .blocks()
            .iter()
            .filter(|b| b.id != me)
            .map(|b| {
                let (kind, title, _) = self.about(&space.blocks[&b.id], cx);
                let uid = space.uids.get(&b.id).map_or("", String::as_str);
                (space.wired(me, b.id), format!("{uid} ({kind} {title:?})"))
            })
            .partition(|(wired, _)| *wired);
        let list = |blocks: Vec<(bool, String)>| {
            if blocks.is_empty() {
                "none".to_string()
            } else {
                blocks
                    .into_iter()
                    .map(|(_, b)| b)
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        // The person's roles, by id and agent: at most ROLES_TOLD.
        let mut roles: Vec<String> = self
            .config
            .roles
            .iter()
            .take(ROLES_TOLD)
            .map(|(id, role)| format!("{id} ({})", flat(&role.agent)))
            .collect();
        if self.config.roles.len() > ROLES_TOLD {
            roles.push("…".into());
        }
        let roles = match roles.is_empty() {
            true => String::new(),
            false => format!(" Roles for spawn_agent: {}.", roles.join(", ")),
        };
        // Agents first: Claude Code shows its own Agent tool whole and Templum's tools by name
        // only, Codex weighs the first 512 characters most, and Claude Code cuts at 4096 the
        // block lists may outgrow.
        format!(
            "In Templum, an agent the person asks for is a block on this canvas: start one with \
             spawn_agent, or hand work to an agent connected to you with delegate. Your built-in \
             subagents run hidden inside you, where the person can't see them: use them only when \
             the person asks for a subagent. \
             You are block {uid}, {title:?}, in Templum workspace {:?}. Connected to you: {}. In this \
             workspace, not connected (ask with connect): {}. Name blocks by these ids. You may only read \
             or write your own block and those connected to you; connect asks the person for another. \
             list_blocks gives the current list.{roles} When a connected kanban has your task, claim its \
             card (claim_card: a card for another agent isn't yours) and move it to Doing when you \
             start; check its checklist's items as you finish them (edit_card); when you are done, \
             comment on it what you did and what the next agent needs to know (comment_card), then \
             move it to Review (Done when the board has no Review); write what the next agent needs \
             to know in the connected note too, and check the items of a connected checklist as you \
             finish them. For work of several steps, open your own checklist with create_block and \
             check its items as you go. A task you \
             hand over without wait comes back typed into your terminal as [task …]: it isn't the \
             person writing. A task handed to you begins [from …, task t7]: when it is done, answer \
             it once with reply, your whole answer as its text. When you need the person, ask with \
             ask_user, not on your screen.",
            space.name,
            list(connected),
            list(others),
        )
    }
}

/// The turn of the agent in block `id`, as `list_blocks` says it: `, working`, `, idle`, `,
/// waiting for its person: <what>`; nothing for a block with no agent, or one not yet heard.
pub(super) fn agent_state(space: &Space, id: BlockId) -> String {
    space
        .terminal(id)
        .filter(|pane| pane.seen.agent.is_some())
        .and_then(|pane| Some((pane, pane.state.as_ref()?)))
        .map(|(pane, state)| match state {
            // Not agent-protocol's subagents, the agents it handed tasks to.
            AgentState::Working => match pane.subagents_at_work() {
                0 => ", working".to_string(),
                1 => ", working: 1 Claude Code subagent".to_string(),
                n => format!(", working: {n} Claude Code subagents"),
            },
            AgentState::Idle => ", idle".to_string(),
            AgentState::NeedsYou(Some(what)) if !what.is_empty() => {
                format!(", waiting for its person: {}", clean_title(what))
            }
            AgentState::NeedsYou(_) => ", waiting for its person".to_string(),
        })
        .unwrap_or_default()
}

/// The most characters of a title an agent is told.
const TITLE_CHARS: usize = 60;
/// The most roles the instructions name.
const ROLES_TOLD: usize = 10;

/// Text as agents are told it, on one line: no control characters, no invisible ones, its line
/// breaks and runs of blanks as one space.
pub(super) fn flat(text: &str) -> String {
    let kept: String = text
        .chars()
        .filter(|&c| (c.is_whitespace() || !c.is_control()) && !invisible(c))
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A title as agents are told it. A terminal's is whatever its programs set (OSC 0), and goes
/// into an agent's instructions: `flat`, and at most 60 characters.
pub(super) fn clean_title(title: &str) -> String {
    let flat = flat(title);
    if flat.chars().count() <= TITLE_CHARS {
        return flat;
    }
    let cut: String = flat.chars().take(TITLE_CHARS - 1).collect();
    format!("{cut}…")
}

/// The block `uid` names in `space`, when the caller `me` may use it: its own, or one wired to
/// it, looked at on each call.
pub(super) fn target(space: &Space, me: BlockId, uid: &str) -> Result<BlockId, Reply> {
    let Some(id) = space.block_by_uid(uid) else {
        return Err(Reply::err(format!(
            "There's no block {uid} in your workspace: list_blocks lists them."
        )));
    };
    if id != me && !space.wired(me, id) {
        return Err(Reply::err(format!(
            "{uid} isn't connected to you. Ask the person with connect(\"{uid}\")."
        )));
    }
    Ok(id)
}
