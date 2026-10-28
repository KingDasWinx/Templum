//! A note, a checklist and the ink of a mirrored workspace: drawn from home's state, and what is
//! done to them done at home (Templum to Templum, part 2e).

use std::sync::Arc;

use templum_api::model::{
    BlockView, ChecklistView, DrawingView, DrawingsView, ItemView, NoteView, NotesShown,
    Rect as ApiRect, State,
};

use super::remote::{apply, draw, live_terminals, remote_of, viewer};
use super::*;
use crate::checklists::{Checklists, Item, List};
use crate::notes::{Entry, Notes};
use crate::workspace::remote::Deed;
use crate::workspace::remote_content::drawings_of;
use gpui_kit::component::WindowExt as _;

/// Home's workspace `w1`: the terminal `b1` "claude", the note block `n1` (tabs 7 "Plan" and 9
/// "Ideas", showing 9), the checklist block `c1` showing list 12 "Release" (13 "Tag it", 14 "Ship"
/// done); notes 7 and 9, checklist 12; the drawings of `w1`: a blue stroke and a rect. The
/// blocks lie apart, and the ink on the empty canvas near the origin.
pub(super) fn content_state() -> State {
    let mut state = super::remote::terminals_of_home(&["b1"]);
    let mut terminal = BlockView::clone(&state.blocks[0]);
    terminal.rect.y = 600.0;
    let block = |id: &str, kind: &str, title: &str, z: usize, x: f32, w: f32, h: f32| BlockView {
        id: id.into(),
        workspace: "w1".into(),
        kind: kind.into(),
        title: title.into(),
        z,
        rect: ApiRect { x, y: 0.0, w, h },
        ..BlockView::default()
    };
    let mut n1 = block("n1", "note", "Ideas", 1, 450.0, 360.0, 280.0);
    n1.notes = Some(NotesShown {
        tabs: vec![7, 9],
        shown: Some(9),
    });
    let mut c1 = block("c1", "checklist", "Release 1/2", 2, 850.0, 320.0, 360.0);
    c1.checklist = Some(12);
    state.blocks = vec![Arc::new(terminal), Arc::new(n1), Arc::new(c1)];
    let note = |id: u64, name: &str, text: &str| {
        Arc::new(NoteView {
            id,
            name: name.into(),
            text: text.into(),
            rev: 100,
            ..NoteView::default()
        })
    };
    state.notes = vec![note(7, "Plan", "first\nsecond"), note(9, "Ideas", "one")];
    let item = |id: u64, text: &str, done: bool| ItemView {
        id,
        text: text.into(),
        done,
    };
    state.checklists = vec![Arc::new(ChecklistView {
        id: 12,
        name: "Release".into(),
        items: vec![item(13, "Tag it", false), item(14, "Ship", true)],
    })];
    state.drawings = vec![Arc::new(DrawingsView {
        id: "w1".into(),
        drawings: vec![
            DrawingView {
                kind: "stroke".into(),
                color: "blue".into(),
                width: 4.0,
                points: vec![[0.0, 0.0], [100.0, 100.0]],
                ..DrawingView::default()
            },
            DrawingView {
                kind: "rect".into(),
                color: "red".into(),
                width: 2.0,
                from: Some([200.0, 200.0]),
                to: Some([300.0, 300.0]),
                ..DrawingView::default()
            },
        ],
    })];
    state
}

/// A shown, live (Control) mirror of home's `content_state()`, the mirror's own `notes.json` and
/// `checklists.json` written once.
pub(super) fn live_content(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let (handle, ws, dir) = live_terminals(cx, &["b1"]);
    // The mirror's own note 7 and list 12 (items 13 and 14) have home's ids: a write made with
    // home's ids would land on them.
    ws.update(cx, |ws, _| {
        ws.notes = Notes {
            next_id: 7,
            notes: vec![Entry {
                id: 7,
                name: "Own".into(),
                text: "own text".into(),
                color: None,
                changed: 1,
                rev: 0,
                closed_in: None,
            }],
            ..Notes::default()
        };
        let item = |id: u64, text: &str| Item {
            id,
            text: text.into(),
            done: false,
        };
        ws.checklists = Checklists {
            next_id: 14,
            lists: vec![List {
                id: 12,
                name: "Own list".into(),
                changed: 1,
                items: vec![item(13, "own a"), item(14, "own b")],
            }],
            ..Checklists::default()
        };
    });
    apply(handle, &ws, cx, content_state());
    // At 100%, where the blocks are drawn in full: home's first view fitted them all.
    ws.update(cx, |ws, cx| {
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 1.0);
        ws.space.view = ws.space.canvas.camera;
        ws.save_canvas_now(cx)
    });
    draw(handle, cx);
    (handle, ws, dir)
}

#[gpui_kit::test]
fn home_s_ink_is_drawn_on_the_mirror_and_can_t_be_picked(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let want = drawings_of(&content_state().drawings[0].drawings);
    assert_eq!(want.len(), 2);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.ink, want);
        assert_eq!(ws.ink_at(templum_canvas::Point::new(50.0, 50.0)), None);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let o = ws.read(cx).origin;
        let (x, y) = (f32::from(o.x) + 50.0, f32::from(o.y) + 50.0);
        click(window, cx, (x, y), 1);
        assert_eq!(ws.read(cx).space.selected_ink, None);
        right_click(window, cx, (x, y));
        window.render_frame(cx);
        // The canvas's menu: an ink menu would have said it isn't there yet.
        assert!(ws.read(cx).menu.is_some());
        assert!(ws.read(cx).said.is_empty());
    })
    .unwrap();
    // A state without drawings empties the ink.
    let mut bare = content_state();
    bare.drawings.clear();
    apply(handle, &ws, cx, bare);
    ws.read_with(cx, |ws, _| assert!(ws.space.ink.is_empty()));
}

#[gpui_kit::test]
fn the_drawing_tools_say_drawing_isn_t_on_a_mirror_yet(cx: &mut TestAppContext) {
    use crate::workspace::Tool;
    let (_handle, ws, _dir) = live_content(cx);
    ws.update(cx, |ws, _| {
        let computer = crate::mirror::computer_of(remote_of(ws));
        assert_eq!(
            ws.why_not(Deed::Draw),
            Some(format!(
                "Drawing on a mirror isn't there yet: draw on {computer}."
            ))
        );
    });
    cx.update_window(_handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.set_tool(Tool::Pen, window, cx));
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.tool, Tool::Select));
    // On a local workspace the deed is free.
    let (_h, local) = open(cx);
    local.read_with(cx, |ws, _| assert_eq!(ws.why_not(Deed::Draw), None));
}

#[gpui_kit::test]
fn a_mirror_s_state_never_holds_a_remote_workspace_s_ink(cx: &mut TestAppContext) {
    let (_handle, ws, _dir) = live_content(cx);
    ws.read_with(cx, |ws, cx| {
        let state = ws.api_state(cx);
        assert!(
            state.drawings.is_empty(),
            "the mirror's own workspace has no ink: {state:?}"
        );
    });
}

// Notes, checklists and their menus on a mirror.

use super::remote::live;
use super::remote_boards::{drawn, el};
use serde_json::json;
use templum_api::client::Tools;
use templum_api::store::Scope;

/// The note editor of tab `index` of the note block `block`.
pub(super) fn note_editor(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    block: BlockId,
    index: usize,
) -> Entity<gpui_kit::component::input::EditorState> {
    ws.read_with(cx, |ws, _| {
        let note = ws.space.blocks[&block].note_view().expect("a note block");
        note.tabs[index].editor.clone()
    })
}

pub(super) fn value(
    editor: &Entity<gpui_kit::component::input::EditorState>,
    cx: &mut TestAppContext,
) -> String {
    editor.read_with(cx, |editor, _| editor.value().to_string())
}

/// `content_state()` with `change` made to its note block `n1` and the notes.
pub(super) fn changed(change: impl FnOnce(&mut State)) -> State {
    let mut state = content_state();
    change(&mut state);
    state
}

pub(super) fn n1_of(state: &mut State) -> &mut BlockView {
    let at = state.blocks.iter().position(|b| b.id == "n1").unwrap();
    Arc::make_mut(&mut state.blocks[at])
}

#[gpui_kit::test]
fn a_remote_note_s_shown_tab_is_the_mirror_s_own(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let shown = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let note = ws.space.blocks[&n1].note_view().unwrap();
            (note.ids(), note.tab().map(|t| t.id))
        })
    };
    assert_eq!(shown(&ws, cx), (vec![7, 9], Some(9)));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.show_note_tab(n1, 0, window, cx))
    })
    .unwrap();
    assert_eq!(shown(&ws, cx), (vec![7, 9], Some(7)));
    // Home shows 7, then 9 again: the mirror's choice stays.
    for home_shows in [7, 9] {
        let state = changed(|s| n1_of(s).notes.as_mut().unwrap().shown = Some(home_shows));
        apply(handle, &ws, cx, state);
        assert_eq!(
            shown(&ws, cx),
            (vec![7, 9], Some(7)),
            "home shows {home_shows}"
        );
    }
    // Home's block drops note 7: its tabs follow.
    let state = changed(|s| {
        let notes = n1_of(s).notes.as_mut().unwrap();
        notes.tabs = vec![9];
        notes.shown = Some(9);
    });
    apply(handle, &ws, cx, state);
    assert_eq!(shown(&ws, cx), (vec![9], Some(9)));
}

#[gpui_kit::test]
fn a_change_that_leaves_a_note_alone_doesn_t_set_its_editor_again(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let (plan, ideas) = (note_editor(&ws, cx, n1, 0), note_editor(&ws, cx, n1, 1));
    // An editor whose text is not the copy's: only a rebuilt note takes the copy's text back.
    cx.update_window(handle, |_, window, cx| {
        plan.update(cx, |editor, cx| {
            editor.set_value("marked".to_string(), window, cx)
        })
    })
    .unwrap();
    // Home's next state: the very notes it sent (the same `Arc`s) with `change` made.
    let next = |cx: &mut TestAppContext, change: &dyn Fn(&mut State)| {
        let mut state = ws.read_with(cx, |ws, _| remote_of(ws).state.clone());
        change(&mut state);
        state
    };
    // Home's block moves, its notes don't.
    let state = next(cx, &|s| n1_of(s).rect.x += 10.0);
    apply(handle, &ws, cx, state);
    assert_eq!(value(&plan, cx), "marked", "note 7 was not rebuilt");
    // Home changes note 9 alone: 9 follows, 7 still isn't set.
    let state = next(cx, &|s| {
        s.notes[1] = Arc::new(NoteView {
            text: "nine".into(),
            ..NoteView::clone(&s.notes[1])
        })
    });
    apply(handle, &ws, cx, state);
    assert_eq!(value(&ideas, cx), "nine");
    assert_eq!(value(&plan, cx), "marked");
    // Home changes note 7: it takes home's text.
    let state = next(cx, &|s| {
        s.notes[0] = Arc::new(NoteView {
            text: "seven".into(),
            ..NoteView::clone(&s.notes[0])
        })
    });
    apply(handle, &ws, cx, state);
    assert_eq!(value(&plan, cx), "seven");
}

#[gpui_kit::test]
fn a_note_home_cut_ends_with_the_rest_is_on_home(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let n1 = viewer(&ws, cx, "n1");
    let state = changed(|s| {
        s.notes[1] = Arc::new(NoteView {
            cut: true,
            ..NoteView::clone(&s.notes[1])
        })
    });
    apply(handle, &ws, cx, state);
    let computer = ws.read_with(cx, |ws, _| crate::mirror::computer_of(remote_of(ws)));
    assert_eq!(
        value(&note_editor(&ws, cx, n1, 1), cx),
        format!("one\n\n… The rest is on {computer}.")
    );
}

#[gpui_kit::test]
fn a_note_is_in_one_tab_at_most_on_a_mirror(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let state = changed(|s| {
        n1_of(s).notes = Some(NotesShown {
            tabs: vec![7, 7, 9, 55],
            shown: Some(7),
        });
        let mut n2 = BlockView::clone(&s.blocks[1]);
        n2.id = "n2".into();
        n2.z = 3;
        n2.rect.x = 450.0;
        n2.rect.y = 300.0;
        n2.notes = Some(NotesShown {
            tabs: vec![7],
            shown: Some(7),
        });
        s.blocks.push(Arc::new(n2));
    });
    apply(handle, &ws, cx, state);
    let (n1, n2) = (viewer(&ws, cx, "n1"), viewer(&ws, cx, "n2"));
    ws.read_with(cx, |ws, _| {
        let tabs = |id: BlockId| ws.space.blocks[&id].note_view().unwrap().ids();
        assert_eq!(tabs(n1), [7, 9]);
        assert!(tabs(n2).is_empty());
        assert_eq!(remote_of(ws).note_editors.len(), 2);
    });
    draw(handle, cx);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find(("note-empty", n2.0 as usize)).is_some(),
            "{n2:?}"
        );
        assert!(window.try_find(("note-empty", n1.0 as usize)).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn new_note_from_a_local_notes_dialog_adds_nothing_once_a_mirror_is_shown(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = super::remote::offline(cx);
    let block = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.new_note(window, cx);
                let block = ws.space.canvas.top().unwrap();
                crate::note::open_notes(ws.space.id.clone(), Some(block), window, cx);
                block
            })
        })
        .unwrap();
    let own = ws.read_with(cx, |ws, _| ws.notes.clone());
    // An agent shows the mirror under the open dialog.
    super::remote::show_remote(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        std::thread::sleep(std::time::Duration::from_millis(300));
        window.render_frame(cx);
        window.within("dialog").click("notes-new", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.on_remote());
        assert_eq!(ws.notes, own, "no blank note was left in notes.json");
    });
    // The guard is the method's own: the block of the local workspace, asked for directly.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_note_tab(block, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.notes, own));
}

#[gpui_kit::test]
fn an_item_of_another_block_dropped_on_a_remote_checklist_moves_nothing(cx: &mut TestAppContext) {
    let (_handle, ws, _dir) = live_content(cx);
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    let copy_before = copy(&ws, cx);
    // A drag that began on a local list 12, in another block, ends on home's.
    let c1 = viewer(&ws, cx, "c1");
    let drag = crate::checklist::ItemDrag {
        block: BlockId(c1.0 + 100),
        list: 12,
        item: 14,
    };
    cx.update_window(_handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::checklist::move_item(ws, c1, 12, &drag, Some(13), window, cx)
        })
    })
    .unwrap();
    untouched(&ws, cx, &own);
    assert_eq!(copy(&ws, cx), copy_before);
    assert!(sent(&ws, cx).is_empty(), "nothing goes home");
}

#[gpui_kit::test]
fn a_remote_checklist_draws_home_s_items(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    ws.read_with(cx, |ws, cx| {
        let body = &ws.space.blocks[&c1];
        let checklist = body.checklist_view().expect("a checklist");
        assert!(checklist.remote && body.checklist().is_none());
        assert_eq!(checklist.list, Some(12));
        assert_eq!(crate::block::title(ws, body, cx), "Release 1/2");
    });
    for item in [13, 14] {
        assert!(drawn(handle, cx, "checklist-box", c1, item), "box {item}");
        assert!(drawn(handle, cx, "checklist-text", c1, item), "text {item}");
    }
    // Home shows none there: a new one, or one of the dialog, as locally.
    let state = changed(|s| {
        let at = s.blocks.iter().position(|b| b.id == "c1").unwrap();
        Arc::make_mut(&mut s.blocks[at]).checklist = None;
    });
    apply(handle, &ws, cx, state);
    draw(handle, cx);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window
                .try_find(("checklist-empty", c1.0 as usize))
                .is_some()
        );
        assert!(
            window
                .try_find(("checklist-new-here", c1.0 as usize))
                .is_some()
        );
        assert!(window.try_find(("checklist-list", c1.0 as usize)).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn no_local_tool_sees_a_remote_note_or_checklist(cx: &mut TestAppContext) {
    // The mirror's own first workspace: a note block showing a local note 7 "Mine" and a checklist
    // block showing a local list 12 "Local", whose ids are home's too.
    let (handle, ws, _dir) = super::remote::offline(cx);
    ws.update(cx, |ws, _| {
        ws.notes.next_id = 6;
        ws.checklists.next_id = 11;
    });
    let local_note = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let space = ws.space.id.clone();
                let new = super::super::ops::NewBlock::Note {
                    text: "Mine".into(),
                    open: None,
                };
                let place = super::super::ops::Place::Free;
                ws.add_block_in(&space, new, place, None, window, cx)
            })
        })
        .unwrap()
        .unwrap();
    let (local_list_block, list) = checklist_block(handle, &ws, cx, "Local", &["a", "b"]);
    assert_eq!(list, 12);
    let local_space = ws.update(cx, |ws, _| {
        assert_eq!(ws.notes.get(7).unwrap().name, "Mine");
        ws.edit_checklist(12, |c| c.set_text(12, 13, "changed", 1));
        ws.space.id.clone()
    });
    super::remote::show_remote(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        live(ws, Scope::Control);
        ws.space.remote.as_mut().unwrap().entry.home = Some("w1".into());
    });
    apply(handle, &ws, cx, content_state());
    let (n1, c1) = (viewer(&ws, cx, "n1"), viewer(&ws, cx, "c1"));
    let before = ws.read_with(cx, |ws, _| (ws.notes.clone(), ws.checklists.clone()));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.note_open_at(7).map(|(s, b)| (s.id.clone(), b)),
            Some((local_space.clone(), local_note))
        );
        assert_eq!(
            ws.checklist_open_at(12).map(|(s, b)| (s.id.clone(), b)),
            Some((local_space.clone(), local_list_block))
        );
    });
    // Ctrl+Z with the remote checklist focused is the canvas's, not the local checklist's.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, _| ws.space.focused = Some(c1));
        window.press(UNDO, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.checklists.get(12).unwrap().items[0].text, "changed");
    });
    // The mirror's own API holds its own notes and checklists only, and no block of r1.
    ws.read_with(cx, |ws, cx| {
        let state = ws.api_state(cx);
        assert_eq!(
            state
                .notes
                .iter()
                .map(|n| (n.id, n.name.as_str()))
                .collect::<Vec<_>>(),
            [(7, "Mine")]
        );
        assert_eq!(
            state
                .checklists
                .iter()
                .map(|c| (c.id, c.name.as_str()))
                .collect::<Vec<_>>(),
            [(12, "Local")]
        );
        assert!(state.blocks.iter().all(|b| b.workspace == local_space));
        assert_eq!((ws.notes.clone(), ws.checklists.clone()), before);
        let _ = n1;
    });
}

#[gpui_kit::test]
fn an_older_home_s_notes_and_checklists_stay_cards(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let (n1, c1) = (viewer(&ws, cx, "n1"), viewer(&ws, cx, "c1"));
    // Tools not read: live as before.
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(("note-editor", n1.0 as usize)).is_some());
        assert!(window.try_find(("checklist-list", c1.0 as usize)).is_some());
        assert!(window.try_find(("mirrored", n1.0 as usize)).is_none());
    })
    .unwrap();
    ws.update(cx, |ws, _| {
        let tools = json!([{ "name": "move_block", "inputSchema": { "properties": {} } }]);
        ws.space.remote.as_mut().unwrap().tools = Some(Tools::read(&tools));
    });
    draw(handle, cx);
    cx.update_window(handle, |_, window, _| {
        for block in [n1, c1] {
            assert!(window.try_find(("mirrored", block.0 as usize)).is_some());
        }
        assert!(window.try_find(("note-editor", n1.0 as usize)).is_none());
        assert!(window.try_find(("checklist-list", c1.0 as usize)).is_none());
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let said = ws.older_content().unwrap();
        assert!(
            said.ends_with("'s Templum is older: update it to see this here."),
            "{said}"
        );
    });
}

#[gpui_kit::test]
fn a_kanban_s_all_boards_is_dimmed_when_the_mirror_can_t_show_boards(cx: &mut TestAppContext) {
    use super::remote_boards::{HOME, live_board};
    let (handle, ws, _dir) = live_board(cx, HOME);
    let k1 = viewer(&ws, cx, "k1");
    ws.update(cx, |ws, _| {
        let tools = json!([{ "name": "move_block", "inputSchema": { "properties": {} } }]);
        ws.space.remote.as_mut().unwrap().tools = Some(Tools::read(&tools));
    });
    draw(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click(("boards", k1.0 as usize), cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx), "its click opens no dialog");
    })
    .unwrap();
    assert!(
        ws.read_with(cx, |ws, _| ws.why_not(Deed::ShowBoard))
            .is_some_and(|w| w.ends_with("'s Templum is older: update it to do this from here."))
    );
}

// Acting on a remote checklist: check, add, edit, delete.

use super::remote::{answer, refusal, said, sent};
use crate::checklist::Field;
use crate::workspace::history::Change;
use crate::workspace::holds::HOLD;
use std::time::{Duration, Instant};

/// `content_state()` with `change` made to its checklist 12.
fn list_state(change: impl FnOnce(&mut ChecklistView)) -> State {
    changed(|s| {
        let mut list = ChecklistView::clone(&s.checklists[0]);
        change(&mut list);
        s.checklists[0] = Arc::new(list);
    })
}

pub(super) fn click_on(handle: AnyWindowHandle, cx: &mut TestAppContext, id: gpui_kit::ElementId) {
    cx.update_window(handle, |_, window, cx| {
        // Active: focus listeners (a field's blur) only hear an active window.
        window.activate_window();
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// The row of item `item` under the pointer: its × shows.
fn hover_row(handle: AnyWindowHandle, cx: &mut TestAppContext, c1: BlockId, item: u64) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover(el("checklist-row", c1, item), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// The mirror is live again: typing lets the executor run, and its client (there is none, a
/// refused address) would have moved the status on.
fn keep_live(ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    ws.update(cx, |ws, _| live(ws, Scope::Control));
}

/// Home's list 12 as the mirror's copy shows it: (item, text, done) of each.
fn copy(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<(u64, String, bool)> {
    ws.read_with(cx, |ws, _| {
        let list = remote_of(ws)
            .checklists
            .get(12)
            .expect("the copy of list 12");
        list.items
            .iter()
            .map(|i| (i.id, i.text.clone(), i.done))
            .collect()
    })
}

fn after_hold() -> Instant {
    Instant::now() + HOLD + Duration::from_millis(10)
}

/// Nothing of home's reached the mirror's own checklists.
fn untouched(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    before: &crate::checklists::Checklists,
) {
    ws.read_with(cx, |ws, _| assert_eq!(&ws.checklists, before));
}

#[gpui_kit::test]
fn a_box_checked_on_the_mirror_sends_check_item_and_shows_at_once(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    click_on(handle, cx, el("checklist-box", c1, 13));
    assert_eq!(
        sent(&ws, cx),
        [(
            "check_item".to_string(),
            json!({"block": "c1", "item": 13, "done": true})
        )]
    );
    assert!(copy(&ws, cx)[0].2, "checked at once");
    ws.read_with(cx, |ws, _| {
        assert!(matches!(ws.space.undo.last(), Some((_, Change::Mark))));
    });
    untouched(&ws, cx, &own);
    // Home's state shows it: the hold ends.
    apply(handle, &ws, cx, list_state(|l| l.items[0].done = true));
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).items_checked.get(&(12, 13)).is_none())
    });
    assert!(copy(&ws, cx)[0].2);
    // No state comes: after HOLD home's list is drawn again.
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    click_on(handle, cx, el("checklist-box", c1, 13));
    assert!(copy(&ws, cx)[0].2);
    ws.update(cx, |ws, _| ws.expire_holds(after_hold()));
    assert!(!copy(&ws, cx)[0].2, "home never checked it");
    // A refusal brings it back at once, and is said once.
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    click_on(handle, cx, el("checklist-box", c1, 13));
    answer(
        handle,
        &ws,
        cx,
        0,
        refusal("There's no item 13 in \"Release\"."),
    );
    assert!(!copy(&ws, cx)[0].2);
    assert_eq!(said(&ws, cx), ["There's no item 13 in \"Release\"."]);
    // Unchecking is a check with done false.
    click_on(handle, cx, el("checklist-box", c1, 14));
    assert_eq!(
        sent(&ws, cx).last().unwrap(),
        &(
            "check_item".to_string(),
            json!({"block": "c1", "item": 14, "done": false})
        )
    );
}

#[gpui_kit::test]
fn an_item_deleted_on_the_mirror_hides_until_home_drops_it(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    hover_row(handle, cx, c1, 14);
    click_on(handle, cx, el("checklist-item-delete", c1, 14));
    assert_eq!(
        sent(&ws, cx),
        [(
            "delete_item".to_string(),
            json!({"block": "c1", "item": 14})
        )]
    );
    assert_eq!(copy(&ws, cx).len(), 1, "14 is gone at once");
    untouched(&ws, cx, &own);
    // Home drops it: the hold ends.
    apply(
        handle,
        &ws,
        cx,
        list_state(|l| l.items.retain(|i| i.id != 14)),
    );
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).items_deleted.get(&(12, 14)).is_none())
    });
    assert_eq!(copy(&ws, cx).len(), 1);
    // No state comes: it is back after HOLD.
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    hover_row(handle, cx, c1, 14);
    click_on(handle, cx, el("checklist-item-delete", c1, 14));
    ws.update(cx, |ws, _| ws.expire_holds(after_hold()));
    assert_eq!(copy(&ws, cx).len(), 2);
    // A refusal brings it back at once.
    hover_row(handle, cx, c1, 14);
    click_on(handle, cx, el("checklist-item-delete", c1, 14));
    assert_eq!(copy(&ws, cx).len(), 1);
    answer(
        handle,
        &ws,
        cx,
        1,
        refusal("There's no item 14 in \"Release\"."),
    );
    assert_eq!(copy(&ws, cx).len(), 2);
    assert_eq!(said(&ws, cx), ["There's no item 14 in \"Release\"."]);
}

#[gpui_kit::test]
fn an_item_edited_on_the_mirror_goes_once_on_enter_and_esc_drops_it(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    click_on(handle, cx, el("checklist-text", c1, 13));
    cx.simulate_input(handle, " v1");
    keep_live(&ws, cx);
    assert!(sent(&ws, cx).is_empty(), "nothing goes per key");
    assert_eq!(copy(&ws, cx)[0].1, "Tag it", "the copy waits for Enter");
    untouched(&ws, cx, &own);
    // Home renames another item meanwhile: the field keeps what was typed.
    apply(
        handle,
        &ws,
        cx,
        list_state(|l| l.items[1].text = "Ship now".into()),
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    assert_eq!(
        sent(&ws, cx),
        [(
            "edit_item".to_string(),
            json!({"block": "c1", "item": 13, "text": "Tag it v1"})
        )]
    );
    assert_eq!(copy(&ws, cx)[0].1, "Tag it v1", "held at once");
    assert_eq!(copy(&ws, cx)[1].1, "Ship now");
    // Esc drops the field: nothing more is sent.
    click_on(handle, cx, el("checklist-text", c1, 13));
    cx.simulate_input(handle, "x");
    keep_live(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(sent(&ws, cx).len(), 1);
    assert_eq!(copy(&ws, cx)[0].1, "Tag it v1");
    // A blank text sends nothing and the item stays.
    click_on(handle, cx, el("checklist-text", c1, 13));
    cx.update_window(handle, |_, window, cx| {
        window.press("secondary-a", cx);
        window.press("delete", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(sent(&ws, cx).len(), 1);
    assert_eq!(copy(&ws, cx).len(), 2);
    untouched(&ws, cx, &own);
}

#[gpui_kit::test]
fn add_an_item_on_the_mirror_sends_add_items_and_opens_the_next(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    click_on(
        handle,
        cx,
        gpui_kit::ElementId::from(("checklist-add", c1.0 as usize)),
    );
    let editing = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.blocks[&c1]
                .checklist_view()
                .unwrap()
                .editing
                .as_ref()
                .map(|e| e.field)
        })
    };
    assert_eq!(editing(&ws, cx), Some(Field::New));
    assert_eq!(copy(&ws, cx).len(), 2, "no item is added here");
    cx.simulate_input(handle, "Release notes");
    keep_live(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        sent(&ws, cx),
        [(
            "add_items".to_string(),
            json!({"block": "c1", "text": "Release notes"})
        )]
    );
    assert_eq!(editing(&ws, cx), Some(Field::New), "the next one opens");
    // Enter on the blank one: nothing is sent and the field closes.
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(sent(&ws, cx).len(), 1);
    assert_eq!(editing(&ws, cx), None);
    // Typed and left (a click elsewhere): it goes too.
    click_on(
        handle,
        cx,
        gpui_kit::ElementId::from(("checklist-add", c1.0 as usize)),
    );
    cx.simulate_input(handle, "x");
    keep_live(&ws, cx);
    ws.update(cx, |ws, _| ws.space.focused = None);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| window.focus(&ws.focus_handle, cx));
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        sent(&ws, cx).last().unwrap(),
        &("add_items".to_string(), json!({"block": "c1", "text": "x"}))
    );
    untouched(&ws, cx, &own);
}

#[gpui_kit::test]
fn an_item_field_left_by_switching_workspaces_sends_to_its_own_remote(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    click_on(handle, cx, el("checklist-text", c1, 13));
    cx.simulate_input(handle, " v2");
    keep_live(&ws, cx);
    // An agent's `show`: the local workspace is shown, the field loses the keyboard.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    let calls = ws.read_with(cx, |ws, _| {
        ws.calls
            .iter()
            .map(|(space, call, _)| (space.clone(), call.tool.clone(), call.args.clone()))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        calls,
        [(
            "r1".to_string(),
            "edit_item".to_string(),
            json!({"block": "c1", "item": 13, "text": "Tag it v2"})
        )]
    );
    untouched(&ws, cx, &own);
}

#[gpui_kit::test]
fn a_remote_checklist_dims_what_it_can_t_do(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    // Not live: the controls are dimmed with the rule's words, and their clicks send nothing.
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = templum_api::client::Status::Connecting;
    });
    ws.read_with(cx, |ws, _| {
        for deed in [
            Deed::CheckItem,
            Deed::DeleteItem,
            Deed::AddItems,
            Deed::EditItem,
        ] {
            assert_eq!(
                ws.why_not(deed).as_deref(),
                Some("Not connected."),
                "{deed:?}"
            );
        }
    });
    click_on(handle, cx, el("checklist-box", c1, 13));
    hover_row(handle, cx, c1, 13);
    click_on(handle, cx, el("checklist-item-delete", c1, 13));
    click_on(
        handle,
        cx,
        gpui_kit::ElementId::from(("checklist-add", c1.0 as usize)),
    );
    assert!(sent(&ws, cx).is_empty());
    // The name opens no field, the dialog doesn't open, and a dropped row moves nothing.
    ws.read_with(cx, |ws, _| {
        for deed in [Deed::EditChecklist, Deed::OpenChecklist] {
            assert_eq!(
                ws.why_not(deed).as_deref(),
                Some("Not connected."),
                "{deed:?}"
            );
        }
    });
    click_on(
        handle,
        cx,
        gpui_kit::ElementId::from(("checklist-name", c1.0 as usize)),
    );
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.space.blocks[&c1]
                .checklist_view()
                .unwrap()
                .editing
                .is_none()
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.click(("checklists", c1.0 as usize), cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
    })
    .unwrap();
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    // A drop of 13 on 14 moves nothing, here or at home.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(
            el("checklist-text", c1, 13),
            el("checklist-text", c1, 14),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    assert!(sent(&ws, cx).is_empty());
    assert_eq!(
        copy(&ws, cx).iter().map(|i| i.0).collect::<Vec<_>>(),
        [13, 14]
    );
    untouched(&ws, cx, &own);
}

/// Home's list 12 as the mirror's copy names it.
fn copy_name(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> String {
    ws.read_with(cx, |ws, _| {
        remote_of(ws).checklists.get(12).unwrap().name.clone()
    })
}

pub(super) fn press_in(handle: AnyWindowHandle, cx: &mut TestAppContext, key: &str) {
    cx.update_window(handle, |_, window, cx| {
        window.press(key, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_name_renamed_on_the_mirror_goes_once_on_enter_and_shows_at_once(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    let name = || gpui_kit::ElementId::from(("checklist-name", c1.0 as usize));
    click_on(handle, cx, name());
    cx.simulate_input(handle, "Release 2");
    keep_live(&ws, cx);
    assert!(sent(&ws, cx).is_empty(), "nothing goes per key");
    assert_eq!(copy_name(&ws, cx), "Release", "the copy waits for Enter");
    press_in(handle, cx, "enter");
    assert_eq!(
        sent(&ws, cx),
        [(
            "edit_checklist".to_string(),
            json!({"block": "c1", "name": "Release 2"})
        )]
    );
    assert_eq!(copy_name(&ws, cx), "Release 2", "held at once");
    ws.read_with(cx, |ws, _| {
        assert!(matches!(ws.space.undo.last(), Some((_, Change::Mark))));
    });
    untouched(&ws, cx, &own);
    // Home's state shows it: the hold ends.
    apply(handle, &ws, cx, list_state(|l| l.name = "Release 2".into()));
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).lists_renamed.get(&12).is_none())
    });
    // The same name, and Esc, send nothing.
    click_on(handle, cx, name());
    press_in(handle, cx, "enter");
    click_on(handle, cx, name());
    cx.simulate_input(handle, "x");
    keep_live(&ws, cx);
    press_in(handle, cx, "escape");
    assert_eq!(sent(&ws, cx).len(), 1);
    assert_eq!(copy_name(&ws, cx), "Release 2");
    // A blank name is home's "Checklist".
    click_on(handle, cx, name());
    press_in(handle, cx, "delete");
    press_in(handle, cx, "enter");
    assert_eq!(
        sent(&ws, cx).last().unwrap(),
        &(
            "edit_checklist".to_string(),
            json!({"block": "c1", "name": "Checklist"})
        )
    );
    // A refusal brings home's back at once, and is said.
    answer(handle, &ws, cx, 1, refusal("c1 shows no checklist."));
    assert_eq!(copy_name(&ws, cx), "Release 2");
    assert_eq!(said(&ws, cx), ["c1 shows no checklist."]);
    // No state comes: after HOLD home's name is drawn again.
    click_on(handle, cx, name());
    cx.simulate_input(handle, "Later");
    keep_live(&ws, cx);
    press_in(handle, cx, "enter");
    assert_eq!(copy_name(&ws, cx), "Later");
    ws.update(cx, |ws, _| ws.expire_holds(after_hold()));
    assert_eq!(copy_name(&ws, cx), "Release 2", "home never renamed it");
    untouched(&ws, cx, &own);
}

#[gpui_kit::test]
fn a_row_dragged_on_the_mirror_moves_at_home_and_shows_at_once(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    let order = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        copy(ws, cx).iter().map(|i| i.0).collect::<Vec<_>>()
    };
    let drag = |cx: &mut TestAppContext, from: u64, to: gpui_kit::ElementId| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.drag_to(el("checklist-row", c1, from), to, cx);
            window.render_frame(cx);
        })
        .unwrap();
    };
    // 13 dropped on 14, the row after it: nothing changes, nothing goes.
    drag(cx, 13, el("checklist-row", c1, 14));
    assert!(sent(&ws, cx).is_empty());
    drag(cx, 14, el("checklist-row", c1, 13));
    assert_eq!(
        sent(&ws, cx),
        [(
            "edit_checklist".to_string(),
            json!({"block": "c1", "item": 14, "before": 13})
        )]
    );
    assert_eq!(order(&ws, cx), [14, 13], "in 13's place at once");
    ws.read_with(cx, |ws, _| {
        assert!(matches!(ws.space.undo.last(), Some((_, Change::Mark))));
    });
    untouched(&ws, cx, &own);
    // Home's state shows it: the hold ends.
    apply(handle, &ws, cx, list_state(|l| l.items.reverse()));
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).items_moved.get(&(12, 14)).is_none())
    });
    assert_eq!(order(&ws, cx), [14, 13]);
    // Dropped on + Add an item: at the end. No state comes: after HOLD home's order is back.
    drag(cx, 14, ("checklist-add", c1.0 as usize).into());
    assert_eq!(
        sent(&ws, cx).last().unwrap(),
        &(
            "edit_checklist".to_string(),
            json!({"block": "c1", "item": 14, "before": "end"})
        )
    );
    assert_eq!(order(&ws, cx), [13, 14]);
    ws.update(cx, |ws, _| ws.expire_holds(after_hold()));
    assert_eq!(order(&ws, cx), [14, 13], "home never moved it");
    // A refusal puts it back at once, and is said.
    drag(cx, 14, ("checklist-add", c1.0 as usize).into());
    assert_eq!(order(&ws, cx), [13, 14]);
    answer(
        handle,
        &ws,
        cx,
        2,
        refusal("There's no item 14 in \"Release\"."),
    );
    assert_eq!(order(&ws, cx), [14, 13]);
    assert_eq!(said(&ws, cx), ["There's no item 14 in \"Release\"."]);
    untouched(&ws, cx, &own);
}

#[gpui_kit::test]
fn all_checklists_on_the_mirror_lists_home_s_and_makes_a_new_one_at_home(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    // This computer's list 99 is never listed: the dialog lists home's.
    let own = ws.update(cx, |ws, _| {
        let mut list = ws.checklists.lists[0].clone();
        list.id = 99;
        ws.checklists.lists.push(list);
        ws.checklists.clone()
    });
    cx.update_window(handle, |_, window, cx| {
        window.click(("checklists", c1.0 as usize), cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        assert!(window.try_find("checklist-row-99").is_none());
        window.click("checklist-row-12", cx);
        // Showing c1 glides the camera by the clock: landed, the next click finds c1 where it
        // was drawn.
        ws.update(cx, |ws, _| ws.space.flight = None);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(c1)));
    // New checklist from a block that shows one: home adds a block beside it.
    cx.update_window(handle, |_, window, cx| {
        window.click(("checklists", c1.0 as usize), cx);
        window.render_frame(cx);
        window.click("checklists-new", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let (tool, args) = sent(&ws, cx).pop().expect("a call");
    assert_eq!(tool, "add_block");
    assert_eq!(
        (&args["kind"], &args["text"]),
        (&json!("checklist"), &json!("Checklist"))
    );
    let c1_rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(c1).unwrap().rect);
    let x = args["x"].as_f64().unwrap() as f32;
    assert!(
        x >= c1_rect.x + c1_rect.w || x + 320.0 <= c1_rect.x,
        "beside c1: {args}"
    );
    // Home shows none in c1: New checklist there is home's new_checklist into it.
    let state = changed(|s| {
        let at = s.blocks.iter().position(|b| b.id == "c1").unwrap();
        Arc::make_mut(&mut s.blocks[at]).checklist = None;
    });
    apply(handle, &ws, cx, state);
    click_on(
        handle,
        cx,
        gpui_kit::ElementId::from(("checklist-new-here", c1.0 as usize)),
    );
    assert_eq!(
        sent(&ws, cx).last().unwrap(),
        &("new_checklist".to_string(), json!({"block": "c1"}))
    );
    // Its Open a checklist… lists what the workspace's blocks show: none now.
    click_on(
        handle,
        cx,
        gpui_kit::ElementId::from(("checklist-open-here", c1.0 as usize)),
    );
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx));
        assert!(window.try_find("checklist-row-12").is_none());
        // Its trash says the mirror can't, and deletes nothing.
        window.close_dialog(cx);
    })
    .unwrap();
    untouched(&ws, cx, &own);
    // The dialog outliving its workspace: a click does nothing and says so.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |_, cx| {
            crate::checklist::open_checklists("r1".into(), Some(c1), window, cx)
        });
        window.render_frame(cx);
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx));
        window.render_frame(cx);
        window.click("checklists-new", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(sent(&ws, cx).len(), 2, "nothing more was sent");
    assert_eq!(
        said(&ws, cx).last().map(String::as_str),
        Some("Another workspace is shown now: nothing was done.")
    );
    untouched(&ws, cx, &own);
}

#[gpui_kit::test]
fn a_mirror_s_checklists_dialog_deletes_nothing(cx: &mut TestAppContext) {
    // The dialog slides in by the clock: still, the trash is where the first click found it.
    no_motion(cx);
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    let own = ws.read_with(cx, |ws, _| ws.checklists.clone());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |_, cx| {
            crate::checklist::open_checklists("r1".into(), Some(c1), window, cx)
        });
        window.render_frame(cx);
        window.click("checklist-delete-12", cx);
        window.render_frame(cx);
        window.click("checklist-delete-12", cx);
        window.render_frame(cx);
    })
    .unwrap();
    untouched(&ws, cx, &own);
    assert!(sent(&ws, cx).is_empty());
    assert_eq!(said(&ws, cx), ["Not on a mirrored workspace yet"]);
    assert!(copy(&ws, cx).len() == 2);
}

#[gpui_kit::test]
fn a_local_checklists_dialog_does_nothing_once_a_mirror_is_shown(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let here = ws.read_with(cx, |ws, _| ws.order().iter().position(|s| s.id == "r1"));
    let here = here.expect("the mirror");
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx));
        let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
        ws.update(cx, |_, cx| {
            crate::checklist::open_checklists(space, None, window, cx)
        });
        window.render_frame(cx);
        // The mirror is shown by the time the person clicks: this computer's list 12 opens
        // nothing there, and New checklist adds nothing there.
        ws.update(cx, |ws, cx| ws.switch_to(here, window, cx));
        window.render_frame(cx);
        window.click("checklist-row-12", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, "r1");
        assert_eq!(ws.space.blocks.len(), blocks);
    });
    assert!(sent(&ws, cx).is_empty());
    assert_eq!(
        said(&ws, cx).last().map(String::as_str),
        Some("Another workspace is shown now: nothing was done.")
    );
}

#[gpui_kit::test]
fn an_older_home_dims_renaming_moving_and_new_checklists(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let c1 = viewer(&ws, cx, "c1");
    ws.update(cx, |ws, _| {
        let tools = json!([
            { "name": "edit_item", "inputSchema": { "properties": {} } },
            { "name": "check_item", "inputSchema": { "properties": {} } },
        ]);
        ws.space.remote.as_mut().unwrap().tools = Some(Tools::read(&tools));
    });
    draw(handle, cx);
    ws.read_with(cx, |ws, _| {
        for deed in [Deed::EditChecklist, Deed::OpenChecklist] {
            let said = ws.why_not(deed).unwrap_or_default();
            assert!(
                said.ends_with("'s Templum is older: update it to do this from here."),
                "{deed:?}: {said}"
            );
        }
    });
    click_on(
        handle,
        cx,
        gpui_kit::ElementId::from(("checklist-name", c1.0 as usize)),
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(el("checklist-row", c1, 14), el("checklist-row", c1, 13), cx);
        window.click(("checklists", c1.0 as usize), cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.space.blocks[&c1]
                .checklist_view()
                .unwrap()
                .editing
                .is_none()
        );
    });
    assert!(sent(&ws, cx).is_empty());
    assert_eq!(
        copy(&ws, cx).iter().map(|i| i.0).collect::<Vec<_>>(),
        [13, 14]
    );
}

#[gpui_kit::test]
fn on_a_mirror_ctrl_z_after_an_item_act_stops_at_the_mark(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_content(cx);
    let (c1, b1) = (viewer(&ws, cx, "c1"), viewer(&ws, cx, "b1"));
    let moved = Rect::new(500.0, 420.0, 410.0, 310.0);
    let was = ws.read_with(cx, |ws, _| ws.space.canvas.get(b1).unwrap().rect);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(b1).unwrap().rect = moved;
        ws.remote_moved(&[b1], Instant::now());
        ws.keep(
            Change::Place(vec![(b1, was)]),
            crate::workspace::history::Then::New,
        );
    });
    click_on(handle, cx, el("checklist-box", c1, 13));
    ws.update(cx, |ws, _| ws.space.focused = Some(c1));
    cx.update_window(handle, |_, window, cx| window.press(UNDO, cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.canvas.get(b1).unwrap().rect,
            moved,
            "no block moved"
        );
        let said = ws.said.last().map(String::as_str).unwrap_or_default();
        assert!(
            said.starts_with("On a mirror, Ctrl+Z undoes moves only: undo the rest on "),
            "{said}"
        );
    });
    cx.update_window(handle, |_, window, cx| window.press(UNDO, cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.canvas.get(b1).unwrap().rect,
            was,
            "the move is undone"
        );
    });
}

// Two ends: the mirror's own files stay as they were.

use super::remote::{pump_home, shown_mirror_as};
use crate::workspace::Body;

#[gpui_kit::test]
fn acts_on_a_mirror_leave_its_notes_json_and_checklists_json_as_they_were(cx: &mut TestAppContext) {
    use crate::workspace::ChecklistAct;
    use templum_canvas::{Color, Drawing};
    let ((home_handle, home), (handle, ws), (_, note, _), [_home_dir, dir]) =
        shown_mirror_as(cx, Scope::Control);
    let (block, list) = checklist_block(home_handle, &home, cx, "Release", &["Tag it", "Ship"]);
    let (uid, note_uid) = home.read_with(cx, |ws, _| {
        (ws.space.uids[&block].clone(), ws.space.uids[&note].clone())
    });
    let (tag, ship) = home.read_with(cx, |ws, _| {
        let items = &ws.checklists.get(list).unwrap().items;
        (items[0].id, items[1].id)
    });
    pump_home(&home, cx, "the list on the mirror", |cx| {
        ws.read_with(cx, |ws, _| {
            remote_of(ws).checklists.get(list).is_some() && remote_of(ws).homes.contains_key(&uid)
        })
    });
    let c1 = viewer(&ws, cx, &uid);
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let files = [
        crate::notes::path(dir.path()),
        crate::checklists::path(dir.path()),
    ];
    let before: Vec<_> = files.iter().map(|f| std::fs::read(f).ok()).collect();
    let own = ws.read_with(cx, |ws, _| (ws.notes.clone(), ws.checklists.clone()));
    let act = |cx: &mut TestAppContext, act: ChecklistAct| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.act_on_checklist("r1", c1, act, window, cx))
        })
        .unwrap();
    };
    let items = |cx: &mut TestAppContext| -> Vec<(String, bool)> {
        home.read_with(cx, |ws, _| {
            let items = &ws.checklists.get(list).unwrap().items;
            items.iter().map(|i| (i.text.clone(), i.done)).collect()
        })
    };
    act(cx, ChecklistAct::Toggle { item: tag });
    pump_home(&home, cx, "checked", |cx| items(cx)[0].1);
    act(
        cx,
        ChecklistAct::Add {
            text: "Notes".into(),
        },
    );
    pump_home(&home, cx, "added", |cx| items(cx).len() == 3);
    act(
        cx,
        ChecklistAct::Edit {
            item: ship,
            text: "Ship it".into(),
        },
    );
    pump_home(&home, cx, "edited", |cx| items(cx)[1].0 == "Ship it");
    act(cx, ChecklistAct::Delete { item: tag });
    pump_home(&home, cx, "deleted", |cx| items(cx).len() == 2);
    // Renamed and moved from the mirror (kanban card 205), once its copy has the new item.
    pump_home(&home, cx, "the mirror's copy", |cx| {
        ws.read_with(cx, |ws, _| {
            let copy = remote_of(ws).checklists.get(list).unwrap();
            copy.items
                .iter()
                .map(|i| i.text.as_str())
                .collect::<Vec<_>>()
                == ["Ship it", "Notes"]
        })
    });
    let added = home.read_with(cx, |ws, _| ws.checklists.get(list).unwrap().items[1].id);
    act(
        cx,
        ChecklistAct::Move {
            item: added,
            before: Some(ship),
        },
    );
    pump_home(&home, cx, "moved", |cx| items(cx)[0].0 == "Notes");
    act(
        cx,
        ChecklistAct::Rename {
            name: "Release 2".into(),
        },
    );
    pump_home(&home, cx, "renamed", |cx| {
        home.read_with(cx, |ws, _| {
            ws.checklists.get(list).unwrap().name == "Release 2"
        })
    });
    // Home types in its note and draws a stroke: the mirror shows each.
    let first = home.read_with(cx, |ws, _| ws.space.id.clone());
    cx.update_window(home_handle, |_, window, cx| {
        home.update(cx, |ws, cx| {
            ws.note_replace(&first, note, "hello there", None, window, cx)
        })
    })
    .unwrap()
    .expect("replaced");
    let stroke = Drawing::Stroke {
        points: vec![Point::new(0.0, 0.0), Point::new(80.0, 40.0)],
        width: 4.0,
        color: Color::Blue,
        highlight: false,
    };
    home.update(cx, |ws, _| ws.space.ink.push(stroke.clone()));
    pump_home(&home, cx, "the mirror to show each", |cx| {
        ws.read_with(cx, |ws, cx| {
            let text = remote_of(ws)
                .homes
                .get(&note_uid)
                .and_then(|id| ws.space.blocks.get(id))
                .and_then(Body::note_view)
                .and_then(|n| n.tab())
                .map(|t| t.editor.read(cx).value().to_string());
            let list = remote_of(ws).checklists.get(list).unwrap();
            text.as_deref() == Some("hello there")
                && list.items.len() == 2
                && list.name == "Release 2"
                && ws.space.ink == [stroke.clone()]
        })
    });
    // Home deletes the checklist: New checklist in the mirror's empty block is home's.
    cx.update_window(home_handle, |_, window, cx| {
        home.update(cx, |ws, cx| ws.delete_checklist(list, window, cx))
    })
    .unwrap();
    pump_home(&home, cx, "the block to show none", |cx| {
        ws.read_with(cx, |ws, _| {
            remote_of(ws)
                .homes
                .get(&uid)
                .is_some_and(|id| ws.space.blocks[id].checklist_view().unwrap().list.is_none())
        })
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_checklist(Some(c1), window, cx))
    })
    .unwrap();
    pump_home(&home, cx, "a new checklist at home", |cx| {
        home.read_with(cx, |ws, _| {
            ws.space.blocks[&block]
                .checklist()
                .unwrap()
                .list
                .is_some_and(|l| l != list && ws.checklists.get(l).is_some())
        })
    });
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let after: Vec<_> = files.iter().map(|f| std::fs::read(f).ok()).collect();
    assert_eq!(after, before, "the mirror's own files are as they were");
    ws.read_with(cx, |ws, _| {
        assert_eq!((ws.notes.clone(), ws.checklists.clone()), own);
    });
}
