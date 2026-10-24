//! The Agents section: a row per agent, "Add an agent", and the dialog that edits or adds one.

use gpui_kit::assets::IconName;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, FontWeight, SharedString,
    TestSupportExt, Window, div, px,
};
use templum_terminal::{ShellOption, is_shell};

use super::row::{Row, menu_button};
use crate::agents::{AgentEntry, Draft};
use crate::grid::hsla;
use crate::theme::Ui;
use crate::workspace::{Workspace, show_notification};

/// One agent: its command; whether the canvas's menu lists it; Edit; ↺ for one of Templum's the
/// user changed.
pub(super) fn agent_row(ws: &Workspace, entity: &Entity<Workspace>, agent: &AgentEntry) -> Row {
    let ui = ws.ui;
    let id = agent.id.clone();
    let description = description(
        agent,
        ws.agent_found(agent),
        ws.agent_shell(agent).is_some(),
    );
    let show = {
        let (e, agent) = (entity.clone(), agent.clone());
        Switch::new(ElementId::Name(format!("show-agent-{id}").into()))
            .checked(agent.show)
            .on_click(move |on: &bool, _, cx| {
                let draft = Draft {
                    show: *on,
                    ..Draft::of(&agent)
                };
                let id = agent.id.clone();
                e.update(cx, |ws, cx| ws.save_agent(Some(&id), &draft, cx).ok());
            })
    };
    let edit = {
        let (e, agent) = (entity.clone(), agent.clone());
        div()
            .id(ElementId::Name(format!("edit-agent-{id}").into()))
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
                let agent = agent.clone();
                e.update(cx, |ws, cx| agent_dialog(ws, Some(agent), window, cx))
            })
            .test_support()
    };
    let e = entity.clone();
    let reset_id = id.clone();
    Row::new(format!("agents.{id}"), agent.title.clone())
        .description(description)
        .control(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(show)
                .child(edit),
        )
        .reset(agent.builtin.is_some() && agent.changed, move |_, cx| {
            e.update(cx, |ws, cx| ws.remove_agent(&reset_id, cx))
        })
}

/// What an agent's row says under its title: its command, or how to install it when its program
/// isn't found (the canvas's menu greys it out), then its shell when it has one. `found`: whether
/// that shell is among the ones found here.
pub(super) fn description(agent: &AgentEntry, installed: bool, found: bool) -> String {
    let text = match agent.install {
        Some(install) if !installed => format!("Not installed · {install}"),
        _ if agent.builtin.is_some() => agent.command.clone(),
        _ if !installed => format!("Yours · not found · {}", agent.command),
        _ => format!("Yours · {}", agent.command),
    };
    with_shell(text, agent.shell.as_deref(), found)
}

/// `text` followed by the shell an entry opens in: ` · in zsh`, and ` (not available)` when no
/// shell of that label is found here.
pub(super) fn with_shell(text: String, shell: Option<&str>, found: bool) -> String {
    match shell {
        Some(label) if found => format!("{text} · in {label}"),
        Some(label) => format!("{text} · in {label} (not available)"),
        None => text,
    }
}

/// The Shell list of an agent's or account's dialog (docs/specs/shells.md, Settings ▸ Agents):
/// value and label of **Default (*default*)**, which follows Settings ▸ Shell, then every one of
/// `shells` (an account's leaves WSL out), then `current` as ***current* (not available)** when
/// it is set and none of them, so Save keeps it. A value is `""` for Default, else a label.
pub(super) fn shell_choices(
    default: &str,
    shells: &[ShellOption],
    account: bool,
    current: Option<&str>,
) -> Vec<(SharedString, SharedString)> {
    let listed: Vec<&ShellOption> = shells
        .iter()
        .filter(|s| !account || !is_shell(&s.program, "wsl"))
        .collect();
    let gone = current.filter(|c| !listed.iter().any(|s| s.label == *c));
    std::iter::once(("".into(), format!("Default ({default})").into()))
        .chain(
            listed
                .iter()
                .map(|s| (s.label.clone().into(), s.label.clone().into())),
        )
        .chain(gone.map(|c| (c.to_string().into(), format!("{c} (not available)").into())))
        .collect()
}

/// "Add an agent", under the agents.
pub(super) fn add_agent(ui: Ui, cx: &mut Context<Workspace>) -> AnyElement {
    div()
        .id("add-agent")
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
        .child("Add an agent")
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| agent_dialog(ws, None, window, cx)))
        .test_support()
        .into_any_element()
}

/// Editing an agent (`agent`), or adding one (None): its name, command and resume command.
/// Saving one of Templum's back at its defaults takes it out of config.toml.
pub(crate) fn agent_dialog(
    ws: &Workspace,
    agent: Option<AgentEntry>,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) {
    let ui = ws.ui;
    let red = hsla(ws.theme.palette.ansi[1], 1.0);
    let entity = cx.entity();
    let draft = agent.as_ref().map_or(
        Draft {
            show: true,
            ..Draft::default()
        },
        Draft::of,
    );
    let field = |text: &str,
                 placeholder: &'static str,
                 window: &mut Window,
                 cx: &mut Context<Workspace>| {
        let text = text.to_string();
        cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(placeholder)
                .default_value(text)
        })
    };
    let shells = ws.shells.clone();
    let default_shell = ws.default_shell_name();
    let default_wsl = ws
        .config
        .shell
        .program
        .as_deref()
        .is_some_and(|p| is_shell(p, "wsl"));
    // The Shell list's choice, redrawn with the dialog as it changes.
    let shell = cx.new(|_| draft.shell.clone());
    let name = field(&draft.name, "What the menu shows", window, cx);
    let command = field(
        &draft.command,
        "What starts it, like my-agent --tui",
        window,
        cx,
    );
    let resume = field(
        &draft.resume,
        "Empty: a new session when the canvas comes back",
        window,
        cx,
    );
    let generate = field(
        &draft.generate,
        "Writes a commit message, like my-agent ask -m {model}",
        window,
        cx,
    );
    let models_text = draft.models.clone();
    let models = cx.new(|cx| {
        TextareaState::new(window, cx)
            .rows(3)
            .placeholder("One per line, the recommended one first")
            .default_value(models_text)
    });
    let builtin = agent.as_ref().and_then(|a| a.builtin);
    let codex = crate::agents::codex_models();
    let preset = builtin.map(|agent| crate::agents::preset(agent, &codex));
    let generate_default = preset
        .as_ref()
        .map(|(generate, _)| format!("Templum's: {}", generate.unwrap_or("none")));
    let models_default = preset.map(|(_, models)| match models.as_slice() {
        [] => "Templum's: none".to_string(),
        list => format!("Templum's: {}", list.join(", ")),
    });
    let id = agent.as_ref().map(|a| a.id.clone());
    let title = match &agent {
        Some(agent) => format!("Edit {}", agent.title),
        None => "Add an agent".to_string(),
    };
    let save = {
        let (entity, name, command, resume, generate, models, shell, id) = (
            entity.clone(),
            name.clone(),
            command.clone(),
            resume.clone(),
            generate.clone(),
            models.clone(),
            shell.clone(),
            id.clone(),
        );
        move |window: &mut Window, cx: &mut App| -> bool {
            let draft = Draft {
                name: name.read(cx).value().to_string(),
                command: command.read(cx).value().to_string(),
                resume: resume.read(cx).value().to_string(),
                show: draft.show,
                generate: generate.read(cx).value().to_string(),
                models: models.read(cx).value().to_string(),
                shell: shell.read(cx).clone(),
            };
            let saved = entity.update(cx, |ws, cx| ws.save_agent(id.as_deref(), &draft, cx));
            match saved {
                Ok(_) => true,
                Err(problem) => {
                    show_notification(window, cx, Notification::warning(problem));
                    false
                }
            }
        }
    };
    let first = name.clone();
    window.open_alert_dialog(cx, move |alert, _, cx| {
        let current = shell.read(cx).clone();
        let choices = shell_choices(&default_shell, &shells, false, current.as_deref());
        // The note while the agent runs in WSL: its own shell is, or it follows Settings ▸ Shell.
        let in_wsl = match &current {
            // A label not found opens the default.
            Some(label) => shells
                .iter()
                .find(|s| s.label == *label)
                .map_or(default_wsl, |s| is_shell(&s.program, "wsl")),
            None => default_wsl,
        };
        let pick = {
            let (shell, entity) = (shell.clone(), entity.clone());
            move |value: SharedString, cx: &mut App| {
                shell.update(cx, |shell, _| {
                    *shell = Some(value.to_string()).filter(|v| !v.is_empty())
                });
                entity.update(cx, |_, cx| cx.notify());
            }
        };
        let label = |text: &'static str| {
            div()
                .mt(px(4.0))
                .text_size(px(12.0))
                .text_color(ui.title_dim)
                .child(text)
        };
        let default = |text: String| {
            div()
                .text_size(px(11.0))
                .text_color(ui.title_dim)
                .child(text)
        };
        let delete = id.clone().filter(|_| builtin.is_none()).map(|id| {
            let entity = entity.clone();
            div()
                .id("delete-agent")
                .px(px(10.0))
                .py(px(4.0))
                .rounded(px(6.0))
                .text_size(px(12.0))
                .text_color(red)
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .child("Delete this agent")
                .on_click(move |_: &ClickEvent, window, cx| {
                    entity.update(cx, |ws, cx| ws.remove_agent(&id, cx));
                    window.close_dialog(cx);
                })
                .test_support()
        });
        let save = save.clone();
        alert
            .confirm()
            .title(title.clone())
            .w(px(480.0))
            .button_props(DialogButtonProps::default().ok_text("Save"))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .child(label("Name"))
                    .child(Input::new(&name))
                    .child(label("Command"))
                    .child(Input::new(&command))
                    .when_some(builtin, |el, agent| {
                        el.child(default(format!("Templum's: {}", agent.command())))
                    })
                    .child(label("Resume command"))
                    .child(Input::new(&resume))
                    .when_some(builtin, |el, agent| {
                        el.child(default(match agent.spec().resume {
                            Some(resume) => format!("Templum's: {resume}"),
                            None => "Templum's: a new session (this agent can't resume by folder)"
                                .to_string(),
                        }))
                    })
                    .child(label("Shell"))
                    .child(menu_button(
                        "menu-agent-shell",
                        choices,
                        current.clone().unwrap_or_default().into(),
                        true,
                        pick,
                    ))
                    .when(in_wsl, |el| {
                        el.child(
                            div()
                                .id("agent-shell-wsl")
                                .child(default(
                                    "In WSL the agent gets none of Templum's tools, hooks or \
                                     status line, and isn't resumed after a restart."
                                        .to_string(),
                                ))
                                .test_support(),
                        )
                    })
                    .child(
                        div()
                            .mt(px(10.0))
                            .text_size(px(13.0))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Commit messages"),
                    )
                    .child(default(
                        "For Generate ✨ in a Git block. {model} is the model; the request goes to \
                         the standard input, or to the file {prompt_file} names; the answer is the \
                         standard output, or what it writes to the file {output_file} names."
                            .to_string(),
                    ))
                    .child(label("Generate command"))
                    .child(
                        div()
                            .id("agent-generate")
                            .child(Input::new(&generate))
                            .test_support(),
                    )
                    .when_some(generate_default.clone(), |el, line| el.child(default(line)))
                    .child(label("Models"))
                    .child(
                        div()
                            .id("agent-models")
                            .child(Textarea::new(&models).w_full())
                            .test_support(),
                    )
                    .when_some(models_default.clone(), |el, line| el.child(default(line)))
                    .children(delete),
            )
            .on_ok(move |_, window, cx| save(window, cx))
    });
    first.update(cx, |input, cx| input.focus(window, cx));
}
