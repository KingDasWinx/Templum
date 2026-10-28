//! Notes of a mirrored workspace, typed in, renamed, colored, made, closed, reopened and deleted
//! here and at home (docs/specs/templum-to-templum.md, Notes on a mirror; the plan is
//! docs/plans/2026-10-26-mirror-notes.md). What is typed is local and instant; the whole text goes
//! home after a pause with the `rev` it was typed on, home writes it only on that `rev`, and when
//! home's text moved on the two changes are merged, or kept as two notes: the person's text is
//! never lost. Every other act goes through one door, `remote_note_act`.

use std::time::{Duration, Instant};

use gpui_kit::component::input::EditorState;
use gpui_kit::{Context, Entity, Window};
use serde_json::json;
use templum_api::client::Call;
use templum_api::model::NOTE_TEXT_MOST;
use templum_canvas::{BlockId, Rect};

use super::holds::HOLD;
use super::remote::{Deed, NOT_CONNECTED, Nay, RemoteSpace, Then};
use super::remote_content::clean_note_text;
use super::{Body, Workspace};
use crate::board::Label;
use crate::note;
use crate::notes::{self, merge};

/// A pause in the typing this long sends the text; while keys keep coming, one send at least this
/// often.
pub(crate) const PAUSE: Duration = Duration::from_millis(500);
pub(crate) const MOST_WAIT: Duration = Duration::from_secs(2);
/// Refusals in a row, with home's state no newer, before the sentence is told and the sending
/// waits for the next key.
pub(crate) const TRIES: u8 = 3;

/// `RemoteSpace::told`'s tags.
const NOT_LIVE: &str = "not live";
const NEEDS_FULL: &str = "needs full";
const OLDER: &str = "older";
const NOTE_CUT: &str = "note cut";
const NOTE_COPYING: &str = "note copying";
const NOTE_LONG: &str = "note too long";
const NOTE_STUCK: &str = "note stuck";

/// A note the person typed in here whose text home does not have yet.
pub(crate) struct NoteEdit {
    /// The editor typed in: kept here so the text outlives its tab (a note home closed).
    pub editor: Entity<EditorState>,
    /// Home's block it was last shown in (home's id), its rectangle, and the note's name: where a
    /// copy goes, and how it is called, when the note is gone.
    pub block: String,
    pub rect: Rect,
    pub name: String,
    /// Home's text the editor's text was made from, and its rev; None: the text home took from the
    /// last send, whose rev the next state brings (any rev above `after`).
    pub base: String,
    pub rev: Option<u64>,
    pub after: u64,
    /// The last key; the first key not sent yet; the first key home doesn't have yet (the " •"
    /// after `HOLD`).
    pub typed: Instant,
    pub unsent: Option<Instant>,
    pub since: Option<Instant>,
    /// The text sent and not answered yet, and `unsent` as it was when it went.
    pub sent: Option<(String, Option<Instant>)>,
    /// A send home may have done (its answer was lost): the text sent. Home's next text says
    /// whether it landed; `base` stays the old one until then, so nothing is merged on it.
    pub doubt: Option<String>,
    /// The last send was refused, and how many times in a row with home's state no newer.
    pub refused: Option<(Instant, u8)>,
    /// Send now: the editor lost the keyboard, or its tab or block is closing.
    pub flush: bool,
    /// The person's text is being made a new note at home.
    pub copying: bool,
    /// That copy was refused, and when: the note takes no keys, and the copy is tried again once
    /// live and `HOLD` later, until it was refused `TRIES` times in a row (`copy_refused`), or on a
    /// key. The text stays in the editor.
    pub stuck: Option<Instant>,
    pub copy_refused: u8,
}

/// What waits until the text typed in a note is at home.
pub(crate) enum Closing {
    Tab { block: BlockId, note: u64 },
    Block(BlockId),
}

/// An act on a note from a tab, a menu or the Notes dialog.
pub(crate) enum NoteAct {
    New,
    Close { note: u64 },
    Rename { note: u64, name: String },
    Color { note: u64, color: Option<Label> },
    Open { note: u64 },
    Delete { note: u64 },
}

/// Why the person's text becomes a new note.
#[derive(Clone, Copy, PartialEq)]
enum Why {
    Conflict,
    Gone,
    Cut,
}

/// What a step decided, done once the remote is back in its space.
enum Out {
    Send {
        note: u64,
        block: String,
        rev: u64,
        text: String,
    },
    Keep {
        note: u64,
        why: Why,
        block: Option<String>,
        title: String,
        name: String,
        text: String,
        rect: Rect,
    },
    Tell(String),
}

impl RemoteSpace {
    /// Home's block that shows `note` as a tab of its workspace.
    fn block_showing(&self, note: u64) -> Option<&templum_api::model::BlockView> {
        let home = self.home.as_ref().map(|h| h.id.as_str())?;
        self.state.blocks.iter().map(|b| &**b).find(|b| {
            b.workspace == home && b.notes.as_ref().is_some_and(|n| n.tabs.contains(&note))
        })
    }

    /// Why a key typed in `note` does nothing, in the first of the rule's words, a cut note's and
    /// a copy's being made; with the tag it is told once under. None: the note takes keys.
    pub(crate) fn note_locked(&self, note: u64) -> Option<(&'static str, String)> {
        let computer = crate::mirror::computer_of(self);
        if let Some(nay) = self.why_not(Deed::EditNote) {
            return Some(match nay {
                Nay::NotConnected | Nay::NotYet | Nay::Drawing => (NOT_LIVE, NOT_CONNECTED.into()),
                // Who the share is isn't known yet: not live for typing.
                Nay::NeedsFull if self.me.is_none() => (NOT_LIVE, NOT_CONNECTED.into()),
                Nay::NeedsFull => (NEEDS_FULL, nay.words(&computer)),
                Nay::Older => (OLDER, nay.words(&computer)),
            });
        }
        if self.notes_cut.contains(&note) {
            return Some((
                NOTE_CUT,
                format!(
                    "This note is longer than a mirror edits (256 KiB): edit it on {computer}."
                ),
            ));
        }
        let edit = self.edits.get(&note)?;
        if edit.copying {
            return Some((
                NOTE_COPYING,
                format!("Keeping your version as a new note on {computer}…"),
            ));
        }
        edit.stuck.map(|_| {
            (
                NOTE_STUCK,
                format!(
                    "Your version of \"{}\" couldn't be kept on {computer}: copy it out, or \
                     press a key to try again.",
                    edit.name
                ),
            )
        })
    }

    /// The tab of `note` shows " •": its text isn't at home yet for `HOLD`, or its copy is being
    /// made or was refused.
    pub(crate) fn note_unsaved(&self, note: u64, now: Instant) -> bool {
        self.edits.get(&note).is_some_and(|e| {
            e.copying
                || e.stuck.is_some()
                || e.since
                    .is_some_and(|since| now.saturating_duration_since(since) >= HOLD)
        })
    }

    /// When the next step of an edit is due after `now`. A due at or before `now` has had its
    /// step: if that did nothing, the edit waits for an event (an answer, home's state, a key, the
    /// remote going live), not for time, and a wake for it would spin.
    fn next_note_due(&self, now: Instant) -> Option<Instant> {
        self.edits
            .values()
            .flat_map(|e| {
                [
                    (!e.copying && e.stuck.is_none()).then_some(e.typed + PAUSE),
                    e.stuck
                        .filter(|_| e.copy_refused < TRIES)
                        .map(|at| at + HOLD),
                    e.unsent.map(|u| u + MOST_WAIT),
                    e.since.map(|s| s + HOLD),
                    e.refused.map(|(at, _)| at + HOLD),
                ]
            })
            .flatten()
            .filter(|due| *due > now)
            .min()
    }

    /// The share's name for a note kept as a copy.
    fn share_name(&self) -> String {
        self.me
            .as_ref()
            .map(|m| super::bus::clean_title(&m.name))
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "another computer".into())
    }

    /// Whether any of `notes` has text home does not have yet.
    fn has_edits(&self, notes: impl IntoIterator<Item = u64>) -> bool {
        notes.into_iter().any(|n| self.edits.contains_key(&n))
    }
}

/// One step of one edit (the plan's §6.4, in its order).
fn step(
    remote: &mut RemoteSpace,
    note: u64,
    now: Instant,
    window: &mut Window,
    cx: &mut gpui_kit::App,
    out: &mut Vec<Out>,
) {
    let computer = crate::mirror::computer_of(remote);
    let share = remote.share_name();
    let showing = remote.block_showing(note).map(|b| {
        (
            b.id.clone(),
            Rect::new(b.rect.x, b.rect.y, b.rect.w, b.rect.h),
        )
    });
    let live = remote.why_not(Deed::EditNote);
    let cut = remote.notes_cut.contains(&note);
    let Some(edit) = remote.edits.get_mut(&note) else {
        return;
    };
    let copy = remote.notes.get(note);
    if let Some(copy) = copy {
        edit.name.clone_from(&copy.name);
    }
    if let Some((block, rect)) = &showing {
        edit.block.clone_from(block);
        edit.rect = *rect;
    }
    let editor = edit.editor.clone();
    let mut mine = editor.read(cx).value().to_string();
    // 0. Only clean text: what is sent, what the base is and what home's copy shows are alike.
    let clean = clean_note_text(&mine);
    if clean != mine {
        note::write(&editor, &clean, window, cx);
        mine = clean;
    }
    // 1. A copy is being made, or was refused: tried again once live, `HOLD` and not `TRIES` times.
    if edit.copying {
        return;
    }
    if let Some(at) = edit.stuck {
        if live.is_some() || edit.copy_refused >= TRIES || now < at + HOLD {
            return;
        }
        edit.stuck = None;
    }
    // 1b. Past 256 KiB nothing can be sent, nor kept by `new_note` (home refuses both): a note
    // home still shows stays editable, so the person can shorten it; what only moves the base
    // still runs.
    let long = mine.len() > NOTE_TEXT_MOST;
    let say_long = |told: &mut std::collections::HashSet<(String, &'static str)>,
                    out: &mut Vec<Out>| {
        if told.insert((format!("note {note}"), NOTE_LONG)) {
            out.push(Out::Tell(format!(
                "This note is past 256 KiB, more than a mirror edits: what you add stays here. \
                 Shorten it, or edit it on {computer}."
            )));
        }
    };
    let keep = |edit: &NoteEdit, why: Why, mine: String| Out::Keep {
        note,
        why,
        block: showing.as_ref().map(|(b, _)| b.clone()),
        title: edit.name.clone(),
        name: format!("{} ({share})", edit.name),
        text: mine,
        rect: edit.rect,
    };
    // 2. Gone: home doesn't have the note any more, or it is cut.
    let (Some(copy), Some((block, _)), false) = (copy, &showing, cut) else {
        if mine == edit.base && edit.sent.is_none() {
            remote.edits.remove(&note);
        } else {
            let why = if cut { Why::Cut } else { Why::Gone };
            let out_keep = keep(edit, why, mine);
            edit.copying = true;
            out.push(out_keep);
        }
        return;
    };
    // 3. One send at a time.
    if edit.sent.is_some() {
        return;
    }
    // 4. Landed: home's text holds the send once its rev is past the one it was made on.
    if edit.rev.is_none() {
        if copy.rev > edit.after {
            edit.rev = Some(copy.rev);
        } else {
            return;
        }
    }
    // 4b. A send home may have done: its text says whether it landed, never a merge on the old
    // base (that would put the sent text in twice).
    if edit.doubt.is_some() && (edit.rev != Some(copy.rev) || copy.text != edit.base) {
        if edit.doubt.as_deref() == Some(copy.text.as_str()) {
            edit.base.clone_from(&copy.text);
        } else if copy.text != edit.base {
            if long {
                return say_long(&mut remote.told, out);
            }
            let out_keep = keep(edit, Why::Conflict, mine);
            edit.copying = true;
            out.push(out_keep);
            return;
        }
        edit.rev = Some(copy.rev);
        edit.refused = None;
        edit.doubt = None;
    }
    // 5. Home moved: its text or rev is not the one this was made on. A change of home's waits
    // for the person's next pause, and is then merged with what they typed.
    if edit.rev != Some(copy.rev) || copy.text != edit.base {
        if !edit.flush && now < edit.typed + PAUSE {
            return;
        }
        match merge(&edit.base, &mine, &copy.text) {
            Some(merged) => {
                note::write(&editor, &merged, window, cx);
                edit.base.clone_from(&copy.text);
                edit.rev = Some(copy.rev);
                edit.refused = None;
                mine = merged;
            }
            None => {
                if long {
                    return say_long(&mut remote.told, out);
                }
                let out_keep = keep(edit, Why::Conflict, mine);
                edit.copying = true;
                out.push(out_keep);
                return;
            }
        }
    }
    // 6. Clean: home has it all.
    if mine == edit.base {
        remote.edits.remove(&note);
        return;
    }
    // 7. Not live: nothing is sent, nothing dropped.
    if live.is_some() {
        return;
    }
    // 8. Over 256 KiB: see 1b.
    if long {
        return say_long(&mut remote.told, out);
    }
    // 9. A refusal stands while home's state is no newer.
    if let Some((at, count)) = edit.refused
        && (count >= TRIES || now < at + HOLD)
    {
        return;
    }
    // 10.
    let unsent = edit.unsent.unwrap_or(edit.typed);
    if !edit.flush && now < (edit.typed + PAUSE).min(unsent + MOST_WAIT) {
        return;
    }
    // 11. Send.
    edit.flush = false;
    edit.sent = Some((mine.clone(), edit.unsent.take()));
    out.push(Out::Send {
        note,
        block: block.clone(),
        rev: copy.rev,
        text: mine,
    });
}

impl Workspace {
    /// A key changed the text of the editor of home's note `note` of the mirrored workspace `space`
    /// (or its editor was written: then it is home's text, and nothing is typed).
    pub(super) fn remote_note_typed(
        &mut self,
        space: &str,
        note: u64,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(remote) = self.remote_mut(space) else {
            return;
        };
        if let Some(edit) = remote.edits.get_mut(&note) {
            edit.typed = now;
            edit.unsent.get_or_insert(now);
            edit.since.get_or_insert(now);
            edit.refused = None;
        } else {
            let (Some(copy), Some(editor)) =
                (remote.notes.get(note), remote.note_editors.get(&note))
            else {
                return;
            };
            // A write of home's text, not a key.
            if editor.read(cx).value().as_ref() == copy.text {
                return;
            }
            let Some((block, rect)) = remote.block_showing(note).map(|b| {
                (
                    b.id.clone(),
                    Rect::new(b.rect.x, b.rect.y, b.rect.w, b.rect.h),
                )
            }) else {
                return;
            };
            let edit = NoteEdit {
                editor: editor.clone(),
                block,
                rect,
                name: copy.name.clone(),
                base: copy.text.clone(),
                rev: Some(copy.rev),
                after: 0,
                typed: now,
                unsent: Some(now),
                since: Some(now),
                sent: None,
                refused: None,
                flush: false,
                copying: false,
                doubt: None,
                stuck: None,
                copy_refused: 0,
            };
            remote.edits.insert(note, edit);
        }
        self.note_steps(space, now, window, cx);
    }

    /// The editor of home's note `note` lost the keyboard: what is typed goes now.
    pub(super) fn flush_note(
        &mut self,
        space: &str,
        note: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(edit) = self.remote_mut(space).and_then(|r| r.edits.get_mut(&note)) else {
            return;
        };
        edit.flush = true;
        self.note_steps(space, Instant::now(), window, cx);
    }

    /// Every edit of the remote `space` takes its next step at `now` (a key, a blur, a state, an
    /// answer, the wake), then the wake is set for the earliest due time.
    pub(super) fn note_steps(
        &mut self,
        space: &str,
        now: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(sp) = self.space_by_id_mut(space) else {
            return;
        };
        let Some(mut remote) = sp.remote.take() else {
            return;
        };
        let mut out = Vec::new();
        let ids: Vec<u64> = remote.edits.keys().copied().collect();
        for note in ids {
            step(&mut remote, note, now, window, cx, &mut out);
        }
        let due = remote.next_note_due(now);
        sp.remote = Some(remote);
        for step in out {
            match step {
                Out::Tell(sentence) => self.warn(sentence, window, cx),
                Out::Send {
                    note,
                    block,
                    rev,
                    text,
                } => self.send_note(space, note, block, rev, text, cx),
                Out::Keep {
                    note,
                    why,
                    block,
                    title,
                    name,
                    text,
                    rect,
                } => self.keep_note(space, note, why, block, title, name, text, rect, cx),
            }
        }
        self.fire_closings(space, window, cx);
        self.wake_notes(space, due, window, cx);
        cx.notify();
    }

    /// `edit_note` with the whole text and the rev it was made on.
    #[allow(clippy::too_many_arguments)]
    fn send_note(
        &mut self,
        space: &str,
        note: u64,
        block: String,
        rev: u64,
        text: String,
        cx: &mut Context<Self>,
    ) {
        let call = Call {
            tool: "edit_note".into(),
            args: json!({ "block": block, "note": note, "rev": rev, "text": text }),
            slow: false,
        };
        let id = space.to_string();
        let then: Then = Box::new(move |ws, answered, window, cx| {
            let now = Instant::now();
            let mut tell = None;
            if let Some(edit) = ws.remote_mut(&id).and_then(|r| r.edits.get_mut(&note)) {
                let (sent_text, sent_unsent) = match edit.sent.take() {
                    Some((text, unsent)) => (Some(text), unsent),
                    None => (None, None),
                };
                match answered {
                    Ok(_) => {
                        edit.after = rev;
                        edit.rev = None;
                        if let Some(text) = sent_text {
                            edit.base = text;
                        }
                        edit.refused = None;
                        edit.doubt = None;
                        edit.copy_refused = 0;
                        edit.since = edit.unsent;
                    }
                    Err(refusal) => {
                        let count = edit.refused.map_or(0, |(_, n)| n) + 1;
                        edit.refused = Some((now, count));
                        // Home may have written it: its next text says (step 4b).
                        if refusal.maybe_done {
                            edit.doubt = sent_text;
                        }
                        edit.unsent = match (edit.unsent, sent_unsent) {
                            (Some(a), Some(b)) => Some(a.min(b)),
                            (a, b) => a.or(b),
                        };
                        if count == TRIES {
                            tell = Some(refusal.sentence);
                        }
                    }
                }
            }
            if let Some(sentence) = tell {
                ws.warn(sentence, window, cx);
            }
            ws.note_steps(&id, now, window, cx);
        });
        self.remote_call(space, call, then, cx);
    }

    /// The person's text becomes a new note at home: in the block that showed the note while home
    /// still shows it as a note block, else in a new note block where it was.
    #[allow(clippy::too_many_arguments)]
    fn keep_note(
        &mut self,
        space: &str,
        note: u64,
        why: Why,
        block: Option<String>,
        title: String,
        name: String,
        text: String,
        rect: Rect,
        cx: &mut Context<Self>,
    ) {
        let Some(remote) = self.remote_mut(space) else {
            return;
        };
        let alive = |b: &str| {
            remote
                .state
                .blocks
                .iter()
                .any(|v| v.id == b && v.notes.is_some())
        };
        let mut named = true;
        // new_note takes at most 256 KiB; add_block takes what its body holds.
        let fits = text.len() <= NOTE_TEXT_MOST;
        let call = match block.filter(|b| fits && alive(b)).or_else(|| {
            remote
                .edits
                .get(&note)
                .map(|e| e.block.clone())
                .filter(|b| fits && alive(b))
        }) {
            Some(block) => Call {
                tool: "new_note".into(),
                args: json!({ "block": block, "name": name, "text": text }),
                slow: false,
            },
            None => {
                let Some(workspace) = remote.home_workspace() else {
                    return;
                };
                // Home names a note made by `add_block` by its first line, not `name`.
                named = false;
                let mut args = json!({ "workspace": workspace, "kind": "note", "text": text });
                if remote.takes("add_block", "x") {
                    args["x"] = rect.x.into();
                    args["y"] = rect.y.into();
                }
                Call {
                    tool: "add_block".into(),
                    args,
                    slow: false,
                }
            }
        };
        let computer = crate::mirror::computer_of(remote);
        let id = space.to_string();
        let then: Then = Box::new(move |ws, answered, window, cx| {
            match answered {
                Ok(_) => {
                    let what = if named {
                        format!("the new note \"{name}\"")
                    } else {
                        "a new note where it was".to_string()
                    };
                    let sentence = match why {
                        Why::Conflict => format!(
                            "{computer} changed \"{title}\" while you typed: your version is \
                             {what} beside it."
                        ),
                        Why::Gone => format!(
                            "\"{title}\" isn't open on {computer} any more: your version is \
                             {what}."
                        ),
                        Why::Cut => format!(
                            "\"{title}\" is longer than a mirror edits (256 KiB): your version \
                             is {what}."
                        ),
                    };
                    // The note shows home's text again; the person's lives in the new note.
                    let kept = ws.remote_mut(&id).and_then(|r| {
                        let edit = r.edits.remove(&note)?;
                        // Shown (again): home's text; a closed note's state has none.
                        let copy = r
                            .note_editors
                            .contains_key(&note)
                            .then(|| r.notes.get(note).map(|n| n.text.clone()))
                            .flatten();
                        Some((edit.editor, copy))
                    });
                    if let Some((editor, Some(text))) = kept {
                        note::write(&editor, &text, window, cx);
                    }
                    ws.warn(sentence, window, cx);
                }
                Err(refusal) => {
                    if let Some(edit) = ws.remote_mut(&id).and_then(|r| r.edits.get_mut(&note)) {
                        edit.copying = false;
                        edit.stuck = Some(Instant::now());
                        // Home may have made it: a second one would be a duplicate there, so
                        // only the person's key tries again.
                        edit.copy_refused = if refusal.maybe_done {
                            TRIES
                        } else {
                            edit.copy_refused + 1
                        };
                    }
                    ws.warn(refusal.sentence, window, cx);
                }
            }
            ws.note_steps(&id, Instant::now(), window, cx);
        });
        self.remote_call(space, call, then, cx);
    }

    /// The next wake: `due`, when anything is due.
    fn wake_notes(
        &mut self,
        space: &str,
        due: Option<Instant>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let task = due.map(|due| {
            let id = space.to_string();
            let wait = due.saturating_duration_since(Instant::now());
            cx.spawn_in(window, async move |ws, cx| {
                cx.background_executor().timer(wait).await;
                ws.update_in(cx, |ws, window, cx| {
                    ws.note_steps(&id, Instant::now(), window, cx)
                })
                .ok();
            })
        });
        if let Some(remote) = self.remote_mut(space) {
            remote.note_wake = task;
        }
    }

    /// A tab or a note block closed while its text was not at home closes now, if home has it.
    fn fire_closings(&mut self, space: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(sp) = self.space_by_id_mut(space) else {
            return;
        };
        let Some(mut remote) = sp.remote.take() else {
            return;
        };
        let tabs_of = |sp: &super::Space, block: BlockId| {
            sp.blocks
                .get(&block)
                .and_then(Body::note_view)
                .map(|n| n.ids())
                .unwrap_or_default()
        };
        let (mut ready, mut waiting) = (Vec::new(), Vec::new());
        for closing in std::mem::take(&mut remote.closing) {
            let free = match &closing {
                Closing::Tab { note, .. } => !remote.edits.contains_key(note),
                Closing::Block(block) => !remote.has_edits(tabs_of(sp, *block)),
            };
            if free {
                ready.push(closing);
            } else {
                waiting.push(closing);
            }
        }
        remote.closing = waiting;
        sp.remote = Some(remote);
        for closing in ready {
            match closing {
                Closing::Tab { block, note } => {
                    self.remote_note_act(space, Some(block), NoteAct::Close { note }, window, cx)
                }
                Closing::Block(block) if self.space.id == space => {
                    self.close_remote_now(&[block], window, cx)
                }
                Closing::Block(_) => {}
            }
        }
    }

    /// Closing waits for the text of these notes to be at home: they are sent now, and `closing`
    /// fires once none of them is an edit. True: it waits; false: nothing to wait for.
    pub(super) fn close_when_saved(
        &mut self,
        space: &str,
        closing: Closing,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let notes = match &closing {
            Closing::Tab { note, .. } => vec![*note],
            Closing::Block(block) => self
                .space_named(space)
                .and_then(|s| s.blocks.get(block))
                .and_then(Body::note_view)
                .map(|n| n.ids())
                .unwrap_or_default(),
        };
        let Some(remote) = self.remote_mut(space) else {
            return false;
        };
        if !remote.has_edits(notes.iter().copied()) {
            return false;
        }
        for note in notes {
            if let Some(edit) = remote.edits.get_mut(&note) {
                edit.flush = true;
            }
        }
        remote.closing.push(closing);
        self.note_steps(space, Instant::now(), window, cx);
        true
    }

    /// A key that would type in the note of `block` does nothing: told once per block and reason.
    pub(crate) fn tell_note_locked(
        &mut self,
        block: BlockId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(note) = self
            .space
            .blocks
            .get(&block)
            .and_then(Body::note_view)
            .and_then(|n| n.tab())
            .map(|t| t.id)
        else {
            return;
        };
        let Some(remote) = self.space.remote.as_mut() else {
            return;
        };
        let (Some(home), Some((tag, sentence))) = (remote.home_of(block), remote.note_locked(note))
        else {
            return;
        };
        // A key on a note whose copy was refused tries the copy again.
        let retry = remote.edits.get_mut(&note).is_some_and(|edit| {
            edit.copy_refused = 0;
            edit.stuck.is_some()
        });
        if remote.told.insert((home, tag)) {
            self.warn(sentence, window, cx);
        }
        if retry {
            let space = self.space.id.clone();
            self.note_steps(&space, Instant::now(), window, cx);
        }
    }
}

/// How long a new tab asked of home is waited for, to be shown with the keyboard.
const TAB_WAIT: Duration = Duration::from_secs(5);

impl Workspace {
    /// A person's act on a note of the mirrored workspace `space`, through the one door: home's
    /// tool does it, asked of the rule first (a refusal says its words and sends nothing). A
    /// rename, a color and a closed tab show at once and stand over home's state until it shows
    /// them; a tab or a block added comes with home's state. Nothing is written here.
    pub(super) fn remote_note_act(
        &mut self,
        space: &str,
        block: Option<BlockId>,
        act: NoteAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let deed = match &act {
            NoteAct::New
            | NoteAct::Close { .. }
            | NoteAct::Rename { .. }
            | NoteAct::Color { .. } => Deed::EditNote,
            NoteAct::Open { .. } => Deed::OpenNote,
            NoteAct::Delete { .. } => Deed::DeleteNote,
        };
        let Some(remote) = self.space_named(space).and_then(|s| s.remote.as_deref()) else {
            return;
        };
        if let Some(nay) = remote.why_not(deed) {
            let words = nay.words(&crate::mirror::computer_of(remote));
            return self.warn(words, window, cx);
        }
        if let (NoteAct::Close { note }, Some(block)) = (&act, block)
            && self.close_when_saved(space, Closing::Tab { block, note: *note }, window, cx)
        {
            return;
        }
        // Where a note opened in no block goes in the mirror's view.
        let spot = matches!(act, NoteAct::Open { .. })
            .then(|| {
                block
                    .is_none()
                    .then(|| self.room_in_view(note::SIZE, window, cx).origin())
            })
            .flatten();
        let Some(remote) = self.space_named(space).and_then(|s| s.remote.as_deref()) else {
            return;
        };
        let home = block.and_then(|b| remote.home_of(b));
        let titled = |note: u64| remote.notes.get(note).map(|n| n.name.clone());
        enum Hold {
            Name(u64, String),
            Color(u64, Option<Label>),
            Tab(u64),
        }
        let (tool, args, hold) = match &act {
            NoteAct::New => {
                let Some(home) = home else { return };
                ("new_note", json!({ "block": home }), None)
            }
            NoteAct::Close { note } => {
                let Some(home) = home else { return };
                (
                    "close_note",
                    json!({ "block": home, "note": note }),
                    Some(Hold::Tab(*note)),
                )
            }
            NoteAct::Rename { note, name } => {
                let Some(home) = home else { return };
                // As home names it: its first line, Untitled when blank.
                let shown = notes::title(&super::bus::clean_title(name));
                if titled(*note).is_none_or(|n| n == shown) {
                    return;
                }
                (
                    "edit_note",
                    json!({ "block": home, "note": note, "name": name.trim() }),
                    Some(Hold::Name(*note, shown)),
                )
            }
            NoteAct::Color { note, color } => {
                let Some(home) = home else { return };
                if remote.notes.get(*note).is_none_or(|n| n.color == *color) {
                    return;
                }
                let word = color.map_or("none", Label::name);
                (
                    "edit_note",
                    json!({ "block": home, "note": note, "color": word }),
                    Some(Hold::Color(*note, *color)),
                )
            }
            NoteAct::Open { note } => match home {
                Some(home) => ("open_note", json!({ "note": note, "block": home }), None),
                None => {
                    let Some(workspace) = remote.home_workspace() else {
                        return;
                    };
                    let mut args = json!({ "note": note, "workspace": workspace });
                    if let Some(at) = spot
                        && remote.takes("open_note", "x")
                    {
                        args["x"] = at.x.into();
                        args["y"] = at.y.into();
                    }
                    ("open_note", args, None)
                }
            },
            NoteAct::Delete { note } => ("delete_note", json!({ "note": note }), None),
        };
        let now = Instant::now();
        // The tabs the block shows now, for the new one to be told from them.
        let tabs: Vec<u64> = block
            .and_then(|b| {
                self.space_named(space)?
                    .blocks
                    .get(&b)?
                    .note_view()
                    .map(|n| n.ids())
            })
            .unwrap_or_default();
        let kept = (self.space.id == space)
            .then_some(self.space.focused)
            .flatten();
        let held = hold.as_ref().map(|h| match h {
            Hold::Name(note, _) | Hold::Color(note, _) | Hold::Tab(note) => *note,
        });
        if let Some(remote) = self.remote_mut(space) {
            match hold {
                Some(Hold::Name(note, name)) => remote.notes_renamed.hold(note, name, now),
                Some(Hold::Color(note, color)) => remote.notes_colored.hold(note, color, now),
                Some(Hold::Tab(note)) => remote.tabs_closed.hold(note, true, now),
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
        let id = space.to_string();
        let asked_tab =
            matches!(act, NoteAct::New) || (matches!(act, NoteAct::Open { .. }) && block.is_some());
        let new_block = matches!(act, NoteAct::Open { .. }) && block.is_none();
        let deleted = act;
        let then: Then = Box::new(move |ws, answered, window, cx| match answered {
            Ok(answer) => {
                // A note deleted at home has no text to keep.
                if let (NoteAct::Delete { note }, Some(remote)) = (&deleted, ws.remote_mut(&id)) {
                    remote.edits.remove(note);
                }
                if asked_tab && let (Some(block), Some(remote)) = (block, ws.remote_mut(&id)) {
                    remote.tab_awaited = Some((block, tabs, kept, Instant::now()));
                }
                if new_block {
                    ws.awaiting(&id, &answer, kept, window, cx);
                }
            }
            Err(refusal) => {
                if let (Some(note), Some(remote)) = (held, ws.remote_mut(&id)) {
                    remote.notes_renamed.end(&note);
                    remote.notes_colored.end(&note);
                    remote.tabs_closed.end(&note);
                }
                ws.remirror(&id, Instant::now());
                ws.warn(refusal.sentence, window, cx);
            }
        });
        self.remote_call(space, call, then, cx);
        if self.space.id == space {
            self.mark_remote();
        }
    }

    /// The tab home was asked for (`tab_awaited`) is shown with the keyboard once it is in the
    /// block, if the keyboard is where it was and `TAB_WAIT` has not passed.
    pub(super) fn show_awaited_tab(
        &mut self,
        space: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((block, before, kept, at)) = self
            .space_named(space)
            .and_then(|s| s.remote.as_deref())
            .and_then(|r| r.tab_awaited.clone())
        else {
            return;
        };
        let now_tabs = self
            .space_named(space)
            .and_then(|s| s.blocks.get(&block))
            .and_then(Body::note_view)
            .map(|n| n.ids());
        let new = now_tabs.and_then(|tabs| tabs.iter().position(|id| !before.contains(id)));
        let shown = self.space.id == space;
        let waiting = shown && self.space.focused == kept && at.elapsed() < TAB_WAIT;
        if !waiting {
            if let Some(remote) = self.remote_mut(space) {
                remote.tab_awaited = None;
            }
            return;
        }
        let Some(new) = new else {
            return;
        };
        if let Some(remote) = self.remote_mut(space) {
            remote.tab_awaited = None;
        }
        if let Some(note) = self
            .space
            .blocks
            .get_mut(&block)
            .and_then(Body::note_view_mut)
        {
            note.active = new;
        }
        self.focus_block(block, window, cx);
    }
}

impl Workspace {
    /// The Notes dialog's click on note `id`: on this computer's workspace `open_note`; on a
    /// mirror, where it is open the tab is shown (the mirror's own view, nothing is called), else
    /// home opens it in the block `into` or in a new one.
    pub(crate) fn open_note_here(
        &mut self,
        id: u64,
        into: Option<BlockId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.on_remote() {
            return self.open_note(id, into, window, cx);
        }
        let space = self.space.id.clone();
        let open = self.space.blocks.iter().find_map(|(block, body)| {
            let at = body.note_view()?.tabs.iter().position(|t| t.id == id)?;
            Some((*block, at))
        });
        match open {
            Some((block, at)) => {
                if let Some(note) = self
                    .space
                    .blocks
                    .get_mut(&block)
                    .and_then(Body::note_view_mut)
                {
                    note.active = at;
                }
                self.reveal(&space, block, window, cx);
            }
            None => {
                let into =
                    into.filter(|b| self.space.blocks.get(b).and_then(Body::note_view).is_some());
                self.remote_note_act(&space, into, NoteAct::Open { note: id }, window, cx)
            }
        }
    }

    /// The Notes dialog's trash on note `id`: this computer's `delete_note`, or home's.
    pub(crate) fn delete_note_here(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.on_remote() {
            return self.delete_note(id, window, cx);
        }
        let space = self.space.id.clone();
        self.remote_note_act(&space, None, NoteAct::Delete { note: id }, window, cx)
    }
}
