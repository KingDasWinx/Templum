//! The main view: the canvas of terminal blocks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use futures::StreamExt;
use gpui_kit::component::WindowExt;
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Bounds, Context, DispatchPhase, ElementInputHandler, EntityInputHandler, FocusHandle,
    KeyBinding, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PinchEvent, Pixels, Point as PixelPoint, ScrollDelta, ScrollWheelEvent, Task, UTF16Selection,
    Window, actions, canvas, div, px, rgb,
};
use templum_canvas::{BlockId, Canvas, Gesture, Point, Rect, Size, chrome};
use templum_terminal::{KeyInput, Notify, Snapshot, SpawnOptions, Terminal};

use crate::block;
use crate::grid::CellMetrics;

actions!(templum, [NewTerminal, CloseTerminal]);

pub(crate) const DEFAULT_COLS: u16 = 80;
pub(crate) const DEFAULT_ROWS: u16 = 24;
const BACKGROUND: u32 = 0x0e0e12;
const HINT_COLOR: u32 = 0x6b6b6b;
#[cfg(target_os = "macos")]
const HINT: &str = "Double-click or press Cmd+T to open a terminal";
#[cfg(not(target_os = "macos"))]
const HINT: &str = "Double-click or press Ctrl+Shift+T to open a terminal";

pub fn bind_keys(cx: &mut App) {
    #[cfg(target_os = "macos")]
    cx.bind_keys([
        KeyBinding::new("cmd-t", NewTerminal, None),
        KeyBinding::new("cmd-w", CloseTerminal, None),
    ]);
    #[cfg(not(target_os = "macos"))]
    cx.bind_keys([
        KeyBinding::new("ctrl-shift-t", NewTerminal, None),
        KeyBinding::new("ctrl-shift-w", CloseTerminal, None),
    ]);
}

pub(crate) struct Pane {
    pub term: Terminal,
    pub snapshot: Rc<RefCell<Snapshot>>,
}

#[derive(Clone, Copy)]
enum Drag {
    Canvas(Gesture),
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
    pristine: bool,
    _wake: Task<()>,
}

impl Workspace {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self::with_command(None, window, cx)
    }

    pub fn with_command(
        command: Option<(String, Vec<String>)>,
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
            None => return,
        }
        cx.notify();
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
            alt: k.modifiers.alt,
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

    fn on_pinch(&mut self, e: &PinchEvent, _: &mut Window, cx: &mut Context<Self>) {
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

fn viewport(window: &Window) -> Size {
    let s = window.viewport_size();
    Size::new(f32::from(s.width), f32::from(s.height))
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
            .bg(rgb(BACKGROUND))
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::new_terminal))
            .on_action(cx.listener(Self::close_terminal))
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
                        .text_color(rgb(HINT_COLOR))
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
                |window, cx| cx.new(|cx| Workspace::with_command(Some(quiet()), window, cx)),
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
}
