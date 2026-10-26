//! The Agents popup (docs/specs/agents-view.md): every running agent of every workspace in one
//! list, drawn as every popup is (crate::popup), each with its state and age; the selected one's details
//! at the right; and the sidebar's button with the count of the agents that finished unseen.
//! Window chrome: the canvas's zoom never scales the popup. Its model is `workspace/agents_view.rs`.

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::input::Input;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClickEvent, Context, Div, ElementId, FontWeight, Hsla, KeyDownEvent,
    ScrollWheelEvent, TestSupportExt, Window, div, px, uniform_list,
};

use crate::balloon::{self, SIDE};
use crate::inbox::{age, count_badge, dim, section};
use crate::popup;
use crate::usage;
use crate::workspace::agents_view::{AgentRow, Doing, Item, SCREEN_LINES, Show, items};
use crate::workspace::{Pane, Workspace};

/// The list's width, pixels.
pub(crate) const LIST_W: f32 = 340.0;
/// Every item of the list, a header or a row.
const ROW: f32 = 48.0;
/// The text's sizes: the names, and what is said about them.
const TEXT: f32 = 13.0;
const SMALL: f32 = 11.5;
/// The lines of a handed task the details show.
const TASK_LINES: usize = 3;

/// The popup, `w` by `h` pixels: the list of every agent at the left, the selected one at the
/// right.
pub(crate) fn popup(
    ws: &Workspace,
    w: f32,
    h: f32,
    all: &[AgentRow],
    listed: Vec<AgentRow>,
    _window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let ui = ws.ui;
    let selected = ws.selected_in(&listed).map(|at| listed[at].clone());
    let now = cx.background_executor().now();
    let needs = all
        .iter()
        .filter(|row| matches!(row.doing, Doing::NeedsYou(_)))
        .count();
    popup::frame("agents", ws, w, h)
        .text_size(px(TEXT))
        // Before the search field, which has the keyboard, hears them.
        .capture_key_down(
            cx.listener(|ws, e: &KeyDownEvent, window, cx| ws.on_agents_key(e, window, cx)),
        )
        // The wheel scrolls the lists and stops there: the popup occludes the canvas under it.
        .on_scroll_wheel(|_: &ScrollWheelEvent, _, cx| cx.stop_propagation())
        .child(popup::header(
            ws,
            "agents",
            popup::tile(IconName::Bot, ui.accent),
            "Agents",
            Some(popup::subtitle(ws, running_words(all.len(), needs))),
            Vec::new(),
            cx,
        ))
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .child(list(
                    ws,
                    all,
                    listed,
                    selected.as_ref().map(|r| r.terminal.clone()),
                    cx,
                ))
                .child(div().w(px(1.0)).h_full().flex_shrink_0().bg(ui.hairline))
                .child(match &selected {
                    Some(row) => detail(ws, row, now, cx),
                    None => div().flex_1().into_any_element(),
                }),
        )
        .test_support()
        .into_any_element()
}

/// What the header says under "Agents": how many run, and how many of them need the person.
pub(crate) fn running_words(running: usize, needs: usize) -> String {
    let running = match running {
        0 => "No agent is running".to_string(),
        n => format!("{n} running"),
    };
    match needs {
        0 => running,
        1 => format!("{running} · 1 needs you"),
        n => format!("{running} · {n} need you"),
    }
}

/// The left column: the search, the chips, then the agents grouped by workspace.
fn list(
    ws: &Workspace,
    all: &[AgentRow],
    listed: Vec<AgentRow>,
    selected: Option<String>,
    cx: &mut Context<Workspace>,
) -> Div {
    let ui = ws.ui;
    let counts = ws.counts_in(all);
    let chips = Show::ALL.into_iter().zip(counts).map(|(show, count)| {
        let chosen = ws.agents_show == show;
        div()
            .id(ElementId::Name(
                format!("agents-chip-{}", show.name()).into(),
            ))
            .flex()
            .gap(px(5.0))
            .px(px(8.0))
            .py(px(3.0))
            .rounded(px(6.0))
            .text_size(px(SMALL))
            .cursor_pointer()
            .when(chosen, |el| el.bg(ui.pressed))
            .when(!chosen, |el| {
                el.text_color(ui.title_dim).hover(move |s| s.bg(ui.hover))
            })
            .child(div().font_weight(FontWeight::MEDIUM).child(show.name()))
            .child(count.to_string())
            .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.set_agents_show(show, cx)))
            .test_support()
    });
    let seen = (ws.agents_show == Show::Done && ws.done_count() > 0).then(|| {
        div().px(px(12.0)).pb(px(8.0)).flex().child(
            balloon::ghost("agents-seen", "Mark all seen", ui, 1.0)
                .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.mark_all_seen(cx)))
                .test_support(),
        )
    });
    let empty = listed.is_empty().then(|| {
        let words = empty_words(all.len());
        div()
            .id("agents-empty")
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .text_color(ui.title_dim)
            .child(words)
            .test_support()
    });
    // How many of each workspace's agents need the person: its header says so, whatever the
    // filter and the search hide.
    let mut needs: HashMap<String, usize> = HashMap::new();
    for row in all {
        if matches!(row.doing, Doing::NeedsYou(_)) {
            *needs.entry(row.space.clone()).or_default() += 1;
        }
    }
    let items = Rc::new(items(listed));
    let entity = cx.entity();
    // A virtual list: only the rows on screen are built.
    let rows = (!items.is_empty()).then(|| {
        let count = items.len();
        uniform_list("agents-list", count, move |range, _, cx| {
            let ws = entity.read(cx);
            let ui = ws.ui;
            let now = cx.background_executor().now();
            range
                .map(|ix| match &items[ix] {
                    Item::Header { space, name, dim } => {
                        let text = header_text(name, needs.get(space).copied().unwrap_or(0));
                        div()
                            .when(*dim, |el| el.opacity(0.5))
                            .h(px(ROW))
                            .px(px(12.0))
                            .pb(px(6.0))
                            .flex()
                            .items_end()
                            .text_size(px(SMALL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ui.title_dim)
                            .child(div().truncate().child(text))
                            .into_any_element()
                    }
                    Item::Row(row) => {
                        let lit = selected.as_deref() == Some(row.terminal.as_str());
                        let terminal = row.terminal.clone();
                        let entity = entity.clone();
                        agent_row(ws, row, now)
                            .when(row.dim, |el| el.opacity(0.5))
                            .when(lit, |el| el.bg(ui.hover))
                            .on_click(move |_: &ClickEvent, window, cx| {
                                entity.update(cx, |ws, cx| ws.select_agent(&terminal, window, cx))
                            })
                            .test_support()
                            .into_any_element()
                    }
                })
                .collect()
        })
        .track_scroll(&ws.agents_list)
        .flex_1()
        .min_h(px(0.0))
    });
    div()
        .w(px(LIST_W))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .children(ws.agents_search.as_ref().map(|search| {
            div()
                .id("agents-find")
                .px(px(12.0))
                .pt(px(12.0))
                .pb(px(8.0))
                .child(Input::new(search))
                .test_support()
        }))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(4.0))
                .px(px(12.0))
                .pb(px(8.0))
                .children(chips),
        )
        .children(seen)
        .child(div().h(px(1.0)).flex_shrink_0().bg(ui.hairline))
        .child(
            // The order holds still while the pointer is over the rows.
            div()
                .id("agents-rows")
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_col()
                .on_hover(cx.listener(|ws, hovered: &bool, _, cx| {
                    if ws.agents_hover != *hovered {
                        ws.agents_hover = *hovered;
                        cx.notify();
                    }
                }))
                .children(rows)
                .children(empty),
        )
}

/// A group's header: the workspace's name, and how many of its agents need the person.
pub(crate) fn header_text(name: &str, needs: usize) -> String {
    match needs {
        0 => name.to_string(),
        1 => format!("{name} · 1 needs you"),
        n => format!("{name} · {n} need you"),
    }
}

/// What the empty list says, `running` agents being there before the filter and the search.
pub(crate) fn empty_words(running: usize) -> &'static str {
    match running {
        0 => "No agent is running",
        _ => "No agent matches",
    }
}

/// The color of the dot of the block: none where the block shows no dot (Idle, Done).
pub(crate) fn dot_color(ws: &Workspace, pane: Option<&Pane>) -> Option<Hsla> {
    pane.and_then(|pane| crate::block::status_dot(ws, pane))
        .map(|(color, _)| color)
}

/// The color of the state's words: its dot's, the accent for Done.
pub(crate) fn tone(ws: &Workspace, row: &AgentRow, pane: Option<&Pane>) -> Option<Hsla> {
    match row.doing {
        Doing::Done => Some(ws.ui.accent),
        _ => dot_color(ws, pane),
    }
}

/// One agent of the list: its dot, title, agent and context; under them what it works on and its
/// state with its age.
fn agent_row(ws: &Workspace, row: &AgentRow, now: Instant) -> gpui_kit::Stateful<Div> {
    let ui = ws.ui;
    let pane = ws.pane(&row.terminal);
    let color = tone(ws, row, pane);
    let dot = div().size(px(8.0)).flex_shrink_0().rounded_full();
    // The block's own dot; a hollow ring where it has none.
    let dot = match dot_color(ws, pane) {
        Some(color) => dot
            .id(ElementId::Name(
                format!("agents-dot-{}", row.terminal).into(),
            ))
            .bg(color)
            .test_support(),
        None => dot
            .id(ElementId::Name(
                format!("agents-ring-{}", row.terminal).into(),
            ))
            .border_1()
            .border_color(ui.title_dim)
            .test_support(),
    };
    // What it works on: the task's first line, else its card, else its folder.
    let about = row
        .task
        .as_ref()
        .and_then(|task| task.lines().next().map(str::to_string))
        .filter(|line| !line.is_empty())
        .or_else(|| {
            row.card
                .as_ref()
                .map(|(id, title)| format!("#{id} {title}"))
        })
        .or_else(|| row.folder.clone())
        .unwrap_or_default();
    let state = format!(
        "{} · {}",
        row.doing.label(),
        age(now.saturating_duration_since(row.since))
    );
    div()
        .id(ElementId::Name(
            format!("agents-row-{}", row.terminal).into(),
        ))
        .w_full()
        .h(px(ROW))
        .flex_shrink_0()
        .overflow_hidden()
        .px(px(12.0))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(2.0))
        .cursor_pointer()
        .hover(move |s| s.bg(ui.hover))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .child(dot)
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .child(row.title.clone()),
                )
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .text_size(px(SMALL))
                        .text_color(ui.title_dim)
                        .child(row.agent.clone()),
                )
                .children(row.context.clone().map(|context| {
                    div()
                        .flex_shrink_0()
                        .text_size(px(SMALL))
                        .text_color(ui.title_dim)
                        .child(context)
                })),
        )
        .child(
            div()
                .flex()
                .gap(px(6.0))
                .text_size(px(SMALL))
                .text_color(ui.title_dim)
                .child(div().flex_1().truncate().child(about))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(color.unwrap_or(ui.title_dim))
                        .child(state),
                ),
        )
}

/// The agent and the block's title, as the inbox names a block.
fn named(ws: &Workspace, terminal: &str) -> Option<String> {
    let pane = ws.pane(terminal)?;
    let title = pane.term.title();
    Some(match ws.agent_name(pane) {
        Some(agent) => format!("{agent} · {title}"),
        None => title,
    })
}

/// The right column: where the agent comes from, what it does and costs, and its screen, over a
/// foot that stays.
fn detail(ws: &Workspace, row: &AgentRow, now: Instant, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let Some(pane) = ws.pane(&row.terminal) else {
        return div().flex_1().into_any_element();
    };
    // Another computer's agent is what home says of it.
    let home = pane.remote.as_ref().and_then(|r| r.agent.as_ref());
    let (model, effort, role) = match (&pane.remote, home) {
        (Some(_), home) => (
            home.and_then(|a| a.model.clone()),
            home.and_then(|a| a.effort.clone()),
            home.and_then(|a| a.role.clone()),
        ),
        (None, _) => (pane.model.clone(), pane.effort.clone(), pane.role.clone()),
    };
    let model = model
        .or_else(|| pane.usage.as_ref().and_then(|u| u.model.clone()))
        .map(|model| match &effort {
            Some(effort) => format!("{model} · {effort}"),
            None => model,
        });
    let from = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(section(ws, "From"))
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .child(if row.agent.is_empty() {
                    "Agent".to_string()
                } else {
                    row.agent.clone()
                }),
        )
        .children(model.map(|model| dim(ws, model)))
        .children(role.map(|role| dim(ws, format!("Role: {role}"))))
        .child(dim(ws, row.title.clone()))
        .children(row.folder.clone().map(|folder| dim(ws, folder)))
        .child(dim(ws, format!("in {}", row.space_name)));
    let waited = age(now.saturating_duration_since(row.since));
    let state = div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(section(ws, "State"))
        .child(div().child(format!("{} for {waited}", row.doing.label())))
        .children(match &row.doing {
            Doing::NeedsYou(asks) if !asks.is_empty() => Some(dim(ws, asks.clone())),
            _ => None,
        });
    let task = match home {
        Some(home) => home
            .task
            .as_ref()
            .filter(|t| !t.text.is_empty())
            .map(|task| {
                let lines: Vec<&str> = task.text.lines().take(TASK_LINES).collect();
                let from = task.by.as_ref().map(|by| match by.as_str() {
                    templum_api::ASKER => "from the API".to_string(),
                    by => match ws.uid_title_in(&row.space, by, cx) {
                        Some(title) => format!("from {title}"),
                        None => format!("from {by}"),
                    },
                });
                let wall = crate::workspace::automations::unix(std::time::SystemTime::now());
                let since =
                    std::time::Duration::from_secs(wall.saturating_sub(task.since).max(0) as u64);
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(section(ws, "Task"))
                    .child(dim(ws, task.id.clone()))
                    .child(div().child(lines.join("\n")))
                    .children(from.map(|from| dim(ws, from)))
                    .child(dim(ws, format!("{} ago", age(since))))
            }),
        None => pane
            .handed
            .as_ref()
            .filter(|h| !h.text.is_empty())
            .map(|handed| {
                let lines: Vec<&str> = handed.text.lines().take(TASK_LINES).collect();
                let from = named(ws, &handed.by).map(|by| format!("from {by}"));
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(section(ws, "Task"))
                    .children((handed.id > 0).then(|| dim(ws, format!("t{}", handed.id))))
                    .child(div().child(lines.join("\n")))
                    .children(from.map(|from| dim(ws, from)))
                    .child(dim(
                        ws,
                        format!("{} ago", age(now.saturating_duration_since(handed.since))),
                    ))
            }),
    };
    let card = row.card.as_ref().map(|(id, title)| {
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(section(ws, "Card"))
            .child(div().child(format!("#{id} {title}")))
    });
    // Home's agent has its subagents' count, not their lines.
    let remote_subagents = home.filter(|a| a.subagents > 0).map(|a| {
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(section(ws, "Subagents"))
            .child(div().child(format!("{} subagents", a.subagents)))
    });
    let subagents = (!pane.subagents.is_empty()).then(|| {
        let working = pane.subagents_at_work();
        let reported = pane.subagents.len() - working;
        let lines = pane.subagents.iter().map(|s| {
            let title = ws
                .spaces()
                .flat_map(|space| space.terminals())
                .find(|(_, p)| {
                    p.follows
                        .as_ref()
                        .is_some_and(|f| f.parent == row.terminal && f.id == s.id)
                })
                .map(|(_, p)| p.term.title())
                .unwrap_or_else(|| s.id.clone());
            let what = if s.reported { "reported" } else { "working" };
            div()
                .flex()
                .gap(px(6.0))
                .child(div().flex_1().truncate().child(title))
                .child(div().text_color(ui.title_dim).child(what))
        });
        div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(section(ws, "Subagents"))
            .child(div().child(format!("{working} working · {reported} reported")))
            .children(lines)
    });
    let spent = pane.usage.as_ref().and_then(|s| {
        let (_, tip) = usage::chip(s)?;
        Some((
            s.context_percent,
            usage::with_effort(tip, effort.as_deref()),
        ))
    });
    let spent = spent.map(|(percent, tip)| {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(section(ws, "Usage"))
            .children(percent.map(|p| usage::bar(p, 200.0, ui.accent, ui.hairline)))
            .children(
                tip.lines()
                    .map(|line| dim(ws, line.to_string()))
                    .collect::<Vec<_>>(),
            )
    });
    // The viewer's lines when home streams it, else the last read of its screen.
    let mut lines = pane.term.shared().lines(SCREEN_LINES);
    if lines.is_empty()
        && let (Some(remote), Some(home)) = (
            ws.space_named(&row.space).and_then(|s| s.remote.as_deref()),
            &pane.remote,
        )
        && let Some((block, read)) = &remote.screen
        && *block == home.block
    {
        lines = read.clone();
    }
    let screen = (!lines.is_empty()).then(|| {
        div()
            .flex()
            .flex_col()
            .gap(px(4.0))
            .child(section(ws, "Screen"))
            .child(
                div()
                    .id("agents-screen")
                    .w_full()
                    .overflow_hidden()
                    .p(px(8.0))
                    .rounded(px(6.0))
                    .bg(ui.card)
                    .font_family(ws.cell.font.family.clone())
                    .text_size(px(SMALL))
                    .children(lines.into_iter().map(|line| {
                        let line = if line.is_empty() {
                            "\u{a0}".to_string()
                        } else {
                            line
                        };
                        div().truncate().child(line)
                    }))
                    .test_support(),
            )
    });
    let terminal = row.terminal.clone();
    let asked = ws.asking(&row.terminal).is_some();
    let foot = div()
        .flex()
        .items_center()
        .flex_shrink_0()
        .gap(px(8.0))
        .px(px(SIDE))
        .py(px(10.0))
        .border_t_1()
        .border_color(ui.hairline)
        .child({
            let terminal = terminal.clone();
            balloon::ghost("agents-go", "Go to terminal", ui, 1.0)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.go_to_agent(&terminal, window, cx)
                }))
                .test_support()
        })
        .children(asked.then(|| {
            balloon::ghost("agents-request", "Open request", ui, 1.0)
                .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.open_agent_request(&terminal, window, cx)
                }))
                .test_support()
        }));
    let body = div()
        .id(ElementId::Name(
            format!("agents-details-{}", row.terminal).into(),
        ))
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(14.0))
                .px(px(SIDE))
                .py(px(14.0))
                .child(from)
                .child(state)
                .children(task)
                .children(card)
                .children(subagents)
                .children(remote_subagents)
                .children(spent)
                .children(screen),
        );
    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .child(body)
        .child(foot)
        .into_any_element()
}

/// The sidebar's button: the popup toggled, and the count of the agents that finished unseen. It
/// never pulses: an animation in the sidebar redraws the whole window.
pub(crate) fn button(ws: &Workspace, size: f32, cx: &mut Context<Workspace>) -> Div {
    let ui = ws.ui;
    div()
        .relative()
        .child(
            div()
                .id("agents-button")
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(size))
                .rounded(px(10.0))
                .cursor_pointer()
                .when(ws.agents_open, move |el| el.bg(ui.pressed))
                .hover(move |s| s.bg(ui.hover))
                .active(move |s| s.bg(ui.pressed))
                .child(Icon::new(IconName::Bot).size(px(16.0)).text_color(ui.title))
                .tooltip(|window, cx| Tooltip::new("Agents").build(window, cx))
                .on_click(
                    cx.listener(|ws, _: &ClickEvent, window, cx| ws.toggle_agents(window, cx)),
                )
                .test_support(),
        )
        .children(count_badge(ws, ws.done_count(), ui.accent, "agents-badge"))
}
