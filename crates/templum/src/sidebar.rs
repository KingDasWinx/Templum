//! The workspaces sidebar: the Home button, a tile per workspace (click: show it; right-click:
//! rename, delete), and + for a new one; the agents, requests and Git buttons at its foot. See
//! docs/specs/workspaces.md.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, ElementId, FontWeight, Hsla, MouseButton, MouseDownEvent,
    ObjectFit, SharedString, TestSupportExt, Window, div, img, px,
};

use crate::grid::hsla;
use crate::menu::Target;
use crate::space_dialog::{Look, tint};
use crate::theme::{Theme, Ui};
use crate::workspace::remote::Deed;
use crate::workspace::{Space, Workspace};

/// The sidebar's width; the canvas gets the rest.
pub(crate) const WIDTH: f32 = 56.0;
const TILE: f32 = 40.0;

pub(crate) fn render(
    ws: &Workspace,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> impl IntoElement {
    let ui = ws.ui;
    // A request just arrived: the inbox button pulses, a frame at a time, for a few seconds.
    let now = cx.background_executor().now();
    let pulse = match ws.inbox_pulse {
        Some(at) if ws.inbox_pulsing(now) => {
            window.request_animation_frame();
            crate::inbox::pulse(now.saturating_duration_since(at))
        }
        _ => 0.0,
    };
    let order = ws.order();
    let tiles: Vec<AnyElement> = order
        .into_iter()
        .enumerate()
        .map(|(i, space)| tile(ws, i, space, cx))
        .collect();
    // Shown, it is marked as the workspace shown is.
    let at_home = ws.home_up();
    let home = div()
        .id("home-button")
        .relative()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(TILE))
        .rounded(px(10.0))
        .border_1()
        .cursor_pointer()
        .map(|el| shown_or_not(el, at_home, ui))
        .active(move |s| s.bg(ui.pressed))
        .child(
            Icon::new(IconName::House)
                .size(px(16.0))
                .text_color(if at_home {
                    hsla(ws.theme.palette.foreground, 1.0)
                } else {
                    ui.title
                }),
        )
        .when(at_home, |el| el.child(shown_bar(ui.accent)))
        .tooltip(|window, cx| Tooltip::new("Home").build(window, cx))
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.show_home(window, cx)))
        .test_support();
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
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.add_space(window, cx)))
        .test_support();
    let connect = div()
        .id("connect-templum")
        .flex()
        .items_center()
        .justify_center()
        .size(px(TILE))
        .rounded(px(10.0))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(
            Icon::new(IconName::Plug)
                .size(px(16.0))
                .text_color(ui.title),
        )
        .tooltip(|window, cx| Tooltip::new("Connect to a Templum…").build(window, cx))
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.connect_templum(window, cx)))
        .test_support();
    let open = ws.git_open && ws.space.git.is_some();
    // On a mirror the button is dimmed, with the rule's words, while home can't be asked.
    let (git_tip, git_dim) = crate::toolbar::gated(ws, Deed::Git("look"), "Git");
    let git = div()
        .id("git-button")
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(TILE))
        .rounded(px(10.0))
        .cursor_pointer()
        .when(open, move |el| el.bg(ui.pressed))
        .when(git_dim, |el| el.opacity(0.4))
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(
            Icon::new(IconName::GitBranch)
                .size(px(16.0))
                .text_color(ui.title),
        )
        .tooltip(move |window, cx| Tooltip::new(git_tip.clone()).build(window, cx))
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.toggle_git(window, cx)))
        .test_support();
    div()
        .id("sidebar")
        .flex()
        .flex_col()
        .items_center()
        .flex_shrink_0()
        .w(px(WIDTH))
        .h_full()
        .py(px(10.0))
        .border_r_1()
        .border_color(ui.hairline)
        .child(
            div()
                .id("sidebar-workspaces")
                .flex()
                .flex_col()
                .items_center()
                .gap(px(8.0))
                .flex_1()
                .min_h(px(0.0))
                .w_full()
                .overflow_y_scroll()
                .child(home)
                .children(tiles)
                .child(plus)
                .child(connect),
        )
        // Apart, in the corner: the agents, requests and Git aren't workspaces.
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.0))
                .pt(px(8.0))
                .mt(px(8.0))
                .border_t_1()
                .border_color(ui.hairline)
                .child(crate::agents_view::button(ws, TILE, cx))
                .child(crate::inbox::button(ws, pulse, TILE, cx))
                .child(crate::automations_view::button(ws, TILE, cx))
                .child(git),
        )
}

/// What the sidebar shows (the Home or a workspace) has the hover fill and a hairline; the others
/// get the fill under the pointer.
fn shown_or_not(
    el: gpui_kit::Stateful<gpui_kit::Div>,
    shown: bool,
    ui: Ui,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    if shown {
        el.bg(ui.hover).border_color(ui.hairline)
    } else {
        el.border_color(gpui_kit::transparent_black())
            .hover(move |s| s.bg(ui.hover))
    }
}

/// The bar at the left of what the sidebar shows, in `color`.
fn shown_bar(color: Hsla) -> impl IntoElement {
    div()
        .id("shown-bar")
        .absolute()
        .left(px(-8.0))
        .top(px(10.0))
        .w(px(3.0))
        .h(px(18.0))
        .rounded_full()
        .bg(color)
        .test_support()
}

/// One workspace: its face, a bar when active (in its color, or the accent), a dot while an
/// agent runs in it, and one when a terminal there has news. Its color tints it.
fn tile(ws: &Workspace, i: usize, space: &Space, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    // On the Home no workspace is shown.
    let active = i == ws.active && !ws.home_up();
    let name = SharedString::from(if let Some(remote) = space.remote.as_deref() {
        crate::mirror::tile_tip(&space.name, remote)
    } else if space.stopped {
        format!("{} (stopped)", space.name)
    } else {
        space.name.clone()
    });
    let id = space.id.clone();
    let color = space.look.color.as_deref().and_then(|c| tint(c, &ws.theme));
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let inner = div().id(ElementId::Name(format!("workspace-{}", space.id).into()));
    let inner = crate::home::draggable(inner, space, i, ui, cx)
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
        .map(|el| shown_or_not(el, active, ui))
        .text_color(if active { fg } else { ui.title })
        .active(move |s| s.bg(ui.pressed))
        .child(face(
            &space.look,
            &space.name,
            TILE - 2.0,
            (TILE * 0.45).round(),
            &ws.theme,
            if active { fg } else { ui.title },
        ))
        .when(active, |el| el.child(shown_bar(color.unwrap_or(ui.accent))))
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
        // Another Templum mirrors it now (docs/specs/templum-to-templum.md); the news dot, if any,
        // is drawn over it.
        .children(
            crate::share_dialog::mirror_mark(ws, &space.id, "workspace-mirrored", 12.0)
                .map(|mark| mark.absolute().bottom(px(2.0)).right(px(2.0))),
        )
        // A remote one: where it mirrors from.
        .when(space.is_remote(), |el| {
            el.child(
                crate::mirror::tile_mark(ui, &space.id)
                    .absolute()
                    .bottom(px(2.0))
                    .right(px(2.0)),
            )
        })
        // A terminal there has news (see alert.rs).
        .when(space.terminals().any(|(_, p)| p.alert.is_some()), |el| {
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
    // The hover and active fills go over the color's.
    div()
        .rounded(px(10.0))
        .when_some(color, |el, c| el.bg(c.opacity(0.18)))
        .when(
            space.stopped || space.remote.as_deref().is_some_and(|r| r.dim()),
            |el| el.opacity(0.45),
        )
        .child(inner)
        .into_any_element()
}

/// What a workspace shows in a square of `size`: its image, else its icon (`icon` px) in its
/// color, else its initials; `plain` colors what has no color.
pub(crate) fn face(
    look: &Look,
    name: &str,
    size: f32,
    icon: f32,
    theme: &Theme,
    plain: Hsla,
) -> AnyElement {
    let color = look.color.as_deref().and_then(|c| tint(c, theme));
    let (symbol, initials) = (look.icon, initials(name));
    let mark = move || match symbol {
        Some(symbol) => Icon::new(symbol)
            .size(px(icon))
            .text_color(color.unwrap_or(plain))
            .into_any_element(),
        None => div()
            .text_size(px((size * 0.35).round()))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(plain)
            .child(initials.clone())
            .into_any_element(),
    };
    match &look.image {
        // An image that can't be read shows the icon instead.
        Some(path) => img(path.clone())
            .size(px(size))
            .rounded(px(size / 4.0))
            .object_fit(ObjectFit::Cover)
            .with_fallback(mark)
            .into_any_element(),
        None => mark(),
    }
}

/// The sidebar's tile as a picture: the edit dialog's preview, the screen's cards.
pub(crate) fn tile_face(look: &Look, name: &str, theme: &Theme, ui: Ui) -> AnyElement {
    let color = look.color.as_deref().and_then(|c| tint(c, theme));
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .size(px(TILE))
        .rounded(px(10.0))
        .border_1()
        .border_color(ui.hairline)
        .bg(color.map_or(ui.card, |c| c.opacity(0.18)))
        .child(face(
            look,
            name,
            TILE - 2.0,
            (TILE * 0.45).round(),
            theme,
            ui.title,
        ))
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
