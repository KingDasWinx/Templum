//! Selecting blocks (Shift+drag, Shift+click on a title), and what acts on them: a title's drag,
//! Delete, Copy and Paste; Ctrl+Z reopening closed blocks.

use super::*;
use crate::workspace::history::Then;

#[gpui_kit::test]
fn shift_drag_selects_the_blocks_it_touches_and_a_click_clears_them(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let (from, to) = (at(&ws, cx, (20.0, 20.0)), at(&ws, cx, (330.0, 100.0)));
    cx.update_window(handle, |_, window, cx| {
        press_with(window, cx, MouseButton::Left, SHIFT, from, to);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.selected, [kanban, note]);
        assert_eq!(ws.space.focused, None, "selecting takes the focus away");
        assert!(ws.drag.is_none());
    });
    // A plain drag pans and keeps them; a click on the empty canvas lets them go.
    let empty = at(&ws, cx, (800.0, 300.0));
    cx.update_window(handle, |_, window, cx| {
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            empty,
            (empty.0 + 30.0, empty.1),
        );
    })
    .unwrap();
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.selected.len()), 2);
    cx.update_window(handle, |_, window, cx| click(window, cx, empty, 1))
        .unwrap();
    assert!(ws.read_with(cx, |ws, _| ws.space.selected.is_empty()));
}

#[gpui_kit::test]
fn shift_click_on_a_title_selects_a_block_and_again_lets_it_go(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, terminal) = three_blocks(handle, &ws, cx);
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(terminal).unwrap().rect);
    let title = at(
        &ws,
        cx,
        (rect.x + rect.w / 2.0, rect.y + chrome::TITLE_H / 2.0),
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(kanban, window, cx));
        press_with(window, cx, MouseButton::Left, SHIFT, title, title);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.selected, [terminal]);
        assert_eq!(ws.space.focused, None);
        assert_eq!(
            ws.space.canvas.get(terminal).unwrap().rect,
            rect,
            "not moved"
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        press_with(window, cx, MouseButton::Left, SHIFT, title, title);
    })
    .unwrap();
    assert!(ws.read_with(cx, |ws, _| ws.space.selected.is_empty()));
}

#[gpui_kit::test]
fn delete_closes_the_focused_block_but_a_terminal_keeps_the_key(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(kanban, window, cx));
        window.press("delete", cx);
        ws.update(cx, |ws, cx| ws.focus_block(terminal, window, cx));
        window.press("delete", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.canvas.get(kanban).is_none());
        assert!(ws.space.canvas.get(note).is_some());
        assert!(ws.space.canvas.get(terminal).is_some());
    });
}

#[gpui_kit::test]
fn copy_paste_and_delete_selected_blocks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let (from, to) = (at(&ws, cx, (20.0, 20.0)), at(&ws, cx, (330.0, 100.0)));
    cx.update_window(handle, |_, window, cx| {
        press_with(window, cx, MouseButton::Left, SHIFT, from, to);
        window.press(COPY, cx);
        window.press(PASTE, cx);
    })
    .unwrap();
    let pasted = ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.space.canvas.blocks().len(), 5);
        let pasted = ws.space.selected.clone();
        assert_eq!(pasted.len(), 2, "the pasted blocks are selected");
        assert!(!pasted.contains(&kanban) && !pasted.contains(&note));
        assert!(ws.space.kanban(pasted[0]).is_some());
        // A copy is another block for agents and wires (docs/specs/connections.md, Identity).
        assert_ne!(ws.space.uids[&pasted[0]], ws.space.uids[&kanban]);
        assert_eq!(ws.space.uids[&pasted[0]].len(), 5);
        let copy = ws.space.blocks[&pasted[1]].note().unwrap();
        assert_eq!(copy.text(cx), "hello");
        let original = ws.space.blocks[&note].note().unwrap();
        assert_ne!(
            copy.ids(),
            original.ids(),
            "a copy of the note, not the note"
        );
        // Kept side by side, somewhere else.
        let (a, b) = (
            ws.space.canvas.get(pasted[0]).unwrap().rect,
            ws.space.canvas.get(pasted[1]).unwrap().rect,
        );
        assert_eq!((b.x - a.x, b.y - a.y), (260.0, 0.0));
        assert_ne!(a, ws.space.canvas.get(kanban).unwrap().rect);
        pasted
    });
    cx.update_window(handle, |_, window, cx| window.press("delete", cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.canvas.blocks().len(), 3);
        assert!(pasted.iter().all(|id| ws.space.canvas.get(*id).is_none()));
        assert!(ws.space.selected.is_empty());
        assert!(ws.space.canvas.get(terminal).is_some());
    });
}

#[gpui_kit::test]
fn text_on_the_clipboard_is_not_pasted_as_blocks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three_blocks(handle, &ws, cx);
    cx.update(|cx| cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("ls\n".into())));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, _| ws.space.focused = None);
        window.press(PASTE, cx);
    })
    .unwrap();
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len()), 3);
}

#[gpui_kit::test]
fn dragging_a_selected_title_moves_every_selected_block(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let before = ws.read_with(cx, |ws, _| {
        [kanban, note, terminal].map(|id| ws.space.canvas.get(id).unwrap().rect)
    });
    let (from, to) = (at(&ws, cx, (20.0, 20.0)), at(&ws, cx, (330.0, 100.0)));
    let title = at(&ws, cx, (150.0, 40.0 + chrome::TITLE_H / 2.0));
    cx.update_window(handle, |_, window, cx| {
        press_with(window, cx, MouseButton::Left, SHIFT, from, to);
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
    ws.read_with(cx, |ws, _| {
        let moved = |id, r: Rect| {
            assert_eq!(
                ws.space.canvas.get(id).unwrap().rect,
                Rect::new(r.x + 100.0, r.y + 50.0, r.w, r.h)
            )
        };
        moved(kanban, before[0]);
        moved(note, before[1]);
        assert_eq!(ws.space.canvas.get(terminal).unwrap().rect, before[2]);
        assert_eq!(ws.space.selected, [kanban, note], "still selected");
        assert_eq!(ws.space.focused, None);
    });
}

#[gpui_kit::test]
fn ctrl_z_reopens_the_blocks_delete_closed_where_they_were(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let rects = ws.read_with(cx, |ws, _| {
        [kanban, note].map(|id| ws.space.canvas.get(id).unwrap().rect)
    });
    let (from, to) = (at(&ws, cx, (20.0, 20.0)), at(&ws, cx, (330.0, 100.0)));
    cx.update_window(handle, |_, window, cx| {
        press_with(window, cx, MouseButton::Left, SHIFT, from, to);
        window.press("delete", cx);
    })
    .unwrap();
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len()), 1);
    cx.update_window(handle, |_, window, cx| window.press(UNDO, cx))
        .unwrap();
    ws.read_with(cx, |ws, cx| {
        let back = ws.space.selected.clone();
        assert_eq!(back.len(), 2, "reopened, selected");
        assert_eq!(ws.space.canvas.get(back[0]).unwrap().rect, rects[0]);
        assert_eq!(ws.space.canvas.get(back[1]).unwrap().rect, rects[1]);
        assert!(ws.space.kanban(back[0]).is_some());
        assert_eq!(ws.space.blocks[&back[1]].note().unwrap().text(cx), "hello");
        assert!(ws.space.undo.is_empty());
        assert_eq!(ws.space.redo.len(), 1, "Ctrl+Shift+Z closes them again");
    });
}

#[gpui_kit::test]
fn a_closed_block_leaves_no_focus_and_ctrl_z_never_reaches_past_a_terminal(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        // As its close button does: the block is focused by the press, then closed.
        ws.update(cx, |ws, cx| {
            ws.focus_block(note, window, cx);
            ws.close(note, window, cx);
        });
        assert_eq!(
            ws.read(cx).space.focused,
            None,
            "the keyboard stays on the canvas"
        );
        // A focused terminal's Ctrl+Z is its shell's.
        ws.update(cx, |ws, cx| ws.focus_block(terminal, window, cx));
        window.press(UNDO, cx);
        assert_eq!(ws.read(cx).space.canvas.blocks().len(), 2);
        ws.update(cx, |ws, cx| ws.focus_block(kanban, window, cx));
        window.press(UNDO, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.space.canvas.blocks().len(), 3);
        let back = ws.space.selected[0];
        assert_eq!(ws.space.blocks[&back].note().unwrap().text(cx), "hello");
    });
}

#[gpui_kit::test]
fn a_reopened_terminal_resumes_its_agent_but_a_pasted_one_does_not(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let folder = tempfile::tempdir().unwrap();
    let agent_of = |ws: &Workspace, id: BlockId| ws.space.terminal(id).unwrap().seen.agent.clone();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let pane = ws.space.terminal_mut(terminal).unwrap();
            pane.seen.agent = Some("claude".into());
            pane.seen.folder = Some(folder.path().to_path_buf());
            (pane.opened_by, pane.model, pane.effort) = (
                Some("b7k2m".into()),
                Some("opus".into()),
                Some("high".into()),
            );
            ws.close_now(&[terminal], Then::New, window, cx);
            assert!(ws.undo_canvas(false, window, cx));
        });
        let back = ws.read(cx).space.selected[0];
        assert_eq!(agent_of(ws.read(cx), back).as_deref(), Some("claude"));
        let kept = |ws: &Workspace, id: BlockId| {
            let p = ws.space.terminal(id).unwrap();
            (p.opened_by.clone(), p.model.clone(), p.effort.clone())
        };
        assert_eq!(
            kept(ws.read(cx), back),
            (
                Some("b7k2m".into()),
                Some("opus".into()),
                Some("high".into())
            )
        );
        window.press(COPY, cx);
        window.press(PASTE, cx);
        let pasted = ws.read(cx).space.selected[0];
        assert_ne!(pasted, back);
        assert_eq!(agent_of(ws.read(cx), pasted), None);
        assert_eq!(
            kept(ws.read(cx), pasted).0,
            None,
            "a pasted block was opened by the person"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_rectangle_held_near_the_edge_moves_the_camera_to_blocks_beyond_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three_blocks(handle, &ws, cx);
    let (far, vp) = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let vp = ws.viewport(window);
                let beyond = crate::session::Item::Kanban(crate::session::KanbanItem {
                    uid: String::new(),
                    rect: Rect::new(vp.w + 200.0, 60.0, 220.0, 200.0).into(),
                    board: None,
                });
                ws.quiet = true;
                let far = ws.add_saved(&beyond, window, cx).unwrap();
                ws.quiet = false;
                (far, vp)
            })
        })
        .unwrap();
    let (from, to) = (at(&ws, cx, (20.0, 20.0)), at(&ws, cx, (vp.w - 2.0, 100.0)));
    let event = |p: (f32, f32)| point(px(p.0), px(p.1));
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: event(from),
                modifiers: SHIFT,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: event(to),
                pressed_button: Some(MouseButton::Left),
                modifiers: SHIFT,
            }),
            cx,
        );
    })
    .unwrap();
    // The pointer stays still at the edge: frames keep moving the camera.
    ws.update(cx, |ws, _| ws.api.moved_at = None);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ws.read_with(cx, |ws, _| ws.space.selected.contains(&far)) {
        assert!(std::time::Instant::now() < deadline, "never reached");
        frame(handle, cx);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(ws.read_with(cx, |ws, _| ws.space.canvas.camera.origin.x) > 0.0);
    assert!(
        ws.read_with(cx, |ws, _| ws.api.moved_at.is_some()),
        "the API's tick follows the edge scroll"
    );
    // Let go: the camera stops.
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: event(to),
                modifiers: SHIFT,
                click_count: 1,
            }),
            cx,
        );
    })
    .unwrap();
    let stopped = ws.read_with(cx, |ws, _| ws.space.canvas.camera.origin);
    frame(handle, cx);
    std::thread::sleep(std::time::Duration::from_millis(40));
    frame(handle, cx);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.space.canvas.camera.origin),
        stopped
    );
    assert!(ws.read_with(cx, |ws, _| ws.space.selected.contains(&far)));
}

#[gpui_kit::test]
fn a_selection_held_near_the_edge_moves_the_canvas_and_every_block_with_it(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let vp = cx
        .update_window(handle, |_, window, cx| ws.read(cx).viewport(window))
        .unwrap();
    let (from, to) = (at(&ws, cx, (20.0, 20.0)), at(&ws, cx, (330.0, 100.0)));
    let title = at(&ws, cx, (150.0, 40.0 + chrome::TITLE_H / 2.0));
    // Held by a selected title, the pointer at the bottom edge.
    let edge = at(&ws, cx, (150.0, vp.h - 2.0));
    cx.update_window(handle, |_, window, cx| {
        press_with(window, cx, MouseButton::Left, SHIFT, from, to);
        window.render_frame(cx);
        let event = |p: (f32, f32)| point(px(p.0), px(p.1));
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: event(title),
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: event(edge),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }),
            cx,
        );
    })
    .unwrap();
    let screen = |ws: &Workspace, id| {
        let r = ws.space.canvas.get(id).unwrap().rect;
        ws.space.canvas.camera.to_screen(Point::new(r.x, r.y))
    };
    let held = ws.read_with(cx, |ws, _| [kanban, note].map(|id| screen(ws, id)));
    let still = ws.read_with(cx, |ws, _| ws.space.canvas.get(terminal).unwrap().rect);
    // Two frames, the pointer still: the second one moves the canvas.
    let t = std::time::Instant::now();
    ws.update(cx, |ws, _| {
        assert!(ws.drag_edge(vp, t));
        assert!(ws.drag_edge(vp, t + std::time::Duration::from_millis(40)));
    });
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.space.canvas.camera.origin.y > 0.0,
            "the canvas moved down"
        );
        assert_eq!(ws.space.canvas.camera.origin.x, 0.0);
        for (id, was) in [kanban, note].into_iter().zip(held) {
            let now = screen(ws, id);
            assert!(
                (now.x - was.x).abs() < 0.01 && (now.y - was.y).abs() < 0.01,
                "{id:?} stays under the pointer"
            );
        }
        assert_eq!(ws.space.canvas.get(terminal).unwrap().rect, still);
    });
}

#[gpui_kit::test]
fn a_shift_press_near_the_edge_leaves_the_camera_still(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    three_blocks(handle, &ws, cx);
    let vp = cx
        .update_window(handle, |_, window, cx| ws.read(cx).viewport(window))
        .unwrap();
    let edge = at(&ws, cx, (vp.w - 2.0, 100.0));
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: point(px(edge.0), px(edge.1)),
                modifiers: SHIFT,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    for _ in 0..3 {
        frame(handle, cx);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.space.canvas.camera.origin),
        Point::new(0.0, 0.0)
    );
}

/// While a title is held, its block (or every selected block, from a selected title) is moving:
/// the title shows a closed hand; let go, an open one again.
#[gpui_kit::test]
fn a_held_title_moves_its_block_until_let_go(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let title = at(&ws, cx, (150.0, 40.0 + chrome::TITLE_H / 2.0));
    let press = |cx: &mut TestAppContext, down: bool| {
        let position = point(px(title.0), px(title.1));
        cx.update_window(handle, |_, window, cx| {
            let event = if down {
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                    first_mouse: false,
                })
            } else {
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position,
                    modifiers: Modifiers::default(),
                    click_count: 1,
                })
            };
            window.dispatch_event(event, cx);
            window.render_frame(cx);
        })
        .unwrap();
    };
    let moving = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| [kanban, note, terminal].map(|id| ws.moving(id)))
    };
    press(cx, true);
    assert_eq!(moving(cx), [true, false, false]);
    press(cx, false);
    assert_eq!(moving(cx), [false, false, false]);
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(cx, true);
    assert_eq!(moving(cx), [true, true, false], "the selection moves");
    press(cx, false);
    assert_eq!(moving(cx), [false, false, false]);
}

/// Selects `ids` (which takes the focus away) and draws a frame, so the blocks wear the
/// selection's handlers.
fn select(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    ids: &[BlockId],
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, _| {
            ws.space.focused = None;
            ws.space.selected = ids.to_vec();
        });
        window.render_frame(cx);
    })
    .unwrap();
}

fn rects(ws: &Entity<Workspace>, cx: &mut TestAppContext, ids: &[BlockId]) -> Vec<Rect> {
    ws.read_with(cx, |ws, _| {
        ids.iter()
            .map(|id| ws.space.canvas.get(*id).unwrap().rect)
            .collect()
    })
}

fn shifted(r: Rect, dx: f32, dy: f32) -> Rect {
    Rect::new(r.x + dx, r.y + dy, r.w, r.h)
}

#[gpui_kit::test]
fn dragging_a_selected_terminals_body_moves_every_selected_block(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[note, terminal]);
    let before = rects(&ws, cx, &[note, terminal]);
    let body = at(&ws, cx, (200.0, 560.0));
    cx.update_window(handle, |_, window, cx| {
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            body,
            (body.0 + 100.0, body.1 + 50.0),
        );
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            [note, terminal].map(|id| ws.space.canvas.get(id).unwrap().rect),
            [
                shifted(before[0], 100.0, 50.0),
                shifted(before[1], 100.0, 50.0)
            ]
        );
        assert_eq!(ws.space.selected, [note, terminal], "still selected");
        assert_eq!(ws.space.focused, None);
        let pane = ws.space.terminal(terminal).unwrap();
        assert!(
            pane.term.shared().selection_text().is_none(),
            "the terminal started no text selection"
        );
    });
}

#[gpui_kit::test]
fn dragging_the_empty_canvas_inside_the_box_moves_the_selection(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[kanban, note]);
    let before = rects(&ws, cx, &[kanban, note]);
    // Between the two blocks, and in the box's margin.
    for (n, gap) in [(280.0, 150.0), (36.0, 150.0)].into_iter().enumerate() {
        let from = at(&ws, cx, gap);
        let dy = 30.0 * (n + 1) as f32;
        cx.update_window(handle, |_, window, cx| {
            press_with(
                window,
                cx,
                MouseButton::Left,
                Modifiers::default(),
                from,
                (from.0, from.1 + dy),
            );
            window.render_frame(cx);
        })
        .unwrap();
        let moved = (30.0 * (n + 1) as f32) * (n + 2) as f32 / 2.0;
        assert_eq!(
            rects(&ws, cx, &[kanban, note]),
            [
                shifted(before[0], 0.0, moved),
                shifted(before[1], 0.0, moved)
            ]
        );
    }
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.selected.len()), 2);
}

#[gpui_kit::test]
fn the_selection_box_wraps_two_or_more_selected_blocks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[kanban]);
    assert_eq!(ws.read_with(cx, |ws, _| ws.selection_box()), None);
    select(handle, &ws, cx, &[kanban, note]);
    let margin = crate::workspace::selection::BOX_MARGIN;
    let [a, b] = rects(&ws, cx, &[kanban, note])[..] else {
        unreachable!()
    };
    let union = a.union(&b);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.selection_box()),
        Some(Rect::new(
            union.x - margin,
            union.y - margin,
            union.w + 2.0 * margin,
            union.h + 2.0 * margin
        ))
    );
}

#[gpui_kit::test]
fn a_click_on_a_selected_body_picks_that_block_and_ends_the_selection(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[kanban, note, terminal]);
    let before = rects(&ws, cx, &[kanban, note, terminal]);
    let body = at(&ws, cx, (200.0, 560.0));
    cx.update_window(handle, |_, window, cx| click(window, cx, body, 1))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(terminal));
        assert!(ws.space.selected.is_empty());
        assert!(ws.drag.is_none());
    });
    assert_eq!(
        rects(&ws, cx, &[kanban, note, terminal]),
        before,
        "not moved"
    );
}

#[gpui_kit::test]
fn a_click_on_the_empty_canvas_inside_the_box_ends_the_selection(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[kanban, note]);
    let before = rects(&ws, cx, &[kanban, note]);
    let gap = at(&ws, cx, (280.0, 150.0));
    cx.update_window(handle, |_, window, cx| click(window, cx, gap, 1))
        .unwrap();
    assert!(ws.read_with(cx, |ws, _| ws.space.selected.is_empty()));
    assert_eq!(rects(&ws, cx, &[kanban, note]), before);
}

#[gpui_kit::test]
fn shift_click_on_a_selected_body_takes_the_block_out_of_the_selection(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[kanban, note, terminal]);
    let body = at(&ws, cx, (200.0, 560.0));
    cx.update_window(handle, |_, window, cx| {
        press_with(window, cx, MouseButton::Left, SHIFT, body, body);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.selected, [kanban, note]);
        assert_eq!(ws.space.focused, None);
        assert!(
            ws.space
                .terminal(terminal)
                .unwrap()
                .term
                .shared()
                .selection_text()
                .is_none()
        );
    });
}

#[gpui_kit::test]
fn right_click_on_a_selected_body_still_opens_the_menu(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[kanban, note]);
    let body = at(&ws, cx, (410.0, 140.0));
    cx.update_window(handle, |_, window, cx| right_click(window, cx, body))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.menu.is_some());
        assert!(ws.drag.is_none());
    });
}

fn move_to(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32)) {
    window.dispatch_event(
        PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
            position: point(px(at.0), px(at.1)),
            pressed_button: None,
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

fn down(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32), modifiers: Modifiers) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(at.0), px(at.1)),
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn up(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32)) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: point(px(at.0), px(at.1)),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
}

/// Held by a body, the wheel pans the canvas (the selected block under the pointer scrolls
/// nothing) and carries the group; let go without moving the mouse, that is no click: the
/// blocks stay selected, where they were carried, and Ctrl+Z takes them back.
#[gpui_kit::test]
fn the_wheel_while_held_by_a_body_carries_the_group_and_is_no_click(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[note, terminal]);
    let before = rects(&ws, cx, &[note, terminal]);
    let camera = ws.read_with(cx, |ws, _| ws.space.canvas.camera.origin);
    let body = at(&ws, cx, (200.0, 560.0));
    cx.update_window(handle, |_, window, cx| {
        down(window, cx, body, Modifiers::default());
        wheel(
            window,
            cx,
            body,
            ScrollDelta::Pixels(point(px(0.0), px(-60.0))),
            false,
        );
        up(window, cx, body);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_ne!(ws.space.canvas.camera.origin, camera, "the canvas panned");
        assert_eq!(
            ws.space.selected,
            [note, terminal],
            "no click: still selected"
        );
        assert_eq!(ws.space.focused, None);
        assert_eq!(ws.space.undo.len(), 1, "carried: a step");
    });
    assert_ne!(rects(&ws, cx, &[note, terminal]), before);
    cx.update_window(handle, |_, window, cx| window.press(UNDO, cx))
        .unwrap();
    assert_eq!(rects(&ws, cx, &[note, terminal]), before);
}

/// At far zoom the label lies over the top of the body: it is the title, so a click on it keeps
/// the selection, as on a title near.
#[gpui_kit::test]
fn a_click_on_a_selected_far_label_keeps_the_selection(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 0.3);
        ws.space.flight = None;
    });
    frame(handle, cx);
    select(handle, &ws, cx, &[kanban, note]);
    // The label's lowest row, which is over the body, not the header.
    let (label, body_top) = cx
        .update_window(handle, |_, window, _| {
            let label = window.find(("title", kanban.0 as usize)).bounds();
            let body = window.find(("body", kanban.0 as usize)).bounds();
            (label, body.origin.y)
        })
        .unwrap();
    assert!(label.bottom() > body_top + px(4.0), "over the body");
    let at = (
        f32::from(label.origin.x + label.size.width / 2.0),
        f32::from(label.bottom() - px(1.0)),
    );
    cx.update_window(handle, |_, window, cx| click(window, cx, at, 1))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.selected, [kanban, note]);
        assert_eq!(ws.space.focused, None);
    });
}

/// The bottom-left corner's square is twice the strips' width: a press anywhere in it resizes.
#[gpui_kit::test]
fn a_press_in_a_selected_blocks_resize_corner_resizes(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[note, terminal]);
    let rect = rects(&ws, cx, &[terminal])[0];
    let inside = at(&ws, cx, (rect.x + 100.0, rect.y + 100.0));
    let corner = at(&ws, cx, (rect.x + 9.0, rect.bottom() - 9.0));
    cx.update_window(handle, |_, window, cx| {
        move_to(window, cx, inside);
        window.render_frame(cx);
        down(window, cx, corner, Modifiers::default());
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(
            matches!(
                ws.drag,
                Some(crate::workspace::Drag::Canvas(
                    templum_canvas::Gesture::Resize { .. }
                ))
            ),
            "a resize, not the group's move nor a text selection"
        );
    });
}

/// Ctrl+click (Cmd on macOS) on a selected terminal is the terminal's (a link opens): no group
/// move starts.
#[gpui_kit::test]
fn ctrl_click_on_a_selected_terminal_starts_no_group_move(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[note, terminal]);
    let body = at(&ws, cx, (200.0, 560.0));
    cx.update_window(handle, |_, window, cx| {
        down(window, cx, body, Modifiers::secondary_key());
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.moving(terminal) && !ws.moving(note));
        assert_eq!(ws.space.focused, Some(terminal), "a click on the terminal");
    });
}

/// A click that slips a pixel or two is still a click: the body is picked, or the selection ends,
/// and the blocks are back where they were, with no step for Ctrl+Z.
#[gpui_kit::test]
fn a_click_that_moves_a_little_is_still_a_click(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let ids = [kanban, note, terminal];
    let before = rects(&ws, cx, &ids);
    let slip = |cx: &mut TestAppContext, from: (f32, f32)| {
        cx.update_window(handle, |_, window, cx| {
            press_with(
                window,
                cx,
                MouseButton::Left,
                Modifiers::default(),
                from,
                (from.0 + 2.0, from.1 + 1.0),
            );
        })
        .unwrap();
    };
    select(handle, &ws, cx, &ids);
    let body = at(&ws, cx, (200.0, 560.0));
    slip(cx, body);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(terminal), "the body is picked");
        assert!(ws.space.selected.is_empty());
        assert!(ws.space.undo.is_empty());
    });
    assert_eq!(rects(&ws, cx, &ids), before);
    select(handle, &ws, cx, &[kanban, note]);
    let gap = at(&ws, cx, (280.0, 150.0));
    slip(cx, gap);
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.selected.is_empty(), "the box's click ends it");
        assert!(ws.space.undo.is_empty());
    });
    assert_eq!(rects(&ws, cx, &ids), before);
}

/// Far away, a double-click on a selected block's label frames it, as on a title.
#[gpui_kit::test]
fn a_double_click_on_a_selected_far_label_frames_the_block(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 0.3);
        ws.space.flight = None;
    });
    frame(handle, cx);
    select(handle, &ws, cx, &[kanban, note]);
    let label = cx
        .update_window(handle, |_, window, _| {
            window.find(("title", kanban.0 as usize)).bounds()
        })
        .unwrap();
    let at = (
        f32::from(label.origin.x + label.size.width / 2.0),
        f32::from(label.bottom() - px(1.0)),
    );
    cx.update_window(handle, |_, window, cx| click(window, cx, at, 2))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.space.canvas.camera.zoom() > 0.3,
            "the camera flies to it"
        );
        assert_eq!(ws.space.focused, Some(kanban));
    });
}

/// Only a terminal has links: Ctrl+click on a selected note is any other press, a group move.
#[gpui_kit::test]
fn ctrl_click_on_a_selected_note_is_a_plain_press(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    select(handle, &ws, cx, &[kanban, note]);
    let body = at(&ws, cx, (410.0, 140.0));
    cx.update_window(handle, |_, window, cx| {
        down(window, cx, body, Modifiers::secondary_key());
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.moving(note) && ws.moving(kanban));
    });
}

fn drag_move(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32)) {
    window.dispatch_event(
        PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
            position: point(px(at.0), px(at.1)),
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }),
        cx,
    );
}

/// Carried by the canvas's edge and brought back to where it was pressed, a selection held by a
/// body is no click: it stays selected, where the canvas carried it.
#[gpui_kit::test]
fn a_selection_carried_by_the_edge_and_brought_back_is_no_click(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    let vp = cx
        .update_window(handle, |_, window, cx| ws.read(cx).viewport(window))
        .unwrap();
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(terminal).unwrap().rect =
            Rect::new(40.0, vp.h - 200.0, 400.0, 300.0);
    });
    select(handle, &ws, cx, &[note, terminal]);
    let before = rects(&ws, cx, &[note, terminal]);
    let (body, edge) = (
        at(&ws, cx, (200.0, vp.h - 30.0)),
        at(&ws, cx, (200.0, vp.h - 2.0)),
    );
    cx.update_window(handle, |_, window, cx| {
        down(window, cx, body, Modifiers::default());
        drag_move(window, cx, edge);
    })
    .unwrap();
    let t = std::time::Instant::now();
    ws.update(cx, |ws, _| {
        assert!(ws.drag_edge(vp, t));
        assert!(ws.drag_edge(vp, t + std::time::Duration::from_millis(40)));
    });
    cx.update_window(handle, |_, window, cx| {
        drag_move(window, cx, body);
        up(window, cx, body);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.selected,
            [note, terminal],
            "no click: still selected"
        );
        assert_eq!(ws.space.focused, None);
        assert!(ws.space.canvas.camera.origin.y > 0.0, "the canvas went on");
    });
    assert_ne!(
        rects(&ws, cx, &[note, terminal]),
        before,
        "carried, not put back"
    );
}
