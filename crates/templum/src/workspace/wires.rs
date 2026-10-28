//! Wires between blocks (docs/specs/connections.md, Wires): wiring and unwiring them, each a
//! step of Ctrl+Z.

use std::time::Instant;

use gpui_kit::{Context, Hsla, MouseDownEvent, MouseUpEvent};
use templum_canvas::{BlockId, Curve, HIT_PX, Point, Rect, Wire};
use templum_mcp::Reply;

use super::history::{Change, Then};
use super::{Drag, Space, Workspace};

/// The longest a label is, in characters.
pub(crate) const LABEL_MAX: usize = templum_mcp::control::LABEL_MOST;

/// `text` as a label: on one line, at most LABEL_MAX characters; None when blank.
pub(super) fn tidy_label(text: &str) -> Option<String> {
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (!text.is_empty()).then(|| text.chars().take(LABEL_MAX).collect())
}

/// A label's key from its wire's ends' block ids, in either order.
pub(crate) fn label_key(a: &str, b: &str) -> (String, String) {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    (a.to_string(), b.to_string())
}

impl Space {
    /// The key of `wire`'s label; None while one of its blocks has no id.
    pub(crate) fn label_key(&self, wire: Wire) -> Option<(String, String)> {
        // A remote workspace's blocks are named by home's ids.
        let id = |b: BlockId| {
            self.uids
                .get(&b)
                .cloned()
                .or_else(|| self.remote.as_ref()?.home_of(b))
        };
        Some(label_key(&id(wire.a)?, &id(wire.b)?))
    }

    /// `wire`'s label, if it has one.
    pub(crate) fn label(&self, wire: Wire) -> Option<&str> {
        self.labels.get(&self.label_key(wire)?).map(String::as_str)
    }

    /// `a` and `b` are wired.
    pub(crate) fn wired(&self, a: BlockId, b: BlockId) -> bool {
        Wire::new(a, b).is_some_and(|w| self.wires.contains(&w))
    }

    /// Wires `a` and `b`, as no step of Ctrl+Z (a restored canvas, the bench). None for one
    /// block, two already wired, or a block not on this canvas.
    pub(crate) fn add_wire(&mut self, a: BlockId, b: BlockId) -> Option<Wire> {
        let wire = Wire::new(a, b)?;
        if self.wired(a, b) || self.canvas.get(a).is_none() || self.canvas.get(b).is_none() {
            return None;
        }
        self.wires.push(wire);
        Some(wire)
    }

    /// False when they weren't wired.
    pub(super) fn remove_wire(&mut self, wire: Wire) -> bool {
        let before = self.wires.len();
        self.wires.retain(|w| *w != wire);
        if self.selected_wire == Some(wire) {
            self.selected_wire = None;
        }
        self.wires.len() < before
    }

    /// Block `id` leaves the canvas: its wires go.
    pub(super) fn unwire(&mut self, id: BlockId) {
        self.wires.retain(|w| !w.touches(id));
        if self.selected_wire.is_some_and(|w| w.touches(id)) {
            self.selected_wire = None;
        }
    }

    /// The top block drawn at a world point.
    pub(super) fn block_at(&self, p: Point) -> Option<BlockId> {
        self.canvas
            .blocks()
            .iter()
            .rev()
            .find(|b| self.shown(b.id) && b.rect.contains(p))
            .map(|b| b.id)
    }

    /// `wire`'s curve between its blocks' rects, around the blocks in its way
    /// ([`Curve::around`]); None when one isn't on the canvas.
    pub(super) fn curve(&self, wire: Wire) -> Option<Curve> {
        let (a, b) = (self.canvas.get(wire.a)?.rect, self.canvas.get(wire.b)?.rect);
        let others = (self.canvas.blocks().iter())
            .filter(|o| o.id != wire.a && o.id != wire.b)
            .map(|o| o.rect);
        Some(Curve::around(a, b, others))
    }
}

impl Workspace {
    /// Wires `a` and `b`, a step Ctrl+Z takes back. False, silently, when they can't be: one
    /// block, already wired, or one isn't on the canvas.
    pub(crate) fn connect(&mut self, a: BlockId, b: BlockId, cx: &mut Context<Self>) -> bool {
        if self.on_remote() {
            return Wire::new(a, b).is_some_and(|wire| self.remote_wire(wire, true, cx));
        }
        self.set_wire(Wire { a, b }, true, Then::New, cx)
    }

    /// Rename wire…: `text` on one line, at most LABEL_MAX characters, becomes `wire`'s label;
    /// blank, it has none. A step Ctrl+Z takes back. False when the wire is gone or nothing
    /// changes.
    pub(crate) fn rename_wire(&mut self, wire: Wire, text: &str, cx: &mut Context<Self>) -> bool {
        if !self.space.wires.contains(&wire) {
            return false;
        }
        if self.on_remote() {
            return self.remote_label(wire, tidy_label(text), cx);
        }
        self.set_label(wire, tidy_label(text), Then::New, cx)
    }

    /// Gives `wire` `label` (None: none); the change that takes it back is kept as `then` says.
    /// False when nothing changes.
    pub(super) fn set_label(
        &mut self,
        wire: Wire,
        label: Option<String>,
        then: Then,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.on_remote() {
            return false;
        }
        let Some(key) = self.space.label_key(wire) else {
            return false;
        };
        let before = match label.clone() {
            Some(label) => self.space.labels.insert(key, label),
            None => self.space.labels.remove(&key),
        };
        if before == label {
            return false;
        }
        self.keep(Change::Label(wire, before), then);
        cx.notify();
        true
    }

    /// `templum wire <a> <b>` from the terminal `from`: wires two blocks of its workspace, a step
    /// Ctrl+Z takes back. A person's command, or a script's (an agent's shell's too): it asks no
    /// one.
    pub(super) fn wire_blocks(
        &mut self,
        from: &str,
        a: &str,
        b: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space_id, _) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let find = |uid: &str| {
            space.block_by_uid(uid).ok_or_else(|| {
                Reply::err(format!(
                    "There's no block {uid} in your workspace: templum blocks lists them."
                ))
            })
        };
        let (x, y) = match (find(a), find(b)) {
            (Ok(x), Ok(y)) => (x, y),
            (Err(refused), _) | (_, Err(refused)) => return refused,
        };
        if x == y {
            return Reply::err(format!("{a} can't be wired to itself."));
        }
        if space.wired(x, y) {
            return Reply::ok(format!("{a} and {b} are already connected."));
        }
        match self.in_space(&space_id, |ws| ws.connect(x, y, cx)) {
            Some(true) => Reply::ok(format!("Connected {a} and {b}.")),
            _ => Reply::err(format!("{a} and {b} can't be wired.")),
        }
    }

    /// Unwires them, a step Ctrl+Z takes back. False when they weren't wired.
    pub(crate) fn disconnect(&mut self, wire: Wire, cx: &mut Context<Self>) -> bool {
        if self.on_remote() {
            return self.remote_wire(wire, false, cx);
        }
        self.set_wire(wire, false, Then::New, cx)
    }

    /// The wires to paint under the blocks, with their colors: the title's dim one; and the one
    /// the Wire tool draws, in the accent, to the facing edge of the block under the pointer or
    /// to the pointer. Grid mode hides them.
    /// A wire's color: the title's dim one, the accent while it is selected.
    fn wire_color(&self, wire: Wire) -> Hsla {
        if self.space.selected_wire == Some(wire) {
            self.ui.accent
        } else {
            self.ui.title_dim
        }
    }

    /// The wires' labels to draw: each at its curve's middle (world units), in its wire's color.
    /// Grid mode hides them.
    pub(super) fn wire_labels(&self) -> Vec<(Point, String, Hsla)> {
        if self.space.grid.is_some() || self.space.labels.is_empty() {
            return Vec::new();
        }
        self.space
            .wires
            .iter()
            .filter_map(|w| {
                let label = self.space.label(*w)?;
                Some((
                    self.space.curve(*w)?.at(0.5),
                    label.to_string(),
                    self.wire_color(*w),
                ))
            })
            .collect()
    }

    pub(super) fn wire_curves(&self) -> Vec<(Curve, Hsla)> {
        if self.space.grid.is_some() {
            return Vec::new();
        }
        let mut curves: Vec<(Curve, Hsla)> = self
            .space
            .wires
            .iter()
            .filter_map(|w| {
                let color = self.wire_color(*w);
                Some((self.space.curve(*w)?, color))
            })
            .collect();
        if let Some(Drag::Wire { from, to }) = &self.drag
            && let Some(a) = self.space.canvas.get(*from)
        {
            let end = self
                .space
                .block_at(*to)
                .filter(|b| b != from)
                .and_then(|b| self.space.canvas.get(b))
                .map_or(Rect::new(to.x, to.y, 0.0, 0.0), |b| b.rect);
            curves.push((Curve::between(a.rect, end), self.ui.accent));
        }
        curves
    }

    /// The dots on the wires along which a task is handed over (docs/specs/connections.md, Wires):
    /// each on its way from the agent that handed the task over to the one working on it, a trip
    /// every DOT_TRIP; at the middle when `still` (reduced motion). None in grid mode, which hides
    /// wires, nor without a wire between the two.
    pub(super) fn wire_dots(&self, now: Instant, still: bool) -> Vec<Point> {
        if self.space.grid.is_some() {
            return Vec::new();
        }
        self.space
            .terminals()
            .filter_map(|(to, pane)| {
                let handed = pane.task(now)?;
                let (from, _) = self
                    .space
                    .terminals()
                    .find(|(_, p)| p.term.id() == handed.by)?;
                let wire = *self
                    .space
                    .wires
                    .iter()
                    .find(|w| **w == Wire { a: from, b: to })?;
                let phase = if still {
                    0.5
                } else {
                    let gone = now.saturating_duration_since(handed.since).as_secs_f32();
                    (gone / crate::wires::DOT_TRIP.as_secs_f32()) % 1.0
                };
                // The curve runs from the wire's `a` to its `b`.
                let t = if wire.a == from { phase } else { 1.0 - phase };
                Some(self.space.curve(wire)?.at(t))
            })
            .collect()
    }

    /// The wire under a world point, within a click's reach; the newest first.
    pub(super) fn wire_at(&self, world: Point) -> Option<Wire> {
        let reach = HIT_PX / self.space.canvas.camera.zoom() + crate::wires::WIDTH / 2.0;
        self.space
            .wires
            .iter()
            .rev()
            .copied()
            .find(|w| self.space.curve(*w).is_some_and(|c| c.hit(world, reach)))
    }

    /// A press with the Wire tool: on a block, a wire starts from it. No block keeps the
    /// keyboard, so Ctrl+Z takes the wire back rather than reaching a terminal.
    pub(super) fn begin_wire(&mut self, e: &MouseDownEvent) {
        self.space.pristine = false;
        self.press = None;
        let at = self.space.canvas.camera.to_world(self.local(e.position));
        self.drag = self
            .space
            .block_at(at)
            .map(|from| Drag::Wire { from, to: at });
        self.space.focused = None;
    }

    /// The Wire tool let go over the canvas: on another block, the two are wired. Let go
    /// anywhere else, `release` drops the drag.
    pub(super) fn end_wire(&mut self, e: &MouseUpEvent, cx: &mut Context<Self>) {
        if let Some(Drag::Wire { from, .. }) = self.drag.take() {
            let at = self.space.canvas.camera.to_world(self.local(e.position));
            if let Some(to) = self.space.block_at(at) {
                self.connect(from, to, cx);
            }
            cx.notify();
        }
    }

    /// Wires (`on`) or unwires `wire`; the change that takes it back is kept as `then` says.
    pub(super) fn set_wire(
        &mut self,
        wire: Wire,
        on: bool,
        then: Then,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.on_remote() {
            return false;
        }
        let done = if on {
            self.space.add_wire(wire.a, wire.b).is_some()
        } else {
            self.space.remove_wire(wire)
        };
        if done {
            let back = if on {
                Change::Disconnect(wire)
            } else {
                Change::Connect(wire)
            };
            self.keep(back, then);
            cx.notify();
        }
        done
    }
}
