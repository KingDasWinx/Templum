//! The camera gliding from where it is drawn to where it is going (zoom steps, reset, fit).

use crate::{Camera, Point};

/// Zooms drawn on the way sit on multiples of this step (like block scales): text is rasterized
/// at a few extra sizes only. Under 16% the step halves each time the zoom does (`on_step`), so
/// a glide near 1% still has frames between its ends.
pub const FLIGHT_ZOOM_STEP: f32 = 0.02;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFlight {
    pub from: Camera,
    pub to: Camera,
    /// The screen point whose world position moves in a straight line: the pointer for a zoom
    /// step, the viewport's center for a reset or a fit.
    pub pivot: Point,
}

impl CameraFlight {
    /// The camera drawn at eased progress `t` (0 at `from`, 1 at `to`).
    pub fn at(&self, t: f32) -> Camera {
        if t <= 0.0 {
            return self.from;
        }
        if t >= 1.0 {
            return self.to;
        }
        let (z0, z1) = (self.from.zoom(), self.to.zoom());
        // 1/zoom moves linearly: every world point then travels in a straight line on screen.
        let inv = 1.0 / z0 + (1.0 / z1 - 1.0 / z0) * t;
        let zoom = on_step(1.0 / inv).clamp(z0.min(z1), z0.max(z1));
        let (w0, w1) = (self.from.to_world(self.pivot), self.to.to_world(self.pivot));
        let w = Point::new(w0.x + (w1.x - w0.x) * t, w0.y + (w1.y - w0.y) * t);
        Camera::unsnapped(
            Point::new(w.x - self.pivot.x / zoom, w.y - self.pivot.y / zoom),
            zoom,
        )
    }

    /// Panning mid-flight moves both ends, so the world follows the pointer and nothing jumps.
    pub fn pan_by(&mut self, screen_delta: Point) {
        self.from.pan_by(screen_delta);
        self.to.pan_by(screen_delta);
    }
}

/// `zoom` (positive) on the nearest multiple of [`FLIGHT_ZOOM_STEP`], or of its half, its quarter…
/// while the step is more than an eighth of the zoom: 0.02 from 16% up, 0.00125 under 2%.
fn on_step(zoom: f32) -> f32 {
    let mut step = FLIGHT_ZOOM_STEP;
    while step > zoom / 8.0 {
        step /= 2.0;
    }
    (zoom / step).round() * step
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Rect, Size};

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    fn samples() -> impl Iterator<Item = f32> {
        (1..100).map(|i| i as f32 / 100.0)
    }

    /// One zoom step in at `pivot`, like Ctrl+wheel.
    fn step_in(pivot: Point) -> CameraFlight {
        let from = Camera::at(Point::new(-40.0, 25.0), 1.0);
        let mut to = from;
        to.zoom_step(1, pivot);
        CameraFlight { from, to, pivot }
    }

    #[test]
    fn endpoints_are_exact() {
        let f = step_in(Point::new(300.0, 200.0));
        assert_eq!(f.at(0.0), f.from);
        assert_eq!(f.at(-1.0), f.from);
        assert_eq!(f.at(1.0), f.to);
        assert_eq!(f.at(1.5), f.to);
    }

    #[test]
    fn a_zoom_step_keeps_the_pointer_still_at_every_frame() {
        let pivot = Point::new(300.0, 200.0);
        let f = step_in(pivot);
        let world = f.from.to_world(pivot);
        for t in samples() {
            assert!(close(f.at(t).to_world(pivot), world), "t {t}");
        }
    }

    #[test]
    fn zooms_on_the_way_sit_on_steps_and_stay_between_the_ends() {
        // Up, and down at the far end, where a 0.02 step would round 1% to nothing.
        for (a, b, step) in [
            (0.5, 2.0, 0.02),
            (0.01, 0.015, 0.00125),
            (0.03, 0.02, 0.0025),
        ] {
            let f = CameraFlight {
                from: Camera::at(Point::default(), a),
                to: Camera::at(Point::default(), b),
                pivot: Point::default(),
            };
            let (lo, hi) = (f32::min(a, b), f32::max(a, b));
            let mut last = a;
            let mut between = 0;
            for t in samples() {
                let z = f.at(t).zoom();
                let k = z / step;
                assert!((k - k.round()).abs() < 1e-3, "{a} to {b}, t {t}: {z}");
                assert!(
                    (lo..=hi).contains(&z) && (z - last) * (b - a) >= 0.0,
                    "{a} to {b}, t {t}: {z} after {last}"
                );
                between += usize::from(z != last && z != b);
                last = z;
            }
            assert!(between >= 2, "{a} to {b}: {between} zooms on the way");
        }
    }

    #[test]
    fn the_step_is_0_02_from_16_percent_and_halves_with_the_zoom_under_it() {
        let near = |z: f32, want: f32| (on_step(z) - want).abs() < 1e-6;
        assert!(near(1.013, 1.02));
        assert!(
            near(0.161, 0.16) && near(0.171, 0.18),
            "0.02 apart from 16%"
        );
        assert!(near(0.153, 0.15), "0.01 apart from 8%");
        assert!(near(0.0113, 0.01125), "0.00125 apart under 2%");
    }

    #[test]
    fn the_pivot_moves_in_a_straight_line() {
        // A fit: different origins and zooms, pivoting on the viewport center.
        let viewport = Size::new(1280.0, 800.0);
        let pivot = Point::new(640.0, 400.0);
        let from = Camera::at(Point::new(5000.0, -300.0), 2.0);
        let mut to = from;
        to.fit(Rect::new(0.0, 0.0, 2400.0, 900.0), viewport);
        let f = CameraFlight { from, to, pivot };
        let (w0, w1) = (from.to_world(pivot), to.to_world(pivot));
        let half = f.at(0.5).to_world(pivot);
        assert!(close(
            half,
            Point::new((w0.x + w1.x) / 2.0, (w0.y + w1.y) / 2.0)
        ));
    }

    #[test]
    fn pan_by_moves_both_ends() {
        let mut f = step_in(Point::new(300.0, 200.0));
        let (a, b) = (f.from.origin, f.to.origin);
        f.pan_by(Point::new(30.0, -10.0));
        assert!(close(f.from.origin, Point::new(a.x - 30.0, a.y + 10.0)));
        let z = f.to.zoom();
        assert!(close(
            f.to.origin,
            Point::new(b.x - 30.0 / z, b.y + 10.0 / z)
        ));
    }
}
