//! Undo and redo of the canvas's blocks (Ctrl+Z, Ctrl+Shift+Z): blocks added, closed, moved and
//! resized. What is inside a block (a shell, a note's text, a board's cards) keeps its own undo.

use gpui_kit::{Context, Window};
use templum_canvas::{BlockId, Rect, Wire};

use crate::session;

use super::{CANVAS_UNDO, Workspace};

/// A change to the canvas's blocks, kept to take back another: applying one gives the change
/// that takes it back in turn.
#[derive(Clone, Debug)]
pub(crate) enum Change {
    /// Close these blocks (they were added).
    Close(Vec<BlockId>),
    /// Bring these blocks back as they were when closed, by the ids they had, with the wires they
    /// had.
    Open(Vec<(BlockId, session::Item)>, Vec<Wire>),
    /// Put these blocks at these rects (they were moved or resized).
    Place(Vec<(BlockId, Rect)>),
    /// Wire these blocks (they were unwired).
    Connect(Wire),
    /// Unwire them (they were wired).
    Disconnect(Wire),
    /// Give this wire this label, or none (it had another).
    Label(Wire, Option<String>),
}

impl Change {
    fn ids(&mut self) -> Vec<&mut BlockId> {
        match self {
            Change::Close(ids) => ids.iter_mut().collect(),
            Change::Open(items, wires) => items
                .iter_mut()
                .map(|(id, _)| id)
                .chain(wires.iter_mut().flat_map(|Wire { a, b }| [a, b]))
                .collect(),
            Change::Place(rects) => rects.iter_mut().map(|(id, _)| id).collect(),
            Change::Connect(Wire { a, b })
            | Change::Disconnect(Wire { a, b })
            | Change::Label(Wire { a, b }, _) => vec![a, b],
        }
    }
}

/// What a change applied now is: where the change that takes it back goes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Then {
    /// The person's: Ctrl+Z takes it back, and what was undone can't be redone anymore.
    New,
    /// The undo of step `n`: Ctrl+Shift+Z does it again.
    Undo(u64),
    /// The redo of step `n`: Ctrl+Z takes it back.
    Redo(u64),
}

impl Workspace {
    /// Keeps `change`, which takes back what was just done.
    pub(super) fn keep(&mut self, change: Change, then: Then) {
        self.space.steps += 1;
        let step = (self.space.steps, change);
        let stack = match then {
            Then::New => {
                self.space.redo.clear();
                &mut self.space.undo
            }
            Then::Undo(_) => &mut self.space.redo,
            Then::Redo(_) => &mut self.space.undo,
        };
        stack.push(step);
        if stack.len() > CANVAS_UNDO {
            stack.remove(0);
        }
    }

    /// Takes the step an undo or a redo applies off its stack. False when it isn't next anymore
    /// (the person did something else while a question was open): nothing is done then.
    pub(super) fn take_step(&mut self, then: Then) -> bool {
        let (stack, n) = match then {
            Then::New => return true,
            Then::Undo(n) => (&mut self.space.undo, n),
            Then::Redo(n) => (&mut self.space.redo, n),
        };
        if stack.last().is_some_and(|(m, _)| *m == n) {
            stack.pop();
            true
        } else {
            false
        }
    }

    /// Ctrl+Z, or Ctrl+Shift+Z (`redo`), on the canvas. False when there is nothing to do.
    pub(super) fn undo_canvas(
        &mut self,
        redo: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        loop {
            let stack = if redo {
                &self.space.redo
            } else {
                &self.space.undo
            };
            let Some((n, mut change)) = stack.last().cloned() else {
                return false;
            };
            let then = if redo { Then::Redo(n) } else { Then::Undo(n) };
            // A step whose blocks are all gone (a shell that exited) has nothing to take back; nor
            // has a wire's when one of its blocks is.
            let wire = matches!(
                change,
                Change::Connect(_) | Change::Disconnect(_) | Change::Label(..)
            );
            let missing = |id: &&mut BlockId| self.space.canvas.get(**id).is_none();
            let gone = !matches!(change, Change::Open(..))
                && if wire {
                    change.ids().iter().any(missing)
                } else {
                    change.ids().iter().all(missing)
                };
            if gone {
                self.take_step(then);
                continue;
            }
            match change {
                // It may ask first (a terminal at work, unsaved files).
                Change::Close(ids) => self.close_blocks(ids, then, window, cx),
                Change::Open(items, wires) => {
                    self.take_step(then);
                    self.reopen(items, wires, then, window, cx);
                }
                Change::Place(rects) => {
                    self.take_step(then);
                    self.place(rects, then, cx);
                }
                Change::Connect(wire) => {
                    self.take_step(then);
                    self.set_wire(wire, true, then, cx);
                }
                Change::Disconnect(wire) => {
                    self.take_step(then);
                    self.set_wire(wire, false, then, cx);
                }
                Change::Label(wire, label) => {
                    self.take_step(then);
                    self.set_label(wire, label, then, cx);
                }
            }
            return true;
        }
    }

    /// Brings closed blocks back where they were, selected (a terminal as a shell in its folder,
    /// resuming its agent), with their wires to the blocks still there. The steps that named them,
    /// and their wires, name them by their new ids.
    fn reopen(
        &mut self,
        items: Vec<(BlockId, session::Item)>,
        mut wires: Vec<Wire>,
        then: Then,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (old, items): (Vec<_>, Vec<_>) = items.into_iter().unzip();
        let added = self.add_selected(&items, window, cx);
        for (item, new) in items.iter().zip(&added) {
            if let Some(new) = new {
                self.adopt_uid(*new, item.uid());
            }
        }
        let mut ids = Vec::new();
        for (old, new) in old.into_iter().zip(added) {
            let Some(new) = new else { continue };
            let steps = self.space.undo.iter_mut().chain(&mut self.space.redo);
            let ends = wires.iter_mut().flat_map(|Wire { a, b }| [a, b]);
            for id in steps.flat_map(|(_, change)| change.ids()).chain(ends) {
                if *id == old {
                    *id = new;
                }
            }
            ids.push(new);
        }
        // A wire whose other block is closed too, or gone, stays out.
        for wire in wires {
            self.space.add_wire(wire.a, wire.b);
        }
        if !ids.is_empty() {
            self.keep(Change::Close(ids), then);
        }
    }

    /// Puts blocks at `rects`; a terminal's grid follows.
    fn place(&mut self, rects: Vec<(BlockId, Rect)>, then: Then, cx: &mut Context<Self>) {
        let mut before = Vec::new();
        for (id, rect) in rects {
            // A tile's rect is the grid's: its place on the canvas is what moves.
            if let Some(home) = self.space.grid.as_mut().and_then(|g| g.homes.get_mut(&id)) {
                before.push((id, std::mem::replace(home, rect)));
                continue;
            }
            let Some(b) = self.space.canvas.get_mut(id) else {
                continue;
            };
            before.push((id, b.rect));
            b.rect = rect;
            self.fit_terminal(id, rect);
        }
        self.keep(Change::Place(before), then);
        cx.notify();
    }

    /// A drag starts moving or resizing the blocks `ids`: their rects now are what Ctrl+Z puts
    /// back once it ends.
    pub(super) fn placing(&mut self, ids: &[BlockId]) {
        self.placed = ids
            .iter()
            .filter_map(|id| Some((*id, self.space.canvas.get(*id)?.rect)))
            .collect();
    }

    /// The drag has moved or resized something since `placing`.
    pub(super) fn displaced(&self) -> bool {
        self.placed
            .iter()
            .any(|(id, r)| self.space.canvas.get(*id).is_some_and(|b| b.rect != *r))
    }

    /// The blocks `placing` took go back where they were, and the drag will keep no step: a click
    /// that nudged them.
    pub(super) fn unplace(&mut self) {
        for (id, rect) in &self.placed {
            if let Some(b) = self.space.canvas.get_mut(*id) {
                b.rect = *rect;
            }
        }
    }

    /// The drag ended: if it moved or resized anything, it is a step.
    pub(super) fn end_placing(&mut self) {
        let moved = self.displaced();
        let placed = std::mem::take(&mut self.placed);
        if moved {
            self.keep(Change::Place(placed), Then::New);
        }
    }
}
