//! Pure geometry and interaction logic of the infinite canvas. No UI, no IO.
//!
//! World units are logical pixels at zoom 1; screen units are logical pixels of the viewport.

mod camera;
mod canvas;
pub mod chrome;
mod geometry;
mod gesture;

pub use camera::{Camera, FIT_MARGIN, ZOOM_STEPS};
pub use canvas::{Block, BlockId, CASCADE, Canvas};
pub use geometry::{Point, Rect, Size};
pub use gesture::Gesture;
