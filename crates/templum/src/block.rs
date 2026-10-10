//! One terminal block, a continuous card: header (title, close button), body (the grid) and
//! resize grip, in screen space. Every size is multiplied by the zoom, so text is laid out at
//! its real size. The close button and the grip show only while the pointer is over the block.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, BoxShadow, ClickEvent, Context, ElementId, Hsla,
    MouseButton, MouseDownEvent, ScrollWheelEvent, TestSupportExt, div, point, px,
};
use templum_canvas::{Block, Rect, chrome};

use crate::grid::{self, hsla};
use crate::motion::Look;
use crate::workspace::{Pane, Workspace};

/// World units, like everything in a block.
const TITLE_FONT: f32 = 12.0;
const RADIUS: f32 = 10.0;
const FADE_IN: Duration = Duration::from_millis(120);

/// `look` scales the block around its center (it is drawn at zoom × scale) and fades it.
/// A block that isn't `live` is a ghost fading out: no controls, and clicks go through it.
pub(crate) fn render(
    ws: &Workspace,
    block: Block,
    pane: &Pane,
    look: Look,
    live: bool,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let id = block.id;
    let n = id.0 as usize;
    let z = ws.canvas.camera.zoom() * look.scale;
    let full = ws.canvas.camera.rect_to_screen(block.rect);
    let r = Rect::new(
        full.x + full.w * (1.0 - look.scale) / 2.0,
        full.y + full.h * (1.0 - look.scale) / 2.0,
        full.w * look.scale,
        full.h * look.scale,
    );
    let focused = live && ws.focused == Some(id);
    let hovered = live && ws.hovered == Some(id);
    let ui = ws.theme.ui;

    let title = div()
        .id(("title", n))
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        // Clear of the close button on the right, and centered.
        .px(px(28.0 * z))
        .overflow_hidden()
        .child(
            div()
                .truncate()
                .text_size(px(TITLE_FONT * z))
                .text_color(if focused { ui.title } else { ui.title_dim })
                .child(pane.term.title()),
        )
        .when(live, |el| {
            el.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, e: &MouseDownEvent, _, _| ws.begin_move(id, e.position)),
            )
        })
        .test_support();

    // Painted over the title: pressing it never starts a move.
    let close = div()
        .id(("close", n))
        .absolute()
        .top(px(4.0 * z))
        .right(px(4.0 * z))
        .flex()
        .items_center()
        .justify_center()
        .size(px(20.0 * z))
        .rounded(px(5.0 * z))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(
            Icon::new(IconName::Close)
                .size(px(12.0 * z))
                .text_color(ui.title),
        )
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.close(id, cx)))
        .test_support();

    let header = div()
        .relative()
        .w_full()
        .h(px(chrome::TITLE_H * z))
        .child(title)
        .when(hovered, |el| el.child(fade_in(close, ("close-fade", n))));

    let body =
        div()
            .id(("body", n))
            .absolute()
            .left(px(0.0))
            .top(px(chrome::TITLE_H * z))
            .w_full()
            .h(px((r.h - chrome::TITLE_H * z).max(0.0)))
            .p(px(chrome::PAD * z))
            .overflow_hidden()
            .when(live, |el| {
                el.cursor_text()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |ws, e: &MouseDownEvent, _, _| ws.begin_select(id, e)),
                    )
                    .on_scroll_wheel(cx.listener(move |ws, e: &ScrollWheelEvent, _, cx| {
                        ws.scroll_terminal(id, e, cx)
                    }))
            })
            .child(grid::body(
                pane.term.shared().clone(),
                pane.snapshot.clone(),
                ws.cell.clone(),
                z,
                focused,
                ws.marked.clone().filter(|_| focused),
                ws.theme.clone(),
                cx.entity(),
            ))
            .test_support();

    let handle = div()
        .id(("handle", n))
        .absolute()
        .right(px(0.0))
        .bottom(px(0.0))
        .size(px(chrome::HANDLE * z))
        .flex()
        .items_center()
        .justify_center()
        .cursor_nwse_resize()
        .child(
            Icon::new(IconName::ResizeCorner)
                .size(px(10.0 * z))
                .text_color(ui.title_dim),
        )
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

    let root: ElementId = if live {
        ("block", n).into()
    } else {
        ("ghost", n).into()
    };
    div()
        .id(root)
        .absolute()
        .left(px(r.x))
        .top(px(r.y))
        .w(px(r.w))
        .h(px(r.h))
        .opacity(look.opacity)
        .rounded(px(RADIUS * z))
        // Always 1 px, so focusing never moves the grid.
        .border_1()
        .border_color(if focused { ui.accent } else { ui.hairline })
        .bg(hsla(ws.theme.palette.background, 1.0))
        .shadow(vec![shadow(ui.shadow, focused, z)])
        .overflow_hidden()
        .when(live, |el| {
            // Nothing behind a block reacts to its clicks; wheel events still bubble
            // (title bar pans).
            el.block_mouse_except_scroll()
                .on_hover(
                    cx.listener(move |ws, hovered: &bool, _, cx| ws.hover_block(id, *hovered, cx)),
                )
                // Blocking hides the workspace from pinch (it hit-tests like a click): zoom
                // here too.
                .on_pinch(cx.listener(|ws, e, window, cx| ws.on_pinch(e, window, cx)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, _: &MouseDownEvent, window, cx| {
                        ws.focus_block(id, window, cx);
                        cx.stop_propagation();
                    }),
                )
        })
        .child(header)
        .child(body)
        .when(hovered, |el| el.child(fade_in(handle, ("handle-fade", n))))
        .test_support()
        .into_any_element()
}

/// Hover-only controls fade in; they disappear at once. The wrapper covers its parent, since
/// absolute children are placed relative to their direct parent.
fn fade_in(el: impl IntoElement + 'static, id: impl Into<ElementId>) -> impl IntoElement {
    div()
        .absolute()
        .inset_0()
        .child(el)
        .with_animation(id, Animation::new(FADE_IN), |el, t| el.opacity(t))
}

/// The focused block floats higher.
fn shadow(color: Hsla, focused: bool, z: f32) -> BoxShadow {
    let (y, blur) = if focused { (10.0, 30.0) } else { (4.0, 12.0) };
    BoxShadow {
        color,
        offset: point(px(0.0), px(y * z)),
        blur_radius: px(blur * z),
        spread_radius: px(0.0),
        inset: false,
    }
}
