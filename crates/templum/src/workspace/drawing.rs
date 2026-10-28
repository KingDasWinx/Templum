//! The drawing tools: ink, the eraser, undo and redo, and the layer that paints the drawings.

use gpui_kit::prelude::*;
use gpui_kit::{Context, MouseDownEvent, Window, canvas};
use templum_canvas::{Color, Drawing, HIT_PX, Point, ShapeKind};

use crate::ink;

use super::remote::Deed;
use super::{Drag, INK_UNDO, Tool, Workspace};

impl Workspace {
    /// The toolbar's tool; the keyboard goes back to the canvas. A wire being drawn is given up.
    pub(crate) fn set_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        let deed = match tool {
            Tool::Select | Tool::Hand => None,
            Tool::Wire => Some(Deed::Wire),
            _ => Some(Deed::Draw),
        };
        if deed.is_some_and(|deed| self.why_not(deed).is_some()) {
            return;
        }
        if matches!(self.drag, Some(Drag::Wire { .. })) {
            self.drag = None;
        }
        self.tool = tool;
        self.menu = None;
        self.shapes_open = false;
        self.space.selected_ink = None;
        self.space.selected_wire = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    pub(crate) fn set_ink_color(&mut self, color: Color, cx: &mut Context<Self>) {
        self.ink_color = color;
        cx.notify();
    }

    pub(crate) fn set_ink_width(&mut self, width: ink::Width, cx: &mut Context<Self>) {
        self.ink_width = width;
        cx.notify();
    }

    /// The shapes pill above the toolbar, or closes it.
    pub(crate) fn toggle_shapes(&mut self, cx: &mut Context<Self>) {
        if self.why_not(Deed::Draw).is_some() {
            return;
        }
        self.shapes_open = !self.shapes_open;
        cx.notify();
    }

    pub(crate) fn pick_shape(
        &mut self,
        kind: ShapeKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.last_shape = Some(kind);
        self.set_tool(Tool::Shape(kind), window, cx);
    }

    /// The topmost drawing under a world point, within the eraser's reach. None on a mirrored
    /// workspace: home's ink is painted there, never picked.
    pub(super) fn ink_at(&self, world: Point) -> Option<usize> {
        if self.on_remote() {
            return None;
        }
        let tolerance = HIT_PX / self.space.canvas.camera.zoom();
        self.space.ink.iter().rposition(|d| d.hit(world, tolerance))
    }

    /// Before a change to the drawings: the current ones go on the undo stack.
    pub(super) fn remember_ink(&mut self) {
        self.space.ink_undo.push(self.space.ink.clone());
        if self.space.ink_undo.len() > INK_UNDO {
            self.space.ink_undo.remove(0);
        }
        self.space.ink_redo.clear();
    }

    pub(crate) fn delete_ink(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.space.ink.len() {
            return;
        }
        self.remember_ink();
        self.space.ink.remove(index);
        self.space.selected_ink = None;
        cx.notify();
    }

    pub(super) fn undo_ink(&mut self, cx: &mut Context<Self>) {
        if let Some(previous) = self.space.ink_undo.pop() {
            let current = std::mem::replace(&mut self.space.ink, previous);
            self.space.ink_redo.push(current);
            self.space.selected_ink = None;
            cx.notify();
        }
    }

    pub(super) fn redo_ink(&mut self, cx: &mut Context<Self>) {
        if let Some(next) = self.space.ink_redo.pop() {
            let current = std::mem::replace(&mut self.space.ink, next);
            self.space.ink_undo.push(current);
            self.space.selected_ink = None;
            cx.notify();
        }
    }

    /// Removes every drawing the eraser passes over from `from` to `to` (world points); true when
    /// any was. The first removal of a drag (`!erased_before`) takes an undo step.
    pub(super) fn erase_along(&mut self, from: Point, to: Point, erased_before: bool) -> bool {
        let tolerance = HIT_PX / self.space.canvas.camera.zoom();
        // Points along the way no farther apart than the eraser's reach: one move can cross a
        // stroke without landing near it.
        let len = (to.x - from.x).hypot(to.y - from.y);
        let steps = (len / tolerance).ceil().max(1.0) as usize;
        let along = |i: usize| {
            let t = i as f32 / steps as f32;
            Point::new(from.x + (to.x - from.x) * t, from.y + (to.y - from.y) * t)
        };
        let hit = |d: &Drawing| (0..=steps).any(|i| d.hit(along(i), tolerance));
        if !self.space.ink.iter().any(hit) {
            return false;
        }
        if !erased_before {
            self.remember_ink();
        }
        self.space.ink.retain(|d| !hit(d));
        true
    }

    /// A press on the ink layer: starts the tool's drawing at the pointer.
    pub(crate) fn begin_ink(&mut self, e: &MouseDownEvent) {
        // A mirrored workspace is only looked at and moved around in, for now.
        if self.on_remote() {
            return;
        }
        self.space.pristine = false;
        self.press = None;
        let p = self.space.canvas.camera.to_world(self.local(e.position));
        let drawing = match self.tool {
            Tool::Pen => Drawing::Stroke {
                points: vec![p],
                width: self.ink_width.units(),
                color: self.ink_color,
                highlight: false,
            },
            Tool::Highlighter => Drawing::Stroke {
                points: vec![p],
                width: ink::HIGHLIGHTER_WIDTH,
                color: self.ink_color,
                highlight: true,
            },
            Tool::Shape(kind) => Drawing::Shape {
                kind,
                from: p,
                to: p,
                width: self.ink_width.units(),
                color: self.ink_color,
            },
            Tool::Eraser => {
                let erased = self.erase_along(p, p, false);
                self.drag = Some(Drag::Erase { erased, last: p });
                return;
            }
            Tool::Select | Tool::Hand | Tool::Wire => return,
        };
        self.drag = Some(Drag::Ink(drawing));
    }

    /// The drawings, painted above the blocks. No listeners: the pointer goes through it. It
    /// reads the workspace when it paints: cloning every stroke into the closure each frame is
    /// what it avoids.
    pub(super) fn ink_layer(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let entity = cx.entity();
        let cache = self.ink_cache.clone();
        canvas(
            |_, _, _| {},
            move |bounds, _, window, cx| {
                let ws = entity.read(cx);
                let live = match &ws.drag {
                    Some(Drag::Ink(d)) => Some(d),
                    _ => None,
                };
                ink::paint(
                    &ws.space.ink,
                    live,
                    ws.space.selected_ink,
                    &ws.space.view,
                    &ws.theme,
                    ws.ui,
                    bounds,
                    &mut cache.borrow_mut(),
                    window,
                );
            },
        )
        .absolute()
        .size_full()
    }
}
