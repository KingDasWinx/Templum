//! The workspace operations both MCP servers call (docs/specs/agent-tools.md#shared-operations,
//! docs/specs/templum-agent.md#shared-operations): each takes its target, never a caller. The
//! block server's tools check the caller's wires first; the control server's check the gate.

use std::path::{Path, PathBuf};

use gpui_kit::component::input::EditorState;
use gpui_kit::{App, Context, Entity, Task, Window};
use templum_canvas::{BlockId, Rect, Size, chrome};
use templum_mcp::{Reply, tools};
use templum_terminal::Pattern;

use super::bus::{Outcome, clean_title, flat};
use super::content::Caller;
use super::history::Then;
use super::{Body, DEFAULT_COLS, DEFAULT_ROWS, Kanban, Space, Workspace, remember};
use crate::board::{Assignee, Card, Priority};
use crate::checklist::{self, Checklist};
use crate::note::{self, Note};
use crate::{attachments, checklists, notes};

/// A block [`Workspace::add_block_in`] opens.
pub(crate) enum NewBlock {
    /// A new note with `text`.
    Note { text: String },
    /// A new checklist named `name` ("Checklist" when blank) with `items`, each checked or not.
    Checklist {
        name: String,
        items: Vec<(String, bool)>,
    },
    /// A terminal in `folder`, `command` typed and run once its shell reads it.
    Terminal {
        folder: PathBuf,
        command: Option<String>,
    },
    /// A kanban showing `board`.
    Kanban { board: u64 },
    /// An editor of `folder`.
    Editor { folder: PathBuf },
}

/// Where [`Workspace::add_block_in`] opens a block, never over another block.
pub(crate) enum Place {
    /// Beside the block, toward the blocks wired to it.
    Near(BlockId),
    /// At the middle of the workspace's view.
    Free,
}

/// What [`Workspace::stop_in`] ended: the program in front of the shell (its agent's name, else its
/// process's; empty when only what it left ran), and how many processes besides it.
pub(super) struct Stopped {
    pub program: String,
    pub processes: usize,
}

impl Workspace {
    /// Ends what runs in the terminal `block` of the workspace `space`: the program in front of
    /// its shell, everything below it, and what it left running after its parent quit; the shell
    /// stays, at its prompt (docs/specs/agent-tools.md#stop_agent). SIGTERM now, SIGKILL KILL_AFTER
    /// later to what is left.
    pub(super) fn stop_in(
        &mut self,
        space: &str,
        block: BlockId,
        cx: &mut Context<Self>,
    ) -> Result<Stopped, String> {
        let space = self.space_named(space).ok_or("That workspace is gone.")?;
        let uid = space.uids.get(&block).map_or("", String::as_str);
        let nothing = format!("Nothing runs in {uid}: its shell is at its prompt.");
        let pane = space
            .terminal(block)
            .ok_or_else(|| format!("{uid} isn't a terminal."))?;
        let shells = pane.term.shells();
        if shells.is_empty() {
            return Err(nothing);
        }
        let program = pane
            .term
            .program(&shells, pane.seen.pid.zip(pane.seen.started));
        let front = program.as_ref().map(|(pid, _)| *pid);
        let found = templum_terminal::strays(pane.term.id(), &shells, front, pane.agent_since());
        if found.is_empty() {
            return Err(nothing);
        }
        let program = match program.filter(|(pid, _)| found.iter().any(|r| r.pid == *pid)) {
            Some((_, name)) => self.agent_name(pane).map_or(name, str::to_string),
            None => String::new(),
        };
        let processes = found.iter().filter(|r| Some(r.pid) != front).count();
        templum_terminal::terminate(&found);
        self.end_later(found, cx);
        Ok(Stopped { program, processes })
    }

    /// Looks for `pattern` in each line the terminal `terminal` (its `TEMPLUM_TERMINAL`) prints
    /// from now on; the watch's id, None when no block of any workspace shows that terminal.
    pub(super) fn watch_add(&self, terminal: &str, pattern: Pattern) -> Option<u64> {
        Some(self.pane(terminal)?.term.watch(pattern))
    }

    /// Opens `new` in the workspace `space`, shown or not, at `place`, as one step of Ctrl+Z: the
    /// keyboard, the selection and the camera stay where they were; in grid mode a block near
    /// another takes half of its tile. A terminal's `opened_by` is `opener` (the block id, or the
    /// Templum Agent's word, of whoever opened it), which may stop and close it.
    pub(crate) fn add_block_in(
        &mut self,
        space: &str,
        new: NewBlock,
        place: Place,
        opener: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<BlockId, String> {
        let added = self.in_space(space, |ws| {
            let size = match &new {
                NewBlock::Note { .. } => note::SIZE,
                NewBlock::Checklist { .. } => checklist::SIZE,
                NewBlock::Terminal { .. } => {
                    chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, ws.cell.size)
                }
                NewBlock::Kanban { .. } => crate::kanban::SIZE,
                NewBlock::Editor { .. } => crate::editor::SIZE,
            };
            let center = ws.space.camera().to_world(ws.center(window));
            let rect = ws
                .space
                .spot(&place, size, center)
                .ok_or("That block isn't in this workspace.")?;
            let kept = (
                ws.space.focused,
                ws.space.selected.clone(),
                ws.space.selected_wire,
            );
            // In grid mode a new block takes half of the focused tile.
            if let Place::Near(near) = place {
                ws.space.focused = Some(near);
            }
            ws.space.pristine = false;
            let now = notes::now();
            // Built here, not by the menus' constructors, which give the new block the keyboard.
            let id = match new {
                NewBlock::Note { text } => {
                    let note = ws.notes.add(text, None, now);
                    let tabs = ws.note_tab(note, window, cx).into_iter().collect();
                    Some(ws.add_block(
                        rect,
                        Body::Note(Note {
                            tabs,
                            ..Default::default()
                        }),
                    ))
                }
                NewBlock::Checklist { name, items } => {
                    let list = ws.checklists.add(now);
                    if !name.trim().is_empty() {
                        ws.checklists.rename(list, name.trim(), now);
                    }
                    for (text, done) in &items {
                        ws.checklists.add_item(list, None, text, *done, now);
                    }
                    let shown = Checklist {
                        list: Some(list),
                        editing: None,
                        scroll: Default::default(),
                    };
                    Some(ws.add_block(rect, Body::Checklist(shown)))
                }
                NewBlock::Terminal { folder, command } => {
                    let id = ws.spawn_block(rect, Some(folder), window, cx);
                    if let Some(pane) = id.and_then(|id| ws.space.terminal_mut(id)) {
                        pane.opened_by = opener;
                        // The shell reads it once it is ready.
                        if let Some(command) = command {
                            pane.term.write(format!("{command}\r").into_bytes());
                        }
                    }
                    id
                }
                NewBlock::Kanban { board } => {
                    let kanban = Kanban { board: Some(board) };
                    Some(ws.add_block(rect, Body::Kanban(kanban)))
                }
                NewBlock::Editor { folder } => {
                    let editor = crate::editor::Editor::new(folder.clone(), true, cx);
                    remember(&mut ws.space.recent, folder);
                    Some(ws.add_block(rect, Body::Editor(Box::new(editor))))
                }
            };
            (ws.space.focused, ws.space.selected, ws.space.selected_wire) = kept;
            id.ok_or("The shell couldn't start.")
        });
        cx.notify();
        added
            .unwrap_or(Err("That workspace is gone."))
            .map_err(str::to_string)
    }

    /// The note the note block `block` of the workspace `space` shows: its whole text becomes
    /// `text`, or with `old`, its one occurrence of `old` does. One step of the note's Ctrl+Z; the
    /// person's selection stays, within the new text.
    pub(super) fn note_replace(
        &mut self,
        space: &str,
        block: BlockId,
        text: &str,
        old: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let editor = self
            .space_named(space)
            .and_then(|s| {
                s.blocks
                    .get(&block)?
                    .note()?
                    .tab()
                    .map(|t| t.editor.clone())
            })
            .ok_or("That block shows no note.")?;
        let value = editor.read(cx).value().to_string();
        let range = match old {
            None => 0..value.len(),
            Some(old) => match value.matches(old).count() {
                0 => return Err(format!("\"{old}\" isn't in the note: read_note shows it.")),
                1 => {
                    let at = value.find(old).unwrap_or_default();
                    at..at + old.len()
                }
                n => {
                    return Err(format!(
                        "\"{old}\" is in the note {n} times: give more of the text around it."
                    ));
                }
            },
        };
        // Also `old` the whole text, or all of it but blanks.
        if value[..range.start].trim().is_empty()
            && text.trim().is_empty()
            && value[range.end..].trim().is_empty()
        {
            return Err("That would empty the note: the person does that.".into());
        }
        editor.update(cx, |e, cx| {
            let kept = e.selected_range();
            e.set_selected_range(range, cx);
            e.replace(text.to_string(), window, cx);
            // Clipped to a character's boundary there.
            let end = e.value().len();
            e.set_selected_range(kept.start.min(end)..kept.end.min(end), cx);
        });
        Ok(())
    }

    /// The text of item `item` of the checklist `list` becomes `text`, as one step of its Ctrl+Z.
    /// False when the checklist has no such item.
    pub(super) fn item_edit(&mut self, list: u64, item: u64, text: &str) -> bool {
        if self
            .checklists
            .get(list)
            .and_then(|l| l.item(item))
            .is_none()
        {
            return false;
        }
        self.edit_checklist(list, |c| c.set_text(list, item, text, notes::now()));
        self.agent_changed(list);
        true
    }

    /// Keeps `schedule`, to be delivered at its time (`schedules.rs`); its id, counting from 1.
    pub(super) fn schedule_add(
        &mut self,
        mut schedule: super::schedules::Schedule,
        cx: &mut Context<Self>,
    ) -> u64 {
        self.next_schedule += 1;
        schedule.id = self.next_schedule;
        self.schedules.push(schedule);
        self.look_at_schedules(cx);
        cx.notify();
        self.next_schedule
    }

    /// Drops schedule `id`; false when there is none.
    pub(super) fn schedule_cancel(&mut self, id: u64) -> bool {
        let before = self.schedules.len();
        self.schedules.retain(|s| s.id != id);
        self.schedules.len() < before
    }

    /// Keeps `watch`, its match, its timeout or its block's end typed back to its asker
    /// (`schedules.rs`); its id, counting from 1 (`w1`).
    pub(super) fn watch_keep(
        &mut self,
        mut watch: super::schedules::Watch,
        cx: &mut Context<Self>,
    ) -> u64 {
        self.next_watch += 1;
        watch.id = self.next_watch;
        self.watches.push(watch);
        self.look_at_watches(cx);
        self.next_watch
    }

    /// The worktree `name` that `spawn_agent` made in the repository of `folder`, against
    /// `folder`'s working tree (where it goes back), and that repository; why not.
    // ponytail: read on the UI thread (a few git commands); on the background executor if a big
    // repository makes it slow.
    pub(super) fn worktree_state(
        &self,
        folder: &Path,
        name: &str,
    ) -> Result<(templum_git::Repo, templum_git::WorktreeState), String> {
        let program = self.config.git.program();
        let repo = match templum_git::Repo::discover(program, folder) {
            Ok(Some(repo)) => repo,
            Ok(None) => return Err(format!("{} isn't in a Git repository.", folder.display())),
            Err(e) => return Err(format!("Couldn't run git: {}", e.first_line())),
        };
        match repo.worktree_state(name) {
            Ok(Some(state)) => Ok((repo, state)),
            Ok(None) => Err(format!(
                "There's no worktree {name} in this repository: git worktree list shows them."
            )),
            Err(e) => Err(format!("Couldn't read the worktree: {}", e.first_line())),
        }
    }

    /// Removes the worktree `name` of `repo` and its branch on the background executor; with
    /// `discard`, its changes and commits not merged are lost.
    pub(super) fn worktree_remove(
        &self,
        repo: templum_git::Repo,
        name: String,
        discard: bool,
        cx: &mut Context<Self>,
    ) -> Task<Reply> {
        cx.background_executor().spawn(async move {
            match repo.remove_worktree(&name, discard) {
                Ok(()) => Reply::ok(format!("Removed the worktree {name} and its branch.")),
                Err(e) => Reply::err(format!("Couldn't remove {name}: {}", e.first_line())),
            }
        })
    }

    /// Merges the worktree `name`'s `unmerged` commits into `into`, the branch of `repo`'s working
    /// tree, on the background executor; a conflict is aborted, naming its files.
    pub(super) fn worktree_merge(
        &self,
        repo: templum_git::Repo,
        name: String,
        into: String,
        unmerged: u32,
        cx: &mut Context<Self>,
    ) -> Task<Reply> {
        cx.background_executor().spawn(async move {
            let commits = match unmerged {
                1 => "1 commit".to_string(),
                n => format!("{n} commits"),
            };
            match repo.merge_from(&name) {
                Ok(Ok(())) => Reply::ok(format!(
                    "Merged {name} into {into} ({commits}). remove_worktree removes it."
                )),
                Ok(Err(files)) => Reply::err(format!(
                    "The merge would conflict in {}: merge {into} into {name} and resolve them \
                     there, then merge again.",
                    files.join(", ")
                )),
                Err(e) => Reply::err(format!("Couldn't merge {name}: {}", e.first_line())),
            }
        })
    }

    /// The workspace `name` names, by its id, then by its name in either case. A stopped one is
    /// refused unless `stopped` takes it (`show` starts it).
    pub(super) fn space_of(&self, name: &str, stopped: bool) -> Result<String, Reply> {
        let lower = name.trim().to_lowercase();
        let space = self
            .spaces()
            .find(|s| s.id == name.trim())
            .or_else(|| {
                self.spaces()
                    .find(|s| s.name.trim().to_lowercase() == lower)
            })
            .ok_or_else(|| {
                Reply::err(format!("There's no workspace {name}: overview lists them."))
            })?;
        if space.stopped && !stopped {
            return Err(Reply::err(format!(
                "Workspace {} is stopped: show it first (show).",
                clean_title(&space.name)
            )));
        }
        Ok(space.id.clone())
    }

    /// What a terminal shows, `read_terminal`'s answer: its last `lines`, or its last finished
    /// command and that command's output. `block` names it in the answer.
    pub(super) fn terminal_text(
        &self,
        (space, id): (&str, BlockId),
        block: &str,
        lines: usize,
        last_command: bool,
    ) -> Reply {
        let Some(pane) = self.space_named(space).and_then(|s| s.terminal(id)) else {
            return Reply::err(format!("{block} isn't a terminal."));
        };
        let shared = pane.term.shared();
        if last_command {
            match shared.last_command() {
                Some(text) => Reply::ok(tools::tail(&text, tools::MAX_TEXT)),
                None => Reply::err(format!(
                    "No command has finished in {block} yet (it needs the shell's integration)."
                )),
            }
        } else {
            Reply::ok(tools::tail(
                &shared.lines(lines).join("\n"),
                tools::MAX_TEXT,
            ))
        }
    }

    /// Types `text` into a terminal, as a paste; Enter after it when `enter`, answered once it
    /// went: a caller that waits for each answer gets its texts and Enters in order. `asker`,
    /// the terminal that calls (`ASKER` for the Templum Agent), answers a dialog of an agent that
    /// went quiet on its task by it (`asker_wrote`).
    pub(super) fn type_text(
        &mut self,
        asker: &str,
        (space, id): (&str, BlockId),
        block: &str,
        text: &str,
        enter: bool,
        cx: &mut Context<Self>,
    ) -> Outcome {
        let Some(pane) = self.space_named(space).and_then(|s| s.terminal(id)) else {
            return Outcome::Now(Reply::err(format!("{block} isn't a terminal.")));
        };
        if let Err(refused) = pane.read_only(block) {
            return Outcome::Now(refused);
        }
        let worker = pane.term.id().to_string();
        self.asker_wrote(&worker, asker);
        if !enter {
            if let Some(pane) = self.pane(&worker) {
                pane.term.type_in(text);
            }
            return Outcome::Now(Reply::ok(format!("Typed into {block}.")));
        }
        let entered = self.type_then_enter(worker, text, cx);
        let block = block.to_string();
        Outcome::Later(cx.spawn(async move |_, _| {
            if entered.await {
                Reply::ok(format!("Typed into {block}."))
            } else {
                Reply::err(format!("{block} was closed before its Enter."))
            }
        }))
    }

    /// Closes a block, also in a workspace not shown, as one step of Ctrl+Z. The task `asker`
    /// handed its agent ends untold: the asker knows.
    pub(super) fn block_close(
        &mut self,
        (space, id): (&str, BlockId),
        block: &str,
        asker: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        let terminal = self
            .space_named(space)
            .and_then(|s| s.terminal(id))
            .map(|p| p.term.id().to_string());
        if let Some(pane) = terminal.as_deref().and_then(|t| self.pane_mut(t))
            && pane.handed.as_ref().is_some_and(|h| h.by == asker)
        {
            pane.handed = None;
        }
        self.in_space(space, |ws| {
            ws.close_now(&[id], Then::New, window, cx);
            // Its shell and agent end now: a ghost waits for a frame of its workspace, which one
            // not shown, or a window not drawn, doesn't have.
            ws.space.ghosts.retain(|g| g.block.id != id);
        });
        cx.notify();
        Reply::ok(format!(
            "Closed {block}. The person can bring it back with Ctrl+Z."
        ))
    }

    /// What closing a block would lose, as the person's own close asks: a program at work in a
    /// terminal, unsaved edits in an editor. None when nothing would be.
    pub(super) fn would_lose(&self, (space, id): (&str, BlockId)) -> Option<String> {
        let space = self.space_named(space)?;
        if let Some(pane) = space.terminal(id) {
            // A shell that ended has nothing left to lose.
            if pane.term.exited() {
                return None;
            }
            return crate::alert::busy(&pane.term.status(), self.agent_name(pane));
        }
        let unsaved = space.unsaved(Some(id));
        (!unsaved.is_empty()).then(|| format!("Unsaved edits: {}.", unsaved.join(", ")))
    }

    /// The editor of the note a note block shows.
    pub(super) fn note_editor(
        &self,
        (space, id): (&str, BlockId),
        block: &str,
    ) -> Result<Entity<EditorState>, Reply> {
        let Some(note) = self
            .space_named(space)
            .and_then(|s| s.blocks.get(&id))
            .and_then(|b| b.note())
        else {
            return Err(Reply::err(format!("{block} isn't a note.")));
        };
        note.tab()
            .map(|t| t.editor.clone())
            .ok_or_else(|| Reply::err(format!("{block} shows no note.")))
    }

    /// The text of the note a note block shows, `read_note`'s answer.
    pub(super) fn note_text(&self, at: (&str, BlockId), block: &str, cx: &App) -> Reply {
        match self.note_editor(at, block) {
            Ok(editor) => Reply::ok(tools::tail(&editor.read(cx).value(), tools::MAX_TEXT)),
            Err(refused) => refused,
        }
    }

    /// Any board, by its id or its name in either case.
    pub(super) fn board_named(&self, name: &str) -> Result<u64, Reply> {
        let lower = name.trim().to_lowercase();
        let boards = &self.boards.boards;
        boards
            .iter()
            .find(|b| b.id.to_string() == name.trim())
            .or_else(|| {
                boards
                    .iter()
                    .find(|b| b.name.trim().to_lowercase() == lower)
            })
            .map(|b| b.id)
            .ok_or_else(|| {
                let names: Vec<String> = boards
                    .iter()
                    .map(|b| format!("\"{}\" ({})", flat(&b.name), b.id))
                    .collect();
                Reply::err(format!(
                    "There's no board {name}. Boards: {}.",
                    if names.is_empty() {
                        "none".to_string()
                    } else {
                        names.join(", ")
                    }
                ))
            })
    }

    pub(super) fn no_column(&self, board: u64, column: &str) -> Reply {
        let titles: Vec<String> = self
            .boards
            .board(board)
            .map(|b| b.columns.iter().map(|c| flat(&c.title)).collect())
            .unwrap_or_default();
        Reply::err(format!(
            "There's no column {column}. Its columns: {}.",
            titles.join(", ")
        ))
    }

    /// `list_cards`' answer: the board's columns and cards, or one card whole. `viewer` is the
    /// block that asks, whose cards say "(you)"; None for the Templum Agent.
    pub(super) fn cards_of(
        &self,
        board: u64,
        card: Option<u64>,
        viewer: Option<&Caller>,
        cx: &App,
    ) -> Reply {
        if let Some(card) = card {
            return Reply::ok(tools::tail(
                &self.card_whole(board, card, viewer, cx),
                tools::MAX_TEXT,
            ));
        }
        let Some(b) = self.boards.board(board) else {
            return Reply::err("That board was deleted.");
        };
        let mut text = format!("Board \"{}\" ({})", flat(&b.name), b.id);
        for column in &b.columns {
            text.push_str(&format!("\n{} ({}):", flat(&column.title), column.id));
            if column.cards.is_empty() {
                text.push_str(" no cards");
            }
            for c in &column.cards {
                text.push_str(&format!("\n  {}", self.card_line(c, viewer, cx)));
            }
        }
        Reply::ok(tools::tail(&text, tools::MAX_TEXT))
    }

    /// `5  Fix login  [red, green]  priority high  assignee b7k2m "Codex"  checklist 2/5  2 comments`
    fn card_line(&self, c: &Card, viewer: Option<&Caller>, cx: &App) -> String {
        let mut line = format!("{}  {}", c.id, flat(&c.title));
        if !c.labels.is_empty() {
            let names: Vec<&str> = c.labels.iter().map(|l| l.name()).collect();
            line.push_str(&format!("  [{}]", names.join(", ")));
        }
        if let Some(p) = c.priority {
            line.push_str(&format!("  priority {}", p.name()));
        }
        match &c.assignee {
            Some(Assignee::Block(uid)) => {
                line.push_str(&format!("  assignee {}", self.who(uid, cx)))
            }
            Some(Assignee::Agent(id)) => line.push_str(&format!("  for any {id}")),
            None => {}
        }
        if let (Some(a), Some(viewer)) = (&c.assignee, viewer)
            && self.is_callers(a, viewer)
        {
            line.push_str(" (you)");
        }
        if !c.checklist.is_empty() {
            let (done, all) = c.progress();
            line.push_str(&format!("  checklist {done}/{all}"));
        }
        match c.comments.len() {
            0 => {}
            1 => line.push_str("  1 comment"),
            n => line.push_str(&format!("  {n} comments")),
        }
        match c.attachments.len() {
            0 => {}
            1 => line.push_str("  1 attachment"),
            n => line.push_str(&format!("  {n} attachments")),
        }
        line
    }

    fn card_whole(&self, board: u64, card: u64, viewer: Option<&Caller>, cx: &App) -> String {
        let (Some(b), Some(c)) = (self.boards.board(board), self.boards.card(board, card)) else {
            return String::new();
        };
        let column = b
            .columns
            .iter()
            .find(|col| col.cards.iter().any(|x| x.id == card));
        let indent = |text: &str| {
            text.lines()
                .map(|l| format!("  {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut text = format!(
            "Card {} on board \"{}\" ({}), column \"{}\"\n{}",
            c.id,
            flat(&b.name),
            b.id,
            column.map(|col| flat(&col.title)).unwrap_or_default(),
            self.card_line(c, viewer, cx)
        );
        if !c.description.trim().is_empty() {
            text.push_str(&format!("\ndescription:\n{}", indent(&c.description)));
        }
        if !c.checklist.is_empty() {
            text.push_str("\nchecklist:");
            for item in &c.checklist {
                let mark = if item.done { 'x' } else { ' ' };
                text.push_str(&format!("\n  {} [{mark}] {}", item.id, flat(&item.text)));
            }
        }
        if let (Some(data), false) = (&self.data_dir, c.attachments.is_empty()) {
            text.push_str("\nattachments:");
            for a in &c.attachments {
                let path = attachments::dir(data, c.id).join(&a.name);
                let size = attachments::size_text(a.size);
                text.push_str(&format!(
                    "\n  {}  {size}  {}",
                    flat(&a.name),
                    path.display()
                ));
            }
        }
        let now = notes::now();
        for comment in &c.comments {
            text.push_str(&format!(
                "\ncomment {} by {}, {}{}:\n{}",
                comment.id,
                self.who(&comment.by, cx),
                notes::ago(comment.at, now),
                if comment.edited { ", edited" } else { "" },
                indent(&comment.text)
            ));
        }
        text
    }

    /// Adds the card `new` describes to `board`: its id.
    pub(super) fn card_new(
        &mut self,
        board: u64,
        new: tools::NewCard,
        cx: &mut Context<Self>,
    ) -> Result<u64, Reply> {
        if new.title.trim().is_empty() {
            return Err(Reply::err("A card needs a title."));
        }
        let assignee = match new.who.as_deref().map(|w| self.resolve_for(w)) {
            Some(Ok(who)) => who,
            Some(Err(refused)) => return Err(refused),
            None => None,
        };
        let priority = new.priority.as_deref().and_then(Priority::named);
        let items = new
            .checklist
            .as_deref()
            .map(checklists::parse_items)
            .unwrap_or_default();
        let column = match new.column.as_deref() {
            Some(name) => match self.boards.column_named(board, name) {
                Some(c) => c,
                None => return Err(self.no_column(board, name)),
            },
            None => match self.boards.board(board).and_then(|b| b.columns.first()) {
                Some(c) => c.id,
                None => return Err(Reply::err("That board has no columns.")),
            },
        };
        let title = new.title.trim().to_string();
        let id = self.edit_boards(|b| {
            let id = b.add_card(board, column)?;
            b.edit_card(board, id, |c| {
                c.title = title;
                c.description = new.description.unwrap_or_default();
                c.priority = priority;
                c.assignee = assignee;
            });
            b.add_items(board, id, &items);
            Some(id)
        });
        cx.notify();
        id.ok_or_else(|| Reply::err("That column is gone."))
    }

    /// The board holding `card`, in any workspace.
    fn board_of(&self, card: u64) -> Result<u64, Reply> {
        self.boards
            .find_card(card)
            .map(|(board, _)| board)
            .ok_or_else(|| Reply::err(format!("There's no card {card}: list_cards lists them.")))
    }

    /// Sets a card's priority and who it is for, adds items, checks and unchecks them: every
    /// part is checked first, then all apply as one Ctrl+Z step. `caller`, a block, may only
    /// take or hand on a card as `claim_card` lets it; the Templum Agent (None) may.
    pub(super) fn card_edit(
        &mut self,
        edit: tools::CardEdit,
        caller: Option<&Caller>,
        cx: &mut Context<Self>,
    ) -> Reply {
        let card = edit.card;
        let board = match self.board_of(card) {
            Ok(b) => b,
            Err(refused) => return refused,
        };
        let assignee = match edit.who.as_deref().map(|w| self.resolve_for(w)) {
            Some(Ok(who)) => Some(who),
            Some(Err(refused)) => return refused,
            None => None,
        };
        // Its `for` follows claim_card: never a card another block holds freed or handed on,
        // nor one for another agent given to the caller.
        if let (Some(who), Some(caller)) = (&assignee, caller)
            && let Some(c) = self.boards.card(board, card)
        {
            let held = matches!(&c.assignee, Some(Assignee::Block(uid)) if *uid != caller.0);
            let names_caller = who.as_ref().is_some_and(|w| self.is_callers(w, caller));
            if (held || names_caller)
                && let Err(refused) = self.may_take(c, caller, cx)
            {
                return refused;
            }
        }
        let has = |item: &u64| {
            self.boards
                .card(board, card)
                .is_some_and(|c| c.checklist.iter().any(|i| i.id == *item))
        };
        if let Some(item) = edit.check.iter().chain(&edit.uncheck).find(|i| !has(i)) {
            return Reply::err(format!(
                "Card {card} has no item {item}: list_cards with card shows its items."
            ));
        }
        let priority = edit.priority.as_deref().map(Priority::named);
        let items = edit
            .add_items
            .as_deref()
            .map(checklists::parse_items)
            .unwrap_or_default();
        let added = self.edit_boards(|b| {
            b.edit_card(board, card, |c| {
                if let Some(p) = priority {
                    c.priority = p;
                }
                if let Some(who) = assignee.clone() {
                    c.assignee = who;
                }
            });
            for (ids, done) in [(&edit.check, true), (&edit.uncheck, false)] {
                for item in ids {
                    b.set_item_done(board, card, *item, done);
                }
            }
            b.add_items(board, card, &items)
        });
        cx.notify();
        let list = |ids: &[u64]| {
            ids.iter()
                .map(u64::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut said = Vec::new();
        if let Some(p) = priority {
            said.push(format!("priority {}", p.map_or("none", |p| p.name())));
        }
        match &assignee {
            Some(None) => said.push("for anyone".into()),
            Some(Some(Assignee::Agent(id))) => said.push(format!("for any {id}")),
            Some(Some(Assignee::Block(uid))) => said.push(format!("for {}", self.who(uid, cx))),
            None => {}
        }
        match added.as_slice() {
            [] => {}
            [one] => said.push(format!("added item {one}")),
            more => said.push(format!("added items {}", list(more))),
        }
        if !edit.check.is_empty() {
            said.push(format!("checked {}", list(&edit.check)));
        }
        if !edit.uncheck.is_empty() {
            said.push(format!("unchecked {}", list(&edit.uncheck)));
        }
        Reply::ok(format!("Card {card}: {}.", said.join("; ")))
    }

    /// Moves a card to the end of a column of its board.
    pub(super) fn card_move(&mut self, card: u64, column: &str, cx: &mut Context<Self>) -> Reply {
        let board = match self.board_of(card) {
            Ok(b) => b,
            Err(refused) => return refused,
        };
        let Some(to) = self.boards.column_named(board, column) else {
            return self.no_column(board, column);
        };
        self.edit_boards(|b| b.move_card(board, card, to, None));
        cx.notify();
        Reply::ok(format!("Moved card {card} to {column}."))
    }

    /// Adds a comment to a card, by `by`: a block's id, or the Templum Agent's word.
    pub(super) fn card_comment(
        &mut self,
        card: u64,
        text: &str,
        by: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        let board = match self.board_of(card) {
            Ok(b) => b,
            Err(refused) => return refused,
        };
        if text.trim().is_empty() {
            return Reply::err("A comment needs text.");
        }
        let said =
            self.edit_boards(|b| b.add_comment(board, card, by, text.trim_end(), notes::now()));
        cx.notify();
        match said {
            Some(id) => Reply::ok(format!("Commented on card {card} (comment {id}).")),
            None => Reply::err(format!("Card {card} is gone.")),
        }
    }

    /// Adds `text` at the end of the note a note block shows, after a blank line, as one step of
    /// the note's own Ctrl+Z; the person's caret and selection stay where they were.
    pub(super) fn note_append(
        &mut self,
        at: (&str, BlockId),
        block: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        let editor = match self.note_editor(at, block) {
            Ok(editor) => editor,
            Err(refused) => return refused,
        };
        if text.trim().is_empty() {
            return Reply::err("There's nothing to add.");
        }
        editor.update(cx, |e, cx| {
            let value = e.value();
            let sep = if value.is_empty() || value.ends_with("\n\n") {
                ""
            } else if value.ends_with('\n') {
                "\n"
            } else {
                "\n\n"
            };
            let (kept, end) = (e.selected_range(), value.len());
            e.set_selected_range(end..end, cx);
            e.insert(format!("{sep}{}", text.trim_end()), window, cx);
            e.set_selected_range(kept, cx);
        });
        Reply::ok(format!("Added to the note in {block}."))
    }

    /// The checklist a checklist block shows.
    pub(super) fn list_of(&self, (space, id): (&str, BlockId), block: &str) -> Result<u64, Reply> {
        let Some(checklist) = self
            .space_named(space)
            .and_then(|s| s.blocks.get(&id))
            .and_then(|b| b.checklist())
        else {
            return Err(Reply::err(format!("{block} isn't a checklist.")));
        };
        checklist
            .list
            .filter(|list| self.checklists.get(*list).is_some())
            .ok_or_else(|| Reply::err(format!("{block} shows no checklist.")))
    }

    /// The checklist's name and progress, then a line per item: its id, [x] or [ ], its text.
    pub(super) fn checklist_text(&self, at: (&str, BlockId), block: &str) -> Reply {
        let list = match self.list_of(at, block) {
            Ok(list) => list,
            Err(refused) => return refused,
        };
        let Some(list) = self.checklists.get(list) else {
            return Reply::err(format!("{block} shows no checklist."));
        };
        let (done, all) = list.progress();
        let mut text = format!("{} ({done}/{all})", list.name_shown());
        for item in &list.items {
            let mark = if item.done { 'x' } else { ' ' };
            text.push_str(&format!("\n{} [{mark}] {}", item.id, item.text));
        }
        Reply::ok(tools::tail(&text, tools::MAX_TEXT))
    }

    /// "There's no item 99 in "Plan"…": the checklist `list` has no item `item`.
    pub(super) fn no_item(&self, list: u64, item: u64) -> Reply {
        let name = self.checklists.get(list).map_or("", |l| l.name_shown());
        Reply::err(format!(
            "There's no item {item} in \"{name}\". read_checklist shows its items."
        ))
    }

    /// Checks (or unchecks) an item of the checklist a block shows: one Ctrl+Z step.
    pub(super) fn item_check(
        &mut self,
        at: (&str, BlockId),
        block: &str,
        item: u64,
        done: bool,
        cx: &mut Context<Self>,
    ) -> Reply {
        let list = match self.list_of(at, block) {
            Ok(list) => list,
            Err(refused) => return refused,
        };
        if !self.edit_checklist(list, |c| c.set_done(list, item, done, notes::now())) {
            return self.no_item(list, item);
        }
        self.agent_changed(list);
        cx.notify();
        let state = if done { "checked" } else { "unchecked" };
        Reply::ok(format!("Item {item} is {state}."))
    }

    /// Adds an item per line of `text` at the end of the checklist a block shows: one Ctrl+Z
    /// step.
    pub(super) fn items_add(
        &mut self,
        at: (&str, BlockId),
        block: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        let list = match self.list_of(at, block) {
            Ok(list) => list,
            Err(refused) => return refused,
        };
        let items = checklists::parse_items(text);
        if items.is_empty() {
            return Reply::err("There's nothing to add.");
        }
        let now = notes::now();
        let ids: Vec<String> = self.edit_checklist(list, |c| {
            items
                .iter()
                .filter_map(|(text, done)| c.add_item(list, None, text, *done, now))
                .map(|id| id.to_string())
                .collect()
        });
        self.agent_changed(list);
        cx.notify();
        match ids.as_slice() {
            [one] => Reply::ok(format!("Added item {one}.")),
            many => Reply::ok(format!("Added items {}.", many.join(", "))),
        }
    }
}

impl Space {
    /// Where a block of `size` goes at `place`: beside its block, toward the blocks wired to it,
    /// or centered on `center` (the view's middle); never over a block. None when the block of
    /// `Near` isn't on this canvas.
    pub(super) fn spot(
        &self,
        place: &Place,
        size: Size,
        center: templum_canvas::Point,
    ) -> Option<Rect> {
        match place {
            Place::Near(id) => self.room_near(*id, size),
            Place::Free => Some(self.room_around(center, size)),
        }
    }
}
