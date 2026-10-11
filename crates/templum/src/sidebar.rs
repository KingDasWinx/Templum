//! The workspaces sidebar: a tile per workspace (click: show it; right-click: rename, delete),
//! and + for a new one. See docs/specs/workspaces.md.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, SharedString, TestSupportExt, Window,
    div, px,
};

use crate::grid::hsla;
use crate::prompt;
use crate::workspace::{Space, Workspace};

/// The sidebar's width; the canvas gets the rest.
pub(crate) const WIDTH: f32 = 56.0;
const TILE: f32 = 40.0;

pub(crate) fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    let order = ws.order();
    let only_one = order.len() == 1;
    let tiles: Vec<AnyElement> = order
        .into_iter()
        .enumerate()
        .map(|(i, space)| tile(ws, i, space, only_one, cx))
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

/// One workspace: its initials, an accent bar when active, a dot while an agent runs in it.
fn tile(
    ws: &Workspace,
    i: usize,
    space: &Space,
    only_one: bool,
    cx: &mut Context<Workspace>,
) -> AnyElement {
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
        .when(space.has_agent(), |el| {
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
        .tooltip({
            let name = name.clone();
            move |window, cx| Tooltip::new(name.clone()).build(window, cx)
        })
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| ws.switch_to(i, window, cx)))
        .test_support();
    let entity = cx.entity();
    // The menu sits on a wrapper: an element observed for tests can't carry one.
    div()
        .id(ElementId::Name(
            format!("workspace-menu-{}", space.id).into(),
        ))
        .child(inner)
        .context_menu(move |menu, _, _| {
            let (rename, delete) = (entity.clone(), entity.clone());
            let (rename_id, delete_id) = (id.clone(), id.clone());
            let (rename_name, delete_name) = (name.clone(), name.clone());
            menu.item(
                PopupMenuItem::new("Rename…").on_click(move |_, window, cx| {
                    rename.update(cx, |_, cx| {
                        ask_rename(rename_id.clone(), rename_name.clone(), window, cx)
                    })
                }),
            )
            .item(PopupMenuItem::new("Delete…").disabled(only_one).on_click(
                move |_, window, cx| {
                    delete.update(cx, |_, cx| {
                        ask_delete(delete_id.clone(), delete_name.clone(), window, cx)
                    })
                },
            ))
        })
        .into_any_element()
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

/// Enter or Rename applies the name; an empty one keeps the dialog open.
fn ask_rename(id: String, name: SharedString, window: &mut Window, cx: &mut Context<Workspace>) {
    let ws = cx.entity();
    prompt::ask_text(
        "Rename workspace",
        "Rename",
        name,
        move |text, window, cx| {
            ws.update(cx, |ws, cx| ws.rename_space(&id, &text, window, cx))
                .is_ok()
        },
        window,
        cx,
    );
}

fn ask_delete(id: String, name: SharedString, window: &mut Window, cx: &mut Context<Workspace>) {
    let ws = cx.entity();
    prompt::confirm(
        format!("Delete {name}?"),
        "Its terminals will close.",
        "Delete",
        move |window, cx| ws.update(cx, |ws, cx| ws.delete_space(&id, window, cx)),
        window,
        cx,
    );
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
