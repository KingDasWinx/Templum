//! Settings ▸ Templum Agent (docs/specs/templum-agent.md#settings--templum-agent): the agent the
//! chat runs on, its model and effort, and the pill.

use gpui_kit::component::switch::Switch;
use gpui_kit::{App, Context, ElementId, Entity, SharedString, Window};

use super::fields::MAX_AGENTS;
use super::row::{Row, menu_button, remove, text_input};
use super::sections::number_row;
use crate::config::Edit;
use crate::workspace::Workspace;

/// The engines the Templum Agent runs on: Claude Code.
fn claude() -> templum_terminal::Agent {
    templum_terminal::Agent::from_name("claude").expect("Claude Code")
}

pub(super) fn rows(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Row> {
    let config = &ws.config.templum_agent;
    let label = if templum_terminal::installed("claude") {
        "Claude Code".to_string()
    } else {
        "Claude Code (not installed)".to_string()
    };
    let efforts = std::iter::once((SharedString::default(), SharedString::from("Default")))
        .chain(
            claude()
                .efforts()
                .iter()
                .map(|e| (SharedString::from(*e), SharedString::from(*e))),
        )
        .collect();
    let (e, p) = (entity.clone(), entity.clone());
    let pill = config.pill;
    let mut rows = vec![
        Row::new("templum_agent.agent", "Agent")
            .description("The CLI agent the chat runs on, with your login and plan")
            .control(menu_button(
                ElementId::Name("menu-templum_agent.agent".into()),
                vec![(SharedString::from("claude"), SharedString::from(label))],
                SharedString::from(config.agent.clone()),
                false,
                |_, _| {},
            )),
    ];
    rows.extend(model_rows(ws, entity, window, cx));
    rows.extend([
        Row::new("templum_agent.effort", "Effort")
            .description("How hard it thinks")
            .control(menu_button(
                ElementId::Name("menu-templum_agent.effort".into()),
                efforts,
                SharedString::from(config.effort.clone()),
                false,
                move |effort, cx| e.update(cx, |ws, cx| set_effort(ws, &effort, cx)),
            ))
            .reset(
                !config.effort.is_empty(),
                remove(entity, "templum_agent", "effort", |c| {
                    c.templum_agent.effort.clear()
                }),
            ),
        Row::new("templum_agent.pill", "Prompt pill")
            .description("Its input at the bottom of the canvas; off, the shortcut still opens it")
            .control(Switch::new("templum-agent-pill").checked(pill).on_click(
                move |on: &bool, _, cx| {
                    let on = *on;
                    p.update(cx, |ws, cx| {
                        ws.edit(
                            Edit::set("templum_agent", "pill", on),
                            |c| c.templum_agent.pill = on,
                            cx,
                        )
                    })
                },
            ))
            .reset(
                !pill,
                remove(entity, "templum_agent", "pill", |c| {
                    c.templum_agent.pill = true
                }),
            ),
        number_row(ws, entity, &MAX_AGENTS, "Agents at once", window, cx)
            .description("How many of its tasks may run together; a new one waits for one to end"),
    ]);
    rows
}

/// The agent's models (Default, then its list) with Another model under them to type one the
/// list doesn't have; an engine with no list gets the text field alone.
fn model_rows(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Row> {
    let model = ws.config.templum_agent.model.clone();
    let models = ws.engine_models();
    let typed = |row: Row, key, window: &mut Window, cx: &mut App| {
        row.control(text_input(
            entity,
            key,
            model.clone(),
            set_model,
            window,
            cx,
        ))
    };
    let reset = |row: Row| {
        row.reset(
            !model.is_empty(),
            remove(entity, "templum_agent", "model", |c| {
                c.templum_agent.model.clear()
            }),
        )
    };
    if models.is_empty() {
        let row = Row::new("templum_agent.model", "Model").description(
            "Empty is the agent's own default: 1 to 64 letters, digits, ., -, _, : and /",
        );
        return vec![reset(typed(row, "templum_agent.model", window, cx))];
    }
    let options = std::iter::once(SharedString::default())
        .chain(models.into_iter().map(SharedString::from))
        .map(|m| {
            let label = if m.is_empty() {
                "Default".into()
            } else {
                m.clone()
            };
            (m, label)
        })
        .collect();
    let e = entity.clone();
    let picker = Row::new("templum_agent.model", "Model")
        .description("Default is the agent's own; another can be typed below")
        .control(menu_button(
            ElementId::Name("menu-templum_agent.model".into()),
            options,
            SharedString::from(model.clone()),
            false,
            move |m, cx| e.update(cx, |ws, cx| set_model(ws, &m, cx)),
        ));
    let other = Row::new("templum_agent.other_model", "Another model")
        .description("Typed: one the list doesn't have");
    vec![
        reset(picker),
        typed(other, "templum_agent.other_model", window, cx),
    ]
}

/// `templum_agent.model`, trimmed: empty removes the key; a model Templum wouldn't type isn't
/// taken (the field goes back to the last one).
pub(crate) fn set_model(ws: &mut Workspace, text: &str, cx: &mut Context<Workspace>) {
    let model = text.trim().to_string();
    if model.is_empty() {
        ws.edit(
            Edit::remove("templum_agent", "model"),
            |c| c.templum_agent.model.clear(),
            cx,
        );
    } else if claude().launch_args(Some(&model), None).is_ok() {
        ws.edit(
            Edit::set("templum_agent", "model", model.clone()),
            move |c| c.templum_agent.model = model.clone(),
            cx,
        );
    }
}

/// `templum_agent.effort`: Default removes the key.
fn set_effort(ws: &mut Workspace, effort: &str, cx: &mut Context<Workspace>) {
    let effort = effort.to_string();
    if effort.is_empty() {
        ws.edit(
            Edit::remove("templum_agent", "effort"),
            |c| c.templum_agent.effort.clear(),
            cx,
        );
    } else {
        ws.edit(
            Edit::set("templum_agent", "effort", effort.clone()),
            move |c| c.templum_agent.effort = effort.clone(),
            cx,
        );
    }
}
