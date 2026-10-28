//! Deliveries an agent asked for later (docs/specs/agent-tools.md#schedule-and-cancel_schedule):
//! kept in `Workspace::schedules`, looked at every SCHEDULE_LOOK while there are any. Their times
//! are the computer's clock: GPUI's timers and `Instant` don't count time asleep, so a time
//! missed while it slept goes at the first look after, once.

use std::time::{Duration, Instant, SystemTime};

use chrono::Local;
use gpui_kit::Context;
use templum_mcp::{Reply, tools};

use templum_agent::NoticeLevel;

use super::Workspace;
use super::bus::{Outcome, target};
use super::delegate::NoticeKind;
use super::waits::{EVENT_LOOK, PRINTED_LINES};
use crate::automations::clock;

/// How often the schedules are looked at, while there are any.
pub(crate) const SCHEDULE_LOOK: Duration = Duration::from_secs(15);
/// How long a repeating schedule lasts, as Claude Code's cron does.
const LASTS: Duration = Duration::from_secs(7 * 24 * 60 * 60);
/// The most characters of a schedule's text `whoami` gives.
const TEXT_CHARS: usize = 60;

/// A text the terminal `by` asked to have delivered to the block `to` (its block id: the asker's
/// own, or one wired to it running an agent) at `next`, and again every `every` until `until`.
pub(crate) struct Schedule {
    pub id: u64,
    pub by: String,
    pub to: String,
    pub text: String,
    pub next: SystemTime,
    pub every: Option<Duration>,
    pub until: SystemTime,
}

/// The next time the local clock shows `hour`:`minute`, today or tomorrow; a time a daylight-saving
/// change skips is the hour after.
fn next_at(hour: u8, minute: u8) -> SystemTime {
    let now = Local::now();
    (0..=2)
        .flat_map(|days| {
            let day = now.date_naive() + chrono::Days::new(days);
            let at = day.and_hms_opt(hour.into(), minute.into(), 0)?;
            clock::resolve(&Local, at)
        })
        .find(|at| *at > now)
        .map_or_else(SystemTime::now, SystemTime::from)
}

/// `t` on the local clock, as `HH:MM`, or `d Mon HH:MM`.
fn clock(t: SystemTime, with_day: bool) -> String {
    let at: chrono::DateTime<Local> = t.into();
    at.format(if with_day { "%-d %b %H:%M" } else { "%H:%M" })
        .to_string()
}

/// "1 minute", "10 minutes".
fn minutes(n: u64) -> String {
    match n {
        1 => "1 minute".into(),
        n => format!("{n} minutes"),
    }
}

/// A `wait_for` with a pattern and `wait: false`: the watch `watch` on the terminal `of` (the
/// block `uid`), set by the terminal `by`, until its first match, or `until` (`timeout` seconds
/// from its call, on the executor's clock), or its block's end.
pub(crate) struct Watch {
    pub id: u64,
    pub by: String,
    pub of: String,
    pub uid: String,
    pub pattern: String,
    pub watch: u64,
    pub timeout: u64,
    pub until: Instant,
}

impl Workspace {
    /// `schedule` from the terminal `from`: `text` to the block `block` (its own, or one wired to
    /// it, running an agent) at its time, and every `every` minutes after.
    pub(super) fn schedule(
        &mut self,
        from: &str,
        block: &str,
        text: &str,
        when: tools::When,
        every: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = match target(space, me, block) {
            Ok(to) => to,
            Err(refused) => return refused,
        };
        let Some(pane) = space.terminal(to) else {
            return Reply::err(format!("{block} isn't a terminal."));
        };
        // Also one Templum started there that no snapshot found yet (`spawn_agent` a moment ago,
        // its shell still in front), and one that quit the last snapshot still has. The caller's
        // own too: a shell running `templum schedule` would never take its notices.
        if pane.agent().is_none() {
            return Reply::err(format!(
                "{block} runs no agent: schedules go to agents. To run a command, open a terminal \
                 with create_block's command."
            ));
        }
        let now = SystemTime::now();
        let every = every.map(|m| Duration::from_secs(m * 60));
        let next = match when {
            tools::When::At { hour, minute } => next_at(hour, minute),
            tools::When::After { minutes } => now + Duration::from_secs(minutes * 60),
            tools::When::None => now + every.unwrap_or_default(),
        };
        let until = if every.is_some() { next + LASTS } else { next };
        let id = self.schedule_add(
            Schedule {
                id: 0,
                by: from.to_string(),
                to: block.to_string(),
                text: text.to_string(),
                next,
                every,
                until,
            },
            cx,
        );
        let to = if to == me { "you" } else { block };
        let wait = next
            .duration_since(now)
            .unwrap_or_default()
            .as_secs()
            .div_ceil(60);
        let mut said = match when {
            tools::When::At { .. } => format!(
                "Scheduled s{id}: to {to} at {} (in {})",
                clock(next, false),
                minutes(wait)
            ),
            _ => format!("Scheduled s{id}: to {to} in {}", minutes(wait)),
        };
        if let Some(every) = every {
            said.push_str(&format!(
                ", then every {} until {}",
                minutes(every.as_secs() / 60),
                clock(until, true)
            ));
        }
        said.push('.');
        Reply::ok(said)
    }

    /// `cancel_schedule` from the terminal `from`: one of its schedules, `s3` or `3`.
    pub(super) fn cancel_schedule(&mut self, from: &str, schedule: &str) -> Reply {
        let id = schedule.trim().trim_start_matches('s').parse::<u64>().ok();
        let mine = id.filter(|id| self.schedules.iter().any(|s| s.id == *id && s.by == from));
        match mine {
            Some(id) if self.schedule_cancel(id) => Reply::ok(format!("Cancelled s{id}.")),
            _ => Reply::err(format!(
                "s{} isn't one of your schedules.",
                schedule.trim().trim_start_matches('s')
            )),
        }
    }

    /// The schedules the terminal `by` asked for, oldest first, each said as its block's menu
    /// says it: `s3: to k9x1d every 10 min`, its asker's block `me` being "you".
    pub(crate) fn schedules_of(&self, by: &str, me: &str) -> Vec<(u64, String)> {
        self.schedules
            .iter()
            .filter(|s| s.by == by)
            .map(|s| {
                let to = if s.to == me { "you" } else { s.to.as_str() };
                let every = s.every.map_or(String::new(), |e| {
                    format!(" every {} min", e.as_secs() / 60)
                });
                (s.id, format!("s{}: to {to}{every}", s.id))
            })
            .collect()
    }

    /// `whoami`'s lines for the terminal `by`'s schedules, and its clock's tooltip's: `schedule
    /// s3: to k9x1d every 10 min, next 15:10: <its text's first line>`.
    pub(crate) fn schedule_lines(&self, by: &str, me: &str) -> Vec<String> {
        self.schedules_of(by, me)
            .into_iter()
            .zip(self.schedules.iter().filter(|s| s.by == by))
            .map(|((_, about), s)| {
                let line: String = s
                    .text
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .take(TEXT_CHARS)
                    .collect();
                format!("schedule {about}, next {}: {line}", clock(s.next, false))
            })
            .collect()
    }

    /// The person cancels every schedule the terminal `by` asked for (its block's menu).
    pub(crate) fn cancel_schedules_of(&mut self, by: &str, cx: &mut Context<Self>) {
        self.schedules.retain(|s| s.by != by);
        cx.notify();
    }

    /// The person cancels schedule `id` (its asker's block's menu).
    pub(crate) fn cancel_schedule_now(&mut self, id: u64, cx: &mut Context<Self>) {
        self.schedule_cancel(id);
        cx.notify();
    }

    /// Looks at the schedules every SCHEDULE_LOOK until there are none; started by the first.
    pub(super) fn look_at_schedules(&mut self, cx: &mut Context<Self>) {
        if self.scheduling {
            return;
        }
        self.scheduling = true;
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor().timer(SCHEDULE_LOOK).await;
                let more = ws
                    .update(cx, |ws, cx| ws.look_schedules(cx))
                    .unwrap_or(false);
                if !more {
                    break;
                }
            }
        })
        .detach();
    }

    /// Delivers the schedules whose time came, once each; drops those whose asker's block is gone,
    /// silently, and the repeating ones past their 7 days, saying so. False once none is left.
    fn look_schedules(&mut self, cx: &mut Context<Self>) -> bool {
        let gone: Vec<u64> = self
            .schedules
            .iter()
            .filter(|s| self.pane(&s.by).is_none())
            .map(|s| s.id)
            .collect();
        self.schedules.retain(|s| !gone.contains(&s.id));
        let now = SystemTime::now();
        let due: Vec<u64> = self
            .schedules
            .iter()
            .filter(|s| s.next <= now || (s.every.is_some() && s.until <= now))
            .map(|s| s.id)
            .collect();
        for id in due {
            let Some(at) = self.schedules.iter().position(|s| s.id == id) else {
                continue;
            };
            let (by, to) = (self.schedules[at].by.clone(), self.schedules[at].to.clone());
            if self.schedules[at].every.is_some() && self.schedules[at].until <= now {
                self.schedules.remove(at);
                self.queue_notice(
                    by,
                    format!("[schedule s{id} ended: 7 days]"),
                    NoticeKind::Other,
                    cx,
                );
                continue;
            }
            let ends = match self.deliver(id, cx) {
                Ok(()) => false,
                Err((why, closed)) => {
                    let notice = format!("[schedule s{id} to {to} didn't go: {why}]");
                    self.queue_notice(by, notice, NoticeKind::Other, cx);
                    closed
                }
            };
            let Some(s) = self.schedules.iter_mut().find(|s| s.id == id) else {
                continue;
            };
            match s.every {
                Some(every) if !ends => s.next = now + every,
                _ => self.schedules.retain(|s| s.id != id),
            }
        }
        cx.notify();
        if self.schedules.is_empty() {
            self.scheduling = false;
        }
        !self.schedules.is_empty()
    }

    /// Schedule `id` goes now, as the tool it stands for would from its asker: a notice to the
    /// asker's own block, a task to an agent that has none. Err: why not, and whether its block
    /// was closed (which ends it).
    fn deliver(&mut self, id: u64, cx: &mut Context<Self>) -> Result<(), (String, bool)> {
        let s = self.schedules.iter().find(|s| s.id == id).expect("due");
        let (by, uid, text) = (s.by.clone(), s.to.clone(), s.text.clone());
        let (space_id, me) = self
            .caller(&by)
            .ok_or_else(|| ("Your block was closed.".to_string(), true))?;
        let space = self.space_named(&space_id).expect("the caller's workspace");
        let to = target(space, me, &uid).map_err(|r| (r.text, !self.uid_taken(&uid)))?;
        if to == me {
            let notice = format!("[scheduled s{id}] Its text is in the pasted text.\n{text}");
            self.queue_notice(by, notice, NoticeKind::Other, cx);
            return Ok(());
        }
        let pane = space
            .terminal(to)
            .ok_or_else(|| (format!("{uid} isn't a terminal."), false))?;
        // Its agent quit: the text would run as a command.
        if pane.seen.agent.is_none() || pane.shell_in_front() {
            return Err((
                format!(
                    "{uid} runs no agent now: a schedule for an agent never types into its shell."
                ),
                false,
            ));
        }
        // A schedule never changes a task: one still under way, the asker's included.
        if let Some(handed) = &pane.handed {
            return Err((
                format!(
                    "{uid} is on a task (t{}): a schedule hands a task only to an agent that has \
                     none.",
                    handed.id
                ),
                false,
            ));
        }
        match self.delegate(&by, &uid, &text, false, tools::TASK_WAIT, Some(id), cx) {
            Outcome::Now(reply) if reply.is_error => Err((reply.text, false)),
            _ => Ok(()),
        }
    }

    /// Looks at the watches every EVENT_LOOK until there are none; started by the first.
    pub(super) fn look_at_watches(&mut self, cx: &mut Context<Self>) {
        if self.watching {
            return;
        }
        self.watching = true;
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor().timer(EVENT_LOOK).await;
                let more = ws.update(cx, |ws, cx| ws.look_watches(cx)).unwrap_or(false);
                if !more {
                    break;
                }
            }
        })
        .detach();
    }

    /// Each watch whose line came, whose time ran out, or whose block closed ends, telling its
    /// asker once (the Templum Agent in its chat); one whose asker's block is gone ends silently.
    /// False once none is left.
    fn look_watches(&mut self, cx: &mut Context<Self>) -> bool {
        let now = cx.background_executor().now();
        let mut kept = Vec::new();
        for w in std::mem::take(&mut self.watches) {
            let pane = self.pane(&w.of);
            let told = if w.by != templum_agent::ASKER
                && w.by != templum_api::ASKER
                && self.pane(&w.by).is_none()
            {
                None
            } else if let Some(pane) = pane {
                match pane.term.hit(w.watch) {
                    Some(line) => {
                        let last = pane.term.shared().lines(PRINTED_LINES).join("\n");
                        let text = format!(
                            "[watch w{} on {}: a line matches \"{}\"] The line is in the pasted \
                             text.\n{}\nIts last lines:\n{}",
                            w.id,
                            w.uid,
                            w.pattern,
                            tools::tail(&line, tools::MAX_TEXT),
                            tools::tail(&last, tools::MAX_TEXT)
                        );
                        Some((text, NoticeLevel::Done))
                    }
                    None if now >= w.until => {
                        let text = format!(
                            "[watch w{} on {} ended: no line matched \"{}\" in {} s]",
                            w.id, w.uid, w.pattern, w.timeout
                        );
                        Some((text, NoticeLevel::Failed))
                    }
                    None => {
                        kept.push(w);
                        continue;
                    }
                }
            } else {
                let text = format!("[watch w{} on {} ended: {} was closed]", w.id, w.uid, w.uid);
                Some((text, NoticeLevel::Failed))
            };
            if let Some(pane) = self.pane(&w.of) {
                pane.term.unwatch(w.watch);
            }
            if let Some((told, level)) = told {
                self.notice_to(w.by, told, &w.of, level, None, cx);
            }
        }
        self.watches = kept;
        if self.watches.is_empty() {
            self.watching = false;
        }
        !self.watches.is_empty()
    }
}
