//! A note of a mirrored workspace, typed in and acted on here and done at home (Templum to
//! Templum, kanban card 204; docs/plans/2026-10-26-mirror-notes.md, section 7). Time is the
//! tests': `note_steps` takes the `now` they choose.

use std::time::{Duration, Instant};

use std::sync::Arc;

use serde_json::json;
use templum_api::model::NOTE_TEXT_MOST;
use templum_api::model::{BlockView, NoteView, State};
use templum_api::store::Scope;

use super::remote::{answer, draw, live, refusal, remote_of, sent, viewer};
use super::remote_content::{changed, content_state, live_content, n1_of, note_editor, value};
use super::*;
use crate::workspace::history::Change;
use crate::workspace::holds::HOLD;
use crate::workspace::remote::Deed;
use crate::workspace::remote_notes::{MOST_WAIT, PAUSE};
use gpui_kit::component::WindowExt as _;

type Editor = Entity<gpui_kit::component::input::EditorState>;

/// Home's state with note `id` as `text` at `rev`.
fn home_note(id: u64, text: &str, rev: u64) -> State {
    changed(|s| {
        let at = s.notes.iter().position(|n| n.id == id).unwrap();
        s.notes[at] = Arc::new(NoteView {
            text: text.into(),
            rev,
            ..NoteView::clone(&s.notes[at])
        });
    })
}

/// `text` typed into `editor` at byte `at`, as keys do (their change event comes after).
fn type_at(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    editor: &Editor,
    at: usize,
    text: &str,
) {
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |e, cx| {
            e.set_selected_range(at..at, cx);
            e.insert(text.to_string(), window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
}

/// Home's `state` arrives; the mirror is live (nothing here has a client to keep it so).
fn apply_live(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    state: State,
) {
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    super::remote::apply(handle, ws, cx, state);
}

/// Every edit of the mirror takes a step at `at`.
fn step(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, at: Instant) {
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    step_as_is(handle, ws, cx, at);
}

/// `step`, the mirror's status as it is.
fn step_as_is(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    at: Instant,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.note_steps("r1", at, window, cx))
    })
    .unwrap();
}

fn edits(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> usize {
    ws.read_with(cx, |ws, _| remote_of(ws).edits.len())
}

fn ok() -> templum_api::client::Answered {
    Ok(json!({ "text": "Saved." }))
}

fn edit_note(note: u64, rev: u64, text: &str) -> (String, serde_json::Value) {
    (
        "edit_note".into(),
        json!({"block": "n1", "note": note, "rev": rev, "text": text}),
    )
}

/// A key pressed with the note block `n1` focused (the shown tab).
fn press_key(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    n1: BlockId,
    key: &str,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(n1, window, cx));
        window.render_frame(cx);
        window.press(key, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn show_tab(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    n1: BlockId,
    index: usize,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.show_note_tab(n1, index, window, cx))
    })
    .unwrap();
}

fn told(ws: &Entity<Workspace>, cx: &mut TestAppContext, starts: &str) -> usize {
    ws.read_with(cx, |ws, _| {
        ws.said.iter().filter(|s| s.starts_with(starts)).count()
    })
}

#[gpui_kit::test]
fn typing_in_a_remote_note_goes_home_after_a_pause_with_home_s_rev(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    let before = Instant::now();
    type_at(handle, cx, &editor, 3, "x");
    assert_eq!(value(&editor, cx), "onex", "every key shows at once");
    assert!(sent(&ws, cx).is_empty());
    // The tab shows " •" once home hasn't had the text for HOLD, and not before.
    ws.read_with(cx, |ws, _| {
        let remote = remote_of(ws);
        assert!(!remote.note_unsaved(9, Instant::now()));
        assert!(remote.note_unsaved(9, Instant::now() + HOLD + Duration::from_millis(50)));
    });
    step(handle, &ws, cx, before + Duration::from_millis(100));
    assert!(sent(&ws, cx).is_empty(), "before the pause nothing goes");
    let after = Instant::now();
    step(handle, &ws, cx, after + PAUSE + Duration::from_millis(1));
    assert_eq!(sent(&ws, cx), [edit_note(9, 100, "onex")]);
    // Keys while it is in flight call nothing.
    type_at(handle, cx, &editor, 4, "y");
    step(handle, &ws, cx, after + PAUSE * 4);
    assert_eq!(sent(&ws, cx).len(), 1);
    // Its answer, and home's state with the sent text and a newer rev: the keys typed meanwhile
    // go in the next send, with that rev.
    answer(handle, &ws, cx, 0, ok());
    apply_live(handle, &ws, cx, home_note(9, "onex", 101));
    assert_eq!(value(&editor, cx), "onexy");
    step(handle, &ws, cx, after + PAUSE * 8);
    assert_eq!(sent(&ws, cx), [edit_note(9, 101, "onexy")]);
    answer(handle, &ws, cx, 0, ok());
    apply_live(handle, &ws, cx, home_note(9, "onexy", 102));
    step(handle, &ws, cx, after + PAUSE * 12);
    assert_eq!(edits(&ws, cx), 0, "home has it all: the note is clean");
    assert!(sent(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn keys_held_down_still_go_every_two_seconds(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    let start = Instant::now();
    type_at(handle, cx, &editor, 3, "x");
    // A key every 400 ms: the pause never comes, yet by MOST_WAIT a send goes.
    for key in 1..=4u32 {
        let at = start + Duration::from_millis(400) * key;
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.remote_note_typed("r1", 9, at, window, cx))
        })
        .unwrap();
        assert!(sent(&ws, cx).is_empty(), "key {key}");
    }
    step(
        handle,
        &ws,
        cx,
        start + MOST_WAIT + Duration::from_millis(100),
    );
    assert_eq!(sent(&ws, cx), [edit_note(9, 100, "onex")]);
}

#[gpui_kit::test]
fn home_s_change_to_a_clean_note_keeps_the_caret(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    editor.update(cx, |e, cx| e.set_selected_range(3..3, cx));
    apply_live(handle, &ws, cx, home_note(9, "zero one", 101));
    cx.run_until_parked();
    assert_eq!(value(&editor, cx), "zero one");
    assert_eq!(editor.read_with(cx, |e, _| e.selected_range()), 8..8);
    // The write's own change event is no typing.
    step(handle, &ws, cx, Instant::now() + MOST_WAIT * 4);
    assert_eq!(edits(&ws, cx), 0);
    assert!(sent(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn home_s_change_while_typing_waits_for_the_pause_and_merges(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 0, "X");
    // The pause is the test's, not the clock's: however long home's state takes, it holds.
    let typed = Instant::now() + HOLD * 10;
    ws.update(cx, |ws, _| {
        ws.space
            .remote
            .as_mut()
            .unwrap()
            .edits
            .get_mut(&7)
            .unwrap()
            .typed = typed
    });
    apply_live(handle, &ws, cx, home_note(7, "first\nsecond\nthird", 101));
    assert_eq!(value(&editor, cx), "Xfirst\nsecond", "home's change waits");
    step(handle, &ws, cx, typed + PAUSE + Duration::from_millis(10));
    assert_eq!(value(&editor, cx), "Xfirst\nsecond\nthird");
    assert_eq!(sent(&ws, cx), [edit_note(7, 101, "Xfirst\nsecond\nthird")]);
}

#[gpui_kit::test]
fn a_change_of_both_in_one_place_keeps_both_texts(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().me.as_mut().unwrap().name = "Another computer".into()
    });
    let n1 = viewer(&ws, cx, "n1");
    show_tab(handle, &ws, cx, n1, 0);
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 8, "MINE");
    apply_live(handle, &ws, cx, home_note(7, "first\nsHOMEcond", 101));
    step(
        handle,
        &ws,
        cx,
        Instant::now() + PAUSE + Duration::from_millis(10),
    );
    assert_eq!(
        sent(&ws, cx),
        [(
            "new_note".to_string(),
            json!({"block": "n1", "name": "Plan (Another computer)", "text": "first\nseMINEcond"})
        )],
        "no edit_note: nothing of home's is written over"
    );
    // The editor is read-only while the copy is made, and a key says why.
    press_key(handle, &ws, cx, n1, "q");
    assert_eq!(value(&editor, cx), "first\nseMINEcond");
    assert_eq!(told(&ws, cx, "Keeping your version as a new note on "), 1);
    press_key(handle, &ws, cx, n1, "q");
    assert_eq!(
        told(&ws, cx, "Keeping your version as a new note on "),
        1,
        "once"
    );
    answer(handle, &ws, cx, 0, ok());
    assert_eq!(
        value(&editor, cx),
        "first\nsHOMEcond",
        "home's text is back"
    );
    assert_eq!(edits(&ws, cx), 0);
    let computer = ws.read_with(cx, |ws, _| crate::mirror::computer_of(remote_of(ws)));
    assert_eq!(
        told(
            &ws,
            cx,
            &format!(
                "{computer} changed \"Plan\" while you typed: your version is the new note \"Plan (Another computer)\" beside it."
            )
        ),
        1
    );
    // A refused copy keeps the text in the editor and says why; the note takes no keys, and the
    // copy is tried again once live, `TRIES` times, then on a key.
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    show_tab(handle, &ws, cx, n1, 0);
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 8, "MINE");
    apply_live(handle, &ws, cx, home_note(7, "first\nsHOMEcond", 101));
    step(
        handle,
        &ws,
        cx,
        Instant::now() + PAUSE + Duration::from_millis(10),
    );
    answer(handle, &ws, cx, 0, refusal("n1 is gone."));
    assert_eq!(told(&ws, cx, "n1 is gone."), 1);
    assert_eq!(
        value(&editor, cx),
        "first\nseMINEcond",
        "the text is not lost"
    );
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).note_unsaved(7, Instant::now()))
    });
    // A key says why, once, and types nothing.
    let stuck = "Your version of \"Plan\" couldn't be kept on ";
    press_key(handle, &ws, cx, n1, "q");
    press_key(handle, &ws, cx, n1, "q");
    assert_eq!(told(&ws, cx, stuck), 1, "once");
    assert_eq!(value(&editor, cx), "first\nseMINEcond");
    // Not live: nothing is tried. Live again: the copy goes again.
    let mut at = Instant::now() + HOLD * 5;
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = templum_api::client::Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        }
    });
    step_as_is(handle, &ws, cx, at);
    assert!(sent(&ws, cx).is_empty(), "not live: nothing is tried");
    for tries in 1..=3 {
        at += HOLD * 2;
        step(handle, &ws, cx, at);
        let calls = sent(&ws, cx);
        assert_eq!(calls.len(), 1, "try {tries}");
        assert_eq!(calls[0].0, "new_note");
        answer(handle, &ws, cx, 0, refusal("n1 is gone."));
    }
    // Refused `TRIES` times in a row: no more, until a key.
    at += HOLD * 2;
    step(handle, &ws, cx, at);
    assert!(sent(&ws, cx).is_empty(), "no fourth try");
    press_key(handle, &ws, cx, n1, "q");
    step(handle, &ws, cx, at + HOLD * 2);
    assert_eq!(sent(&ws, cx).len(), 1, "a key tries again");
    answer(handle, &ws, cx, 0, ok());
    assert_eq!(edits(&ws, cx), 0);
    assert_eq!(value(&editor, cx), "first\nsHOMEcond");
}

#[gpui_kit::test]
fn a_refused_edit_is_sent_again_on_home_s_newer_text(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 0, "X");
    let far = Instant::now() + HOLD * 10;
    step(handle, &ws, cx, far);
    assert_eq!(sent(&ws, cx), [edit_note(7, 100, "Xfirst\nsecond")]);
    answer(
        handle,
        &ws,
        cx,
        0,
        refusal("Note 7 changed since: its newer text and rev are in the state."),
    );
    // Home's newer text: merged and sent with its rev, not kept as a copy.
    apply_live(handle, &ws, cx, home_note(7, "first\nsecond!", 101));
    step(handle, &ws, cx, far + HOLD);
    assert_eq!(sent(&ws, cx), [edit_note(7, 101, "Xfirst\nsecond!")]);
    // Refused three times in a row with no newer state: its sentence once, then nothing more.
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 0, "X");
    let mut at = Instant::now() + HOLD * 10;
    for _ in 0..3 {
        step(handle, &ws, cx, at);
        assert_eq!(sent(&ws, cx).len(), 1);
        answer(handle, &ws, cx, 0, refusal("Not now."));
        at += HOLD * 2;
    }
    step(handle, &ws, cx, at);
    assert!(sent(&ws, cx).is_empty(), "no fourth send");
    assert_eq!(told(&ws, cx, "Not now."), 1);
    // The next key tries again.
    type_at(handle, cx, &editor, 0, "Y");
    step(handle, &ws, cx, at + HOLD * 10);
    assert_eq!(sent(&ws, cx).len(), 1);
}

#[gpui_kit::test]
fn a_state_older_than_the_answer_doesn_t_undo_the_person_s_text(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, "x");
    let far = Instant::now() + HOLD * 10;
    step(handle, &ws, cx, far);
    answer(handle, &ws, cx, 0, ok());
    // A state that left home before the edit: the old text, the old rev.
    apply_live(handle, &ws, cx, content_state());
    step(handle, &ws, cx, far + HOLD);
    assert_eq!(value(&editor, cx), "onex", "the person's text stays");
    assert!(sent(&ws, cx).is_empty());
    assert_eq!(edits(&ws, cx), 1);
    // The state that shows it: clean.
    apply_live(handle, &ws, cx, home_note(9, "onex", 101));
    step(handle, &ws, cx, far + HOLD * 2);
    assert_eq!(edits(&ws, cx), 0);
    assert_eq!(value(&editor, cx), "onex");
}

#[gpui_kit::test]
fn a_note_closed_at_home_while_typing_becomes_a_new_note(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().me.as_mut().unwrap().name = "Another computer".into()
    });
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, "x");
    // Home closed note 9's tab in n1.
    let closed = changed(|s| {
        n1_of(s).notes.as_mut().unwrap().tabs = vec![7];
        n1_of(s).notes.as_mut().unwrap().shown = Some(7);
        s.notes[1] = Arc::new(NoteView {
            text: String::new(),
            workspace: Some("w1".into()),
            ..NoteView::clone(&s.notes[1])
        });
    });
    apply_live(handle, &ws, cx, closed.clone());
    step(handle, &ws, cx, Instant::now() + HOLD);
    assert_eq!(
        sent(&ws, cx),
        [(
            "new_note".to_string(),
            json!({"block": "n1", "name": "Ideas (Another computer)", "text": "onex"})
        )]
    );
    answer(handle, &ws, cx, 0, ok());
    assert_eq!(edits(&ws, cx), 0);
    assert_eq!(told(&ws, cx, "\"Ideas\" isn't open on "), 1);
    // Its block gone too: a new note block where it was.
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, "x");
    let gone = changed(|s| {
        s.blocks.retain(|b| b.id != "n1");
        s.notes.retain(|n| n.id != 9);
    });
    apply_live(handle, &ws, cx, gone);
    step(handle, &ws, cx, Instant::now() + HOLD);
    assert_eq!(
        sent(&ws, cx),
        [(
            "add_block".to_string(),
            json!({"workspace": "w1", "kind": "note", "text": "onex", "x": 450.0, "y": 0.0})
        )]
    );
    // add_block has no name: home names the note by its first line, and the sentence doesn't
    // name one that doesn't exist.
    answer(handle, &ws, cx, 0, ok());
    assert_eq!(told(&ws, cx, "\"Ideas\" isn't open on "), 1);
    assert_eq!(told(&ws, cx, "\"Ideas (Another"), 0);
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.said
                .iter()
                .any(|s| s.ends_with("your version is a new note where it was."))
        )
    });
}

#[gpui_kit::test]
fn a_cut_note_is_read_only_and_nothing_past_256_kib_is_sent(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let cut = changed(|s| {
        s.notes[1] = Arc::new(NoteView {
            cut: true,
            ..NoteView::clone(&s.notes[1])
        })
    });
    apply_live(handle, &ws, cx, cut);
    let editor = note_editor(&ws, cx, n1, 1);
    press_key(handle, &ws, cx, n1, "q");
    assert!(value(&editor, cx).starts_with("one\n\n… The rest is on "));
    assert!(!value(&editor, cx).contains('q'));
    let sentence = "This note is longer than a mirror edits (256 KiB): edit it on ";
    assert_eq!(told(&ws, cx, sentence), 1);
    press_key(handle, &ws, cx, n1, "q");
    assert_eq!(told(&ws, cx, sentence), 1, "once");
    assert!(sent(&ws, cx).is_empty());
    // A clean note the person makes longer than 256 KiB: sent nowhere, said once.
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 0, &"x".repeat(NOTE_TEXT_MOST + 1));
    let far = Instant::now() + HOLD * 10;
    step(handle, &ws, cx, far);
    step(handle, &ws, cx, far + HOLD);
    assert!(sent(&ws, cx).is_empty());
    assert_eq!(
        told(
            &ws,
            cx,
            "This note is past 256 KiB, more than a mirror edits"
        ),
        1
    );
}

#[gpui_kit::test]
fn a_note_past_256_kib_is_never_copied_and_stays_editable(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    show_tab(handle, &ws, cx, n1, 0);
    let editor = note_editor(&ws, cx, n1, 0);
    let big = "x".repeat(NOTE_TEXT_MOST);
    type_at(handle, cx, &editor, 8, &big);
    // Home changes the note: the merge would conflict and the copy would be refused at home.
    apply_live(handle, &ws, cx, home_note(7, "first\nsHOMEcond", 101));
    let mut at = Instant::now() + HOLD * 10;
    for _ in 0..3 {
        step(handle, &ws, cx, at);
        at += HOLD * 2;
    }
    assert!(sent(&ws, cx).is_empty(), "nothing goes past 256 KiB");
    ws.read_with(cx, |ws, _| assert!(remote_of(ws).note_locked(7).is_none()));
    // The person shortens it: the copy goes now.
    editor.update(cx, |e, cx| e.set_selected_range(8..8 + big.len(), cx));
    press_key(handle, &ws, cx, n1, "backspace");
    cx.run_until_parked();
    type_at(handle, cx, &editor, 8, "MINE");
    assert_eq!(value(&editor, cx), "first\nseMINEcond");
    step(handle, &ws, cx, at + PAUSE * 2);
    let calls = sent(&ws, cx);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "new_note");
}

#[gpui_kit::test]
fn typing_while_not_connected_sends_nothing_and_the_rest_goes_when_live(cx: &mut TestAppContext) {
    use templum_api::client::Status;
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, "x");
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        }
    });
    let far = Instant::now() + HOLD * 10;
    step_as_is(handle, &ws, cx, far);
    assert!(sent(&ws, cx).is_empty(), "nothing goes, nothing is dropped");
    assert_eq!(edits(&ws, cx), 1);
    press_key(handle, &ws, cx, n1, "q");
    assert_eq!(value(&editor, cx), "onex");
    let words = "Not connected: what you type isn't sent.";
    assert_eq!(told(&ws, cx, words), 1);
    press_key(handle, &ws, cx, n1, "q");
    assert_eq!(told(&ws, cx, words), 1, "once");
    // Live again, with a snapshot at the same rev: the text goes, checked against that rev.
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    apply_live(handle, &ws, cx, content_state());
    step(handle, &ws, cx, far + HOLD);
    assert_eq!(sent(&ws, cx), [edit_note(9, 100, "onex")]);
}

fn click_id(handle: AnyWindowHandle, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) {
    let id = id.into();
    draw(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn tabs(ws: &Entity<Workspace>, cx: &mut TestAppContext, n1: BlockId) -> Vec<u64> {
    ws.read_with(cx, |ws, _| ws.space.blocks[&n1].note_view().unwrap().ids())
}

fn copy_name(ws: &Entity<Workspace>, cx: &mut TestAppContext, note: u64) -> String {
    ws.read_with(cx, |ws, _| {
        remote_of(ws).notes.get(note).unwrap().name.clone()
    })
}

fn after_hold() -> Instant {
    Instant::now() + HOLD + Duration::from_millis(10)
}

#[gpui_kit::test]
fn the_tabs_rename_and_color_call_home_and_show_at_once(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let b1 = viewer(&ws, cx, "b1");
    let own = ws.read_with(cx, |ws, _| ws.notes.clone());
    // + makes a new note at home.
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    click_id(handle, cx, ("note-tab-new", n1.0 as usize));
    assert_eq!(
        sent(&ws, cx),
        [("new_note".to_string(), json!({"block": "n1"}))]
    );
    answer(handle, &ws, cx, 0, ok());
    // × closes the tab at home: it goes at once, and comes back on a refusal.
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    show_tab(handle, &ws, cx, n1, 0);
    click_id(
        handle,
        cx,
        gpui_kit::ElementId::Name(format!("note-tab-close-{}-0", n1.0).into()),
    );
    assert_eq!(
        sent(&ws, cx),
        [("close_note".to_string(), json!({"block": "n1", "note": 7}))]
    );
    assert_eq!(tabs(&ws, cx, n1), [9], "gone at once");
    ws.read_with(cx, |ws, _| {
        assert!(matches!(ws.space.undo.last(), Some((_, Change::Mark))));
    });
    answer(handle, &ws, cx, 0, refusal("n1 shows no note 7."));
    assert_eq!(tabs(&ws, cx, n1), [7, 9], "back on a refusal");
    assert_eq!(told(&ws, cx, "n1 shows no note 7."), 1);
    // Closed, and home never shows it: back after HOLD.
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    show_tab(handle, &ws, cx, n1, 0);
    click_id(
        handle,
        cx,
        gpui_kit::ElementId::Name(format!("note-tab-close-{}-0", n1.0).into()),
    );
    assert_eq!(tabs(&ws, cx, n1), [9]);
    ws.update(cx, |ws, _| ws.expire_holds(after_hold()));
    assert_eq!(tabs(&ws, cx, n1), [7, 9], "home never closed it");
    // Home's state shows it closed: the hold ends and the tab stays gone.
    answer(handle, &ws, cx, 0, ok());
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    show_tab(handle, &ws, cx, n1, 0);
    click_id(
        handle,
        cx,
        gpui_kit::ElementId::Name(format!("note-tab-close-{}-0", n1.0).into()),
    );
    let closed = changed(|s| {
        n1_of(s).notes.as_mut().unwrap().tabs = vec![9];
        s.notes[0] = Arc::new(NoteView {
            text: String::new(),
            workspace: Some("w1".into()),
            ..NoteView::clone(&s.notes[0])
        });
    });
    apply_live(handle, &ws, cx, closed);
    assert_eq!(tabs(&ws, cx, n1), [9]);
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).tabs_closed.get(&7).is_none())
    });
    // Rename: a double click's field, typed and Enter.
    apply_live(handle, &ws, cx, content_state());
    sent(&ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().closing.clear()
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.rename_note_tab(n1, 0, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_input(handle, "Plan B");
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let last = sent(&ws, cx).pop().unwrap();
    assert_eq!(
        last,
        (
            "edit_note".to_string(),
            json!({"block": "n1", "note": 7, "name": "Plan B"})
        )
    );
    assert_eq!(copy_name(&ws, cx, 7), "Plan B", "shown at once");
    ws.update(cx, |ws, _| ws.expire_holds(after_hold()));
    assert_eq!(copy_name(&ws, cx, 7), "Plan", "home never renamed it");
    // Color ▸ Blue tints at once, until home's state shows it.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, _| live(ws, Scope::Control));
        ws.update(cx, |ws, cx| {
            ws.set_note_color(n1, Some(crate::board::Label::Blue), window, cx)
        });
    })
    .unwrap();
    let last = sent(&ws, cx).pop().unwrap();
    assert_eq!(
        last,
        (
            "edit_note".to_string(),
            json!({"block": "n1", "note": 7, "color": "blue"})
        )
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            remote_of(ws).notes.get(7).unwrap().color,
            Some(crate::board::Label::Blue)
        );
        // Ctrl+Z on the canvas stops at the mark.
        assert!(matches!(ws.space.undo.last(), Some((_, Change::Mark))));
        assert_eq!(
            ws.notes, own,
            "nothing of home's entered this computer's notes"
        );
    });
    // Copy all copies the shown note; Send to ▸ lists home's terminals only.
    ws.read_with(cx, |ws, cx| {
        let note = ws.space.blocks[&n1].note_view().unwrap();
        assert_eq!(note.text(cx), "first\nsecond");
        assert_eq!(ws.remote_terminals(), [(b1, "b1".to_string())]);
    });
}

#[gpui_kit::test]
fn a_tab_with_unsaved_text_closes_once_home_has_it(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    show_tab(handle, &ws, cx, n1, 0);
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 0, "X");
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    click_id(
        handle,
        cx,
        gpui_kit::ElementId::Name(format!("note-tab-close-{}-0", n1.0).into()),
    );
    // The text goes first, flushed; the tab stays, with its " •".
    assert_eq!(sent(&ws, cx), [edit_note(7, 100, "Xfirst\nsecond")]);
    assert_eq!(tabs(&ws, cx, n1), [7, 9]);
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    answer(handle, &ws, cx, 0, ok());
    assert!(
        sent(&ws, cx).is_empty(),
        "no close before the state shows it"
    );
    apply_live(handle, &ws, cx, home_note(7, "Xfirst\nsecond", 101));
    assert_eq!(
        sent(&ws, cx),
        [("close_note".to_string(), json!({"block": "n1", "note": 7}))]
    );
    // The same for closing the block.
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, "x");
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remote_close(vec![n1], window, cx))
    })
    .unwrap();
    assert_eq!(sent(&ws, cx), [edit_note(9, 100, "onex")]);
    answer(handle, &ws, cx, 0, ok());
    assert!(sent(&ws, cx).is_empty());
    apply_live(handle, &ws, cx, home_note(9, "onex", 101));
    assert_eq!(
        sent(&ws, cx),
        [("close_block".to_string(), json!({"block": "n1"}))]
    );
}

#[gpui_kit::test]
fn the_new_tab_home_adds_is_shown_with_the_keyboard(cx: &mut TestAppContext) {
    let with_12 = || {
        changed(|s| {
            let n1 = n1_of(s).notes.as_mut().unwrap();
            n1.tabs.push(12);
            n1.shown = Some(12);
            s.notes.push(Arc::new(NoteView {
                id: 12,
                name: "Untitled".into(),
                rev: 100,
                ..NoteView::default()
            }));
        })
    };
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    ws.update(cx, |ws, cx| {
        live(ws, Scope::Control);
        ws.space.focused = None;
        cx.notify();
    });
    click_id(handle, cx, ("note-tab-new", n1.0 as usize));
    answer(handle, &ws, cx, 0, ok());
    apply_live(handle, &ws, cx, with_12());
    ws.read_with(cx, |ws, _| {
        let note = ws.space.blocks[&n1].note_view().unwrap();
        assert_eq!(note.tab().map(|t| t.id), Some(12), "the new tab is shown");
        assert_eq!(ws.space.focused, Some(n1), "with the keyboard");
    });
    // Not when the person focused another block meanwhile.
    let (handle, ws, _dir) = live_content(cx);
    let (n1, b1) = (viewer(&ws, cx, "n1"), viewer(&ws, cx, "b1"));
    ws.update(cx, |ws, cx| {
        live(ws, Scope::Control);
        ws.space.focused = None;
        cx.notify();
    });
    click_id(handle, cx, ("note-tab-new", n1.0 as usize));
    answer(handle, &ws, cx, 0, ok());
    ws.update(cx, |ws, _| ws.space.focused = Some(b1));
    apply_live(handle, &ws, cx, with_12());
    ws.read_with(cx, |ws, _| {
        let note = ws.space.blocks[&n1].note_view().unwrap();
        assert_eq!(note.tab().map(|t| t.id), Some(9), "the tab shown stays");
        assert_eq!(ws.space.focused, Some(b1));
    });
}

fn with_closed_5() -> State {
    changed(|s| {
        s.notes.push(Arc::new(NoteView {
            id: 5,
            name: "Old".into(),
            rev: 100,
            changed: 1,
            workspace: Some("w1".into()),
            ..NoteView::default()
        }));
    })
}

#[gpui_kit::test]
fn all_notes_on_a_mirror_lists_the_workspace_s_notes_and_reopens_a_closed_one(
    cx: &mut TestAppContext,
) {
    no_motion(cx);
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    apply_live(handle, &ws, cx, with_closed_5());
    let own = ws.read_with(cx, |ws, _| ws.notes.clone());
    // The rows: open for 7 and 9, closed for 5; this computer's own notes never.
    ws.read_with(cx, |ws, _| {
        let rows = crate::note::note_rows(ws, "r1", "");
        let detail = |id: u64| rows.iter().find(|r| r.id == id).map(|r| r.detail.clone());
        assert!(detail(7).unwrap().contains("Open"));
        assert!(detail(9).unwrap().contains("Open"));
        assert!(detail(5).unwrap().ends_with("Closed"), "{:?}", detail(5));
        assert_eq!(rows.len(), 3);
    });
    let open_dialog = |cx: &mut TestAppContext, into: Option<BlockId>| {
        ws.update(cx, |ws, _| live(ws, Scope::Control));
        cx.update_window(handle, |_, window, cx| {
            if window.has_active_dialog(cx) {
                window.close_dialog(cx);
            }
            ws.update(cx, |ws, cx| {
                crate::note::open_notes(ws.space.id.clone(), into, window, cx)
            });
            window.render_frame(cx);
        })
        .unwrap();
    };
    // A click on a closed one: home opens it in the block the dialog was asked from.
    open_dialog(cx, Some(n1));
    click_id(handle, cx, "note-row-5");
    assert_eq!(
        sent(&ws, cx),
        [("open_note".to_string(), json!({"note": 5, "block": "n1"}))]
    );
    answer(handle, &ws, cx, 0, ok());
    // On an open one: its tab is shown, nothing is called.
    open_dialog(cx, Some(n1));
    click_id(handle, cx, "note-row-7");
    assert!(sent(&ws, cx).is_empty());
    ws.read_with(cx, |ws, _| {
        let note = ws.space.blocks[&n1].note_view().unwrap();
        assert_eq!(note.tab().map(|t| t.id), Some(7));
    });
    // The trash on a Control share says it needs Full, and deletes nothing.
    open_dialog(cx, Some(n1));
    click_id(handle, cx, "note-delete-5");
    click_id(handle, cx, "note-delete-5");
    assert!(sent(&ws, cx).is_empty());
    let computer = ws.read_with(cx, |ws, _| crate::mirror::computer_of(remote_of(ws)));
    assert_eq!(
        told(
            &ws,
            cx,
            &format!("Needs a Full share: share again on {computer}.")
        ),
        1
    );
    // On a Full share it is home's delete_note.
    ws.update(cx, |ws, _| live(ws, Scope::Full));
    open_dialog(cx, Some(n1));
    ws.update(cx, |ws, _| live(ws, Scope::Full));
    click_id(handle, cx, "note-delete-5");
    ws.update(cx, |ws, _| live(ws, Scope::Full));
    click_id(handle, cx, "note-delete-5");
    assert_eq!(
        sent(&ws, cx),
        [("delete_note".to_string(), json!({"note": 5}))]
    );
    answer(handle, &ws, cx, 0, ok());
    // New note: the tabs' +.
    open_dialog(cx, Some(n1));
    click_id(handle, cx, "notes-new");
    assert_eq!(
        sent(&ws, cx),
        [("new_note".to_string(), json!({"block": "n1"}))]
    );
    answer(handle, &ws, cx, 0, ok());
    // The canvas menu's Notes…: a closed note into a new block where one would go.
    open_dialog(cx, None);
    click_id(handle, cx, "note-row-5");
    let (tool, args) = sent(&ws, cx).pop().expect("a call");
    assert_eq!(tool, "open_note");
    assert_eq!(
        (&args["note"], &args["workspace"]),
        (&json!(5), &json!("w1"))
    );
    assert!(args["x"].is_number() && args["y"].is_number(), "{args}");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.notes, own, "nothing entered this computer's notes")
    });
}

#[gpui_kit::test]
fn a_local_notes_dialog_does_nothing_once_a_mirror_is_shown_and_back(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let here = ws
        .read_with(cx, |ws, _| ws.order().iter().position(|s| s.id == "r1"))
        .expect("the mirror");
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    let own = ws.read_with(cx, |ws, _| ws.notes.clone());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx));
        let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
        ws.update(cx, |_, cx| crate::note::open_notes(space, None, window, cx));
        window.render_frame(cx);
        // The mirror is shown by the time the person clicks: this computer's rows, New and the
        // trash do nothing there.
        ws.update(cx, |ws, cx| ws.switch_to(here, window, cx));
        window.render_frame(cx);
        window.click("notes-new", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, "r1");
        assert_eq!(ws.space.blocks.len(), blocks);
        assert_eq!(ws.notes, own);
    });
    assert!(sent(&ws, cx).is_empty());
    assert_eq!(told(&ws, cx, "Another workspace is shown now"), 1);
    // And the mirror's dialog, once the local workspace is shown: calls nothing.
    let n1 = viewer(&ws, cx, "n1");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |_, cx| {
            crate::note::open_notes("r1".into(), Some(n1), window, cx)
        });
        window.render_frame(cx);
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx));
        window.render_frame(cx);
        window.click("note-row-7", cx);
        window.render_frame(cx);
        ws.update(cx, |_, cx| {
            crate::note::open_notes("r1".into(), Some(n1), window, cx)
        });
        window.render_frame(cx);
        window.click("notes-new", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(sent(&ws, cx).is_empty());
    ws.read_with(cx, |ws, _| assert_eq!(ws.notes, own));
    // A Rename… prompt opened on a mirror's note renames nothing once the local workspace is shown.
    let name = copy_name(&ws, cx, 7);
    assert_eq!(name, "Plan");
}

#[gpui_kit::test]
fn an_older_home_dims_every_note_edit(cx: &mut TestAppContext) {
    use templum_api::client::Tools;
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let tools = |names: &[&str]| {
        Tools::read(&serde_json::Value::Array(
            names
                .iter()
                .map(|n| json!({ "name": n, "inputSchema": { "properties": {} } }))
                .collect(),
        ))
    };
    ws.update(cx, |ws, _| {
        live(ws, Scope::Control);
        ws.space.remote.as_mut().unwrap().tools = Some(tools(&["add_block", "edit_item"]));
    });
    let computer = ws.read_with(cx, |ws, _| crate::mirror::computer_of(remote_of(ws)));
    let older = format!("{computer}'s Templum is older: update it to do this from here.");
    let editor = note_editor(&ws, cx, n1, 1);
    press_key(handle, &ws, cx, n1, "q");
    assert_eq!(value(&editor, cx), "one");
    assert_eq!(told(&ws, cx, &older), 1);
    press_key(handle, &ws, cx, n1, "q");
    assert_eq!(told(&ws, cx, &older), 1, "once");
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    ws.read_with(cx, |ws, _| {
        let gates = crate::menu::Gates::of(ws, &[Deed::AddNote, Deed::EditNote, Deed::OpenNote]);
        for deed in [Deed::AddNote, Deed::EditNote, Deed::OpenNote] {
            assert_eq!(gates.why(deed).as_deref(), Some(older.as_str()), "{deed:?}");
            assert_eq!(
                ws.why_not(deed).as_deref(),
                Some(older.as_str()),
                "{deed:?}"
            );
        }
    });
    // Nothing is sent from a tab, and the note is still live (read as home's text).
    click_id(handle, cx, ("note-tab-new", n1.0 as usize));
    assert!(sent(&ws, cx).is_empty());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_note_at(Point::new(0.0, 0.0), window, cx)
        })
    })
    .unwrap();
    assert!(sent(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn add_note_on_a_mirror_adds_a_note_block_at_home(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    let p = Point::new(120.0, -40.0);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.add_note_at(p, window, cx))
    })
    .unwrap();
    assert_eq!(
        sent(&ws, cx),
        [(
            "add_block".to_string(),
            json!({"workspace": "w1", "kind": "note", "x": 120.0, "y": -40.0})
        )]
    );
    // Its block takes the keyboard when home's state brings it.
    ws.update(cx, |ws, cx| {
        ws.space.focused = None;
        cx.notify();
    });
    answer(
        handle,
        &ws,
        cx,
        0,
        Ok(json!({"text": "Added.", "block": "n2"})),
    );
    let with_n2 = changed(|s| {
        let mut n2 = BlockView::clone(&s.blocks[1]);
        n2.id = "n2".into();
        n2.notes = Some(templum_api::model::NotesShown {
            tabs: vec![],
            shown: None,
        });
        s.blocks.push(Arc::new(n2));
    });
    apply_live(handle, &ws, cx, with_n2);
    let n2 = viewer(&ws, cx, "n2");
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(n2)));
}

#[gpui_kit::test]
fn a_send_whose_answer_was_lost_is_not_merged_on_the_old_base(cx: &mut TestAppContext) {
    let maybe = || {
        Err(templum_api::client::Refusal::maybe_done(
            "The other computer may have done it: the connection dropped before it answered.",
        ))
    };
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 5, " wor");
    let far = Instant::now() + HOLD * 10;
    step(handle, &ws, cx, far);
    assert_eq!(sent(&ws, cx), [edit_note(7, 100, "first wor\nsecond")]);
    // Home wrote it, the answer was lost, and the person kept typing at the same place.
    answer(handle, &ws, cx, 0, maybe());
    type_at(handle, cx, &editor, 9, "ld");
    apply_live(handle, &ws, cx, home_note(7, "first wor\nsecond", 101));
    step(handle, &ws, cx, far + HOLD * 2);
    assert_eq!(
        value(&editor, cx),
        "first world\nsecond",
        "the sent text isn't put in twice"
    );
    assert_eq!(sent(&ws, cx), [edit_note(7, 101, "first world\nsecond")]);
    answer(handle, &ws, cx, 0, ok());
    apply_live(handle, &ws, cx, home_note(7, "first world\nsecond", 102));
    step(handle, &ws, cx, far + HOLD * 4);
    assert_eq!(edits(&ws, cx), 0);
    // It didn't land (home's text is the old one, its rev moved): the person's text goes again.
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 0, "X");
    step(handle, &ws, cx, far);
    answer(handle, &ws, cx, 0, maybe());
    apply_live(handle, &ws, cx, home_note(7, "first\nsecond", 101));
    step(handle, &ws, cx, far + HOLD * 2);
    assert_eq!(sent(&ws, cx), [edit_note(7, 101, "Xfirst\nsecond")]);
    // Home's text is neither: a conflict, kept as a new note, never merged on the old base.
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 0, "X");
    step(handle, &ws, cx, far);
    answer(handle, &ws, cx, 0, maybe());
    apply_live(handle, &ws, cx, home_note(7, "first\nsecond!", 101));
    step(handle, &ws, cx, far + HOLD * 2);
    let calls = sent(&ws, cx);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "new_note");
    assert_eq!(calls[0].1["text"], "Xfirst\nsecond");
}

#[gpui_kit::test]
fn home_s_text_written_over_a_composition_lands_where_it_belongs(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    // A dead key pending at the end of "one": marked text, with no change event.
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |e, cx| {
            e.set_selected_range(3..3, cx);
            e.replace_and_mark_text_in_range(None, "´", None, window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    apply_live(handle, &ws, cx, home_note(9, "zero one", 101));
    cx.run_until_parked();
    assert_eq!(value(&editor, cx), "zero one", "nothing at the caret");
    step(handle, &ws, cx, Instant::now() + HOLD * 10);
    assert!(sent(&ws, cx).is_empty(), "nothing corrupted goes home");
    assert_eq!(edits(&ws, cx), 0);
}

#[gpui_kit::test]
fn typing_is_told_as_home_s_rule_says_it(cx: &mut TestAppContext) {
    let (_handle, ws, _dir) = live_content(cx);
    ws.read_with(cx, |ws, _| {
        let remote = remote_of(ws);
        assert_eq!(remote.note_locked(9), None, "a Control share types");
    });
    // Who the share is isn't known yet: not live, not "needs Full".
    ws.update(cx, |ws, _| ws.space.remote.as_mut().unwrap().me = None);
    let computer = ws.read_with(cx, |ws, _| crate::mirror::computer_of(remote_of(ws)));
    ws.read_with(cx, |ws, _| {
        let (_, words) = remote_of(ws).note_locked(9).unwrap();
        assert_eq!(words, "Not connected: what you type isn't sent.");
    });
    // A Read share: the rule's own words.
    ws.update(cx, |ws, _| live(ws, Scope::Read));
    ws.read_with(cx, |ws, _| {
        let (_, words) = remote_of(ws).note_locked(9).unwrap();
        assert_eq!(
            words,
            format!("Needs a Full share: share again on {computer}.")
        );
    });
}

#[gpui_kit::test]
fn the_wake_never_spins_on_a_due_time_already_past(cx: &mut TestAppContext) {
    use templum_api::client::{Status, Update};
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, "x");
    // Past the pause, a real clock: the send goes, its answer is awaited, and the pause's due time
    // is behind: nothing is due, so nothing is woken for (a past due is a `timer(0)`, at once).
    std::thread::sleep(PAUSE + Duration::from_millis(50));
    step(handle, &ws, cx, Instant::now());
    assert_eq!(sent(&ws, cx).len(), 1);
    let mut ticks = 0;
    while cx.executor().tick() {
        ticks += 1;
        assert!(ticks < 1000, "the wake spins");
    }
    // Not live with unsent text: still nothing is woken for, and the remote coming back sends it.
    answer(handle, &ws, cx, 0, refusal("n1 is gone."));
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        }
    });
    std::thread::sleep(HOLD + Duration::from_millis(50));
    step_as_is(handle, &ws, cx, Instant::now());
    assert!(sent(&ws, cx).is_empty());
    let mut ticks = 0;
    while cx.executor().tick() {
        ticks += 1;
        assert!(ticks < 1000, "the wake spins while not live");
    }
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.heard("r1", Update::Status(Status::Live), window, cx)
        })
    })
    .unwrap();
    assert_eq!(
        sent(&ws, cx),
        [edit_note(9, 100, "onex")],
        "live again: no key, no step of the test"
    );
}

#[gpui_kit::test]
fn a_copy_whose_answer_was_lost_is_not_made_again_by_itself(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    show_tab(handle, &ws, cx, n1, 0);
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 8, "MINE");
    apply_live(handle, &ws, cx, home_note(7, "first\nsHOMEcond", 101));
    let mut at = Instant::now() + PAUSE + Duration::from_millis(10);
    step(handle, &ws, cx, at);
    assert_eq!(sent(&ws, cx)[0].0, "new_note");
    let maybe = templum_api::client::Refusal::maybe_done(
        "The other computer may have done it: the connection dropped before it answered.",
    );
    answer(handle, &ws, cx, 0, Err(maybe));
    for _ in 0..3 {
        at += HOLD * 2;
        step(handle, &ws, cx, at);
        assert!(
            sent(&ws, cx).is_empty(),
            "a second copy would be a duplicate"
        );
    }
    assert_eq!(value(&editor, cx), "first\nseMINEcond");
    press_key(handle, &ws, cx, n1, "q");
    step(handle, &ws, cx, at + HOLD * 2);
    assert_eq!(sent(&ws, cx).len(), 1, "a key tries again");
}

#[gpui_kit::test]
fn a_note_past_256_kib_that_home_closes_is_kept_whatever_its_size(cx: &mut TestAppContext) {
    let big = "x".repeat(NOTE_TEXT_MOST);
    // Its tab closed at home, the block still there: a new note where it was (new_note refuses
    // that size).
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, &big);
    let closed = changed(|s| {
        n1_of(s).notes.as_mut().unwrap().tabs = vec![7];
        n1_of(s).notes.as_mut().unwrap().shown = Some(7);
        s.notes[1] = Arc::new(NoteView {
            text: String::new(),
            workspace: Some("w1".into()),
            ..NoteView::clone(&s.notes[1])
        });
    });
    apply_live(handle, &ws, cx, closed);
    step(handle, &ws, cx, Instant::now() + HOLD * 10);
    let calls = sent(&ws, cx);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "add_block");
    assert_eq!(
        calls[0].1["text"].as_str().map(str::len),
        Some(big.len() + 3)
    );
    // Its block gone too.
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, &big);
    let gone = changed(|s| {
        s.blocks.retain(|b| b.id != "n1");
        s.notes.retain(|n| n.id != 9);
    });
    apply_live(handle, &ws, cx, gone);
    step(handle, &ws, cx, Instant::now() + HOLD * 10);
    let calls = sent(&ws, cx);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "add_block");
    assert_eq!(
        calls[0].1["text"].as_str().map(str::len),
        Some(big.len() + 3)
    );
}

#[gpui_kit::test]
fn a_note_home_reopens_while_a_copy_is_refused_shows_the_person_s_editor(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, "x");
    let closed = changed(|s| {
        n1_of(s).notes.as_mut().unwrap().tabs = vec![7];
        n1_of(s).notes.as_mut().unwrap().shown = Some(7);
        s.notes[1] = Arc::new(NoteView {
            text: String::new(),
            workspace: Some("w1".into()),
            ..NoteView::clone(&s.notes[1])
        });
    });
    apply_live(handle, &ws, cx, closed);
    let mut at = Instant::now() + HOLD * 10;
    step(handle, &ws, cx, at);
    assert_eq!(sent(&ws, cx)[0].0, "new_note");
    answer(handle, &ws, cx, 0, refusal("Nope."));
    // Home reopens it: the tab shows the person's text, not home's stale "one".
    let reopened = changed(|s| {
        s.notes[1] = Arc::new(NoteView {
            rev: NoteView::clone(&s.notes[1]).rev + 5,
            ..NoteView::clone(&s.notes[1])
        });
    });
    apply_live(handle, &ws, cx, reopened.clone());
    let shown = note_editor(&ws, cx, n1, 1);
    assert!(shown == editor, "the edit's own editor");
    assert_eq!(value(&shown, cx), "onex");
    // The copy is tried again, lands, and a key in the tab goes on the person's text.
    at += HOLD * 2;
    step(handle, &ws, cx, at);
    let n = sent(&ws, cx).len() - 1;
    answer(handle, &ws, cx, n, ok());
    let rev = NoteView::clone(&reopened.notes[1]).rev;
    let landed = changed(|s| {
        s.notes[1] = Arc::new(NoteView {
            text: "onex".into(),
            rev: rev + 1,
            ..NoteView::clone(&s.notes[1])
        });
    });
    apply_live(handle, &ws, cx, landed);
    step(handle, &ws, cx, at + HOLD * 4);
    let shown = note_editor(&ws, cx, n1, 1);
    assert_eq!(value(&shown, cx), "onex");
    type_at(handle, cx, &shown, 0, "K");
    step(handle, &ws, cx, at + HOLD * 8);
    assert_eq!(sent(&ws, cx).last().unwrap().1["text"], "Konex");
}

#[gpui_kit::test]
fn a_copy_that_lands_after_home_reopened_the_note_shows_home_s_text(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let editor = note_editor(&ws, cx, n1, 1);
    type_at(handle, cx, &editor, 3, "x");
    let closed = changed(|s| {
        n1_of(s).notes.as_mut().unwrap().tabs = vec![7];
        n1_of(s).notes.as_mut().unwrap().shown = Some(7);
        s.notes[1] = Arc::new(NoteView {
            text: String::new(),
            workspace: Some("w1".into()),
            ..NoteView::clone(&s.notes[1])
        });
    });
    apply_live(handle, &ws, cx, closed);
    let at = Instant::now() + HOLD * 10;
    step(handle, &ws, cx, at);
    assert_eq!(sent(&ws, cx)[0].0, "new_note");
    // Home reopens it, an agent's line added, before the copy's answer.
    let reopened = changed(|s| {
        s.notes[1] = Arc::new(NoteView {
            text: "one\n\nagent".into(),
            rev: NoteView::clone(&s.notes[1]).rev + 1,
            ..NoteView::clone(&s.notes[1])
        });
    });
    apply_live(handle, &ws, cx, reopened);
    step(handle, &ws, cx, at + HOLD);
    answer(handle, &ws, cx, 0, ok());
    let shown = note_editor(&ws, cx, n1, 1);
    assert_eq!(value(&shown, cx), "one\n\nagent");
    type_at(handle, cx, &shown, 0, "K");
    step(handle, &ws, cx, at + HOLD * 4);
    assert_eq!(sent(&ws, cx), [edit_note(9, 101, "Kone\n\nagent")]);
}

#[gpui_kit::test]
fn past_256_kib_home_s_changes_still_come_in_and_a_landed_send_is_not_copied(
    cx: &mut TestAppContext,
) {
    let maybe = || {
        Err(templum_api::client::Refusal::maybe_done(
            "The other computer may have done it: the connection dropped before it answered.",
        ))
    };
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    show_tab(handle, &ws, cx, n1, 0);
    let editor = note_editor(&ws, cx, n1, 0);
    type_at(handle, cx, &editor, 0, "X");
    let far = Instant::now() + HOLD * 10;
    step(handle, &ws, cx, far);
    assert_eq!(sent(&ws, cx), [edit_note(7, 100, "Xfirst\nsecond")]);
    answer(handle, &ws, cx, 0, maybe());
    // The person pastes a lot at the end, past 256 KiB.
    let end = value(&editor, cx).len();
    type_at(handle, cx, &editor, end, &"y".repeat(NOTE_TEXT_MOST));
    // Home had written the send; then an agent appends: it shows while the note is long.
    apply_live(handle, &ws, cx, home_note(7, "Xfirst\nsecond", 101));
    step(handle, &ws, cx, far + HOLD * 2);
    apply_live(
        handle,
        &ws,
        cx,
        home_note(7, "Xfirst\nsecond\n\nagent", 102),
    );
    step(handle, &ws, cx, far + HOLD * 4);
    assert!(value(&editor, cx).contains("agent"));
    assert!(sent(&ws, cx).is_empty(), "nothing goes past 256 KiB");
    // The person removes the paste: clean at home's text, nothing copied.
    let len = value(&editor, cx).len();
    editor.update(cx, |e, cx| {
        e.set_selected_range(len - NOTE_TEXT_MOST..len, cx)
    });
    press_key(handle, &ws, cx, n1, "backspace");
    cx.run_until_parked();
    step(handle, &ws, cx, far + HOLD * 8);
    assert!(sent(&ws, cx).is_empty(), "no duplicate note");
    assert_eq!(value(&editor, cx), "Xfirst\nsecond\n\nagent");
    assert_eq!(edits(&ws, cx), 0);
}
