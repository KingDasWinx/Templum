//! Keeping the canvas: restoring a saved one, and writing snapshots of every workspace.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::notification::Notification;
use gpui_kit::{App, Context, Window};
use templum_canvas::{BlockId, Camera, Point, Rect, Size, chrome};

use crate::agents::AgentEntry;
use crate::automations::{self, Automations};
use crate::board::{self, Boards};
use crate::checklist::{self, Checklist};
use crate::checklists::{self, Checklists};
use crate::editor::{self, Editor};
use crate::git_block::{self, Git};
use crate::instructions::{self, Instructions};
use crate::kanban::{self, Kanban};
use crate::note::{self, Note};
use crate::notes;
use crate::session;

use super::{Body, MAX_COLS, MAX_ROWS, Pane, Space, Workspace, show_notification};

impl Workspace {
    /// Puts a saved canvas back: the camera, then each block back to front, each in its folder.
    pub(super) fn restore(
        &mut self,
        saved: session::Saved,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.space.pristine = false;
        // What was saved isn't a change to take back.
        self.quiet = true;
        let c = saved.camera;
        if c.x.is_finite() && c.y.is_finite() {
            self.space.canvas.camera = Camera::at(Point::new(c.x, c.y), c.zoom);
        }
        if let Some(git) = &saved.git {
            self.restore_git(git, window, cx);
        }
        let mut ids = Vec::new();
        let mut gone = Vec::new();
        let mut failed = false;
        for item in &saved.items {
            let t = match item {
                session::Item::Terminal(t) => t,
                other => {
                    ids.push(self.add_saved(other, window, cx));
                    continue;
                }
            };
            let Some(rect) = self.restorable(t.rect.into()) else {
                ids.push(None);
                continue;
            };
            let folder = match t.folder.as_ref().map(PathBuf::from) {
                Some(f) if f.is_dir() => Some(f),
                Some(f) => {
                    gone.push(f.display().to_string());
                    None
                }
                None => None,
            };
            // An agent resumes the folder's latest conversation: anywhere else, it's another one.
            let in_its_folder = folder.is_some();
            let shell = t.shell.as_deref().and_then(|l| self.found_shell(l));
            let id = self.spawn_block_with(rect, folder, Vec::new(), shell, window, cx);
            failed |= id.is_none();
            if let Some(id) = id {
                self.restore_terminal(id, t, in_its_folder, cx);
                if let Some(pane) = self.space.terminal_mut(id) {
                    pane.restored = true;
                }
            }
            ids.push(id);
        }
        self.quiet = false;
        for (item, id) in saved.items.iter().zip(&ids) {
            if let Some(id) = id {
                self.adopt_uid(*id, item.uid());
            }
        }
        // The wires, by block id, once the blocks have theirs back: one whose block didn't come
        // back, of a block to itself, or a second between two, is dropped.
        for wire in &saved.wires {
            let (Some(a), Some(b)) = (
                self.space.block_by_uid(&wire.a),
                self.space.block_by_uid(&wire.b),
            ) else {
                continue;
            };
            let label = wire
                .label
                .as_deref()
                .map(str::trim)
                .filter(|l| !l.is_empty());
            if self.space.add_wire(a, b).is_some()
                && let Some(label) = label
            {
                let key = super::wires::label_key(&wire.a, &wire.b);
                let label = label.chars().take(super::wires::LABEL_MAX).collect();
                self.space.labels.insert(key, label);
            }
        }
        self.space.focused = saved.focused.and_then(|i| ids.get(i).copied().flatten());
        // Grid mode as it was left, over the canvas as it was.
        if let Some(tiles) = saved
            .grid
            .as_ref()
            .and_then(|g| g.tile(|i| ids.get(i).copied().flatten()))
        {
            self.tile_space(tiles, super::tiling::grid_area(self.viewport(window)));
            if !self.space.focused.is_some_and(|f| self.space.shown(f)) {
                self.space.focused = self.space.top();
            }
        }
        if failed {
            // A shell setting with a typo: fixing it must bring the canvas back, so it isn't
            // written over with the blocks that couldn't start.
            self.space.frozen = true;
            say_unstarted(window, cx);
        }
        if !gone.is_empty() {
            let message = format!(
                "Gone, so their terminals start in the home folder: {}",
                gone.join(", ")
            );
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::warning(message));
        }
        self.space.ink = saved
            .ink
            .iter()
            .filter_map(session::InkItem::drawing)
            .collect();
        self.space.recent = saved.recent.iter().map(PathBuf::from).collect();
        self.space.saved = Some(saved);
    }

    /// Keeps the file of each workspace where a restored shell ended as it started
    /// (`Pane::died_starting`), as when one can't start at all: its block stays, and its saved
    /// canvas is the next start's. Before every save, also one before any frame (the window
    /// minimized). True when it kept one more, to say so.
    pub(super) fn keep_dead_starts(&mut self) -> bool {
        let mut died = false;
        for space in std::iter::once(&mut self.space).chain(&mut self.others) {
            if !space.frozen && space.terminals().any(|(_, pane)| pane.died_starting()) {
                space.frozen = true;
                died = true;
            }
        }
        died
    }

    /// A saved terminal `saved`, just started as the terminal `id`: who opened it, and, when
    /// `resume`, its agent (one Templum knows) typed back with its session when it was found, and
    /// its model and effort when it can type them safely (a hand-edited file may hold others),
    /// worded for the shell the block runs. The shell reads it once it is ready.
    fn restore_terminal(
        &mut self,
        id: BlockId,
        saved: &session::TerminalItem,
        resume: bool,
        cx: &mut Context<Self>,
    ) {
        // Its account, when Templum still has it: one deleted gives a plain terminal.
        let account = saved
            .account
            .as_deref()
            .and_then(|a| {
                self.agents
                    .iter()
                    .find(|e| e.id == a && e.account.is_some())
            })
            .map(|e| e.id.clone());
        if let Some(pane) = self.space.terminal_mut(id) {
            pane.opened_by = saved.opened_by.clone();
            pane.account = account;
            pane.role = saved.role.clone();
        }
        let Some(agent) = saved
            .agent
            .as_deref()
            .and_then(|a| self.agents.iter().find(|known| known.id == a))
            .filter(|_| resume)
        else {
            return;
        };
        let session = saved.session.as_deref();
        let kept = match (session, &self.data_dir) {
            (Some(id), Some(data)) => {
                let (of, home) = match &agent.account {
                    Some(a) => (a.kind.agent, Some(a.kind.state_dir(&a.folder))),
                    None => (agent.id.as_str(), None),
                };
                let dirs = crate::agents::dirs(data);
                templum_usage::session_kept(of, id, &dirs, home.as_deref())
            }
            _ => true,
        };
        let command = agent.resume_command_for(session, kept);
        let (model, effort) = (saved.model.as_deref(), saved.effort.as_deref());
        // Its role's words again, while the role exists; its instructions are in the conversation.
        let words = saved
            .role
            .as_ref()
            .and_then(|r| self.config.roles.get(r)?.args.clone());
        let args = agent.launch_args_with(model, effort, words.as_deref());
        // For the shell the block runs, not the default's.
        let shell = self
            .space
            .terminal(id)
            .and_then(|p| p.shell.as_ref())
            .map(|s| s.program.as_str());
        let line = format!(
            "{}\r",
            self.typed_in(
                shell,
                Some(agent),
                &format!("{command}{}", args.as_deref().unwrap_or(""))
            )
        );
        let (agent, title) = (agent.id.clone(), agent.title.clone());
        if let Some(pane) = self.space.terminal_mut(id) {
            pane.term.write(line.into_bytes());
            pane.seen.agent = Some(agent);
            // A slow shell, or a question of its that takes the line: until the agent is up, the
            // next start resumes it still.
            pane.resuming = Some((std::time::Instant::now(), saved.clone()));
            if args.is_ok() {
                (pane.model, pane.effort) = (saved.model.clone(), saved.effort.clone());
                pane.flags_for = pane.seen.agent.clone();
            }
            self.cover_start(id, title, cx);
        }
    }

    /// The shell of that label among the shells found here. A saved terminal names its shell by
    /// its label, never by a program: a file edited or from another computer can only pick a
    /// shell this computer lists, and one that isn't here opens the default (the block forgets
    /// it, and the canvas loses no block for a shell that is gone).
    fn found_shell(&self, label: &str) -> Option<templum_terminal::ShellOption> {
        let found = self.shells.iter().find(|s| s.label == label).cloned();
        if found.is_none() {
            eprintln!("templum: no shell {label:?} here: the default one opens");
        }
        found
    }

    /// A block from a saved item, on top and focused. A terminal starts a fresh shell in the
    /// item's folder (the home folder when it is gone), resuming its agent there. None when its
    /// rect has a number that isn't finite, or its shell couldn't start.
    pub(super) fn add_saved(
        &mut self,
        item: &session::Item,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        match item {
            session::Item::Terminal(t) => {
                let rect = self.restorable(t.rect.into())?;
                let folder = t.folder.as_ref().map(PathBuf::from).filter(|f| f.is_dir());
                // An agent resumes the folder's latest conversation: anywhere else, it's another one.
                let in_its_folder = folder.is_some();
                // The agent it resumes gets its variable, as the one it opened did.
                let agent = t
                    .agent
                    .as_deref()
                    .and_then(|a| self.agents.iter().find(|known| known.id == a))
                    .filter(|_| in_its_folder)
                    .cloned();
                // Its account's variable, with or without its agent, even out of its folder.
                let account = t
                    .account
                    .as_deref()
                    .and_then(|a| {
                        self.agents
                            .iter()
                            .find(|e| e.id == a && e.account.is_some())
                    })
                    .cloned();
                if let Some(account) = &account {
                    self.prepare_account(account, window, cx);
                }
                // The block's own shell, as it was saved: not the agent's setting now.
                let shell = t.shell.as_deref().and_then(|l| self.found_shell(l));
                let program = shell.as_ref().map(|s| s.program.as_str());
                let mut vars = self.agent_vars(agent.as_ref(), program, &super::usage::in_env);
                if let Some((name, folder)) = account.as_ref().and_then(AgentEntry::var)
                    && !vars.iter().any(|(k, _)| k == name)
                {
                    vars.insert(0, (name.to_string(), folder.to_string_lossy().into_owned()));
                }
                let id = self.spawn_block_with(rect, folder, vars, shell, window, cx)?;
                self.restore_terminal(id, t, in_its_folder, cx);
                Some(id)
            }
            session::Item::Kanban(k) => clamped_rect(k.rect.into(), kanban::MIN).map(|rect| {
                self.add_block(
                    rect,
                    Body::Kanban(Kanban {
                        board: k.board,
                        remote: false,
                    }),
                )
            }),
            session::Item::Note(n) => {
                let rect = clamped_rect(n.rect.into(), note::MIN)?;
                let mut ids = n.notes.clone();
                // A file from before notes.json: its text becomes a note there.
                if ids.is_empty() && !n.text.trim().is_empty() {
                    ids.push(self.notes.add(n.text.clone(), n.color, notes::now()));
                }
                let shown = n.notes.get(n.active).copied();
                // A note is open in one tab at most; a deleted one is gone.
                ids.retain(|id| self.note_open_at(*id).is_none());
                let tabs: Vec<_> = ids
                    .into_iter()
                    .filter_map(|id| self.note_tab(id, window, cx))
                    .collect();
                let active = tabs.iter().position(|t| Some(t.id) == shown).unwrap_or(0);
                Some(self.add_block(
                    rect,
                    Body::Note(Note {
                        tabs,
                        active,
                        ..Default::default()
                    }),
                ))
            }
            session::Item::Checklist(c) => {
                let rect = clamped_rect(c.rect.into(), checklist::MIN)?;
                // A file from before one checklist per block: its tab on screen, else the first
                // of its tabs still there.
                let tabs = c.checklists.get(c.active).into_iter().chain(&c.checklists);
                // A checklist is open in one block at most, even named twice; a deleted one is
                // gone.
                let list = c.checklist.iter().chain(tabs).copied().find(|&id| {
                    self.checklists.get(id).is_some() && self.checklist_open_at(id).is_none()
                });
                // Its other tabs' checklists stay in the Checklists dialog, but for blank ones:
                // nothing is lost.
                for &id in &c.checklists {
                    if Some(id) != list && self.checklist_open_at(id).is_none() {
                        self.drop_checklist_if_blank(id);
                    }
                }
                let checklist = Checklist {
                    list,
                    editing: None,
                    scroll: Default::default(),
                    remote: false,
                };
                Some(self.add_block(rect, Body::Checklist(checklist)))
            }
            // An older Templum's Git block: the workspace's Git, unless it has one.
            session::Item::Git(g) => {
                if self.space.git.is_none() {
                    self.restore_git(&g.git, window, cx);
                }
                None
            }
            session::Item::Editor(e) => clamped_rect(e.rect.into(), editor::MIN).map(|rect| {
                let folder = PathBuf::from(&e.folder);
                let mut editor = Editor::new(folder.clone(), e.tree, cx);
                for rel in &e.open {
                    let rel = Path::new(rel);
                    // Only files inside the folder, as they were saved: a path that climbs out
                    // or is absolute is not followed.
                    let inside = rel
                        .components()
                        .all(|c| matches!(c, std::path::Component::Normal(_)));
                    let path = folder.join(rel);
                    if inside && path.is_file() {
                        editor.open_tab(&path, window, cx).ok();
                    }
                }
                if let Some(a) = e.active.filter(|a| *a < editor.tabs.len()) {
                    editor.active = Some(a);
                }
                self.add_block(rect, Body::Editor(Box::new(editor)))
            }),
        }
    }

    /// The workspace's Git as it was saved, looked up again.
    fn restore_git(&mut self, g: &session::SavedGit, window: &mut Window, cx: &mut Context<Self>) {
        let mut git = Git::new(
            PathBuf::from(&g.folder),
            git_block::Tab::from_name(&g.tab),
            window,
            cx,
        );
        git.lists_folded = g.lists == "folded";
        git.set_split(g.diff == "split");
        git.summary
            .update(cx, |s, cx| s.set_value(g.summary.clone(), window, cx));
        git.description
            .update(cx, |s, cx| s.set_value(g.description.clone(), window, cx));
        self.space.git = Some(Box::new(git));
        self.look_up_git(window, cx);
    }

    /// The agents to look for in terminals, for a snapshot's background read.
    pub(super) fn known_agents(&self) -> Arc<Vec<templum_terminal::Detect>> {
        Arc::new(crate::agents::detects(&self.agents))
    }

    /// The accounts, whose agent found in one's block is that account.
    pub(super) fn known_accounts(&self) -> Arc<Vec<crate::agents::Home>> {
        Arc::new(crate::agents::homes(&self.agents))
    }

    /// Every session::EVERY: reads the processes and writes each workspace's file in the
    /// background, when its snapshot differs from the last one written.
    pub(super) fn snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // The API's tokens' last uses, told by its server since the last one.
        self.keep_api_uses(cx.background_executor().now());
        // A token that expired takes its automations with it.
        let names = self.token_names();
        self.api_cut_rules(&names, cx);
        let Some(dir) = self.data_dir.clone().filter(|_| !self.first_run_welcome()) else {
            return;
        };
        if self.snapshotting {
            return;
        }
        if self.keep_dead_starts() {
            say_unstarted(window, cx);
        }
        self.snapshotting = true;
        self.generation += 1;
        let generation = self.generation;
        let (known, accounts) = (self.known_agents(), self.known_accounts());
        // A stopped workspace's file holds its canvas until it starts: it isn't written.
        let jobs: Vec<_> = self
            .spaces()
            .filter(|s| !s.stopped && !s.is_remote())
            .map(|s| {
                let layout = s.layout(known.clone(), accounts.clone(), cx);
                // A frozen workspace's file is never written, but its terminals are read.
                let last = (!s.frozen).then(|| s.saved.clone());
                (s.id.clone(), s.terminal_order(), layout, last)
            })
            .collect();
        let boards = (!self.boards_frozen && self.boards_saved.as_ref() != Some(&self.boards))
            .then(|| self.boards.clone());
        let notes = (!self.notes_frozen && self.notes_saved.as_ref() != Some(&self.notes))
            .then(|| self.notes.clone());
        let checklists = (!self.checklists_frozen
            && self.checklists_saved.as_ref() != Some(&self.checklists))
        .then(|| self.checklists.clone());
        let standing = (!self.standing_frozen
            && self.standing_saved.as_ref() != Some(&self.standing))
        .then(|| self.standing.clone());
        let routines = (!self.automations_frozen
            && self.automations_saved.as_ref() != Some(&self.automations))
        .then(|| self.automations.clone());
        cx.spawn_in(window, async move |ws, cx| {
            let (done, boards, notes, checklists, standing, routines) = cx
                .background_executor()
                .spawn(async move {
                    let notes = notes.map(|n| {
                        let written = notes::save(&dir, &n, generation);
                        (n, written)
                    });
                    // A canvas names its notes by id: it is written once they are.
                    let notes_kept = !matches!(notes, Some((_, Err(_))));
                    let done = jobs
                        .into_iter()
                        .map(|(id, terminals, layout, last)| {
                            let (saved, found) = layout.snapshot(Some(&dir));
                            let terminals = terminals.into_iter().zip(found).collect();
                            let written = last
                                .filter(|last| notes_kept && last.as_ref() != Some(&saved))
                                .map(|_| session::save(&dir, &id, &saved, generation));
                            (id, terminals, saved, written)
                        })
                        .collect::<Vec<_>>();
                    let boards = boards.map(|b| {
                        let written = board::save(&dir, &b, generation);
                        (b, written)
                    });
                    let checklists = checklists.map(|c| {
                        let written = checklists::save(&dir, &c, generation);
                        (c, written)
                    });
                    let standing = standing.map(|s| {
                        let written = instructions::save(&dir, &s, generation);
                        (s, written)
                    });
                    let routines = routines.map(|r| {
                        let written = automations::save(&dir, &r, generation);
                        (r, written)
                    });
                    (done, boards, notes, checklists, standing, routines)
                })
                .await;
            ws.update_in(cx, |ws, window, cx| {
                ws.snapshot_done(
                    done, boards, notes, checklists, standing, routines, window, cx,
                )
            })
            .ok();
        })
        .detach();
    }

    // One argument for each app-wide file a snapshot writes.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn snapshot_done(
        &mut self,
        done: Vec<SpaceSnapshot>,
        boards: Option<(Boards, std::io::Result<()>)>,
        notes: Option<(notes::Notes, std::io::Result<()>)>,
        checklists: Option<(Checklists, std::io::Result<()>)>,
        standing: Option<(Instructions, std::io::Result<()>)>,
        routines: Option<(Automations, std::io::Result<()>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.snapshotting = false;
        let mut wrote = false;
        let mut failed = None;
        match boards {
            Some((boards, Ok(()))) => {
                wrote = true;
                self.boards_saved = Some(boards);
            }
            Some((_, Err(err))) => failed = Some(err),
            None => {}
        }
        match notes {
            Some((notes, Ok(()))) => {
                wrote = true;
                self.notes_saved = Some(notes);
            }
            Some((_, Err(err))) => failed = Some(err),
            None => {}
        }
        match checklists {
            Some((checklists, Ok(()))) => {
                wrote = true;
                self.checklists_saved = Some(checklists);
            }
            Some((_, Err(err))) => failed = Some(err),
            None => {}
        }
        match standing {
            Some((standing, Ok(()))) => {
                wrote = true;
                self.standing_saved = Some(standing);
            }
            Some((_, Err(err))) => failed = Some(err),
            None => {}
        }
        match routines {
            Some((routines, Ok(()))) => {
                wrote = true;
                self.automations_saved = Some(routines);
            }
            Some((_, Err(err))) => failed = Some(err),
            None => {}
        }
        let homes = crate::agents::homes(&self.agents);
        for (id, terminals, saved, written) in done {
            let gone = self
                .space_by_id_mut(&id)
                .map(|space| space.see(terminals, &homes))
                .unwrap_or_default();
            for terminal in gone {
                self.agent_left(&terminal, cx);
                self.subagents_done(&terminal, None, window, cx);
            }
            match written {
                None => {}
                Some(Ok(())) => {
                    wrote = true;
                    // A workspace deleted meanwhile is gone, and one stopped meanwhile keeps the
                    // canvas it was stopped with: nothing to update.
                    if let Some(space) = self.space_by_id_mut(&id).filter(|s| !s.stopped) {
                        space.saved = Some(saved);
                    }
                }
                Some(Err(err)) => failed = Some(err),
            }
        }
        if let Some(err) = failed {
            eprintln!("templum: couldn't save the canvas: {err}");
            if !std::mem::replace(&mut self.canvas_save_failed, true) {
                let message = format!("Couldn't save the canvas: {err}");
                show_notification(window, cx, Notification::error(message));
            }
        } else if wrote && std::mem::take(&mut self.canvas_save_failed) {
            show_notification(
                window,
                cx,
                Notification::info("The canvas is saved again".to_string()),
            );
        }
        self.read_usage(window, cx);
        cx.notify();
    }

    /// When the window closes or Templum quits: the last snapshot of every workspace, read and
    /// written at once.
    pub(super) fn save_canvas_now(&mut self, cx: &App) {
        // A card dialog still open: what was typed in it is kept as a draft.
        // A mirrored workspace's card keeps no draft here: the mirror writes nothing of home's.
        if let Some(work) = &self.card_work
            && self
                .space_named(work.borrow().space())
                .is_some_and(|s| !s.is_remote())
        {
            work.borrow().keep(&mut self.boards);
        }
        // A first run's welcome still open: the next start is a first run too.
        let Some(dir) = self.data_dir.clone().filter(|_| !self.first_run_welcome()) else {
            return;
        };
        self.keep_dead_starts();
        self.generation += 1;
        let generation = self.generation;
        if !self.boards_frozen && self.boards_saved.as_ref() != Some(&self.boards) {
            match board::save(&dir, &self.boards, generation) {
                Ok(()) => self.boards_saved = Some(self.boards.clone()),
                Err(err) => eprintln!("templum: couldn't save the boards: {err}"),
            }
        }
        if !self.checklists_frozen && self.checklists_saved.as_ref() != Some(&self.checklists) {
            match checklists::save(&dir, &self.checklists, generation) {
                Ok(()) => self.checklists_saved = Some(self.checklists.clone()),
                Err(err) => eprintln!("templum: couldn't save the checklists: {err}"),
            }
        }
        if !self.standing_frozen && self.standing_saved.as_ref() != Some(&self.standing) {
            match instructions::save(&dir, &self.standing, generation) {
                Ok(()) => self.standing_saved = Some(self.standing.clone()),
                Err(err) => eprintln!("templum: couldn't save the instructions: {err}"),
            }
        }
        if !self.automations_frozen && self.automations_saved.as_ref() != Some(&self.automations) {
            match automations::save(&dir, &self.automations, generation) {
                Ok(()) => self.automations_saved = Some(self.automations.clone()),
                Err(err) => eprintln!("templum: couldn't save the automations: {err}"),
            }
        }
        if !self.notes_frozen && self.notes_saved.as_ref() != Some(&self.notes) {
            match notes::save(&dir, &self.notes, generation) {
                Ok(()) => self.notes_saved = Some(self.notes.clone()),
                Err(err) => {
                    // A canvas names its notes by id: it is written once they are.
                    eprintln!("templum: couldn't save the notes: {err}");
                    return;
                }
            }
        }
        let (known, accounts) = (self.known_agents(), self.known_accounts());
        for space in std::iter::once(&mut self.space).chain(&mut self.others) {
            if space.frozen || space.stopped || space.is_remote() {
                continue;
            }
            let (saved, _) = space
                .layout(known.clone(), accounts.clone(), cx)
                .snapshot(Some(&dir));
            // Written even when it looks unchanged: a snapshot in flight may have written an
            // older one.
            match session::save(&dir, &space.id, &saved, generation) {
                Ok(()) => space.saved = Some(saved),
                Err(err) => eprintln!("templum: couldn't save the canvas: {err}"),
            }
        }
        // A remote workspace's camera is kept in the index, which nothing else writes on a pan.
        if !self.index_frozen
            && self.spaces().any(Space::is_remote)
            && let Err(err) = session::save_index(&dir, &self.index())
        {
            eprintln!("templum: couldn't save the workspace list: {err}");
        }
    }

    /// A saved rect made safe: None with a number that isn't finite; its size kept between the
    /// smallest block and MAX_COLS x MAX_ROWS cells.
    pub(super) fn restorable(&self, r: Rect) -> Option<Rect> {
        restorable_rect(r, self.cell.size)
    }
}

/// `Workspace::restorable` for a cell of `cell`.
pub(super) fn restorable_rect(r: Rect, cell: Size) -> Option<Rect> {
    if ![r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) {
        return None;
    }
    let min = chrome::min_block_size(cell);
    let max = chrome::block_size(MAX_COLS, MAX_ROWS, cell);
    Some(Rect::new(
        r.x,
        r.y,
        r.w.clamp(min.w, max.w),
        r.h.clamp(min.h, max.h),
    ))
}

/// How soon a restored shell that ends, having run nothing, couldn't start.
const DIES_STARTING: Duration = Duration::from_secs(10);

impl Pane {
    /// Restored with its canvas, its shell ended as it started, given no input: it couldn't
    /// start (docs/specs/persistence.md, Errors). One ended by a Ctrl+D or an `exit` did.
    pub(crate) fn died_starting(&self) -> bool {
        self.restored
            && !self.term.had_input()
            && self
                .term
                .lived()
                .is_some_and(|lived| ended_starting(lived, &self.term.status()))
    }
}

/// A shell that lived `lived` ended as it started: within DIES_STARTING, before running a
/// command (with shell integration, `exit` typed is one).
fn ended_starting(lived: Duration, status: &templum_terminal::Status) -> bool {
    lived < DIES_STARTING && status.running.is_none() && status.finished == 0
}

/// Terminals of a saved canvas couldn't start: their workspace's file is kept (`frozen`).
pub(super) fn say_unstarted(window: &mut Window, cx: &mut App) {
    let message = "Some terminals couldn't start, so this session won't save their canvas: \
                   the saved one stays for the next start"
        .to_string();
    eprintln!("templum: {message}");
    show_notification(window, cx, Notification::warning(message));
}

/// A saved rect made safe: None with a number that isn't finite; at least `min`.
fn clamped_rect(r: Rect, min: Size) -> Option<Rect> {
    [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()).then(|| {
        Rect::new(
            r.x,
            r.y,
            r.w.clamp(min.w, 20_000.0),
            r.h.clamp(min.h, 20_000.0),
        )
    })
}

/// A workspace's snapshot, read in the background: its id, what was found in each of its
/// terminals, the file's content, and the write's outcome (None: unchanged).
pub(super) type SpaceSnapshot = (
    String,
    Vec<(BlockId, templum_terminal::Inspection)>,
    session::Saved,
    Option<std::io::Result<()>>,
);

#[cfg(test)]
mod tests {
    use super::*;
    use templum_terminal::Status;

    #[test]
    fn a_shell_couldnt_start_when_it_ended_soon_having_run_nothing() {
        let soon = Duration::from_secs(1);
        assert!(ended_starting(soon, &Status::default()));
        // The person's `exit`, later; or at once, which shell integration tells.
        assert!(!ended_starting(DIES_STARTING, &Status::default()));
        let running = Status {
            running: Some(std::time::Instant::now()),
            ..Status::default()
        };
        assert!(!ended_starting(soon, &running));
        let finished = Status {
            finished: 1,
            ..Status::default()
        };
        assert!(!ended_starting(soon, &finished));
    }
}
