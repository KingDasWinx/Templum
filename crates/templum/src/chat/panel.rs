//! The chat (docs/specs/templum-agent.md#the-chat): a floating card that rises from the pill, the
//! pill its footer; window chrome over the canvas, which works around it.

use std::time::Duration;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, Animation, AnimationExt as _, AnyElement, BoxShadow, ClickEvent, Context, ElementId,
    FollowMode, MouseButton, MouseDownEvent, TestSupportExt, Window, div, list, point, px,
};

use crate::settings::templum_agent::set_model;
use crate::workspace::Workspace;

const WIDTH: f32 = 640.0;
const SIDES: f32 = 32.0;
/// Full screen leaves this much canvas on every side.
const INSET: f32 = 16.0;
const MIN_H: f32 = 240.0;
/// At most this much of the canvas's height.
const MAX_SHARE: f32 = 0.7;
const HEADER: f32 = 36.0;
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

/// The width of an approval pinned in the open card: the card's less its border and the padding
/// around the approval.
pub(crate) fn approval_width(canvas_w: f32, full: bool) -> f32 {
    width(canvas_w, full) - 2.0 - 2.0 * 8.0
}

/// The person's message Catch me up sends.
pub(crate) const CATCH_UP: &str = "Catch me up: what's going on?";

/// What an empty conversation offers; a click puts its text in the field (Catch me up sends).
pub(crate) const SUGGESTIONS: [&str; 3] = [
    "Set up a workspace for a project, with a kanban and an agent per card",
    CATCH_UP,
    "Tidy this workspace into a grid",
];

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
    let button = |id: &'static str, icon: IconName, tip: &'static str| {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_center()
            .size(px(24.0))
            .rounded(px(6.0))
            .cursor_pointer()
            .hover(move |s| s.bg(ui.hover))
            .child(Icon::new(icon).size(px(14.0)).text_color(ui.title_dim))
            .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
    };
    let header = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .h(px(HEADER))
        .px(px(12.0))
        .border_b_1()
        .border_color(ui.hairline)
        .child(
            Icon::new(IconName::Sparkles)
                .size(px(14.0))
                .text_color(ui.accent),
        )
        .child(
            div()
                .text_size(px(13.0))
                .text_color(ui.title)
                .child("Templum Agent"),
        )
        .child(model_chip(ws, cx))
        .child(div().flex_1())
        .when_some(meter(ws), |el, meter| {
            el.child(
                div()
                    .id("agent-meter")
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(6.0))
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    .cursor_pointer()
                    .hover(move |s| s.bg(ui.hover))
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| {
                        ws.templum_agent.tasks_open = !ws.templum_agent.tasks_open;
                        cx.notify();
                    }))
                    .child(meter)
                    .tooltip(|window, cx| Tooltip::new("Tasks under way").build(window, cx))
                    .test_support(),
            )
        })
        .when(ws.agent_tasks() > 0, |el| {
            el.child(
                div()
                    .id("agent-catch-up")
                    .px(px(8.0))
                    .py(px(2.0))
                    .rounded(px(6.0))
                    .text_size(px(12.0))
                    .text_color(ui.title_dim)
                    .cursor_pointer()
                    .hover(move |s| s.bg(ui.hover))
                    .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                        ws.send_to_agent(CATCH_UP.to_string(), window, cx)
                    }))
                    .child("Catch me up")
                    .test_support(),
            )
        })
        .when(agent.conversation.working, |el| {
            el.child(
                button("agent-stop", IconName::CircleStop, "Stop")
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.stop_engine(cx)))
                    .test_support(),
            )
        })
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
            button("agent-new", IconName::MessageSquarePlus, "New chat")
                .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.new_chat(cx)))
                .test_support(),
        )
        .child(
            button("agent-close", IconName::ChevronDown, "Close")
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.leave_agent(window, cx)))
                .test_support(),
        );
    // The meter's list: a line per task under way; a click shows its block.
    let tasks = agent.tasks_open.then(|| {
        div()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(8.0))
            .border_b_1()
            .border_color(ui.hairline)
            .children(
                ws.agent_task_blocks()
                    .into_iter()
                    .enumerate()
                    .map(|(n, (space, id))| {
                        let title = ws
                            .spaces()
                            .find(|s| s.id == space)
                            .and_then(|s| s.blocks.get(&id))
                            .map(|b| crate::block::title(ws, b, cx).to_string())
                            .unwrap_or_default();
                        div()
                            .id(ElementId::Name(format!("agent-task-{n}").into()))
                            .px(px(6.0))
                            .py(px(2.0))
                            .rounded(px(6.0))
                            .text_size(px(12.0))
                            .text_color(ui.title)
                            .cursor_pointer()
                            .hover(move |s| s.bg(ui.hover))
                            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                ws.reveal(&space, id, window, cx)
                            }))
                            .child(title)
                            .test_support()
                    }),
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
    agent.seen.set(rows.len());
    let body = if rows.is_empty() {
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(16.0))
            .children(SUGGESTIONS.iter().enumerate().map(|(n, text)| {
                div()
                    .id(ElementId::Name(format!("agent-suggestion-{n}").into()))
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(px(12.0))
                    .border_1()
                    .border_color(ui.hairline)
                    .text_size(px(13.0))
                    .text_color(ui.title)
                    .cursor_pointer()
                    .hover(move |s| s.bg(ui.hover))
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.suggest_to_agent(text, window, cx)
                    }))
                    .child(*text)
                    .test_support()
            }))
            .into_any_element()
    } else {
        let row = cx.processor(|ws: &mut Workspace, n: usize, _, cx| {
            let row = ws.templum_agent.conversation.rows.get(n).cloned();
            let row = row.map(|row| super::rows::row(ws, n, &row, cx));
            div()
                .px(px(12.0))
                .py(px(4.0))
                .children(row)
                .into_any_element()
        });
        list(state.clone(), row)
            .size_full()
            .py(px(8.0))
            .into_any_element()
    };
    let new_below = agent.unseen.get().then(|| {
        div()
            .absolute()
            .bottom(px(8.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .id("agent-new-below")
                    .px(px(10.0))
                    .py(px(2.0))
                    .rounded_full()
                    .bg(ui.accent)
                    .text_size(px(12.0))
                    .text_color(ui.canvas)
                    .cursor_pointer()
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| {
                        ws.templum_agent.list.set_follow_mode(FollowMode::Tail);
                        cx.notify();
                    }))
                    .child("↓ New")
                    .test_support(),
            )
    });
    let max_h = (canvas_h * MAX_SHARE).max(MIN_H);
    let chat = div()
        .id("agent-chat")
        .flex()
        .flex_col()
        .w(px(width(canvas_w, full)))
        .min_h(px(MIN_H))
        .when(full, |el| el.h(px((canvas_h - 2.0 * INSET).max(MIN_H))))
        .when(!full, |el| el.max_h(px(max_h)))
        .rounded(px(14.0))
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
                .flex_1()
                .min_h(px(0.0))
                .child(div().id("agent-body").size_full().child(body))
                .children(new_below),
        )
        // An approval is pinned over its field.
        .children(approval.map(|a| div().flex_none().px(px(8.0)).pt(px(8.0)).child(a)))
        .child(
            div()
                .flex_none()
                .p(px(8.0))
                .border_t_1()
                .border_color(ui.hairline)
                .child(footer),
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

/// The engine chip: the agent and its model; a click opens the models Settings offers, the
/// current one checked, and Another model… goes to Settings ▸ Templum Agent to type one.
fn model_chip(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let current = ws.config.templum_agent.model.clone();
    let models = ws.engine_models();
    let ws_entity = cx.entity();
    Button::new("agent-model")
        .ghost()
        .small()
        .dropdown_caret(true)
        .label(super::pill::engine(ws))
        .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
            let other = !current.is_empty() && !models.contains(&current);
            let choices = std::iter::once((String::new(), "Default".to_string()))
                .chain(models.iter().map(|m| (m.clone(), m.clone())));
            let menu = choices.fold(menu, |menu, (model, label)| {
                let ws = ws_entity.clone();
                menu.item(
                    PopupMenuItem::new(label)
                        .checked(model == current)
                        .on_click(move |_, window, cx| {
                            ws.update(cx, |ws, cx| {
                                set_model(ws, &model, cx);
                                // The button took the keyboard from the field: it goes back.
                                let input = ws.agent_input(window, cx);
                                input.update(cx, |input, cx| input.focus(window, cx));
                            })
                        }),
                )
            });
            let ws = ws_entity.clone();
            menu.item(
                PopupMenuItem::new("Another model…")
                    .checked(other)
                    .on_click(move |_, window, cx| {
                        ws.update(cx, |ws, cx| ws.open_agent_settings(window, cx))
                    }),
            )
        })
}
