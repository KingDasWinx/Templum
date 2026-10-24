//! Undo and redo of the canvas's blocks: added, closed, moved, resized.

use super::*;

/// A click on the empty canvas: no block keeps the keyboard.
fn unfocus(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    let empty = at(ws, cx, (560.0, 330.0));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, empty, 1);
    })
    .unwrap();
}

fn press(handle: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
}

fn rect_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Option<Rect> {
    ws.read_with(cx, |ws, _| ws.space.canvas.get(id).map(|b| b.rect))
}

/// Drags the kanban block's title (at its setup place) by (100, 50).
fn drag_kanban(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    let title = at(ws, cx, (150.0, 40.0 + chrome::TITLE_H / 2.0));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            title,
            (title.0 + 100.0, title.1 + 50.0),
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_startup_terminal_and_a_restored_canvas_are_no_steps(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three_blocks(handle, &ws, cx);
    unfocus(handle, &ws, cx);
    press(handle, UNDO, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.canvas.blocks().len(), 3);
        assert!(ws.space.undo.is_empty());
    });
}

#[gpui_kit::test]
fn an_added_block_is_taken_back_and_brought_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three_blocks(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_note_at(Point::new(600.0, 40.0), window, cx)
        })
    })
    .unwrap();
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len()), 4);
    unfocus(handle, &ws, cx);
    press(handle, UNDO, cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len()), 3);
    press(handle, REDO, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.canvas.blocks().len(), 4);
        let back = ws.space.canvas.get(ws.space.selected[0]).unwrap();
        assert_eq!((back.rect.x, back.rect.y), (600.0, 40.0));
        assert!(ws.space.blocks[&back.id].note().is_some());
    });
}

#[gpui_kit::test]
fn a_move_is_taken_back_and_done_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, _) = three_blocks(handle, &ws, cx);
    let before = rect_of(&ws, cx, kanban).unwrap();
    let moved = Rect::new(before.x + 100.0, before.y + 50.0, before.w, before.h);
    drag_kanban(handle, &ws, cx);
    assert_eq!(rect_of(&ws, cx, kanban), Some(moved));
    unfocus(handle, &ws, cx);
    press(handle, UNDO, cx);
    assert_eq!(rect_of(&ws, cx, kanban), Some(before));
    press(handle, REDO, cx);
    assert_eq!(rect_of(&ws, cx, kanban), Some(moved));
    // A click that doesn't move is no step.
    unfocus(handle, &ws, cx);
    let title = at(&ws, cx, (250.0, 90.0 + chrome::TITLE_H / 2.0));
    cx.update_window(handle, |_, window, cx| click(window, cx, title, 1))
        .unwrap();
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.undo.len()), 1);
}

#[gpui_kit::test]
fn a_resized_terminal_gets_its_rect_and_its_grid_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let before = rect_of(&ws, cx, terminal).unwrap();
    let corner = at(&ws, cx, (before.right() - 2.0, before.bottom() - 2.0));
    let grid = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(terminal).unwrap().term.grid_size()
        })
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.fit_terminal(terminal, before);
            let at = |p: (f32, f32)| point(px(p.0), px(p.1));
            ws.begin_resize(terminal, templum_canvas::Edges::BOTTOM_RIGHT, at(corner));
            let to = at((corner.0 + 200.0, corner.1 + 60.0));
            ws.drag_to(to, false, cx);
            ws.release(to, window, cx);
        })
    })
    .unwrap();
    assert_ne!(rect_of(&ws, cx, terminal), Some(before));
    let wider = grid(&ws, cx);
    unfocus(handle, &ws, cx);
    press(handle, UNDO, cx);
    assert_eq!(rect_of(&ws, cx, terminal), Some(before));
    let back = grid(&ws, cx);
    assert!(
        back.0 < wider.0 && back.1 < wider.1,
        "{back:?} after {wider:?}"
    );
}

#[gpui_kit::test]
fn a_reopened_block_takes_back_its_earlier_move(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, _) = three_blocks(handle, &ws, cx);
    let before = rect_of(&ws, cx, kanban).unwrap();
    drag_kanban(handle, &ws, cx);
    let moved = rect_of(&ws, cx, kanban).unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(kanban, window, cx))
    })
    .unwrap();
    press(handle, UNDO, cx);
    let back = ws.read_with(cx, |ws, _| ws.space.selected[0]);
    assert_ne!(back, kanban, "a new block");
    assert_eq!(rect_of(&ws, cx, back), Some(moved));
    press(handle, UNDO, cx);
    assert_eq!(rect_of(&ws, cx, back), Some(before), "the move names it");
    press(handle, REDO, cx);
    press(handle, REDO, cx);
    assert_eq!(rect_of(&ws, cx, back), None, "closed again");
}

#[gpui_kit::test]
fn a_new_change_drops_what_was_undone(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, _) = three_blocks(handle, &ws, cx);
    let before = rect_of(&ws, cx, kanban).unwrap();
    drag_kanban(handle, &ws, cx);
    unfocus(handle, &ws, cx);
    press(handle, UNDO, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_kanban_at(Point::new(900.0, 40.0), window, cx)
        })
    })
    .unwrap();
    unfocus(handle, &ws, cx);
    press(handle, REDO, cx);
    assert_eq!(rect_of(&ws, cx, kanban), Some(before));
}

#[gpui_kit::test]
fn a_paste_is_one_step(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three_blocks(handle, &ws, cx);
    let (from, to) = (at(&ws, cx, (20.0, 20.0)), at(&ws, cx, (330.0, 100.0)));
    cx.update_window(handle, |_, window, cx| {
        press_with(window, cx, MouseButton::Left, SHIFT, from, to);
        window.press(COPY, cx);
        window.press(PASTE, cx);
        window.press(UNDO, cx);
    })
    .unwrap();
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len()), 3);
}

/// Ctrl+Z brings a closed terminal back with its shell.
#[gpui_kit::test]
fn ctrl_z_brings_a_closed_terminal_back_with_its_shell(cx: &mut TestAppContext) {
    use templum_terminal::ShellOption;
    let (handle, ws) = open(cx);
    let (program, args) = quiet();
    let quiet_shell = ShellOption {
        label: "Quiet".into(),
        program,
        args,
    };
    let id = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.shells = vec![quiet_shell.clone()];
                ws.open_shell(Some(quiet_shell.clone()), None, window, cx);
                let id = ws.space.canvas.top().unwrap();
                ws.close(id, window, cx);
                id
            })
        })
        .unwrap();
    press(handle, UNDO, cx);
    ws.read_with(cx, |ws, _| {
        let back = ws.space.top().unwrap();
        assert_ne!(back, id, "a new block");
        assert_eq!(ws.space.terminal(back).unwrap().shell, Some(quiet_shell));
    });
}
