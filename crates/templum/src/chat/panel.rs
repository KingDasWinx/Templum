//! The chat (docs/specs/templum-agent.md#the-chat): a floating card that rises from the pill, the
//! pill its footer; window chrome over the canvas, which works around it.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, Size as UiSize};
use gpui_kit::prelude::*;
use gpui_kit::{
    Animation, AnimationExt as _, AnyElement, BoxShadow, ClickEvent, Context, ElementId,
    FollowMode, FontWeight, MouseButton, MouseDownEvent, TestSupportExt, Window, canvas, div, list,
    point, px,
};

use crate::grid::hsla;
use crate::workspace::Workspace;

const WIDTH: f32 = 640.0;
const SIDES: f32 = 32.0;
/// Full screen leaves this much canvas on every side.
const INSET: f32 = 16.0;
/// The header and the composer take about 150 of it; the rest is the rows'.
const MIN_H: f32 = 300.0;
/// At most this much of the canvas's height.
const MAX_SHARE: f32 = 0.7;
const HEADER: f32 = 44.0;
/// It rises this far as it fades in.
const RISE: f32 = 12.0;
const APPEAR: Duration = Duration::from_millis(220);

/// The card's width on a canvas `canvas_w` wide.
fn width(canvas_w: f32, full: bool) -> f32 {
    if full {
        canvas_w - 2.0 * INSET
    } else {
        WIDTH.min(canvas_w - SIDES)
    }
}

/// The width of an approval pinned in the open card: the column the rows share.
pub(crate) fn approval_width(canvas_w: f32, full: bool) -> f32 {
    super::look::column(width(canvas_w, full))
}

const GREETING: &str = "It builds and runs your canvas: workspaces, agents, boards and notes. \
Anything it can't take back waits for your click.";
/// A suggestion chip is as wide as the column, up to this.
const CHIP_MAX: f32 = 520.0;

/// The person's message Catch me up sends.
pub(crate) const CATCH_UP: &str = "Catch me up: what's going on?";

/// What an empty conversation offers; a click puts its text in the field (Catch me up sends).
pub(crate) const SUGGESTIONS: [(IconName, &str); 4] = [
    (
        IconName::LayoutGrid,
        "Set up a workspace for a project, with a kanban and an agent per card",
    ),
    (IconName::Eye, CATCH_UP),
    (IconName::Grid2x2, "Tidy this workspace into a grid"),
    (IconName::Bot, "Open an agent for each card on my board"),
];

/// The chat greets: it has no rows, or only notes (a new chat leaves one).
pub(crate) fn greets(rows: &[templum_agent::Row]) -> bool {
    rows.iter()
        .all(|row| matches!(row, templum_agent::Row::Note { .. }))
}

/// The body was scrolled up: its content is taller than it, it doesn't follow its end, and it
/// isn't at its end.
pub(crate) fn scrolled_up(list: &gpui_kit::ListState) -> bool {
    list.max_offset_for_scrollbar().y > px(0.0)
        && !list.is_following_tail()
        && list.is_scrolled_to_end() != Some(true)
}

/// The run meter: its tasks under way and what the conversation spent ("3 working · $1.24");
/// None before it ran anything.
pub(crate) fn meter(ws: &Workspace) -> Option<String> {
    let tasks = ws.agent_tasks();
    let cost = ws.templum_agent.conversation.cost();
    (tasks > 0 || cost > 0.0).then(|| format!("{tasks} working · ${cost:.2}"))
}

/// The chat, `canvas` the canvas's width and height, `footer` the pill; None while it is closed.
pub(crate) fn render(
    ws: &Workspace,
    (canvas_w, canvas_h): (f32, f32),
    footer: AnyElement,
    approval: Option<AnyElement>,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    let agent = &ws.templum_agent;
    if !agent.open {
        return None;
    }
    let ui = ws.ui;
    let full = agent.full;
    let rows = &agent.conversation.rows;
    let fg = hsla(ws.theme.palette.foreground, 1.0);
    let col = super::look::column(width(canvas_w, full));
    let button = |id: &'static str, icon: IconName, tip: &'static str| {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .size(px(28.0))
            .rounded(px(8.0))
            .cursor_pointer()
            .hover(move |s| s.bg(ui.hover))
            .active(move |s| s.bg(ui.pressed))
            .child(Icon::new(icon).size(px(15.0)).text_color(ui.title_dim))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
    };
    let working = agent.conversation.working;
    let tasks_under_way = ws.agent_tasks() > 0;
    let lead = if working {
        Spinner::new()
            .icon(IconName::Orbit)
            .color(ui.accent)
            .with_size(UiSize::Size(px(15.0)))
            .into_any_element()
    } else {
        Icon::new(IconName::Sparkles)
            .size(px(15.0))
            .text_color(ui.accent)
            .into_any_element()
    };
    let header = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(HEADER))
        .px(px(12.0))
        .child(lead)
        .child(
            div()
                .min_w(px(0.0))
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(fg)
                .child("Templum Agent"),
        )
        .child(div().flex_1())
        .when_some(meter(ws), |el, meter| {
            el.child(
                div()
                    .id("agent-meter")
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .h(px(24.0))
                    .px(px(10.0))
                    .rounded(px(12.0))
                    .bg(ui.card)
                    .border_1()
                    .border_color(ui.hairline)
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    // A label, unless tasks are under way: then it opens their drawer.
                    .when(tasks_under_way, |el| {
                        el.cursor_pointer()
                            .hover(move |s| s.bg(ui.hover))
                            .tooltip(|window, cx| Tooltip::new("Tasks under way").build(window, cx))
                    })
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| {
                        if ws.agent_tasks() > 0 {
                            ws.templum_agent.tasks_open = !ws.templum_agent.tasks_open;
                            cx.notify();
                        }
                    }))
                    .child(meter)
                    .when(tasks_under_way, |el| {
                        el.child(
                            Icon::new(IconName::ChevronDown)
                                .size(px(12.0))
                                .text_color(ui.title_dim),
                        )
                    })
                    .test_support(),
            )
        })
        .child(
            button("agent-new", IconName::MessageSquarePlus, "New chat")
                .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.new_chat(cx)))
                .test_support(),
        )
        .child(
            if full {
                button("agent-full", IconName::Minimize2, "Exit full screen")
            } else {
                button("agent-full", IconName::Maximize2, "Full screen")
            }
            .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| {
                ws.templum_agent.full = !ws.templum_agent.full;
                cx.notify();
            }))
            .test_support(),
        )
        .child(
            button("agent-close", IconName::ChevronDown, "Close (Esc)")
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.leave_agent(window, cx)))
                .test_support(),
        );
    // The usage chip's drawer: a line per task under way (a click shows its block), then Catch me
    // up.
    let line = |id: ElementId, icon: IconName, color| {
        div()
            .id(id)
            .flex()
            .items_center()
            .gap(px(8.0))
            .h(px(28.0))
            .px(px(8.0))
            .rounded(px(8.0))
            .text_size(px(13.0))
            .cursor_pointer()
            .hover(move |s| s.bg(ui.hover))
            .child(Icon::new(icon).size(px(14.0)).text_color(color))
    };
    let tasks = (agent.tasks_open && tasks_under_way).then(|| {
        div()
            .flex_none()
            .flex()
            .justify_center()
            .border_b_1()
            .border_color(ui.hairline)
            .child(
                div()
                    .w(px(col))
                    .flex()
                    .flex_col()
                    .py(px(6.0))
                    .children(ws.agent_task_blocks().into_iter().enumerate().map(
                        |(n, (space, id))| {
                            let title = ws
                                .spaces()
                                .find(|s| s.id == space)
                                .and_then(|s| s.blocks.get(&id))
                                .map(|b| crate::block::title(ws, b, cx).to_string())
                                .unwrap_or_default();
                            line(
                                ElementId::Name(format!("agent-task-{n}").into()),
                                IconName::Bot,
                                ui.title_dim,
                            )
                            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                ws.reveal(&space, id, window, cx)
                            }))
                            .child(
                                div()
                                    .min_w(px(0.0))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_color(fg)
                                    .child(title),
                            )
                            .test_support()
                        },
                    ))
                    .child(
                        line("agent-catch-up".into(), IconName::Sparkles, ui.accent)
                            .text_color(ui.accent)
                            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                                ws.send_to_agent(CATCH_UP.to_string(), window, cx)
                            }))
                            .child("Catch me up")
                            .test_support(),
                    ),
            )
    });
    // Only the rows in view are drawn: the list learns of each new one (and of a new chat), and
    // follows its end while it is there; a row below the view says so.
    let state = &agent.list;
    match state.item_count() {
        n if rows.len() < n => state.reset(rows.len()),
        n if rows.len() > n => state.splice(n..n, rows.len() - n),
        _ => {}
    }
    if state.is_following_tail() {
        agent.unseen.set(false);
    } else if rows.len() > agent.seen.get() {
        agent.unseen.set(true);
    }
    // A row that arrives while the chat is open fades in; the rows there when it opens don't.
    let now = cx.background_executor().now();
    if agent.drawn.get() && rows.len() > agent.seen.get() {
        agent.arrival.set(Some((agent.seen.get(), now)));
    }
    agent.drawn.set(true);
    agent.seen.set(rows.len());
    let body = if greets(rows) {
        let chip_w = col.min(CHIP_MAX);
        let chip = |n: usize, icon: IconName, text: &'static str| {
            div()
                .id(ElementId::Name(format!("agent-suggestion-{n}").into()))
                .w_full()
                .min_h(px(40.0))
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(14.0))
                .py(px(10.0))
                .rounded(px(12.0))
                .border_1()
                .border_color(ui.hairline)
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover).border_color(ui.hover_border))
                .active(move |s| s.bg(ui.pressed))
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.suggest_to_agent(text, window, cx)
                }))
                .child(
                    div()
                        .flex_none()
                        .child(Icon::new(icon).size(px(15.0)).text_color(ui.title_dim)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .text_size(px(13.0))
                        .text_color(fg)
                        .child(text),
                )
                .test_support()
        };
        div()
            .id("agent-greeting-scroll")
            .size_full()
            .overflow_y_scroll()
            .child(
                div()
                    .id("agent-greeting")
                    .w_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .pt(px(24.0))
                    .pb(px(16.0))
                    .child(
                        div()
                            .size(px(40.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(ui.accent.opacity(0.12))
                            .child(
                                Icon::new(IconName::Sparkles)
                                    .size(px(20.0))
                                    .text_color(ui.accent),
                            ),
                    )
                    .child(
                        div()
                            .mt(px(12.0))
                            .text_size(px(16.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(fg)
                            .child("What should Templum do?"),
                    )
                    .child(
                        div()
                            .mt(px(6.0))
                            .max_w(px(420.0))
                            .text_center()
                            .text_size(px(13.0))
                            .text_color(ui.title_dim)
                            .child(GREETING),
                    )
                    .child(
                        div()
                            .mt(px(20.0))
                            .w(px(chip_w))
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .children(
                                SUGGESTIONS
                                    .iter()
                                    .enumerate()
                                    .map(|(n, (icon, text))| chip(n, *icon, text)),
                            ),
                    )
                    .test_support(),
            )
            .into_any_element()
    } else {
        let row = cx.processor(move |ws: &mut Workspace, n: usize, _, cx| {
            let agent = &ws.templum_agent;
            let rows = &agent.conversation.rows;
            let gap = rows.get(n).map_or(0.0, |row| {
                super::look::gap_above(n.checked_sub(1).and_then(|p| rows.get(p)), row)
            });
            let fades = super::look::fades(n, agent.arrival.get(), cx.background_executor().now());
            let row = rows.get(n).cloned();
            let row = row.map(|row| super::rows::row(ws, n, &row, cx));
            let el = div()
                .w_full()
                .flex()
                .justify_center()
                .child(div().w(px(col)).pt(px(gap)).children(row));
            if fades {
                el.with_animation(
                    ElementId::Name(format!("agent-fade-{n}").into()),
                    Animation::new(super::look::FADE).with_easing(crate::motion::slide),
                    |el, t| el.opacity(t),
                )
                .into_any_element()
            } else {
                el.into_any_element()
            }
        });
        list(state.clone(), row)
            .size_full()
            .pt(px(8.0))
            .pb(px(16.0))
            .into_any_element()
    };
    // Outside full screen the body grows with its rows (the card is as tall as they are, up to its
    // cap): the list can't size itself, so after it is laid out a zero-size canvas measures how much
    // of it is out of view and the body's basis grows to hold it, one frame later.
    let measure = (!full && !greets(rows)).then(|| {
        let (list, rows_h, ws_id) = (state.clone(), agent.rows_h.clone(), cx.entity_id());
        canvas(
            move |_, _, cx| {
                let over = f32::from(list.max_offset_for_scrollbar().y);
                // The list's padding: 8 above its rows and 16 under.
                let want = f32::from(list.viewport_bounds().size.height) + over + 24.0;
                if over > 0.0 && want > rows_h.get() + 0.5 {
                    rows_h.set(want);
                    cx.notify(ws_id);
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    });
    // Back to the end, once the person scrolled up; a dot when rows came below meanwhile.
    let to_end = scrolled_up(state).then(|| {
        div()
            .absolute()
            .bottom(px(12.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("agent-to-end")
                    .relative()
                    .size(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(ui.surface)
                    .border_1()
                    .border_color(ui.hairline)
                    .shadow(vec![BoxShadow {
                        color: ui.shadow,
                        offset: point(px(0.0), px(2.0)),
                        blur_radius: px(8.0),
                        spread_radius: px(0.0),
                        inset: false,
                    }])
                    .cursor_pointer()
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| {
                        ws.templum_agent.list.set_follow_mode(FollowMode::Tail);
                        cx.notify();
                    }))
                    .child(Icon::new(IconName::ArrowDown).size(px(16.0)).text_color(fg))
                    .when(agent.unseen.get(), |el| {
                        el.child(
                            div()
                                .id("agent-to-end-new")
                                .absolute()
                                .top(px(-2.0))
                                .right(px(-2.0))
                                .size(px(8.0))
                                .rounded_full()
                                .bg(ui.accent)
                                .test_support(),
                        )
                    })
                    .tooltip(|window, cx| Tooltip::new("Scroll to the end").build(window, cx))
                    .test_support(),
            )
            .with_animation(
                "agent-to-end-fade",
                Animation::new(crate::motion::HOVER),
                |el, t| el.opacity(t),
            )
    });
    // A pinned approval needs room: with one the card may take the canvas less its insets.
    let max_h = if approval.is_some() {
        canvas_h - 2.0 * INSET
    } else {
        canvas_h * MAX_SHARE
    }
    .max(MIN_H);
    let chat = div()
        .id("agent-chat")
        .flex()
        .flex_col()
        .w(px(width(canvas_w, full)))
        .min_h(px(MIN_H))
        .when(full, |el| el.h(px((canvas_h - 2.0 * INSET).max(MIN_H))))
        .when(!full, |el| el.max_h(px(max_h)))
        .rounded(px(super::look::CARD_RADIUS))
        .bg(ui.surface)
        .border_1()
        .border_color(ui.hairline)
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(0.0), px(10.0)),
            blur_radius: px(30.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .overflow_hidden()
        .on_key_down(cx.listener(Workspace::on_agent_key))
        // The canvas under it gets none of its presses; a press outside doesn't close it.
        .occlude()
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
            cx.stop_propagation()
        })
        .child(header)
        .children(tasks)
        .child(
            div()
                .relative()
                .min_h(px(0.0))
                // The greeting has a height of its own; so has full screen's body.
                .when(full || greets(rows), |el| el.flex_1())
                .when(!full && !greets(rows), |el| {
                    el.flex_grow(1.0)
                        .flex_shrink(1.0)
                        .flex_basis(px(agent.rows_h.get()))
                })
                .child(
                    div()
                        .id("agent-body")
                        .size_full()
                        .child(body)
                        .children(measure)
                        .test_support(),
                )
                .children(to_end),
        )
        // An approval is pinned over its field.
        .children(approval.map(|a| {
            div()
                .flex_none()
                .flex()
                .justify_center()
                .child(div().w(px(col)).child(a))
        }))
        .child(
            div()
                .flex_none()
                .flex()
                .justify_center()
                .pt(px(8.0))
                .pb(px(16.0))
                .child(div().w(px(col)).child(footer)),
        )
        .test_support();
    Some(
        chat.with_animation(
            "agent-chat-rise",
            Animation::new(APPEAR).with_easing(crate::motion::slide),
            |el, t| el.opacity(t).mt(px(RISE * (1.0 - t))),
        )
        .into_any_element(),
    )
}
