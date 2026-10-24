//! The integrated title bar: on the left the sidebar's button, the workspace's name in the
//! middle, and on the right the zoom level (click: back to 100%), the theme picker and the
//! settings. Blocks are added from the toolbar at the bottom of the canvas.

use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, TitleBar};
use gpui_kit::prelude::*;
use gpui_kit::{
    ClickEvent, Context, Div, FontWeight, MouseButton, Stateful, TestSupportExt, div, px,
    transparent_black,
};

use crate::sidebar;
use crate::theme::Ui;
use crate::workspace::Workspace;

pub(crate) fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    let zoom = format!("{}%", (ws.space.canvas.camera.zoom() * 100.0).round());
    let closing = cx.entity();
    TitleBar::new()
        // Linux draws its own ✕: unsaved files are asked about, as the system's close does.
        .on_close_window(move |_, window, cx| {
            if closing.update(cx, |ws, cx| ws.may_close(window, cx)) {
                window.remove_window();
            }
        })
        .bg(transparent_black())
        .border_color(ui.hairline)
        .child(
            div().flex_1().flex().items_center().pl(px(8.0)).child(
                button("sidebar-button", ui)
                    .relative()
                    // The count of the requests, while the sidebar that has their button is hidden.
                    .children(
                        (!ws.sidebar)
                            .then(|| crate::inbox::badge(ws, "sidebar-badge"))
                            .flatten(),
                    )
                    .child(
                        Icon::new(if ws.sidebar {
                            IconName::PanelLeftClose
                        } else {
                            IconName::PanelLeftOpen
                        })
                        .size(px(14.0))
                        .text_color(ui.title),
                    )
                    .on_click(
                        cx.listener(|ws, _: &ClickEvent, window, cx| ws.toggle_sidebar(window, cx)),
                    )
                    .test_support(),
            ),
        )
        .child(
            button("workspace-name", ui)
                .gap(px(6.0))
                .font_weight(FontWeight::MEDIUM)
                .when(
                    ws.space.look.icon.is_some() || ws.space.look.image.is_some(),
                    |el| {
                        el.child(sidebar::face(
                            &ws.space.look,
                            &ws.space.name,
                            14.0,
                            14.0,
                            &ws.theme,
                            ui.title,
                        ))
                    },
                )
                .child(ws.space.name.clone())
                .on_click(
                    cx.listener(|ws, _: &ClickEvent, window, cx| ws.toggle_screen(window, cx)),
                )
                .test_support(),
        )
        .child(
            div()
                .flex_1()
                .flex()
                .justify_end()
                .items_center()
                .gap(px(2.0))
                .pr(px(8.0))
                .children(crate::update::pill(ws, cx))
                .children(crate::usage::meter(ws, cx))
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
                ),
        )
}

pub(crate) fn button(id: &'static str, ui: Ui) -> Stateful<Div> {
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
