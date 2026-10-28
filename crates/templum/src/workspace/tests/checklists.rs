//! Checklist blocks: one checklist each, kept app-wide (never lost with a block but for blank
//! ones), their items checked, edited and deleted, and Ctrl+Z on their content.

use gpui_kit::ElementId;
use gpui_kit::component::input::InputState;

use crate::checklist::Field;
use crate::session::Loaded;

use super::*;

/// Opens a checklist block with the toolbar's button and returns it. The window is active: focus
/// listeners (a field's blur) only hear an active window.
fn new_checklist(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> BlockId {
    cx.update_window(handle, |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
        window.click("tool-checklist", cx);
        // Popped in, and the view landed where it glides to show it.
        ws.update(cx, |ws, _| {
            ws.space.appearing.clear();
            ws.space.flight = None;
        });
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let id = ws.space.canvas.top().unwrap();
        assert!(
            ws.space
                .blocks
                .get(&id)
                .and_then(|b| b.checklist())
                .is_some(),
            "the top block is a checklist"
        );
        id
    })
}

/// The checklist `block` shows.
fn shown(ws: &Workspace, block: BlockId) -> u64 {
    list_of(ws, block).unwrap()
}

/// The checklist `block` shows, if any.
fn list_of(ws: &Workspace, block: BlockId) -> Option<u64> {
    ws.space.blocks[&block].checklist().unwrap().list
}

fn name(what: &str, block: BlockId, i: u64) -> ElementId {
    ElementId::Name(format!("{what}-{}-{i}", block.0).into())
}

fn click(handle: AnyWindowHandle, cx: &mut TestAppContext, id: impl Into<ElementId>) {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Items "a" and "b" added to the checklist on screen in `block`, not as an undo step: (list, a).
fn two_items(ws: &Entity<Workspace>, cx: &mut TestAppContext, block: BlockId) -> (u64, u64) {
    ws.update(cx, |ws, _| {
        let list = shown(ws, block);
        let a = ws.checklists.add_item(list, None, "a", false, 1).unwrap();
        ws.checklists.add_item(list, None, "b", false, 1);
        (list, a)
    })
}

fn done(ws: &Entity<Workspace>, cx: &mut TestAppContext, list: u64, item: u64) -> bool {
    ws.read_with(cx, |ws, _| {
        ws.checklists.get(list).unwrap().item(item).unwrap().done
    })
}

fn press_undo(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    block: BlockId,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(block, window, cx));
        window.press(UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_toolbar_opens_a_block_with_a_new_checklist(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    ws.read_with(cx, |ws, cx| {
        let rect = ws.space.canvas.get(id).unwrap().rect;
        assert_eq!((rect.w, rect.h), (320.0, 360.0));
        let list = ws.checklists.get(shown(ws, id)).unwrap();
        assert_eq!(list.name, "Checklist");
        assert_eq!(
            crate::block::title(ws, &ws.space.blocks[&id], cx).as_ref(),
            "Checklist"
        );
        assert_eq!(ws.space.focused, Some(id));
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(("checklist-tabs", id.0 as usize)).is_none(),
            "one checklist, no tabs"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn add_checklist_in_the_canvas_menu_opens_one_where_it_was_asked(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let world = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let o = ws.read(cx).origin;
            let at = (f32::from(o.x) + 20.0, f32::from(o.y) + 600.0);
            right_click(window, cx, at);
            window.render_frame(cx);
            let ws = ws.read(cx);
            ws.space
                .canvas
                .camera
                .to_world(ws.local(point(px(at.0), px(at.1))))
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        // AI agent ▸, Add ▸ Terminal, Kanban board, Note, Checklist.
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        window
            .within("submenu")
            .within("popup-menu")
            .click(3usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let top = ws.space.canvas.top().unwrap();
        assert!(ws.space.blocks[&top].checklist().is_some());
        assert_eq!(ws.space.canvas.get(top).unwrap().rect.origin(), world);
    });
}

#[gpui_kit::test]
fn the_box_checks_an_item_and_the_title_counts_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a) = two_items(&ws, cx, id);
    click(handle, cx, name("checklist-box", id, a));
    assert!(done(&ws, cx, list, a));
    ws.read_with(cx, |ws, cx| {
        let title = crate::block::title(ws, &ws.space.blocks[&id], cx);
        assert_eq!(title.as_ref(), "Checklist 1/2");
    });
    click(handle, cx, name("checklist-box", id, a));
    assert!(!done(&ws, cx, list, a), "a second click unchecks it");
}

#[gpui_kit::test]
fn the_rows_x_deletes_its_item(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a) = two_items(&ws, cx, id);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            !window.find(name("checklist-item-delete", id, a)).visible(),
            "shown only under the pointer"
        );
        window.hover(name("checklist-row", id, a), cx);
        window.render_frame(cx);
    })
    .unwrap();
    click(handle, cx, name("checklist-item-delete", id, a));
    ws.read_with(cx, |ws, _| {
        let list = ws.checklists.get(list).unwrap();
        assert!(list.item(a).is_none());
        assert_eq!(list.items.len(), 1);
    });
}

#[gpui_kit::test]
fn closing_a_blank_checklists_block_removes_it_and_a_named_one_stays(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let close = |cx: &mut TestAppContext, id: BlockId| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.close(id, window, cx))
        })
        .unwrap();
    };
    let named = new_checklist(handle, &ws, cx);
    let groceries = ws.update(cx, |ws, _| {
        let list = shown(ws, named);
        ws.checklists.rename(list, "Groceries", 1);
        list
    });
    close(cx, named);
    let blank = new_checklist(handle, &ws, cx);
    let gone = ws.read_with(cx, |ws, _| shown(ws, blank));
    close(cx, blank);
    ws.read_with(cx, |ws, _| {
        assert!(ws.checklists.get(gone).is_none(), "blank: gone");
        assert!(ws.checklists.get(groceries).is_some(), "named: kept");
    });
}

#[gpui_kit::test]
fn ctrl_z_on_a_checklist_block_undoes_a_check_and_skips_a_deleted_checklist(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a) = two_items(&ws, cx, id);
    ws.update(cx, |ws, cx| ws.toggle_item(list, a, cx));
    assert!(done(&ws, cx, list, a));
    press_undo(handle, &ws, cx, id);
    assert!(!done(&ws, cx, list, a), "undone");
    // A step of a checklist deleted since is skipped, and never brings it back.
    ws.update(cx, |ws, cx| ws.toggle_item(list, a, cx));
    let other = ws.update(cx, |ws, cx| {
        let other = ws.checklists.add(1);
        let b = ws.checklists.add_item(other, None, "b", false, 1).unwrap();
        ws.toggle_item(other, b, cx);
        ws.checklists.remove(other);
        other
    });
    press_undo(handle, &ws, cx, id);
    assert!(!done(&ws, cx, list, a), "the step before it is undone");
    ws.read_with(cx, |ws, _| assert!(ws.checklists.get(other).is_none()));
}

#[gpui_kit::test]
fn a_checklist_is_open_in_one_block_at_most(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = new_checklist(handle, &ws, cx);
    let list = ws.update(cx, |ws, _| {
        let list = shown(ws, first);
        ws.checklists.rename(list, "Here", 1);
        list
    });
    let second = new_checklist(handle, &ws, cx);
    let its_own = ws.read_with(cx, |ws, _| shown(ws, second));
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_checklist(list, Some(second), window, cx)
        })
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(first), "shown where it is");
        assert_eq!(shown(ws, second), its_own);
        assert_eq!(ws.space.blocks.len(), blocks, "no new block");
    });
}

#[gpui_kit::test]
fn a_pasted_checklist_block_shows_copies(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, _) = two_items(&ws, cx, id);
    ws.update(cx, |ws, _| ws.checklists.rename(list, "Deploy", 1));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.space.focused = None;
            ws.space.selected = vec![id];
            window.focus(&ws.focus_handle, cx);
        });
        window.press(COPY, cx);
        window.press(PASTE, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let pasted = ws.space.selected.clone();
        assert_eq!(pasted.len(), 1);
        assert_ne!(pasted[0], id);
        let copy = shown(ws, pasted[0]);
        assert_ne!(copy, list, "a copy, not the checklist");
        let (copy, original) = (
            ws.checklists.get(copy).unwrap(),
            ws.checklists.get(list).unwrap(),
        );
        assert_eq!(copy.name, "Deploy");
        let texts = |l: &crate::checklists::List| {
            l.items.iter().map(|i| i.text.clone()).collect::<Vec<_>>()
        };
        assert_eq!(texts(copy), texts(original));
    });
}

/// The field editing in `block`: what it edits, and its input.
fn editing(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    block: BlockId,
) -> Option<(Field, Entity<InputState>)> {
    ws.read_with(cx, |ws, _| {
        let c = ws.space.blocks[&block].checklist().unwrap();
        c.editing.as_ref().map(|e| (e.field, e.input.clone()))
    })
}

fn press(handle: AnyWindowHandle, cx: &mut TestAppContext, key: &str) {
    cx.update_window(handle, |_, window, cx| {
        window.press(key, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

fn texts(ws: &Entity<Workspace>, cx: &mut TestAppContext, list: u64) -> Vec<String> {
    ws.read_with(cx, |ws, _| {
        let list = ws.checklists.get(list).unwrap();
        list.items.iter().map(|i| i.text.clone()).collect()
    })
}

#[gpui_kit::test]
fn a_new_checklist_edits_its_name_and_enter_keeps_it(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (field, input) = editing(&ws, cx, id).expect("the name is edited");
    assert_eq!(field, Field::Name);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            input.read(cx).focus_handle(cx).is_focused(window),
            "the field has the keyboard"
        );
    })
    .unwrap();
    // The name is selected: typing replaces it.
    cx.simulate_input(handle, "Deploy");
    let list = ws.read_with(cx, |ws, _| shown(ws, id));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.checklists.get(list).unwrap().name, "Deploy")
    });
    press(handle, cx, "enter");
    assert!(editing(&ws, cx, id).is_none());
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.checklists.get(list).unwrap().name, "Deploy")
    });
}

#[gpui_kit::test]
fn enter_in_an_item_adds_the_next_and_every_key_is_kept(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let list = ws.read_with(cx, |ws, _| shown(ws, id));
    click(handle, cx, ("checklist-add", id.0 as usize));
    assert!(matches!(editing(&ws, cx, id), Some((Field::Item(_), _))));
    cx.simulate_input(handle, "Build");
    assert_eq!(texts(&ws, cx, list), ["Build"], "kept before Enter");
    press(handle, cx, "enter");
    cx.simulate_input(handle, "Test");
    press(handle, cx, "escape");
    assert_eq!(texts(&ws, cx, list), ["Build", "Test"]);
    assert!(editing(&ws, cx, id).is_none());
}

#[gpui_kit::test]
fn backspace_in_an_empty_item_deletes_it_and_edits_the_one_before(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a) = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        (
            list,
            ws.checklists.add_item(list, None, "a", false, 1).unwrap(),
        )
    });
    click(handle, cx, ("checklist-add", id.0 as usize));
    assert_eq!(texts(&ws, cx, list), ["a", ""]);
    press(handle, cx, "backspace");
    assert_eq!(texts(&ws, cx, list), ["a"]);
    let (field, input) = editing(&ws, cx, id).expect("the one before is edited");
    assert_eq!(field, Field::Item(a));
    assert_eq!(
        input.read_with(cx, |i, _| i.cursor()),
        1,
        "its caret at the end"
    );
}

/// The first item has the name before it: a held Backspace eats the item, then the name, and
/// never reaches the canvas, where it would close the block.
#[gpui_kit::test]
fn backspace_held_in_the_first_item_stops_in_the_name(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let list = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        ws.checklists.rename(list, "Go", 1);
        list
    });
    click(handle, cx, ("checklist-add", id.0 as usize));
    cx.simulate_input(handle, "x");
    press(handle, cx, "backspace");
    press(handle, cx, "backspace");
    assert!(texts(&ws, cx, list).is_empty());
    let (field, input) = editing(&ws, cx, id).expect("the name is edited");
    assert_eq!(field, Field::Name);
    assert_eq!(
        input.read_with(cx, |i, _| i.cursor()),
        2,
        "its caret at the end"
    );
    for _ in 0..4 {
        press(handle, cx, "backspace");
    }
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.blocks.contains_key(&id), "the block stays");
        assert_eq!(ws.checklists.get(list).unwrap().name, "");
    });
}

#[gpui_kit::test]
fn an_item_left_empty_goes_and_enter_in_an_empty_item_stops(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a) = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        (
            list,
            ws.checklists.add_item(list, None, "a", false, 1).unwrap(),
        )
    });
    let add = ("checklist-add", id.0 as usize);
    click(handle, cx, add);
    press(handle, cx, "enter");
    assert_eq!(
        texts(&ws, cx, list),
        ["a"],
        "Enter in an empty item: it goes"
    );
    assert!(editing(&ws, cx, id).is_none());
    click(handle, cx, add);
    cx.simulate_input(handle, "x");
    click(handle, cx, name("checklist-text", id, a));
    assert_eq!(texts(&ws, cx, list), ["a", "x"], "kept");
    assert!(matches!(editing(&ws, cx, id), Some((Field::Item(item), _)) if item == a));
    let x = ws.read_with(cx, |ws, _| ws.checklists.get(list).unwrap().items[1].id);
    click(handle, cx, name("checklist-text", id, x));
    press(handle, cx, "backspace");
    press(handle, cx, "escape");
    assert_eq!(texts(&ws, cx, list), ["a"], "left empty: gone");
}

#[gpui_kit::test]
fn one_ctrl_z_takes_back_an_edits_typing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a) = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        (
            list,
            ws.checklists.add_item(list, None, "a", false, 1).unwrap(),
        )
    });
    click(handle, cx, name("checklist-text", id, a));
    cx.simulate_input(handle, "bc");
    press(handle, cx, "escape");
    assert_eq!(texts(&ws, cx, list), ["abc"]);
    press_undo(handle, &ws, cx, id);
    assert_eq!(texts(&ws, cx, list), ["a"]);
    click(handle, cx, ("checklist-add", id.0 as usize));
    cx.simulate_input(handle, "new");
    press(handle, cx, "escape");
    assert_eq!(texts(&ws, cx, list), ["a", "new"]);
    press_undo(handle, &ws, cx, id);
    assert_eq!(texts(&ws, cx, list), ["a"], "the add was the step");
}

#[gpui_kit::test]
fn closing_the_block_while_editing_keeps_the_text(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let list = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        ws.checklists.rename(list, "Groceries", 1);
        list
    });
    click(handle, cx, ("checklist-add", id.0 as usize));
    cx.simulate_input(handle, "milk");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(id, window, cx))
    })
    .unwrap();
    assert_eq!(texts(&ws, cx, list), ["milk"]);
}

#[gpui_kit::test]
fn ctrl_z_in_the_field_is_the_fields_own(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a) = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        (
            list,
            ws.checklists.add_item(list, None, "a", false, 1).unwrap(),
        )
    });
    click(handle, cx, name("checklist-text", id, a));
    cx.simulate_input(handle, "b");
    press(handle, cx, UNDO);
    assert_eq!(texts(&ws, cx, list), ["a"], "the field's undo");
    press(handle, cx, UNDO);
    assert!(editing(&ws, cx, id).is_some(), "still editing");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.checklists_undo.len(), 1, "the checklist's step is left")
    });
}

#[gpui_kit::test]
fn a_click_on_another_block_stops_editing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let id = new_checklist(handle, &ws, cx);
    let list = ws.read_with(cx, |ws, _| shown(ws, id));
    let elsewhere = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.focus_block(terminal, window, cx));
            window.render_frame(cx);
        })
        .unwrap();
    };
    click(handle, cx, ("checklist-add", id.0 as usize));
    cx.simulate_input(handle, "x");
    elsewhere(cx);
    assert!(editing(&ws, cx, id).is_none());
    assert_eq!(texts(&ws, cx, list), ["x"], "kept");
    click(handle, cx, ("checklist-add", id.0 as usize));
    elsewhere(cx);
    assert_eq!(texts(&ws, cx, list), ["x"], "an item left empty goes");
}

/// Zoom scales like a photo: at every zoom, an item's text and the name stay where they were
/// while their field edits them, one line as high as the text's.
#[gpui_kit::test]
fn a_field_takes_its_texts_place_at_every_zoom(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (_, a) = two_items(&ws, cx, id);
    let n = id.0 as usize;
    for zoom in [0.5, 1.0, 2.0] {
        for (target, edit) in [
            (name("checklist-text", id, a), Field::Item(a)),
            (ElementId::from(("checklist-name", n)), Field::Name),
        ] {
            let bounds = |cx: &mut TestAppContext, field: Option<Field>| {
                let target = target.clone();
                cx.update_window(handle, |_, window, cx| {
                    ws.update(cx, |ws, cx| {
                        ws.stop_editing(id, window, cx);
                        let camera = ws.space.canvas.camera;
                        ws.space.canvas.camera = Camera::at(camera.origin, zoom);
                        ws.space.flight = None;
                        if let Some(field) = field {
                            ws.edit_field(id, field, window, cx);
                        }
                    });
                    window.render_frame(cx);
                    window.find(target).bounds()
                })
                .unwrap()
            };
            let (shown, edited) = (bounds(cx, None), bounds(cx, Some(edit)));
            let (_, input) = editing(&ws, cx, id).unwrap();
            // The caret at the text's start is where its first letter was.
            let caret = cx
                .update_window(handle, |_, window, cx| {
                    input.update(cx, |i, cx| i.set_selected_range(0..0, cx));
                    window.render_frame(cx);
                    input.read(cx).cursor_layout().unwrap().0
                })
                .unwrap();
            let near = |a: gpui_kit::Pixels, b: gpui_kit::Pixels| {
                (f32::from(a) - f32::from(b)).abs() < 0.5
            };
            assert!(
                near(shown.origin.x, edited.origin.x)
                    && near(shown.origin.y, edited.origin.y)
                    && near(shown.size.height, edited.size.height)
                    && near(shown.size.width, edited.size.width),
                "zoom {zoom}, {edit:?}: {shown:?} then {edited:?}"
            );
            // The line is the text's first: its center, the caret's.
            let line = if edit == Field::Name { 15.0 } else { 13.0 } * 1.5 * zoom;
            let center = f32::from(shown.origin.y) + line / 2.0;
            let caret_center = f32::from(caret.origin.y) + f32::from(caret.size.height) / 2.0;
            assert!(
                near(caret.origin.x, shown.origin.x) && (caret_center - center).abs() < 1.0,
                "zoom {zoom}, {edit:?}: caret {caret:?}, line center {center}"
            );
        }
    }
}

/// One snapshot period, with the background write finished.
fn tick(cx: &mut TestAppContext) {
    cx.executor().advance_clock(crate::session::EVERY);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn checklists_and_their_blocks_come_back_after_a_restart(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let id = new_checklist(handle, &ws, cx);
    let (second, item) = ws.update(cx, |ws, _| {
        let second = shown(ws, id);
        ws.checklists.rename(second, "Deploy", 1);
        let item = ws
            .checklists
            .add_item(second, None, "Build", true, 1)
            .unwrap();
        (second, item)
    });
    // The snapshot writes them.
    tick(cx);
    let Loaded::Saved(written) = crate::checklists::load(dir.path()) else {
        panic!("checklists.json not written");
    };
    assert!(
        written
            .get(second)
            .is_some_and(|l| l.item(item).unwrap().done)
    );
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let (_, again) = open_in(cx, None, Some(dir.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        let c = ws
            .space
            .blocks
            .values()
            .find_map(|b| b.checklist())
            .unwrap();
        assert_eq!(c.list, Some(second));
        let deploy = ws.checklists.get(second).unwrap();
        assert_eq!(deploy.name, "Deploy");
        assert!(deploy.item(item).unwrap().done);
    });
}

#[gpui_kit::test]
fn a_checklist_named_twice_in_a_canvas_opens_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        crate::checklists::path(dir.path()),
        r#"{"version":1,"next_id":3,"checklists":[{"id":3,"name":"Deploy"}]}"#,
    )
    .unwrap();
    write_canvas(
        dir.path(),
        r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[
            {"kind":"checklist","rect":{"x":0,"y":0,"w":320,"h":360},"checklist":3},
            {"kind":"checklist","rect":{"x":400,"y":0,"w":320,"h":360},"checklist":3}]}"#,
    );
    let (_, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    ws.read_with(cx, |ws, _| {
        let lists: Vec<u64> = ws
            .space
            .blocks
            .values()
            .filter_map(|b| b.checklist()?.list)
            .collect();
        assert_eq!(lists, [3], "one block in all");
    });
}

/// A canvas from before one checklist per block: a block of several tabs shows the one that was
/// on screen (the first still there, when that one is gone); the others stay in the Checklists
/// dialog, but for blank ones. The canvas is written back with one checklist per block.
#[gpui_kit::test]
fn a_block_of_tabs_from_before_shows_its_tab_on_screen_and_keeps_the_others(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        crate::checklists::path(dir.path()),
        r#"{"version":1,"next_id":9,"checklists":[
            {"id":1,"name":"Groceries","items":[{"id":2,"text":"Milk"}]},
            {"id":3,"name":"Deploy"},
            {"id":4,"name":"Checklist","items":[{"id":5,"text":""}]},
            {"id":6,"name":"Release"}]}"#,
    )
    .unwrap();
    write_canvas(
        dir.path(),
        r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[
            {"kind":"checklist","rect":{"x":0,"y":0,"w":320,"h":360},
             "checklists":[1,3,4],"active":1},
            {"kind":"checklist","rect":{"x":400,"y":0,"w":320,"h":360},
             "checklists":[8,6],"active":0}]}"#,
    );
    let (_, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, cx| {
        let mut lists: Vec<u64> = ws
            .space
            .blocks
            .values()
            .filter_map(|b| b.checklist()?.list)
            .collect();
        lists.sort();
        assert_eq!(
            lists,
            [3, 6],
            "the tab on screen; else the first still there"
        );
        let groceries = ws.checklists.get(1).expect("a tab not shown stays");
        assert_eq!(groceries.items[0].text, "Milk");
        assert!(ws.checklists.get(4).is_none(), "a blank one goes");
        ws.save_canvas_now(cx);
    });
    let text =
        std::fs::read_to_string(crate::session::path(dir.path(), crate::session::DEFAULT_ID))
            .unwrap();
    assert!(text.contains(r#""checklist": 3"#), "{text}");
    assert!(
        !text.contains("checklists") && !text.contains("active"),
        "{text}"
    );
}

#[gpui_kit::test]
fn an_unparsable_checklists_json_is_set_aside_and_the_canvas_still_saves(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(crate::checklists::path(dir.path()), "{").unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    assert!(dir.path().join("checklists.json.bak").exists());
    assert_eq!(notification_count(handle, cx), 1);
    ws.read_with(cx, |ws, _| assert!(ws.checklists.lists.is_empty()));
    let terminal = first_id(&ws, cx);
    ws.update(cx, |ws, cx| {
        ws.space.canvas.get_mut(terminal).unwrap().rect.x += 100.0;
        ws.checklists.add(1);
        ws.save_canvas_now(cx);
    });
    assert!(crate::session::path(dir.path(), crate::session::DEFAULT_ID).exists());
    assert!(crate::checklists::path(dir.path()).exists(), "a new file");
}

/// A checklists.json Templum can't open isn't written over that session; the canvas is.
#[cfg(unix)]
#[gpui_kit::test]
fn an_unopenable_checklists_json_is_left_alone(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let path = crate::checklists::path(dir.path());
    let text = r#"{"version":1,"checklists":[{"id":1,"name":"Keep"}]}"#;
    std::fs::write(&path, text).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    assert_eq!(notification_count(handle, cx), 1);
    ws.update(cx, |ws, cx| {
        ws.checklists.add(1);
        ws.save_canvas_now(cx);
    });
    tick(cx);
    assert!(crate::session::path(dir.path(), crate::session::DEFAULT_ID).exists());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        text,
        "never written over"
    );
}

fn row(id: u64) -> ElementId {
    ElementId::Name(format!("checklist-row-{id}").into())
}

fn trash(id: u64) -> ElementId {
    ElementId::Name(format!("checklist-delete-{id}").into())
}

#[gpui_kit::test]
fn the_dialog_lists_checklists_newest_first_and_finds_by_item_text(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (a, b) = ws.update(cx, |ws, _| {
        let a = ws.checklists.add(1);
        ws.checklists.rename(a, "Groceries", 1);
        let b = ws.checklists.add(2);
        ws.checklists.rename(b, "Deploy", 2);
        ws.checklists.add_item(b, None, "Buy milk", false, 2);
        (a, b)
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::checklist::open_checklists(ws.space.id.clone(), None, window, cx)
        });
        window.render_frame(cx);
        let (ya, yb) = (
            window.find(row(a)).bounds().origin.y,
            window.find(row(b)).bounds().origin.y,
        );
        assert!(yb < ya, "the most recently changed first");
    })
    .unwrap();
    cx.simulate_input(handle, "MILK");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(row(a)).is_none(),
            "the search leaves it out"
        );
        window.find(row(b));
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_dialog_opens_one_where_it_is_open_and_deletes_one_from_its_blocks(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let first = new_checklist(handle, &ws, cx);
    let list = ws.update(cx, |ws, _| {
        let list = shown(ws, first);
        ws.checklists.rename(list, "Deploy", 1);
        list
    });
    let second = new_checklist(handle, &ws, cx);
    // The dialog asked from the second block: the checklist is shown where it is open.
    click(handle, cx, ("checklists", second.0 as usize));
    let its_own = ws.read_with(cx, |ws, _| shown(ws, second));
    click(handle, cx, row(list));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(first));
        assert_eq!(shown(ws, second), its_own);
    });
    // Its trash deletes it on the second click, from the block too.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::checklist::open_checklists(ws.space.id.clone(), None, window, cx)
        });
        window.render_frame(cx);
        window.click(trash(list), cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.checklists.get(list).is_some(), "one click asks")
    });
    click(handle, cx, trash(list));
    ws.read_with(cx, |ws, _| {
        assert!(ws.checklists.get(list).is_none(), "deleted");
        assert_eq!(list_of(ws, first), None);
    });
}

/// A block whose checklist was deleted in the dialog.
fn empty_block(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> BlockId {
    let id = new_checklist(handle, ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let list = shown(ws, id);
            ws.delete_checklist(list, window, cx)
        });
        window.render_frame(cx);
        window.find(("checklist-empty", id.0 as usize));
    })
    .unwrap();
    id
}

#[gpui_kit::test]
fn open_a_checklist_in_an_empty_block_opens_it_there(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let id = empty_block(handle, &ws, cx);
    let kept = ws.update(cx, |ws, _| {
        let kept = ws.checklists.add(1);
        ws.checklists.rename(kept, "Kept", 1);
        kept
    });
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    click(handle, cx, ("checklist-open-here", id.0 as usize));
    click(handle, cx, row(kept));
    ws.read_with(cx, |ws, _| {
        assert_eq!(shown(ws, id), kept);
        assert_eq!(ws.space.blocks.len(), blocks, "no new block");
    });
}

#[gpui_kit::test]
fn new_checklist_in_an_empty_block_fills_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = empty_block(handle, &ws, cx);
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    click(handle, cx, ("checklist-new-here", id.0 as usize));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.checklists.get(shown(ws, id)).unwrap().name, "Checklist");
        assert_eq!(ws.space.blocks.len(), blocks, "no new block");
    });
    assert!(matches!(editing(&ws, cx, id), Some((Field::Name, _))));
}

/// From a block that shows a checklist, the dialog opens another, and its New checklist adds
/// one, in a new block beside it: the block keeps its own.
#[gpui_kit::test]
fn the_dialog_opens_a_checklist_beside_a_block_that_shows_one(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (own, other) = ws.update(cx, |ws, _| {
        let other = ws.checklists.add(1);
        ws.checklists.rename(other, "Other", 1);
        (shown(ws, id), other)
    });
    let opened = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let top = ws.space.canvas.top().unwrap();
            assert_ne!(top, id, "a new block");
            assert_eq!(shown(ws, id), own, "the block keeps its own");
            let (a, b) = (
                ws.space.canvas.get(id).unwrap().rect,
                ws.space.canvas.get(top).unwrap().rect,
            );
            assert!(!a.intersects(&b), "beside it: {a:?}, {b:?}");
            top
        })
    };
    click(handle, cx, ("checklists", id.0 as usize));
    click(handle, cx, row(other));
    let beside = opened(cx);
    ws.read_with(cx, |ws, _| assert_eq!(shown(ws, beside), other));
    // The camera went to the new block: the dialog is asked from the first one again.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::checklist::open_checklists(ws.space.id.clone(), Some(id), window, cx)
        });
    })
    .unwrap();
    click(handle, cx, "checklists-new");
    let new = opened(cx);
    assert_ne!(new, beside);
    assert!(matches!(editing(&ws, cx, new), Some((Field::Name, _))));
}

#[gpui_kit::test]
fn all_checklists_in_the_blocks_menu_opens_the_dialog(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.right_click(("title", id.0 as usize), cx);
        window.render_frame(cx);
        window.within("popup-menu").click(0usize, cx);
        window.render_frame(cx);
        window.find("checklists-find");
    })
    .unwrap();
}

#[gpui_kit::test]
fn dragging_a_row_puts_it_in_another_rows_place_or_at_the_end(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a, c) = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        let a = ws.checklists.add_item(list, None, "a", false, 1).unwrap();
        ws.checklists.add_item(list, None, "b", false, 1);
        let c = ws.checklists.add_item(list, None, "c", false, 1).unwrap();
        (list, a, c)
    });
    let drag = |cx: &mut TestAppContext, to: ElementId| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.drag_to(name("checklist-row", id, c), to, cx);
            window.render_frame(cx);
        })
        .unwrap();
    };
    drag(cx, name("checklist-row", id, a));
    assert_eq!(texts(&ws, cx, list), ["c", "a", "b"], "in a's place");
    assert!(editing(&ws, cx, id).is_none(), "a drag edits nothing");
    drag(cx, ("checklist-add", id.0 as usize).into());
    assert_eq!(texts(&ws, cx, list), ["a", "b", "c"], "at the end");
    press_undo(handle, &ws, cx, id);
    assert_eq!(texts(&ws, cx, list), ["c", "a", "b"], "Ctrl+Z puts it back");
}

/// A click on a box while the name is edited, before any key: the field stops (GPUI's blur only
/// reaches a field drawn again since it took the keyboard).
#[gpui_kit::test]
fn a_click_elsewhere_in_the_block_stops_editing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let (list, a) = two_items(&ws, cx, id);
    assert!(matches!(editing(&ws, cx, id), Some((Field::Name, _))));
    click(handle, cx, name("checklist-box", id, a));
    assert!(editing(&ws, cx, id).is_none());
    assert!(done(&ws, cx, list, a));
}

/// Review focus: editing stops when its block closes (an item left empty goes) and when its
/// checklist is deleted in the dialog (no field is left on a checklist that is gone).
#[gpui_kit::test]
fn closing_the_block_or_deleting_the_checklist_stops_editing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let list = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        ws.checklists.rename(list, "Groceries", 1);
        ws.checklists.add_item(list, None, "milk", false, 1);
        list
    });
    click(handle, cx, ("checklist-add", id.0 as usize));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(id, window, cx))
    })
    .unwrap();
    assert_eq!(texts(&ws, cx, list), ["milk"], "the empty item went");
    let other = new_checklist(handle, &ws, cx);
    let gone = ws.read_with(cx, |ws, _| shown(ws, other));
    ws.update(cx, |ws, _| ws.checklists.rename(gone, "Gone", 1));
    click(handle, cx, ("checklist-add", other.0 as usize));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.delete_checklist(gone, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    assert!(editing(&ws, cx, other).is_none());
    ws.read_with(cx, |ws, _| assert!(ws.checklists.get(gone).is_none()));
}

/// Zoom scales like a photo: a long item wraps at the same words at every zoom.
#[gpui_kit::test]
fn a_long_item_wraps_the_same_at_every_zoom(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = new_checklist(handle, &ws, cx);
    let item = ws.update(cx, |ws, _| {
        let list = shown(ws, id);
        let text =
            "Check that a long item wraps over a few lines at the block's width, and stays so";
        ws.checklists.add_item(list, None, text, false, 1).unwrap()
    });
    press(handle, cx, "escape");
    let lines: Vec<f32> = [0.5, 1.0, 2.0]
        .into_iter()
        .map(|zoom| {
            cx.update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, _| {
                    let camera = ws.space.canvas.camera;
                    ws.space.canvas.camera = Camera::at(camera.origin, zoom);
                    ws.space.flight = None;
                });
                window.render_frame(cx);
                let h = f32::from(
                    window
                        .find(name("checklist-text", id, item))
                        .bounds()
                        .size
                        .height,
                );
                (h / (13.0 * 1.5 * zoom)).round()
            })
            .unwrap()
        })
        .collect();
    assert!(lines[0] > 1.0, "it wraps: {lines:?}");
    assert!(lines.iter().all(|l| *l == lines[0]), "{lines:?}");
}

/// A checklist block of 40 items, none edited, drawn: its list overflows the block.
struct LongChecklist {
    handle: AnyWindowHandle,
    ws: Entity<Workspace>,
    block: BlockId,
    first: u64,
}

impl LongChecklist {
    fn new(cx: &mut TestAppContext) -> Self {
        let (handle, ws) = open(cx);
        let block = new_checklist(handle, &ws, cx);
        let first = ws.update(cx, |ws, _| {
            let list = shown(ws, block);
            (0..40)
                .map(|i| {
                    let text = format!("item {i}");
                    ws.checklists.add_item(list, None, &text, false, 1).unwrap()
                })
                .min()
                .unwrap()
        });
        press(handle, cx, "escape");
        Self {
            handle,
            ws,
            block,
            first,
        }
    }

    /// The bounds of the list and of what `id` names, as drawn now.
    fn bounds(
        &self,
        cx: &mut TestAppContext,
        id: impl Into<ElementId>,
    ) -> (Bounds<gpui_kit::Pixels>, Bounds<gpui_kit::Pixels>) {
        let id = id.into();
        let n = self.block.0 as usize;
        cx.update_window(self.handle, |_, window, cx| {
            window.render_frame(cx);
            (
                window.find(("checklist-list", n)).bounds(),
                window.find(id).bounds(),
            )
        })
        .unwrap()
    }

    /// How far the first item's top is below the list's: less as the list scrolls.
    fn first_below(&self, cx: &mut TestAppContext) -> f32 {
        let (list, row) = self.bounds(cx, name("checklist-row", self.block, self.first));
        f32::from(row.top() - list.top())
    }

    /// Inside the scrollbar's track (16 pixels wide), near the list's bottom.
    fn strip(&self, cx: &mut TestAppContext) -> (f32, f32) {
        let (list, _) = self.bounds(cx, ("checklist-list", self.block.0 as usize));
        (
            f32::from(list.right() - px(6.0)),
            f32::from(list.bottom() - px(12.0)),
        )
    }
}

/// A checklist longer than its block has a scrollbar over exactly its list, which a press on its
/// track scrolls.
#[gpui_kit::test]
fn a_long_checklist_has_a_scrollbar_that_scrolls_it(cx: &mut TestAppContext) {
    let long = LongChecklist::new(cx);
    let n = long.block.0 as usize;
    let (list, bar) = long.bounds(cx, ("checklist-scrollbar", n));
    assert_eq!(bar, list, "the scrollbar is over the list");
    let (before, at) = (long.first_below(cx), long.strip(cx));
    cx.update_window(long.handle, |_, window, cx| {
        super::kanban::hover_at(window, cx, at);
        press_with(window, cx, MouseButton::Left, Modifiers::default(), at, at);
    })
    .unwrap();
    let now = long.first_below(cx);
    assert!(
        now < before - 100.0,
        "the track scrolled it: {before} to {now}"
    );
}

/// Zoom scales like a photo: a scrolled checklist shows the same items at another zoom, its
/// scroll scaling with them.
#[gpui_kit::test]
fn a_scrolled_checklist_shows_the_same_items_at_another_zoom(cx: &mut TestAppContext) {
    let long = LongChecklist::new(cx);
    let (list, _) = long.bounds(cx, ("checklist-list", long.block.0 as usize));
    let center = (f32::from(list.center().x), f32::from(list.center().y));
    cx.update_window(long.handle, |_, window, cx| {
        let down = ScrollDelta::Pixels(point(px(0.), px(-100.)));
        wheel(window, cx, center, down, false);
    })
    .unwrap();
    let at_one = long.first_below(cx);
    assert!(at_one < 0.0, "scrolled past the first item: {at_one}");
    long.ws.update(cx, |ws, _| {
        let camera = ws.space.canvas.camera;
        ws.space.canvas.camera = Camera::at(camera.origin, 0.5);
        ws.space.flight = None;
    });
    let at_half = long.first_below(cx);
    assert!(
        (at_half - at_one / 2.0).abs() < 1.0,
        "at zoom 0.5: {at_half}, at 1: {at_one}"
    );
}

/// With "Focus follows the pointer" off, a press on a long checklist's scrollbar picks its block
/// like any other press in it, so the wheel scrolls its list after; a middle drag from there
/// pans the canvas.
#[gpui_kit::test]
fn a_press_on_a_long_checklists_scrollbar_picks_its_block_and_the_middle_pans(
    cx: &mut TestAppContext,
) {
    let long = LongChecklist::new(cx);
    long.ws.update(cx, |ws, _| {
        ws.config.canvas.focus_follows_pointer = false;
        ws.space.focused = None;
    });
    let at = long.strip(cx);
    cx.update_window(long.handle, |_, window, cx| {
        super::kanban::hover_at(window, cx, at);
        press_with(window, cx, MouseButton::Left, Modifiers::default(), at, at);
    })
    .unwrap();
    long.ws
        .read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(long.block)));
    let camera = long.ws.read_with(cx, |ws, _| ws.space.canvas.camera);
    cx.update_window(long.handle, |_, window, cx| {
        super::kanban::hover_at(window, cx, at);
        let to = (at.0 - 50.0, at.1);
        press_with(
            window,
            cx,
            MouseButton::Middle,
            Modifiers::default(),
            at,
            to,
        );
    })
    .unwrap();
    long.ws.read_with(cx, |ws, _| {
        assert_ne!(ws.space.canvas.camera, camera, "a middle drag pans");
    });
}

/// A press in the field of a long checklist's picked block is the field's: the block isn't
/// picked again, which would stop it.
#[gpui_kit::test]
fn a_press_in_a_long_checklists_field_keeps_editing(cx: &mut TestAppContext) {
    let long = LongChecklist::new(cx);
    let text = name("checklist-text", long.block, long.first);
    click(long.handle, cx, text.clone());
    assert!(matches!(
        editing(&long.ws, cx, long.block),
        Some((Field::Item(item), _)) if item == long.first
    ));
    let (_, field) = long.bounds(cx, text);
    let at = (f32::from(field.center().x), f32::from(field.center().y));
    cx.update_window(long.handle, |_, window, cx| {
        press_with(window, cx, MouseButton::Left, Modifiers::default(), at, at);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(editing(&long.ws, cx, long.block).is_some(), "still editing");
}

#[gpui_kit::test]
fn every_item_act_is_one_ctrl_z_step_through_one_door(cx: &mut TestAppContext) {
    use crate::workspace::ChecklistAct;
    let (handle, ws) = open(cx);
    let block = new_checklist(handle, &ws, cx);
    let (list, a) = two_items(&ws, cx, block);
    let b = ws.read_with(cx, |ws, _| ws.checklists.get(list).unwrap().items[1].id);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let act = |cx: &mut TestAppContext, act: ChecklistAct| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.act_on_checklist(&space, block, act, window, cx)
            })
        })
        .unwrap();
    };
    let items = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.checklists
                .get(list)
                .unwrap()
                .items
                .iter()
                .map(|i| (i.text.clone(), i.done))
                .collect::<Vec<_>>()
        })
    };
    let steps = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.checklists_undo.len())
    };
    let start = steps(&ws, cx);
    act(cx, ChecklistAct::Toggle { item: a });
    assert!(done(&ws, cx, list, a));
    act(cx, ChecklistAct::Delete { item: b });
    act(
        cx,
        ChecklistAct::Edit {
            item: a,
            text: "A".into(),
        },
    );
    act(
        cx,
        ChecklistAct::Add {
            text: "c\n[x] d".into(),
        },
    );
    assert_eq!(
        items(&ws, cx),
        [
            ("A".to_string(), true),
            ("c".to_string(), false),
            ("d".to_string(), true)
        ]
    );
    assert_eq!(steps(&ws, cx), start + 4, "one step each");
    for want in [
        vec![("A".to_string(), true)],
        vec![("a".to_string(), true)],
        vec![("a".to_string(), true), ("b".to_string(), false)],
        vec![("a".to_string(), false), ("b".to_string(), false)],
    ] {
        ws.update(cx, |ws, cx| assert!(ws.undo_checklists(cx)));
        assert_eq!(items(&ws, cx), want);
    }
}
