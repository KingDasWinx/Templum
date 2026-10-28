//! `spawn_agent` (docs/specs/connections.md, Spawning): a new agent block beside the caller, wired
//! to it, in its folder, another, or a new Git worktree; its prompt typed once it is ready.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::{Context, Window};
use templum_canvas::{BlockId, Rect, Size, chrome};
use templum_mcp::{Reply, tools};

use super::bus::{Outcome, target};
use super::delegate::{TASK_LEAD, answer_how};
use super::ops::Place;
use super::{AGENT_COLS, AGENT_ROWS, AgentState, Handed, Prompt, Space, Workspace};
use crate::agents::AgentEntry;

/// How often a spawned agent's readiness for its prompt is looked at.
pub(super) const READY_LOOK: Duration = Duration::from_secs(1);
/// Who opens an agent: a block's agent with `spawn_agent` (its terminal's `TEMPLUM_TERMINAL` and
/// its block), the Templum Agent, or an automation or a token of the API (its task's asker and its signature).
pub(super) enum Opener {
    Block { terminal: String, id: BlockId },
    TemplumAgent,
    Automation { asker: String, by: String },
}

/// How long after the spawn a shell may take to start its agent (its startup files): a snapshot
/// that still finds none there lets it go, prompt and task (its folder's trust was declined, a
/// question of its shell took the command).
pub(super) const STARTS_WITHIN: Duration = Duration::from_secs(60);

impl Workspace {
    /// `spawn_agent` from the terminal `from`. With a worktree, git makes it on the background
    /// executor, and the call is answered once the agent opened there.
    pub(super) fn spawn_agent(
        &mut self,
        from: &str,
        spawn: tools::Spawn,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let spawn = match spawn.role.clone() {
            None => spawn,
            Some(id) => match self
                .config
                .roles
                .get(&id)
                .map(|role| role.spawn(&id, spawn))
            {
                Some(Ok(spawn)) => spawn,
                Some(Err(refused)) => return Outcome::Now(Reply::err(refused)),
                None if self.config.roles.is_empty() => {
                    return Outcome::Now(Reply::err(
                        "No roles are saved: [roles] in config.toml holds them.",
                    ));
                }
                None => {
                    let ids: Vec<&str> = self.config.roles.keys().map(String::as_str).collect();
                    return Outcome::Now(Reply::err(format!(
                        "There's no role {id}. Roles: {}.",
                        ids.join(", ")
                    )));
                }
            },
        };
        let (entry, fork) = if spawn.fork {
            match self.fork_of(from, spawn.agent.as_deref()) {
                Ok((entry, command)) => (entry, Some(command)),
                Err(refused) => return Outcome::Now(Reply::err(refused)),
            }
        } else {
            let agent = spawn.agent.as_deref().unwrap_or_default();
            let Some(entry) = self.agents.iter().find(|a| a.id == agent).cloned() else {
                let ids: Vec<&str> = self.agents.iter().map(|a| a.id.as_str()).collect();
                return Outcome::Now(Reply::err(format!(
                    "There's no agent {agent}. These are: {}.",
                    ids.join(", ")
                )));
            };
            (entry, None)
        };
        // Typed into a shell: anything it can't type safely opens nothing, a worktree included.
        let (model, effort) = (spawn.model.as_deref(), spawn.effort.as_deref());
        if let Err(why) = entry.launch_args_with(model, effort, spawn.args.as_deref()) {
            return Outcome::Now(Reply::err(why));
        }
        if let Some(problem) = self.spawn_problem(&entry) {
            return Outcome::Now(Reply::err(problem));
        }
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let folder = match space.folder_for(me, spawn.folder.as_deref()) {
            Ok(folder) => folder,
            Err(refused) => return Outcome::Now(Reply::err(refused)),
        };
        let (from, worktree) = (from.to_string(), spawn.worktree.clone());
        let open =
            move |ws: &mut Self, folder: &Path, window: &mut Window, cx: &mut Context<Self>| {
                let Some((space, me)) = ws.caller(&from) else {
                    return Reply::err("Your block was closed before the agent could open.");
                };
                let opener = Opener::Block {
                    terminal: from.clone(),
                    id: me,
                };
                let place = Place::Near(me);
                ws.open_agent_in(
                    &space,
                    &entry,
                    fork.as_deref(),
                    folder,
                    &spawn,
                    place,
                    &[me],
                    opener,
                    window,
                    cx,
                )
                .0
            };
        self.in_worktree(folder, worktree, None, open, window, cx)
    }

    /// `open` in `folder`, at once; with `worktree`, in a new Git worktree of that name that git
    /// makes on the background executor, the call answered once `open` ran there. With `roots` (a
    /// token limited to some workspaces), the repository's main tree, as git reports it, must lie
    /// inside one of them, or nothing is made.
    pub(super) fn in_worktree(
        &mut self,
        folder: PathBuf,
        worktree: Option<String>,
        roots: Option<Vec<PathBuf>>,
        open: impl FnOnce(&mut Self, &Path, &mut Window, &mut Context<Self>) -> Reply + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let Some(name) = worktree else {
            return Outcome::Now(open(self, &folder, window, cx));
        };
        let program = self.config.git.program().to_string();
        Outcome::Later(cx.spawn_in(window, async move |ws, cx| {
            let made = cx
                .background_executor()
                .spawn(async move {
                    match templum_git::Repo::discover(&program, &folder) {
                        Ok(Some(repo)) => match roots
                            .as_deref()
                            .map(|r| super::api::main_tree_inside(&repo, r))
                        {
                            Some(Err(why)) => Err(why),
                            _ => repo.add_worktree(&name).map_err(|e| {
                                format!("Couldn't make the worktree: {}", e.first_line())
                            }),
                        },
                        Ok(None) => Err(format!(
                            "{} isn't in a Git repository: a worktree needs one.",
                            folder.display()
                        )),
                        Err(e) => Err(format!("Couldn't run git: {}", e.first_line())),
                    }
                })
                .await;
            match made {
                Ok(path) => ws
                    .update_in(cx, |ws, window, cx| open(ws, &path, window, cx))
                    .unwrap_or_else(|_| Reply::err("Templum closed.")),
                Err(why) => Reply::err(why),
            }
        }))
    }

    /// The agent a fork from the terminal `from` opens (the caller's own, `named` when given)
    /// and the command that forks the caller's session, as the last snapshot found it; why not.
    fn fork_of(&self, from: &str, named: Option<&str>) -> Result<(AgentEntry, String), String> {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let pane = space.terminal(me).ok_or("Only an agent forks itself.")?;
        // An account's block runs the account: its id is the agent's.
        let own = pane
            .account
            .clone()
            .or_else(|| pane.seen.agent.clone())
            .ok_or("Templum doesn't see an agent in your block: only an agent forks itself.")?;
        if named.is_some_and(|named| named != own) {
            return Err(format!(
                "A fork is a copy of your own agent, {own}: leave agent out."
            ));
        }
        let entry = self
            .agents
            .iter()
            .find(|a| a.id == own)
            .cloned()
            .ok_or_else(|| format!("{own} isn't an agent of Settings ▸ Agents."))?;
        if entry.forking().is_none() {
            return Err(format!(
                "{} can't fork a session: forks are for Claude Code and Codex.",
                entry.title
            ));
        }
        let uid = space.uids.get(&me).map_or("", String::as_str);
        space
            .session_of(uid, &own)
            .and_then(|session| entry.fork_command(&session))
            .map(|command| (entry, command))
            .ok_or_else(|| {
                "Templum doesn't know your session yet: try again in a few seconds.".into()
            })
    }

    /// Opens `entry` in `folder` in the workspace `space` at `place`, where it overlaps nothing,
    /// wired to `wire_to`: one Ctrl+Z closes it and its wires. The keyboard, the selection and the
    /// camera stay. Its agent starts on the spawn's model and effort (checked by the caller), with
    /// `fork` typed instead of its start command for a fork of the opener's session, and its
    /// prompt is typed once the agent is ready, as a task `opener` handed over. The answer, and
    /// the new block's id when it opened.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_agent_in(
        &mut self,
        space: &str,
        entry: &AgentEntry,
        fork: Option<&str>,
        folder: &Path,
        spawn: &tools::Spawn,
        place: Place,
        wire_to: &[BlockId],
        opener: Opener,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Reply, Option<String>) {
        let (model, effort) = (spawn.model.as_deref(), spawn.effort.as_deref());
        let args = entry
            .launch_args_with(model, effort, spawn.args.as_deref())
            .unwrap_or_default();
        // A task the opener hands over, under its `[from …]` line, as `delegate` types one.
        let (by, asker, opened_by) = match &opener {
            Opener::Block { terminal, id } => {
                let home = self.space_named(space);
                (
                    home.map(|s| self.signature(s, *id, cx)).unwrap_or_default(),
                    terminal.clone(),
                    home.and_then(|s| s.uids.get(id).cloned()),
                )
            }
            Opener::TemplumAgent => (
                "Templum Agent".to_string(),
                templum_agent::ASKER.to_string(),
                Some(templum_agent::ASKER.to_string()),
            ),
            // Wired to nothing, closed by the person only.
            Opener::Automation { asker, by } => (by.clone(), asker.clone(), None),
        };
        let opened = self
            .in_space(space, |ws| {
                let size = chrome::block_size(AGENT_COLS, AGENT_ROWS, ws.cell.size);
                let center = ws.space.camera().to_world(ws.center(window));
                let near = match place {
                    Place::Near(id) => Some(id),
                    Place::Free | Place::At(_) => None,
                };
                let rect = ws.space.spot(&place, size, center)?;
                let kept = (
                    ws.space.focused,
                    ws.space.selected.clone(),
                    ws.space.selected_wire,
                );
                // In grid mode a new block takes half of the focused tile: the one it opens near.
                if near.is_some() {
                    ws.space.focused = near;
                }
                let id = ws.start_agent(Some(entry), fork, &args, folder, rect, window, cx);
                (ws.space.focused, ws.space.selected, ws.space.selected_wire) = kept;
                let id = id?;
                for to in wire_to {
                    ws.space.add_wire(*to, id);
                }
                // Its opener may close it (`close_block`); a resume types its model again.
                let pane = ws.space.terminal_mut(id)?;
                pane.opened_by = opened_by.clone();
                (pane.model, pane.effort) = (spawn.model.clone(), spawn.effort.clone());
                pane.role = spawn.role.clone();
                pane.flags_for = Some(entry.id.clone());
                let terminal = ws.space.terminal(id)?.term.id().to_string();
                Some((ws.space.uids.get(&id)?.clone(), terminal))
            })
            .flatten();
        let Some((uid, terminal)) = opened else {
            let why = format!("Couldn't open {} in {}.", entry.title, folder.display());
            return (Reply::err(why), None);
        };
        let on = match (model, effort) {
            (Some(m), Some(e)) => format!(", on {m} ({e})"),
            (Some(m), None) => format!(", on {m}"),
            (None, Some(e)) => format!(", with {e} effort"),
            (None, None) => String::new(),
        };
        let wired = match &opener {
            Opener::Block { .. } => ", connected to you".to_string(),
            Opener::TemplumAgent | Opener::Automation { .. } if wire_to.is_empty() => String::new(),
            Opener::TemplumAgent | Opener::Automation { .. } => {
                let home = self.space_named(space);
                let uids: Vec<String> = wire_to
                    .iter()
                    .filter_map(|id| home?.uids.get(id).cloned())
                    .collect();
                format!(", connected to {}", uids.join(", "))
            }
        };
        let what = match fork {
            Some(_) => "a fork of your session".to_string(),
            None => entry.title.clone(),
        };
        let mut said = match opener {
            Opener::Block { .. } => {
                format!("Opened {what} as {uid} in {}{wired}{on}.", folder.display())
            }
            Opener::TemplumAgent | Opener::Automation { .. } => {
                format!("Opened {what} as {uid} in {}{on}{wired}.", folder.display())
            }
        };
        if let Some(prompt) = spawn.prompt.as_ref() {
            self.next_task += 1;
            let id = self.next_task;
            // A fork that took itself for the original would answer its tasks too.
            let copy = match (fork, &opened_by) {
                (Some(_), Some(original)) => format!(
                    "You are a copy of the agent in {original}, started from its conversation so \
                     far, in a block of your own.\n\n"
                ),
                _ => String::new(),
            };
            let role = spawn
                .role
                .as_ref()
                .map_or(String::new(), |r| format!(", as {r}"));
            let text = format!(
                "[from {by}, task t{id}{role}] {TASK_LEAD}\n{copy}{prompt}{}",
                answer_how(id)
            );
            let handed = Handed {
                id,
                text: Handed::text_of(prompt),
                // Its session starts with its task: all it spends is the task's.
                usage: Some(templum_usage::Session {
                    cost_usd: Some(0.0),
                    tokens: Some(0),
                    ..Default::default()
                }),
                ..Handed::new(&asker, Duration::from_secs(tools::TASK_WAIT))
            };
            if let Some(pane) = self.pane_mut(&terminal) {
                pane.prompt = Some(Prompt::new(text, entry.builtin));
                pane.handed = Some(handed);
            }
            // Every agent is looked at, hooks or not: Codex's SessionStart comes only with its
            // first prompt, and Claude Code's while it doesn't take pastes yet, after asking
            // whether to trust the folder (a typed Enter would answer that).
            self.watch_ready(terminal, cx);
            said.push_str(&format!(
                " Its prompt is typed once it is ready, as task t{id}."
            ));
        }
        cx.notify();
        (Reply::ok(said), Some(uid))
    }

    /// `close_block` from the terminal `from`: the block `block`, which it opened with
    /// `spawn_agent` and is still wired to (unwiring it takes the consent back), closed without
    /// asking the person, as one step of Ctrl+Z, also in a workspace not shown. The task the
    /// caller handed it ends untold: the caller knows (docs/specs/connections.md, Delegating 12).
    pub(super) fn close_block(
        &mut self,
        from: &str,
        block: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let id = match target(space, me, block) {
            Ok(id) => id,
            Err(refused) => return refused,
        };
        // Any other kind of block was opened by no agent.
        let opener = space.terminal(id).and_then(|p| p.opened_by.as_ref());
        if opener.is_none() || opener != space.uids.get(&me) {
            return Reply::err(format!(
                "Only the person closes {block}: you didn't open it."
            ));
        }
        self.block_close((&space_id, id), block, from, window, cx)
    }

    /// Looks, every READY_LOOK, whether the agent in `terminal` is ready for its prompt, until it
    /// is typed, its block is gone, a snapshot dropped it, or a minute (STARTS_WITHIN) passed.
    pub(super) fn watch_ready(&self, terminal: String, cx: &mut Context<Self>) {
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor().timer(READY_LOOK).await;
                let looking = ws
                    .update(cx, |ws, cx| ws.look_ready(&terminal, cx))
                    .unwrap_or(false);
                if !looking {
                    break;
                }
            }
        })
        .detach();
    }

    /// One look: once a snapshot saw the agent's process, it is still in front of its shell, its
    /// screen is as it was a look ago, it doesn't wait for its person (a hook said so), and
    /// (Claude Code, Codex; not on Windows) it takes typing, its prompt is typed. A minute
    /// (STARTS_WITHIN) after it was handed it is dropped instead, its task ending with the screen.
    /// False once there is nothing more to look for.
    fn look_ready(&mut self, terminal: &str, cx: &mut Context<Self>) -> bool {
        let Some(pane) = self.pane_mut(terminal) else {
            return false;
        };
        if pane.prompt.is_none() {
            return false;
        }
        let late = pane.handed.as_ref();
        if late.is_some_and(|h| h.since.elapsed() >= STARTS_WITHIN) {
            let (what, after) = match pane.prompt.take().is_some_and(|p| p.delegated) {
                true => ("task", "it was handed"),
                false => ("prompt", "it opened"),
            };
            let lines = pane.tail();
            let tail = tools::tail(&lines, tools::MAX_TEXT);
            let uid = self.uid_of(terminal);
            let why = format!(
                "{uid} never got its {what}: a minute after {after}, it still asks its person \
                 something (trusting its folder, Codex's hooks), or its input wasn't ready. Look \
                 at its block. The last lines of its screen:\n{tail}"
            );
            self.end_turn(terminal, Reply::err(why), cx);
            return false;
        }
        let seen = pane.seen.pid.is_some() && !pane.shell_in_front();
        let screen = seen.then(|| pane.screen());
        // Windows' console doesn't pass the cursor's mode on: its still screen alone tells.
        let typing = cfg!(windows) || pane.term.shared().modes().typing;
        // A dialog its hooks told of would take the prompt for its answer.
        let asks = matches!(pane.state, Some(AgentState::NeedsYou(_)));
        let task = pane.handed.as_ref().map(|h| h.id);
        let Some(prompt) = pane.prompt.as_mut() else {
            return false;
        };
        if screen.is_none() || prompt.screen != screen || (prompt.gated && !typing) || asks {
            prompt.screen = screen;
            return true;
        }
        let text = pane.prompt.take().map(|p| p.text).unwrap_or_default();
        // No call waits to say how it went: a task not taken comes back as its end does.
        drop(self.send_task(terminal, &text, task, cx));
        false
    }
}

impl Space {
    /// The folder `named` gives, relative to the folder of block `me` (`~` expanded), else that
    /// folder: where `spawn_agent` and `create_block` open. Why not, when it isn't a folder or
    /// Templum doesn't know `me`'s.
    pub(super) fn folder_for(&self, me: BlockId, named: Option<&str>) -> Result<PathBuf, String> {
        let here = self.blocks[&me]
            .folder()
            .or_else(|| self.terminal(me)?.seen.folder.clone());
        let folder = match (named, here) {
            (Some(named), here) => {
                let named = crate::launcher::expand(named);
                match here {
                    Some(here) if named.is_relative() => here.join(named),
                    _ => named,
                }
            }
            (None, Some(here)) => here,
            (None, None) => {
                return Err("Templum doesn't know your folder yet: name one with folder.".into());
            }
        };
        if !folder.is_dir() {
            return Err(format!("{} isn't a folder.", folder.display()));
        }
        Ok(folder)
    }

    /// A spot for a block of `size` beside block `id` that overlaps nothing, drawn toward the
    /// blocks wired to it, down the column the ones it opened started (`room_beside`); not toward
    /// the block that opened it, behind it, so a subagent's own agents go on its other side. None
    /// when `id` isn't on the canvas.
    pub(super) fn room_near(&self, id: BlockId, size: Size) -> Option<Rect> {
        let home = |id| Some(self.home(self.canvas.get(id)?));
        let opener = (self.terminal(id).and_then(|p| p.opened_by.as_deref()))
            .and_then(|uid| self.block_by_uid(uid));
        let wired = (self.wires.iter().filter(|w| w.touches(id)))
            .map(|w| if w.a == id { w.b } else { w.a })
            .filter(|other| Some(*other) != opener);
        let anchors: Vec<Rect> = std::iter::once(home(id)?)
            .chain(wired.filter_map(home))
            .collect();
        Some(self.room_beside(&anchors, size))
    }
}
