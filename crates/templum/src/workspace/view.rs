//! The camera: the viewport, zooming, panning, and gliding to a block.

use std::time::Instant;

use gpui_kit::component::{TITLE_BAR_HEIGHT, window_paddings};
use gpui_kit::{Context, Window};
use templum_canvas::{BlockId, Camera, CameraFlight, Point, Rect, Size};

use crate::sidebar;
use crate::toolbar;

use super::Workspace;

impl Workspace {
    /// Shows a block: its workspace, then the block focused, the camera moved just enough to show
    /// it whole above the toolbar.
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
        // The block gets the keyboard: a welcome over it makes way.
        self.close_welcome(window, cx);
        self.switch_to(to, window, cx);
        let Some(rect) = self.space.canvas.get(block).map(|b| b.rect) else {
            return;
        };
        // In grid mode a block on the canvas comes into the grid to be seen.
        if !self.space.shown(block) {
            self.join_grid(block, self.space.focused);
        }
        self.focus_block(block, window, cx);
        self.show(rect, window, cx);
    }

    /// The camera glides just enough to show `rect` (world units) whole above the toolbar.
    pub(super) fn show(&mut self, rect: Rect, window: &Window, cx: &mut Context<Self>) {
        let vp = self.viewport(window);
        let to = self
            .space
            .canvas
            .camera
            .reveal(rect, Size::new(vp.w, vp.h - toolbar::SPACE));
        self.fly(to, self.center(window), cx);
    }

    /// Where a block of `size` the person opens goes: the view's middle, or beside the blocks
    /// there (`Space::room_around`); the camera glides to show it whole.
    pub(super) fn room_in_view(
        &mut self,
        size: Size,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Rect {
        let center = self.space.camera().to_world(self.center(window));
        let rect = self.space.room_around(center, size);
        self.show(rect, window, cx);
        rect
    }

    /// A double-click on a block's title: the view glides to show the block centered, as large as
    /// fits above the toolbar; on the block it framed, back to where the view was.
    pub(crate) fn frame_block(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rect) = self.space.canvas.get(id).map(|b| b.rect) else {
            return;
        };
        if self.space.grid.is_some() {
            return;
        }
        let vp = self.viewport(window);
        let camera = self.space.canvas.camera;
        let mut framed = camera;
        framed.fit(rect, Size::new(vp.w, vp.h - toolbar::SPACE));
        let to = match self.space.framed.take() {
            Some((was, back)) if was == id && camera == framed => back,
            _ => {
                self.space.framed = Some((id, camera));
                framed
            }
        };
        self.fly(to, self.center(window), cx);
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

    pub(crate) fn center(&self, window: &Window) -> Point {
        let vp = self.viewport(window);
        Point::new(vp.w / 2.0, vp.h / 2.0)
    }

    /// Moves the camera to `to`: the view glides there from where it is drawn, pivoting on the
    /// screen point `pivot`.
    pub(super) fn fly(&mut self, to: Camera, pivot: Point, cx: &mut Context<Self>) {
        // Grid mode's camera stays put: zooming, framing and showing a block don't move it.
        if self.space.grid.is_some() {
            return;
        }
        self.space.pristine = false;
        if to == self.space.canvas.camera {
            return;
        }
        self.api.moved_at = Some(Instant::now());
        // Mid-flight, from where it is drawn; otherwise the camera is what was last drawn (or
        // moved since, by a drag), at rest there until now, on a canvas not drawn yet too.
        let from = if self.space.flight.is_some() {
            self.space.view
        } else {
            self.space.rest = self.space.canvas.camera.zoom();
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
        self.api.moved_at = Some(Instant::now());
        self.space.canvas.camera.pan_by(screen_delta);
        if let Some((flight, _)) = &mut self.space.flight {
            flight.pan_by(screen_delta);
        }
    }

    pub(super) fn zoom_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_to(1, self.center(window), cx);
    }

    pub(super) fn zoom_out(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_to(-1, self.center(window), cx);
    }

    pub(crate) fn zoom_reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let center = self.center(window);
        let mut to = self.space.canvas.camera;
        to.zoom_reset(center);
        self.fly(to, center, cx);
    }

    pub(super) fn fit_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let vp = self.viewport(window);
        let mut to = self.space.canvas.camera;
        match self.space.canvas.bounds() {
            Some(bounds) => to.fit(bounds, vp),
            None => to.zoom_reset(Point::new(vp.w / 2.0, vp.h / 2.0)),
        }
        self.fly(to, self.center(window), cx);
    }
}
