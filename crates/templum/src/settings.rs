//! The settings panel: it slides in from the right over the canvas. Tabs pick a section, and
//! each field is a row (title, description, control, reset) in one card, built every frame from
//! the workspace's state. A change goes through the workspace, which applies it on the next frame
//! and saves it shortly after.

use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::input::{
    Input, InputEvent, InputState, NumberInput, NumberInputEvent, StepAction,
};
use gpui_kit::component::kbd::Kbd;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{Icon, Sizable as _, TITLE_BAR_HEIGHT, WindowExt as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    Anchor, AnyElement, App, BoxShadow, ClickEvent, Context, ElementId, Entity, FontWeight,
    MouseDownEvent, SharedString, Subscription, TestSupportExt, Window, div, font, point, px,
};

use crate::agents::{AgentEntry, Draft};
use crate::config::{self, Config, Edit, Mode, Shape, ThemeConfig, WorkingDirectory};
use crate::grid::{self, hsla};
use crate::keys::Command;
use crate::motion;
use crate::theme::Ui;
use crate::workspace::{Workspace, show_notification};

/// The panel's width, at most 60% of the canvas.
const WIDTH: f32 = 480.0;

/// The panel's sections, in tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Appearance,
    Font,
    Terminal,
    Shell,
    Agents,
    Shortcuts,
}

impl Section {
    pub const ALL: [Section; 6] = [
        Section::Appearance,
        Section::Font,
        Section::Terminal,
        Section::Shell,
        Section::Agents,
        Section::Shortcuts,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Appearance => "Appearance",
            Section::Font => "Font",
            Section::Terminal => "Terminal",
            Section::Shell => "Shell",
            Section::Agents => "Agents",
            Section::Shortcuts => "Shortcuts",
        }
    }

    /// Its tab's element id.
    fn tab_id(self) -> &'static str {
        match self {
            Section::Appearance => "settings-tab-appearance",
            Section::Font => "settings-tab-font",
            Section::Terminal => "settings-tab-terminal",
            Section::Shell => "settings-tab-shell",
            Section::Agents => "settings-tab-agents",
            Section::Shortcuts => "settings-tab-shortcuts",
        }
    }

    /// A line under the heading, for what holds for the whole section.
    fn note(self) -> Option<&'static str> {
        match self {
            Section::Shell => Some("Applies to new terminals"),
            Section::Agents => Some(
                "The AI agents in the canvas menu. Change one of Templum's, or add your own; ↺ puts \
                 one of Templum's back as it was.",
            ),
            Section::Shortcuts => Some("Click a shortcut to record a new one"),
            _ => None,
        }
    }
}

/// The open panel, or the closed one still sliding out.
pub(crate) struct Panel {
    pub since: Instant,
    pub closing: bool,
    pub section: Section,
}

impl Panel {
    pub fn new(now: Instant) -> Panel {
        Panel {
            since: now,
            closing: false,
            section: Section::Appearance,
        }
    }

    /// How much of the panel is in view at `now`: 0 hidden, 1 all.
    pub fn shown(&self, now: Instant) -> f32 {
        let t = motion::slide(motion::progress(self.since, now, motion::SLIDE));
        if self.closing { 1.0 - t } else { t }
    }

    /// The panel sliding the other way, from where it is (no jump when turned back mid-slide).
    pub fn reversed(&self, now: Instant) -> Panel {
        let closing = !self.closing;
        let shown = self.shown(now);
        let eased = if closing { 1.0 - shown } else { shown };
        // motion::slide inverted: the progress that eases to `eased`.
        let progress = 1.0 - (1.0 - eased).cbrt();
        Panel {
            since: now
                .checked_sub(motion::SLIDE.mul_f32(progress))
                .unwrap_or(now),
            closing,
            section: self.section,
        }
    }
}

/// Puts one setting back to its default.
type Reset = Rc<dyn Fn(&mut Window, &mut App)>;

/// One field: a title and a description on the left, its control on the right.
struct Row {
    /// `table.key`: the control's element id is `field-<key>`, its reset's `reset-<key>`.
    key: SharedString,
    title: SharedString,
    description: Option<SharedString>,
    control: Option<AnyElement>,
    /// Drawn under the title, full width (the font preview).
    below: Option<AnyElement>,
    /// Some while the value isn't the default.
    reset: Option<Reset>,
}

impl Row {
    fn new(key: impl Into<SharedString>, title: impl Into<SharedString>) -> Row {
        Row {
            key: key.into(),
            title: title.into(),
            description: None,
            control: None,
            below: None,
            reset: None,
        }
    }

    fn description(mut self, text: impl Into<SharedString>) -> Row {
        self.description = Some(text.into());
        self
    }

    fn control(mut self, control: impl IntoElement) -> Row {
        self.control = Some(control.into_any_element());
        self
    }

    fn below(mut self, el: impl IntoElement) -> Row {
        self.below = Some(el.into_any_element());
        self
    }

    /// The reset button shows while `dirty`.
    fn reset(mut self, dirty: bool, reset: impl Fn(&mut Window, &mut App) + 'static) -> Row {
        self.reset = dirty.then(|| Rc::new(reset) as Reset);
        self
    }

    fn render(self, ui: Ui, first: bool) -> impl IntoElement {
        let key = self.key;
        let reset = self.reset.map(|reset| {
            div()
                .id(ElementId::Name(format!("reset-{key}").into()))
                .flex()
                .items_center()
                .justify_center()
                .size(px(24.0))
                .rounded_full()
                .cursor_pointer()
                .hover(move |s| s.bg(ui.hover))
                .active(move |s| s.bg(ui.pressed))
                .child(
                    Icon::new(IconName::Undo2)
                        .size(px(13.0))
                        .text_color(ui.title_dim),
                )
                .on_click(move |_: &ClickEvent, window, cx| reset(window, cx))
                .test_support()
        });
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .px(px(14.0))
            .py(px(12.0))
            .when(!first, |el| el.border_t_1().border_color(ui.hairline))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.0))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(2.0))
                            .child(
                                div()
                                    .text_size(px(13.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(self.title),
                            )
                            .when_some(self.description, |el, text| {
                                el.child(
                                    div()
                                        .text_size(px(12.0))
                                        .text_color(ui.title_dim)
                                        .child(text),
                                )
                            }),
                    )
                    .when_some(self.control, |el, control| {
                        el.child(
                            div()
                                .id(ElementId::Name(format!("field-{key}").into()))
                                .flex_shrink_0()
                                .child(control)
                                .test_support(),
                        )
                    })
                    // The reset button's place is kept, so controls line up.
                    .child(div().w(px(24.0)).flex_shrink_0().children(reset)),
            )
            .children(self.below)
    }
}

/// A key or a word in a small outlined pill.
pub(crate) fn chip<E: Styled>(el: E, ui: Ui) -> E {
    el.px(px(6.0))
        .py(px(2.0))
        .rounded(px(6.0))
        .border_1()
        .border_color(ui.hairline)
        .bg(ui.card)
        .text_color(ui.title)
        .text_size(px(11.0))
}

pub(crate) fn render(
    ws: &Workspace,
    panel: &Panel,
    now: Instant,
    canvas_w: f32,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let width = WIDTH.min(canvas_w * 0.6);
    let shown = panel.shown(now);
    let entity = cx.entity();

    let close = div()
        .id("settings-close")
        .flex()
        .items_center()
        .justify_center()
        .size(px(26.0))
        .rounded_full()
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(
            Icon::new(IconName::Close)
                .size(px(14.0))
                .text_color(ui.title),
        )
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.close_settings(window, cx)))
        .test_support();
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(48.0))
        .px(px(16.0))
        .child(
            div()
                .text_size(px(15.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child("Settings"),
        )
        .child(close);
    let tabs = div()
        .flex()
        .flex_wrap()
        .gap(px(4.0))
        .px(px(12.0))
        .pb(px(12.0))
        .border_b_1()
        .border_color(ui.hairline)
        .children(Section::ALL.map(|section| tab(ui, section, section == panel.section, cx)));

    let section = panel.section;
    let rows = rows(ws, &entity, section, window, cx);
    let resets: Vec<Reset> = rows.iter().filter_map(|r| r.reset.clone()).collect();
    let heading = div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(20.0))
                .font_weight(FontWeight::SEMIBOLD)
                .child(section.title()),
        )
        .when(!resets.is_empty(), |el| {
            el.child(
                div()
                    .id("settings-reset-section")
                    .px(px(10.0))
                    .py(px(3.0))
                    .rounded_full()
                    .text_size(px(12.0))
                    .text_color(ui.title)
                    .cursor_pointer()
                    .hover(move |s| s.bg(ui.hover))
                    .active(move |s| s.bg(ui.pressed))
                    .child("Reset all")
                    // Together, in one event: shortcuts reset at once may trade keys.
                    .on_click(move |_: &ClickEvent, window, cx| {
                        for reset in &resets {
                            reset(window, cx);
                        }
                    })
                    .test_support(),
            )
        });
    let card = div()
        .flex()
        .flex_col()
        .rounded(px(10.0))
        .border_1()
        .border_color(ui.hairline)
        .bg(ui.card)
        .children(
            rows.into_iter()
                .enumerate()
                .map(|(i, row)| row.render(ui, i == 0)),
        );
    let content = div()
        .id("settings-scroll")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .test_support()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .p(px(16.0))
                .child(heading)
                .when_some(section.note(), |el, note| {
                    el.child(
                        div()
                            .text_size(px(12.0))
                            .text_color(ui.title_dim)
                            .child(note),
                    )
                })
                .child(div().mt(px(4.0)).child(card))
                .when(section == Section::Agents, |el| el.child(add_agent(ui, cx))),
        );

    let path = ws.config_dir.as_ref().map(|d| d.join(config::FILE));
    let status = match (&path, &ws.save_error) {
        (None, _) => Some((
            "No config folder: changes last until Templum quits".to_string(),
            ui.title_dim,
        )),
        (Some(_), Some(error)) => Some((error.clone(), hsla(ws.theme.palette.ansi[1], 1.0))),
        (Some(_), None) => None,
    };
    let footer = div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .px(px(16.0))
        .py(px(12.0))
        .border_t_1()
        .border_color(ui.hairline)
        .text_size(px(12.0))
        .when_some(path, |el, path| {
            el.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.0))
                    .child(
                        div()
                            .truncate()
                            .text_color(ui.title_dim)
                            .child(path.display().to_string()),
                    )
                    .child(
                        Button::new("edit-config")
                            .label("Edit config.toml")
                            .small()
                            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                                ws.open_config(window, cx)
                            })),
                    ),
            )
        })
        .when_some(status, |el, (text, color)| {
            el.child(
                div()
                    .id("settings-status")
                    .text_color(color)
                    .child(text)
                    .test_support(),
            )
        });

    div()
        .id("settings-panel")
        .absolute()
        .top(TITLE_BAR_HEIGHT)
        .bottom(px(0.0))
        .right(px(-width * (1.0 - shown)))
        .w(px(width))
        .flex()
        .flex_col()
        .bg(ws.ui.surface)
        .text_color(hsla(ws.theme.palette.foreground, 1.0))
        .border_l_1()
        .border_color(ui.hairline)
        .shadow(vec![BoxShadow {
            color: ui.shadow,
            offset: point(px(-8.0), px(0.0)),
            blur_radius: px(32.0),
            spread_radius: px(0.0),
            inset: false,
        }])
        .track_focus(&ws.settings_focus)
        .on_key_down(cx.listener(Workspace::on_settings_key))
        // A press anywhere in the panel stops recording a shortcut (a row's click, on release,
        // starts it again).
        .capture_any_mouse_down(cx.listener(|ws, _: &MouseDownEvent, _, cx| {
            if ws.recording.take().is_some() {
                ws.key_note = None;
                cx.notify();
            }
        }))
        // Nothing under the panel (blocks, the canvas) gets its presses or its wheel.
        .occlude()
        .child(header)
        .child(tabs)
        .child(content)
        .child(footer)
        .test_support()
        .into_any_element()
}

fn tab(ui: Ui, section: Section, selected: bool, cx: &mut Context<Workspace>) -> AnyElement {
    div()
        .id(section.tab_id())
        .px(px(12.0))
        .py(px(4.0))
        .rounded_full()
        .border_1()
        .text_size(px(12.0))
        .cursor_pointer()
        .map(|el| {
            if selected {
                el.bg(ui.hover)
                    .border_color(ui.hairline)
                    .font_weight(FontWeight::MEDIUM)
            } else {
                el.border_color(gpui_kit::transparent_black())
                    .text_color(ui.title)
                    .hover(move |s| s.bg(ui.hover))
            }
        })
        .active(move |s| s.bg(ui.pressed))
        .child(section.title())
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.set_section(section, cx)))
        .test_support()
        .into_any_element()
}

/// The section's rows, read from the workspace as it is now.
fn rows(
    ws: &Workspace,
    entity: &Entity<Workspace>,
    section: Section,
    window: &mut Window,
    cx: &mut App,
) -> Vec<Row> {
    match section {
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
        Section::Font => vec![
            font_family_row(ws, entity),
            number_row(ws, entity, &FONT_SIZE, "Size", window, cx)
                .description("Points at 100% zoom"),
            Row::new("font.preview", "Preview").below(preview(ws)),
        ],
        Section::Terminal => {
            let blink = ws.config.cursor.blink;
            let e = entity.clone();
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
            ]
        }
        Section::Shell => {
            let shell = &ws.config.shell;
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
            rows
        }
        Section::Agents => ws
            .agents
            .iter()
            .map(|agent| agent_row(ws, entity, agent))
            .collect(),
        Section::Shortcuts => Command::ALL
            .map(|command| shortcut_row(ws, entity, command))
            .into_iter()
            .collect(),
    }
}

/// One agent: its command; whether the canvas's menu lists it; Edit; ↺ for one of Templum's the
/// user changed.
fn agent_row(ws: &Workspace, entity: &Entity<Workspace>, agent: &AgentEntry) -> Row {
    let ui = ws.ui;
    let id = agent.id.clone();
    let description = match agent.builtin {
        Some(_) => agent.command.clone(),
        None => format!("Yours · {}", agent.command),
    };
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

/// "Add an agent", under the agents.
fn add_agent(ui: Ui, cx: &mut Context<Workspace>) -> AnyElement {
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
    let builtin = agent.as_ref().and_then(|a| a.builtin);
    let id = agent.as_ref().map(|a| a.id.clone());
    let title = match &agent {
        Some(agent) => format!("Edit {}", agent.title),
        None => "Add an agent".to_string(),
    };
    let save = {
        let (entity, name, command, resume, id) = (
            entity.clone(),
            name.clone(),
            command.clone(),
            resume.clone(),
            id.clone(),
        );
        move |window: &mut Window, cx: &mut App| -> bool {
            let draft = Draft {
                name: name.read(cx).value().to_string(),
                command: command.read(cx).value().to_string(),
                resume: resume.read(cx).value().to_string(),
                show: draft.show,
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
    window.open_alert_dialog(cx, move |alert, _, _| {
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
                    .children(delete),
            )
            .on_ok(move |_, window, cx| save(window, cx))
    });
    first.update(cx, |input, cx| input.focus(window, cx));
}

/// Removes `table.key` from config.toml: the default again.
fn remove(
    entity: &Entity<Workspace>,
    table: &'static str,
    key: &'static str,
    to_default: impl Fn(&mut Config) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let entity = entity.clone();
    move |_, cx| {
        entity.update(cx, |ws, cx| {
            ws.edit(Edit::remove(table, key), &to_default, cx)
        })
    }
}

/// A button showing the current option, opening a menu of them; the current one is checked.
fn menu_button(
    id: impl Into<ElementId>,
    options: Vec<(SharedString, SharedString)>,
    current: SharedString,
    scrollable: bool,
    pick: impl Fn(SharedString, &mut App) + 'static,
) -> impl IntoElement {
    let label = options
        .iter()
        .find(|(value, _)| *value == current)
        .map_or(current.clone(), |(_, label)| label.clone());
    let pick = Rc::new(pick);
    Button::new(id)
        .label(label)
        .dropdown_caret(true)
        .outline()
        .small()
        .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
            options
                .iter()
                .fold(menu, |menu, (value, label)| {
                    let pick = pick.clone();
                    let value = value.clone();
                    menu.item(
                        PopupMenuItem::new(label.clone())
                            .checked(value == current)
                            .on_click(move |_, _, cx| pick(value.clone(), cx)),
                    )
                })
                .scrollable(scrollable)
        })
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
                ("inherit".into(), "Inherit from the focused terminal".into()),
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

fn number_row(
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

/// A text field's editor; each change goes to `set`.
struct TextState {
    input: Entity<InputState>,
    _change: Subscription,
}

/// A text field showing `value`; typing calls `set` with the whole text.
fn text_input(
    entity: &Entity<Workspace>,
    key: &'static str,
    value: String,
    set: fn(&mut Workspace, &str, &mut Context<Workspace>),
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let value = SharedString::from(value);
    let e = entity.clone();
    let initial = value.clone();
    let state = window.use_keyed_state(
        SharedString::from(format!("text-{key}")),
        cx,
        move |window, cx| {
            let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
            let _change = cx.subscribe(&input, move |_, input, event: &InputEvent, cx| {
                if let InputEvent::Change = event {
                    let text = input.read(cx).value();
                    e.update(cx, |ws, cx| set(ws, &text, cx));
                }
            });
            TextState { input, _change }
        },
    );
    // What the workspace shows wins over the field's text: a hand edit, a reset.
    state.update(cx, |state, cx| {
        if state.input.read(cx).value() != value {
            state
                .input
                .update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        }
    });
    Input::new(&state.read(cx).input)
        .small()
        .w(px(200.0))
        .into_any_element()
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

/// A dropdown setting: its options (value, label), the option the config is at, and what
/// choosing one does to the config and writes to the file.
struct Choice {
    table: &'static str,
    key: &'static str,
    options: &'static [(&'static str, &'static str)],
    get: fn(&Config) -> &'static str,
    set: fn(&mut Config, &str),
    toml: fn(&str) -> toml_edit::Value,
}

static MODE: Choice = Choice {
    table: "theme",
    key: "mode",
    options: &[("system", "System"), ("light", "Light"), ("dark", "Dark")],
    get: |c| match c.theme.mode {
        Mode::System => "system",
        Mode::Light => "light",
        Mode::Dark => "dark",
    },
    set: |c, v| {
        c.theme.mode = match v {
            "light" => Mode::Light,
            "dark" => Mode::Dark,
            _ => Mode::System,
        }
    },
    toml: |v| v.into(),
};

static CURSOR: Choice = Choice {
    table: "cursor",
    key: "shape",
    options: &[
        ("block", "Block"),
        ("bar", "Bar"),
        ("underline", "Underline"),
    ],
    get: |c| match c.cursor.shape {
        Shape::Block => "block",
        Shape::Bar => "bar",
        Shape::Underline => "underline",
    },
    set: |c, v| {
        c.cursor.shape = match v {
            "bar" => Shape::Bar,
            "underline" => Shape::Underline,
            _ => Shape::Block,
        }
    },
    toml: |v| v.into(),
};

/// A whole-number setting and its limits.
pub(crate) struct Number {
    table: &'static str,
    key: &'static str,
    min: f64,
    max: f64,
    step: f64,
    get: fn(&Config) -> f64,
    set: fn(&mut Config, f64),
}

pub(crate) static SCROLLBACK: Number = Number {
    table: "terminal",
    key: "scrollback",
    min: 0.0,
    max: 100_000.0,
    step: 1000.0,
    get: |c| c.terminal.scrollback as f64,
    set: |c, v| c.terminal.scrollback = v as usize,
};

/// A number field's editor. The value applies on Enter, when the field loses the focus, or on a
/// step (+/−, ↑/↓), never while typing: scrollback passing through "2" on the way to "20000"
/// would trim every terminal's history.
pub(crate) struct NumberState {
    pub input: Entity<InputState>,
    /// The config's value last shown: a different one (a hand edit, a reset) replaces the text.
    shown: f64,
    _events: [Subscription; 2],
}

impl NumberState {
    pub fn new(
        ws: Entity<Workspace>,
        n: &'static Number,
        value: f64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // No step, min or max on the input: it then leaves stepping to us (NumberInputEvent).
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()));
        let typed = {
            let ws = ws.clone();
            cx.subscribe_in(
                &input,
                window,
                move |_, input, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur)
                        && let Ok(v) = input.read(cx).value().trim().parse::<f64>()
                    {
                        set_number(&ws, n, v, cx);
                    }
                },
            )
        };
        let stepped = cx.subscribe_in(
            &input,
            window,
            move |_, input, event: &NumberInputEvent, window, cx| {
                let NumberInputEvent::Step(action) = event;
                let current = input
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<f64>()
                    .unwrap_or_else(|_| (n.get)(&ws.read(cx).config));
                let v = match action {
                    StepAction::Increment => current + n.step,
                    StepAction::Decrement => current - n.step,
                }
                .clamp(n.min, n.max);
                input.update(cx, |input, cx| input.set_value(v.to_string(), window, cx));
                set_number(&ws, n, v, cx);
            },
        );
        Self {
            input,
            shown: value,
            _events: [typed, stepped],
        }
    }
}

/// Typed values may have decimals or go past the limits; the file gets whole numbers within them.
fn set_number(ws: &Entity<Workspace>, n: &'static Number, v: f64, cx: &mut App) {
    let v = v.clamp(n.min, n.max).round();
    ws.update(cx, |ws, cx| {
        ws.edit(Edit::set(n.table, n.key, v as i64), |c| (n.set)(c, v), cx)
    });
}

static FONT_SIZE: Number = Number {
    table: "font",
    key: "size",
    min: 6.0,
    max: 72.0,
    step: 1.0,
    get: |c| f64::from(c.font.size),
    set: |c, v| c.font.size = v as f32,
};

/// Equal advances for "i" and "m": a monospaced font (GPUI doesn't say which fonts are).
fn is_monospace(i: f32, m: f32) -> bool {
    (i - m).abs() <= 0.01
}

/// The installed monospaced families, sorted. Measured when the panel opens: one advance pair
/// per family.
pub(crate) fn monospace_families(window: &Window) -> Vec<SharedString> {
    let ts = window.text_system();
    let installed = ts.all_font_names();
    let candidates = if cfg!(any(target_os = "macos", target_os = "windows")) {
        installed
    } else {
        safe_to_load(installed, fc_list_with_i_and_m())
    };
    candidates
        .into_iter()
        .filter(|name| {
            let id = ts.resolve_font(&font(name.clone()));
            // A family that doesn't load resolves to a fallback font: not this family.
            let loaded = ts
                .get_font_for_id(id)
                .is_some_and(|f| f.family.as_ref() == name.as_str());
            let advance = |c| ts.advance(id, px(14.0), c).map(|s| f32::from(s.width)).ok();
            loaded
                && matches!((advance('i'), advance('m')), (Some(i), Some(m)) if m > 0.0 && is_monospace(i, m))
        })
        .map(SharedString::from)
        .collect()
}

/// The families that are safe to load on Linux. There GPUI's text system drops a loaded family's
/// faces that have no "m" from the font database for good, and those are the emoji, symbol and
/// script fonts other text falls back to. So only families fontconfig says have both "i" and "m"
/// are loaded; without fc-list, only the built-in list.
fn safe_to_load(installed: Vec<String>, with_i_and_m: Option<HashSet<String>>) -> Vec<String> {
    installed
        .into_iter()
        .filter(|name| match &with_i_and_m {
            Some(families) => families.contains(name),
            None => grid::MONOSPACE.contains(&name.as_str()),
        })
        .collect()
}

/// Every family name of the fonts that have both "i" and "m", from fontconfig.
fn fc_list_with_i_and_m() -> Option<HashSet<String>> {
    let out = std::process::Command::new("fc-list")
        .args([":charset=69 6d", "--format", "%{family}\n"])
        .output()
        .ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .flat_map(|line| line.split(','))
            .map(str::to_owned)
            .collect()
    })
}

/// `font.family`; "" is Default (the key is removed: the first installed font of the built-in list).
pub(crate) fn set_family(ws: &mut Workspace, family: &str, cx: &mut Context<Workspace>) {
    if family.is_empty() {
        ws.edit(Edit::remove("font", "family"), |c| c.font.family = None, cx);
    } else {
        ws.edit(
            Edit::set("font", "family", family),
            |c| c.font.family = Some(family.to_string()),
            cx,
        );
    }
}

/// Splits on spaces; "…" or '…' keeps spaces (and the other quote) inside one argument.
pub(crate) fn split_args(text: &str) -> Result<Vec<String>, &'static str> {
    let mut args = Vec::new();
    let mut current: Option<String> = None;
    let mut quote: Option<char> = None;
    for c in text.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => current.get_or_insert_default().push(c),
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                current.get_or_insert_default();
            }
            None if c.is_whitespace() => args.extend(current.take()),
            None => current.get_or_insert_default().push(c),
        }
    }
    if quote.is_some() {
        return Err("Unclosed quote");
    }
    args.extend(current);
    Ok(args)
}

/// The text field's form of `args`: quoted where an argument is empty or has a space or a quote.
pub(crate) fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if !a.is_empty() && !a.contains(|c: char| c.is_whitespace() || c == '"' || c == '\'') {
                a.clone()
            } else if !a.contains('"') {
                format!("\"{a}\"")
            } else if !a.contains('\'') {
                format!("'{a}'")
            } else {
                // Both quotes: "…", each " written as "'"'" (adjacent quoted parts join up).
                format!("\"{}\"", a.replace('"', "\"'\"'\""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `shell.program`, trimmed; empty removes the key (the default shell). The typed
/// text is kept as a draft, so a space typed before the rest of a path stays in the field.
pub(crate) fn set_program(ws: &mut Workspace, text: &str, cx: &mut Context<Workspace>) {
    ws.drafts.insert("program", text.to_string());
    let text = text.trim();
    if text.is_empty() {
        ws.edit(
            Edit::remove("shell", "program"),
            |c| c.shell.program = None,
            cx,
        );
    } else {
        ws.edit(
            Edit::set("shell", "program", text),
            |c| c.shell.program = Some(text.to_string()),
            cx,
        );
    }
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

/// `shell.program` and `shell.args` from the list: a shell found here, or None for the default
/// (both keys removed).
pub(crate) fn set_shell(
    ws: &mut Workspace,
    shell: Option<&templum_terminal::ShellOption>,
    cx: &mut Context<Workspace>,
) {
    // The text fields show what the list chose.
    ws.drafts.remove("program");
    ws.drafts.remove("args");
    match shell {
        None => {
            ws.edit(
                Edit::remove("shell", "program"),
                |c| c.shell.program = None,
                cx,
            );
            ws.edit(Edit::remove("shell", "args"), |c| c.shell.args.clear(), cx);
        }
        Some(shell) => {
            let program = shell.program.clone();
            ws.edit(
                Edit::set("shell", "program", program.as_str()),
                |c| c.shell.program = Some(program.clone()),
                cx,
            );
            let args = shell.args.clone();
            let value: toml_edit::Array = args.iter().map(String::as_str).collect();
            ws.edit(
                Edit::set("shell", "args", value),
                |c| c.shell.args = args,
                cx,
            );
        }
    }
    cx.notify();
}

/// `shell.args` from the text field. An unclosed quote waits (the field says so); the typed text
/// is kept as a draft, so the field shows it rather than the config.
pub(crate) fn set_args(ws: &mut Workspace, text: &str, cx: &mut Context<Workspace>) {
    ws.drafts.insert("args", text.to_string());
    if let Ok(args) = split_args(text) {
        let value: toml_edit::Array = args.iter().map(String::as_str).collect();
        ws.edit(
            Edit::set("shell", "args", value),
            |c| c.shell.args = args,
            cx,
        );
    }
    cx.notify();
}

/// What the arguments field shows: the draft while it is what's in effect (or can't be yet),
/// else the config's.
pub(crate) fn args_text(ws: &Workspace) -> String {
    match ws.drafts.get("args") {
        Some(d) if split_args(d).map_or(true, |a| a == ws.config.shell.args) => d.clone(),
        _ => join_args(&ws.config.shell.args),
    }
}

pub(crate) fn args_error(ws: &Workspace) -> Option<&'static str> {
    ws.drafts.get("args").and_then(|d| split_args(d).err())
}

/// The start folder dropdown: "inherit", "home" or "folder" (the home folder's path, to edit).
pub(crate) fn set_folder_kind(ws: &mut Workspace, kind: &str, cx: &mut Context<Workspace>) {
    // What was typed in the folder field belongs to the choice being left.
    ws.drafts.remove("working_directory");
    let (value, wd) = match kind {
        "home" => ("~".to_string(), WorkingDirectory::Home),
        "folder" => {
            if matches!(ws.config.shell.working_directory, WorkingDirectory::Path(_)) {
                return;
            }
            let Some(home) = std::env::home_dir() else {
                return;
            };
            (home.display().to_string(), WorkingDirectory::Path(home))
        }
        _ => ("inherit".to_string(), WorkingDirectory::Inherit),
    };
    ws.edit(
        Edit::set("shell", "working_directory", value),
        |c| c.shell.working_directory = wd,
        cx,
    );
}

/// The folder field. "~" and "inherit" alone would switch the dropdown to Home or Inherit and
/// hide the field mid-typing: they wait for the rest of the path.
pub(crate) fn set_folder(ws: &mut Workspace, text: &str, cx: &mut Context<Workspace>) {
    ws.drafts.insert("working_directory", text.to_string());
    if matches!(text, "" | "~" | "inherit") {
        cx.notify();
        return;
    }
    let wd = WorkingDirectory::from(text.to_string());
    ws.edit(
        Edit::set("shell", "working_directory", text),
        |c| c.shell.working_directory = wd,
        cx,
    );
}

pub(crate) fn folder_text(ws: &Workspace) -> String {
    let current = &ws.config.shell.working_directory;
    match ws.drafts.get("working_directory") {
        Some(d)
            if matches!(d.as_str(), "" | "~" | "inherit")
                || WorkingDirectory::from(d.clone()) == *current =>
        {
            d.clone()
        }
        _ => match current {
            WorkingDirectory::Path(p) => p.display().to_string(),
            _ => String::new(),
        },
    }
}

/// Why the folder field's text isn't in effect yet (see `set_folder`).
pub(crate) fn folder_hint(ws: &Workspace) -> Option<&'static str> {
    match ws.drafts.get("working_directory")?.as_str() {
        "" => Some("Not applied: type a path"),
        "~" | "inherit" => Some("Not applied: type the rest of the path"),
        _ => None,
    }
}

/// What the program field shows: the draft while it is what's in effect, else the config's.
pub(crate) fn program_text(ws: &Workspace) -> String {
    let current = ws.config.shell.program.as_deref().unwrap_or_default();
    match ws.drafts.get("program") {
        Some(d) if d.trim() == current => d.clone(),
        _ => current.to_string(),
    }
}

fn set_theme(c: &mut Config, dark: bool, name: String) {
    if dark {
        c.theme.dark = name;
    } else {
        c.theme.light = name;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_choice_reads_back_what_it_writes() {
        for choice in [&MODE, &CURSOR] {
            for (value, _) in choice.options {
                let mut c = Config::default();
                (choice.set)(&mut c, value);
                assert_eq!((choice.get)(&c), *value, "{}.{}", choice.table, choice.key);
                let text = format!(
                    "[{}]\n{} = {}\n",
                    choice.table,
                    choice.key,
                    (choice.toml)(value)
                );
                let parsed = config::parse(&text).unwrap();
                assert_eq!((choice.get)(&parsed), *value, "{text}");
            }
        }
    }

    #[test]
    fn numbers_read_back_what_they_write() {
        for n in [&SCROLLBACK, &FONT_SIZE] {
            for v in [n.min, n.min + n.step, n.max] {
                let mut c = Config::default();
                (n.set)(&mut c, v);
                assert_eq!((n.get)(&c), v, "{}.{}", n.table, n.key);
                let text = format!("[{}]\n{} = {}\n", n.table, n.key, v as i64);
                assert_eq!((n.get)(&config::parse(&text).unwrap()), v, "{text}");
            }
        }
    }

    #[test]
    fn turning_back_mid_slide_starts_where_the_panel_is() {
        let start = Instant::now();
        let opening = Panel::new(start);
        for quarter in 0..=4 {
            let now = start + motion::SLIDE * quarter / 4;
            let closing = opening.reversed(now);
            assert!(closing.closing);
            assert!(
                (closing.shown(now) - opening.shown(now)).abs() < 1e-4,
                "closing at {quarter}/4"
            );
            let later = now + motion::SLIDE / 8;
            let reopening = closing.reversed(later);
            assert!(!reopening.closing);
            assert!(
                (reopening.shown(later) - closing.shown(later)).abs() < 1e-4,
                "reopening at {quarter}/4"
            );
        }
        let settled = start + motion::SLIDE * 2;
        assert_eq!(opening.reversed(settled).shown(settled), 1.0);
    }

    #[test]
    fn equal_advances_are_monospaced() {
        assert!(is_monospace(8.4, 8.4));
        assert!(is_monospace(8.4, 8.405));
        assert!(!is_monospace(3.9, 12.6));
    }

    #[test]
    fn args_split_on_spaces_and_keep_quoted_ones() {
        assert_eq!(split_args("--login -i").unwrap(), ["--login", "-i"]);
        assert_eq!(split_args("  -c  \"echo hi\" ").unwrap(), ["-c", "echo hi"]);
        assert_eq!(split_args("-c 'say \"hi\"'").unwrap(), ["-c", "say \"hi\""]);
        assert_eq!(split_args("\"\"").unwrap(), [""]);
        assert!(split_args("").unwrap().is_empty());
        assert_eq!(split_args("-c \"echo hi"), Err("Unclosed quote"));
    }

    #[test]
    fn joined_args_split_back() {
        let args: Vec<String> = [
            "--login",
            "a b",
            "it's",
            "say \"hi\"",
            "",
            "printf '%s' \"x\"",
            "echo \"it's\"",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(split_args(&join_args(&args)).unwrap(), args);
        assert_eq!(join_args(&["--login".into(), "-i".into()]), "--login -i");
    }

    #[test]
    fn only_families_with_i_and_m_are_loaded_on_linux() {
        let installed: Vec<String> = ["Fira Code", "JetBrains Mono", "Noto Color Emoji"]
            .map(String::from)
            .to_vec();
        let with_i_and_m: std::collections::HashSet<String> =
            ["Fira Code", "JetBrains Mono"].map(String::from).into();
        assert_eq!(
            safe_to_load(installed.clone(), Some(with_i_and_m)),
            ["Fira Code", "JetBrains Mono"]
        );
        assert_eq!(
            safe_to_load(installed, None),
            ["JetBrains Mono"],
            "without fc-list, the built-in list"
        );
    }
}
