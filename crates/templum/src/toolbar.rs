//! The floating toolbar at the bottom of the canvas: pointer tools, block tools, the AI agents
//! and the drawing tools, with the stroke options above it. See docs/specs/cockpit.md, section 3.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, BoxShadow, ClickEvent, Context, Div, ElementId, MouseButton, MouseDownEvent,
    ScrollWheelEvent, SharedString, Stateful, TestSupportExt, div, point, px,
};
use templum_canvas::{Color, ShapeKind};

use crate::ink;
use crate::keys::{self, Command};
use crate::menu::Target;
use crate::theme::Ui;
use crate::workspace::remote::Deed;
use crate::workspace::{Tool, Workspace};

const BUTTON: f32 = 32.0;
/// Clear of the canvas's bottom edge.
const MARGIN: f32 = 12.0;
/// The strip at the bottom of the canvas the toolbar covers.
pub(crate) const SPACE: f32 = BUTTON + 8.0 + MARGIN;
/// The height of the pill of options above it (the shapes, the stroke options).
const OPTIONS: f32 = 32.0;

/// How far above the canvas's bottom what floats over the toolbar sits (the Templum Agent's
/// pill): 8 px above it, or above its options while they show.
pub(crate) fn above(ws: &Workspace) -> f32 {
    let options = ws.shapes_open || (ws.tool.draws() && ws.tool != Tool::Eraser);
    SPACE + 8.0 + if options { OPTIONS + 8.0 } else { 0.0 }
}

pub(crate) fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    // Holding Alt is the hand while it lasts.
    let tool = if ws.hand { Tool::Hand } else { ws.tool };
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
            // Held, Alt is the hand while it lasts (Option on macOS).
            if cfg!(target_os = "macos") {
                "Hand: move the canvas over blocks · Option"
            } else {
                "Hand: move the canvas over blocks · Alt"
            },
            tool == Tool::Hand,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.set_tool(Tool::Hand, window, cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-wire",
            IconName::Cable,
            gated(ws, Deed::Wire, "Wire: connect two blocks"),
            tool == Tool::Wire,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.set_tool(Tool::Wire, window, cx)))
        .test_support()
        .into_any_element(),
    ]);
    let blocks = group(vec![
        button(
            "tool-terminal",
            IconName::SquareTerminal,
            gated(ws, Deed::AddTerminal, tip("Terminal", Command::NewTerminal)),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, e: &ClickEvent, window, cx| {
            // The shells to pick from (a mirror's are home's); Ctrl+Shift+T opens the default
            // without a menu.
            ws.open_menu(Target::Shells, e.position(), window, cx)
        }))
        .test_support()
        .into_any_element(),
        button(
            "tool-note",
            IconName::StickyNote,
            gated(ws, Deed::AddNote, tip("Note", Command::NewNote)),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_note(window, cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-checklist",
            IconName::ListChecks,
            gated(ws, Deed::AddChecklist, "Checklist"),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_checklist(None, window, cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-kanban",
            IconName::LayoutDashboard,
            gated(ws, Deed::AddKanban, tip("Kanban board", Command::NewKanban)),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_kanban(window, cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-editor",
            IconName::FileCode,
            gated(ws, Deed::AddEditor, tip("Editor", Command::NewEditor)),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_editor(window, cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-agent",
            IconName::Bot,
            gated(ws, Deed::OpenAgent, "AI agent"),
            false,
            ui,
        )
        .on_click(cx.listener(|ws, e: &ClickEvent, window, cx| {
            // The agent's block opens in the middle of the canvas.
            let at = ws.space.canvas.camera.to_world(ws.center(window));
            ws.open_menu(Target::Agents(at), e.position(), window, cx);
        }))
        .test_support()
        .into_any_element(),
    ]);
    let drawing = group(vec![
        button(
            "tool-pen",
            IconName::Pencil,
            gated(ws, Deed::Draw, "Pen"),
            tool == Tool::Pen,
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.set_tool(Tool::Pen, window, cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-highlighter",
            IconName::Highlighter,
            gated(ws, Deed::Draw, "Highlighter"),
            tool == Tool::Highlighter,
            ui,
        )
        .on_click(
            cx.listener(|ws, _: &ClickEvent, window, cx| {
                ws.set_tool(Tool::Highlighter, window, cx)
            }),
        )
        .test_support()
        .into_any_element(),
        button(
            "tool-shapes",
            ws.last_shape.map_or(IconName::Shapes, shape_icon),
            gated(ws, Deed::Draw, "Shapes"),
            matches!(tool, Tool::Shape(_)),
            ui,
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.toggle_shapes(cx)))
        .test_support()
        .into_any_element(),
        button(
            "tool-eraser",
            IconName::Eraser,
            gated(ws, Deed::Draw, "Eraser"),
            tool == Tool::Eraser,
            ui,
        )
        .on_click(
            cx.listener(|ws, _: &ClickEvent, window, cx| ws.set_tool(Tool::Eraser, window, cx)),
        )
        .test_support()
        .into_any_element(),
    ]);
    div()
        .absolute()
        .bottom(px(MARGIN))
        .left_0()
        .right_0()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.0))
        .children(options(ws, cx))
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
                .child(div().w(px(1.0)).h(px(20.0)).bg(ui.hairline))
                .child(drawing)
                .test_support(),
        )
}

/// Above the toolbar: the shapes pill while it is open, else the stroke options while a drawing
/// tool other than the eraser is on: the colors, and the widths (the highlighter has one).
fn options(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<AnyElement> {
    let ui = ws.ui;
    if ws.shapes_open {
        let kinds = ShapeKind::ALL.map(|kind| {
            button(
                shape_id(kind),
                shape_icon(kind),
                shape_name(kind),
                ws.tool == Tool::Shape(kind),
                ui,
            )
            .on_click(
                cx.listener(move |ws, _: &ClickEvent, window, cx| ws.pick_shape(kind, window, cx)),
            )
            .test_support()
            .into_any_element()
        });
        return Some(
            pill("ink-shapes", ui)
                .children(kinds)
                .test_support()
                .into_any_element(),
        );
    }
    if !ws.tool.draws() || ws.tool == Tool::Eraser {
        return None;
    }
    let swatches = Color::ALL.map(|color| {
        let on = ws.ink_color == color;
        div()
            .id(ElementId::Name(
                format!("ink-color-{}", color.name()).into(),
            ))
            .size(px(20.0))
            .rounded_full()
            .bg(ink::color_of(color, &ws.theme, false))
            .border_2()
            .border_color(if on {
                ui.accent
            } else {
                gpui_kit::transparent_black()
            })
            .cursor_pointer()
            .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.set_ink_color(color, cx)))
            .test_support()
            .into_any_element()
    });
    let widths: Vec<AnyElement> = if ws.tool == Tool::Highlighter {
        Vec::new()
    } else {
        ink::Width::ALL
            .map(|width| {
                let on = ws.ink_width == width;
                div()
                    .id(ElementId::Name(
                        format!("ink-width-{}", width.name()).into(),
                    ))
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(28.0))
                    .rounded(px(6.0))
                    .when(on, |el| el.bg(ui.card))
                    .hover(move |s| s.bg(ui.hover))
                    .cursor_pointer()
                    .child(
                        div()
                            .rounded_full()
                            .bg(ui.title)
                            .size(px(width.units() * 1.5)),
                    )
                    .on_click(
                        cx.listener(move |ws, _: &ClickEvent, _, cx| ws.set_ink_width(width, cx)),
                    )
                    .test_support()
                    .into_any_element()
            })
            .into()
    };
    Some(
        pill("ink-options", ui)
            .children(swatches)
            .when(!widths.is_empty(), |el| {
                el.child(div().w(px(1.0)).h(px(18.0)).bg(ui.hairline))
                    .children(widths)
            })
            .test_support()
            .into_any_element(),
    )
}

/// The small pill above the toolbar.
fn pill(id: &'static str, ui: Ui) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .h(px(OPTIONS))
        .rounded(px(12.0))
        .bg(ui.surface)
        .border_1()
        .border_color(ui.hairline)
        // Nothing under the pill reacts to its clicks.
        .block_mouse_except_scroll()
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| {
            cx.stop_propagation()
        })
}

fn shape_icon(kind: ShapeKind) -> IconName {
    match kind {
        ShapeKind::Line => IconName::Minus,
        ShapeKind::Arrow => IconName::ArrowUpRight,
        ShapeKind::Rect => IconName::Square,
        ShapeKind::Ellipse => IconName::Circle,
    }
}

fn shape_id(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Line => "shape-line",
        ShapeKind::Arrow => "shape-arrow",
        ShapeKind::Rect => "shape-rect",
        ShapeKind::Ellipse => "shape-ellipse",
    }
}

fn shape_name(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Line => "Line",
        ShapeKind::Arrow => "Arrow",
        ShapeKind::Rect => "Rectangle",
        ShapeKind::Ellipse => "Ellipse",
    }
}

/// What a button's tooltip says, and whether the button is dimmed.
struct Tip(SharedString, bool);

impl From<&'static str> for Tip {
    fn from(tip: &'static str) -> Tip {
        Tip(tip.into(), false)
    }
}

impl From<(SharedString, bool)> for Tip {
    fn from((tip, dim): (SharedString, bool)) -> Tip {
        Tip(tip, dim)
    }
}

/// A control a mirrored workspace can't use now is dimmed, and its tooltip says why: the rule's
/// words for `deed`, else `tip`.
pub(crate) fn gated(
    ws: &Workspace,
    deed: Deed,
    tip: impl Into<SharedString>,
) -> (SharedString, bool) {
    match ws.why_not(deed) {
        Some(why) => (why.into(), true),
        None => (tip.into(), false),
    }
}

fn button(
    id: &'static str,
    icon: IconName,
    tip: impl Into<Tip>,
    active: bool,
    ui: Ui,
) -> Stateful<Div> {
    let Tip(tip, dim) = tip.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(BUTTON))
        .rounded(px(8.0))
        .cursor_pointer()
        .when(active, |el| el.bg(ui.card))
        .when(dim, |el| el.opacity(0.4))
        .aria_selected(active)
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(
            Icon::new(icon)
                .size(px(16.0))
                .text_color(if active { ui.accent } else { ui.title }),
        )
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
}
