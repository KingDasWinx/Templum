//! The blocks on the canvas, in z-order.

use crate::{Camera, Point, Rect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub rect: Rect,
}

/// Offset applied by [`Canvas::place_new`] while a spot is taken.
pub const CASCADE: f32 = 32.0;

#[derive(Debug, Default)]
pub struct Canvas {
    pub camera: Camera,
    /// Back-to-front: the last block is drawn on top.
    blocks: Vec<Block>,
    next_id: u64,
}

impl Canvas {
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Adds a block on top. Ids are never reused.
    pub fn add(&mut self, rect: Rect) -> BlockId {
        let id = BlockId(self.next_id);
        self.next_id += 1;
        self.blocks.push(Block { id, rect });
        id
    }

    pub fn remove(&mut self, id: BlockId) -> Option<Block> {
        let i = self.index(id)?;
        Some(self.blocks.remove(i))
    }

    /// Brings a block to the front.
    pub fn raise(&mut self, id: BlockId) {
        if let Some(i) = self.index(id) {
            let block = self.blocks.remove(i);
            self.blocks.push(block);
        }
    }

    pub fn get(&self, id: BlockId) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    pub fn get_mut(&mut self, id: BlockId) -> Option<&mut Block> {
        self.blocks.iter_mut().find(|b| b.id == id)
    }

    pub fn top(&self) -> Option<BlockId> {
        self.blocks.last().map(|b| b.id)
    }

    /// Blocks intersecting a viewport of `viewport` size (screen units), back-to-front.
    pub fn visible(&self, viewport: Size) -> impl Iterator<Item = &Block> {
        self.visible_from(&self.camera, viewport)
    }

    /// Like [`Canvas::visible`], seen through `camera` (a frame of a flight).
    pub fn visible_from(&self, camera: &Camera, viewport: Size) -> impl Iterator<Item = &Block> {
        let z = camera.zoom();
        let world = Rect::new(
            camera.origin.x,
            camera.origin.y,
            viewport.w / z,
            viewport.h / z,
        );
        self.blocks
            .iter()
            .filter(move |b| b.rect.intersects(&world))
    }

    pub fn bounds(&self) -> Option<Rect> {
        self.blocks
            .iter()
            .map(|b| b.rect)
            .reduce(|a, b| a.union(&b))
    }

    /// Where a new block of `size` goes: at `near`, shifted by [`CASCADE`] while another
    /// block's top-left corner is within 8 units.
    pub fn place_new(&self, size: Size, near: Point) -> Rect {
        let mut p = near;
        while self
            .blocks
            .iter()
            .any(|b| (b.rect.x - p.x).abs() < 8.0 && (b.rect.y - p.y).abs() < 8.0)
        {
            p = Point::new(p.x + CASCADE, p.y + CASCADE);
        }
        Rect::new(p.x, p.y, size.w, size.h)
    }

    /// Frames every block; with none, back to 100% around the viewport center.
    pub fn fit_all(&mut self, viewport: Size) {
        match self.bounds() {
            Some(bounds) => self.camera.fit(bounds, viewport),
            None => self
                .camera
                .zoom_reset(Point::new(viewport.w / 2.0, viewport.h / 2.0)),
        }
    }

    fn index(&self, id: BlockId) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32) -> Rect {
        Rect::new(x, y, 100.0, 50.0)
    }

    #[test]
    fn add_remove_raise_keep_back_to_front_order() {
        let mut c = Canvas::default();
        let a = c.add(rect(0.0, 0.0));
        let b = c.add(rect(10.0, 0.0));
        let d = c.add(rect(20.0, 0.0));
        assert_ne!(a, b);
        c.raise(a);
        let order: Vec<_> = c.blocks().iter().map(|b| b.id).collect();
        assert_eq!(order, [b, d, a]);
        assert_eq!(c.top(), Some(a));
        assert_eq!(c.remove(b).map(|b| b.id), Some(b));
        assert_eq!(c.remove(b), None);
        assert_eq!(c.blocks().len(), 2);
        let e = c.add(rect(0.0, 0.0));
        assert!(e != a && e != b && e != d, "ids are never reused");
    }

    #[test]
    fn visible_culls_blocks_outside_the_viewport() {
        let mut c = Canvas::default();
        let inside = c.add(rect(10.0, 10.0));
        c.add(rect(5000.0, 10.0));
        let ids: Vec<_> = c.visible(Size::new(800.0, 600.0)).map(|b| b.id).collect();
        assert_eq!(ids, [inside]);
        c.camera.zoom_step(-6, Point::new(0.0, 0.0)); // zoom 0.25: sees 3200 world units
        c.camera.origin = Point::new(2000.0, 0.0);
        assert_eq!(c.visible(Size::new(800.0, 600.0)).count(), 1);
    }

    #[test]
    fn visible_from_culls_with_another_camera() {
        let mut c = Canvas::default();
        c.add(rect(10.0, 10.0));
        let far = c.add(rect(5000.0, 10.0));
        let elsewhere = Camera::at(Point::new(4900.0, 0.0), 1.0);
        let ids: Vec<_> = c
            .visible_from(&elsewhere, Size::new(800.0, 600.0))
            .map(|b| b.id)
            .collect();
        assert_eq!(ids, [far]);
    }

    #[test]
    fn bounds_is_the_union_of_all_blocks() {
        let mut c = Canvas::default();
        assert_eq!(c.bounds(), None);
        c.add(rect(0.0, 0.0));
        c.add(rect(200.0, 100.0));
        assert_eq!(c.bounds(), Some(Rect::new(0.0, 0.0, 300.0, 150.0)));
    }

    #[test]
    fn place_new_cascades_off_taken_spots() {
        let mut c = Canvas::default();
        let size = Size::new(100.0, 50.0);
        assert_eq!(c.place_new(size, Point::new(0.0, 0.0)), rect(0.0, 0.0));
        c.add(rect(0.0, 0.0));
        c.add(rect(CASCADE + 3.0, CASCADE - 2.0));
        assert_eq!(
            c.place_new(size, Point::new(1.0, 1.0)),
            rect(1.0 + 2.0 * CASCADE, 1.0 + 2.0 * CASCADE)
        );
    }

    #[test]
    fn fit_all_frames_blocks_or_resets_when_empty() {
        let mut c = Canvas::default();
        c.camera.zoom_step(3, Point::new(0.0, 0.0));
        c.fit_all(Size::new(800.0, 600.0));
        assert_eq!(c.camera.zoom(), 1.0, "empty canvas resets to 100%");
        c.add(Rect::new(0.0, 0.0, 2000.0, 100.0));
        c.fit_all(Size::new(800.0, 600.0));
        assert!(c.camera.zoom() < 1.0);
        assert_eq!(c.visible(Size::new(800.0, 600.0)).count(), 1);
    }
}
