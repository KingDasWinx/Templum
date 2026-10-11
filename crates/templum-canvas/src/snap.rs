//! Snapping: a moved or resized block lines up with its neighbours' edges and centers when it
//! comes within a few screen pixels, with no key held, and lets go as soon as it is farther.

use crate::Rect;

/// How close, in screen pixels, an edge must come to snap. The caller divides by the zoom.
pub const SNAP_PX: f32 = 8.0;

/// A line drawn while a snap is in effect, world units, along the aligned edge or center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guide {
    Vertical { x: f32, y0: f32, y1: f32 },
    Horizontal { y: f32, x0: f32, x1: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapped {
    pub rect: Rect,
    pub guides: Vec<Guide>,
}

/// A block being moved: its left, center and right line up with the others' left, center and
/// right (and the same vertically). Each axis takes its nearest candidate within `threshold`.
pub fn snap_move(rect: Rect, others: &[Rect], threshold: f32) -> Snapped {
    let xs = [rect.x, rect.x + rect.w / 2.0, rect.right()];
    let ys = [rect.y, rect.y + rect.h / 2.0, rect.bottom()];
    let x = nearest(
        &xs,
        others,
        |o| [o.x, o.x + o.w / 2.0, o.right()],
        threshold,
    );
    let y = nearest(
        &ys,
        others,
        |o| [o.y, o.y + o.h / 2.0, o.bottom()],
        threshold,
    );
    let mut snapped = rect;
    if let Some(hit) = &x {
        snapped.x += hit.delta;
    }
    if let Some(hit) = &y {
        snapped.y += hit.delta;
    }
    Snapped {
        rect: snapped,
        guides: guides(snapped, x, y),
    }
}

/// A block being resized by its bottom-right corner: its right and bottom edges line up with the
/// others' edges, or its width or height matches another block's. It never moves.
pub fn snap_resize(rect: Rect, others: &[Rect], threshold: f32) -> Snapped {
    let x = nearest(
        &[rect.right()],
        others,
        |o| [o.x, o.right(), rect.x + o.w],
        threshold,
    );
    let y = nearest(
        &[rect.bottom()],
        others,
        |o| [o.y, o.bottom(), rect.y + o.h],
        threshold,
    );
    let mut snapped = rect;
    if let Some(hit) = &x {
        snapped.w += hit.delta;
    }
    if let Some(hit) = &y {
        snapped.h += hit.delta;
    }
    Snapped {
        rect: snapped,
        guides: guides(snapped, x, y),
    }
}

/// One axis's snap: how far to move, the line landed on, and whose it is.
struct Hit {
    delta: f32,
    at: f32,
    other: Rect,
}

/// The nearest (mine, theirs) pair within `threshold`, over every other block.
fn nearest(
    mine: &[f32],
    others: &[Rect],
    theirs: impl Fn(&Rect) -> [f32; 3],
    threshold: f32,
) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for other in others {
        for at in theirs(other) {
            for m in mine {
                let delta = at - m;
                if delta.abs() <= threshold
                    && best.as_ref().is_none_or(|b| delta.abs() < b.delta.abs())
                {
                    best = Some(Hit {
                        delta,
                        at,
                        other: *other,
                    });
                }
            }
        }
    }
    best
}

/// The guides for the snapped rect: along the line, from the nearest end of the two blocks to
/// the farthest.
fn guides(rect: Rect, x: Option<Hit>, y: Option<Hit>) -> Vec<Guide> {
    let mut guides = Vec::new();
    if let Some(hit) = x {
        guides.push(Guide::Vertical {
            x: hit.at,
            y0: rect.y.min(hit.other.y),
            y1: rect.bottom().max(hit.other.bottom()),
        });
    }
    if let Some(hit) = y {
        guides.push(Guide::Horizontal {
            y: hit.at,
            x0: rect.x.min(hit.other.x),
            x1: rect.right().max(hit.other.right()),
        });
    }
    guides
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: f32 = 8.0;

    fn others() -> Vec<Rect> {
        vec![Rect::new(100.0, 100.0, 200.0, 100.0)]
    }

    #[test]
    fn a_left_edge_near_another_left_edge_snaps_and_gives_a_guide() {
        let s = snap_move(Rect::new(105.0, 300.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect, Rect::new(100.0, 300.0, 50.0, 50.0));
        assert_eq!(
            s.guides,
            [Guide::Vertical {
                x: 100.0,
                y0: 100.0,
                y1: 350.0
            }]
        );
    }

    #[test]
    fn snapping_lets_go_past_the_threshold() {
        let s = snap_move(Rect::new(109.0, 300.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect, Rect::new(109.0, 300.0, 50.0, 50.0));
        assert!(s.guides.is_empty());
    }

    #[test]
    fn right_edges_and_centers_snap_too() {
        // Right edge 303 near the other's right edge 300.
        let s = snap_move(Rect::new(253.0, 300.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect.right(), 300.0);
        // Vertical center 152 near the other's 150 (y 127 + 25).
        let s = snap_move(Rect::new(500.0, 127.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect.y, 125.0);
        assert_eq!(
            s.guides,
            [Guide::Horizontal {
                y: 150.0,
                x0: 100.0,
                x1: 550.0
            }]
        );
    }

    #[test]
    fn each_axis_snaps_on_its_own_and_the_nearest_candidate_wins() {
        // x: left 103 → 100. y: top 203 → the other's bottom 200.
        let s = snap_move(Rect::new(103.0, 203.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect, Rect::new(100.0, 200.0, 50.0, 50.0));
        assert_eq!(s.guides.len(), 2);
        // Two others: the nearer edge wins.
        let two = vec![others()[0], Rect::new(400.0, 400.0, 10.0, 10.0)];
        let s = snap_move(Rect::new(398.0, 600.0, 50.0, 50.0), &two, T);
        assert_eq!(s.rect.x, 400.0);
    }

    #[test]
    fn resizing_snaps_the_far_edges_and_matches_a_width() {
        // Right edge 297 → the other's right edge 300.
        let s = snap_resize(Rect::new(0.0, 300.0, 297.0, 50.0), &others(), T);
        assert_eq!(s.rect.w, 300.0);
        assert_eq!(s.rect.x, 0.0, "resizing never moves the block");
        // Width 203 → the other's width 200.
        let s = snap_resize(Rect::new(500.0, 300.0, 203.0, 50.0), &others(), T);
        assert_eq!(s.rect.w, 200.0);
        // Bottom edge 198 → the other's bottom 200.
        let s = snap_resize(Rect::new(500.0, 100.0, 50.0, 98.0), &others(), T);
        assert_eq!(s.rect.h, 100.0);
    }

    #[test]
    fn nothing_to_snap_to_changes_nothing() {
        let r = Rect::new(1.5, 2.5, 3.0, 4.0);
        assert_eq!(snap_move(r, &[], T).rect, r);
        assert_eq!(snap_resize(r, &[], T).rect, r);
    }
}
