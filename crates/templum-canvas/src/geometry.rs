//! World-space geometry.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    pub const fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn origin(&self) -> Point {
        Point::new(self.x, self.y)
    }

    pub fn size(&self) -> Size {
        Size::new(self.w, self.h)
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Top and left edges are inside, right and bottom edges are not.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// True when the rects overlap by more than a shared edge.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    pub fn union(&self, other: &Rect) -> Rect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Rect::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_includes_top_left_edges_only() {
        let r = Rect::new(10.0, 20.0, 100.0, 50.0);
        assert!(r.contains(Point::new(10.0, 20.0)));
        assert!(r.contains(Point::new(109.9, 69.9)));
        assert!(!r.contains(Point::new(110.0, 30.0)));
        assert!(!r.contains(Point::new(50.0, 70.0)));
    }

    #[test]
    fn touching_rects_do_not_intersect() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(!a.intersects(&Rect::new(10.0, 0.0, 10.0, 10.0)));
        assert!(a.intersects(&Rect::new(9.0, 9.0, 10.0, 10.0)));
    }

    #[test]
    fn union_covers_both() {
        let u = Rect::new(0.0, 0.0, 10.0, 10.0).union(&Rect::new(-5.0, 20.0, 10.0, 10.0));
        assert_eq!(u, Rect::new(-5.0, 0.0, 15.0, 30.0));
        assert_eq!(u.size(), Size::new(15.0, 30.0));
        assert_eq!(u.origin(), Point::new(-5.0, 0.0));
    }
}
