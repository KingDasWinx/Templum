//! A remote workspace's copies of home's notes and checklists, and its drawings (docs/specs/
//! templum-to-templum.md, Boards, checklists, notes, ink): made from home's state where it enters,
//! bounded and cleaned before they are kept, drawn or made an editor, never saved. A note block, a
//! checklist block and the canvas's ink of a mirrored workspace are drawn from them by the views
//! a local block uses.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{Context, Window};
use serde_json::json;
use templum_api::client::Call;
use templum_api::model::{ChecklistView, DrawingView, NoteView, State, newest_drawings};
use templum_canvas::{BlockId, Drawing};

use super::bus::{clean_title, clean_to};
use super::remote::{Deed, MOST_FAR, RemoteSpace, Then};
use super::remote_boards::ITEM_CHARS;
use super::{Body, ChecklistAct, Space, Workspace};
use crate::board::Label;
use crate::checklist::Checklist;
use crate::checklists::{self, Checklists, Item, List};
use crate::note::{self, Note, NoteTab};
use crate::notes::{Entry, Notes};
use crate::session::InkItem;
use gpui_kit::component::input::InputEvent;

/// The most notes and checklists the copy keeps, and the most items of a checklist.
pub(super) const MOST_NOTES: usize = 1_000;
pub(super) const MOST_CHECKLISTS: usize = 1_000;
pub(super) const MOST_LIST_ITEMS: usize = 2_000;
/// A note's text, in characters (its line breaks and tabs kept): home cuts at the same size in
/// bytes, so a text home sent is never cut again but a hostile home's is.
pub(super) const NOTE_CHARS: usize = 262_144;

/// A note's text with only what a mirror shows: its line breaks and tabs kept, every other control
/// character dropped. What is typed, sent and kept as home's copy is alike, so a pasted carriage
/// return can't make every later copy look like a change of home's.
pub(super) fn clean_note_text(text: &str) -> String {
    text.chars().filter(|c| kept(*c)).collect()
}

/// A character of a note's text a mirror shows (the one filter of the typed, the sent and the
/// copy's text).
fn kept(c: char) -> bool {
    !c.is_control() || matches!(c, '\n' | '\t')
}

/// Home's note as the copy keeps it: its name one line, its text cleaned and capped (a cut note,
/// home's or this cleaning's, ends with the sentence that says the rest is at home), its color
/// one of the six; and whether the note is cut (home's cut, or this cleaning's): it is never
/// edited from here.
fn note_cut_of(view: &NoteView, computer: &str) -> (Entry, bool) {
    let mut chars = view.text.chars().filter(|c| kept(*c));
    let mut text: String = chars.by_ref().take(NOTE_CHARS).collect();
    let cut = view.cut || chars.next().is_some();
    if cut {
        text.push_str(&format!("\n\n… The rest is on {computer}."));
    }
    let entry = Entry {
        id: view.id,
        name: clean_title(&view.name),
        text,
        color: view
            .color
            .as_deref()
            .and_then(|name| Label::ALL.into_iter().find(|l| l.name() == name)),
        changed: view.changed,
        rev: view.rev,
        closed_in: view.workspace.clone(),
    };
    (entry, cut)
}

/// Home's checklist as the copy keeps it: a name of one line, at most `MOST_LIST_ITEMS` items of
/// one line each.
pub(super) fn list_of(view: &ChecklistView) -> List {
    List {
        id: view.id,
        name: clean_title(&view.name),
        changed: 0,
        items: view
            .items
            .iter()
            .take(MOST_LIST_ITEMS)
            .map(|i| Item {
                id: i.id,
                text: clean_to(&i.text, ITEM_CHARS),
                done: i.done,
            })
            .collect(),
    }
}

/// Home's drawings as the canvas keeps its ink: each as the canvas file reads one (an unknown
/// kind, a stroke under two points or a number that isn't finite is dropped, an unknown color is
/// the foreground, a width is clamped), none with a point beyond `MOST_FAR`, then home's own caps
/// again, the newest kept.
pub(super) fn drawings_of(views: &[DrawingView]) -> Vec<Drawing> {
    let near = |p: &templum_canvas::Point| p.x.abs() <= MOST_FAR && p.y.abs() <= MOST_FAR;
    let drawings: Vec<Drawing> = views
        .iter()
        .filter_map(|view| InkItem::from(view).drawing())
        .filter(|drawing| match drawing {
            Drawing::Stroke { points, .. } => points.iter().all(near),
            Drawing::Shape { from, to, .. } => near(from) && near(to),
        })
        .collect();
    newest_drawings(&drawings, |drawing| match drawing {
        Drawing::Stroke { points, .. } => points.len(),
        Drawing::Shape { .. } => 2,
    })
    .to_vec()
}

impl RemoteSpace {
    /// The copy of home's notes, made again from `state`: the first `MOST_NOTES`, each id once. A
    /// note home didn't change since the state this one holds (the very same `Arc`) and no hold
    /// was laid over is moved over as it is, not cleaned again, so this is called before `state`
    /// becomes `self.state`. Answers the ids of the notes it made anew: the only ones whose
    /// editors may have a text to take. Then what was done here to a note's name and color and
    /// home's state doesn't show yet is laid over the copy (`lay_note_holds`).
    pub(super) fn mirror_notes(&mut self, state: &State, now: Instant) -> HashSet<u64> {
        let computer = crate::mirror::computer_of(self);
        let laid = std::mem::take(&mut self.notes_laid);
        let was_cut = std::mem::take(&mut self.notes_cut);
        let mut old: HashMap<u64, Entry> = std::mem::take(&mut self.notes)
            .notes
            .into_iter()
            .map(|n| (n.id, n))
            .collect();
        let before: HashMap<u64, &Arc<NoteView>> =
            self.state.notes.iter().map(|n| (n.id, n)).collect();
        let (mut seen, mut made, mut cut) = (HashSet::new(), HashSet::new(), HashSet::new());
        let notes = state
            .notes
            .iter()
            .filter(|view| seen.insert(view.id))
            .take(MOST_NOTES)
            .map(|view| {
                let same = before.get(&view.id).is_some_and(|b| Arc::ptr_eq(b, view))
                    && !laid.contains(&view.id);
                match old.remove(&view.id).filter(|_| same) {
                    Some(entry) => {
                        if was_cut.contains(&view.id) {
                            cut.insert(view.id);
                        }
                        entry
                    }
                    None => {
                        made.insert(view.id);
                        let (entry, is_cut) = note_cut_of(view, &computer);
                        if is_cut {
                            cut.insert(view.id);
                        }
                        entry
                    }
                }
            })
            .collect();
        self.notes = Notes {
            notes,
            ..Notes::default()
        };
        self.notes_cut = cut;
        self.lay_note_holds(state, now);
        made
    }

    /// What was done to a note's name or color here and home's state doesn't show yet stands over
    /// the copy. Each hold ends when home's state shows it, or after `HOLD`; a note one was laid
    /// over is cleaned again from home's state at the next copy, not moved over.
    fn lay_note_holds(&mut self, state: &State, now: Instant) {
        let views: HashMap<u64, &Arc<NoteView>> = state.notes.iter().map(|n| (n.id, n)).collect();
        let mut laid = HashSet::new();
        for note in self.notes_renamed.keys() {
            let Some(home) = views.get(&note).map(|n| clean_title(&n.name)) else {
                self.notes_renamed.end(&note);
                continue;
            };
            let name = self
                .notes_renamed
                .shown(note, home.clone(), now, |mine, home| mine == home);
            if name != home
                && let Some(entry) = self.notes.notes.iter_mut().find(|n| n.id == note)
            {
                entry.name = name;
                laid.insert(note);
            }
        }
        for note in self.notes_colored.keys() {
            let Some(home) = views.get(&note).map(|n| {
                n.color
                    .as_deref()
                    .and_then(|name| Label::ALL.into_iter().find(|l| l.name() == name))
            }) else {
                self.notes_colored.end(&note);
                continue;
            };
            let color = self
                .notes_colored
                .shown(note, home, now, |mine, home| mine == home);
            if color != home
                && let Some(entry) = self.notes.notes.iter_mut().find(|n| n.id == note)
            {
                entry.color = color;
                laid.insert(note);
            }
        }
        self.notes_laid = laid;
    }

    /// A tab closed here ends its hold once home's state no longer shows it.
    pub(super) fn end_closed_tabs(&mut self, state: &State) {
        let home = self.home.as_ref().map(|h| h.id.as_str());
        let shown: HashSet<u64> = state
            .blocks
            .iter()
            .filter(|b| home.is_some_and(|h| h == b.workspace))
            .filter_map(|b| b.notes.as_ref())
            .flat_map(|n| n.tabs.iter().copied())
            .collect();
        for note in self.tabs_closed.keys() {
            if !shown.contains(&note) {
                self.tabs_closed.end(&note);
            }
        }
    }

    /// The copy of home's checklists, made again from `state` as the notes' are; a checklist an
    /// item hold was laid over is cleaned again from home's state, not moved over.
    pub(super) fn mirror_checklists(&mut self, state: &State, now: Instant) {
        let laid = std::mem::take(&mut self.lists_laid);
        let mut old: HashMap<u64, List> = std::mem::take(&mut self.checklists)
            .lists
            .into_iter()
            .map(|l| (l.id, l))
            .collect();
        let before: HashMap<u64, &Arc<ChecklistView>> =
            self.state.checklists.iter().map(|l| (l.id, l)).collect();
        let mut seen = HashSet::new();
        let lists = state
            .checklists
            .iter()
            .filter(|view| seen.insert(view.id))
            .take(MOST_CHECKLISTS)
            .map(|view| {
                let same = before.get(&view.id).is_some_and(|b| Arc::ptr_eq(b, view))
                    && !laid.contains(&view.id);
                match old.remove(&view.id).filter(|_| same) {
                    Some(list) => list,
                    None => list_of(view),
                }
            })
            .collect();
        self.checklists = Checklists {
            lists,
            ..Checklists::default()
        };
        self.lay_item_holds(state, now);
    }

    /// What was done to an item here and home's state doesn't show yet stands over the copy: an
    /// item deleted is left out, an item checked, edited or moved is as it was left, and so is a
    /// checklist renamed. Each hold ends when home's state shows it, or after `HOLD`; a checklist
    /// one was laid over is cleaned again from home's state at the next copy, not moved over.
    fn lay_item_holds(&mut self, state: &State, now: Instant) {
        let views: HashMap<u64, &Arc<ChecklistView>> =
            state.checklists.iter().map(|l| (l.id, l)).collect();
        let home_item = |list: u64, item: u64| {
            views
                .get(&list)
                .and_then(|l| l.items.iter().find(|i| i.id == item))
        };
        let mut laid = HashSet::new();
        for key @ (list, item) in self.items_deleted.keys() {
            let Some(view) = views.get(&list) else {
                self.items_deleted.end(&key);
                continue;
            };
            // True when home's list no longer holds the item: the same as what was done here.
            let gone = !view.items.iter().any(|i| i.id == item);
            let deleted = self
                .items_deleted
                .shown(key, gone, now, |mine, home| mine == home);
            if deleted
                && !gone
                && let Some(list) = self.checklists.lists.iter_mut().find(|l| l.id == list)
            {
                list.items.retain(|i| i.id != item);
                laid.insert(list.id);
            }
        }
        for key @ (list, item) in self.items_checked.keys() {
            let Some(home) = home_item(list, item).map(|i| i.done) else {
                self.items_checked.end(&key);
                continue;
            };
            let done = self
                .items_checked
                .shown(key, home, now, |mine, home| mine == home);
            if done != home
                && let Some(item) = self.item_mut(list, item)
            {
                item.done = done;
                laid.insert(list);
            }
        }
        for key @ (list, item) in self.items_edited.keys() {
            let Some(home) = home_item(list, item).map(|i| i.text.clone()) else {
                self.items_edited.end(&key);
                continue;
            };
            let text = self
                .items_edited
                .shown(key, home.clone(), now, |mine, home| mine == home);
            if text != home
                && let Some(item) = self.item_mut(list, item)
            {
                item.text = clean_to(&text, ITEM_CHARS);
                laid.insert(list);
            }
        }
        for key @ (list, item) in self.items_moved.keys() {
            let Some(view) = views.get(&list) else {
                self.items_moved.end(&key);
                continue;
            };
            let Some(at) = view.items.iter().position(|i| i.id == item) else {
                self.items_moved.end(&key);
                continue;
            };
            // Where home has it, said as the item after it.
            let home = view.items.get(at + 1).map(|i| i.id);
            let before = self
                .items_moved
                .shown(key, home, now, |mine, home| mine == home);
            if before != home {
                self.checklists.move_item(list, item, before, 0);
                laid.insert(list);
            }
        }
        for list in self.lists_renamed.keys() {
            let Some(home) = views.get(&list).map(|l| clean_title(&l.name)) else {
                self.lists_renamed.end(&list);
                continue;
            };
            let name = self
                .lists_renamed
                .shown(list, home.clone(), now, |mine, home| mine == home);
            if name != home
                && let Some(copy) = self.checklists.lists.iter_mut().find(|l| l.id == list)
            {
                copy.name = name;
                laid.insert(list);
            }
        }
        self.lists_laid = laid;
    }

    fn item_mut(&mut self, list: u64, item: u64) -> Option<&mut Item> {
        self.checklists
            .lists
            .iter_mut()
            .find(|l| l.id == list)?
            .items
            .iter_mut()
            .find(|i| i.id == item)
    }
}

impl Workspace {
    /// The notes a note block draws: this computer's for a local one; a mirrored workspace's copy
    /// of home's for a remote one, which is drawn only while its workspace is shown (None else).
    pub(crate) fn notes_of(&self, note: &Note) -> Option<&Notes> {
        if note.remote {
            self.space.remote.as_deref().map(|r| &r.notes)
        } else {
            Some(&self.notes)
        }
    }

    /// The checklists a checklist block draws, as `notes_of` says of notes.
    pub(crate) fn checklists_of(&self, checklist: &Checklist) -> Option<&Checklists> {
        if checklist.remote {
            self.space.remote.as_deref().map(|r| &r.checklists)
        } else {
            Some(&self.checklists)
        }
    }

    /// The notes of workspace `space`: this computer's, or a mirrored workspace's copy.
    pub(crate) fn notes_in(&self, space: &str) -> Option<&Notes> {
        match self.space_named(space)?.remote.as_deref() {
            Some(remote) => Some(&remote.notes),
            None => Some(&self.notes),
        }
    }

    /// The checklists of workspace `space`: this computer's, or a mirrored workspace's copy.
    pub(crate) fn checklists_in(&self, space: &str) -> Option<&Checklists> {
        match self.space_named(space)?.remote.as_deref() {
            Some(remote) => Some(&remote.checklists),
            None => Some(&self.checklists),
        }
    }

    /// Why a note or checklist block of the mirrored workspace shown is a card, not the block: its
    /// home's tools were read and lack `edit_item`, which a home before batch 2e does too.
    pub(crate) fn older_content(&self) -> Option<String> {
        let remote = self.space.remote.as_deref()?;
        let tools = remote.tools.as_ref()?;
        (!tools.has("edit_item")).then(|| {
            format!(
                "{}'s Templum is older: update it to see this here.",
                crate::mirror::computer_of(remote)
            )
        })
    }

    /// The note editors of the mirrored workspace `id`, made again from its copy of home's notes:
    /// one editor, subscribed to its keys, for each note a note block of home's workspace
    /// shows (each note once); an editor whose text is not the copy's takes it by `note::write`
    /// unless the note is being typed in (`edits`), its selection kept where the text still
    /// reaches. Editors of notes no longer shown go. Then the blocks'
    /// tabs are laid (`lay_note_tabs`). Only an editor of a note in `made` (`mirror_notes`) is
    /// compared with the copy's text: a note moved over is the text it already has.
    pub(super) fn remote_notes(
        &mut self,
        id: &str,
        made: &HashSet<u64>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(space) = self.space_by_id_mut(id) else {
            return;
        };
        let Some(mut remote) = space.remote.take() else {
            return;
        };
        let shown = remote.shown_notes();
        remote.note_editors.retain(|note, _| shown.contains(note));
        for note in &shown {
            let Some(text) = remote.notes.get(*note).map(|n| n.text.clone()) else {
                continue;
            };
            match remote.note_editors.get(note) {
                // Home's text goes in as a ranged change: the caret, the scroll and the undo
                // history stay. A note being typed in is never written here: `note_steps` merges
                // at the person's pause.
                Some(editor) if made.contains(note) && !remote.edits.contains_key(note) => {
                    note::write(editor, &text, window, cx);
                }
                Some(_) => {}
                // Shown again while typed in here: the person's editor, still subscribed.
                None if remote.edits.contains_key(note) => {
                    let editor = remote.edits[note].editor.clone();
                    remote.note_editors.insert(*note, editor);
                }
                None => {
                    let editor = note::editor(&text, window, cx);
                    let (space_id, note_id) = (id.to_string(), *note);
                    cx.subscribe_in(
                        &editor,
                        window,
                        move |ws, _, event: &InputEvent, window, cx| match event {
                            InputEvent::Change => {
                                ws.remote_note_typed(&space_id, note_id, Instant::now(), window, cx)
                            }
                            InputEvent::Blur => ws.flush_note(&space_id, note_id, window, cx),
                            _ => {}
                        },
                    )
                    .detach();
                    remote.note_editors.insert(*note, editor);
                }
            }
        }
        let state = remote.state.clone();
        remote.end_closed_tabs(&state);
        space.lay_note_tabs(&remote, &state);
        space.remote = Some(remote);
        self.note_steps(id, Instant::now(), window, cx);
        self.show_awaited_tab(id, window, cx);
        cx.notify();
    }

    /// A person's act on an item of the checklist block `block` of the mirrored workspace `space`:
    /// home's tool does it, asked of the rule first (a refusal says its words once, and nothing is
    /// sent). A check, an edit and a delete show at once and stand over home's state until it
    /// shows them; added items come with home's state. Nothing is written here.
    pub(super) fn remote_checklist_act(
        &mut self,
        space: &str,
        block: BlockId,
        act: ChecklistAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let deed = match &act {
            ChecklistAct::Toggle { .. } => Deed::CheckItem,
            ChecklistAct::Delete { .. } => Deed::DeleteItem,
            ChecklistAct::Edit { .. } => Deed::EditItem,
            ChecklistAct::Add { .. } => Deed::AddItems,
            ChecklistAct::Move { .. } | ChecklistAct::Rename { .. } => Deed::EditChecklist,
        };
        let Some(sp) = self.space_named(space) else {
            return;
        };
        let Some(remote) = sp.remote.as_deref() else {
            return;
        };
        if let Some(nay) = remote.why_not(deed) {
            let words = nay.words(&crate::mirror::computer_of(remote));
            return self.warn(words, window, cx);
        }
        let (Some(home), Some(list)) = (
            remote.home_of(block),
            sp.checklist_view(block).and_then(|c| c.list),
        ) else {
            return;
        };
        let item_of = |item: u64| remote.checklists.get(list)?.item(item);
        enum Hold {
            Checked(u64, bool),
            Edited(u64, String),
            Deleted(u64),
            Moved(u64, Option<u64>),
            Renamed(String),
        }
        let (tool, args, hold) = match act {
            ChecklistAct::Toggle { item } => {
                let Some(shown) = item_of(item).map(|i| i.done) else {
                    return;
                };
                let done = !shown;
                (
                    "check_item",
                    json!({ "block": home, "item": item, "done": done }),
                    Some(Hold::Checked(item, done)),
                )
            }
            ChecklistAct::Delete { item } => {
                if item_of(item).is_none() {
                    return;
                }
                (
                    "delete_item",
                    json!({ "block": home, "item": item }),
                    Some(Hold::Deleted(item)),
                )
            }
            ChecklistAct::Edit { item, text } => {
                let text = text.trim().to_string();
                if text.is_empty() || item_of(item).is_none_or(|i| i.text == text) {
                    return;
                }
                (
                    "edit_item",
                    json!({ "block": home, "item": item, "text": text }),
                    Some(Hold::Edited(item, text)),
                )
            }
            ChecklistAct::Add { text } => {
                if checklists::parse_items(&text).is_empty() {
                    return;
                }
                ("add_items", json!({ "block": home, "text": text }), None)
            }
            ChecklistAct::Move { item, before } => {
                let Some(items) = remote.checklists.get(list).map(|l| &l.items) else {
                    return;
                };
                let Some(at) = items.iter().position(|i| i.id == item) else {
                    return;
                };
                // A drop that changes nothing sends nothing.
                let next = items.get(at + 1).map(|i| i.id);
                if before == Some(item) || next == before {
                    return;
                }
                let to = before.map_or(json!("end"), |b| json!(b));
                (
                    "edit_checklist",
                    json!({ "block": home, "item": item, "before": to }),
                    Some(Hold::Moved(item, before)),
                )
            }
            ChecklistAct::Rename { name } => {
                // A blank name reads "Checklist", as home shows it.
                let name = match clean_title(&name).trim() {
                    "" => checklists::NAME.to_string(),
                    name => name.to_string(),
                };
                if remote.checklists.get(list).is_none_or(|l| l.name == name) {
                    return;
                }
                (
                    "edit_checklist",
                    json!({ "block": home, "name": name }),
                    Some(Hold::Renamed(name)),
                )
            }
        };
        let now = Instant::now();
        // What a refusal ends: the item's holds, or the name's.
        let held = hold.as_ref().map(|hold| match hold {
            Hold::Checked(item, _)
            | Hold::Edited(item, _)
            | Hold::Deleted(item)
            | Hold::Moved(item, _) => Some(*item),
            Hold::Renamed(_) => None,
        });
        if let Some(remote) = self.remote_mut(space) {
            match hold {
                Some(Hold::Checked(item, done)) => {
                    remote.items_checked.hold((list, item), done, now)
                }
                Some(Hold::Edited(item, text)) => remote.items_edited.hold((list, item), text, now),
                Some(Hold::Deleted(item)) => remote.items_deleted.hold((list, item), true, now),
                Some(Hold::Moved(item, before)) => {
                    remote.items_moved.hold((list, item), before, now)
                }
                Some(Hold::Renamed(name)) => remote.lists_renamed.hold(list, name, now),
                None => {}
            }
        }
        if held.is_some() {
            self.remirror(space, now);
        }
        let call = Call {
            tool: tool.into(),
            args,
            slow: false,
        };
        let space_id = space.to_string();
        let then: Then = Box::new(move |ws, answered, window, cx| {
            let Err(refusal) = answered else {
                return;
            };
            // Home's state is the truth: what stood over it goes, and its last state is drawn.
            if let (Some(held), Some(remote)) = (held, ws.remote_mut(&space_id)) {
                match held {
                    Some(item) => {
                        remote.items_checked.end(&(list, item));
                        remote.items_edited.end(&(list, item));
                        remote.items_deleted.end(&(list, item));
                        remote.items_moved.end(&(list, item));
                    }
                    None => {
                        remote.lists_renamed.end(&list);
                    }
                }
            }
            ws.remirror(&space_id, Instant::now());
            ws.warn(refusal.sentence, window, cx);
        });
        self.remote_call(space, call, then, cx);
        if self.space.id == space {
            self.mark_remote();
        }
    }
}

impl RemoteSpace {
    /// The notes the note blocks of home's workspace show, each once, that the copy holds: what
    /// has an editor.
    fn shown_notes(&self) -> HashSet<u64> {
        let home = self.home.as_ref().map(|h| h.id.as_str());
        self.state
            .blocks
            .iter()
            .filter(|b| home.is_some_and(|h| h == b.workspace))
            .filter_map(|b| b.notes.as_ref())
            .flat_map(|n| n.tabs.iter().copied())
            .filter(|id| self.notes.get(*id).is_some())
            .collect()
    }
}

impl Space {
    /// The tabs of each note block of home's, laid from its state: home's `notes.tabs`, in home's
    /// back-to-front order, each note once across the workspace and only one with an editor. A
    /// block whose tabs were empty starts on home's `shown`; else it keeps the note it shows by
    /// id (clamped when that went).
    pub(super) fn lay_note_tabs(&mut self, remote: &RemoteSpace, state: &State) {
        let home = remote.home.as_ref().map(|h| h.id.as_str());
        let mut blocks: Vec<_> = state
            .blocks
            .iter()
            .filter(|b| home.is_some_and(|h| h == b.workspace) && b.notes.is_some())
            .collect();
        blocks.sort_by_key(|b| b.z);
        let mut seen = HashSet::new();
        for b in blocks {
            let (Some(shown), Some(local)) = (b.notes.as_ref(), remote.homes.get(&b.id)) else {
                continue;
            };
            let Some(Body::Note(note)) = self.blocks.get_mut(local) else {
                continue;
            };
            if !note.remote {
                continue;
            }
            let tabs: Vec<u64> = shown
                .tabs
                .iter()
                .copied()
                .filter(|id| {
                    remote.note_editors.contains_key(id)
                        && remote.tabs_closed.get(id) != Some(&true)
                        && seen.insert(*id)
                })
                .collect();
            if note.ids() == tabs {
                continue;
            }
            let was_empty = note.tabs.is_empty();
            let was = note.tab().map(|t| t.id);
            let mut old: HashMap<u64, NoteTab> = std::mem::take(&mut note.tabs)
                .into_iter()
                .map(|t| (t.id, t))
                .collect();
            note.tabs = tabs
                .iter()
                .map(|id| {
                    old.remove(id).unwrap_or_else(|| NoteTab {
                        id: *id,
                        editor: remote.note_editors[id].clone(),
                        far: Default::default(),
                    })
                })
                .collect();
            let at = |id: Option<u64>| id.and_then(|id| tabs.iter().position(|t| *t == id));
            note.active = if was_empty { at(shown.shown) } else { at(was) }
                .unwrap_or(note.active)
                .min(note.tabs.len().saturating_sub(1));
            note.renaming = None;
        }
    }

    /// Home's drawings of workspace `home` are this canvas's ink (a remote space is never saved,
    /// and what it draws is never picked): empty without any.
    pub(super) fn mirror_drawings(&mut self, state: &State, home: Option<&str>) {
        self.ink = home
            .and_then(|home| state.drawings.iter().find(|d| d.id == home))
            .map(|d| drawings_of(&d.drawings))
            .unwrap_or_default();
        self.selected_ink = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use templum_api::model::{DrawingsView, ItemView};

    fn stroke(points: &[[f32; 2]]) -> DrawingView {
        DrawingView {
            kind: "stroke".into(),
            color: "blue".into(),
            width: 2.0,
            points: points.to_vec(),
            ..Default::default()
        }
    }

    fn remote() -> RemoteSpace {
        RemoteSpace::new(crate::session::RemoteEntry::default())
    }

    #[test]
    fn home_s_notes_are_cleaned_and_capped() {
        let view = NoteView {
            id: 7,
            name: format!("a\x1b[31m\nb{}", "n".repeat(200)),
            text: format!("x\r\n\x07\ty{}", "t".repeat(300_000)),
            color: Some("purple".into()),
            cut: false,
            ..Default::default()
        };
        let note = note_cut_of(&view, "winx-desktop").0;
        assert!(!note.name.contains(['\n', '\x1b']) && note.name.chars().count() <= 60);
        assert!(!note.text.contains(['\r', '\x07']));
        assert!(note.text.starts_with("x\n\tyttt"));
        let tail = "\n\n… The rest is on winx-desktop.";
        assert!(note.text.ends_with(tail));
        assert_eq!(note.text.chars().count(), NOTE_CHARS + tail.chars().count());
        assert_eq!(note.color, None);
        assert_eq!(note.changed, 0);

        let short = NoteView {
            id: 9,
            text: "short".into(),
            color: Some("blue".into()),
            cut: true,
            ..Default::default()
        };
        let note = note_cut_of(&short, "winx-desktop").0;
        assert_eq!(note.text, format!("short{tail}"));
        assert_eq!(note.color, Some(Label::Blue));
        let whole = NoteView {
            text: "short".into(),
            ..Default::default()
        };
        assert_eq!(note_cut_of(&whole, "winx-desktop").0.text, "short");

        let state = State {
            notes: (1..=1_500)
                .map(|id| {
                    Arc::new(NoteView {
                        id,
                        ..Default::default()
                    })
                })
                .collect(),
            ..Default::default()
        };
        let mut space = remote();
        space.mirror_notes(&state, std::time::Instant::now());
        assert_eq!(space.notes.notes.len(), MOST_NOTES);
        assert_eq!(space.notes.notes[0].id, 1);
    }

    #[test]
    fn home_s_checklists_are_cleaned_and_capped() {
        let view = ChecklistView {
            id: 12,
            name: "R\nel".into(),
            items: (1..=3_000)
                .map(|id| ItemView {
                    id,
                    text: format!("\x1b{}", "i".repeat(5_000)),
                    done: id % 2 == 0,
                })
                .collect(),
        };
        let list = list_of(&view);
        assert_eq!(list.name, "R el");
        assert_eq!(list.items.len(), MOST_LIST_ITEMS);
        assert!(
            list.items.iter().all(|i| {
                i.text.chars().count() <= ITEM_CHARS && !i.text.contains(['\x1b', '\n'])
            })
        );
        assert_eq!((list.items[0].id, list.items[0].done), (1, false));
        assert_eq!((list.items[1].id, list.items[1].done), (2, true));

        let state = State {
            checklists: (1..=1_200)
                .map(|id| {
                    Arc::new(ChecklistView {
                        id,
                        ..Default::default()
                    })
                })
                .collect(),
            ..Default::default()
        };
        let mut space = remote();
        space.mirror_checklists(&state, std::time::Instant::now());
        assert_eq!(space.checklists.lists.len(), MOST_CHECKLISTS);
    }

    #[test]
    fn home_s_drawings_are_cleaned_and_capped() {
        let mut odd = stroke(&[[0.0, 0.0], [5.0, 5.0]]);
        odd.width = 1000.0;
        odd.color = "chartreuse".into();
        let views = [
            stroke(&[[1.0, 1.0]]),
            stroke(&[[0.0, 0.0], [1e30, 0.0]]),
            stroke(&[[0.0, 0.0], [f32::INFINITY, 0.0]]),
            DrawingView {
                kind: "spiral".into(),
                ..stroke(&[[0.0, 0.0], [1.0, 1.0]])
            },
            DrawingView {
                kind: "rect".into(),
                color: "red".into(),
                width: 2.0,
                ..Default::default()
            },
            odd,
        ];
        let drawings = drawings_of(&views);
        assert_eq!(drawings.len(), 1);
        match &drawings[0] {
            Drawing::Stroke { width, color, .. } => {
                assert_eq!(*width, templum_canvas::MAX_WIDTH);
                assert_eq!(*color, templum_canvas::Color::Foreground);
            }
            other => panic!("{other:?}"),
        }
        let many: Vec<DrawingView> = (0..2_500)
            .map(|i| stroke(&[[i as f32, 0.0], [i as f32, 1.0]]))
            .collect();
        let kept = drawings_of(&many);
        assert_eq!(kept.len(), 2_000);
        match &kept[0] {
            Drawing::Stroke { points, .. } => assert_eq!(points[0].x, 500.0),
            other => panic!("{other:?}"),
        }
        let state = State {
            drawings: vec![Arc::new(DrawingsView {
                id: "w1".into(),
                drawings: many,
            })],
            ..Default::default()
        };
        let mut space = Space::default();
        space.mirror_drawings(&state, Some("w1"));
        assert_eq!(space.ink.len(), 2_000);
        space.mirror_drawings(&state, Some("w2"));
        assert!(space.ink.is_empty());
    }

    #[test]
    fn a_note_or_checklist_home_didn_t_change_is_not_cleaned_again() {
        let note = |id: u64, text: &str| {
            Arc::new(NoteView {
                id,
                text: text.into(),
                ..Default::default()
            })
        };
        let list = |id: u64, text: &str| {
            Arc::new(ChecklistView {
                id,
                items: vec![ItemView {
                    id: 1,
                    text: text.into(),
                    done: false,
                }],
                ..Default::default()
            })
        };
        let (n7, l12) = (note(7, "seven"), list(12, "twelve"));
        let first = State {
            notes: vec![n7.clone()],
            checklists: vec![l12.clone()],
            ..Default::default()
        };
        let mut space = remote();
        let now = std::time::Instant::now();
        assert_eq!(space.mirror_notes(&first, now), HashSet::from([7]));
        space.mirror_checklists(&first, now);
        space.state = first;
        let (text7, text12) = (
            space.notes.notes[0].text.as_ptr(),
            space.checklists.lists[0].items[0].text.as_ptr(),
        );
        let second = State {
            notes: vec![n7, note(9, "nine")],
            checklists: vec![l12, list(13, "thirteen")],
            ..Default::default()
        };
        assert_eq!(
            space.mirror_notes(&second, now),
            HashSet::from([9]),
            "7 moved over"
        );
        space.mirror_checklists(&second, now);
        assert_eq!(
            space.notes.notes[0].text.as_ptr(),
            text7,
            "note 7 moved over"
        );
        assert_eq!(space.notes.notes[1].text, "nine");
        assert_eq!(
            space.checklists.lists[0].items[0].text.as_ptr(),
            text12,
            "list 12 moved over"
        );
        assert_eq!(space.checklists.lists[1].items[0].text, "thirteen");
    }
}
