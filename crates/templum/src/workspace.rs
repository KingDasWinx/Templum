//! The main view: the canvas of terminal blocks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui_kit::base::motion::{MotionStatus, Transition, transition_with_status};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::notification::{Notification, NotificationDelivery};
use gpui_kit::component::{TITLE_BAR_HEIGHT, WindowExt, window_paddings};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, AnyElement, App, Bounds, ClipboardItem, Context, DispatchPhase, ElementInputHandler,
    EntityInputHandler, FocusHandle, FontWeight, Hsla, KeyBinding, KeyDownEvent, Keystroke,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent,
    Pixels, Point as PixelPoint, ScrollDelta, ScrollWheelEvent, SharedString, Subscription, Task,
    TestSupportExt, UTF16Selection, Window, WindowAppearance, canvas, div, fill, point, px, size,
};
use templum_canvas::{
    Block, BlockId, Camera, CameraFlight, Canvas, Gesture, Point, Rect, Size, chrome, dot_grid,
    dot_size,
};
use templum_terminal::{KeyInput, Notify, SelectionKind, Side, Snapshot, SpawnOptions, Terminal};

use crate::agents::{self, AgentEntry};
use crate::alert::{self, Alert};
use crate::block::{self, Content};
use crate::board::{self, Boards};
use crate::config::{self, Config, Edit, ThemeConfig, WorkingDirectory};
use crate::grid::CellMetrics;
use crate::kanban::{self, Kanban};
use crate::keys::{self, Command, Keymap, Keys};
use crate::menu;
use crate::motion;
use crate::session;
use crate::settings::{self, Panel};
use crate::sidebar;
use crate::theme::{self, Entry, Theme, Ui};
use crate::theme_picker::{self, Picker};
use crate::title_bar;

pub(crate) const DEFAULT_COLS: u16 = 80;
pub(crate) const DEFAULT_ROWS: u16 = 24;
/// An agent's terminal: agents draw wide interfaces.
const AGENT_COLS: u16 = 110;
const AGENT_ROWS: u16 = 32;
/// How many folders a workspace remembers for agents.
pub(crate) const RECENT: usize = 8;
/// The largest block a saved canvas restores: a corrupt size mustn't allocate a giant grid.
const MAX_COLS: u16 = 2000;
const MAX_ROWS: u16 = 1000;
/// A press that moves less than this (screen pixels) before release is a click, not a pan.
const CLICK_SLOP: f32 = 4.0;
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

pub(crate) struct Pane {
    pub term: Terminal,
    pub snapshot: Rc<RefCell<Snapshot>>,
    /// The terminal's finished and attention counts when last heard (`Workspace::hear`).
    pub heard: (u64, u64),
    /// News nobody has looked at yet; gone once the block is focused in the active window.
    pub alert: Option<Alert>,
}

/// A closed block, still drawn while it fades out. A terminal's shell lives until then.
pub(crate) struct Ghost {
    pub block: Block,
    pub body: GhostBody,
    since: Instant,
}

pub(crate) enum GhostBody {
    Terminal(Pane),
    Kanban(Kanban),
}

#[derive(Clone, Copy)]
enum Drag {
    Canvas(Gesture),
    /// Selecting text in a block's terminal.
    Select(BlockId),
}

/// One canvas and everything on it: its blocks, their terminals, the focus, and what is
/// animating.
#[derive(Default)]
pub(crate) struct Space {
    /// Names its file; chosen once, never changed.
    pub id: String,
    pub name: String,
    pub canvas: Canvas,
    pub terminals: HashMap<BlockId, Pane>,
    /// The kanban blocks: every block is a terminal or one of these.
    pub kanbans: HashMap<BlockId, Kanban>,
    pub focused: Option<BlockId>,
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
    /// The camera drawn this frame: `canvas.camera` (where it is going), or a frame of `flight`.
    pub view: Camera,
    /// The view gliding to `canvas.camera`, since when.
    pub flight: Option<(CameraFlight, Instant)>,
    /// Blocks still popping in, since when.
    pub appearing: HashMap<BlockId, Instant>,
    /// Closed blocks still fading out.
    pub ghosts: Vec<Ghost>,
    /// The folders agents were opened in on this canvas, newest first.
    pub recent: Vec<PathBuf>,
}

impl Space {
    /// The canvas as a snapshot needs it: rects and process ids, cheap on the UI thread.
    fn layout(&self, agents: Arc<Vec<templum_terminal::Detect>>) -> session::Layout {
        let blocks = self.canvas.blocks();
        session::Layout {
            camera: self.canvas.camera,
            blocks: blocks
                .iter()
                .map(|b| match self.terminals.get(&b.id) {
                    Some(pane) => session::LayoutBlock::Terminal(b.rect, pane.term.processes()),
                    None => session::LayoutBlock::Kanban(
                        b.rect,
                        self.kanbans.get(&b.id).and_then(|k| k.board),
                    ),
                })
                .collect(),
            focused: self
                .focused
                .and_then(|id| blocks.iter().position(|b| b.id == id)),
            recent: self.recent.clone(),
            agents,
        }
    }

    /// The last snapshot found one of `agents` running in one of its terminals.
    pub fn has_agent(&self, agents: &[AgentEntry]) -> bool {
        self.saved.as_ref().is_some_and(|saved| {
            saved.items.iter().any(|item| match item {
                session::Item::Terminal(t) => t
                    .agent
                    .as_ref()
                    .is_some_and(|id| agents.iter().any(|a| a.id == *id)),
                session::Item::Kanban(_) => false,
            })
        })
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
    /// The boards as last read or written, to write only what changed.
    boards_saved: Option<Boards>,
    /// `boards.json`, or the workspace list, couldn't be read: kept as it is this session.
    boards_frozen: bool,
    index_frozen: bool,
    /// The sidebar is shown.
    pub(crate) sidebar: bool,
    pub(crate) cell: CellMetrics,
    /// IME pre-edit text, drawn at the cursor until committed.
    pub(crate) marked: Option<String>,
    pub(crate) ime_cursor: Option<Bounds<Pixels>>,
    pub(crate) on_frame: Option<FrameHook>,
    drag: Option<Drag>,
    /// Where a press on the empty canvas started: released in place, it's a click.
    press: Option<Point>,
    pub(crate) focus_handle: FocusHandle,
    /// Window position of the workspace's top-left: screen coordinates are relative to it.
    origin: PixelPoint<Pixels>,
    notify: Notify,
    /// Sub-step accumulators: wheel zoom, terminal scrolling (in lines).
    zoom_acc: f32,
    scroll_acc: f32,
    /// Program for new terminals instead of the login shell (tests).
    command: Option<(String, Vec<String>)>,
    /// Where config.toml and the themes folder live (None: defaults only, nothing saved).
    pub(crate) config_dir: Option<PathBuf>,
    /// Where the canvas is saved (None: it isn't kept).
    pub(crate) data_dir: Option<PathBuf>,
    /// Where the shell integration's scripts were written (None: not written, no integration).
    integration: Option<PathBuf>,
    /// Counts snapshots: a write older than the last one done is dropped (session::save).
    generation: u64,
    /// A background snapshot is reading or writing: the next waits for the next period.
    snapshotting: bool,
    /// The last canvas write failed (and said so); the next good one says it works again.
    canvas_save_failed: bool,
    _snapshots: Task<()>,
    pub(crate) themes: Vec<Entry>,
    /// The system's appearance.
    dark: bool,
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
    /// What config.toml looked like at the last read, for polling.
    config_stamp: config::Stamp,
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
    /// The shells found on this computer, and the default's name, read when the panel opens.
    pub(crate) shells: Vec<templum_terminal::ShellOption>,
    pub(crate) default_shell: String,
    /// What the user typed in the panel's text fields that can't apply yet (an unclosed quote),
    /// by the config key it is for.
    pub(crate) drafts: HashMap<&'static str, String>,
    /// When the quit shortcut was pressed once: a second press soon after quits.
    quit_armed: Option<Instant>,
    /// The settings panel, open or sliding out.
    pub(crate) settings: Option<Panel>,
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
    /// The hand layer is drawn: Alt was held at the last frame.
    hand: bool,
    /// The focused terminal's cursor blinks (painting reports it); and the blink phase.
    pub(crate) cursor_blinks: bool,
    pub(crate) blink_on: bool,
    _blink: Task<()>,
    _wake: Task<()>,
}

impl Workspace {
    /// `data_dir`: where the canvas is kept (None: it isn't, as for `--bench`).
    pub fn new(data_dir: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with(None, config::dir(), data_dir, window, cx)
    }

    /// `command`: program for new terminals instead of the login shell (tests).
    pub fn with(
        command: Option<(String, Vec<String>)>,
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
        let (config, config_error) = match config::load(config_dir.as_deref()) {
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
        let effective_agents = agents::effective(&config.agents);
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
        cx.observe_window_appearance(window, |ws, window, cx| {
            ws.set_dark(is_dark(window.appearance()), cx)
        })
        .detach();
        let blink = cx.spawn_in(window, async move |ws, cx| {
            loop {
                cx.background_executor().timer(BLINK).await;
                if ws.update(cx, |ws, cx| ws.tick_blink(cx)).is_err() {
                    break;
                }
            }
        });
        let flush = vec![
            cx.on_release(|ws, _| {
                ws.save();
                ws.save_canvas_now();
            }),
            cx.on_app_quit(|ws, _| {
                ws.save();
                ws.save_canvas_now();
                async {}
            }),
        ];
        let mut ws = Self {
            space: Space::default(),
            others: Vec::new(),
            active: 0,
            boards: Boards::default(),
            boards_saved: None,
            boards_frozen: false,
            index_frozen: false,
            sidebar: true,
            cell,
            marked: None,
            ime_cursor: None,
            on_frame: None,
            drag: None,
            press: None,
            focus_handle,
            origin: PixelPoint::default(),
            notify,
            zoom_acc: 0.0,
            scroll_acc: 0.0,
            command,
            config_dir,
            data_dir,
            integration,
            generation: 0,
            snapshotting: false,
            canvas_save_failed: false,
            _snapshots: snapshots,
            themes,
            dark,
            ui: theme.ui,
            theme,
            config_failed: config_error.is_some(),
            config_stamp,
            system_dark,
            _poll: poll,
            pending: None,
            unsaved: Vec::new(),
            _save: Task::ready(()),
            save_error: None,
            _flush: flush,
            fonts: Vec::new(),
            shells: Vec::new(),
            default_shell: String::new(),
            drafts: HashMap::new(),
            quit_armed: None,
            settings: None,
            settings_focus: cx.focus_handle(),
            recording: None,
            key_note: None,
            resetting: Vec::new(),
            picker: None,
            menu: None,
            hand: false,
            cursor_blinks: false,
            blink_on: true,
            _blink: blink,
            config,
            keymap,
            agents: effective_agents,
            _wake: wake,
        };
        ws.load_spaces(window, cx);
        ws
    }

    /// Opens every workspace of the index (without one: `default`, as the version without
    /// workspaces saved it), each canvas as it was saved, and shows the active one. A first run
    /// gets one terminal, centered.
    fn load_spaces(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        use session::Loaded;
        match self.data_dir.as_deref().map(board::load) {
            Some(Loaded::Saved(boards)) => {
                self.boards_saved = Some(boards.clone());
                self.boards = boards;
            }
            Some(Loaded::Moved { to, why }) => {
                let message = format!(
                    "Couldn't read the boards ({why}); they were kept as {}",
                    to.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
            }
            Some(Loaded::Unreadable(why)) => {
                let message =
                    format!("Couldn't read the boards ({why}); they won't be saved this session");
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                self.boards_frozen = true;
            }
            Some(Loaded::Nothing) | None => {}
        }
        let (index, no_index) = match self.data_dir.as_deref().map(session::load_index) {
            Some(Loaded::Saved(index)) => (index, false),
            Some(Loaded::Moved { to, why }) => {
                let message = format!(
                    "Couldn't read the workspace list ({why}); it was kept as {}",
                    to.display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::warning(message));
                (session::Index::first_run(), true)
            }
            Some(Loaded::Unreadable(why)) => {
                let message = format!(
                    "Couldn't read the workspace list ({why}); it won't be saved this session"
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
                self.index_frozen = true;
                (session::Index::first_run(), true)
            }
            Some(Loaded::Nothing) | None => (session::Index::first_run(), true),
        };
        let mut spaces = Vec::new();
        let mut first_saved = false;
        for (i, entry) in index.workspaces.iter().enumerate() {
            self.space = Space {
                id: entry.id.clone(),
                name: entry.name.clone(),
                ..Space::default()
            };
            match self
                .data_dir
                .as_deref()
                .map(|d| session::load(d, &entry.id))
            {
                Some(Loaded::Saved(saved)) => {
                    first_saved |= i == 0;
                    self.restore(saved, window, cx);
                }
                Some(Loaded::Moved { to, why }) => {
                    let message = format!(
                        "Couldn't read the saved canvas ({why}); it was kept as {}",
                        to.display()
                    );
                    eprintln!("templum: {message}");
                    show_notification(window, cx, Notification::warning(message));
                }
                Some(Loaded::Unreadable(why)) => {
                    let message = format!(
                        "Couldn't read the saved canvas ({why}); it won't be saved this session"
                    );
                    eprintln!("templum: {message}");
                    show_notification(window, cx, Notification::error(message));
                    self.space.frozen = true;
                }
                Some(Loaded::Nothing) | None => {}
            }
            spaces.push(std::mem::take(&mut self.space));
        }
        self.sidebar = index.sidebar;
        self.active = index.active.min(spaces.len() - 1);
        self.space = spaces.remove(self.active);
        self.others = spaces;
        if no_index && !first_saved {
            // A first run: one terminal, centered.
            self.space.pristine = true;
            let vp = self.viewport(window);
            let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
            let rect = Rect::new((vp.w - size.w) / 2.0, (vp.h - size.h) / 2.0, size.w, size.h);
            self.open(rect, window, cx);
        }
    }

    /// Shows workspace `to` (a position in the sidebar). What only makes sense on screen ends
    /// with the one left: its fading blocks, its animations, a drag.
    pub(crate) fn switch_to(&mut self, to: usize, window: &mut Window, cx: &mut Context<Self>) {
        if to == self.active || to > self.others.len() {
            return;
        }
        // A menu for something on the canvas left behind.
        self.menu = None;
        let left = &mut self.space;
        // Their shells close now.
        left.ghosts.clear();
        left.appearing.clear();
        left.hovered = None;
        left.flight = None;
        left.view = left.canvas.camera;
        self.drag = None;
        self.press = None;
        self.marked = None;
        let current = std::mem::take(&mut self.space);
        self.others.insert(self.active, current);
        self.space = self.others.remove(to);
        self.active = to;
        self.space.view = self.space.canvas.camera;
        window.focus(&self.focus_handle, cx);
        self.write_index(window, cx);
        cx.notify();
    }

    /// The next (`step` 1) or previous (-1) workspace, wrapping around.
    fn cycle_space(&mut self, step: i32, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.others.len() as i32 + 1;
        let to = (self.active as i32 + step).rem_euclid(n) as usize;
        self.switch_to(to, window, cx);
    }

    /// Creates an empty workspace after the last one and shows it.
    pub(crate) fn new_space(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dir = self.data_dir.clone();
        let id = session::next_id(|id| {
            self.spaces().any(|s| s.id == id)
                || dir.as_deref().is_some_and(|d| session::id_in_folder(d, id))
        });
        let total = self.others.len() + 1;
        let name = (total + 1..)
            .map(|n| format!("Workspace {n}"))
            .find(|name| !self.spaces().any(|s| s.name == *name))
            .expect("an unused name");
        self.others.push(Space {
            id,
            name,
            ..Space::default()
        });
        self.switch_to(total, window, cx);
    }

    /// Renames the workspace with `id`; an empty name is refused.
    pub(crate) fn rename_space(
        &mut self,
        id: &str,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), &'static str> {
        let name = name.trim();
        if name.is_empty() {
            return Err("A workspace needs a name");
        }
        if let Some(space) = self.space_by_id_mut(id) {
            space.name = name.to_string();
        }
        self.write_index(window, cx);
        cx.notify();
        Ok(())
    }

    /// Deletes the workspace with `id`, closing its terminals and keeping its file as `.bak`.
    /// The last one stays.
    pub(crate) fn delete_space(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(at) = self.order().iter().position(|s| s.id == id) else {
            return;
        };
        if self.others.is_empty() {
            return;
        }
        if at == self.active {
            self.switch_to(if at > 0 { at - 1 } else { 1 }, window, cx);
        }
        let space = self
            .others
            .remove(if at < self.active { at } else { at - 1 });
        if at < self.active {
            self.active -= 1;
        }
        if let Some(dir) = &self.data_dir
            && let Err(err) = session::set_aside(dir, &space.id)
        {
            eprintln!("templum: couldn't set {}.json aside: {err}", space.id);
        }
        // Dropping its terminals shuts their shells down.
        drop(space);
        self.write_index(window, cx);
        cx.notify();
    }

    pub(crate) fn toggle_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar = !self.sidebar;
        self.write_index(window, cx);
        cx.notify();
    }

    /// The workspaces as the index lists them.
    fn index(&self) -> session::Index {
        session::Index {
            version: session::VERSION,
            workspaces: self
                .order()
                .iter()
                .map(|s| session::IndexEntry {
                    id: s.id.clone(),
                    name: s.name.clone(),
                })
                .collect(),
            active: self.active,
            sidebar: self.sidebar,
        }
    }

    /// Writes the index at once: it changes only on a user's action.
    fn write_index(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.data_dir.as_ref().filter(|_| !self.index_frozen) else {
            return;
        };
        if let Err(err) = session::save_index(dir, &self.index()) {
            let message = format!("Couldn't save the workspace list: {err}");
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::error(message));
        }
    }

    /// Adds a kanban block on top, focused, popping in.
    fn add_kanban(&mut self, rect: Rect, kanban: Kanban) -> BlockId {
        let id = self.space.canvas.add(rect);
        self.space.kanbans.insert(id, kanban);
        self.space.focused = Some(id);
        self.space.appearing.insert(id, Instant::now());
        id
    }

    /// A saved kanban block's rect made safe: None with a number that isn't finite; at least
    /// the smallest kanban block.
    fn kanban_rect(r: Rect) -> Option<Rect> {
        [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()).then(|| {
            Rect::new(
                r.x,
                r.y,
                r.w.clamp(kanban::MIN.w, 20_000.0),
                r.h.clamp(kanban::MIN.h, 20_000.0),
            )
        })
    }

    /// A kanban block at the viewport's center, showing the board list.
    pub(crate) fn new_kanban(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let center = self.space.canvas.camera.to_world(self.center(window));
        let size = kanban::SIZE;
        let at = self
            .space
            .canvas
            .place_new(
                size,
                Point::new(center.x - size.w / 2.0, center.y - size.h / 2.0),
            )
            .origin();
        self.add_kanban_at(at, window, cx);
    }

    /// A kanban block showing the board list, its top-left corner at `at` (world units).
    pub(crate) fn add_kanban_at(&mut self, at: Point, window: &mut Window, cx: &mut Context<Self>) {
        self.space.pristine = false;
        let rect = Rect::new(at.x, at.y, kanban::SIZE.w, kanban::SIZE.h);
        self.add_kanban(rect, Kanban::default());
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// A terminal of the default size, its top-left corner at `at` (world units).
    pub(crate) fn add_terminal_at(
        &mut self,
        at: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.space.pristine = false;
        let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
        self.open(Rect::new(at.x, at.y, size.w, size.h), window, cx);
    }

    /// A terminal in `folder`, its top-left corner at `at`, with an agent's `command` typed into
    /// its shell;
    /// the folder becomes this workspace's newest recent one.
    pub(crate) fn launch_agent(
        &mut self,
        command: &str,
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
        let id = self.spawn_block(rect, Some(folder.to_path_buf()), window, cx)?;
        // The shell reads it once it is ready.
        let line = format!("{command}\r");
        self.space.terminals[&id].term.write(line.into_bytes());
        remember(&mut self.space.recent, folder.to_path_buf());
        window.focus(&self.focus_handle, cx);
        Some(id)
    }

    /// Saves an agent from the settings: `id` names one already known (one of Templum's, or of
    /// the user's), None a new one of the user's. One of Templum's back at its defaults leaves
    /// config.toml. Returns its id.
    pub(crate) fn save_agent(
        &mut self,
        id: Option<&str>,
        draft: &agents::Draft,
        cx: &mut Context<Self>,
    ) -> Result<String, &'static str> {
        if let Some(problem) = draft.problem() {
            return Err(problem);
        }
        let builtin = id.and_then(templum_terminal::Agent::from_name);
        let pending = self.pending.as_ref().unwrap_or(&self.config);
        let id = match id {
            Some(id) => id.to_string(),
            None => agents::new_id(&draft.name, |x| pending.agents.contains_key(x)),
        };
        match draft.to_config(builtin) {
            Some(set) => {
                let value = agents::to_value(&set);
                let key = id.clone();
                self.edit(
                    Edit::set("agents", id.clone(), value),
                    |c| {
                        c.agents.insert(key, set);
                    },
                    cx,
                );
            }
            None => self.remove_agent(&id, cx),
        }
        Ok(id)
    }

    /// One of Templum's agents back to its defaults, or one of the user's deleted.
    pub(crate) fn remove_agent(&mut self, id: &str, cx: &mut Context<Self>) {
        let key = id.to_string();
        self.edit(
            Edit::remove("agents", id.to_string()),
            |c| {
                c.agents.remove(&key);
            },
            cx,
        );
    }

    /// Starts moving the canvas with the pointer, wherever it is (the hand, the middle button).
    pub(crate) fn begin_pan(&mut self, pos: PixelPoint<Pixels>) {
        self.space.pristine = false;
        self.drag = Some(Drag::Canvas(Gesture::pan(self.local(pos))));
        // Not a click on the canvas: letting go keeps the focus where it is.
        self.press = None;
    }

    /// A right-click on the empty canvas: its menu adds blocks there.
    fn on_background_right_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = self.space.canvas.camera.to_world(self.local(e.position));
        self.open_menu(menu::Target::Canvas(at), e.position, window, cx);
    }

    /// The kanban block `block` shows `board`.
    pub(crate) fn show_board(&mut self, block: BlockId, board: u64, cx: &mut Context<Self>) {
        if let Some(k) = self.space.kanbans.get_mut(&block) {
            *k = Kanban {
                board: Some(board),
                picking: false,
            };
            cx.notify();
        }
    }

    /// The kanban block `block` shows the board list.
    pub(crate) fn pick_board(&mut self, block: BlockId, cx: &mut Context<Self>) {
        if let Some(k) = self.space.kanbans.get_mut(&block) {
            k.picking = !k.picking;
            cx.notify();
        }
    }

    /// A new board, shown in `block`.
    pub(crate) fn new_board(&mut self, block: BlockId, cx: &mut Context<Self>) {
        let board = self.boards.add_board();
        self.show_board(block, board, cx);
    }

    /// Deletes a board and its cards; the blocks that showed it show the board list.
    pub(crate) fn delete_board(&mut self, board: u64, cx: &mut Context<Self>) {
        self.boards.remove_board(board);
        cx.notify();
    }

    /// Starts a terminal in a new focused block, in the configured start folder.
    pub(crate) fn open(&mut self, rect: Rect, window: &mut Window, cx: &mut Context<Self>) {
        let folder = self.start_dir(window, cx);
        self.spawn_block(rect, folder, window, cx);
    }

    /// Starts a terminal in `folder` (None: the home folder) in a new focused block; on failure,
    /// a notification instead.
    fn spawn_block(
        &mut self,
        rect: Rect,
        folder: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<BlockId> {
        let (cols, rows) = chrome::grid_size(chrome::content_rect(rect).size(), self.cell.size);
        let command = self.command.clone().or_else(|| {
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
            working_directory: folder,
            integration: self
                .integration
                .clone()
                .filter(|_| self.config.shell.integration),
        };
        let id = match Terminal::spawn(opts, self.notify.clone()) {
            Ok(term) => {
                let id = self.space.canvas.add(rect);
                self.space.terminals.insert(
                    id,
                    Pane {
                        term,
                        snapshot: Rc::default(),
                        heard: (0, 0),
                        alert: None,
                    },
                );
                self.space.focused = Some(id);
                self.space.appearing.insert(id, Instant::now());
                Some(id)
            }
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

    /// Puts a saved canvas back: the camera, then each block back to front, each in its folder.
    fn restore(&mut self, saved: session::Saved, window: &mut Window, cx: &mut Context<Self>) {
        self.space.pristine = false;
        let c = saved.camera;
        if c.x.is_finite() && c.y.is_finite() {
            self.space.canvas.camera = Camera::at(Point::new(c.x, c.y), c.zoom);
        }
        let mut ids = Vec::new();
        let mut gone = Vec::new();
        let mut failed = false;
        for item in &saved.items {
            let t = match item {
                session::Item::Terminal(t) => t,
                session::Item::Kanban(k) => {
                    let id = Self::kanban_rect(k.rect.into()).map(|rect| {
                        self.add_kanban(
                            rect,
                            Kanban {
                                board: k.board,
                                picking: false,
                            },
                        )
                    });
                    ids.push(id);
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
            let id = self.spawn_block(rect, folder, window, cx);
            failed |= id.is_none();
            // The shell reads it once it is ready.
            let agent = t
                .agent
                .as_ref()
                .and_then(|a| self.agents.iter().find(|known| known.id == *a));
            if let (Some(id), Some(agent), true) = (id, agent, in_its_folder) {
                let line = format!("{}\r", agent.resume_command());
                self.space.terminals[&id].term.write(line.into_bytes());
            }
            ids.push(id);
        }
        self.space.focused = saved.focused.and_then(|i| ids.get(i).copied().flatten());
        if failed {
            // A shell setting with a typo: fixing it must bring the canvas back, so it isn't
            // written over with the blocks that couldn't start.
            self.space.frozen = true;
            let message =
                "Some terminals couldn't start, so this session won't save their canvas: \
                           the saved one stays for the next start"
                    .to_string();
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::warning(message));
        }
        if !gone.is_empty() {
            let message = format!(
                "Gone, so their terminals start in the home folder: {}",
                gone.join(", ")
            );
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::warning(message));
        }
        self.space.recent = saved.recent.iter().map(PathBuf::from).collect();
        self.space.saved = Some(saved);
    }

    /// The agents to look for in terminals, for a snapshot's background read.
    fn known_agents(&self) -> Arc<Vec<templum_terminal::Detect>> {
        Arc::new(self.agents.iter().map(AgentEntry::detect).collect())
    }

    /// Every workspace: the active one, then the hidden ones.
    pub(crate) fn spaces(&self) -> impl Iterator<Item = &Space> {
        std::iter::once(&self.space).chain(&self.others)
    }

    /// The workspaces in the sidebar's order.
    pub(crate) fn order(&self) -> Vec<&Space> {
        let mut order: Vec<&Space> = self.others.iter().collect();
        order.insert(self.active, &self.space);
        order
    }

    fn space_by_id_mut(&mut self, id: &str) -> Option<&mut Space> {
        std::iter::once(&mut self.space)
            .chain(&mut self.others)
            .find(|s| s.id == id)
    }

    /// Every session::EVERY: reads the processes and writes each workspace's file in the
    /// background, when its snapshot differs from the last one written.
    fn snapshot(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.data_dir.clone() else {
            return;
        };
        if self.snapshotting {
            return;
        }
        self.snapshotting = true;
        self.generation += 1;
        let generation = self.generation;
        let known = self.known_agents();
        let jobs: Vec<_> = self
            .spaces()
            .filter(|s| !s.frozen)
            .map(|s| (s.id.clone(), s.layout(known.clone()), s.saved.clone()))
            .collect();
        let boards = (!self.boards_frozen && self.boards_saved.as_ref() != Some(&self.boards))
            .then(|| self.boards.clone());
        cx.spawn_in(window, async move |ws, cx| {
            let (done, boards) = cx
                .background_executor()
                .spawn(async move {
                    let done = jobs
                        .into_iter()
                        .map(|(id, layout, last)| {
                            let saved = layout.snapshot();
                            let written = (last.as_ref() != Some(&saved))
                                .then(|| session::save(&dir, &id, &saved, generation));
                            (id, saved, written)
                        })
                        .collect::<Vec<_>>();
                    let boards = boards.map(|b| {
                        let written = board::save(&dir, &b, generation);
                        (b, written)
                    });
                    (done, boards)
                })
                .await;
            ws.update_in(cx, |ws, window, cx| {
                ws.snapshot_done(done, boards, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn snapshot_done(
        &mut self,
        done: Vec<(String, session::Saved, Option<std::io::Result<()>>)>,
        boards: Option<(Boards, std::io::Result<()>)>,
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
        for (id, saved, written) in done {
            match written {
                None => {}
                Some(Ok(())) => {
                    wrote = true;
                    // A workspace deleted meanwhile is gone: nothing to update.
                    if let Some(space) = self.space_by_id_mut(&id) {
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
        cx.notify();
    }

    /// When the window closes or Templum quits: the last snapshot of every workspace, read and
    /// written at once.
    fn save_canvas_now(&mut self) {
        let Some(dir) = self.data_dir.clone() else {
            return;
        };
        self.generation += 1;
        let generation = self.generation;
        if !self.boards_frozen && self.boards_saved.as_ref() != Some(&self.boards) {
            match board::save(&dir, &self.boards, generation) {
                Ok(()) => self.boards_saved = Some(self.boards.clone()),
                Err(err) => eprintln!("templum: couldn't save the boards: {err}"),
            }
        }
        let known = self.known_agents();
        for space in std::iter::once(&mut self.space).chain(&mut self.others) {
            if space.frozen {
                continue;
            }
            let saved = space.layout(known.clone()).snapshot();
            if space.saved.as_ref() == Some(&saved) {
                continue;
            }
            match session::save(&dir, &space.id, &saved, generation) {
                Ok(()) => space.saved = Some(saved),
                Err(err) => eprintln!("templum: couldn't save the canvas: {err}"),
            }
        }
    }

    /// A saved rect made safe: None with a number that isn't finite; its size kept between the
    /// smallest block and MAX_COLS x MAX_ROWS cells.
    fn restorable(&self, r: Rect) -> Option<Rect> {
        if ![r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()) {
            return None;
        }
        let min = chrome::min_block_size(self.cell.size);
        let max = chrome::block_size(MAX_COLS, MAX_ROWS, self.cell.size);
        Some(Rect::new(
            r.x,
            r.y,
            r.w.clamp(min.w, max.w),
            r.h.clamp(min.h, max.h),
        ))
    }

    /// Removes a block and kills its shell (dropping the Terminal shuts it down).
    /// The block leaves the canvas at once; it stays on screen as a ghost while it fades out.
    pub(crate) fn close(&mut self, id: BlockId, cx: &mut Context<Self>) {
        let block = self.space.canvas.get(id).copied();
        self.space.canvas.remove(id);
        self.space.appearing.remove(&id);
        let body = match self.space.terminals.remove(&id) {
            Some(pane) => Some(GhostBody::Terminal(pane)),
            None => self.space.kanbans.remove(&id).map(GhostBody::Kanban),
        };
        if let (Some(block), Some(body)) = (block, body) {
            self.space.ghosts.push(Ghost {
                block,
                body,
                since: Instant::now(),
            });
        }
        if self.space.hovered == Some(id) {
            self.space.hovered = None;
        }
        if self.space.focused == Some(id) {
            self.space.focused = self.space.canvas.top();
        }
        // A gesture on a block that's gone would do nothing anyway; end it.
        self.drag = None;
        cx.notify();
    }

    /// Draws with `theme` from the next frame on, in every terminal and gpui-kit widget.
    pub(crate) fn apply(&mut self, theme: Rc<Theme>, cx: &mut Context<Self>) {
        for pane in self.spaces().flat_map(|s| s.terminals.values()) {
            pane.term.set_palette(theme.palette);
        }
        theme::apply_to_kit(&theme, cx);
        self.theme = theme;
        cx.notify();
    }

    /// The system switched between light and dark.
    pub(crate) fn set_dark(&mut self, system_dark: bool, cx: &mut Context<Self>) {
        // GPUI also reports appearance on Wayland decoration and window-control changes.
        if system_dark == self.system_dark {
            return;
        }
        self.system_dark = system_dark;
        // A preview from the other mode must not survive (Esc would restore the wrong theme).
        self.picker = None;
        self.refresh_theme(cx);
    }

    /// Picks the theme for the mode in effect (the system's, unless config.toml pins one).
    fn refresh_theme(&mut self, cx: &mut Context<Self>) {
        self.dark = self.config.theme.mode.dark(self.system_dark);
        let theme = theme::pick(
            &self.themes,
            self.config.theme.name(self.dark),
            theme::default_name(self.dark),
        );
        self.apply(theme, cx);
    }

    /// Every config::POLL: reloads config.toml when it changed (or appeared, or went away).
    fn poll_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stamp = config::Stamp::of(self.config_dir.as_deref());
        if stamp != self.config_stamp {
            // The panel's unsaved changes go in first: saving re-reads the file, so the reload
            // brings back both them and what changed by hand. If the file still doesn't parse,
            // the reload says so.
            if !self.unsaved.is_empty() {
                self.save();
            }
            self.config_stamp = config::Stamp::of(self.config_dir.as_deref());
            self.reload_config(window, cx);
        }
    }

    /// Reads config.toml again and applies what changed. An invalid file keeps the current
    /// config and says why.
    pub(crate) fn reload_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match config::load(self.config_dir.as_deref()) {
            Ok(new) => {
                if std::mem::take(&mut self.config_failed) {
                    show_notification(
                        window,
                        cx,
                        Notification::info(format!("{} loaded", config::FILE)),
                    );
                }
                // A file that loads is a file a save can write again.
                self.save_error = None;
                // The panel's half-typed shell fields would hide the file's new values.
                if new.shell != self.config.shell {
                    self.drafts.clear();
                }
                self.apply_config(new, window, cx);
            }
            Err(message) => {
                eprintln!("templum: {message}");
                self.config_failed = true;
                show_notification(window, cx, Notification::error(message));
            }
        }
    }

    fn apply_config(&mut self, new: Config, window: &mut Window, cx: &mut Context<Self>) {
        let old = std::mem::replace(&mut self.config, new);
        if self.config.font != old.font {
            self.set_font(window, cx);
        }
        if self.config.theme != old.theme {
            self.picker = None;
            // A theme file may have been added since startup.
            self.themes = theme::load(self.config_dir.as_deref());
            for warning in missing_themes(&self.themes, &self.config.theme) {
                eprintln!("templum: {warning}");
                show_notification(window, cx, Notification::warning(warning));
            }
            self.refresh_theme(cx);
        }
        if self.config.cursor != old.cursor || self.config.terminal != old.terminal {
            let options = self.config.term_options();
            for pane in self.spaces().flat_map(|s| s.terminals.values()) {
                pane.term.shared().set_options(options);
            }
        }
        if self.config.agents != old.agents {
            self.agents = agents::effective(&self.config.agents);
        }
        if self.config.keys != old.keys {
            let (keymap, warnings) = Keymap::new(&self.config.keys, cfg!(target_os = "macos"))
                .expect("config::parse validated the shortcuts");
            self.keymap = keymap;
            for warning in warnings {
                eprintln!("templum: {warning}");
                show_notification(window, cx, Notification::warning(warning));
            }
        }
        cx.notify();
    }

    /// A change from the settings panel: in effect from the next frame, written SAVE_DELAY after
    /// the last one. A change that changes nothing is dropped.
    pub(crate) fn edit(
        &mut self,
        edit: Edit,
        change: impl FnOnce(&mut Config),
        cx: &mut Context<Self>,
    ) {
        // Back to the default: what was half-typed for the key goes too (drafts are keyed by it).
        if edit.value.is_none() && self.drafts.remove(&*edit.key).is_some() {
            cx.notify();
        }
        let current = self.pending.as_ref().unwrap_or(&self.config);
        let mut config = current.clone();
        change(&mut config);
        if config == *current {
            return;
        }
        self.pending = Some(config);
        self.unsaved
            .retain(|e| e.table != edit.table || e.key != edit.key);
        self.unsaved.push(edit);
        cx.notify();
    }

    /// Writes the unsaved settings changes to config.toml. If that fails they stay in effect and
    /// unsaved, for the next try, and the message is returned and kept for the panel's footer.
    fn save(&mut self) -> Option<String> {
        let edits = std::mem::take(&mut self.unsaved);
        // No config folder: changes last until Templum quits (the panel's footer says so).
        let dir = self.config_dir.clone()?;
        if edits.is_empty() {
            return None;
        }
        // The write merges in a hand edit the poll hasn't seen yet: then the poll must still reload.
        let hand_edited = config::Stamp::of(Some(&dir)) != self.config_stamp;
        match config::save_edits(&dir, &edits) {
            Ok(()) => {
                // Templum's own write: the poll must not reload it.
                if !hand_edited {
                    self.config_stamp = config::Stamp::of(Some(&dir));
                }
                self.save_error = None;
                None
            }
            Err(err) => {
                let message = format!("Not saved: {err}");
                eprintln!("templum: {message}");
                self.unsaved = edits;
                self.save_error = Some(message.clone());
                Some(message)
            }
        }
    }

    /// `save`, with a notification when it fails.
    fn save_or_notify(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(message) = self.save() {
            show_notification(window, cx, Notification::error(message));
        }
        cx.notify();
    }

    /// New cell metrics: every block keeps its rect, and its terminal gets the columns and rows
    /// that now fit.
    fn set_font(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let font = &self.config.font;
        let (cell, warning) = CellMetrics::measure(window, font.family.as_deref(), font.size);
        if let Some(warning) = warning {
            eprintln!("templum: {warning}");
            show_notification(window, cx, Notification::warning(warning));
        }
        self.cell = cell;
        for space in std::iter::once(&mut self.space).chain(&mut self.others) {
            for (id, pane) in space.terminals.iter_mut() {
                if let Some(block) = space.canvas.get(*id) {
                    let content = chrome::content_rect(block.rect).size();
                    let (cols, rows) = chrome::grid_size(content, self.cell.size);
                    pane.term.resize(cols, rows, self.cell.px());
                }
            }
        }
    }

    /// Finds the news in every workspace's terminals (see [`alert::news`]). A terminal being looked
    /// at, focused in the active window, has none, and loses its alert; another gets an alert and
    /// a notification (the first news until it is seen), also the system's when the window is in
    /// the background.
    fn hear(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let active = window.is_window_active();
        let several = !self.others.is_empty();
        let mut news = Vec::new();
        let spaces = std::iter::once(&mut self.space).chain(&mut self.others);
        for (i, space) in spaces.enumerate() {
            for (&id, pane) in &mut space.terminals {
                let status = pane.term.status();
                let new = alert::news(&status, pane.heard);
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
                        news.push((space.id.clone(), id, title, alert.clone()));
                    }
                    pane.alert = Some(alert);
                }
            }
        }
        let delivery = if self.config.terminal.notifications && !active {
            NotificationDelivery::InAppAndSystem
        } else {
            NotificationDelivery::InApp
        };
        let ws = cx.entity().downgrade();
        for (space, block, title, alert) in news {
            let ws = ws.clone();
            let key = SharedString::from(format!("alert-{space}-{}", block.0));
            let notification = Notification::new()
                .id1::<Alert>(key)
                .with_type(alert.kind())
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

    /// Shows a block: its workspace, then the block focused, the camera on it if it was off screen.
    pub(crate) fn reveal(
        &mut self,
        space: &str,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(to) = self.order().iter().position(|s| s.id == space) else {
            return;
        };
        self.switch_to(to, window, cx);
        let Some(rect) = self.space.canvas.get(block).map(|b| b.rect) else {
            return;
        };
        self.focus_block(block, window, cx);
        let vp = self.viewport(window);
        let camera = self.space.canvas.camera;
        let shown = camera.rect_to_screen(rect);
        if shown.x + shown.w < 0.0 || shown.y + shown.h < 0.0 || shown.x > vp.w || shown.y > vp.h {
            let z = camera.zoom();
            let mut to = camera;
            to.origin = Point::new(
                rect.x + rect.w / 2.0 - vp.w / 2.0 / z,
                rect.y + rect.h / 2.0 - vp.h / 2.0 / z,
            );
            self.fly(to, self.center(window), cx);
        }
    }

    /// Where a new terminal starts (None: the home folder).
    fn start_dir(&self, window: &mut Window, cx: &mut App) -> Option<PathBuf> {
        match &self.config.shell.working_directory {
            WorkingDirectory::Inherit => self
                .space
                .focused
                .and_then(|id| self.space.terminals.get(&id))
                .and_then(|pane| pane.term.cwd()),
            WorkingDirectory::Home => None,
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

    /// Opens the theme picker (re-reading the user's themes folder), or closes it like Esc.
    pub(crate) fn toggle_theme_picker(&mut self, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            self.cancel_theme(cx);
            return;
        }
        self.themes = theme::load(self.config_dir.as_deref());
        let selected = self
            .themes
            .iter()
            .position(|e| e.theme.name == self.theme.name);
        self.picker = Some(Picker {
            before: self.theme.clone(),
            selected,
            error: None,
        });
        cx.notify();
    }

    pub(crate) fn preview_theme(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(theme) = self.themes.get(i).map(|e| e.theme.clone()) else {
            return;
        };
        if let Some(picker) = &mut self.picker {
            picker.selected = Some(i);
        }
        self.apply(theme, cx);
    }

    /// ↑/↓ in the picker, wrapping around.
    fn move_theme(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = self.themes.len() as i32;
        if n == 0 {
            return;
        }
        let at = self
            .picker
            .as_ref()
            .and_then(|p| p.selected)
            .map_or(if delta > 0 { -1 } else { 0 }, |i| i as i32);
        self.preview_theme((at + delta).rem_euclid(n) as usize, cx);
    }

    /// Keeps theme `i` for the current mode and saves it. If saving fails, the choice still
    /// applies and the picker stays open to say why.
    pub(crate) fn choose_theme(&mut self, i: usize, cx: &mut Context<Self>) {
        self.preview_theme(i, cx);
        let name = self.theme.name.clone();
        let key = if self.dark { "dark" } else { "light" };
        for config in [Some(&mut self.config), self.pending.as_mut()]
            .into_iter()
            .flatten()
        {
            if self.dark {
                config.theme.dark = name.clone();
            } else {
                config.theme.light = name.clone();
            }
        }
        let saved = match &self.config_dir {
            Some(dir) => config::save_theme(dir, self.dark, &name),
            None => Err(config::SaveError::NoDir),
        };
        match saved {
            Ok(()) => {
                // A panel change of the same theme, not written yet, would write over this choice.
                self.unsaved.retain(|e| e.table != "theme" || e.key != key);
                self.picker = None;
            }
            Err(err) => {
                eprintln!("templum: couldn't save the theme: {err}");
                if let Some(picker) = &mut self.picker {
                    picker.before = self.theme.clone();
                    picker.error = Some(format!("Not saved: {err}"));
                }
            }
        }
        cx.notify();
    }

    /// Esc or a click outside: back to the theme from before the picker opened.
    pub(crate) fn cancel_theme(&mut self, cx: &mut Context<Self>) {
        if let Some(picker) = self.picker.take() {
            self.apply(picker.before, cx);
        }
    }

    /// Opens the settings panel (re-reading the themes folder for its lists), or closes it.
    pub(crate) fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.as_ref().is_some_and(|p| !p.closing) {
            self.close_settings(window, cx);
            return;
        }
        self.themes = theme::load(self.config_dir.as_deref());
        self.fonts = settings::monospace_families(window);
        self.shells = templum_terminal::available_shells();
        self.default_shell = templum_terminal::default_shell_label();
        let now = Instant::now();
        self.settings = Some(match &self.settings {
            // Still sliding out: it turns back from where it is.
            Some(closing) => closing.reversed(now),
            None => Panel::new(now),
        });
        window.focus(&self.settings_focus, cx);
        cx.notify();
    }

    /// Shows another section of the settings panel.
    pub(crate) fn set_section(&mut self, section: settings::Section, cx: &mut Context<Self>) {
        if let Some(panel) = &mut self.settings {
            panel.section = section;
            cx.notify();
        }
    }

    /// Slides the panel out, saves what's pending, and gives the keyboard back to the terminals.
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(panel) = &self.settings
            && !panel.closing
        {
            self.settings = Some(panel.reversed(Instant::now()));
        }
        self.drafts.clear();
        self.recording = None;
        self.key_note = None;
        self.save_or_notify(window, cx);
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Keys while the panel has the focus (and its fields left them): the theme picker's keys, the
    /// shortcuts that don't act on a terminal, and Esc to close it.
    pub(crate) fn on_settings_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = self.recording {
            // While recording, no shortcut runs and nothing reaches a terminal.
            cx.stop_propagation();
            // A held key repeats: only its first press counts.
            if e.is_held {
                return;
            }
            let k = &e.keystroke;
            let bare = !k.modifiers.modified();
            match k.key.as_str() {
                "escape" if bare => {
                    self.recording = None;
                    self.key_note = None;
                }
                "backspace" if bare => self.set_shortcut(command, Keys::Many(Vec::new()), cx),
                _ if !stands_as_shortcut(k) => {
                    self.key_note = Some(
                        if cfg!(target_os = "macos") {
                            "Add Cmd, Ctrl or Option, or use an F key"
                        } else if cfg!(windows) {
                            "Add Ctrl, Alt or Win, or use an F key"
                        } else {
                            "Add Ctrl, Alt or Super, or use an F key"
                        }
                        .to_string(),
                    );
                }
                _ => match self.keymap.command(k) {
                    Some(owner) if owner != command => {
                        self.key_note = Some(format!(
                            "{} is already {}",
                            keys::display(&k.unparse()),
                            owner.title()
                        ));
                    }
                    _ => self.set_shortcut(command, Keys::One(k.unparse()), cx),
                },
            }
            cx.notify();
            return;
        }
        // A shortcut just recorded may still be held: its repeats mustn't run it.
        if e.is_held {
            return;
        }
        // Copy, paste and close act on the focused terminal, which doesn't have the keyboard now.
        let command = self.keymap.command(&e.keystroke).filter(|c| {
            !matches!(
                c,
                Command::Copy
                    | Command::Paste
                    | Command::CloseTerminal
                    | Command::PreviousPrompt
                    | Command::NextPrompt
            )
        });
        if self.picker.is_some() || command.is_some() {
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
        } else if e.keystroke.key == "escape" {
            self.close_settings(window, cx);
            cx.stop_propagation();
        }
    }

    /// The next key press with a main key becomes `command`'s only shortcut.
    pub(crate) fn record(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        self.recording = Some(command);
        self.key_note = None;
        window.focus(&self.settings_focus, cx);
        cx.notify();
    }

    /// "Reset to default" of a shortcut. "Reset All" asks for several in one go, so they are
    /// decided together once the event is over.
    pub(crate) fn reset_shortcut(
        &mut self,
        command: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.resetting.push(command);
        if self.resetting.len() == 1 {
            cx.defer_in(window, |ws, window, cx| ws.finish_resets(window, cx));
        }
    }

    /// A reset is refused while a command that keeps its shortcuts has one of the defaults, which
    /// would then be set twice. Commands reset together may trade keys (their defaults differ).
    fn finish_resets(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let macos = cfg!(target_os = "macos");
        let mut resetting = std::mem::take(&mut self.resetting);
        let keys = &self.pending.as_ref().unwrap_or(&self.config).keys;
        let (keymap, _) = Keymap::new(keys, macos).expect("config::parse validated the shortcuts");
        // Refusing one can make another's default stay taken: repeat until nothing changes.
        while let Some((i, owner, default)) = resetting.iter().enumerate().find_map(|(i, c)| {
            c.defaults(macos).iter().find_map(|default| {
                let typed = Keystroke::parse(default).expect("the default shortcuts parse");
                keymap
                    .command(&typed)
                    .filter(|owner| owner != c && !resetting.contains(owner))
                    .map(|owner| (i, owner, *default))
            })
        }) {
            resetting.remove(i);
            let message = format!(
                "Not reset: {} is {} now",
                keys::display(default),
                owner.title()
            );
            show_notification(window, cx, Notification::warning(message));
        }
        for command in resetting {
            self.edit(
                Edit::remove("keys", command.name()),
                |c| c.keys.set(command, None),
                cx,
            );
        }
    }

    fn set_shortcut(&mut self, command: Command, keys: Keys, cx: &mut Context<Self>) {
        self.recording = None;
        self.key_note = None;
        let value: toml_edit::Value = match &keys {
            Keys::One(one) => one.as_str().into(),
            Keys::Many(many) => many
                .iter()
                .map(String::as_str)
                .collect::<toml_edit::Array>()
                .into(),
        };
        self.edit(
            Edit::set("keys", command.name(), value),
            |c| c.keys.set(command, Some(keys)),
            cx,
        );
    }

    pub(crate) fn focus_block(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        self.space.pristine = false;
        self.space.canvas.raise(id);
        self.space.focused = Some(id);
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Every BLINK: toggles a blinking cursor, and makes sure a steady one is shown.
    pub(crate) fn tick_blink(&mut self, cx: &mut Context<Self>) {
        let next = !self.cursor_blinks || !self.blink_on;
        if next != self.blink_on {
            self.blink_on = next;
            cx.notify();
        }
    }

    pub(crate) fn hover_block(&mut self, id: BlockId, hovered: bool, cx: &mut Context<Self>) {
        let next = if hovered {
            Some(id)
        } else if self.space.hovered == Some(id) {
            None
        } else {
            self.space.hovered
        };
        if next != self.space.hovered {
            self.space.hovered = next;
            cx.notify();
        }
    }

    fn local(&self, pos: PixelPoint<Pixels>) -> Point {
        Point::new(
            f32::from(pos.x - self.origin.x),
            f32::from(pos.y - self.origin.y),
        )
    }

    pub(crate) fn begin_move(&mut self, id: BlockId, pos: PixelPoint<Pixels>) {
        let p = self.local(pos);
        self.drag = Gesture::move_block(&self.space.canvas, id, p).map(Drag::Canvas);
    }

    pub(crate) fn begin_resize(&mut self, id: BlockId, pos: PixelPoint<Pixels>) {
        let p = self.local(pos);
        let min = if self.space.kanbans.contains_key(&id) {
            kanban::MIN
        } else {
            chrome::min_block_size(self.cell.size)
        };
        self.drag = Gesture::resize_block(&self.space.canvas, id, p, min).map(Drag::Canvas);
    }

    fn drag_to(&mut self, pos: PixelPoint<Pixels>, cx: &mut Context<Self>) {
        let p = self.local(pos);
        match &mut self.drag {
            Some(Drag::Canvas(gesture)) => {
                if let Some((id, rect)) = gesture.update(&mut self.space.canvas, p) {
                    let (cols, rows) =
                        chrome::grid_size(chrome::content_rect(rect).size(), self.cell.size);
                    if let Some(pane) = self.space.terminals.get_mut(&id) {
                        pane.term.resize(cols, rows, self.cell.px());
                    }
                }
            }
            Some(Drag::Select(id)) => {
                let id = *id;
                if let (Some((row, col, side)), Some(pane)) =
                    (self.cell_under(id, pos), self.space.terminals.get(&id))
                {
                    pane.term.shared().select_update(row, col, side);
                }
            }
            None => return,
        }
        cx.notify();
    }

    /// The terminal cell under a window position: (row, col, side), clamped by the terminal.
    fn cell_under(&self, id: BlockId, pos: PixelPoint<Pixels>) -> Option<(usize, usize, Side)> {
        let block = self.space.canvas.get(id)?;
        let world = self.space.canvas.camera.to_world(self.local(pos));
        let (row, col, right) = chrome::cell_at(block.rect, world, self.cell.size);
        let side = if right { Side::Right } else { Side::Left };
        Some((row, col, side))
    }

    /// Mouse down in a body: 1 click selects by character, 2 by word, 3 by line.
    pub(crate) fn begin_select(&mut self, id: BlockId, e: &MouseDownEvent) {
        let Some((row, col, side)) = self.cell_under(id, e.position) else {
            return;
        };
        let Some(pane) = self.space.terminals.get(&id) else {
            return;
        };
        let kind = match e.click_count {
            1 => SelectionKind::Simple,
            2 => SelectionKind::Word,
            _ => SelectionKind::Line,
        };
        pane.term.shared().select_start(row, col, side, kind);
        self.drag = Some(Drag::Select(id));
    }

    pub(crate) fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let text = self
            .space
            .focused
            .and_then(|id| self.space.terminals.get(&id))
            .and_then(|pane| pane.term.shared().selection_text());
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    /// The canvas area: the window minus the title bar, the sidebar and, on Linux, the
    /// client-side shadow.
    pub(crate) fn viewport(&self, window: &Window) -> Size {
        let s = window.viewport_size();
        let pad = window_paddings(window);
        let sidebar = if self.sidebar { sidebar::WIDTH } else { 0.0 };
        Size::new(
            f32::from(s.width - pad.left - pad.right) - sidebar,
            f32::from(s.height - pad.top - pad.bottom - TITLE_BAR_HEIGHT),
        )
    }

    fn center(&self, window: &Window) -> Point {
        let vp = self.viewport(window);
        Point::new(vp.w / 2.0, vp.h / 2.0)
    }

    /// Moves the camera to `to`: the view glides there from where it is drawn, pivoting on the
    /// screen point `pivot`.
    fn fly(&mut self, to: Camera, pivot: Point, cx: &mut Context<Self>) {
        self.space.pristine = false;
        if to == self.space.canvas.camera {
            return;
        }
        // Mid-flight, from where it is drawn; otherwise the camera is what was last drawn (or
        // moved since, by a drag).
        let from = if self.space.flight.is_some() {
            self.space.view
        } else {
            self.space.canvas.camera
        };
        self.space.canvas.camera = to;
        let flight = CameraFlight { from, to, pivot };
        self.space.flight = Some((flight, Instant::now()));
        cx.notify();
    }

    /// `delta` zoom steps, keeping the world point under `anchor` (screen units) still.
    pub(crate) fn zoom_to(&mut self, delta: i32, anchor: Point, cx: &mut Context<Self>) {
        let mut to = self.space.canvas.camera;
        to.zoom_step(delta, anchor);
        self.fly(to, anchor, cx);
    }

    /// Pans the camera, and the flight under way with it, so the view doesn't jump.
    pub(crate) fn pan_by(&mut self, screen_delta: Point) {
        self.space.canvas.camera.pan_by(screen_delta);
        if let Some((flight, _)) = &mut self.space.flight {
            flight.pan_by(screen_delta);
        }
    }

    fn zoom_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_to(1, self.center(window), cx);
    }

    fn zoom_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_to(-1, self.center(window), cx);
    }

    pub(crate) fn zoom_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let center = self.center(window);
        let mut to = self.space.canvas.camera;
        to.zoom_reset(center);
        self.fly(to, center, cx);
    }

    fn fit_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let vp = self.viewport(window);
        let mut to = self.space.canvas.camera;
        match self.space.canvas.bounds() {
            Some(bounds) => to.fit(bounds, vp),
            None => to.zoom_reset(Point::new(vp.w / 2.0, vp.h / 2.0)),
        }
        self.fly(to, self.center(window), cx);
    }

    pub(crate) fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(pane) = self
            .space
            .focused
            .and_then(|id| self.space.terminals.get(&id))
        {
            pane.term.paste(&text);
        }
    }

    fn end_drag(&mut self) {
        self.drag = None;
        self.press = None;
    }

    /// Mouse up: a press on the empty canvas that didn't pan is a click, and leaves no terminal
    /// focused (typing then reaches none; panning keeps the focus).
    fn release(&mut self, pos: PixelPoint<Pixels>, cx: &mut Context<Self>) {
        if let Some(start) = self.press {
            let p = self.local(pos);
            let moved = (p.x - start.x).abs().max((p.y - start.y).abs());
            if moved < CLICK_SLOP && self.space.focused.take().is_some() {
                cx.notify();
            }
        }
        self.end_drag();
    }

    pub(crate) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.space.pristine = false;
        let vp = self.viewport(window);
        let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
        let center = self
            .space
            .canvas
            .camera
            .to_world(Point::new(vp.w / 2.0, vp.h / 2.0));
        let near = Point::new(center.x - size.w / 2.0, center.y - size.h / 2.0);
        let rect = self.space.canvas.place_new(size, near);
        self.open(rect, window, cx);
    }

    fn close_terminal(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.space.focused {
            self.close(id, cx);
        }
    }

    fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::NewTerminal => self.new_terminal(window, cx),
            Command::CloseTerminal => self.close_terminal(cx),
            Command::Copy => self.copy_selection(cx),
            Command::Paste => self.paste_clipboard(cx),
            Command::PreviousPrompt => self.jump_to_prompt(true, cx),
            Command::NextPrompt => self.jump_to_prompt(false, cx),
            Command::ZoomIn => self.zoom_in(window, cx),
            Command::ZoomOut => self.zoom_out(window, cx),
            Command::ZoomReset => self.zoom_reset(window, cx),
            Command::FitAll => self.fit_all(window, cx),
            Command::ThemePicker => self.toggle_theme_picker(cx),
            Command::OpenConfig => self.open_config(window, cx),
            Command::Settings => self.toggle_settings(window, cx),
            Command::NewKanban => self.new_kanban(window, cx),
            Command::NextWorkspace => self.cycle_space(1, window, cx),
            Command::PreviousWorkspace => self.cycle_space(-1, window, cx),
            Command::NewWorkspace => self.new_space(window, cx),
            Command::ToggleSidebar => self.toggle_sidebar(window, cx),
            Command::Quit => self.quit(window, cx),
        }
    }

    /// Scrolls the focused terminal to its previous or next prompt (shell integration marks them).
    fn jump_to_prompt(&mut self, up: bool, cx: &mut Context<Self>) {
        let pane = self
            .space
            .focused
            .and_then(|id| self.space.terminals.get(&id));
        if pane.is_some_and(|pane| pane.term.jump_to_prompt(up)) {
            cx.notify();
        }
    }

    /// Quitting closes every terminal, and Q sits next to W (close one terminal): the first
    /// press only says to press again. Pending settings are saved on the way out.
    fn quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self
            .quit_armed
            .take()
            .is_some_and(|at| now.duration_since(at) < QUIT_AGAIN)
        {
            cx.quit();
            return;
        }
        self.quit_armed = Some(now);
        let shortcut = self
            .keymap
            .first(Command::Quit)
            .map(keys::display)
            .unwrap_or_else(|| "the quit shortcut".to_string());
        show_notification(
            window,
            cx,
            Notification::info(format!("Press {shortcut} again to quit")),
        );
    }

    /// Opens config.toml in the system's editor, writing the commented template first if needed.
    pub(crate) fn open_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.config_dir.clone() else {
            let message = "No config folder: neither HOME nor APPDATA is set".to_string();
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::error(message));
            return;
        };
        match config::ensure_file(&dir) {
            Ok(path) => cx.open_with_system(&path),
            Err(err) => {
                let message = format!(
                    "Couldn't create {}: {err}",
                    dir.join(config::FILE).display()
                );
                eprintln!("templum: {message}");
                show_notification(window, cx, Notification::error(message));
            }
        }
    }

    /// The empty canvas: the name, a line about it, and how to start, with the shortcuts in
    /// effect.
    fn hint(&self) -> impl IntoElement {
        let ui = self.ui;
        let pill = move |text: &'static str| settings::chip(div(), ui).child(text);
        let row = |keys: AnyElement, label: &'static str| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(24.0))
                .w(px(280.0))
                .child(div().text_color(ui.title).child(label))
                .child(keys)
        };
        let [new, settings] = hint_keys(&self.keymap);
        let keys = |stroke: Option<Keystroke>| match stroke {
            Some(stroke) => settings::chip(Kbd::new(stroke), ui).into_any_element(),
            None => pill("No shortcut").into_any_element(),
        };
        div()
            .id("empty-hint")
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(28.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(crate::grid::hsla(self.theme.palette.foreground, 1.0))
                    .child("Templum"),
            )
            .child(
                div()
                    .text_size(px(14.0))
                    .text_color(ui.hint)
                    .child("An infinite canvas of terminals"),
            )
            .child(
                div()
                    .mt(px(20.0))
                    .flex()
                    .flex_col()
                    .gap(px(10.0))
                    .text_size(px(13.0))
                    .child(row(keys(new), "New terminal"))
                    .child(row(
                        pill("Double-click").into_any_element(),
                        "Open one where you click",
                    ))
                    .child(row(
                        pill(if cfg!(target_os = "macos") {
                            "Hold Option"
                        } else {
                            "Hold Alt"
                        })
                        .into_any_element(),
                        "Move the canvas over blocks",
                    ))
                    .child(row(keys(settings), "Settings")),
            )
            .test_support()
    }

    fn on_key_down(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // The cursor shows again on any key.
        self.blink_on = true;
        let command = self.keymap.command(&e.keystroke);
        if self.picker.is_some() || command.is_some() {
            // While the picker is open, typing and shortcuts don't reach the terminal.
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
            return;
        }
        if let Some(to) = workspace_digit(&e.keystroke, cfg!(target_os = "macos"))
            && to <= self.others.len()
        {
            self.switch_to(to, window, cx);
            cx.stop_propagation();
            return;
        }
        let Some(pane) = self
            .space
            .focused
            .and_then(|id| self.space.terminals.get(&id))
        else {
            return;
        };
        let k = &e.keystroke;
        let input = KeyInput {
            key: &k.key,
            key_char: k.key_char.as_deref(),
            ctrl: k.modifiers.control,
            alt: k.modifiers.alt
                && !option_types_text(&k.key, k.key_char.as_deref(), cfg!(target_os = "macos")),
            shift: k.modifiers.shift,
        };
        if pane.term.key(&input) {
            // Keeps the platform from also feeding key_char to the text-input path.
            cx.stop_propagation();
        }
    }

    /// The theme picker's keys while it is open, else `command`.
    fn command_key(
        &mut self,
        e: &KeyDownEvent,
        command: Option<Command>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.picker.is_some() {
            if command == Some(Command::ThemePicker) {
                self.cancel_theme(cx);
            } else {
                match e.keystroke.key.as_str() {
                    "up" => self.move_theme(-1, cx),
                    "down" => self.move_theme(1, cx),
                    "enter" => {
                        if let Some(i) = self.picker.as_ref().and_then(|p| p.selected) {
                            self.choose_theme(i, cx);
                        }
                    }
                    "escape" => self.cancel_theme(cx),
                    _ => {}
                }
            }
        } else if let Some(command) = command {
            // Holding the quit shortcut repeats it: that's not a second press.
            if !(e.is_held && command == Command::Quit) {
                self.run(command, window, cx);
            }
        }
    }

    fn on_background_down(
        &mut self,
        e: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        self.space.pristine = false;
        let p = self.local(e.position);
        if e.click_count == 2 {
            let at = self.space.canvas.camera.to_world(p);
            self.add_terminal_at(at, window, cx);
        } else {
            self.drag = Some(Drag::Canvas(Gesture::pan(p)));
            self.press = Some(p);
        }
    }

    fn on_background_scroll(
        &mut self,
        e: &ScrollWheelEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.space.pristine = false;
        let p = self.local(e.position);
        if e.modifiers.control {
            self.zoom_acc += match e.delta {
                ScrollDelta::Lines(d) => d.y * 50.0,
                ScrollDelta::Pixels(d) => f32::from(d.y),
            };
            if self.zoom_acc.abs() >= 50.0 {
                let delta = self.zoom_acc.signum() as i32;
                self.zoom_acc = 0.0;
                self.zoom_to(delta, p, cx);
            }
        } else {
            let d = e.delta.pixel_delta(px(self.cell.size.h));
            self.pan_by(Point::new(f32::from(d.x), f32::from(d.y)));
        }
        cx.notify();
    }

    pub(crate) fn on_pinch(&mut self, e: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.space.pristine = false;
        self.zoom_acc += e.delta;
        if self.zoom_acc.abs() >= 0.15 {
            let p = self.local(e.position);
            let delta = self.zoom_acc.signum() as i32;
            self.zoom_acc = 0.0;
            self.zoom_to(delta, p, cx);
        }
    }

    /// Wheel over a terminal body scrolls it; with Ctrl it bubbles up and zooms the canvas.
    pub(crate) fn scroll_terminal(
        &mut self,
        id: BlockId,
        e: &ScrollWheelEvent,
        cx: &mut Context<Self>,
    ) {
        if e.modifiers.control {
            return;
        }
        cx.stop_propagation();
        let line = self.cell.size.h * self.space.canvas.camera.zoom();
        self.scroll_acc += f32::from(e.delta.pixel_delta(px(line)).y) / line;
        let lines = self.scroll_acc.trunc();
        self.scroll_acc -= lines;
        if lines != 0.0 {
            if let Some(pane) = self.space.terminals.get(&id) {
                pane.term.scroll(lines as i32);
            }
            cx.notify();
        }
    }
}

/// A shortcut needs Ctrl, Alt or Super (Cmd), or is an F key: any other key is typing, which the
/// terminals must keep getting.
fn stands_as_shortcut(k: &Keystroke) -> bool {
    let m = &k.modifiers;
    let f_key = k
        .key
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=35).contains(&n));
    m.control || m.alt || m.platform || m.function || f_key
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

/// `folder` becomes the newest recent folder: once, and only the RECENT newest are kept.
pub(crate) fn remember(recent: &mut Vec<PathBuf>, folder: PathBuf) {
    recent.retain(|f| *f != folder);
    recent.insert(0, folder);
    recent.truncate(RECENT);
}

/// Ctrl+1 to Ctrl+9 (Cmd on macOS), with no other modifier: the position of a workspace.
fn workspace_digit(k: &Keystroke, macos: bool) -> Option<usize> {
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

/// The dot grid under the blocks: round dots centered on the world lattice.
fn paint_dots(camera: &Camera, color: Hsla, bounds: Bounds<Pixels>, window: &mut Window) {
    let grid = dot_grid(camera);
    let step = grid.spacing * camera.zoom();
    let d = dot_size(camera.zoom());
    let start = camera.to_screen(grid.first);
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let mut y = start.y;
    while y < h + d {
        let mut x = start.x;
        while x < w + d {
            let at = point(
                bounds.origin.x + px(x - d / 2.0),
                bounds.origin.y + px(y - d / 2.0),
            );
            window.paint_quad(
                fill(Bounds::new(at, size(px(d), px(d))), color).corner_radii(px(d / 2.0)),
            );
            x += step;
        }
        y += step;
    }
}

fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(mut hook) = self.on_frame.take()
            && hook(self, window, cx)
        {
            self.on_frame = Some(hook);
        }
        // Settings changes apply here, where the window is at hand; the save waits for them to settle.
        if let Some(config) = self.pending.take() {
            self.apply_config(config, window, cx);
            self._save = cx.spawn_in(window, async move |ws, cx| {
                cx.background_executor().timer(SAVE_DELAY).await;
                ws.update_in(cx, |ws, window, cx| ws.save_or_notify(window, cx))
                    .ok();
            });
        }
        let fade = transition_with_status(
            "theme-ui",
            self.theme.ui,
            Transition::new(motion::THEME),
            window,
            cx,
        );
        // Blending ends a rounding error away from the theme: take the theme's own colors.
        self.ui = match fade.status {
            MotionStatus::Running | MotionStatus::Delayed => fade.value,
            MotionStatus::Idle | MotionStatus::Finished => self.theme.ui,
        };
        // The focused terminal's painting sets it again this frame, if its cursor blinks.
        self.cursor_blinks = false;
        let exited: Vec<BlockId> = self
            .space
            .terminals
            .iter()
            .filter(|(_, pane)| pane.term.exited())
            .map(|(id, _)| *id)
            .collect();
        for id in exited {
            self.close(id, cx);
        }
        // Re-arm once per frame: output is coalesced to frame rate.
        for pane in self.space.terminals.values() {
            pane.term.rearm();
        }
        self.hear(window, cx);

        let vp = self.viewport(window);
        if self.space.pristine
            && let Some(b) = self.space.canvas.bounds()
        {
            let z = self.space.canvas.camera.zoom();
            self.space.canvas.camera.origin = Point::new(
                b.x + b.w / 2.0 - vp.w / 2.0 / z,
                b.y + b.h / 2.0 - vp.h / 2.0 / z,
            );
        }
        // Animations: drop the finished ones, and keep frames coming while any runs.
        let now = Instant::now();
        self.space
            .appearing
            .retain(|_, since| motion::progress(*since, now, motion::OPEN) < 1.0);
        self.space
            .ghosts
            .retain(|g| motion::progress(g.since, now, motion::CLOSE) < 1.0);
        if let Some(panel) = &self.settings {
            let t = motion::progress(panel.since, now, motion::SLIDE);
            if t < 1.0 {
                window.request_animation_frame();
            } else if panel.closing {
                self.settings = None;
            }
        }
        if !self.space.appearing.is_empty() || !self.space.ghosts.is_empty() {
            window.request_animation_frame();
        }
        // The view glides to the camera. Anything that moved the camera itself (a drag, the
        // startup centering) ends the flight: the view is then the camera.
        self.space.view = match self.space.flight {
            Some((flight, since)) if flight.to == self.space.canvas.camera => {
                let t = motion::progress(since, now, motion::FLIGHT);
                if t < 1.0 {
                    window.request_animation_frame();
                    flight.at(motion::slide(t))
                } else {
                    self.space.flight = None;
                    self.space.canvas.camera
                }
            }
            _ => {
                self.space.flight = None;
                self.space.canvas.camera
            }
        };
        let visible: Vec<_> = self
            .space
            .canvas
            .visible_from(&self.space.view, vp)
            .copied()
            .collect();
        let mut blocks: Vec<_> = visible
            .into_iter()
            .map(|b| {
                let look = self
                    .space
                    .appearing
                    .get(&b.id)
                    .map_or(motion::Look::REST, |since| {
                        motion::opening(motion::progress(*since, now, motion::OPEN))
                    });
                let content = match self.space.terminals.get(&b.id) {
                    Some(pane) => Content::Terminal(pane),
                    None => {
                        Content::Kanban(self.space.kanbans.get(&b.id).copied().unwrap_or_default())
                    }
                };
                block::render(self, b, content, look, true, window, cx)
            })
            .collect();
        // Ghosts fade above the blocks, where they were.
        blocks.extend(self.space.ghosts.iter().map(|g| {
            let look = motion::closing(motion::progress(g.since, now, motion::CLOSE));
            let content = match &g.body {
                GhostBody::Terminal(pane) => Content::Terminal(pane),
                GhostBody::Kanban(k) => Content::Kanban(*k),
            };
            block::render(self, g.block, content, look, false, window, cx)
        }));

        let entity = cx.entity();
        let focus = self.focus_handle.clone();
        let camera = self.space.view;
        let dot = self.ui.dot;
        // A full-size layer under the blocks: the IME handler, the workspace origin, and
        // window-wide drag listeners (drags keep working outside the window).
        let layer = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                paint_dots(&camera, dot, bounds, window);
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                entity.update(cx, |ws, _| ws.origin = bounds.origin);
                let on_move = entity.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        on_move.update(cx, |ws, cx| {
                            // Alt let go while the window wasn't listening (another window had
                            // the keyboard): the next move puts the hand right.
                            if e.modifiers.alt != ws.hand {
                                cx.notify();
                            }
                            // Any button: the middle one pans too.
                            if e.pressed_button.is_some() {
                                ws.drag_to(e.position, cx);
                            } else {
                                ws.end_drag();
                            }
                        });
                    }
                });
                let on_up = entity.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        on_up.update(cx, |ws, cx| match e.button {
                            MouseButton::Left => ws.release(e.position, cx),
                            MouseButton::Middle => ws.end_drag(),
                            _ => {}
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full();

        let hand = window.modifiers().alt;
        self.hand = hand;
        let panning = matches!(self.drag, Some(Drag::Canvas(Gesture::Pan { .. })));
        let area = div()
            .id("workspace")
            .relative()
            .flex_1()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_background_down))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(Self::on_background_right_down),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|ws, e: &MouseDownEvent, _, _| ws.begin_pan(e.position)),
            )
            .on_scroll_wheel(cx.listener(Self::on_background_scroll))
            .on_pinch(cx.listener(Self::on_pinch))
            .child(layer)
            .children(blocks)
            // The hand: while Alt is held, a layer over the blocks takes the pointer, and a drag
            // or the wheel moves the canvas wherever it is.
            .when(hand, |el| {
                el.child(
                    div()
                        .id("hand")
                        .absolute()
                        .size_full()
                        .map(|el| {
                            if panning {
                                el.cursor_grabbing()
                            } else {
                                el.cursor_grab()
                            }
                        })
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, e: &MouseDownEvent, _, cx| {
                                ws.begin_pan(e.position);
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .on_scroll_wheel(cx.listener(Self::on_background_scroll))
                        .on_pinch(cx.listener(Self::on_pinch))
                        .test_support(),
                )
            })
            .when(self.space.canvas.blocks().is_empty(), |el| {
                el.child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(self.hint()),
                )
            });
        let picker = self
            .picker
            .as_ref()
            .map(|p| theme_picker::render(self, p, cx));
        if self.recording.is_some() && !self.settings_focus.is_focused(window) {
            self.recording = None;
            self.key_note = None;
        }
        let panel = self
            .settings
            .as_ref()
            .map(|p| settings::render(self, p, now, vp.w, window, cx));
        div()
            .id("window")
            // Alt held or let go: the hand comes or goes.
            .on_modifiers_changed(cx.listener(|_, _: &ModifiersChangedEvent, _, cx| cx.notify()))
            .size_full()
            .flex()
            .flex_col()
            // The settings panel slides in from beyond the right edge.
            .overflow_hidden()
            .bg(self.ui.canvas)
            .child(title_bar::render(self, cx))
            .child(
                div()
                    .flex_1()
                    .min_h(px(0.0))
                    .flex()
                    .when(self.sidebar, |el| el.child(sidebar::render(self, cx)))
                    .child(area),
            )
            .children(panel)
            .children(picker)
            .children(self.menu.as_ref().map(|m| menu::render(m, window)))
    }
}

/// Text input path (IME, dead keys, composed chars): GPUI calls it for keys on_key_down left.
impl EntityInputHandler for Workspace {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| 0..m.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = None;
        if let Some(pane) = self
            .space
            .focused
            .and_then(|id| self.space.terminals.get(&id))
        {
            pane.term.write(text.as_bytes().to_vec());
        }
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked = (!text.is_empty()).then(|| text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.ime_cursor
    }

    fn character_index_for_point(
        &mut self,
        _: PixelPoint<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt;
    use gpui_kit::{
        AnyWindowHandle, AppContext as _, Bounds, Context, Entity, Modifiers, MouseButton,
        MouseDownEvent, MouseUpEvent, PlatformInput, ScrollDelta, ScrollWheelEvent, TestAppContext,
        TouchPhase, Window, WindowBounds, WindowOptions, point, px, size,
    };

    use super::{Workspace, bind_keys};

    #[test]
    fn a_workspace_shows_an_agent_from_its_last_snapshot() {
        use crate::session::{Item, Saved, SavedCamera, SavedRect, TerminalItem, VERSION};
        let with = |agent: Option<&str>| super::Space {
            saved: Some(Saved {
                version: VERSION,
                camera: SavedCamera {
                    x: 0.0,
                    y: 0.0,
                    zoom: 1.0,
                },
                items: vec![Item::Terminal(TerminalItem {
                    rect: SavedRect {
                        x: 0.0,
                        y: 0.0,
                        w: 1.0,
                        h: 1.0,
                    },
                    folder: None,
                    agent: agent.map(String::from),
                })],
                focused: None,
                recent: Vec::new(),
            }),
            ..super::Space::default()
        };
        let known = crate::agents::effective(&Default::default());
        assert!(with(Some("claude")).has_agent(&known));
        assert!(
            !with(Some("vim")).has_agent(&known),
            "not an agent Templum knows"
        );
        assert!(!with(None).has_agent(&known));
        assert!(
            !super::Space::default().has_agent(&known),
            "no snapshot yet"
        );
    }

    #[test]
    fn ctrl_digits_pick_a_workspace() {
        let digit =
            |k: &str, macos| super::workspace_digit(&gpui_kit::Keystroke::parse(k).unwrap(), macos);
        assert_eq!(digit("ctrl-1", false), Some(0));
        assert_eq!(digit("ctrl-9", false), Some(8));
        assert_eq!(digit("cmd-2", true), Some(1));
        assert_eq!(digit("ctrl-2", true), None, "Cmd on macOS");
        assert_eq!(digit("ctrl-0", false), None, "zoom reset");
        assert_eq!(digit("ctrl-shift-1", false), None, "fit all");
        assert_eq!(digit("ctrl-alt-1", false), None);
        assert_eq!(digit("1", false), None);
    }

    #[test]
    fn the_hint_shows_the_shortcuts_in_effect() {
        use crate::keys::{Keymap, Keys, KeysConfig};
        let (keymap, _) = Keymap::new(&KeysConfig::default(), false).unwrap();
        let rows = super::hint_keys(&keymap);
        assert_eq!(rows[0].as_ref().map(|k| k.key.as_str()), Some("t"));
        assert_eq!(rows[1].as_ref().map(|k| k.key.as_str()), Some(","));
        let keys = KeysConfig {
            new_terminal: Some(Keys::Many(vec![])),
            ..KeysConfig::default()
        };
        let (keymap, _) = Keymap::new(&keys, false).unwrap();
        assert!(
            super::hint_keys(&keymap)[0].is_none(),
            "no shortcut, no keys"
        );
    }

    #[cfg(target_os = "macos")]
    const NEW: &str = "cmd-t";
    #[cfg(not(target_os = "macos"))]
    const NEW: &str = "ctrl-shift-t";
    #[cfg(target_os = "macos")]
    const CLOSE: &str = "cmd-w";
    #[cfg(not(target_os = "macos"))]
    const CLOSE: &str = "ctrl-shift-w";

    /// A long-lived command that prints nothing, so tests don't depend on shell output.
    fn quiet() -> (String, Vec<String>) {
        #[cfg(unix)]
        return ("cat".into(), vec![]);
        #[cfg(windows)]
        return ("cmd".into(), vec!["/K".into()]);
    }

    fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Workspace>) {
        open_with(cx, None)
    }

    /// Opens a workspace whose config folder is `config_dir` (None: no config, no user themes).
    fn open_with(
        cx: &mut TestAppContext,
        config_dir: Option<std::path::PathBuf>,
    ) -> (AnyWindowHandle, Entity<Workspace>) {
        open_in(cx, config_dir, None)
    }

    /// Opens a workspace with a config folder and a data folder (None: nothing read or saved).
    fn open_in(
        cx: &mut TestAppContext,
        config_dir: Option<std::path::PathBuf>,
        data_dir: Option<std::path::PathBuf>,
    ) -> (AnyWindowHandle, Entity<Workspace>) {
        open_as(cx, Some(quiet()), config_dir, data_dir)
    }

    /// `open_in` with the program new terminals run.
    fn open_as(
        cx: &mut TestAppContext,
        command: Option<(String, Vec<String>)>,
        config_dir: Option<std::path::PathBuf>,
        data_dir: Option<std::path::PathBuf>,
    ) -> (AnyWindowHandle, Entity<Workspace>) {
        // PTY threads wake the UI from outside the test scheduler.
        cx.executor().allow_parking();
        let opened = cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
            let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.)));
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    cx.new(|cx| Workspace::with(command, config_dir, data_dir, window, cx))
                },
            )
            .expect("open window")
        });
        // The startup block at rest: tests that measure elements must not depend on how far its
        // pop-in got (new_terminals_pop_in_then_settle checks the animation itself).
        opened.1.update(cx, |ws, _| ws.space.appearing.clear());
        opened
    }

    fn click(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32), count: usize) {
        let position = point(px(at.0), px(at.1));
        let modifiers = Modifiers::default();
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: count,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers,
                click_count: count,
            }),
            cx,
        );
    }

    fn right_click(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32)) {
        let position = point(px(at.0), px(at.1));
        let modifiers = Modifiers::default();
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Right,
                position,
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Right,
                position,
                modifiers,
                click_count: 1,
            }),
            cx,
        );
    }

    /// Holds or lets go of the hand key (Alt).
    fn hold_alt(window: &mut Window, cx: &mut gpui_kit::App, held: bool) {
        window.dispatch_event(
            PlatformInput::ModifiersChanged(gpui_kit::ModifiersChangedEvent {
                modifiers: Modifiers {
                    alt: held,
                    ..Default::default()
                },
                capslock: Default::default(),
            }),
            cx,
        );
        window.render_frame(cx);
    }

    fn press_with(
        window: &mut Window,
        cx: &mut gpui_kit::App,
        button: MouseButton,
        modifiers: Modifiers,
        from: (f32, f32),
        to: (f32, f32),
    ) {
        let at = |p: (f32, f32)| point(px(p.0), px(p.1));
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button,
                position: at(from),
                modifiers,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: at(to),
                pressed_button: Some(button),
                modifiers,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button,
                position: at(to),
                modifiers,
                click_count: 1,
            }),
            cx,
        );
    }

    fn wheel(
        window: &mut Window,
        cx: &mut gpui_kit::App,
        at: (f32, f32),
        delta: ScrollDelta,
        control: bool,
    ) {
        window.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(at.0), px(at.1)),
                delta,
                modifiers: Modifiers {
                    control,
                    ..Default::default()
                },
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    }

    #[gpui_kit::test]
    fn starts_with_one_focused_terminal(cx: &mut TestAppContext) {
        let (_, ws) = open(cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.blocks().len(), 1);
            assert_eq!(ws.space.focused, ws.space.canvas.top());
            assert_eq!(ws.space.terminals.len(), 1);
        });
    }

    #[gpui_kit::test]
    fn startup_terminal_stays_centered_when_the_window_is_resized(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        // Tiling compositors resize the window right after it opens.
        cx.simulate_window_resize(handle, size(px(600.), px(400.)));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // Measured, not computed with viewport(): the canvas area starts at the workspace's
            // origin (below the title bar) and runs to the window's edge.
            let full = window.viewport_size();
            let ws = ws.read(cx);
            let vp = templum_canvas::Size::new(
                f32::from(full.width - ws.origin.x),
                f32::from(full.height - ws.origin.y),
            );
            let b = ws.space.canvas.blocks()[0].rect;
            let center = ws
                .space
                .canvas
                .camera
                .to_screen(templum_canvas::Point::new(b.x + b.w / 2.0, b.y + b.h / 2.0));
            assert!(
                (center.x - vp.w / 2.0).abs() < 1.0 && (center.y - vp.h / 2.0).abs() < 1.0,
                "startup block center on screen: {center:?}, canvas {vp:?}"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn shortcuts_open_and_close_terminals(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.blocks().len(), 2);
            assert_eq!(
                ws.space.focused,
                ws.space.canvas.top(),
                "the new terminal is focused"
            );
            // The startup block already sits at the viewport center: the new one cascades.
            let [first, second] = ws.space.canvas.blocks() else {
                unreachable!()
            };
            assert_eq!(
                (second.rect.x - first.rect.x, second.rect.y - first.rect.y),
                (templum_canvas::CASCADE, templum_canvas::CASCADE)
            );
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(CLOSE, cx);
            window.press(CLOSE, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.space.canvas.blocks().is_empty());
            assert!(ws.space.terminals.is_empty());
            assert_eq!(ws.space.focused, None);
        });
    }

    /// Draws frames (each hears the terminals) until `done`.
    fn until(
        what: &str,
        handle: AnyWindowHandle,
        ws: &Entity<Workspace>,
        cx: &mut TestAppContext,
        done: impl Fn(&Workspace) -> bool,
    ) {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            if ws.read_with(cx, |ws, _| done(ws)) {
                return;
            }
            assert!(Instant::now() < deadline, "waited for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The terminal rings its bell: `cat` prints back the BEL it reads.
    #[cfg(unix)]
    fn ring(ws: &Entity<Workspace>, id: templum_canvas::BlockId, cx: &mut TestAppContext) {
        ws.read_with(cx, |ws, _| {
            ws.space.terminals[&id].term.write(b"\x07\r".to_vec())
        });
    }

    fn set_active(handle: AnyWindowHandle, active: bool, cx: &mut TestAppContext) {
        if active {
            cx.update_window(handle, |_, window, _| window.activate_window())
                .unwrap();
            cx.run_until_parked();
        } else {
            gpui_kit::VisualTestContext::from_window(handle, cx).deactivate_window();
        }
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    fn a_bell_in_another_block_alerts_until_it_is_focused(cx: &mut TestAppContext) {
        use crate::alert::Alert;
        let (handle, ws) = open(cx);
        set_active(handle, true, cx);
        let ringing = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW, cx);
        })
        .unwrap();
        ring(&ws, ringing, cx);
        until("the alert", handle, &ws, cx, |ws| {
            ws.space.terminals[&ringing].alert.is_some()
        });
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.space.terminals[&ringing].alert,
                Some(Alert::Attention(None))
            );
        });
        assert!(
            cx.shown_system_notifications().is_empty(),
            "the window is in front: only the in-app one"
        );
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.focus_block(ringing, window, cx))
        })
        .unwrap();
        until("the alert to go", handle, &ws, cx, |ws| {
            ws.space.terminals[&ringing].alert.is_none()
        });
        // The focused terminal, looked at: no news.
        ring(&ws, ringing, cx);
        until("the bell", handle, &ws, cx, |ws| {
            ws.space.terminals[&ringing].heard.1 == 2
        });
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.terminals[&ringing].alert, None)
        });
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    fn in_the_background_a_bell_is_a_system_notification_unless_turned_off(
        cx: &mut TestAppContext,
    ) {
        let (handle, ws) = open(cx);
        // As main does: without it, no system notification (on Windows too).
        cx.update(|cx| cx.set_app_identity("test.Templum", "Templum"));
        let id = first_id(&ws, cx);
        // Rings, then draws until it has been heard `bells` times, and the frame after (when
        // notifications are shown).
        let bell = |bells: u64, cx: &mut TestAppContext| {
            ring(&ws, id, cx);
            until("the bell", handle, &ws, cx, |ws| {
                ws.space.terminals[&id].heard.1 == bells
            });
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap();
            cx.run_until_parked();
        };
        set_active(handle, false, cx);
        ws.update(cx, |ws, _| ws.config.terminal.notifications = false);
        bell(1, cx);
        assert!(ws.read_with(cx, |ws, _| ws.space.terminals[&id].alert.is_some()));
        assert_eq!(cx.shown_system_notifications().len(), 0, "turned off");
        // Looked at, then in the background again.
        set_active(handle, true, cx);
        until("the alert to go", handle, &ws, cx, |ws| {
            ws.space.terminals[&id].alert.is_none()
        });
        set_active(handle, false, cx);
        ws.update(cx, |ws, _| ws.config.terminal.notifications = true);
        bell(2, cx);
        let shown = cx.shown_system_notifications();
        assert_eq!(shown.len(), 1, "{shown:?}");
        assert_eq!(shown[0].title.as_ref(), "cat");
        assert_eq!(shown[0].body.as_ref(), "Needs your attention");
        bell(3, cx);
        assert_eq!(
            cx.shown_system_notifications().len(),
            1,
            "not again until it is seen"
        );
    }

    fn first_id(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> templum_canvas::BlockId {
        ws.read_with(cx, |ws, _| ws.space.canvas.top().unwrap())
    }

    #[gpui_kit::test]
    fn double_click_on_empty_canvas_opens_a_terminal(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // The canvas starts below the title bar.
            let o = ws.read(cx).origin;
            let at = (f32::from(o.x) + 40.0, f32::from(o.y) + 40.0);
            click(window, cx, at, 1);
            click(window, cx, at, 2);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.blocks().len(), 2);
            let new = ws.space.canvas.get(ws.space.canvas.top().unwrap()).unwrap();
            assert_eq!((new.rect.x, new.rect.y), (40.0, 40.0));
        });
    }

    #[gpui_kit::test]
    fn close_button_removes_the_block(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.hover(("block", id.0 as usize), cx);
            window.render_frame(cx);
            window.click(("close", id.0 as usize), cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert!(ws.space.canvas.blocks().is_empty()));
    }

    #[gpui_kit::test]
    fn dragging_the_title_moves_the_block(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let before = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(("title", id.0 as usize)).bounds().center();
            window.drag(from, point(from.x + px(100.), from.y + px(50.)), cx);
        })
        .unwrap();
        let after = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
        assert_eq!((after.x - before.x, after.y - before.y), (100.0, 50.0));
        assert_eq!((after.w, after.h), (before.w, before.h));
    }

    #[gpui_kit::test]
    fn dragging_the_handle_resizes_block_and_terminal(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let (before, cell) = ws.read_with(cx, |ws, _| {
            (ws.space.canvas.get(id).unwrap().rect, ws.cell.size)
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.hover(("block", id.0 as usize), cx);
            window.render_frame(cx);
            let from = window.find(("handle", id.0 as usize)).bounds().center();
            let to = point(from.x + px(cell.w * 10.0), from.y + px(cell.h * 3.0));
            window.drag(from, to, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            let rect = ws.space.canvas.get(id).unwrap().rect;
            assert!(rect.w > before.w && rect.h > before.h);
            let expected = templum_canvas::chrome::grid_size(
                templum_canvas::chrome::content_rect(rect).size(),
                cell,
            );
            assert_eq!(ws.space.terminals[&id].term.grid_size(), expected);
            assert_eq!(
                expected,
                (super::DEFAULT_COLS + 10, super::DEFAULT_ROWS + 3)
            );
        });
    }

    #[gpui_kit::test]
    fn wheel_on_empty_canvas_pans_and_ctrl_wheel_zooms(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let o = ws.read(cx).origin;
            wheel(
                window,
                cx,
                (f32::from(o.x) + 30.0, f32::from(o.y) + 30.0),
                ScrollDelta::Pixels(point(px(0.), px(100.))),
                false,
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.camera.origin.y, -100.0)
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let o = ws.read(cx).origin;
            wheel(
                window,
                cx,
                (f32::from(o.x) + 30.0, f32::from(o.y) + 30.0),
                ScrollDelta::Lines(point(0.0, 1.0)),
                true,
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.space.canvas.camera.zoom(), 1.1));
    }

    #[cfg(target_os = "macos")]
    const COPY: &str = "cmd-c";
    #[cfg(not(target_os = "macos"))]
    const COPY: &str = "ctrl-shift-c";

    #[gpui_kit::test]
    fn copy_puts_the_selection_on_the_clipboard(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        use templum_terminal::{SelectionKind, Side, Snapshot};
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        // `cat` echoes its input; on Windows the quiet command is `cmd /K`.
        #[cfg(unix)]
        let input = "hello\r";
        #[cfg(windows)]
        let input = "echo hello\r";
        ws.read_with(cx, |ws, _| {
            ws.space.terminals[&id]
                .term
                .write(input.as_bytes().to_vec())
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let shown = ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
                snap.text()
            });
            if shown.contains("hello") {
                break;
            }
            assert!(Instant::now() < deadline, "no echo, screen: {shown:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
        ws.read_with(cx, |ws, _| {
            let pane = &ws.space.terminals[&id];
            let (cols, rows) = pane.term.grid_size();
            let term = pane.term.shared();
            term.select_start(0, 0, Side::Left, SelectionKind::Simple);
            term.select_update(rows as usize - 1, cols as usize - 1, Side::Right);
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(COPY, cx);
        })
        .unwrap();
        let text = cx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .unwrap_or_default();
        assert!(text.contains("hello"), "clipboard: {text:?}");
    }

    #[gpui_kit::test]
    fn mouse_down_in_a_body_starts_a_selection(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let body = window.find(("body", id.0 as usize)).bounds();
            let from = point(body.origin.x + px(20.), body.origin.y + px(20.));
            window.drag(from, point(from.x + px(80.), from.y), cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.focused, Some(id));
            assert!(ws.drag.is_none(), "the selection drag ended on mouse up");
        });
    }

    #[cfg(target_os = "macos")]
    const ZOOM_IN: &str = "cmd-=";
    #[cfg(not(target_os = "macos"))]
    const ZOOM_IN: &str = "ctrl-=";
    #[cfg(target_os = "macos")]
    const ZOOM_OUT: &str = "cmd--";
    #[cfg(not(target_os = "macos"))]
    const ZOOM_OUT: &str = "ctrl--";
    #[cfg(target_os = "macos")]
    const ZOOM_RESET: &str = "cmd-0";
    #[cfg(not(target_os = "macos"))]
    const ZOOM_RESET: &str = "ctrl-0";
    #[cfg(target_os = "macos")]
    const FIT: &str = "cmd-shift-1";
    #[cfg(not(target_os = "macos"))]
    const FIT: &str = "ctrl-shift-1";

    #[gpui_kit::test]
    fn zoom_keys_step_and_reset(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(ZOOM_IN, cx);
            window.press(ZOOM_IN, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.space.canvas.camera.zoom(), 1.25));
        cx.update_window(handle, |_, window, cx| {
            window.press(ZOOM_OUT, cx);
            window.press(ZOOM_RESET, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.space.canvas.camera.zoom(), 1.0));
    }

    #[gpui_kit::test]
    fn fit_all_brings_every_block_into_view(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open(
                    templum_canvas::Rect::new(5000.0, 3000.0, 600.0, 400.0),
                    window,
                    cx,
                )
            });
            window.render_frame(cx);
            window.press(FIT, cx);
            let vp = ws.read(cx).viewport(window);
            assert_eq!(ws.read(cx).space.canvas.visible(vp).count(), 2);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn a_theme_change_crossfades_the_chrome(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        let light = ws.read_with(cx, |ws, _| {
            assert_eq!(ws.ui, ws.theme.ui, "the first theme shows at once");
            ws.theme.ui
        });
        ws.update(cx, |ws, cx| ws.set_dark(true, cx));
        // The fade starts on the first frame that draws the new theme.
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        cx.executor().advance_clock(crate::motion::THEME / 2);
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_ne!(ws.ui, ws.theme.ui, "fading");
            assert_ne!(ws.ui, light, "fading");
        });
        cx.executor()
            .advance_clock(crate::motion::THEME + std::time::Duration::from_millis(10));
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.ui, ws.theme.ui, "settled"));
    }

    #[gpui_kit::test]
    fn zoom_keys_glide_to_the_step(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(ZOOM_IN, cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.space.canvas.camera.zoom(),
                1.1,
                "the target is set at once"
            );
            assert!(ws.space.flight.is_some(), "the view glides");
            assert!(
                ws.space.view.zoom() < 1.1,
                "drawn on the way: {}",
                ws.space.view.zoom()
            );
        });
        std::thread::sleep(crate::motion::FLIGHT + std::time::Duration::from_millis(50));
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.space.flight.is_none(), "landed");
            assert_eq!(ws.space.view, ws.space.canvas.camera);
        });
    }

    #[gpui_kit::test]
    fn a_wheel_pan_during_a_glide_moves_the_glide_too(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(ZOOM_IN, cx);
        })
        .unwrap();
        ws.update(cx, |ws, _| {
            let (before, _) = ws.space.flight.unwrap();
            ws.pan_by(Point::new(30.0, 0.0));
            let (after, _) = ws.space.flight.expect("still gliding");
            assert_eq!(
                after.to, ws.space.canvas.camera,
                "the glide still lands on the camera"
            );
            let z = before.from.zoom();
            assert!((before.from.origin.x - after.from.origin.x - 30.0 / z).abs() < 1e-3);
        });
    }

    #[gpui_kit::test]
    fn dragging_the_canvas_ends_a_glide_on_the_camera(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(ZOOM_OUT, cx);
        })
        .unwrap();
        ws.update(cx, |ws, _| {
            ws.space.canvas.camera.pan_by(Point::new(5.0, 5.0))
        });
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.space.flight.is_none());
            assert_eq!(ws.space.view, ws.space.canvas.camera);
        });
    }

    #[gpui_kit::test]
    fn fit_all_glides_then_shows_every_block(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open(
                    templum_canvas::Rect::new(5000.0, 3000.0, 600.0, 400.0),
                    window,
                    cx,
                )
            });
            window.render_frame(cx);
            window.press(FIT, cx);
            window.render_frame(cx);
            assert!(ws.read(cx).space.flight.is_some());
        })
        .unwrap();
        std::thread::sleep(crate::motion::FLIGHT + std::time::Duration::from_millis(50));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let vp = ws.read(cx).viewport(window);
            let ws = ws.read(cx);
            assert_eq!(ws.space.canvas.visible_from(&ws.space.view, vp).count(), 2);
        })
        .unwrap();
    }

    /// Presses `key`, then types "x" + Enter into `cat`, and returns the screen once "x" shows.
    #[cfg(unix)]
    fn echo_after(
        key: &str,
        handle: AnyWindowHandle,
        ws: &Entity<Workspace>,
        cx: &mut TestAppContext,
    ) -> String {
        use std::time::{Duration, Instant};
        use templum_terminal::Snapshot;
        let id = first_id(ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(key, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            ws.space.terminals[&id].term.write(b"x\r".to_vec())
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let shown = ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
                snap.text()
            });
            if shown.contains('x') {
                return shown;
            }
            assert!(Instant::now() < deadline, "no echo: {shown:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    // gpui-kit's Root binds tab to focus navigation; the shell must get it (completion).
    #[cfg(unix)]
    #[gpui_kit::test]
    fn tab_reaches_the_shell(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let shown = echo_after("tab", handle, &ws, cx);
        let first = shown.lines().next().unwrap_or("");
        assert!(
            first.starts_with(' '),
            "Tab never reached the shell; screen: {shown:?}"
        );
    }

    #[gpui_kit::test]
    fn pinch_over_a_block_zooms(cx: &mut TestAppContext) {
        use gpui_kit::PinchEvent;
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let at = window.find(("body", id.0 as usize)).bounds().center();
            window.dispatch_event(
                PlatformInput::Pinch(PinchEvent {
                    position: at,
                    delta: 0.3,
                    modifiers: Modifiers::default(),
                    phase: TouchPhase::Moved,
                }),
                cx,
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.space.canvas.camera.zoom(), 1.1));
    }

    #[gpui_kit::test]
    fn follows_the_system_appearance(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        // The test window is light.
        ws.read_with(cx, |ws, _| assert_eq!(ws.theme.name, "Catppuccin Latte"));
        ws.update(cx, |ws, cx| ws.set_dark(true, cx));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.theme.name, "Catppuccin Mocha");
            assert_eq!(ws.space.terminals.len(), 2);
            for pane in ws.space.terminals.values() {
                assert_eq!(
                    pane.term.palette(),
                    ws.theme.palette,
                    "old and new terminals"
                );
            }
        });
        cx.update(|cx| {
            let kit = gpui_kit::component::Theme::global(cx);
            assert!(kit.mode.is_dark());
        });
        ws.read_with(cx, |ws, cx| {
            assert_eq!(
                gpui_kit::component::Theme::global(cx).background,
                ws.theme.ui.canvas,
                "gpui-kit's root paints the opaque canvas"
            );
        });
    }

    #[gpui_kit::test]
    fn config_names_pick_the_theme(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "[theme]\nlight = \"One Light\"\n",
        )
        .unwrap();
        let (_, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.read_with(cx, |ws, _| assert_eq!(ws.theme.name, "One Light"));
    }

    #[gpui_kit::test]
    fn title_bar_buttons_reset_zoom_and_open_terminals(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(ZOOM_IN, cx);
            window.render_frame(cx);
            window.click("zoom-level", cx);
            window.render_frame(cx);
            window.click("new-terminal", cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.camera.zoom(), 1.0);
            assert_eq!(ws.space.canvas.blocks().len(), 2);
        });
    }

    #[gpui_kit::test]
    fn focus_does_not_shift_the_terminal(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let focused = window.find(("body", id.0 as usize)).bounds();
            ws.update(cx, |ws, _| ws.space.focused = None);
            window.render_frame(cx);
            let unfocused = window.find(("body", id.0 as usize)).bounds();
            assert_eq!(focused, unfocused);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn close_button_appears_on_hover(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find(("close", id.0 as usize)).is_none(),
                "hidden until hovered"
            );
            window.hover(("block", id.0 as usize), cx);
            window.render_frame(cx);
            assert!(window.try_find(("close", id.0 as usize)).is_some());
            assert!(window.try_find(("handle", id.0 as usize)).is_some());
        })
        .unwrap();
    }

    fn builtin_index(name: &str) -> usize {
        crate::theme::BUILTIN
            .iter()
            .position(|b| b.name == name)
            .unwrap()
    }

    #[gpui_kit::test]
    fn theme_picker_previews_cancels_and_saves(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-button", cx);
            window.render_frame(cx);
            window.hover(("theme-item", builtin_index("One Dark")), cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.theme.name, "One Dark", "hover previews")
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press("escape", cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.theme.name, "Catppuccin Latte", "Esc puts it back");
            assert!(ws.picker.is_none());
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-button", cx);
            window.render_frame(cx);
            window.press("down", cx);
            window.press("enter", cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.theme.name, "Catppuccin Frappé");
            assert!(ws.picker.is_none());
        });
        let saved = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(saved.contains("light = \"Catppuccin Frappé\""), "{saved}");
    }

    #[gpui_kit::test]
    fn clicking_outside_the_picker_cancels(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-button", cx);
            window.render_frame(cx);
            window.hover(("theme-item", builtin_index("Apple Light")), cx);
            window.render_frame(cx);
            window.click("picker-backdrop", cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.theme.name, "Catppuccin Latte");
            assert!(ws.picker.is_none());
            assert_eq!(
                ws.space.canvas.blocks().len(),
                1,
                "the click didn't reach the canvas"
            );
        });
    }

    #[gpui_kit::test]
    fn a_broken_config_is_not_overwritten(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "[theme\n").unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-button", cx);
            window.render_frame(cx);
            window.click(("theme-item", builtin_index("One Dark")), cx);
            window.render_frame(cx);
            assert!(window.try_find("picker-error").is_some());
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.theme.name, "One Dark", "the choice applies anyway");
            assert!(ws.picker.is_some(), "the picker stays open to show why");
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[theme\n");
    }

    #[gpui_kit::test]
    fn appearance_change_closes_the_picker(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-button", cx);
            window.render_frame(cx);
            window.hover(("theme-item", builtin_index("One Light")), cx);
        })
        .unwrap();
        ws.update(cx, |ws, cx| ws.set_dark(true, cx));
        ws.read_with(cx, |ws, _| {
            assert!(ws.picker.is_none());
            assert_eq!(ws.theme.name, "Catppuccin Mocha");
        });
    }

    #[gpui_kit::test]
    fn appearance_event_without_a_mode_change_keeps_the_picker(cx: &mut TestAppContext) {
        // GPUI also reports appearance on Wayland decoration and window-control changes.
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-button", cx);
            window.render_frame(cx);
            window.hover(("theme-item", builtin_index("One Dark")), cx);
        })
        .unwrap();
        ws.update(cx, |ws, cx| ws.set_dark(false, cx));
        ws.read_with(cx, |ws, _| {
            assert!(ws.picker.is_some(), "still light: the picker stays open");
            assert_eq!(ws.theme.name, "One Dark", "and the preview stays");
        });
    }

    #[gpui_kit::test]
    fn new_terminals_pop_in_then_settle(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW, cx);
            let ws = ws.read(cx);
            let new = ws.space.canvas.top().unwrap();
            assert!(
                ws.space.appearing.contains_key(&new),
                "the new block is animating"
            );
        })
        .unwrap();
        std::thread::sleep(crate::motion::OPEN + std::time::Duration::from_millis(50));
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.space.appearing.is_empty(), "settled")
        });
    }

    #[gpui_kit::test]
    fn a_closed_block_fades_as_a_ghost_that_ignores_clicks(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let n = first_id(&ws, cx).0 as usize;
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(CLOSE, cx);
            window.render_frame(cx);
            // Checked right after the frame that drew it: the ghost lives 0.18 s, and a slow
            // machine may spend longer than that on the clicks below.
            assert_eq!(ws.read(cx).space.ghosts.len(), 1);
            // A double click where the ghost is reaches the canvas: a terminal opens there.
            let at = window.find(("ghost", n)).bounds().center();
            let at = (f32::from(at.x), f32::from(at.y));
            click(window, cx, at, 1);
            click(window, cx, at, 2);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.space.canvas.blocks().len(),
                1,
                "the click went through the ghost"
            );
        });
        std::thread::sleep(crate::motion::CLOSE + std::time::Duration::from_millis(50));
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.space.ghosts.is_empty(), "faded away")
        });
    }

    #[gpui_kit::test]
    fn blink_toggles_only_while_the_cursor_blinks(cx: &mut TestAppContext) {
        let (_, ws) = open(cx);
        ws.update(cx, |ws, cx| {
            ws.cursor_blinks = false;
            ws.tick_blink(cx);
            assert!(ws.blink_on, "steady cursors stay visible");
            ws.cursor_blinks = true;
            ws.tick_blink(cx);
            assert!(!ws.blink_on);
            ws.tick_blink(cx);
            assert!(ws.blink_on);
        });
    }

    fn write_config(dir: &std::path::Path, text: &str) {
        std::fs::write(dir.join("config.toml"), text).unwrap();
    }

    fn reload(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.reload_config(window, cx))
        })
        .unwrap();
    }

    fn notification_count(handle: AnyWindowHandle, cx: &mut TestAppContext) -> usize {
        use gpui_kit::component::WindowExt as _;
        cx.update_window(handle, |_, window, cx| window.notifications(cx).len())
            .unwrap()
    }

    #[gpui_kit::test]
    fn a_bigger_font_keeps_blocks_and_shrinks_grids(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let id = first_id(&ws, cx);
        let (rect, cell, grid) = ws.read_with(cx, |ws, _| {
            (
                ws.space.canvas.get(id).unwrap().rect,
                ws.cell.size,
                ws.space.terminals[&id].term.grid_size(),
            )
        });
        write_config(dir.path(), "[font]\nsize = 28\n");
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.space.canvas.get(id).unwrap().rect,
                rect,
                "the block keeps its rect"
            );
            assert!(ws.cell.size.w > cell.w);
            let now = ws.space.terminals[&id].term.grid_size();
            assert!(now.0 < grid.0 && now.1 < grid.1, "{now:?} vs {grid:?}");
        });
    }

    #[gpui_kit::test]
    fn theme_mode_applies_on_reload_and_an_old_window_table_is_ignored(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        write_config(
            dir.path(),
            "[window]\nblur = false\n[theme]\nmode = \"dark\"\n",
        );
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| {
            assert!(!ws.config_failed, "the old [window] table loads");
            assert_eq!(
                ws.theme.name, "Catppuccin Mocha",
                "dark pinned on a light system"
            );
        });
    }

    #[gpui_kit::test]
    fn cursor_options_reach_open_terminals(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let id = first_id(&ws, cx);
        write_config(dir.path(), "[cursor]\nshape = \"bar\"\nblink = true\n");
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| {
            let mut snap = templum_terminal::Snapshot::default();
            snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
            let c = snap.cursor.unwrap();
            assert_eq!(
                (c.shape, c.blinking),
                (templum_terminal::CursorShape::Beam, true)
            );
        });
    }

    #[gpui_kit::test]
    fn an_invalid_file_keeps_the_config_and_says_why(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        write_config(dir.path(), "[font]\nsize = 20\n");
        reload(handle, &ws, cx);
        write_config(dir.path(), "fnot = 1\n");
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| assert_eq!(ws.config.font.size, 20.0, "kept"));
        assert_eq!(notification_count(handle, cx), 1, "the error");
        write_config(dir.path(), "[font]\nsize = 20\n");
        reload(handle, &ws, cx);
        assert_eq!(notification_count(handle, cx), 2, "\"config.toml loaded\"");
    }

    #[gpui_kit::test]
    fn missing_font_warns_but_applies_the_rest(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        write_config(dir.path(), "[font]\nfamily = \"No Such Font\"\nsize = 20\n");
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| assert_eq!(ws.cell.font_size, 20.0));
        assert_eq!(notification_count(handle, cx), 1, "the warning");
    }

    #[gpui_kit::test]
    fn theme_reload_closes_the_picker(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-button", cx);
            window.render_frame(cx);
            window.hover(("theme-item", builtin_index("One Light")), cx);
        })
        .unwrap();
        write_config(dir.path(), "[theme]\nlight = \"Apple Light\"\n");
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| {
            assert!(ws.picker.is_none());
            assert_eq!(ws.theme.name, "Apple Light");
        });
    }

    #[gpui_kit::test]
    fn new_terminals_inherit_the_focused_folder(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        let dir = tempfile::tempdir().unwrap();
        // The plain path (cmd.exe can't start in a canonical \\?\ path), compared canonicalized.
        let start_dir = tempfile::tempdir().unwrap();
        let start = start_dir.path().to_path_buf();
        // A TOML literal string: Windows backslashes need no escaping.
        write_config(
            dir.path(),
            &format!("[shell]\nworking_directory = '{}'\n", start.display()),
        );
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        write_config(dir.path(), "[shell]\nworking_directory = \"inherit\"\n");
        reload(handle, &ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW, cx);
        })
        .unwrap();
        let new = ws.read_with(cx, |ws, _| ws.space.canvas.top().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let cwd = ws.read_with(cx, |ws, _| ws.space.terminals[&new].term.cwd());
            if let Some(cwd) = cwd {
                assert_eq!(cwd.canonicalize().unwrap(), start.canonicalize().unwrap());
                break;
            }
            assert!(Instant::now() < deadline, "no cwd");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[gpui_kit::test]
    fn a_theme_added_after_startup_can_be_set_in_the_config(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        std::fs::create_dir(dir.path().join("themes")).unwrap();
        std::fs::write(
            dir.path().join("themes").join("mine.toml"),
            include_str!("../themes/one-dark.toml"),
        )
        .unwrap();
        write_config(dir.path(), "[theme]\nlight = \"mine\"\n");
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| assert_eq!(ws.theme.name, "mine"));
        assert_eq!(notification_count(handle, cx), 0);
    }

    #[gpui_kit::test]
    fn an_unknown_theme_name_warns(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        write_config(dir.path(), "[theme]\nlight = \"Nope\"\n");
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| assert_eq!(ws.theme.name, "Catppuccin Latte"));
        assert_eq!(notification_count(handle, cx), 1, "the warning");
    }

    #[cfg(target_os = "macos")]
    const PICKER: &str = "cmd-shift-p";
    #[cfg(not(target_os = "macos"))]
    const PICKER: &str = "ctrl-shift-p";

    #[gpui_kit::test]
    fn remapped_shortcuts_apply_on_reload(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        write_config(dir.path(), "[keys]\nnew_terminal = \"ctrl-alt-n\"\n");
        reload(handle, &ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.space.canvas.blocks().len(),
                1,
                "the old shortcut is gone"
            )
        });
        cx.update_window(handle, |_, window, cx| window.press("ctrl-alt-n", cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.blocks().len(), 2, "the new one works")
        });
    }

    #[gpui_kit::test]
    fn the_picker_shortcut_toggles_and_others_wait(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(PICKER, cx);
            window.render_frame(cx);
            window.press(NEW, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.picker.is_some());
            assert_eq!(
                ws.space.canvas.blocks().len(),
                1,
                "no new terminal under the picker"
            );
        });
        cx.update_window(handle, |_, window, cx| window.press(PICKER, cx))
            .unwrap();
        ws.read_with(cx, |ws, _| assert!(ws.picker.is_none(), "closed like Esc"));
    }

    #[gpui_kit::test]
    fn choosing_in_dark_mode_saves_the_dark_theme(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| ws.set_dark(true, cx));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("theme-button", cx);
            window.render_frame(cx);
            window.click(("theme-item", builtin_index("One Dark")), cx);
        })
        .unwrap();
        let saved = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(saved.contains("dark = \"One Dark\""), "{saved}");
        assert!(!saved.contains("light"), "{saved}");
    }

    #[gpui_kit::test]
    fn clicking_the_empty_canvas_unfocuses_terminals(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let o = ws.read(cx).origin;
            click(
                window,
                cx,
                (f32::from(o.x) + 40.0, f32::from(o.y) + 40.0),
                1,
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, None));
    }

    #[gpui_kit::test]
    fn panning_the_canvas_keeps_the_focused_terminal(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let before = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                ws.read(cx).space.canvas.camera.origin
            })
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            let o = ws.read(cx).origin;
            let from = point(o.x + px(40.), o.y + px(40.));
            window.drag(from, point(from.x + px(100.), from.y + px(50.)), cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.focused, Some(id));
            let after = ws.space.canvas.camera.origin;
            assert_eq!(
                (before.x - after.x, before.y - after.y),
                (100.0, 50.0),
                "it panned"
            );
        });
    }

    #[test]
    fn macos_option_types_the_layout_character() {
        use super::option_types_text;
        // German layout: Option+L types "@".
        assert!(option_types_text("l", Some("@"), true));
        // Option+arrows and Option+Backspace carry no text: they stay Meta.
        assert!(!option_types_text("left", None, true));
        assert!(!option_types_text("backspace", Some("\x7f"), true));
        // Elsewhere Alt is always Meta (AltGr is not reported as Alt on Linux).
        assert!(!option_types_text("l", Some("@"), false));
    }

    use crate::config::Edit;

    fn frame(handle: AnyWindowHandle, cx: &mut TestAppContext) {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }

    #[gpui_kit::test]
    fn a_settings_edit_applies_on_the_next_frame(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        write_config(dir.path(), "# mine\n[cursor]\nblink = false\n");
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let cell = ws.read_with(cx, |ws, _| ws.cell.size);
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("font", "size", 28_i64),
                |c| c.font.size = 28.0,
                cx,
            );
            // Checked inside the update: the test app draws the window as soon as it ends.
            assert_eq!(ws.config.font.size, 14.0, "not before the frame");
        });
        frame(handle, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.config.font.size, 28.0);
            assert!(ws.cell.size.w > cell.w, "measured with the window");
        });
        let path = dir.path().join("config.toml");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# mine\n[cursor]\nblink = false\n",
            "not saved yet"
        );
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("# mine\n[cursor]\nblink = false"), "{text}");
        assert!(text.contains("[font]\nsize = 28"), "{text}");
    }

    #[gpui_kit::test]
    fn a_reset_removes_the_key_from_the_file(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        write_config(
            dir.path(),
            "[cursor]\nblink = true # mine\nshape = \"bar\"\n",
        );
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        // What a field's "reset to default" does.
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::remove("cursor", "blink"),
                |c| c.cursor.blink = false,
                cx,
            )
        });
        frame(handle, cx);
        ws.update(cx, |ws, _| {
            assert!(!ws.config.cursor.blink);
            ws.save();
        });
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(!text.contains("blink"), "{text}");
        assert!(text.contains("shape = \"bar\""), "{text}");
    }

    #[gpui_kit::test]
    fn edits_are_saved_once_they_settle(cx: &mut TestAppContext) {
        use std::time::Duration;
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let path = dir.path().join("config.toml");
        let scrollback = |n: usize, cx: &mut TestAppContext| {
            ws.update(cx, |ws, cx| {
                ws.edit(
                    Edit::set("terminal", "scrollback", n as i64),
                    |c| c.terminal.scrollback = n,
                    cx,
                )
            });
            frame(handle, cx);
        };
        scrollback(1000, cx);
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert!(!path.exists(), "not before 0.3 s");
        scrollback(2000, cx);
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert!(!path.exists(), "the second edit restarted the wait");
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("scrollback = 2000"), "{text}");
        assert!(!text.contains("1000"), "one write, the last value: {text}");
    }

    #[gpui_kit::test]
    fn an_edit_that_changes_nothing_is_not_saved(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (_, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("terminal", "scrollback", 10_000_i64),
                |c| c.terminal.scrollback = 10_000,
                cx,
            );
            assert!(ws.unsaved.is_empty());
            assert!(ws.save().is_none());
        });
        assert!(!dir.path().join("config.toml").exists());
    }

    #[gpui_kit::test]
    fn a_broken_file_is_never_overwritten_and_says_why(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        // Broken after startup, as if the user were halfway through an edit.
        write_config(dir.path(), "fnot = = 1\n");
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("cursor", "blink", true),
                |c| c.cursor.blink = true,
                cx,
            )
        });
        frame(handle, cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.save_or_notify(window, cx))
        })
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("config.toml")).unwrap(),
            "fnot = = 1\n"
        );
        ws.read_with(cx, |ws, _| {
            assert!(ws.config.cursor.blink, "still in effect");
            assert_eq!(
                ws.save_error.as_deref(),
                Some("Not saved: config.toml has an error on line 1")
            );
        });
        assert_eq!(notification_count(handle, cx), 1);
        write_config(dir.path(), "[cursor]\nblink = true\n");
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.poll_config(window, cx))
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.save_error, None, "fixed"));
    }

    #[gpui_kit::test]
    fn closing_the_window_saves_what_is_pending(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("cursor", "blink", true),
                |c| c.cursor.blink = true,
                cx,
            )
        });
        // The test's handle would keep the workspace alive.
        drop(ws);
        cx.update_window(handle, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("blink = true"), "{text}");
    }

    #[gpui_kit::test]
    fn without_a_config_folder_edits_apply_and_nothing_is_written(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("cursor", "blink", true),
                |c| c.cursor.blink = true,
                cx,
            )
        });
        frame(handle, cx);
        ws.update(cx, |ws, _| {
            assert!(ws.config.cursor.blink);
            assert_eq!(ws.save(), None);
            assert_eq!(ws.save_error, None, "the footer says it, not an error");
        });
    }

    #[cfg(target_os = "macos")]
    const SETTINGS: &str = "cmd-,";
    #[cfg(not(target_os = "macos"))]
    const SETTINGS: &str = "ctrl-,";

    fn panel_open(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
        ws.read_with(cx, |ws, _| ws.settings.as_ref().is_some_and(|p| !p.closing))
    }

    /// Lets the panel's slide finish (it runs on the real clock, like the block animations).
    fn settle_slide(handle: AnyWindowHandle, cx: &mut TestAppContext) {
        std::thread::sleep(crate::motion::SLIDE + std::time::Duration::from_millis(50));
        frame(handle, cx);
    }

    #[gpui_kit::test]
    fn the_settings_shortcut_and_buttons_open_and_close_the_panel(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
            window.render_frame(cx);
        })
        .unwrap();
        assert!(panel_open(&ws, cx), "the shortcut opens it");
        cx.update_window(handle, |_, window, cx| {
            window.press(SETTINGS, cx);
            window.render_frame(cx);
        })
        .unwrap();
        assert!(!panel_open(&ws, cx), "and closes it");
        settle_slide(handle, cx);
        ws.read_with(cx, |ws, _| assert!(ws.settings.is_none(), "slid out"));
        cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
            .unwrap();
        settle_slide(handle, cx);
        assert!(panel_open(&ws, cx), "the gear opens it");
        cx.update_window(handle, |_, window, cx| window.click("settings-close", cx))
            .unwrap();
        assert!(!panel_open(&ws, cx), "the close button closes it");
    }

    #[gpui_kit::test]
    fn the_panel_opens_on_appearance_and_tabs_switch_sections(cx: &mut TestAppContext) {
        let (handle, _ws) = open(cx);
        cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
            .unwrap();
        settle_slide(handle, cx);
        cx.update_window(handle, |_, window, cx| {
            assert!(window.try_find("field-theme.mode").is_some());
            assert!(window.try_find("shortcut-new_terminal").is_none());
            window.click("settings-tab-shortcuts", cx);
            window.render_frame(cx);
            assert!(window.try_find("shortcut-new_terminal").is_some());
            assert!(window.try_find("field-theme.mode").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn the_blink_switch_edits_the_config_and_its_reset_removes_the_key(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
            .unwrap();
        settle_slide(handle, cx);
        cx.update_window(handle, |_, window, cx| {
            window.click("settings-tab-terminal", cx);
            window.render_frame(cx);
            assert!(
                window.try_find("reset-cursor.blink").is_none(),
                "no reset while at the default"
            );
            window.click("field-cursor.blink", cx);
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(ws.read(cx).config.cursor.blink, "the switch turned it on");
            assert!(window.try_find("reset-cursor.blink").is_some());
            window.click("reset-cursor.blink", cx);
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(!ws.read(cx).config.cursor.blink, "back to the default");
        })
        .unwrap();
        ws.update(cx, |ws, _| ws.save());
        let text =
            std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap_or_default();
        assert!(!text.contains("blink"), "{text}");
    }

    #[gpui_kit::test]
    fn the_integration_and_notification_switches_turn_them_off_and_back(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
            .unwrap();
        settle_slide(handle, cx);
        let on = |ws: &Workspace| {
            (
                ws.config.shell.integration,
                ws.config.terminal.notifications,
            )
        };
        for (tab, field, reset, line) in [
            (
                "settings-tab-shell",
                "field-shell.integration",
                "reset-shell.integration",
                "integration = false",
            ),
            (
                "settings-tab-terminal",
                "field-terminal.notifications",
                "reset-terminal.notifications",
                "notifications = false",
            ),
        ] {
            cx.update_window(handle, |_, window, cx| {
                window.click(tab, cx);
                window.render_frame(cx);
                assert!(window.try_find(reset).is_none());
                window.click(field, cx);
                window.render_frame(cx);
                window.render_frame(cx);
            })
            .unwrap();
            ws.update(cx, |ws, _| ws.save());
            let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
            assert!(text.contains(line), "{text}");
            cx.update_window(handle, |_, window, cx| {
                window.click(reset, cx);
                window.render_frame(cx);
                window.render_frame(cx);
            })
            .unwrap();
        }
        assert_eq!(ws.read_with(cx, |ws, _| on(ws)), (true, true), "back on");
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
        assert!(
            !text.contains("integration") && !text.contains("notifications"),
            "{text}"
        );
    }

    #[cfg(target_os = "macos")]
    const PREVIOUS_PROMPT: &str = "cmd-up";
    #[cfg(not(target_os = "macos"))]
    const PREVIOUS_PROMPT: &str = "ctrl-shift-up";
    #[cfg(target_os = "macos")]
    const NEXT_PROMPT: &str = "cmd-down";
    #[cfg(not(target_os = "macos"))]
    const NEXT_PROMPT: &str = "ctrl-shift-down";

    /// A command whose output scrolled its prompt out of view: the shortcut brings the prompt to
    /// the top, the other goes back down.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn the_command_shortcuts_jump_between_prompts(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        use templum_terminal::Snapshot;
        let data = tempfile::tempdir().unwrap();
        let bash = Some(("bash".to_string(), Vec::new()));
        let (handle, ws) = open_as(cx, bash, None, Some(data.path().to_path_buf()));
        let id = first_id(&ws, cx);
        let top = |cx: &mut TestAppContext| {
            ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
                (snap.rows[0].text.clone(), snap.rows[0].mark)
            })
        };
        ws.read_with(cx, |ws, _| {
            ws.space.terminals[&id].term.write(&b"seq 100\r"[..])
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while ws.read_with(cx, |ws, _| ws.space.terminals[&id].term.status().finished) < 1 {
            assert!(Instant::now() < deadline, "seq never ended");
            std::thread::sleep(Duration::from_millis(50));
        }
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(PREVIOUS_PROMPT, cx);
        })
        .unwrap();
        let (text, mark) = top(cx);
        assert!(text.ends_with("seq 100"), "{text:?}");
        assert_eq!(mark, Some(templum_terminal::Mark::Ended(Some(0))));
        cx.update_window(handle, |_, window, cx| window.press(NEXT_PROMPT, cx))
            .unwrap();
        assert!(
            !top(cx).0.contains("seq"),
            "back at the bottom: {:?}",
            top(cx)
        );
    }

    /// The startup terminal runs bash with Templum's integration; turned off, a new one doesn't.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn new_terminals_get_the_shell_integration_unless_turned_off(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        use templum_terminal::Snapshot;
        let data = tempfile::tempdir().unwrap();
        let bash = Some(("bash".to_string(), Vec::new()));
        let (handle, ws) = open_as(cx, bash, None, Some(data.path().to_path_buf()));
        assert!(data.path().join("shell-integration/templum.bash").is_file());
        let shown = |id, cx: &mut TestAppContext| {
            ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
                snap.text()
            })
        };
        let ask = |id, want: &str, cx: &mut TestAppContext| {
            ws.read_with(cx, |ws, _| {
                ws.space.terminals[&id]
                    .term
                    .write(&b"echo \"[${__templum_installed-no}]\"\r"[..])
            });
            let deadline = Instant::now() + Duration::from_secs(10);
            while !shown(id, cx).contains(want) {
                assert!(Instant::now() < deadline, "no {want}: {}", shown(id, cx));
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        let first = first_id(&ws, cx);
        ask(first, "[1]\n", cx);
        ws.update(cx, |ws, _| ws.config.shell.integration = false);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW, cx);
        })
        .unwrap();
        let second = ws.read_with(cx, |ws, _| ws.space.focused.unwrap());
        assert_ne!(first, second);
        ask(second, "[no]\n", cx);
    }

    #[gpui_kit::test]
    fn the_agents_tab_resets_ours_and_deletes_yours(cx: &mut TestAppContext) {
        use crate::agents::Draft;
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| {
            let claude = ws.agents.iter().find(|a| a.id == "claude").unwrap();
            let draft = Draft {
                command: "claude --model opus".into(),
                ..Draft::of(claude)
            };
            ws.save_agent(Some("claude"), &draft, cx).unwrap();
            let mine = Draft {
                name: "Mine".into(),
                command: "mine".into(),
                resume: String::new(),
                show: true,
            };
            ws.save_agent(None, &mine, cx).unwrap();
        });
        cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
            .unwrap();
        settle_slide(handle, cx);
        cx.update_window(handle, |_, window, cx| {
            window.click("settings-tab-agents", cx);
            window.render_frame(cx);
            assert!(
                window.try_find("field-agents.mine").is_some(),
                "yours is listed"
            );
            assert!(
                window.try_find("reset-agents.codex").is_none(),
                "unchanged: no ↺"
            );
            window.click("reset-agents.claude", cx);
            window.render_frame(cx);
            window.render_frame(cx);
            let claude = ws
                .read(cx)
                .agents
                .iter()
                .find(|a| a.id == "claude")
                .unwrap()
                .clone();
            assert_eq!(claude.command, "claude", "back to Templum's");
            // Yours is last: scroll down to it.
            window.scroll(
                "settings-scroll",
                ScrollDelta::Pixels(point(px(0.), px(-5000.))),
                cx,
            );
            window.render_frame(cx);
            window.click("edit-agent-mine", cx);
            window.render_frame(cx);
        })
        .unwrap();
        // The dialog slides in for 0.25 s first: presses meanwhile are lost.
        std::thread::sleep(std::time::Duration::from_millis(300));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("delete-agent", cx);
            window.render_frame(cx);
            window.render_frame(cx);
            assert!(ws.read(cx).agents.iter().all(|a| a.id != "mine"), "deleted");
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn the_shell_list_sets_the_program_and_its_arguments(cx: &mut TestAppContext) {
        use templum_terminal::ShellOption;
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
            .unwrap();
        settle_slide(handle, cx);
        // Two shells "found" on this computer.
        ws.update(cx, |ws, _| {
            ws.shells = vec![
                ShellOption {
                    label: "Git Bash".into(),
                    program: "C:\\Git\\bin\\bash.exe".into(),
                    args: vec!["--login".into(), "-i".into()],
                },
                ShellOption {
                    label: "PowerShell 7".into(),
                    program: "pwsh".into(),
                    args: vec!["-NoLogo".into()],
                },
            ];
            ws.default_shell = "Git Bash".into();
        });
        cx.update_window(handle, |_, window, cx| {
            window.click("settings-tab-shell", cx);
            window.render_frame(cx);
            assert!(
                window.try_find("reset-shell.shell").is_none(),
                "the default"
            );
            window
                .within("field-shell.shell")
                .click("menu-shell.shell", cx);
            window.render_frame(cx);
            // Default (Git Bash), Git Bash, PowerShell 7.
            window.within("popup-menu").click(2usize, cx);
            window.render_frame(cx);
            window.render_frame(cx);
            let shell = ws.read(cx).config.shell.clone();
            assert_eq!(shell.program.as_deref(), Some("pwsh"));
            assert_eq!(shell.args, ["-NoLogo"]);
            assert!(window.try_find("reset-shell.shell").is_some());
        })
        .unwrap();
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
        assert!(
            text.contains("program = \"pwsh\"") && text.contains("-NoLogo"),
            "{text}"
        );
        cx.update_window(handle, |_, window, cx| {
            window.click("reset-shell.shell", cx);
            window.render_frame(cx);
            window.render_frame(cx);
            let shell = ws.read(cx).config.shell.clone();
            assert_eq!(
                (shell.program, shell.args.len()),
                (None, 0),
                "back to the default"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn esc_closes_the_panel_only_from_inside_it(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
            window.render_frame(cx);
            window.press("escape", cx);
        })
        .unwrap();
        assert!(!panel_open(&ws, cx), "Esc in the panel closes it");
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
        })
        .unwrap();
        settle_slide(handle, cx);
        // Click the terminal's left part, clear of the panel on the right.
        cx.update_window(handle, |_, window, cx| {
            let (o, r) = {
                let ws = ws.read(cx);
                let rect = ws.space.canvas.get(id).unwrap().rect;
                (ws.origin, ws.space.canvas.camera.rect_to_screen(rect))
            };
            let at = (
                f32::from(o.x) + r.x + 20.0,
                f32::from(o.y) + r.y + templum_canvas::chrome::TITLE_H + 20.0,
            );
            click(window, cx, at, 1);
            window.render_frame(cx);
            window.press("escape", cx);
            let focused = ws.read(cx).focus_handle.is_focused(window);
            assert!(focused, "keys go to the terminal");
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(id)));
        assert!(panel_open(&ws, cx), "Esc went to the terminal");
    }

    #[gpui_kit::test]
    fn closing_the_panel_saves_at_once(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("cursor", "blink", true),
                |c| c.cursor.blink = true,
                cx,
            )
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
        })
        .unwrap();
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("blink = true"), "{text}");
    }

    #[gpui_kit::test]
    fn a_number_applies_on_enter_or_a_step_not_while_typing(cx: &mut TestAppContext) {
        use crate::settings::{NumberState, SCROLLBACK};
        use gpui_kit::component::input::{InputEvent, NumberInputEvent, StepAction};
        let (handle, ws) = open(cx);
        let state = cx
            .update_window(handle, |_, window, cx| {
                let ws = ws.clone();
                cx.new(|cx| NumberState::new(ws, &SCROLLBACK, 10_000.0, window, cx))
            })
            .unwrap();
        let scrollback =
            |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.config.terminal.scrollback);
        // Typing "20000" passes through "2": applied, it would trim every terminal's history.
        cx.update_window(handle, |_, window, cx| {
            let input = state.read(cx).input.clone();
            input.update(cx, |input, cx| input.set_value("2", window, cx));
            window.render_frame(cx);
        })
        .unwrap();
        assert_eq!(scrollback(cx), 10_000, "not while typing");
        cx.update_window(handle, |_, window, cx| {
            let input = state.read(cx).input.clone();
            input.update(cx, |input, cx| {
                input.set_value("20000", window, cx);
                cx.emit(InputEvent::PressEnter {
                    secondary: false,
                    shift: false,
                });
            });
            window.render_frame(cx);
        })
        .unwrap();
        assert_eq!(scrollback(cx), 20_000, "Enter applies it");
        cx.update_window(handle, |_, window, cx| {
            let input = state.read(cx).input.clone();
            input.update(cx, |_, cx| {
                cx.emit(NumberInputEvent::Step(StepAction::Decrement))
            });
            window.render_frame(cx);
        })
        .unwrap();
        assert_eq!(scrollback(cx), 19_000, "a step applies at once");
    }

    #[gpui_kit::test]
    fn a_change_made_while_the_file_is_broken_is_saved_once_it_is_fixed(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        write_config(dir.path(), "fnot = = 1\n");
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("cursor", "blink", true),
                |c| c.cursor.blink = true,
                cx,
            )
        });
        frame(handle, cx);
        ws.update(cx, |ws, _| {
            assert!(ws.save().is_some(), "the file is broken")
        });
        // The user fixes the file, with something else in it; the poll notices.
        write_config(dir.path(), "[cursor]\nshape = \"bar\"\n");
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.poll_config(window, cx))
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.config.cursor.blink, "the panel's change survived");
            assert_eq!(
                ws.config.cursor.shape,
                crate::config::Shape::Bar,
                "and the fix applied"
            );
            assert_eq!(ws.save_error, None);
        });
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(
            text.contains("blink = true") && text.contains("shape = \"bar\""),
            "{text}"
        );
    }

    fn poll(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.poll_config(window, cx))
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn a_hand_edit_that_a_panel_save_writes_over_still_applies(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        // A hand edit the poll hasn't seen yet, then a panel change saved before the poll comes.
        write_config(dir.path(), "[cursor]\nshape = \"bar\"\n");
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("cursor", "blink", true),
                |c| c.cursor.blink = true,
                cx,
            )
        });
        frame(handle, cx);
        ws.update(cx, |ws, _| ws.save());
        poll(handle, &ws, cx);
        ws.read_with(cx, |ws, _| {
            assert!(ws.config.cursor.blink);
            assert_eq!(
                ws.config.cursor.shape,
                crate::config::Shape::Bar,
                "the hand edit applies too"
            );
        });
    }

    #[gpui_kit::test]
    fn a_hand_edit_of_the_shell_drops_what_the_panel_was_typing(cx: &mut TestAppContext) {
        use crate::settings::{args_error, args_text, set_args};
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| set_args(ws, "--login \"a b", cx));
        frame(handle, cx);
        write_config(dir.path(), "[shell]\nargs = [\"-l\"]\n");
        poll(handle, &ws, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(args_text(ws), "-l", "the file's value shows");
            assert_eq!(args_error(ws), None);
        });
    }

    #[gpui_kit::test]
    fn the_footer_shows_a_failed_save_until_the_file_is_fixed(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let status = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                window.try_find("settings-status").is_some()
            })
            .unwrap()
        };
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
        })
        .unwrap();
        assert!(!status(cx), "nothing to say");
        write_config(dir.path(), "fnot = = 1\n");
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("cursor", "blink", true),
                |c| c.cursor.blink = true,
                cx,
            )
        });
        frame(handle, cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.save_or_notify(window, cx))
        })
        .unwrap();
        assert!(status(cx), "the save error shows");
        write_config(dir.path(), "[cursor]\nblink = true\n");
        poll(handle, &ws, cx);
        assert!(!status(cx), "gone once the file is fixed");
    }

    #[gpui_kit::test]
    fn a_theme_picked_right_after_a_panel_change_is_the_one_saved(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| {
            ws.edit(
                Edit::set("theme", "light", "One Light"),
                |c| c.theme.light = "One Light".into(),
                cx,
            )
        });
        frame(handle, cx);
        // The picker, before the panel's change is written.
        ws.update(cx, |ws, cx| {
            ws.choose_theme(builtin_index("Apple Light"), cx)
        });
        frame(handle, cx);
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("light = \"Apple Light\""), "{text}");
        ws.read_with(cx, |ws, _| assert_eq!(ws.config.theme.light, "Apple Light"));
    }

    #[gpui_kit::test]
    fn a_reset_or_another_folder_choice_drops_what_was_typed(cx: &mut TestAppContext) {
        use crate::settings::{
            args_error, args_text, folder_hint, folder_text, set_args, set_folder, set_folder_kind,
        };
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| set_args(ws, "--login \"a b", cx));
        frame(handle, cx);
        // What the field's "reset to default" does.
        ws.update(cx, |ws, cx| {
            ws.edit(Edit::remove("shell", "args"), |c| c.shell.args.clear(), cx)
        });
        frame(handle, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(args_text(ws), "");
            assert_eq!(args_error(ws), None);
        });
        // "~" typed, then Home chosen, then "A folder…" again (it starts at the home folder).
        let steps: [fn(&mut Workspace, &mut Context<Workspace>); 4] = [
            |ws, cx| set_folder_kind(ws, "folder", cx),
            |ws, cx| set_folder(ws, "~", cx),
            |ws, cx| set_folder_kind(ws, "home", cx),
            |ws, cx| set_folder_kind(ws, "folder", cx),
        ];
        for step in steps {
            ws.update(cx, step);
            frame(handle, cx);
        }
        let home = std::env::home_dir().unwrap().display().to_string();
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                folder_text(ws),
                home,
                "the folder in effect, not the old \"~\""
            );
            assert_eq!(folder_hint(ws), None);
        });
    }

    #[gpui_kit::test]
    fn the_default_font_removes_the_family(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| {
            crate::settings::set_family(ws, "Some Mono", cx)
        });
        frame(handle, cx);
        ws.update(cx, |ws, _| ws.save());
        let path = dir.path().join("config.toml");
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("family = \"Some Mono\"")
        );
        ws.update(cx, |ws, cx| crate::settings::set_family(ws, "", cx));
        frame(handle, cx);
        ws.update(cx, |ws, _| {
            assert_eq!(ws.config.font.family, None);
            ws.save();
        });
        assert!(!std::fs::read_to_string(&path).unwrap().contains("family"));
    }

    #[gpui_kit::test]
    fn an_unclosed_quote_waits_for_the_closing_one(cx: &mut TestAppContext) {
        use crate::settings::set_args;
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        ws.update(cx, |ws, cx| set_args(ws, "--login \"a b", cx));
        frame(handle, cx);
        ws.read_with(cx, |ws, _| {
            assert!(ws.config.shell.args.is_empty(), "not applied");
            assert_eq!(
                crate::settings::args_text(ws),
                "--login \"a b",
                "kept as typed"
            );
            assert_eq!(crate::settings::args_error(ws), Some("Unclosed quote"));
        });
        ws.update(cx, |ws, cx| set_args(ws, "--login \"a b\"", cx));
        frame(handle, cx);
        ws.update(cx, |ws, _| {
            assert_eq!(ws.config.shell.args, ["--login", "a b"]);
            assert_eq!(crate::settings::args_error(ws), None);
            ws.save();
        });
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("args = [\"--login\", \"a b\"]"), "{text}");
    }

    #[gpui_kit::test]
    fn a_bare_tilde_waits_for_the_rest_of_the_path(cx: &mut TestAppContext) {
        use crate::config::WorkingDirectory;
        use crate::settings::{folder_hint, folder_text, set_folder, set_folder_kind};
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let target = dir.path().join("work");
        ws.update(cx, |ws, cx| set_folder_kind(ws, "folder", cx));
        frame(handle, cx);
        ws.update(cx, |ws, cx| set_folder(ws, "~", cx));
        frame(handle, cx);
        ws.read_with(cx, |ws, _| {
            assert!(
                matches!(ws.config.shell.working_directory, WorkingDirectory::Path(_)),
                "\"~\" alone doesn't switch to Home"
            );
            assert_eq!(folder_text(ws), "~", "kept as typed");
            assert!(folder_hint(ws).is_some(), "and the field says why");
        });
        let typed = target.display().to_string();
        ws.update(cx, |ws, cx| set_folder(ws, &typed, cx));
        frame(handle, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.config.shell.working_directory,
                WorkingDirectory::Path(target.clone())
            );
            assert_eq!(folder_text(ws), typed);
            assert_eq!(folder_hint(ws), None);
        });
        // Written now, while the temporary folder still exists.
        ws.update(cx, |ws, _| ws.save());
    }

    #[gpui_kit::test]
    fn start_folder_and_program_choices_write_the_file(cx: &mut TestAppContext) {
        use crate::settings::{set_folder_kind, set_program};
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let path = dir.path().join("config.toml");
        let mut check = |change: &dyn Fn(&mut Workspace, &mut Context<Workspace>), want: &str| {
            ws.update(cx, |ws, cx| change(ws, cx));
            frame(handle, cx);
            ws.update(cx, |ws, _| ws.save());
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(text.contains(want), "want {want:?} in {text}");
        };
        check(
            &|ws, cx| set_folder_kind(ws, "home", cx),
            "working_directory = \"~\"",
        );
        check(
            &|ws, cx| set_folder_kind(ws, "inherit", cx),
            "working_directory = \"inherit\"",
        );
        check(&|ws, cx| set_program(ws, "fish", cx), "program = \"fish\"");
        ws.update(cx, |ws, cx| set_program(ws, "", cx));
        frame(handle, cx);
        ws.update(cx, |ws, _| ws.save());
        assert!(!std::fs::read_to_string(&path).unwrap().contains("program"));
    }

    #[gpui_kit::test]
    fn the_program_is_saved_trimmed_but_typing_keeps_its_spaces(cx: &mut TestAppContext) {
        use crate::settings::{program_text, set_program};
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        // Halfway through typing "C:\Program Files\…".
        ws.update(cx, |ws, cx| set_program(ws, "C:\\Program ", cx));
        frame(handle, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.config.shell.program.as_deref(), Some("C:\\Program"));
            assert_eq!(
                program_text(ws),
                "C:\\Program ",
                "the space stays in the field"
            );
        });
        ws.update(cx, |ws, cx| set_program(ws, " fish ", cx));
        frame(handle, cx);
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("program = \"fish\""), "{text}");
        ws.update(cx, |ws, cx| set_program(ws, "  ", cx));
        frame(handle, cx);
        ws.read_with(cx, |ws, _| assert_eq!(ws.config.shell.program, None));
    }

    use crate::keys::{Command, Keys};

    /// Opens the panel (unless it is open) and starts recording `command`'s shortcut.
    fn start_recording(
        handle: AnyWindowHandle,
        ws: &Entity<Workspace>,
        command: Command,
        cx: &mut TestAppContext,
    ) {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // Pressed again, the shortcut would close the panel.
            if !ws.read(cx).settings.as_ref().is_some_and(|p| !p.closing) {
                window.press(SETTINGS, cx);
            }
            window.render_frame(cx);
            ws.update(cx, |ws, cx| ws.record(command, window, cx));
            window.render_frame(cx);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn recording_a_free_shortcut_applies_and_saves(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        start_recording(handle, &ws, Command::NewTerminal, cx);
        cx.update_window(handle, |_, window, cx| {
            window.press("ctrl-alt-n", cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.update(cx, |ws, _| {
            assert_eq!(ws.recording, None);
            let pressed = gpui_kit::Keystroke::parse("ctrl-alt-n").unwrap();
            assert_eq!(ws.keymap.command(&pressed), Some(Command::NewTerminal));
            assert_eq!(
                ws.keymap.keystrokes(Command::NewTerminal).len(),
                1,
                "the only one"
            );
            ws.save();
        });
        let text = std::fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(text.contains("new_terminal = \"ctrl-alt-n\""), "{text}");
    }

    #[gpui_kit::test]
    fn a_taken_shortcut_keeps_recording(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        start_recording(handle, &ws, Command::Copy, cx);
        cx.update_window(handle, |_, window, cx| window.press(NEW, cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.recording, Some(Command::Copy));
            let shortcut = if cfg!(target_os = "macos") {
                "Cmd+T"
            } else {
                "Ctrl+Shift+T"
            };
            assert_eq!(
                ws.key_note.as_deref(),
                Some(format!("{shortcut} is already New terminal").as_str())
            );
            assert_eq!(ws.space.canvas.blocks().len(), 1, "no shortcut ran");
            assert!(ws.config.keys.copy.is_none(), "nothing changed");
        });
    }

    #[gpui_kit::test]
    fn esc_cancels_recording_and_backspace_clears(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        start_recording(handle, &ws, Command::Copy, cx);
        cx.update_window(handle, |_, window, cx| window.press("escape", cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.recording, None);
            assert!(ws.config.keys.copy.is_none());
        });
        assert!(
            panel_open(&ws, cx),
            "Esc cancelled the recording, not the panel"
        );
        start_recording(handle, &ws, Command::Copy, cx);
        cx.update_window(handle, |_, window, cx| {
            window.press("backspace", cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.config.keys.copy, Some(Keys::Many(vec![])));
            assert!(ws.keymap.keystrokes(Command::Copy).is_empty());
        });
    }

    #[gpui_kit::test]
    fn a_modifier_alone_keeps_recording(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        start_recording(handle, &ws, Command::Copy, cx);
        // What the platforms send for a modifier key: no key press, a change of modifiers.
        cx.update_window(handle, |_, window, cx| {
            window.dispatch_event(
                PlatformInput::ModifiersChanged(gpui_kit::ModifiersChangedEvent {
                    modifiers: Modifiers::shift(),
                    capslock: Default::default(),
                }),
                cx,
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.recording, Some(Command::Copy));
            assert_eq!(ws.key_note, None);
        });
    }

    /// A key held down: the platform repeats its press.
    fn hold(key: &str, window: &mut Window, cx: &mut gpui_kit::App) {
        window.dispatch_event(
            PlatformInput::KeyDown(gpui_kit::KeyDownEvent {
                keystroke: gpui_kit::Keystroke::parse(key).unwrap(),
                is_held: true,
                prefer_character_input: false,
            }),
            cx,
        );
    }

    #[gpui_kit::test]
    fn ctrl_backspace_and_ctrl_esc_record_like_other_keys(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        for key in ["ctrl-backspace", "ctrl-escape"] {
            start_recording(handle, &ws, Command::Copy, cx);
            cx.update_window(handle, |_, window, cx| {
                window.press(key, cx);
                window.render_frame(cx);
            })
            .unwrap();
            ws.read_with(cx, |ws, _| {
                assert_eq!(ws.config.keys.copy, Some(Keys::One(key.into())))
            });
        }
    }

    #[gpui_kit::test]
    fn a_held_key_runs_nothing_from_the_panel(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        start_recording(handle, &ws, Command::Settings, cx);
        cx.update_window(handle, |_, window, cx| {
            window.press("ctrl-alt-s", cx);
            window.render_frame(cx);
            // Still held after the recording took it: the new shortcut mustn't run.
            hold("ctrl-alt-s", window, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.config.keys.settings,
                Some(Keys::One("ctrl-alt-s".into()))
            )
        });
        assert!(panel_open(&ws, cx), "the repeat didn't close the panel");
    }

    #[gpui_kit::test]
    fn the_theme_picker_gets_its_keys_while_the_panel_has_the_focus(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
            window.render_frame(cx);
            window.press(PICKER, cx);
            window.render_frame(cx);
            window.press("down", cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.picker.is_some(), "the shortcut opened it");
            assert_ne!(ws.theme.name, "Catppuccin Latte", "↓ previews the next one");
        });
        cx.update_window(handle, |_, window, cx| window.press("escape", cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.picker.is_none());
            assert_eq!(ws.theme.name, "Catppuccin Latte");
        });
        assert!(panel_open(&ws, cx), "Esc closed the picker, not the panel");
    }

    #[gpui_kit::test]
    fn canvas_shortcuts_run_from_the_panel_but_terminal_ones_dont(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
            window.render_frame(cx);
            // Close acts on the focused terminal, which doesn't have the keyboard.
            window.press(CLOSE, cx);
            window.press(ZOOM_IN, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.blocks().len(), 1, "close didn't run");
            assert!(ws.space.canvas.camera.zoom() > 1.0, "zoom did");
        });
        cx.update_window(handle, |_, window, cx| window.press(NEW, cx))
            .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.space.canvas.blocks().len(), 2));
        assert!(panel_open(&ws, cx));
    }

    #[gpui_kit::test]
    fn a_reset_that_would_share_a_shortcut_is_refused(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let taken = Command::NewTerminal.defaults(cfg!(target_os = "macos"))[0];
        write_config(
            dir.path(),
            &format!("[keys]\nnew_terminal = \"ctrl-alt-y\"\nclose_terminal = \"{taken}\"\n"),
        );
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let reset = |command: Command, cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| ws.reset_shortcut(command, window, cx));
                window.render_frame(cx);
            })
            .unwrap();
        };
        reset(Command::NewTerminal, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.config.keys.new_terminal,
                Some(Keys::One("ctrl-alt-y".into())),
                "close_terminal has its default"
            )
        });
        assert_eq!(notification_count(handle, cx), 1, "and says so");
        reset(Command::CloseTerminal, cx);
        reset(Command::NewTerminal, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.config.keys.new_terminal, None, "free again: reset");
            assert_eq!(ws.config.keys.close_terminal, None);
        });
    }

    #[gpui_kit::test]
    fn a_key_without_ctrl_alt_or_super_is_refused(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        start_recording(handle, &ws, Command::NewTerminal, cx);
        cx.update_window(handle, |_, window, cx| {
            window.press("n", cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.recording, Some(Command::NewTerminal), "still recording");
            assert!(ws.key_note.is_some(), "and says why");
            assert!(
                ws.config.keys.new_terminal.is_none(),
                "terminals keep their n"
            );
        });
        cx.update_window(handle, |_, window, cx| {
            window.press("f5", cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.config.keys.new_terminal,
                Some(Keys::One("f5".into())),
                "F keys may stand alone"
            );
        });
    }

    #[gpui_kit::test]
    fn clicking_elsewhere_in_the_panel_stops_recording(cx: &mut TestAppContext) {
        // No config folder: the footer shows its line, a place to click.
        let (handle, ws) = open(cx);
        start_recording(handle, &ws, Command::Copy, cx);
        settle_slide(handle, cx);
        cx.update_window(handle, |_, window, cx| window.click("settings-status", cx))
            .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.recording, None));
        assert!(panel_open(&ws, cx));
    }

    #[gpui_kit::test]
    fn clicking_a_terminal_stops_recording(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        start_recording(handle, &ws, Command::Copy, cx);
        settle_slide(handle, cx);
        cx.update_window(handle, |_, window, cx| {
            let (o, r) = {
                let ws = ws.read(cx);
                let rect = ws.space.canvas.get(id).unwrap().rect;
                (ws.origin, ws.space.canvas.camera.rect_to_screen(rect))
            };
            let at = (
                f32::from(o.x) + r.x + 20.0,
                f32::from(o.y) + r.y + templum_canvas::chrome::TITLE_H + 20.0,
            );
            click(window, cx, at, 1);
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.recording, None));
    }

    #[gpui_kit::test]
    fn closing_the_panel_stops_recording(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        start_recording(handle, &ws, Command::Copy, cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.close_settings(window, cx);
                assert_eq!(ws.recording, None);
            })
        })
        .unwrap();
    }

    #[cfg(target_os = "macos")]
    const QUIT: &str = "cmd-q";
    #[cfg(not(target_os = "macos"))]
    const QUIT: &str = "ctrl-shift-q";

    #[gpui_kit::test]
    fn quitting_takes_a_second_press(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(QUIT, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.quit_armed.is_some(), "the first press only warns")
        });
        assert_eq!(notification_count(handle, cx), 1);
        cx.update_window(handle, |_, window, cx| window.press(QUIT, cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.quit_armed.is_none(), "the second one quits")
        });
        assert_eq!(notification_count(handle, cx), 1, "no second warning");
    }

    #[gpui_kit::test]
    fn reset_all_untangles_shortcuts_that_traded_keys(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let macos = cfg!(target_os = "macos");
        let new = Command::NewTerminal.defaults(macos)[0];
        let close = Command::CloseTerminal.defaults(macos)[0];
        write_config(
            dir.path(),
            &format!("[keys]\nnew_terminal = \"{close}\"\nclose_terminal = \"{new}\"\n"),
        );
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        // "Reset All" calls each row's reset in one go, with no frame between them.
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.reset_shortcut(Command::NewTerminal, window, cx);
                ws.reset_shortcut(Command::CloseTerminal, window, cx);
            })
        })
        .unwrap();
        frame(handle, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.config.keys.new_terminal, None);
            assert_eq!(ws.config.keys.close_terminal, None);
        });
        assert_eq!(notification_count(handle, cx), 0, "nothing refused");
    }

    #[gpui_kit::test]
    fn holding_the_quit_shortcut_is_not_a_second_press(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(QUIT, cx);
            hold(QUIT, window, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.quit_armed.is_some(), "still waiting for a second press")
        });
    }

    #[gpui_kit::test]
    fn a_late_second_press_warns_again(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let long_ago = std::time::Instant::now()
            .checked_sub(super::QUIT_AGAIN + std::time::Duration::from_millis(100))
            .unwrap();
        ws.update(cx, |ws, _| ws.quit_armed = Some(long_ago));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(QUIT, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert!(ws.quit_armed.is_some_and(|at| at > long_ago), "armed again")
        });
        assert_eq!(notification_count(handle, cx), 1);
    }

    #[gpui_kit::test]
    fn quit_works_from_the_settings_panel(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(SETTINGS, cx);
            window.render_frame(cx);
            window.press(QUIT, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert!(ws.quit_armed.is_some()));
    }

    use super::{MAX_COLS, MAX_ROWS};
    use templum_canvas::{BlockId, Camera, Point, Rect, chrome};

    fn write_canvas(dir: &std::path::Path, text: &str) {
        std::fs::create_dir_all(dir.join("workspaces")).unwrap();
        std::fs::write(crate::session::path(dir, crate::session::DEFAULT_ID), text).unwrap();
    }

    /// Waits for a terminal's shell to report its folder (canonicalized).
    fn folder_of(
        ws: &Entity<Workspace>,
        id: BlockId,
        cx: &mut TestAppContext,
    ) -> std::path::PathBuf {
        use std::time::{Duration, Instant};
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(cwd) = ws.read_with(cx, |ws, _| ws.space.terminals[&id].term.cwd()) {
                return cwd.canonicalize().unwrap();
            }
            assert!(Instant::now() < deadline, "no cwd");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[gpui_kit::test]
    fn a_saved_canvas_comes_back(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        // JSON strings: Windows backslashes doubled.
        let json = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
        write_canvas(
            data.path(),
            &format!(
                r#"{{"version":1,"camera":{{"x":-100,"y":50,"zoom":1.1}},"focused":1,"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{}"}},
                  {{"kind":"terminal","rect":{{"x":800,"y":20,"w":600,"h":400}},"folder":"{}"}}]}}"#,
                json(a.path()),
                json(b.path())
            ),
        );
        let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        let (ids, rects, camera, focused) = ws.read_with(cx, |ws, _| {
            let blocks = ws.space.canvas.blocks();
            (
                blocks.iter().map(|b| b.id).collect::<Vec<_>>(),
                blocks.iter().map(|b| b.rect).collect::<Vec<_>>(),
                ws.space.canvas.camera,
                ws.space.focused,
            )
        });
        assert_eq!(
            rects,
            [
                Rect::new(0.0, 0.0, 700.0, 450.0),
                Rect::new(800.0, 20.0, 600.0, 400.0)
            ]
        );
        assert_eq!(camera.origin, Point::new(-100.0, 50.0));
        assert_eq!(camera.zoom(), 1.1);
        assert_eq!(focused, Some(ids[1]));
        assert_eq!(folder_of(&ws, ids[0], cx), a.path().canonicalize().unwrap());
        assert_eq!(folder_of(&ws, ids[1], cx), b.path().canonicalize().unwrap());
    }

    #[gpui_kit::test]
    fn a_saved_canvas_with_no_blocks_opens_empty(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        write_canvas(
            data.path(),
            r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#,
        );
        let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        ws.read_with(cx, |ws, _| {
            assert!(ws.space.canvas.blocks().is_empty());
            assert_eq!(ws.space.focused, None);
        });
    }

    #[gpui_kit::test]
    fn a_folder_thats_gone_starts_at_home_and_says_so(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let gone = data
            .path()
            .join("gone")
            .display()
            .to_string()
            .replace('\\', "\\\\");
        write_canvas(
            data.path(),
            &format!(
                r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{gone}"}}]}}"#
            ),
        );
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        assert_eq!(notification_count(handle, cx), 1);
        let id = first_id(&ws, cx);
        let home = std::env::home_dir().unwrap().canonicalize().unwrap();
        assert_eq!(folder_of(&ws, id, cx), home);
    }

    #[gpui_kit::test]
    fn a_broken_canvas_file_is_set_aside(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        write_canvas(data.path(), "{nope");
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        assert_eq!(notification_count(handle, cx), 1);
        let bak = crate::session::path(data.path(), crate::session::DEFAULT_ID)
            .with_extension("json.bak");
        assert_eq!(std::fs::read_to_string(bak).unwrap(), "{nope");
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.blocks().len(), 1, "as on a first run")
        });
    }

    #[gpui_kit::test]
    fn absurd_numbers_in_the_saved_canvas_are_made_safe(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        write_canvas(
            data.path(),
            r#"{"version":1,"camera":{"x":1e40,"y":0,"zoom":1e40},"focused":7,"items":[
              {"kind":"terminal","rect":{"x":1e40,"y":0,"w":700,"h":450}},
              {"kind":"terminal","rect":{"x":10,"y":10,"w":-5,"h":1e30}}]}"#,
        );
        let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        ws.read_with(cx, |ws, _| {
            let blocks = ws.space.canvas.blocks();
            assert_eq!(blocks.len(), 1, "the infinite one is skipped");
            let min = chrome::min_block_size(ws.cell.size);
            let max = chrome::block_size(MAX_COLS, MAX_ROWS, ws.cell.size);
            assert_eq!(blocks[0].rect, Rect::new(10.0, 10.0, min.w, max.h));
            assert_eq!(ws.space.focused, None, "index 7 doesn't exist");
            assert_eq!(
                ws.space.canvas.camera,
                Camera::default(),
                "an infinite camera isn't used"
            );
        });
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    fn an_unreadable_canvas_file_is_never_written_over(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt as _;
        let data = tempfile::tempdir().unwrap();
        write_canvas(
            data.path(),
            r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#,
        );
        let path = crate::session::path(data.path(), crate::session::DEFAULT_ID);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        assert_eq!(notification_count(handle, cx), 1);
        ws.read_with(cx, |ws, _| {
            assert!(ws.space.frozen, "its canvas won't be saved");
            assert_eq!(ws.space.canvas.blocks().len(), 1, "as on a first run");
        });
        // The rest still saves: the boards, a new workspace.
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.boards.add_board();
                ws.new_space(window, cx);
            })
        })
        .unwrap();
        tick(cx);
        assert!(crate::board::path(data.path()).exists(), "boards saved");
        assert!(crate::session::path(data.path(), "w2").exists(), "w2 saved");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#,
            "never written over"
        );
    }

    /// One snapshot period, with the background read-and-write finished.
    fn tick(cx: &mut TestAppContext) {
        cx.executor().advance_clock(crate::session::EVERY);
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn the_canvas_comes_back_as_it_was_left(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open(Rect::new(900.0, 40.0, 600.0, 400.0), window, cx);
                let first = ws.space.canvas.blocks()[0].id;
                ws.space.canvas.get_mut(first).unwrap().rect = Rect::new(-50.0, 60.0, 700.0, 450.0);
                ws.focus_block(first, window, cx);
                ws.space.canvas.camera = Camera::at(Point::new(-40.0, 25.0), 1.25);
                ws.save_canvas_now();
            })
        })
        .unwrap();
        let before = ws.read_with(cx, |ws, _| {
            let blocks = ws.space.canvas.blocks();
            let focused = blocks.iter().position(|b| Some(b.id) == ws.space.focused);
            (blocks.iter().map(|b| b.rect).collect::<Vec<_>>(), focused)
        });
        let (_, again) = open_in(cx, None, Some(data.path().to_path_buf()));
        again.read_with(cx, |ws, _| {
            assert_eq!(
                ws.space.name, "Main",
                "no index: the canvas is the first workspace"
            );
            let blocks = ws.space.canvas.blocks();
            assert_eq!(blocks.iter().map(|b| b.rect).collect::<Vec<_>>(), before.0);
            assert_eq!(
                blocks.iter().position(|b| Some(b.id) == ws.space.focused),
                before.1
            );
            assert_eq!(ws.space.canvas.camera.origin, Point::new(-40.0, 25.0));
            assert_eq!(ws.space.canvas.camera.zoom(), 1.25);
        });
    }

    #[cfg(target_os = "macos")]
    const SECOND: &str = "cmd-2";
    #[cfg(not(target_os = "macos"))]
    const SECOND: &str = "ctrl-2";

    #[gpui_kit::test]
    fn a_new_workspace_is_empty_and_the_first_keeps_running(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let first = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.new_space(window, cx));
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!((ws.active, ws.others.len()), (1, 1));
            assert_eq!(ws.space.name, "Workspace 2");
            assert!(ws.space.canvas.blocks().is_empty(), "a new one is empty");
            assert_eq!(ws.others[0].name, "Main");
        });
        cx.update_window(handle, |_, window, cx| {
            window.find("workspace-name");
            window.click("workspace-default", cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.active, 0);
            assert_eq!(ws.space.focused, Some(first));
            assert!(!ws.space.terminals[&first].term.exited(), "still running");
        });
    }

    #[gpui_kit::test]
    fn workspace_keys_cycle_and_jump(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.new_space(window, cx);
                ws.new_space(window, cx);
            });
            window.render_frame(cx);
            assert_eq!(ws.read(cx).active, 2);
            window.press("ctrl-tab", cx);
            assert_eq!(ws.read(cx).active, 0, "wraps around");
            window.press("ctrl-shift-tab", cx);
            assert_eq!(ws.read(cx).active, 2);
            window.press(SECOND, cx);
            assert_eq!(ws.read(cx).active, 1);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn hidden_workspaces_get_theme_changes(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let first = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.new_space(window, cx));
        })
        .unwrap();
        ws.update(cx, |ws, cx| ws.set_dark(true, cx));
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.others[0].terminals[&first].term.palette(),
                ws.theme.palette
            );
        });
    }

    #[gpui_kit::test]
    fn renaming_and_deleting_workspaces(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                assert!(ws.rename_space("default", "  ", window, cx).is_err());
                assert_eq!(ws.space.name, "Main", "an empty name is refused");
                ws.rename_space("default", " Work ", window, cx).unwrap();
                assert_eq!(ws.space.name, "Work");
                ws.delete_space("default", window, cx);
                assert_eq!(ws.others.len(), 0, "the last one stays");
                ws.new_space(window, cx);
                ws.open(Rect::new(0.0, 0.0, 600.0, 400.0), window, cx);
                ws.save_canvas_now();
                ws.delete_space("w2", window, cx);
                assert_eq!((ws.active, ws.others.len()), (0, 0));
                assert_eq!(ws.space.name, "Work", "the one before it is shown");
            });
            window.render_frame(cx);
            let name = window.find("workspace-name");
            assert!(name.bounds().size.width > px(0.0));
        })
        .unwrap();
        let w2 = crate::session::path(data.path(), "w2");
        assert!(!w2.exists() && w2.with_extension("json.bak").exists());
        let index = crate::session::load_index(data.path());
        let crate::session::Loaded::Saved(index) = index else {
            panic!("no index: {index:?}");
        };
        assert_eq!(index.workspaces.len(), 1);
        assert_eq!(index.workspaces[0].name, "Work");
    }

    #[gpui_kit::test]
    fn every_workspace_comes_back_in_order(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.new_space(window, cx);
                ws.open(Rect::new(10.0, 20.0, 600.0, 400.0), window, cx);
                ws.open(Rect::new(700.0, 20.0, 600.0, 400.0), window, cx);
                ws.rename_space("w2", "Side project", window, cx).unwrap();
                ws.new_space(window, cx);
                ws.switch_to(1, window, cx);
                ws.toggle_sidebar(window, cx);
                ws.save_canvas_now();
            })
        })
        .unwrap();
        let (_, again) = open_in(cx, None, Some(data.path().to_path_buf()));
        again.read_with(cx, |ws, _| {
            let names: Vec<&str> = ws.order().iter().map(|s| s.name.as_str()).collect();
            assert_eq!(names, ["Main", "Side project", "Workspace 3"]);
            assert_eq!(ws.active, 1);
            assert!(!ws.sidebar);
            assert_eq!(ws.space.canvas.blocks().len(), 2);
            assert_eq!(ws.others[0].canvas.blocks().len(), 1, "Main's terminal");
            assert!(ws.others[1].canvas.blocks().is_empty());
        });
    }

    /// The kanban block on top.
    fn top_kanban(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> BlockId {
        ws.read_with(cx, |ws, _| {
            let id = ws.space.canvas.top().unwrap();
            assert!(
                ws.space.kanbans.contains_key(&id),
                "the top block is a kanban"
            );
            id
        })
    }

    #[gpui_kit::test]
    fn the_board_button_opens_a_block_with_the_board_list(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("new-kanban", cx);
            window.render_frame(cx);
        })
        .unwrap();
        let block = top_kanban(&ws, cx);
        let n = block.0 as usize;
        cx.update_window(handle, |_, window, cx| {
            assert!(window.try_find(("board-list", n)).is_some());
            window.click(("new-board", n), cx);
            window.render_frame(cx);
            assert!(
                window.try_find(("columns", n)).is_some(),
                "the new board shows"
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            let board = ws.boards.boards[0].id;
            assert_eq!(ws.space.kanbans[&block].board, Some(board));
            assert_eq!(ws.boards.board(board).unwrap().columns.len(), 3);
        });
    }

    #[gpui_kit::test]
    fn two_blocks_on_one_board_show_the_same_cards(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let (a, b, board, todo) = cx
            .update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.new_kanban(window, cx);
                    let a = ws.space.canvas.top().unwrap();
                    ws.new_board(a, cx);
                    ws.new_kanban(window, cx);
                    let b = ws.space.canvas.top().unwrap();
                    let board = ws.space.kanbans[&a].board.unwrap();
                    ws.show_board(b, board, cx);
                    let todo = ws.boards.board(board).unwrap().columns[0].id;
                    ws.space.appearing.clear();
                    (a, b, board, todo)
                })
            })
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            // Through the block on top: it covers most of the other.
            let add = gpui_kit::ElementId::Name(format!("add-card-{}-{todo}", b.0).into());
            window.click(add, cx);
            window.render_frame(cx);
            let card = ws.read(cx).boards.board(board).unwrap().columns[0].cards[0].id;
            for block in [a, b] {
                let id = gpui_kit::ElementId::Name(format!("card-{}-{card}", block.0).into());
                assert!(window.try_find(id).is_some(), "block {block:?} shows it");
            }
        })
        .unwrap();
        ws.update(cx, |ws, cx| ws.delete_board(board, cx));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            for block in [a, b] {
                let list = ("board-list", block.0 as usize);
                assert!(window.try_find(list).is_some(), "back to the list");
            }
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn enter_applies_a_typed_name(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            let target = ws.clone();
            ws.update(cx, |_, cx| {
                crate::prompt::ask_text(
                    "Rename workspace",
                    "Rename",
                    "Main",
                    move |text, window, cx| {
                        target
                            .update(cx, |ws, cx| ws.rename_space("default", &text, window, cx))
                            .is_ok()
                    },
                    window,
                    cx,
                )
            });
            window.render_frame(cx);
            window.input("Work", cx);
            window.press("enter", cx);
            window.render_frame(cx);
            assert_eq!(ws.read(cx).space.name, "Work");
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn an_open_card_toggles_labels_and_can_be_deleted(cx: &mut TestAppContext) {
        use crate::board::Label;
        let (handle, ws) = open(cx);
        let (block, board, card) = cx
            .update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.new_kanban(window, cx);
                    let block = ws.space.canvas.top().unwrap();
                    ws.new_board(block, cx);
                    let board = ws.space.kanbans[&block].board.unwrap();
                    let todo = ws.boards.board(board).unwrap().columns[0].id;
                    let card = ws.boards.add_card(board, todo).unwrap();
                    ws.space.appearing.clear();
                    (block, board, card)
                })
            })
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let id = gpui_kit::ElementId::Name(format!("card-{}-{card}", block.0).into());
            assert!(window.try_find(id).is_some());
            ws.update(cx, |ws, cx| {
                let current = ws.boards.card(board, card).cloned().unwrap();
                crate::kanban::open_card(board, current, false, window, cx)
            });
            window.render_frame(cx);
            // The dialog slides in for 0.25 s first: presses meanwhile are lost.
            std::thread::sleep(std::time::Duration::from_millis(300));
            window.render_frame(cx);
            // The title has the keyboard when the card opens.
            window.input("!", cx);
        })
        .unwrap();
        // Subscribers hear the typing once that update ends.
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let title = ws.read(cx).boards.card(board, card).unwrap().title.clone();
            assert!(title.contains('!'), "typing applies at once: {title:?}");
            window.click("label-green", cx);
            window.render_frame(cx);
            let labels = ws.read(cx).boards.card(board, card).unwrap().labels.clone();
            assert_eq!(labels, [Label::Green]);
            window.click("delete-card", cx);
            window.render_frame(cx);
            assert!(ws.read(cx).boards.card(board, card).is_none());
            assert!(
                window.try_find("delete-card").is_none(),
                "the dialog closed"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn the_wheel_over_a_kanban_block_stays_in_it_and_ctrl_zooms(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let (block, doing) = cx
            .update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.new_kanban(window, cx);
                    let block = ws.space.canvas.top().unwrap();
                    ws.new_board(block, cx);
                    ws.space.appearing.clear();
                    let board = ws.space.kanbans[&block].board.unwrap();
                    (block, ws.boards.board(board).unwrap().columns[1].id)
                })
            })
            .unwrap();
        let column = gpui_kit::ElementId::Name(format!("column-{}-{doing}", block.0).into());
        // As a pointer does: over the column first, then the wheel.
        let wheel_over = |window: &mut Window, cx: &mut gpui_kit::App, delta, control| {
            window.hover(column.clone(), cx);
            let at = window.find(column.clone()).bounds().center();
            wheel(
                window,
                cx,
                (f32::from(at.x), f32::from(at.y)),
                delta,
                control,
            );
            window.render_frame(cx);
        };
        let before = ws.read_with(cx, |ws, _| ws.space.canvas.camera);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let x = window.find(column.clone()).bounds().origin.x;
            wheel_over(
                window,
                cx,
                ScrollDelta::Pixels(point(px(0.), px(-200.))),
                false,
            );
            assert_eq!(
                window.find(column.clone()).bounds().origin.x,
                x,
                "a vertical wheel doesn't slide the columns"
            );
            wheel_over(
                window,
                cx,
                ScrollDelta::Pixels(point(px(-200.), px(0.))),
                false,
            );
            assert!(
                window.find(column.clone()).bounds().origin.x < x,
                "a sideways one does"
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.camera, before, "no pan")
        });
        cx.update_window(handle, |_, window, cx| {
            wheel_over(window, cx, ScrollDelta::Lines(point(0.0, 1.0)), true);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.camera.zoom(), 1.1, "Ctrl zooms")
        });
    }

    #[gpui_kit::test]
    fn holding_alt_moves_the_canvas_over_blocks(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let alt = Modifiers {
            alt: true,
            ..Default::default()
        };
        let before = ws.read_with(cx, |ws, _| ws.space.canvas.camera.origin);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("hand").is_none(), "not without Alt");
            hold_alt(window, cx, true);
            assert!(
                window.try_find("hand").is_some(),
                "the hand covers the canvas"
            );
            // A drag over the terminal: the canvas moves, nothing in the terminal is selected.
            let body = window.find(("body", id.0 as usize)).bounds().center();
            let from = (f32::from(body.x), f32::from(body.y));
            press_with(
                window,
                cx,
                MouseButton::Left,
                alt,
                from,
                (from.0 + 100.0, from.1 + 40.0),
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            let moved = ws.space.canvas.camera.origin;
            assert_eq!((moved.x, moved.y), (before.x - 100.0, before.y - 40.0));
            let term = ws.space.terminals[&id].term.shared();
            assert!(term.selection_text().is_none(), "no selection");
            assert!(ws.drag.is_none(), "the drag ended");
        });
        // The wheel over the terminal pans the canvas, not the terminal's history.
        cx.update_window(handle, |_, window, cx| {
            let body = window.find(("body", id.0 as usize)).bounds().center();
            window.dispatch_event(
                PlatformInput::ScrollWheel(ScrollWheelEvent {
                    position: body,
                    delta: ScrollDelta::Pixels(point(px(0.), px(-60.))),
                    modifiers: alt,
                    touch_phase: TouchPhase::Moved,
                }),
                cx,
            );
            hold_alt(window, cx, false);
            assert!(window.try_find("hand").is_none(), "gone when Alt is let go");
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.camera.origin.y, before.y - 40.0 + 60.0);
        });
    }

    #[gpui_kit::test]
    fn alt_let_go_elsewhere_is_noticed_at_the_next_move(cx: &mut TestAppContext) {
        use std::cell::Cell;
        use std::rc::Rc;
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            hold_alt(window, cx, true);
        })
        .unwrap();
        let redraw = Rc::new(Cell::new(false));
        let seen = redraw.clone();
        cx.update(|cx| cx.observe(&ws, move |_, _| seen.set(true)).detach());
        // Let go while another window had the keyboard: only the next move says so, and the
        // workspace redraws without its hand.
        cx.update_window(handle, |_, window, cx| {
            window.dispatch_event(
                PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                    position: point(px(300.), px(300.)),
                    pressed_button: None,
                    modifiers: Modifiers::default(),
                }),
                cx,
            );
        })
        .unwrap();
        cx.run_until_parked();
        assert!(redraw.get(), "the move asks for a frame");
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("hand").is_none());
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn a_middle_drag_moves_the_canvas_anywhere(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let before = ws.read_with(cx, |ws, _| ws.space.canvas.camera.origin);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let body = window.find(("body", id.0 as usize)).bounds().center();
            let from = (f32::from(body.x), f32::from(body.y));
            press_with(
                window,
                cx,
                MouseButton::Middle,
                Modifiers::default(),
                from,
                (from.0 - 50.0, from.1),
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.canvas.camera.origin.x, before.x + 50.0);
            assert!(ws.drag.is_none());
        });
    }

    #[gpui_kit::test]
    fn the_canvas_menu_adds_blocks_where_it_was_opened(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        // Left of the startup terminal, then right of it: empty canvas both times.
        for (item, at) in [(0usize, (20.0, 600.0)), (1, (1100.0, 20.0))] {
            let world = cx
                .update_window(handle, |_, window, cx| {
                    window.render_frame(cx);
                    let o = ws.read(cx).origin;
                    let at = (f32::from(o.x) + at.0, f32::from(o.y) + at.1);
                    right_click(window, cx, at);
                    window.render_frame(cx);
                    assert!(ws.read(cx).menu.is_some(), "the menu opened");
                    let ws = ws.read(cx);
                    ws.space
                        .canvas
                        .camera
                        .to_world(ws.local(point(px(at.0), px(at.1))))
                })
                .unwrap();
            cx.update_window(handle, |_, window, cx| {
                // AI agent ▸, Add ▸ Terminal or Kanban board.
                window.within("popup-menu").click(1usize, cx);
                window.render_frame(cx);
                window
                    .within("submenu")
                    .within("popup-menu")
                    .click(item, cx);
                window.render_frame(cx);
            })
            .unwrap();
            ws.read_with(cx, |ws, _| {
                let top = ws.space.canvas.top().unwrap();
                assert_eq!(ws.space.canvas.get(top).unwrap().rect.origin(), world);
                assert_eq!(ws.space.kanbans.contains_key(&top), item == 1);
                assert!(ws.menu.is_none(), "a choice closes it");
            });
        }
    }

    #[gpui_kit::test]
    fn the_canvas_menu_opens_an_agent_in_a_recent_folder(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let folder = tempfile::tempdir().unwrap();
        ws.update(cx, |ws, _| {
            ws.space.recent = vec![folder.path().to_path_buf()];
            // One agent, installed everywhere: the menu's first, whatever this computer has.
            ws.agents = vec![crate::agents::AgentEntry {
                id: "mine".into(),
                title: "Mine".into(),
                command: quiet().0,
                resume: None,
                install: None,
                builtin: None,
                changed: false,
                show: true,
            }];
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let o = ws.read(cx).origin;
            right_click(window, cx, (f32::from(o.x) + 20.0, f32::from(o.y) + 600.0));
            window.render_frame(cx);
            // AI agent ▸ its first agent.
            window.within("popup-menu").click(0usize, cx);
            window.render_frame(cx);
            window
                .within("submenu")
                .within("popup-menu")
                .click(0usize, cx);
            window.render_frame(cx);
            assert!(
                window.try_find("choose-folder").is_some(),
                "the folder dialog"
            );
        })
        .unwrap();
        // The dialog slides in for 0.25 s first: presses meanwhile are lost.
        std::thread::sleep(std::time::Duration::from_millis(300));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("recent-0", cx);
            window.render_frame(cx);
            assert!(window.try_find("choose-folder").is_none(), "it closed");
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.terminals.len(), 2, "the agent's terminal");
            assert_eq!(ws.space.recent, [folder.path()]);
        });
    }

    #[gpui_kit::test]
    fn the_innermost_menu_opens_and_acts_on_what_was_clicked(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let terminal = first_id(&ws, cx);
        let (block, board, card) = cx
            .update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.add_kanban_at(Point::new(900.0, 0.0), window, cx);
                    let block = ws.space.canvas.top().unwrap();
                    ws.new_board(block, cx);
                    let board = ws.space.kanbans[&block].board.unwrap();
                    let todo = ws.boards.board(board).unwrap().columns[0].id;
                    let card = ws.boards.add_card(board, todo).unwrap();
                    ws.space.appearing.clear();
                    (block, board, card)
                })
            })
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let card_el = gpui_kit::ElementId::Name(format!("card-{}-{card}", block.0).into());
            window.right_click(card_el, cx);
            window.render_frame(cx);
            // Open, —, Delete card: the card's menu, not its block's (Board list, —, Close).
            window.within("popup-menu").click(2usize, cx);
            window.render_frame(cx);
            let ws = ws.read(cx);
            assert!(ws.boards.card(board, card).is_none(), "the card went");
            assert!(ws.space.kanbans.contains_key(&block), "its block stayed");
        })
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            window.right_click(("body", terminal.0 as usize), cx);
            window.render_frame(cx);
            // A second right-click replaces the menu.
            window.right_click(("body", terminal.0 as usize), cx);
            window.render_frame(cx);
            // Copy, Paste, —, Close terminal.
            window.within("popup-menu").click(3usize, cx);
            window.render_frame(cx);
            assert!(
                !ws.read(cx).space.terminals.contains_key(&terminal),
                "the terminal closed"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn kanban_blocks_and_boards_come_back(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        let board = cx
            .update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.new_kanban(window, cx);
                    let block = ws.space.canvas.top().unwrap();
                    ws.new_board(block, cx);
                    let board = ws.space.kanbans[&block].board.unwrap();
                    let todo = ws.boards.board(board).unwrap().columns[0].id;
                    ws.boards.add_card(board, todo);
                    ws.new_kanban(window, cx);
                    ws.save_canvas_now();
                    board
                })
            })
            .unwrap();
        let before = ws.read_with(cx, |ws, _| ws.boards.clone());
        let (_, again) = open_in(cx, None, Some(data.path().to_path_buf()));
        again.read_with(cx, |ws, _| {
            assert_eq!(ws.boards, before);
            let mut shown: Vec<Option<u64>> = ws
                .space
                .canvas
                .blocks()
                .iter()
                .filter_map(|b| ws.space.kanbans.get(&b.id).map(|k| k.board))
                .collect();
            shown.sort();
            assert_eq!(shown, [None, Some(board)]);
            assert_eq!(ws.space.terminals.len(), 1, "and the terminal");
        });
    }

    #[gpui_kit::test]
    fn snapshots_write_only_what_changed(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let path = crate::session::path(data.path(), crate::session::DEFAULT_ID);
        let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        tick(cx);
        assert!(path.exists(), "the first snapshot is written");
        std::fs::remove_file(&path).unwrap();
        tick(cx);
        assert!(!path.exists(), "nothing changed: nothing written");
        ws.update(cx, |ws, _| {
            ws.space.canvas.camera.pan_by(Point::new(30.0, 0.0))
        });
        tick(cx);
        assert!(path.exists(), "a pan is a change");
    }

    #[gpui_kit::test]
    fn a_failing_canvas_write_says_so_once_and_recovers(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        // A file where the workspaces folder should be: every write fails.
        let blocker = data.path().join("workspaces");
        std::fs::write(&blocker, "").unwrap();
        let pan = |cx: &mut TestAppContext| {
            ws.update(cx, |ws, _| {
                ws.space.canvas.camera.pan_by(Point::new(10.0, 0.0))
            })
        };
        pan(cx);
        tick(cx);
        assert_eq!(notification_count(handle, cx), 1, "said once");
        // Still failing: retried every period without saying so again (the first hides after 5 s).
        for _ in 0..3 {
            pan(cx);
            tick(cx);
        }
        assert_eq!(notification_count(handle, cx), 0, "not again");
        std::fs::remove_file(&blocker).unwrap();
        tick(cx);
        assert!(
            crate::session::path(data.path(), crate::session::DEFAULT_ID).exists(),
            "retried"
        );
        assert_eq!(notification_count(handle, cx), 1, "and says it works again");
    }

    #[gpui_kit::test]
    fn closing_the_window_saves_the_canvas(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        ws.update(cx, |ws, _| {
            let first = ws.space.canvas.blocks()[0].id;
            ws.space.canvas.get_mut(first).unwrap().rect = Rect::new(-500.0, 0.0, 700.0, 450.0);
        });
        // The test's handle would keep the workspace alive.
        drop(ws);
        cx.update_window(handle, |_, window, _| window.remove_window())
            .unwrap();
        cx.run_until_parked();
        let text = std::fs::read_to_string(crate::session::path(
            data.path(),
            crate::session::DEFAULT_ID,
        ))
        .unwrap();
        assert!(text.contains("-500.0"), "{text}");
    }

    #[gpui_kit::test]
    fn without_a_data_folder_nothing_is_kept(cx: &mut TestAppContext) {
        let (_, ws) = open(cx);
        tick(cx);
        ws.read_with(cx, |ws, _| assert_eq!(ws.space.saved, None));
    }

    #[gpui_kit::test]
    fn shells_that_cant_start_leave_the_saved_canvas_alone(cx: &mut TestAppContext) {
        let data = tempfile::tempdir().unwrap();
        let text = r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[
              {"kind":"terminal","rect":{"x":0,"y":0,"w":700,"h":450}}]}"#;
        write_canvas(data.path(), text);
        // A shell setting with a typo: no block can start.
        let broken = Some(("/nonexistent/templum-shell".to_string(), Vec::new()));
        let (_, ws) = open_as(cx, broken, None, Some(data.path().to_path_buf()));
        tick(cx);
        ws.read_with(cx, |ws, _| assert!(ws.space.canvas.blocks().is_empty()));
        assert_eq!(
            std::fs::read_to_string(crate::session::path(
                data.path(),
                crate::session::DEFAULT_ID
            ))
            .unwrap(),
            text,
            "kept for the start after the setting is fixed"
        );
    }

    #[gpui_kit::test]
    fn a_saved_agent_is_resumed_in_its_block(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        use templum_terminal::Snapshot;
        let data = tempfile::tempdir().unwrap();
        let folder = tempfile::tempdir().unwrap();
        let folder = folder.path().display().to_string().replace('\\', "\\\\");
        write_canvas(
            data.path(),
            &format!(
                r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{folder}","agent":"claude"}}]}}"#
            ),
        );
        let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        let id = first_id(&ws, cx);
        // The test shell echoes what it is typed.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let shown = ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
                snap.text()
            });
            if shown.contains("claude --continue") {
                break;
            }
            assert!(Instant::now() < deadline, "not typed: {shown:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn recent_folders_are_newest_first_without_repeats_and_few() {
        use std::path::PathBuf;
        let mut recent = Vec::new();
        for i in 0..12 {
            super::remember(&mut recent, PathBuf::from(format!("/p{i}")));
        }
        super::remember(&mut recent, PathBuf::from("/p8"));
        assert_eq!(recent.len(), super::RECENT);
        assert_eq!(recent[0], PathBuf::from("/p8"));
        assert_eq!(recent[1], PathBuf::from("/p11"));
        assert_eq!(
            recent
                .iter()
                .filter(|p| *p == std::path::Path::new("/p8"))
                .count(),
            1
        );
    }

    #[gpui_kit::test]
    fn an_agent_opens_in_its_folder_and_the_folder_becomes_recent(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        use templum_terminal::Snapshot;
        let (handle, ws) = open(cx);
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let id = cx
            .update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.launch_agent("codex", b.path(), Point::new(0.0, 900.0), window, cx);
                    ws.launch_agent("claude", a.path(), Point::new(40.0, 0.0), window, cx)
                })
            })
            .unwrap()
            .expect("a block");
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.recent, [a.path(), b.path()]);
            assert_eq!(
                ws.space.canvas.get(id).unwrap().rect.origin(),
                Point::new(40.0, 0.0)
            );
            assert_eq!(ws.space.focused, Some(id));
        });
        assert_eq!(folder_of(&ws, id, cx), a.path().canonicalize().unwrap());
        // The test shell echoes what it is typed.
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let shown = ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
                snap.text()
            });
            if shown.contains("claude") {
                break;
            }
            assert!(Instant::now() < deadline, "not typed: {shown:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[gpui_kit::test]
    fn agents_are_changed_reset_added_and_deleted_from_the_settings(cx: &mut TestAppContext) {
        use crate::agents::Draft;
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        let frame = |cx: &mut TestAppContext| {
            cx.update_window(handle, |_, window, cx| window.render_frame(cx))
                .unwrap()
        };
        let mine = ws.update(cx, |ws, cx| {
            let claude = ws.agents.iter().find(|a| a.id == "claude").unwrap();
            let mut draft = Draft::of(claude);
            draft.command = "claude --model opus".into();
            ws.save_agent(Some("claude"), &draft, cx).unwrap();
            let new = Draft {
                name: "My Agent".into(),
                command: "my-agent --tui".into(),
                resume: String::new(),
                show: true,
            };
            assert_eq!(
                ws.save_agent(
                    None,
                    &Draft {
                        command: " ".into(),
                        ..new.clone()
                    },
                    cx
                ),
                Err("An agent needs a command")
            );
            ws.save_agent(None, &new, cx).unwrap()
        });
        assert_eq!(mine, "my-agent");
        frame(cx);
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
        assert!(
            text.contains("claude --model opus") && text.contains("my-agent --tui"),
            "{text}"
        );
        ws.read_with(cx, |ws, _| {
            let claude = ws.agents.iter().find(|a| a.id == "claude").unwrap();
            assert_eq!(
                (claude.command.as_str(), claude.changed),
                ("claude --model opus", true)
            );
            assert_eq!(ws.agents.last().unwrap().title, "My Agent");
        });
        ws.update(cx, |ws, cx| {
            ws.remove_agent("claude", cx);
            ws.remove_agent("my-agent", cx);
        });
        frame(cx);
        ws.update(cx, |ws, _| ws.save());
        let text = std::fs::read_to_string(dir.path().join(crate::config::FILE)).unwrap();
        assert!(
            !text.contains("claude --model") && !text.contains("my-agent"),
            "{text}"
        );
        ws.read_with(cx, |ws, _| {
            let claude = ws.agents.iter().find(|a| a.id == "claude").unwrap();
            assert_eq!((claude.command.as_str(), claude.changed), ("claude", false));
            assert!(ws.agents.iter().all(|a| a.id != "my-agent"));
        });
    }

    #[gpui_kit::test]
    fn a_changed_resume_command_is_what_comes_back(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        use templum_terminal::Snapshot;
        let (config, data) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        write_config(
            config.path(),
            "[agents.claude]\nresume = \"claude -c --fast\"\n",
        );
        let folder = tempfile::tempdir().unwrap();
        let folder = folder.path().display().to_string().replace('\\', "\\\\");
        write_canvas(
            data.path(),
            &format!(
                r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{folder}","agent":"claude"}}]}}"#
            ),
        );
        let (_, ws) = open_in(
            cx,
            Some(config.path().to_path_buf()),
            Some(data.path().to_path_buf()),
        );
        let id = first_id(&ws, cx);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let shown = ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
                snap.text()
            });
            if shown.contains("claude -c --fast") {
                break;
            }
            assert!(Instant::now() < deadline, "not typed: {shown:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[gpui_kit::test]
    fn an_agent_whose_folder_is_gone_isnt_resumed_elsewhere(cx: &mut TestAppContext) {
        use std::time::{Duration, Instant};
        use templum_terminal::Snapshot;
        let data = tempfile::tempdir().unwrap();
        let here = tempfile::tempdir().unwrap();
        let json = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
        write_canvas(
            data.path(),
            &format!(
                r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{}","agent":"claude"}},
                  {{"kind":"terminal","rect":{{"x":800,"y":0,"w":700,"h":450}},"folder":"{}","agent":"claude"}}]}}"#,
                json(&data.path().join("gone")),
                json(here.path())
            ),
        );
        let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
        let ids: Vec<BlockId> = ws.read_with(cx, |ws, _| {
            ws.space.canvas.blocks().iter().map(|b| b.id).collect()
        });
        let shown = |id: BlockId, cx: &mut TestAppContext| {
            ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.space.terminals[&id].term.shared(), &ws.theme.palette);
                snap.text()
            })
        };
        // Both would be typed at once: once the second shows it, the first would too.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !shown(ids[1], cx).contains("claude --continue") {
            assert!(Instant::now() < deadline, "the block in its folder resumes");
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            !shown(ids[0], cx).contains("claude"),
            "not in the home folder: that's another conversation"
        );
    }
}
