//! The blocks on the canvas, in z-order.

use crate::{Camera, Point, Rect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub id: BlockId,
    pub rect: Rect,
}

/// The gap between a block and one Templum places beside it, and the least a block Templum
/// places keeps from every other block and drawing (world units): two dots of the canvas's grid.
pub const BESIDE: f32 = 48.0;

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

    /// The blocks a selection rectangle `area` (world units) touches, back-to-front.
    pub fn hits(&self, area: Rect) -> Vec<BlockId> {
        self.blocks
            .iter()
            .filter(|b| b.rect.intersects(&area))
            .map(|b| b.id)
            .collect()
    }

    /// The box drawn around the blocks `ids`: their union, `margin` outside. None with fewer than
    /// two of them on the canvas (one block is its own box).
    pub fn selection_box(&self, ids: &[BlockId], margin: f32) -> Option<Rect> {
        let rects: Vec<Rect> = self
            .blocks
            .iter()
            .filter(|b| ids.contains(&b.id))
            .map(|b| b.rect)
            .collect();
        let union = rects.iter().copied().reduce(|a, b| a.union(&b))?;
        (rects.len() > 1).then(|| {
            Rect::new(
                union.x - margin,
                union.y - margin,
                union.w + 2.0 * margin,
                union.h + 2.0 * margin,
            )
        })
    }

    pub fn bounds(&self) -> Option<Rect> {
        self.blocks
            .iter()
            .map(|b| b.rect)
            .reduce(|a, b| a.union(&b))
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

/// Where a block of `size` goes beside `anchors` (a block, then those wired to it), [`BESIDE`] or
/// more from every rect of `taken`. When blocks wired to it already line up on one of its sides,
/// [`BESIDE`] from it (right or left of it with their tops level, below or above it with their
/// left edges level), the new one takes the first free place down that column or along that row,
/// so the blocks it opens line up evenly spaced; with lines on several sides, the longest (on a
/// tie: right, below, left, above). Else the spot nearest the anchors all together (the sum of its
/// gaps to them), so a block's connections draw it their way: [`BESIDE`] right of an anchor,
/// below, left or above it, at its edge or past each rect in the way along that side; on a tie the
/// first anchor's, in that order of sides. With none free, right of everything.
pub fn free_spot(taken: &[Rect], anchors: &[Rect], size: Size) -> Rect {
    let first = anchors.first().copied().unwrap_or_default();
    let (w, h) = (size.w, size.h);
    let wired = anchors.get(1..).unwrap_or_default();
    let on_line = |side: usize, p: &Rect| match side {
        0 => (p.x - (first.right() + BESIDE)).abs() < 0.5,
        1 => (p.y - (first.bottom() + BESIDE)).abs() < 0.5,
        2 => (p.right() - (first.x - BESIDE)).abs() < 0.5,
        _ => (p.bottom() - (first.y - BESIDE)).abs() < 0.5,
    };
    let (lined, side) = (0..4)
        .map(|side| (wired.iter().filter(|p| on_line(side, p)).count(), side))
        .max_by_key(|&(lined, side)| (lined, std::cmp::Reverse(side)))
        .unwrap_or_default();
    if lined > 0 {
        let down = |x: f32| {
            let pasts = taken.iter().map(|t| t.bottom() + BESIDE);
            first_clear(taken, first.y, pasts, |y| Rect::new(x, y, w, h))
        };
        let along = |y: f32| {
            let pasts = taken.iter().map(|t| t.right() + BESIDE);
            first_clear(taken, first.x, pasts, |x| Rect::new(x, y, w, h))
        };
        return match side {
            0 => down(first.right() + BESIDE),
            1 => along(first.bottom() + BESIDE),
            2 => down(first.x - BESIDE - w),
            _ => along(first.y - BESIDE - h),
        };
    }
    let clear = |spot: &Rect| keeps_off(taken, spot);
    let mut best: Option<(f32, Rect)> = None;
    for a in anchors {
        // Along a side: at the anchor's edge, then just past each rect in the way.
        let ys: Vec<f32> = std::iter::once(a.y)
            .chain(
                taken
                    .iter()
                    .flat_map(|t| [t.bottom() + BESIDE, t.y - BESIDE - h]),
            )
            .collect();
        let xs: Vec<f32> = std::iter::once(a.x)
            .chain(
                taken
                    .iter()
                    .flat_map(|t| [t.right() + BESIDE, t.x - BESIDE - w]),
            )
            .collect();
        let (right, left) = (a.right() + BESIDE, a.x - BESIDE - w);
        let (below, above) = (a.bottom() + BESIDE, a.y - BESIDE - h);
        let spots = (ys.iter().map(|&y| Rect::new(right, y, w, h)))
            .chain(xs.iter().map(|&x| Rect::new(x, below, w, h)))
            .chain(ys.iter().map(|&y| Rect::new(left, y, w, h)))
            .chain(xs.iter().map(|&x| Rect::new(x, above, w, h)));
        for spot in spots.filter(clear) {
            let far: f32 = anchors.iter().map(|a| gap(&spot, a).round()).sum();
            if best.is_none_or(|(least, _)| far < least) {
                best = Some((far, spot));
            }
        }
    }
    best.map_or_else(
        || {
            let edge = taken.iter().map(Rect::right).fold(first.right(), f32::max);
            Rect::new(edge + BESIDE, first.y, w, h)
        },
        |(_, spot)| spot,
    )
}

/// Where a block goes that would sit at `at` (the view's middle): there when it is [`BESIDE`] or
/// more from every rect of `taken`; else, of the places [`BESIDE`] from a rect of `taken` and level
/// with it (right or left of it with their tops level, below or above it with their left edges
/// level) that keep off the rest, the nearest `at`, so blocks opened in one view grow a tidy grid
/// around its middle; on a tie right, below, left, above. With none, beside `at` ([`free_spot`]).
pub fn room_at(taken: &[Rect], at: Rect) -> Rect {
    if keeps_off(taken, &at) {
        return at;
    }
    let (w, h) = (at.w, at.h);
    let near = |spot: &Rect| gap(spot, &at).round();
    taken
        .iter()
        .flat_map(|t| {
            [
                Rect::new(t.right() + BESIDE, t.y, w, h),
                Rect::new(t.x, t.bottom() + BESIDE, w, h),
                Rect::new(t.x - BESIDE - w, t.y, w, h),
                Rect::new(t.x, t.y - BESIDE - h, w, h),
            ]
        })
        .filter(|spot| keeps_off(taken, spot))
        .min_by(|a, b| near(a).total_cmp(&near(b)))
        .unwrap_or_else(|| free_spot(taken, &[at], at.size()))
}

/// `spot` is [`BESIDE`] or more from every rect of `taken` (half a unit spared for rounding).
fn keeps_off(taken: &[Rect], spot: &Rect) -> bool {
    let m = BESIDE - 0.5;
    let room = Rect::new(spot.x - m, spot.y - m, spot.w + 2.0 * m, spot.h + 2.0 * m);
    !taken.iter().any(|t| t.intersects(&room))
}

/// The first of `at(start)`, then `at` each of `pasts` beyond `start`, nearest first, that keeps
/// off `taken`. The farthest keeps off everything, as `pasts` are [`BESIDE`] past each rect.
fn first_clear(
    taken: &[Rect],
    start: f32,
    pasts: impl Iterator<Item = f32>,
    at: impl Fn(f32) -> Rect,
) -> Rect {
    let mut steps: Vec<f32> = std::iter::once(start)
        .chain(pasts.filter(|&p| p > start))
        .collect();
    steps.sort_by(f32::total_cmp);
    let last = steps.last().copied().unwrap_or(start);
    steps
        .into_iter()
        .map(&at)
        .find(|spot| keeps_off(taken, spot))
        .unwrap_or_else(|| at(last))
}

/// How far apart two rects are: 0 when they touch or overlap.
fn gap(a: &Rect, b: &Rect) -> f32 {
    let dx = (b.x - a.right()).max(a.x - b.right()).max(0.0);
    let dy = (b.y - a.bottom()).max(a.y - b.bottom()).max(0.0);
    dx.hypot(dy)
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
    fn the_selection_box_wraps_two_or_more_blocks_with_a_margin() {
        let mut c = Canvas::default();
        let a = c.add(Rect::new(0.0, 0.0, 100.0, 50.0));
        let b = c.add(Rect::new(200.0, 100.0, 100.0, 50.0));
        let other = c.add(Rect::new(900.0, 900.0, 10.0, 10.0));
        assert_eq!(
            c.selection_box(&[a, b], 8.0),
            Some(Rect::new(-8.0, -8.0, 316.0, 166.0))
        );
        assert_eq!(
            c.selection_box(&[b, a, BlockId(99)], 0.0),
            c.selection_box(&[a, b], 0.0)
        );
        assert_eq!(c.selection_box(&[a], 8.0), None, "one block is its own box");
        assert_eq!(c.selection_box(&[a, BlockId(99)], 8.0), None);
        assert_eq!(c.selection_box(&[], 8.0), None);
        assert!(
            c.selection_box(&[a, other], 0.0)
                .unwrap()
                .contains(Point::new(500.0, 500.0))
        );
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
    fn hits_are_the_blocks_a_rectangle_touches_in_any_direction() {
        let mut c = Canvas::default();
        let a = c.add(rect(0.0, 0.0));
        let b = c.add(rect(200.0, 0.0));
        c.add(rect(0.0, 300.0));
        // Dragged from bottom right to top left, over a's corner and b's left half.
        let area = Rect::corners(Point::new(250.0, 20.0), Point::new(90.0, -10.0));
        assert_eq!(area, Rect::new(90.0, -10.0, 160.0, 30.0));
        assert_eq!(c.hits(area), [a, b]);
        assert!(c.hits(Rect::new(500.0, 500.0, 10.0, 10.0)).is_empty());
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
    fn a_free_spot_sits_to_the_right_and_slides_past_what_is_in_the_way() {
        let anchor = Rect::new(100.0, 50.0, 300.0, 200.0);
        let size = Size::new(100.0, 60.0);
        assert_eq!(
            free_spot(&[anchor], &[anchor], size),
            Rect::new(100.0 + 300.0 + 48.0, 50.0, 100.0, 60.0),
            "two dots of the grid apart, tops level"
        );
        let small = Rect::new(100.0 + 300.0 + BESIDE + 3.0, 52.0, 10.0, 10.0);
        assert_eq!(
            free_spot(&[anchor, small], &[anchor], size),
            Rect::new(100.0 + 300.0 + BESIDE, 62.0 + BESIDE, 100.0, 60.0),
            "down its right side, past the block there"
        );
    }

    /// `spot` is over no rect of `taken`.
    fn clear(spot: Rect, taken: &[Rect]) -> bool {
        !taken.iter().any(|t| t.intersects(&spot))
    }

    #[test]
    fn a_free_spot_goes_toward_the_anchors_connections() {
        let a = Rect::new(100.0, 50.0, 300.0, 200.0);
        let size = Size::new(100.0, 60.0);
        assert_eq!(
            free_spot(&[a], &[a], size),
            Rect::new(448.0, 50.0, 100.0, 60.0),
            "alone: on its right"
        );
        // Its connection is below it: the new block goes between them.
        let below = Rect::new(100.0, 700.0, 300.0, 100.0);
        assert_eq!(
            free_spot(&[a, below], &[a, below], size),
            Rect::new(100.0, 298.0, 100.0, 60.0)
        );
    }

    #[test]
    fn a_boxed_in_anchor_sends_the_block_beside_its_connection() {
        let a = Rect::new(100.0, 50.0, 300.0, 200.0);
        let walls = [
            Rect::new(448.0, 0.0, 400.0, 400.0),
            Rect::new(0.0, 298.0, 1000.0, 400.0),
            Rect::new(-500.0, 0.0, 552.0, 400.0),
            Rect::new(0.0, -500.0, 1000.0, 502.0),
        ];
        let far = Rect::new(2000.0, 50.0, 200.0, 100.0);
        let taken: Vec<Rect> = [a, far].into_iter().chain(walls).collect();
        let size = Size::new(100.0, 60.0);
        let spot = free_spot(&taken, &[a, far], size);
        assert!(clear(spot, &taken), "{spot:?}");
        assert!(
            spot.x > 848.0 && spot.right() <= far.x - BESIDE + 0.5,
            "past the walls, toward its connection: {spot:?}"
        );
    }

    /// Blocks opened one after the other beside a block (an agent's subagents) form a column on
    /// its right, tops level with it, evenly spaced; one closed leaves a place the next one takes.
    #[test]
    fn blocks_opened_beside_one_line_up_down_its_right() {
        let parent = Rect::new(0.0, 0.0, 600.0, 400.0);
        let size = Size::new(500.0, 300.0);
        let mut opened: Vec<Rect> = Vec::new();
        for _ in 0..4 {
            let taken: Vec<Rect> = std::iter::once(parent).chain(opened.clone()).collect();
            opened.push(free_spot(&taken, &taken, size));
        }
        let column: Vec<(f32, f32)> = opened.iter().map(|r| (r.x, r.y)).collect();
        assert_eq!(
            column,
            [
                (648.0, 0.0),
                (648.0, 348.0),
                (648.0, 696.0),
                (648.0, 1044.0)
            ]
        );
        opened.remove(1);
        let taken: Vec<Rect> = std::iter::once(parent).chain(opened).collect();
        assert_eq!(
            free_spot(&taken, &taken, size).origin(),
            Point::new(648.0, 348.0)
        );
    }

    /// When its first block went another way (toward a connection on its left), the next ones
    /// follow it down that side.
    #[test]
    fn the_next_blocks_follow_the_first_ones_side() {
        let parent = Rect::new(0.0, 0.0, 600.0, 400.0);
        let board = Rect::new(-3000.0, 0.0, 900.0, 540.0);
        let size = Size::new(500.0, 300.0);
        let mut taken = vec![parent, board];
        for _ in 0..3 {
            taken.push(free_spot(&taken, &taken, size));
        }
        let column: Vec<Point> = taken[2..].iter().map(Rect::origin).collect();
        assert_eq!(
            column,
            [(-548.0, 0.0), (-548.0, 348.0), (-548.0, 696.0)].map(|(x, y)| Point::new(x, y))
        );
    }

    /// Whatever is around, the spot found keeps [`BESIDE`] from everything.
    #[test]
    fn a_spot_keeps_its_distance_from_everything() {
        let mut seed: u32 = 7;
        let mut next = |n: u32| {
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            ((seed >> 16) % n) as f32
        };
        for _ in 0..200 {
            let taken: Vec<Rect> = (0..12)
                .map(|_| Rect::new(next(1600), next(1200), 50.0 + next(500), 40.0 + next(400)))
                .collect();
            let anchors = [taken[0], taken[1], taken[2]];
            let size = Size::new(80.0 + next(700), 60.0 + next(500));
            let at = Rect::new(next(1600), next(1200), size.w, size.h);
            for spot in [free_spot(&taken, &anchors, size), room_at(&taken, at)] {
                assert_eq!(spot.size(), size);
                for t in &taken {
                    assert!(gap(&spot, t) >= BESIDE - 0.5, "{spot:?} by {t:?}");
                }
            }
        }
    }

    /// Blocks opened one after the other in the same view grow a grid around its middle, each
    /// level with its neighbours, [`BESIDE`] apart; one where nothing is stays there.
    #[test]
    fn blocks_opened_in_one_view_grow_a_grid_around_its_middle() {
        let at = Rect::new(0.0, 0.0, 600.0, 400.0);
        let mut taken: Vec<Rect> = Vec::new();
        for _ in 0..9 {
            taken.push(room_at(&taken, at));
        }
        let at_ = |x: f32, y: f32| Rect::new(x, y, 600.0, 400.0);
        let (left, right, above, below) = (-648.0, 648.0, -448.0, 448.0);
        assert_eq!(
            taken,
            [
                at,
                at_(right, 0.0),
                at_(0.0, below),
                at_(left, 0.0),
                at_(0.0, above),
                at_(right, below),
                at_(right, above),
                at_(left, below),
                at_(left, above),
            ]
        );
        let elsewhere = Rect::new(5000.0, 0.0, 600.0, 400.0);
        assert_eq!(room_at(&taken, elsewhere), elsewhere);
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
