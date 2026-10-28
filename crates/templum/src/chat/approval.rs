//! The approval popup (docs/specs/templum-agent.md#approvals): a dangerous call held until the
//! person answers, in the chat's place above the pill, or pinned over the chat's field.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, BoxShadow, ClickEvent, Context, FontWeight, MouseButton, MouseDownEvent,
    SharedString, TestSupportExt, Window, div, point, px,
};
use templum_mcp::control::Level;

use super::{Answer, Approval};
use crate::grid::hsla;
use crate::workspace::Workspace;

/// Keys within this of an approval showing are ignored, as the balloon's.
pub(crate) const KEYS_AFTER: Duration = Duration::from_millis(500);
/// The box of what will happen scrolls past this.
const DETAILS_MAX: f32 = 160.0;

/// The front approval, `of` its place among those waiting (1 of 3).
pub(crate) fn render(
    ws: &Workspace,
    approval: &Approval,
    (n, of): (usize, usize),
    width: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let yellow = hsla(ws.theme.palette.ansi[3], 1.0);
    let red = hsla(ws.theme.palette.ansi[1], 1.0);
    if approval.shown.get().is_none() {
        approval.shown.set(Some(cx.background_executor().now()));
    }
    let read = approval.seen();
    let each = approval.level == Level::AskEach;
    // The three weigh the same; the key sits in a small cap.
    let button =
        |id: &'static str, label: String, key: &'static str, strong: Option<gpui_kit::Hsla>| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(px(30.0))
                .px(px(12.0))
                .rounded(px(8.0))
                .border_1()
                .border_color(strong.unwrap_or(ui.hairline))
                .when_some(strong, |el, color| el.bg(color.opacity(0.12)))
                .text_size(px(13.0))
                .text_color(fg)
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .active(move |s| s.bg(ui.pressed))
                .child(label)
                .child(
                    div()
                        .size(px(18.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.0))
                        .bg(ui.card)
                        .text_size(px(12.0))
                        .text_color(ui.title_dim)
                        .child(key),
                )
        };
    let allow = |el: gpui_kit::Stateful<gpui_kit::Div>| el.when(!read, |el| el.opacity(0.4));
    let first = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            Icon::new(IconName::ShieldAlert)
                .size(px(15.0))
                .text_color(yellow),
        )
        .child(
            div()
                .flex_1()
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child("Templum Agent wants to"),
        )
        .when(of > 1, |el| {
            el.child(
                div()
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    .child(format!("{n} of {of}")),
            )
        });
    let more = (!read).then(|| {
        div()
            .id("approval-more")
            .text_size(px(12.0))
            .text_color(yellow)
            .child("Scroll to its end to allow it.")
            .test_support()
    });
    // Asked again with words that may read the same: the click wasn't lost.
    let changed = approval.changed.then(|| {
        div()
            .id("approval-changed")
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(px(12.0))
            .text_color(yellow)
            .child(
                Icon::new(IconName::TriangleAlert)
                    .size(px(12.0))
                    .text_color(yellow),
            )
            .child("It changed since you looked.")
            .test_support()
    });
    // The box scrolls past DETAILS_MAX; its scrollbar is a sibling over it.
    let details = (!approval.details.is_empty()).then(|| {
        div()
            .relative()
            .child(
                div()
                    .id("approval-details")
                    .max_h(px(DETAILS_MAX))
                    .overflow_y_scroll()
                    .track_scroll(&approval.scroll)
                    .p(px(10.0))
                    .rounded(px(8.0))
                    .bg(super::look::code_fill(ui.surface, fg))
                    .font_family(ws.cell.font.family.clone())
                    .text_size(px(12.0))
                    .text_color(fg)
                    .child(approval.details.clone())
                    .test_support(),
            )
            .child(crate::kanban::scrollbar_over(
                &approval.scroll,
                "approval-details-bar".into(),
                1.0,
            ))
    });
    let reason = approval.parsed.reason.clone().map(|reason| {
        div()
            .flex()
            .gap(px(6.0))
            .text_size(px(12.0))
            .text_color(ui.title_dim)
            .child("Its reason:")
            .child(reason)
    });
    let buttons = div()
        .flex()
        .flex_wrap()
        .gap(px(8.0))
        .child(
            allow(button(
                "approval-once",
                "Allow once".into(),
                "1",
                each.then_some(red),
            ))
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                ws.answer_front(Answer::Once, window, cx)
            }))
            .test_support(),
        )
        .when(!each, |el| {
            el.child(
                allow(button(
                    "approval-conversation",
                    "Allow for this conversation".into(),
                    "2",
                    None,
                ))
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                    ws.answer_front(Answer::Conversation, window, cx)
                }))
                .test_support(),
            )
        })
        .child(
            button("approval-deny", "Deny".into(), "3", None)
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                    ws.answer_front(Answer::Deny(None), window, cx)
                }))
                .test_support(),
        );
    let words = ws.templum_agent.words.as_ref().map(|words| {
        div()
            .id("approval-words")
            .child(Input::new(words).with_size(gpui_kit::component::Size::Small))
            .test_support()
    });
    // While it has the keyboard: full yellow and a soft ring of it. Floating (the chat closed), it
    // has the card's shadow.
    let focused = ws
        .templum_agent
        .approval_focus
        .as_ref()
        .is_some_and(|f| f.contains_focused(window, cx));
    let mut shadow = Vec::new();
    if focused {
        shadow.push(BoxShadow {
            color: yellow.opacity(0.25),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(2.0),
            inset: false,
        });
    }
    if !ws.templum_agent.open {
        shadow.push(BoxShadow {
            color: ui.shadow,
            offset: point(px(0.0), px(10.0)),
            blur_radius: px(30.0),
            spread_radius: px(0.0),
            inset: false,
        });
    }
    let mut root = div()
        .id("approval")
        .flex()
        .flex_col()
        .gap(px(10.0))
        .w(px(width))
        .p(px(14.0))
        .rounded(px(12.0))
        .bg(ui.surface)
        .border_1()
        .border_color(if focused {
            yellow
        } else {
            yellow.opacity(0.55)
        })
        .shadow(shadow)
        .on_key_down(cx.listener(Workspace::on_approval_key))
        .on_key_down(cx.listener(Workspace::on_agent_key))
        .occlude()
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
            cx.stop_propagation()
        });
    if let Some(focus) = &ws.templum_agent.approval_focus {
        root = root.track_focus(focus);
    }
    root.on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
        // A click on it gives it the keyboard; its field keeps its own.
        if !ws
            .templum_agent
            .words
            .as_ref()
            .is_some_and(|w| gpui_kit::Focusable::focus_handle(w.read(cx), cx).is_focused(window))
        {
            ws.focus_approval(window, cx);
        }
    }))
    .child(first)
    .child(
        div()
            .text_size(px(14.0))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(fg)
            .child(SharedString::from(approval.said.clone())),
    )
    .children(changed)
    .children(details)
    .children(more)
    .children(reason)
    .child(buttons)
    .children(words)
    .test_support()
    .into_any_element()
}
