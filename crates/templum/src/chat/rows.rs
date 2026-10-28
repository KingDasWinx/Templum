//! The chat's rows (docs/specs/templum-agent.md#rows, docs/specs/templum-agent-look.md): what the
//! person wrote, what the agent wrote, and each action as one line Templum writes, which opens to
//! show its call.

use gpui_kit::assets::IconName;
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::shimmer::ShimmerText;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Icon, Sizable as _, Size as UiSize};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, ClickEvent, Context, Div, ElementId, Hsla,
    SharedString, Stateful, TestSupportExt, div, px, relative,
};
use serde_json::Value;
use templum_agent::{ActionState, NoticeLevel, Row};

use super::look;
use crate::grid::hsla;
use crate::workspace::Workspace;

/// A call shows at most this many lines.
pub(crate) const CALL_LINES: usize = 12;

/// Row `n` of the conversation, `row`.
pub(crate) fn row(ws: &Workspace, n: usize, row: &Row, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let red = hsla(ws.theme.palette.ansi[1], 1.0);
    let id = ElementId::Name(format!("agent-row-{n}").into());
    let small = |text: String, color| div().text_size(px(12.0)).text_color(color).child(text);
    let el = div().id(id).w_full().flex();
    let el = match row {
        Row::Person { text, queued } => el
            .flex_col()
            .items_end()
            .child(
                div()
                    .id(ElementId::Name(format!("agent-bubble-{n}").into()))
                    .max_w(relative(0.8))
                    .px(px(14.0))
                    .py(px(9.0))
                    .rounded(px(16.0))
                    .bg(ui.hover)
                    .text_size(px(14.0))
                    .line_height(px(22.0))
                    .text_color(fg)
                    .child(text.clone())
                    .test_support(),
            )
            .when(*queued, |el| {
                el.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(4.0))
                        .mt(px(4.0))
                        .text_size(px(12.0))
                        .text_color(ui.title_dim)
                        .child(
                            Icon::new(IconName::Clock)
                                .size(px(12.0))
                                .text_color(ui.title_dim),
                        )
                        .child("Queued"),
                )
            }),
        Row::Text { text } => {
            let conversation = &ws.templum_agent.conversation;
            let text = SharedString::from(text.clone());
            // Markdown once it is whole: parsed at each delta, each left a layout behind.
            let body = if conversation.streaming() && n + 1 == conversation.rows.len() {
                div()
                    .id("agent-streaming")
                    .child(text)
                    .test_support()
                    .into_any_element()
            } else {
                let fill = look::code_fill(ui.surface, fg);
                let lang_color = ui.title_dim;
                TextView::markdown(ElementId::Name(format!("agent-md-{n}").into()), text)
                    .style(crate::markdown::chat_style(&ui, fg))
                    .selectable(true)
                    // The language and the copy button, on the fill, over gpui-base's own.
                    .code_block_actions(move |block, _, _| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.0))
                            .bg(fill)
                            .children(block.lang().filter(|lang| !lang.is_empty()).map(|lang| {
                                div().text_size(px(12.0)).text_color(lang_color).child(lang)
                            }))
                            .child(
                                div()
                                    .id("agent-copy")
                                    .child(
                                        Clipboard::new("copy").value(block.code()).tooltip("Copy"),
                                    )
                                    .test_support(),
                            )
                    })
                    .into_any_element()
            };
            el.child(
                div()
                    .w_full()
                    .text_size(px(14.0))
                    .line_height(px(22.0))
                    .text_color(fg)
                    .child(body),
            )
        }
        Row::Thinking => el
            .items_center()
            .gap(px(8.0))
            .child(
                Spinner::new()
                    .icon(IconName::Orbit)
                    .color(ui.accent)
                    .with_size(UiSize::Size(px(14.0))),
            )
            .child(
                ShimmerText::new("Thinking")
                    .highlight_color(fg)
                    .text_size(px(13.0))
                    .text_color(ui.title_dim),
            ),
        Row::Action {
            tool,
            input,
            state,
            said,
            target,
            ..
        } => {
            let opened = ws.templum_agent.expanded.get(&n).copied();
            let open = opened.is_some();
            let status: AnyElement = match state {
                ActionState::Running => Spinner::new()
                    .color(ui.accent)
                    .with_size(UiSize::Size(px(14.0)))
                    .into_any_element(),
                ActionState::Failed => status_icon(IconName::CircleX, red),
                ActionState::Denied => status_icon(IconName::Ban, ui.title_dim),
                ActionState::Done => status_icon(IconName::Check, ui.title_dim),
            };
            let sentence = match state {
                ActionState::Running => format!("{}…", templum_mcp::control::title(tool)),
                ActionState::Denied => "You denied this.".to_string(),
                ActionState::Failed => templum_mcp::control::title(tool).to_string(),
                ActionState::Done => said.clone(),
            };
            let color = if *state == ActionState::Denied {
                ui.title_dim
            } else {
                ui.title
            };
            let group: SharedString = format!("agent-row-{n}-group").into();
            let shows = target.is_some();
            let line = line(n, status)
                .group(group.clone())
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    if shows {
                        ws.show_row(n, window, cx)
                    } else {
                        ws.toggle_row(n, cx)
                    }
                }))
                .child(clipped(sentence, color))
                .child(
                    div()
                        .id(ElementId::Name(format!("agent-row-{n}-more").into()))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(20.0))
                        // Seen while the pointer is over the row, and while it is open (its color,
                        // not its opacity, so a click on it is still a click on something seen).
                        .text_color(if open {
                            ui.title_dim
                        } else {
                            Hsla::transparent_black()
                        })
                        .group_hover(group, move |s| s.text_color(ui.title_dim))
                        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            ws.toggle_row(n, cx)
                        }))
                        .child(
                            Icon::new(if open {
                                IconName::ChevronDown
                            } else {
                                IconName::ChevronRight
                            })
                            .size(px(14.0)),
                        )
                        .test_support(),
                );
            el.flex_col()
                .child(line)
                .when(*state == ActionState::Failed && !said.is_empty(), |el| {
                    el.child(
                        div()
                            .id(ElementId::Name(format!("agent-row-{n}-said").into()))
                            .pl(px(30.0))
                            .text_size(px(12.0))
                            .text_color(red.opacity(0.8))
                            .child(said.clone())
                            .test_support(),
                    )
                })
                .when(open, |el| {
                    // The whole sentence only when it is the line's; a refusal is under the line.
                    let sentence = matches!(state, ActionState::Done)
                        .then(|| said.clone())
                        .filter(|said| !said.is_empty());
                    // It fades in as it opens, not each time it is scrolled back into view.
                    let now = cx.background_executor().now();
                    let fade = opened.is_some_and(|at| look::fades(n, Some((n, at)), now));
                    el.child(call_box(ws, n, tool, input, sentence, fade))
                })
        }
        Row::Reads { count } => el.child(line(n, status_icon(IconName::Eye, ui.title_dim)).child(
            clipped(format!("Looked at the canvas ({count})"), ui.title_dim),
        )),
        Row::Approval { said, answer } => {
            let icon = if answer.starts_with("Allowed") {
                IconName::ShieldCheck
            } else {
                IconName::ShieldX
            };
            el.child(
                line(n, status_icon(icon, ui.title_dim))
                    .child(clipped(said.clone(), ui.title))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.0))
                            .text_color(ui.title_dim)
                            .child(answer.clone()),
                    ),
            )
        }
        Row::End { ms, cost_usd } => {
            let mut parts = Vec::new();
            if let Some(ms) = ms {
                parts.push(crate::alert::took(std::time::Duration::from_millis(*ms)));
            }
            if let Some(cost) = cost_usd {
                parts.push(format!("${cost:.2}"));
            }
            el.child(small(parts.join(" · "), ui.title_dim))
        }
        Row::Failure { text } => el
            .items_start()
            .gap(px(8.0))
            .child(
                div().flex_none().mt(px(2.0)).child(
                    Icon::new(IconName::CircleAlert)
                        .size(px(14.0))
                        .text_color(red),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(13.0))
                    .text_color(red)
                    .child(text.clone()),
            ),
        Row::Note { text } => el.justify_center().child(small(text.clone(), ui.title_dim)),
        Row::Notice {
            text,
            level,
            target,
        } => {
            let color = match level {
                NoticeLevel::Done => ui.accent,
                NoticeLevel::Failed => red,
                NoticeLevel::Waiting => hsla(ws.theme.palette.ansi[3], 1.0),
            };
            let dot = div()
                .size(px(8.0))
                .rounded_full()
                .bg(color)
                .into_any_element();
            el.child(
                line(n, dot)
                    .when(target.is_some(), |el| {
                        el.cursor_pointer()
                            .hover(move |s| s.bg(ui.hover))
                            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                ws.show_row(n, window, cx)
                            }))
                    })
                    .child(clipped(notice_words(text), ui.title)),
            )
        }
    };
    el.test_support().into_any_element()
}

/// The 14 px icon of a line's status slot.
fn status_icon(icon: IconName, color: Hsla) -> AnyElement {
    Icon::new(icon)
        .size(px(14.0))
        .text_color(color)
        .into_any_element()
}

/// A tool row's line: 28 px tall, its status in a 14 px slot, ready for its words.
fn line(n: usize, status: AnyElement) -> Stateful<Div> {
    div()
        .id(ElementId::Name(format!("agent-row-{n}-line").into()))
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(28.0))
        .px(px(8.0))
        .rounded(px(8.0))
        .child(
            div()
                .flex_none()
                .w(px(14.0))
                .flex()
                .justify_center()
                .child(status),
        )
}

/// A line's words: 13 px, one line, cut with "…".
fn clipped(text: String, color: Hsla) -> Div {
    div()
        .flex_1()
        .min_w(px(0.0))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .text_size(px(13.0))
        .text_color(color)
        .child(text)
}

/// An open tool row's details, under its words: the whole sentence when it was cut, the tool's
/// name, and its call, on the code fill.
fn call_box(
    ws: &Workspace,
    n: usize,
    tool: &str,
    input: &Value,
    sentence: Option<String>,
    fade: bool,
) -> AnyElement {
    let ui = ws.ui;
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let font = ws.cell.font.family.clone();
    let details = div()
        .id(ElementId::Name(format!("agent-row-{n}-call").into()))
        .ml(px(30.0))
        .mt(px(2.0))
        .p(px(10.0))
        .flex()
        .flex_col()
        .gap(px(6.0))
        .rounded(px(8.0))
        .bg(look::code_fill(ui.surface, fg))
        .children(sentence.map(|said| div().text_size(px(13.0)).text_color(ui.title).child(said)))
        .child(
            div()
                .font_family(font.clone())
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child(tool.to_string()),
        )
        .children(call_lines(input).into_iter().map(|line| {
            div()
                .font_family(font.clone())
                .text_size(px(12.0))
                .text_color(fg)
                .child(line)
        }))
        .test_support();
    if fade {
        details
            .with_animation(
                ElementId::Name(format!("agent-row-{n}-fade").into()),
                Animation::new(look::FADE).with_easing(crate::motion::slide),
                |el, t| el.opacity(t),
            )
            .into_any_element()
    } else {
        details.into_any_element()
    }
}

/// A call as lines: one per argument (`agent: codex`), a string as it is with its further lines
/// indented two spaces, any other value as compact JSON; at most `CALL_LINES`, the last
/// "⋯ N more lines" when there are more.
pub(crate) fn call_lines(input: &Value) -> Vec<String> {
    let mut lines: Vec<String> = match input {
        Value::Null => Vec::new(),
        Value::Object(map) => map
            .iter()
            .flat_map(|(key, value)| match value {
                Value::String(text) => {
                    let mut each = text.lines();
                    let first = format!("{key}: {}", each.next().unwrap_or_default());
                    std::iter::once(first)
                        .chain(each.map(|line| format!("  {line}")))
                        .collect()
                }
                other => vec![format!("{key}: {other}")],
            })
            .collect(),
        other => vec![other.to_string()],
    };
    if lines.len() > CALL_LINES {
        let more = lines.len() - (CALL_LINES - 1);
        lines.truncate(CALL_LINES - 1);
        lines.push(format!("⋯ {more} more lines"));
    }
    lines
}

/// A notice's first line as its row says it: `[task t7 from Codex · api (k9x1d): done in 31 s,
/// $0.31] Its answer…` becomes `Codex · api (k9x1d): done in 31 s, $0.31`.
fn notice_words(line: &str) -> String {
    let inner = line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map_or(line, |(inner, _)| inner);
    inner
        .split_once(" from ")
        .map_or(inner, |(_, rest)| rest)
        .to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_notice_says_who_and_what() {
        assert_eq!(
            super::notice_words(
                "[task t7 from Codex · api (k9x1d): done in 31 s, $0.31] Its answer is in the pasted text."
            ),
            "Codex · api (k9x1d): done in 31 s, $0.31"
        );
        assert_eq!(super::notice_words("plain"), "plain");
    }

    #[test]
    fn a_call_shows_a_line_per_argument() {
        use serde_json::json;
        assert_eq!(
            super::call_lines(&json!({"agent": "codex", "workspace": "w6"})),
            ["agent: codex", "workspace: w6"]
        );
        assert_eq!(
            super::call_lines(&json!({"keys": ["enter", "y"], "n": 3})),
            ["keys: [\"enter\",\"y\"]", "n: 3"]
        );
        assert_eq!(
            super::call_lines(&json!({"text": "ls\ncd /"})),
            ["text: ls", "  cd /"]
        );
        assert!(super::call_lines(&json!({})).is_empty());
        assert!(super::call_lines(&serde_json::Value::Null).is_empty());
        assert_eq!(super::call_lines(&json!("x")), ["\"x\""]);
        let long: Vec<String> = (0..20).map(|i| i.to_string()).collect();
        let lines = super::call_lines(&json!({"text": long.join("\n")}));
        assert_eq!(lines.len(), super::CALL_LINES);
        assert_eq!(lines[11], "⋯ 9 more lines");
    }
}
