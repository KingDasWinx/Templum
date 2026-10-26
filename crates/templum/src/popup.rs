//! The popups over the canvas's bottom-left corner — Git, Requests, Agents and Automations —
//! drawn one way (docs/specs/design-system.md, Popups): the frame, the header across its width,
//! the slot (its place, its resize edges, the backdrop a press outside closes it on) and full
//! screen. Window chrome: the canvas's zoom never scales them. Only one is open at a time, and
//! they share one size (`Workspace::git_popup_size`).

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, ElementId, FontWeight, Hsla, MouseButton,
    MouseDownEvent, SharedString, Stateful, TestSupportExt, div, point, px,
};
use templum_canvas::Edges;

use crate::git_block::widgets::icon_button;
use crate::grid::hsla;
use crate::sidebar;
use crate::workspace::Workspace;

/// Between a popup and the canvas's edges and the sidebar, pixels.
pub(crate) const GAP: f32 = 8.0;
/// The header's height: the icon's tile, its padding and the hairline under it.
pub(crate) const HEADER: f32 = 55.0;

/// The popup's box, `w` × `h` pixels, a column: the header, then its body. Presses inside never
/// reach the canvas below.
pub(crate) fn frame(id: &'static str, ws: &Workspace, w: f32, h: f32) -> Stateful<Div> {
    let ui = ws.ui;
    div()
        .id(id)
        .w(px(w))
        .h(px(h))
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(ui.surface)
        .text_color(hsla(ws.theme.palette.foreground, 1.0))
        .border_1()
        .border_color(ui.hairline)
        .rounded(px(12.0))
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(0.0), px(12.0)),
            blur_radius: px(32.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .occlude()
}

/// The header's icon: `icon` in `color` on a tile of a faint fill of it.
pub(crate) fn tile(icon: IconName, color: Hsla) -> Div {
    div()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .size(px(34.0))
        .rounded(px(9.0))
        .bg(color.opacity(0.14))
        .child(Icon::new(icon).size(px(17.0)).text_color(color))
}

/// A header's dim line under its title.
pub(crate) fn subtitle(ws: &Workspace, text: impl Into<SharedString>) -> AnyElement {
    div()
        .min_w(px(0.0))
        .truncate()
        .text_size(px(12.0))
        .text_color(ws.ui.title_dim)
        .child(text.into())
        .into_any_element()
}

/// The header across the popup's width, `HEADER` high: `tile`, the title over its `subtitle`,
/// the popup's own `actions`, then Full screen and Close (Esc). `name` makes its ids:
/// `{name}-header`, `{name}-close`.
pub(crate) fn header(
    ws: &Workspace,
    name: &'static str,
    tile: Div,
    title: impl IntoElement,
    subtitle: Option<AnyElement>,
    actions: Vec<AnyElement>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let ui = ws.ui;
    let full = ws.popup_full;
    let (icon, tip) = if full {
        (IconName::Minimize2, "Exit full screen")
    } else {
        (IconName::Maximize2, "Full screen")
    };
    div()
        .id(ElementId::Name(format!("{name}-header").into()))
        .flex()
        .items_center()
        .flex_shrink_0()
        .h(px(HEADER))
        .gap(px(10.0))
        .px(px(12.0))
        .border_b_1()
        .border_color(ui.hairline)
        .child(tile)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .min_w(px(0.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(14.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title),
                )
                .children(subtitle),
        )
        .child(div().flex_1())
        .children(actions)
        .child(
            icon_button("popup-full", icon, ui, 1.0)
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| {
                    ws.popup_full = !ws.popup_full;
                    cx.notify();
                }))
                .test_support(),
        )
        .child(
            icon_button(
                ElementId::Name(format!("{name}-close").into()),
                IconName::Close,
                ui,
                1.0,
            )
            .tooltip(|window, cx| Tooltip::new("Close (Esc)").build(window, cx))
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.close_popup(window, cx)))
            .test_support(),
        )
}

/// `popup`, `w` × `h` pixels, over the canvas's bottom-left corner beside the sidebar, with its
/// resize edges (none at full screen), over a backdrop: a press anywhere outside it closes it,
/// and nothing below gets the press. The sidebar stays out of the backdrop: its buttons switch
/// from one popup to another, and the open one's closes it.
pub(crate) fn slot(
    ws: &Workspace,
    w: f32,
    h: f32,
    popup: AnyElement,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let placed = div()
        .absolute()
        .left(px(GAP))
        .bottom(px(GAP))
        .w(px(w))
        .h(px(h))
        // Presses inside the popup don't reach the backdrop.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(popup)
        .when(!ws.popup_full, |el| el.children(resize_zones(cx)));
    div()
        .id("popup-backdrop")
        .absolute()
        .top_0()
        .bottom_0()
        .right_0()
        .left(px(if ws.sidebar { sidebar::WIDTH } else { 0.0 }))
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, _: &MouseDownEvent, window, cx| {
                ws.close_popup(window, cx);
                cx.stop_propagation();
            }),
        )
        .child(placed)
}

/// How far the resize strips reach out of the popup and into it, pixels: outside it they take
/// nothing the canvas needs, inside it they leave the diff's scrollbar its width.
const EDGE_OUT: f32 = 6.0;
const EDGE_IN: f32 = 2.0;

/// Invisible strips on the popup's free edges, the right and the top, and a square at the corner
/// between them, that resize it like a window's border (children of the popup's box, which they
/// stick out of). The left edge rests against the sidebar and the bottom one against the
/// window's, so they have none.
fn resize_zones(cx: &mut Context<Workspace>) -> [AnyElement; 3] {
    let thick = EDGE_OUT + EDGE_IN;
    // Along an edge, clear of the corner's square.
    let clear = 2.0 * thick - EDGE_OUT;
    [
        zone("popup-resize-right", Edges::RIGHT, cx)
            .right(px(-EDGE_OUT))
            .w(px(thick))
            .top(px(clear))
            .bottom(px(0.0))
            .cursor_ew_resize()
            .test_support()
            .into_any_element(),
        zone("popup-resize-top", Edges::TOP, cx)
            .top(px(-EDGE_OUT))
            .h(px(thick))
            .left(px(0.0))
            .right(px(clear))
            .cursor_ns_resize()
            .test_support()
            .into_any_element(),
        zone("popup-resize-top-right", Edges::TOP_RIGHT, cx)
            .top(px(-EDGE_OUT))
            .right(px(-EDGE_OUT))
            .size(px(2.0 * thick))
            .cursor_nesw_resize()
            .test_support()
            .into_any_element(),
    ]
}

/// A strip that starts the resize by `edges` when pressed.
fn zone(name: &'static str, edges: Edges, cx: &mut Context<Workspace>) -> Stateful<Div> {
    div().id(name).absolute().on_mouse_down(
        MouseButton::Left,
        cx.listener(move |ws, e: &MouseDownEvent, _, cx| {
            ws.begin_git_resize(edges, e.position);
            cx.stop_propagation();
        }),
    )
}
