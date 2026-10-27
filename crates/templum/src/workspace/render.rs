//! Drawing the workspace every frame.

use std::time::Instant;

use gpui_kit::base::motion::{MotionStatus, Transition, transition_with_status};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, Context, DispatchPhase, ElementInputHandler, FontWeight, Hsla, Keystroke,
    ModifiersChangedEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    TestSupportExt, Window, canvas, div, fill, point, px, size,
};
use templum_canvas::{BlockId, Camera, Gesture, Guide, Point, Rect, Size, dot_grid, dot_size};

use crate::block;
use crate::git_block;
use crate::menu;
use crate::motion;
use crate::popup;
use crate::settings;
use crate::sidebar;
use crate::theme_picker;
use crate::title_bar;
use crate::toolbar;

use super::{Drag, SAVE_DELAY, Tool, Workspace, hint_keys};

impl Workspace {
    /// Every BLINK: toggles a blinking cursor, and makes sure a steady one is shown.
    pub(crate) fn tick_blink(&mut self, cx: &mut Context<Self>) {
        let next = !self.cursor_blinks || !self.blink_on;
        if next != self.blink_on {
            self.blink_on = next;
            cx.notify();
        }
    }

    /// The empty canvas: the name, a line about it, and how to start, with the shortcuts in
    /// effect.
    pub(super) fn hint(&self) -> impl IntoElement {
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
}

impl Workspace {
    /// Grid mode, a tile held by its title past a click: its place outlined, dashed; the tile it
    /// would trade places with, tinted and saying so; and its title following the pointer. Grid
    /// mode's camera is at the origin at zoom 1: tile rects are where they are drawn.
    fn swap_overlay(&self, cx: &mut gpui_kit::Context<Self>) -> Vec<gpui_kit::AnyElement> {
        let Some(Drag::Swap { id, over, at, .. }) = self.drag else {
            return Vec::new();
        };
        if !self.held(id) {
            return Vec::new();
        }
        let ui = self.ui;
        let title = |id: BlockId| {
            self.space
                .blocks
                .get(&id)
                .map(|body| block::title(self, body, cx))
                .unwrap_or_default()
        };
        let place = |r: Rect| {
            div()
                .absolute()
                .left(px(r.x))
                .top(px(r.y))
                .w(px(r.w))
                .h(px(r.h))
                .rounded(px(10.0))
        };
        let mut layers = Vec::new();
        if let Some(r) = self.space.canvas.get(id).map(|b| b.rect) {
            layers.push(
                place(r)
                    .id("swap-from")
                    .border_2()
                    .border_dashed()
                    .border_color(ui.accent.opacity(0.7))
                    .test_support()
                    .into_any_element(),
            );
        }
        if let Some((r, name)) = over.and_then(|o| Some((self.space.canvas.get(o)?.rect, title(o))))
        {
            layers.push(
                place(r)
                    .id("swap-target")
                    .border_2()
                    .border_color(ui.accent)
                    .bg(ui.accent.opacity(0.14))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .px(px(14.0))
                            .py(px(8.0))
                            .rounded(px(8.0))
                            .bg(ui.surface)
                            .border_1()
                            .border_color(ui.accent)
                            .shadow(vec![shadow(ui.shadow)])
                            .text_size(px(13.0))
                            .text_color(ui.title)
                            .child(
                                gpui_kit::component::Icon::new(
                                    gpui_kit::assets::IconName::ArrowLeftRight,
                                )
                                .size(px(14.0))
                                .text_color(ui.accent),
                            )
                            .child(
                                div()
                                    .max_w(px(320.0))
                                    .truncate()
                                    .child(format!("Swap with {name}")),
                            ),
                    )
                    .test_support()
                    .into_any_element(),
            );
        }
        layers.push(
            div()
                .id("swap-chip")
                .absolute()
                .left(px(at.x + 14.0))
                .top(px(at.y + 14.0))
                .max_w(px(280.0))
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(8.0))
                .bg(ui.surface)
                .border_1()
                .border_color(ui.accent)
                .shadow(vec![shadow(ui.shadow)])
                .text_size(px(12.0))
                .text_color(ui.title)
                .truncate()
                .child(title(id))
                .test_support()
                .into_any_element(),
        );
        layers
    }
}

/// A floating card's drop shadow.
fn shadow(color: Hsla) -> gpui_kit::BoxShadow {
    gpui_kit::BoxShadow {
        color,
        offset: point(px(0.0), px(8.0)),
        blur_radius: px(20.0),
        spread_radius: px(0.0),
        inset: false,
    }
}

/// The dot grid under the blocks: round dots centered on the world lattice. One layer: the
/// dots don't overlap and share a draw order, so GPUI skips the per-primitive bounds lookup.
fn paint_dots(camera: &Camera, color: Hsla, bounds: Bounds<Pixels>, window: &mut Window) {
    let grid = dot_grid(camera);
    let step = grid.spacing * camera.zoom();
    let d = dot_size(camera.zoom());
    let start = camera.to_screen(grid.first);
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    window.paint_layer(bounds, |window| {
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
    });
}

/// The snap guides: 1 px lines in the accent color, world units to screen.
fn paint_guides(
    camera: &Camera,
    color: Hsla,
    bounds: Bounds<Pixels>,
    guides: &[Guide],
    window: &mut Window,
) {
    for guide in guides {
        let (a, b) = match *guide {
            Guide::Vertical { x, y0, y1 } => (Point::new(x, y0), Point::new(x, y1)),
            Guide::Horizontal { y, x0, x1 } => (Point::new(x0, y), Point::new(x1, y)),
        };
        let (a, b) = (camera.to_screen(a), camera.to_screen(b));
        let rect = Bounds::new(
            point(bounds.origin.x + px(a.x), bounds.origin.y + px(a.y)),
            size(px((b.x - a.x).max(1.0)), px((b.y - a.y).max(1.0))),
        );
        window.paint_quad(fill(rect, color));
    }
}

impl Workspace {
    /// The canvas of the workspace shown: its blocks in view, its wires and drawings, the layers
    /// the tools put over them, and the toolbar.
    fn canvas_area(
        &mut self,
        vp: Size,
        now: Instant,
        tiled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let visible: Vec<_> = self
            .space
            .canvas
            .visible_from(&self.space.view, vp)
            .filter(|b| self.space.shown(b.id))
            .copied()
            .collect();
        // Tiles trading places slide there.
        if let Some(grid) = &mut self.space.grid {
            grid.slides
                .retain(|_, (_, since)| motion::progress(*since, now, motion::TILE) < 1.0);
            if !grid.slides.is_empty() {
                window.request_animation_frame();
            }
        }
        let gates = block::Gates::of(self);
        let mut blocks: Vec<_> = visible
            .into_iter()
            .map(|mut b| {
                let look = self
                    .space
                    .appearing
                    .get(&b.id)
                    .map_or(motion::Look::REST, |since| {
                        motion::opening(motion::progress(*since, now, motion::OPEN))
                    });
                // A tile held to trade places is picked up.
                let look = if self.held(b.id) { motion::HELD } else { look };
                if let Some((was, since)) =
                    self.space.grid.as_ref().and_then(|g| g.slides.get(&b.id))
                {
                    let t = motion::slide(motion::progress(*since, now, motion::TILE));
                    b.rect = super::tiling::between(*was, b.rect, t);
                }
                let body = &self.space.blocks[&b.id];
                block::render(self, b, body, look, true, &gates, window, cx)
            })
            .collect();
        // Ghosts fade above the blocks, where they were.
        blocks.extend(self.space.ghosts.iter().map(|g| {
            let look = motion::closing(motion::progress(g.since, now, motion::CLOSE));
            block::render(self, g.block, &g.body, look, false, &gates, window, cx)
        }));

        let entity = cx.entity();
        let focus = self.focus_handle.clone();
        let camera = self.space.view;
        let dot = self.ui.dot;
        let guides = self.space.guides.clone();
        let accent = self.ui.accent;
        // The wires, under the blocks (none in grid mode).
        let wires = self.wire_curves();
        // A dot on each wire along which a task is handed over: still when motion is reduced, else
        // drawn again about 30 times a second while one is in view (never at the display's rate).
        let still = cx.reduce_motion();
        let dots = self.wire_dots(now, still);
        let zoom = camera.zoom();
        let in_view = Rect::new(camera.origin.x, camera.origin.y, vp.w / zoom, vp.h / zoom);
        // The wires' labels: on the wires, under the blocks.
        let labels: Vec<_> = crate::wires::label_scale(self.space.rest, zoom)
            .map(|s| {
                self.wire_labels()
                    .into_iter()
                    .filter(|(at, ..)| in_view.contains(*at))
                    .enumerate()
                    .map(|(n, (at, text, color))| {
                        crate::wires::label(n, text, camera.to_screen(at), s, color, self.ui)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !still && dots.iter().any(|p| in_view.contains(*p)) {
            self._dot_frame = cx.spawn(async move |ws, cx| {
                cx.background_executor()
                    .timer(crate::wires::DOT_FRAME)
                    .await;
                ws.update(cx, |_, cx| cx.notify()).ok();
            });
        }
        // A full-size layer under the blocks: the IME handler, the workspace origin, and
        // window-wide drag listeners (drags keep working outside the window).
        let layer = canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                // Grid mode shows no canvas.
                if !tiled {
                    paint_dots(&camera, dot, bounds, window);
                    crate::wires::paint(&wires, &dots, accent, &camera, bounds, window);
                    paint_guides(&camera, accent, bounds, &guides, window);
                }
                window.handle_input(&focus, ElementInputHandler::new(bounds, entity.clone()), cx);
                entity.update(cx, |ws, _| ws.origin = bounds.origin);
                let on_move = entity.clone();
                window.on_mouse_event(move |e: &MouseMoveEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        on_move.update(cx, |ws, cx| {
                            // Alt let go while the window wasn't listening (another window had
                            // the keyboard): the next move puts the hand right.
                            if e.modifiers.alt != ws.hand {
                                cx.notify();
                            }
                            // Any button: the middle one pans too.
                            if e.pressed_button.is_some() {
                                ws.drag_to(e.position, e.modifiers.shift, cx);
                            } else {
                                // A release this window never heard still keeps the popup's size,
                                // and copies a selection.
                                if !ws.release_git_resize(window, cx) {
                                    ws.copy_on_select(e.position, cx);
                                    ws.end_drag();
                                }
                                ws.hover_program(e.position, &e.modifiers);
                                if ws.pointer.replace(e.position) != Some(e.position) {
                                    ws.follow_pointer(window, cx);
                                }
                            }
                        });
                    }
                });
                let on_up = entity.clone();
                window.on_mouse_event(move |e: &MouseUpEvent, phase, window, cx| {
                    if phase == DispatchPhase::Bubble {
                        on_up.update(cx, |ws, cx| {
                            if ws.release_program(e) || ws.release_git_resize(window, cx) {
                                return;
                            }
                            match e.button {
                                MouseButton::Left => ws.release(e.position, window, cx),
                                MouseButton::Middle => ws.end_drag(),
                                _ => {}
                            }
                        });
                    }
                });
            },
        )
        .absolute()
        .size_full();

        self.hand = window.modifiers().alt;
        let hand = (self.hand || self.tool == Tool::Hand) && !tiled;
        let panning = matches!(self.drag, Some(Drag::Canvas(Gesture::Pan { .. })));
        // The selection rectangle being dragged, on screen.
        let marquee = match &self.drag {
            Some(Drag::Marquee { from, to, .. }) => {
                Some(self.space.view.rect_to_screen(Rect::corners(*from, *to)))
            }
            _ => None,
        };
        // The box around two or more selected blocks: a hairline, no events (a press in it is the
        // workspace's, `on_background_down`).
        let selection = self
            .selection_box()
            .filter(|_| !tiled)
            .map(|b| self.space.view.rect_to_screen(b));
        let swapping = self.swap_overlay(cx);
        div()
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
            .children(labels)
            .children(blocks)
            // The Select tool's wheel: a layer over the blocks sends it to the canvas before a
            // block that isn't selected can scroll (`route_wheel`).
            .when(self.tool == Tool::Select && !tiled, |el| {
                el.child(
                    div()
                        .id("wheel")
                        .absolute()
                        .size_full()
                        .on_scroll_wheel(cx.listener(Self::route_wheel)),
                )
            })
            // The drawings, above the blocks; with a drawing tool on, a layer over them takes the
            // pointer.
            .when(!tiled, |el| el.child(self.ink_layer(cx)))
            .children(marquee.map(|r| {
                div()
                    .absolute()
                    .left(px(r.x))
                    .top(px(r.y))
                    .w(px(r.w))
                    .h(px(r.h))
                    .border_1()
                    .border_color(self.ui.accent)
                    .bg(self.ui.accent.opacity(0.08))
            }))
            .children(selection.map(|r| {
                div()
                    .absolute()
                    .left(px(r.x))
                    .top(px(r.y))
                    .w(px(r.w))
                    .h(px(r.h))
                    .border_1()
                    .border_color(self.ui.accent)
            }))
            .children(swapping)
            .when(self.tool.draws() && !tiled, |el| {
                el.child(
                    div()
                        .id("ink-layer")
                        .absolute()
                        .size_full()
                        .cursor_crosshair()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, e: &MouseDownEvent, _, cx| {
                                ws.begin_ink(e);
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .on_scroll_wheel(cx.listener(Self::on_background_scroll))
                        .on_pinch(cx.listener(Self::on_pinch))
                        .test_support(),
                )
            })
            // The Wire tool: a layer over the blocks takes the pointer, as a drawing tool's does.
            .when(self.tool == Tool::Wire && !tiled, |el| {
                el.child(
                    div()
                        .id("wire-layer")
                        .absolute()
                        .size_full()
                        .cursor_crosshair()
                        .occlude()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|ws, e: &MouseDownEvent, window, cx| {
                                window.focus(&ws.focus_handle, cx);
                                ws.begin_wire(e);
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        // Only over the canvas: not over the toolbar, the sidebar or outside the
                        // window, where the block under the pointer can't be seen.
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|ws, e: &MouseUpEvent, _, cx| ws.end_wire(e, cx)),
                        )
                        .on_scroll_wheel(cx.listener(Self::on_background_scroll))
                        .on_pinch(cx.listener(Self::on_pinch))
                        .test_support(),
                )
            })
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
            // A remote workspace is empty until home's first state; a stopped one has its card.
            .children(crate::mirror::stopped_card(self, cx))
            .when(
                self.space.canvas.blocks().is_empty() && !self.space.is_remote(),
                |el| {
                    el.child(
                        div()
                            .absolute()
                            .size_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(self.hint()),
                    )
                },
            )
            // Not live: the blocks keep their last frames under a veil.
            .children(crate::mirror::veil(self))
            .when(!tiled, |el| el.child(toolbar::render(self, cx)))
            .children(crate::mirror::banner(self, cx))
            .test_support()
            .into_any_element()
    }
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
        if self.keep_dead_starts() {
            super::persist::say_unstarted(window, cx);
        }
        let exited: Vec<(BlockId, Option<String>)> = self
            .space
            .terminals()
            .filter(|(_, pane)| pane.term.exited() && !pane.died_starting())
            .map(|(id, pane)| (id, pane.term.killed().then(|| pane.term.title())))
            .collect();
        // Nothing left to lose: no question.
        for (id, killed) in exited {
            // Its block closes: say why it went (docs/specs/shells.md#a-scope-for-each-terminal).
            if let Some(title) = killed {
                let message = format!(
                    "The terminal “{title}” was stopped by the system, most likely for \
                     using too much memory"
                );
                eprintln!("templum: {message}");
                super::show_notification(
                    window,
                    cx,
                    gpui_kit::component::notification::Notification::warning(message),
                );
            }
            self.remove_block(id, window, cx);
        }
        self.hear(window, cx);
        self.watch_agents(window.is_window_active(), cx.background_executor().now());
        // A tool call whose block is gone is withdrawn (docs/specs/connections.md).
        self.drop_orphans(cx);

        let vp = self.viewport(window);
        self.api.viewport = vp;
        // Grid mode's tiles fill the area as it is now: the window or the sidebar may have changed.
        if let Some(grid) = &mut self.space.grid {
            grid.area = super::tiling::grid_area(vp);
            self.retile();
        }
        let tiled = self.space.grid.is_some();
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
        if self.drag_edge(vp, now) {
            window.request_animation_frame();
        }
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
        if let Some(welcome) = &self.welcome {
            let t = motion::progress(welcome.since, now, motion::SLIDE);
            if t < 1.0 {
                window.request_animation_frame();
            } else if welcome.closing {
                self.welcome = None;
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
        // The level of detail changes only at rest (docs/specs/level-of-detail.md, section 1).
        if self.space.flight.is_none() {
            self.space.rest = self.space.view.zoom();
        }
        // The remote workspaces' terminals to stream follow what is in view.
        let now = Instant::now();
        let held = self.expire_holds(now);
        if let Some(due) = [self.stream_terminals(now), held]
            .into_iter()
            .flatten()
            .min()
        {
            self.stream_at(due, window, cx);
        }
        // The Home in the canvas's place: no block of the workspace under it is laid out or drawn.
        let area = match &self.screen {
            Some(screen) => crate::home::render(self, screen, window, cx),
            None => self.canvas_area(vp, now, tiled, window, cx),
        };
        let picker = self
            .picker
            .as_ref()
            .map(|p| theme_picker::render(self, p, cx));
        let usage = self.usage_open.then(|| crate::usage::popover(self, cx));
        // The popups (crate::popup): over the canvas's bottom-left corner, beside the sidebar, at
        // the size the person dragged them to; a small window gets a smaller one. Only one is open.
        self.git_room = Size::new(vp.w - 2.0 * popup::GAP, vp.h - 2.0 * popup::GAP);
        let Size { w, h } = self.git_popup_size();
        let git = self
            .space
            .git()
            .filter(|_| self.git_open)
            .map(|g| git_block::popup(self, g, w, h, cx));
        self.inbox_keeps_keys(window, cx);
        let inbox = self
            .inbox_open
            .then(|| crate::inbox::popup(self, w, h, window, cx));
        // The rows are made once a frame: the selection is pinned on them, and the popup draws them.
        let agents = self.agents_open.then(|| {
            let all = self.agent_rows();
            let listed = self.listed_in(&all);
            self.pin_agent(&all, &listed);
            crate::agents_view::popup(self, w, h, &all, listed, window, cx)
        });
        self.prepare_automation_form(window, cx);
        self.follow_automation_draft(window, cx);
        let automations = self
            .automations_open
            .then(|| crate::automations_view::popup(self, w, h, cx));
        let popup = git
            .or(inbox)
            .or(agents)
            .or(automations)
            .map(|p| popup::slot(self, w, h, p, cx));
        // Window chrome over the canvas's bottom center, above the toolbar: the zoom never scales it.
        let open = self.templum_agent.open;
        // Rows that come while it is closed show at once when it opens.
        if !open {
            self.templum_agent.drawn.set(false);
        }
        let full = open && self.templum_agent.full;
        let waiting = self.templum_agent.approvals.len();
        // The Templum Agent works on the canvas: not on the Home, where a notification says what
        // it asks (`tell_approval`).
        let home = self.home_up();
        let approval = self
            .templum_agent
            .approvals
            .first()
            .filter(|_| !home)
            .map(|a| {
                let width = if open {
                    crate::chat::panel::approval_width(vp.w, full)
                } else {
                    640.0_f32.min(vp.w - 32.0)
                };
                crate::chat::approval::render(self, a, (1, waiting), width, window, cx)
            });
        let pill = (!home && (!tiled || open))
            .then(|| crate::chat::pill::render(self, vp.w, open, window, cx))
            .flatten();
        let agent = match (open, pill) {
            (true, Some(pill)) => {
                crate::chat::panel::render(self, (vp.w, vp.h), pill, approval, window, cx)
            }
            (_, pill) if pill.is_some() || approval.is_some() => Some(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(8.0))
                    .children(approval)
                    .children(pill)
                    .into_any_element(),
            ),
            _ => None,
        }
        .map(|pill| {
            let left = if self.sidebar { sidebar::WIDTH } else { 0.0 };
            // Full screen fills the canvas to its inset; else the card sits above the toolbar.
            let bottom = if full { 16.0 } else { toolbar::above(self) };
            div()
                .absolute()
                .left(px(left))
                .right_0()
                .bottom(px(bottom))
                .flex()
                .justify_center()
                .child(pill)
        });
        let welcome = self
            .welcome
            .as_ref()
            .map(|w| crate::welcome::render(self, w, now, window, cx));
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
                    .when(self.sidebar, |el| {
                        el.child(sidebar::render(self, window, cx))
                    })
                    .child(area),
            )
            // The popups over the Templum agent.
            .children(agent)
            .children(popup)
            // Under what the title bar opens: the settings, the theme picker, the usage popover.
            .children(welcome)
            .children(panel)
            .children(picker)
            .children(usage)
            .children(self.menu.as_ref().map(|m| menu::render(m, window)))
    }
}
