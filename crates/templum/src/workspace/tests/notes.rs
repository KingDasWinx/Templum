//! Note blocks: plain text in tabs, kept in the notes (never lost with a block), and the Notes
//! dialog that finds, opens and deletes them.

use gpui_kit::Entity;
use gpui_kit::component::input::EditorState;

use super::*;

#[cfg(target_os = "macos")]
const NEW_NOTE: &str = "cmd-shift-m";
#[cfg(not(target_os = "macos"))]
const NEW_NOTE: &str = "ctrl-shift-m";

fn top_note(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> BlockId {
    ws.read_with(cx, |ws, _| {
        let id = ws.space.canvas.top().unwrap();
        assert!(
            ws.space.blocks.get(&id).and_then(|b| b.note()).is_some(),
            "the top block is a note"
        );
        id
    })
}

/// Opens a note with the shortcut and returns it.
fn new_note(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) -> BlockId {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(NEW_NOTE, cx);
        // Popped in, and the view landed where it glides to show it.
        ws.update(cx, |ws, _| {
            ws.space.appearing.clear();
            ws.space.flight = None;
        });
        window.render_frame(cx);
    })
    .unwrap();
    top_note(ws, cx)
}

/// The editor of the note on screen in `block`.
fn editor(ws: &Entity<Workspace>, cx: &mut TestAppContext, block: BlockId) -> Entity<EditorState> {
    ws.read_with(cx, |ws, _| {
        ws.space.blocks[&block]
            .note()
            .unwrap()
            .tab()
            .unwrap()
            .editor
            .clone()
    })
}

/// The note on screen's id in `block`.
fn shown(ws: &Entity<Workspace>, cx: &mut TestAppContext, block: BlockId) -> u64 {
    ws.read_with(cx, |ws, _| {
        ws.space.blocks[&block].note().unwrap().tab().unwrap().id
    })
}

fn text_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, note: u64) -> Option<String> {
    ws.read_with(cx, |ws, _| ws.notes.get(note).map(|n| n.text.clone()))
}

#[gpui_kit::test]
fn a_new_note_takes_the_keyboard_and_keeps_every_key_in_the_notes(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;
    let (handle, ws) = open(cx);
    let id = new_note(handle, &ws, cx);
    let note = shown(&ws, cx, id);
    let edit = editor(&ws, cx, id);
    cx.update_window(handle, |_, window, cx| {
        let ws_now = ws.read(cx);
        assert_eq!(ws_now.space.focused, Some(id));
        let rect = ws_now.space.canvas.get(id).unwrap().rect;
        assert_eq!((rect.w, rect.h), (crate::note::SIZE.w, crate::note::SIZE.h));
        assert!(
            edit.read(cx).focus_handle(cx).is_focused(window),
            "the editor has the keyboard"
        );
    })
    .unwrap();
    cx.simulate_input(handle, "  Groceries\nmilk");
    assert_eq!(text_of(&ws, cx, note).as_deref(), Some("  Groceries\nmilk"));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.blocks[&id].note().unwrap().title(ws).as_ref(),
            "Untitled",
            "typing never names a note"
        );
    });
}

/// Markdown is text: no list goes on, no bold markers, nothing is rendered.
#[gpui_kit::test]
fn a_note_is_plain_text(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_note(handle, &ws, cx);
    let secondary = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.simulate_input(handle, "# Plan\n- a");
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
        window.press(&format!("{secondary}-b"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    let edit = editor(&ws, cx, id);
    assert_eq!(
        edit.read_with(cx, |e, _| e.value().to_string()),
        "# Plan\n- a\n"
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.blocks[&id].note().unwrap().title(ws).as_ref(),
            "Untitled"
        );
    });
}

#[gpui_kit::test]
fn shortcuts_run_from_inside_a_note(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    new_note(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| window.press(NEW, cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.blocks.len(), 3, "terminal, note, terminal");
        assert_eq!(ws.space.terminals().count(), 2);
    });
}

#[gpui_kit::test]
fn clicking_a_note_focuses_its_editor(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;
    let (handle, ws) = open(cx);
    let id = new_note(handle, &ws, cx);
    let edit = editor(&ws, cx, id);
    let terminal = ws.read_with(cx, |ws, _| ws.space.terminals().next().unwrap().0);
    cx.update_window(handle, |_, window, cx| {
        // The terminal gets the keyboard, but stays under the note (focus_block would raise
        // it, and it covers the note).
        ws.update(cx, |ws, cx| {
            ws.space.focused = Some(terminal);
            window.focus(&ws.focus_handle, cx);
        });
        window.render_frame(cx);
        let body = window.find(("note-editor", id.0 as usize)).bounds();
        click(
            window,
            cx,
            (
                f32::from(body.origin.x) + 20.0,
                f32::from(body.origin.y) + 20.0,
            ),
            1,
        );
        window.render_frame(cx);
        assert_eq!(ws.read(cx).space.focused, Some(id));
        assert!(
            edit.read(cx).focus_handle(cx).is_focused(window),
            "the editor keeps the keyboard"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn closing_a_focused_note_gives_the_keyboard_back_to_the_canvas(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    new_note(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press(CLOSE, cx);
        window.render_frame(cx);
        window.press(NEW, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.space.blocks.values().all(|b| b.note().is_none()),
            "closed"
        );
        assert_eq!(ws.space.terminals().count(), 2, "the shortcut still works");
    });
}

/// + opens a new note in a new tab; a click shows a tab; its × closes it, and the note stays.
#[gpui_kit::test]
fn tabs_hold_several_notes_and_closing_one_keeps_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_note(handle, &ws, cx);
    let n = id.0 as usize;
    let first = shown(&ws, cx, id);
    cx.simulate_input(handle, "first");
    cx.update_window(handle, |_, window, cx| {
        window.click(("note-tab-new", n), cx);
        window.render_frame(cx);
    })
    .unwrap();
    let second = shown(&ws, cx, id);
    assert_ne!(second, first);
    cx.simulate_input(handle, "second");
    let tab = |i: usize| gpui_kit::ElementId::Name(format!("note-tab-{n}-{i}").into());
    let close = |i: usize| gpui_kit::ElementId::Name(format!("note-tab-close-{n}-{i}").into());
    cx.update_window(handle, |_, window, cx| {
        window.click(tab(0), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(shown(&ws, cx, id), first);
    // The × of a tab that isn't on screen shows while the pointer is on the tab.
    cx.update_window(handle, |_, window, cx| {
        window.hover(tab(1), cx);
        window.click(close(1), cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.blocks[&id].note().unwrap().ids(), [first]);
    });
    assert_eq!(text_of(&ws, cx, second).as_deref(), Some("second"), "kept");
    assert_eq!(text_of(&ws, cx, first).as_deref(), Some("first"));
}

/// Closing a block keeps its notes for the Notes dialog, but a blank one: nothing is lost.
#[gpui_kit::test]
fn a_closed_block_keeps_its_written_notes_and_drops_blank_ones(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let written = new_note(handle, &ws, cx);
    let kept = shown(&ws, cx, written);
    cx.simulate_input(handle, "keep me");
    let blank = new_note(handle, &ws, cx);
    let dropped = shown(&ws, cx, blank);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.close(written, window, cx);
            ws.close(blank, window, cx);
        })
    })
    .unwrap();
    assert_eq!(text_of(&ws, cx, kept).as_deref(), Some("keep me"));
    assert_eq!(text_of(&ws, cx, dropped), None);
    // Opened again, without a block to go in: a new block shows it.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_note(kept, None, window, cx))
    })
    .unwrap();
    let again = top_note(&ws, cx);
    assert_eq!(shown(&ws, cx, again), kept);
    assert_eq!(
        editor(&ws, cx, again).read_with(cx, |e, _| e.value().to_string()),
        "keep me"
    );
}

/// Opening a note open in another block shows it there; the other block is left as it is.
#[gpui_kit::test]
fn a_note_is_open_in_one_tab_at_most(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = new_note(handle, &ws, cx);
    let note = shown(&ws, cx, first);
    cx.simulate_input(handle, "here");
    let second = new_note(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_note(note, Some(second), window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(first), "shown where it is");
        assert_eq!(ws.space.blocks[&second].note().unwrap().tabs.len(), 1);
    });
}

/// The dialog lists the notes a search finds; a click on one opens it; the trash deletes a note
/// on its second click, from the blocks that show it too.
#[gpui_kit::test]
fn the_notes_dialog_finds_opens_and_deletes_notes(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let block = new_note(handle, &ws, cx);
    let pie = shown(&ws, cx, block);
    cx.simulate_input(handle, "Apple pie");
    let banana = ws.update(cx, |ws, _| ws.notes.add("Banana bread".into(), None, 1));
    let row = |id: u64| gpui_kit::ElementId::Name(format!("note-row-{id}").into());
    let trash = |id: u64| gpui_kit::ElementId::Name(format!("note-delete-{id}").into());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::note::open_notes(ws.space.id.clone(), None, window, cx)
        });
        window.render_frame(cx);
        window.find(row(pie));
        window.find(row(banana));
    })
    .unwrap();
    cx.simulate_input(handle, "BANANA");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(row(pie)).is_none(),
            "the search leaves it out"
        );
        window.click(row(banana), cx);
        window.render_frame(cx);
    })
    .unwrap();
    let opened = top_note(&ws, cx);
    assert_eq!(shown(&ws, cx, opened), banana, "a new block shows it");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::note::open_notes(ws.space.id.clone(), None, window, cx)
        });
        window.render_frame(cx);
        window.click(trash(pie), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(text_of(&ws, cx, pie).is_some(), "one click asks");
    cx.update_window(handle, |_, window, cx| {
        window.click(trash(pie), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(text_of(&ws, cx, pie), None, "deleted");
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.blocks[&block].note().unwrap().tabs.is_empty());
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(("note-empty", block.0 as usize));
    })
    .unwrap();
}

#[gpui_kit::test]
fn notes_and_their_tabs_come_back_after_a_restart(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let id = new_note(handle, &ws, cx);
    cx.simulate_input(handle, "one");
    cx.update_window(handle, |_, window, cx| {
        window.click(("note-tab-new", id.0 as usize), cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_input(handle, "two");
    let set_aside = ws.update(cx, |ws, _| ws.notes.add("set aside".into(), None, 1));
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    assert!(crate::notes::path(dir.path()).exists());
    let (_, again) = open_in(cx, None, Some(dir.path().to_path_buf()));
    again.read_with(cx, |ws, cx| {
        let note = ws
            .space
            .blocks
            .values()
            .find_map(|b| b.note())
            .expect("the note block");
        let texts: Vec<String> = note
            .tabs
            .iter()
            .map(|t| t.editor.read(cx).value().to_string())
            .collect();
        assert_eq!(texts, ["one", "two"]);
        assert_eq!(note.active, 1);
        assert_eq!(
            ws.notes.get(set_aside).map(|n| n.text.as_str()),
            Some("set aside"),
            "a note no block shows"
        );
    });
}

/// A canvas from before notes.json: its note's text and color become a note there, and the
/// canvas names it by its id from then on.
#[gpui_kit::test]
fn an_older_note_block_moves_its_text_into_the_notes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    write_canvas(
        dir.path(),
        r##"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[
            {"kind":"note","rect":{"x":0,"y":0,"w":360,"h":280},"text":"# Old\n- x",
             "preview":true,"color":"yellow"}]}"##,
    );
    let (_, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let note = ws.read_with(cx, |ws, cx| {
        let note = ws.space.blocks.values().find_map(|b| b.note()).unwrap();
        assert_eq!(note.text(cx), "# Old\n- x");
        let id = note.tab().unwrap().id;
        let entry = ws.notes.get(id).unwrap();
        assert_eq!(entry.color, Some(crate::board::Label::Yellow));
        assert_eq!(entry.name, "# Old", "called what it was: its first line");
        id
    });
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let Ok(crate::session::Loaded::Saved(saved)) =
        Ok::<_, ()>(crate::session::load(dir.path(), crate::session::DEFAULT_ID))
    else {
        panic!("not saved");
    };
    assert!(matches!(
        &saved.items[0],
        crate::session::Item::Note(n) if n.notes == [note] && n.text.is_empty()
    ));
}

/// Send to ▸ a terminal types the note's selection there (never Enter) and shows the terminal
/// with the keyboard.
#[cfg(unix)]
#[gpui_kit::test]
fn send_to_types_a_notes_selection_into_the_chosen_terminal(cx: &mut TestAppContext) {
    use std::time::{Duration, Instant};
    use templum_terminal::Snapshot;
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let note = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.add_note_at(Point::new(900.0, 40.0), window, cx);
                ws.space.appearing.clear();
                ws.space.canvas.top().unwrap()
            })
        })
        .unwrap();
    let edit = editor(&ws, cx, note);
    cx.update_window(handle, |_, window, cx| {
        edit.update(cx, |e, cx| {
            e.set_value("one two", window, cx);
            e.set_selected_range(4..7, cx);
        });
        window.render_frame(cx);
        window.right_click(("title", note.0 as usize), cx);
        window.render_frame(cx);
        // All notes…, Rename…, Copy all, Send to ▸, Color ▸, —, Close.
        window.within("popup-menu").click(3usize, cx);
        window.render_frame(cx);
        window
            .within("submenu")
            .within("popup-menu")
            .click(0usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(terminal)));
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let shown = ws.read_with(cx, |ws, _| {
            let mut snap = Snapshot::default();
            snap.refresh(
                ws.space.terminal(terminal).unwrap().term.shared(),
                &ws.theme.palette,
            );
            snap.text()
        });
        if shown.contains("two") {
            assert!(!shown.contains("one"), "only the selection: {shown:?}");
            break;
        }
        assert!(Instant::now() < deadline, "not typed: {shown:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[gpui_kit::test]
fn the_note_menu_colors_the_note_on_screen(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let block = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.add_note_at(Point::new(900.0, 40.0), window, cx);
                ws.space.appearing.clear();
                ws.space.canvas.top().unwrap()
            })
        })
        .unwrap();
    let note = shown(&ws, cx, block);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("title", block.0 as usize), cx);
        window.render_frame(cx);
        // All notes…, Rename…, Copy all, Color ▸ (an empty note has nothing to send), —, Close.
        window.within("popup-menu").click(3usize, cx);
        window.render_frame(cx);
        // None, Red, Yellow…
        window
            .within("submenu")
            .within("popup-menu")
            .click(2usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.notes.get(note).unwrap().color,
            Some(crate::board::Label::Yellow)
        );
    });
}

/// Double-clicks tab `index` of note block `block`.
fn double_click_tab(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    block: BlockId,
    index: usize,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let tab = gpui_kit::ElementId::Name(format!("note-tab-{}-{index}", block.0).into());
        let at = window.find(tab).bounds().center();
        click(window, cx, (f32::from(at.x), f32::from(at.y)), 2);
        window.render_frame(cx);
    })
    .unwrap();
}

/// The name of note `note`, and whether `block` is renaming one.
fn name_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, note: u64) -> String {
    ws.read_with(cx, |ws, _| ws.notes.name(note).to_string())
}

fn renaming(ws: &Entity<Workspace>, cx: &mut TestAppContext, block: BlockId) -> bool {
    ws.read_with(cx, |ws, _| {
        ws.space.blocks[&block].note().unwrap().renaming.is_some()
    })
}

/// A new note is called Untitled whatever is typed in it; a double click on its tab selects the
/// name in place; Enter keeps what is typed and gives the keyboard back to the note, Esc leaves
/// the name as it was, and a blank name is Untitled again.
#[gpui_kit::test]
fn a_double_click_on_a_tab_renames_its_note_in_place(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;
    let (handle, ws) = open(cx);
    let id = new_note(handle, &ws, cx);
    let note = shown(&ws, cx, id);
    let edit = editor(&ws, cx, id);
    cx.simulate_input(handle, "milk");
    assert_eq!(name_of(&ws, cx, note), "Untitled");
    double_click_tab(handle, cx, id, 0);
    assert!(renaming(&ws, cx, id));
    let input = ws.read_with(cx, |ws, _| {
        ws.space.blocks[&id]
            .note()
            .unwrap()
            .renaming
            .as_ref()
            .unwrap()
            .input
            .clone()
    });
    cx.update_window(handle, |_, window, cx| {
        assert!(input.read(cx).focus_handle(cx).is_focused(window));
        assert_eq!(
            input.read(cx).value().as_ref(),
            "Untitled",
            "the name, selected"
        );
    })
    .unwrap();
    // Typed over the selected name.
    cx.simulate_input(handle, "Groceries");
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        assert!(
            edit.read(cx).focus_handle(cx).is_focused(window),
            "the note has the keyboard again"
        );
    })
    .unwrap();
    assert!(!renaming(&ws, cx, id));
    assert_eq!(name_of(&ws, cx, note), "Groceries");
    assert_eq!(
        text_of(&ws, cx, note).as_deref(),
        Some("milk"),
        "its text is its own"
    );
    ws.read_with(cx, |ws, cx| {
        let title = crate::block::title(ws, &ws.space.blocks[&id], cx);
        assert_eq!(title.as_ref(), "Groceries", "the block's title follows");
    });
    // Esc: as it was.
    double_click_tab(handle, cx, id, 0);
    cx.simulate_input(handle, "Other");
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!renaming(&ws, cx, id));
    assert_eq!(name_of(&ws, cx, note), "Groceries");
    // Blank: Untitled.
    double_click_tab(handle, cx, id, 0);
    cx.update_window(handle, |_, window, cx| {
        window.press("backspace", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(name_of(&ws, cx, note), "Untitled");
}

/// The keyboard leaving the field (a click on the note) keeps the name typed, as Enter does.
#[gpui_kit::test]
fn the_keyboard_leaving_the_tabs_field_keeps_its_name(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    // Focus listeners (a field's blur) only hear an active window.
    super::terminals::set_active(handle, true, cx);
    let id = new_note(handle, &ws, cx);
    let note = shown(&ws, cx, id);
    let edit = editor(&ws, cx, id);
    double_click_tab(handle, cx, id, 0);
    cx.simulate_input(handle, "Plan");
    cx.update_window(handle, |_, window, cx| {
        edit.update(cx, |e, cx| e.focus(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!renaming(&ws, cx, id));
    assert_eq!(name_of(&ws, cx, note), "Plan");
}

/// A note the person named stays in the Notes dialog when its tab closes though it has no text;
/// one never named and blank goes, as before.
#[gpui_kit::test]
fn a_named_note_without_text_is_kept_when_closed(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_note(handle, &ws, cx);
    let named = shown(&ws, cx, id);
    ws.update(cx, |ws, _| ws.notes.rename(named, "Ideas"));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(id, window, cx))
    })
    .unwrap();
    assert_eq!(text_of(&ws, cx, named).as_deref(), Some(""), "kept");
    assert_eq!(name_of(&ws, cx, named), "Ideas");
}

/// The Notes dialog lists a note by its name and finds it by the name too.
#[gpui_kit::test]
fn the_notes_dialog_shows_and_finds_notes_by_name(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let milk = ws.update(cx, |ws, _| ws.notes.add("milk\neggs".into(), None, 1));
    ws.update(cx, |ws, _| ws.notes.rename(milk, "Groceries"));
    let row = gpui_kit::ElementId::Name(format!("note-row-{milk}").into());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::note::open_notes(ws.space.id.clone(), None, window, cx)
        });
        window.render_frame(cx);
        window.find(row.clone());
    })
    .unwrap();
    cx.simulate_input(handle, "grocer");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(row);
    })
    .unwrap();
}

/// Names are kept with the notes: after a restart a note is called what it was, and a file from
/// before names calls each note by its first line.
#[gpui_kit::test]
fn a_notes_name_comes_back_after_a_restart(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let id = new_note(handle, &ws, cx);
    let note = shown(&ws, cx, id);
    cx.simulate_input(handle, "text");
    ws.update(cx, |ws, _| ws.notes.rename(note, "Mine"));
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let (_, again) = open_in(cx, None, Some(dir.path().to_path_buf()));
    again.read_with(cx, |ws, _| assert_eq!(ws.notes.name(note), "Mine"));
}

/// The block's menu (right click, or its ⋯ button) has Rename…, which asks for the name in a
/// dialog, its field holding the current one.
#[gpui_kit::test]
fn the_note_menu_renames_the_note_on_screen(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let block = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.add_note_at(Point::new(900.0, 40.0), window, cx);
                ws.space.appearing.clear();
                ws.space.canvas.top().unwrap()
            })
        })
        .unwrap();
    let note = shown(&ws, cx, block);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("title", block.0 as usize), cx);
        window.render_frame(cx);
        // All notes…, Rename…, Copy all, Color ▸, —, Close.
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        // The dialog slides in for 0.25 s first: presses meanwhile are lost.
        std::thread::sleep(std::time::Duration::from_millis(300));
        window.render_frame(cx);
    })
    .unwrap();
    // The current name is selected: typing replaces it.
    cx.simulate_input(handle, "Plan");
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(name_of(&ws, cx, note), "Plan");
}
