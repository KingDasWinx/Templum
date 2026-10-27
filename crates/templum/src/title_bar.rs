//! The integrated title bar: on the left the sidebar's button, the workspace's name in the
//! middle ("Home" on the Home), and on the right the zoom level (click: back to 100%; not on the
//! Home, which has no canvas), the theme picker and the settings. Blocks are added from the
//! toolbar at the bottom of the canvas.

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
    let home = ws.home_up();
    let zoom = zoom_label(ws.space.canvas.camera.zoom());
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
                .map(|el| {
                    if home {
                        return el
                            .child(
                                Icon::new(IconName::House)
                                    .size(px(14.0))
                                    .text_color(ui.title),
                            )
                            .child(div().id("title-home").child("Home").test_support());
                    }
                    el.when(
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
                })
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.show_home(window, cx)))
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
                .children(api_pill(ws, cx))
                .children(crate::usage::meter(ws, cx))
                .children((!home).then(|| {
                    button("zoom-level", ui)
                        .child(zoom)
                        .on_click(
                            cx.listener(|ws, _: &ClickEvent, window, cx| ws.zoom_reset(window, cx)),
                        )
                        .test_support()
                }))
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

/// The API's pill while it is on, before the usage meter: "API" and how many clients are
/// connected, or "API off" in yellow when it can't listen (the reason is its tooltip). A click
/// opens Settings ▸ API. Off, no pill: the person always sees a door that is open.
fn api_pill(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<gpui_kit::AnyElement> {
    let store = ws.api.store.as_ref().filter(|s| s.settings.enabled)?;
    let (label, tip, warn) = match (&ws.api.server, &ws.api.error) {
        (Some(server), _) => {
            let clients = ws.api.clients;
            let url = format!(
                "{}://{}",
                if store.settings.tls == templum_api::store::Tls::Off {
                    "http"
                } else {
                    "https"
                },
                server.addr()
            );
            let plural = if clients == 1 { "client" } else { "clients" };
            (
                if clients == 0 {
                    "API".to_string()
                } else {
                    format!("API · {clients}")
                },
                format!("The API listens on {url} · {clients} {plural} connected"),
                false,
            )
        }
        (None, error) => (
            "API off".to_string(),
            error.clone().unwrap_or_default(),
            true,
        ),
    };
    let ui = ws.ui;
    let yellow = crate::grid::hsla(ws.theme.palette.ansi[3], 1.0);
    Some(
        button("api-pill", ui)
            .when(warn, |el| el.text_color(yellow))
            .child(label)
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                if ws.settings.as_ref().is_none_or(|p| p.closing) {
                    ws.toggle_settings(window, cx);
                }
                ws.set_section(crate::settings::Section::Api, cx);
            }))
            .test_support()
            .into_any_element(),
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

/// The zoom as the title bar reads it: whole percents, and tenths under 10% ("1.5%").
fn zoom_label(zoom: f32) -> String {
    let percent = zoom * 100.0;
    if percent < 10.0 {
        format!("{}%", (percent * 10.0).round() / 10.0)
    } else {
        format!("{}%", percent.round())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_zoom_reads_in_whole_percents_and_tenths_under_10() {
        assert_eq!(zoom_label(1.0), "100%");
        assert_eq!(zoom_label(0.33), "33%");
        assert_eq!(zoom_label(0.25), "25%");
        assert_eq!(zoom_label(0.03), "3%");
        assert_eq!(zoom_label(0.015), "1.5%");
        assert_eq!(zoom_label(0.01), "1%");
    }
}
