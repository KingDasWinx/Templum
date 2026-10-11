//! The floating toolbar at the bottom of the canvas: pointer tools, block tools and the AI
//! agents. See docs/specs/cockpit.md, section 3.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, MouseButton, MouseDownEvent, ScrollWheelEvent,
    SharedString, Stateful, TestSupportExt, div, point, px,
};

use crate::keys::{self, Command};
use crate::menu::Target;
use crate::theme::Ui;
use crate::workspace::{Tool, Workspace};

const BUTTON: f32 = 32.0;
/// Clear of the canvas's bottom edge.
const MARGIN: f32 = 12.0;

pub(crate) fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    let tool = ws.tool;
    // "Terminal · Ctrl+Shift+T", or the name alone without a shortcut.
    let tip = |name: &str, command: Command| match ws.keymap.first(command) {
        Some(k) => format!("{name} · {}", keys::display(k)),
        None => name.to_string(),
    };
    let group = |children: Vec<AnyElement>| {
        div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .children(children)
    };
    let pointer = group(vec![
        button(
            "tool-select",
            IconName::MousePointer2,
            "Select · Esc",
            tool == Tool::Select,
            ui,
        )
        .on_click(
            cx.listener(|ws, _: &ClickEvent, window, cx| ws.set_tool(Tool::Select, window, cx)),
        )
        .test_support()
        .into_any_element(),
        button(
            "tool-hand",
            IconName::Hand,
            "Hand: move the canvas over blocks",
            tool == Tool::Hand,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.set_tool(Tool::Hand, window, cx)))
        .test_support()
        .into_any_element(),
    ]);
    let blocks = group(vec![
        button(
            "tool-terminal",
            IconName::SquareTerminal,
            tip("Terminal", Command::NewTerminal),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_terminal(window, cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-note",
            IconName::StickyNote,
            tip("Note", Command::NewNote),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_note(window, cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-kanban",
            IconName::LayoutDashboard,
            tip("Kanban board", Command::NewKanban),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_kanban(window, cx)))
        .test_support()
        .into_any_element(),
        button("tool-agent", IconName::Bot, "AI agent", false, ui)
            .on_click(cx.listener(|ws, e: &ClickEvent, window, cx| {
                // The agent's block opens in the middle of the canvas.
                let at = ws.space.canvas.camera.to_world(ws.center(window));
                ws.open_menu(Target::Agents(at), e.position(), window, cx);
            }))
            .test_support()
            .into_any_element(),
    ]);
    div()
        .absolute()
        .bottom(px(MARGIN))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .id("toolbar")
                .flex()
                .items_center()
                .h(px(BUTTON + 8.0))
                .rounded(px(12.0))
                .bg(ui.surface)
                .border_1()
                .border_color(ui.hairline)
                .shadow(vec![BoxShadow {
                    color: ui.shadow,
                    offset: point(px(0.0), px(4.0)),
                    blur_radius: px(12.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                // Nothing under the toolbar reacts to its clicks or its wheel.
                .block_mouse_except_scroll()
                .on_scroll_wheel(|_: &ScrollWheelEvent, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
                    cx.stop_propagation()
                })
                .child(pointer)
                .child(div().w(px(1.0)).h(px(20.0)).bg(ui.hairline))
                .child(blocks)
                .test_support(),
        )
}

fn button(
    id: &'static str,
    icon: IconName,
    tip: impl Into<SharedString>,
    active: bool,
    ui: Ui,
) -> Stateful<Div> {
    let tip = tip.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(BUTTON))
        .rounded(px(8.0))
        .cursor_pointer()
        .when(active, |el| el.bg(ui.card))
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(
            Icon::new(icon)
                .size(px(16.0))
                .text_color(if active { ui.accent } else { ui.title }),
        )
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
}
