//! Selecting blocks: a rectangle (Shift+drag on the empty canvas) or Shift+click on a title; and
//! what acts on the selected blocks, or on the focused one: a drag from a selected block or from
//! the box around them moves them together, Delete closes them, Copy and Paste copy them.

use gpui_kit::{ClipboardItem, Context, Pixels, Point as PixelPoint, Window};
use serde::{Deserialize, Serialize};
use templum_canvas::{BlockId, Gesture, Point, Rect};

use crate::session;

use super::history::{Change, Then};
use super::remote::Deed;
use super::{Drag, Workspace};

/// How far outside the selected blocks their box is drawn (world units).
pub(crate) const BOX_MARGIN: f32 = 8.0;

/// What Copy puts on the clipboard for blocks: JSON that Paste knows, here or in another
/// Templum. A terminal comes back as a fresh shell in its folder, without its agent.
#[derive(Serialize, Deserialize)]
struct Clip {
    templum_blocks: Vec<session::Item>,
}

impl Workspace {
    /// Shift+press on the empty canvas at `pointer` (a canvas point): a rectangle from there
    /// selects the blocks it touches, with those selected before. Selecting takes the focus away.
    pub(super) fn begin_marquee(&mut self, pointer: Point) {
        self.space.focused = None;
        let at = self.space.camera().to_world(pointer);
        self.drag = Some(Drag::Marquee {
            base: self.space.selected.clone(),
            from: at,
            to: at,
            pointer,
            scrolled: None,
        });
        self.press = None;
    }

    /// The rectangle's corner is under `pointer` (a canvas point).
    pub(super) fn marquee_to(&mut self, pointer: Point) {
        let to = self.space.camera().to_world(pointer);
        let Some(Drag::Marquee {
            base,
            from,
            to: at,
            pointer: under,
            ..
        }) = &mut self.drag
        else {
            return;
        };
        *at = to;
        *under = pointer;
        let mut selected = base.clone();
        for id in self.space.canvas.hits(Rect::corners(*from, to)) {
            if !selected.contains(&id) {
                selected.push(id);
            }
        }
        self.space.selected = selected;
    }

    /// The box drawn around two or more selected blocks (world units); a press in it moves them.
    pub(crate) fn selection_box(&self) -> Option<Rect> {
        self.space
            .canvas
            .selection_box(&self.space.selected, BOX_MARGIN)
    }

    /// A press on a selected block's title, or on its body (`picks` it, if let go in place), or on
    /// the empty canvas in the selection's box: dragging moves every selected block.
    pub(crate) fn begin_group_move(&mut self, pos: PixelPoint<Pixels>, picks: Option<BlockId>) {
        if self.space.grid.is_some() || self.why_not(Deed::Move).is_some() {
            return;
        }
        let p = self.local(pos);
        self.space.pristine = false;
        self.drag = Gesture::move_blocks(&self.space.canvas, &self.space.selected, p)
            .map(|g| Drag::held(g, p, picks));
        self.placing(&self.space.selected.clone());
    }

    /// Shift+click on a block's title: it joins the selection, or leaves it.
    pub(crate) fn toggle_selected(
        &mut self,
        id: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.space.focused = None;
        self.space.selected_ink = None;
        self.space.selected_wire = None;
        let selected = &mut self.space.selected;
        match selected.iter().position(|s| *s == id) {
            Some(i) => {
                selected.remove(i);
            }
            None => selected.push(id),
        }
        // Delete and Copy reach the workspace, not a field of a block.
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// The keyboard is on the canvas, not in a field: the workspace has it (a focused terminal's
    /// keys go through it).
    pub(super) fn canvas_has_keys(&self, window: &Window) -> bool {
        // On the Home its focus is the Home's.
        !self.home_up() && self.focus_handle.is_focused(window)
    }

    /// Copies the selected blocks to the clipboard. False when none is selected.
    pub(super) fn copy_blocks(&mut self, cx: &mut Context<Self>) -> bool {
        if self.why_not(Deed::CopyBlocks).is_some() {
            return false;
        }
        let items: Vec<_> = self
            .space
            .canvas
            .blocks()
            .iter()
            .filter(|b| self.space.selected.contains(&b.id))
            .map(|b| self.space.blocks[&b.id].item(self.space.home(b)))
            .collect();
        if items.is_empty() {
            return false;
        }
        let clip = Clip {
            templum_blocks: items,
        };
        if let Ok(text) = serde_json::to_string(&clip) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        true
    }

    /// Pastes the blocks on the clipboard, centered in the view (or beside the blocks already
    /// there), and selects them. False when the clipboard holds no blocks.
    pub(super) fn paste_blocks(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.why_not(Deed::CopyBlocks).is_some() {
            return false;
        }
        let Some(clip) = cx
            .read_from_clipboard()
            .and_then(|item| item.text())
            .and_then(|text| serde_json::from_str::<Clip>(&text).ok())
        else {
            return false;
        };
        let mut items = clip.templum_blocks;
        let Some(bounds) = items
            .iter_mut()
            .map(|item| Rect::from(*item.rect_mut()))
            .reduce(|a, b| a.union(&b))
        else {
            return false;
        };
        let (dx, dy) = self.landing(bounds, window);
        let landed = Rect::new(bounds.x + dx, bounds.y + dy, bounds.w, bounds.h);
        self.show(landed, window, cx);
        for item in &mut items {
            let rect = item.rect_mut();
            rect.x += dx;
            rect.y += dy;
            match item {
                // A second agent resuming the same conversation would fight over it.
                // Opened by the person; a model needs its agent.
                session::Item::Terminal(t) => {
                    t.agent = None;
                    t.opened_by = None;
                }
                // A note is open in one tab at most: the copy shows copies.
                session::Item::Note(n) => {
                    let now = crate::notes::now();
                    n.notes = n
                        .notes
                        .iter()
                        .filter_map(|id| self.notes.duplicate(*id, now))
                        .collect();
                }
                // So is a checklist.
                session::Item::Checklist(c) => {
                    let now = crate::notes::now();
                    c.checklist = c
                        .checklist
                        .and_then(|id| self.checklists.duplicate(id, now));
                }
                _ => {}
            }
        }
        let pasted: Vec<_> = self
            .add_selected(&items, window, cx)
            .into_iter()
            .flatten()
            .collect();
        if !pasted.is_empty() {
            self.keep(Change::Close(pasted), Then::New);
        }
        true
    }

    /// Where blocks within `bounds` go to land in the view: centered, or beside the blocks
    /// already there, level with one; the offset to move them by.
    pub(super) fn landing(&self, bounds: Rect, window: &Window) -> (f32, f32) {
        let center = self.space.camera().to_world(self.center(window));
        let at = self.space.room_around(center, bounds.size());
        (at.x - bounds.x, at.y - bounds.y)
    }

    /// Adds the blocks `items`, selected, with the keyboard on the canvas: what each one became
    /// (None: it couldn't come back). Ctrl+Z is the caller's to keep.
    pub(super) fn add_selected(
        &mut self,
        items: &[session::Item],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Option<BlockId>> {
        self.space.pristine = false;
        self.quiet = true;
        let added: Vec<_> = items
            .iter()
            .map(|item| self.add_saved(item, window, cx))
            .collect();
        self.quiet = false;
        self.space.focused = None;
        self.space.selected = added.iter().flatten().copied().collect();
        window.focus(&self.focus_handle, cx);
        cx.notify();
        added
    }

    /// Delete or Backspace on the canvas: closes the selected blocks, else the focused one. A
    /// focused terminal keeps the key (it deletes a character): false.
    pub(super) fn delete_blocks(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let ids: Vec<BlockId> = if self.space.selected.is_empty() {
            match self.space.focused {
                Some(id) if self.space.terminal(id).is_none() => vec![id],
                _ => return false,
            }
        } else {
            self.space
                .canvas
                .blocks()
                .iter()
                .map(|b| b.id)
                .filter(|id| self.space.selected.contains(id))
                .collect()
        };
        self.close_blocks(ids, Then::New, window, cx);
        true
    }
}
