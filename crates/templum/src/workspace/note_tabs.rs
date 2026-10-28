//! Note blocks' tabs: new notes, opening one where it is or in a block, closing a tab, and
//! deleting a note for good (the Notes dialog). A note is open in one tab at most: opening it
//! again shows that tab. Every change to a note's text is written to `Workspace::notes` as it is
//! typed, and saved with the canvas.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{AppContext as _, Context, Entity, Focusable as _, Window};
use templum_canvas::{BlockId, Point, Rect};
use templum_mcp::control::Kind;

use templum_agent::Target;
use templum_api::model::NOTE_TEXT_MOST;
use templum_api::registry::{NoteInto, Own};
use templum_mcp::Reply;

use crate::board::Label;
use crate::note::{self, Note, NoteTab, Renaming};
use crate::notes;

use super::api::{Refusal, not_found};
use super::ops::{NewBlock, Place};
use super::remote::Deed;
use super::remote_notes::NoteAct;
use super::{Body, Space, Workspace};

impl Workspace {
    /// A note block at the viewport's center with a new note, its editor with the keyboard.
    pub(crate) fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.why_not(Deed::AddNote).is_some() {
            return;
        }
        let at = self.room_in_view(note::SIZE, window, cx).origin();
        self.add_note_at(at, window, cx);
    }

    /// A note block with a new note, its top-left corner at `at` (world units), its editor with
    /// the keyboard.
    pub(crate) fn add_note_at(&mut self, at: Point, window: &mut Window, cx: &mut Context<Self>) {
        if self.why_not(Deed::AddNote).is_some() {
            return;
        }
        if self.on_remote() {
            return self.remote_add(Kind::Note, at, cx);
        }
        let id = self.notes.add(String::new(), None, notes::now());
        let tab = self.note_tab(id, window, cx);
        self.add_note_block(at, tab.into_iter().collect(), window, cx);
    }

    /// A note block of the default size at `at` showing `tabs`, focused.
    fn add_note_block(
        &mut self,
        at: Point,
        tabs: Vec<NoteTab>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.space.pristine = false;
        let rect = Rect::new(at.x, at.y, note::SIZE.w, note::SIZE.h);
        let block = Body::Note(Note {
            tabs,
            ..Default::default()
        });
        if !block.focus_inner(window, cx) {
            window.focus(&self.focus_handle, cx);
        }
        self.add_block(rect, block);
        cx.notify();
    }

    /// A tab for note `id`: an editor on its text, whose every change is written to the notes.
    /// None when there is no such note.
    pub(super) fn note_tab(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<NoteTab> {
        let text = self.notes.get(id)?.text.clone();
        // Shown now: the mark of the workspace it was last closed in must not outlive its tab
        // (a workspace deleted with the tab open would hand the note to another one's share).
        self.notes.opened(id);
        let editor = note::editor(&text, window, cx);
        cx.subscribe(&editor, move |ws, editor, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let text = editor.read(cx).value().to_string();
                ws.notes.set_text(id, &text, notes::now());
            }
        })
        .detach();
        Some(NoteTab {
            id,
            editor,
            far: Default::default(),
        })
    }

    /// The workspace and block where note `id` is open, if it is.
    pub(crate) fn note_open_at(&self, id: u64) -> Option<(&Space, BlockId)> {
        self.spaces().find_map(|space| {
            space.blocks.iter().find_map(|(block, body)| {
                body.note()
                    .is_some_and(|n| n.tabs.iter().any(|t| t.id == id))
                    .then_some((space, *block))
            })
        })
    }

    /// Opens note `id`: where it is open, that tab is shown; otherwise as a new tab of the note
    /// block `into`, or in a new note block at the view's center without one.
    pub(crate) fn open_note(
        &mut self,
        id: u64,
        into: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // (A mirrored workspace's notes are home's, opened by `open_note_here`: this computer's
        // note of that id is another.)
        if self.on_remote() || self.why_not(Deed::OpenNote).is_some() {
            return;
        }
        if let Some((space, block)) = self.note_open_at(id) {
            let space = space.id.clone();
            if let Some(note) = self
                .space_by_id_mut(&space)
                .and_then(|s| s.blocks.get_mut(&block))
                .and_then(Body::note_mut)
            {
                note.active = note.tabs.iter().position(|t| t.id == id).unwrap_or(0);
            }
            self.reveal(&space, block, window, cx);
            return;
        }
        let Some(tab) = self.note_tab(id, window, cx) else {
            return;
        };
        match into.filter(|b| self.space.blocks.get(b).is_some_and(|b| b.note().is_some())) {
            Some(block) => self.add_tab(block, tab, window, cx),
            None => {
                let at = self.room_in_view(note::SIZE, window, cx).origin();
                self.add_note_block(at, vec![tab], window, cx);
            }
        }
    }

    /// `tab` after the block's last one, shown, with the keyboard.
    fn add_tab(
        &mut self,
        block: BlockId,
        tab: NoteTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.release_focus(block, window, cx);
        if let Some(note) = self.space.blocks.get_mut(&block).and_then(Body::note_mut) {
            note.tabs.push(tab);
            note.active = note.tabs.len() - 1;
        }
        self.focus_block(block, window, cx);
    }

    /// The tab strip's +: a new note in a new tab of `block`.
    pub(crate) fn new_note_tab(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A mirrored block's notes are home's: home makes the new one.
        if self.on_remote() {
            let space = self.space.id.clone();
            return self.remote_note_act(&space, Some(block), NoteAct::New, window, cx);
        }
        let id = self.notes.add(String::new(), None, notes::now());
        if let Some(tab) = self.note_tab(id, window, cx) {
            self.add_tab(block, tab, window, cx);
        }
    }

    /// Shows the block's tab `index`, with the keyboard.
    pub(crate) fn show_note_tab(
        &mut self,
        block: BlockId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.release_focus(block, window, cx);
        // (A mirrored note's shown tab is the mirror's own choice.)
        if let Some(note) = self.space.note_view_mut(block)
            && index < note.tabs.len()
        {
            note.active = index;
        }
        self.focus_block(block, window, cx);
    }

    /// Closes the block's tab `index`. The note stays in the Notes dialog, unless it is blank:
    /// nothing is lost then.
    pub(crate) fn close_note_tab(
        &mut self,
        block: BlockId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.on_remote() {
            let note = self
                .space
                .blocks
                .get(&block)
                .and_then(Body::note_view)
                .and_then(|n| n.tabs.get(index))
                .map(|t| t.id);
            let space = self.space.id.clone();
            if let Some(note) = note {
                self.remote_note_act(&space, Some(block), NoteAct::Close { note }, window, cx);
            }
            return;
        }
        self.release_focus(block, window, cx);
        let Some(note) = self.space.blocks.get_mut(&block).and_then(Body::note_mut) else {
            return;
        };
        if index >= note.tabs.len() {
            return;
        }
        let tab = note.remove_tab(index);
        self.note_closed(tab.id);
        self.focus_block(block, window, cx);
    }

    /// Note `id` is no more open in this workspace: a blank one goes (no text, and never named);
    /// any other is remembered as last open here, so a share can list and reopen it.
    pub(super) fn note_closed(&mut self, id: u64) {
        if self.notes.get(id).is_some_and(notes::Entry::is_blank) {
            self.notes.remove(id);
        } else {
            self.notes.close_in(id, &self.space.id);
        }
    }

    /// The Notes dialog's delete: note `id` goes for good, from every block that shows it.
    pub(crate) fn delete_note(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some((space, block)) = self.note_open_at(id) {
            let space = space.id.clone();
            if space == self.space.id {
                self.release_focus(block, window, cx);
            }
            if let Some(note) = self
                .space_by_id_mut(&space)
                .and_then(|s| s.blocks.get_mut(&block))
                .and_then(Body::note_mut)
                && let Some(index) = note.tabs.iter().position(|t| t.id == id)
            {
                note.remove_tab(index);
            }
        }
        self.notes.remove(id);
        cx.notify();
    }

    /// A double click on the block's tab `index`: its name is selected in the tab, to be typed over
    /// with the keyboard. Enter keeps it, and so does the keyboard leaving; Esc drops it.
    pub(crate) fn rename_note_tab(
        &mut self,
        block: BlockId,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_note_rename(block, true, window, cx);
        let Some(id) = self
            .space
            .blocks
            .get(&block)
            .and_then(|b| b.note_view()?.tabs.get(index))
            .map(|t| t.id)
        else {
            return;
        };
        // A mirrored note's rename is home's: the rule asks first, and the field is not opened
        // when it would do nothing.
        if self.why_not(Deed::EditNote).is_some() {
            return;
        }
        let name = self
            .notes_in(&self.space.id)
            .map_or(notes::UNTITLED, |notes| notes.name(id))
            .to_string();
        let space = self.space.id.clone();
        let input = cx.new(|cx| InputState::new(window, cx).default_value(name));
        input.update(cx, |input, cx| input.select_all(window, cx));
        cx.subscribe_in(
            &input,
            window,
            move |ws, input, event: &InputEvent, window, cx| match event {
                // Enter, or a click elsewhere, while the workspace it began on is shown.
                InputEvent::PressEnter { .. } | InputEvent::Blur
                    if ws.space.id == space && ws.renaming_with(block, input) =>
                {
                    ws.stop_note_rename(block, true, window, cx)
                }
                _ => {}
            },
        )
        .detach();
        if let Some(note) = self
            .space
            .blocks
            .get_mut(&block)
            .and_then(Body::note_view_mut)
        {
            note.active = index;
            note.renaming = Some(Renaming {
                id,
                input: input.clone(),
            });
        }
        self.focus_block(block, window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    /// `block`'s tab is being renamed in `input`.
    fn renaming_with(&self, block: BlockId, input: &Entity<InputState>) -> bool {
        self.space
            .blocks
            .get(&block)
            .and_then(|b| b.note_view()?.renaming.as_ref())
            .is_some_and(|r| &r.input == input)
    }

    /// Ends the rename of `block`'s tab: the name typed is kept, or with `keep` false dropped. The
    /// note takes the keyboard back from the field.
    pub(crate) fn stop_note_rename(
        &mut self,
        block: BlockId,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(renaming) = self
            .space
            .blocks
            .get_mut(&block)
            .and_then(Body::note_view_mut)
            .and_then(|n| n.renaming.take())
        else {
            return;
        };
        if keep {
            let name = renaming.input.read(cx).value().to_string();
            if self.on_remote() {
                let space = self.space.id.clone();
                let act = NoteAct::Rename {
                    note: renaming.id,
                    name,
                };
                self.remote_note_act(&space, Some(block), act, window, cx);
            } else {
                self.notes.rename(renaming.id, &name);
            }
        }
        if renaming.input.read(cx).focus_handle(cx).is_focused(window) {
            self.focus_block(block, window, cx);
        }
        cx.notify();
    }

    /// Esc in the tab's field: the old name stays.
    pub(crate) fn cancel_note_rename(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.stop_note_rename(block, false, window, cx);
    }

    /// The menu's Rename: note `id` of the block `block` is called `name`; a mirrored note's
    /// rename is home's.
    pub(crate) fn rename_note(
        &mut self,
        id: u64,
        block: BlockId,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.on_remote() {
            let space = self.space.id.clone();
            let act = NoteAct::Rename {
                note: id,
                name: name.to_string(),
            };
            return self.remote_note_act(&space, Some(block), act, window, cx);
        }
        self.notes.rename(id, name);
        cx.notify();
    }

    /// Tints the note on screen in `block` with `color` (None: the blocks' own); a mirrored
    /// note's color is home's.
    pub(crate) fn set_note_color(
        &mut self,
        block: BlockId,
        color: Option<Label>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self
            .space
            .blocks
            .get(&block)
            .and_then(|b| b.note_view()?.tab())
            .map(|t| t.id)
        else {
            return;
        };
        if self.on_remote() {
            let space = self.space.id.clone();
            let act = NoteAct::Color { note: id, color };
            return self.remote_note_act(&space, Some(block), act, window, cx);
        }
        self.notes.set_color(id, color);
        cx.notify();
    }

    /// The note block `uid` names: its workspace's id and its id. Err: the words of a call that
    /// can't have it (a mirrored workspace's blocks are home's), or the block not being there.
    fn note_block_named(&self, uid: &str) -> Result<(String, BlockId), Result<Reply, Refusal>> {
        let (space, block) = self
            .find_block(uid)
            .ok_or_else(|| Err(not_found("block", uid)))?;
        if let Some(words) = self.space_named(&space).and_then(Space::mirror_words) {
            return Err(Ok(Reply::err(words)));
        }
        let is_note = self
            .space_named(&space)
            .and_then(|s| s.blocks.get(&block))
            .and_then(Body::note)
            .is_some();
        if !is_note {
            return Err(Ok(Reply::err(format!("{uid} isn't a note block."))));
        }
        Ok((space, block))
    }

    /// The API's five note tools (docs/api.md): the person's acts on notes from another computer.
    /// None of them moves this computer's keyboard, selection or camera. The call's block, and for
    /// `open_note` and `delete_note` the note, were checked against a limited token already
    /// (`api_allows`).
    pub(super) fn api_note(
        &mut self,
        own: Own,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(Reply, Option<Target>), Refusal> {
        let uid_of = |ws: &Self, space: &str, block: BlockId| {
            ws.space_named(space)
                .and_then(|s| s.uids.get(&block).cloned())
                .unwrap_or_default()
        };
        let at = |space: String, uid: String| {
            Some(Target {
                workspace: Some(space),
                block: Some(uid),
            })
        };
        match own {
            Own::EditNote {
                block,
                note,
                name,
                color,
                text,
            } => {
                let (space, id) = match self.note_block_named(&block) {
                    Ok(found) => found,
                    Err(reply) => return reply.map(|r| (r, None)),
                };
                let reply = self
                    .in_space(&space, |ws| {
                        ws.note_edit(id, &block, note, name, color, text, window, cx)
                    })
                    .unwrap_or_else(|| Reply::err("That workspace is gone."));
                Ok((reply, at(space, block)))
            }
            Own::NewNote { block, name, text } => {
                let (space, id) = match self.note_block_named(&block) {
                    Ok(found) => found,
                    Err(reply) => return reply.map(|r| (r, None)),
                };
                let reply = self
                    .in_space(&space, |ws| {
                        let note = ws.notes.add(text.unwrap_or_default(), None, notes::now());
                        if let Some(name) = name {
                            ws.notes.rename(note, &name);
                        }
                        match ws.note_tab(note, window, cx) {
                            Some(tab) => {
                                ws.add_tab_quietly(id, tab);
                                Reply::ok(format!("Made note {note} in {block}."))
                            }
                            None => Reply::err("The note couldn't be made."),
                        }
                    })
                    .unwrap_or_else(|| Reply::err("That workspace is gone."));
                cx.notify();
                Ok((reply, at(space, block)))
            }
            Own::OpenNote { note, into } => {
                if self.notes.get(note).is_none() {
                    return Err(not_found("note", &note.to_string()));
                }
                // Where it is open, that tab is shown and nothing else changes.
                if let Some((space, block)) =
                    self.note_open_at(note).map(|(s, b)| (s.id.clone(), b))
                {
                    let uid = uid_of(self, &space, block);
                    if let Some(shown) = self
                        .space_by_id_mut(&space)
                        .and_then(|s| s.blocks.get_mut(&block))
                        .and_then(Body::note_mut)
                    {
                        shown.active = shown.tabs.iter().position(|t| t.id == note).unwrap_or(0);
                    }
                    cx.notify();
                    return Ok((
                        Reply::ok(format!("Opened note {note} in {uid}.")),
                        at(space, uid),
                    ));
                }
                match into {
                    NoteInto::Block(block) => {
                        let (space, id) = match self.note_block_named(&block) {
                            Ok(found) => found,
                            Err(reply) => return reply.map(|r| (r, None)),
                        };
                        let reply = self
                            .in_space(&space, |ws| match ws.note_tab(note, window, cx) {
                                Some(tab) => {
                                    ws.add_tab_quietly(id, tab);
                                    Reply::ok(format!("Opened note {note} in {block}."))
                                }
                                None => Reply::err("The note is gone."),
                            })
                            .unwrap_or_else(|| Reply::err("That workspace is gone."));
                        cx.notify();
                        Ok((reply, at(space, block)))
                    }
                    NoteInto::Workspace {
                        workspace,
                        at: spot,
                    } => {
                        let space = match self.space_of(&workspace, true) {
                            Ok(space) => space,
                            Err(reply) => return Ok((reply, None)),
                        };
                        let place = match spot {
                            Some((x, y)) => Place::At(Point::new(x, y)),
                            None => Place::Free,
                        };
                        let new = NewBlock::Note {
                            text: String::new(),
                            open: Some(note),
                        };
                        match self.add_block_in(&space, new, place, None, window, cx) {
                            Ok(block) => {
                                let uid = uid_of(self, &space, block);
                                let reply = Reply::ok(format!("Opened note {note} in {uid}."));
                                Ok((reply, at(space, uid)))
                            }
                            Err(why) => Ok((Reply::err(why), None)),
                        }
                    }
                }
            }
            Own::CloseNote { block, note } => {
                let (space, id) = match self.note_block_named(&block) {
                    Ok(found) => found,
                    Err(reply) => return reply.map(|r| (r, None)),
                };
                let reply = self
                    .in_space(&space, |ws| ws.note_close(id, &block, note, window, cx))
                    .unwrap_or_else(|| Reply::err("That workspace is gone."));
                cx.notify();
                Ok((reply, at(space, block)))
            }
            Own::DeleteNote { note } => {
                let Some(name) = self.notes.get(note).map(|n| n.name.clone()) else {
                    return Err(not_found("note", &note.to_string()));
                };
                self.delete_note(note, window, cx);
                Ok((Reply::ok(format!("Deleted note {note} \"{name}\".")), None))
            }
            _ => Err(not_found("tool", "")),
        }
    }

    /// `tab` after the block's last one, shown. The keyboard stays where it is.
    fn add_tab_quietly(&mut self, block: BlockId, tab: NoteTab) {
        if let Some(note) = self.space.blocks.get_mut(&block).and_then(Body::note_mut) {
            note.tabs.push(tab);
            note.active = note.tabs.len() - 1;
        }
    }

    /// `edit_note` on the note block `block` (`uid`), shown in the workspace swapped in: a name, a
    /// color, and a text written only while its `rev` is still the one it was made on.
    #[allow(clippy::too_many_arguments)]
    fn note_edit(
        &mut self,
        block: BlockId,
        uid: &str,
        note: u64,
        name: Option<String>,
        color: Option<Option<String>>,
        text: Option<(String, u64)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        let Some(editor) = self
            .space
            .blocks
            .get(&block)
            .and_then(Body::note)
            .and_then(|n| n.tabs.iter().find(|t| t.id == note))
            .map(|t| t.editor.clone())
        else {
            return Reply::err(format!("{uid} shows no note {note}."));
        };
        let Some(entry) = self.notes.get(note) else {
            return Reply::err(format!("{uid} shows no note {note}."));
        };
        // Checked before anything is done: a refusal changes nothing.
        if let Some((_, rev)) = &text {
            if entry.rev != *rev {
                return Reply::err(format!(
                    "Note {note} changed since: its newer text and rev are in the state."
                ));
            }
            if entry.text.len() > NOTE_TEXT_MOST {
                return Reply::err(format!(
                    "Note {note} is longer than the API shows (256 KiB): edit it in Templum on \
                     the computer that holds it."
                ));
            }
        }
        let mut done = Vec::new();
        if let Some(name) = name {
            self.notes.rename(note, &name);
            done.push(format!(
                "Renamed note {note} \"{}\".",
                self.notes.name(note)
            ));
        }
        if let Some(color) = color {
            let label = color.and_then(|c| Label::ALL.into_iter().find(|l| l.name() == c));
            self.notes.set_color(note, label);
            done.push(match label {
                Some(label) => format!("Colored note {note} {}.", label.name()),
                None => format!("Cleared the color of note {note}."),
            });
        }
        if let Some((text, _)) = text {
            note::write(&editor, &text, window, cx);
            // At once: the editor's own Change comes after this update, and a second call in
            // the same tick would find the old rev.
            let now = editor.read(cx).value().to_string();
            self.notes.set_text(note, &now, notes::now());
            self.notes.touch(note);
            done.push(format!("Saved note {note}."));
        }
        cx.notify();
        Reply::ok(done.join(" "))
    }

    /// `close_note` on the note block `block` (`uid`): its tab `note` goes as its × does.
    fn note_close(
        &mut self,
        block: BlockId,
        uid: &str,
        note: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        let Some(shown) = self.space.blocks.get_mut(&block).and_then(Body::note_mut) else {
            return Reply::err(format!("{uid} isn't a note block."));
        };
        let Some(index) = shown.tabs.iter().position(|t| t.id == note) else {
            return Reply::err(format!("{uid} shows no note {note}."));
        };
        let tab = shown.remove_tab(index);
        // Its editor leaves the frame: the keyboard it had goes back to the canvas.
        if tab.editor.read(cx).focus_handle(cx).is_focused(window) {
            window.focus(&self.focus_handle, cx);
        }
        self.note_closed(note);
        Reply::ok(format!("Closed note {note} in {uid}."))
    }
}
