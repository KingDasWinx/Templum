//! The Automations popup (docs/specs/automations.md): the person's rules at the left, the selected
//! one's editor at the right, drawn as every popup is (crate::popup); and the sidebar's button with the count
//! of the rules whose last run went wrong. Window chrome: the canvas's zoom never scales it. Its
//! model is `workspace/automations.rs`.

pub(crate) mod editor;

use std::time::SystemTime;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, FontWeight, KeyDownEvent,
    ScrollWheelEvent, Stateful, TestSupportExt, Window, div, px,
};

use crate::automations::{Do, Went};
use crate::git_block::widgets::icon_button;
use crate::grid::hsla;
use crate::inbox::{count_badge, section};
use crate::popup;
use crate::workspace::Workspace;
use crate::workspace::automations::NEW;
use crate::workspace::remote::Deed;
use crate::workspace::remote_rules::{RuleGroup, key_home};

/// A row of the list.
const ROW: f32 = 48.0;
/// The text's sizes: the names, and what is said about them.
pub(super) const TEXT: f32 = 13.0;
pub(super) const SMALL: f32 = 11.5;
/// What the empty list says under "No automations yet".
pub(crate) const EMPTY_HINT: &str = "Type a prompt, run a command, start an agent or notify you at a time you choose, or when an agent finishes.";
/// The most pixels of a row's last run: the words after it give way.
const LAST_W: f32 = 150.0;

/// What the header says under "Automations": how many this computer has.
pub(crate) fn count_words(count: usize) -> String {
    match count {
        0 => "No automations yet".to_string(),
        1 => "1 automation".to_string(),
        n => format!("{n} automations"),
    }
}

/// The popup, `w` by `h` pixels: the header with New, then the list at the left and the selected
/// automation at the right.
pub(crate) fn popup(ws: &Workspace, w: f32, h: f32, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let selected = ws.automation_selected();
    let mine = ws
        .automation_ids()
        .into_iter()
        .filter(|id| *id != NEW && key_home(*id).is_none())
        .count();
    let new = div()
        .id("automation-new")
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.0))
        .h(px(28.0))
        .px(px(12.0))
        .rounded(px(8.0))
        .bg(ui.accent.opacity(0.16))
        .text_color(ui.accent)
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .hover(move |s| s.bg(ui.accent.opacity(0.24)))
        .child(Icon::new(IconName::Plus).size(px(14.0)))
        .child("New")
        .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
            ws.new_automation(cx);
            ws.select_automation(NEW, window, cx);
        }))
        .test_support()
        .into_any_element();
    popup::frame("automations", ws, w, h)
        .text_size(px(TEXT))
        .track_focus(&ws.automations_focus)
        // Before the fields, which may have the keyboard, hear them.
        .capture_key_down(
            cx.listener(|ws, e: &KeyDownEvent, window, cx| ws.on_automations_key(e, window, cx)),
        )
        // The wheel scrolls the lists and stops there: the popup occludes the canvas under it.
        .on_scroll_wheel(|_: &ScrollWheelEvent, _, cx| cx.stop_propagation())
        .child(popup::header(
            ws,
            "automations",
            popup::tile(IconName::AlarmClock, ui.accent),
            "Automations",
            Some(popup::subtitle(ws, count_words(mine))),
            vec![new],
            cx,
        ))
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .child(list(ws, selected, cx))
                .child(div().w(px(1.0)).h_full().flex_shrink_0().bg(ui.hairline))
                .child(match selected {
                    Some(id) => editor::editor(ws, id, cx),
                    None => div().flex_1().into_any_element(),
                }),
        )
        .test_support()
        .into_any_element()
}

/// The left column: the automations, then what agents asked for, then home's.
fn list(ws: &Workspace, selected: Option<u64>, cx: &mut Context<Workspace>) -> Div {
    let ui = ws.ui;
    let now = SystemTime::now();
    // This computer's rules; home's are in the groups of their remotes.
    let ids: Vec<u64> = ws
        .automation_ids()
        .into_iter()
        .filter(|id| key_home(*id).is_none())
        .collect();
    let rows: Vec<_> = ids
        .iter()
        .map(|id| automation_row(ws, *id, selected == Some(*id), now, cx).test_support())
        .collect();
    let groups = ws.rule_groups(cx);
    let empty = (ids.is_empty() && groups.is_empty()).then(|| {
        div()
            .id("automations-empty")
            .px(px(16.0))
            .py(px(24.0))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(4.0))
            .text_color(ui.title_dim)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child("No automations yet"),
            )
            .child(div().text_size(px(SMALL)).text_center().child(EMPTY_HINT))
            .test_support()
    });
    let asked = ws.schedule_rows();
    let asked = (!asked.is_empty()).then(|| {
        let rows = asked.into_iter().map(|(id, from, about)| {
            schedule_row(
                ws,
                format!("{id}"),
                from,
                about,
                format!("Cancel s{id}"),
                None,
                cx.listener(move |ws, _: &ClickEvent, _, cx| ws.cancel_schedule_now(id, cx)),
            )
        });
        div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .child(
                div()
                    .px(px(12.0))
                    .pt(px(14.0))
                    .pb(px(6.0))
                    .child(section(ws, "Asked by agents")),
            )
            .children(rows)
    });
    div()
        .w(px(crate::agents_view::LIST_W))
        .h_full()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .child(
            div()
                .id("automations-rows")
                .flex_1()
                .min_h(px(0.0))
                .flex()
                .flex_col()
                .overflow_y_scroll()
                .children(rows)
                .children(empty)
                .children(asked)
                .children(
                    groups
                        .into_iter()
                        .map(|group| home_group(ws, group, selected, now, cx)),
                ),
        )
}

/// One remote workspace's group of the list: its title with a + for a new rule at home, home's
/// rules as rows, what its agents asked for with a Cancel each, and under them what isn't shown.
fn home_group(
    ws: &Workspace,
    group: RuleGroup,
    selected: Option<u64>,
    now: SystemTime,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let ui = ws.ui;
    let space = group.space.clone();
    let new_refused = ws.why_not_in(&space, Deed::SaveRule(false));
    let new = {
        let (space, dimmed) = (space.clone(), new_refused.is_some());
        let tip = new_refused.unwrap_or_else(|| "New automation at home".to_string());
        icon_button(
            ElementId::Name(format!("automation-new-{space}").into()),
            IconName::Plus,
            ui,
            1.0,
        )
        .when(dimmed, |el| el.opacity(0.4))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
            ws.new_home_automation(&space, cx);
            if let Some(key) = ws.automation_selected() {
                ws.select_automation(key, window, cx);
            }
        }))
        .test_support()
    };
    let rows: Vec<_> = group
        .keys
        .iter()
        .map(|key| automation_row(ws, *key, selected == Some(*key), now, cx).test_support())
        .collect();
    let schedules: Vec<_> = group
        .schedules
        .into_iter()
        .map(|(id, from, about)| {
            let (space, schedule) = (space.clone(), id.clone());
            schedule_row(
                ws,
                format!("{space}-{id}"),
                from,
                about,
                format!("Cancel {id}"),
                ws.why_not_in(&space, Deed::CancelSchedule),
                cx.listener(move |ws, _: &ClickEvent, window, cx| {
                    ws.cancel_home_schedule(&space, &schedule, window, cx)
                }),
            )
        })
        .collect();
    div()
        .id(ElementId::Name(format!("automation-group-{space}").into()))
        .flex()
        .flex_col()
        .flex_shrink_0()
        .when(group.dim, |el| el.opacity(0.5))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(12.0))
                .pt(px(14.0))
                .pb(px(6.0))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_size(px(SMALL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(ui.title_dim)
                        .child(group.title),
                )
                .child(new),
        )
        .children(rows)
        .children(schedules)
        .child(
            div()
                .px(px(12.0))
                .py(px(6.0))
                .text_size(px(SMALL))
                .text_color(ui.title_dim)
                .child(group.foot),
        )
}

/// A delivery an agent asked for: who asked, what and when, and a Cancel (dimmed with `refused`'s
/// words when this computer can't do it). `key` makes the ids of the row and its button.
fn schedule_row(
    ws: &Workspace,
    key: String,
    from: String,
    about: String,
    tip: String,
    refused: Option<String>,
    cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let ui = ws.ui;
    let dimmed = refused.is_some();
    let tip = refused.unwrap_or(tip);
    div()
        .id(ElementId::Name(format!("schedule-{key}").into()))
        .w_full()
        .h(px(ROW))
        .flex_shrink_0()
        .overflow_hidden()
        .px(px(12.0))
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(div().truncate().font_weight(FontWeight::MEDIUM).child(from))
                .child(
                    div()
                        .truncate()
                        .text_size(px(SMALL))
                        .text_color(ui.title_dim)
                        .child(about),
                ),
        )
        .child(
            crate::balloon::ghost(
                ElementId::Name(format!("schedule-cancel-{key}").into()),
                "Cancel",
                ui,
                1.0,
            )
            .flex_shrink_0()
            .when(dimmed, |el| el.opacity(0.4))
            .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
            .on_click(cancel)
            .test_support(),
        )
}

/// One automation of the list: its switch, name and next run; under them its When and its last
/// run. Nothing is drawn over them: its Run now, Duplicate and Delete are on its card in the
/// editor (editor::actions).
fn automation_row(
    ws: &Workspace,
    id: u64,
    lit: bool,
    now: SystemTime,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let ui = ws.ui;
    let Some(a) = ws.automation_draft(id) else {
        return div().id(ElementId::Name(format!("automation-row-{id}").into()));
    };
    let saved = ws.rule(id);
    // Home's rule: its acts are home's, dimmed with the rule's words when this share can't.
    let home = key_home(id).map(|(space, _)| space);
    let command = matches!(a.act, Do::Command { .. });
    let refused = |deed: Deed| home.as_deref().and_then(|space| ws.why_not_in(space, deed));
    let yellow = hsla(ws.theme.palette.ansi[3], 1.0);
    let red = hsla(ws.theme.palette.ansi[1], 1.0);
    let is_new = id == NEW || key_home(id).is_some_and(|(_, home_id)| home_id == 0);
    let name = match a.name.as_str() {
        "" if is_new => "New automation".to_string(),
        "" => ws.automation_sentence(a, cx),
        name => name.to_string(),
    };
    let missing = saved.and_then(|s| ws.automation_target(s).err());
    let off = saved.is_some_and(|s| !s.on) || missing.is_some();
    let when = ws.when_said(a, cx);
    let (last, tone) = match ws.last_words(id, now) {
        Some(words) => {
            let tone = match saved.and_then(|s| s.last.as_ref()).map(|l| l.went) {
                _ if words.starts_with("Waiting") => Some(yellow),
                Some(Went::Failed) => Some(red),
                Some(Went::Skipped | Went::Missed) => Some(yellow),
                _ => None,
            };
            (Some(words), tone)
        }
        None => (None, None),
    };
    let next = if is_new {
        "Not saved".to_string()
    } else {
        ws.next_words(id, now)
    };
    let drafted = !is_new && ws.automation_drafts.contains_key(&id);
    let switch_refused = refused(Deed::SetRule(command));
    let switch = saved.map(|s| {
        let entity = cx.entity();
        let switch = Switch::new(ElementId::Name(format!("automation-switch-{id}").into()))
            .checked(s.on)
            .on_click(move |on: &bool, _, cx| {
                let on = *on;
                entity.update(cx, |ws, cx| ws.set_automation_on(id, on, cx));
            });
        // Dimmed with the rule's words; a click still says them and sends nothing.
        div()
            .id(ElementId::Name(
                format!("automation-switch-box-{id}").into(),
            ))
            .when_some(switch_refused, |el, why| {
                el.opacity(0.4)
                    .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
            })
            .child(switch)
    });
    let dim = if off { ui.hint } else { ui.title_dim };
    div()
        .id(ElementId::Name(format!("automation-row-{id}").into()))
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
        .when(lit, |el| el.bg(ui.hover))
        .hover(move |s| s.bg(ui.hover))
        .on_click(
            cx.listener(move |ws, _: &ClickEvent, window, cx| ws.select_automation(id, window, cx)),
        )
        .child(
            div()
                .w_full()
                .flex()
                .items_center()
                .gap(px(8.0))
                .children(switch)
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .font_weight(FontWeight::MEDIUM)
                        .when(off, move |el| el.text_color(dim))
                        .child(name),
                )
                .children(drafted.then(|| {
                    div()
                        .id(ElementId::Name(format!("automation-draft-{id}").into()))
                        .size(px(6.0))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(ui.accent)
                        .test_support()
                }))
                .child(
                    div()
                        .id(ElementId::Name(format!("automation-next-{id}").into()))
                        .flex_shrink_0()
                        .max_w(px(LAST_W))
                        .truncate()
                        .text_size(px(SMALL))
                        .text_color(dim)
                        .child(next)
                        .test_support(),
                ),
        )
        .child(
            div()
                .w_full()
                .flex()
                .gap(px(6.0))
                .text_size(px(SMALL))
                .text_color(ui.title_dim)
                .child(match &missing {
                    Some(why) => div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .text_color(yellow)
                        .child(why.clone()),
                    None => div().flex_1().min_w(px(0.0)).truncate().child(when),
                })
                .children(last.filter(|_| missing.is_none()).map(|words| {
                    // A held run says why it waits.
                    let why = ws.automation_held_why(id);
                    div()
                        .id(ElementId::Name(format!("automation-lastrun-{id}").into()))
                        .flex_shrink_0()
                        .max_w(px(LAST_W))
                        .truncate()
                        .text_color(tone.unwrap_or(ui.title_dim))
                        .child(words)
                        .when_some(why, |el, why| {
                            el.tooltip(move |window, cx| {
                                Tooltip::new(why.clone()).build(window, cx)
                            })
                        })
                })),
        )
}

/// The sidebar's button: the popup toggled, and the count of the automations whose last run went
/// wrong since it was opened. It never pulses: an animation in the sidebar redraws the whole
/// window.
pub(crate) fn button(ws: &Workspace, size: f32, cx: &mut Context<Workspace>) -> Div {
    let ui = ws.ui;
    div()
        .relative()
        .child(
            div()
                .id("automations-button")
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .size(px(size))
                .rounded(px(10.0))
                .cursor_pointer()
                .when(ws.automations_open, move |el| el.bg(ui.pressed))
                .hover(move |s| s.bg(ui.hover))
                .active(move |s| s.bg(ui.pressed))
                .child(
                    Icon::new(IconName::AlarmClock)
                        .size(px(16.0))
                        .text_color(ui.title),
                )
                .tooltip(|window, cx| Tooltip::new("Automations").build(window, cx))
                .on_click(
                    cx.listener(|ws, _: &ClickEvent, window, cx| ws.toggle_automations(window, cx)),
                )
                .test_support(),
        )
        .children(count_badge(
            ws,
            ws.automations_unseen() + ws.home_unseen(),
            hsla(ws.theme.palette.ansi[1], 1.0),
            "automations-badge",
        ))
}
