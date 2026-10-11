//! The theme picker: a list under the palette button. Hovering or arrowing previews a theme, Enter or
//! a click keeps it for the current mode (and saves it), Esc or a click outside puts the old one
//! back.

use std::rc::Rc;

use gpui_kit::component::TITLE_BAR_HEIGHT;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, BoxShadow, ClickEvent, Context, MouseButton, MouseDownEvent, TestSupportExt, div,
    point, px,
};

use crate::grid::hsla;
use crate::theme::Theme;
use crate::workspace::Workspace;

pub(crate) struct Picker {
    /// The theme in use when the picker opened: Esc puts it back.
    pub before: Rc<Theme>,
    /// Index into the workspace's themes.
    pub selected: Option<usize>,
    /// Why the last choice couldn't be saved.
    pub error: Option<String>,
}

pub(crate) fn render(ws: &Workspace, picker: &Picker, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.theme.ui;
    let foreground = hsla(ws.theme.palette.foreground, 1.0);
    let mut list = div()
        .id("theme-list")
        .flex()
        .flex_col()
        .max_h(px(420.0))
        .overflow_y_scroll()
        .p(px(4.0));
    let mut group = "";
    for (i, entry) in ws.themes.iter().enumerate() {
        if entry.group != group {
            group = entry.group;
            list = list.child(
                div()
                    .px(px(8.0))
                    .pt(px(8.0))
                    .pb(px(4.0))
                    .text_size(px(11.0))
                    .text_color(ui.title_dim)
                    .child(group),
            );
        }
        let selected = picker.selected == Some(i);
        let current = entry.theme.name == picker.before.name;
        list = list.child(
            div()
                .id(("theme-item", i))
                .flex()
                .items_center()
                .justify_between()
                .h(px(26.0))
                .px(px(8.0))
                .rounded(px(6.0))
                .text_size(px(13.0))
                .cursor_pointer()
                .when(selected, |el| el.bg(ui.accent.opacity(0.22)))
                .child(entry.theme.name.clone())
                .when(current, |el| el.child("✓"))
                .on_hover(cx.listener(move |ws, hovered: &bool, _, cx| {
                    if *hovered {
                        ws.preview_theme(i, cx);
                    }
                }))
                .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.choose_theme(i, cx)))
                .test_support(),
        );
    }
    let panel = div()
        .id("theme-picker")
        .absolute()
        .top(TITLE_BAR_HEIGHT + px(4.0))
        .right(px(8.0))
        .w(px(240.0))
        .flex()
        .flex_col()
        .bg(ws.theme.ui.surface)
        .text_color(foreground)
        .border_1()
        .border_color(ui.hairline)
        .rounded(px(10.0))
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(0.0), px(12.0)),
            blur_radius: px(32.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        // Presses inside the panel don't reach the backdrop.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(list)
        .when_some(picker.error.clone(), |el, error| {
            el.child(
                div()
                    .id("picker-error")
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_t_1()
                    .border_color(ui.hairline)
                    .text_size(px(12.0))
                    .text_color(hsla(ws.theme.palette.ansi[1], 1.0))
                    .child(error)
                    .test_support(),
            )
        })
        .test_support();
    // Covers the whole window: a press anywhere outside the panel cancels, and nothing below
    // (blocks, canvas, title bar) gets it.
    div()
        .id("picker-backdrop")
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|ws, _: &MouseDownEvent, _, cx| {
                ws.cancel_theme(cx);
                cx.stop_propagation();
            }),
        )
        .child(panel)
        .test_support()
        .into_any_element()
}
