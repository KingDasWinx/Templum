//! Blocks: adding, focusing and closing them, and the news from their terminals.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::component::Icon;
use gpui_kit::component::notification::Notification;
use gpui_kit::{App, Context, SharedString, Window};
use templum_canvas::{Block, BlockId, Point, Rect, chrome};
use templum_terminal::{ShellOption, SpawnOptions, Terminal};

use crate::agents::AgentEntry;
use crate::alert::{self, Alert};
use crate::board::{Boards, Comment, Draft};
use crate::config::WorkingDirectory;
use crate::kanban::{self, Kanban};
use crate::note::Note;
use crate::prompt;
use templum_mcp::control::Kind;

use super::history::{Change, Then};
use super::remote::Deed;
use super::{
    AGENT_COLS, AGENT_ROWS, BOARD_UNDO, Body, DEFAULT_COLS, DEFAULT_ROWS, Ghost, Pane, Space,
    Workspace, remember, show_notification,
};
use crate::notes;

/// What the person does to a kanban's boards: on this computer an edit of `Workspace::boards`
/// (one Ctrl+Z step each), on a mirrored workspace a call of home's.
pub(crate) enum BoardAct {
    /// `card` into `column`, in the place of card `to` (the end without one), as a drop puts it.
    MoveCard {
        board: u64,
        card: u64,
        column: u64,
        to: Option<u64>,
    },
    /// `column` into the place of column `to`, as a drop puts it.
    MoveColumn {
        board: u64,
        column: u64,
        to: u64,
    },
    AddColumn {
        board: u64,
    },
    RenameColumn {
        board: u64,
        column: u64,
        title: String,
    },
    DeleteColumn {
        board: u64,
        column: u64,
    },
    RenameBoard {
        board: u64,
        name: String,
    },
    DeleteBoard {
        board: u64,
    },
    /// Kanban `block` shows `board`, or a new board (None).
    Show {
        block: BlockId,
        board: Option<u64>,
    },
    DeleteCard {
        board: u64,
        card: u64,
    },
    /// Done on a card's dialog.
    Finish {
        board: u64,
        draft: Draft,
    },
    Comment {
        board: u64,
        card: u64,
        text: String,
    },
    EditComment {
        board: u64,
        card: u64,
        comment: u64,
        text: String,
    },
    DeleteComment {
        board: u64,
        card: u64,
        comment: u64,
    },
    /// This computer only: a deleted comment's Undo.
    RestoreComment {
        board: u64,
        card: u64,
        index: usize,
        comment: Comment,
    },
    /// This computer only: Save draft.
    KeepDraft(Draft),
    /// This computer only: Discard.
    DropDraft(u64),
}

impl Workspace {
    /// The one door of a kanban's writes. `space` is the workspace the person acted on: for a
    /// local one the edit each site made before, for a mirrored one nothing yet.
    pub(crate) fn act_on_boards(
        &mut self,
        space: &str,
        act: BoardAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A Boards dialog may outlive the workspace it was opened on: block and board ids mean
        // something else in another one.
        if matches!(act, BoardAct::Show { .. } | BoardAct::DeleteBoard { .. })
            && !self.still_shown(space, window, cx)
        {
            return;
        }
        match self.space_named(space) {
            Some(s) if s.is_remote() => return self.remote_board_act(space, act, window, cx),
            Some(_) => {}
            None => return,
        }
        match act {
            BoardAct::MoveCard {
                board,
                card,
                column,
                to,
            } => self.edit_boards(|b| b.move_card(board, card, column, to)),
            BoardAct::MoveColumn { board, column, to } => {
                self.edit_boards(|b| b.move_column(board, column, to))
            }
            BoardAct::AddColumn { board } => {
                self.edit_boards(|b| b.add_column(board));
            }
            BoardAct::RenameColumn {
                board,
                column,
                title,
            } => self.edit_boards(|b| b.rename_column(board, column, &title)),
            BoardAct::DeleteColumn { board, column } => {
                self.edit_boards(|b| b.remove_column(board, column))
            }
            BoardAct::RenameBoard { board, name } => {
                self.edit_boards(|b| b.rename_board(board, &name))
            }
            BoardAct::DeleteBoard { board } => self.delete_board(board, cx),
            BoardAct::Show {
                block,
                board: Some(board),
            } => self.show_board(block, board, cx),
            BoardAct::Show { block, board: None } => self.new_board(block, cx),
            BoardAct::DeleteCard { board, card } => {
                self.edit_boards(|b| b.remove_card(board, card))
            }
            BoardAct::Finish { board, draft } => {
                self.edit_boards(|b| b.finish_draft(board, &draft));
            }
            BoardAct::Comment { board, card, text } => {
                self.edit_boards(|b| b.add_comment(board, card, "you", &text, notes::now()));
            }
            BoardAct::EditComment {
                board,
                card,
                comment,
                text,
            } => self.edit_boards(|b| b.edit_comment(board, card, comment, &text)),
            BoardAct::DeleteComment {
                board,
                card,
                comment,
            } => self.edit_boards(|b| b.remove_comment(board, card, comment)),
            BoardAct::RestoreComment {
                board,
                card,
                index,
                comment,
            } => self.edit_boards(|b| {
                b.edit_card(board, card, |c| {
                    c.restore_comment(index, comment);
                })
            }),
            BoardAct::KeepDraft(draft) => self.boards.keep_draft(draft),
            BoardAct::DropDraft(at) => self.boards.drop_draft(at),
        }
        cx.notify();
    }

    /// An id for something a dialog makes before home hears of it (an item, a comment, a card's
    /// first attachment): the boards' next one for a local workspace; a mirror's own count, which
    /// never meets home's.
    pub(crate) fn fresh_id(&mut self, space: &str) -> u64 {
        match self.remote_mut(space) {
            Some(remote) => remote.fresh_id(),
            None => self.boards.next(),
        }
    }

    /// Adds a block on top, focused, popping in.
    pub(super) fn add_block(&mut self, rect: Rect, body: Body) -> BlockId {
        let id = self.space.canvas.add(rect);
        self.space.blocks.insert(id, body);
        self.added(id);
        id
    }

    /// A block was just added: it is focused, pops in, and Ctrl+Z takes it away (unless the
    /// canvas is being restored or blocks come back by an undo: `quiet`). It gets a new id.
    fn added(&mut self, id: BlockId) {
        let uid = self.new_uid();
        self.space.uids.insert(id, uid);
        let before = self.space.focused;
        self.space.focused = Some(id);
        self.space.selected.clear();
        self.space.selected_wire = None;
        self.space.appearing.insert(id, Instant::now());
        if !self.quiet {
            self.keep(Change::Close(vec![id]), Then::New);
        }
        // In grid mode it takes half of the focused tile; its place on the canvas, for when the
        // grid ends, is off the others' places there.
        if self.space.grid.is_some() {
            let taken: Vec<Rect> = self
                .space
                .canvas
                .blocks()
                .iter()
                .filter(|b| b.id != id)
                .map(|b| self.space.home(b))
                .collect();
            if let Some(b) = self.space.canvas.get_mut(id)
                && taken.iter().any(|t| t.intersects(&b.rect))
            {
                b.rect = templum_canvas::room_at(&taken, b.rect);
            }
            self.join_grid(id, before);
        }
    }

    /// A block id no block of any workspace has: five letters and digits.
    // ponytail: a stopped workspace's saved ids aren't checked (1 in 60 million per pair); check
    // `Space::saved` too if one ever collides.
    fn new_uid(&self) -> String {
        loop {
            let uid = base36(uuid::Uuid::new_v4().as_u128() % 36u128.pow(5), 5);
            if !self.uid_taken(&uid) {
                return uid;
            }
        }
    }

    pub(super) fn uid_taken(&self, uid: &str) -> bool {
        self.spaces().any(|s| s.uids.values().any(|u| u == uid))
    }

    /// Block `id`, just restored, takes back its saved id `uid`; one that isn't five lowercase
    /// letters and digits, or is taken (a file edited by hand), leaves it the new one it got.
    pub(super) fn adopt_uid(&mut self, id: BlockId, uid: &str) {
        let valid = uid.len() == 5
            && uid
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
        if valid && !self.uid_taken(uid) {
            self.space.uids.insert(id, uid.to_string());
        }
    }

    /// The terminals running an agent wired to a kanban block that shows `board`, as (block id,
    /// label), for a card's assignee menu: the active workspace first, then the others in the
    /// sidebar's order, each back to front. The label is its title as agents see it, its folder's
    /// name and its id ("Claude Code · app · k3x9a"), with " — {workspace}" outside the active
    /// one.
    pub(crate) fn agent_sessions(&self, board: u64, cx: &App) -> Vec<(String, String)> {
        self.spaces()
            .flat_map(|space| {
                let active = std::ptr::eq(space, &self.space);
                let kanbans: Vec<BlockId> = space
                    .canvas
                    .blocks()
                    .iter()
                    .filter(|b| space.kanban(b.id).is_some_and(|k| k.board == Some(board)))
                    .map(|b| b.id)
                    .collect();
                space.canvas.blocks().iter().filter_map(move |b| {
                    space.terminal(b.id)?.agent()?;
                    if !kanbans.iter().any(|&k| space.wired(b.id, k)) {
                        return None;
                    }
                    let uid = space.uids.get(&b.id)?.clone();
                    let (_, title, folder) = self.about(&space.blocks[&b.id], cx);
                    let mut label = title;
                    if let Some(name) = folder.as_deref().and_then(Path::file_name) {
                        label = format!("{label} · {}", name.to_string_lossy());
                    }
                    label = format!("{label} · {uid}");
                    if !active {
                        label = format!("{label} — {}", space.name);
                    }
                    Some((uid, label))
                })
            })
            .collect()
    }

    /// Changes the boards with `f`, keeping them as they were for Ctrl+Z (when it changed
    /// them).
    pub(crate) fn edit_boards<R>(&mut self, f: impl FnOnce(&mut Boards) -> R) -> R {
        let before = self.boards.clone();
        let result = f(&mut self.boards);
        self.boards.prune_drafts();
        if self.boards != before {
            self.boards_undo.push(before);
            if self.boards_undo.len() > BOARD_UNDO {
                self.boards_undo.remove(0);
            }
        }
        result
    }

    /// Ctrl+Z on a kanban block: the boards as before their last change. False: nothing to undo.
    pub(super) fn undo_boards(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(mut before) = self.boards_undo.pop() else {
            return false;
        };
        // Ids are never handed out twice: a block or a file may still name an undone board.
        before.next_id = before.next_id.max(self.boards.next_id);
        // Drafts aren't board edits: the ones kept since stay.
        before.drafts = std::mem::take(&mut self.boards.drafts);
        self.boards = before;
        cx.notify();
        true
    }

    /// A terminal in `folder`, placed to the right of `beside` (None: at the viewport's center),
    /// with `command` typed in (not run: Enter is the person's).
    pub(crate) fn terminal_beside(
        &mut self,
        beside: Option<BlockId>,
        folder: &Path,
        command: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        self.space.pristine = false;
        let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
        let rect = match beside.and_then(|b| self.space.canvas.get(b)) {
            Some(b) => self.space.room_beside(&[self.space.home(b)], size),
            None => self.room_in_view(size, window, cx),
        };
        let id = self.spawn_block(rect, Some(folder.to_path_buf()), window, cx);
        if let (Some(id), Some(command)) = (id, command) {
            // Typed, not run: the person reads it and presses Enter (it may delete or push).
            // The shell reads it once it is ready.
            if let Some(pane) = self.space.terminal(id) {
                pane.term.write(command.to_string().into_bytes());
            }
        }
        window.focus(&self.focus_handle, cx);
        id
    }

    /// Opens the inbox when a request waits in this workspace. Else shows the terminal waiting
    /// for the person (it asked for attention): this workspace's first, then the others' in the
    /// sidebar's order, the oldest block first. Going there is seeing it, so the next time shows
    /// the next one. None waiting: a notification says so.
    pub(crate) fn go_to_waiting(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // A request waits here: the inbox has it.
        if !self.inbox().is_empty() {
            self.open_inbox(None, window, cx);
            return;
        }
        let order = self.order();
        let spaces =
            std::iter::once(&self.space).chain(order.into_iter().filter(|s| s.id != self.space.id));
        let waiting = spaces
            .filter_map(|space| {
                space
                    .terminals()
                    .filter(|(_, pane)| matches!(pane.alert, Some(Alert::Attention(_))))
                    .map(|(id, _)| id)
                    .min_by_key(|id| id.0)
                    .map(|id| (space.id.clone(), id))
            })
            .next();
        let Some((space, id)) = waiting else {
            show_notification(
                window,
                cx,
                Notification::info("No terminal is waiting for you".to_string()),
            );
            return;
        };
        if let Some(pane) = self
            .space_by_id_mut(&space)
            .and_then(|s| s.terminal_mut(id))
        {
            pane.alert = None;
        }
        self.reveal(&space, id, window, cx);
    }

    /// The terminals of this workspace, labeled, those running an agent first: every one, or
    /// those whose folder is inside `root`.
    pub(crate) fn terminal_list(&self, root: Option<&Path>) -> Vec<(BlockId, String)> {
        let mut found: Vec<_> = self
            .space
            .terminals()
            .filter(|(_, pane)| root.is_none_or(|root| pane.works_in(root)))
            .map(|(id, pane)| {
                let agent = self.agent_name(pane);
                let label = label(agent, &pane.term.title(), pane.seen.folder.as_deref());
                (agent.is_none(), id, label)
            })
            .collect();
        found.sort_by_key(|(no_agent, id, _)| (*no_agent, id.0));
        found
            .into_iter()
            .map(|(_, id, label)| (id, label))
            .collect()
    }

    /// The agent running in a terminal, by its title (its id when Templum doesn't know it).
    pub(crate) fn agent_name<'a>(&'a self, pane: &'a Pane) -> Option<&'a str> {
        let agent = pane.seen.agent.as_ref()?;
        Some(
            self.agents
                .iter()
                .find(|a| a.id == *agent)
                .map_or(agent.as_str(), |a| a.title.as_str()),
        )
    }

    /// Types `text` into the terminal `to` for the person to send (never Enter), then shows it
    /// with the keyboard.
    pub(crate) fn send_to(
        &mut self,
        to: BlockId,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.on_remote() {
            self.remote_send_to(to, text, window, cx);
            return;
        }
        let Some(pane) = self.space.terminal(to) else {
            return;
        };
        pane.person_wrote();
        pane.term.type_in(text);
        let space = self.space.id.clone();
        self.reveal(&space, to, window, cx);
    }

    /// A block's editor is about to leave the frame: if it has the keyboard, the canvas takes it.
    /// GPUI sends keys to the window's root when the focused element isn't drawn, and the
    /// shortcuts would stop working until a click.
    pub(super) fn release_focus(
        &mut self,
        id: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .space
            .blocks
            .get(&id)
            .is_some_and(|b| b.has_focus(window, cx))
        {
            window.focus(&self.focus_handle, cx);
        }
    }

    /// A kanban block at the viewport's center, showing no board.
    pub(crate) fn new_kanban(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.room_in_view(kanban::SIZE, window, cx).origin();
        self.add_kanban_at(at, window, cx);
    }

    /// A kanban block showing no board, its top-left corner at `at` (world units).
    pub(crate) fn add_kanban_at(&mut self, at: Point, window: &mut Window, cx: &mut Context<Self>) {
        if self.why_not(Deed::AddKanban).is_some() {
            return;
        }
        if self.on_remote() {
            return self.remote_add(Kind::Kanban, at, cx);
        }
        self.space.pristine = false;
        let rect = Rect::new(at.x, at.y, kanban::SIZE.w, kanban::SIZE.h);
        self.add_block(rect, Body::Kanban(Kanban::default()));
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// A terminal running `shell` (None: the default) in the start folder, its top-left corner at
    /// `at` (world units) with the default size, else where a new terminal goes. The block keeps
    /// the shell (docs/specs/shells.md, The block keeps its shell).
    pub(crate) fn open_shell(
        &mut self,
        shell: Option<ShellOption>,
        at: Option<Point>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.why_not(Deed::AddTerminal).is_some() {
            return;
        }
        if self.on_remote() {
            // Home's default shell: a mirror's menus choose home's others (`open_home_shell`).
            return self.open_home_shell(None, at, window, cx);
        }
        let rect = match at {
            Some(at) => {
                self.space.pristine = false;
                let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
                Rect::new(at.x, at.y, size.w, size.h)
            }
            None => self.new_terminal_rect(window, cx),
        };
        let folder = self.start_dir(window, cx);
        self.spawn_block_with(rect, folder, Vec::new(), shell, window, cx);
    }

    /// What **Default** names in the shells' menus, the shell Ctrl+Shift+T opens: with a program
    /// set in Settings, the label of the shell found with that program and those arguments, else
    /// the program's file name (a path may hold spaces: the stem, not `program_name`); without
    /// one, the system's default.
    pub(crate) fn default_shell_name(&self) -> String {
        let shell = &self.config.shell;
        let Some(program) = &shell.program else {
            return self.default_shell.clone();
        };
        self.shells
            .iter()
            .find(|s| s.program == *program && s.args == shell.args)
            .map(|s| s.label.clone())
            .unwrap_or_else(|| {
                Path::new(program)
                    .file_stem()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
            })
    }

    /// A terminal in `folder`, its top-left corner at `at`, with the agent `agent`'s command (an
    /// entry's id; one Templum doesn't know is typed as it is) typed into its shell; the folder
    /// becomes this workspace's newest recent one.
    pub(crate) fn launch_agent(
        &mut self,
        agent: &str,
        folder: &std::path::Path,
        at: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        if !folder.is_dir() {
            let message = format!("{} isn't a folder", folder.display());
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::warning(message));
            return None;
        }
        self.space.pristine = false;
        let size = chrome::block_size(AGENT_COLS, AGENT_ROWS, self.cell.size);
        let rect = Rect::new(at.x, at.y, size.w, size.h);
        let entry = self.agents.iter().find(|a| a.id == agent).cloned();
        let command = entry.is_none().then_some(agent);
        let id = self.start_agent(entry.as_ref(), command, "", folder, rect, window, cx)?;
        window.focus(&self.focus_handle, cx);
        Some(id)
    }

    /// The shell the agent `entry` opens in (docs/specs/shells.md, A shell for each agent): the
    /// one its `shell` label names among the shells found here. None for no label, a label not
    /// found (another computer's file, a shell uninstalled), and WSL for an account, which would
    /// lose its folder there: the default shell opens. Prints nothing: Settings asks every frame.
    pub(crate) fn agent_shell(&self, entry: &AgentEntry) -> Option<&ShellOption> {
        let label = entry.shell.as_deref()?;
        self.shells
            .iter()
            .find(|s| s.label == label)
            .filter(|s| entry.account.is_none() || !templum_terminal::is_shell(&s.program, "wsl"))
    }

    /// Whether the agent `entry` runs in WSL (docs/specs/shells.md, Installed): its shell is WSL,
    /// or it has none and Settings ▸ Shell is. The system's default never is, so this never scans
    /// the disk (Settings asks every frame).
    pub(crate) fn in_wsl(&self, entry: &AgentEntry) -> bool {
        self.agent_shell(entry)
            .map(|s| &s.program)
            .or(self.config.shell.program.as_ref())
            .is_some_and(|program| templum_terminal::is_shell(program, "wsl"))
    }

    /// Whether the menus and the welcome list the agent `entry` as installed: WSL is another
    /// system, whose programs aren't on Windows' PATH, so what runs there is trusted.
    pub(crate) fn agent_installed(&self, entry: &AgentEntry) -> bool {
        self.in_wsl(entry) || templum_terminal::installed(entry.program())
    }

    /// `agent_installed` as Settings ▸ Agents asks it, every frame: from what `find_programs`
    /// found (looked for now only for a program it hasn't seen).
    pub(crate) fn agent_found(&self, entry: &AgentEntry) -> bool {
        self.in_wsl(entry)
            || self
                .found
                .get(entry.program())
                .copied()
                .unwrap_or_else(|| crate::agents::locate(entry.program()).is_some())
    }

    /// Looks for every agent's program again, for `agent_found`.
    pub(crate) fn find_programs(&mut self) {
        self.found = self
            .agents
            .iter()
            .map(|a| {
                let program = a.program().to_string();
                let found = crate::agents::locate(&program).is_some();
                (program, found)
            })
            .collect();
    }

    /// Why `spawn_agent` and the Templum Agent can't open the agent `entry`: it runs in WSL,
    /// where Templum can't see it start nor type its prompt, or it isn't installed here.
    pub(crate) fn spawn_problem(&self, entry: &AgentEntry) -> Option<String> {
        if self.in_wsl(entry) {
            Some(format!(
                "{} runs in WSL, where Templum can't see it start nor type its prompt: open it from the canvas.",
                entry.title
            ))
        } else if !templum_terminal::installed(entry.program()) {
            Some(format!(
                "{} isn't installed on this computer ({} wasn't found).",
                entry.title,
                entry.program()
            ))
        } else {
            None
        }
    }

    /// `agent_shell`'s shell, for an agent about to open (the list read again if it was early): a
    /// label that finds none says so on stderr, as a restored terminal's does (no notification;
    /// the label stays in the file).
    fn shell_for(&mut self, entry: &AgentEntry) -> Option<ShellOption> {
        self.reload_early_shells();
        let shell = self.agent_shell(entry).cloned();
        if shell.is_none()
            && let Some(label) = &entry.shell
        {
            eprintln!(
                "templum: no shell {label:?} for {}: the default one opens",
                entry.title
            );
        }
        shell
    }

    /// A terminal at `rect` in `folder` with `command` (None: the agent `entry`'s start command)
    /// typed into its shell, `args` after it (`launch_args`: a model and an effort) and before
    /// what Templum adds. It is focused on the canvas; the keyboard stays
    /// where it is. The folder becomes this workspace's newest recent one.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_agent(
        &mut self,
        entry: Option<&AgentEntry>,
        command: Option<&str>,
        args: &str,
        folder: &Path,
        rect: Rect,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        if let Some(entry) = entry {
            self.prepare_account(entry, window, cx);
        }
        let shell = entry.and_then(|a| self.shell_for(a));
        let program = shell.as_ref().map(|s| s.program.as_str());
        let vars = self.agent_vars(entry, program, &super::usage::in_env);
        let id = self.spawn_block_with(
            rect,
            Some(folder.to_path_buf()),
            vars,
            shell.clone(),
            window,
            cx,
        )?;
        let command = command
            .map(str::to_string)
            .or_else(|| entry.map(|a| a.start_command(&uuid::Uuid::new_v4().to_string())))
            .unwrap_or_default()
            + args;
        // The shell reads it once it is ready.
        let line = format!(
            "{}\r",
            self.typed_in(shell.as_ref().map(|s| s.program.as_str()), entry, &command)
        );
        // Known at once (Shift+Enter, closing), not at the next snapshot.
        let agent = entry.map(|a| a.id.clone());
        let title = entry.map_or_else(
            || templum_terminal::program_name(&command),
            |a| a.title.clone(),
        );
        let pane = self.space.terminal_mut(id).expect("just spawned");
        pane.term.write(line.into_bytes());
        pane.account = entry.filter(|a| a.account.is_some()).map(|a| a.id.clone());
        pane.seen.agent = agent;
        self.cover_start(id, title, cx);
        remember(&mut self.space.recent, folder.to_path_buf());
        self.tell_gemini(entry, window, cx);
        Some(id)
    }

    /// Settings ▸ Agents ▸ Accounts ▸ Log in: a terminal of the account `id` at the view's
    /// center, in the home folder, with its agent's login command typed and sent; the person logs
    /// in there (docs/specs/accounts.md, Log in). Templum never reads nor writes the login.
    pub(crate) fn log_in(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        let entry = self
            .agents
            .iter()
            .find(|a| a.id == id && a.account.is_some())
            .cloned()?;
        let kind = entry.account.as_ref()?.kind;
        self.prepare_account(&entry, window, cx);
        let shell = self.shell_for(&entry);
        let program = shell.as_ref().map(|s| s.program.as_str());
        let vars = self.agent_vars(Some(&entry), program, &super::usage::in_env);
        let rect = self.new_terminal_rect(window, cx);
        let block = self.spawn_block_with(rect, std::env::home_dir(), vars, shell, window, cx)?;
        let pane = self.space.terminal_mut(block)?;
        // The shell reads it once it is ready.
        pane.term
            .write(format!("{}\r", kind.login_command).into_bytes());
        pane.account = Some(entry.id.clone());
        if let Some(hint) = kind.login_hint {
            let agent = entry.builtin.map_or(kind.agent, |a| a.title());
            show_notification(
                window,
                cx,
                Notification::info(format!("Type {hint} in {agent} to log in.")),
            );
        }
        window.focus(&self.focus_handle, cx);
        Some(block)
    }

    /// Applies the account `entry`'s choices to its folder as it starts (docs/specs/accounts.md,
    /// Making and changing it), and says what went wrong; nothing for an agent.
    pub(crate) fn prepare_account(
        &self,
        entry: &AgentEntry,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if entry.account.is_none() {
            return;
        }
        let report = entry.apply(&self.agents, templum_usage::accounts::When::Starting);
        self.tell_account(entry, &report, window, cx);
    }

    /// The notifications of what applying the account `entry`'s choices met: one for what
    /// couldn't be done, one for what was kept aside or left, one per item its agent replaced
    /// with its own copy.
    pub(crate) fn tell_account(
        &self,
        entry: &AgentEntry,
        report: &templum_usage::accounts::Report,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(account) = &entry.account else {
            return;
        };
        if !report.problems.is_empty() {
            let message = format!("{}: {}", entry.title, report.problems.join("; "));
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::warning(message));
        }
        if !report.kept.is_empty() {
            let kept: Vec<String> = report
                .kept
                .iter()
                .filter_map(|(_, aside)| {
                    let name = aside.file_name()?.to_string_lossy().into_owned();
                    let shared = name.rsplit_once(".own").map(|(item, _)| item.to_string())?;
                    Some(format!("{shared} as {name}"))
                })
                .collect();
            let message = format!(
                "{} kept its own aside to share them: {} (in {}).",
                entry.title,
                kept.join(", "),
                crate::agents::tilde(&account.folder)
            );
            show_notification(window, cx, Notification::info(message));
        }
        let path = |key: &str| {
            account
                .kind
                .item(key)
                .map(|i| i.paths[0].trim_end_matches('/'))
        };
        if !report.left.is_empty() {
            let left: Vec<&str> = report.left.iter().filter_map(|k| path(k)).collect();
            let message = format!(
                "{} keeps its own {}: Templum never moves files in a folder you chose ({}).",
                entry.title,
                left.join(", "),
                crate::agents::tilde(&account.folder)
            );
            show_notification(window, cx, Notification::info(message));
        }
        let agent = entry.builtin.map_or(account.kind.agent, |a| a.title());
        for key in &report.replaced {
            let Some(path) = path(key) else {
                continue;
            };
            let message = format!(
                "{}'s {path} isn't shared any more: {agent} wrote its own. Edit the account to \
                 share it again.",
                entry.title
            );
            show_notification(window, cx, Notification::warning(message));
        }
    }

    /// The kanban block `block` shows `board`.
    pub(crate) fn show_board(&mut self, block: BlockId, board: u64, cx: &mut Context<Self>) {
        if let Some(k) = self.space.kanban_mut(block) {
            k.board = Some(board);
            cx.notify();
        }
    }

    /// The first workspace with a kanban block showing board `id`, the active one first.
    pub(crate) fn board_open_at(&self, id: u64) -> Option<&Space> {
        self.spaces().find(|space| {
            space
                .blocks
                .values()
                .any(|body| body.kanban().is_some_and(|k| k.board == Some(id)))
        })
    }

    /// A new board, shown in `block`.
    pub(crate) fn new_board(&mut self, block: BlockId, cx: &mut Context<Self>) {
        let board = self.edit_boards(Boards::add_board);
        self.show_board(block, board, cx);
    }

    /// Deletes a board and its cards; the blocks that showed it show none.
    pub(crate) fn delete_board(&mut self, board: u64, cx: &mut Context<Self>) {
        self.edit_boards(|b| b.remove_board(board));
        cx.notify();
    }

    /// Starts a terminal in a new focused block, in the configured start folder.
    pub(crate) fn open(&mut self, rect: Rect, window: &mut Window, cx: &mut Context<Self>) {
        if self.on_remote() || self.why_not(Deed::AddTerminal).is_some() {
            return;
        }
        let folder = self.start_dir(window, cx);
        self.spawn_block(rect, folder, window, cx);
    }

    /// Starts a terminal in `folder` (None: the home folder) in a new focused block; on failure,
    /// a notification instead.
    pub(super) fn spawn_block(
        &mut self,
        rect: Rect,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        self.spawn_block_with(rect, folder, Vec::new(), None, window, cx)
    }

    /// `spawn_block`, with `vars` in its shell's environment (`agent_vars`), running `shell` (None:
    /// the workspace's command in the tests, else the `[shell]` of config.toml, else the default).
    /// The block keeps `shell`.
    pub(super) fn spawn_block_with(
        &mut self,
        rect: Rect,
        folder: Option<PathBuf>,
        vars: Vec<(String, String)>,
        shell: Option<ShellOption>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        if self.on_remote() {
            return None;
        }
        let (cols, rows) = chrome::grid_size(chrome::content_rect(rect).size(), self.cell.size);
        let command = shell
            .as_ref()
            .map(|s| (s.program.clone(), s.args.clone()))
            .or_else(|| self.command.clone())
            .or_else(|| {
                let shell = &self.config.shell;
                shell
                    .program
                    .clone()
                    .map(|program| (program, shell.args.clone()))
            });
        let opts = SpawnOptions {
            cols,
            rows,
            cell_px: self.cell.px(),
            command,
            palette: self.theme.palette,
            options: self.config.term_options(),
            working_directory: folder.clone(),
            integration: self
                .integration
                .clone()
                .filter(|_| self.config.shell.integration),
            env: {
                let mut env = match (&self.bus, &self.data_dir) {
                    (Some(bus), Some(data)) => super::bus::terminal_env(bus, data),
                    // Not the bus of a Templum this one was started from, which doesn't know its
                    // terminals: an empty one names none.
                    _ => vec![("TEMPLUM_BUS".to_string(), String::new())],
                };
                // For a `claude` or `codex` typed there, always (empty: not that Templum's either),
                // by the terminal's own shell.
                env.extend(self.agent_env(shell.as_ref().map(|s| s.program.as_str())));
                env.extend(vars);
                env
            },
        };
        let start = folder.or_else(std::env::home_dir);
        let id = match Terminal::spawn(opts, self.notify.clone()) {
            Ok(term) => Some(self.add_pane(rect, term, start, shell)),
            Err(err) => {
                eprintln!("templum: couldn't start a shell: {err}");
                let message = format!("Couldn't start a shell: {err}");
                show_notification(window, cx, Notification::error(message));
                None
            }
        };
        cx.notify();
        id
    }

    /// A new terminal block at `rect` showing `term`, its folder `start`, keeping `shell`: focused,
    /// and a step of Ctrl+Z unless `quiet` (`added`).
    pub(super) fn add_pane(
        &mut self,
        rect: Rect,
        term: Terminal,
        start: Option<PathBuf>,
        shell: Option<ShellOption>,
    ) -> BlockId {
        let id = self.space.canvas.add(rect);
        self.space
            .blocks
            .insert(id, Body::Terminal(Box::new(Pane::new(term, start, shell))));
        self.added(id);
        id
    }

    /// Closes a block. An editor with unsaved edits asks first: Save all, Don't save, Cancel; a
    /// terminal at work (an agent, a command running) too: Cancel or Close.
    pub(crate) fn close(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        self.close_blocks(vec![id], Then::New, window, cx);
    }

    /// Closes blocks as `close` does one; several ask once. `then` says where Ctrl+Z keeps them:
    /// an undo or a redo closes only if its step is still next when the person answers.
    pub(super) fn close_blocks(
        &mut self,
        ids: Vec<BlockId>,
        then: Then,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.why_not(Deed::Close).is_some() {
            return;
        }
        if self.on_remote() {
            return self.remote_close(ids, window, cx);
        }
        let one = ids.len() == 1;
        let busy: Vec<String> = ids
            .iter()
            .filter_map(|id| {
                // A shell that ended has nothing left to lose.
                let pane = self.space.terminal(*id).filter(|p| !p.term.exited())?;
                let why = alert::busy(&pane.term.status(), self.agent_name(pane))?;
                Some(if one {
                    why
                } else {
                    format!("{}: {why}", pane.term.title())
                })
            })
            .collect();
        // Only this workspace: block ids count from 0 in every one.
        let unsaved: Vec<String> = ids
            .iter()
            .flat_map(|id| self.space.unsaved(Some(*id)))
            .collect();
        if busy.is_empty() && unsaved.is_empty() {
            self.close_now(&ids, then, window, cx);
            return;
        }
        let (ws, space) = (cx.entity(), self.space.id.clone());
        let shown = space.clone();
        let close = {
            let (ws, ids) = (ws.clone(), ids.clone());
            move |window: &mut Window, cx: &mut App| {
                ws.update(cx, |ws, cx| {
                    if ws.still_shown(&space, window, cx) {
                        ws.close_now(&ids, then, window, cx);
                    }
                })
            }
        };
        let busy = busy.join(" ");
        if unsaved.is_empty() {
            let title = if one {
                "Close the terminal?".to_string()
            } else {
                format!("Close {} blocks?", ids.len())
            };
            prompt::confirm(title, busy, "Close", close, window, cx);
            return;
        }
        let mut why = format!("Unsaved: {}", unsaved.join(", "));
        if !busy.is_empty() {
            why = format!("{why}. {busy}");
        }
        let save = close.clone();
        prompt::save_or_discard(
            "Save changes?",
            why,
            move |window, cx| {
                // Every one is tried, even after a failure.
                let saved = ws.update(cx, |ws, cx| {
                    if !ws.still_shown(&shown, window, cx) {
                        return false;
                    }
                    let mut ok = true;
                    for id in &ids {
                        ok &= ws.save_all(*id, window, cx);
                    }
                    ok
                });
                if saved {
                    save(window, cx);
                }
                saved
            },
            close,
            window,
            cx,
        );
    }

    /// Closes the blocks `ids` now, kept for Ctrl+Z (or Ctrl+Shift+Z, after an undo) as one
    /// step. Nothing is focused after: the keyboard stays on the canvas, so Ctrl+Z reaches it,
    /// not a terminal.
    pub(super) fn close_now(
        &mut self,
        ids: &[BlockId],
        then: Then,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.take_step(then) {
            return;
        }
        // A subagent's block never comes back: it belongs to a live session.
        let items: Vec<_> = self
            .space
            .canvas
            .blocks()
            .iter()
            .filter(|b| ids.contains(&b.id) && self.space.kept(b.id))
            .map(|b| (b.id, self.space.item_of(b)))
            .collect();
        // Their wires go with them; undoing the close brings them back.
        let wires: Vec<_> = self
            .space
            .wires
            .iter()
            .filter(|w| ids.iter().any(|id| w.touches(*id)))
            .copied()
            .collect();
        let was_focused = self.space.focused.is_some_and(|id| ids.contains(&id));
        for id in ids {
            self.remove_block(*id, window, cx);
        }
        if was_focused {
            self.space.focused = None;
        }
        if !items.is_empty() {
            self.keep(Change::Open(items, wires), then);
        }
    }

    /// Removes a block and kills its shell (dropping the Terminal shuts it down).
    /// The block leaves the canvas at once; it stays on screen as a ghost while it fades out.
    /// The task its agent was on ends: the agent that handed it over hears so.
    pub(super) fn remove_block(
        &mut self,
        id: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let terminal = self.space.terminal(id).map(|p| p.term.id().to_string());
        if let Some(terminal) = &terminal {
            let uid = self.space.uids.get(&id).cloned().unwrap_or_default();
            let why = format!("{uid} was closed before it answered.");
            self.end_turn(terminal, templum_mcp::Reply::err(why), cx);
        }
        if let Some((block, _, body)) = self.take_block(id, window, cx) {
            // Its notes stay in the Notes dialog, but for blank ones: nothing is lost. (A mirrored
            // block's are home's, by home's ids.)
            for note in body.note().map(Note::ids).unwrap_or_default() {
                self.note_closed(note);
            }
            if let Some(list) = body.checklist().and_then(|c| c.list) {
                self.drop_checklist_if_blank(list);
            }
            self.space.ghosts.push(Ghost {
                block,
                body,
                since: Instant::now(),
            });
        }
        // Its subagents' blocks go with it (docs/specs/claude-subagents.md).
        if let Some(terminal) = terminal {
            self.close_subagents_of(&terminal, window, cx);
        }
        cx.notify();
    }

    /// Takes a block off the canvas with what it holds, still running: the block, its rect on
    /// the canvas (also while it is a tile) and its body.
    pub(super) fn take_block(
        &mut self,
        id: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Block, Rect, Body)> {
        // A checklist's field stops: its text is kept, an item left empty goes.
        self.stop_editing(id, window, cx);
        self.release_focus(id, window, cx);
        let block = self.space.canvas.get(id).copied();
        let home = block.map(|b| self.space.home(&b));
        self.space.canvas.remove(id);
        self.space.appearing.remove(&id);
        let body = self.space.blocks.remove(&id);
        self.space.uids.remove(&id);
        self.space.unwire(id);
        if self.space.hovered == Some(id) {
            self.space.hovered = None;
        }
        self.space.selected.retain(|s| *s != id);
        // A tile's neighbour takes its place.
        self.untile(id);
        if self.space.focused == Some(id) {
            self.space.focused = self.space.top();
        }
        // A gesture on a block that's gone would do nothing anyway; end it.
        self.drag = None;
        Some((block?, home?, body?))
    }

    /// Finds the news in every workspace's terminals (see [`alert::news`]). A terminal being looked
    /// at, focused in the active window, has none, and loses its alert; another gets an alert and
    /// a notification (the first news until it is seen), also the system's when the window is in
    /// the background.
    pub(super) fn hear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = window.is_window_active();
        let several = !self.others.is_empty();
        let mut news = Vec::new();
        let mut finished_in: Vec<PathBuf> = Vec::new();
        let spaces = std::iter::once(&mut self.space).chain(&mut self.others);
        for (i, space) in spaces.enumerate() {
            let terminals = space
                .blocks
                .iter_mut()
                .filter_map(|(id, body)| Some((*id, body.terminal_mut()?)));
            for (id, pane) in terminals {
                let status = pane.term.status();
                let new = alert::news(&status, pane.heard);
                if status.finished > pane.heard.0 {
                    let reported = pane.term.processes().reported;
                    if reported.is_some() {
                        pane.seen.folder.clone_from(&reported);
                    }
                    if i == 0 {
                        finished_in.extend(reported);
                    }
                }
                pane.heard = (status.finished, status.attention);
                if active && i == 0 && space.focused == Some(id) {
                    pane.alert = None;
                } else if let Some(alert) = new {
                    // One notification until it is seen: a terminal ringing on and on doesn't flood.
                    if pane.alert.is_none() {
                        let mut title = pane.term.title();
                        if several {
                            title = format!("{title} · {}", space.name);
                        }
                        // Another workspace's news shows its icon.
                        let icon = space.look.icon.filter(|_| i > 0);
                        news.push((space.id.clone(), id, title, alert.clone(), icon));
                    }
                    pane.alert = Some(alert);
                }
            }
        }
        // `git add` in a terminal shows in the Git popup within a frame.
        let in_repo = self
            .space
            .git()
            .and_then(|g| g.repo())
            .is_some_and(|r| finished_in.iter().any(|f| f.starts_with(&r.root)));
        if self.git_open && in_repo {
            self.refresh_git(window, cx);
        }
        let delivery = self.delivery(window);
        let ws = cx.entity().downgrade();
        for (space, block, title, alert, icon) in news {
            let ws = ws.clone();
            let key = SharedString::from(format!("alert-{space}-{}", block.0));
            let notification = Notification::new().id1::<Alert>(key);
            // A notification's type would draw its own icon over the workspace's.
            let notification = match icon {
                Some(icon) => notification.icon(Icon::new(icon)),
                None => notification.with_type(alert.kind()),
            };
            let notification = notification
                .title(title)
                .message(alert.text())
                .delivery(delivery)
                .on_click(move |_, window, cx| {
                    ws.update(cx, |ws, cx| ws.reveal(&space, block, window, cx))
                        .ok();
                });
            show_notification(window, cx, notification);
        }
    }

    /// Where a new terminal starts (None: the home folder): the focused block's folder when
    /// the settings say so, else the workspace's, else the settings'.
    pub(super) fn start_dir(&self, window: &mut Window, cx: &mut App) -> Option<PathBuf> {
        let setting = &self.config.shell.working_directory;
        if matches!(setting, WorkingDirectory::Inherit)
            && let Some(folder) = self
                .space
                .focused
                .and_then(|id| self.space.blocks.get(&id)?.folder())
        {
            return Some(folder);
        }
        // The workspace's choice is nearer than the settings'.
        if let Some(folder) = self.space.folder.clone().filter(|f| f.is_dir()) {
            return Some(folder);
        }
        match setting {
            WorkingDirectory::Inherit | WorkingDirectory::Home => None,
            WorkingDirectory::Path(path) if path.is_dir() => Some(path.clone()),
            WorkingDirectory::Path(path) => {
                let message = format!(
                    "{} isn't a folder; starting in the home folder",
                    path.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
                None
            }
        }
    }

    /// Block `id` comes to the top with the keyboard; nothing stays selected.
    pub(crate) fn focus_block(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        self.space.canvas.raise(id);
        self.space.selected.clear();
        self.space.selected_ink = None;
        self.space.selected_wire = None;
        self.give_keyboard(id, window, cx);
    }

    /// Block `id` takes the keyboard, where it is in the stack.
    pub(super) fn give_keyboard(
        &mut self,
        id: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The block that had the keyboard loses it: its checklist field stops. GPUI's blur would
        // say so too, but only to a field drawn again since it took the keyboard.
        if let Some(had) = self.space.focused {
            self.stop_editing(had, window, cx);
        }
        self.space.pristine = false;
        self.space.focused = Some(id);
        // What types inside the block takes the keyboard; anything else leaves it with the
        // workspace.
        if !self
            .space
            .blocks
            .get(&id)
            .is_some_and(|b| b.focus_inner(window, cx))
        {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    pub(crate) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open_shell(None, None, window, cx);
    }

    /// Where a new terminal goes: the view's center, or beside the blocks there, shown.
    pub(super) fn new_terminal_rect(&mut self, window: &Window, cx: &mut Context<Self>) -> Rect {
        self.space.pristine = false;
        let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
        self.room_in_view(size, window, cx)
    }

    pub(super) fn close_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.space.focused {
            self.close(id, window, cx);
            // The keyboard goes on to the block on top, as a terminal app's next tab.
            if self.space.focused.is_none() {
                self.space.focused = self.space.top();
            }
        }
    }
}

/// A terminal as a menu names it: its agent, its title and its folder's name, each once.
pub(super) fn label(agent: Option<&str>, title: &str, folder: Option<&Path>) -> String {
    let folder = folder
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned());
    let mut parts: Vec<&str> = Vec::new();
    for part in [agent, Some(title), folder.as_deref()]
        .into_iter()
        .flatten()
    {
        if !part.is_empty() && !parts.contains(&part) {
            parts.push(part);
        }
    }
    parts.join(" · ")
}

/// `n` in base 36 (digits, then lowercase letters), `width` characters, zeros in front.
fn base36(mut n: u128, width: usize) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut text = vec![b'0'; width];
    for c in text.iter_mut().rev() {
        *c = DIGITS[(n % 36) as usize];
        n /= 36;
    }
    String::from_utf8(text).expect("ASCII")
}

impl Pane {
    /// A terminal block's pane around `term`, its folder `start`, keeping `shell`.
    pub(super) fn new(term: Terminal, start: Option<PathBuf>, shell: Option<ShellOption>) -> Pane {
        Pane {
            term,
            snapshot: Rc::default(),
            heard: (0, 0),
            alert: None,
            seen: templum_terminal::Inspection {
                folder: start,
                ..Default::default()
            },
            usage: None,
            find: None,
            state: None,
            handed: None,
            answer: None,
            prompt: None,
            looks: Default::default(),
            typed: Default::default(),
            keyed: Default::default(),
            draft: Default::default(),
            scrolled: Default::default(),
            submitted: 0,
            dialogs: 0,
            speaks: false,
            keys_from: None,
            opened_by: None,
            model: None,
            effort: None,
            flags_for: None,
            sent: None,
            transcript: None,
            starting: None,
            typed_agent: None,
            restored: false,
            resuming: None,
            account: None,
            role: None,
            quit: None,
            shell,
            subagents: Vec::new(),
            awaits_subagents: false,
            asked_by: None,
            settles: None,
            follows: None,
            watch: Default::default(),
            remote: None,
        }
    }
}
