//! The workspaces sidebar: a tile per workspace (click: show it; right-click: rename, delete),
//! and + for a new one. See docs/specs/workspaces.md.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, MouseButton, MouseDownEvent,
    SharedString, TestSupportExt, div, px,
};

use crate::grid::hsla;
use crate::menu::Target;
use crate::workspace::{Space, Workspace};

/// The sidebar's width; the canvas gets the rest.
pub(crate) const WIDTH: f32 = 56.0;
const TILE: f32 = 40.0;

pub(crate) fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    let order = ws.order();
    let tiles: Vec<AnyElement> = order
        .into_iter()
        .enumerate()
        .map(|(i, space)| tile(ws, i, space, cx))
        .collect();
    let plus = div()
        .id("new-workspace")
        .flex()
        .items_center()
        .justify_center()
        .size(px(TILE))
        .rounded(px(10.0))
        .border_1()
        .border_color(ui.hairline)
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(
            Icon::new(IconName::Plus)
                .size(px(16.0))
                .text_color(ui.title),
        )
        .tooltip(|window, cx| Tooltip::new("New workspace").build(window, cx))
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_space(window, cx)))
        .test_support();
    div()
        .id("sidebar")
        .flex()
        .flex_col()
        .items_center()
        .flex_shrink_0()
        .gap(px(8.0))
        .w(px(WIDTH))
        .h_full()
        .py(px(10.0))
        .border_r_1()
        .border_color(ui.hairline)
        .overflow_y_scroll()
        .children(tiles)
        .child(plus)
}

/// One workspace: its initials, an accent bar when active, a dot while an agent runs in it, and
/// one when a terminal there has news.
fn tile(ws: &Workspace, i: usize, space: &Space, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let active = i == ws.active;
    let name = SharedString::from(space.name.clone());
    let id = space.id.clone();
    let inner = div()
        .id(ElementId::Name(format!("workspace-{}", space.id).into()))
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .size(px(TILE))
        .rounded(px(10.0))
        .border_1()
        .text_size(px(14.0))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .map(|el| {
            if active {
                el.bg(ui.hover)
                    .border_color(ui.hairline)
                    .text_color(hsla(ws.theme.palette.foreground, 1.0))
            } else {
                el.border_color(gpui_kit::transparent_black())
                    .text_color(ui.title)
                    .hover(move |s| s.bg(ui.hover))
            }
        })
        .active(move |s| s.bg(ui.pressed))
        .child(initials(&space.name))
        .when(active, |el| {
            el.child(
                div()
                    .absolute()
                    .left(px(-8.0))
                    .top(px(10.0))
                    .w(px(3.0))
                    .h(px(18.0))
                    .rounded_full()
                    .bg(ui.accent),
            )
        })
        .when(space.has_agent(&ws.agents), |el| {
            el.child(
                div()
                    .absolute()
                    .top(px(4.0))
                    .right(px(4.0))
                    .size(px(7.0))
                    .rounded_full()
                    .bg(hsla(ws.theme.palette.ansi[2], 1.0)),
            )
        })
        // A terminal there has news (see alert.rs).
        .when(space.terminals.values().any(|p| p.alert.is_some()), |el| {
            el.child(
                div()
                    .id(ElementId::Name(
                        format!("workspace-alert-{}", space.id).into(),
                    ))
                    .absolute()
                    .bottom(px(4.0))
                    .right(px(4.0))
                    .size(px(7.0))
                    .rounded_full()
                    .bg(hsla(ws.theme.palette.ansi[3], 1.0))
                    .test_support(),
            )
        })
        .tooltip(move |window, cx| Tooltip::new(name.clone()).build(window, cx))
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| ws.switch_to(i, window, cx)))
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                ws.open_menu(Target::Space(id.clone()), e.position, window, cx);
                cx.stop_propagation();
            }),
        )
        .test_support();
    inner.into_any_element()
}

/// "Side project" is "SP", "main" is "M".
pub(crate) fn initials(name: &str) -> String {
    let mut words = name.split_whitespace();
    let first = |w: Option<&str>| w.and_then(|w| w.chars().next());
    first(words.next())
        .into_iter()
        .chain(first(words.next()))
        .flat_map(char::to_uppercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_the_first_two_words() {
        assert_eq!(initials("Main"), "M");
        assert_eq!(initials("side project"), "SP");
        assert_eq!(initials("  a  b  c "), "AB");
        assert_eq!(initials("éclair"), "É");
        assert_eq!(initials(""), "");
    }
}
