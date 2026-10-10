//! Pure geometry and interaction logic of the infinite canvas. No UI, no IO.
//!
//! World units are logical pixels at zoom 1; screen units are logical pixels of the viewport.

mod camera;
mod geometry;

pub use camera::{Camera, FIT_MARGIN, ZOOM_STEPS};
pub use geometry::{Point, Rect, Size};
