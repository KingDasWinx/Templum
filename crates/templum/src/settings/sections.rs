//! Each section's rows, read from the workspace as it is every frame.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::NumberInput;
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::switch::Switch;
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, ElementId, Entity, SharedString, TestSupportExt, Window, div, px};

use super::agents::agent_row;
use super::edits::set_shell;
use super::fields::{CURSOR, Choice, FONT_SIZE, MODE, Number, RESET, WARN};
use super::instructions::{empty_row, instruction_row};
use super::row::{Row, menu_button, remove, text_input};
use super::{
    NumberState, SCROLLBACK, Section, args_error, args_text, chip, folder_hint, folder_text,
    program_text, set_args, set_family, set_folder, set_folder_kind, set_git_program, set_program,
};
use crate::config::{Config, Edit, ThemeConfig, WorkingDirectory};
use crate::grid::hsla;
use crate::keys::Command;
use crate::workspace::Workspace;

/// The section's rows, read from the workspace as it is now.
pub(super) fn rows(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    section: Section,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Row> {
    match section {
        Section::General => super::general::rows(ws, entity),
        Section::Appearance => {
            let names: Vec<SharedString> = ws
                .themes
                .iter()
                .map(|e| SharedString::from(e.theme.name.clone()))
                .collect();
            vec![
                choice_row(ws, entity, &MODE, "Mode")
                    .description("System follows the desktop's light or dark setting"),
                theme_row(ws, entity, names.clone(), false),
                theme_row(ws, entity, names, true),
            ]
        }
        Section::Canvas => {
            let on = ws.config.canvas.focus_follows_pointer;
            let e = entity.clone();
            vec![
                Row::new("canvas.focus_follows_pointer", "Focus follows the pointer")
                    .description(
                        "The block under the pointer takes the keyboard, no click needed. Grid \
                         mode's tiles always do",
                    )
                    .control(Switch::new("focus-follows-switch").checked(on).on_click(
                        move |on: &bool, _, cx| {
                            let on = *on;
                            e.update(cx, |ws, cx| {
                                ws.edit(
                                    Edit::set("canvas", "focus_follows_pointer", on),
                                    |c| c.canvas.focus_follows_pointer = on,
                                    cx,
                                )
                            })
                        },
                    ))
                    .reset(
                        !on,
                        remove(entity, "canvas", "focus_follows_pointer", |c| {
                            c.canvas.focus_follows_pointer = true
                        }),
                    ),
            ]
        }
        Section::Font => vec![
            font_family_row(ws, entity),
            number_row(ws, entity, &FONT_SIZE, "Size", window, cx)
                .description("Points at 100% zoom"),
            Row::new("font.preview", "Preview").below(preview(ws)),
        ],
        Section::Terminal => {
            let blink = ws.config.cursor.blink;
            let notifications = ws.config.terminal.notifications;
            let copy = ws.config.terminal.copy_on_select;
            let e = entity.clone();
            let n = entity.clone();
            let c = entity.clone();
            vec![
                choice_row(ws, entity, &CURSOR, "Cursor").description("Block, bar or underline"),
                Row::new("cursor.blink", "Blink")
                    .description("The focused terminal's cursor blinks")
                    .control(Switch::new("blink-switch").checked(blink).on_click(
                        move |on: &bool, _, cx| {
                            let on = *on;
                            e.update(cx, |ws, cx| {
                                ws.edit(
                                    Edit::set("cursor", "blink", on),
                                    |c| c.cursor.blink = on,
                                    cx,
                                )
                            })
                        },
                    ))
                    .reset(
                        blink,
                        remove(entity, "cursor", "blink", |c| c.cursor.blink = false),
                    ),
                number_row(ws, entity, &SCROLLBACK, "Scrollback", window, cx)
                    .description("Lines of history per terminal"),
                Row::new("terminal.notifications", "System notifications")
                    .description(
                        "When Templum is in the background and a long command ends or a terminal \
                         asks for attention",
                    )
                    .control(
                        Switch::new("notifications-switch")
                            .checked(notifications)
                            .on_click(move |on: &bool, _, cx| {
                                let on = *on;
                                n.update(cx, |ws, cx| {
                                    ws.edit(
                                        Edit::set("terminal", "notifications", on),
                                        |c| c.terminal.notifications = on,
                                        cx,
                                    )
                                })
                            }),
                    )
                    .reset(
                        !notifications,
                        remove(entity, "terminal", "notifications", |c| {
                            c.terminal.notifications = true
                        }),
                    ),
                Row::new("terminal.copy_on_select", "Copy selection automatically")
                    .description("Text selected with the mouse goes to the clipboard as you let go")
                    .control(Switch::new("copy-on-select-switch").checked(copy).on_click(
                        move |on: &bool, _, cx| {
                            let on = *on;
                            c.update(cx, |ws, cx| {
                                ws.edit(
                                    Edit::set("terminal", "copy_on_select", on),
                                    |c| c.terminal.copy_on_select = on,
                                    cx,
                                )
                            })
                        },
                    ))
                    .reset(
                        !copy,
                        remove(entity, "terminal", "copy_on_select", |c| {
                            c.terminal.copy_on_select = true
                        }),
                    ),
            ]
        }
        Section::Shell => {
            let shell = &ws.config.shell;
            let integration = shell.integration;
            let e = entity.clone();
            let mut rows = vec![
                shell_row(ws, entity),
                Row::new("shell.program", "Program")
                    .description("Empty: the default shell, above. Set by the list, or typed")
                    .control(text_input(
                        entity,
                        "shell.program",
                        program_text(ws),
                        set_program,
                        window,
                        cx,
                    ))
                    .reset(
                        shell.program.is_some(),
                        remove(entity, "shell", "program", |c| c.shell.program = None),
                    ),
                Row::new("shell.args", "Arguments")
                    .description(
                        args_error(ws)
                            .unwrap_or("Separated by spaces; \"…\" or '…' keeps spaces inside one"),
                    )
                    .control(text_input(
                        entity,
                        "shell.args",
                        args_text(ws),
                        set_args,
                        window,
                        cx,
                    ))
                    .reset(
                        !shell.args.is_empty(),
                        remove(entity, "shell", "args", |c| c.shell.args.clear()),
                    ),
                folder_kind_row(ws, entity),
            ];
            if matches!(shell.working_directory, WorkingDirectory::Path(_)) {
                rows.push(
                    Row::new("shell.folder", "Folder")
                        .description(folder_hint(ws).unwrap_or("A path; ~/ is the home folder"))
                        .control(text_input(
                            entity,
                            "shell.folder",
                            folder_text(ws),
                            set_folder,
                            window,
                            cx,
                        )),
                );
            }
            rows.push(
                Row::new("shell.integration", "Shell integration")
                    .description(
                        "bash, zsh, fish and PowerShell tell Templum their folder, and when \
                         commands end and how",
                    )
                    .control(
                        Switch::new("integration-switch")
                            .checked(integration)
                            .on_click(move |on: &bool, _, cx| {
                                let on = *on;
                                e.update(cx, |ws, cx| {
                                    ws.edit(
                                        Edit::set("shell", "integration", on),
                                        |c| c.shell.integration = on,
                                        cx,
                                    )
                                })
                            }),
                    )
                    .reset(
                        !integration,
                        remove(entity, "shell", "integration", |c| {
                            c.shell.integration = true
                        }),
                    ),
            );
            rows
        }
        Section::Tools => {
            let program = ws.config.git.program.clone();
            let changed = ws.config.git.program() != "git";
            let version = match &ws.git_version {
                Some(Ok(v)) => v.clone(),
                Some(Err(e)) => format!("Not found: {e}"),
                None => "Looking…".to_string(),
            };
            vec![
                Row::new("git.program", "Git")
                    .description(version)
                    .control(text_input(
                        entity,
                        "git.program",
                        program.clone(),
                        set_git_program,
                        window,
                        cx,
                    ))
                    .reset(
                        changed,
                        remove(entity, "git", "program", |c| {
                            c.git.program = "git".to_string()
                        }),
                    ),
            ]
        }
        Section::Git => super::git::rows(ws, entity, window, cx),
        // The accounts are under them (`accounts.rs`).
        Section::Agents => ws
            .agents
            .iter()
            .filter(|agent| agent.account.is_none())
            .map(|agent| agent_row(ws, entity, agent))
            .collect(),
        Section::Instructions => match ws.standing.instructions.as_slice() {
            [] => vec![empty_row()],
            list => list
                .iter()
                .map(|instruction| instruction_row(ws, entity, instruction))
                .collect(),
        },
        Section::Usage => {
            let on = ws.config.usage.claude_limits;
            let e = entity.clone();
            let mut rows = vec![
                choice_row(ws, entity, &RESET, "Limit reset notifications")
                    .description("When a 5-hour or weekly limit is back"),
                choice_row(ws, entity, &WARN, "Warn near a limit")
                    .description("Once per limit, with when it resets"),
                Row::new("usage.claude_limits", "Claude Code limits")
                    .description(
                        "Templum asks Claude Code for them every 5 minutes (its /usage, which \
                         costs nothing), and the Claude Code it opens hands them over as it \
                         works. Your own status line still shows.",
                    )
                    .control(Switch::new("claude-limits-switch").checked(on).on_click(
                        move |on: &bool, _, cx| {
                            let on = *on;
                            e.update(cx, |ws, cx| {
                                ws.edit(
                                    Edit::set("usage", "claude_limits", on),
                                    |c| c.usage.claude_limits = on,
                                    cx,
                                )
                            })
                        },
                    ))
                    .reset(
                        !on,
                        remove(entity, "usage", "claude_limits", |c| {
                            c.usage.claude_limits = true
                        }),
                    ),
            ];
            // Which agents' limits the title bar's meter shows, in turn: an account's are its own.
            let accounts = ws.agents.iter().filter(|a| {
                a.account
                    .as_ref()
                    .is_some_and(|acc| templum_usage::LIMITED.contains(&acc.kind.agent))
            });
            let ids: Vec<String> = templum_usage::LIMITED
                .iter()
                .map(|a| a.to_string())
                .chain(accounts.map(|a| a.id.clone()))
                .collect();
            for agent in ids {
                let shown = !ws.config.usage.hide.contains(&agent);
                let (e, r) = (entity.clone(), entity.clone());
                let (on_agent, reset_agent) = (agent.clone(), agent.clone());
                rows.push(
                    Row::new(
                        format!("usage.hide.{agent}"),
                        format!("{} in the title bar", ws.agent_title(&agent)),
                    )
                    .description("Its limits take turns with the others' in the title bar")
                    .control(
                        Switch::new(SharedString::from(format!("title-bar-{agent}-switch")))
                            .checked(shown)
                            .on_click(move |on: &bool, _, cx| {
                                let on = *on;
                                e.update(cx, |ws, cx| ws.show_in_title_bar(&on_agent, on, cx))
                            }),
                    )
                    .reset(!shown, move |_, cx| {
                        r.update(cx, |ws, cx| ws.show_in_title_bar(&reset_agent, true, cx))
                    }),
                );
            }
            rows
        }
        Section::Connections => super::connections::rows(ws, entity, window, cx),
        Section::Api => super::api::rows(ws, entity, window, cx),
        Section::TemplumAgent => super::templum_agent::rows(ws, entity, window, cx),
        Section::Updates => super::updates::rows(ws, entity),
        Section::Shortcuts => Command::ALL
            .map(|command| shortcut_row(ws, entity, command))
            .into_iter()
            .collect(),
    }
}

fn choice_row(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    choice: &'static Choice,
    title: &'static str,
) -> Row {
    let current = (choice.get)(&ws.config);
    let default = (choice.get)(&Config::default());
    let options = choice
        .options
        .iter()
        .map(|(value, label)| (SharedString::from(*value), SharedString::from(*label)))
        .collect();
    let e = entity.clone();
    Row::new(format!("{}.{}", choice.table, choice.key), title)
        .control(menu_button(
            ElementId::Name(format!("menu-{}.{}", choice.table, choice.key).into()),
            options,
            current.into(),
            false,
            move |value, cx| {
                e.update(cx, |ws, cx| {
                    ws.edit(
                        Edit::set(choice.table, choice.key, (choice.toml)(&value)),
                        |c| (choice.set)(c, &value),
                        cx,
                    )
                })
            },
        ))
        .reset(
            current != default,
            remove(entity, choice.table, choice.key, move |c| {
                (choice.set)(c, default)
            }),
        )
}

/// `theme.light` or `theme.dark`: the built-in themes and the user's themes folder.
fn theme_row(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    names: Vec<SharedString>,
    dark: bool,
) -> Row {
    let key = if dark { "dark" } else { "light" };
    let current = ws.config.theme.name(dark).to_string();
    let default = ThemeConfig::default().name(dark).to_string();
    let e = entity.clone();
    Row::new(
        format!("theme.{key}"),
        if dark { "Dark theme" } else { "Light theme" },
    )
    .description(if dark {
        "Used in dark mode"
    } else {
        "Used in light mode"
    })
    .control(menu_button(
        ElementId::Name(format!("menu-theme.{key}").into()),
        names.into_iter().map(|n| (n.clone(), n)).collect(),
        current.clone().into(),
        true,
        move |name, cx| {
            e.update(cx, |ws, cx| {
                ws.edit(
                    Edit::set("theme", key, name.to_string()),
                    |c| set_theme(c, dark, name.to_string()),
                    cx,
                )
            })
        },
    ))
    .reset(
        current != default,
        remove(entity, "theme", key, move |c| {
            set_theme(c, dark, default.clone())
        }),
    )
}

fn font_family_row(ws: &Workspace, entity: &Entity<Workspace>) -> Row {
    let options = std::iter::once((
        SharedString::default(),
        SharedString::from("Default (built-in list)"),
    ))
    .chain(ws.fonts.iter().map(|f| (f.clone(), f.clone())))
    .collect();
    let current = ws.config.font.family.clone().unwrap_or_default();
    let e = entity.clone();
    Row::new("font.family", "Family")
        .description("Installed monospaced fonts")
        .control(menu_button(
            "menu-font.family",
            options,
            current.into(),
            true,
            move |family, cx| e.update(cx, |ws, cx| set_family(ws, &family, cx)),
        ))
        .reset(
            ws.config.font.family.is_some(),
            remove(entity, "font", "family", |c| c.font.family = None),
        )
}

fn folder_kind_row(ws: &Workspace, entity: &Entity<Workspace>) -> Row {
    let current = match ws.config.shell.working_directory {
        WorkingDirectory::Inherit => "inherit",
        WorkingDirectory::Home => "home",
        WorkingDirectory::Path(_) => "folder",
    };
    let e = entity.clone();
    Row::new("shell.working_directory", "Start folder")
        .description("Where a new terminal starts")
        .control(menu_button(
            "menu-shell.working_directory",
            vec![
                ("inherit".into(), "Inherit from the focused block".into()),
                ("home".into(), "Home".into()),
                ("folder".into(), "A folder…".into()),
            ],
            current.into(),
            false,
            move |kind, cx| e.update(cx, |ws, cx| set_folder_kind(ws, &kind, cx)),
        ))
        .reset(
            current != "inherit",
            remove(entity, "shell", "working_directory", |c| {
                c.shell.working_directory = WorkingDirectory::Inherit
            }),
        )
}

pub(super) fn number_row(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    n: &'static Number,
    title: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> Row {
    let value = (n.get)(&ws.config);
    let default = (n.get)(&Config::default());
    let e = entity.clone();
    let key = SharedString::from(format!("number-{}.{}", n.table, n.key));
    let state = window.use_keyed_state(key, cx, move |window, cx| {
        NumberState::new(e, n, value, window, cx)
    });
    // A hand edit of the file, or a reset, replaces the text.
    state.update(cx, |state, cx| {
        if state.shown != value {
            state.shown = value;
            state.input.update(cx, |input, cx| {
                input.set_value(value.to_string(), window, cx)
            });
        }
    });
    Row::new(format!("{}.{}", n.table, n.key), title)
        .control(NumberInput::new(&state.read(cx).input).small().w_32())
        .reset(
            value != default,
            remove(entity, n.table, n.key, move |c| (n.set)(c, default)),
        )
}

/// One command's shortcuts as keys; a click starts recording. Resetting removes
/// `keys.<command>` (the OS defaults again).
fn shortcut_row(ws: &Workspace, entity: &Entity<Workspace>, command: Command) -> Row {
    let recording = ws.recording == Some(command);
    let strokes = ws.keymap.keystrokes(command);
    let ui = ws.ui;
    let e = entity.clone();
    let control = div()
        .id(ElementId::Name(
            format!("shortcut-{}", command.name()).into(),
        ))
        .flex()
        .items_center()
        .gap(px(4.0))
        .px(px(6.0))
        .py(px(3.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(if recording {
            ui.accent
        } else {
            gpui_kit::transparent_black()
        })
        .text_size(px(12.0))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .map(|el| {
            if recording {
                el.child("Press a shortcut…")
            } else if strokes.is_empty() {
                el.text_color(ui.title_dim).child("None")
            } else {
                el.children(strokes.iter().cloned().map(|s| chip(Kbd::new(s), ui)))
            }
        })
        .on_click(move |_: &ClickEvent, window, cx| {
            e.update(cx, |ws, cx| ws.record(command, window, cx))
        })
        .test_support();
    let description = match &ws.key_note {
        Some(note) if recording => Some(SharedString::from(note.clone())),
        _ if recording => Some("Esc cancels; Backspace leaves it without a shortcut".into()),
        _ => None,
    };
    let e = entity.clone();
    let row = Row::new(format!("keys.{}", command.name()), command.title())
        .control(control)
        .reset(ws.config.keys.get(command).is_some(), move |window, cx| {
            e.update(cx, |ws, cx| ws.reset_shortcut(command, window, cx))
        });
    match description {
        Some(text) => row.description(text),
        None => row,
    }
}

/// A few lines of terminal output in the chosen font, size and theme.
fn preview(ws: &Workspace) -> impl IntoElement {
    let family = ws.cell.font.family.clone();
    let size = ws.config.font.size;
    let p = ws.theme.palette;
    let line = |parts: &[(&str, templum_terminal::Rgb)]| {
        div().flex().children(
            parts
                .iter()
                .map(|(text, color)| div().text_color(hsla(*color, 1.0)).child(text.to_string())),
        )
    };
    div()
        .w_full()
        .p(px(12.0))
        .rounded(px(8.0))
        .border_1()
        .border_color(ws.ui.hairline)
        .bg(hsla(p.background, 1.0))
        .font_family(family)
        .text_size(px(size))
        .child(line(&[("~/templum", p.ansi[4])]))
        .child(line(&[("❯ ", p.ansi[2]), ("cargo test", p.foreground)]))
        .child(line(&[
            ("test result: ", p.foreground),
            ("ok", p.ansi[2]),
            (". 135 passed", p.foreground),
        ]))
}

/// The shell new terminals run: the default, or one found on this computer. One set by hand that
/// isn't in the list shows as Custom.
fn shell_row(ws: &Workspace, entity: &Entity<Workspace>) -> Row {
    let shell = &ws.config.shell;
    let current = match &shell.program {
        None => "default".to_string(),
        Some(program) => ws
            .shells
            .iter()
            .position(|s| s.program == *program && s.args == shell.args)
            .map_or_else(|| "custom".to_string(), |i| i.to_string()),
    };
    let mut options: Vec<(SharedString, SharedString)> = vec![(
        "default".into(),
        format!("Default ({})", ws.default_shell).into(),
    )];
    options.extend(
        ws.shells
            .iter()
            .enumerate()
            .map(|(i, s)| (i.to_string().into(), s.label.clone().into())),
    );
    if current == "custom" {
        options.push(("custom".into(), "Custom".into()));
    }
    let (e, shells) = (entity.clone(), ws.shells.clone());
    let reset = entity.clone();
    Row::new("shell.shell", "Shell")
        .description("What new terminals run")
        .control(menu_button(
            "menu-shell.shell",
            options,
            current.into(),
            false,
            move |value, cx| {
                let chosen = value.parse::<usize>().ok().and_then(|i| shells.get(i));
                if chosen.is_none() && value != "default" {
                    return;
                }
                e.update(cx, |ws, cx| set_shell(ws, chosen, cx))
            },
        ))
        .reset(shell.program.is_some(), move |_, cx| {
            reset.update(cx, |ws, cx| set_shell(ws, None, cx))
        })
}

pub(crate) fn set_theme(c: &mut Config, dark: bool, name: String) {
    if dark {
        c.theme.dark = name;
    } else {
        c.theme.light = name;
    }
}
