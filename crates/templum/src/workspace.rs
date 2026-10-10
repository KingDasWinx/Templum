//! The main view: the canvas of terminal blocks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, ClipboardItem, Context, DispatchPhase, ElementInputHandler, EntityInputHandler,
    FocusHandle, KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PinchEvent, Pixels, Point as PixelPoint, ScrollDelta, ScrollWheelEvent, Task,
    UTF16Selection, Window, WindowAppearance, actions, canvas, div, px,
};
use templum_canvas::{BlockId, Canvas, Gesture, Point, Rect, Size, chrome};
use templum_terminal::{KeyInput, Notify, SelectionKind, Side, Snapshot, SpawnOptions, Terminal};

use crate::block;
use crate::config::{self, ThemeNames};
use crate::grid::{CellMetrics, hsla};
use crate::theme::{self, Entry, Theme};

actions!(
    templum,
    [
        NewTerminal,
        CloseTerminal,
        CopySelection,
        PasteClipboard,
        ZoomIn,
        ZoomOut,
        ZoomReset,
        FitAll
    ]
);

pub(crate) const DEFAULT_COLS: u16 = 80;
pub(crate) const DEFAULT_ROWS: u16 = 24;
#[cfg(target_os = "macos")]
const HINT: &str = "Double-click or press Cmd+T to open a terminal";
#[cfg(not(target_os = "macos"))]
const HINT: &str = "Double-click or press Ctrl+Shift+T to open a terminal";

pub fn bind_keys(cx: &mut App) {
    // gpui-kit's Root binds tab/shift-tab to focus navigation: give them back to the shell.
    cx.bind_keys([
        KeyBinding::new("tab", gpui_kit::NoAction {}, Some("Root")),
        KeyBinding::new("shift-tab", gpui_kit::NoAction {}, Some("Root")),
    ]);
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-t", NewTerminal, None),
        KeyBinding::new("cmd-w", CloseTerminal, None),
        KeyBinding::new("cmd-c", CopySelection, None),
        KeyBinding::new("cmd-v", PasteClipboard, None),
        KeyBinding::new("cmd-=", ZoomIn, None),
        KeyBinding::new("cmd-+", ZoomIn, None),
        KeyBinding::new("cmd--", ZoomOut, None),
        KeyBinding::new("cmd-0", ZoomReset, None),
        // Shift+1 is reported as "!" on some layouts: bind both.
        KeyBinding::new("cmd-shift-1", FitAll, None),
        KeyBinding::new("cmd-!", FitAll, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-t", NewTerminal, None),
        KeyBinding::new("ctrl-shift-w", CloseTerminal, None),
        KeyBinding::new("ctrl-shift-c", CopySelection, None),
        KeyBinding::new("ctrl-shift-v", PasteClipboard, None),
        KeyBinding::new("ctrl-=", ZoomIn, None),
        KeyBinding::new("ctrl-+", ZoomIn, None),
        KeyBinding::new("ctrl--", ZoomOut, None),
        KeyBinding::new("ctrl-0", ZoomReset, None),
        // Shift+1 is reported as "!" on some layouts: bind both.
        KeyBinding::new("ctrl-shift-1", FitAll, None),
        KeyBinding::new("ctrl-!", FitAll, None),
    ]);
}

pub(crate) struct Pane {
    pub term: Terminal,
    pub snapshot: Rc<RefCell<Snapshot>>,
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
    pub(crate) themes: Vec<Entry>,
    theme_names: ThemeNames,
    /// The system's appearance.
    dark: bool,
    /// What everything is drawn with.
    pub(crate) theme: Rc<Theme>,
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
        let theme_names = config::read(config_dir.as_deref());
        let themes = theme::load(config_dir.as_deref());
        let dark = is_dark(window.appearance());
        let theme = theme::pick(&themes, theme_names.get(dark), theme::default_name(dark));
        theme::apply_to_kit(&theme, cx);
        cx.observe_window_appearance(window, |ws, window, cx| {
            ws.set_dark(is_dark(window.appearance()), cx)
        })
        .detach();
        let mut ws = Self {
            canvas: Canvas::default(),
            terminals: HashMap::new(),
            focused: None,
            cell: CellMetrics::measure(window),
            marked: None,
            ime_cursor: None,
            on_frame: None,
            drag: None,
            focus_handle,
            origin: PixelPoint::default(),
            notify,
            zoom_acc: 0.0,
            scroll_acc: 0.0,
            command,
            pristine: true,
            themes,
            theme_names,
            dark,
            theme,
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
        let opts = SpawnOptions {
            cols,
            rows,
            cell_px: self.cell.px(),
            command: self.command.clone(),
            palette: self.theme.palette,
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
            }
            Err(err) => {
                eprintln!("templum: couldn't start a shell: {err}");
                let message = format!("Couldn't start a shell: {err}");
                // Deferred: at startup the window's notification layer doesn't exist yet.
                window.defer(cx, move |window, cx| {
                    window.push_notification(Notification::error(message), cx);
                });
            }
        }
        cx.notify();
    }

    /// Removes a block and kills its shell (dropping the Terminal shuts it down).
    pub(crate) fn close(&mut self, id: BlockId, cx: &mut Context<Self>) {
        self.canvas.remove(id);
        self.terminals.remove(&id);
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

    /// The system switched between light and dark: use that mode's theme.
    pub(crate) fn set_dark(&mut self, dark: bool, cx: &mut Context<Self>) {
        self.dark = dark;
        let theme = theme::pick(
            &self.themes,
            self.theme_names.get(dark),
            theme::default_name(dark),
        );
        self.apply(theme, cx);
    }

    pub(crate) fn focus_block(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.raise(id);
        self.focused = Some(id);
        window.focus(&self.focus_handle, cx);
        cx.notify();
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

    fn copy_selection(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
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

    fn zoom_in(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.camera.zoom_step(1, Self::center(window));
        cx.notify();
    }

    fn zoom_out(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.camera.zoom_step(-1, Self::center(window));
        cx.notify();
    }

    fn zoom_reset(&mut self, _: &ZoomReset, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.camera.zoom_reset(Self::center(window));
        cx.notify();
    }

    fn fit_all(&mut self, _: &FitAll, window: &mut Window, cx: &mut Context<Self>) {
        self.pristine = false;
        self.canvas.fit_all(viewport(window));
        cx.notify();
    }

    fn paste_clipboard(&mut self, _: &PasteClipboard, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        if let Some(pane) = self.focused.and_then(|id| self.terminals.get(&id)) {
            pane.term.paste(&text);
        }
    }

    fn end_drag(&mut self) {
        self.drag = None;
    }

    fn new_terminal(&mut self, _: &NewTerminal, window: &mut Window, cx: &mut Context<Self>) {
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

    fn close_terminal(&mut self, _: &CloseTerminal, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.focused {
            self.close(id, cx);
        }
    }

    fn on_key_down(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
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

fn viewport(window: &Window) -> Size {
    let s = window.viewport_size();
    Size::new(f32::from(s.width), f32::from(s.height))
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
        let visible: Vec<_> = self.canvas.visible(vp).copied().collect();
        let blocks: Vec<_> = visible
            .into_iter()
            .map(|b| block::render(self, b, cx))
            .collect();

        let entity = cx.entity();
        let focus = self.focus_handle.clone();
        // A full-size layer under the blocks: the IME handler, the workspace origin, and
        // window-wide drag listeners (drags keep working outside the window).
        let layer = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
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
                        on_up.update(cx, |ws, _| ws.end_drag());
                    }
                });
            },
        )
        .absolute()
        .size_full();

        div()
            .id("workspace")
            .relative()
            .size_full()
            .bg(hsla(self.theme.ui.canvas, 1.0))
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::close_terminal))
            .on_action(cx.listener(Self::copy_selection))
            .on_action(cx.listener(Self::paste_clipboard))
            .on_action(cx.listener(Self::zoom_in))
            .on_action(cx.listener(Self::zoom_out))
            .on_action(cx.listener(Self::zoom_reset))
            .on_action(cx.listener(Self::fit_all))
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
                        .child(HINT),
                )
            })
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
        cx.update(|cx| {
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
        })
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
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap();
        ws.read_with(cx, |ws, _| {
            let b = ws.canvas.blocks()[0].rect;
            let center = ws
                .canvas
                .camera
                .to_screen(templum_canvas::Point::new(b.x + b.w / 2.0, b.y + b.h / 2.0));
            assert!(
                (center.x - 300.0).abs() < 1.0 && (center.y - 200.0).abs() < 1.0,
                "startup block center on screen: {center:?}"
            );
        });
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
            click(window, cx, (40.0, 40.0), 1);
            click(window, cx, (40.0, 40.0), 2);
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
            wheel(
                window,
                cx,
                (30.0, 30.0),
                ScrollDelta::Pixels(point(px(0.), px(100.))),
                false,
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.canvas.camera.origin.y, -100.0));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            wheel(
                window,
                cx,
                (30.0, 30.0),
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
