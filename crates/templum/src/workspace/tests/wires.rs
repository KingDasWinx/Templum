//! Wires between blocks (docs/specs/connections.md, Wires).

use serde_json::json;
use templum_canvas::{Color, Drawing, ShapeKind, Wire};

use super::bus::{call, call_later, me};
use super::*;
use crate::workspace::Tool;
use crate::workspace::history::Then;

/// A click on the empty canvas: no block keeps the keyboard.
fn unfocus(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    let empty = at(ws, cx, (700.0, 300.0));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, empty, 1);
    })
    .unwrap();
}

/// The block ids (`uid`) of blocks `ids`.
fn uids<const N: usize>(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    ids: [BlockId; N],
) -> [String; N] {
    ws.read_with(cx, |ws, _| ids.map(|id| ws.space.uids[&id].clone()))
}

/// `three_blocks`, the note moved right of the kanban block (40, 40, 420 × 280, its smallest) to
/// (500, 80, 220 × 200): the wire between them runs straight across the gap, at y 180, from x
/// 460 to 500.
fn apart(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (BlockId, BlockId, BlockId) {
    let (kanban, note, terminal) = three_blocks(handle, ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(note).unwrap().rect = Rect::new(500.0, 80.0, 220.0, 200.0);
    });
    (kanban, note, terminal)
}

fn wires(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<Wire> {
    ws.read_with(cx, |ws, _| ws.space.wires.clone())
}

/// Right-clicks block `id`'s title, picks the menu's item `index` (separators count), then the
/// submenu's item `sub`.
fn pick(handle: AnyWindowHandle, cx: &mut TestAppContext, id: BlockId, index: usize, sub: usize) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("title", id.0 as usize), cx);
        window.render_frame(cx);
        window.within("popup-menu").click(index, cx);
        window.render_frame(cx);
        window.within("submenu").within("popup-menu").click(sub, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn two_blocks_are_wired_once_and_never_to_themselves(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        assert!(ws.connect(kanban, note, cx));
        assert!(
            !ws.connect(note, kanban, cx),
            "already wired, whichever end asks"
        );
        assert!(!ws.connect(kanban, kanban, cx), "a block to itself");
        assert!(
            !ws.connect(kanban, BlockId(999), cx),
            "a block that isn't there"
        );
        assert!(ws.space.wired(note, kanban));
        assert_eq!(ws.space.wires.len(), 1);
        assert_eq!(ws.space.undo.len(), 1, "the refused ones are no steps");
    });
}

#[gpui_kit::test]
fn ctrl_z_takes_a_wire_back_and_ctrl_shift_z_makes_it_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let wire = Wire::new(kanban, note).unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.connect(kanban, note, cx);
            assert!(ws.undo_canvas(false, window, cx));
            assert!(ws.space.wires.is_empty());
            assert!(ws.undo_canvas(true, window, cx));
            assert_eq!(ws.space.wires, [wire]);
            assert!(ws.disconnect(wire, cx));
            assert!(ws.space.wires.is_empty());
            assert!(ws.undo_canvas(false, window, cx), "unwiring is a step too");
            assert_eq!(ws.space.wires, [wire]);
        })
    })
    .unwrap();
}

#[gpui_kit::test]
fn connect_names_a_wire_in_one_ctrl_z_step(cx: &mut TestAppContext) {
    use super::templum_agent::control;
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let wire = Wire::new(kanban, note).unwrap();
    let [a, b] = uids(&ws, cx, [kanban, note]);
    let steps = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.undo.len())
    };
    let label = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.label(wire).map(str::to_string))
    };
    let undo = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| assert!(ws.undo_canvas(false, window, cx)))
        })
        .unwrap()
    };
    let (failed, text) = control(handle, &ws, cx, "connect", json!({"a": a, "b": b}));
    assert!(!failed, "{text}");
    assert_eq!(wires(&ws, cx), [wire]);
    let n = steps(&ws, cx);
    // Wired already: the label renames, one step.
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "connect",
        json!({"a": a, "b": b, "label": "deploys"}),
    );
    assert!(!failed, "{text}");
    assert_eq!(
        text,
        format!("Named the wire between {a} and {b} \"deploys\".")
    );
    assert_eq!(label(&ws, cx).as_deref(), Some("deploys"));
    assert_eq!(steps(&ws, cx), n + 1);
    undo(&ws, cx);
    assert_eq!(label(&ws, cx), None);
    assert_eq!(wires(&ws, cx), [wire]);
    // Blank takes the name off.
    control(
        handle,
        &ws,
        cx,
        "connect",
        json!({"a": a, "b": b, "label": "deploys"}),
    );
    let (_, text) = control(
        handle,
        &ws,
        cx,
        "connect",
        json!({"a": a, "b": b, "label": " "}),
    );
    assert_eq!(
        text,
        format!("Took the name off the wire between {a} and {b}.")
    );
    assert_eq!(label(&ws, cx), None);
    // Not wired: the wire is the step, the name is kept by its key.
    control(handle, &ws, cx, "disconnect", json!({"a": a, "b": b}));
    let n = steps(&ws, cx);
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "connect",
        json!({"a": a, "b": b, "label": "feeds"}),
    );
    assert!(!failed, "{text}");
    assert_eq!(text, format!("Connected {a} and {b} as \"feeds\"."));
    assert_eq!(label(&ws, cx).as_deref(), Some("feeds"));
    assert_eq!(steps(&ws, cx), n + 1);
    undo(&ws, cx);
    assert!(wires(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn closing_a_block_takes_its_wires_and_undoing_brings_them_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let [kanban_uid] = uids(&ws, cx, [kanban]);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.connect(kanban, note, cx);
            ws.connect(kanban, terminal, cx);
            ws.connect(note, terminal, cx);
            ws.close_now(&[kanban], Then::New, window, cx);
            assert_eq!(ws.space.wires, [Wire::new(note, terminal).unwrap()]);
            assert!(ws.undo_canvas(false, window, cx));
            let back = ws
                .space
                .block_by_uid(&kanban_uid)
                .expect("back, with its id");
            assert!(ws.space.wired(back, note) && ws.space.wired(back, terminal));
            assert_eq!(ws.space.wires.len(), 3);
            // Closed again (Ctrl+Shift+Z), back again (Ctrl+Z): its wires each time.
            assert!(ws.undo_canvas(true, window, cx));
            assert_eq!(ws.space.wires.len(), 1);
            assert!(ws.undo_canvas(false, window, cx));
            assert_eq!(ws.space.wires.len(), 3);
        })
    })
    .unwrap();
}

/// Two wired blocks closed one after the other, both brought back: each got a new canvas id, and
/// the wire comes back between them.
#[gpui_kit::test]
fn a_wire_comes_back_between_two_blocks_closed_one_after_the_other(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let [k, n] = uids(&ws, cx, [kanban, note]);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.connect(kanban, note, cx);
            ws.close_now(&[kanban], Then::New, window, cx);
            ws.close_now(&[note], Then::New, window, cx);
            assert!(ws.space.wires.is_empty());
            assert!(ws.undo_canvas(false, window, cx));
            assert!(ws.space.wires.is_empty(), "its other end is still closed");
            assert!(ws.undo_canvas(false, window, cx));
            let (kanban, note) = (
                ws.space.block_by_uid(&k).unwrap(),
                ws.space.block_by_uid(&n).unwrap(),
            );
            assert_eq!(ws.space.wires, [Wire::new(kanban, note).unwrap()]);
        })
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_block_moved_to_another_workspace_leaves_its_wires(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.connect(kanban, note, cx);
            ws.connect(note, terminal, cx);
            ws.move_blocks(vec![kanban], None, window, cx);
        })
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let counts: Vec<usize> = ws.spaces().map(|s| s.wires.len()).collect();
        assert_eq!(
            counts.iter().sum::<usize>(),
            1,
            "note–terminal stays: {counts:?}"
        );
    });
}

#[gpui_kit::test]
fn a_pasted_copy_of_a_wired_block_comes_without_wires(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    unfocus(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
        ws.space.selected = vec![kanban];
    });
    cx.update_window(handle, |_, window, cx| {
        window.press(COPY, cx);
        window.press(PASTE, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_ne!(ws.space.selected, [kanban], "the copy is selected");
    });
    assert_eq!(wires(&ws, cx), [Wire::new(kanban, note).unwrap()]);
}

#[gpui_kit::test]
fn a_blocks_menu_connects_and_disconnects_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    // All boards…, —, Fullscreen, Connect to ▸ (the terminal, the note: back to front).
    pick(handle, cx, kanban, 3, 1);
    assert_eq!(wires(&ws, cx), [Wire::new(kanban, note).unwrap()]);
    // All boards…, —, Fullscreen, Connect to ▸ (the terminal), Disconnect from ▸ (the note).
    pick(handle, cx, kanban, 4, 0);
    assert!(wires(&ws, cx).is_empty());
    // Both were steps: Ctrl+Z wires them again.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| assert!(ws.undo_canvas(false, window, cx)))
    })
    .unwrap();
    assert_eq!(wires(&ws, cx), [Wire::new(kanban, note).unwrap()]);
}

#[gpui_kit::test]
fn wires_follow_their_blocks_in_the_titles_dim_color_and_grid_mode_hides_them(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.connect(kanban, note, cx);
            // From the middle of the kanban block's right edge to the middle of the note's left
            // edge.
            let curves = ws.wire_curves();
            assert_eq!(curves.len(), 1);
            assert_eq!(
                (curves[0].0.from, curves[0].0.to),
                (Point::new(460.0, 180.0), Point::new(500.0, 180.0))
            );
            assert_eq!(curves[0].1, ws.ui.title_dim);
            // The note moves down: the wire follows.
            ws.space.canvas.get_mut(note).unwrap().rect.y = 100.0;
            assert_eq!(ws.wire_curves()[0].0.to, Point::new(500.0, 200.0));
            ws.enter_grid(vec![kanban, note], window, cx);
            assert!(ws.wire_curves().is_empty(), "grid mode hides wires");
            assert_eq!(ws.space.wires.len(), 1, "and keeps them");
            ws.leave_grid(window, cx);
            assert_eq!(ws.wire_curves().len(), 1);
        });
        // A frame paints them.
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_wire_tool_wires_the_block_pressed_to_the_one_let_go_on(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    let (from, over, empty) = (
        at(&ws, cx, (150.0, 140.0)),
        at(&ws, cx, (600.0, 180.0)),
        at(&ws, cx, (700.0, 300.0)),
    );
    let event = |p: (f32, f32)| point(px(p.0), px(p.1));
    let plain = Modifiers::default();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("tool-wire", cx);
        window.render_frame(cx);
        assert_eq!(window.find("tool-wire").selected(), Some(true));
        // Let go on the empty canvas, or on the block it started on: nothing.
        press_with(window, cx, MouseButton::Left, plain, from, empty);
        press_with(window, cx, MouseButton::Left, plain, from, from);
        assert!(ws.read(cx).space.wires.is_empty());
        // Held over the note: the curve already runs to its facing edge.
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: event(from),
                modifiers: plain,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: event(over),
                pressed_button: Some(MouseButton::Left),
                modifiers: plain,
            }),
            cx,
        );
        let drawn = ws.read(cx).wire_curves();
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].0.to, Point::new(500.0, 180.0));
        assert_eq!(drawn[0].1, ws.read(cx).ui.accent);
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: event(over),
                modifiers: plain,
                click_count: 1,
            }),
            cx,
        );
        window.render_frame(cx);
        let w = ws.read(cx);
        assert_eq!(w.space.wires, [Wire::new(kanban, note).unwrap()]);
        assert_eq!(w.tool, Tool::Wire, "it stays on");
        assert_eq!(w.space.focused, None, "Ctrl+Z reaches the canvas");
        // Ctrl+Z takes it back; Esc puts Select back.
        window.press(UNDO, cx);
        assert!(ws.read(cx).space.wires.is_empty());
        window.press("escape", cx);
        assert_eq!(ws.read(cx).tool, Tool::Select);
    })
    .unwrap();
}

/// Esc while the Wire tool is held over a block gives the wire up: the curve goes, and letting
/// go there wires nothing.
#[gpui_kit::test]
fn esc_during_a_wire_tool_drag_gives_it_up(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    let (from, over) = (at(&ws, cx, (150.0, 140.0)), at(&ws, cx, (600.0, 180.0)));
    let event = |p: (f32, f32)| point(px(p.0), px(p.1));
    let plain = Modifiers::default();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("tool-wire", cx);
        window.render_frame(cx);
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: event(from),
                modifiers: plain,
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: event(over),
                pressed_button: Some(MouseButton::Left),
                modifiers: plain,
            }),
            cx,
        );
        window.press("escape", cx);
        assert_eq!(ws.read(cx).tool, Tool::Select);
        assert!(ws.read(cx).wire_curves().is_empty(), "the curve went");
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: event(over),
                modifiers: plain,
                click_count: 1,
            }),
            cx,
        );
    })
    .unwrap();
    assert!(
        wires(&ws, cx).is_empty(),
        "{kanban:?} and {note:?} aren't wired"
    );
}

/// The Wire tool let go over the toolbar, or outside the window, wires nothing, though a block
/// lies under the pointer there: no one sees it.
#[gpui_kit::test]
fn the_wire_tool_let_go_off_the_canvas_wires_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, _) = apart(handle, &ws, cx);
    let from = at(&ws, cx, (150.0, 140.0));
    let o = ws.read_with(cx, |ws, _| ws.origin);
    let toolbar = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("tool-wire", cx);
            window.render_frame(cx);
            let c = window.find("toolbar").bounds().center();
            (f32::from(c.x), f32::from(c.y))
        })
        .unwrap();
    // The camera is at 100% on the world's origin: canvas points are world points.
    let local = |p: (f32, f32)| (p.0 - f32::from(o.x), p.1 - f32::from(o.y));
    let under = local(toolbar);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(note).unwrap().rect =
            Rect::new(under.0 - 50.0, under.1 - 50.0, 220.0, 200.0);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            from,
            toolbar,
        );
    })
    .unwrap();
    assert!(wires(&ws, cx).is_empty(), "under the toolbar");
    // Above the window: the note reaches past its top.
    let outside = (f32::from(o.x) + 600.0, -20.0);
    let past = local(outside);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(note).unwrap().rect =
            Rect::new(past.0 - 50.0, past.1 - 150.0, 220.0, 200.0);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            from,
            outside,
        );
    })
    .unwrap();
    assert!(wires(&ws, cx).is_empty(), "outside the window");
}

/// Where the straight wire between the kanban block and the note runs (y 180, x 460 to 500), as
/// a window position.
fn on_the_wire(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> (f32, f32) {
    at(ws, cx, (480.0, 180.0))
}

/// The wire between two selected blocks runs inside their box: a click on it selects the wire.
#[gpui_kit::test]
fn a_click_on_a_wire_inside_the_selection_box_selects_the_wire(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    let wire = Wire::new(kanban, note).unwrap();
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
        ws.space.focused = None;
        ws.space.selected = vec![kanban, note];
    });
    let on = on_the_wire(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, on, 1);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.selected_wire, Some(wire));
        assert!(ws.space.selected.is_empty());
    });
}

#[gpui_kit::test]
fn a_click_selects_a_wire_and_delete_removes_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    let wire = Wire::new(kanban, note).unwrap();
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
    });
    let on = on_the_wire(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, on, 1);
        window.render_frame(cx);
        let w = ws.read(cx);
        assert_eq!(w.space.selected_wire, Some(wire));
        assert_eq!(w.space.focused, None, "Delete and Backspace go to the wire");
        assert_eq!(w.wire_curves()[0].1, w.ui.accent);
        window.press("escape", cx);
        assert_eq!(ws.read(cx).space.selected_wire, None, "Esc unselects");
        click(window, cx, on, 1);
        window.press("backspace", cx);
        assert!(ws.read(cx).space.wires.is_empty());
        assert_eq!(ws.read(cx).space.selected_wire, None);
        window.press(UNDO, cx);
        assert_eq!(ws.read(cx).space.wires, [wire], "Ctrl+Z brings it back");
        click(window, cx, on, 1);
        window.press("delete", cx);
        assert!(ws.read(cx).space.wires.is_empty(), "Delete too");
    })
    .unwrap();
}

/// Selecting blocks unselects the wire, so Delete closes the blocks: Shift+click on a title, or
/// Shift+drag around them.
#[gpui_kit::test]
fn selecting_blocks_lets_the_wire_go(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = apart(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
        // Straight down from the kanban block's bottom (y 320) to the terminal's top (y 400).
        ws.connect(kanban, terminal, cx);
    });
    let (on, below) = (on_the_wire(&ws, cx), at(&ws, cx, (245.0, 360.0)));
    let title = at(&ws, cx, (610.0, 80.0 + chrome::TITLE_H / 2.0));
    let (from, to) = (at(&ws, cx, (700.0, 330.0)), at(&ws, cx, (300.0, 250.0)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, on, 1);
        assert!(ws.read(cx).space.selected_wire.is_some());
        press_with(window, cx, MouseButton::Left, SHIFT, title, title);
        assert_eq!(ws.read(cx).space.selected, [note]);
        window.press("delete", cx);
        assert!(
            ws.read(cx).space.canvas.get(note).is_none(),
            "the note closed"
        );
        window.render_frame(cx);
        click(window, cx, below, 1);
        assert_eq!(
            ws.read(cx).space.selected_wire,
            Some(Wire::new(kanban, terminal).unwrap())
        );
        press_with(window, cx, MouseButton::Left, SHIFT, from, to);
        assert_eq!(ws.read(cx).space.selected, [kanban]);
        window.press("delete", cx);
        assert!(
            ws.read(cx).space.canvas.get(kanban).is_none(),
            "the kanban block closed"
        );
    })
    .unwrap();
}

/// A wire's step whose other block went (moved to another workspace) is skipped: one Ctrl+Z
/// takes back the step before it.
#[gpui_kit::test]
fn a_wire_step_to_a_block_gone_is_skipped(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.connect(note, terminal, cx);
            ws.connect(kanban, note, cx);
            ws.move_blocks(vec![kanban], None, window, cx);
            assert_eq!(ws.space.wires, [Wire::new(note, terminal).unwrap()]);
            assert!(ws.undo_canvas(false, window, cx));
            assert!(ws.space.wires.is_empty(), "the note–terminal wire's step");
        })
    })
    .unwrap();
}

/// Wires run through the gaps people grab to move the canvas: a drag that starts on one still
/// moves it, and like any pan it leaves the keyboard where it was and selects nothing.
#[gpui_kit::test]
fn a_drag_from_a_wire_moves_the_canvas_and_keeps_the_keyboard(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = apart(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
    });
    let on = on_the_wire(&ws, cx);
    let before = ws.read_with(cx, |ws, _| ws.space.canvas.camera.origin);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("body", terminal.0 as usize), cx);
        let to = (on.0 - 100.0, on.1);
        press_with(window, cx, MouseButton::Left, Modifiers::default(), on, to);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.canvas.camera.origin.x,
            before.x + 100.0,
            "it panned"
        );
        assert_eq!(ws.space.focused, Some(terminal), "typing still reaches it");
        assert_eq!(ws.space.selected_wire, None);
    });
}

/// Grid mode hides wires: entering it lets a selected one go, so no Delete removes a wire no one
/// sees.
#[gpui_kit::test]
fn entering_grid_mode_lets_the_wire_go(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
    });
    let on = on_the_wire(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, on, 1);
        assert!(ws.read(cx).space.selected_wire.is_some());
        ws.update(cx, |ws, cx| ws.enter_grid(vec![kanban, note], window, cx));
        assert_eq!(ws.read(cx).space.selected_wire, None);
    })
    .unwrap();
}

/// A wire selected in a workspace left isn't selected when the workspace comes back.
#[gpui_kit::test]
fn a_workspace_left_lets_the_wire_go(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
    });
    let on = on_the_wire(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, on, 1);
        assert!(ws.read(cx).space.selected_wire.is_some());
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            ws.switch_to(0, window, cx);
        });
        assert_eq!(ws.read(cx).space.selected_wire, None);
    })
    .unwrap();
}

/// A selected wire never takes a terminal's Backspace: a block that took the keyboard since
/// (clicked, or just added) drops the wire's selection, and with a block focused the key is
/// never the wire's.
#[gpui_kit::test]
fn a_terminal_with_the_keyboard_keeps_its_backspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = apart(handle, &ws, cx);
    let wire = Wire::new(kanban, note).unwrap();
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
    });
    let on = on_the_wire(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, on, 1);
        window.click(("body", terminal.0 as usize), cx);
        assert_eq!(ws.read(cx).space.focused, Some(terminal));
        assert_eq!(ws.read(cx).space.selected_wire, None);
        window.press("backspace", cx);
        click(window, cx, on, 1);
        window.press(NEW, cx);
        assert_eq!(ws.read(cx).space.selected_wire, None);
        window.press("backspace", cx);
        ws.update(cx, |ws, _| {
            ws.space.selected_wire = Some(wire);
            ws.space.focused = Some(terminal);
        });
        window.press("backspace", cx);
        assert_eq!(
            ws.read(cx).space.wires,
            [wire],
            "every Backspace was a terminal's"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_wire_under_a_drawing_or_a_block_isnt_the_one_clicked(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = apart(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
        // A line drawn across the wire: drawings are above wires.
        ws.space.ink.push(Drawing::Shape {
            kind: ShapeKind::Line,
            from: Point::new(480.0, 140.0),
            to: Point::new(480.0, 220.0),
            width: 4.0,
            color: Color::Red,
        });
    });
    let on = on_the_wire(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        click(window, cx, on, 1);
        assert_eq!(ws.read(cx).space.selected_ink, Some(0));
        assert_eq!(ws.read(cx).space.selected_wire, None);
        ws.update(cx, |ws, _| {
            ws.space.ink.clear();
            ws.space.selected_ink = None;
            // The terminal, behind the other two, now covers the gap the wire crosses.
            ws.space.canvas.get_mut(terminal).unwrap().rect = Rect::new(450.0, 100.0, 300.0, 200.0);
        });
        window.render_frame(cx);
        click(window, cx, on, 1);
        assert_eq!(
            ws.read(cx).space.focused,
            Some(terminal),
            "the block's click"
        );
        assert_eq!(ws.read(cx).space.selected_wire, None);
    })
    .unwrap();
}

#[gpui_kit::test]
fn right_clicking_a_wire_offers_disconnect(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
    });
    let on = on_the_wire(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        right_click(window, cx, on);
        window.render_frame(cx);
        assert!(ws.read(cx).menu.is_some(), "the wire's menu");
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        assert!(ws.read(cx).space.wires.is_empty());
    })
    .unwrap();
}

#[gpui_kit::test]
fn wires_come_back_after_a_restart(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let [k, n, t] = uids(&ws, cx, [kanban, note, terminal]);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
        ws.connect(note, terminal, cx);
        ws.save_canvas_now(cx);
    });
    let saved =
        std::fs::read_to_string(data.path().join("workspaces").join("default.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        saved["wires"],
        serde_json::json!([{ "a": k, "b": n }, { "a": n, "b": t }]),
        "by block id"
    );
    let (_, again) = open_in(cx, None, Some(data.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        let id = |uid: &str| ws.space.block_by_uid(uid).unwrap();
        assert_eq!(ws.space.wires.len(), 2);
        assert!(ws.space.wired(id(&k), id(&n)));
        assert!(ws.space.wired(id(&n), id(&t)));
    });
}

/// A file edited by hand, or one whose block didn't come back: a wire to a missing block, to the
/// block itself and a second one between the same two are dropped. "aaaaa" names two items: the
/// first keeps it (the second gets an id of its own), so its wires are the first one's.
#[gpui_kit::test]
fn a_saved_wire_to_a_missing_or_doubled_block_is_dropped(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let kanban = |x: f32, uid: &str| {
        format!(r#"{{"kind":"kanban","uid":"{uid}","rect":{{"x":{x},"y":0,"w":300,"h":200}}}}"#)
    };
    let wire = |a: &str, b: &str| format!(r#"{{"a":"{a}","b":"{b}"}}"#);
    write_canvas(
        data.path(),
        &format!(
            r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[{},{},{}],"wires":[{},{},{},{}]}}"#,
            kanban(0.0, "aaaaa"),
            kanban(400.0, "aaaaa"),
            kanban(800.0, "ccccc"),
            wire("aaaaa", "ccccc"),
            wire("aaaaa", "zzzzz"),
            wire("aaaaa", "aaaaa"),
            wire("ccccc", "aaaaa"),
        ),
    );
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.read_with(cx, |ws, _| {
        let blocks: Vec<BlockId> = ws.space.canvas.blocks().iter().map(|b| b.id).collect();
        assert_eq!(ws.space.wires, [Wire::new(blocks[0], blocks[2]).unwrap()]);
    });
}

#[gpui_kit::test]
fn templum_wire_wires_two_blocks_of_its_workspace_as_a_step(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let (from, _) = me(&ws, cx, terminal);
    let [k, n] = uids(&ws, cx, [kanban, note]);
    let (failed, text) = call(handle, &ws, cx, &from, "wire", json!({ "a": k, "b": n }));
    assert_eq!((failed, text), (false, format!("Connected {k} and {n}.")));
    assert!(ws.read_with(cx, |ws, _| ws.space.wired(kanban, note)));
    let (failed, text) = call(handle, &ws, cx, &from, "wire", json!({ "a": n, "b": k }));
    assert_eq!(
        (failed, text),
        (false, format!("{n} and {k} are already connected."))
    );
    let (failed, text) = call(handle, &ws, cx, &from, "wire", json!({ "a": k, "b": k }));
    assert_eq!(
        (failed, text),
        (true, format!("{k} can't be wired to itself."))
    );
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &from,
        "wire",
        json!({ "a": k, "b": "zzzzz" }),
    );
    assert_eq!(
        (failed, text),
        (
            true,
            "There's no block zzzzz in your workspace: templum blocks lists them.".to_string()
        )
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            assert!(ws.undo_canvas(false, window, cx), "a step of Ctrl+Z");
            assert!(!ws.space.wired(kanban, note));
        })
    })
    .unwrap();
    // An agent's MCP server never gets it: the command line's alone.
    let answers = call_later(handle, &ws, cx, &from, 1, "wire", json!({ "a": k, "b": n }));
    let line = answers.try_recv().unwrap();
    assert!(line.contains("Unknown tool: wire"), "{line}");
}

#[gpui_kit::test]
fn rename_wire_labels_it_and_ctrl_z_takes_the_label_back(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
    });
    let wire = Wire::new(kanban, note).unwrap();
    let on = on_the_wire(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        right_click(window, cx, on);
        window.render_frame(cx);
        window.within("popup-menu").click(0usize, cx);
        window.render_frame(cx);
        assert!(
            window.has_active_dialog(cx),
            "Rename wire… asks for the label"
        );
        window.input("dev server", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            assert_eq!(ws.space.label(wire), Some("dev server"));
            assert!(ws.undo_canvas(false, window, cx));
            assert!(
                ws.space.wired(kanban, note),
                "Ctrl+Z takes the label, not the wire"
            );
            assert_eq!(ws.space.label(wire), None);
            assert!(ws.undo_canvas(true, window, cx));
            assert_eq!(ws.space.label(wire), Some("dev server"));
        })
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_label_is_one_line_of_60_at_most(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let wire = Wire::new(kanban, note).unwrap();
    ws.update(cx, |ws, cx| {
        assert!(
            !ws.rename_wire(wire, "api", cx),
            "not wired: nothing to name"
        );
        ws.connect(kanban, note, cx);
        assert!(ws.rename_wire(wire, "  dev \t server\n", cx));
        assert_eq!(ws.space.label(wire), Some("dev server"));
        assert_eq!(
            ws.space.label(Wire::new(note, kanban).unwrap()),
            Some("dev server"),
            "either way round"
        );
        assert!(
            !ws.rename_wire(wire, "dev server", cx),
            "the same label is no step"
        );
        assert!(ws.rename_wire(wire, &"x".repeat(200), cx));
        assert_eq!(ws.space.label(wire).map(|l| l.chars().count()), Some(60));
        assert!(ws.rename_wire(wire, "   ", cx), "blank: no label");
        assert_eq!(ws.space.label(wire), None);
        assert_eq!(ws.space.label(Wire::new(kanban, terminal).unwrap()), None);
    });
}

#[gpui_kit::test]
fn a_label_comes_back_with_its_wire(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let [kanban_uid] = uids(&ws, cx, [kanban]);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.connect(kanban, note, cx);
            let wire = Wire::new(kanban, note).unwrap();
            ws.rename_wire(wire, "board", cx);
            ws.disconnect(wire, cx);
            assert!(ws.undo_canvas(false, window, cx));
            assert_eq!(
                ws.space.label(wire),
                Some("board"),
                "Ctrl+Z of the disconnect"
            );
            ws.close_now(&[kanban], Then::New, window, cx);
            assert!(ws.undo_canvas(false, window, cx));
            let back = ws.space.block_by_uid(&kanban_uid).unwrap();
            assert_eq!(
                ws.space.label(Wire::new(back, note).unwrap()),
                Some("board"),
                "by its blocks' ids, through a close"
            );
        })
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_label_comes_back_after_a_restart(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let [k, n, t] = uids(&ws, cx, [kanban, note, terminal]);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
        ws.connect(note, terminal, cx);
        ws.rename_wire(Wire::new(note, terminal).unwrap(), "dev server", cx);
        ws.save_canvas_now(cx);
    });
    let saved =
        std::fs::read_to_string(data.path().join("workspaces").join("default.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(
        saved["wires"],
        json!([{ "a": k, "b": n }, { "a": n, "b": t, "label": "dev server" }])
    );
    let (_, again) = open_in(cx, None, Some(data.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        let id = |uid: &str| ws.space.block_by_uid(uid).unwrap();
        assert_eq!(
            ws.space.label(Wire::new(id(&n), id(&t)).unwrap()),
            Some("dev server")
        );
        assert_eq!(ws.space.label(Wire::new(id(&k), id(&n)).unwrap()), None);
    });
}

/// A file edited by hand: a label is trimmed, a blank one is none, and one on a wire that is
/// dropped (its block is missing) stays out.
#[gpui_kit::test]
fn a_saved_label_needs_its_wire_and_some_text(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let kanban = |x: f32, uid: &str| {
        format!(r#"{{"kind":"kanban","uid":"{uid}","rect":{{"x":{x},"y":0,"w":300,"h":200}}}}"#)
    };
    write_canvas(
        data.path(),
        &format!(
            r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[{},{},{}],"wires":[{{"a":"aaaaa","b":"bbbbb","label":"  api  "}},{{"a":"bbbbb","b":"ccccc","label":"   "}},{{"a":"aaaaa","b":"zzzzz","label":"gone"}}]}}"#,
            kanban(0.0, "aaaaa"),
            kanban(400.0, "bbbbb"),
            kanban(800.0, "ccccc"),
        ),
    );
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.read_with(cx, |ws, _| {
        let id = |uid: &str| ws.space.block_by_uid(uid).unwrap();
        assert_eq!(
            ws.space.label(Wire::new(id("aaaaa"), id("bbbbb")).unwrap()),
            Some("api")
        );
        assert_eq!(
            ws.space.label(Wire::new(id("bbbbb"), id("ccccc")).unwrap()),
            None
        );
        assert_eq!(
            ws.space.labels.len(),
            1,
            "a label on a dropped wire stays out"
        );
    });
}

#[gpui_kit::test]
fn a_label_sits_at_the_wires_middle_down_to_25_percent_and_grid_mode_hides_it(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = apart(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.connect(kanban, note, cx);
        ws.rename_wire(Wire::new(kanban, note).unwrap(), "board", cx);
        let labels = ws.wire_labels();
        assert_eq!(labels.len(), 1);
        assert_eq!(
            (labels[0].0, labels[0].1.as_str(), labels[0].2),
            (Point::new(480.0, 180.0), "board", ws.ui.title_dim)
        );
    });
    for zoom in [1.0, 0.5, 0.25] {
        ws.update(cx, |ws, _| {
            ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), zoom);
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(
                window.try_find(("wire-label", 0usize)).is_some(),
                "at {zoom}"
            );
        })
        .unwrap();
    }
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.enter_grid(vec![kanban, note], window, cx));
        window.render_frame(cx);
        assert!(
            window.try_find(("wire-label", 0usize)).is_none(),
            "grid mode hides it"
        );
    })
    .unwrap();
}
