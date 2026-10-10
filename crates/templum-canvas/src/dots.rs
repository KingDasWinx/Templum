//! The dot grid drawn on the canvas, fixed to the world so it moves with pan and zoom.

use crate::{Camera, Point};

/// World units between dots at zoom 1.
pub const DOT_SPACING: f32 = 24.0;
/// Dots closer than this on screen are thinned out, 4× at a time.
pub const MIN_DOT_GAP: f32 = 16.0;

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
        spacing *= 4.0;
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

    fn at_step(i: usize, origin: Point) -> Camera {
        let mut cam = Camera::default();
        cam.zoom_step(i as i32 - 6, Point::default());
        cam.origin = origin;
        cam
    }

    #[test]
    fn spacing_is_24_at_zoom_1() {
        assert_eq!(dot_grid(&Camera::default()).spacing, DOT_SPACING);
    }

    #[test]
    fn dots_are_never_closer_than_16_px_nor_thinned_needlessly() {
        for (i, zoom) in ZOOM_STEPS.iter().enumerate() {
            let grid = dot_grid(&at_step(i, Point::default()));
            let gap = grid.spacing * zoom;
            assert!(gap >= MIN_DOT_GAP, "zoom {zoom}: {gap} px");
            if grid.spacing > DOT_SPACING {
                assert!(
                    grid.spacing / 4.0 * zoom < MIN_DOT_GAP,
                    "zoom {zoom}: over-thinned"
                );
            }
        }
        assert_eq!(
            dot_grid(&at_step(0, Point::default())).spacing,
            96.0,
            "zoom 0.25"
        );
    }

    #[test]
    fn first_dot_is_on_the_world_lattice_at_or_before_the_origin() {
        for origin in [
            Point::new(0.0, 0.0),
            Point::new(30.0, 47.5),
            Point::new(-30.0, -1.0),
        ] {
            let grid = dot_grid(&at_step(6, origin));
            for (first, o) in [(grid.first.x, origin.x), (grid.first.y, origin.y)] {
                assert_eq!(first % grid.spacing, 0.0, "{first} on the lattice");
                assert!(first <= o && o - first < grid.spacing, "{first} vs {o}");
            }
        }
        // Panning keeps the dots where they were in the world.
        assert_eq!(dot_grid(&at_step(6, Point::new(-30.0, 0.0))).first.x, -48.0);
    }
}
