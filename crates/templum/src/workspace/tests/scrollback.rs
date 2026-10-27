//! A terminal's scrollbar and its Jump to the bottom.

use super::kanban::hover_at;
use super::*;

/// The startup terminal at 400 × 300 at the canvas's origin, the camera at `zoom` there, its
/// history 100 lines long (fed to its grid as its program's output): (window, workspace, block).
fn long_terminal(
    cx: &mut TestAppContext,
    zoom: f32,
) -> (AnyWindowHandle, Entity<Workspace>, BlockId) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let text: String = (1..=100).map(|i| format!("line {i}\r\n")).collect();
    ws.update(cx, |ws, _| {
        ws.space.pristine = false;
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), zoom);
        ws.space.canvas.get_mut(id).unwrap().rect = Rect::new(40.0, 40.0, 400.0, 300.0);
        ws.space.terminal(id).unwrap().term.feed(text.as_bytes());
    });
    frame(handle, cx);
    (handle, ws, id)
}

fn view(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
) -> templum_terminal::Scrollback {
    ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(id)
            .unwrap()
            .term
            .shared()
            .scrollback()
            .unwrap()
    })
}

/// Whether element `what` of block `id` is drawn, after a frame.
fn drawn(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    what: &'static str,
    id: BlockId,
) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find((what, id.0 as usize)).is_some()
    })
    .unwrap()
}

fn strip(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    id: BlockId,
) -> Bounds<gpui_kit::Pixels> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(("history-bar", id.0 as usize)).bounds()
    })
    .unwrap()
}

fn down_at(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32)) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position: point(px(at.0), px(at.1)),
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.render_frame(cx);
}

fn up_at(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32)) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position: point(px(at.0), px(at.1)),
            modifiers: Modifiers::default(),
            click_count: 1,
        }),
        cx,
    );
    window.render_frame(cx);
}

#[gpui_kit::test]
fn a_terminal_with_a_history_has_a_scrollbar_over_its_text_s_right(cx: &mut TestAppContext) {
    let (handle, _, id) = long_terminal(cx, 1.0);
    assert!(drawn(handle, cx, "history-bar", id));
    assert!(!drawn(handle, cx, "to-bottom", id), "at the live screen");
    let (block, body) = cx
        .update_window(handle, |_, window, _| {
            let n = id.0 as usize;
            (
                window.find(("block", n)).bounds(),
                window.find(("body", n)).bounds(),
            )
        })
        .unwrap();
    let bar = strip(handle, cx, id);
    let pad = px(chrome::PAD);
    assert_eq!(bar.size.width, px(16.0));
    assert_eq!(bar.right(), body.right() - pad, "at the text's right");
    assert_eq!(bar.top(), body.top() + pad);
    let grip = block.bottom() - px(crate::block::BORDER + chrome::HANDLE);
    assert_eq!(bar.bottom(), grip, "down to the grip's square");
    // A fresh terminal has none: nothing to scroll.
    let (handle, ws, id) = {
        let (handle, ws) = open(cx);
        (handle, ws.clone(), first_id(&ws, cx))
    };
    assert_eq!(view(&ws, cx, id).history, 0);
    assert!(!drawn(handle, cx, "history-bar", id));
}

/// The track and its offset from the text's edge are the zoom's, like the rest of the block.
#[gpui_kit::test]
fn the_terminal_s_scrollbar_scales_with_the_zoom(cx: &mut TestAppContext) {
    let (handle, _ws, id) = long_terminal(cx, 0.5);
    assert_eq!(strip(handle, cx, id).size.width, px(8.0));
}

#[gpui_kit::test]
fn a_press_on_the_track_the_pointer_is_over_jumps_there(cx: &mut TestAppContext) {
    let (handle, ws, id) = long_terminal(cx, 1.0);
    let bar = strip(handle, cx, id);
    let near_top = (f32::from(bar.center().x), f32::from(bar.top()) + 4.0);
    // Not shown, the strip is the text's: the press selects.
    cx.update_window(handle, |_, window, cx| {
        down_at(window, cx, near_top);
        up_at(window, cx, near_top);
    })
    .unwrap();
    assert_eq!(view(&ws, cx, id).below, 0, "the bar wasn't shown");
    cx.update_window(handle, |_, window, cx| {
        hover_at(window, cx, near_top);
        down_at(window, cx, near_top);
        up_at(window, cx, near_top);
    })
    .unwrap();
    let back = view(&ws, cx, id);
    assert_eq!(back.below, back.history, "the top of the history");
    assert!(drawn(handle, cx, "to-bottom", id));
}

#[gpui_kit::test]
fn dragging_the_thumb_moves_the_view_through_the_history(cx: &mut TestAppContext) {
    let (handle, ws, id) = long_terminal(cx, 1.0);
    let bar = strip(handle, cx, id);
    let x = f32::from(bar.center().x);
    // At the live screen the thumb is at the end of the track.
    let thumb = (x, f32::from(bar.bottom()) - 8.0);
    cx.update_window(handle, |_, window, cx| {
        hover_at(window, cx, thumb);
        window.drag(
            point(px(thumb.0), px(thumb.1)),
            point(px(x), bar.center().y),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    let back = view(&ws, cx, id);
    assert!(
        back.below > 10 && back.below < back.history,
        "halfway up: {back:?}"
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws
            .space
            .terminal(id)
            .unwrap()
            .term
            .shared()
            .selection_text()),
        None,
        "the drag selected nothing"
    );
}

/// The wheel shows the bar while the view scrolls: a press on its track, the pointer never over
/// it before, is the bar's.
#[gpui_kit::test]
fn the_bar_shows_while_the_wheel_scrolls(cx: &mut TestAppContext) {
    let (handle, ws, id) = long_terminal(cx, 1.0);
    let bar = strip(handle, cx, id);
    let over_text = at(&ws, cx, (120.0, 160.0));
    cx.update_window(handle, |_, window, cx| {
        click(window, cx, over_text, 1);
        wheel(
            window,
            cx,
            over_text,
            ScrollDelta::Lines(point(0., 3.)),
            false,
        );
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(view(&ws, cx, id).below, 3);
    let near_top = (f32::from(bar.center().x), f32::from(bar.top()) + 4.0);
    cx.update_window(handle, |_, window, cx| {
        down_at(window, cx, near_top);
        up_at(window, cx, near_top);
    })
    .unwrap();
    let back = view(&ws, cx, id);
    assert_eq!(back.below, back.history, "the track took the press");
}

#[gpui_kit::test]
fn jump_to_the_bottom_shows_the_live_screen_and_typing_does_too(cx: &mut TestAppContext) {
    let (handle, ws, id) = long_terminal(cx, 1.0);
    let n = id.0 as usize;
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(id).unwrap().term.shared().scroll_to(40)
    });
    assert!(drawn(handle, cx, "to-bottom", id));
    cx.update_window(handle, |_, window, cx| window.click(("to-bottom", n), cx))
        .unwrap();
    assert_eq!(view(&ws, cx, id).below, 0);
    assert!(!drawn(handle, cx, "to-bottom", id));
    ws.update(cx, |ws, _| ws.space.focused = Some(id));
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(id).unwrap().term.shared().scroll_to(5)
    });
    cx.update_window(handle, |_, window, cx| window.press("a", cx))
        .unwrap();
    assert_eq!(view(&ws, cx, id).below, 0, "typing went back");
}

/// Nothing over a full-screen program (it has no history) nor far away (the wheel moves the
/// canvas there); a program that asked for the mouse keeps its clicks until the person goes back
/// with Shift and the wheel.
#[gpui_kit::test]
fn no_scrollbar_full_screen_far_away_nor_over_a_program_s_mouse(cx: &mut TestAppContext) {
    let (handle, ws, id) = long_terminal(cx, 1.0);
    let feed = |cx: &mut TestAppContext, bytes: &'static [u8]| {
        ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.feed(bytes))
    };
    feed(cx, b"\x1b[?1049h");
    assert!(!drawn(handle, cx, "history-bar", id), "full screen");
    feed(cx, b"\x1b[?1049l");
    assert!(
        drawn(handle, cx, "history-bar", id),
        "back on the main screen"
    );
    feed(cx, b"\x1b[?1000h\x1b[?1006h");
    assert!(!drawn(handle, cx, "history-bar", id), "the program's mouse");
    let over_text = at(&ws, cx, (120.0, 160.0));
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(over_text.0), px(over_text.1)),
                delta: ScrollDelta::Lines(point(0., 2.)),
                modifiers: SHIFT,
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    })
    .unwrap();
    assert_eq!(view(&ws, cx, id).below, 2);
    assert!(drawn(handle, cx, "history-bar", id), "back in the history");
    ws.update(cx, |ws, _| {
        ws.space.focused = None;
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 0.1);
    });
    assert!(!drawn(handle, cx, "history-bar", id), "a card far away");
    assert!(!drawn(handle, cx, "to-bottom", id));
}
