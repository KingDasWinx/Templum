//! One terminal block: title bar, body (the grid) and resize handle, in screen space.
//! Every size is multiplied by the zoom, so text is laid out at its real size.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, MouseButton, MouseDownEvent, ScrollWheelEvent, TestSupportExt,
    div, px, rgb,
};
use templum_canvas::{Block, chrome};

use crate::grid;
use crate::workspace::Workspace;

const BORDER: u32 = 0x3a3a40;
const BORDER_FOCUSED: u32 = 0x6a9fb5;
const TITLE_BG: u32 = 0x26262c;
const TITLE_FG: u32 = 0xb0b0b8;
const CLOSE_HOVER: u32 = 0x3a3a40;
/// World units, like everything in a block.
const TITLE_FONT: f32 = 12.0;

pub(crate) fn render(ws: &Workspace, block: Block, cx: &mut Context<Workspace>) -> AnyElement {
    let id = block.id;
    let n = id.0 as usize;
    let z = ws.canvas.camera.zoom();
    let r = ws.canvas.camera.rect_to_screen(block.rect);
    let pane = &ws.terminals[&id];
    let focused = ws.focused == Some(id);
    let border = if focused { BORDER_FOCUSED } else { BORDER };

    let title = div()
        .id(("title", n))
        .flex_1()
        .h_full()
        .flex()
        .items_center()
        .px(px(8.0 * z))
        .overflow_hidden()
        .child(
            div()
                .truncate()
                .text_size(px(TITLE_FONT * z))
                .text_color(rgb(TITLE_FG))
                .child(pane.term.title()),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, e: &MouseDownEvent, _, _| ws.begin_move(id, e.position)),
        )
        .test_support();

    // A sibling of the title, not a child: pressing it never starts a move.
    let close = div()
        .id(("close", n))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(20.0 * z))
        .mr(px(4.0 * z))
        .rounded(px(4.0 * z))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(CLOSE_HOVER)))
        .child(
            Icon::new(IconName::Close)
                .size(px(12.0 * z))
                .text_color(rgb(TITLE_FG)),
        )
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.close(id, cx)))
        .test_support();

    let title_bar = div()
        .flex()
        .items_center()
        .w_full()
        .h(px(chrome::TITLE_H * z))
        .bg(rgb(TITLE_BG))
        .child(title)
        .child(close);

    let body = div()
        .id(("body", n))
        .absolute()
        .left(px(0.0))
        .top(px(chrome::TITLE_H * z))
        .w_full()
        .h(px((r.h - chrome::TITLE_H * z).max(0.0)))
        .p(px(chrome::PAD * z))
        .overflow_hidden()
        .cursor_text()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, e: &MouseDownEvent, _, _| ws.begin_select(id, e)),
        )
        .on_scroll_wheel(
            cx.listener(move |ws, e: &ScrollWheelEvent, _, cx| ws.scroll_terminal(id, e, cx)),
        )
        .child(grid::body(
            pane.term.shared().clone(),
            pane.snapshot.clone(),
            ws.cell.clone(),
            z,
            focused,
            ws.marked.clone().filter(|_| focused),
            cx.entity(),
        ))
        .test_support();

    let handle = div()
        .id(("handle", n))
        .absolute()
        .right(px(0.0))
        .bottom(px(0.0))
        .size(px(chrome::HANDLE * z))
        .bg(rgb(border))
        .rounded(px(2.0 * z))
        .cursor_nwse_resize()
        .on_mouse_down(
            MouseButton::Left,
            // The handle overlaps the body: stop here so the body doesn't start a selection.
            cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                ws.focus_block(id, window, cx);
                ws.begin_resize(id, e.position);
                cx.stop_propagation();
            }),
        )
        .test_support();

    div()
        .id(("block", n))
        .absolute()
        .left(px(r.x))
        .top(px(r.y))
        .w(px(r.w))
        .h(px(r.h))
        .rounded(px(6.0 * z))
        .when(focused, |el| el.border_2())
        .when(!focused, |el| el.border_1())
        .border_color(rgb(border))
        .bg(grid::hsla(
            templum_terminal::Palette::default().background,
            1.0,
        ))
        .overflow_hidden()
        // Nothing behind a block reacts to its clicks; wheel events still bubble (title bar pans).
        .block_mouse_except_scroll()
        // Blocking hides the workspace from pinch (it hit-tests like a click): zoom here too.
        .on_pinch(cx.listener(|ws, e, window, cx| ws.on_pinch(e, window, cx)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                ws.focus_block(id, window, cx);
                cx.stop_propagation();
            }),
        )
        .child(title_bar)
        .child(body)
        .child(handle)
        .test_support()
        .into_any_element()
}
