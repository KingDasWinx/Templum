//! The Instructions section: a row per instruction, "Add an instruction", and the dialog that
//! edits or adds one (docs/specs/instructions.md).

use gpui_kit::assets::IconName;
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, SharedString, TestSupportExt, Window,
    div, px,
};

use super::row::{Row, menu_button};
use crate::grid::hsla;
use crate::instructions::{Draft, Instruction};
use crate::theme::Ui;
use crate::workspace::{Workspace, show_notification};

/// One instruction: where and for whom; its switch; Edit.
pub(super) fn instruction_row(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    instruction: &Instruction,
) -> Row {
    let ui = ws.ui;
    let id = instruction.id;
    let on = {
        let e = entity.clone();
        Switch::new(ElementId::Name(format!("instruction-on-{id}").into()))
            .checked(instruction.on)
            .on_click(move |on: &bool, _, cx| {
                let on = *on;
                e.update(cx, |ws, cx| ws.set_instruction_on(id, on, cx));
            })
    };
    let edit = {
        let (e, instruction) = (entity.clone(), instruction.clone());
        div()
            .id(ElementId::Name(format!("edit-instruction-{id}").into()))
            .px(px(10.0))
            .py(px(3.0))
            .rounded_full()
            .border_1()
            .border_color(ui.hairline)
            .text_size(px(12.0))
            .cursor_pointer()
            .hover(move |s| s.bg(ui.hover))
            .active(move |s| s.bg(ui.pressed))
            .child("Edit")
            .on_click(move |_: &ClickEvent, window, cx| {
                let instruction = instruction.clone();
                e.update(cx, |ws, cx| {
                    instruction_dialog(ws, Some(instruction), window, cx)
                })
            })
            .test_support()
    };
    Row::new(format!("instructions.{id}"), instruction.name.clone())
        .description(description(ws, instruction))
        .control(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(on)
                .child(edit),
        )
}

/// What an instruction's row says under its name: where it applies, then for whom.
pub(crate) fn description(ws: &Workspace, instruction: &Instruction) -> String {
    let place = match &instruction.workspace {
        None => "Every workspace".to_string(),
        Some(id) => ws
            .spaces()
            .find(|s| s.id == *id)
            .map_or("A deleted workspace".to_string(), |s| s.name.clone()),
    };
    let who = match &instruction.agents {
        None => "Every agent".to_string(),
        Some(ids) if ids.is_empty() => "No agent".to_string(),
        Some(ids) => ids
            .iter()
            .map(|id| {
                ws.agents
                    .iter()
                    .find(|a| a.id == *id)
                    .map_or("a removed agent", |a| a.title.as_str())
            })
            .collect::<Vec<_>>()
            .join(", "),
    };
    format!("{place} · {who}")
}

/// "No instructions yet", the list's only row when there are none.
pub(super) fn empty_row() -> Row {
    Row::new("instructions.none", "No instructions yet")
}

/// "Add an instruction", under the instructions.
pub(super) fn add_instruction(ui: Ui, cx: &mut Context<Workspace>) -> AnyElement {
    div()
        .id("add-instruction")
        .flex()
        .items_center()
        .gap(px(8.0))
        .px(px(14.0))
        .py(px(10.0))
        .rounded(px(10.0))
        .border_1()
        .border_color(ui.hairline)
        .text_size(px(13.0))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(Icon::new(IconName::Plus).size(px(14.0)))
        .child("Add an instruction")
        .on_click(
            cx.listener(|ws, _: &ClickEvent, window, cx| instruction_dialog(ws, None, window, cx)),
        )
        .test_support()
        .into_any_element()
}

/// The Where list: every workspace, then the workspaces in the sidebar's order, then `current`
/// as a deleted workspace when it is none of them, so Save keeps it. A value is `""` for every
/// workspace, else the workspace's id.
fn place_choices(
    spaces: &[(String, String)],
    current: Option<&str>,
) -> Vec<(SharedString, SharedString)> {
    let gone = current.filter(|c| !spaces.iter().any(|(id, _)| id == c));
    std::iter::once(("".into(), "Every workspace".into()))
        .chain(
            spaces
                .iter()
                .map(|(id, name)| (id.clone().into(), name.clone().into())),
        )
        .chain(gone.map(|c| (c.to_string().into(), "A deleted workspace".into())))
        .collect()
}

/// Editing an instruction (`instruction`), or adding one (None): its name, where, which agents,
/// and its text.
pub(crate) fn instruction_dialog(
    ws: &Workspace,
    instruction: Option<Instruction>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let ui = ws.ui;
    let red = hsla(ws.theme.palette.ansi[1], 1.0);
    let entity = cx.entity();
    let draft = instruction.as_ref().map_or_else(Draft::default, |i| Draft {
        name: i.name.clone(),
        text: i.text.clone(),
        workspace: i.workspace.clone(),
        agents: i.agents.clone(),
    });
    let name = {
        let text = draft.name.clone();
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("What the list shows")
                .default_value(text)
        })
    };
    let text = {
        let text = draft.text.clone();
        cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(8)
                .placeholder("Markdown, as the agent should read it")
                .default_value(text)
        })
    };
    // The place and the agents, redrawn with the dialog as they change.
    let place = cx.new(|_| draft.workspace.clone());
    let agents = cx.new(|_| draft.agents.clone());
    // The agents to tick: Settings ▸ Agents' in its order, then the chosen ones it lost.
    let listed: Vec<(String, String)> = {
        let known = ws.agents.iter().map(|a| (a.id.clone(), a.title.clone()));
        let lost = draft
            .agents
            .iter()
            .flatten()
            .filter(|id| !ws.agents.iter().any(|a| a.id == **id))
            .map(|id| (id.clone(), format!("{id} (a removed agent)")));
        known.chain(lost).collect()
    };
    let id = instruction.as_ref().map(|i| i.id);
    let title = match id {
        Some(_) => "Edit instruction",
        None => "Add an instruction",
    };
    let save = {
        let (entity, name, text, place, agents) = (
            entity.clone(),
            name.clone(),
            text.clone(),
            place.clone(),
            agents.clone(),
        );
        move |window: &mut Window, cx: &mut App| -> bool {
            let draft = Draft {
                name: name.read(cx).value().to_string(),
                text: text.read(cx).value().to_string(),
                workspace: place.read(cx).clone(),
                agents: agents.read(cx).clone(),
            };
            match entity.update(cx, |ws, cx| ws.save_instruction(id, &draft, cx)) {
                Ok(_) => true,
                Err(problem) => {
                    show_notification(window, cx, Notification::warning(problem));
                    false
                }
            }
        }
    };
    let first = name.clone();
    let spaces: Vec<(String, String)> = ws
        .order()
        .iter()
        .map(|s| (s.id.clone(), s.name.clone()))
        .collect();
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let current = place.read(cx).clone();
        let options = place_choices(&spaces, current.as_deref());
        let pick_place = {
            let (place, entity) = (place.clone(), entity.clone());
            move |value: SharedString, cx: &mut App| {
                place.update(cx, |place, _| {
                    *place = Some(value.to_string()).filter(|v| !v.is_empty())
                });
                entity.update(cx, |_, cx| cx.notify());
            }
        };
        let chosen = agents.read(cx).clone();
        // "Every agent" off leaves a choice with no agent yet: Save needs one.
        let every = {
            let (agents, entity) = (agents.clone(), entity.clone());
            Checkbox::new("instruction-every-agent")
                .label("Every agent")
                .checked(chosen.is_none())
                .on_click(move |on: &bool, _, cx| {
                    let on = *on;
                    agents.update(cx, |agents, _| {
                        *agents = if on { None } else { Some(Vec::new()) }
                    });
                    entity.update(cx, |_, cx| cx.notify());
                })
        };
        let boxes = listed.iter().map(|(agent, title)| {
            let (agents, entity, agent) = (agents.clone(), entity.clone(), agent.clone());
            let checked = chosen.as_ref().is_some_and(|c| c.contains(&agent));
            Checkbox::new(ElementId::Name(format!("instruction-agent-{agent}").into()))
                .label(title.clone())
                .checked(checked)
                .on_click(move |on: &bool, _, cx| {
                    let on = *on;
                    agents.update(cx, |agents, _| {
                        let list = agents.get_or_insert_with(Vec::new);
                        list.retain(|a| *a != agent);
                        if on {
                            list.push(agent.clone());
                        }
                    });
                    entity.update(cx, |_, cx| cx.notify());
                })
        });
        let label = |text: &'static str| {
            div()
                .mt(px(4.0))
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child(text)
        };
        let delete = id.map(|id| {
            let entity = entity.clone();
            div()
                .id("delete-instruction")
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .text_size(px(12.0))
                .text_color(red)
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .child("Delete this instruction")
                .on_click(move |_: &ClickEvent, window, cx| {
                    entity.update(cx, |ws, cx| ws.delete_instruction(id, cx));
                    window.close_dialog(cx);
                })
                .test_support()
        });
        let save = save.clone();
        alert
            .confirm()
            .title(title)
            .w(px(480.0))
            .button_props(DialogButtonProps::default().ok_text("Save"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(label("Name"))
                    .child(Input::new(&name))
                    .child(label("Where"))
                    .child(menu_button(
                        "menu-instruction-workspace",
                        options,
                        current.unwrap_or_default().into(),
                        true,
                        pick_place,
                    ))
                    .child(label("Agents"))
                    // Side by side, wrapping: the list is long and the text needs the room.
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_x(px(14.0))
                            .gap_y(px(4.0))
                            .child(every)
                            .children(boxes),
                    )
                    .child(label("Text"))
                    .child(
                        div()
                            .id("instruction-text")
                            .child(Textarea::new(&text).w_full())
                            .test_support(),
                    )
                    .children(delete),
            )
            .on_ok(move |_, window, cx| save(window, cx))
    });
    first.update(cx, |input, cx| input.focus(window, cx));
}
