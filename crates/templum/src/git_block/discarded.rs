//! Recently discarded…: a repository's discards still in the trash, newest first, each with
//! Restore. See docs/specs/git.md, section 4, The trash.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::Sizable as _;
use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::prelude::*;
use gpui_kit::{App, Entity, Hsla, Window, div, px};
use templum_git::trash::Kept;

use crate::workspace::{GitAt, Workspace};

/// `at` is the Git whose trash is listed: a Restore acts on that one only.
pub(crate) fn open(
    ws: Entity<Workspace>,
    at: GitAt,
    kept: Vec<Kept>,
    dim: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    window.open_alert_dialog(cx, move |alert, _, _| {
        let body = if kept.is_empty() {
            div()
                .text_color(dim)
                .child("Nothing discarded in this repository in the last 7 days.")
                .into_any_element()
        } else {
            div()
                .id("git-discarded")
                .max_h(px(360.0))
                .overflow_y_scroll()
                .children(kept.iter().enumerate().map(|(i, k)| {
                    let (ws, at, entry) = (ws.clone(), at.clone(), k.id.clone());
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .py(px(4.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .truncate()
                                .child(k.what.clone()),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_color(dim)
                                .child(templum_git::ago(now, k.time as i64)),
                        )
                        .child(
                            Button::new(("git-restore", i))
                                .label("Restore")
                                .small()
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let (at, entry) = (at.clone(), entry.clone());
                                    ws.update(cx, |ws, cx| {
                                        ws.restore_discarded(at, entry, false, window, cx)
                                    });
                                }),
                        )
                }))
                .into_any_element()
        };
        alert
            .confirm()
            .title("Recently discarded")
            .w(px(520.0))
            .button_props(
                DialogButtonProps::default()
                    .show_cancel(false)
                    .ok_text("Close"),
            )
            .child(body)
    });
}
