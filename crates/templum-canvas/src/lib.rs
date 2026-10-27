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
mod ink;
mod lod;
mod snap;
mod tile;
mod wire;

pub use camera::{Camera, FIT_MARGIN, ZOOM_STEPS};
pub use canvas::{BESIDE, Block, BlockId, Canvas, free_spot, room_at};
pub use dots::{DOT_SPACING, DotGrid, MIN_DOT_GAP, dot_grid, dot_size};
pub use flight::{CameraFlight, FLIGHT_ZOOM_STEP};
pub use geometry::{Point, Rect, Size};
pub use gesture::{EDGE_SPEED, EDGE_ZONE, Edges, Gesture, Update, edge_scroll};
pub use ink::{
    Color, Drawing, HIT_PX, MAX_WIDTH, MIN_POINT_DISTANCE, MIN_SHAPE, MIN_WIDTH, STRING_PX,
    ShapeKind, constrain, simplify,
};
pub use lod::{CARD_BELOW, GREEK_BELOW, LABEL_FITS, Level, block_level, level};
pub use snap::{Guide, SNAP_PX, Snapped, snap_move, snap_resize};
pub use tile::{Direction, GAP, Split, Tile, neighbour};
pub use wire::{Curve, Wire};
