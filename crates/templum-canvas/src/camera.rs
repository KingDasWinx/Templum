//! Maps world space to the screen.

use crate::{Point, Rect, Size};

/// Fixed zoom steps keep the glyph cache bounded: text is rasterized once per step. Under 25%
/// each is 1.5 or 1.33 times the one below (every second one doubles), down to 1%, where every
/// block of default text is a card or a shape (`lod.rs`): no text but the fixed-size labels.
pub const ZOOM_STEPS: [f32; 23] = [
    0.01, 0.015, 0.02, 0.03, 0.04, 0.06, 0.08, 0.12, 0.16, 0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0,
    1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];
const ZOOM_1: usize = 15;
/// Screen margin kept around the blocks by [`Camera::fit`].
pub const FIT_MARGIN: f32 = 32.0;

/// Every way of moving the camera lands its zoom on [`ZOOM_STEPS`]; only a frame of a
/// [`CameraFlight`](crate::CameraFlight) is drawn between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    /// World point shown at the viewport's top-left corner.
    pub origin: Point,
    zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            origin: Point::default(),
            zoom: ZOOM_STEPS[ZOOM_1],
        }
    }
}

impl Camera {
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// The step nearest to the zoom: the zoom's own step, except during a flight.
    pub fn zoom_index(&self) -> usize {
        nearest_step(self.zoom)
    }

    /// A camera at `origin` with the zoom step nearest to `zoom` (a saved canvas). A zoom that
    /// isn't a finite number gives 100%.
    pub fn at(origin: Point, zoom: f32) -> Camera {
        Camera {
            origin,
            zoom: ZOOM_STEPS[nearest_step(zoom)],
        }
    }

    /// A camera at any zoom: a frame of a flight, never where the camera rests.
    pub fn unsnapped(origin: Point, zoom: f32) -> Camera {
        Camera { origin, zoom }
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
        let idx = (self.zoom_index() as i32 + delta).clamp(0, ZOOM_STEPS.len() as i32 - 1);
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
        self.zoom = ZOOM_STEPS[ZOOM_STEPS
            .iter()
            .rposition(|&z| bounds.w * z <= avail_w && bounds.h * z <= avail_h)
            .unwrap_or(0)];
        let z = self.zoom();
        self.origin = Point::new(
            bounds.x + bounds.w / 2.0 - viewport.w / 2.0 / z,
            bounds.y + bounds.h / 2.0 - viewport.h / 2.0 / z,
        );
    }

    /// The camera showing `rect` whole, with [`FIT_MARGIN`] around it, moved as little as
    /// possible: itself when it already does, a pan when the rect fits at this zoom, else
    /// [`fit`](Self::fit), which zooms out.
    pub fn reveal(&self, rect: Rect, viewport: Size) -> Camera {
        let z = self.zoom();
        let (margin, view) = (FIT_MARGIN / z, Size::new(viewport.w / z, viewport.h / z));
        if rect.w + 2.0 * margin > view.w || rect.h + 2.0 * margin > view.h {
            let mut fit = *self;
            fit.fit(rect, viewport);
            return fit;
        }
        // One axis: the world interval [lo, lo + len] inside [origin, origin + view], margins
        // included.
        let axis = |origin: f32, lo: f32, len: f32, view: f32| {
            if lo - margin < origin {
                lo - margin
            } else if lo + len + margin > origin + view {
                lo + len + margin - view
            } else {
                origin
            }
        };
        Camera {
            origin: Point::new(
                axis(self.origin.x, rect.x, rect.w, view.w),
                axis(self.origin.y, rect.y, rect.h, view.h),
            ),
            zoom: z,
        }
    }

    fn set_zoom(&mut self, idx: usize, anchor: Point) {
        let world = self.to_world(anchor);
        self.zoom = ZOOM_STEPS[idx];
        self.origin = Point::new(
            world.x - anchor.x / self.zoom(),
            world.y - anchor.y / self.zoom(),
        );
    }
}

/// The index of the step nearest to `zoom`; 100% for a zoom that isn't a finite number.
fn nearest_step(zoom: f32) -> usize {
    if !zoom.is_finite() {
        return ZOOM_1;
    }
    ZOOM_STEPS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (*a - zoom).abs().total_cmp(&(*b - zoom).abs()))
        .map_or(ZOOM_1, |(i, _)| i)
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
        assert_eq!(cam.zoom(), 0.01, "1% is the smallest");
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
        assert_eq!(Camera::at(origin, 0.0).zoom(), 0.01);
        assert_eq!(
            Camera::at(origin, 0.06).zoom(),
            0.06,
            "a zoom under 25% is kept"
        );
        assert_eq!(Camera::at(origin, 0.013).zoom(), 0.015);
        assert_eq!(Camera::at(origin, f32::NAN).zoom(), 1.0);
        assert_eq!(Camera::at(origin, f32::INFINITY).zoom(), 1.0);
        assert_eq!(Camera::at(origin, 1.5).origin, origin);
    }

    #[test]
    fn unsnapped_keeps_its_zoom_and_a_step_from_there_lands_on_a_step() {
        let mut cam = Camera::unsnapped(Point::new(7.0, 8.0), 1.18);
        assert_eq!(cam.zoom(), 1.18);
        assert_eq!(ZOOM_STEPS[cam.zoom_index()], 1.25, "the nearest step");
        cam.zoom_step(1, Point::new(0.0, 0.0));
        assert_eq!(cam.zoom(), 1.5);
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
    fn reveal_moves_just_enough_to_show_a_rect_whole() {
        let viewport = Size::new(1000.0, 800.0);
        let mut cam = Camera::default();
        cam.zoom_step(-2, Point::new(0.0, 0.0)); // zoom 0.75
        // Already shown with its margins: nothing moves.
        let inside = Rect::new(100.0, 100.0, 400.0, 300.0);
        assert_eq!(cam.reveal(inside, viewport), cam);
        // Hanging off the right and the bottom: a pan, the zoom stays.
        let right = Rect::new(1100.0, 900.0, 400.0, 200.0);
        let to = cam.reveal(right, viewport);
        assert_eq!(to.zoom(), 0.75);
        let shown = to.rect_to_screen(right);
        assert!((shown.x + shown.w - (viewport.w - FIT_MARGIN)).abs() < 1e-3);
        assert!((shown.y + shown.h - (viewport.h - FIT_MARGIN)).abs() < 1e-3);
        // Off the left and the top.
        let left = Rect::new(-500.0, -400.0, 200.0, 100.0);
        let shown = cam.reveal(left, viewport).rect_to_screen(left);
        assert!(close(shown.origin(), Point::new(FIT_MARGIN, FIT_MARGIN)));
        // Too big at this zoom: framed, at a smaller step.
        let big = Rect::new(0.0, 0.0, 2000.0, 400.0);
        let to = cam.reveal(big, viewport);
        assert!(to.zoom() < 0.75);
        let shown = to.rect_to_screen(big);
        assert!(shown.x >= 0.0 && shown.x + shown.w <= viewport.w);
    }

    #[test]
    fn steps_go_down_to_1_percent_a_third_or_a_half_apart() {
        assert_eq!(ZOOM_STEPS[0], 0.01);
        assert_eq!(ZOOM_STEPS[ZOOM_1], 1.0);
        assert!(ZOOM_STEPS.is_sorted());
        // Under 25% as far apart as 25%, 33% and 50% are (16% to 25% is 1.56 times).
        for pair in ZOOM_STEPS.windows(2).filter(|p| p[1] <= 0.25) {
            let k = pair[1] / pair[0];
            assert!((1.33..=1.57).contains(&k), "{pair:?}: {k}");
        }
    }

    #[test]
    fn fit_frames_a_wide_canvas_under_25_percent() {
        let mut cam = Camera::default();
        // 1216 px across: 0.06 shows 20,000 units in 1,200.
        cam.fit(
            Rect::new(0.0, 0.0, 20_000.0, 500.0),
            Size::new(1280.0, 800.0),
        );
        assert_eq!(cam.zoom(), 0.06);
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
