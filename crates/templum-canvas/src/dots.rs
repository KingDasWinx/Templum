//! The dot grid drawn on the canvas, fixed to the world so it moves with pan and zoom.

use crate::{Camera, Point};

/// World units between dots at zoom 1.
pub const DOT_SPACING: f32 = 24.0;
/// Dots closer than this on screen are thinned out, 2× at a time: a Full HD viewport never
/// draws more than ~3,700 of them, and a zoom glide never jumps to a dense frame.
pub const MIN_DOT_GAP: f32 = 24.0;

/// A dot's diameter on screen: it grows with the zoom, within 1.5 and 3 px.
pub fn dot_size(zoom: f32) -> f32 {
    (2.0 * zoom).clamp(1.5, 3.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DotGrid {
    /// World units between dots.
    pub spacing: f32,
    /// The dot at or just before the viewport's top-left corner, in world units.
    pub first: Point,
}

pub fn dot_grid(camera: &Camera) -> DotGrid {
    let mut spacing = DOT_SPACING;
    while spacing * camera.zoom() < MIN_DOT_GAP {
        spacing *= 2.0;
    }
    let snap = |v: f32| (v / spacing).floor() * spacing;
    DotGrid {
        spacing,
        first: Point::new(snap(camera.origin.x), snap(camera.origin.y)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ZOOM_STEPS;

    fn at(zoom: f32, origin: Point) -> Camera {
        Camera::at(origin, zoom)
    }

    #[test]
    fn spacing_is_24_at_zoom_1() {
        assert_eq!(dot_grid(&Camera::default()).spacing, DOT_SPACING);
    }

    #[test]
    fn dots_are_never_closer_than_24_px_nor_thinned_needlessly() {
        for zoom in ZOOM_STEPS {
            let grid = dot_grid(&at(zoom, Point::default()));
            let gap = grid.spacing * zoom;
            assert!(gap >= 24.0, "zoom {zoom}: {gap} px");
            if grid.spacing > DOT_SPACING {
                assert!(
                    grid.spacing / 2.0 * zoom < 24.0,
                    "zoom {zoom}: over-thinned"
                );
            }
        }
        assert_eq!(dot_grid(&at(0.25, Point::default())).spacing, 96.0);
        assert_eq!(
            dot_grid(&at(0.01, Point::default())).spacing,
            3072.0,
            "30.72 px apart at 1%"
        );
    }

    /// How many dots a 1920×1080 viewport draws at `zoom`.
    fn dots_on_full_hd(zoom: f32) -> f32 {
        let grid = dot_grid(&Camera::unsnapped(Point::default(), zoom));
        let step = grid.spacing * zoom;
        ((1920.0 / step).ceil() + 1.0) * ((1080.0 / step).ceil() + 1.0)
    }

    #[test]
    fn a_full_hd_viewport_stays_under_3800_dots_at_every_zoom_a_flight_draws() {
        let mut last: Option<f32> = None;
        // 1% to 300%, a thousandth apart: finer than any CameraFlight draws.
        for zoom in (10..=3000).map(|k| k as f32 / 1000.0) {
            let n = dots_on_full_hd(zoom);
            assert!(n <= 3800.0, "zoom {zoom}: {n} dots");
            if let Some(l) = last {
                // Thinning halves the spacing: at most 4× fewer dots from one zoom to the next.
                assert!(n / l <= 4.5 && l / n <= 4.5, "zoom {zoom}: {l} -> {n}");
            }
            last = Some(n);
        }
    }

    #[test]
    fn dot_size_follows_the_zoom_between_a_floor_and_a_cap() {
        assert_eq!(dot_size(1.0), 2.0);
        assert_eq!(dot_size(0.25), 1.5);
        assert_eq!(dot_size(3.0), 3.0);
        // Never more than a quarter of the smallest gap: dots never run together.
        for zoom in ZOOM_STEPS {
            assert!(dot_size(zoom) <= MIN_DOT_GAP / 4.0, "zoom {zoom}");
        }
    }

    #[test]
    fn first_dot_is_on_the_world_lattice_at_or_before_the_origin() {
        for origin in [
            Point::new(0.0, 0.0),
            Point::new(30.0, 47.5),
            Point::new(-30.0, -1.0),
        ] {
            let grid = dot_grid(&at(1.0, origin));
            for (first, o) in [(grid.first.x, origin.x), (grid.first.y, origin.y)] {
                assert_eq!(first % grid.spacing, 0.0, "{first} on the lattice");
                assert!(first <= o && o - first < grid.spacing, "{first} vs {o}");
            }
        }
        // Panning keeps the dots where they were in the world.
        assert_eq!(dot_grid(&at(1.0, Point::new(-30.0, 0.0))).first.x, -48.0);
    }
}
