//! One block, a continuous card: header (title, close button), body (a terminal's grid, or a
//! kanban board) and resize grip, in screen space. Every size is multiplied by the zoom, so text
//! is laid out at its real size. The close button and the grip show only while the pointer is
//! over the block.

use gpui_kit::assets::IconName;
use gpui_kit::base::animation::Lerp as _;
use gpui_kit::base::motion::{Transition, transition};
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, BoxShadow, ClickEvent, Context, ElementId, Hsla,
    MouseButton, MouseDownEvent, ScrollWheelEvent, SharedString, TestSupportExt, Window, div,
    point, px,
};
use templum_canvas::{Block, Rect, chrome};

use crate::alert::{self, Alert};
use crate::grid::{self, hsla};
use crate::kanban;
use crate::menu::Target;
use crate::motion::{self, Look};
use crate::workspace::{Body, Pane, Workspace};

/// World units, like everything in a block.
const TITLE_FONT: f32 = 12.0;
const RADIUS: f32 = 10.0;
/// The focused block's ring, outside its border.
const RING: f32 = 2.0;

/// `look` scales the block around its center (it is drawn at zoom × scale) and fades it.
/// A block that isn't `live` is a ghost fading out: no controls, and clicks go through it.
pub(crate) fn render(
    ws: &Workspace,
    block: Block,
    body: &Body,
    look: Look,
    live: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let id = block.id;
    let n = id.0 as usize;
    let z = ws.space.view.zoom() * look.scale;
    let full = ws.space.view.rect_to_screen(block.rect);
    let r = Rect::new(
        full.x + full.w * (1.0 - look.scale) / 2.0,
        full.y + full.h * (1.0 - look.scale) / 2.0,
        full.w * look.scale,
        full.h * look.scale,
    );
    let focused = live && ws.space.focused == Some(id);
    let target = Target::Block(id);
    let hovered = live && ws.space.hovered == Some(id);
    let ui = ws.ui;
    // 0 at rest, 1 hovered; eased both ways.
    let lift = transition(
        ElementId::from(("lift", n)),
        if hovered { 1.0_f32 } else { 0.0 },
        Transition::new(motion::HOVER),
        window,
        cx,
    );
    let mut shadows = vec![shadow(ui.shadow, focused, lift, z)];
    if focused {
        shadows.push(BoxShadow {
            color: ui.ring,
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(RING * z),
            inset: false,
        });
    }

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
                .child(match body {
                    Body::Terminal(pane) => SharedString::from(pane.term.title()),
                    Body::Kanban(k) => k.title(ws),
                }),
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

    // A terminal's state, on the left of its header.
    let dot = match body {
        Body::Terminal(pane) if live => status_dot(ws, pane).map(|(color, text)| {
            div()
                .id(("status", n))
                .absolute()
                .top(px((chrome::TITLE_H - 7.0) / 2.0 * z))
                .left(px(12.0 * z))
                .size(px(7.0 * z))
                .rounded_full()
                .bg(color)
                .tooltip(move |window, cx| Tooltip::new(text.clone()).build(window, cx))
                .test_support()
        }),
        _ => None,
    };

    // A kanban block's board list, on the left of its header (over the title: no move).
    let boards = match body {
        Body::Kanban(_) if live => Some(
            div()
                .id(("boards", n))
                .absolute()
                .top(px(4.0 * z))
                .left(px(4.0 * z))
                .flex()
                .items_center()
                .justify_center()
                .size(px(20.0 * z))
                .rounded(px(5.0 * z))
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .child(
                    Icon::new(IconName::ChevronsUpDown)
                        .size(px(12.0 * z))
                        .text_color(ui.title),
                )
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.pick_board(id, cx)))
                .test_support(),
        ),
        _ => None,
    };
    let header = div()
        .relative()
        .w_full()
        .h(px(chrome::TITLE_H * z))
        .child(title)
        .children(dot)
        .children(boards)
        .when(hovered, |el| el.child(fade_in(close, ("close-fade", n))));

    let frame = div()
        .id(("body", n))
        .absolute()
        .left(px(0.0))
        .top(px(chrome::TITLE_H * z))
        .w_full()
        .h(px((r.h - chrome::TITLE_H * z).max(0.0)))
        .p(px(chrome::PAD * z))
        .overflow_hidden();
    let content = match body {
        Body::Terminal(pane) => frame
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
                ws.blink_on,
                cx.entity(),
            )),
        // A fading kanban block is drawn empty: its board is still live elsewhere.
        Body::Kanban(k) => frame.when(live, |el| el.child(kanban::body(ws, id, *k, z, cx))),
    }
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
        .border_color(if focused {
            ui.accent
        } else {
            ui.hairline.lerp(&ui.hover_border, lift)
        })
        .bg(ws.ui.surface)
        .shadow(shadows)
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
                // The middle button moves the canvas, over blocks too.
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(|ws, e: &MouseDownEvent, _, cx| {
                        ws.begin_pan(e.position);
                        cx.stop_propagation();
                    }),
                )
                // Its menu acts on it: it is focused first (copy, paste).
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                        ws.focus_block(id, window, cx);
                        ws.open_menu(target.clone(), e.position, window, cx);
                        cx.stop_propagation();
                    }),
                )
        })
        .child(header)
        .child(content)
        .when(hovered, |el| el.child(fade_in(handle, ("handle-fade", n))))
        .test_support()
        .into_any_element()
}

/// Hover-only controls fade in; they disappear at once. The wrapper covers its parent, since
/// absolute children are placed relative to their direct parent.
fn fade_in(el: impl IntoElement + 'static, id: impl Into<ElementId>) -> impl IntoElement {
    div().absolute().inset_0().child(el).with_animation(
        id,
        Animation::new(motion::HOVER),
        |el, t| el.opacity(t),
    )
}

/// The drop shadow's offset and blur (world units): the focused block floats higher, and a
/// hovered one (`lift` 1) rises a little more.
fn lift_metrics(focused: bool, lift: f32) -> (f32, f32) {
    let (y, blur) = if focused { (10.0, 30.0) } else { (4.0, 12.0) };
    (y + 3.0 * lift, blur + 8.0 * lift)
}

fn shadow(color: Hsla, focused: bool, lift: f32, z: f32) -> BoxShadow {
    let (y, blur) = lift_metrics(focused, lift);
    BoxShadow {
        color,
        offset: point(px(0.0), px(y * z)),
        blur_radius: px(blur * z),
        spread_radius: px(0.0),
        inset: false,
    }
}

/// The dot before a terminal's title, and what it says. News first: yellow when it asked for
/// attention, red or green when a long command ended. Then a command running, then a last command
/// that failed. Nothing when the shell doesn't say (no shell integration).
fn status_dot(ws: &Workspace, pane: &Pane) -> Option<(Hsla, SharedString)> {
    let ansi = |i: usize| hsla(ws.theme.palette.ansi[i], 1.0);
    let (color, text) = match &pane.alert {
        Some(a @ Alert::Attention(_)) => (ansi(3), a.text()),
        Some(a) if a.failed() => (ansi(1), a.text()),
        Some(a) => (ansi(2), a.text()),
        None => {
            let status = pane.term.status();
            match (status.running, status.last) {
                (Some(since), _) => (
                    ws.ui.accent,
                    format!("Running for {}", alert::took(since.elapsed())),
                ),
                (None, Some(last)) if last.code.is_some_and(|c| c != 0) => {
                    (ansi(1), alert::finished(&last))
                }
                _ => return None,
            }
        }
    };
    Some((color, text.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hovered_block_lifts() {
        assert_eq!(lift_metrics(false, 0.0), (4.0, 12.0));
        assert_eq!(lift_metrics(false, 1.0), (7.0, 20.0));
        assert_eq!(lift_metrics(true, 0.0), (10.0, 30.0));
        assert_eq!(lift_metrics(true, 1.0), (13.0, 38.0));
    }
}
