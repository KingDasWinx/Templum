//! Right-click menus: one at a time, built for what was right-clicked, drawn at the pointer.
//! Each right-clickable element opens its target's menu and stops the event, so the innermost
//! one wins. See docs/specs/context-menus.md.

mod build;

pub(crate) use build::cards;

use std::path::PathBuf;
use std::time::Instant;

use crate::agents::AgentEntry;
use gpui_kit::SharedString;
use gpui_kit::component::menu::PopupMenu;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, DismissEvent, Entity, Focusable as _, Pixels, Point as PixelPoint,
    ScrollWheelEvent, Subscription, Window, anchored, deferred, div, px,
};
use templum_canvas::{BlockId, Point, Wire};
use templum_git::{Branch, Commit, Entry};
use templum_terminal::ShellOption;

use crate::git_block;
use crate::workspace::remote::{Deed, GIT_GATES, HomeAgent, HomeShells};
use crate::workspace::{Body, Workspace};

/// What was right-clicked.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Target {
    /// The empty canvas, at a world point: blocks are added there.
    Canvas(Point),
    /// A block, whatever it holds: the menu is built for its kind. `group`: the blocks selected
    /// with it when it was right-clicked (focusing it drops the selection).
    Block {
        id: BlockId,
        group: Vec<BlockId>,
    },
    /// The AI agents alone (the toolbar): the chosen one opens at this world point.
    Agents(Point),
    /// The shells alone (the toolbar's Terminal): the chosen one opens where a new terminal goes.
    Shells,
    Column {
        board: u64,
        column: u64,
    },
    Card {
        board: u64,
        card: u64,
    },
    /// A workspace's tile in the sidebar, by its id.
    Space(String),
    /// A drawing, by its index in the space's ink.
    Ink(usize),
    /// A wire between two blocks.
    Wire(Wire),
    /// A stash in a Git block's Stashes tab, by N in `stash@{N}`.
    GitStash {
        index: usize,
    },
    /// A branch in a Git block's Branches tab, by its name.
    GitBranch {
        name: String,
    },
    /// A commit in a Git block's History, by its hash.
    GitCommit {
        hash: String,
    },
    /// A file of the commit shown in a Git block's History.
    GitCommitFile {
        hash: String,
        path: String,
    },
    /// The ▾ beside a Git block's Commit button.
    GitCommitMenu,
    /// The ⚙ beside a Git block's Generate ✨.
    GitGenerateMenu,
    /// A row of a Git block's Changes lists, by its list and its index in it.
    GitRow {
        list: git_block::List,
        index: usize,
    },
    /// The terminals working in the Git's repository (its top bar).
    GitTerminals,
    /// The Git popup's folder: other folders to show.
    GitFolders,
    /// The ⋯ of a Git block's top bar.
    GitMore,
    /// Publish's remotes, when there are several: for the block `at` names.
    GitRemotes {
        at: crate::workspace::GitAt,
        branch: String,
        remotes: Vec<String>,
    },
    /// A line row of a Git block's diff panel, by its row index.
    GitLine {
        row: usize,
    },
    /// A row of an editor's file tree (a file or a folder), or its empty space (None).
    Tree {
        block: BlockId,
        path: Option<PathBuf>,
    },
}

/// The open menu.
pub(crate) struct OpenMenu {
    view: Entity<PopupMenu>,
    /// Window position of the right-click.
    at: PixelPoint<Pixels>,
    _dismissed: Subscription,
}

impl Workspace {
    /// Opens `target`'s menu at `at`, replacing any open one.
    pub(crate) fn open_menu(
        &mut self,
        target: Target,
        at: PixelPoint<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The ink a mirrored workspace doesn't have: the menu says so instead of opening. The
        // agents and the shells it has only when home can open them.
        if self.on_remote() {
            let why = match target {
                Target::Ink(_) => Some(crate::workspace::remote::NOT_YET.into()),
                Target::Agents(_) => self.why_not(Deed::OpenAgent),
                Target::Shells => self.why_not(Deed::AddTerminal),
                _ => None,
            };
            if let Some(why) = why {
                return self.warn(why, window, cx);
            }
            // Home's agents and shells are read at each connect, and again when they are a minute
            // old.
            let (now, id) = (Instant::now(), self.space.id.clone());
            let remote = self.space.remote.as_deref();
            let (agents, shells) = (
                remote.is_some_and(|r| r.agents_stale(now)),
                remote.is_some_and(|r| r.shells_stale(now)),
            );
            if agents && matches!(target, Target::Canvas(_) | Target::Agents(_)) {
                self.read_home_agents(&id, cx);
            }
            if shells && matches!(target, Target::Canvas(_) | Target::Shells) {
                self.read_home_shells(&id, cx);
            }
            // Read again for the next opening: home may have opened a folder since.
            if matches!(target, Target::GitFolders) {
                self.read_home_recent(&id, cx);
            }
        }
        self.reload_early_shells();
        let ws = cx.entity();
        // Esc or a choice gives the keyboard back to what had it.
        let back = window
            .focused(cx)
            .unwrap_or_else(|| self.focus_handle.clone());
        let Some(items) = self.items(&target, cx) else {
            return;
        };
        let view = PopupMenu::build(window, cx, move |menu, window, cx| {
            items.build(&ws, menu.action_context(back), window, cx)
        });
        let _dismissed = cx.subscribe_in(&view, window, |ws, view, _: &DismissEvent, _, cx| {
            if ws.menu.as_ref().is_some_and(|m| m.view == *view) {
                ws.menu = None;
                cx.notify();
            }
        });
        view.focus_handle(cx).focus(window, cx);
        self.menu = Some(OpenMenu {
            view,
            at,
            _dismissed,
        });
        cx.notify();
    }

    /// Block `id`'s menu at `at`: the right-click's, and the header's ⋯. The block is focused
    /// first so its menu acts on it (copy, paste); the selection it is in goes along (Fullscreen
    /// takes it), read before focusing drops it.
    pub(crate) fn open_block_menu(
        &mut self,
        id: BlockId,
        at: PixelPoint<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let group = if self.space.selected.contains(&id) {
            self.space.selected.clone()
        } else {
            Vec::new()
        };
        self.focus_block(id, window, cx);
        self.open_menu(Target::Block { id, group }, at, window, cx);
    }

    /// The shells the menus list (docs/specs/shells.md, The menus): Default's name, and the
    /// others, without the one that name is already.
    fn shells_for_menu(&self) -> Shells {
        let default = self.default_shell_name();
        let others = self
            .shells
            .iter()
            .filter(|s| s.label != default)
            .cloned()
            .collect();
        Shells { default, others }
    }

    /// The agents the menus list, and whether each one's program was found.
    pub(super) fn agents_for_menu(&self) -> Vec<(AgentEntry, bool)> {
        self.agents
            .iter()
            .filter(|a| a.show)
            .map(|a| (a.clone(), self.agent_installed(a)))
            .collect()
    }

    /// The shells a menu lists: this computer's, or on a mirror home's (none while unread).
    fn shell_list_for_menu(&self) -> ShellList {
        match self.space.remote.as_deref() {
            Some(remote) => ShellList::Home {
                shells: remote.shells.clone(),
                computer: crate::mirror::computer_of(remote),
                why: self.why_not(Deed::AddShell).map(Into::into),
            },
            None => ShellList::Local(self.shells_for_menu()),
        }
    }

    /// The agents a menu lists: this computer's, or on a mirror home's (none while unread).
    fn agent_list_for_menu(&self) -> AgentList {
        match self.space.remote.as_deref() {
            Some(remote) => AgentList::Home {
                agents: remote.agents.clone(),
                computer: crate::mirror::computer_of(remote),
            },
            None => AgentList::Local(self.agents_for_menu()),
        }
    }

    /// How block `block`'s menu ends: in grid mode, the tile's items; on the canvas, Fullscreen
    /// (with `group`, the selection it is in) and its wiring.
    fn tail(&self, block: BlockId, group: Vec<BlockId>, cx: &App) -> Tail {
        match &self.space.grid {
            None => {
                let (disconnect, connect) = self
                    .space
                    .canvas
                    .blocks()
                    .iter()
                    .filter(|b| b.id != block)
                    .map(|b| {
                        (
                            b.id,
                            crate::block::title(self, &self.space.blocks[&b.id], cx),
                        )
                    })
                    .partition(|(id, _)| self.space.wired(block, *id));
                Tail::Canvas {
                    group,
                    connect,
                    disconnect,
                }
            }
            Some(grid) => Tail::Tile {
                at: self.space.camera().to_world(Point::new(
                    grid.area.x + grid.area.w / 2.0,
                    grid.area.y + grid.area.h / 2.0,
                )),
                agents: self.agent_list_for_menu(),
                shells: self.shell_list_for_menu(),
                flip: grid.tiles.ids().len() > 1 && grid.homes.contains_key(&block),
                others: self
                    .space
                    .canvas
                    .blocks()
                    .iter()
                    .filter(|b| !self.space.shown(b.id))
                    .map(|b| {
                        (
                            b.id,
                            crate::block::title(self, &self.space.blocks[&b.id], cx),
                        )
                    })
                    .collect(),
            },
        }
    }

    /// The terminals Send to lists on the workspace shown: a mirror's are home's, never one of
    /// this computer's.
    pub(crate) fn send_to_targets(&self) -> Vec<(BlockId, String)> {
        if self.on_remote() {
            self.remote_terminals()
        } else {
            self.terminal_list(None)
        }
    }

    /// A menu's Send to: `text`, and the terminals it can go to.
    fn send_to_menu(&self, text: String) -> SendTo {
        SendTo {
            text,
            to: self.send_to_targets(),
            // A mirror's terminals are home's: Send to types into them as typing does.
            why: self
                .on_remote()
                .then(|| self.why_not(Deed::Type).map(Into::into))
                .flatten(),
        }
    }

    /// What the menu for `target` needs to know, read now: building it can't read the workspace.
    fn items(&self, target: &Target, cx: &App) -> Option<Items> {
        Some(match target.clone() {
            Target::Canvas(at) => Items::Canvas {
                at,
                agents: self.agent_list_for_menu(),
                shells: self.shell_list_for_menu(),
                gates: Gates::of(
                    self,
                    &[
                        Deed::OpenAgent,
                        Deed::AddTerminal,
                        Deed::AddKanban,
                        Deed::AddNote,
                        Deed::AddChecklist,
                        Deed::AddEditor,
                        Deed::OpenNote,
                        Deed::OpenChecklist,
                    ],
                ),
            },
            Target::Shells => Items::Shells(self.shell_list_for_menu()),
            Target::Agents(at) => Items::Agents {
                at,
                agents: self.agent_list_for_menu(),
            },
            Target::Block { id: block, group } => {
                let body = self.space.blocks.get(&block)?;
                let inner = match body {
                    Body::Terminal(pane) => Items::Terminal {
                        block,
                        selection: pane.term.shared().selection_text().is_some(),
                        hand: pane.remote.as_ref().is_some_and(|r| r.agent.is_some()),
                        gates: Gates::of(self, &[Deed::Type, Deed::Delegate]),
                    },
                    Body::Kanban(k) => Items::Kanban {
                        block,
                        board: k
                            .shown(self)
                            .map(|b| (b.id, b.name.clone(), b.card_count())),
                        gates: Gates::of(self, &[Deed::ShowBoard, Deed::EditBoard]),
                    },
                    Body::Note(n) => Items::Note {
                        block,
                        color: n.tab().map(|t| {
                            self.notes_of(n)
                                .and_then(|notes| notes.get(t.id))
                                .and_then(|e| e.color)
                        }),
                        rename: n.tab().map(|t| {
                            let name = self.notes_of(n).map_or("", |notes| notes.name(t.id));
                            (t.id, name.to_string())
                        }),
                        send: self.send_to_menu(n.to_send(cx)),
                        gates: Gates::of(self, &[Deed::EditNote, Deed::OpenNote]),
                    },
                    Body::Checklist(_) => Items::Checklist {
                        block,
                        gates: Gates::of(self, &[Deed::OpenChecklist]),
                    },
                    Body::Mirrored(_) => Items::Mirrored,
                    Body::Editor(e) => Items::Editor {
                        block,
                        tree: e.show_tree,
                        wrap: e.wrap,
                        send: self.send_to_menu(e.to_send(cx).unwrap_or_default()),
                    },
                };
                let moving = if group.contains(&block) {
                    group.clone()
                } else {
                    vec![block]
                };
                let terminal = body.terminal().map(|p| p.term.id().to_string());
                let uid = self.space.uids.get(&block).cloned().unwrap_or_default();
                let schedules = terminal
                    .as_deref()
                    .map_or_else(Vec::new, |t| self.schedules_of(t, &uid));
                Items::Block {
                    block,
                    inner: Box::new(inner),
                    schedules,
                    terminal,
                    moving,
                    spaces: self
                        .others
                        .iter()
                        .map(|s| (s.id.clone(), s.name.clone(), s.takes_blocks()))
                        .collect(),
                    tail: self.tail(block, group, cx),
                    gates: Gates::of(
                        self,
                        &[
                            Deed::Wire,
                            Deed::Unwire,
                            Deed::MoveToWorkspace,
                            Deed::Close,
                            // A tile's new blocks.
                            Deed::OpenAgent,
                            Deed::AddTerminal,
                            Deed::AddKanban,
                            Deed::AddNote,
                            Deed::AddChecklist,
                            Deed::AddEditor,
                        ],
                    ),
                    close: if body.terminal().is_some() {
                        "Close terminal"
                    } else {
                        "Close"
                    },
                }
            }
            Target::Column { board, column } => {
                let boards = self.boards_in(&self.space.id)?;
                let found = boards
                    .board(board)
                    .and_then(|b| b.columns.iter().find(|c| c.id == column));
                Items::Column {
                    board,
                    column,
                    title: found.map_or_else(String::new, |c| c.title.clone()),
                    cards: found.map_or(0, |c| c.cards.len()),
                    draft: boards.draft(column).is_some(),
                    gates: Gates::of(self, &[Deed::EditColumn, Deed::CreateCard]),
                }
            }
            Target::Card { board, card } => {
                let boards = self.boards_in(&self.space.id)?;
                // A mirrored card's attachments are home's: no path of this computer's.
                let data = self.data_dir.as_deref().filter(|_| !self.space.is_remote());
                Items::Card {
                    board,
                    card,
                    send: self.send_to_menu(
                        boards
                            .card(board, card)
                            .map_or_else(String::new, |c| c.text(data)),
                    ),
                    gates: Gates::of(self, &[Deed::DeleteCard]),
                }
            }
            Target::Ink(index) => Items::Ink { index },
            Target::Wire(wire) => Items::Wire {
                wire,
                gates: Gates::of(self, &[Deed::Label, Deed::Unwire]),
            },
            Target::GitCommitMenu => {
                let git = self.space.git()?;
                Items::GitCommitMenu {
                    label: git.commit_label(),
                    can: git.can_commit(cx),
                    amend: git.amend,
                }
            }
            Target::GitGenerateMenu => {
                self.space.git()?.repo()?;
                let agent = self.commit_agent.as_ref()?;
                Items::GitGenerateMenu {
                    models: agent.models.clone(),
                    model: agent.model.clone(),
                }
            }
            Target::GitRow { list, index } => {
                let git = self.space.git()?;
                let entries = match list {
                    git_block::List::Conflicts => git.status.conflicts(),
                    git_block::List::Files => git.status.files(),
                };
                let entry = (*entries.get(index)?).clone();
                Items::GitRow {
                    list,
                    labels: git.conflict_labels(&entry.path),
                    entry,
                    gates: Gates::of(self, &GIT_GATES),
                    remote: self.on_remote(),
                }
            }
            Target::GitStash { index } => {
                self.space
                    .git()?
                    .stashes
                    .iter()
                    .find(|s| s.index == index)?;
                Items::GitStash {
                    index,
                    gates: Gates::of(self, &GIT_GATES),
                }
            }
            Target::GitBranch { ref name } => {
                let git = self.space.git()?;
                Items::GitBranch {
                    branch: git.branches.iter().find(|b| b.name == *name)?.clone(),
                    current: git.status.branch.clone(),
                    base: git.default_branch.clone(),
                    gates: Gates::of(self, &GIT_GATES),
                }
            }
            Target::GitCommit { ref hash } => {
                let git = self.space.git()?;
                let commit = git.commits.iter().find(|c| c.hash == *hash)?.clone();
                let s = &git.status;
                // The newest commit of a branch, not on its upstream: it can be taken back.
                let undo = s.oid.as_deref() == Some(commit.hash.as_str())
                    && s.branch.is_some()
                    && (s.upstream.is_none() || s.gone || s.ahead > 0);
                // In a file's history: the file's path then, unless the commit deleted it.
                let restore = git
                    .file
                    .as_ref()
                    .and_then(|_| git.commits.iter().position(|c| c.hash == *hash))
                    .and_then(|i| git.file_paths.get(i))
                    .filter(|f| f.change != templum_git::Change::Deleted)
                    .map(|f| f.path.clone());
                Items::GitCommit {
                    commit,
                    undo,
                    restore,
                    gates: Gates::of(self, &GIT_GATES),
                }
            }
            Target::GitCommitFile { ref hash, ref path } => Items::GitCommitFile {
                hash: hash.clone(),
                path: path.clone(),
            },
            Target::GitLine { row } => {
                use git_block::diff_view::Row;
                let p = self.space.git()?.diff();
                let (file, hunk, line) = match *p.rows.get(row)? {
                    Row::Line { file, hunk, line } => (file, hunk, line),
                    // A pair: the selection's side, else the new line, else the old one.
                    Row::Pair {
                        file,
                        hunk,
                        old,
                        new,
                    } => {
                        let side = match (p.selection.is_some(), p.split_right) {
                            (true, true) => new,
                            (true, false) => old,
                            (false, _) => None,
                        };
                        (file, hunk, side.or(new).or(old)?)
                    }
                    _ => return None,
                };
                let s = &p.files[file];
                let h = &s.diff.hunks[hunk];
                let l = &h.lines[line];
                let hunk_text = h
                    .lines
                    .iter()
                    .map(|l| {
                        format!(
                            "{}{}",
                            l.kind.sign(),
                            l.text.strip_suffix('\r').unwrap_or(&l.text)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                let selection = p.selected_text();
                Items::GitLine {
                    working: p.working(),
                    remote: self.on_remote(),
                    path: s.diff.path().to_string(),
                    line: l.new_no.unwrap_or(h.new_start.max(1)),
                    send: self.send_to_menu(selection.clone().unwrap_or_else(|| hunk_text.clone())),
                    selection,
                    line_text: l.text.strip_suffix('\r').unwrap_or(&l.text).to_string(),
                    hunk_text,
                }
            }
            Target::GitMore => {
                let git = self.space.git()?;
                Items::GitMore {
                    busy: git.acting > 0,
                    // A mirror's trash is home's.
                    trash: self.trash.is_some() || self.on_remote(),
                    gates: Gates::of(self, &GIT_GATES),
                }
            }
            Target::GitRemotes {
                ref at,
                ref branch,
                ref remotes,
            } => Items::GitRemotes {
                at: at.clone(),
                branch: branch.clone(),
                remotes: remotes.clone(),
            },
            Target::GitFolders if self.on_remote() => Items::HomeGitFolders {
                space: self.space.id.clone(),
                folders: self.home_git_folders(),
                root: self.space.git().and_then(|g| match &g.found {
                    git_block::Found::Home(root) => Some(root.clone()),
                    _ => None,
                }),
                gates: Gates::of(self, &[Deed::GitLeftOut, Deed::AddTerminal]),
            },
            Target::GitFolders => Items::GitFolders {
                folders: self.git_folders(),
                folder: self.space.git().map(|g| {
                    g.repo()
                        .map_or_else(|| g.folder.clone(), |r| r.root.clone())
                }),
            },
            Target::GitTerminals if self.on_remote() => {
                let git_block::Found::Home(root) = &self.space.git()?.found else {
                    return None;
                };
                Items::Reveal {
                    terminals: self.home_terminals_in(root),
                }
            }
            Target::GitTerminals => Items::Reveal {
                terminals: self.terminal_list(Some(&self.space.git()?.repo()?.root)),
            },
            Target::Tree { block, ref path } => {
                let is_dir = path.as_ref().is_some_and(|p| p.is_dir());
                Items::Tree {
                    block,
                    folder: self.space.editor(block)?.folder.clone(),
                    path: path.clone(),
                    is_dir,
                    entries: path
                        .as_ref()
                        .filter(|_| is_dir)
                        .map_or(0, |p| std::fs::read_dir(p).map_or(0, Iterator::count)),
                }
            }
            Target::Space(ref id) if self.spaces().any(|s| s.id == *id && s.is_remote()) => {
                Items::Remote {
                    id: id.clone(),
                    active: self.space.id == *id,
                    live: self.spaces().any(|s| {
                        s.id == *id
                            && s.remote.as_deref().is_some_and(|r| {
                                matches!(
                                    r.status,
                                    templum_api::client::Status::Live
                                        | templum_api::client::Status::Reconnecting { .. }
                                )
                            })
                    }),
                }
            }
            Target::Space(ref id) => Items::Space {
                id: id.clone(),
                name: self
                    .spaces()
                    .find(|s| s.id == *id)
                    .map_or_else(String::new, |s| s.name.clone()),
                only_one: self.others.is_empty(),
                active: self.space.id == *id,
                stoppable: self
                    .spaces()
                    .find(|s| s.id == *id)
                    .is_some_and(|s| self.can_stop(s)),
                unsaved: self
                    .spaces()
                    .find(|s| s.id == *id)
                    .map_or_else(Vec::new, |s| s.unsaved(None)),
                busy: self
                    .spaces()
                    .find(|s| s.id == *id)
                    .map_or_else(Vec::new, |s| self.busy_in(s)),
            },
        })
    }
}

/// A Send to submenu: the text, and the terminals listed (see `Workspace::terminal_list`).
struct SendTo {
    text: String,
    to: Vec<(BlockId, String)>,
    /// A mirror that can't type: Send to ▸ stands dimmed, saying why.
    why: Option<SharedString>,
}

/// How a block's menu ends, before Close.
enum Tail {
    /// On the canvas: Fullscreen, for the block or the selection it was in; Connect to ▸ the
    /// blocks not wired to it and Disconnect from ▸ those that are, by title, back to front.
    Canvas {
        group: Vec<BlockId>,
        connect: Vec<(BlockId, SharedString)>,
        disconnect: Vec<(BlockId, SharedString)>,
    },
    /// In grid mode: new blocks (`agents` and `shells`, placed on the canvas at `at`, as the
    /// canvas's menu) and the blocks left on the canvas, taking half of the tile; flip the split
    /// (with a neighbour), leave the grid, back to the canvas.
    Tile {
        flip: bool,
        others: Vec<(BlockId, SharedString)>,
        at: Point,
        agents: AgentList,
        shells: ShellList,
    },
}

/// Why a mirrored workspace's menu items can't be used, by the deed each does: the rule's words,
/// read when the menu is built (a local workspace has none).
#[derive(Clone, Default)]
pub(crate) struct Gates(Vec<(Deed, SharedString)>);

impl Gates {
    /// The words of each of `deeds` the workspace shown can't do now.
    pub(crate) fn of(ws: &Workspace, deeds: &[Deed]) -> Gates {
        Gates(
            deeds
                .iter()
                .filter_map(|deed| Some((*deed, ws.why_not(*deed)?.into())))
                .collect(),
        )
    }

    /// Why `deed` can't be done, or None.
    pub(crate) fn why(&self, deed: Deed) -> Option<SharedString> {
        self.0
            .iter()
            .find(|(gated, _)| *gated == deed)
            .map(|(_, why)| why.clone())
    }
}

/// The agents a menu lists.
#[derive(Clone)]
enum AgentList {
    /// This computer's, and whether each one's program was found.
    Local(Vec<(AgentEntry, bool)>),
    /// A mirror's: home's, none while they are not read yet; and the computer's name.
    Home {
        agents: Option<Vec<HomeAgent>>,
        computer: String,
    },
}

/// What the shells' menus list: **Default (*name*)**, then the shells found other than it.
#[derive(Clone)]
struct Shells {
    default: String,
    others: Vec<ShellOption>,
}

/// The shells a menu lists.
#[derive(Clone)]
enum ShellList {
    /// This computer's.
    Local(Shells),
    /// A mirror's: home's, none while they are not read yet; the computer's name; and why home's
    /// shells other than its default can't open (the rule's words).
    Home {
        shells: Option<HomeShells>,
        computer: String,
        why: Option<SharedString>,
    },
}

/// A target's menu, with what it shows read from the workspace.
enum Items {
    /// A block's menu: its kind's, then the same end for every kind.
    Block {
        block: BlockId,
        inner: Box<Items>,
        tail: Tail,
        close: &'static str,
        /// What Move to workspace moves: the selection the block is in, else the block.
        moving: Vec<BlockId>,
        /// The other workspaces, by id and name, and whether they take blocks (not a stopped
        /// one, nor one whose file isn't written).
        spaces: Vec<(String, String, bool)>,
        /// The schedules its agent asked for, by id and as Cancel schedules ▸ lists them, and
        /// its terminal (`TEMPLUM_TERMINAL`).
        schedules: Vec<(u64, String)>,
        terminal: Option<String>,
        /// Connect to, Disconnect from, Move to workspace, Close and a tile's new blocks on a
        /// mirror.
        gates: Gates,
    },
    Canvas {
        at: Point,
        agents: AgentList,
        shells: ShellList,
        gates: Gates,
    },
    Shells(ShellList),
    Agents {
        at: Point,
        agents: AgentList,
    },
    Terminal {
        block: BlockId,
        selection: bool,
        /// A remote terminal with an agent in it: Hand a task….
        hand: bool,
        /// Paste and Hand a task… on a mirror.
        gates: Gates,
    },
    /// A card of home's block: nothing of its own to do.
    Mirrored,
    Kanban {
        block: BlockId,
        /// The board it shows: its id, its name and how many cards it has.
        board: Option<(u64, String, usize)>,
        /// All boards… and the board's own items on a mirror.
        gates: Gates,
    },
    Note {
        block: BlockId,
        /// The color of the note on screen; None without one.
        color: Option<Option<crate::board::Label>>,
        /// The note on screen's id and name; None without one.
        rename: Option<(u64, String)>,
        send: SendTo,
        /// Rename…, Color ▸ and All notes… on a mirror, whose notes are read-only.
        gates: Gates,
    },
    Checklist {
        block: BlockId,
        /// All checklists… on a mirror.
        gates: Gates,
    },
    Editor {
        block: BlockId,
        tree: bool,
        wrap: bool,
        send: SendTo,
    },
    /// The Git popup's folder menu: other folders to show, and the one shown (its repository's
    /// root, its folder while there is none).
    GitFolders {
        folders: Vec<PathBuf>,
        folder: Option<PathBuf>,
    },
    /// A mirror's folder menu: home's folders, as home writes them, and the shown repository's
    /// root at home.
    HomeGitFolders {
        /// The mirrored workspace the menu was opened on: a folder is for its Git only.
        space: String,
        folders: Vec<String>,
        root: Option<String>,
        gates: Gates,
    },
    GitMore {
        /// A command runs: Fetch, Pull and Push wait.
        busy: bool,
        /// Discards are kept (a data folder, or home's on a mirror): Recently discarded… is offered.
        trash: bool,
        gates: Gates,
    },
    GitRemotes {
        at: crate::workspace::GitAt,
        branch: String,
        remotes: Vec<String>,
    },
    GitCommitMenu {
        /// "Commit", or "Commit all changes (5 files)".
        label: String,
        /// Commit is possible (`Git::can_commit`).
        can: bool,
        /// Amend is on.
        amend: bool,
    },
    GitGenerateMenu {
        /// The agent's models, the recommended one first, and the one in use.
        models: Vec<String>,
        model: String,
    },
    GitRow {
        list: git_block::List,
        entry: Entry,
        /// A conflict's parts, named after its markers' labels (the whole-file choices).
        labels: (String, String),
        gates: Gates,
        /// On a mirror: Open (in an editor here) is left out.
        remote: bool,
    },
    GitCommit {
        commit: Commit,
        /// HEAD's commit, not pushed: Undo this commit.
        undo: bool,
        /// In a file's history, the file's path in this commit: Restore this version….
        restore: Option<String>,
        gates: Gates,
    },
    GitCommitFile {
        hash: String,
        path: String,
    },
    GitBranch {
        branch: Branch,
        current: Option<String>,
        /// The default branch: Compare with it.
        base: Option<String>,
        gates: Gates,
    },
    GitStash {
        index: usize,
        gates: Gates,
    },
    GitLine {
        /// The working changes' panel: its lines are the file's (a comparison's aren't).
        working: bool,
        /// On a mirror: Open in editor is left out.
        remote: bool,
        path: String,
        /// Where Open in editor goes: the line's new number, else its hunk's first.
        line: u32,
        /// The panel's selection, for Copy.
        selection: Option<String>,
        line_text: String,
        hunk_text: String,
        /// The selection, else the hunk.
        send: SendTo,
    },
    Column {
        board: u64,
        column: u64,
        title: String,
        cards: usize,
        /// A new card's draft is kept here: "Add a card" reads "Continue draft".
        draft: bool,
        /// Rename…, Delete and Add a card on a mirror.
        gates: Gates,
    },
    Card {
        board: u64,
        card: u64,
        send: SendTo,
        /// Delete card on a mirror.
        gates: Gates,
    },
    Space {
        id: String,
        name: String,
        only_one: bool,
        /// It is the one shown.
        active: bool,
        /// Stop applies: it isn't the last one, nor stopped already.
        stoppable: bool,
        /// Its editors' changed files.
        unsaved: Vec<String>,
        /// Its terminals at work, and why.
        busy: Vec<String>,
    },
    /// A remote workspace's tile: Open, Edit…, Disconnect (or Connect), Forget….
    Remote {
        id: String,
        active: bool,
        live: bool,
    },
    Ink {
        index: usize,
    },
    /// A wire: Rename wire…, Disconnect.
    Wire {
        wire: Wire,
        gates: Gates,
    },
    /// Terminals to show, one item each.
    Reveal {
        terminals: Vec<(BlockId, String)>,
    },
    Tree {
        block: BlockId,
        /// The editor's folder: where New file… goes from the empty space.
        folder: PathBuf,
        path: Option<PathBuf>,
        is_dir: bool,
        /// A folder's entries, for what Delete… says.
        entries: usize,
    },
}

/// The open menu, above everything, kept inside the window; the wheel doesn't reach what is
/// under it while it is open.
pub(crate) fn render(menu: &OpenMenu, window: &Window) -> AnyElement {
    let size = window.bounds().size;
    deferred(
        anchored().child(
            div()
                .w(size.width)
                .h(size.height)
                .on_scroll_wheel(|_: &ScrollWheelEvent, _, cx| cx.stop_propagation())
                .child(
                    anchored()
                        .position(menu.at)
                        .snap_to_window_with_margin(px(8.0))
                        .child(menu.view.clone()),
                ),
        ),
    )
    .with_priority(gpui_kit::base::POPUP_PRIORITY)
    .into_any_element()
}
