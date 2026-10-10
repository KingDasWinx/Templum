//! Pointer drags on the canvas. Pointer positions are screen units.

use crate::{BlockId, Canvas, Point, Rect, Size};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// Dragging empty canvas; `last` is the previous pointer position.
    Pan { last: Point },
    /// Dragging a title bar; `grab` is the pointer's world offset from the block's top-left.
    Move { id: BlockId, grab: Point },
    /// Dragging the resize handle; `grab` is the world offset from the block's bottom-right.
    Resize { id: BlockId, grab: Point, min: Size },
}

impl Gesture {
    pub fn pan(pointer: Point) -> Self {
        Gesture::Pan { last: pointer }
    }

    pub fn move_block(canvas: &Canvas, id: BlockId, pointer: Point) -> Option<Self> {
        let r = canvas.get(id)?.rect;
        let w = canvas.camera.to_world(pointer);
        Some(Gesture::Move {
            id,
            grab: Point::new(w.x - r.x, w.y - r.y),
        })
    }

    pub fn resize_block(canvas: &Canvas, id: BlockId, pointer: Point, min: Size) -> Option<Self> {
        let r = canvas.get(id)?.rect;
        let w = canvas.camera.to_world(pointer);
        Some(Gesture::Resize {
            id,
            grab: Point::new(r.right() - w.x, r.bottom() - w.y),
            min,
        })
    }

    /// Applies a pointer move. Returns the new rect of a resized block (to resize its PTY).
    /// A gesture whose block is gone does nothing.
    pub fn update(&mut self, canvas: &mut Canvas, pointer: Point) -> Option<(BlockId, Rect)> {
        match self {
            Gesture::Pan { last } => {
                canvas
                    .camera
                    .pan_by(Point::new(pointer.x - last.x, pointer.y - last.y));
                *last = pointer;
                None
            }
            Gesture::Move { id, grab } => {
                let w = canvas.camera.to_world(pointer);
                if let Some(b) = canvas.get_mut(*id) {
                    b.rect.x = w.x - grab.x;
                    b.rect.y = w.y - grab.y;
                }
                None
            }
            Gesture::Resize { id, grab, min } => {
                let w = canvas.camera.to_world(pointer);
                let b = canvas.get_mut(*id)?;
                b.rect.w = (w.x + grab.x - b.rect.x).max(min.w);
                b.rect.h = (w.y + grab.y - b.rect.y).max(min.h);
                Some((*id, b.rect))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_moves_the_camera_with_the_pointer() {
        let mut c = Canvas::default();
        let mut g = Gesture::pan(Point::new(100.0, 100.0));
        assert_eq!(g.update(&mut c, Point::new(130.0, 90.0)), None);
        assert_eq!(c.camera.origin, Point::new(-30.0, 10.0));
    }

    #[test]
    fn move_keeps_the_grab_offset_at_any_zoom() {
        let mut c = Canvas::default();
        let id = c.add(Rect::new(100.0, 100.0, 200.0, 100.0));
        c.camera.zoom_step(5, Point::new(0.0, 0.0)); // zoom 2
        let grab = c.camera.to_screen(Point::new(110.0, 105.0));
        let mut g = Gesture::move_block(&c, id, grab).unwrap();
        g.update(&mut c, Point::new(grab.x + 20.0, grab.y + 40.0));
        assert_eq!(
            c.get(id).unwrap().rect,
            Rect::new(110.0, 120.0, 200.0, 100.0)
        );
    }

    #[test]
    fn resize_reports_the_new_rect_and_clamps_to_minimum() {
        let mut c = Canvas::default();
        let id = c.add(Rect::new(0.0, 0.0, 200.0, 100.0));
        let min = Size::new(150.0, 80.0);
        let mut g = Gesture::resize_block(&c, id, Point::new(195.0, 95.0), min).unwrap();
        assert_eq!(
            g.update(&mut c, Point::new(295.0, 145.0)),
            Some((id, Rect::new(0.0, 0.0, 300.0, 150.0)))
        );
        assert_eq!(
            g.update(&mut c, Point::new(10.0, 10.0)),
            Some((id, Rect::new(0.0, 0.0, 150.0, 80.0)))
        );
    }

    #[test]
    fn gesture_on_a_removed_block_is_inert() {
        let mut c = Canvas::default();
        let id = c.add(Rect::new(0.0, 0.0, 200.0, 100.0));
        let mut mv = Gesture::move_block(&c, id, Point::new(5.0, 5.0)).unwrap();
        let mut rs =
            Gesture::resize_block(&c, id, Point::new(195.0, 95.0), Size::new(1.0, 1.0)).unwrap();
        c.remove(id);
        assert_eq!(mv.update(&mut c, Point::new(50.0, 50.0)), None);
        assert_eq!(rs.update(&mut c, Point::new(50.0, 50.0)), None);
        assert_eq!(Gesture::move_block(&c, id, Point::new(0.0, 0.0)), None);
    }
}
