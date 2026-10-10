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
    EntityInputHandler, FocusHandle, Hsla, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PinchEvent, Pixels, Point as PixelPoint, ScrollDelta,
    ScrollWheelEvent, Task, UTF16Selection, Window, WindowAppearance, canvas, div, fill, point, px,
    size,
};
use templum_canvas::{
    Block, BlockId, Camera, Canvas, Gesture, Point, Rect, Size, chrome, dot_grid,
};
use templum_terminal::{KeyInput, Notify, SelectionKind, Side, Snapshot, SpawnOptions, Terminal};

use crate::block;
use crate::config::{self, Config, ThemeConfig, WorkingDirectory};
use crate::grid::{CellMetrics, hsla};
use crate::keys::{self, Command, Keymap};
use crate::motion;
use crate::theme::{self, Entry, Theme};
use crate::theme_picker::{self, Picker};
use crate::title_bar;

pub(crate) const DEFAULT_COLS: u16 = 80;
pub(crate) const DEFAULT_ROWS: u16 = 24;
/// A press that moves less than this (screen pixels) before release is a click, not a pan.
const CLICK_SLOP: f32 = 4.0;
/// Cursor blink half-period.
const BLINK: Duration = Duration::from_millis(500);

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
    config_dir: Option<PathBuf>,
    pub(crate) themes: Vec<Entry>,
    /// The system's appearance.
    dark: bool,
    /// What everything is drawn with.
    pub(crate) theme: Rc<Theme>,
    /// The window shows the blurred desktop: the canvas is drawn at window.opacity.
    translucent: bool,
    /// The active config.toml: the last valid one.
    pub(crate) config: Config,
    /// Its shortcuts.
    keymap: Keymap,
    /// What config.toml looked like at the last read, for polling.
    config_stamp: config::Stamp,
    /// The last read failed: the next good one says so.
    config_failed: bool,
    /// The system's appearance. `dark` is what's in effect, since config.toml may pin a mode.
    system_dark: bool,
    _poll: Task<()>,
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
            self.config_stamp = stamp;
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
        if self.dark {
            self.config.theme.dark = name.clone();
        } else {
            self.config.theme.light = name.clone();
        }
        let saved = match &self.config_dir {
            Some(dir) => config::save_theme(dir, self.dark, &name),
            None => Err(config::SaveError::NoDir),
        };
        match saved {
            Ok(()) => self.picker = None,
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
        }
    }

    /// Opens config.toml in the system's editor, writing the commented template first if needed.
    fn open_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
            // While the picker is open, typing and shortcuts don't reach the terminal.
            cx.stop_propagation();
            return;
        }
        if let Some(command) = command {
            self.run(command, window, cx);
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
        div()
            .id("window")
            .size_full()
            .flex()
            .flex_col()
            .bg(hsla(self.theme.ui.canvas, alpha))
            .child(title_bar::render(self, cx))
            .child(area)
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
        AnyWindowHandle, AppContext as _, Bounds, Entity, Modifiers, MouseButton, MouseDownEvent,
        MouseUpEvent, PlatformInput, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
        Window, WindowBounds, WindowOptions, point, px, size,
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
}
