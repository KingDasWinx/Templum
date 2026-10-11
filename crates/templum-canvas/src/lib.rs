//! Pure geometry and interaction logic of the infinite canvas. No UI, no IO.
//!
//! World units are logical pixels at zoom 1; screen units are logical pixels of the viewport.

mod camera;
mod canvas;
pub mod chrome;
mod dots;
mod flight;
mod geometry;
mod gesture;
mod snap;

pub use camera::{Camera, FIT_MARGIN, ZOOM_STEPS};
pub use canvas::{Block, BlockId, CASCADE, Canvas};
pub use dots::{DOT_SPACING, DotGrid, MIN_DOT_GAP, dot_grid, dot_size};
pub use flight::{CameraFlight, FLIGHT_ZOOM_STEP};
pub use geometry::{Point, Rect, Size};
pub use gesture::{Gesture, Update};
pub use snap::{Guide, SNAP_PX, Snapped, snap_move, snap_resize};
