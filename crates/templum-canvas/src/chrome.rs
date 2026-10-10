//! The block frame: a title bar on top, the padded terminal grid below. World units.

use crate::{Point, Rect, Size};

pub const TITLE_H: f32 = 28.0;
pub const PAD: f32 = 6.0;
pub const HANDLE: f32 = 14.0;
pub const MIN_COLS: u16 = 20;
pub const MIN_ROWS: u16 = 4;

/// The grid area of a block.
pub fn content_rect(block: Rect) -> Rect {
    Rect::new(
        block.x + PAD,
        block.y + TITLE_H + PAD,
        (block.w - 2.0 * PAD).max(0.0),
        (block.h - TITLE_H - 2.0 * PAD).max(0.0),
    )
}

/// Whole cells fitting in `content`, never below MIN_COLS x MIN_ROWS.
pub fn grid_size(content: Size, cell: Size) -> (u16, u16) {
    // The epsilon keeps block_size -> grid_size exact despite float error.
    let fit = |len: f32, cell: f32| {
        if cell > 0.0 {
            (len / cell + 1e-3).floor() as u16
        } else {
            0
        }
    };
    (
        fit(content.w, cell.w).max(MIN_COLS),
        fit(content.h, cell.h).max(MIN_ROWS),
    )
}

/// The block size whose content holds exactly `cols` x `rows` cells.
pub fn block_size(cols: u16, rows: u16, cell: Size) -> Size {
    Size::new(
        cols as f32 * cell.w + 2.0 * PAD,
        rows as f32 * cell.h + TITLE_H + 2.0 * PAD,
    )
}

pub fn min_block_size(cell: Size) -> Size {
    block_size(MIN_COLS, MIN_ROWS, cell)
}

/// The grid cell under a world point: (row, col, pointer in the cell's right half).
/// Points above or left of the grid clamp to 0; the terminal clamps the far edges.
pub fn cell_at(block: Rect, world: Point, cell: Size) -> (usize, usize, bool) {
    let content = content_rect(block);
    let x = ((world.x - content.x) / cell.w).max(0.0);
    let y = ((world.y - content.y) / cell.h).max(0.0);
    (y as usize, x as usize, x.fract() >= 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CELL: Size = Size::new(8.4, 19.0);

    #[test]
    fn grid_and_block_sizes_round_trip() {
        for (cols, rows) in [(80, 24), (20, 4), (133, 41)] {
            let block = block_size(cols, rows, CELL);
            let content = content_rect(Rect::new(5.0, 5.0, block.w, block.h));
            assert_eq!(grid_size(content.size(), CELL), (cols, rows));
        }
    }

    #[test]
    fn grid_size_never_goes_below_minimum_or_divides_by_zero() {
        assert_eq!(grid_size(Size::new(10.0, 10.0), CELL), (MIN_COLS, MIN_ROWS));
        assert_eq!(
            grid_size(Size::new(500.0, 500.0), Size::new(0.0, 0.0)),
            (MIN_COLS, MIN_ROWS)
        );
    }

    #[test]
    fn cell_at_maps_world_points_to_grid_cells() {
        let block = Rect::new(100.0, 100.0, 500.0, 300.0);
        let origin = content_rect(block).origin();
        let p = Point::new(origin.x + CELL.w * 3.0 + 1.0, origin.y + CELL.h * 2.0 + 1.0);
        assert_eq!(cell_at(block, p, CELL), (2, 3, false));
        let p = Point::new(origin.x + CELL.w * 3.0 + 7.0, origin.y + 1.0);
        assert_eq!(cell_at(block, p, CELL), (0, 3, true));
        assert_eq!(
            cell_at(block, Point::new(0.0, 0.0), CELL),
            (0, 0, false),
            "clamped at 0"
        );
    }
}
