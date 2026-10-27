//! Automations at run time (docs/specs/automations.md): a look every AUTOMATION_LOOK finds the
//! rules whose time came, holds a prompt or a command until its target is free, and runs each Do
//! through the operations that already exist. Times are the computer's clock (`SystemTime`,
//! `chrono::Local`): GPUI's timers don't count time asleep, so a time missed while it slept goes
//! at the first look after, once. The rules and their file are `crate::automations`.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, Local, NaiveDateTime, NaiveTime};
use gpui_kit::component::notification::{Notification, NotificationType};
use gpui_kit::{App, AppContext as _, Context, KeyDownEvent, Task, Window};
use templum_canvas::BlockId;

use std::path::PathBuf;

use templum_mcp::tools;

use super::agents_view::Doing;
use super::bus::Outcome;
use super::delegate::{NOTICE_GAP, TYPING};
use super::ops::{NewBlock, Place};
use super::remote_rules::key_home;
use super::spawn::Opener;
use super::{Pane, Workspace, show_notification};
use crate::automations::clock::{next_after, time_words, when_problem, when_words};
use crate::automations::{
    Automation, COOLDOWN, COOLDOWN_MOST, Do, FIRED, Heard, LastRun, NAME_MAX, Went, When, Who,
    event_short, event_words,
};
use crate::keys::Command;
use crate::launcher;

/// How often the rules that are on are looked at.
pub(crate) const AUTOMATION_LOOK: Duration = Duration::from_secs(1);
/// How long a prompt or a command waits for its target to be free.
pub(crate) const HOLD_MOST: Duration = Duration::from_secs(60 * 60);
/// The most characters of a quoted text in a sentence.
const QUOTE_CHARS: usize = 40;

/// The asker of an automation's tasks: a word no terminal has.
pub(crate) fn asker(id: u64) -> String {
    format!("automation-{id}")
}

/// What an automation is doing, in memory only.
#[derive(Default)]
pub(crate) struct Run {
    /// None: off, or never again.
    pub next: Option<SystemTime>,
    /// Its runs under way: None a time's or Run now's, Some(uid) an event's, by the block that
    /// fired it, so each agent's run is held or typed apart.
    pub slots: BTreeMap<Option<String>, Slot>,
    /// When an event last started a run, by the block that fired it ("" for All of them): the
    /// cooldown.
    pub cooled: HashMap<String, SystemTime>,
    /// A command rule's terminal's count of finished commands at the last look.
    pub finished: Option<u64>,
    /// Its last run went other than Ran since the popup was opened.
    pub unseen: bool,
}

/// One run under way.
#[derive(Default)]
pub(crate) struct Slot {
    /// Due, waiting for its target.
    pub held: Option<Held>,
    /// A try's answer is on its way (`Tried::Later`): no new try meanwhile.
    pub typing: bool,
    /// The run being typed counts toward Stop after; a time that falls due meanwhile joins it.
    pub typing_counts: bool,
}

impl Run {
    /// The slot of the run `by` fired (None: a time's), made when missing.
    pub(crate) fn slot(&mut self, by: Option<&str>) -> &mut Slot {
        self.slots.entry(by.map(str::to_string)).or_default()
    }

    /// The first run held, its own (a time's) first, and the block that fired it.
    pub(crate) fn held(&self) -> Option<(Option<&str>, &Held)> {
        (self.slots.iter()).find_map(|(by, slot)| Some((by.as_deref(), slot.held.as_ref()?)))
    }

    /// It was turned on, saved or off: the runs held are dropped, those being typed no longer
    /// count, and the cooldowns and the command's count start over.
    pub(crate) fn reset(&mut self) {
        self.slots.retain(|_, slot| {
            slot.held = None;
            slot.typing_counts = false;
            slot.typing
        });
        self.cooled.clear();
        self.finished = None;
    }
}

pub(crate) struct Held {
    pub since: SystemTime,
    pub why: String,
    /// It is a run that counts toward Stop after (not a Run now).
    pub counts: bool,
}

/// One try of a Do.
pub(super) enum Tried {
    Went(Went, String),
    /// Its answer, Ok(what happened) or Err(why not).
    Later(Task<Result<String, String>>),
    /// Not now: why.
    Hold(String),
}

pub(crate) fn unix(t: SystemTime) -> i64 {
    match t.duration_since(UNIX_EPOCH) {
        Ok(after) => after.as_secs() as i64,
        Err(before) => -(before.duration().as_secs() as i64),
    }
}

pub(crate) fn local(secs: i64) -> DateTime<Local> {
    DateTime::from_timestamp(secs, 0)
        .unwrap_or_default()
        .with_timezone(&Local)
}

/// The first time `a` falls due after `now`; None: never.
fn next_for(a: &Automation, now: SystemTime) -> Option<SystemTime> {
    next_after(&a.when, &local(a.since), &DateTime::<Local>::from(now)).map(SystemTime::from)
}

/// A folder as the rule has it ("~/api"), else the path it came to.
fn shown_folder(typed: Option<&str>, path: &std::path::Path) -> String {
    let typed = typed.map(str::trim).filter(|typed| !typed.is_empty());
    typed.map_or_else(|| path.display().to_string(), str::to_string)
}

/// The terminal a command opened still serves `new`: a command again, in the same terminal or
/// folder.
fn keeps_terminal(old: &Do, new: &Do) -> bool {
    match (old, new) {
        (
            Do::Command {
                terminal: t,
                folder: f,
                ..
            },
            Do::Command {
                terminal: t2,
                folder: f2,
                ..
            },
        ) => t == t2 && f == f2,
        _ => false,
    }
}

/// The first line of `text`, at most QUOTE_CHARS characters, in quotes.
fn quote(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() > QUOTE_CHARS || text.trim().lines().count() > 1 {
        let cut: String = line.chars().take(QUOTE_CHARS).collect();
        format!("\"{}…\"", cut.trim_end())
    } else {
        format!("\"{line}\"")
    }
}

/// `a` with "the agent that fired it" made the block `by`.
fn for_firer(a: &Automation, by: Option<&str>) -> Automation {
    let mut a = a.clone();
    if let (Do::Prompt { block, .. }, Some(by)) = (&mut a.act, by)
        && block == FIRED
    {
        *block = by.to_string();
    }
    a
}

/// The uid a prompt or a command waits for: the block it is typed into.
fn typed_into(a: &Automation) -> Option<&str> {
    match &a.act {
        Do::Prompt { block, .. } => Some(block),
        Do::Command { terminal, .. } => terminal.as_deref(),
        _ => None,
    }
}

impl Workspace {
    /// At start: a time that passed while Templum was closed is Missed, not run; the next time of
    /// each rule that is on is the first after `now`.
    pub(super) fn start_automations(&mut self, now: SystemTime, cx: &mut Context<Self>) {
        let missed_text = "Templum was closed.";
        for a in &mut self.automations.automations {
            if !a.on {
                continue;
            }
            if a.since == 0 {
                a.since = unix(now);
            }
            let from = a.since.max(a.last.as_ref().map_or(0, |l| l.at));
            let mut unseen = false;
            let first = next_after(&a.when, &local(a.since), &local(from)).map(SystemTime::from);
            if let Some(missed) = first.filter(|first| *first <= now) {
                a.last = Some(LastRun {
                    at: unix(missed),
                    went: Went::Missed,
                    about: missed_text.into(),
                });
                unseen = true;
                // A Once that was missed is as done as one that ran.
                if matches!(a.when, When::Once { .. }) {
                    a.on = false;
                }
            }
            let next = a.on.then(|| next_for(a, now)).flatten();
            self.automation_runs.insert(
                a.id,
                Run {
                    next,
                    unseen,
                    ..Run::default()
                },
            );
        }
        self.look_at_automations(cx);
    }

    /// Some rule that is on waits for an agent: the watch notes what agents do.
    pub(super) fn automations_listen(&self) -> bool {
        (self.automations.automations.iter()).any(|a| a.on && matches!(a.when, When::Agent { .. }))
    }

    /// Some rule is on, or has a run held: there is something to look at.
    fn automations_active(&self) -> bool {
        self.automations.automations.iter().any(|a| a.on)
            || (self.automation_runs.values()).any(|run| run.held().is_some())
    }

    /// Looks at the automations every AUTOMATION_LOOK while there is something to look at; started
    /// by the first rule turned on (or run by hand and held).
    pub(super) fn look_at_automations(&mut self, cx: &mut Context<Self>) {
        if self.automating || !self.automations_active() {
            return;
        }
        self.automating = true;
        let window = self.window_handle;
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor().timer(AUTOMATION_LOOK).await;
                let more = cx
                    .update_window(window, |_, window, cx| {
                        ws.update(cx, |ws, cx| ws.look_automations(window, cx))
                    })
                    .ok()
                    .and_then(Result::ok);
                if more != Some(true) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One look: the rules whose time came run (or are held), the events heard since the last
    /// look fire their rules, the held runs are tried again. A rule that is off is looked at only
    /// for a run held by hand. False once there is nothing to look at.
    pub(super) fn look_automations(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let now = SystemTime::now();
        let mut changed = false;
        // The queue never outlives a look.
        let heard = std::mem::take(&mut self.automation_heard);
        let fires = self.events_due(&heard);
        let held = |ws: &Self, id| {
            ws.automation_runs
                .get(&id)
                .is_some_and(|run| run.held().is_some())
        };
        let looked: Vec<Automation> = (self.automations.automations.iter())
            .filter(|a| a.on || held(self, a.id))
            .cloned()
            .collect();
        for a in looked {
            let id = a.id;
            // Added since the start.
            let run = if a.on {
                self.automation_runs.entry(id).or_insert_with(|| {
                    changed = true;
                    Run {
                        next: next_for(&a, now),
                        ..Run::default()
                    }
                })
            } else {
                match self.automation_runs.get_mut(&id) {
                    Some(run) => run,
                    None => continue,
                }
            };
            let due = a.on && run.next.is_some_and(|next| next <= now);
            if due {
                // A sleep runs a missed time once: the next one is after now.
                run.next = next_for(&a, now);
                changed = true;
            }
            // Its own run (a time's), then the runs of the agents that fired it, held.
            let mut keys = vec![None];
            keys.extend(run.slots.iter().filter_map(|(by, slot)| {
                by.as_ref()
                    .filter(|_| slot.held.is_some())
                    .map(|by| Some(by.clone()))
            }));
            for by in keys {
                let own = by.is_none();
                self.look_slot(&a, by.as_deref(), due && own, now, window, cx);
            }
        }
        for (id, by, key) in fires {
            self.fire_event(id, &by, key, now, window, cx);
            changed = true;
        }
        if changed {
            cx.notify();
        }
        let more = self.automations_active();
        self.automating = more;
        more
    }

    /// The look at one run of `a`: typed (a due time joins it), held (given up after HOLD_MOST,
    /// else tried again), or a time that came.
    fn look_slot(
        &mut self,
        a: &Automation,
        by: Option<&str>,
        due: bool,
        now: SystemTime,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let id = a.id;
        let Some(run) = self.automation_runs.get_mut(&id) else {
            return;
        };
        let counts = match run.slots.get_mut(&by.map(str::to_string)) {
            Some(slot) if slot.typing => {
                slot.typing_counts |= due;
                return;
            }
            Some(Slot {
                held: Some(held), ..
            }) => {
                held.counts |= due;
                if now.duration_since(held.since).unwrap_or_default() >= HOLD_MOST {
                    let counts = held.counts;
                    let target = for_firer(a, by);
                    let uid = typed_into(&target).unwrap_or_default();
                    let why = format!("{uid} stayed busy for an hour.");
                    self.run_ended(id, Went::Skipped, why, counts, by, cx);
                    return;
                }
                held.counts
            }
            _ if due => true,
            _ => return,
        };
        self.attempt(id, counts, by, window, cx);
    }

    /// The events of `heard` that fire a rule that is on, in the list's order, and the commands
    /// that ended since the last look: (rule, the block that fired it, the cooldown's key).
    fn events_due(&mut self, heard: &[(String, Heard)]) -> Vec<(u64, String, String)> {
        let mut fires = Vec::new();
        for a in &self.automations.automations {
            if !a.on {
                continue;
            }
            match &a.when {
                When::Agent { event, who } => {
                    let mut ours: Vec<&String> = Vec::new();
                    for (uid, said) in heard {
                        if said == event && !ours.contains(&uid) && self.hears(a, who, uid) {
                            ours.push(uid);
                        }
                    }
                    let Some(last) = ours.last() else {
                        continue;
                    };
                    match who {
                        // The last of the set to finish, once, when the others rest.
                        Who::Blocks { blocks, all: true }
                            if *event == Heard::Idle && blocks.len() > 1 =>
                        {
                            if blocks.iter().all(|b| self.rests(b)) {
                                fires.push((a.id, (*last).clone(), String::new()));
                            }
                        }
                        _ => fires.extend(
                            ours.iter()
                                .map(|uid| (a.id, (*uid).clone(), (*uid).clone())),
                        ),
                    }
                }
                When::Command { terminal, failures } => {
                    // Heard where the rule is: a terminal in another workspace is as missing.
                    let status = (self.find_block(terminal))
                        .filter(|(space, _)| a.workspace.as_ref().is_none_or(|home| home == space))
                        .and_then(|(space, id)| self.space_named(&space)?.terminal(id))
                        .map(|pane| pane.term.status());
                    let run = self.automation_runs.entry(a.id).or_default();
                    let Some(status) = status else {
                        run.finished = None;
                        continue;
                    };
                    let seen = run.finished.replace(status.finished);
                    let failed = status.last.and_then(|o| o.code).is_some_and(|c| c != 0);
                    if seen.is_some_and(|seen| status.finished > seen) && (!failures || failed) {
                        fires.push((a.id, terminal.clone(), terminal.clone()));
                    }
                }
                _ => {}
            }
        }
        fires
    }

    /// The block `uid` is heard by `a`: in its workspace (when it has one), and chosen in `who`
    /// or of its kind.
    fn hears(&self, a: &Automation, who: &Who, uid: &str) -> bool {
        let Some((space, id)) = self.find_block(uid) else {
            return false;
        };
        if a.workspace.as_ref().is_some_and(|home| *home != space) {
            return false;
        }
        match who {
            Who::Blocks { blocks, .. } => blocks.iter().any(|b| b == uid),
            Who::Kind { kind } => (self.space_named(&space))
                .and_then(|space| space.terminal(id))
                .is_some_and(|pane| self.runs(pane.seen.agent.as_deref(), kind)),
        }
    }

    /// The block `uid` doesn't hold an All of them back: no agent there, idle, or not open.
    fn rests(&self, uid: &str) -> bool {
        let pane =
            (self.find_block(uid)).and_then(|(space, id)| self.space_named(&space)?.terminal(id));
        pane.is_none_or(|pane| {
            pane.agent().is_none() || matches!(self.doing(pane), Doing::Idle | Doing::Starting)
        })
    }

    /// An event of the rule `id`, fired by the block `by`: outside its cooldown it starts a run
    /// (or joins the one held or typed for that block).
    pub(super) fn fire_event(
        &mut self,
        id: u64,
        by: &str,
        key: String,
        now: SystemTime,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(a) = self.automations.get(id).filter(|a| a.on) else {
            return;
        };
        let (minutes, runs, most) = (a.cooldown, a.runs, a.max_runs);
        let run = self.automation_runs.entry(id).or_default();
        let cooldown = Duration::from_secs(60 * u64::from(minutes));
        if (run.cooled.get(&key))
            .is_some_and(|at| now.duration_since(*at).unwrap_or_default() < cooldown)
        {
            // Inside the cooldown: nothing, not even written.
            return;
        }
        let slot = run.slot(Some(by));
        if slot.typing {
            slot.typing_counts = true;
        } else if let Some(held) = &mut slot.held {
            held.counts = true;
        } else {
            // The runs being typed count too: Stop after is not passed by a crowd. (A held one
            // may be dropped by it, as it is when the rule turns off.)
            let coming = run
                .slots
                .values()
                .filter(|slot| slot.typing && slot.typing_counts);
            if most.is_some_and(|most| runs + coming.count() as u32 >= most) {
                return;
            }
            run.cooled.insert(key, now);
            self.attempt(id, true, Some(by), window, cx);
        }
    }

    /// Tries the Do of `id` once: it ends (Ran, Failed, Skipped), is held, or waits for an answer.
    /// `counts`: a due run, which counts toward Stop after; a Run now doesn't. `by`: the block
    /// whose event made it due.
    fn attempt(
        &mut self,
        id: u64,
        counts: bool,
        by: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(saved) = self.automations.get(id) else {
            return;
        };
        let a = for_firer(saved, by);
        // The rule as saved (its When's blocks, the agent that fired it), then as it runs.
        let target = self
            .automation_target(saved)
            .and_then(|()| self.automation_target(&a));
        if let Err(why) = target {
            self.run_ended(id, Went::Skipped, why, counts, by, cx);
            return;
        }
        match self.try_do(&a, by, window, cx) {
            Tried::Went(went, about) => self.run_ended(id, went, about, counts, by, cx),
            Tried::Hold(why) => {
                let slot = self.automation_runs.entry(id).or_default().slot(by);
                match &mut slot.held {
                    Some(held) if held.why == why => {}
                    Some(held) => {
                        held.why = why;
                        cx.notify();
                    }
                    None => {
                        let (since, counts) = (SystemTime::now(), counts);
                        slot.held = Some(Held { since, why, counts });
                        cx.notify();
                    }
                }
            }
            Tried::Later(task) => {
                let slot = self.automation_runs.entry(id).or_default().slot(by);
                (slot.typing, slot.typing_counts, slot.held) = (true, counts, None);
                let since = a.since;
                let key = by.map(str::to_string);
                cx.spawn(async move |ws, cx| {
                    let answer = task.await;
                    ws.update(cx, |ws, cx| {
                        let by = key.as_deref();
                        // A time that fell due while it was typed counted into it.
                        let slot = (ws.automation_runs.get_mut(&id))
                            .and_then(|run| run.slots.get_mut(&key));
                        let counts = slot.map_or(counts, |slot| {
                            slot.typing = false;
                            slot.typing_counts
                        });
                        let (went, about) = match answer {
                            Ok(about) => (Went::Ran, about),
                            Err(why) => (Went::Failed, why),
                        };
                        // Saved or turned off meanwhile: the new rule starts counting from its
                        // save; the answer is only the last thing that happened.
                        let same = ws
                            .automations
                            .get(id)
                            .is_some_and(|a| a.on && a.since == since);
                        if same {
                            ws.run_ended(id, went, about, counts, by, cx);
                        } else {
                            ws.run_ended(id, went, about, false, by, cx);
                            // A time or an event of the new rule fell due while the old task was
                            // typed: it runs at the next look, and a Once then turns off.
                            if counts && ws.automations.get(id).is_some_and(|a| a.on) {
                                match by {
                                    None => {
                                        if let Some(run) = ws.automation_runs.get_mut(&id) {
                                            run.next = Some(SystemTime::now());
                                        }
                                    }
                                    Some(by) => {
                                        let run = ws.automation_runs.entry(id).or_default();
                                        run.slot(Some(by)).held = Some(Held {
                                            since: SystemTime::now(),
                                            why: String::new(),
                                            counts: true,
                                        });
                                    }
                                }
                                ws.look_at_automations(cx);
                            }
                        }
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// One try of the Do of `a`, already made `for_firer(by)`.
    fn try_do(
        &mut self,
        a: &Automation,
        by: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Tried {
        let a_id = a.id;
        match &a.act {
            Do::Notify { text } => {
                let title = match a.name.as_str() {
                    "" => self.automation_sentence(a, cx),
                    name => name.to_string(),
                };
                let ws = cx.entity().downgrade();
                let fired = by.map(str::to_string);
                self.api_notice(templum_api::model::Notice {
                    text: format!("{title}: {text}"),
                    workspace: a.workspace.clone(),
                    block: None,
                    automation: Some(a_id),
                });
                let notification = Notification::new()
                    .with_type(NotificationType::Info)
                    .title(title)
                    .message(text.clone())
                    .delivery(self.delivery(window))
                    .on_click(move |_, window, cx| {
                        ws.update(cx, |ws, cx| ws.open_fired(a_id, fired.clone(), window, cx))
                            .ok();
                    });
                show_notification(window, cx, notification);
                Tried::Went(Went::Ran, "Notified.".into())
            }
            Do::Prompt { block, text } => self.try_prompt(a, block, text, cx),
            Do::Command {
                terminal: Some(uid),
                text,
                ..
            } => match self.find_block(uid) {
                Some(at) => match self.command_busy_at(&at) {
                    Some(why) => Tried::Hold(format!("Waiting for {uid}: {why}")),
                    None => self.type_command(a, at, uid, text, cx),
                },
                None => Tried::Went(Went::Skipped, format!("Target missing: {uid} isn't open")),
            },
            Do::Command { folder, text, .. } => {
                let place = self.place_beside(a, by);
                self.command_in_new_terminal(a, folder.as_deref(), text, place, window, cx)
            }
            Do::Agent {
                agent,
                folder,
                model,
                effort,
                prompt,
            } => {
                let place = self.place_beside(a, by);
                self.start_agent_for(
                    a,
                    (agent, folder.as_deref()),
                    (model, effort, prompt),
                    place,
                    window,
                    cx,
                )
            }
        }
    }

    /// Where a new block of `a` opens: beside the block that fired it when that one is in the
    /// rule's workspace, else at the middle of the view.
    fn place_beside(&self, a: &Automation, by: Option<&str>) -> Place {
        let near = (by.and_then(|uid| self.find_block(uid)))
            .filter(|(space, _)| a.workspace.as_ref() == Some(space));
        near.map_or(Place::Free, |(_, id)| Place::Near(id))
    }

    /// A notification's click: the block that fired the run is shown, else (a time, Run now, a
    /// block closed since) the popup opens on the automation.
    pub(crate) fn open_fired(
        &mut self,
        id: u64,
        by: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match by.as_deref().and_then(|uid| self.find_block(uid)) {
            Some((space, block)) => self.reveal(&space, block, window, cx),
            None => self.show_automation(id, window, cx),
        }
    }

    /// Why a command can't be typed into the terminal in `pane` now; None: its shell is at its
    /// prompt.
    fn command_busy(&self, pane: &Pane) -> Option<&'static str> {
        let processes = pane.term.processes();
        let in_front = processes
            .foreground
            .is_some_and(|front| Some(front) != processes.shell);
        let recent = |at: Option<Instant>, within| at.is_some_and(|at| at.elapsed() < within);
        if pane.seen.agent.is_some() && !pane.shell_in_front() {
            Some("an agent runs in it")
        } else if pane.term.status().running.is_some() || in_front {
            Some("a command runs in it")
        } else if recent(pane.keyed.get(), TYPING) || pane.has_draft() {
            Some("you are typing in it")
        } else {
            None
        }
    }

    /// `command_busy` of the terminal at `(workspace id, block)`; a block that isn't one is free,
    /// and fails when typed into.
    fn command_busy_at(&self, (space, id): &(String, BlockId)) -> Option<&'static str> {
        let pane = self.space_named(space)?.terminal(*id)?;
        self.command_busy(pane)
    }

    /// Types the command `text` and Enter into the terminal `uid` at `at`.
    fn type_command(
        &mut self,
        a: &Automation,
        (space, id): (String, BlockId),
        uid: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Tried {
        match self.type_text(&asker(a.id), (&space, id), uid, text, true, cx) {
            Outcome::Later(answer) => {
                let uid = uid.to_string();
                Tried::Later(cx.spawn(async move |_, _| {
                    let reply = answer.await;
                    match reply.is_error {
                        true => Err(reply.text),
                        false => Ok(format!("Typed into {uid}.")),
                    }
                }))
            }
            Outcome::Now(reply) if reply.is_error => Tried::Went(Went::Failed, reply.text),
            Outcome::Now(reply) => Tried::Went(Went::Ran, reply.text),
            Outcome::Wait(..) => Tried::Went(Went::Failed, "It waited for an answer.".into()),
        }
    }

    /// The folder a new block opens in: the rule's (`~` expanded), else its workspace's, else the
    /// home folder.
    fn automation_folder(&self, a: &Automation, typed: Option<&str>) -> PathBuf {
        if let Some(typed) = typed.map(str::trim).filter(|typed| !typed.is_empty()) {
            return launcher::expand(typed);
        }
        let space = a.workspace.as_deref().and_then(|id| self.space_named(id));
        space
            .and_then(|space| space.folder.clone())
            .or_else(std::env::home_dir)
            .unwrap_or_default()
    }

    /// The folder a new block of `a` would open in, as the Folder box's placeholder says it.
    pub(crate) fn automation_folder_shown(&self, a: &Automation) -> String {
        // A home rule opens in home's workspace folder: a path on another computer.
        if let Some((space, _)) = key_home(a.id) {
            return self.home_folder(&space);
        }
        self.automation_folder(a, None).display().to_string()
    }

    /// Run a command in a new terminal, or the one the last run kept while it is at its prompt.
    fn command_in_new_terminal(
        &mut self,
        a: &Automation,
        folder: Option<&str>,
        text: &str,
        place: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Tried {
        let Some(space) = a.workspace.clone() else {
            return Tried::Went(Went::Failed, "Choose the workspace it opens in.".into());
        };
        let path = self.automation_folder(a, folder);
        let shown = shown_folder(folder, &path);
        if !path.is_dir() {
            return Tried::Went(Went::Failed, format!("No folder {shown}."));
        }
        // The terminal it kept, while it is open there and free.
        let kept = a.opened.as_deref().and_then(|uid| {
            let (home, id) = self.find_block(uid)?;
            let pane = self.space_named(&home)?.terminal(id)?;
            (home == space && pane.seen.agent.is_none()).then(|| (uid.to_string(), home, id))
        });
        if let Some((uid, home, id)) = kept
            && self.command_busy_at(&(home.clone(), id)).is_none()
        {
            return self.type_command(a, (home, id), &uid, text, cx);
        }
        let new = NewBlock::Terminal {
            folder: path,
            command: Some(text.to_string()),
            shell: None,
        };
        match self.add_block_in(&space, new, place, None, window, cx) {
            Ok(id) => {
                let uid = self
                    .space_named(&space)
                    .and_then(|s| s.uids.get(&id).cloned())
                    .unwrap_or_default();
                if let Some(a) = self.automations.get_mut(a.id) {
                    a.opened = Some(uid.clone());
                }
                Tried::Went(Went::Ran, format!("Opened {uid} in {shown}."))
            }
            Err(why) => Tried::Went(Went::Failed, why),
        }
    }

    /// Start an agent: opened as `spawn_agent` opens one, in the rule's workspace, wired to
    /// nothing, with its first prompt a task from the automation.
    fn start_agent_for(
        &mut self,
        a: &Automation,
        (agent, folder): (&str, Option<&str>),
        (model, effort, prompt): (&Option<String>, &Option<String>, &Option<String>),
        place: Place,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Tried {
        let Some(entry) = self.agents.iter().find(|e| e.id == agent).cloned() else {
            let why = format!("Target missing: {agent} isn't in Settings ▸ Agents");
            return Tried::Went(Went::Skipped, why);
        };
        let Some(space) = a.workspace.clone() else {
            return Tried::Went(Went::Failed, "Choose the workspace it opens in.".into());
        };
        let (model, effort) = (model.as_deref(), effort.as_deref());
        if let Err(why) = entry.launch_args_with(model, effort, None) {
            return Tried::Went(Went::Failed, why);
        }
        if let Some(problem) = self.spawn_problem(&entry) {
            return Tried::Went(Went::Failed, problem);
        }
        let path = self.automation_folder(a, folder);
        if !path.is_dir() {
            let shown = shown_folder(folder, &path);
            return Tried::Went(Went::Failed, format!("No folder {shown}."));
        }
        let spawn = tools::Spawn {
            agent: Some(agent.to_string()),
            prompt: prompt.clone().filter(|p| !p.trim().is_empty()),
            folder: Some(path.display().to_string()),
            worktree: None,
            model: model.map(str::to_string),
            effort: effort.map(str::to_string),
            fork: false,
            role: None,
            args: None,
        };
        let called = match a.name.as_str() {
            "" => self.when_said(a, cx),
            name => name.to_string(),
        };
        let opener = Opener::Automation {
            asker: asker(a.id),
            by: format!("automation \"{called}\""),
        };
        let (reply, _) = self.open_agent_in(
            &space,
            &entry,
            None,
            &path,
            &spawn,
            place,
            &[],
            opener,
            window,
            cx,
        );
        match reply.is_error {
            true => Tried::Went(Went::Failed, reply.text),
            // The folder as the rule has it: "in ~/api", not the path it expands to.
            false => {
                let in_path = format!(" in {}", path.display());
                let as_typed = format!(" in {}", shown_folder(folder, &path));
                Tried::Went(Went::Ran, reply.text.replacen(&in_path, &as_typed, 1))
            }
        }
    }

    /// Why the agent in `pane` can't take a prompt now; None: it can.
    fn prompt_busy(&self, pane: &Pane) -> Option<&'static str> {
        let doing = self.doing(pane);
        let recent = |at: Option<Instant>, within| at.is_some_and(|at| at.elapsed() < within);
        if pane.agent_starting() || pane.prompt.is_some() || doing == Doing::Starting {
            Some("it is starting")
        } else if matches!(doing, Doing::NeedsYou(_)) {
            Some("it needs you")
        } else if doing.works() {
            Some("it is working")
        } else if pane.task(Instant::now()).is_some() {
            Some("it is on a task")
        } else if pane.has_draft() || recent(pane.keyed.get(), TYPING) {
            Some("you are typing in it")
        } else if recent(pane.sent, NOTICE_GAP) {
            Some("Templum is typing into it")
        } else {
            None
        }
    }

    /// Send a prompt: handed to the agent in `block` as `delegate` hands a task, from the
    /// automation; held while the agent is busy; never typed into a shell.
    fn try_prompt(
        &mut self,
        a: &Automation,
        block: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Tried {
        let Some((space_id, id)) = self.find_block(block) else {
            return Tried::Went(Went::Skipped, format!("Target missing: {block} isn't open"));
        };
        let Some(pane) = self.space_named(&space_id).and_then(|s| s.terminal(id)) else {
            return Tried::Went(Went::Failed, format!("{block} isn't a terminal."));
        };
        if pane.seen.agent.is_none() || pane.shell_in_front() {
            let why = format!("{block} runs no agent now: a prompt is never typed into a shell.");
            return Tried::Went(Went::Failed, why);
        }
        if let Some(why) = self.prompt_busy(pane) {
            return Tried::Hold(format!("Waiting for {block}: {why}"));
        }
        let called = match a.name.as_str() {
            "" => self.when_said(a, cx),
            name => name.to_string(),
        };
        let by = format!("automation \"{called}\"");
        let outcome = self.hand_task(
            &asker(a.id),
            &by,
            (&space_id, id),
            block,
            text,
            false,
            tools::TASK_WAIT,
            None,
            cx,
        );
        let task = self
            .space_named(&space_id)
            .and_then(|s| s.terminal(id))
            .and_then(|pane| pane.handed.as_ref())
            .map_or(0, |handed| handed.id);
        match outcome {
            Outcome::Later(answer) => {
                let block = block.to_string();
                Tried::Later(cx.spawn(async move |_, _| {
                    let reply = answer.await;
                    match reply.is_error {
                        true => Err(reply.text),
                        false => Ok(format!("Typed into {block} as task t{task}.")),
                    }
                }))
            }
            Outcome::Now(reply) if reply.is_error => Tried::Went(Went::Failed, reply.text),
            Outcome::Now(reply) => Tried::Went(Went::Ran, reply.text),
            Outcome::Wait(..) => Tried::Went(Went::Failed, "It waited for an answer.".into()),
        }
    }

    /// Its run ended (the one `by` fired, None a time's): `last`, then for a due run (`counts`)
    /// its count, Stop after, and a Once turned off.
    fn run_ended(
        &mut self,
        id: u64,
        went: Went,
        about: String,
        counts: bool,
        by: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        let Some(a) = self.automations.get_mut(id) else {
            return;
        };
        let run = self.automation_runs.entry(id).or_default();
        let key = by.map(str::to_string);
        if let Some(slot) = run.slots.get_mut(&key) {
            slot.held = None;
            if !slot.typing {
                run.slots.remove(&key);
            }
        }
        run.unseen |= went != Went::Ran;
        let mut about = about;
        if counts {
            if went == Went::Ran {
                a.runs += 1;
                if let Some(most) = a.max_runs.filter(|most| a.runs >= *most) {
                    a.on = false;
                    about.push_str(&format!(" Ran {most} of {most} times, now off."));
                }
            }
            if matches!(a.when, When::Once { .. }) {
                a.on = false;
            }
            if !a.on {
                run.next = None;
                // Off: no agent's held run goes on.
                run.slots.retain(|_, slot| slot.typing);
            }
        }
        a.last = Some(LastRun {
            at: unix(SystemTime::now()),
            went,
            about,
        });
        cx.notify();
    }

    /// Run now: its Do goes now, also when it is off, through the checks any run has; it doesn't
    /// move its next time and doesn't count toward Stop after.
    pub(crate) fn run_automation_now(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Home's rule runs at home, never here.
        if key_home(id).is_some() {
            return self.run_home_rule(id, window, cx);
        }
        let trying = (self.automation_runs.get(&id))
            .and_then(|run| run.slots.get(&None))
            .is_some_and(|slot| slot.typing || slot.held.is_some());
        // The agent that fired it is no one's, here.
        let fired = matches!(
            self.automations.get(id).map(|a| &a.act),
            Some(Do::Prompt { block, .. }) if block == FIRED
        );
        if self.automations.get(id).is_some() && !trying && !fired {
            self.attempt(id, false, None, window, cx);
            // Held, it is tried again at each look.
            self.look_at_automations(cx);
        }
    }

    /// The switch of a row: turning it on counts from now; off drops its next time and its held
    /// run.
    pub(crate) fn set_automation_on(&mut self, id: u64, on: bool, cx: &mut Context<Self>) {
        // Home's rule is switched at home, never here.
        if key_home(id).is_some() {
            return self.set_home_rule(id, on, cx);
        }
        let now = SystemTime::now();
        let Some(a) = self.automations.get_mut(id) else {
            return;
        };
        a.on = on;
        if on {
            (a.since, a.runs) = (unix(now), 0);
        }
        let next = on.then(|| next_for(a, now)).flatten();
        let run = self.automation_runs.entry(id).or_default();
        run.next = next;
        // A held run is dropped; a time that fell due while an older task was typed was the old
        // rule's.
        run.reset();
        self.look_at_automations(cx);
        cx.notify();
    }

    /// Why the Do of `a` can't act: its workspace or its target is gone or stopped. Err: "Target
    /// missing: abc12 isn't open".
    pub(crate) fn automation_target(&self, a: &Automation) -> Result<(), String> {
        let missing = |why: String| Err(format!("Target missing: {why}"));
        // A home rule is home's to run: its target is checked there.
        if key_home(a.id).is_some() {
            return Ok(());
        }
        // The sweep that turns off a revoked token's rules runs every few seconds: until then.
        if self.api_token_is_gone(a) {
            return missing("its token was revoked".into());
        }
        if let Some(id) = &a.workspace {
            match self.spaces().find(|s| s.id == *id) {
                None => return missing("a deleted workspace".into()),
                // Home's automations run at home: a stale rule for a mirror never acts here.
                Some(space) if space.is_remote() => {
                    return missing(space.automations_there().unwrap_or_default());
                }
                Some(space) if space.stopped => {
                    return missing(format!("{} is stopped", space.name));
                }
                Some(_) => {}
            }
        }
        // What it hears must be open, where it is heard.
        match &a.when {
            When::Agent {
                who: Who::Blocks { blocks, .. },
                ..
            } => {
                for block in blocks {
                    self.event_block(a, block)?;
                }
            }
            When::Agent {
                who: Who::Kind { kind },
                ..
            } if !self.agents.iter().any(|e| e.id == *kind) => {
                return missing(format!("{kind} isn't in Settings ▸ Agents"));
            }
            When::Command { terminal, .. } => self.event_block(a, terminal)?,
            _ => {}
        }
        match &a.act {
            Do::Prompt { block, .. } if block == FIRED => match a.when {
                When::Agent { .. } => Ok(()),
                _ => missing("only an agent event has an agent that fired it".into()),
            },
            Do::Prompt { block, .. } if self.find_block(block).is_none() => {
                missing(format!("{block} isn't open"))
            }
            Do::Command {
                terminal: Some(block),
                ..
            } if self.find_block(block).is_none() => missing(format!("{block} isn't open")),
            Do::Agent { agent, .. } if !self.agents.iter().any(|e| e.id == *agent) => {
                missing(format!("{agent} isn't in Settings ▸ Agents"))
            }
            // What a token limited to some workspaces saved opens only inside their folders.
            Do::Agent { folder, .. }
            | Do::Command {
                terminal: None,
                folder,
                ..
            } => match self.api_folder_problem(a, &self.automation_folder(a, folder.as_deref())) {
                Some(why) => missing(why),
                None => Ok(()),
            },
            _ => Ok(()),
        }
    }

    /// Whether the block `uid`, which an event of `a` hears, is open in its workspace. Err: "Target
    /// missing: abc12 isn't open", "…: abc12 is in api".
    fn event_block(&self, a: &Automation, uid: &str) -> Result<(), String> {
        let Some((space, _)) = self.find_block(uid) else {
            return Err(format!("Target missing: {uid} isn't open"));
        };
        match (&a.workspace, self.space_named(&space)) {
            (Some(home), Some(found)) if *home != space => {
                Err(format!("Target missing: {uid} is in {}", found.name))
            }
            _ => Ok(()),
        }
    }

    /// The When's words, the blocks as "Title (uid)", a kind by its title, a kind of a
    /// one-workspace rule with its workspace: "When Claude Code (abc12) finishes its turn".
    pub(crate) fn when_said(&self, a: &Automation, cx: &App) -> String {
        // A home rule names its blocks, agents and workspace as the remote does.
        let home = key_home(a.id).and_then(|(remote, _)| self.space_named(&remote));
        let block = |uid: &str| match self.uid_title_of(home, uid, cx) {
            Some(title) => format!("{title} ({uid})"),
            None => uid.to_string(),
        };
        let space = match home {
            Some(remote) => Some(remote),
            None => a.workspace.as_deref().and_then(|id| self.space_named(id)),
        };
        let said = event_words(
            &a.when,
            &block,
            &|kind| match home {
                Some(remote) => self.home_kind_title(&remote.id, kind),
                None => self.agent_title(kind),
            },
            space.map(|space| space.name.as_str()),
        );
        said.unwrap_or_else(|| when_words(&a.when, Local::now().date_naive()))
    }

    /// The preview sentence: "Every weekday at 09:00, type "review open PRs" into Claude Code
    /// (abc12)."
    pub(crate) fn automation_sentence(&self, a: &Automation, cx: &App) -> String {
        let home = key_home(a.id).and_then(|(remote, _)| self.space_named(&remote));
        // A saved home rule that is not being edited says what home says of it.
        if let Some((_, rule)) = self.home_rule(a.id)
            && !self.automation_drafts.contains_key(&a.id)
            && !rule.sentence.is_empty()
        {
            return rule.sentence.clone();
        }
        let block = |uid: &str| match self.uid_title_of(home, uid, cx) {
            Some(title) => format!("{title} ({uid})"),
            None => uid.to_string(),
        };
        let agent_title = |agent: &str| match home {
            Some(remote) => self.home_kind_title(&remote.id, agent),
            None => self.agent_title(agent),
        };
        let does = match &a.act {
            Do::Prompt { block: uid, text } if uid == FIRED => {
                format!("type {} into the agent that fired it", quote(text))
            }
            Do::Prompt { block: uid, text } => format!("type {} into {}", quote(text), block(uid)),
            Do::Command {
                terminal: Some(uid),
                text,
                ..
            } => format!("run {} in {}", quote(text), block(uid)),
            Do::Command { folder, text, .. } => {
                let folder = folder.as_deref().filter(|f| !f.is_empty());
                let folder = folder.map_or(String::new(), |f| format!(" in {f}"));
                format!("run {} in a new terminal{folder}", quote(text))
            }
            Do::Agent {
                agent,
                folder,
                prompt,
                ..
            } => {
                let folder = folder.as_deref().filter(|f| !f.is_empty());
                let folder = folder.map_or(String::new(), |f| format!(" in {f}"));
                let prompt = prompt.as_deref().filter(|p| !p.trim().is_empty());
                let prompt = prompt.map_or(String::new(), |p| format!(" with {}", quote(p)));
                format!("start {}{folder}{prompt}", agent_title(agent))
            }
            Do::Notify { text } => format!("notify you: {}", quote(text)),
        };
        format!("{}, {does}.", self.when_said(a, cx))
    }

    /// What could make the rule `a` run again and again, in the editor's words; None: nothing
    /// does. An agent's prompt that ends a turn of an agent the event hears, a started agent the
    /// event hears, a command that ends in the terminal it runs in.
    pub(crate) fn automation_loop(&self, a: &Automation) -> Option<String> {
        // A home rule's kinds and agents are home's: only what its words alone say.
        let home = key_home(a.id).is_some();
        let cooldown = "The cooldown spaces the runs out.";
        match (&a.when, &a.act) {
            (
                When::Command { terminal, .. },
                Do::Command {
                    terminal: Some(runs_in),
                    ..
                },
            ) if terminal == runs_in => Some(format!(
                "This can loop: its command ends in {terminal}, which fires it again."
            )),
            (
                When::Agent {
                    event: Heard::Idle, ..
                },
                Do::Prompt { block, .. },
            ) if block == FIRED => Some(format!(
                "This can loop: its prompt ends a turn of the agent that fired it, which fires it again. {cooldown}"
            )),
            (
                When::Agent {
                    event: Heard::Idle,
                    who,
                },
                Do::Prompt { block, .. },
            ) => {
                let hears = match who {
                    Who::Blocks { blocks, .. } => blocks.contains(block),
                    Who::Kind { .. } if home => false,
                    Who::Kind { kind } => (self.find_block(block))
                        .and_then(|(space, id)| self.space_named(&space)?.terminal(id))
                        .is_some_and(|pane| self.runs(pane.seen.agent.as_deref(), kind)),
                };
                hears.then(|| {
                    format!(
                        "This can loop: its prompt ends a turn of {block}, which fires it again. {cooldown}"
                    )
                })
            }
            (
                When::Agent {
                    who: Who::Kind { kind },
                    ..
                },
                Do::Agent { agent, .. },
            ) if !home && self.runs(Some(agent), kind) => Some(
                "This can loop: the agent it starts can fire it again, and each new agent has its own cooldown. Stop after bounds it."
                    .into(),
            ),
            _ => None,
        }
    }

    /// How many automations ended other than Ran since the popup was opened.
    pub(crate) fn automations_unseen(&self) -> usize {
        self.automation_runs
            .values()
            .filter(|run| run.unseen)
            .count()
    }

    /// A row's right side, line 1: "17:00", "Tomorrow 09:00", "Mon 09:00", "3 Nov 09:00", "Off",
    /// "Never".
    pub(crate) fn next_words(&self, id: u64, now: SystemTime) -> String {
        if key_home(id).is_some() {
            return self.home_next_words(id, now);
        }
        let Some(a) = self.automations.get(id).filter(|a| a.on) else {
            return "Off".into();
        };
        if let Some(event) = event_short(&a.when, &|kind| self.agent_title(kind)) {
            return event;
        }
        match self.automation_runs.get(&a.id).and_then(|run| run.next) {
            Some(next) => {
                let (next, now) = (DateTime::<Local>::from(next), DateTime::<Local>::from(now));
                time_words(next.naive_local(), now.naive_local())
            }
            None => "Never".into(),
        }
    }
}

/// The runs of the automations, by id.
pub(crate) type Runs = HashMap<u64, Run>;

/// The draft key of a new automation, not saved yet.
pub(crate) const NEW: u64 = 0;

/// What the editor changes of an automation: a draft equal to the saved one in these is no draft.
fn edits(a: &Automation) -> (&str, &Option<String>, &When, &Do, Option<u32>, u32) {
    (
        &a.name,
        &a.workspace,
        &a.when,
        &a.act,
        a.max_runs,
        a.cooldown,
    )
}

impl Workspace {
    /// The ids the list shows: the automations in their order, then the one being written.
    pub(crate) fn automation_ids(&self) -> Vec<u64> {
        let saved = self.automations.automations.iter().map(|a| a.id);
        let new = self.automation_drafts.contains_key(&NEW).then_some(NEW);
        saved.chain(new).chain(self.home_keys()).collect()
    }

    /// The automation the editor shows: the one chosen if it is listed, else the first.
    pub(crate) fn automation_selected(&self) -> Option<u64> {
        let ids = self.automation_ids();
        self.automations_at
            .filter(|at| ids.contains(at))
            .or(ids.first().copied())
    }

    /// Opens the popup with the keyboard on the list; the other popups close and the badge clears.
    pub(crate) fn open_automations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.automations_open = true;
        self.git_open = false;
        self.inbox_open = false;
        self.agents_open = false;
        self.see_home_runs();
        for run in self.automation_runs.values_mut() {
            run.unseen = false;
        }
        self.automations_focus.focus(window, cx);
        cx.notify();
    }

    /// Closes the popup; the keyboard goes back to the canvas only if the popup had it.
    pub(crate) fn close_automations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.automations_open = false;
        if self.automations_focus.contains_focused(window, cx) {
            window.focus(&self.focus_handle, cx);
        }
        cx.notify();
    }

    /// The sidebar's button, and the shortcut.
    pub(crate) fn toggle_automations(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.automations_open {
            self.close_automations(window, cx);
        } else {
            self.open_automations(window, cx);
        }
    }

    /// Opens the popup on the automation `id`, if it is still there (a notification's click).
    pub(crate) fn show_automation(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.open_automations(window, cx);
        self.select_automation(id, window, cx);
    }

    /// Selects `id` and gives the popup the keyboard on its list.
    pub(crate) fn select_automation(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.automation_ids().contains(&id) {
            self.automations_at = Some(id);
        }
        self.automations_focus.focus(window, cx);
        cx.notify();
    }

    /// ↑ / ↓: the previous or next automation is selected (none past the ends).
    fn automations_step(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let ids = self.automation_ids();
        let Some(at) = self
            .automation_selected()
            .and_then(|id| ids.iter().position(|i| *i == id))
        else {
            return;
        };
        if let Some(next) = at.checked_add_signed(by).and_then(|i| ids.get(i)) {
            self.select_automation(*next, window, cx);
        }
    }

    /// The popup's keys, run before its fields get them: ↑ ↓ select while the list has the
    /// keyboard, Esc in a field gives it back to the list and on the list closes the popup; the
    /// workspace's shortcuts go on, but those that act on the terminal in front.
    pub(crate) fn on_automations_key(
        &mut self,
        e: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let k = &e.keystroke;
        let command = super::effective(
            self.keymap.command(k),
            false,
            self.space.grid.is_some(),
            false,
        );
        // The theme picker takes no focus: while it is open its keys are its own.
        if self.picker.is_some() {
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
            return;
        }
        let on_list = self.automations_focus.is_focused(window);
        let handled = !k.modifiers.modified()
            && match k.key.as_str() {
                "up" | "down" if on_list => {
                    self.automations_step(if k.key == "up" { -1 } else { 1 }, window, cx);
                    true
                }
                "escape" if on_list => {
                    self.close_automations(window, cx);
                    true
                }
                "escape" => {
                    self.automations_focus.focus(window, cx);
                    true
                }
                _ => false,
            };
        if handled {
            cx.stop_propagation();
            return;
        }
        let command = command.filter(|c| {
            !matches!(
                c,
                Command::Copy
                    | Command::Paste
                    | Command::CloseTerminal
                    | Command::PreviousPrompt
                    | Command::NextPrompt
                    | Command::Find
            )
        });
        if command.is_some() {
            self.command_key(e, command, window, cx);
            cx.stop_propagation();
        }
    }

    /// Why the run of `id` is held, in full: "Waiting for abc12: it is working".
    pub(crate) fn automation_held_why(&self, id: u64) -> Option<String> {
        let run = self.automation_runs.get(&id)?;
        run.held().map(|(_, held)| held.why.clone())
    }

    /// The editor's last line: while a run is held, why it waits and nothing else; else the last
    /// run in full: "Failed Yesterday 16:00: abc12 runs no agent now…".
    pub(crate) fn automation_last_line(&self, id: u64, now: SystemTime) -> Option<String> {
        if let Some(why) = self.automation_held_why(id) {
            return Some(why);
        }
        let words = self.last_words(id, now)?;
        let about = self.rule(id)?.last.as_ref()?.about.clone();
        Some(match about.is_empty() {
            true => words,
            false => format!("{words}: {about}"),
        })
    }

    /// Line 2's right side: "Ran 16:00", "Failed Yesterday 16:00", "Missed 24 Oct 17:00",
    /// "Waiting for abc12"; None before the first run.
    pub(crate) fn last_words(&self, id: u64, now: SystemTime) -> Option<String> {
        let a = self.rule(id)?;
        if let Some((by, _)) = self.automation_runs.get(&id).and_then(|run| run.held()) {
            let target = for_firer(a, by);
            return Some(match typed_into(&target) {
                Some(uid) => format!("Waiting for {uid}"),
                None => "Waiting".into(),
            });
        }
        let last = a.last.as_ref()?;
        let went = match last.went {
            Went::Ran => "Ran",
            Went::Skipped => "Skipped",
            Went::Failed => "Failed",
            Went::Missed => "Missed",
        };
        let (at, now) = (
            local(last.at).naive_local(),
            DateTime::<Local>::from(now).naive_local(),
        );
        Some(format!("{went} {}", time_words(at, now)))
    }

    /// The schedules agents asked for, oldest first: (id, "s3 from Claude Code · api (b7k2m) to
    /// k9x1d", "every 10 min · next 15:10 · <first line>").
    pub(crate) fn schedule_rows(&self) -> Vec<(u64, String, String)> {
        let clock = |t: SystemTime| DateTime::<Local>::from(t).format("%H:%M").to_string();
        self.schedules
            .iter()
            .map(|s| {
                let from = match self.pane(&s.by) {
                    Some(pane) => {
                        let title = pane.term.title();
                        let uid = self.caller(&s.by).and_then(|(space, id)| {
                            self.space_named(&space)?.uids.get(&id).cloned()
                        });
                        let name = match self.agent_name(pane) {
                            Some(agent) if !title.is_empty() => format!("{agent} · {title}"),
                            Some(agent) => agent.to_string(),
                            None => title,
                        };
                        let uid = uid.unwrap_or_default();
                        let to = if uid == s.to { "itself" } else { s.to.as_str() };
                        format!("{name} ({uid}) to {to}")
                    }
                    None => format!("Templum Agent to {}", s.to),
                };
                let line = s.text.lines().next().unwrap_or_default();
                let when = match s.every {
                    Some(every) => format!(
                        "every {} min · next {}",
                        every.as_secs() / 60,
                        clock(s.next)
                    ),
                    None => format!("once · {}", clock(s.next)),
                };
                (
                    s.id,
                    format!("s{} from {from}", s.id),
                    format!("{when} · {line}"),
                )
            })
            .collect()
    }

    /// Duplicate: an off copy after the last, selected.
    pub(crate) fn duplicate_automation(&mut self, id: u64, cx: &mut Context<Self>) -> Option<u64> {
        // Home saves its copy, and its answer selects it.
        if key_home(id).is_some() {
            self.duplicate_home_rule(id, cx);
            return None;
        }
        let copy = self.automations.duplicate(id)?;
        self.automations_at = Some(copy);
        cx.notify();
        Some(copy)
    }

    /// Delete asks first.
    pub(crate) fn ask_delete_automation(
        &mut self,
        id: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(a) = self.automation_draft(id).cloned() else {
            return;
        };
        // Home's rule is deleted at home, which may refuse: asked before the question.
        if key_home(id).is_some()
            && let Some(why) = self.home_delete_refusal(id)
        {
            return self.warn(why, window, cx);
        }
        let name = match a.name.as_str() {
            "" => self.automation_sentence(&a, cx),
            name => name.to_string(),
        };
        let entity = cx.entity();
        crate::prompt::confirm(
            format!("Delete \"{name}\"?"),
            "It won't run again. This can't be undone.",
            "Delete",
            move |_, cx| {
                entity.update(cx, |ws, cx| ws.delete_automation(id, cx));
            },
            window,
            cx,
        );
    }

    /// Removes the automation, its run and its draft.
    pub(crate) fn delete_automation(&mut self, id: u64, cx: &mut Context<Self>) {
        if key_home(id).is_some() {
            return self.delete_home_rule(id, cx);
        }
        let ids = self.automation_ids();
        let at = ids.iter().position(|i| *i == id);
        self.automations.remove(id);
        self.automation_runs.remove(&id);
        self.api_unconfine(id);
        self.automation_drafts.remove(&id);
        if self.automations_at == Some(id) {
            let left = self.automation_ids();
            self.automations_at =
                at.and_then(|at| left.get(at.min(left.len().saturating_sub(1))).copied());
        }
        cx.notify();
    }

    /// New: a draft at the list's end, selected.
    pub(crate) fn new_automation(&mut self, cx: &mut Context<Self>) {
        let nine = NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default();
        // Never a mirror: its automations run at home.
        let workspace = self
            .order()
            .into_iter()
            .filter(|s| !s.is_remote())
            .find(|s| s.id == self.space.id)
            .or_else(|| self.order().into_iter().find(|s| !s.is_remote()))
            .map(|s| s.id.clone());
        self.automation_drafts
            .entry(NEW)
            .or_insert_with(|| Automation {
                id: NEW,
                name: String::new(),
                on: true,
                workspace,
                when: When::Daily { at: nine },
                act: Do::Notify {
                    text: String::new(),
                },
                max_runs: None,
                runs: 0,
                since: 0,
                last: None,
                opened: None,
                cooldown: COOLDOWN,
            });
        self.automations_at = Some(NEW);
        cx.notify();
    }

    /// The draft of `id`, else the saved rule.
    pub(crate) fn automation_draft(&self, id: u64) -> Option<&Automation> {
        self.automation_drafts.get(&id).or_else(|| self.rule(id))
    }

    /// Changes the draft of `id` (made from the saved rule if there is none); one equal to the
    /// saved rule again is no draft.
    pub(crate) fn edit_automation(
        &mut self,
        id: u64,
        edit: impl FnOnce(&mut Automation),
        cx: &mut Context<Self>,
    ) {
        let Some(current) = self.automation_draft(id).cloned() else {
            return;
        };
        let mut next = current.clone();
        edit(&mut next);
        if edits(&next) == edits(&current) {
            return;
        }
        match self.rule(id) {
            Some(saved) if edits(saved) == edits(&next) => {
                self.automation_drafts.remove(&id);
            }
            _ => {
                self.automation_drafts.insert(id, next);
            }
        }
        cx.notify();
    }

    /// What stops `a` from being saved, in the spec's words; None: nothing.
    pub(crate) fn automation_problem(&self, a: &Automation, now: NaiveDateTime) -> Option<String> {
        if a.name.chars().count() > NAME_MAX {
            return Some(format!("A name is at most {NAME_MAX} characters."));
        }
        if let Some(there) = a
            .workspace
            .as_ref()
            .and_then(|id| self.spaces().find(|s| s.id == *id))
            .and_then(|space| space.automations_there())
        {
            return Some(there);
        }
        if let Some(problem) = self.form_problem(a, now) {
            return Some(problem);
        }
        if let Do::Agent {
            agent,
            model,
            effort,
            ..
        } = &a.act
            && let Some(entry) = self.agents.iter().find(|e| e.id == *agent)
            && let Err(why) = entry.launch_args_with(model.as_deref(), effort.as_deref(), None)
        {
            return Some(why);
        }
        None
    }

    /// What stops `a` from being saved by the form's own rules (its name, times, dates, required
    /// fields, the cooldown, a workspace for what opens something), whatever computer it is for.
    pub(crate) fn form_problem(&self, a: &Automation, now: NaiveDateTime) -> Option<String> {
        if a.name.chars().count() > NAME_MAX {
            return Some(format!("A name is at most {NAME_MAX} characters."));
        }
        if let Some(problem) = when_problem(&a.when, now) {
            return Some(problem);
        }
        let blank = |text: &str| text.trim().is_empty();
        let event = match &a.when {
            When::Agent {
                who: Who::Blocks { blocks, .. },
                ..
            } if blocks.is_empty() => Some("Choose at least one agent."),
            When::Agent {
                who: Who::Kind { kind },
                ..
            } if blank(kind) => Some("Choose a kind of agent."),
            When::Command { terminal, .. } if blank(terminal) => Some("Choose a terminal."),
            _ if a.when.is_event() && a.cooldown > COOLDOWN_MOST => {
                Some("The cooldown runs from 0 to 1440 minutes.")
            }
            _ => None,
        };
        if let Some(problem) = event {
            return Some(problem.into());
        }
        let agent_event = matches!(a.when, When::Agent { .. });
        let own = match &a.act {
            Do::Prompt { block, .. } if blank(block) || (block == FIRED && !agent_event) => {
                Some("Choose an agent.")
            }
            Do::Prompt { text, .. } if blank(text) => Some("Write the prompt."),
            Do::Command {
                terminal: Some(t), ..
            } if blank(t) => Some("Choose a terminal."),
            Do::Command { text, .. } if blank(text) => Some("Write the command."),
            Do::Agent { agent, .. } if blank(agent) => Some("Choose an agent."),
            Do::Notify { text } if blank(text) => Some("Write the text."),
            _ => None,
        };
        if let Some(problem) = own {
            return Some(problem.into());
        }
        let opens = matches!(a.act, Do::Command { terminal: None, .. } | Do::Agent { .. });
        (opens && a.workspace.is_none()).then(|| "Choose the workspace it opens in.".into())
    }

    /// Saves the draft of `id`: a new one is added on; any is counted from now (`since`, no runs,
    /// its held run dropped, `next` computed). Err: its problem, nothing saved.
    pub(crate) fn save_automation(
        &mut self,
        id: u64,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        // Home's rule is saved at home: the answer comes later.
        if key_home(id).is_some() {
            return self.save_home_rule(id, cx);
        }
        let draft = self.automation_draft(id).cloned().ok_or("It is gone.")?;
        let saved = self.save_rule(id, draft, cx)?;
        self.automation_drafts.remove(&id);
        // The person's own save: no longer a limited token's rule.
        self.api_unconfine(saved);
        self.automations_at = Some(saved);
        cx.notify();
        Ok(saved)
    }

    /// Saves `draft` as the automation `id` (`NEW`: a new one, added on) with the editor's checks:
    /// counted from now (`since`, no runs, its held run dropped, `next` computed). Err: its
    /// problem, nothing saved. The editor's Save and the API's `save_automation` share it.
    pub(crate) fn save_rule(
        &mut self,
        id: u64,
        draft: Automation,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        if let Some(problem) = self.automation_problem(&draft, Local::now().naive_local()) {
            return Err(problem);
        }
        let now = SystemTime::now();
        let saved = if id == NEW {
            let new = Automation {
                since: unix(now),
                ..draft
            };
            self.automations.add(new)
        } else {
            let live = self.automations.get_mut(id).ok_or("It is gone.")?;
            // Another kind, terminal or folder is not the terminal the old one kept; another
            // command in it is.
            if !keeps_terminal(&live.act, &draft.act) {
                live.opened = None;
            }
            (live.name, live.workspace, live.when, live.act) =
                (draft.name, draft.workspace, draft.when, draft.act);
            (live.max_runs, live.since, live.runs) = (draft.max_runs, unix(now), 0);
            live.cooldown = draft.cooldown;
            id
        };
        let a = self.automations.get(saved).cloned().ok_or("It is gone.")?;
        let run = self.automation_runs.entry(saved).or_default();
        run.reset();
        run.next = a.on.then(|| next_for(&a, now)).flatten();
        self.look_at_automations(cx);
        Ok(saved)
    }

    /// Revert: the draft goes (a new one is removed).
    pub(crate) fn revert_automation(&mut self, id: u64, cx: &mut Context<Self>) {
        self.automation_drafts.remove(&id);
        let new = id == NEW || key_home(id).is_some_and(|(_, home)| home == 0);
        if new && self.automations_at == Some(id) {
            self.automations_at = None;
        }
        cx.notify();
    }
}
