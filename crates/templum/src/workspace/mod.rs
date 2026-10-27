//! The main view: the canvas of terminal blocks.

mod activity;
mod agent;
pub(crate) mod agents_view;
pub(crate) mod api;
mod api_git;
mod api_state;
mod approvals;
pub(crate) mod automations;
mod autostart;
mod blocks;
mod bus;
mod checklist_blocks;
mod content;
mod control;
mod delegate;
mod drawing;
mod file_tree;
mod files;
mod find;
mod git_branches;
mod git_changes;
mod git_compare;
mod git_conflicts;
mod git_discard;
mod git_history;
mod git_home;
mod git_keys;
mod git_message;
mod git_ops;
mod git_switch;
mod git_sync;
mod history;
mod holds;
pub(crate) mod inbox;
mod input;
mod note_tabs;
mod ops;
mod persist;
mod preferences;
mod questions;
pub(crate) mod remote;
mod remote_boards;
mod remote_content;
mod remote_notes;
pub(crate) mod remote_rules;
mod render;
mod schedules;
mod selection;
mod spaces;
mod spawn;
pub(crate) mod starting;
pub(crate) mod subagents;
mod tiling;
mod update;
mod usage;
mod view;
mod waits;
mod wires;
mod worktrees;

pub(crate) use blocks::BoardAct;
pub(crate) use bus::templum_in;
pub(crate) use checklist_blocks::ChecklistAct;
pub(crate) use find::FindBar;
pub(crate) use git_message::TryIt;
pub(crate) use waits::{Approved, Wait, Waiting};

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, App, Bounds, Context, FocusHandle, Focusable as _, KeyBinding, Keystroke, Pixels,
    Point as PixelPoint, SharedString, Subscription, Task, UniformListScrollHandle, Window,
    WindowAppearance, px,
};
use templum_canvas::{
    Block, BlockId, Camera, CameraFlight, Canvas, Color, Drawing, Edges, Gesture, Guide, Point,
    Rect, ShapeKind, Size, Tile, Wire, chrome, free_spot, room_at,
};
use templum_git::GitError;
use templum_terminal::{Notify, Snapshot, Terminal};

use crate::agents::{self, AgentEntry};
use crate::alert::Alert;
use crate::autostart::Autostart;
use crate::board::Boards;
use crate::checklist::{self, Checklist};
use crate::checklists::{Checklists, List};
use crate::config::{self, Config, Edit, ThemeConfig};
use crate::editor::{self, Editor};
use crate::git_block::{self, Git};
use crate::grid::CellMetrics;
use crate::ink;
use crate::kanban::{self, Kanban};
use crate::keys::{Command, Keymap};
use crate::menu;
use crate::note::{self, Note};
use crate::session;
use crate::settings::Panel;
use crate::theme::{self, Entry, Theme, Ui};
use crate::theme_picker::Picker;

pub(crate) use git_ops::{Extra, notify_git_error, notify_git_failure, notify_with};

pub(crate) const DEFAULT_COLS: u16 = 80;
pub(crate) const DEFAULT_ROWS: u16 = 24;
/// An agent's terminal: agents draw wide interfaces.
const AGENT_COLS: u16 = 110;
const AGENT_ROWS: u16 = 32;
/// How many folders a workspace remembers for agents.
pub(crate) const RECENT: usize = 8;
/// The largest block a saved canvas restores: a corrupt size mustn't allocate a giant grid.
pub(super) const MAX_COLS: u16 = 2000;
pub(super) const MAX_ROWS: u16 = 1000;
/// A press that moves less than this (screen pixels) before release is a click, not a pan.
const CLICK_SLOP: f32 = 4.0;
/// Drawing changes kept for undo, per workspace.
pub(crate) const INK_UNDO: usize = 50;
/// Block changes kept for Ctrl+Z, per workspace; and as many undone ones.
const CANVAS_UNDO: usize = 100;
/// Board changes kept for undo, for the whole app (the boards are).
pub(crate) const BOARD_UNDO: usize = 50;
/// Checklist changes kept for undo, for the whole app (the checklists are).
const CHECKLIST_UNDO: usize = 50;
/// Cursor blink half-period.
const BLINK: Duration = Duration::from_millis(500);
/// Quitting takes a second press of its shortcut within this long.
const QUIT_AGAIN: Duration = Duration::from_secs(2);
/// A settings change is written to config.toml this long after the last one.
const SAVE_DELAY: Duration = Duration::from_millis(300);

pub fn bind_keys(cx: &mut App) {
    // gpui-kit's Root binds tab/shift-tab to focus navigation: give them back to the shell.
    cx.bind_keys([
        KeyBinding::new("tab", gpui_kit::NoAction {}, Some("Root")),
        KeyBinding::new("shift-tab", gpui_kit::NoAction {}, Some("Root")),
    ]);
    // Templum's own shortcuts are matched by the workspace's keymap, so config.toml can change
    // them.
}

/// What an agent's hooks last said of its turn (docs/specs/connections.md, Hooks).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AgentState {
    Working,
    /// Done with its turn, or not started on one.
    Idle,
    /// Waiting for the person, with what it asks.
    NeedsYou(Option<String>),
}

/// A task handed to a terminal's agent (`delegate`, or `spawn_agent`'s prompt), while its answer
/// is awaited (docs/specs/connections.md, Delegating).
pub(crate) struct Handed {
    /// Its id, said `t7`; counts from 1 each time Templum starts (`Workspace::next_task`); 0 for
    /// one made with `new`.
    pub id: u64,
    /// The terminal of the agent that handed it over.
    pub by: String,
    pub since: Instant,
    /// After this it is no longer awaited: its dot stops, and another task may be handed over.
    pub until: Instant,
    /// The task as its sender wrote it, cut to `Handed::TEXT_MAX` characters (the inbox shows its
    /// first lines as why the agent asks); empty for one made with `new`.
    pub text: String,
    /// Its agent took it (`Submitted`, the first after it was typed): only now may a turn's end
    /// answer it.
    pub taken: bool,
    /// Claude Code queued it behind the turn under way (`Submitted` at its Enter while it worked):
    /// that turn's end isn't its answer, unless a tool's end takes it into the turn first.
    pub behind: bool,
    /// Its master was told its agent went quiet (a still screen, no answer): once per task; a
    /// change from the master starts it over (docs/specs/agent-protocol.md, 5).
    pub quiet: bool,
    /// Its agent's usage when it was handed over: what its end says it spent counts from it.
    pub usage: Option<templum_usage::Session>,
    /// The Claude Code subagents its agent had when it took it: their reports come in turns of
    /// their own, so a turn's end with only them left is its answer
    /// (docs/specs/claude-subagents.md).
    pub subagents: Vec<String>,
}

impl Handed {
    /// The most characters of a task's text kept.
    const TEXT_MAX: usize = 2000;

    /// `task` as `text` keeps it.
    pub(crate) fn text_of(task: &str) -> String {
        task.chars().take(Self::TEXT_MAX).collect()
    }

    /// Handed over by the terminal `by` now, awaited for `timeout`; not taken yet.
    pub(crate) fn new(by: &str, timeout: Duration) -> Self {
        let since = Instant::now();
        Self {
            id: 0,
            by: by.to_string(),
            since,
            until: since + timeout,
            text: String::new(),
            taken: false,
            behind: false,
            quiet: false,
            usage: None,
            subagents: Vec::new(),
        }
    }
}

/// One of Claude Code's subagents, which Templum heard start (docs/specs/claude-subagents.md).
pub(crate) struct Subagent {
    pub id: String,
    /// It finished in the background: its report comes to its agent in a turn of its own,
    /// not yet begun.
    pub reported: bool,
}

/// A prompt `spawn_agent` types into its agent once the agent is ready
/// (docs/specs/connections.md, Spawning).
pub(crate) struct Prompt {
    pub text: String,
    /// The agent's screen at the last look, once its process was seen: the same at the next look,
    /// the agent is ready.
    pub screen: Option<String>,
    /// Its agent is Claude Code or Codex, whose prompt also waits for its input box
    /// (`Modes::typing`): their trust questions come first.
    pub gated: bool,
    /// A task `delegate` handed the agent while it started, not its spawn's prompt.
    pub delegated: bool,
}

impl Prompt {
    /// `text`, for an agent built on `builtin`.
    pub(crate) fn new(text: String, builtin: Option<templum_terminal::Agent>) -> Self {
        use templum_terminal::Agent;
        Self {
            text,
            screen: None,
            gated: matches!(builtin, Some(Agent::CLAUDE | Agent::CODEX)),
            delegated: false,
        }
    }
}

pub(crate) struct Pane {
    pub term: Terminal,
    pub snapshot: Rc<RefCell<Snapshot>>,
    /// The terminal's finished and attention counts when last heard (`Workspace::hear`).
    pub heard: (u64, u64),
    /// News nobody has looked at yet; gone once the block is focused in the active window.
    pub alert: Option<Alert>,
    /// Its folder and its agent as last read: at most a snapshot old (5 s), cheap to ask every
    /// frame. The folder is the start folder until then, and follows each finished command.
    pub seen: templum_terminal::Inspection,
    /// Its agent's session, as last read after a snapshot (docs/specs/usage.md).
    pub usage: Option<templum_usage::Session>,
    /// Its find field, while open.
    pub find: Option<Box<find::FindBar>>,
    /// Its agent's turn, as the agent's hooks last said; None until one speaks, and after its
    /// session ends.
    pub state: Option<AgentState>,
    /// The task another agent handed its agent, while its answer is awaited.
    pub handed: Option<Handed>,
    /// Its agent's last answer: the message its last turn ended with, or its last `reply`.
    pub answer: Option<String>,
    /// The prompt `spawn_agent` types once its agent is ready.
    pub prompt: Option<Prompt>,
    /// The looks at its screen: its agent's state where its hooks say nothing (`activity.rs`).
    pub looks: activity::Looks,
    /// The person typed into it since the last look, which then tells nothing.
    pub typed: Cell<bool>,
    /// When the person last typed into it (a key, a paste, text): a second Enter for a task
    /// would send their half-written line.
    pub keyed: Cell<Option<Instant>>,
    /// Text the person typed or pasted into its agent's input and hasn't sent: no notice is
    /// typed over it, however long ago the last key was (`Pane::person_keyed`).
    pub draft: Cell<bool>,
    /// The person scrolled it, or its program heard their mouse, since the last look, which then
    /// tells nothing.
    pub scrolled: Cell<bool>,
    /// How many prompts its agent took (`Submitted`): a task's Enter is confirmed by one more.
    pub submitted: u64,
    /// How many times its agent began to wait for its person (a dialog, or it seemed stuck): two
    /// dialogs in the same words ("Allow Bash?" for every command) are two.
    pub dialogs: u64,
    /// Its agent's hooks say when a turn starts and when it asks its person (Claude Code's,
    /// Gemini CLI's, and Codex's once its person trusted them), not only when a turn ends: a
    /// dialog of it would be heard.
    pub speaks: bool,
    /// Who pressed keys in it last (`send_keys`, `interrupt`), as its `[from …]` line names
    /// them, and when.
    pub keys_from: Option<(String, Instant)>,
    /// The block id of the agent that opened it with `spawn_agent`, which may close it; none for
    /// one the person opened or pasted.
    pub opened_by: Option<String>,
    /// The model and the effort `spawn_agent` gave its agent, typed again when it resumes; gone
    /// with that agent.
    pub model: Option<String>,
    pub effort: Option<String>,
    /// The agent `model` and `effort` were typed for, by id: another found there isn't given
    /// them, also when no snapshot found this one (it never started, or quit at once).
    pub flags_for: Option<String>,
    /// When Templum last typed into it (a task, a reply, a notice): a notice waits NOTICE_GAP
    /// after, while that text's Enter, and a second one, may still be due.
    pub sent: Option<Instant>,
    /// The transcript its agent's last turn's end named, until its next session starts.
    pub transcript: Option<String>,
    /// Its agent starting, under a cover, until its screen shows (`starting.rs`).
    pub starting: Option<starting::Starting>,
    /// When Templum last typed its agent's command (a spawn, a restore, the launcher): one no
    /// snapshot found within STARTS_WITHIN of it never started.
    pub typed_agent: Option<Instant>,
    /// It came back with its saved canvas: a shell that ends as it starts couldn't start
    /// (`Pane::died_starting`).
    pub restored: bool,
    /// Its saved item, whose agent Templum typed back, since when: until a snapshot finds an
    /// agent there, at most STARTS_WITHIN, snapshots keep its folder, agent and session.
    pub resuming: Option<(Instant, session::TerminalItem)>,
    /// The account whose variable its shell has, by id (docs/specs/accounts.md, Its block): its
    /// agent found there is that account.
    pub account: Option<String>,
    /// The role `spawn_agent` opened it as (docs/specs/agent-tools.md, Roles), kept with it.
    pub role: Option<String>,
    /// When its last agent started, and when a snapshot found that agent gone: `stop_agent` still
    /// ends what it left (docs/specs/agent-tools.md#stop_agent), until the person types into it
    /// or a snapshot finds another agent there.
    pub quit: Option<(u64, Instant)>,
    /// The shell chosen for it in a menu (docs/specs/shells.md, The block keeps its shell); none:
    /// it runs the default, and follows it.
    pub shell: Option<templum_terminal::ShellOption>,
    /// Its agent's subagents at work, or done with a report its agent hasn't taken yet
    /// (docs/specs/claude-subagents.md): while there are any, a turn's end isn't its answer.
    pub subagents: Vec<Subagent>,
    /// Its agent's last turn ended while it had subagents, and none started since: a prompt
    /// typed now starts a turn, it isn't queued behind one.
    pub awaits_subagents: bool,
    /// The subagent whose dialog its agent waits on its person for (`NeedsYou`), by id; None:
    /// its own. Another's tool ending doesn't close that dialog.
    pub asked_by: Option<String>,
    /// Its agent's turn ended with only reported subagents left: unless a turn starts first, they
    /// are forgotten then, and that turn's end answers (docs/specs/claude-subagents.md).
    pub settles: Option<Instant>,
    /// The Claude Code subagent it shows (`templum follow`): read-only, never saved.
    pub follows: Option<subagents::Follows>,
    /// What the Agents view keeps of its agent between frames (docs/specs/agents-view.md).
    pub watch: agents_view::Watch,
    /// Home's block it mirrors, when it is a remote workspace's viewer (`remote.rs`): its agent
    /// is what home says, never what this computer's hooks and processes find.
    pub remote: Option<RemotePane>,
}

/// What a remote workspace's terminal keeps of home's block.
pub(crate) struct RemotePane {
    /// Home's block id.
    pub block: String,
    /// Its title at home, which the viewer's own follows only while it is streamed.
    pub title: String,
    /// The agent in it, as home's state tells it.
    pub agent: Option<templum_api::model::AgentView>,
    /// It takes no input (a subagent's transcript).
    pub read_only: bool,
}

impl Pane {
    /// Its agent waits on its person for a dialog of `by`: one of its subagents, by id, or
    /// itself (None).
    pub fn asks(&self, by: Option<&str>) -> bool {
        matches!(self.state, Some(AgentState::NeedsYou(_))) && self.asked_by.as_deref() == by
    }

    /// What a remote agent's dot says while it works, with its subagents as home counts them.
    pub fn remote_working(&self) -> String {
        match self
            .remote
            .as_ref()
            .and_then(|r| r.agent.as_ref())
            .map_or(0, |a| a.subagents)
        {
            0 => "Working".to_string(),
            1 => "Working: 1 subagent".to_string(),
            n => format!("Working: {n} subagents"),
        }
    }

    /// How many of its agent's subagents work.
    pub fn subagents_at_work(&self) -> usize {
        self.subagents.iter().filter(|s| !s.reported).count()
    }

    /// What its dot says while its agent works: how many of its subagents do too.
    pub fn working(&self) -> String {
        match self.subagents_at_work() {
            0 => "Working".to_string(),
            1 => "Working: 1 subagent".to_string(),
            n => format!("Working: {n} subagents"),
        }
    }

    /// Its agent's subagents but those `keep` keeps: a dialog one of those gone asked is over.
    pub fn keep_subagents(&mut self, keep: impl Fn(&Subagent) -> bool) {
        self.subagents.retain(|s| keep(s));
        if let Some(by) = self.asked_by.clone()
            && !self.subagents.iter().any(|s| s.id == by)
        {
            self.dialog_over(&by);
        }
    }

    /// The dialog its agent's subagent `id` asked its person is over, if it asked one.
    pub fn dialog_over(&mut self, id: &str) {
        if self.asked_by.as_deref() == Some(id) {
            if self.asks(Some(id)) {
                self.state = Some(AgentState::Working);
            }
            self.asked_by = None;
        }
    }

    /// Its agent's subagent `id` is at work, as one heard to start or one a turn's end names.
    pub fn add_subagent(&mut self, id: String) {
        match self.subagents.iter_mut().find(|s| s.id == id) {
            // Resumed: at work again.
            Some(subagent) => subagent.reported = false,
            None => self.subagents.push(Subagent {
                id,
                reported: false,
            }),
        }
    }

    /// Its agent's turn ended while its subagents work or a report of one is still to come: it
    /// works on, and the turn's end answers nothing (docs/specs/claude-subagents.md). A task
    /// queued behind that turn starts now: the next turn's end is its own.
    pub fn await_subagents(&mut self) {
        if !matches!(self.state, Some(AgentState::NeedsYou(_))) {
            self.state = Some(AgentState::Working);
        }
        match self.handed.as_mut().filter(|h| h.behind) {
            Some(handed) => handed.behind = false,
            None => self.awaits_subagents = true,
        }
    }

    /// The agent it runs, by id: the one Templum sees running in it, else the one Templum
    /// started there (before the first snapshot read it).
    pub fn agent(&self) -> Option<&str> {
        if let Some(remote) = &self.remote {
            return remote.agent.as_ref().map(|a| a.agent.as_str());
        }
        self.seen.agent.as_deref().or(self.flags_for.as_deref())
    }

    /// Its saved item at `rect`, with the block id `uid`: its folder and agent as last seen (a
    /// snapshot reads them in the background), no session (pasted, it resumes no agent).
    pub fn item(&self, rect: templum_canvas::Rect, uid: String) -> session::TerminalItem {
        session::TerminalItem {
            uid,
            rect: rect.into(),
            folder: self.seen.folder.clone().and_then(session::utf8),
            agent: self.seen.agent.clone(),
            session: None,
            opened_by: self.opened_by.clone(),
            model: self.model.clone(),
            effort: self.effort.clone(),
            account: self.account.clone(),
            role: self.role.clone(),
            // The label: the file never holds a program to run (persistence.md, Safety).
            shell: self.shell.as_ref().map(|s| s.label.clone()),
        }
    }

    /// The columns and rows of its grid now. A remote viewer's grid is home's, which the client
    /// resizes from another thread: the grid's own size, not the one the viewer was made with.
    pub fn grid_size(&self) -> (u16, u16) {
        if self.remote.is_some() {
            self.term.shared().grid_size()
        } else {
            self.term.grid_size()
        }
    }

    /// Its terminal resized to `cols` × `rows`: its program redraws at the new size, which the
    /// next look at its screen mustn't take for its agent at work.
    pub fn fit(&mut self, cols: u16, rows: u16, cell_px: (u16, u16)) {
        // A remote viewer keeps home's grid: the block's size is not its program's.
        if self.remote.is_some() {
            return;
        }
        if self.term.grid_size() != (cols, rows) {
            self.scrolled.set(true);
        }
        self.term.resize(cols, rows, cell_px);
    }

    /// Its folder, as last seen, is `root` or inside it.
    pub fn works_in(&self, root: &std::path::Path) -> bool {
        self.seen
            .folder
            .as_ref()
            .is_some_and(|f| f.starts_with(root))
    }

    /// When the agent `stop_agent` ends what was left after started: the one the last snapshot
    /// found, else the last one, gone, while the person hasn't typed into it since.
    pub fn agent_since(&self) -> Option<u64> {
        let typed_after = |gone| self.keyed.get().is_some_and(|at| at > gone);
        self.seen.started.or(self
            .quit
            .filter(|(_, gone)| !typed_after(*gone))
            .map(|(started, _)| started))
    }
}

/// The Git of a workspace, by its id: a result that comes back after a switch still finds it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GitAt {
    pub space: String,
    /// Its `Git::generation` when the command started: the result of one started before it
    /// changed folder is dropped.
    pub generation: u64,
    /// The other computer's name when this is a mirrored workspace's Git: its failures are told
    /// in that computer's words (a notification is built where the workspace can't be read).
    pub home: Option<String>,
}

/// A closed block, still drawn while it fades out. A terminal's shell lives until then.
pub(crate) struct Ghost {
    pub block: Block,
    pub body: Body,
    since: Instant,
}

/// What a block holds. Every block on a canvas is one of these.
pub(crate) enum Body {
    /// Boxed: what a terminal keeps for its agent outweighs the other kinds.
    Terminal(Box<Pane>),
    Kanban(Kanban),
    Note(Note),
    Checklist(Checklist),
    /// Boxed: on Windows, where a path takes more bytes, it is over 200 bytes larger than a note.
    Editor(Box<Editor>),
    /// A block of a remote workspace that this computer doesn't draw yet: a card in its place.
    Mirrored(Mirrored),
}

/// What a card of a block "not mirrored yet" says.
pub(crate) struct Mirrored {
    /// Home's kind of block (`note`, `kanban`, `checklist`, `editor`, or one this Templum
    /// doesn't know).
    pub kind: String,
    pub title: String,
}

impl Body {
    pub fn terminal(&self) -> Option<&Pane> {
        match self {
            Body::Terminal(pane) => Some(pane),
            _ => None,
        }
    }

    pub fn terminal_mut(&mut self) -> Option<&mut Pane> {
        match self {
            Body::Terminal(pane) => Some(pane),
            _ => None,
        }
    }

    /// A kanban of this computer's boards: a mirrored workspace's has home's ids, which no local
    /// lookup by board id may meet.
    pub fn kanban(&self) -> Option<&Kanban> {
        match self {
            Body::Kanban(kanban) if !kanban.remote => Some(kanban),
            _ => None,
        }
    }

    pub fn kanban_mut(&mut self) -> Option<&mut Kanban> {
        match self {
            Body::Kanban(kanban) if !kanban.remote => Some(kanban),
            _ => None,
        }
    }

    /// A checklist of this computer's: a mirrored workspace's has home's ids, which no local
    /// lookup by checklist id may meet (`checklist_view` answers any).
    pub fn checklist(&self) -> Option<&Checklist> {
        match self {
            Body::Checklist(checklist) if !checklist.remote => Some(checklist),
            _ => None,
        }
    }

    pub fn checklist_mut(&mut self) -> Option<&mut Checklist> {
        match self {
            Body::Checklist(checklist) if !checklist.remote => Some(checklist),
            _ => None,
        }
    }

    /// Any checklist block, a mirrored workspace's too: for what only draws it and acts on its
    /// own copy.
    pub fn checklist_view(&self) -> Option<&Checklist> {
        match self {
            Body::Checklist(checklist) => Some(checklist),
            _ => None,
        }
    }

    pub fn checklist_view_mut(&mut self) -> Option<&mut Checklist> {
        match self {
            Body::Checklist(checklist) => Some(checklist),
            _ => None,
        }
    }

    /// The smallest block of this kind.
    pub fn min_size(&self, cell: Size) -> Size {
        match self {
            Body::Terminal(_) => chrome::min_block_size(cell),
            Body::Kanban(_) => kanban::MIN,
            Body::Note(_) => note::MIN,
            Body::Checklist(_) => checklist::MIN,
            Body::Editor(_) => editor::MIN,
            Body::Mirrored(_) => note::MIN,
        }
    }

    /// A note block of this computer's: a mirrored workspace's has home's note ids, which no local
    /// lookup by note id may meet (`note_view` answers any).
    pub fn note(&self) -> Option<&Note> {
        match self {
            Body::Note(note) if !note.remote => Some(note),
            _ => None,
        }
    }

    pub fn note_mut(&mut self) -> Option<&mut Note> {
        match self {
            Body::Note(note) if !note.remote => Some(note),
            _ => None,
        }
    }

    /// Any note block, a mirrored workspace's too.
    pub fn note_view(&self) -> Option<&Note> {
        match self {
            Body::Note(note) => Some(note),
            _ => None,
        }
    }

    pub fn note_view_mut(&mut self) -> Option<&mut Note> {
        match self {
            Body::Note(note) => Some(note),
            _ => None,
        }
    }

    pub fn editor(&self) -> Option<&Editor> {
        match self {
            Body::Editor(editor) => Some(editor),
            _ => None,
        }
    }

    pub fn editor_mut(&mut self) -> Option<&mut Editor> {
        match self {
            Body::Editor(editor) => Some(editor),
            _ => None,
        }
    }

    /// Gives the keyboard to what types inside the block: a note's editor, an editor's open
    /// file. False when nothing does.
    pub fn focus_inner(&self, window: &mut Window, cx: &mut App) -> bool {
        match self {
            Body::Note(note) => note.focus(window, cx),
            Body::Editor(e) => e.focus(window, cx),
            _ => false,
        }
    }

    /// The folder a terminal opened from this block starts in: a terminal's current one, an
    /// editor's. None for a note or a kanban block.
    pub fn folder(&self) -> Option<PathBuf> {
        match self {
            Body::Terminal(pane) => pane.term.cwd(),
            Body::Editor(e) => Some(e.folder.clone()),
            Body::Kanban(_) | Body::Note(_) | Body::Checklist(_) | Body::Mirrored(_) => None,
        }
    }

    /// The block as a saved item at `rect`. A terminal's folder and agent are those last seen (a
    /// snapshot reads them in the background).
    pub fn item(&self, rect: templum_canvas::Rect) -> session::Item {
        match self {
            Body::Terminal(pane) => session::Item::Terminal(pane.item(rect, String::new())),
            Body::Kanban(k) => session::Item::Kanban(session::KanbanItem {
                uid: String::new(),
                rect: rect.into(),
                board: k.board,
            }),
            Body::Note(n) => session::Item::Note(session::NoteItem {
                uid: String::new(),
                rect: rect.into(),
                notes: n.ids(),
                active: n.active,
                text: String::new(),
                color: None,
            }),
            Body::Checklist(c) => session::Item::Checklist(session::ChecklistItem {
                uid: String::new(),
                rect: rect.into(),
                checklist: c.list,
                checklists: Vec::new(),
                active: 0,
            }),
            Body::Editor(e) => session::Item::Editor(e.item(rect)),
            // Never saved: home has it (`Space::layout` skips a remote workspace).
            Body::Mirrored(_) => session::Item::Note(session::NoteItem {
                uid: String::new(),
                rect: rect.into(),
                notes: Vec::new(),
                active: 0,
                text: String::new(),
                color: None,
            }),
        }
    }

    /// The keyboard is inside the block.
    pub fn has_focus(&self, window: &Window, cx: &App) -> bool {
        match self {
            Body::Note(note) => {
                note.tabs.iter().any(|t| {
                    t.editor
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                }) || note.renaming.as_ref().is_some_and(|r| {
                    r.input
                        .read(cx)
                        .focus_handle(cx)
                        .contains_focused(window, cx)
                })
            }
            Body::Editor(e) => e.has_focus(window, cx),
            Body::Terminal(pane) => pane.find.as_ref().is_some_and(|b| b.has_focus(window, cx)),
            Body::Checklist(c) => c.has_focus(window, cx),
            Body::Kanban(_) | Body::Mirrored(_) => false,
        }
    }
}

enum Drag {
    Canvas(Gesture),
    /// A block, or every selected block, held by its title (`gesture` is a `Move` or `MoveGroup`):
    /// pressed at `from`, the pointer now at `pointer` (canvas points). Near the canvas's edge,
    /// once it is past a click, the camera moves, since when the last frame moved it (`scrolled`).
    /// A selection held by a selected block's body (`picks`) takes the keyboard to that block when
    /// let go in place, unless the wheel or the canvas's edge `carried` it meanwhile.
    Held {
        gesture: Gesture,
        from: Point,
        pointer: Point,
        scrolled: Option<Instant>,
        picks: Option<BlockId>,
        carried: bool,
    },
    /// Selecting text in a block's terminal.
    Select(BlockId),
    /// A press the program in a terminal heard (it asked for the mouse): its moves, at another
    /// `cell` (row, column), and its release go to the program too.
    Program {
        id: BlockId,
        button: templum_terminal::Button,
        cell: (usize, usize),
    },
    /// Drawing with a pen, a highlighter or a shape tool.
    Ink(Drawing),
    /// Erasing: whether anything was erased yet (for undo), and the pointer's last world point.
    Erase {
        erased: bool,
        last: Point,
    },
    /// Selecting blocks with a rectangle from `from` to `to` (world points): the blocks it touches
    /// join those selected before (`base`). `pointer` is where the pointer is on the canvas; near
    /// its edge the camera moves, since when the last frame moved it (`scrolled`).
    Marquee {
        base: Vec<BlockId>,
        from: Point,
        to: Point,
        pointer: Point,
        scrolled: Option<Instant>,
    },
    /// Grid mode: a tile held by its title, from the canvas point `from` (screen units, as grid
    /// mode's camera is), the pointer now `at`; and the tile under it it would trade places with
    /// on release, once it moved past a click.
    Swap {
        id: BlockId,
        over: Option<BlockId>,
        from: Point,
        at: Point,
    },
    /// Moving a selected drawing: `last` is the pointer's previous world point; `moved` once it
    /// has moved (and the undo step is taken).
    MoveInk {
        index: usize,
        last: Point,
        moved: bool,
    },
    /// The Wire tool pressed on block `from`; the pointer is at the world point `to`.
    Wire {
        from: BlockId,
        to: Point,
    },
    /// A popup's top or right edge (a corner is both) pressed at `from` (canvas points) while the
    /// popup was drawn at `start`; the others rest against the sidebar and the window.
    GitResize {
        edges: Edges,
        from: Point,
        start: Size,
    },
}

impl Drag {
    /// A title pressed at the canvas point `at`.
    fn held(gesture: Gesture, at: Point, picks: Option<BlockId>) -> Self {
        Drag::Held {
            gesture,
            from: at,
            pointer: at,
            scrolled: None,
            picks,
            carried: false,
        }
    }
}

/// A workspace in grid mode (docs/specs/grid.md): some of its blocks tiled over its area, at
/// zoom one. Meanwhile their canvas rects are their tiles'; their rects on the canvas, and the
/// canvas's camera, wait here.
pub(crate) struct Grid {
    pub tiles: Tile,
    /// Each tiled block's rect on the canvas.
    pub homes: HashMap<BlockId, Rect>,
    /// The canvas's camera.
    pub camera: Camera,
    /// What the tiles fill, as last drawn: the workspace's area inside the gap.
    pub area: Rect,
    /// Tiles sliding to the places they traded: where each was, since when.
    pub slides: HashMap<BlockId, (Rect, Instant)>,
}

/// What a selection copied as the button was let go shows (`block.rs`): in which terminal,
/// where it was let go (world units from the top-left of its body), how many lines with text went,
/// and since when (the executor's clock).
pub(crate) struct Copied {
    pub block: BlockId,
    pub at: Point,
    pub lines: usize,
    pub since: Instant,
}

/// One canvas and everything on it: its blocks, their terminals, the focus, and what is
/// animating.
#[derive(Default)]
pub(crate) struct Space {
    /// Names its file; chosen once, never changed.
    pub id: String,
    pub name: String,
    /// Its icon, color and image.
    pub look: crate::space_dialog::Look,
    /// Where its new terminals start.
    pub folder: Option<PathBuf>,
    /// Its programs were ended and its blocks dropped; `saved` holds its canvas, which opening
    /// it brings back.
    pub stopped: bool,
    pub canvas: Canvas,
    /// What each block of the canvas holds. Inserted and removed together with `canvas`'s blocks.
    pub blocks: HashMap<BlockId, Body>,
    /// Each block's id for agents and wires (docs/specs/connections.md): unique in every
    /// workspace, kept by restarts, undo and moves.
    pub uids: HashMap<BlockId, String>,
    pub focused: Option<BlockId>,
    /// The selected blocks (Shift+drag, Shift+click on a title): Delete closes them, Copy and
    /// Paste copy them. Never the focused one: selecting takes the focus away.
    pub selected: Vec<BlockId>,
    /// True until the user first moves the camera or touches a block: meanwhile the startup
    /// terminal stays centered, since tiling compositors resize the window after it opens.
    pub pristine: bool,
    /// The canvas as last read or written, to write only what changed.
    pub saved: Option<session::Saved>,
    /// Its file is kept as it is this session: it couldn't be read, or its terminals couldn't
    /// all start, and writing would lose what it holds.
    pub frozen: bool,
    /// The block under the pointer: it shows its close button and resize grip.
    pub hovered: Option<BlockId>,
    /// The snaps in effect during a drag, drawn as lines; empty otherwise.
    pub guides: Vec<Guide>,
    /// The camera drawn this frame: `canvas.camera` (where it is going), or a frame of `flight`.
    pub view: Camera,
    /// The view gliding to `canvas.camera`, since when.
    pub flight: Option<(CameraFlight, Instant)>,
    /// The zoom the view last came to rest at. Blocks are drawn at its level of detail, so a
    /// glide keeps the level it started at, and the level changes on the frame the glide ends.
    pub rest: f32,
    /// Blocks still popping in, since when.
    pub appearing: HashMap<BlockId, Instant>,
    /// Closed blocks still fading out.
    pub ghosts: Vec<Ghost>,
    /// The "Copied" an automatic copy shows, until motion::COPIED is over.
    pub copied: Option<Copied>,
    /// The block a double-click on its title framed, and the camera before: double-clicking it
    /// again goes back there.
    pub framed: Option<(BlockId, Camera)>,
    /// The folders agents were opened in on this canvas, newest first.
    pub recent: Vec<PathBuf>,
    /// Its Git (docs/specs/git.md): one repository, shown in the Git popup. None until the
    /// popup first opens here. Boxed: the editors of its diff and commit form are large.
    pub git: Option<Box<Git>>,
    /// The drawings, above the blocks.
    pub ink: Vec<Drawing>,
    /// The selected drawing, by its index in `ink`.
    pub selected_ink: Option<usize>,
    /// The wires between its blocks (docs/specs/connections.md), oldest first.
    pub wires: Vec<Wire>,
    /// The selected wire: Delete removes it. Never with a block focused or selected.
    pub selected_wire: Option<Wire>,
    /// The wires' labels, by the block ids (`uids`) of their ends, the smaller first. A label
    /// stays when its wire goes: Ctrl+Z bringing the wire back, or its closed block, brings it
    /// back too. Only the wires there are save theirs.
    pub labels: HashMap<(String, String), String>,
    /// The changes that take back each change of the blocks (added, closed, moved, resized), by
    /// their step numbers, newest last, at most CANVAS_UNDO; and those of the undone ones.
    pub undo: Vec<(u64, history::Change)>,
    pub redo: Vec<(u64, history::Change)>,
    /// The last step's number.
    pub steps: u64,
    /// `ink` before each change, newest last, at most INK_UNDO; and the undone ones.
    pub ink_undo: Vec<Vec<Drawing>>,
    pub ink_redo: Vec<Vec<Drawing>>,
    /// Grid mode, while it is on.
    pub grid: Option<Grid>,
    /// It mirrors a workspace of another computer; its canvas is never saved (`remote.rs`).
    pub remote: Option<Box<remote::RemoteSpace>>,
}

impl Space {
    /// Block `id` is drawn: on the canvas every block is; in grid mode, the tiled ones.
    pub fn shown(&self, id: BlockId) -> bool {
        self.grid.as_ref().is_none_or(|g| g.homes.contains_key(&id))
    }

    /// The canvas's camera, also while grid mode puts its own in its place.
    pub fn camera(&self) -> Camera {
        self.grid.as_ref().map_or(self.canvas.camera, |g| g.camera)
    }

    /// A block's rect on the canvas, also while it is a tile.
    pub fn home(&self, block: &Block) -> Rect {
        self.grid
            .as_ref()
            .and_then(|g| g.homes.get(&block.id))
            .copied()
            .unwrap_or(block.rect)
    }

    /// A spot for a block of `size` beside `anchors` (a block, then those wired to it) that keeps
    /// off every block and drawing ([`free_spot`]): to the right of a lone anchor, top-aligned, when
    /// nothing is there; down the column its wired blocks already started.
    pub(super) fn room_beside(&self, anchors: &[Rect], size: Size) -> Rect {
        free_spot(&self.taken(), anchors, size)
    }

    /// A spot for a block of `size` centered on `center` (the view's middle), or beside the
    /// blocks there, level with one ([`room_at`]).
    pub(super) fn room_around(&self, center: Point, size: Size) -> Rect {
        let at = Rect::new(
            center.x - size.w / 2.0,
            center.y - size.h / 2.0,
            size.w,
            size.h,
        );
        room_at(&self.taken(), at)
    }

    /// What a new block keeps off: every block where it sits on the canvas (also in grid mode),
    /// and every drawing.
    fn taken(&self) -> Vec<Rect> {
        (self.canvas.blocks().iter().map(|b| self.home(b)))
            .chain(self.ink.iter().map(Drawing::bounds))
            .collect()
    }

    /// The block on top among those drawn.
    pub fn top(&self) -> Option<BlockId> {
        self.canvas
            .blocks()
            .iter()
            .rev()
            .map(|b| b.id)
            .find(|id| self.shown(*id))
    }

    /// The block whose id is `uid`.
    pub fn block_by_uid(&self, uid: &str) -> Option<BlockId> {
        self.uids.iter().find(|(_, u)| *u == uid).map(|(id, _)| *id)
    }

    /// A block as a saved item at its rect on the canvas, with its id. A terminal's agent keeps
    /// the session the last snapshot found it on: closed and brought back, it resumes that one,
    /// not the folder's latest (another agent's there, maybe the one that closed it).
    fn item_of(&self, block: &Block) -> session::Item {
        let uid = self.uids.get(&block.id).cloned().unwrap_or_default();
        let mut item = self.blocks[&block.id].item(self.home(block));
        if let session::Item::Terminal(t) = &mut item {
            t.session = t.agent.as_deref().and_then(|a| self.session_of(&uid, a));
        }
        item.set_uid(uid);
        item
    }

    /// The session the last snapshot found the agent `agent` on in the terminal block `uid`.
    pub(super) fn session_of(&self, uid: &str, agent: &str) -> Option<String> {
        self.saved
            .iter()
            .flat_map(|s| &s.items)
            .find_map(|saved| match saved {
                session::Item::Terminal(s) if s.uid == uid && s.agent.as_deref() == Some(agent) => {
                    s.session.clone()
                }
                _ => None,
            })
    }

    /// The canvas as a snapshot needs it: rects and process ids, cheap on the UI thread.
    fn layout(
        &self,
        agents: Arc<Vec<templum_terminal::Detect>>,
        accounts: Arc<Vec<agents::Home>>,
        cx: &App,
    ) -> session::Layout {
        let blocks: Vec<&Block> = (self.canvas.blocks().iter())
            .filter(|b| self.kept(b.id))
            .collect();
        // The canvas as it is, also in grid mode.
        session::Layout {
            camera: self.camera(),
            blocks: blocks
                .iter()
                .map(|b| match &self.blocks[&b.id] {
                    Body::Terminal(pane) => {
                        let uid = self.uids.get(&b.id).cloned().unwrap_or_default();
                        let mut item = pane.item(self.home(b), uid);
                        // What the snapshot compares with the agent it finds there.
                        item.agent = pane.flags_for.clone().or(item.agent);
                        let resuming = pane
                            .resuming
                            .as_ref()
                            .filter(|(since, _)| since.elapsed() < spawn::STARTS_WITHIN)
                            .map(|(_, saved)| Box::new(saved.clone()));
                        session::LayoutBlock::Terminal(item, pane.term.processes(), resuming)
                    }
                    _ => session::LayoutBlock::Saved(self.item_of(b)),
                })
                .collect(),
            focused: self
                .focused
                .and_then(|id| blocks.iter().position(|b| b.id == id)),
            recent: self.recent.clone(),
            agents,
            accounts,
            ink: self.ink.iter().map(session::InkItem::of).collect(),
            grid: self.grid.as_ref().and_then(|g| {
                let mut unkept = g.tiles.ids().into_iter().filter(|id| !self.kept(*id));
                let tiles = unkept.try_fold(g.tiles.clone(), |tiles, id| tiles.without(id))?;
                session::SavedTile::of(&tiles, &|id| blocks.iter().position(|b| b.id == id))
            }),
            wires: self
                .wires
                .iter()
                .filter(|w| self.kept(w.a) && self.kept(w.b))
                .filter_map(|w| {
                    Some(session::SavedWire {
                        a: self.uids.get(&w.a)?.clone(),
                        b: self.uids.get(&w.b)?.clone(),
                        label: self.label(*w).map(str::to_string),
                    })
                })
                .collect(),
            git: self.git.as_ref().map(|g| g.saved(cx)),
        }
    }

    /// The last snapshot found one of `agents` running in one of its terminals (a stopped
    /// workspace runs none).
    pub fn has_agent(&self, agents: &[AgentEntry]) -> bool {
        !self.stopped
            && self.saved.as_ref().is_some_and(|saved| {
                saved.items.iter().any(|item| match item {
                    session::Item::Terminal(t) => t
                        .agent
                        .as_ref()
                        .is_some_and(|id| agents.iter().any(|a| a.id == *id)),
                    _ => false,
                })
            })
    }

    /// Blocks moved here are kept: it isn't stopped, and its file is written. A mirror is home's.
    pub fn takes_blocks(&self) -> bool {
        !self.stopped && !self.frozen && !self.is_remote()
    }

    /// Drops every block (their shells hang up) and what only made sense with them. The canvas
    /// keeps counting ids: a block started again is never taken for one of these (a Git job
    /// still running finds its block by id).
    fn empty(&mut self) {
        let ids: Vec<BlockId> = self.canvas.blocks().iter().map(|b| b.id).collect();
        for id in ids {
            self.canvas.remove(id);
        }
        self.blocks.clear();
        self.uids.clear();
        self.focused = None;
        self.selected.clear();
        self.hovered = None;
        self.guides.clear();
        self.flight = None;
        self.appearing.clear();
        self.ghosts.clear();
        self.framed = None;
        self.ink.clear();
        self.selected_ink = None;
        self.wires.clear();
        self.selected_wire = None;
        self.labels.clear();
        self.undo.clear();
        self.redo.clear();
        self.ink_undo.clear();
        self.ink_redo.clear();
        self.grid = None;
        self.git = None;
    }

    /// The terminal blocks back to front: the order of a snapshot's terminal items.
    fn terminal_order(&self) -> Vec<BlockId> {
        self.canvas
            .blocks()
            .iter()
            .filter(|b| self.terminal(b.id).is_some() && self.kept(b.id))
            .map(|b| b.id)
            .collect()
    }

    /// Block `id` is saved with the canvas: any but a Claude Code subagent's, which belongs to a
    /// live session (docs/specs/claude-subagents.md).
    pub(super) fn kept(&self, id: BlockId) -> bool {
        self.terminal(id).is_none_or(|p| p.follows.is_none())
    }

    /// Tells each terminal what a snapshot found in it. The terminals whose agent, found running
    /// before, is gone, or whose agent spawned with a prompt no snapshot found within
    /// `STARTS_WITHIN` never started: what was handed to it has no one to answer. `homes`: the
    /// accounts, whose agent found in a block of one is that account.
    fn see(
        &mut self,
        found: Vec<(BlockId, templum_terminal::Inspection)>,
        homes: &[agents::Home],
    ) -> Vec<String> {
        let now = Instant::now();
        let mut gone = Vec::new();
        for (id, mut seen) in found {
            if let Some(pane) = self.terminal_mut(id) {
                // An agent found on a folder is that folder's account, typed by hand or not; an
                // environment that couldn't be read keeps what the block had. Another agent run in
                // an account's block (codex in a claude2 block) is on its own folder: the block
                // keeps its account, whose shell still has the variable.
                if let (Some(agent), Some(var)) = (seen.agent.as_deref(), &seen.folder_var) {
                    let found = var
                        .as_deref()
                        .and_then(|folder| agents::account_of(agent, folder, homes));
                    let current = pane
                        .account
                        .as_deref()
                        .and_then(|a| homes.iter().find(|h| h.id == a));
                    if found.is_some() || current.is_none_or(|home| home.agent == agent) {
                        pane.account = found;
                    }
                }
                let home = pane
                    .account
                    .as_deref()
                    .and_then(|a| homes.iter().find(|h| h.id == a));
                seen.agent = agents::in_block(seen.agent.as_deref(), home);
                // A folder the snapshot couldn't read keeps the one known.
                seen.folder = seen.folder.or(pane.seen.folder.take());
                // The agent typed back is up, or the person's own: what is found is saved.
                if seen.agent.is_some() {
                    pane.resuming = None;
                }
                // The model and the effort are the agent's they were typed for: gone once another
                // agent is found there (typed by hand), or once that agent, found before, left.
                // One still starting keeps them.
                let other = seen.agent.is_some() && seen.agent != pane.flags_for;
                let left = pane.seen.pid.is_some() && seen.agent.is_none();
                if other || left {
                    (pane.model, pane.effort, pane.flags_for) = (None, None, None);
                }
                // What the agent that left started after it is still its own for `stop_agent`.
                if left {
                    pane.quit = pane.seen.started.map(|started| (started, now));
                } else if seen.agent.is_some() {
                    pane.quit = None;
                }
                // Its agent is gone (one that crashed says no SessionEnd): what its hooks said
                // mustn't come back with the next agent started there by hand.
                if seen.agent.is_none() {
                    pane.state = None;
                    pane.speaks = false;
                    // Found before, so it left; one not found yet may still be starting, and one
                    // Templum started there stays known while its prompt waits, until its shell
                    // took too long to start it: it never did.
                    let starting = pane
                        .task(now)
                        .is_some_and(|h| now < h.since + spawn::STARTS_WITHIN);
                    if pane.seen.pid.is_some() {
                        pane.prompt = None;
                        gone.push(pane.term.id().to_string());
                    } else if pane.prompt.is_some() && starting {
                        seen.agent = pane.seen.agent.take();
                    } else if pane.prompt.take().is_some() {
                        // Nor what it was given: it isn't the agent there.
                        (pane.model, pane.effort, pane.flags_for) = (None, None, None);
                        gone.push(pane.term.id().to_string());
                    }
                }
                pane.seen = seen;
            }
        }
        gone
    }

    /// Every terminal block, in no order.
    /// A remote workspace's viewers are not among them: nothing local (the agents, the control
    /// server, the API's terminals, wires, the Agents view) reaches a block of home's.
    pub fn terminals(&self) -> impl Iterator<Item = (BlockId, &Pane)> {
        self.blocks
            .iter()
            .filter_map(|(id, body)| Some((*id, body.terminal()?)))
            .filter(|(_, pane)| pane.remote.is_none())
    }

    /// Every terminal block, a remote workspace's viewers too: the Agents view and its watch list
    /// the agents of another computer, and nothing else may (see `terminals`).
    pub fn watched_terminals(&self) -> impl Iterator<Item = (BlockId, &Pane)> {
        self.blocks
            .iter()
            .filter_map(|(id, body)| Some((*id, body.terminal()?)))
    }

    pub fn terminal(&self, id: BlockId) -> Option<&Pane> {
        self.blocks.get(&id)?.terminal()
    }

    pub fn terminal_mut(&mut self, id: BlockId) -> Option<&mut Pane> {
        self.blocks.get_mut(&id)?.terminal_mut()
    }

    pub fn checklist_mut(&mut self, id: BlockId) -> Option<&mut Checklist> {
        self.blocks.get_mut(&id)?.checklist_mut()
    }

    /// The checklist block `id`, a mirrored workspace's too.
    pub fn checklist_view(&self, id: BlockId) -> Option<&Checklist> {
        self.blocks.get(&id)?.checklist_view()
    }

    pub fn checklist_view_mut(&mut self, id: BlockId) -> Option<&mut Checklist> {
        self.blocks.get_mut(&id)?.checklist_view_mut()
    }

    /// The note block `id`, a mirrored workspace's too.
    pub fn note_view_mut(&mut self, id: BlockId) -> Option<&mut Note> {
        self.blocks.get_mut(&id)?.note_view_mut()
    }

    pub fn kanban(&self, id: BlockId) -> Option<&Kanban> {
        self.blocks.get(&id)?.kanban()
    }

    pub fn kanban_mut(&mut self, id: BlockId) -> Option<&mut Kanban> {
        self.blocks.get_mut(&id)?.kanban_mut()
    }

    pub fn editor(&self, id: BlockId) -> Option<&Editor> {
        self.blocks.get(&id)?.editor()
    }

    pub fn editor_mut(&mut self, id: BlockId) -> Option<&mut Editor> {
        self.blocks.get_mut(&id)?.editor_mut()
    }

    pub fn git(&self) -> Option<&Git> {
        self.git.as_deref()
    }

    pub fn git_mut(&mut self) -> Option<&mut Git> {
        self.git.as_deref_mut()
    }

    /// The changed files' names: of one block, or of every block of this workspace.
    pub fn unsaved(&self, block: Option<BlockId>) -> Vec<String> {
        self.blocks
            .iter()
            .filter(|(id, _)| block.is_none_or(|b| b == **id))
            .filter_map(|(_, b)| b.editor())
            .flat_map(|e| e.tabs.iter())
            .filter(|t| t.dirty)
            .map(editor::Tab::name)
            .collect()
    }

    /// The focused block's terminal, when the focused block is one.
    pub fn focused_terminal(&self) -> Option<&Pane> {
        self.focused.and_then(|id| self.terminal(id))
    }
}

/// What the pointer does on the canvas. Block tools act at once and don't stay on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    #[default]
    Select,
    /// Pans with any drag or wheel, over blocks too, like holding Alt.
    Hand,
    /// Wires two blocks: pressed on one, let go on another. Stays on.
    Wire,
    Pen,
    Highlighter,
    Shape(ShapeKind),
    Eraser,
}

impl Tool {
    /// A tool that draws: it stays on, and a layer over the blocks takes the pointer.
    pub fn draws(self) -> bool {
        matches!(
            self,
            Tool::Pen | Tool::Highlighter | Tool::Shape(_) | Tool::Eraser
        )
    }
}

/// Called at the start of every render; returning false removes it (the bench drives frames).
pub(crate) type FrameHook =
    Box<dyn FnMut(&mut Workspace, &mut Window, &mut Context<Workspace>) -> bool>;

pub struct Workspace {
    /// The workspace on screen.
    pub(crate) space: Space,
    /// The hidden ones, in the sidebar's order without the active one. Their terminals run on.
    pub(crate) others: Vec<Space>,
    /// The active workspace's position in the sidebar.
    pub(crate) active: usize,
    /// Every kanban board, shown by kanban blocks in any workspace.
    pub(crate) boards: Boards,
    /// Every note, shown by note blocks in any workspace (`notes.rs`).
    pub(crate) notes: crate::notes::Notes,
    /// The notes as last read or written, to write only what changed.
    notes_saved: Option<crate::notes::Notes>,
    /// `notes.json` couldn't be read: neither it nor the canvases are written this session (a
    /// canvas from before it holds notes' text that only it would keep).
    notes_frozen: bool,
    /// Every checklist, shown by checklist blocks in any workspace (`checklists.rs`).
    pub(crate) checklists: Checklists,
    /// One checklist as it was before each change to it, newest last, at most CHECKLIST_UNDO
    /// (see `edit_checklist`).
    checklists_undo: Vec<List>,
    /// The checklists as last read or written, to write only what changed.
    checklists_saved: Option<Checklists>,
    /// `checklists.json` couldn't be read: it isn't written this session.
    checklists_frozen: bool,
    /// What the person tells their agents, given as a session starts (`instructions.rs`).
    pub(crate) standing: crate::instructions::Instructions,
    /// The instructions as last read or written, to write only what changed.
    standing_saved: Option<crate::instructions::Instructions>,
    /// `instructions.json` couldn't be read: it isn't written this session.
    standing_frozen: bool,
    /// The person's rules that run at a time (`automations/`, `workspace/automations.rs`).
    pub(crate) automations: crate::automations::Automations,
    /// The automations as last read or written, to write only what changed.
    automations_saved: Option<crate::automations::Automations>,
    /// `automations.json` couldn't be read: it isn't written this session.
    automations_frozen: bool,
    /// What each automation is doing, by id (`automations.rs`).
    pub(crate) automation_runs: automations::Runs,
    /// What agents did that an event rule may hear, oldest first: (the block's uid, what); filled
    /// only while an event rule is on and taken at every look.
    pub(crate) automation_heard: Vec<(String, crate::automations::Heard)>,
    /// The window the automations act in, for the look that has none of its own.
    window_handle: gpui_kit::AnyWindowHandle,
    /// The look at the automations runs (`automations::look_at_automations`).
    automating: bool,
    /// The Automations popup is open. Never together with the Git popup, the inbox or Agents.
    pub(crate) automations_open: bool,
    /// The automation its editor shows (`automations::NEW`: the one being written); None: the
    /// first.
    pub(crate) automations_at: Option<u64>,
    /// The popup, or one of its fields, has the keyboard.
    pub(crate) automations_focus: FocusHandle,
    /// What the person edited and didn't save, by id (`automations::NEW` for a new one).
    pub(crate) automation_drafts: HashMap<u64, crate::automations::Automation>,
    /// The calls on home's rules (their popup keys and tools) whose answer is still to come: the
    /// same one is not sent twice (`remote_rules::home_rule_call`).
    pub(crate) home_busy: std::collections::HashSet<(u64, &'static str)>,
    /// The editor's fields, made at the first open.
    pub(crate) automation_form: Option<crate::automations_view::editor::Form>,
    /// The open card dialog's work, kept as a draft when the window closes (see `kanban::Work`).
    pub(crate) card_work: Option<crate::kanban::Open>,
    /// `boards` before each change, newest last, at most BOARD_UNDO (see `edit_boards`).
    boards_undo: Vec<Boards>,
    /// The boards as last read or written, to write only what changed.
    boards_saved: Option<Boards>,
    /// `boards.json`, or the workspace list, couldn't be read: kept as it is this session.
    boards_frozen: bool,
    index_frozen: bool,
    /// The sidebar is shown.
    pub(crate) sidebar: bool,
    /// The popups' size the person dragged it to (none: `git_block::SIZE`), kept as dragged
    /// and written in the index; what is drawn is `git_popup_size`. Git, Requests, Agents and
    /// Automations share it (crate::popup).
    pub(crate) git_size: Option<Size>,
    /// What the window leaves the popups, as the last frame measured it (infinite before the
    /// first): the drag and the diff panel read it without a window.
    git_room: Size,
    /// The popups fill their room (the header's Full screen) until the person restores them;
    /// window chrome for this session, like the Templum Agent's.
    pub(crate) popup_full: bool,
    pub(crate) cell: CellMetrics,
    /// IME pre-edit text, drawn at the cursor until committed.
    pub(crate) marked: Option<String>,
    pub(crate) ime_cursor: Option<Bounds<Pixels>>,
    pub(crate) on_frame: Option<FrameHook>,
    drag: Option<Drag>,
    /// The rects of the blocks a drag moves or resizes, as they were when it started.
    placed: Vec<(BlockId, templum_canvas::Rect)>,
    /// Where the pointer last moved to with no button down (window position): a move elsewhere
    /// gives the keyboard to the block under it (`follow_pointer`).
    pointer: Option<PixelPoint<Pixels>>,
    /// Blocks added now aren't kept for Ctrl+Z: a canvas being restored, closed blocks coming
    /// back.
    quiet: bool,
    /// Where a press on the empty canvas started, and the wire under it: released in place,
    /// it's a click (that selects the wire).
    press: Option<(Point, Option<Wire>)>,
    pub(crate) focus_handle: FocusHandle,
    /// Window position of the workspace's top-left: screen coordinates are relative to it.
    origin: PixelPoint<Pixels>,
    notify: Notify,
    /// Sub-step accumulators: wheel zoom, terminal scrolling (in lines).
    zoom_acc: f32,
    scroll_acc: f32,
    /// The terminal cell the pointer was last reported over, with no button down, to a program
    /// that asked for every move.
    hover_cell: Option<(BlockId, usize, usize)>,
    /// Program for new terminals instead of the login shell (tests).
    command: Option<(String, Vec<String>)>,
    /// Where config.toml and the themes folder live (None: defaults only, nothing saved).
    pub(crate) config_dir: Option<PathBuf>,
    /// Where the canvas is saved (None: it isn't kept).
    pub(crate) data_dir: Option<PathBuf>,
    /// What discards keep (`trash/` in the data folder); None without one, as in `--bench`.
    pub(crate) trash: Option<templum_git::trash::Trash>,
    /// Where the shell integration's scripts were written (None: not written, no integration).
    integration: Option<PathBuf>,
    /// Counts snapshots: a write older than the last one done is dropped (session::save).
    generation: u64,
    /// A background snapshot is reading or writing: the next waits for the next period.
    snapshotting: bool,
    /// The last canvas write failed (and said so); the next good one says it works again.
    canvas_save_failed: bool,
    _snapshots: Task<()>,
    /// Looks at the agents' screens each `activity::LOOK`.
    _looks: Task<()>,
    /// Each agent's plan limits, as last read after a snapshot (docs/specs/usage.md).
    pub(crate) limits: Vec<(String, templum_usage::Limits)>,
    /// When to tell the user about them.
    watch: templum_usage::Watch,
    /// The title bar's usage popover is open.
    pub(crate) usage_open: bool,
    /// The Git popup (the sidebar's bottom button) is open, on the workspace shown's Git.
    pub(crate) git_open: bool,
    /// The requests inbox (docs/specs/ask-inbox.md) is open, on the requests of the workspace
    /// shown. Never together with the Git popup.
    pub(crate) inbox_open: bool,
    /// The key of the request the inbox has selected; none, or one it doesn't list: the first.
    pub(crate) inbox_at: Option<u64>,
    /// What holds the keyboard while the inbox has it.
    pub(crate) inbox_focus: FocusHandle,
    /// A request ended while the inbox was open: the next frame gives the inbox the keyboard back
    /// if one of its fields (which ended with it) had it.
    inbox_refocus: bool,
    /// When the last request arrived in the workspace shown: the sidebar's button pulses for
    /// `inbox::PULSE` after it (never with reduced motion).
    pub(crate) inbox_pulse: Option<Instant>,
    /// The list's scroll, kept to show the selected row.
    pub(crate) inbox_list: gpui_kit::ScrollHandle,
    /// The selection changed: the list scrolls to its row once it is drawn and laid out.
    pub(crate) inbox_reveal: std::cell::Cell<bool>,
    /// The Agents view (docs/specs/agents-view.md): the chip chosen, the search's words (trimmed),
    /// whether the pointer is over the list, and the order its rows were last listed in, which
    /// they keep while it is.
    pub(crate) agents_show: agents_view::Show,
    pub(crate) agents_query: String,
    pub(crate) agents_hover: bool,
    pub(crate) agents_held: std::cell::RefCell<Vec<String>>,
    /// The Agents popup is open. Never together with the Git popup or the inbox.
    pub(crate) agents_open: bool,
    /// The terminal of the agent the person chose in the popup; none, or one it doesn't list:
    /// `agents_pinned`'s row, else the first.
    pub(crate) agents_at: Option<String>,
    /// The terminal of the agent shown selected in its stead (the first row, say): it stays
    /// selected as the order changes, until the chosen one is listed again.
    pub(crate) agents_pinned: Option<String>,
    /// Its search field, made at the first open; its text is `agents_query`.
    pub(crate) agents_search: Option<gpui_kit::Entity<gpui_kit::component::input::InputState>>,
    _agents_search_sub: Option<Subscription>,
    /// The list's scroll, kept to show the selected row.
    pub(crate) agents_list: UniformListScrollHandle,
    /// The plan's limits as Claude Code's `/usage` last told them, by account (the agent's id
    /// for its own folder).
    pub(crate) claude_polled: HashMap<String, templum_usage::Limits>,
    /// When `/usage` was last asked for each account (seconds since the epoch): it is never asked
    /// again within `usage::CLAUDE_MIN`.
    pub(crate) claude_asked: HashMap<String, u64>,
    /// The `/usage` asks waiting their turn, the next first, and the account whose runs: Claude Code
    /// runs one at a time, so its probes don't hold the background pool's threads.
    pub(crate) claude_queue: std::collections::VecDeque<usage::Job>,
    pub(crate) claude_probing: Option<String>,
    /// Looks every few seconds for the accounts whose limits are due.
    _claude_poll: Task<()>,
    /// The title bar's meter shows the agents in turn: this one (an index among those shown),
    /// since `meter_turned` (seconds since the epoch); not while the pointer is over it.
    pub(crate) meter_turn: usize,
    pub(crate) meter_turned: u64,
    pub(crate) meter_hovered: bool,
    /// Reading the agents' usage; the next read replaces it.
    _usage: Task<()>,
    /// Where an update stands (docs/specs/updates.md).
    pub(crate) update: crate::update::Update,
    /// How this Templum was installed, read at start: after a swap, Linux names the running
    /// executable "(deleted)".
    install: templum_update::Install,
    /// The news last told about an update ("ready-0.2.0"): each is told once.
    update_told: Option<String>,
    /// Checking or downloading; the check at start and every day.
    _update: Task<()>,
    _update_loop: Task<()>,
    pub(crate) themes: Vec<Entry>,
    /// The system's appearance.
    pub(crate) dark: bool,
    /// What everything is drawn with.
    pub(crate) theme: Rc<Theme>,
    /// The chrome's colors this frame: the theme's, or on their way to them after a change.
    /// Terminal text and gpui-kit's widgets use the theme's at once.
    pub(crate) ui: Ui,
    /// The active config.toml: the last valid one.
    pub(crate) config: Config,
    /// Its shortcuts.
    pub(crate) keymap: Keymap,
    /// The AI agents in effect: Templum's, with the config's changes, and the user's.
    pub(crate) agents: Vec<AgentEntry>,
    /// Each account's command: the file that runs it, or why it has none.
    pub(crate) commands: BTreeMap<String, Result<PathBuf, String>>,
    /// Whether each agent's program was found, by program, for Settings ▸ Agents: read when the
    /// panel opens and when the agents change, not each frame (finding a program looks in many
    /// folders).
    pub(crate) found: BTreeMap<String, bool>,
    /// The agent Generate ✨ runs, and whether it is ready; read when the config changes, not
    /// each frame (finding a program looks in many folders).
    // ponytail: an agent installed while Templum runs shows its ✨ after the next config change
    // or a restart; look again when the settings panel opens if someone waits for it.
    pub(crate) commit_agent: Option<agents::CommitAgent>,
    /// What config.toml looked like at the last read, for polling.
    config_stamp: config::Stamp,
    /// The file of the theme in use, as it was when last read, for polling.
    theme_file: Option<(PathBuf, config::Stamp)>,
    /// The color editor's changes not yet written to that theme file, one per key; written with
    /// `unsaved`.
    theme_edits: Option<(PathBuf, Vec<Edit>)>,
    /// The last read failed: the next good one says so.
    config_failed: bool,
    /// The system's appearance. `dark` is what's in effect, since config.toml may pin a mode.
    system_dark: bool,
    _poll: Task<()>,
    /// Settings changes for the next frame to apply (applying a font measures text in the window).
    pending: Option<Config>,
    /// Settings changes not yet written to config.toml, one per key.
    unsaved: Vec<Edit>,
    /// Writes `unsaved` SAVE_DELAY after the last change.
    _save: Task<()>,
    /// Why the last write of config.toml failed; cleared by the next good write or reload.
    pub(crate) save_error: Option<String>,
    /// Write `unsaved` when the window closes or the app quits.
    _flush: Vec<Subscription>,
    /// Installed monospaced families, measured when the settings panel opens.
    pub(crate) fonts: Vec<SharedString>,
    /// The shells found on this computer, and the default's name, read at start and when the
    /// panel or the welcome opens.
    pub(crate) shells: Vec<templum_terminal::ShellOption>,
    pub(crate) default_shell: String,
    /// `shells` was read before the login shell's PATH arrived (`read_shell_path` is a thread
    /// started with Templum): it may lack a shell only that PATH has, outside the usual install
    /// folders. Read again by the first menu opened after it arrives.
    pub(crate) shells_early: bool,
    /// What the user typed in the panel's text fields that can't apply yet (an unclosed quote),
    /// by the config key it is for.
    pub(crate) drafts: HashMap<&'static str, String>,
    /// When the quit shortcut was pressed once: a second press soon after quits.
    quit_armed: Option<Instant>,
    /// The settings panel, open or sliding out.
    pub(crate) settings: Option<Panel>,
    /// The workspaces screen, open or sliding out.
    pub(crate) screen: Option<crate::spaces_screen::Screen>,
    /// The welcome, open or fading out.
    pub(crate) welcome: Option<crate::welcome::Welcome>,
    /// The panel's keyboard focus: Esc and the settings shortcut close it from there.
    pub(crate) settings_focus: FocusHandle,
    /// The command whose shortcut the panel is recording: keys go there before anything else.
    pub(crate) recording: Option<Command>,
    /// Why the last key pressed while recording wasn't taken.
    pub(crate) key_note: Option<String>,
    /// Shortcut resets asked for in this event ("Reset All" asks for several), decided together.
    resetting: Vec<Command>,
    /// The open theme picker.
    pub(crate) picker: Option<Picker>,
    /// The open right-click menu.
    pub(crate) menu: Option<menu::OpenMenu>,
    /// Alt was held at the last frame (the hand layer is drawn).
    pub(crate) hand: bool,
    /// The toolbar's active tool.
    pub(crate) tool: Tool,
    /// The drawing tools' color and width.
    pub(crate) ink_color: Color,
    pub(crate) ink_width: ink::Width,
    /// The shape last picked: the Shapes button shows it.
    pub(crate) last_shape: Option<ShapeKind>,
    /// The shapes pill is open above the toolbar.
    pub(crate) shapes_open: bool,
    /// The drawings' paths, built once per zoom step.
    ink_cache: Rc<RefCell<ink::Cache>>,
    /// The focused terminal's cursor blinks (painting reports it); and the blink phase.
    pub(crate) cursor_blinks: bool,
    pub(crate) blink_on: bool,
    _blink: Task<()>,
    /// Checks open files for changes on disk every editor::POLL.
    _files: Task<()>,
    /// Reads the workspace's Git every git_block::POLL while the popup is open.
    _git: Task<()>,
    /// `git --version` with the configured program, read when the settings panel opens: Ok is
    /// the version, Err why it can't run. None while it is being read.
    pub(crate) git_version: Option<Result<String, String>>,
    /// The system's entry that starts Templum at login, and what the toggle shows of it
    /// (docs/specs/settings.md, Start at login).
    pub(crate) autostart: crate::autostart::State,
    /// Settings ▸ Git ▸ Try it, running or done.
    pub(crate) message_try: Option<TryIt>,
    /// Settings ▸ Connections ▸ Gemini CLI: Install or Remove running, or what went wrong.
    pub(crate) gemini_job: Option<TryIt>,
    /// A Gemini CLI opened without Templum's extension said so this run (`tell_gemini`).
    pub(super) gemini_told: std::collections::HashSet<String>,
    /// The bus the programs in Templum's terminals talk to (docs/specs/connections.md); None
    /// without a data folder.
    pub(crate) bus: Option<templum_mcp::Bus>,
    /// What every new terminal's environment gets while the bus runs.
    bus_env: Vec<(String, String)>,
    _bus: Task<()>,
    /// The tool calls waiting for an answer, oldest first (`waits.rs`).
    waits: Vec<waits::Wait>,
    /// The last wait's key. It starts at the Unix time in milliseconds: home's request ids
    /// (`r<key>`) are named by a mirror that may outlive home's process, and a restarted home
    /// must not reuse one.
    next_wait: u64,
    /// When a request last made its sound (`chime::GAP`), by the executor's clock.
    pub(crate) last_chime: Option<std::time::Instant>,
    /// The last task's id.
    pub(crate) next_task: u64,
    /// The MCP connections of the agents' Templum servers (`templum mcp`, which said `initialize`
    /// while connections were on), by connection: the terminal each one is in. A terminal in it
    /// has Templum's tools (docs/specs/agent-protocol.md, 8).
    pub(crate) tool_conns: HashMap<u64, String>,
    /// What tasks' agents did, waiting to be typed into the agents that handed them over, oldest
    /// first (`delegate.rs`).
    pub(crate) notices: Vec<delegate::Notice>,
    /// How many ends of tasks each agent handed (by terminal) are on their way to `notices`, the
    /// short delay before what they spent is read (`tell_end`): tasks still running to it.
    ending: HashMap<String, usize>,
    /// Texts agents asked to have delivered later, and the last one's id (`schedules.rs`).
    pub(crate) schedules: Vec<schedules::Schedule>,
    pub(crate) next_schedule: u64,
    /// The look at the schedules runs (`schedules.rs`).
    scheduling: bool,
    /// `wait_for` watches whose match is typed back (`wait: false`), the last one's id, and
    /// whether their look runs (`schedules.rs`).
    pub(crate) watches: Vec<schedules::Watch>,
    next_watch: u64,
    watching: bool,
    /// The Templum Agent: its conversation, and its engine's secret (docs/specs/templum-agent.md).
    pub(crate) templum_agent: crate::chat::TemplumAgent,
    /// The Templum API: its file, its server and what Settings shows (`api.rs`).
    pub(crate) api: api::Api,
    /// Draws the next frame while a dot travels along a wire in view (`wires::DOT_FRAME`).
    _dot_frame: Task<()>,
    _wake: Task<()>,
    /// The client that follows the remote workspaces, started at the first one (`remote.rs`).
    pub(crate) mirror: Option<templum_api::client::Mirror>,
    /// Where the remotes' keys are kept.
    pub(crate) keys: templum_api::client::Keys,
    /// Takes what the client says whenever it wakes this workspace.
    _mirror: Task<()>,
    /// A timer waits to repaint when the next stream choice is due (`remote.rs`).
    stream_waiting: bool,
    /// The remote the Connect dialog is waiting on, by id, and how that went (`remote.rs`).
    pub(crate) pending_connect: Option<String>,
    /// Test: what a remote workspace warned the person of.
    #[cfg(test)]
    pub(crate) said: Vec<String>,
    /// Test: the calls a remote workspace made with no client to make them, to answer by hand.
    #[cfg(test)]
    pub(crate) calls: Vec<(String, templum_api::client::Call, remote::Then)>,
    connect_done: Option<Result<(), String>>,
    /// The code as the clipboard may still hold it: cleared after a success, dropped otherwise.
    connect_clip: Option<String>,
    /// Whether the system's keyring answers, as `Mirror::probe` last said; None: not yet.
    pub(crate) keyring: Option<bool>,
}

impl Workspace {
    /// `autostart`: the system's entry that starts Templum at login (None: it has none, as for
    /// `--bench`); `config_dir`: where the settings file and themes are; `data_dir`: where the
    /// canvas is kept (None: it isn't, as for `--bench`).
    pub fn new(
        autostart: Option<Autostart>,
        config_dir: Option<PathBuf>,
        data_dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with(None, autostart, config_dir, data_dir, window, cx)
    }

    /// `command`: program for new terminals instead of the login shell (tests).
    pub fn with(
        command: Option<(String, Vec<String>)>,
        autostart: Option<Autostart>,
        config_dir: Option<PathBuf>,
        data_dir: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // PTY threads notify through a channel; this task turns that into a redraw.
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<()>();
        let notify: Notify = Arc::new(move || {
            let _ = tx.unbounded_send(());
        });
        let wake = cx.spawn(async move |this, cx| {
            while rx.next().await.is_some() {
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let (config, config_error) = match config::load(config_dir.as_deref(), data_dir.as_deref())
        {
            Ok(config) => (config, None),
            Err(message) => (Config::default(), Some(message)),
        };
        let config_stamp = config::Stamp::of(config_dir.as_deref());
        let themes = theme::load(config_dir.as_deref());
        for warning in missing_themes(&themes, &config.theme) {
            eprintln!("templum: {warning}");
            show_notification(window, cx, Notification::warning(warning));
        }
        let system_dark = is_dark(window.appearance());
        let dark = config.theme.mode.dark(system_dark);
        let theme = theme::pick(&themes, config.theme.name(dark), theme::default_name(dark));
        theme::apply_to_kit(&theme, cx);
        let (cell, font_warning) =
            CellMetrics::measure(window, config.font.family.as_deref(), config.font.size);
        if let Some(message) = &config_error {
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::error(message.clone()));
        }
        if let Some(warning) = font_warning {
            eprintln!("templum: {warning}");
            show_notification(window, cx, Notification::warning(warning));
        }
        let effective_agents =
            agents::effective(&config.agents, &agents::codex_models(), data_dir.as_deref());
        let commit_agent = agents::commit_agent(
            &effective_agents,
            &config.git.commit_message,
            templum_terminal::installed,
        );
        let integration = data_dir.as_deref().and_then(|dir| {
            templum_terminal::install(dir)
                .inspect_err(|err| {
                    let message = format!(
                        "Couldn't write the shell integration to {}: {err}",
                        dir.display()
                    );
                    eprintln!("templum: {message}");
                    show_notification(window, cx, Notification::warning(message));
                })
                .ok()
        });
        let (keymap, key_warnings) = Keymap::new(&config.keys, cfg!(target_os = "macos"))
            .expect("config::parse validated the shortcuts");
        for warning in key_warnings {
            eprintln!("templum: {warning}");
            show_notification(window, cx, Notification::warning(warning));
        }
        let poll = cx.spawn_in(window, async move |ws, cx| {
            loop {
                cx.background_executor().timer(config::POLL).await;
                if ws
                    .update_in(cx, |ws, window, cx| ws.poll_config(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let snapshots = cx.spawn_in(window, async move |ws, cx| {
            loop {
                cx.background_executor().timer(session::EVERY).await;
                if ws
                    .update_in(cx, |ws, window, cx| ws.snapshot(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let looks = cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor().timer(activity::LOOK).await;
                let looked = ws.update(cx, |ws, cx| ws.look_around(cx));
                if looked.is_err() {
                    break;
                }
            }
        });
        cx.observe_window_appearance(window, |ws, window, cx| {
            ws.set_dark(is_dark(window.appearance()), cx)
        })
        .detach();
        // Tests never run the user's Claude Code.
        let claude_poll = if cfg!(test) {
            Task::ready(())
        } else {
            cx.spawn(async move |ws, cx| {
                loop {
                    if ws.update(cx, |ws, cx| ws.ask_claude_due(cx)).is_err() {
                        break;
                    }
                    cx.background_executor().timer(usage::CLAUDE_LOOK).await;
                }
            })
        };
        let blink = cx.spawn_in(window, async move |ws, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                if ws.update(cx, |ws, cx| ws.tick_blink(cx)).is_err() {
                    break;
                }
            }
        });
        let files = cx.spawn_in(window, async move |ws, cx| {
            loop {
                cx.background_executor().timer(editor::POLL).await;
                if ws
                    .update_in(cx, |ws, window, cx| ws.check_files(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        let git = cx.spawn_in(window, async move |ws, cx| {
            loop {
                cx.background_executor().timer(git_block::POLL).await;
                if ws
                    .update_in(cx, |ws, window, cx| ws.poll_git(window, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        // Closing the window with unsaved files asks first, as quitting does.
        let closing = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            closing
                .update(cx, |ws, cx| ws.may_close(window, cx))
                .unwrap_or(true)
        });
        let install = templum_update::Install::detect();
        let update_loop = Self::update_loop(&install, data_dir.as_ref(), window, cx);
        let update_told = data_dir
            .as_ref()
            .and_then(|dir| std::fs::read_to_string(dir.join("updates").join(update::TOLD)).ok());
        let flush = vec![
            cx.on_release(|ws, cx| {
                ws.flush_api();
                ws.save();
                ws.save_canvas_now(cx);
                ws.save_conversation();
                ws.finish_update();
            }),
            cx.on_app_quit(|ws, cx| {
                ws.flush_api();
                ws.save();
                ws.save_canvas_now(cx);
                ws.save_conversation();
                ws.finish_update();
                async {}
            }),
        ];
        let bus::Started {
            bus,
            env: bus_env,
            task: bus_task,
            tx: bus_tx,
        } = bus::start(data_dir.as_deref(), window, cx);
        let data_dir_for_keys = data_dir.clone();
        let mut ws = Self {
            space: Space::default(),
            others: Vec::new(),
            active: 0,
            boards: Boards::default(),
            card_work: None,
            boards_undo: Vec::new(),
            boards_saved: None,
            boards_frozen: false,
            notes: Default::default(),
            notes_saved: None,
            checklists: Checklists::default(),
            checklists_undo: Vec::new(),
            checklists_saved: None,
            checklists_frozen: false,
            standing: Default::default(),
            standing_saved: None,
            standing_frozen: false,
            automations: Default::default(),
            automations_saved: None,
            automations_frozen: false,
            automation_runs: Default::default(),
            automation_heard: Vec::new(),
            window_handle: window.window_handle(),
            automating: false,
            automations_open: false,
            automations_at: None,
            automations_focus: cx.focus_handle(),
            automation_drafts: Default::default(),
            home_busy: Default::default(),
            automation_form: None,
            notes_frozen: false,
            index_frozen: false,
            sidebar: true,
            git_size: None,
            git_room: Size::new(f32::INFINITY, f32::INFINITY),
            popup_full: false,
            cell,
            marked: None,
            ime_cursor: None,
            on_frame: None,
            drag: None,
            placed: Vec::new(),
            pointer: None,
            quiet: false,
            press: None,
            focus_handle,
            origin: PixelPoint::default(),
            notify,
            zoom_acc: 0.0,
            scroll_acc: 0.0,
            hover_cell: None,
            command,
            config_dir,
            trash: data_dir
                .as_ref()
                .map(|dir| templum_git::trash::Trash::new(dir.join("trash"))),
            data_dir,
            integration,
            generation: 0,
            snapshotting: false,
            canvas_save_failed: false,
            _snapshots: snapshots,
            _looks: looks,
            limits: Vec::new(),
            watch: templum_usage::Watch::new(crate::notes::now()),
            usage_open: false,
            git_open: false,
            inbox_open: false,
            inbox_at: None,
            inbox_focus: cx.focus_handle(),
            inbox_refocus: false,
            inbox_pulse: None,
            inbox_list: Default::default(),
            inbox_reveal: Default::default(),
            agents_show: Default::default(),
            agents_query: String::new(),
            agents_hover: false,
            agents_held: Default::default(),
            agents_open: false,
            agents_at: None,
            agents_pinned: None,
            agents_search: None,
            _agents_search_sub: None,
            agents_list: UniformListScrollHandle::new(),
            claude_polled: HashMap::new(),
            claude_asked: HashMap::new(),
            claude_queue: Default::default(),
            claude_probing: None,
            _claude_poll: claude_poll,
            meter_turn: 0,
            meter_turned: 0,
            meter_hovered: false,
            _usage: Task::ready(()),
            update: Default::default(),
            install,
            update_told,
            _update: Task::ready(()),
            _update_loop: update_loop,
            themes,
            dark,
            ui: theme.ui,
            theme,
            config_failed: config_error.is_some(),
            config_stamp,
            theme_file: None,
            theme_edits: None,
            system_dark,
            _poll: poll,
            pending: None,
            unsaved: Vec::new(),
            _save: Task::ready(()),
            save_error: None,
            _flush: flush,
            fonts: Vec::new(),
            // Read before `start_space` restores the canvas: its terminals look their shells up.
            // Whether the login PATH is pending is asked first: one that arrives meanwhile only
            // costs a second read.
            shells_early: templum_terminal::shell_path_pending(),
            shells: templum_terminal::available_shells(),
            default_shell: templum_terminal::default_shell_label(),
            drafts: HashMap::new(),
            quit_armed: None,
            settings: None,
            screen: None,
            welcome: None,
            settings_focus: cx.focus_handle(),
            recording: None,
            key_note: None,
            resetting: Vec::new(),
            picker: None,
            menu: None,
            hand: false,
            tool: Tool::default(),
            ink_color: Color::Blue,
            ink_width: ink::Width::default(),
            last_shape: None,
            shapes_open: false,
            ink_cache: Default::default(),
            cursor_blinks: false,
            blink_on: true,
            _blink: blink,
            _files: files,
            _git: git,
            git_version: None,
            autostart: crate::autostart::State {
                entry: autostart,
                ..Default::default()
            },
            message_try: None,
            gemini_job: None,
            gemini_told: Default::default(),
            config,
            keymap,
            agents: effective_agents,
            commands: BTreeMap::new(),
            found: BTreeMap::new(),
            commit_agent,
            bus,
            bus_env,
            _bus: bus_task,
            waits: Vec::new(),
            next_wait: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |t| t.as_millis() as u64),
            last_chime: None,
            next_task: 0,
            tool_conns: HashMap::new(),
            api: api::Api::new(bus_tx),
            templum_agent: Default::default(),
            notices: Vec::new(),
            ending: HashMap::new(),
            schedules: Vec::new(),
            next_schedule: 0,
            scheduling: false,
            watches: Vec::new(),
            next_watch: 0,
            watching: false,
            _dot_frame: Task::ready(()),
            _wake: wake,
            mirror: None,
            // Tests never open the person's keyring (they set a mock one).
            keys: match &data_dir_for_keys {
                Some(dir) if !cfg!(test) => {
                    templum_api::client::Keys::system(templum_api::client::SERVICE, Some(dir))
                }
                dir => templum_api::client::Keys::without_keyring(dir.as_deref()),
            },
            _mirror: Task::ready(()),
            stream_waiting: false,
            pending_connect: None,
            #[cfg(test)]
            said: Vec::new(),
            #[cfg(test)]
            calls: Vec::new(),
            connect_done: None,
            connect_clip: None,
            keyring: None,
        };
        // Relay files of Claude Code sessions long gone, at startup.
        if let Some(relay) = ws.data_dir.as_deref().map(templum_usage::relay_dir) {
            cx.background_executor()
                .spawn(async move { templum_usage::claude::prune(&relay, crate::notes::now()) })
                .detach();
        }
        // The trash's old discards go at startup (and after each discard).
        if let Some(trash) = ws.trash.clone() {
            cx.background_executor()
                .spawn(async move {
                    let _ = trash.prune(std::time::SystemTime::now());
                })
                .detach();
        }
        for failure in ws.sync_commands() {
            preferences::tell_commands(Some(failure), window, cx);
        }
        ws.refresh_autostart(cx);
        ws.load_spaces(window, cx);
        // Once this is built, so that the keys can still be changed first (the tests').
        if ws.spaces().any(Space::is_remote) {
            cx.spawn_in(window, async move |ws, cx| {
                ws.update_in(cx, |ws, window, cx| ws.connect_remotes(window, cx))
                    .ok();
            })
            .detach();
        }
        ws.load_api(window, cx);
        ws.restore_conversation();
        ws
    }
}

/// On macOS, Option+key types the layout's character (`@` on German, `~`...), like Terminal.app.
/// Keys that produce no printable text (Option+arrows, Option+Backspace) stay Meta.
fn option_types_text(key: &str, key_char: Option<&str>, macos: bool) -> bool {
    macos && key_char.is_some_and(|c| c != key && !c.is_empty() && !c.chars().any(char::is_control))
}

/// "No theme named X" for each configured name that is neither built in nor in the themes
/// folder.
fn missing_themes(themes: &[Entry], config: &ThemeConfig) -> Vec<String> {
    [
        (config.light.as_str(), theme::DEFAULT_LIGHT),
        (config.dark.as_str(), theme::DEFAULT_DARK),
    ]
    .into_iter()
    .filter(|(name, _)| theme::find(themes, name).is_none())
    .map(|(name, default)| format!("No theme named \"{name}\"; using \"{default}\""))
    .collect()
}

#[cfg(test)]
thread_local! {
    /// Test: what a notification or a question told the person, oldest first (`told`): neither
    /// gpui-component's `Notification` nor its dialogs give their words back.
    static TOLD: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Test: keeps `words`, which the person is told in a notification or a question.
#[cfg(test)]
pub(crate) fn told(words: &str) {
    TOLD.with_borrow_mut(|told| told.push(words.to_string()));
}

/// Test: what the person was told so far in this test (its own thread), oldest first.
#[cfg(test)]
pub(crate) fn told_so_far() -> Vec<String> {
    TOLD.with_borrow(Clone::clone)
}

/// Shows a notification at the bottom right, clear of the title bar's buttons. Deferred: at
/// startup the window's notification layer doesn't exist yet.
pub(crate) fn show_notification(window: &mut Window, cx: &mut App, notification: Notification) {
    let notification = notification
        .placement(Anchor::BottomRight)
        .rounded(px(10.0));
    window.defer(cx, move |window, cx| {
        window.push_notification(notification, cx)
    });
}

/// What a failed command's notification says: the line that says why, and when git would
/// have asked for a key or a password, that it needs one (spec §8, Credentials).
pub(crate) fn git_error_message(err: &GitError) -> String {
    let why = err.first_line().to_string();
    if err.needs_input() {
        format!("{why} Git needs your key or password: run it in a terminal.")
    } else {
        why
    }
}

/// `folder` becomes the newest recent folder: once, and only the RECENT newest are kept.
pub(crate) fn remember(recent: &mut Vec<PathBuf>, folder: PathBuf) {
    recent.retain(|f| *f != folder);
    recent.insert(0, folder);
    recent.truncate(RECENT);
}

/// Ctrl+Z / Ctrl+Shift+Z (Cmd on macOS): Some(false) undoes, Some(true) redoes.
/// The command a key press runs: one that only works in an editor is nothing without one, so
/// the key goes on to the terminal (Ctrl+S is XOFF, Ctrl+W erases a word).
/// One that only works in grid mode is nothing outside it.
/// `program`: a program took over the focused terminal (see `Command::yields_to_programs`).
fn effective(
    command: Option<Command>,
    in_editor: bool,
    tiled: bool,
    program: bool,
) -> Option<Command> {
    command.filter(|c| {
        (in_editor || !c.editor_only())
            && (tiled || !c.grid_only())
            && !(program && c.yields_to_programs())
    })
}

fn undo_key(k: &Keystroke, macos: bool) -> Option<bool> {
    let m = &k.modifiers;
    let held = if macos {
        m.platform && !m.control
    } else {
        m.control && !m.platform
    };
    if !held || m.alt || m.function || k.key != "z" {
        return None;
    }
    Some(m.shift)
}

/// Ctrl+`key` (Cmd on macOS), with no other modifier.
fn secondary(k: &Keystroke, key: &str, macos: bool) -> bool {
    let m = &k.modifiers;
    let held = if macos {
        m.platform && !m.control
    } else {
        m.control && !m.platform
    };
    held && !m.alt && !m.shift && !m.function && k.key == key
}

/// Ctrl+1 to Ctrl+9 (Cmd on macOS), with no other modifier: the position of a workspace.
pub(crate) fn workspace_digit(k: &Keystroke, macos: bool) -> Option<usize> {
    let m = &k.modifiers;
    let held = if macos {
        m.platform && !m.control
    } else {
        m.control && !m.platform
    };
    if !held || m.alt || m.shift || m.function {
        return None;
    }
    let digit: usize = k.key.parse().ok()?;
    (1..=9).contains(&digit).then(|| digit - 1)
}

/// The first shortcut of the commands the empty canvas shows: new terminal, then settings.
fn hint_keys(keymap: &Keymap) -> [Option<Keystroke>; 2] {
    [Command::NewTerminal, Command::Settings].map(|c| keymap.keystrokes(c).into_iter().next())
}

fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

#[cfg(test)]
mod tests;
