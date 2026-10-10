//! Maps world space to the screen.

use crate::{Point, Rect, Size};

/// Fixed zoom steps keep the glyph cache bounded: text is rasterized once per step.
pub const ZOOM_STEPS: [f32; 14] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];
const ZOOM_1: usize = 6;
/// Screen margin kept around the blocks by [`Camera::fit`].
pub const FIT_MARGIN: f32 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// World point shown at the viewport's top-left corner.
    pub origin: Point,
    zoom_idx: usize,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            origin: Point::default(),
            zoom_idx: ZOOM_1,
        }
    }
}

impl Camera {
    pub fn zoom(&self) -> f32 {
        ZOOM_STEPS[self.zoom_idx]
    }

    pub fn zoom_index(&self) -> usize {
        self.zoom_idx
    }

    /// A camera at `origin` with the zoom step nearest to `zoom` (a saved canvas). A zoom that
    /// isn't a finite number gives 100%.
    pub fn at(origin: Point, zoom: f32) -> Camera {
        let zoom_idx = if zoom.is_finite() {
            ZOOM_STEPS
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| (*a - zoom).abs().total_cmp(&(*b - zoom).abs()))
                .map_or(ZOOM_1, |(i, _)| i)
        } else {
            ZOOM_1
        };
        Camera { origin, zoom_idx }
    }

    pub fn to_world(&self, screen: Point) -> Point {
        Point::new(
            screen.x / self.zoom() + self.origin.x,
            screen.y / self.zoom() + self.origin.y,
        )
    }

    pub fn to_screen(&self, world: Point) -> Point {
        Point::new(
            (world.x - self.origin.x) * self.zoom(),
            (world.y - self.origin.y) * self.zoom(),
        )
    }

    pub fn rect_to_screen(&self, r: Rect) -> Rect {
        let p = self.to_screen(r.origin());
        Rect::new(p.x, p.y, r.w * self.zoom(), r.h * self.zoom())
    }

    /// Dragging the canvas by `screen_delta` moves the world along with the pointer.
    pub fn pan_by(&mut self, screen_delta: Point) {
        self.origin.x -= screen_delta.x / self.zoom();
        self.origin.y -= screen_delta.y / self.zoom();
    }

    /// Moves `delta` zoom steps (clamped), keeping the world point under `anchor` still.
    pub fn zoom_step(&mut self, delta: i32, anchor: Point) {
        let idx = (self.zoom_idx as i32 + delta).clamp(0, ZOOM_STEPS.len() as i32 - 1);
        self.set_zoom(idx as usize, anchor);
    }

    /// Back to 100%, keeping the world point under `anchor` still.
    pub fn zoom_reset(&mut self, anchor: Point) {
        self.set_zoom(ZOOM_1, anchor);
    }

    /// Frames `bounds`: the largest step showing it with [`FIT_MARGIN`] on every side (the
    /// smallest step if none does), centered in the viewport.
    pub fn fit(&mut self, bounds: Rect, viewport: Size) {
        let avail_w = (viewport.w - 2.0 * FIT_MARGIN).max(1.0);
        let avail_h = (viewport.h - 2.0 * FIT_MARGIN).max(1.0);
        self.zoom_idx = ZOOM_STEPS
            .iter()
            .rposition(|&z| bounds.w * z <= avail_w && bounds.h * z <= avail_h)
            .unwrap_or(0);
        let z = self.zoom();
        self.origin = Point::new(
            bounds.x + bounds.w / 2.0 - viewport.w / 2.0 / z,
            bounds.y + bounds.h / 2.0 - viewport.h / 2.0 / z,
        );
    }

    fn set_zoom(&mut self, idx: usize, anchor: Point) {
        let world = self.to_world(anchor);
        self.zoom_idx = idx;
        self.origin = Point::new(
            world.x - anchor.x / self.zoom(),
            world.y - anchor.y / self.zoom(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    #[test]
    fn screen_world_round_trip_at_any_zoom() {
        let mut cam = Camera {
            origin: Point::new(100.0, -50.0),
            ..Camera::default()
        };
        for step in [-6, 3, 4] {
            cam.zoom_step(step, Point::new(0.0, 0.0));
            let p = Point::new(321.0, 123.0);
            assert!(close(cam.to_screen(cam.to_world(p)), p));
        }
    }

    #[test]
    fn zoom_keeps_anchor_still_and_clamps() {
        let mut cam = Camera {
            origin: Point::new(100.0, -50.0),
            ..Camera::default()
        };
        let anchor = Point::new(300.0, 200.0);
        let before = cam.to_world(anchor);
        cam.zoom_step(3, anchor);
        assert_eq!(cam.zoom(), 1.5);
        assert!(close(cam.to_world(anchor), before));
        cam.zoom_step(-100, anchor);
        assert_eq!(cam.zoom(), ZOOM_STEPS[0]);
        cam.zoom_step(100, anchor);
        assert_eq!(cam.zoom(), ZOOM_STEPS[ZOOM_STEPS.len() - 1]);
        cam.zoom_reset(anchor);
        assert_eq!(cam.zoom(), 1.0);
        assert!(close(cam.to_world(anchor), before));
    }

    #[test]
    fn at_snaps_a_saved_zoom_to_the_nearest_step() {
        let origin = Point::new(3.0, -4.0);
        assert_eq!(Camera::at(origin, 1.0).zoom(), 1.0);
        assert_eq!(Camera::at(origin, 1.04).zoom(), 1.0);
        assert_eq!(Camera::at(origin, 1.2).zoom(), 1.25);
        assert_eq!(Camera::at(origin, 10.0).zoom(), 3.0);
        assert_eq!(Camera::at(origin, 0.0).zoom(), 0.25);
        assert_eq!(Camera::at(origin, f32::NAN).zoom(), 1.0);
        assert_eq!(Camera::at(origin, f32::INFINITY).zoom(), 1.0);
        assert_eq!(Camera::at(origin, 1.5).origin, origin);
    }

    #[test]
    fn pan_moves_the_world_with_the_pointer() {
        let mut cam = Camera::default();
        cam.zoom_step(5, Point::new(0.0, 0.0)); // zoom 2.0
        assert_eq!(cam.zoom(), 2.0);
        let before = cam.origin;
        cam.pan_by(Point::new(10.0, -4.0));
        assert!(close(
            cam.origin,
            Point::new(before.x - 5.0, before.y + 2.0)
        ));
    }

    #[test]
    fn fit_picks_largest_step_and_centers() {
        let mut cam = Camera::default();
        let bounds = Rect::new(0.0, 0.0, 1000.0, 500.0);
        let viewport = Size::new(1280.0, 800.0);
        cam.fit(bounds, viewport);
        // 1216x736 available: 1.1 fits (1100x550), 1.25 does not (1250 > 1216).
        assert_eq!(cam.zoom(), 1.1);
        assert!(close(
            cam.to_screen(Point::new(500.0, 250.0)),
            Point::new(640.0, 400.0)
        ));
    }

    #[test]
    fn fit_uses_smallest_step_when_nothing_fits() {
        let mut cam = Camera::default();
        cam.fit(
            Rect::new(0.0, 0.0, 100_000.0, 10.0),
            Size::new(800.0, 600.0),
        );
        assert_eq!(cam.zoom(), ZOOM_STEPS[0]);
    }
}
