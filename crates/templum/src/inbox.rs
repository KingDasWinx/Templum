//! The requests inbox (docs/specs/ask-inbox.md): every question, plan and yes/no request of the
//! workspace in one popup, drawn as every popup is (crate::popup), each with where it comes from and why it is
//! asked; and the notice on a terminal whose agent waits for an answer. Window chrome: the
//! canvas's zoom never scales the popup. The request itself is `balloon.rs`'s.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, ElementId, FontWeight, Hsla, KeyDownEvent,
    ScrollWheelEvent, TestSupportExt, Window, div, px,
};
use templum_canvas::BlockId;

use crate::balloon::{self, SIDE, part};
use crate::grid::hsla;
use crate::kanban::scrollbar_over;
use crate::popup;
use crate::workspace::{Approved, Wait, Waiting, Workspace};

/// The list's width, pixels.
const LIST_W: f32 = 240.0;
/// The foot's height: its padding, its buttons and its hairline.
const FOOT: f32 = 51.0;
/// The text's sizes: the names, and what is said about them.
const TEXT: f32 = 13.0;
const SMALL: f32 = 11.5;

/// What a request is, as a row says it.
fn kind(wait: &Wait) -> String {
    match &wait.what {
        Waiting::Question { ask, .. } if ask.plan.is_some() => "a plan".into(),
        Waiting::Question { ask, .. } => match ask.questions.len() {
            1 => "a question".into(),
            n => format!("{n} questions"),
        },
        Waiting::Allow { then } => match then {
            Approved::Connect { .. } => "a request to connect".into(),
            Approved::RemoveWorktree { .. } => "a request to delete a worktree".into(),
            Approved::MergeWorktree { .. } => "a request to merge a worktree".into(),
            Approved::Remote { .. } => "a request".into(),
        },
        Waiting::Turn { .. } | Waiting::Event { .. } => String::new(),
    }
}

/// How long a request has waited: "now", "2m", "1h", "3d".
pub(crate) fn age(waited: Duration) -> String {
    match waited.as_secs() {
        0..60 => "now".into(),
        s @ 60..3600 => format!("{}m", s / 60),
        s @ 3600..86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// The asking agent and its block's title; empty when its block is gone.
fn asker(ws: &Workspace, wait: &Wait) -> String {
    let Some(pane) = ws.pane(&wait.from) else {
        return String::new();
    };
    // Another computer's agent, as its block says.
    if let Some(remote) = &pane.remote {
        return match &remote.agent {
            Some(agent) => format!("{} · {}", agent.agent, remote.title),
            None => remote.title.clone(),
        };
    }
    let title = pane.term.title();
    match ws.agent_name(pane) {
        Some(agent) => format!("{agent} · {title}"),
        None => title,
    }
}

/// What the header says under "Requests": how many wait.
pub(crate) fn waiting_words(count: usize) -> String {
    match count {
        0 => "Nothing waits for you".to_string(),
        1 => "1 request waits for your answer".to_string(),
        n => format!("{n} requests wait for your answer"),
    }
}

/// The popup, `w` by `h` pixels: the header, then the list of the workspace's requests at the
/// left and the selected one at the right.
pub(crate) fn popup(
    ws: &Workspace,
    w: f32,
    h: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let selected = ws.inbox_selected();
    let right = w - LIST_W - 1.0;
    let yellow = hsla(ws.theme.palette.ansi[3], 1.0);
    popup::frame("inbox", ws, w, h)
        .text_size(px(TEXT))
        .track_focus(&ws.inbox_focus)
        .on_key_down(cx.listener(|ws, e: &KeyDownEvent, window, cx| ws.on_inbox_key(e, window, cx)))
        // The wheel scrolls the lists and stops there: the popup occludes the canvas under it.
        .on_scroll_wheel(|_: &ScrollWheelEvent, _, cx| cx.stop_propagation())
        .child(popup::header(
            ws,
            "inbox",
            popup::tile(IconName::CircleQuestionMark, yellow),
            "Requests",
            Some(popup::subtitle(ws, waiting_words(ws.inbox().len()))),
            Vec::new(),
            cx,
        ))
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .child(list(ws, selected.map(|w| w.key), window, cx))
                .child(div().w(px(1.0)).h_full().flex_shrink_0().bg(ui.hairline))
                .child(match selected {
                    Some(wait) => detail(ws, wait, right, h - popup::HEADER, cx),
                    None => div()
                        .id("inbox-empty")
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(ui.title_dim)
                        .child("Nothing waits for you")
                        .test_support()
                        .into_any_element(),
                }),
        )
        .test_support()
        .into_any_element()
}

/// The left column: a row per request, oldest first.
fn list(
    ws: &Workspace,
    selected: Option<u64>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Div {
    let ui = ws.ui;
    let now = cx.background_executor().now();
    // The selection moved: its row comes into view, once the list has a size to scroll in.
    if ws.inbox_reveal.get() {
        if ws.inbox_list.bounds().size.height > px(0.0) {
            ws.inbox_reveal.set(false);
            if let Some(ix) = ws.inbox().iter().position(|w| Some(w.key) == selected) {
                ws.inbox_list.scroll_to_item(ix);
            }
        } else {
            window.request_animation_frame();
        }
    }
    let rows =
        ws.inbox().into_iter().map(|wait| {
            let key = wait.key;
            let why = ws.why(wait);
            let lit = selected == Some(key);
            div()
                .id(ElementId::Name(format!("inbox-row-{key}").into()))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .px(px(12.0))
                .py(px(8.0))
                .cursor_pointer()
                .when(lit, |el| el.bg(ui.hover))
                .hover(move |s| s.bg(ui.hover))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(asker(ws, wait)),
                )
                .when_some(why.card, |el, (id, title)| {
                    el.child(
                        div()
                            .truncate()
                            .text_size(px(SMALL))
                            .text_color(ui.title_dim)
                            .child(format!("#{id} {title}")),
                    )
                })
                .child(
                    div()
                        .flex()
                        .gap(px(6.0))
                        .text_size(px(SMALL))
                        .text_color(ui.title_dim)
                        .child(div().flex_1().truncate().child(kind(wait)))
                        .child(
                            div()
                                .flex_shrink_0()
                                .child(age(now.saturating_duration_since(wait.since))),
                        ),
                )
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.select_request(key, window, cx)
                }))
                .test_support()
        });
    div()
        .w(px(LIST_W))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .child(
            div()
                .id("inbox-list")
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scroll()
                .track_scroll(&ws.inbox_list)
                .children(rows)
                .children(ws.inbox_more().map(|more| {
                    div()
                        .id("inbox-more")
                        .px(px(12.0))
                        .py(px(8.0))
                        .text_size(px(SMALL))
                        .text_color(ui.title_dim)
                        .child(more)
                        .test_support()
                }))
                .test_support(),
        )
}

/// The right column, `w` wide and `h` high: a card of where the request comes from and why it is
/// asked, then the request itself, its body scrolling over a foot that stays; both at most
/// `popup::CONTENT_MAX` wide, centered.
fn detail(ws: &Workspace, wait: &Wait, w: f32, h: f32, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    // Presses count from TOO_SOON after it first shows.
    if wait.shown.get().is_none() {
        wait.shown.set(Some(cx.background_executor().now()));
    }
    let key = wait.key;
    let n = ws.asker(wait).map_or(0, |(_, BlockId(id))| id as usize);
    let width = w.min(popup::CONTENT_MAX);
    let Some(request) = balloon::request(ws, wait, n, width, h - 2.0 - FOOT, cx) else {
        return div().flex_1().into_any_element();
    };
    let why = ws.why(wait);
    let stopped = matches!(wait.what, Waiting::Question { stopped: true, .. });
    let pane = ws.pane(&wait.from);
    let folder = pane
        .and_then(|p| p.seen.folder.clone())
        .map(|f| f.display().to_string());
    let from = div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .font_weight(FontWeight::MEDIUM)
                .child(asker(ws, wait)),
        )
        .children(ws.asker(wait).is_some().then(|| {
            balloon::ghost("inbox-go", "Go to terminal", ui, 1.0)
                .flex_shrink_0()
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.go_to_request(key, window, cx)
                }))
                .test_support()
        }));
    let known = why.card.is_some() || why.task.is_some() || why.agent.is_some();
    let why = known.then(|| {
        div()
            .id("inbox-why")
            .flex()
            .flex_col()
            .gap(px(6.0))
            .when_some(why.card, |el, (id, title)| {
                el.child(popup::row(ws, "Working on", text(format!("#{id} {title}"))))
            })
            .when_some(why.task, |el, task| {
                el.child(popup::row(ws, "Handed", text(task)))
            })
            .when_some(why.agent, |el, said| {
                el.child(popup::row(ws, "The agent says", text(said)))
            })
            .test_support()
    });
    let card = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(12.0))
        .rounded(px(8.0))
        .bg(ui.card)
        .child(popup::row(ws, "From", from))
        .child(popup::row(
            ws,
            "Where",
            dim_line(ws, where_words(folder.as_deref(), &ws.space.name)),
        ))
        .children(why);
    // Its key in the body's and the foot's ids: a press on a request withdrawn before its release
    // is no click on the next one's button drawn in its place.
    let content = popup::content()
        .id(("balloon-wait", key))
        .gap(px(14.0))
        .px(px(SIDE))
        .py(px(14.0))
        .child(card)
        .child(request.body)
        .test_support();
    let foot = popup::foot(ws).child(
        popup::content()
            .id(("balloon-foot", key))
            .flex_row()
            .items_center()
            .gap(px(8.0))
            .px(px(SIDE))
            .child(request.foot)
            .test_support(),
    );
    let body = div()
        .id(part(n, "body"))
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .track_scroll(&wait.body.handle)
        .children(stopped.then(|| balloon::stopped_banner(ws, n)))
        .child(content)
        .test_support();
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .child(
            // The scrollbar goes over the body, not in it.
            div()
                .relative()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_col()
                .child(body)
                .child(scrollbar_over(&wait.body.handle, part(n, "scrollbar"), 1.0)),
        )
        .child(foot)
        .into_any_element()
}

/// Where the asking terminal works: its folder, when known, and the workspace.
pub(crate) fn where_words(folder: Option<&str>, space: &str) -> String {
    match folder {
        Some(folder) => format!("{folder} · {space}"),
        None => space.to_string(),
    }
}

/// A field of the card: text on as many lines as it takes, the height of a label's line at least.
fn text(words: String) -> Div {
    div()
        .min_h(px(24.0))
        .flex()
        .flex_col()
        .justify_center()
        .child(words)
}

/// A dim field of the card, cut to one line.
fn dim_line(ws: &Workspace, words: String) -> Div {
    div()
        .h(px(24.0))
        .flex()
        .flex_col()
        .justify_center()
        .text_color(ws.ui.title_dim)
        .child(div().truncate().child(words))
}

/// A section's name over its lines.
pub(crate) fn section(ws: &Workspace, name: &'static str) -> Div {
    div()
        .text_size(px(SMALL))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(ws.ui.title_dim)
        .child(name.to_uppercase())
}

/// A line of what is only known around the request.
pub(crate) fn dim(ws: &Workspace, text: String) -> Div {
    div()
        .min_w(px(0.0))
        .text_size(px(SMALL))
        .text_color(ws.ui.title_dim)
        .child(text)
}

/// What the notice says: that its agent waits, and how many requests when several.
pub(crate) fn notice_text(count: usize) -> String {
    match count {
        1 => "Waiting for your answer".to_string(),
        count => format!("Waiting for your answer · {count}"),
    }
}

/// The notice over the terminal block `id`, whose terminal is `terminal`, at zoom `z`: its agent
/// waits for an answer, with how many when several. A click opens the inbox at its oldest
/// request. None when it asks nothing.
pub(crate) fn notice(
    ws: &Workspace,
    id: BlockId,
    terminal: &str,
    z: f32,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    let (oldest, count) = ws.asking(terminal)?;
    let key = oldest.key;
    let n = id.0 as usize;
    // The yellow of the dot of a block waiting for its person.
    let waiting = hsla(ws.theme.palette.ansi[3], 1.0);
    let text = notice_text(count);
    Some(
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(12.0 * z))
            .flex()
            .justify_center()
            .child(
                div()
                    .id(ElementId::Name(format!("waiting-notice-{n}").into()))
                    .flex()
                    .items_center()
                    .gap(px(6.0 * z))
                    .px(px(10.0 * z))
                    .py(px(5.0 * z))
                    .rounded(px(8.0 * z))
                    .bg(ws.ui.surface)
                    .border_1()
                    .border_color(waiting)
                    .text_size(px(12.0 * z))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(hsla(ws.theme.palette.foreground, 1.0))
                    .cursor_pointer()
                    // Its point is its own: a press on it reaches neither its block nor the body under it, a
                    // selected block's group move included.
                    .occlude()
                    .child(
                        Icon::new(IconName::CircleQuestionMark)
                            .size(px(14.0 * z))
                            .text_color(waiting),
                    )
                    .child(text)
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.open_inbox(Some(key), window, cx)
                    }))
                    .test_support(),
            )
            .into_any_element(),
    )
}

/// What the badge reads: the count, "99+" past 99.
pub(crate) fn badge_text(count: usize) -> String {
    match count {
        0..=99 => count.to_string(),
        _ => "99+".to_string(),
    }
}

/// The count of the workspace's requests as a small yellow badge, `id` its element: none when
/// nothing waits.
pub(crate) fn badge(ws: &Workspace, id: &'static str) -> Option<AnyElement> {
    count_badge(
        ws,
        ws.inbox().len(),
        hsla(ws.theme.palette.ansi[3], 1.0),
        id,
    )
}

/// `count` as a small badge of `color` at the top right of a button, `id` its element: none for 0.
pub(crate) fn count_badge(
    ws: &Workspace,
    count: usize,
    color: Hsla,
    id: &'static str,
) -> Option<AnyElement> {
    (count > 0).then(|| {
        div()
            .id(id)
            .absolute()
            .top(px(-2.0))
            .right(px(-2.0))
            .min_w(px(16.0))
            .h(px(16.0))
            .px(px(4.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(color)
            .text_size(px(10.0))
            .font_weight(FontWeight::BOLD)
            .text_color(ws.ui.surface)
            .child(badge_text(count))
            .test_support()
            .into_any_element()
    })
}

/// The sidebar's button: the inbox toggled, its badge, and while a request has just arrived a
/// pulse (the caller draws frames while it lasts: `Workspace::inbox_pulsing`).
pub(crate) fn button(ws: &Workspace, pulse: f32, size: f32, cx: &mut Context<Workspace>) -> Div {
    let ui = ws.ui;
    let waiting = hsla(ws.theme.palette.ansi[3], 1.0);
    div()
        .relative()
        .child(
            div()
                .id("inbox-button")
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(size))
                .rounded(px(10.0))
                .cursor_pointer()
                .when(ws.inbox_open, move |el| el.bg(ui.pressed))
                .when(pulse > 0.0, move |el| el.bg(waiting.opacity(0.3 * pulse)))
                .hover(move |s| s.bg(ui.hover))
                .active(move |s| s.bg(ui.pressed))
                .child(
                    Icon::new(IconName::Inbox)
                        .size(px(16.0))
                        .text_color(if pulse > 0.0 { waiting } else { ui.title }),
                )
                .tooltip(|window, cx| Tooltip::new("Requests").build(window, cx))
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.toggle_inbox(window, cx)))
                .test_support(),
        )
        .children(badge(ws, "inbox-badge"))
}

/// How strong the pulse is `elapsed` after a request arrived: it swells and fades once a second
/// while `Workspace::inbox_pulsing`, 0 to 1.
pub(crate) fn pulse(elapsed: Duration) -> f32 {
    0.5 - 0.5 * (elapsed.as_secs_f32() * std::f32::consts::TAU).cos()
}
