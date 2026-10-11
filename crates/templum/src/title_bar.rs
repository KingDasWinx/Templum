//! The integrated title bar, drawn on the window's glass: the title in the middle, and on the
//! right the zoom level (click: back to 100%), the theme picker, the settings and a button for a
//! new terminal.

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Div, MouseButton, Stateful, TestSupportExt, div, px, transparent_black,
};

use crate::theme::Ui;
use crate::workspace::Workspace;

pub(crate) fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    let zoom = format!("{}%", (ws.canvas.camera.zoom() * 100.0).round());
    TitleBar::new()
        .bg(transparent_black())
        .border_color(ui.hairline)
        .child(div().flex_1())
        .child(
            div()
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child("Templum"),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .justify_end()
                .items_center()
                .gap(px(2.0))
                .pr(px(8.0))
                .child(
                    button("zoom-level", ui)
                        .child(zoom)
                        .on_click(
                            cx.listener(|ws, _: &ClickEvent, window, cx| ws.zoom_reset(window, cx)),
                        )
                        .test_support(),
                )
                .child(
                    button("theme-button", ui)
                        .child(
                            Icon::new(IconName::Palette)
                                .size(px(14.0))
                                .text_color(ui.title),
                        )
                        .on_click(
                            cx.listener(|ws, _: &ClickEvent, _, cx| ws.toggle_theme_picker(cx)),
                        )
                        .test_support(),
                )
                .child(
                    button("settings-button", ui)
                        .child(
                            Icon::new(IconName::Settings)
                                .size(px(14.0))
                                .text_color(ui.title),
                        )
                        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                            ws.toggle_settings(window, cx)
                        }))
                        .test_support(),
                )
                .child(
                    button("new-terminal", ui)
                        .child(
                            Icon::new(IconName::Plus)
                                .size(px(14.0))
                                .text_color(ui.title),
                        )
                        .on_click(
                            cx.listener(|ws, _: &ClickEvent, window, cx| {
                                ws.new_terminal(window, cx)
                            }),
                        )
                        .test_support(),
                ),
        )
}

fn button(id: &'static str, ui: Ui) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .h(px(24.0))
        .min_w(px(28.0))
        .px(px(8.0))
        .rounded_full()
        .text_size(px(12.0))
        .text_color(ui.title)
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        // The title bar moves the window on a press: keep presses on its buttons.
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}
