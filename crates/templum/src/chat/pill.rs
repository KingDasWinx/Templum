//! The pill (docs/specs/templum-agent.md#the-pill): the Templum Agent's input, floating over the
//! canvas at the bottom center, above the toolbar.

use gpui_kit::Focusable as _;
use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Sizable as _, Size as UiSize};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, Animation, AnimationExt as _, AnyElement, BoxShadow, ClickEvent, Context, MouseButton,
    MouseDownEvent, SharedString, SpringAnimation, SpringConfig, TestSupportExt, Window, div,
    point, px, relative,
};

use crate::grid::hsla;

use crate::keys::Command;
use crate::settings::templum_agent::set_model;
use crate::workspace::Workspace;

/// At rest.
const REST_W: f32 = 300.0;
const HEIGHT: f32 = 36.0;
/// While it has the keyboard or text, at most the canvas's width less this.
const WIDE_W: f32 = 640.0;
const SIDES: f32 = 32.0;
/// It grows and shrinks between the two, eased out: critically damped (no overshoot), there in
/// about 0.18 s; at once with less motion.
const GROW: SpringConfig = SpringConfig::new(1369.0, 74.0, 1.0);
/// How long the working line's band takes to sweep across.
const SWEEP: std::time::Duration = std::time::Duration::from_millis(1600);

/// The model chip is cut past this.
const CHIP_MAX: f32 = 220.0;

/// What the pill says at rest.
pub(crate) fn rest(ws: &Workspace) -> &'static str {
    if ws.templum_agent.installed() {
        "Ask Templum"
    } else {
        "Claude Code isn't installed"
    }
}

/// The engine chip: the agent and its model ("Claude Code · opus"): the one the engine reported,
/// else the one chosen.
pub(crate) fn engine(ws: &Workspace) -> String {
    let settings = &ws.config.templum_agent;
    let model = ws
        .templum_agent
        .model
        .clone()
        .or_else(|| Some(settings.model.clone()).filter(|m| !m.is_empty()));
    match model {
        Some(model) => format!("Claude Code · {model}"),
        None => "Claude Code".to_string(),
    }
}

/// The pill, `canvas_w` the canvas's width; as the open chat's `footer`, as wide as it. None
/// when it is turned off and has neither the keyboard nor the chat.
pub(crate) fn render(
    ws: &Workspace,
    canvas_w: f32,
    footer: bool,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    let ui = ws.ui;
    let agent = &ws.templum_agent;
    let typing = ws.agent_has_keys(window, cx);
    // The field itself has the keyboard (not the approval): its border and ring.
    let field_focused = agent
        .input
        .as_ref()
        .is_some_and(|i| i.read(cx).focus_handle(cx).is_focused(window));
    if !ws.config.templum_agent.pill && !typing && !footer && !agent.model_menu {
        return None;
    }
    let text = agent
        .input
        .as_ref()
        .map(|i| i.read(cx).value())
        .unwrap_or_default();
    // The chat's footer is always the composer: Stop and the model chip stay while it works.
    let wide = footer || typing || !text.is_empty() || agent.model_menu;
    let width = if wide {
        WIDE_W.min(canvas_w - SIDES)
    } else {
        REST_W
    };
    let installed = agent.installed();
    let yellow = hsla(ws.theme.palette.ansi[3], 1.0);
    let asks = agent.approvals.first().filter(|_| !wide);
    let working = agent.conversation.working && !wide && asks.is_none();
    let border = if field_focused {
        ui.accent
    } else if asks.is_some() {
        yellow
    } else {
        ui.hairline
    };
    let icon = if agent.conversation.working {
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
    // Something new since the chat was last open: a dot on the icon.
    let icon = div()
        .relative()
        .flex_none()
        .child(icon)
        .when(agent.unread && !agent.open, |el| {
            el.child(
                div()
                    .id("agent-unread")
                    .absolute()
                    .top(px(-2.0))
                    .right(px(-2.0))
                    .size(px(6.0))
                    .rounded_full()
                    .bg(ui.accent)
                    .test_support(),
            )
        });
    let pill = div()
        .id("agent-pill")
        .relative()
        .when(footer, |el| el.w_full())
        .rounded(px(18.0))
        .bg(ui.surface)
        .border_1()
        .border_color(border)
        .shadow(
            // The soft ring of a field with the keyboard, then the floating pill's shadow.
            field_focused
                .then(|| BoxShadow {
                    color: ui.ring,
                    offset: point(px(0.0), px(0.0)),
                    blur_radius: px(0.0),
                    spread_radius: px(2.0),
                    inset: false,
                })
                .into_iter()
                .chain((!footer).then(|| BoxShadow {
                    color: ui.shadow,
                    offset: point(px(0.0), px(4.0)),
                    blur_radius: px(12.0),
                    spread_radius: px(0.0),
                    inset: false,
                }))
                .collect(),
        )
        .on_key_down(cx.listener(Workspace::on_agent_key))
        // Nothing under the pill reacts to its clicks.
        .block_mouse_except_scroll()
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
            cx.stop_propagation()
        });
    // At rest, or asking: the icon and one line.
    let one_line = |pill: gpui_kit::Stateful<gpui_kit::Div>| {
        pill.flex()
            .items_center()
            .gap(px(8.0))
            .px(px(14.0))
            .min_h(px(HEIGHT))
            .child(icon)
    };
    let line = |text: SharedString, id: &'static str, color| {
        div()
            .id(id)
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .text_size(px(13.0))
            .text_color(color)
            .child(text)
            .test_support()
    };
    let pill = match (&agent.input, wide) {
        (Some(input), true) => {
            let working = agent.conversation.working;
            let has_text = !text.trim().is_empty();
            let fg = hsla(ws.theme.palette.foreground, 1.0);
            let on_accent = ws.theme.on_accent();
            let round = |id: &'static str| {
                div()
                    .id(id)
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.0))
                    .rounded_full()
            };
            // Send: always there while it doesn't work, dim and dead without text; beside Stop
            // when text waits (it is queued).
            let send = (!working || has_text).then(|| {
                let el = round("agent-send").child(
                    Icon::new(IconName::ArrowUp)
                        .size(px(15.0))
                        .text_color(if has_text { on_accent } else { ui.title_dim }),
                );
                if has_text {
                    el.bg(ui.accent)
                        .cursor_pointer()
                        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                            ws.submit_to_agent(window, cx)
                        }))
                        .tooltip(|window, cx| Tooltip::new("Send").build(window, cx))
                } else {
                    el.bg(ui.hover)
                }
                .test_support()
            });
            let stop = working.then(|| {
                round("agent-stop")
                    .bg(ui.accent)
                    .cursor_pointer()
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.stop_engine(cx)))
                    .child(div().size(px(10.0)).rounded(px(2.0)).bg(on_accent))
                    .tooltip(|window, cx| Tooltip::new("Stop").build(window, cx))
                    .test_support()
            });
            let bar = div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .h(px(28.0))
                .child(
                    div()
                        .flex_none()
                        .max_w(px(CHIP_MAX))
                        .overflow_hidden()
                        .child(model_chip(ws, cx)),
                )
                .child(div().flex_1())
                .children(send)
                .children(stop)
                .with_animation(
                    "agent-bar-fade",
                    Animation::new(crate::motion::HOVER),
                    |el, t| el.opacity(t),
                );
            pill.flex()
                .flex_col()
                .gap(px(4.0))
                .pt(px(12.0))
                .pb(px(8.0))
                .pl(px(14.0))
                .pr(px(8.0))
                .child(
                    div()
                        .min_w(px(0.0))
                        .text_size(px(14.0))
                        .text_color(fg)
                        .child(Textarea::new(input).appearance(false)),
                )
                .child(bar)
        }
        _ if asks.is_some() => {
            let said = asks.map(|a| a.said.clone()).unwrap_or_default();
            let asked: SharedString = format!("Templum Agent asks you: {said}").into();
            let tip = asked.clone();
            // A click on it is for its field; Review is for the approval.
            one_line(pill)
                .cursor_pointer()
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.toggle_agent(window, cx)))
                // The line may be cut short: the whole question.
                .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
                .child(div().flex_none().size(px(8.0)).rounded_full().bg(yellow))
                .child(line(asked, "agent-asks", ui.title))
                .child(
                    div()
                        .id("agent-review")
                        .flex_none()
                        .flex()
                        .items_center()
                        .h(px(22.0))
                        .px(px(10.0))
                        .rounded_full()
                        .border_1()
                        .border_color(yellow)
                        .text_size(px(12.0))
                        .text_color(hsla(ws.theme.palette.foreground, 1.0))
                        .cursor_pointer()
                        .hover(move |s| s.bg(ui.hover))
                        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                            cx.stop_propagation();
                            ws.review_agent(window, cx)
                        }))
                        .child("Review")
                        .test_support(),
                )
        }
        _ => {
            let shortcut = ws.keymap.first(Command::Agent);
            let under_way = working.then(|| doing(ws));
            let tip = if installed {
                "Ask the Templum Agent"
            } else {
                "Set up the Templum Agent"
            };
            one_line(pill)
                .cursor_pointer()
                .hover(move |s| s.border_color(ui.hover_border))
                .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.toggle_agent(window, cx)))
                .tooltip(move |window, cx| Tooltip::new(tip).build(window, cx))
                .child(match under_way {
                    Some(doing) => line(doing.into(), "agent-working", ui.title_dim),
                    None if installed => line(rest(ws).into(), "agent-rest", ui.title_dim),
                    None => line(rest(ws).into(), "agent-missing", ui.title_dim),
                })
                .when_some(
                    shortcut
                        .filter(|_| installed && !working)
                        .and_then(|k| gpui_kit::Keystroke::parse(k).ok()),
                    |el, keystroke| el.child(div().opacity(0.6).child(Kbd::new(keystroke))),
                )
                // While it works: a line along the bottom edge, a light band sweeping across it.
                .when(working, |el| {
                    el.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left(px(14.0))
                            .right(px(14.0))
                            .h(px(2.0))
                            .bg(ui.accent.opacity(0.35))
                            .child(
                                div()
                                    .absolute()
                                    .top_0()
                                    .h_full()
                                    .w(relative(0.3))
                                    .bg(ui.accent)
                                    .with_animation(
                                        "agent-sweep",
                                        Animation::new(SWEEP).repeat(),
                                        |el, t| el.left(relative(t * 0.7)),
                                    ),
                            ),
                    )
                })
        }
    };
    let pill = pill.test_support();
    Some(if footer {
        pill.into_any_element()
    } else {
        pill.with_spring(
            "agent-pill-grow",
            SpringAnimation::new(GROW).to(px(width)),
            |el, w| el.w(w),
        )
        .into_any_element()
    })
}

/// The last action under way, as its row says while it runs: "Opening an agent…".
pub(crate) fn doing(ws: &Workspace) -> String {
    ws.templum_agent
        .conversation
        .rows
        .iter()
        .rev()
        .find_map(|r| match r {
            templum_agent::Row::Action {
                tool,
                state: templum_agent::ActionState::Running,
                ..
            } => Some(format!("{}…", templum_mcp::control::title(tool))),
            _ => None,
        })
        .unwrap_or_else(|| "Working…".to_string())
}

/// The composer's chip: the agent and its model; a click opens, upward, the models Settings
/// offers, the current one checked, and Another model… goes to Settings ▸ Templum Agent to type one.
fn model_chip(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    let current = ws.config.templum_agent.model.clone();
    let models = ws.engine_models();
    let ws_entity = cx.entity();
    let menu_open = ws_entity.clone();
    Button::new("agent-model")
        .ghost()
        .small()
        .dropdown_caret(true)
        .rounded(px(12.0))
        .h(px(24.0))
        .text_size(px(12.0))
        .text_color(ui.title_dim)
        .label(engine(ws))
        .dropdown_menu_with_anchor(Anchor::BottomLeft, move |menu, _, _| {
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
        // The menu takes the keyboard from the field: the composer stays while it is open.
        .on_open_change(move |open, _, cx| {
            menu_open.update(cx, |ws, cx| {
                ws.templum_agent.model_menu = *open;
                cx.notify();
            })
        })
}
