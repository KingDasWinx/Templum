//! Checklist blocks: new checklists, opening one where it is, in an empty block or in a new one
//! beside, the field that edits a name or an item in place, and the changes to a checklist's
//! content, each kept for Ctrl+Z (`edit_checklist`). A block shows one checklist, and a checklist
//! is open in one block at most: opening it again shows that block.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext as _, Context, Entity, Focusable as _, Window};
use templum_canvas::{BlockId, Point, Rect};

use crate::checklist::{self, Checklist, Editing, Field};
use crate::checklists::Checklists;
use crate::notes;

use templum_mcp::control::Kind;

use super::remote::Deed;
use super::{Body, CHECKLIST_UNDO, Space, Workspace};

/// What the person does to a checklist block's items: on this computer an edit of
/// `Workspace::checklists` (one Ctrl+Z step each), on a mirrored workspace a call of home's.
pub(crate) enum ChecklistAct {
    /// The check box: checked, or unchecked.
    Toggle { item: u64 },
    /// A row's ×.
    Delete { item: u64 },
    /// An item's whole text (a mirror's field sends it once, on Enter or when it loses the
    /// keyboard).
    Edit { item: u64, text: String },
    /// New items at the end, a line each (a mirror's + Add an item).
    Add { text: String },
    /// An item dropped in the place of item `before` (None: at the end).
    Move { item: u64, before: Option<u64> },
    /// The checklist's whole name (a mirror's field sends it once, as an item's).
    Rename { name: String },
}

impl Workspace {
    /// A new checklist, its name being edited: in the checklist block `into` when it shows none,
    /// otherwise in a new block beside it, or at the viewport's center without one.
    pub(crate) fn new_checklist(
        &mut self,
        into: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.why_not(Deed::AddChecklist).is_some() {
            return;
        }
        if self.on_remote() {
            return self.remote_new_checklist(into, window, cx);
        }
        let id = self.checklists.add(notes::now());
        let block = self.place_checklist(id, into, window, cx);
        self.edit_field(block, Field::Name, window, cx);
    }

    /// A checklist block with a new checklist, its top-left corner at `at` (world units), its
    /// name being edited.
    pub(crate) fn add_checklist_at(
        &mut self,
        at: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.why_not(Deed::AddChecklist).is_some() {
            return;
        }
        if self.on_remote() {
            return self.remote_add(Kind::Checklist, at, cx);
        }
        let id = self.checklists.add(notes::now());
        let block = self.add_checklist_block(at, id, window, cx);
        self.edit_field(block, Field::Name, window, cx);
    }

    /// A checklist block of the default size at `at` showing checklist `list`, focused.
    fn add_checklist_block(
        &mut self,
        at: Point,
        list: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> BlockId {
        self.space.pristine = false;
        let rect = Rect::new(at.x, at.y, checklist::SIZE.w, checklist::SIZE.h);
        let checklist = Checklist {
            list: Some(list),
            editing: None,
            scroll: Default::default(),
            remote: false,
        };
        let id = self.add_block(rect, Body::Checklist(checklist));
        self.focus_block(id, window, cx);
        id
    }

    /// Shows checklist `id` in the checklist block `into` when it shows none; otherwise in a new
    /// block beside `into` (the camera goes there), or at the viewport's center without one. The
    /// block, focused.
    fn place_checklist(
        &mut self,
        id: u64,
        into: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> BlockId {
        if let Some(block) = into
            && let Some(checklist) = self.space.checklist_mut(block)
            && checklist.list.is_none()
        {
            checklist.list = Some(id);
            self.focus_block(block, window, cx);
            return block;
        }
        let (at, beside) = self.checklist_spot(into, window, cx);
        let block = self.add_checklist_block(at, id, window, cx);
        if beside {
            let space = self.space.id.clone();
            self.reveal(&space, block, window, cx);
        }
        block
    }

    /// Where a new checklist block goes: beside `into` (true), else in the view.
    fn checklist_spot(
        &mut self,
        into: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (Point, bool) {
        let size = checklist::SIZE;
        let beside = into.and_then(|b| self.space.canvas.get(b).map(|b| self.space.home(b)));
        match beside {
            Some(beside) => (self.space.room_beside(&[beside], size).origin(), true),
            None => (self.room_in_view(size, window, cx).origin(), false),
        }
    }

    /// New checklist on the mirrored workspace shown: home shows one in `into` when that shows
    /// none (`new_checklist`), else home adds a block with one beside `into`, or in the view.
    fn remote_new_checklist(
        &mut self,
        into: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let empty = into.filter(|block| {
            self.space
                .checklist_view(*block)
                .filter(|c| c.remote)
                .is_some_and(|c| {
                    c.list
                        .and_then(|id| self.checklists_of(c)?.get(id))
                        .is_none()
                })
        });
        let Some(block) = empty else {
            let (at, _) = self.checklist_spot(into, window, cx);
            return self.remote_add(Kind::Checklist, at, cx);
        };
        if let Some(words) = self.why_not(Deed::OpenChecklist) {
            return self.warn(words, window, cx);
        }
        let Some(home) = self.space.remote.as_deref().and_then(|r| r.home_of(block)) else {
            return;
        };
        self.focus_block(block, window, cx);
        let call = templum_api::client::Call {
            tool: "new_checklist".into(),
            args: serde_json::json!({ "block": home }),
            slow: false,
        };
        let then: super::remote::Then = Box::new(|ws, answered, window, cx| {
            if let Err(refusal) = answered {
                ws.warn(refusal.sentence, window, cx);
            }
        });
        let space = self.space.id.clone();
        self.remote_call(&space, call, then, cx);
    }

    /// The workspace and block where checklist `id` is open, if it is.
    pub(crate) fn checklist_open_at(&self, id: u64) -> Option<(&Space, BlockId)> {
        self.spaces().find_map(|space| {
            space.blocks.iter().find_map(|(block, body)| {
                body.checklist()
                    .is_some_and(|c| c.list == Some(id))
                    .then_some((space, *block))
            })
        })
    }

    /// Opens checklist `id`: where it is open, that block is shown; otherwise in the checklist
    /// block `into` when it shows none, in a new block beside it, or at the viewport's center
    /// without one.
    pub(crate) fn open_checklist(
        &mut self,
        id: u64,
        into: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.why_not(Deed::OpenChecklist).is_some() {
            return;
        }
        // Home's checklists are each open in a block of its: the one of the workspace shown is
        // shown. Nothing of this computer's opens there.
        if self.on_remote() {
            let shows = self.space.blocks.iter().find_map(|(block, body)| {
                body.checklist_view()
                    .is_some_and(|c| c.remote && c.list == Some(id))
                    .then_some(*block)
            });
            if let Some(block) = shows {
                let space = self.space.id.clone();
                self.reveal(&space, block, window, cx);
            }
            return;
        }
        if let Some((space, block)) = self.checklist_open_at(id) {
            let space = space.id.clone();
            self.reveal(&space, block, window, cx);
        } else if self.checklists.get(id).is_some() {
            self.place_checklist(id, into, window, cx);
        }
    }

    /// The Checklists dialog's delete: checklist `id` goes for good, and the block that shows it
    /// shows none.
    pub(crate) fn delete_checklist(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some((space, block)) = self.checklist_open_at(id) {
            let space = space.id.clone();
            if space == self.space.id {
                self.stop_editing(block, window, cx);
            }
            if let Some(checklist) = self
                .space_by_id_mut(&space)
                .and_then(|s| s.blocks.get_mut(&block))
                .and_then(Body::checklist_mut)
            {
                checklist.list = None;
                // A field of a hidden workspace's block goes with its checklist.
                checklist.editing = None;
            }
        }
        self.checklists.remove(id);
        cx.notify();
    }

    /// Checklist `id` is no more open: a blank one goes.
    pub(super) fn drop_checklist_if_blank(&mut self, id: u64) {
        if self.checklists.get(id).is_some_and(|l| l.is_blank()) {
            self.checklists.remove(id);
        }
    }

    /// Changes checklist `list` with `f`, keeping it as it was for Ctrl+Z (when it changed).
    pub(crate) fn edit_checklist<R>(
        &mut self,
        list: u64,
        f: impl FnOnce(&mut Checklists) -> R,
    ) -> R {
        let before = self.checklists.get(list).cloned();
        let result = f(&mut self.checklists);
        if let Some(before) = before
            && self.checklists.get(list) != Some(&before)
        {
            self.checklists_undo.push(before);
            if self.checklists_undo.len() > CHECKLIST_UNDO {
                self.checklists_undo.remove(0);
            }
        }
        result
    }

    /// Ctrl+Z on a checklist block: the last changed checklist as it was; a step whose checklist
    /// was deleted since is skipped. False: nothing to undo.
    pub(super) fn undo_checklists(&mut self, cx: &mut Context<Self>) -> bool {
        while let Some(before) = self.checklists_undo.pop() {
            // A step that would change nothing (its item left empty went since) is skipped too.
            if self
                .checklists
                .get(before.id)
                .is_some_and(|now| same(now, &before))
            {
                continue;
            }
            if self.checklists.restore(before) {
                cx.notify();
                return true;
            }
        }
        false
    }

    /// Edits `field` of the checklist on screen in `block` in place, with the keyboard; a field
    /// editing there stops first. Its every key is written to the checklist at once. On a
    /// mirrored workspace nothing is written: the field starts with home's text, and Enter or
    /// losing the keyboard sends it once (`remote_field`).
    pub(crate) fn edit_field(
        &mut self,
        block: BlockId,
        field: Field,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_editing(block, window, cx);
        let Some((list, remote)) = self
            .space
            .checklist_view(block)
            .and_then(|c| Some((c.list?, c.remote)))
        else {
            return;
        };
        let text = match (
            self.checklists_in(&self.space.id).and_then(|c| c.get(list)),
            field,
        ) {
            (Some(l), Field::Name) => l.name.clone(),
            (Some(l), Field::Item(item)) => match l.item(item) {
                Some(item) => item.text.clone(),
                None => return,
            },
            (Some(_), Field::New) => String::new(),
            (None, _) => return,
        };
        if remote {
            let deed = match field {
                Field::Name => Deed::EditChecklist,
                Field::Item(_) => Deed::EditItem,
                Field::New => Deed::AddItems,
            };
            if let Some(words) = self.why_not(deed) {
                return self.warn(words, window, cx);
            }
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(text));
        // A name is selected, to be typed over; an item's caret goes at its end.
        input.update(cx, |input, cx| match field {
            Field::Name => input.select_all(window, cx),
            Field::Item(_) | Field::New => {
                let end = input.value().len();
                input.set_selected_range(end..end, cx);
            }
        });
        // A mirrored field's events act on the workspace it was opened on, by id, whichever is
        // shown by the time they come.
        let space = self.space.id.clone();
        cx.subscribe_in(
            &input,
            window,
            move |ws, input, event: &InputEvent, window, cx| {
                if remote {
                    match event {
                        InputEvent::PressEnter { .. } => {
                            ws.remote_field(&space, block, input, true, window, cx)
                        }
                        InputEvent::Blur => {
                            ws.remote_field(&space, block, input, false, window, cx)
                        }
                        _ => {}
                    }
                    return;
                }
                match event {
                    InputEvent::Change => ws.field_changed(block, input, cx),
                    InputEvent::PressEnter { .. } => ws.field_enter(block, input, window, cx),
                    // A click elsewhere: on the canvas, another block, another row of this one.
                    InputEvent::Blur if ws.editing_with(block, input) => {
                        ws.stop_editing(block, window, cx)
                    }
                    _ => {}
                }
            },
        )
        .detach();
        self.focus_block(block, window, cx);
        if let Some(checklist) = self.space.checklist_view_mut(block) {
            checklist.editing = Some(Editing {
                field,
                input: input.clone(),
                kept: false,
            });
        }
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// Esc in a field: on this computer it stops, keeping what was typed; on a mirrored workspace
    /// it drops the field and sends nothing.
    pub(crate) fn escape_field(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.space.checklist_view(block).is_some_and(|c| c.remote) {
            let space = self.space.id.clone();
            self.close_remote_field(&space, block, None, window, cx);
        } else {
            self.stop_editing(block, window, cx);
        }
    }

    /// The field of the mirrored `space`'s block `block` ends: Enter (`enter`), or the keyboard
    /// leaving. What was typed goes home as `act_on_checklist` sends it; Enter in a new item's
    /// field then opens the next. An event of a field already ended does nothing.
    fn remote_field(
        &mut self,
        space: &str,
        block: BlockId,
        input: &Entity<InputState>,
        enter: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = self.close_remote_field(space, block, Some(input), window, cx) else {
            return;
        };
        let text = input.read(cx).value().to_string();
        let act = match field {
            Field::Item(item) => ChecklistAct::Edit { item, text },
            Field::New if !text.trim().is_empty() => ChecklistAct::Add { text },
            Field::Name => ChecklistAct::Rename { name: text },
            Field::New => return,
        };
        let again = enter && field == Field::New;
        self.act_on_checklist(space, block, act, window, cx);
        if again && self.space.id == space {
            self.edit_field(block, Field::New, window, cx);
        }
    }

    /// Takes the field out of the mirrored `space`'s block `block` (when it is `input`, or any
    /// without one) and gives the keyboard back to the canvas if it had it; the field it edited.
    fn close_remote_field(
        &mut self,
        space: &str,
        block: BlockId,
        input: Option<&Entity<InputState>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Field> {
        let shown = self.space.id == space;
        let editing = self
            .space_by_id_mut(space)?
            .checklist_view_mut(block)?
            .editing
            .take_if(|e| input.is_none_or(|input| &e.input == input))?;
        if shown && editing.input.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
        Some(editing.field)
    }

    /// `block`'s field is `input`.
    fn editing_with(&self, block: BlockId, input: &Entity<InputState>) -> bool {
        self.space
            .blocks
            .get(&block)
            .and_then(|b| b.checklist()?.editing.as_ref())
            .is_some_and(|e| &e.input == input)
    }

    /// A key typed in the field: written to the checklist, its first one as an undo step.
    fn field_changed(
        &mut self,
        block: BlockId,
        input: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) {
        let Some((list, editing)) = self
            .space
            .checklist_mut(block)
            .and_then(|c| c.list.zip(c.editing.as_mut()))
            .filter(|(_, e)| &e.input == input)
        else {
            return;
        };
        let (field, kept) = (editing.field, editing.kept);
        editing.kept = true;
        let text = input.read(cx).value().to_string();
        let write = |c: &mut Checklists| match field {
            Field::Name => c.rename(list, &text, notes::now()),
            Field::Item(item) => c.set_text(list, item, &text, notes::now()),
            // (A mirrored list's field writes nothing.)
            Field::New => {}
        };
        if kept {
            write(&mut self.checklists);
        } else {
            self.edit_checklist(list, write);
        }
        cx.notify();
    }

    /// Enter in the field: a name is kept; after an item with text, a new one comes, edited; an
    /// empty item goes.
    fn field_enter(
        &mut self,
        block: BlockId,
        input: &Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((list, editing)) = self
            .space
            .blocks
            .get(&block)
            .and_then(|b| b.checklist())
            .and_then(|c| c.list.zip(c.editing.as_ref()))
            .filter(|(_, e)| &e.input == input)
        else {
            return;
        };
        match editing.field {
            Field::Item(item) if !input.read(cx).value().trim().is_empty() => {
                self.add_item_after(block, list, Some(item), window, cx)
            }
            _ => self.stop_editing(block, window, cx),
        }
    }

    /// A new empty item in checklist `list` after `after` (at the end: None), edited; adding it
    /// is the undo step its typing goes with.
    pub(crate) fn add_item_after(
        &mut self,
        block: BlockId,
        list: u64,
        after: Option<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Home adds it, from the field's text.
        if self.space.checklist_view(block).is_some_and(|c| c.remote) {
            return self.edit_field(block, Field::New, window, cx);
        }
        self.stop_editing(block, window, cx);
        let Some(item) =
            self.edit_checklist(list, |c| c.add_item(list, after, "", false, notes::now()))
        else {
            return;
        };
        self.edit_field(block, Field::Item(item), window, cx);
        if let Some(editing) = self
            .space
            .checklist_mut(block)
            .and_then(|c| c.editing.as_mut())
        {
            editing.kept = true;
        }
    }

    /// Backspace at the field's start: an empty item goes, and the one before it is edited (the
    /// first item's: the name), its caret at the end. A held Backspace so stops in the name and
    /// never reaches the canvas, where it closes the block.
    pub(crate) fn backspace_at_start(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // (A mirrored list's block is not `checklist()`: its Backspace does nothing here.)
        let Some((list, editing)) = self
            .space
            .blocks
            .get(&block)
            .and_then(|b| b.checklist())
            .and_then(|c| c.list.zip(c.editing.as_ref()))
        else {
            return;
        };
        let (Field::Item(item), true) = (editing.field, editing.input.read(cx).value().is_empty())
        else {
            return;
        };
        let before = self.checklists.get(list).and_then(|l| {
            let at = l.items.iter().position(|i| i.id == item)?;
            Some(l.items.get(at.checked_sub(1)?)?.id)
        });
        self.stop_editing(block, window, cx);
        let Some(before) = before else {
            self.edit_field(block, Field::Name, window, cx);
            let input = self
                .space
                .blocks
                .get(&block)
                .and_then(|b| Some(b.checklist()?.editing.as_ref()?.input.clone()));
            if let Some(input) = input {
                input.update(cx, |input, cx| {
                    let end = input.value().len();
                    input.set_selected_range(end..end, cx);
                });
            }
            return;
        };
        self.edit_field(block, Field::Item(before), window, cx);
    }

    /// Stops `block`'s field: its text is kept, but an item left empty goes (and a new one leaves
    /// no undo step). With the keyboard in the field, the block keeps it.
    pub(crate) fn stop_editing(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A mirrored workspace's field sends what it has, as the keyboard leaving does.
        if self.space.checklist_view(block).is_some_and(|c| c.remote) {
            let Some(input) = self
                .space
                .checklist_view(block)
                .and_then(|c| Some(c.editing.as_ref()?.input.clone()))
            else {
                return;
            };
            let space = self.space.id.clone();
            return self.remote_field(&space, block, &input, false, window, cx);
        }
        let Some((list, editing)) = self
            .space
            .checklist_mut(block)
            .and_then(|c| c.list.zip(c.editing.take()))
        else {
            return;
        };
        if editing.input.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.focus_handle, cx);
        }
        let blank = |item: u64, c: &Checklists| {
            c.get(list)
                .and_then(|l| l.item(item))
                .is_some_and(|i| i.text.trim().is_empty())
        };
        if let Field::Item(item) = editing.field {
            // An item is empty in a step only while it was edited (an agent's change, the key
            // typed after it): there it goes, so undo never brings back an empty row. A step
            // where it still had text keeps it.
            for step in self.checklists_undo.iter_mut().filter(|s| s.id == list) {
                step.items
                    .retain(|i| i.id != item || !i.text.trim().is_empty());
            }
            if blank(item, &self.checklists) {
                self.checklists.remove_item(list, item, notes::now());
                // Back as it was before the step: the step was only that item.
                if let (Some(step), Some(now)) =
                    (self.checklists_undo.last(), self.checklists.get(list))
                    && same(step, now)
                    && let Some(step) = self.checklists_undo.pop()
                {
                    self.checklists.restore(step);
                }
            }
        }
        cx.notify();
    }

    /// An agent changed checklist `list`: what the person types next in its field is a step of its
    /// own, taken back before the agent's change.
    pub(super) fn agent_changed(&mut self, list: u64) {
        let Some((space, block)) = self.checklist_open_at(list) else {
            return;
        };
        let space = space.id.clone();
        if let Some(editing) = self
            .space_by_id_mut(&space)
            .and_then(|s| s.blocks.get_mut(&block))
            .and_then(Body::checklist_mut)
            .and_then(|c| c.editing.as_mut())
        {
            editing.kept = false;
        }
    }

    /// The check box: checks item `item` of checklist `list`, or unchecks it.
    pub(crate) fn toggle_item(&mut self, list: u64, item: u64, cx: &mut Context<Self>) {
        self.edit_checklist(list, |c| {
            let done = c
                .get(list)
                .and_then(|l| l.item(item))
                .is_some_and(|i| i.done);
            c.set_done(list, item, !done, notes::now())
        });
        cx.notify();
    }

    /// What the person does to the items of the checklist block `block` of workspace `space`: on
    /// this computer an edit of the checklists, one step of Ctrl+Z each; on a mirrored workspace
    /// home's tool, asked of the rule first (`remote_checklist_act`). The one door of the items'
    /// writes.
    pub(crate) fn act_on_checklist(
        &mut self,
        space: &str,
        block: BlockId,
        act: ChecklistAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(checklist) = self
            .space_named(space)
            .and_then(|s| s.blocks.get(&block))
            .and_then(Body::checklist_view)
        else {
            return;
        };
        if checklist.remote {
            return self.remote_checklist_act(space, block, act, window, cx);
        }
        let Some(list) = checklist.list else {
            return;
        };
        match act {
            ChecklistAct::Toggle { item } => self.toggle_item(list, item, cx),
            ChecklistAct::Delete { item } => self.delete_item(list, item, cx),
            ChecklistAct::Edit { item, text } => {
                self.item_edit(list, item, &text);
                cx.notify();
            }
            ChecklistAct::Add { text } => {
                let now = notes::now();
                self.edit_checklist(list, |c| {
                    for (text, done) in crate::checklists::parse_items(&text) {
                        c.add_item(list, None, &text, done, now);
                    }
                });
                self.agent_changed(list);
                cx.notify();
            }
            ChecklistAct::Move { item, before } => {
                self.edit_checklist(list, |c| c.move_item(list, item, before, notes::now()));
                cx.notify();
            }
            ChecklistAct::Rename { name } => {
                self.edit_checklist(list, |c| c.rename(list, &name, notes::now()));
                cx.notify();
            }
        }
    }

    /// A row's ×: deletes item `item` of checklist `list`.
    pub(crate) fn delete_item(&mut self, list: u64, item: u64, cx: &mut Context<Self>) {
        self.edit_checklist(list, |c| c.remove_item(list, item, notes::now()));
        cx.notify();
    }
}

/// Lists `a` and `b` are the same but for when they changed.
fn same(a: &crate::checklists::List, b: &crate::checklists::List) -> bool {
    a.id == b.id && a.name == b.name && a.items == b.items
}
