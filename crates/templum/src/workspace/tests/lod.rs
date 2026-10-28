//! Level of detail at far zoom: what each block draws at 33%, 25% and under, and what keeps
//! working there. See docs/specs/level-of-detail.md.

use std::rc::Rc;
use std::time::Duration;

use gpui_kit::ElementId;
use gpui_kit::component::input::EditorState;

use super::*;
use crate::silhouette::WordMap;

/// The camera at rest at `zoom`, its top-left corner at `at` (world units), and a frame drawn.
fn rest_at(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    at: Point,
    zoom: f32,
) {
    ws.update(cx, |ws, _| {
        ws.space.pristine = false;
        ws.space.canvas.camera = Camera::at(at, zoom);
        ws.space.flight = None;
    });
    frame(handle, cx);
}

/// No block focused, the workspace with the keyboard: as after a click on the empty canvas.
fn unfocus(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        let focus = ws.update(cx, |ws, _| {
            ws.space.focused = None;
            ws.focus_handle.clone()
        });
        window.focus(&focus, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Zooms one `step` from the camera as it is and draws a frame 40 ms into the glide, `then` done
/// first in that frame's update (a pan). The glide runs on the real clock: a frame drawn late on
/// a busy machine lands past its end. Then the camera goes back and zooms again, until a frame
/// is drawn mid-glide.
fn zoom_mid_glide(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    step: i32,
    then: impl Fn(&mut Workspace),
) {
    let from = ws.read_with(cx, |ws, _| ws.space.canvas.camera);
    for tries in 1.. {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let pivot = ws.center(window);
                ws.zoom_to(step, pivot, cx);
            })
        })
        .unwrap();
        ws.update(cx, glide_40ms_in);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, _| then(ws));
            window.render_frame(cx);
        })
        .unwrap();
        if ws.read_with(cx, |ws, _| ws.space.flight.is_some()) {
            return;
        }
        assert!(tries < 20, "no frame drawn mid-glide, step {step}");
        rest_at(handle, ws, cx, from.origin, from.zoom());
    }
}

/// The zoom steps text `font` units high is drawn at, as letters or bars: above the Card level.
fn drawn_steps(font: f32) -> impl Iterator<Item = f32> {
    templum_canvas::ZOOM_STEPS
        .into_iter()
        .filter(move |&z| templum_canvas::level(font, z) < templum_canvas::Level::Card)
}

/// The element was drawn in the last frame.
fn shown(handle: AnyWindowHandle, cx: &mut TestAppContext, id: impl Into<ElementId>) -> bool {
    let id = id.into();
    cx.update_window(handle, |_, window, _| window.try_find(id).is_some())
        .unwrap()
}

/// The level follows the zoom the camera last came to rest at: a glide keeps it, through a pan
/// mid-glide, and it changes on the frame the glide ends.
#[gpui_kit::test]
fn the_rest_zoom_holds_through_a_glide_and_changes_when_it_ends(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    rest_at(handle, &ws, cx, Point::new(0.0, 0.0), 0.5);
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.rest, 0.5));
    zoom_mid_glide(handle, &ws, cx, -1, |ws| ws.pan_by(Point::new(30.0, 0.0)));
    ws.read_with(cx, |ws, _| {
        let z = ws.space.view.zoom();
        assert!(ws.space.flight.is_some() && z < 0.5, "mid-glide at {z}");
        assert_eq!(ws.space.rest, 0.5, "the zoom it rested at");
    });
    std::thread::sleep(crate::motion::FLIGHT + Duration::from_millis(50));
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.flight.is_none(), "landed");
        assert_eq!(ws.space.rest, 0.33);
    });
}

/// A workspace shown by a glide before its first frame (Go to the terminal waiting for you, in
/// another workspace) is drawn at the level of the zoom its camera rests at, not of no zoom.
#[gpui_kit::test]
fn a_canvas_first_drawn_mid_glide_keeps_the_zoom_it_rests_at(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            assert_eq!(ws.space.rest, 0.0, "never drawn");
            let pivot = ws.center(window);
            ws.zoom_to(-1, pivot, cx);
        })
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.flight.is_some());
        assert_eq!(
            ws.space.rest, 1.0,
            "the camera rested at 100% before the glide"
        );
    });
}

/// Below Full a title is a label: 12 px text and an 8 px status dot at any zoom, 4 px in from the
/// card's border. At 100% the header is as it was.
#[gpui_kit::test]
fn a_far_title_is_a_label_of_a_fixed_size(cx: &mut TestAppContext) {
    use crate::alert::Alert;
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    unfocus(handle, &ws, cx);
    // News nobody looked at: the dot shows.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().alert = Some(Alert::Attention(None))
    });
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    let corner = Point::new(rect.x - 10.0, rect.y - 10.0);
    let bounds = |cx: &mut TestAppContext, el: (&'static str, usize)| {
        cx.update_window(handle, |_, window, _| window.find(el).bounds())
            .unwrap()
    };
    rest_at(handle, &ws, cx, corner, 1.0);
    let block = bounds(cx, ("block", n));
    assert_eq!(
        bounds(cx, ("status", n)).size,
        size(px(7.0), px(7.0)),
        "the header's dot"
    );
    assert_eq!(
        bounds(cx, ("title", n)).size.width,
        block.size.width - px(2.0),
        "the header's title spans the card"
    );
    let text_at_100 = bounds(cx, ("title-text", n)).size.height;
    for zoom in [0.33, 0.25] {
        rest_at(handle, &ws, cx, corner, zoom);
        let (block, title) = (bounds(cx, ("block", n)), bounds(cx, ("title", n)));
        let (text, dot) = (bounds(cx, ("title-text", n)), bounds(cx, ("status", n)));
        assert_eq!(dot.size, size(px(8.0), px(8.0)), "an 8 px dot at {zoom}");
        assert_eq!(
            text.size.height, text_at_100,
            "12 px text at {zoom}, as at 100%"
        );
        let inset =
            |a: gpui_kit::Pixels, b: gpui_kit::Pixels| (f32::from(a - b) - 5.0).abs() < 0.01;
        assert!(
            inset(title.origin.x, block.origin.x) && inset(title.origin.y, block.origin.y),
            "1 px border and 4 px in at {zoom}: {title:?} in {block:?}"
        );
        assert!(
            title.size.width <= block.size.width - px(10.0),
            "inside the card at {zoom}"
        );
        assert!(
            dot.origin.x > title.origin.x && dot.right() < title.right(),
            "the dot is in the label at {zoom}"
        );
    }
}

/// A press on a far label moves the block and focuses it, and starts no selection in the terminal
/// under it; a right-click opens the block's menu; a double-click frames the block.
#[gpui_kit::test]
fn a_far_label_moves_opens_the_menu_and_frames(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    unfocus(handle, &ws, cx);
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        0.25,
    );
    let c = cx
        .update_window(handle, |_, window, _| {
            window.find(("title", n)).bounds().center()
        })
        .unwrap();
    let at = (f32::from(c.x), f32::from(c.y));
    cx.update_window(handle, |_, window, cx| {
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            at,
            (at.0 + 40.0, at.1),
        );
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let moved = ws.space.canvas.get(id).unwrap().rect;
        assert!(
            moved.x > rect.x + 100.0,
            "40 px at 25%: {rect:?} -> {moved:?}"
        );
        assert_eq!(ws.space.focused, Some(id), "and focused");
        let term = ws.space.terminal(id).unwrap().term.shared();
        assert!(
            term.selection_text().is_none(),
            "no selection under the label"
        );
    });
    cx.update_window(handle, |_, window, cx| window.right_click(("title", n), cx))
        .unwrap();
    ws.update(cx, |ws, _| {
        assert!(ws.menu.is_some(), "its menu");
        ws.menu = None;
    });
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.double_click(("title", n), cx)
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let z = ws.space.canvas.camera.zoom();
        assert!(z > 0.25, "framed: the camera flies to {z}");
    });
}

/// The header's small buttons (a note's preview eye here) hide behind a far label.
#[gpui_kit::test]
fn the_headers_buttons_hide_behind_a_far_label(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let note = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.add_note_at(Point::new(5000.0, 0.0), window, cx);
                ws.space.appearing.clear();
                ws.space.canvas.top().unwrap().0 as usize
            })
        })
        .unwrap();
    rest_at(handle, &ws, cx, Point::new(4990.0, -10.0), 1.0);
    assert!(shown(handle, cx, ("notes", note)));
    rest_at(handle, &ws, cx, Point::new(4990.0, -10.0), 0.25);
    assert!(!shown(handle, cx, ("notes", note)), "the label sits there");
}

/// Under 25% a block is its card: its label in it while the card holds it (a terminal at 8%), its
/// shape further (4%, 1%): its status dot in the middle of the card, its label over the card's
/// top-left corner, and on hover no buttons, resize strips nor grip.
#[gpui_kit::test]
fn a_block_too_small_for_its_label_is_its_shape(cx: &mut TestAppContext) {
    use crate::alert::Alert;
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    unfocus(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().alert = Some(Alert::Attention(None));
        ws.space.hovered = Some(id);
    });
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    let corner = Point::new(rect.x - 100.0, rect.y - 100.0);
    let bounds = |cx: &mut TestAppContext, el: (&'static str, usize)| {
        cx.update_window(handle, |_, window, _| window.find(el).bounds())
            .unwrap()
    };
    rest_at(handle, &ws, cx, corner, 0.08);
    let (block, title) = (bounds(cx, ("block", n)), bounds(cx, ("title", n)));
    assert!(
        title.origin.y > block.origin.y && title.origin.x > block.origin.x,
        "its label in it at 8%: {title:?} in {block:?}"
    );
    let dot = bounds(cx, ("status", n));
    assert!(dot.origin.x > title.origin.x, "the dot is in the label");
    assert!(!shown(handle, cx, ("silhouette", n)), "a card: no bars");
    assert!(shown(handle, cx, ("close", n)), "hovered: its buttons");
    let left = ElementId::Name(format!("resize-left-{}", id.0).into());
    assert!(shown(handle, cx, left.clone()), "and its strips");
    for zoom in [0.04, 0.01] {
        rest_at(handle, &ws, cx, corner, zoom);
        let (block, cover) = (bounds(cx, ("block", n)), bounds(cx, ("cover", n)));
        let (title, dot) = (bounds(cx, ("title", n)), bounds(cx, ("status", n)));
        let near = |a: gpui_kit::Pixels, b: f32| (f32::from(a) - b).abs() < 0.01;
        assert!(
            near(cover.size.width, f32::from(block.size.width) - 2.0),
            "the cover covers the card at {zoom}: {cover:?} in {block:?}"
        );
        let side = f32::from(block.size.width.min(block.size.height)) - 2.0;
        assert!(
            near(dot.size.width, side.min(8.0)),
            "the dot at {zoom}: {dot:?} in {block:?}"
        );
        // GPUI puts an origin on a whole pixel: within half a pixel.
        let (c, d) = (block.center(), dot.center());
        let off = |a: gpui_kit::Pixels| f32::from(a).abs() <= 0.5;
        assert!(
            off(c.x - d.x) && off(c.y - d.y),
            "in the middle at {zoom}: {dot:?} in {block:?}"
        );
        assert!(
            off(title.origin.x - block.origin.x) && title.bottom() < block.origin.y,
            "the label over the card's corner, clear of the dot, at {zoom}: {title:?} {block:?}"
        );
        for el in [("close", n), ("handle", n)] {
            assert!(!shown(handle, cx, el), "no {el:?} at {zoom}");
        }
        assert!(!shown(handle, cx, left.clone()), "no strips at {zoom}");
    }
}

/// A shape is held anywhere: a press on it moves the block and focuses it (it stays its shape),
/// and a double-click frames it.
#[gpui_kit::test]
fn a_shape_moves_by_a_press_on_it_and_frames_on_a_double_click(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    unfocus(handle, &ws, cx);
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 2000.0, rect.y - 2000.0),
        0.01,
    );
    let c = cx
        .update_window(handle, |_, window, _| {
            window.find(("cover", n)).bounds().center()
        })
        .unwrap();
    let at = (f32::from(c.x), f32::from(c.y));
    cx.update_window(handle, |_, window, cx| {
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            at,
            (at.0 + 20.0, at.1),
        );
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let moved = ws.space.canvas.get(id).unwrap().rect;
        assert!(
            moved.x > rect.x + 1000.0,
            "20 px at 1%: {rect:?} -> {moved:?}"
        );
        assert_eq!(ws.space.focused, Some(id), "and focused");
    });
    assert!(shown(handle, cx, ("cover", n)), "focused, still its shape");
    cx.update_window(handle, |_, window, cx| {
        window.double_click(("cover", n), cx)
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let z = ws.space.canvas.camera.zoom();
        assert!(z > 0.25, "framed: the camera flies to {z}");
    });
}

/// Under 25% the block is a card a few pixels wide, and its label isn't clipped by it: 12 px
/// text, read past the card's right edge, where a press still moves the block, a right-click
/// opens its menu and a double-click frames it. From PR #54.
#[gpui_kit::test]
fn a_tiny_cards_label_reads_past_its_edge(cx: &mut TestAppContext) {
    use crate::alert::Alert;
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    unfocus(handle, &ws, cx);
    // News nobody looked at: the dot shows, in the card's label.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().alert = Some(Alert::Attention(None))
    });
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    let bounds = |cx: &mut TestAppContext, el: (&'static str, usize)| {
        cx.update_window(handle, |_, window, _| window.find(el).bounds())
            .unwrap()
    };
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        1.0,
    );
    let text_at_100 = bounds(cx, ("title-text", n)).size;
    // Far enough down at 1% for a shape's label over it to be on the canvas.
    let corner = Point::new(rect.x - 100.0, rect.y - 5000.0);
    // A card at 6% (41 px wide), a shape at 1%.
    for zoom in [0.06, 0.01] {
        rest_at(handle, &ws, cx, corner, zoom);
        let (block, title) = (bounds(cx, ("block", n)), bounds(cx, ("title", n)));
        let text = bounds(cx, ("title-text", n));
        assert!(
            block.size.width < px(50.0),
            "a tiny card at {zoom}: {block:?}"
        );
        assert_eq!(text.size, text_at_100, "the whole title, 12 px, at {zoom}");
        assert!(
            title.right() > block.right(),
            "past the card's edge at {zoom}: {title:?} over {block:?}"
        );
    }
    // A press on the label, right of the card, moves the block.
    let title = bounds(cx, ("title", n));
    let at = (f32::from(title.right()) - 4.0, f32::from(title.center().y));
    cx.update_window(handle, |_, window, cx| {
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            at,
            (at.0 + 10.0, at.1),
        );
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let moved = ws.space.canvas.get(id).unwrap().rect;
        assert!(
            moved.x > rect.x + 500.0,
            "10 px at 1%: {rect:?} -> {moved:?}"
        );
    });
    // Its menu (not the canvas's: it focuses the block), from the label outside the card, and a
    // framing.
    unfocus(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| window.right_click(("title", n), cx))
        .unwrap();
    ws.update(cx, |ws, _| {
        assert!(ws.menu.is_some(), "a menu");
        assert_eq!(ws.space.focused, Some(id), "the block's");
        ws.menu = None;
    });
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.double_click(("title", n), cx)
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let z = ws.space.canvas.camera.zoom();
        assert!(z > 0.25, "framed: the camera flies to {z}");
    });
}

/// Far out, a card's label shortens to its right neighbour's, and hides when that leaves it a few
/// pixels: two blocks 2,000 units apart show both names at 6% (120 px) and the top one at 1%.
/// From PR #54.
#[gpui_kit::test]
fn crowded_far_labels_hide_instead_of_piling_up(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    unfocus(handle, &ws, cx);
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_note_at(Point::new(rect.x + 2000.0, rect.y), window, cx)
        })
    })
    .unwrap();
    let note = ws.read_with(cx, |ws, _| ws.space.canvas.top().unwrap());
    assert_ne!(note, id);
    unfocus(handle, &ws, cx);
    let corner = Point::new(rect.x - 100.0, rect.y - 100.0);
    rest_at(handle, &ws, cx, corner, 0.06);
    assert!(shown(handle, cx, ("title", n)), "room for both at 6%");
    assert!(shown(handle, cx, ("title", note.0 as usize)));
    rest_at(handle, &ws, cx, corner, 0.01);
    assert!(!shown(handle, cx, ("title", n)), "20 px: no pile at 1%");
    assert!(
        shown(handle, cx, ("title", note.0 as usize)),
        "the top one reads"
    );
}

/// A card whose label a neighbour's hides keeps its status dot, in its middle, and is moved by a
/// press anywhere on it.
#[gpui_kit::test]
fn a_crowded_card_keeps_its_dot_and_moves_by_a_press_anywhere(cx: &mut TestAppContext) {
    use crate::alert::Alert;
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_note_at(Point::new(rect.x + 300.0, rect.y), window, cx)
        })
    })
    .unwrap();
    // A note as big as an editor, 24 px right of the terminal at 8%: a card over its label.
    ws.update(cx, |ws, _| {
        let note = ws.space.canvas.top().unwrap();
        ws.space.canvas.get_mut(note).unwrap().rect =
            Rect::new(rect.x + 300.0, rect.y, 900.0, 600.0);
        ws.space.terminal_mut(id).unwrap().alert = Some(Alert::Attention(None));
    });
    unfocus(handle, &ws, cx);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 100.0, rect.y - 100.0),
        0.08,
    );
    assert!(!shown(handle, cx, ("title", n)), "its label hides");
    let (block, dot) = cx
        .update_window(handle, |_, window, _| {
            (
                window.find(("block", n)).bounds(),
                window.find(("status", n)).bounds(),
            )
        })
        .unwrap();
    let (c, d) = (block.center(), dot.center());
    assert!(
        f32::from(c.x - d.x).abs() <= 0.5 && f32::from(c.y - d.y).abs() <= 0.5,
        "its dot in its middle: {dot:?} in {block:?}"
    );
    let at = (f32::from(block.origin.x) + 10.0, f32::from(c.y) + 10.0);
    cx.update_window(handle, |_, window, cx| {
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            at,
            (at.0 + 10.0, at.1),
        );
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let moved = ws.space.canvas.get(id).unwrap().rect;
        assert!(
            moved.x > rect.x + 50.0,
            "10 px at 8%: {rect:?} -> {moved:?}"
        );
    });
}

/// Under 25% the focused terminal is its card too (its letters would be specks under 2.5 px),
/// and the keys still reach it.
#[cfg(unix)]
#[gpui_kit::test]
fn a_focused_terminal_under_25_is_its_card_and_takes_the_keys(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    ws.update(cx, |ws, _| ws.ime_cursor = None);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        0.16,
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(id), "focused");
        assert!(ws.ime_cursor.is_none(), "no grid painted, no cursor");
    });
    assert!(shown(handle, cx, ("title-text", n)), "its label");
    assert!(!shown(handle, cx, ("silhouette", n)), "no bars");
    cx.update_window(handle, |_, window, cx| window.input("z", cx))
        .unwrap();
    super::terminals::until("the key to reach it", handle, &ws, cx, |ws| {
        let term = ws.space.terminal(id).unwrap().term.shared();
        term.lines(5).concat().contains('z')
    });
}

/// A focused terminal at 25% is Full: its letters, its cursor (the IME popup follows it) and the
/// keys. When the focus moves to another block, it is bars on the next frame.
#[cfg(unix)]
#[gpui_kit::test]
fn a_focused_far_terminal_draws_its_letters_and_the_one_left_bars(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    ws.update(cx, |ws, _| ws.ime_cursor = None);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        0.25,
    );
    assert!(
        !shown(handle, cx, ("silhouette", n)),
        "focused: its letters"
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.ime_cursor.is_some(), "its cursor is drawn")
    });
    cx.update_window(handle, |_, window, cx| window.input("z", cx))
        .unwrap();
    super::terminals::until("the key to show", handle, &ws, cx, |ws| {
        ws.space
            .terminal(id)
            .unwrap()
            .snapshot
            .borrow()
            .text()
            .contains('z')
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_note_at(Point::new(rect.x + rect.w + 100.0, rect.y), window, cx)
        })
    })
    .unwrap();
    frame(handle, cx);
    assert!(shown(handle, cx, ("silhouette", n)), "left: bars");
}

/// A far terminal's bars are its colored runs, painted on its grid's cells (the grid's area is
/// where the letters start); a theme switch while far gives them the new palette at once.
#[cfg(unix)]
#[gpui_kit::test]
fn a_far_terminals_bars_are_its_colored_runs_on_its_cells(cx: &mut TestAppContext) {
    use templum_terminal::{Bar, Span};
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    // `cat` prints the line back: "red" in red, then " ok". Waited for in the block's own
    // snapshot: a snapshot of its own (`shown_in`) would take the rows' damage from it.
    ws.read_with(cx, |ws, _| {
        let term = &ws.space.terminal(id).unwrap().term;
        term.write(b"\x1b[31mred\x1b[0m ok\r".to_vec())
    });
    super::terminals::until("red ok", handle, &ws, cx, |ws| {
        let pane = ws.space.terminal(id).unwrap();
        pane.snapshot.borrow().text().contains("\nred ok")
    });
    unfocus(handle, &ws, cx);
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        0.25,
    );
    let bars = |ws: &Workspace| {
        let snap = ws.space.terminal(id).unwrap().snapshot.borrow();
        let row = snap
            .rows
            .iter()
            .rposition(|r| r.text == "red ok")
            .expect("the row");
        snap.rows[row].bars()
    };
    ws.read_with(cx, |ws, _| {
        let p = &ws.theme.palette;
        assert_eq!(
            bars(ws),
            vec![
                Bar {
                    cols: Span { start: 0, len: 3 },
                    fg: p.ansi[1],
                    dim: false
                },
                Bar {
                    cols: Span { start: 4, len: 2 },
                    fg: p.foreground,
                    dim: false
                },
            ]
        );
    });
    cx.update_window(handle, |_, window, _| {
        let grid = window.find(("silhouette", n)).bounds();
        let body = window.find(("body", n)).bounds();
        let pad = px(chrome::PAD * 0.25);
        assert_eq!(grid.origin, point(body.origin.x + pad, body.origin.y + pad));
    })
    .unwrap();
    let red = ws.read_with(cx, |ws, _| ws.theme.palette.ansi[1]);
    ws.update(cx, |ws, cx| ws.set_dark(true, cx));
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        let now = ws.theme.palette.ansi[1];
        assert_ne!(now, red, "another theme");
        assert_eq!(bars(ws)[0].fg, now, "the bars take its red at once");
    });
}

/// Under 2.5 px (8-point text at 25%) a terminal is its card: its grid isn't painted, so it isn't
/// re-armed, and its output wakes the UI once and then waits, as off screen.
#[cfg(unix)]
#[gpui_kit::test]
fn a_terminal_drawn_as_its_card_isnt_rearmed(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    // 8 × 0.25 = 2 px: the Card level. The grid keeps its cells; only the level changes.
    ws.update(cx, |ws, _| ws.cell.font_size = 8.0);
    unfocus(handle, &ws, cx);
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        0.25,
    );
    assert!(shown(handle, cx, ("block", n)), "on screen");
    assert!(!shown(handle, cx, ("silhouette", n)), "no bars: its card");
    let armed = |ws: &Workspace| ws.space.terminal(id).unwrap().term.shared().armed();
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(id).unwrap().term.write(b"x\r".to_vec())
    });
    super::terminals::until("its output to wake the UI once", handle, &ws, cx, |ws| {
        !armed(ws)
    });
    for _ in 0..3 {
        frame(handle, cx);
    }
    ws.read_with(cx, |ws, _| assert!(!armed(ws), "a card isn't re-armed"));
}

/// Over a far terminal the wheel moves the canvas and Ctrl+wheel zooms; once it is clicked
/// (focused: Full), the wheel is the terminal's again.
#[gpui_kit::test]
fn the_wheel_over_a_far_terminal_moves_the_canvas(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    unfocus(handle, &ws, cx);
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    let corner = Point::new(rect.x - 10.0, rect.y - 10.0);
    rest_at(handle, &ws, cx, corner, 0.25);
    let c = cx
        .update_window(handle, |_, window, _| {
            window.find(("silhouette", n)).bounds().center()
        })
        .unwrap();
    let at = (f32::from(c.x), f32::from(c.y));
    let camera = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.space.canvas.camera);
    let down = ScrollDelta::Pixels(point(px(0.0), px(40.0)));
    let before = camera(cx);
    cx.update_window(handle, |_, window, cx| wheel(window, cx, at, down, false))
        .unwrap();
    assert_ne!(camera(cx).origin, before.origin, "the canvas moved");
    rest_at(handle, &ws, cx, corner, 0.25);
    cx.update_window(handle, |_, window, cx| {
        wheel(window, cx, at, ScrollDelta::Lines(point(0.0, 1.0)), true)
    })
    .unwrap();
    assert_eq!(camera(cx).zoom(), 0.33, "Ctrl+wheel zooms");
    rest_at(handle, &ws, cx, corner, 0.25);
    cx.update_window(handle, |_, window, cx| {
        click(window, cx, at, 1);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!shown(handle, cx, ("silhouette", n)), "clicked: Full");
    let before = camera(cx);
    cx.update_window(handle, |_, window, cx| wheel(window, cx, at, down, false))
        .unwrap();
    assert_eq!(camera(cx), before, "the wheel is the terminal's");
}

/// Ctrl+click on a URL in an unfocused far terminal opens it: its bars keep the press, which
/// reads the grid. The URL is printed 10 rows down (the echo of the line is a single row),
/// below the rows the label covers.
#[cfg(unix)]
#[gpui_kit::test]
fn ctrl_click_on_a_url_in_a_far_terminal_opens_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    ws.read_with(cx, |ws, _| {
        let term = &ws.space.terminal(id).unwrap().term;
        term.write(b"\x1b[10Bsee https://example.com/x\r".to_vec())
    });
    super::terminals::until("the URL", handle, &ws, cx, |ws| {
        let pane = ws.space.terminal(id).unwrap();
        pane.snapshot
            .borrow()
            .text()
            .contains("\nsee https://example.com/x")
    });
    unfocus(handle, &ws, cx);
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        0.25,
    );
    assert!(shown(handle, cx, ("silhouette", id.0 as usize)));
    let row = ws.read_with(cx, |ws, _| {
        let snap = ws.space.terminal(id).unwrap().snapshot.borrow();
        snap.rows
            .iter()
            .rposition(|r| r.text.starts_with("see https"))
            .unwrap()
    });
    super::terminals::ctrl_click_cell(handle, &ws, cx, id, (row, 8));
    assert_eq!(cx.opened_url().as_deref(), Some("https://example.com/x"));
}

/// A find field left open on a terminal that lost the focus at 33% is hidden, and comes back with
/// its text when the terminal is focused again.
#[gpui_kit::test]
fn a_find_field_hides_far_away_and_comes_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        0.33,
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_find(window, cx));
        window.render_frame(cx);
        window.input("needle", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(shown(handle, cx, ("find", n)), "focused: its field");
    unfocus(handle, &ws, cx);
    assert!(!shown(handle, cx, ("find", n)), "far and left: hidden");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(id, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    assert!(shown(handle, cx, ("find", n)), "back");
    ws.read_with(cx, |ws, cx| {
        let bar = ws.space.terminal(id).unwrap().find.as_ref().unwrap();
        assert_eq!(bar.input.read(cx).value().as_ref(), "needle");
    });
}

/// "Go to the terminal waiting for you" focuses a far waiting terminal: it is drawn Full.
#[gpui_kit::test]
fn going_to_a_far_waiting_terminal_draws_it_in_full(cx: &mut TestAppContext) {
    use crate::alert::Alert;
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let n = id.0 as usize;
    unfocus(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().alert = Some(Alert::Attention(None))
    });
    let rect = ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect);
    rest_at(
        handle,
        &ws,
        cx,
        Point::new(rect.x - 10.0, rect.y - 10.0),
        0.25,
    );
    assert!(shown(handle, cx, ("silhouette", n)), "waiting, far: bars");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.go_to_waiting(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!shown(handle, cx, ("silhouette", n)), "gone to: Full");
}

/// Below Full, rendered Markdown is an empty area: an editor's Markdown file keeps its tab row. A
/// kanban block is drawn in full there.
#[gpui_kit::test]
fn far_rendered_markdown_is_left_out_and_kanban_drawn_in_full(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# Hello\n\nSome *words*.\n").unwrap();
    let (handle, ws) = open(cx);
    let editor = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    let kanban = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.toggle_file_preview(editor, window, cx);
                ws.add_kanban_at(Point::new(1200.0, 40.0), window, cx);
                let kanban = ws.space.canvas.top().unwrap();
                ws.space.appearing.clear();
                kanban.0 as usize
            })
        })
        .unwrap();
    let e = editor.0 as usize;
    unfocus(handle, &ws, cx);
    rest_at(handle, &ws, cx, Point::new(0.0, 0.0), 0.5);
    assert!(
        shown(handle, cx, ("file-preview", e)),
        "the file rendered at 50%"
    );
    rest_at(handle, &ws, cx, Point::new(0.0, 0.0), 0.33);
    assert!(
        !shown(handle, cx, ("file-preview", e)),
        "the file's area is empty"
    );
    assert!(
        shown(handle, cx, ElementId::Name(format!("tab-{e}-0").into())),
        "the tab row stays"
    );
    assert!(
        shown(handle, cx, ("kanban-empty", kanban)),
        "the kanban block is drawn in full"
    );
    // Under 25% both are their cards: nothing inside.
    rest_at(handle, &ws, cx, Point::new(0.0, 0.0), 0.16);
    assert!(!shown(handle, cx, ("kanban-empty", kanban)), "no board");
    assert!(
        !shown(handle, cx, ElementId::Name(format!("tab-{e}-0").into())),
        "no tab row"
    );
}

/// A note at `at` holding `text`, at rest (not popping in), focused; and its editor.
fn note_with(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    at: Point,
    text: &str,
) -> (BlockId, Entity<EditorState>) {
    cx.update_window(handle, |_, window, cx| {
        let (id, editor) = ws.update(cx, |ws, cx| {
            ws.add_note_at(at, window, cx);
            ws.space.appearing.clear();
            let id = ws.space.canvas.top().unwrap();
            (
                id,
                ws.space.blocks[&id]
                    .note()
                    .unwrap()
                    .tab()
                    .unwrap()
                    .editor
                    .clone(),
            )
        });
        editor.update(cx, |e, cx| e.set_value(text.to_string(), window, cx));
        window.render_frame(cx);
        (id, editor)
    })
    .unwrap()
}

fn note_map(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Option<Rc<WordMap>> {
    ws.read_with(cx, |ws, _| {
        ws.space.blocks[&id]
            .note()
            .unwrap()
            .tab()
            .unwrap()
            .far
            .borrow()
            .map
            .clone()
    })
}

/// An unfocused note at 33% builds no gpui-kit editor: it paints its words as bars, in its text's
/// own box. At 50% it is its editor again.
#[gpui_kit::test]
fn a_far_note_draws_bars_in_its_box_and_at_fifty_its_editor(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _) = note_with(
        handle,
        &ws,
        cx,
        Point::new(40.0, 40.0),
        "# Plan\nwrite the bars\n",
    );
    let n = id.0 as usize;
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 0.33);
    let full = cx
        .update_window(handle, |_, window, _| {
            window.find(("note-editor", n)).bounds()
        })
        .unwrap();
    unfocus(handle, &ws, cx);
    frame(handle, cx);
    assert!(!shown(handle, cx, ("note-text", n)), "no gpui-kit editor");
    let bars = cx
        .update_window(handle, |_, window, _| {
            window.find(("silhouette", n)).bounds()
        })
        .unwrap();
    assert_eq!(bars, full, "the bars fill the editor's box");
    let map = note_map(&ws, cx, id).expect("a word map");
    assert_eq!(map.words.len(), 5, "#, Plan, write, the, bars");
    rest_at(handle, &ws, cx, corner, 0.5);
    assert!(shown(handle, cx, ("note-text", n)), "Full at 50%");
}

/// Bars line up with letters at every zoom step, within 1 px: a soft-wrapped note scrolled a row
/// and a half, with a folded section on screen, its word map taken at 50% (as after a zoom out
/// from there), against its letters laid out at each step.
#[gpui_kit::test]
fn a_notes_bars_line_up_with_its_letters_at_every_zoom(cx: &mut TestAppContext) {
    use crate::silhouette::{Taken, note_origin};
    use gpui_kit::component::input::FoldRange;
    let mut text = String::from("# Title\nintro\n## Folded\n");
    for i in 0..5 {
        text.push_str(&format!("hidden words {i}\n"));
    }
    text.push_str("## After\n");
    for i in 0..60 {
        text.push_str(&format!(
            "{i:02} the quick brown fox jumps over the lazy dog and keeps on running far\n"
        ));
    }
    let (handle, ws) = open(cx);
    let (id, note) = note_with(handle, &ws, cx, Point::new(40.0, 40.0), &text);
    let n = id.0 as usize;
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 1.0);
    // Folds "## Folded" (line 2; lines 3 to 7 hidden) from its gutter icon, drawn on the cursor's
    // line: 14 px, 2 px into the gutter (gpui-base-0.7.3 src/input/base/element.rs:1334-1348).
    let line2 = "# Title\nintro\n".len();
    cx.update_window(handle, |_, window, cx| {
        note.update(cx, |e, cx| {
            e.apply_highlighter_fold_candidates(vec![FoldRange::new(2, 8)], cx);
            e.set_selected_range(line2..line2, cx);
        });
        window.render_frame(cx);
        let e = note.read(cx);
        let (tb, lh) = (
            e.text_bounds().unwrap(),
            f32::from(e.line_height().unwrap()),
        );
        let icon = (
            f32::from(tb.origin.x) + 9.0,
            f32::from(tb.origin.y) + 2.5 * lh,
        );
        click(window, cx, icon, 1);
        window.render_frame(cx);
    })
    .unwrap();
    let (hidden, after) = (text.find("hidden").unwrap(), text.find("## After").unwrap());
    cx.update_window(handle, |_, _, cx| {
        let b = note
            .read(cx)
            .range_to_bounds(&(hidden..hidden + 6))
            .unwrap();
        assert_eq!(b.size.width, px(0.0), "line 3 is folded away");
    })
    .unwrap();
    // A row and a half down: the fold stays on screen.
    cx.update_window(handle, |_, window, cx| {
        note.update(cx, |e, cx| {
            let row = e.line_height().unwrap();
            e.set_scroll_offset(point(px(0.0), -row * 1.5), cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    rest_at(handle, &ws, cx, corner, 0.5);
    let map = cx
        .update_window(handle, |_, _, cx| {
            let taken = Taken {
                len: text.len(),
                size: (0.0, 0.0),
                font: "".into(),
                wrap: true,
                tree: false,
            };
            WordMap::take(note.read(cx), 0.5, taken).expect("laid out")
        })
        .unwrap();
    assert!(
        (map.scroll.0 - 1.5).abs() < 0.01,
        "taken scrolled: {:?}",
        map.scroll
    );
    assert!(
        map.words
            .iter()
            .all(|w| !(hidden..after).contains(&w.range.start)),
        "no bars for the folded lines"
    );
    // Every step it has letters or bars at: under 25% it is a card.
    for zoom in drawn_steps(crate::note::FONT) {
        rest_at(handle, &ws, cx, corner, zoom);
        cx.update_window(handle, |_, window, cx| {
            let origin = window.find(("note-editor", n)).bounds().origin;
            let e = note.read(cx);
            let lh = f32::from(e.line_height().unwrap());
            let cells = map.cells(note_origin(&map, zoom), crate::note::FONT, zoom);
            let mut compared = 0;
            for (word, (x, y, w, h)) in map.words.iter().zip(cells) {
                let Some(b) = e.range_to_bounds(&word.range) else {
                    continue;
                };
                if f32::from(b.size.height) > 1.5 * lh || b.size.width <= px(0.0) {
                    continue;
                }
                let (x, y) = (f32::from(origin.x) + x, f32::from(origin.y) + y);
                for (what, bar, letters) in [
                    ("left", x, f32::from(b.origin.x)),
                    ("right", x + w, f32::from(b.origin.x + b.size.width)),
                    ("top", y, f32::from(b.origin.y)),
                    ("height", h, f32::from(b.size.height)),
                ] {
                    assert!(
                        (bar - letters).abs() <= 1.0,
                        "{what} of {:?} at {zoom}: bar {bar}, letters {letters}",
                        &text[word.range.clone()]
                    );
                }
                compared += 1;
            }
            assert!(compared > 30, "{compared} words compared at {zoom}");
        })
        .unwrap();
    }
}

/// A word longer than the note's line, which gpui-kit breaks across rows, is a bar on each row it
/// is on, as wide as its letters there.
#[gpui_kit::test]
fn a_far_notes_word_broken_across_rows_is_a_bar_on_each(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let text = format!("a {}\n", "x".repeat(100));
    let (id, _) = note_with(handle, &ws, cx, Point::new(40.0, 40.0), &text);
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 0.5);
    unfocus(handle, &ws, cx);
    rest_at(handle, &ws, cx, corner, 0.33);
    let map = note_map(&ws, cx, id).expect("taken");
    let parts: Vec<_> = map.words.iter().filter(|w| w.range.start == 2).collect();
    assert!(parts.len() >= 2, "{parts:?}");
    assert!(
        parts.windows(2).all(|p| p[1].row == p[0].row + 1),
        "{parts:?}"
    );
    // 100 letters 0.6 em wide at 13 px, world units.
    let w: f32 = parts.iter().map(|p| p.w).sum();
    assert!((w - 100.0 * 0.6 * 13.0).abs() < 2.0, "{w}: {parts:?}");
}

/// A note with no word map yet (a canvas restored at 25%: never drawn Full) is laid out unseen on
/// its first frame, and its next frame takes its word map.
#[gpui_kit::test]
fn a_far_note_with_no_word_map_yet_gets_one_after_its_first_frames(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.add_note_at(Point::new(40.0, 40.0), window, cx);
                let id = ws.space.canvas.top().unwrap();
                let editor = ws.space.blocks[&id]
                    .note()
                    .unwrap()
                    .tab()
                    .unwrap()
                    .editor
                    .clone();
                editor.update(cx, |e, cx| {
                    e.set_value(
                        "# Plan\nwrite the bars\nthen the label\n".to_string(),
                        window,
                        cx,
                    )
                });
                ws.space.appearing.clear();
                ws.space.focused = None;
                ws.space.pristine = false;
                ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 0.25);
                window.focus(&ws.focus_handle, cx);
                id
            })
        })
        .unwrap();
    // The update above drew the first frame: a test context draws a window an update left dirty.
    assert!(
        note_map(&ws, cx, id).is_none(),
        "the first frame lays its editor out, unseen"
    );
    assert!(shown(handle, cx, ("silhouette", id.0 as usize)));
    frame(handle, cx);
    let map = note_map(&ws, cx, id).expect("the next frame takes its word map");
    assert_eq!(
        map.words.len(),
        8,
        "#, Plan, write, the, bars, then, the, label"
    );
}

/// A far note resized (a command, the keyboard elsewhere) gets its word map taken again: one
/// unseen Full frame lays it out at its new width, the next takes it.
#[gpui_kit::test]
fn a_far_notes_word_map_is_taken_again_after_a_resize(cx: &mut TestAppContext) {
    let text: String = (0..20)
        .map(|i| format!("{i} the quick brown fox jumps over the lazy dog\n"))
        .collect();
    let (handle, ws) = open(cx);
    let (id, _) = note_with(handle, &ws, cx, Point::new(40.0, 40.0), &text);
    rest_at(handle, &ws, cx, Point::new(30.0, 30.0), 0.33);
    unfocus(handle, &ws, cx);
    let dog = text.find("dog").unwrap();
    let row = |m: &WordMap| m.words.iter().find(|w| w.range.start == dog).unwrap().row;
    let before = note_map(&ws, cx, id).expect("taken");
    assert_eq!(row(&before), 1, "the first line wraps at 360 wide");
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(id).unwrap().rect.w += 200.0
    });
    frame(handle, cx);
    frame(handle, cx);
    let after = note_map(&ws, cx, id).expect("taken again");
    assert_eq!(after.taken.size.0, before.taken.size.0 + 200.0);
    assert_eq!(row(&after), 0, "one row at 560 wide");
}

/// Over a far note the wheel moves the canvas; once it is clicked (focused: Full), the wheel
/// scrolls the note again.
#[gpui_kit::test]
fn the_wheel_over_a_far_note_moves_the_canvas(cx: &mut TestAppContext) {
    let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
    let (handle, ws) = open(cx);
    let (id, note) = note_with(handle, &ws, cx, Point::new(40.0, 40.0), &text);
    let n = id.0 as usize;
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 0.33);
    unfocus(handle, &ws, cx);
    let c = cx
        .update_window(handle, |_, window, _| {
            window.find(("silhouette", n)).bounds().center()
        })
        .unwrap();
    let at = (f32::from(c.x), f32::from(c.y));
    let camera = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.space.canvas.camera);
    let down = ScrollDelta::Pixels(point(px(0.0), px(-40.0)));
    let before = camera(cx);
    cx.update_window(handle, |_, window, cx| wheel(window, cx, at, down, false))
        .unwrap();
    assert_ne!(camera(cx).origin, before.origin, "the canvas moved");
    rest_at(handle, &ws, cx, corner, 0.33);
    cx.update_window(handle, |_, window, cx| {
        click(window, cx, at, 1);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(shown(handle, cx, ("note-text", n)), "clicked: Full");
    let (before, scroll) = (camera(cx), note.read_with(cx, |e, _| e.scroll_offset()));
    cx.update_window(handle, |_, window, cx| {
        wheel(window, cx, at, down, false);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(camera(cx), before, "the wheel is the note's");
    assert_ne!(
        note.read_with(cx, |e, _| e.scroll_offset()),
        scroll,
        "the note scrolled"
    );
}

/// A note's rows survive Full → bars → Full: the same lines are on screen after.
#[gpui_kit::test]
fn a_notes_rows_survive_being_drawn_far_away(cx: &mut TestAppContext) {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let (handle, ws) = open(cx);
    let (id, note) = note_with(handle, &ws, cx, Point::new(40.0, 40.0), &text);
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 0.5);
    cx.update_window(handle, |_, window, cx| {
        note.update(cx, |e, cx| {
            let row = e.line_height().unwrap();
            e.set_scroll_offset(point(px(0.0), -row * 60.0), cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    let top =
        |cx: &mut TestAppContext| note.read_with(cx, |e, _| e.visible_row_range().unwrap().start);
    assert_eq!(top(cx), 60);
    unfocus(handle, &ws, cx);
    rest_at(handle, &ws, cx, corner, 0.33);
    assert!(shown(handle, cx, ("silhouette", id.0 as usize)));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(id, window, cx))
    })
    .unwrap();
    rest_at(handle, &ws, cx, corner, 0.33);
    assert_eq!(top(cx), 60, "focused again at 33%");
    rest_at(handle, &ws, cx, corner, 0.5);
    assert_eq!(top(cx), 60, "and at 50%");
}

/// An editor on a folder holding `files` (name, text), its first file open and focused, at rest.
fn editor_with(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    files: &[(&str, &str)],
) -> (tempfile::TempDir, BlockId, Entity<EditorState>) {
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let id = open_editor(handle, ws, cx, dir.path());
    click_file(handle, 0, cx);
    let state = ws.read_with(cx, |ws, _| {
        ws.space
            .editor(id)
            .unwrap()
            .active_tab()
            .unwrap()
            .editor
            .clone()
    });
    (dir, id, state)
}

fn tab_map(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Option<Rc<WordMap>> {
    ws.read_with(cx, |ws, _| {
        let tab = ws.space.editor(id).unwrap().active_tab().unwrap();
        tab.far.borrow().map.clone()
    })
}

/// An unfocused editor at 33% builds no gpui-kit editor and no tree rows: its file's words are
/// bars where its code was, its tab row stays. The level changes when a glide ends: out from
/// 50%, bars from the frame it ends (the map of its last frame); in from 33%, its code.
#[gpui_kit::test]
fn a_far_editor_draws_bars_where_its_code_was(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_dir, id, _) = editor_with(
        handle,
        &ws,
        cx,
        &[("a.rs", "fn main() {\n    println!(\"hi\");\n}\n")],
    );
    let n = id.0 as usize;
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 0.33);
    let code = cx
        .update_window(handle, |_, window, _| window.find(("code", n)).bounds())
        .unwrap();
    unfocus(handle, &ws, cx);
    assert!(!shown(handle, cx, ("code", n)), "no gpui-kit editor");
    assert!(!shown(handle, cx, ("tree", n)), "the tree's panel is empty");
    let bars = cx
        .update_window(handle, |_, window, _| {
            window.find(("silhouette", n)).bounds()
        })
        .unwrap();
    assert_eq!(bars, code, "bars where the code was");
    assert!(
        shown(handle, cx, ElementId::Name(format!("tab-{n}-0").into())),
        "the tab row stays"
    );
    let words = tab_map(&ws, cx, id).expect("taken").words.len();
    assert_eq!(words, 5, "fn, main(), {{, println!(\"hi\");, }}");
    rest_at(handle, &ws, cx, corner, 0.5);
    for (step, during, landed) in [(-1, "code", "silhouette"), (1, "silhouette", "code")] {
        zoom_mid_glide(handle, &ws, cx, step, |_| {});
        assert!(
            shown(handle, cx, (during, n)),
            "mid-glide, step {step}: {during}"
        );
        std::thread::sleep(crate::motion::FLIGHT + Duration::from_millis(50));
        frame(handle, cx);
        assert!(
            shown(handle, cx, (landed, n)) && !shown(handle, cx, (during, n)),
            "landed, step {step}: {landed}"
        );
    }
}

/// Bars line up with letters at every zoom step, within 1 px: an editor tab of 1,000 lines (four
/// digits of line numbers) scrolled 10 rows down and 6 rows' worth across, its word map taken at
/// 50%, against its letters laid out at each step.
#[gpui_kit::test]
fn an_editor_tabs_bars_line_up_with_its_letters_at_every_zoom(cx: &mut TestAppContext) {
    use crate::silhouette::{Taken, tab_origin};
    let words: Vec<String> = (0..30).map(|j| format!("w{j}")).collect();
    let text: String = (0..1000)
        .map(|i| format!("line{i} {}\n", words.join(" ")))
        .collect();
    let (handle, ws) = open(cx);
    let (_dir, id, state) = editor_with(handle, &ws, cx, &[("a.txt", text.as_str())]);
    let n = id.0 as usize;
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 0.5);
    cx.update_window(handle, |_, window, cx| {
        state.update(cx, |e, cx| {
            let row = e.line_height().unwrap();
            e.set_scroll_offset(point(-row * 6.0, -row * 10.0), cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    let map = cx
        .update_window(handle, |_, _, cx| {
            let taken = Taken {
                len: text.len(),
                size: (0.0, 0.0),
                font: "".into(),
                wrap: false,
                tree: true,
            };
            WordMap::take(state.read(cx), 0.5, taken).expect("laid out")
        })
        .unwrap();
    assert!(
        (map.scroll.0 - 10.0).abs() < 0.01 && (map.scroll.1 - 6.0).abs() < 0.01,
        "taken scrolled: {:?}",
        map.scroll
    );
    for zoom in drawn_steps(crate::editor::FONT) {
        rest_at(handle, &ws, cx, corner, zoom);
        cx.update_window(handle, |_, window, cx| {
            let origin = window.find(("code", n)).bounds().origin;
            let e = state.read(cx);
            let lh = f32::from(e.line_height().unwrap());
            let cells = map.cells(tab_origin(&map, zoom), crate::editor::FONT, zoom);
            let mut compared = 0;
            for (word, (x, y, w, h)) in map.words.iter().zip(cells) {
                let Some(b) = e.range_to_bounds(&word.range) else {
                    continue;
                };
                if f32::from(b.size.height) > 1.5 * lh || b.size.width <= px(0.0) {
                    continue;
                }
                let (x, y) = (f32::from(origin.x) + x, f32::from(origin.y) + y);
                for (what, bar, letters) in [
                    ("left", x, f32::from(b.origin.x)),
                    ("right", x + w, f32::from(b.origin.x + b.size.width)),
                    ("top", y, f32::from(b.origin.y)),
                    ("height", h, f32::from(b.size.height)),
                ] {
                    assert!(
                        (bar - letters).abs() <= 1.0,
                        "{what} of {:?} at {zoom}: bar {bar}, letters {letters}",
                        &text[word.range.clone()]
                    );
                }
                compared += 1;
            }
            assert!(compared > 50, "{compared} words compared at {zoom}");
        })
        .unwrap();
    }
}

/// A far editor whose file changes on disk shows the new text's bars a frame later, even when
/// the new text is as long as the old: the reload drops its word map, one unseen Full frame lays
/// the new text out, the next takes it.
#[gpui_kit::test]
fn a_far_editor_shows_its_files_new_text_after_a_frame(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (dir, id, _) = editor_with(handle, &ws, cx, &[("a.txt", "old words\n")]);
    rest_at(handle, &ws, cx, Point::new(30.0, 30.0), 0.33);
    unfocus(handle, &ws, cx);
    assert_eq!(tab_map(&ws, cx, id).expect("taken").words.len(), 2);
    // As long as before: only the reload can tell the map is stale. A later modification time.
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(dir.path().join("a.txt"), "a b c d e\n").unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.check_files(window, cx))
    })
    .unwrap();
    assert!(tab_map(&ws, cx, id).is_none(), "the reload dropped the map");
    frame(handle, cx);
    frame(handle, cx);
    let map = tab_map(&ws, cx, id).expect("taken again");
    assert_eq!(map.words.len(), 5, "a, b, c, d, e");
}

/// A focused editor at 25% is Full and typing reaches it; when the focus moves to another block,
/// it is bars on the next frame.
#[gpui_kit::test]
fn a_focused_far_editor_takes_typing_and_the_one_left_draws_bars(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_dir, id, state) = editor_with(handle, &ws, cx, &[("a.txt", "one\n")]);
    let n = id.0 as usize;
    rest_at(handle, &ws, cx, Point::new(30.0, 30.0), 0.25);
    assert!(shown(handle, cx, ("code", n)), "focused: Full at 25%");
    cx.update_window(handle, |_, window, cx| {
        window.input("zz", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        state.read_with(cx, |e, _| e.value().contains("zz")),
        "typing reaches it"
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_note_at(Point::new(3000.0, 0.0), window, cx)
        })
    })
    .unwrap();
    frame(handle, cx);
    assert!(shown(handle, cx, ("silhouette", n)), "left: bars");
}

/// Over a far editor the wheel moves the canvas; once it is clicked (focused: Full), the wheel
/// scrolls its file again.
#[gpui_kit::test]
fn the_wheel_over_a_far_editor_moves_the_canvas(cx: &mut TestAppContext) {
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let (handle, ws) = open(cx);
    let (_dir, id, state) = editor_with(handle, &ws, cx, &[("a.txt", text.as_str())]);
    let n = id.0 as usize;
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 0.33);
    unfocus(handle, &ws, cx);
    let c = cx
        .update_window(handle, |_, window, _| {
            window.find(("silhouette", n)).bounds().center()
        })
        .unwrap();
    let at = (f32::from(c.x), f32::from(c.y));
    let camera = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.space.canvas.camera);
    let down = ScrollDelta::Pixels(point(px(0.0), px(-40.0)));
    let before = camera(cx);
    cx.update_window(handle, |_, window, cx| wheel(window, cx, at, down, false))
        .unwrap();
    assert_ne!(camera(cx).origin, before.origin, "the canvas moved");
    rest_at(handle, &ws, cx, corner, 0.33);
    cx.update_window(handle, |_, window, cx| {
        click(window, cx, at, 1);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(shown(handle, cx, ("code", n)), "clicked: Full");
    let (before, scroll) = (camera(cx), state.read_with(cx, |e, _| e.scroll_offset()));
    cx.update_window(handle, |_, window, cx| {
        wheel(window, cx, at, down, false);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(camera(cx), before, "the wheel is the editor's");
    assert_ne!(
        state.read_with(cx, |e, _| e.scroll_offset()),
        scroll,
        "its file scrolled"
    );
}

/// An editor's state survives Full → bars → Full: its tabs and the active one, its rows on screen,
/// its selection, its text and its undo history.
#[gpui_kit::test]
fn an_editors_state_survives_being_drawn_far_away(cx: &mut TestAppContext) {
    use gpui_kit::component::input::Undo;
    let text: String = (0..200).map(|i| format!("line {i}\n")).collect();
    let (handle, ws) = open(cx);
    let (_dir, id, state) = editor_with(
        handle,
        &ws,
        cx,
        &[("a.txt", text.as_str()), ("b.txt", "b\n")],
    );
    // b.txt opened, then a.txt shown again: two tabs, the first active.
    click_file(handle, 1, cx);
    click_file(handle, 0, cx);
    let corner = Point::new(30.0, 30.0);
    rest_at(handle, &ws, cx, corner, 0.5);
    cx.update_window(handle, |_, window, cx| {
        window.input("typed ", cx);
        state.update(cx, |e, cx| {
            // A selection on line 45, on screen once scrolled 40 rows down.
            let at = e.value().find("line 45").unwrap();
            e.set_selected_range(at..at + 4, cx);
            let row = e.line_height().unwrap();
            e.set_scroll_offset(point(px(0.0), -row * 40.0), cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    let seen = |cx: &mut TestAppContext| {
        state.read_with(cx, |e, _| {
            (e.visible_row_range(), e.selected_range(), e.value())
        })
    };
    let before = seen(cx);
    unfocus(handle, &ws, cx);
    rest_at(handle, &ws, cx, corner, 0.33);
    assert!(shown(handle, cx, ("silhouette", id.0 as usize)));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(id, window, cx))
    })
    .unwrap();
    rest_at(handle, &ws, cx, corner, 0.5);
    assert_eq!(
        seen(cx),
        before,
        "its rows, selection and text as they were"
    );
    ws.read_with(cx, |ws, _| {
        let e = ws.space.editor(id).unwrap();
        assert_eq!((e.tabs.len(), e.active), (2, Some(0)));
    });
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_action(Box::new(Undo), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_ne!(
        state.read_with(cx, |e, _| e.value()),
        before.2,
        "its undo history is there"
    );
}

/// Taking a word map walks what is on screen, not the whole text: a minified file's long line,
/// or a long paragraph soft-wrapped in a note, gets bars for its words near the view only (each
/// word costs gpui-kit a walk from its line's start).
#[gpui_kit::test]
fn a_word_map_takes_only_the_words_near_the_view(cx: &mut TestAppContext) {
    let words: String = (0..2000).map(|i| format!("w{i} ")).collect();
    let text = format!("{words}\n");
    let (handle, ws) = open(cx);
    let (_dir, editor, _) = editor_with(handle, &ws, cx, &[("a.txt", text.as_str())]);
    let (note, _) = note_with(handle, &ws, cx, Point::new(40.0, 1000.0), &text);
    let corner = Point::new(30.0, 30.0);
    // Both Full at 50% (the note was focused last, the editor is on screen), then far.
    rest_at(handle, &ws, cx, corner, 0.5);
    unfocus(handle, &ws, cx);
    rest_at(handle, &ws, cx, corner, 0.5);
    rest_at(handle, &ws, cx, corner, 0.33);
    let mapped = tab_map(&ws, cx, editor).expect("the editor's").words.len();
    assert!(mapped > 0 && mapped < 200, "the editor: {mapped} words");
    let mapped = note_map(&ws, cx, note).expect("the note's").words.len();
    assert!(mapped > 0 && mapped < 600, "the note: {mapped} words");
}
