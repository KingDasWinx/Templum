//! Grid mode: blocks tiled over the workspace at zoom 1, and back to the canvas as they were.

use super::*;
use crate::workspace::history::{Change, Then};
use templum_canvas::GAP;

#[cfg(target_os = "macos")]
pub(super) const TOGGLE: &str = "cmd-shift-enter";
#[cfg(not(target_os = "macos"))]
pub(super) const TOGGLE: &str = "ctrl-shift-enter";

fn press(handle: AnyWindowHandle, key: &str, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// What the tiles fill: the workspace's area inside the gap.
fn area(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Rect {
    cx.update_window(handle, |_, window, cx| {
        let vp = ws.read(cx).viewport(window);
        Rect::new(GAP, GAP, vp.w - 2.0 * GAP, vp.h - 2.0 * GAP)
    })
    .unwrap()
}

fn rect_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Rect {
    ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect)
}

fn grid_size(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> (u16, u16) {
    ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.grid_size())
}

/// Away from the startup centering, at 50%: grid mode's zoom 1 is another camera.
fn far_camera(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Camera {
    let camera = Camera::at(Point::new(-300.0, -200.0), 0.5);
    ws.update(cx, |ws, _| {
        ws.space.pristine = false;
        ws.space.canvas.camera = camera;
    });
    camera
}

#[gpui_kit::test]
fn ctrl_shift_enter_fills_the_workspace_and_gives_the_block_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let camera = far_camera(&ws, cx);
    frame(handle, cx);
    let (rect, cells) = (rect_of(&ws, cx, id), grid_size(&ws, cx, id));
    press(handle, TOGGLE, cx);
    assert_eq!(rect_of(&ws, cx, id), area(handle, &ws, cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.canvas.camera, Camera::default(), "zoom 1");
        assert_eq!(ws.space.camera(), camera, "the canvas's, kept");
        assert_eq!(ws.space.focused, Some(id));
    });
    let tiled = grid_size(&ws, cx, id);
    assert!(
        tiled.0 > cells.0 && tiled.1 > cells.1,
        "{tiled:?} after {cells:?}"
    );
    press(handle, TOGGLE, cx);
    assert_eq!(rect_of(&ws, cx, id), rect);
    assert_eq!(grid_size(&ws, cx, id), cells);
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.grid.is_none());
        assert_eq!(ws.space.canvas.camera, camera);
    });
}

#[gpui_kit::test]
fn a_window_resize_tiles_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    press(handle, TOGGLE, cx);
    let wide = grid_size(&ws, cx, id);
    cx.simulate_window_resize(handle, size(px(900.), px(600.)));
    frame(handle, cx);
    assert_eq!(rect_of(&ws, cx, id), area(handle, &ws, cx));
    assert!(grid_size(&ws, cx, id).0 < wide.0, "fewer columns");
}

#[gpui_kit::test]
fn the_selection_fills_the_workspace_left_to_right(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![note, kanban]);
    press(handle, TOGGLE, cx);
    let area = area(handle, &ws, cx);
    let half = ((area.w - GAP) / 2.0).floor();
    assert_eq!(
        rect_of(&ws, cx, kanban),
        Rect::new(area.x, area.y, half, area.h),
        "the left one first"
    );
    assert_eq!(rect_of(&ws, cx, note).right(), area.right());
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find(("block", terminal.0 as usize)).is_none(),
            "hidden"
        );
        window.find(("block", kanban.0 as usize));
        assert!(window.try_find("toolbar").is_none());
    })
    .unwrap();
    assert_eq!(
        rect_of(&ws, cx, terminal),
        Rect::new(40.0, 400.0, 400.0, 300.0),
        "untouched on the canvas"
    );
}

#[gpui_kit::test]
fn the_camera_stays_put_in_grid_mode(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    press(handle, TOGGLE, cx);
    let tile = rect_of(&ws, cx, id);
    let at = (tile.x + 20.0, tile.y + 4.0);
    let at = {
        let o = ws.read_with(cx, |ws, _| ws.origin);
        (f32::from(o.x) + at.0, f32::from(o.y) + at.1)
    };
    cx.update_window(handle, |_, window, cx| {
        wheel(
            window,
            cx,
            at,
            ScrollDelta::Pixels(point(px(40.), px(40.))),
            false,
        );
        wheel(
            window,
            cx,
            at,
            ScrollDelta::Pixels(point(px(0.), px(200.))),
            true,
        );
        press_with(
            window,
            cx,
            MouseButton::Middle,
            Modifiers::default(),
            at,
            (at.0 + 90.0, at.1 + 60.0),
        );
        window.press(ZOOM_IN, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.canvas.camera, Camera::default());
        assert!(ws.space.flight.is_none());
    });
    assert_eq!(rect_of(&ws, cx, id), tile);
}

#[gpui_kit::test]
fn a_tile_scrolls_with_the_wheel_even_when_not_selected(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    press(handle, TOGGLE, cx);
    ws.update(cx, |ws, _| ws.space.focused = None);
    let tile = rect_of(&ws, cx, id);
    let at = at(&ws, cx, (tile.x + tile.w / 2.0, tile.y + tile.h / 2.0));
    cx.update_window(handle, |_, window, cx| {
        wheel(
            window,
            cx,
            at,
            ScrollDelta::Pixels(point(px(0.), px(60.))),
            false,
        )
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(id).unwrap().scrolled.get());
    });
}

#[gpui_kit::test]
fn a_snapshot_in_grid_mode_keeps_the_canvas(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let camera = far_camera(&ws, cx);
    let before: Vec<Rect> = ws.read_with(cx, |ws, _| {
        ws.space.canvas.blocks().iter().map(|b| b.rect).collect()
    });
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(handle, TOGGLE, cx);
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let crate::session::Loaded::Saved(saved) =
        crate::session::load(dir.path(), crate::session::DEFAULT_ID)
    else {
        panic!("not saved");
    };
    let rects: Vec<Rect> = saved
        .items
        .iter()
        .map(|item| match item {
            crate::session::Item::Terminal(t) => t.rect.into(),
            crate::session::Item::Kanban(k) => k.rect.into(),
            crate::session::Item::Note(n) => n.rect.into(),
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(rects, before, "the canvas rects, not the tiles");
    assert_eq!(
        (saved.camera.x, saved.camera.y),
        (camera.origin.x, camera.origin.y)
    );
    assert_eq!(saved.camera.zoom, camera.zoom());
}

#[gpui_kit::test]
fn closing_a_tile_gives_its_place_and_the_last_one_leaves(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let camera = far_camera(&ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(handle, TOGGLE, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(note, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        rect_of(&ws, cx, kanban),
        area(handle, &ws, cx),
        "the whole area"
    );
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, None));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(kanban, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.grid.is_none(), "the canvas again");
        assert_eq!(ws.space.canvas.camera, camera);
        assert!(ws.space.shown(terminal));
    });
}

/// Ctrl+Z in grid mode moves a tile's place on the canvas, not its tile.
#[gpui_kit::test]
fn ctrl_z_in_grid_mode_moves_the_block_on_the_canvas(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, _) = three_blocks(handle, &ws, cx);
    let was = Rect::new(1.0, 2.0, 220.0, 200.0);
    ws.update(cx, |ws, _| {
        ws.keep(Change::Place(vec![(kanban, was)]), Then::New);
        ws.space.focused = Some(kanban);
    });
    press(handle, TOGGLE, cx);
    let tile = rect_of(&ws, cx, kanban);
    press(handle, UNDO, cx);
    assert_eq!(rect_of(&ws, cx, kanban), tile, "the tile stays");
    press(handle, TOGGLE, cx);
    assert_eq!(rect_of(&ws, cx, kanban), was);
}

#[test]
fn the_grid_key_is_ctrl_shift_enter() {
    let keymap = crate::keys::Keymap::new(&Default::default(), cfg!(target_os = "macos"))
        .unwrap()
        .0;
    let typed = gpui_kit::Keystroke::parse(TOGGLE).unwrap();
    assert_eq!(
        keymap.command(&typed),
        Some(crate::keys::Command::ToggleGrid)
    );
}

/// The blocks added since `before`, oldest first.
fn added_since(ws: &Entity<Workspace>, cx: &mut TestAppContext, before: usize) -> Vec<BlockId> {
    ws.read_with(cx, |ws, _| {
        let mut ids: Vec<BlockId> = ws.space.canvas.blocks().iter().map(|b| b.id).collect();
        ids.sort();
        ids[before..].to_vec()
    })
}

#[gpui_kit::test]
fn a_new_block_takes_half_of_the_focused_tile(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    press(handle, TOGGLE, cx);
    press(handle, NEW, cx);
    let [second] = added_since(&ws, cx, 1)[..] else {
        panic!("one new terminal");
    };
    let area = area(handle, &ws, cx);
    let (a, b) = (rect_of(&ws, cx, first), rect_of(&ws, cx, second));
    assert_eq!((a.x, a.y, a.h), (area.x, area.y, area.h), "side by side");
    assert_eq!((b.right(), b.h), (area.right(), area.h));
    // The new one has the keyboard, and is tall: the next one goes below it.
    press(handle, NEW, cx);
    let third = added_since(&ws, cx, 2)[0];
    let (b, c) = (rect_of(&ws, cx, second), rect_of(&ws, cx, third));
    assert_eq!((b.x, b.w), (c.x, c.w), "one above the other");
    assert!(c.y > b.bottom());
    assert_eq!(c.bottom(), area.bottom());
}

/// New blocks get places on the canvas where the view was, off each other's: in the middle of
/// the view the canvas had, else beside what is there, level with it.
#[gpui_kit::test]
fn new_blocks_land_apart_on_the_canvas_in_the_view_left(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let camera = far_camera(&ws, cx);
    press(handle, TOGGLE, cx);
    press(handle, NEW, cx);
    press(handle, NEW, cx);
    let [second, third] = added_since(&ws, cx, 1)[..] else {
        panic!("two new terminals");
    };
    press(handle, TOGGLE, cx);
    let (a, b, c) = (
        rect_of(&ws, cx, first),
        rect_of(&ws, cx, second),
        rect_of(&ws, cx, third),
    );
    let center = cx
        .update_window(handle, |_, window, cx| {
            let ws = ws.read(cx);
            camera.to_world(ws.center(window))
        })
        .unwrap();
    let at = Rect::new(center.x - b.w / 2.0, center.y - b.h / 2.0, b.w, b.h);
    assert_eq!(
        b,
        templum_canvas::room_at(&[a], at),
        "{b:?} around {center:?}"
    );
    assert_eq!(c, templum_canvas::room_at(&[a, b], at));
    assert!(!b.intersects(&c) && !a.intersects(&b) && !a.intersects(&c));
}

#[gpui_kit::test]
fn a_terminal_beside_a_tile_lands_beside_its_place_on_the_canvas(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open(cx);
    // The startup terminal out of the editor's way: nothing on its right.
    let startup = first_id(&ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(startup).unwrap().rect.x += 5000.0;
    });
    let editor = open_editor(handle, &ws, cx, dir.path());
    let home = rect_of(&ws, cx, editor);
    ws.update(cx, |ws, _| ws.space.focused = Some(editor));
    press(handle, TOGGLE, cx);
    let terminal = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.terminal_beside(Some(editor), dir.path(), None, window, cx)
            })
        })
        .unwrap()
        .unwrap();
    ws.read_with(cx, |ws, _| assert!(ws.space.shown(terminal), "tiled"));
    press(handle, TOGGLE, cx);
    let r = rect_of(&ws, cx, terminal);
    assert_eq!((r.x, r.y), (home.right() + templum_canvas::BESIDE, home.y));
}

#[gpui_kit::test]
fn going_to_a_block_on_the_canvas_brings_it_into_the_grid(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, terminal) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.space.focused = Some(kanban));
    press(handle, TOGGLE, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let space = ws.space.id.clone();
            ws.reveal(&space, terminal, window, cx);
        })
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.shown(terminal));
        assert_eq!(ws.space.focused, Some(terminal));
    });
    let area = area(handle, &ws, cx);
    assert_eq!(
        rect_of(&ws, cx, terminal).right(),
        area.right(),
        "beside it"
    );
    press(handle, TOGGLE, cx);
    assert_eq!(
        rect_of(&ws, cx, terminal),
        Rect::new(40.0, 400.0, 400.0, 300.0),
        "where it was"
    );
}

#[gpui_kit::test]
fn ctrl_z_brings_a_closed_tile_back_into_the_grid(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(handle, TOGGLE, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(note, window, cx))
    })
    .unwrap();
    press(handle, UNDO, cx);
    ws.read_with(cx, |ws, _| {
        let tiles = ws.space.grid.as_ref().unwrap().tiles.ids();
        assert_eq!(tiles.len(), 2, "back in the grid: {tiles:?}");
        assert!(tiles.contains(&kanban));
    });
}

/// The button beside a block's close button: full screen, with the selection it is in; again,
/// back to the canvas.
#[gpui_kit::test]
fn the_fullscreen_button_takes_the_selection_and_comes_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    // The note lies over the kanban block's right end: its button is the one in reach.
    let n = note.0 as usize;
    cx.update_window(handle, |_, window, cx| {
        window.hover(("block", n), cx);
        window.render_frame(cx);
        window.click(("fullscreen", n), cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let tiles = ws.space.grid.as_ref().expect("grid mode").tiles.ids();
        assert_eq!(tiles, [kanban, note]);
        assert!(!ws.space.shown(terminal));
    });
    cx.update_window(handle, |_, window, cx| {
        window.hover(("block", n), cx);
        window.render_frame(cx);
        window.click(("fullscreen", n), cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert!(ws.space.grid.is_none()));
}

/// Ctrl+Alt+arrows move the keyboard to the tile on that side; with Shift, the tile trades
/// places with it.
#[gpui_kit::test]
fn ctrl_alt_arrows_move_the_focus_and_with_shift_swap(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(handle, TOGGLE, cx);
    let (left, right) = (rect_of(&ws, cx, kanban), rect_of(&ws, cx, note));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(note), "it kept the keyboard")
    });
    press(handle, "ctrl-alt-left", cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(kanban)));
    press(handle, "ctrl-alt-left", cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(kanban), "nothing there")
    });
    press(handle, "ctrl-alt-shift-right", cx);
    assert_eq!(rect_of(&ws, cx, kanban), right);
    assert_eq!(rect_of(&ws, cx, note), left);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(kanban), "it moved along")
    });
}

/// A tile's title dropped on another tile: the two trade places, and keep theirs on the canvas.
#[gpui_kit::test]
fn a_title_dropped_on_another_tile_swaps_them(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let homes = (rect_of(&ws, cx, kanban), rect_of(&ws, cx, note));
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(handle, TOGGLE, cx);
    let (left, right) = (rect_of(&ws, cx, kanban), rect_of(&ws, cx, note));
    let title = at(
        &ws,
        cx,
        (left.x + left.w / 2.0, left.y + chrome::TITLE_H / 2.0),
    );
    let target = at(&ws, cx, (right.x + right.w / 2.0, right.y + right.h / 2.0));
    let position = |p: (f32, f32)| point(px(p.0), px(p.1));
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position: position(title),
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
        window.dispatch_event(
            PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: position(target),
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.dropping_on(note), "the target is marked");
        assert!(ws.moving(kanban));
    });
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position: position(target),
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(rect_of(&ws, cx, kanban), right);
    assert_eq!(rect_of(&ws, cx, note), left);
    press(handle, TOGGLE, cx);
    assert_eq!((rect_of(&ws, cx, kanban), rect_of(&ws, cx, note)), homes);
}

/// Right-clicks block `id`'s title and picks its menu's item `index` (separators count), then
/// the submenu's `sub`, if any.
fn pick(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    id: BlockId,
    index: usize,
    sub: Option<usize>,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.right_click(("title", id.0 as usize), cx);
        window.render_frame(cx);
        window.within("popup-menu").click(index, cx);
        window.render_frame(cx);
        if let Some(sub) = sub {
            window.within("submenu").within("popup-menu").click(sub, cx);
            window.render_frame(cx);
        }
    })
    .unwrap();
}

/// A selected block's Fullscreen takes the selection along; a tile's menu flips its split, adds
/// a block from the canvas, sends one back, and ends grid mode.
#[gpui_kit::test]
fn a_blocks_menu_goes_in_flips_adds_leaves_and_comes_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, terminal]);
    // Copy, Paste, —, Fullscreen, Connect to ▸, Move to workspace ▸, Close terminal.
    pick(handle, cx, terminal, 3, None);
    let tiles = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.grid.as_ref().map(|g| g.tiles.ids()))
    };
    assert_eq!(
        tiles(cx),
        Some(vec![kanban, terminal]),
        "the selection went in"
    );
    // Copy, Paste, —, AI agent ▸, Add ▸, From the canvas ▸, —, Flip the split, Leave the grid,
    // Back to the canvas, Close terminal.
    pick(handle, cx, terminal, 7, None);
    let (k, t) = (rect_of(&ws, cx, kanban), rect_of(&ws, cx, terminal));
    assert!(
        t.y > k.bottom() && t.x == k.x,
        "one above the other: {k:?} {t:?}"
    );
    pick(handle, cx, terminal, 5, Some(0));
    assert_eq!(tiles(cx).map(|t| t.len()), Some(3));
    assert!(
        ws.read_with(cx, |ws, _| ws.space.shown(note)),
        "the note came in"
    );
    // Nothing left on the canvas: no From the canvas ▸.
    pick(handle, cx, terminal, 7, None);
    ws.read_with(cx, |ws, _| {
        assert!(!ws.space.shown(terminal), "back on the canvas");
        assert!(ws.space.focused.is_some_and(|f| ws.space.shown(f)));
    });
    assert_eq!(
        rect_of(&ws, cx, terminal),
        Rect::new(40.0, 400.0, 400.0, 300.0)
    );
    // All boards…, —, AI agent ▸, Add ▸, From the canvas ▸, —, Flip the split, Leave the grid,
    // Back to the canvas…
    pick(handle, cx, kanban, 8, None);
    assert_eq!(tiles(cx), None, "the canvas again");
}

/// Quit in grid mode and start again: the grid is back, and leaving it puts every block where it
/// was on the canvas, under the camera as it was.
#[gpui_kit::test]
fn a_restart_brings_the_grid_back_with_the_canvas_intact(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let camera = far_camera(&ws, cx);
    let before: Vec<Rect> = ws.read_with(cx, |ws, _| {
        ws.space.canvas.blocks().iter().map(|b| b.rect).collect()
    });
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(handle, TOGGLE, cx);
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let (again_handle, again) = open_in(cx, None, Some(dir.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        let grid = ws.space.grid.as_ref().expect("grid mode again");
        assert_eq!(grid.tiles.ids().len(), 2);
        assert_eq!(grid.camera, camera);
        assert!(ws.space.focused.is_some_and(|f| ws.space.shown(f)));
        let homes: Vec<Rect> = ws
            .space
            .canvas
            .blocks()
            .iter()
            .map(|b| ws.space.home(b))
            .collect();
        assert_eq!(homes, before);
    });
    press(again_handle, TOGGLE, cx);
    again.read_with(cx, |ws, _| {
        let rects: Vec<Rect> = ws.space.canvas.blocks().iter().map(|b| b.rect).collect();
        assert_eq!(rects, before);
        assert_eq!(ws.space.canvas.camera, camera);
    });
}

/// In grid mode the keyboard follows the pointer, as Hyprland's focus does: the tile under it
/// takes it once it moves; a tile coming under a still pointer doesn't. The canvas's setting
/// turned off doesn't stop it.
#[gpui_kit::test]
fn grid_modes_focus_follows_the_pointer(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    let focused = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.space.focused);
    ws.update(cx, |ws, _| {
        ws.config.canvas.focus_follows_pointer = false;
        ws.space.selected = vec![kanban, note];
    });
    press(handle, TOGGLE, cx);
    let left = rect_of(&ws, cx, kanban);
    let over_left = at(&ws, cx, (left.x + 50.0, left.y + 100.0));
    point_at(handle, cx, over_left);
    assert_eq!(focused(cx), Some(kanban), "no click needed");
    // The kanban block trades places with the note: the note comes under the still pointer.
    press(handle, "ctrl-alt-shift-right", cx);
    assert_eq!(rect_of(&ws, cx, note), left);
    // The two slide there: the note arrives under the pointer, which hasn't moved.
    std::thread::sleep(crate::motion::TILE + std::time::Duration::from_millis(50));
    frame(handle, cx);
    frame(handle, cx);
    assert_eq!(focused(cx), Some(kanban), "it stays with the block moved");
    point_at(handle, cx, (over_left.0 + 5.0, over_left.1));
    assert_eq!(focused(cx), Some(note), "the pointer moved");
}

/// Dragging a tile's title shows what letting go does: the tile picked up, its place dashed, the
/// tile under the pointer saying it swaps, the title following the pointer. A press alone shows
/// none of it. Let go, the two slide to their new places.
#[gpui_kit::test]
fn dragging_a_title_shows_the_swap_and_the_tiles_slide(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(handle, TOGGLE, cx);
    let (left, right) = (rect_of(&ws, cx, kanban), rect_of(&ws, cx, note));
    let title = at(
        &ws,
        cx,
        (left.x + left.w / 2.0, left.y + chrome::TITLE_H / 2.0),
    );
    let target = at(&ws, cx, (right.x + right.w / 2.0, right.y + right.h / 2.0));
    let event = |kind: &str, p: (f32, f32)| {
        let position = point(px(p.0), px(p.1));
        match kind {
            "down" => PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            "move" => PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position,
                pressed_button: Some(MouseButton::Left),
                modifiers: Modifiers::default(),
            }),
            _ => PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
        }
    };
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(event("down", title), cx);
        window.dispatch_event(event("move", (title.0 + 2.0, title.1)), cx);
        window.render_frame(cx);
        assert!(
            window.try_find("swap-chip").is_none(),
            "a click isn't a drag"
        );
        window.dispatch_event(event("move", target), cx);
        window.render_frame(cx);
        window.find("swap-chip");
        window.find("swap-from");
        window.find("swap-target");
    })
    .unwrap();
    assert!(ws.read_with(cx, |ws, _| ws.held(kanban)));
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(event("up", target), cx);
        window.render_frame(cx);
        assert!(window.try_find("swap-chip").is_none());
    })
    .unwrap();
    // On its way: drawn short of its new place, which is its rect already.
    let drawn = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, _| {
            f32::from(window.find(("block", kanban.0 as usize)).bounds().origin.x)
        })
        .unwrap()
    };
    let origin = ws.read_with(cx, |ws, _| f32::from(ws.origin.x));
    assert_eq!(rect_of(&ws, cx, kanban), right);
    assert!(drawn(cx) < origin + right.x - 1.0, "still sliding");
    std::thread::sleep(crate::motion::TILE + std::time::Duration::from_millis(50));
    frame(handle, cx);
    assert!((drawn(cx) - (origin + right.x)).abs() < 1.0, "arrived");
}

/// In grid mode, Add ▸ in a tile's menu opens a block that takes half of that tile; the gaps'
/// menu adds blocks too.
#[gpui_kit::test]
fn the_grids_menus_add_blocks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, _) = three_blocks(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![kanban, note]);
    press(handle, TOGGLE, cx);
    let half = rect_of(&ws, cx, kanban);
    let count = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len());
    // All boards…, —, AI agent ▸, Add ▸: Terminal ▸ Default.
    let before = count(cx);
    pick(handle, cx, kanban, 3, Some(0));
    cx.update_window(handle, |_, window, cx| {
        click_innermost_menu_item(window, cx, 0);
        window.render_frame(cx);
    })
    .unwrap();
    let terminal = added_since(&ws, cx, before)[0];
    let r = rect_of(&ws, cx, terminal);
    assert!(
        ws.read_with(cx, |ws, _| ws.space.shown(terminal)),
        "it came into the grid"
    );
    assert!(
        r.x >= half.x && r.right() <= half.right() && r.y >= half.y && r.bottom() <= half.bottom(),
        "in the kanban block's half: {r:?} in {half:?}"
    );
    // The left gap's menu: AI agent ▸, Add ▸: Terminal, Kanban board, Note.
    let gap = at(&ws, cx, (GAP / 2.0, 300.0));
    let before = count(cx);
    cx.update_window(handle, |_, window, cx| {
        right_click(window, cx, gap);
        window.render_frame(cx);
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        window
            .within("submenu")
            .within("popup-menu")
            .click(2usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    let added = added_since(&ws, cx, before)[0];
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.blocks[&added].note().is_some());
        assert!(ws.space.shown(added));
        assert_eq!(ws.space.grid.as_ref().unwrap().tiles.ids().len(), 4);
    });
}

// On a mirror: grid mode is the mirror's own view of home's blocks.

/// Home's terminals `a`, `b` and `c` with `a` at `rect`, or `b` left out.
fn home_s(a: Rect, with_b: bool) -> templum_api::model::State {
    let mut state = super::remote::terminals_of_home(&["a", "b", "c"]);
    let r = &mut std::sync::Arc::make_mut(&mut state.blocks[0]).rect;
    (r.x, r.y, r.w, r.h) = (a.x, a.y, a.w, a.h);
    if !with_b {
        state.blocks.remove(1);
    }
    state
}

/// On a mirror the shortcut, the block's menu and its button go full screen and back, as on a
/// local workspace: no call of home's.
#[gpui_kit::test]
fn a_mirror_goes_full_screen_by_the_shortcut_the_menu_and_the_button(cx: &mut TestAppContext) {
    use super::remote::{live_terminals, sent, viewer};
    let (handle, ws, _dir) = live_terminals(cx, &["a", "b", "c"]);
    let [a, b, c] = ["a", "b", "c"].map(|h| viewer(&ws, cx, h));
    let tiles = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.grid.as_ref().map(|g| g.tiles.ids()))
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(a, window, cx))
    })
    .unwrap();
    press(handle, TOGGLE, cx);
    assert_eq!(tiles(cx), Some(vec![a]), "the shortcut");
    press(handle, TOGGLE, cx);
    assert_eq!(tiles(cx), None);
    // Copy, Paste, —, Fullscreen.
    pick(handle, cx, b, 3, None);
    assert_eq!(tiles(cx), Some(vec![b]), "the menu's Fullscreen");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.leave_grid(window, cx))
    })
    .unwrap();
    let n = c.0 as usize;
    cx.update_window(handle, |_, window, cx| {
        window.hover(("block", n), cx);
        window.render_frame(cx);
        window.click(("fullscreen", n), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(tiles(cx), Some(vec![c]), "the button");
    assert!(sent(&ws, cx).is_empty(), "nothing asked of home");
}

/// Home's state keeps a mirror's tiles where they are: a tile home moves goes back to home's
/// place when the grid ends, one home closes gives its place to its neighbour, and the last one
/// going brings the canvas back under its camera.
#[gpui_kit::test]
fn home_s_moves_and_closes_keep_a_mirror_s_grid(cx: &mut TestAppContext) {
    use super::remote::{apply, live_terminals, viewer};
    let (handle, ws, _dir) = live_terminals(cx, &["a", "b", "c"]);
    let [a, b, c] = ["a", "b", "c"].map(|h| viewer(&ws, cx, h));
    let camera = far_camera(&ws, cx);
    ws.update(cx, |ws, _| ws.space.selected = vec![a, b]);
    press(handle, TOGGLE, cx);
    let tile = rect_of(&ws, cx, a);
    // Home moves a: its tile stays, its place on the canvas is home's.
    let moved = Rect::new(0.0, 500.0, 400.0, 300.0);
    apply(handle, &ws, cx, home_s(moved, true));
    assert_eq!(rect_of(&ws, cx, a), tile, "a tile is the grid's");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.grid.as_ref().unwrap().homes[&a], moved);
        assert!(!ws.space.shown(c));
    });
    // Home closes b: a fills the grid.
    apply(handle, &ws, cx, home_s(moved, false));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.grid.as_ref().unwrap().tiles.ids(), [a]);
    });
    press(handle, TOGGLE, cx);
    assert_eq!(rect_of(&ws, cx, a), moved, "back where home has it");
    // The last tile closed at home: the canvas again, under its camera.
    ws.update(cx, |ws, _| ws.space.selected = vec![c]);
    press(handle, TOGGLE, cx);
    let mut state = home_s(moved, false);
    state.blocks.pop();
    apply(handle, &ws, cx, state);
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.grid.is_none());
        assert_eq!(ws.space.canvas.camera, camera);
        assert_eq!(ws.space.focused, Some(a));
    });
}

/// A tile's menu on a mirror adds home's blocks, as the canvas's does: Add ▸ Terminal ▸ Default
/// asks home for its default shell, and the terminal home adds takes half of the tile with the keyboard.
/// The index keeps the canvas's camera, not grid mode's.
#[gpui_kit::test]
fn a_mirror_s_tile_adds_home_s_terminal_and_keeps_the_canvas_camera(cx: &mut TestAppContext) {
    use super::remote::{answer, apply, live_terminals, sent, viewer};
    let (handle, ws, _dir) = live_terminals(cx, &["a"]);
    let a = viewer(&ws, cx, "a");
    let camera = far_camera(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(a, window, cx))
    })
    .unwrap();
    press(handle, TOGGLE, cx);
    ws.read_with(cx, |ws, _| {
        let kept = ws.index().workspaces.into_iter().find(|w| w.id == "r1");
        let kept = kept.unwrap().remote.unwrap().camera.unwrap();
        assert_eq!((kept.x, kept.y, kept.zoom), (-300.0, -200.0, 0.5));
    });
    // Copy, Paste, —, AI agent ▸, Add ▸: Terminal ▸ Default.
    pick(handle, cx, a, 4, Some(0));
    cx.update_window(handle, |_, window, cx| {
        click_innermost_menu_item(window, cx, 0);
        window.render_frame(cx);
    })
    .unwrap();
    let calls = sent(&ws, cx);
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0].0, "add_block");
    answer(handle, &ws, cx, 0, Ok(serde_json::json!({ "block": "d" })));
    apply(
        handle,
        &ws,
        cx,
        super::remote::terminals_of_home(&["a", "d"]),
    );
    let d = viewer(&ws, cx, "d");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.grid.as_ref().unwrap().tiles.ids(), [a, d]);
        assert_eq!(ws.space.focused, Some(d));
        assert_eq!(ws.space.camera(), camera);
    });
}

/// Ctrl+Z of a move in a mirror's grid moves home's block to its place on the canvas, never to
/// the tile it is drawn in.
#[gpui_kit::test]
fn a_move_undone_in_a_mirror_s_grid_sends_home_the_canvas_place(cx: &mut TestAppContext) {
    use super::remote::{live_terminals, remote_of, viewer};
    let (handle, ws, _dir) = live_terminals(cx, &["a", "b"]);
    let a = viewer(&ws, cx, "a");
    ws.update(cx, |ws, _| ws.space.selected = vec![a]);
    press(handle, TOGGLE, cx);
    let tile = rect_of(&ws, cx, a);
    let back = Rect::new(-500.0, 40.0, 400.0, 300.0);
    ws.update(cx, |ws, cx| ws.place(vec![(a, back)], Then::New, cx));
    assert_eq!(rect_of(&ws, cx, a), tile);
    ws.read_with(cx, |ws, _| {
        assert_eq!(remote_of(ws).held.get(&a), Some(&back), "sent to home");
    });
}
