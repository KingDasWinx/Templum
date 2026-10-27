//! The popups over the canvas's bottom-left corner — Git, Requests, Agents and Automations —
//! drawn one way (docs/specs/design-system.md, Popups): the frame, the header across its width,
//! the slot (its place, its resize edges, the backdrop a press outside closes it on), full screen
//! and a form's layout. Window chrome: the canvas's zoom never scales them. Only one is open at a
//! time, and they share one size (`Workspace::git_popup_size`). A dialog in the middle of the
//! window (`dialog`) wears the same frame, header and foot.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::dialog::{Dialog, DialogEntrance};
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, BoxShadow, ClickEvent, Context, Div, ElementId, Entity, Focusable as _,
    FontWeight, Hsla, MouseButton, MouseDownEvent, SharedString, Stateful, TestSupportExt, Window,
    div, point, px, transparent_black,
};
use templum_canvas::Edges;

use crate::git_block::widgets::icon_button;
use crate::grid::hsla;
use crate::sidebar;
use crate::theme::Ui;
use crate::workspace::Workspace;

/// Between a popup and the canvas's edges and the sidebar, pixels.
pub(crate) const GAP: f32 = 8.0;
/// The header's height: the icon's tile, its padding and the hairline under it.
pub(crate) const HEADER: f32 = 55.0;
/// The widest a form's or a detail's content grows: full screen leaves the rest at its sides.
pub(crate) const CONTENT_MAX: f32 = 680.0;
/// A form's label column, and the height of a small field a label sits beside.
pub(crate) const LABEL_W: f32 = 92.0;
pub(crate) const FIELD_H: f32 = 24.0;
/// A dialog's width: a short form's.
pub(crate) const DIALOG_W: f32 = 520.0;
/// The frame's corners.
const RADIUS: f32 = 12.0;
/// What is said about things: a subtitle, a form's labels.
const SMALL: f32 = 11.5;
/// A search field's height: the Home's header actions'.
const SEARCH_H: f32 = 28.0;

/// The popup's box, `w` × `h` pixels, a column: the header, then its body. Presses inside never
/// reach the canvas below.
pub(crate) fn frame(id: &'static str, ws: &Workspace, w: f32, h: f32) -> Stateful<Div> {
    surface(id, ws).w(px(w)).h(px(h))
}

/// The frame without its size: a dialog's is as high as what it holds.
fn surface(id: &'static str, ws: &Workspace) -> Stateful<Div> {
    let ui = ws.ui;
    div()
        .id(id)
        .flex()
        .flex_col()
        .overflow_hidden()
        .bg(ui.surface)
        .text_color(hsla(ws.theme.palette.foreground, 1.0))
        .border_1()
        .border_color(ui.hairline)
        .rounded(px(RADIUS))
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
    head(ws, name, tile, title, subtitle)
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

/// What every header holds before its buttons: `tile`, the title over its `subtitle`, then the
/// room that pushes the buttons to the right.
fn head(
    ws: &Workspace,
    name: &'static str,
    tile: Div,
    title: impl IntoElement,
    subtitle: Option<AnyElement>,
) -> Stateful<Div> {
    div()
        .id(ElementId::Name(format!("{name}-header").into()))
        .flex()
        .items_center()
        .flex_shrink_0()
        .h(px(HEADER))
        .gap(px(10.0))
        .px(px(12.0))
        .border_b_1()
        .border_color(ws.ui.hairline)
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
}

/// `popup` in a dialog in the middle of the window: gpui-kit's `Dialog` (opened with
/// `window.open_dialog`) keeps the behavior — the backdrop a press on closes it, Esc, Enter for
/// its `on_ok`, the keyboard kept in it and given back after — shows at once and draws nothing
/// of its own around `popup`, a `dialog_frame`.
pub(crate) fn dialog(dialog: Dialog, popup: impl IntoElement) -> Dialog {
    dialog
        .w(px(DIALOG_W))
        .close_button(false)
        .entrance(DialogEntrance::None)
        .p_0()
        .gap_0()
        .border_0()
        .bg(transparent_black())
        // Its shadow, which it always draws, then follows the frame's corners.
        .rounded(px(RADIUS))
        .child(popup)
}

/// A dialog's box, `DIALOG_W` wide and as high as what it holds: the popups' frame.
pub(crate) fn dialog_frame(id: &'static str, ws: &Workspace) -> Stateful<Div> {
    surface(id, ws).w(px(DIALOG_W))
}

/// A dialog's header: the popups' without Full screen (a short form needs no more room), its
/// Close doing `close`, what Esc does. Ids: `{name}-header`, `{name}-close`.
pub(crate) fn dialog_header(
    ws: &Workspace,
    name: &'static str,
    tile: Div,
    title: impl IntoElement,
    subtitle: Option<AnyElement>,
    close: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    head(ws, name, tile, title, subtitle).child(
        icon_button(
            ElementId::Name(format!("{name}-close").into()),
            IconName::Close,
            ws.ui,
            1.0,
        )
        .tooltip(|window, cx| Tooltip::new("Close (Esc)").build(window, cx))
        .on_click(close)
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

/// A column of a body that grows to `CONTENT_MAX` and stays centered past it.
pub(crate) fn content() -> Div {
    div()
        .w_full()
        .max_w(px(CONTENT_MAX))
        .mx_auto()
        .flex()
        .flex_col()
}

/// A form's section: its name over its rows.
pub(crate) fn section(ws: &Workspace, name: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .child(crate::inbox::section(ws, name))
}

/// A form's row: `label` in the label column, `field` beside it; what the field holds wraps
/// under itself, never under the label.
pub(crate) fn row(ws: &Workspace, label: impl Into<SharedString>, field: impl IntoElement) -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.0))
        .child(
            div()
                .w(px(LABEL_W))
                .h(px(FIELD_H))
                .flex_shrink_0()
                .flex()
                .items_center()
                .text_size(px(SMALL))
                .text_color(ws.ui.title_dim)
                .child(label.into()),
        )
        .child(div().flex_1().min_w(px(0.0)).child(field))
}

/// Related controls on one line of a row, wrapping onto the next when the column is narrow.
pub(crate) fn line() -> Div {
    div().flex().flex_wrap().items_center().gap(px(8.0))
}

/// The foot under a body, which stays while the body scrolls: a hairline over its actions.
pub(crate) fn foot(ws: &Workspace) -> Div {
    div()
        .flex_shrink_0()
        .py(px(10.0))
        .border_t_1()
        .border_color(ws.ui.hairline)
}

/// A search field (design-system.md, Search fields), the Home's and the Agents popup's: quiet at
/// rest (the cards' surface and hairline), the search icon before its words, its clear button
/// only with text. While it has the keyboard its border is the soft ring, never the kit's accent
/// ring: it has the keyboard most of the time.
pub(crate) fn search(ui: Ui, field: &Entity<InputState>, window: &Window, cx: &App) -> Input {
    let focused = field.read(cx).focus_handle(cx).is_focused(window);
    let input = Input::new(field)
        .cleanable(true)
        .focus_bordered(false)
        .prefix(
            Icon::new(IconName::Search)
                .size(px(14.0))
                .text_color(ui.title_dim),
        )
        .py(px(2.0))
        .text_size(px(13.0))
        .rounded(px(8.0))
        .bg(ui.surface)
        .border_color(if focused { ui.ring } else { ui.hairline });
    // Styled's height: Input's own `h` is for multi-line fields only.
    Styled::h(input, px(SEARCH_H))
}
