//! The main view: the canvas of terminal blocks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{TITLE_BAR_HEIGHT, WindowExt, window_paddings};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, App, Bounds, ClipboardItem, Context, DispatchPhase, ElementInputHandler,
    EntityInputHandler, FocusHandle, Hsla, KeyBinding, KeyDownEvent, Keystroke, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, Point as PixelPoint,
    ScrollDelta, ScrollWheelEvent, SharedString, Subscription, Task, UTF16Selection, Window,
    WindowAppearance, canvas, div, fill, point, px, size,
};
use templum_canvas::{
    Block, BlockId, Camera, Canvas, Gesture, Point, Rect, Size, chrome, dot_grid,
};
use templum_terminal::{KeyInput, Notify, SelectionKind, Side, Snapshot, SpawnOptions, Terminal};

use crate::block;
use crate::config::{self, Config, Edit, ThemeConfig, WorkingDirectory};
use crate::grid::{CellMetrics, hsla};
use crate::keys::{self, Command, Keymap, Keys};
use crate::motion;
use crate::settings::{self, Panel};
use crate::theme::{self, Entry, Theme};
use crate::theme_picker::{self, Picker};
use crate::title_bar;

pub(crate) const DEFAULT_COLS: u16 = 80;
pub(crate) const DEFAULT_ROWS: u16 = 24;
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
}

/// A closed block, still drawn while it fades out. Its terminal lives until then.
pub(crate) struct Ghost {
    pub block: Block,
    pub pane: Pane,
    since: Instant,
}

#[derive(Clone, Copy)]
enum Drag {
    Canvas(Gesture),
    /// Selecting text in a block's terminal.
    Select(BlockId),
}

/// Called at the start of every render; returning false removes it (the bench drives frames).
pub(crate) type FrameHook =
    Box<dyn FnMut(&mut Workspace, &mut Window, &mut Context<Workspace>) -> bool>;

pub struct Workspace {
    pub(crate) canvas: Canvas,
    pub(crate) terminals: HashMap<BlockId, Pane>,
    pub(crate) focused: Option<BlockId>,
    pub(crate) cell: CellMetrics,
    /// IME pre-edit text, drawn at the cursor until committed.
    pub(crate) marked: Option<String>,
    pub(crate) ime_cursor: Option<Bounds<Pixels>>,
    pub(crate) on_frame: Option<FrameHook>,
    drag: Option<Drag>,
    /// Where a press on the empty canvas started: released in place, it's a click.
    press: Option<Point>,
    focus_handle: FocusHandle,
    /// Window position of the workspace's top-left: screen coordinates are relative to it.
    origin: PixelPoint<Pixels>,
    notify: Notify,
    /// Sub-step accumulators: wheel zoom, terminal scrolling (in lines).
    zoom_acc: f32,
    scroll_acc: f32,
    /// Program for new terminals instead of the login shell (tests).
    command: Option<(String, Vec<String>)>,
    /// True until the user first moves the camera or touches a block: meanwhile the startup
    /// terminal stays centered, since tiling compositors resize the window after it opens.
    pub(crate) pristine: bool,
    /// Where config.toml and the themes folder live (None: defaults only, nothing saved).
    pub(crate) config_dir: Option<PathBuf>,
    pub(crate) themes: Vec<Entry>,
    /// The system's appearance.
    dark: bool,
    /// What everything is drawn with.
    pub(crate) theme: Rc<Theme>,
    /// The window shows the blurred desktop: the canvas is drawn at window.opacity.
    pub(crate) translucent: bool,
    /// The active config.toml: the last valid one.
    pub(crate) config: Config,
    /// Its shortcuts.
    pub(crate) keymap: Keymap,
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
    /// The block under the pointer: it shows its close button and resize grip.
    pub(crate) hovered: Option<BlockId>,
    /// The open theme picker.
    pub(crate) picker: Option<Picker>,
    /// Blocks still popping in, since when.
    pub(crate) appearing: HashMap<BlockId, Instant>,
    /// Closed blocks still fading out.
    pub(crate) ghosts: Vec<Ghost>,
    /// The focused terminal's cursor blinks (painting reports it); and the blink phase.
    pub(crate) cursor_blinks: bool,
    pub(crate) blink_on: bool,
    _blink: Task<()>,
    _wake: Task<()>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with(None, config::dir(), window, cx)
    }

    /// `command`: program for new terminals instead of the login shell (tests).
    pub fn with(
        command: Option<(String, Vec<String>)>,
        config_dir: Option<PathBuf>,
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
        let translucent = config.window.translucent();
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
            }),
            cx.on_app_quit(|ws, _| {
                ws.save();
                async {}
            }),
        ];
        let mut ws = Self {
            canvas: Canvas::default(),
            terminals: HashMap::new(),
            focused: None,
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
            pristine: true,
            config_dir,
            themes,
            dark,
            theme,
            translucent,
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
            drafts: HashMap::new(),
            quit_armed: None,
            settings: None,
            settings_focus: cx.focus_handle(),
            recording: None,
            key_note: None,
            resetting: Vec::new(),
            hovered: None,
            picker: None,
            appearing: HashMap::new(),
            ghosts: Vec::new(),
            cursor_blinks: false,
            blink_on: true,
            _blink: blink,
            config,
            keymap,
            _wake: wake,
        };
        // Startup: one terminal, centered.
        let vp = viewport(window);
        let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, ws.cell.size);
        let rect = Rect::new((vp.w - size.w) / 2.0, (vp.h - size.h) / 2.0, size.w, size.h);
        ws.open(rect, window, cx);
        ws
    }

    /// Starts a terminal in a new focused block; on failure, a notification instead.
    pub(crate) fn open(&mut self, rect: Rect, window: &mut Window, cx: &mut Context<Self>) {
        let (cols, rows) = chrome::grid_size(chrome::content_rect(rect).size(), self.cell.size);
        let command = self.command.clone().or_else(|| {
            let shell = &self.config.shell;
            shell
                .program
                .clone()
                .map(|program| (program, shell.args.clone()))
        });
        let working_directory = self.start_dir(window, cx);
        let opts = SpawnOptions {
            cols,
            rows,
            cell_px: self.cell.px(),
            command,
            palette: self.theme.palette,
            options: self.config.term_options(),
            working_directory,
        };
        match Terminal::spawn(opts, self.notify.clone()) {
            Ok(term) => {
                let id = self.canvas.add(rect);
                self.terminals.insert(
                    id,
                    Pane {
                        term,
                        snapshot: Rc::default(),
                    },
                );
                self.focused = Some(id);
                self.appearing.insert(id, Instant::now());
            }
            Err(err) => {
                eprintln!("templum: couldn't start a shell: {err}");
                let message = format!("Couldn't start a shell: {err}");
                show_notification(window, cx, Notification::error(message));
            }
        }
        cx.notify();
    }

    /// Removes a block and kills its shell (dropping the Terminal shuts it down).
    /// The block leaves the canvas at once; it stays on screen as a ghost while it fades out.
    pub(crate) fn close(&mut self, id: BlockId, cx: &mut Context<Self>) {
        let block = self.canvas.get(id).copied();
        self.canvas.remove(id);
        self.appearing.remove(&id);
        if let (Some(block), Some(pane)) = (block, self.terminals.remove(&id)) {
            self.ghosts.push(Ghost {
                block,
                pane,
                since: Instant::now(),
            });
        }
        if self.hovered == Some(id) {
            self.hovered = None;
        }
        if self.focused == Some(id) {
            self.focused = self.canvas.top();
        }
        // A gesture on a block that's gone would do nothing anyway; end it.
        self.drag = None;
        cx.notify();
    }

    /// Draws with `theme` from the next frame on, in every terminal and gpui-kit widget.
    pub(crate) fn apply(&mut self, theme: Rc<Theme>, cx: &mut Context<Self>) {
        for pane in self.terminals.values() {
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
        if self.config.window != old.window {
            self.translucent = self.config.window.translucent();
            window.set_background_appearance(self.config.window.background());
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
            for pane in self.terminals.values() {
                pane.term.shared().set_options(options);
            }
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
        if edit.value.is_none() && self.drafts.remove(edit.key).is_some() {
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
            .retain(|e| (e.table, e.key) != (edit.table, edit.key));
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
        for (id, pane) in self.terminals.iter_mut() {
            if let Some(block) = self.canvas.get(*id) {
                let content = chrome::content_rect(block.rect).size();
                let (cols, rows) = chrome::grid_size(content, self.cell.size);
                pane.term.resize(cols, rows, self.cell.px());
            }
        }
    }

    /// Where a new terminal starts (None: the home folder).
    fn start_dir(&self, window: &mut Window, cx: &mut App) -> Option<PathBuf> {
        match &self.config.shell.working_directory {
            WorkingDirectory::Inherit => self
                .focused
                .and_then(|id| self.terminals.get(&id))
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

    /// The canvas's alpha: window.opacity when the window is translucent, else opaque.
    pub(crate) fn canvas_alpha(&self) -> f32 {
        if self.translucent {
            self.config.window.opacity
        } else {
            1.0
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
                self.unsaved.retain(|e| (e.table, e.key) != ("theme", key));
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
        let now = Instant::now();
        self.settings = Some(match &self.settings {
            // Still sliding out: it turns back from where it is.
            Some(closing) => closing.reversed(now),
            None => Panel {
                since: now,
                closing: false,
            },
        });
        window.focus(&self.settings_focus, cx);
        cx.notify();
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
        let command = self
            .keymap
            .command(&e.keystroke)
            .filter(|c| !matches!(c, Command::Copy | Command::Paste | Command::CloseTerminal));
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
        self.pristine = false;
        self.canvas.raise(id);
        self.focused = Some(id);
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
        } else if self.hovered == Some(id) {
            None
        } else {
            self.hovered
        };
        if next != self.hovered {
            self.hovered = next;
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
        self.drag = Gesture::move_block(&self.canvas, id, p).map(Drag::Canvas);
    }

    pub(crate) fn begin_resize(&mut self, id: BlockId, pos: PixelPoint<Pixels>) {
        let p = self.local(pos);
        let min = chrome::min_block_size(self.cell.size);
        self.drag = Gesture::resize_block(&self.canvas, id, p, min).map(Drag::Canvas);
    }

    fn drag_to(&mut self, pos: PixelPoint<Pixels>, cx: &mut Context<Self>) {
        let p = self.local(pos);
        match &mut self.drag {
            Some(Drag::Canvas(gesture)) => {
                if let Some((id, rect)) = gesture.update(&mut self.canvas, p) {
                    let (cols, rows) =
                        chrome::grid_size(chrome::content_rect(rect).size(), self.cell.size);
                    if let Some(pane) = self.terminals.get_mut(&id) {
                        pane.term.resize(cols, rows, self.cell.px());
                    }
                }
            }
            Some(Drag::Select(id)) => {
                let id = *id;
                if let (Some((row, col, side)), Some(pane)) =
                    (self.cell_under(id, pos), self.terminals.get(&id))
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
        let block = self.canvas.get(id)?;
        let world = self.canvas.camera.to_world(self.local(pos));
        let (row, col, right) = chrome::cell_at(block.rect, world, self.cell.size);
        let side = if right { Side::Right } else { Side::Left };
        Some((row, col, side))
    }

    /// Mouse down in a body: 1 click selects by character, 2 by word, 3 by line.
    pub(crate) fn begin_select(&mut self, id: BlockId, e: &MouseDownEvent) {
        let Some((row, col, side)) = self.cell_under(id, e.position) else {
            return;
        };
        let Some(pane) = self.terminals.get(&id) else {
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

    fn copy_selection(&mut self, cx: &mut Context<Self>) {
        let text = self
            .focused
            .and_then(|id| self.terminals.get(&id))
            .and_then(|pane| pane.term.shared().selection_text());
        if let Some(text) = text {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn center(window: &Window) -> Point {
        let vp = viewport(window);
        Point::new(vp.w / 2.0, vp.h / 2.0)
    }

    fn zoom_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.camera.zoom_step(1, Self::center(window));
        cx.notify();
    }

    fn zoom_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.camera.zoom_step(-1, Self::center(window));
        cx.notify();
    }

    pub(crate) fn zoom_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.camera.zoom_reset(Self::center(window));
        cx.notify();
    }

    fn fit_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.fit_all(viewport(window));
        cx.notify();
    }

    fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(pane) = self.focused.and_then(|id| self.terminals.get(&id)) {
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
            if moved < CLICK_SLOP && self.focused.take().is_some() {
                cx.notify();
            }
        }
        self.end_drag();
    }

    pub(crate) fn new_terminal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        let vp = viewport(window);
        let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
        let center = self
            .canvas
            .camera
            .to_world(Point::new(vp.w / 2.0, vp.h / 2.0));
        let near = Point::new(center.x - size.w / 2.0, center.y - size.h / 2.0);
        let rect = self.canvas.place_new(size, near);
        self.open(rect, window, cx);
    }

    fn close_terminal(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.focused {
            self.close(id, cx);
        }
    }

    fn run(&mut self, command: Command, window: &mut Window, cx: &mut Context<Self>) {
        match command {
            Command::NewTerminal => self.new_terminal(window, cx),
            Command::CloseTerminal => self.close_terminal(cx),
            Command::Copy => self.copy_selection(cx),
            Command::Paste => self.paste_clipboard(cx),
            Command::ZoomIn => self.zoom_in(window, cx),
            Command::ZoomOut => self.zoom_out(window, cx),
            Command::ZoomReset => self.zoom_reset(window, cx),
            Command::FitAll => self.fit_all(window, cx),
            Command::ThemePicker => self.toggle_theme_picker(cx),
            Command::OpenConfig => self.open_config(window, cx),
            Command::Settings => self.toggle_settings(window, cx),
            Command::Quit => self.quit(window, cx),
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

    /// The empty-canvas hint, with the new_terminal shortcut in effect.
    fn hint(&self) -> String {
        match self.keymap.first(Command::NewTerminal) {
            Some(shortcut) => format!(
                "Double-click or press {} to open a terminal",
                keys::display(shortcut)
            ),
            None => "Double-click to open a terminal".to_string(),
        }
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
        let Some(pane) = self.focused.and_then(|id| self.terminals.get(&id)) else {
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
        self.pristine = false;
        let p = self.local(e.position);
        if e.click_count == 2 {
            let at = self.canvas.camera.to_world(p);
            let size = chrome::block_size(DEFAULT_COLS, DEFAULT_ROWS, self.cell.size);
            self.open(Rect::new(at.x, at.y, size.w, size.h), window, cx);
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
        self.pristine = false;
        let p = self.local(e.position);
        if e.modifiers.control {
            self.zoom_acc += match e.delta {
                ScrollDelta::Lines(d) => d.y * 50.0,
                ScrollDelta::Pixels(d) => f32::from(d.y),
            };
            if self.zoom_acc.abs() >= 50.0 {
                self.canvas
                    .camera
                    .zoom_step(self.zoom_acc.signum() as i32, p);
                self.zoom_acc = 0.0;
            }
        } else {
            let d = e.delta.pixel_delta(px(self.cell.size.h));
            self.canvas
                .camera
                .pan_by(Point::new(f32::from(d.x), f32::from(d.y)));
        }
        cx.notify();
    }

    pub(crate) fn on_pinch(&mut self, e: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.zoom_acc += e.delta;
        if self.zoom_acc.abs() >= 0.15 {
            let p = self.local(e.position);
            self.canvas
                .camera
                .zoom_step(self.zoom_acc.signum() as i32, p);
            self.zoom_acc = 0.0;
            cx.notify();
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
        let line = self.cell.size.h * self.canvas.camera.zoom();
        self.scroll_acc += f32::from(e.delta.pixel_delta(px(line)).y) / line;
        let lines = self.scroll_acc.trunc();
        self.scroll_acc -= lines;
        if lines != 0.0 {
            if let Some(pane) = self.terminals.get(&id) {
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
fn show_notification(window: &mut Window, cx: &mut App, notification: Notification) {
    let notification = notification.placement(Anchor::BottomRight);
    window.defer(cx, move |window, cx| {
        window.push_notification(notification, cx)
    });
}

/// The canvas area: the window minus the title bar and, on Linux, the client-side shadow.
fn viewport(window: &Window) -> Size {
    let s = window.viewport_size();
    let pad = window_paddings(window);
    Size::new(
        f32::from(s.width - pad.left - pad.right),
        f32::from(s.height - pad.top - pad.bottom - TITLE_BAR_HEIGHT),
    )
}

/// The dot grid under the blocks; dots are 1 px on screen at any zoom.
fn paint_dots(camera: &Camera, color: Hsla, bounds: Bounds<Pixels>, window: &mut Window) {
    let grid = dot_grid(camera);
    let step = grid.spacing * camera.zoom();
    let start = camera.to_screen(grid.first);
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    let mut y = start.y;
    while y < h {
        let mut x = start.x;
        while x < w {
            let at = point(bounds.origin.x + px(x), bounds.origin.y + px(y));
            window.paint_quad(fill(Bounds::new(at, size(px(1.0), px(1.0))), color));
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
        // The focused terminal's painting sets it again this frame, if its cursor blinks.
        self.cursor_blinks = false;
        let exited: Vec<BlockId> = self
            .terminals
            .iter()
            .filter(|(_, pane)| pane.term.exited())
            .map(|(id, _)| *id)
            .collect();
        for id in exited {
            self.close(id, cx);
        }
        // Re-arm once per frame: output is coalesced to frame rate.
        for pane in self.terminals.values() {
            pane.term.rearm();
        }

        let vp = viewport(window);
        if self.pristine
            && let Some(b) = self.canvas.bounds()
        {
            let z = self.canvas.camera.zoom();
            self.canvas.camera.origin = Point::new(
                b.x + b.w / 2.0 - vp.w / 2.0 / z,
                b.y + b.h / 2.0 - vp.h / 2.0 / z,
            );
        }
        // Animations: drop the finished ones, and keep frames coming while any runs.
        let now = Instant::now();
        self.appearing
            .retain(|_, since| motion::progress(*since, now, motion::OPEN) < 1.0);
        self.ghosts
            .retain(|g| motion::progress(g.since, now, motion::CLOSE) < 1.0);
        if let Some(panel) = &self.settings {
            let t = motion::progress(panel.since, now, motion::SLIDE);
            if t < 1.0 {
                window.request_animation_frame();
            } else if panel.closing {
                self.settings = None;
            }
        }
        if !self.appearing.is_empty() || !self.ghosts.is_empty() {
            window.request_animation_frame();
        }
        let visible: Vec<_> = self.canvas.visible(vp).copied().collect();
        let mut blocks: Vec<_> = visible
            .into_iter()
            .map(|b| {
                let look = self
                    .appearing
                    .get(&b.id)
                    .map_or(motion::Look::REST, |since| {
                        motion::opening(motion::progress(*since, now, motion::OPEN))
                    });
                block::render(self, b, &self.terminals[&b.id], look, true, cx)
            })
            .collect();
        // Ghosts fade above the blocks, where they were.
        blocks.extend(self.ghosts.iter().map(|g| {
            let look = motion::closing(motion::progress(g.since, now, motion::CLOSE));
            block::render(self, g.block, &g.pane, look, false, cx)
        }));

        let entity = cx.entity();
        let focus = self.focus_handle.clone();
        let camera = self.canvas.camera;
        let dot = self.theme.ui.dot;
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
                            if e.pressed_button == Some(MouseButton::Left) {
                                ws.drag_to(e.position, cx);
                            } else {
                                ws.end_drag();
                            }
                        });
                    }
                });
                let on_up = entity.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble && e.button == MouseButton::Left {
                        on_up.update(cx, |ws, cx| ws.release(e.position, cx));
                    }
                });
            },
        )
        .absolute()
        .size_full();

        let area = div()
            .id("workspace")
            .relative()
            .flex_1()
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_background_down))
            .on_scroll_wheel(cx.listener(Self::on_background_scroll))
            .on_pinch(cx.listener(Self::on_pinch))
            .child(layer)
            .children(blocks)
            .when(self.canvas.blocks().is_empty(), |el| {
                el.child(
                    div()
                        .absolute()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(self.theme.ui.hint)
                        .child(self.hint()),
                )
            });
        let alpha = self.canvas_alpha();
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
            .map(|p| settings::render(self, p, now, vp.w, cx));
        div()
            .id("window")
            .size_full()
            .flex()
            .flex_col()
            // The settings panel slides in from beyond the right edge.
            .overflow_hidden()
            .bg(hsla(self.theme.ui.canvas, alpha))
            .child(title_bar::render(self, cx))
            .child(area)
            .children(panel)
            .children(picker)
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
        if let Some(pane) = self.focused.and_then(|id| self.terminals.get(&id)) {
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
                |window, cx| cx.new(|cx| Workspace::with(Some(quiet()), config_dir, window, cx)),
            )
            .expect("open window")
        });
        // The startup block at rest: tests that measure elements must not depend on how far its
        // pop-in got (new_terminals_pop_in_then_settle checks the animation itself).
        opened.1.update(cx, |ws, _| ws.appearing.clear());
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
            assert_eq!(ws.canvas.blocks().len(), 1);
            assert_eq!(ws.focused, ws.canvas.top());
            assert_eq!(ws.terminals.len(), 1);
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
            let b = ws.canvas.blocks()[0].rect;
            let center = ws
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
            assert_eq!(ws.canvas.blocks().len(), 2);
            assert_eq!(ws.focused, ws.canvas.top(), "the new terminal is focused");
            // The startup block already sits at the viewport center: the new one cascades.
            let [first, second] = ws.canvas.blocks() else {
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
            assert!(ws.canvas.blocks().is_empty());
            assert!(ws.terminals.is_empty());
            assert_eq!(ws.focused, None);
        });
    }

    fn first_id(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> templum_canvas::BlockId {
        ws.read_with(cx, |ws, _| ws.canvas.top().unwrap())
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
            assert_eq!(ws.canvas.blocks().len(), 2);
            let new = ws.canvas.get(ws.canvas.top().unwrap()).unwrap();
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
        ws.read_with(cx, |ws, _| assert!(ws.canvas.blocks().is_empty()));
    }

    #[gpui_kit::test]
    fn dragging_the_title_moves_the_block(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let before = ws.read_with(cx, |ws, _| ws.canvas.get(id).unwrap().rect);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(("title", id.0 as usize)).bounds().center();
            window.drag(from, point(from.x + px(100.), from.y + px(50.)), cx);
        })
        .unwrap();
        let after = ws.read_with(cx, |ws, _| ws.canvas.get(id).unwrap().rect);
        assert_eq!((after.x - before.x, after.y - before.y), (100.0, 50.0));
        assert_eq!((after.w, after.h), (before.w, before.h));
    }

    #[gpui_kit::test]
    fn dragging_the_handle_resizes_block_and_terminal(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let (before, cell) =
            ws.read_with(cx, |ws, _| (ws.canvas.get(id).unwrap().rect, ws.cell.size));
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
            let rect = ws.canvas.get(id).unwrap().rect;
            assert!(rect.w > before.w && rect.h > before.h);
            let expected = templum_canvas::chrome::grid_size(
                templum_canvas::chrome::content_rect(rect).size(),
                cell,
            );
            assert_eq!(ws.terminals[&id].term.grid_size(), expected);
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
        ws.read_with(cx, |ws, _| assert_eq!(ws.canvas.camera.origin.y, -100.0));
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
        ws.read_with(cx, |ws, _| assert_eq!(ws.canvas.camera.zoom(), 1.1));
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
            ws.terminals[&id].term.write(input.as_bytes().to_vec())
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let shown = ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.terminals[&id].term.shared(), &ws.theme.palette);
                snap.text()
            });
            if shown.contains("hello") {
                break;
            }
            assert!(Instant::now() < deadline, "no echo, screen: {shown:?}");
            std::thread::sleep(Duration::from_millis(50));
        }
        ws.read_with(cx, |ws, _| {
            let pane = &ws.terminals[&id];
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
            assert_eq!(ws.focused, Some(id));
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
        ws.read_with(cx, |ws, _| assert_eq!(ws.canvas.camera.zoom(), 1.25));
        cx.update_window(handle, |_, window, cx| {
            window.press(ZOOM_OUT, cx);
            window.press(ZOOM_RESET, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.canvas.camera.zoom(), 1.0));
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
            let vp = super::viewport(window);
            assert_eq!(ws.read(cx).canvas.visible(vp).count(), 2);
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
        ws.read_with(cx, |ws, _| ws.terminals[&id].term.write(b"x\r".to_vec()));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let shown = ws.read_with(cx, |ws, _| {
                let mut snap = Snapshot::default();
                snap.refresh(ws.terminals[&id].term.shared(), &ws.theme.palette);
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
        ws.read_with(cx, |ws, _| assert_eq!(ws.canvas.camera.zoom(), 1.1));
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
            assert_eq!(ws.terminals.len(), 2);
            for pane in ws.terminals.values() {
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
            assert_eq!(
                kit.background,
                gpui_kit::transparent_black(),
                "the glass stays clear"
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
            assert_eq!(ws.canvas.camera.zoom(), 1.0);
            assert_eq!(ws.canvas.blocks().len(), 2);
        });
    }

    #[gpui_kit::test]
    fn focus_does_not_shift_the_terminal(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let focused = window.find(("body", id.0 as usize)).bounds();
            ws.update(cx, |ws, _| ws.focused = None);
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
                ws.canvas.blocks().len(),
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
            let new = ws.canvas.top().unwrap();
            assert!(
                ws.appearing.contains_key(&new),
                "the new block is animating"
            );
        })
        .unwrap();
        std::thread::sleep(crate::motion::OPEN + std::time::Duration::from_millis(50));
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| assert!(ws.appearing.is_empty(), "settled"));
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
            assert_eq!(ws.read(cx).ghosts.len(), 1);
            // A double click where the ghost is reaches the canvas: a terminal opens there.
            let at = window.find(("ghost", n)).bounds().center();
            let at = (f32::from(at.x), f32::from(at.y));
            click(window, cx, at, 1);
            click(window, cx, at, 2);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.canvas.blocks().len(),
                1,
                "the click went through the ghost"
            );
        });
        std::thread::sleep(crate::motion::CLOSE + std::time::Duration::from_millis(50));
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| assert!(ws.ghosts.is_empty(), "faded away"));
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
                ws.canvas.get(id).unwrap().rect,
                ws.cell.size,
                ws.terminals[&id].term.grid_size(),
            )
        });
        write_config(dir.path(), "[font]\nsize = 28\n");
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                ws.canvas.get(id).unwrap().rect,
                rect,
                "the block keeps its rect"
            );
            assert!(ws.cell.size.w > cell.w);
            let now = ws.terminals[&id].term.grid_size();
            assert!(now.0 < grid.0 && now.1 < grid.1, "{now:?} vs {grid:?}");
        });
    }

    #[gpui_kit::test]
    fn window_and_theme_mode_apply_on_reload(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_with(cx, Some(dir.path().to_path_buf()));
        write_config(
            dir.path(),
            "[window]\nblur = false\n[theme]\nmode = \"dark\"\n",
        );
        reload(handle, &ws, cx);
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.canvas_alpha(), 1.0, "opaque");
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
            snap.refresh(ws.terminals[&id].term.shared(), &ws.theme.palette);
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
        let new = ws.read_with(cx, |ws, _| ws.canvas.top().unwrap());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let cwd = ws.read_with(cx, |ws, _| ws.terminals[&new].term.cwd());
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
            assert_eq!(ws.canvas.blocks().len(), 1, "the old shortcut is gone")
        });
        cx.update_window(handle, |_, window, cx| window.press("ctrl-alt-n", cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.canvas.blocks().len(), 2, "the new one works")
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
                ws.canvas.blocks().len(),
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
        ws.read_with(cx, |ws, _| assert_eq!(ws.focused, None));
    }

    #[gpui_kit::test]
    fn panning_the_canvas_keeps_the_focused_terminal(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let before = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                ws.read(cx).canvas.camera.origin
            })
            .unwrap();
        cx.update_window(handle, |_, window, cx| {
            let o = ws.read(cx).origin;
            let from = point(o.x + px(40.), o.y + px(40.));
            window.drag(from, point(from.x + px(100.), from.y + px(50.)), cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.focused, Some(id));
            let after = ws.canvas.camera.origin;
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
                let rect = ws.canvas.get(id).unwrap().rect;
                (ws.origin, ws.canvas.camera.rect_to_screen(rect))
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
        ws.read_with(cx, |ws, _| assert_eq!(ws.focused, Some(id)));
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
            assert_eq!(ws.canvas.blocks().len(), 1, "no shortcut ran");
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
            assert_eq!(ws.canvas.blocks().len(), 1, "close didn't run");
            assert!(ws.canvas.camera.zoom() > 1.0, "zoom did");
        });
        cx.update_window(handle, |_, window, cx| window.press(NEW, cx))
            .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.canvas.blocks().len(), 2));
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
                let rect = ws.canvas.get(id).unwrap().rect;
                (ws.origin, ws.canvas.camera.rect_to_screen(rect))
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
}
