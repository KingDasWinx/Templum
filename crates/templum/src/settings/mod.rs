//! The settings panel: it slides in from the right over the canvas. Tabs pick a section, and
//! each field is a row (title, description, control, reset) in one card, built every frame from
//! the workspace's state. A change goes through the workspace, which applies it on the next frame
//! and saves it shortly after.

use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::Button;
use gpui_kit::component::{Icon, Sizable as _, TITLE_BAR_HEIGHT};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, BoxShadow, ClickEvent, Context, FontWeight, MouseDownEvent, TestSupportExt, Window,
    div, point, px,
};

use crate::config;
use crate::grid::hsla;
use crate::motion;
use crate::theme::Ui;
use crate::workspace::Workspace;

pub(crate) mod accounts;
mod agents;
pub(crate) mod api;
mod colors;
pub(crate) mod connections;
mod edits;
mod fields;
mod fonts;
mod general;
mod git;
mod instructions;
pub(crate) mod row;
mod sections;
pub(crate) mod templum_agent;
#[cfg(test)]
mod tests;
mod updates;
mod welcome;

use agents::add_agent;
pub(crate) use edits::{
    args_error, args_text, folder_hint, folder_text, program_text, set_args, set_family,
    set_folder, set_folder_kind, set_git_program, set_program,
};
pub(crate) use fields::{NumberState, SCROLLBACK};
pub(crate) use fonts::monospace_families;
use instructions::add_instruction;
// What a row of Settings ▸ Instructions says: the tests read it.
#[cfg(test)]
pub(crate) use instructions::description as instruction_description;
// What the Git section's text areas call on each change: the tests type through them.
#[cfg(test)]
pub(crate) use git::{set_commit_instructions, set_commit_prompt};
use row::{Reset, Row};
use sections::rows;
pub(crate) use sections::set_theme;
pub(crate) use welcome::{theme_card, welcome_groups};

/// The panel's width, at most 60% of the canvas.
const WIDTH: f32 = 480.0;

/// The panel's sections, in tab order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Section {
    Appearance,
    General,
    Canvas,
    Font,
    Terminal,
    Shell,
    Tools,
    Git,
    Agents,
    Instructions,
    Connections,
    Api,
    TemplumAgent,
    Usage,
    Updates,
    Shortcuts,
}

impl Section {
    pub const ALL: [Section; 16] = [
        Section::Appearance,
        Section::General,
        Section::Canvas,
        Section::Font,
        Section::Terminal,
        Section::Shell,
        Section::Tools,
        Section::Git,
        Section::Agents,
        Section::Instructions,
        Section::Connections,
        Section::Api,
        Section::TemplumAgent,
        Section::Usage,
        Section::Updates,
        Section::Shortcuts,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Appearance => "Appearance",
            Section::Canvas => "Canvas",
            Section::Font => "Font",
            Section::Terminal => "Terminal",
            Section::Shell => "Shell",
            Section::Tools => "Tools",
            Section::Git => "Git",
            Section::Agents => "Agents",
            Section::Instructions => "Instructions",
            Section::Connections => "Connections",
            Section::Api => "API",
            Section::TemplumAgent => "Templum Agent",
            Section::Usage => "Usage",
            Section::Updates => "Updates",
            Section::Shortcuts => "Shortcuts",
        }
    }

    /// Its tab's element id.
    fn tab_id(self) -> &'static str {
        match self {
            Section::General => "settings-tab-general",
            Section::Appearance => "settings-tab-appearance",
            Section::Canvas => "settings-tab-canvas",
            Section::Font => "settings-tab-font",
            Section::Terminal => "settings-tab-terminal",
            Section::Shell => "settings-tab-shell",
            Section::Tools => "settings-tab-tools",
            Section::Git => "settings-tab-git",
            Section::Agents => "settings-tab-agents",
            Section::Instructions => "settings-tab-instructions",
            Section::Connections => "settings-tab-connections",
            Section::Api => "settings-tab-api",
            Section::TemplumAgent => "settings-tab-templum-agent",
            Section::Usage => "settings-tab-usage",
            Section::Updates => "settings-tab-updates",
            Section::Shortcuts => "settings-tab-shortcuts",
        }
    }

    /// A line under the heading, for what holds for the whole section.
    pub(crate) fn note(self) -> Option<&'static str> {
        match self {
            Section::Shell => Some("Applies to new terminals"),
            Section::Tools => {
                Some("The programs the Git block runs. Checked when the panel opens.")
            }
            Section::Git => Some(
                "Commit messages written by an AI agent: Generate ✨, beside a Git block's summary, \
                 sends it the diff of what you commit and fills the message for you to read.",
            ),
            Section::Agents => Some(
                "The AI agents in the canvas menu. Change one of Templum's, or add your own; ↺ puts \
                 one of Templum's back as it was.",
            ),
            Section::Instructions => Some(
                "Claude Code, Codex, Gemini CLI, Copilot CLI and Qwen Code read them as a session \
                 starts: a change reaches the sessions that start after it, and a running one at \
                 its next resume, /clear or compaction.",
            ),
            Section::Connections => Some(
                "What the agents in the canvas get from Templum: its MCP server, to read and type \
                 into the blocks wired to them, and its hooks, for the dot on their terminal.",
            ),
            Section::Api => Some(
                "Another door to Templum, from a phone, a dashboard or another computer: off until you \
                 turn it on, and it answers only to the tokens you make. Templum runs no server of \
                 its own: point your tunnel, proxy or VPN at its port.",
            ),
            Section::TemplumAgent => Some(
                "The chat at the bottom of the canvas: an agent that builds and runs the canvas for \
                 you, and asks before anything it can't take back.",
            ),
            Section::Usage => Some(
                "Claude Code's and Codex's plan limits, shown in the title bar, and each agent's \
                 session on its terminal. Templum reads what the agents write, never their login.",
            ),
            Section::Updates => Some(
                "New versions from GitHub Releases, for your system and the way Templum was \
                 installed. Templum downloads them and asks before restarting.",
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
        motion::slid(self.since, self.closing, now)
    }

    /// The panel sliding the other way, from where it is (no jump when turned back mid-slide).
    pub fn reversed(&self, now: Instant) -> Panel {
        Panel {
            since: motion::turned(self.since, self.closing, now),
            closing: !self.closing,
            section: self.section,
        }
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
    let card = card(ui, rows);
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
                            .id("settings-note")
                            .text_size(px(12.0))
                            .text_color(ui.title_dim)
                            .child(note)
                            .test_support(),
                    )
                })
                .child(div().mt(px(4.0)).child(card))
                .when(section == Section::Agents, |el| {
                    el.child(add_agent(ui, cx))
                        .child(accounts::accounts(ws, &entity, cx))
                })
                .when(section == Section::Instructions, |el| {
                    el.child(add_instruction(ui, cx))
                })
                .when(section == Section::Appearance, |el| {
                    el.child(colors::theme_card(ws, window, cx))
                }),
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
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_color(ui.title_dim)
                        .children(path.as_ref().map(|p| p.display().to_string())),
                )
                .child(
                    Button::new("show-welcome")
                        .label("Welcome")
                        .small()
                        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                            ws.close_settings(window, cx);
                            ws.open_welcome(false, window, cx);
                        })),
                )
                .when(path.is_some(), |el| {
                    el.child(
                        Button::new("edit-config")
                            .label("Edit config.toml")
                            .small()
                            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                                ws.open_config(window, cx)
                            })),
                    )
                }),
        )
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

/// Rows in one rounded card, hairlines between them.
fn card(ui: Ui, rows: Vec<Row>) -> gpui_kit::Div {
    div()
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
        )
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
