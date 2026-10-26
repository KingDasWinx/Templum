//! The Automations popup's right column: the selected automation as a form (docs/specs/automations.md,
//! The editor). What is typed is a draft (`Workspace::automation_drafts`) until Save; the fields
//! below are only the text boxes, kept in step with the draft that is shown.

use std::time::SystemTime;

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, NaiveTime, Timelike};
use gpui_kit::component::Sizable as _;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Div, ElementId, Entity, FontWeight, SharedString,
    Stateful, Subscription, TestSupportExt, Window, div, px,
};

use super::{SMALL, TEXT};
use crate::automations::clock::{Cron, next_after, time_words};
use crate::automations::{Automation, COOLDOWN, Do, FIRED, Heard, When, Who};
use crate::balloon;
use crate::grid::hsla;
use crate::popup;
use crate::settings::row::menu_button;
use crate::workspace::Workspace;
use crate::workspace::automations::NEW;
use crate::workspace::remote::Deed;
use crate::workspace::remote_rules::key_home;

/// The text boxes of the form: which one a change came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Field {
    Name,
    Time,
    Date,
    Minutes,
    Cron,
    Folder,
    Model,
    Effort,
    Stop,
    Cooldown,
    Command,
    Notify,
    Prompt,
    AgentPrompt,
}

/// The form's text boxes, made at the first open; `for_id` is the automation they show now.
pub(crate) struct Form {
    pub for_id: Option<u64>,
    name: Entity<InputState>,
    time: Entity<InputState>,
    date: Entity<InputState>,
    minutes: Entity<InputState>,
    cron: Entity<InputState>,
    folder: Entity<InputState>,
    model: Entity<InputState>,
    effort: Entity<InputState>,
    stop: Entity<InputState>,
    cooldown: Entity<InputState>,
    command: Entity<InputState>,
    notify: Entity<InputState>,
    prompt: Entity<TextareaState>,
    agent_prompt: Entity<TextareaState>,
    /// Every counts hours, not minutes.
    hours: bool,
    /// The placeholders the Name and the Folder boxes have now.
    placeholders: (String, String),
    /// What Save refused, until the next change.
    problem: Option<String>,
    /// A box whose text the form can't read: the draft keeps what it had.
    unreadable: Option<(Field, &'static str)>,
    _subs: Vec<Subscription>,
}

fn text(state: &Entity<InputState>, cx: &App) -> String {
    state.read(cx).value().to_string()
}

fn area(state: &Entity<TextareaState>, cx: &App) -> String {
    state.read(cx).value().to_string()
}

/// A box shows `value`, without telling anyone when it already does.
fn show(state: &Entity<InputState>, value: &str, window: &mut Window, cx: &mut App) {
    if state.read(cx).value() != value {
        let value = SharedString::from(value.to_string());
        state.update(cx, |input, cx| input.set_value(value, window, cx));
    }
}

fn show_area(state: &Entity<TextareaState>, value: &str, window: &mut Window, cx: &mut App) {
    if state.read(cx).value() != value {
        let value = SharedString::from(value.to_string());
        state.update(cx, |input, cx| input.set_value(value, window, cx));
    }
}

/// `HH:MM`.
fn time_text(t: NaiveTime) -> String {
    t.format("%H:%M").to_string()
}

fn when_of(a: &Automation) -> (Option<NaiveDate>, Option<NaiveTime>) {
    match &a.when {
        When::Once { at } => (Some(at.date()), Some(at.time())),
        When::Daily { at } | When::Weekly { at, .. } | When::Monthly { at, .. } => {
            (None, Some(*at))
        }
        _ => (None, None),
    }
}

/// The next full hour of the local clock.
fn next_hour() -> NaiveDateTime {
    let now = Local::now().naive_local();
    let hour = now
        .with_minute(0)
        .and_then(|t| t.with_second(0))
        .and_then(|t| t.with_nanosecond(0));
    hour.unwrap_or(now) + chrono::Duration::hours(1)
}

impl Workspace {
    /// Before a frame: the form's boxes exist and show the automation selected.
    pub(crate) fn prepare_automation_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.automations_open {
            return;
        }
        if self.automation_form.is_none() {
            self.automation_form = Some(Form::new(window, cx));
        }
        let selected = self.automation_selected();
        let Some(id) = selected.filter(|id| {
            self.automation_form
                .as_ref()
                .is_some_and(|f| f.for_id != Some(*id))
        }) else {
            return;
        };
        let Some(a) = self.automation_draft(id).cloned() else {
            return;
        };
        let Some(form) = self.automation_form.as_mut() else {
            return;
        };
        form.for_id = Some(id);
        form.problem = None;
        form.unreadable = None;
        form.load(&a, window, cx);
    }

    /// The Name box's placeholder is the preview sentence, the Folder's the folder a new block
    /// would open in: both follow the draft shown.
    pub(crate) fn follow_automation_draft(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.automations_open {
            return;
        }
        let Some(a) = self
            .automation_selected()
            .and_then(|id| self.automation_draft(id))
        else {
            return;
        };
        let sentence = self.automation_sentence(a, cx);
        let folder = self.automation_folder_shown(a);
        let Some(form) = self.automation_form.as_mut() else {
            return;
        };
        if form.placeholders != (sentence.clone(), folder.clone()) {
            for (state, words) in [(&form.name, &sentence), (&form.folder, &folder)] {
                let words = words.clone();
                state.update(cx, |input, cx| input.set_placeholder(words, window, cx));
            }
            form.placeholders = (sentence, folder);
        }
    }

    /// Types `value` into the box of `field`, as the person would.
    #[cfg(test)]
    pub(crate) fn type_in(
        &mut self,
        field: Field,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.automation_form.as_ref() else {
            return;
        };
        let say = |state: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>| {
            state.update(cx, |input, cx| {
                input.select_all(window, cx);
                input.replace(value.to_string(), window, cx);
            })
        };
        let say_area =
            |state: &Entity<TextareaState>, window: &mut Window, cx: &mut Context<Self>| {
                state.update(cx, |input, cx| {
                    input.select_all(window, cx);
                    input.replace(value.to_string(), window, cx);
                })
            };
        match field {
            Field::Name => say(&form.name, window, cx),
            Field::Time => say(&form.time, window, cx),
            Field::Date => say(&form.date, window, cx),
            Field::Minutes => say(&form.minutes, window, cx),
            Field::Cron => say(&form.cron, window, cx),
            Field::Folder => say(&form.folder, window, cx),
            Field::Model => say(&form.model, window, cx),
            Field::Effort => say(&form.effort, window, cx),
            Field::Stop => say(&form.stop, window, cx),
            Field::Cooldown => say(&form.cooldown, window, cx),
            Field::Command => say(&form.command, window, cx),
            Field::Notify => say(&form.notify, window, cx),
            Field::Prompt => say_area(&form.prompt, window, cx),
            Field::AgentPrompt => say_area(&form.agent_prompt, window, cx),
        }
    }

    /// Home refused a Save of the home rule `key`: its sentence is the editor's red line, if the
    /// form still shows that rule.
    pub(crate) fn home_save_refused(&mut self, key: u64, sentence: String) {
        if let Some(form) = self
            .automation_form
            .as_mut()
            .filter(|f| f.for_id == Some(key))
        {
            form.problem = Some(sentence);
        }
    }

    /// Makes the boxes show the draft again (after Revert).
    pub(crate) fn reload_automation_form(&mut self) {
        if let Some(form) = self.automation_form.as_mut() {
            form.for_id = None;
        }
    }

    /// A box changed: its text goes into the draft, or the form says it can't read it.
    fn form_edited(&mut self, field: Field, value: String, cx: &mut Context<Self>) {
        let Some(id) = self.automation_form.as_ref().and_then(|f| f.for_id) else {
            return;
        };
        if self.automation_draft(id).is_none() {
            return;
        }
        let value = value.as_str();
        let trimmed = value.trim();
        let optional = |v: &str| Some(v.trim().to_string()).filter(|v| !v.is_empty());
        let mut unreadable = None;
        let mut bad = |why: &'static str| unreadable = Some((field, why));
        let mut edit: Box<dyn FnOnce(&mut Automation)> = Box::new(|_| {});
        match field {
            Field::Name => edit = Box::new(move |a| a.name = trimmed.to_string()),
            Field::Time => match NaiveTime::parse_from_str(trimmed, "%H:%M") {
                Err(_) => bad("Write the time as HH:MM."),
                Ok(t) => {
                    edit = Box::new(move |a| match &mut a.when {
                        When::Once { at } => *at = at.date().and_time(t),
                        When::Daily { at } | When::Weekly { at, .. } | When::Monthly { at, .. } => {
                            *at = t
                        }
                        _ => {}
                    })
                }
            },
            Field::Date => match NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
                Err(_) => bad("Write the date as YYYY-MM-DD."),
                Ok(d) => {
                    edit = Box::new(move |a| {
                        if let When::Once { at } = &mut a.when {
                            *at = d.and_time(at.time());
                        }
                    })
                }
            },
            Field::Minutes => {
                let hours = self.automation_form.as_ref().is_some_and(|f| f.hours);
                match trimmed.parse::<u32>() {
                    Err(_) => bad("Every runs from 1 minute to 7 days."),
                    Ok(n) => {
                        let minutes = n.saturating_mul(if hours { 60 } else { 1 });
                        edit = Box::new(move |a| {
                            if let When::Every { minutes: m } = &mut a.when {
                                *m = minutes;
                            }
                        })
                    }
                }
            }
            Field::Cron => {
                let expr = trimmed.to_string();
                edit = Box::new(move |a| {
                    if let When::Cron { expr: e } = &mut a.when {
                        *e = expr;
                    }
                })
            }
            Field::Folder => {
                let folder = optional(value);
                edit = Box::new(move |a| match &mut a.act {
                    Do::Command { folder: f, .. } | Do::Agent { folder: f, .. } => *f = folder,
                    _ => {}
                })
            }
            Field::Model => {
                let model = optional(value);
                edit = Box::new(move |a| {
                    if let Do::Agent { model: m, .. } = &mut a.act {
                        *m = model;
                    }
                })
            }
            Field::Effort => {
                let effort = optional(value);
                edit = Box::new(move |a| {
                    if let Do::Agent { effort: e, .. } = &mut a.act {
                        *e = effort;
                    }
                })
            }
            Field::Stop => {
                if trimmed.is_empty() {
                    edit = Box::new(|a| a.max_runs = None);
                } else {
                    match trimmed.parse::<u32>() {
                        Err(_) => bad("Write a number, or leave it empty."),
                        Ok(n) => edit = Box::new(move |a| a.max_runs = Some(n).filter(|n| *n > 0)),
                    }
                }
            }
            Field::Cooldown => {
                if trimmed.is_empty() {
                    edit = Box::new(|a| a.cooldown = COOLDOWN);
                } else {
                    match trimmed.parse::<u32>() {
                        Err(_) => bad("Write the cooldown in minutes."),
                        Ok(n) => edit = Box::new(move |a| a.cooldown = n),
                    }
                }
            }
            Field::Command => {
                let command = trimmed.to_string();
                edit = Box::new(move |a| {
                    if let Do::Command { text, .. } = &mut a.act {
                        *text = command;
                    }
                })
            }
            Field::Notify => {
                let note = trimmed.to_string();
                edit = Box::new(move |a| {
                    if let Do::Notify { text } = &mut a.act {
                        *text = note;
                    }
                })
            }
            Field::Prompt => {
                let prompt = value.trim_end().to_string();
                edit = Box::new(move |a| {
                    if let Do::Prompt { text, .. } = &mut a.act {
                        *text = prompt;
                    }
                })
            }
            Field::AgentPrompt => {
                let prompt = Some(value.trim_end().to_string()).filter(|p| !p.trim().is_empty());
                edit = Box::new(move |a| {
                    if let Do::Agent { prompt: p, .. } = &mut a.act {
                        *p = prompt;
                    }
                })
            }
        }
        if let Some(form) = self.automation_form.as_mut() {
            form.problem = None;
            match unreadable {
                Some(found) => form.unreadable = Some(found),
                None => {
                    if form.unreadable.is_some_and(|(f, _)| f == field) {
                        form.unreadable = None;
                    }
                }
            }
        }
        self.edit_automation(id, edit, cx);
    }

    /// The When dropdown: the draft's When becomes `kind`, from what the boxes hold.
    pub(crate) fn set_when_kind(&mut self, kind: &str, cx: &mut Context<Self>) {
        let Some(id) = self.automation_form.as_ref().and_then(|f| f.for_id) else {
            return;
        };
        let Some(form) = self.automation_form.as_ref() else {
            return;
        };
        let time = NaiveTime::parse_from_str(text(&form.time, cx).trim(), "%H:%M")
            .unwrap_or_else(|_| NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default());
        let minutes = text(&form.minutes, cx)
            .trim()
            .parse::<u32>()
            .ok()
            .filter(|n| *n > 0);
        let minutes = minutes.map_or(10, |n| n.saturating_mul(if form.hours { 60 } else { 1 }));
        let expr = Some(text(&form.cron, cx).trim().to_string())
            .filter(|e| !e.is_empty())
            .unwrap_or_else(|| "0 9 * * 1-5".into());
        let current = self.automation_draft(id).map(|a| a.when.clone());
        let when = match kind {
            "idle" | "needs_you" => {
                let (event, own) = match kind {
                    "idle" => (Heard::Idle, true),
                    _ => (Heard::NeedsYou, false),
                };
                // Who stays between the two events; All of them is for a turn's end only.
                let who = match current {
                    Some(When::Agent {
                        who: Who::Blocks { blocks, all },
                        ..
                    }) => Who::Blocks {
                        blocks,
                        all: all && own,
                    },
                    Some(When::Agent { who, .. }) => who,
                    _ => Who::Blocks {
                        blocks: Vec::new(),
                        all: false,
                    },
                };
                When::Agent { event, who }
            }
            "command" => match current {
                Some(command @ When::Command { .. }) => command,
                _ => When::Command {
                    terminal: String::new(),
                    failures: false,
                },
            },
            // The boxes hold the last rule's date and time, often past: a new Once is the next hour.
            "once" => match self.automation_draft(id).map(|a| &a.when) {
                Some(once @ When::Once { .. }) => once.clone(),
                _ => When::Once { at: next_hour() },
            },
            "every" => When::Every { minutes },
            "daily" => When::Daily { at: time },
            "weekly" => When::Weekly {
                days: vec![1, 2, 3, 4, 5],
                at: time,
            },
            "monthly" => When::Monthly {
                days: vec![1],
                at: time,
            },
            _ => When::Cron { expr },
        };
        let agent_event = matches!(when, When::Agent { .. });
        self.edit_automation(
            id,
            |a| {
                a.when = when;
                // Only an agent event has an agent that fired it.
                if let Do::Prompt { block, .. } = &mut a.act
                    && block == FIRED
                    && !agent_event
                {
                    block.clear();
                }
            },
            cx,
        );
        // The boxes the new kind uses show what the draft holds.
        self.reload_automation_form();
        cx.notify();
    }

    /// The Do dropdown: the draft's Do becomes `kind`, from what the boxes hold.
    pub(crate) fn set_do_kind(&mut self, kind: &str, cx: &mut Context<Self>) {
        let Some(id) = self.automation_form.as_ref().and_then(|f| f.for_id) else {
            return;
        };
        let Some(form) = self.automation_form.as_ref() else {
            return;
        };
        let optional = |s: String| Some(s.trim().to_string()).filter(|s| !s.is_empty());
        let folder = optional(text(&form.folder, cx));
        let act = match kind {
            "prompt" => Do::Prompt {
                block: String::new(),
                text: area(&form.prompt, cx).trim_end().into(),
            },
            "command" => Do::Command {
                terminal: None,
                folder,
                text: text(&form.command, cx).trim().into(),
            },
            "agent" => Do::Agent {
                agent: String::new(),
                folder,
                model: optional(text(&form.model, cx)),
                effort: optional(text(&form.effort, cx)),
                prompt: optional(area(&form.agent_prompt, cx)),
            },
            _ => Do::Notify {
                text: text(&form.notify, cx).trim().into(),
            },
        };
        let opens = matches!(act, Do::Agent { .. } | Do::Command { terminal: None, .. });
        let space = self.space.id.clone();
        self.edit_automation(
            id,
            |a| {
                a.act = act;
                if opens && a.workspace.is_none() {
                    a.workspace = Some(space);
                }
            },
            cx,
        );
        self.reload_automation_form();
        cx.notify();
    }

    /// The Who dropdown of an agent event: chosen blocks (none yet), or a kind (the first of
    /// Settings ▸ Agents).
    pub(crate) fn set_who_kind(&mut self, kind: &str, cx: &mut Context<Self>) {
        let Some(id) = self.automation_form.as_ref().and_then(|f| f.for_id) else {
            return;
        };
        // The option already chosen keeps the choice.
        let chosen = self.automation_draft(id).map(|a| &a.when);
        if matches!(
            (kind, chosen),
            (
                "kind",
                Some(When::Agent {
                    who: Who::Kind { .. },
                    ..
                })
            ) | (
                "blocks",
                Some(When::Agent {
                    who: Who::Blocks { .. },
                    ..
                })
            )
        ) {
            return;
        }
        let first = match key_home(id) {
            Some((space, _)) => self
                .home_kinds(&space)
                .first()
                .map(|(id, _)| id.to_string()),
            None => self.agents.first().map(|e| e.id.clone()),
        };
        let who = match kind {
            "kind" => Who::Kind {
                kind: first.unwrap_or_default(),
            },
            _ => Who::Blocks {
                blocks: Vec::new(),
                all: false,
            },
        };
        self.edit_automation(
            id,
            |a| {
                if let When::Agent { who: w, .. } = &mut a.when {
                    *w = who;
                }
            },
            cx,
        );
    }

    /// The Every unit dropdown.
    fn set_every_unit(&mut self, hours: bool, cx: &mut Context<Self>) {
        let Some(form) = self.automation_form.as_mut() else {
            return;
        };
        form.hours = hours;
        let value = text(&form.minutes, cx);
        self.form_edited(Field::Minutes, value, cx);
    }

    /// Save: the draft is checked; a problem is shown above the buttons.
    fn save_form(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(why) = self.automation_form.as_ref().and_then(|f| f.unreadable) {
            if let Some(form) = self.automation_form.as_mut() {
                form.problem = Some(why.1.to_string());
            }
            cx.notify();
            return;
        }
        match self.save_automation(id, cx) {
            // Home's rule is saved when home says so: its draft stays until then.
            Ok(_) if key_home(id).is_some() => {
                if let Some(form) = self.automation_form.as_mut() {
                    form.problem = None;
                }
            }
            Ok(_) => {
                if let Some(form) = self.automation_form.as_mut() {
                    form.problem = None;
                }
                self.reload_automation_form();
            }
            Err(problem) => {
                if let Some(form) = self.automation_form.as_mut() {
                    form.problem = Some(problem);
                }
            }
        }
        cx.notify();
    }
}

impl Form {
    fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Form {
        let mut subs = Vec::new();
        let mut input = |placeholder: &'static str,
                         field: Field,
                         window: &mut Window,
                         cx: &mut Context<Workspace>| {
            let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
            subs.push(
                cx.subscribe(&state, move |ws, state, event: &InputEvent, cx| {
                    if let InputEvent::Change = event {
                        let value = state.read(cx).value().to_string();
                        ws.form_edited(field, value, cx);
                    }
                }),
            );
            state
        };
        let name = input("What the list shows", Field::Name, window, cx);
        let time = input("HH:MM", Field::Time, window, cx);
        let date = input("YYYY-MM-DD", Field::Date, window, cx);
        let minutes = input("10", Field::Minutes, window, cx);
        let cron = input("minute hour day month weekday", Field::Cron, window, cx);
        let folder = input("The workspace's folder", Field::Folder, window, cx);
        let model = input("The agent's own", Field::Model, window, cx);
        let effort = input("The agent's own", Field::Effort, window, cx);
        let stop = input("No limit", Field::Stop, window, cx);
        let cooldown = input("5", Field::Cooldown, window, cx);
        let command = input("make test", Field::Command, window, cx);
        let notify = input("Stand-up in 5 minutes", Field::Notify, window, cx);
        let mut areas = Vec::new();
        let mut area_of = |placeholder: &'static str,
                           field: Field,
                           window: &mut Window,
                           cx: &mut Context<Workspace>| {
            let state = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .rows(4)
                    .placeholder(placeholder)
            });
            areas.push(
                cx.subscribe(&state, move |ws, state, event: &InputEvent, cx| {
                    if let InputEvent::Change = event {
                        let value = state.read(cx).value().to_string();
                        ws.form_edited(field, value, cx);
                    }
                }),
            );
            state
        };
        let prompt = area_of("What to type into the agent", Field::Prompt, window, cx);
        let agent_prompt = area_of(
            "Its first prompt (optional)",
            Field::AgentPrompt,
            window,
            cx,
        );
        subs.extend(areas);
        Form {
            for_id: None,
            name,
            time,
            date,
            minutes,
            cron,
            folder,
            model,
            effort,
            stop,
            cooldown,
            command,
            notify,
            prompt,
            agent_prompt,
            hours: false,
            placeholders: Default::default(),
            problem: None,
            unreadable: None,
            _subs: subs,
        }
    }

    /// The boxes show `a`.
    fn load(&mut self, a: &Automation, window: &mut Window, cx: &mut App) {
        show(&self.name, &a.name, window, cx);
        let (date, time) = when_of(a);
        if let Some(date) = date {
            show(&self.date, &date.format("%Y-%m-%d").to_string(), window, cx);
        } else {
            show(&self.date, "", window, cx);
        }
        if let Some(time) = time {
            show(&self.time, &time_text(time), window, cx);
        }
        match &a.when {
            When::Every { minutes } => {
                self.hours = *minutes >= 60 && minutes % 60 == 0;
                let n = if self.hours { minutes / 60 } else { *minutes };
                show(&self.minutes, &n.to_string(), window, cx);
            }
            When::Cron { expr } => show(&self.cron, expr, window, cx),
            _ => {}
        }
        show(
            &self.stop,
            &a.max_runs.map(|n| n.to_string()).unwrap_or_default(),
            window,
            cx,
        );
        show(&self.cooldown, &a.cooldown.to_string(), window, cx);
        match &a.act {
            Do::Prompt { text, .. } => show_area(&self.prompt, text, window, cx),
            Do::Command { folder, text, .. } => {
                show(
                    &self.folder,
                    folder.as_deref().unwrap_or_default(),
                    window,
                    cx,
                );
                show(&self.command, text, window, cx);
            }
            Do::Agent {
                folder,
                model,
                effort,
                prompt,
                ..
            } => {
                show(
                    &self.folder,
                    folder.as_deref().unwrap_or_default(),
                    window,
                    cx,
                );
                show(
                    &self.model,
                    model.as_deref().unwrap_or_default(),
                    window,
                    cx,
                );
                show(
                    &self.effort,
                    effort.as_deref().unwrap_or_default(),
                    window,
                    cx,
                );
                show_area(
                    &self.agent_prompt,
                    prompt.as_deref().unwrap_or_default(),
                    window,
                    cx,
                );
            }
            Do::Notify { text } => show(&self.notify, text, window, cx),
        }
        // The boxes this rule doesn't use are empty, not the last rule's text: a new kind made
        // from them starts blank.
        let (prompt, command, notify) = (
            matches!(a.act, Do::Prompt { .. }),
            matches!(a.act, Do::Command { .. }),
            matches!(a.act, Do::Notify { .. }),
        );
        let agent = matches!(a.act, Do::Agent { .. });
        for (used, state) in [
            (command || agent, &self.folder),
            (command, &self.command),
            (agent, &self.model),
            (agent, &self.effort),
            (notify, &self.notify),
        ] {
            if !used {
                show(state, "", window, cx);
            }
        }
        if !prompt {
            show_area(&self.prompt, "", window, cx);
        }
        if !agent {
            show_area(&self.agent_prompt, "", window, cx);
        }
        let when = &a.when;
        if !matches!(when, When::Every { .. }) {
            show(&self.minutes, "", window, cx);
        }
        if !matches!(when, When::Cron { .. }) {
            show(&self.cron, "", window, cx);
        }
    }
}

/// A field with its name before it and its unit after it, kept together on a row's line.
fn pair(ws: &Workspace, name: &'static str, field: Div, unit: Option<&'static str>) -> Div {
    let dim = |words: &'static str| {
        div()
            .flex_shrink_0()
            .text_size(px(SMALL))
            .text_color(ws.ui.title_dim)
            .child(words)
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(dim(name))
        .child(field)
        .children(unit.map(dim))
}

/// A small toggle: a weekday, a day of the month.
fn toggle(
    ws: &Workspace,
    id: impl Into<ElementId>,
    words: String,
    on: bool,
    cx: &mut Context<Workspace>,
    click: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
) -> Stateful<Div> {
    let ui = ws.ui;
    div()
        .id(id)
        .min_w(px(30.0))
        .max_w_full()
        .h(px(26.0))
        .px(px(6.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(6.0))
        .border_1()
        .border_color(if on { ui.accent } else { ui.hairline })
        .text_size(px(SMALL))
        .cursor_pointer()
        .when(on, move |el| {
            el.bg(ui.pressed).font_weight(FontWeight::SEMIBOLD)
        })
        .hover(move |s| s.bg(ui.hover))
        .child(div().min_w(px(0.0)).truncate().child(words))
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| click(ws, cx)))
}

/// The most characters of a dropdown label's title and of its workspace's name: a long title
/// never widens the column, and the block's id stays whole.
const TITLE_CHARS: usize = 20;
const SPACE_CHARS: usize = 12;

/// "Claude Code (abc12) · api", the title and the workspace cut, never the id.
pub(crate) fn block_label(title: &str, uid: &str, space: &str) -> String {
    format!(
        "{} ({uid}) · {}",
        cut(title, TITLE_CHARS),
        cut(space, SPACE_CHARS)
    )
}

/// The label of a block the draft names that isn't among the choices: "abc12 (in api)" when
/// another workspace has it, "abc12 (not open)" when none does.
pub(crate) fn kept_label(ws: &Workspace, uid: &str) -> String {
    match ws
        .find_block(uid)
        .and_then(|(space, _)| ws.spaces().find(|s| s.id == space))
    {
        Some(space) => format!("{uid} (in {})", cut(&space.name, SPACE_CHARS)),
        None => format!("{uid} (not open)"),
    }
}

/// `text` cut to `most` characters, with "…" when it was longer.
fn cut(text: &str, most: usize) -> String {
    match text.chars().count() > most {
        true => format!("{}…", text.chars().take(most - 1).collect::<String>()),
        false => text.to_string(),
    }
}

fn options(pairs: &[(&'static str, &'static str)]) -> Vec<(SharedString, SharedString)> {
    pairs
        .iter()
        .map(|(v, l)| ((*v).into(), (*l).into()))
        .collect()
}

fn when_kind(when: &When) -> &'static str {
    match when {
        When::Once { .. } => "once",
        When::Every { .. } => "every",
        When::Daily { .. } => "daily",
        When::Weekly { .. } => "weekly",
        When::Monthly { .. } => "monthly",
        When::Cron { .. } => "cron",
        When::Agent {
            event: Heard::Idle, ..
        } => "idle",
        When::Agent {
            event: Heard::NeedsYou,
            ..
        } => "needs_you",
        When::Command { .. } => "command",
    }
}

fn do_kind(act: &Do) -> &'static str {
    match act {
        Do::Prompt { .. } => "prompt",
        Do::Command { .. } => "command",
        Do::Agent { .. } => "agent",
        Do::Notify { .. } => "notify",
    }
}

/// The agents a rule may name: Settings ▸ Agents, or home's for a home rule (`home` its remote).
fn agent_choices(ws: &Workspace, home: &Option<String>) -> Vec<(SharedString, SharedString)> {
    match home {
        Some(space) => ws.home_kinds(space),
        None => ws
            .agents
            .iter()
            .map(|e| (e.id.clone().into(), e.title.clone().into()))
            .collect(),
    }
}

/// The next three times of the cron `expr`, or what is wrong with it.
fn cron_preview(expr: &str) -> Result<String, String> {
    Cron::parse(expr)?;
    let now: DateTime<Local> = Local::now();
    let when = When::Cron { expr: expr.into() };
    let mut times = Vec::new();
    let mut after = now;
    while times.len() < 3 {
        let Some(next) = next_after(&when, &now, &after) else {
            break;
        };
        times.push(time_words(next.naive_local(), now.naive_local()));
        after = next;
    }
    match times.is_empty() {
        true => Err("It would never run.".into()),
        false => Ok(format!("Next: {}", times.join(", "))),
    }
}

/// The agents' blocks a prompt can go to, and the terminals a command can run in: (uid, label),
/// in the workspace of the rule (every workspace for none).
pub(crate) fn blocks(
    ws: &Workspace,
    a: &Automation,
    agents: bool,
    cx: &App,
) -> Vec<(SharedString, SharedString)> {
    // A home rule names home's blocks, not this computer's.
    if let Some((space, _)) = key_home(a.id) {
        return ws.home_blocks(&space, agents, cx);
    }
    let mut found = Vec::new();
    for space in ws.order() {
        if space.is_remote() || a.workspace.as_ref().is_some_and(|w| *w != space.id) {
            continue;
        }
        for (id, pane) in space.terminals() {
            if pane.follows.is_some() || (agents && pane.agent().is_none()) {
                continue;
            }
            let Some(uid) = space.uids.get(&id) else {
                continue;
            };
            let title = ws.uid_title(uid, cx).unwrap_or_default();
            let label = block_label(&title, uid, &space.name);
            found.push((uid.clone().into(), label.into()));
        }
    }
    found
}

/// The workspaces a rule may be for: this computer's own (never a mirror: its automations run at
/// home), and "All workspaces" unless the rule opens something. A rule saved for a mirror before
/// keeps its value, labelled as the mirror it is, and one for a deleted workspace says so.
pub(crate) fn where_choices(ws: &Workspace, a: &Automation) -> Vec<(SharedString, SharedString)> {
    // A home rule is for home's workspace and no other: "API on winx-desktop".
    if let (Some((space, _)), Some(home)) = (key_home(a.id), a.workspace.as_ref()) {
        let words = ws.space_named(&space).and_then(|s| {
            let remote = s.remote.as_deref()?;
            Some(format!(
                "{} on {}",
                s.name,
                crate::mirror::computer_of(remote)
            ))
        });
        return vec![(home.clone().into(), words.unwrap_or_default().into())];
    }
    let opens = matches!(a.act, Do::Agent { .. } | Do::Command { terminal: None, .. });
    let mut places: Vec<(SharedString, SharedString)> = ws
        .order()
        .iter()
        .filter(|s| !s.is_remote())
        .map(|s| (s.id.clone().into(), s.name.clone().into()))
        .collect();
    if !opens {
        places.insert(0, ("".into(), "All workspaces".into()));
    }
    if let Some(current) = a
        .workspace
        .as_ref()
        .filter(|w| !places.iter().any(|(v, _)| v == w.as_str()))
    {
        let label = ws
            .order()
            .iter()
            .find(|s| s.id == *current)
            .and_then(|s| s.mirror_title())
            .unwrap_or_else(|| "A deleted workspace".into());
        places.push((current.clone().into(), label.into()));
    }
    places
}

/// A dropdown of `choices` with the chosen `current` (one that is not among them stays, as
/// `kept` labels it).
fn block_menu(
    id: &'static str,
    mut choices: Vec<(SharedString, SharedString)>,
    current: &str,
    kept: String,
    entity: &Entity<Workspace>,
    pick: fn(&mut Automation, String),
    draft: u64,
) -> impl IntoElement {
    if !current.is_empty() && !choices.iter().any(|(v, _)| *v == current) {
        choices.push((current.to_string().into(), kept.into()));
    }
    if choices.is_empty() {
        choices.push(("".into(), "None".into()));
    }
    let entity = entity.clone();
    menu_button(
        id,
        choices,
        current.to_string().into(),
        true,
        move |value, cx| {
            entity.update(cx, |ws, cx| {
                ws.edit_automation(draft, |a| pick(a, value.to_string()), cx)
            });
        },
    )
}

/// "Next: Tomorrow 09:00": when the draft `a` of `id` would run next. A draft counts from now,
/// as Save will; a saved rule from its own `since`; a home rule with no draft over it from the
/// next time home says.
pub(crate) fn preview_next(
    ws: &Workspace,
    id: u64,
    a: &Automation,
    now: SystemTime,
    cx: &App,
) -> String {
    if let Some((_, rule)) = ws
        .home_rule(id)
        .filter(|_| !ws.automation_drafts.contains_key(&id))
        .filter(|_| !a.when.is_event())
    {
        return match rule.next {
            Some(next) => format!(
                "Next: {}",
                time_words(
                    crate::workspace::automations::local(next).naive_local(),
                    DateTime::<Local>::from(now).naive_local()
                )
            ),
            None => "Next: never".to_string(),
        };
    }
    // An event has no time to name: it says what it waits for.
    if a.when.is_event() {
        let said = ws.when_said(a, cx);
        let mut letters = said.chars();
        let first = letters.next().map(|c| c.to_lowercase().to_string());
        return format!("Runs {}{}", first.unwrap_or_default(), letters.as_str());
    }
    let now = DateTime::<Local>::from(now);
    let since = match ws
        .automations
        .get(id)
        .filter(|_| !ws.automation_drafts.contains_key(&id))
    {
        Some(saved) => crate::workspace::automations::local(saved.since),
        None => now,
    };
    match next_after(&a.when, &since, &now) {
        Some(next) => format!(
            "Next: {}",
            time_words(next.naive_local(), now.naive_local())
        ),
        None => "Next: never".to_string(),
    }
}

/// The right column for the automation `id`.
pub(crate) fn editor(ws: &Workspace, id: u64, cx: &mut Context<Workspace>) -> AnyElement {
    let ui = ws.ui;
    let (Some(form), Some(a)) = (ws.automation_form.as_ref(), ws.automation_draft(id)) else {
        return div().flex_1().into_any_element();
    };
    let entity = cx.entity();
    let now = SystemTime::now();
    let red = hsla(ws.theme.palette.ansi[1], 1.0);
    let yellow = hsla(ws.theme.palette.ansi[3], 1.0);
    let saved = ws.rule(id);
    let home = key_home(id).map(|(space, _)| space);
    // Home's blocks and agents have no title here but home's: a block it lacks is "not open".
    let kept_label = |ws: &Workspace, uid: &str| match &home {
        Some(_) => format!("{uid} (not open)"),
        None => kept_label(ws, uid),
    };

    // When.
    let kind = when_kind(&a.when);
    let when_menu = {
        let entity = entity.clone();
        menu_button(
            "automation-when",
            options(&[
                ("once", "Once"),
                ("every", "Every"),
                ("daily", "Every day"),
                ("weekly", "On weekdays"),
                ("monthly", "On days of the month"),
                ("cron", "Advanced (cron)"),
                ("idle", "When an agent finishes its turn"),
                ("needs_you", "When an agent needs you"),
                ("command", "When a command ends"),
            ]),
            kind.into(),
            false,
            move |value, cx| entity.update(cx, |ws, cx| ws.set_when_kind(&value, cx)),
        )
    };
    let fixed = |input: &Entity<InputState>, width: f32| {
        div().w(px(width)).child(Input::new(input).small())
    };
    // What sits beside the When dropdown, and the rows under it.
    let (when_inline, when_rows): (Option<AnyElement>, Vec<AnyElement>) = match &a.when {
        When::Once { .. } => (
            Some(
                popup::line()
                    .child(fixed(&form.date, 140.0))
                    .child(fixed(&form.time, 90.0))
                    .into_any_element(),
            ),
            Vec::new(),
        ),
        When::Every { .. } => {
            let unit = {
                let entity = entity.clone();
                menu_button(
                    "automation-unit",
                    options(&[("minutes", "minutes"), ("hours", "hours")]),
                    (if form.hours { "hours" } else { "minutes" }).into(),
                    false,
                    move |value, cx| {
                        entity.update(cx, |ws, cx| ws.set_every_unit(value == "hours", cx))
                    },
                )
            };
            (
                Some(
                    popup::line()
                        .child(fixed(&form.minutes, 90.0))
                        .child(unit)
                        .into_any_element(),
                ),
                Vec::new(),
            )
        }
        When::Daily { .. } => (Some(fixed(&form.time, 90.0).into_any_element()), Vec::new()),
        When::Agent { event, who } => {
            let who_kind = match who {
                Who::Blocks { .. } => "blocks",
                Who::Kind { .. } => "kind",
            };
            let who_menu = {
                let entity = entity.clone();
                menu_button(
                    "automation-who",
                    options(&[("blocks", "Chosen agents"), ("kind", "Any agent of a kind")]),
                    who_kind.into(),
                    false,
                    move |value, cx| entity.update(cx, |ws, cx| ws.set_who_kind(&value, cx)),
                )
            };
            let choose = match who {
                Who::Blocks {
                    blocks: chosen,
                    all,
                } => {
                    let mut offered = blocks(ws, a, true, cx);
                    for uid in chosen {
                        if !offered.iter().any(|(v, _)| v.as_ref() == uid.as_str()) {
                            offered.push((uid.clone().into(), kept_label(ws, uid).into()));
                        }
                    }
                    let chips = offered.into_iter().map(|(uid, words)| {
                        let on = chosen.iter().any(|c| c.as_str() == uid.as_ref());
                        let uid = uid.to_string();
                        toggle(
                            ws,
                            ElementId::Name(format!("automation-who-{uid}").into()),
                            words.to_string(),
                            on,
                            cx,
                            move |ws, cx| {
                                let Some(id) = ws.automation_form.as_ref().and_then(|f| f.for_id)
                                else {
                                    return;
                                };
                                ws.edit_automation(
                                    id,
                                    |a| {
                                        let When::Agent {
                                            who: Who::Blocks { blocks, all },
                                            ..
                                        } = &mut a.when
                                        else {
                                            return;
                                        };
                                        match blocks.iter().position(|b| *b == uid) {
                                            Some(at) => {
                                                blocks.remove(at);
                                            }
                                            None => blocks.push(uid.clone()),
                                        }
                                        // One block is not a set.
                                        if blocks.len() < 2 {
                                            *all = false;
                                        }
                                    },
                                    cx,
                                );
                            },
                        )
                        .test_support()
                    });
                    // All of them is for a turn's end, and for a set.
                    let set = (*event == Heard::Idle && chosen.len() > 1).then(|| {
                        let entity = entity.clone();
                        menu_button(
                            "automation-who-all",
                            options(&[("any", "Any of them"), ("all", "All of them")]),
                            (if *all { "all" } else { "any" }).into(),
                            false,
                            move |value, cx| {
                                entity.update(cx, |ws, cx| {
                                    let Some(id) =
                                        ws.automation_form.as_ref().and_then(|f| f.for_id)
                                    else {
                                        return;
                                    };
                                    ws.edit_automation(
                                        id,
                                        |a| {
                                            if let When::Agent {
                                                who: Who::Blocks { all, .. },
                                                ..
                                            } = &mut a.when
                                            {
                                                *all = value == "all";
                                            }
                                        },
                                        cx,
                                    );
                                })
                            },
                        )
                    });
                    popup::row(
                        ws,
                        "Agents",
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .child(
                                div()
                                    .w_full()
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(4.0))
                                    .children(chips),
                            )
                            .children(set),
                    )
                    .into_any_element()
                }
                Who::Kind { kind } => {
                    let choices = agent_choices(ws, &home);
                    let menu = block_menu(
                        "automation-kind",
                        choices,
                        kind,
                        format!("{kind} (removed)"),
                        &entity,
                        |a, value| {
                            if let When::Agent {
                                who: Who::Kind { kind },
                                ..
                            } = &mut a.when
                            {
                                *kind = value;
                            }
                        },
                        id,
                    );
                    popup::row(ws, "Kind", menu).into_any_element()
                }
            };
            (
                None,
                vec![
                    popup::row(ws, "Who", popup::line().child(who_menu)).into_any_element(),
                    choose,
                ],
            )
        }
        When::Command { terminal, failures } => {
            let on = *failures;
            let line = popup::line()
                .child(block_menu(
                    "automation-event-terminal",
                    blocks(ws, a, false, cx),
                    terminal,
                    kept_label(ws, terminal),
                    &entity,
                    |a, value| {
                        if let When::Command { terminal, .. } = &mut a.when {
                            *terminal = value;
                        }
                    },
                    id,
                ))
                .child(
                    toggle(
                        ws,
                        ElementId::Name("automation-failures".into()),
                        "Failures only".into(),
                        on,
                        cx,
                        move |ws, cx| {
                            let Some(id) = ws.automation_form.as_ref().and_then(|f| f.for_id)
                            else {
                                return;
                            };
                            ws.edit_automation(
                                id,
                                |a| {
                                    if let When::Command { failures, .. } = &mut a.when {
                                        *failures = !on;
                                    }
                                },
                                cx,
                            );
                        },
                    )
                    .test_support(),
                );
            (
                None,
                vec![popup::row(ws, "Terminal", line).into_any_element()],
            )
        }
        When::Weekly { days, .. } => {
            let chips = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
                .into_iter()
                .enumerate()
                .map(|(i, name)| {
                    let day = i as u8 + 1;
                    toggle(
                        ws,
                        ElementId::Name(format!("automation-day-{day}").into()),
                        name.to_string(),
                        days.contains(&day),
                        cx,
                        move |ws, cx| {
                            let Some(id) = ws.automation_form.as_ref().and_then(|f| f.for_id)
                            else {
                                return;
                            };
                            ws.edit_automation(
                                id,
                                |a| {
                                    if let When::Weekly { days, .. } = &mut a.when {
                                        match days.iter().position(|d| *d == day) {
                                            Some(at) => {
                                                days.remove(at);
                                            }
                                            None => days.push(day),
                                        }
                                        days.sort_unstable();
                                    }
                                },
                                cx,
                            );
                        },
                    )
                });
            (
                Some(fixed(&form.time, 90.0).into_any_element()),
                vec![
                    popup::row(
                        ws,
                        "Days",
                        div().flex().flex_wrap().gap(px(4.0)).children(chips),
                    )
                    .into_any_element(),
                ],
            )
        }
        When::Monthly { days, .. } => {
            let rows = (0..5u8).map(|row| {
                let chips = (1..=7u8)
                    .map(|col| row * 7 + col)
                    .filter(|d| *d <= 31)
                    .map(|day| {
                        toggle(
                            ws,
                            ElementId::Name(format!("automation-date-{day}").into()),
                            day.to_string(),
                            days.contains(&day),
                            cx,
                            move |ws, cx| {
                                let Some(id) = ws.automation_form.as_ref().and_then(|f| f.for_id)
                                else {
                                    return;
                                };
                                ws.edit_automation(
                                    id,
                                    |a| {
                                        if let When::Monthly { days, .. } = &mut a.when {
                                            match days.iter().position(|d| *d == day) {
                                                Some(at) => {
                                                    days.remove(at);
                                                }
                                                None => days.push(day),
                                            }
                                            days.sort_unstable();
                                        }
                                    },
                                    cx,
                                );
                            },
                        )
                    });
                div().flex().gap(px(4.0)).children(chips)
            });
            let days = div().flex().flex_col().gap(px(4.0)).children(rows).child(
                div()
                    .text_size(px(SMALL))
                    .text_color(ui.title_dim)
                    .child("29, 30 and 31 run on the last day of shorter months."),
            );
            (
                Some(fixed(&form.time, 90.0).into_any_element()),
                vec![popup::row(ws, "Days", days).into_any_element()],
            )
        }
        When::Cron { expr } => {
            let under = match cron_preview(expr) {
                Ok(next) => div()
                    .text_size(px(SMALL))
                    .text_color(ui.title_dim)
                    .child(next),
                Err(why) => div().text_size(px(SMALL)).text_color(red).child(why),
            };
            let field = div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(Input::new(&form.cron).small())
                .child(under.id("automation-cron-next").test_support());
            (
                None,
                vec![popup::row(ws, "Expression", field).into_any_element()],
            )
        }
    };

    // Do.
    let do_menu = {
        let entity = entity.clone();
        menu_button(
            "automation-do",
            options(&[
                ("prompt", "Send a prompt to an agent"),
                ("command", "Run a command"),
                ("agent", "Start an agent"),
                ("notify", "Notify me"),
            ]),
            do_kind(&a.act).into(),
            false,
            move |value, cx| entity.update(cx, |ws, cx| ws.set_do_kind(&value, cx)),
        )
    };
    let draft = id;
    // The rows under the Do dropdown.
    let do_rows: Vec<AnyElement> = match &a.act {
        Do::Prompt { block, .. } => vec![
            popup::row(
                ws,
                "Agent",
                block_menu(
                    "automation-block",
                    {
                        // An agent event can type into the agent that fired it.
                        let mut choices = blocks(ws, a, true, cx);
                        if matches!(a.when, When::Agent { .. }) {
                            choices.insert(0, (FIRED.into(), "The agent that fired it".into()));
                        }
                        choices
                    },
                    block,
                    kept_label(ws, block),
                    &entity,
                    |a, value| {
                        if let Do::Prompt { block, .. } = &mut a.act {
                            *block = value;
                        }
                    },
                    draft,
                ),
            )
            .into_any_element(),
            popup::row(
                ws,
                "Prompt",
                div()
                    .id("automation-prompt")
                    .child(Textarea::new(&form.prompt).w_full())
                    .test_support(),
            )
            .into_any_element(),
        ],
        Do::Command { terminal, .. } => {
            let in_terminal = terminal.is_some();
            let place = {
                let entity = entity.clone();
                menu_button(
                    "automation-command-in",
                    options(&[("new", "A new terminal"), ("terminal", "A terminal")]),
                    (if in_terminal { "terminal" } else { "new" }).into(),
                    false,
                    move |value, cx| {
                        entity.update(cx, |ws, cx| {
                            let Some(id) = ws.automation_form.as_ref().and_then(|f| f.for_id)
                            else {
                                return;
                            };
                            let terminal = (value == "terminal").then(String::new);
                            ws.edit_automation(
                                id,
                                |a| {
                                    if let Do::Command { terminal: t, .. } = &mut a.act {
                                        *t = terminal;
                                    }
                                },
                                cx,
                            );
                        })
                    },
                )
            };
            let target = match terminal {
                Some(current) => popup::row(
                    ws,
                    "Terminal",
                    block_menu(
                        "automation-terminal",
                        blocks(ws, a, false, cx),
                        current,
                        kept_label(ws, current),
                        &entity,
                        |a, value| {
                            if let Do::Command { terminal, .. } = &mut a.act {
                                *terminal = Some(value);
                            }
                        },
                        draft,
                    ),
                ),
                None => popup::row(ws, "Folder", Input::new(&form.folder).small()),
            };
            vec![
                popup::row(ws, "In", popup::line().child(place)).into_any_element(),
                target.into_any_element(),
                popup::row(ws, "Command", Input::new(&form.command).small()).into_any_element(),
            ]
        }
        Do::Agent { agent, .. } => {
            let choices = agent_choices(ws, &home);
            vec![
                popup::row(
                    ws,
                    "Agent",
                    block_menu(
                        "automation-agent",
                        choices,
                        agent,
                        format!("{agent} (removed)"),
                        &entity,
                        |a, value| {
                            if let Do::Agent { agent, .. } = &mut a.act {
                                *agent = value;
                            }
                        },
                        draft,
                    ),
                )
                .into_any_element(),
                popup::row(ws, "Folder", Input::new(&form.folder).small()).into_any_element(),
                popup::row(
                    ws,
                    "Model",
                    popup::line().child(fixed(&form.model, 140.0)).child(pair(
                        ws,
                        "Effort",
                        fixed(&form.effort, 110.0),
                        None,
                    )),
                )
                .into_any_element(),
                popup::row(
                    ws,
                    "Prompt",
                    div()
                        .id("automation-agent-prompt")
                        .child(Textarea::new(&form.agent_prompt).w_full())
                        .test_support(),
                )
                .into_any_element(),
            ]
        }
        Do::Notify { .. } => {
            vec![popup::row(ws, "Text", Input::new(&form.notify).small()).into_any_element()]
        }
    };

    // Where.
    let places = where_choices(ws, a);
    let where_menu = {
        let entity = entity.clone();
        menu_button(
            "automation-where",
            places,
            a.workspace.clone().unwrap_or_default().into(),
            true,
            move |value, cx| {
                entity.update(cx, |ws, cx| {
                    let Some(id) = ws.automation_form.as_ref().and_then(|f| f.for_id) else {
                        return;
                    };
                    ws.edit_automation(
                        id,
                        |a| a.workspace = Some(value.to_string()).filter(|v| !v.is_empty()),
                        cx,
                    );
                })
            },
        )
    };

    // The preview, the next run and the last one.
    let sentence = ws.automation_sentence(a, cx);
    let next = if id == NEW || saved.is_some() {
        let missing = ws.automation_target(a).err();
        let words = match (&missing, saved.is_some_and(|s| !s.on)) {
            (Some(why), _) => why.clone(),
            (None, true) => "Next: never, it is off".to_string(),
            (None, false) => preview_next(ws, id, a, now, cx),
        };
        Some((words, missing.is_some()))
    } else {
        None
    };
    let last = saved.and_then(|_| ws.automation_last_line(id, now));
    let problem = form
        .problem
        .clone()
        .or_else(|| form.unreadable.map(|(_, why)| why.to_string()));

    // What it does and when it runs next, apart from the fields that say it.
    let summary = div()
        .id("automation-summary")
        .flex()
        .flex_col()
        .gap(px(4.0))
        .p(px(12.0))
        .rounded(px(8.0))
        .bg(ui.card)
        .child(
            div()
                .id("automation-sentence")
                .w_full()
                .overflow_hidden()
                .font_weight(FontWeight::MEDIUM)
                .child(sentence)
                .test_support(),
        )
        .children(next.map(|(words, missing)| {
            div()
                .id("automation-next")
                .w_full()
                .overflow_hidden()
                .text_size(px(SMALL))
                .text_color(if missing { yellow } else { ui.title_dim })
                .child(div().truncate().child(words))
                .test_support()
        }))
        .children(last.map(|words| {
            div()
                .id("automation-last")
                .w_full()
                .overflow_hidden()
                .text_size(px(SMALL))
                .text_color(ui.title_dim)
                .child(words)
                .test_support()
        }))
        .test_support();
    let numbers = popup::line()
        .child(where_menu)
        .child(pair(
            ws,
            "Stop after",
            fixed(&form.stop, 80.0),
            Some("runs"),
        ))
        .children(
            a.when
                .is_event()
                .then(|| pair(ws, "Cooldown", fixed(&form.cooldown, 80.0), Some("minutes"))),
        );
    let body = div()
        .id("automation-editor")
        .flex_1()
        .min_h(px(0.0))
        .overflow_y_scroll()
        .py(px(14.0))
        .child(
            popup::content()
                .px(px(16.0))
                .gap(px(18.0))
                .child(summary)
                .child(popup::row(ws, "Name", Input::new(&form.name).small()))
                .child(
                    popup::section(ws, "Trigger")
                        .child(popup::row(
                            ws,
                            "When",
                            popup::line().child(when_menu).children(when_inline),
                        ))
                        .children(when_rows),
                )
                .child(
                    popup::section(ws, "Action")
                        .child(popup::row(ws, "Do", popup::line().child(do_menu)))
                        .children(do_rows)
                        .children(ws.automation_loop(a).map(|words| {
                            div()
                                .id("automation-loop")
                                .pl(px(popup::LABEL_W + 12.0))
                                .text_size(px(SMALL))
                                .text_color(yellow)
                                .child(words)
                                .test_support()
                        })),
                )
                .child(popup::section(ws, "Options").child(popup::row(ws, "Where", numbers))),
        );

    // Home's rule is saved at home: from another computer it turns off when the share ends, and
    // Save says why when this share can't.
    let command = matches!(a.act, Do::Command { .. });
    let save_refused = home
        .as_deref()
        .and_then(|space| ws.why_not_in(space, Deed::SaveRule(command)));
    let from_elsewhere = home.as_deref().map(|space| {
        format!(
            "Saved from another computer, it turns off when this share ends. Save it on {} to keep it.",
            ws.space_named(space)
                .and_then(|s| s.remote.as_deref())
                .map(crate::mirror::computer_of)
                .unwrap_or_default()
        )
    });
    let saving = ws.home_saving(id);
    let save = balloon::solid(
        ElementId::Name("automation-save".into()),
        if saving { "Saving at home…" } else { "Save" },
        None,
        ui,
        1.0,
    )
    .when_some(save_refused, |el, why| {
        el.opacity(0.4)
            .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
    })
    .when(saving, |el| el.opacity(0.4))
    .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.save_form(id, cx)))
    .test_support();
    let revert = balloon::ghost("automation-revert", "Revert", ui, 1.0)
        .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| {
            ws.revert_automation(id, cx);
            ws.reload_automation_form();
        }))
        .test_support();
    let foot = popup::foot(ws).child(
        popup::content()
            .px(px(16.0))
            .gap(px(6.0))
            .children(from_elsewhere.map(|words| {
                div()
                    .id("automation-elsewhere")
                    .w_full()
                    .text_size(px(SMALL))
                    .text_color(ui.title_dim)
                    .child(words)
                    .test_support()
            }))
            .children(problem.map(|why| {
                div()
                    .id("automation-problem")
                    .w_full()
                    .text_size(px(SMALL))
                    .text_color(red)
                    .child(why)
                    .test_support()
            }))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.0))
                    .child(revert)
                    .child(save),
            ),
    );

    div()
        .flex_1()
        .min_w(px(0.0))
        .flex()
        .flex_col()
        .text_size(px(TEXT))
        .child(body)
        .child(foot)
        .into_any_element()
}
