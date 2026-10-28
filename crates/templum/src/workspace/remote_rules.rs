//! A remote workspace's copies of home's automations and of its agents' schedules (docs/specs/
//! templum-to-templum.md, Automations): made from home's state where it enters, bounded and
//! cleaned before they are kept or drawn, never saved. Home's automations run only at home: a
//! copy here is shown and edited through home's API, and is never in `Workspace::automations`.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, Local};
use gpui_kit::{App, AppContext as _, Context, SharedString, Window};
use serde_json::{Value, json};
use templum_api::client::Call;
use templum_api::model::{AutomationView, ScheduleView, State};

use super::automations::local;
use super::bus::{clean_title, clean_to};
use super::remote::{Deed, LINE_CHARS, MOST_BLOCKS, RemoteSpace, Then, clean_text};
use super::{Space, Workspace};
use crate::automations::clock::time_words;
use crate::automations::{
    Automation, COOLDOWN, COOLDOWN_MOST, Do, LastRun, NAME_MAX, Went, When, Who, copy_name,
    event_short,
};
use crate::automations_view::editor::block_label;

/// The Automations popup's keys of home's rules: `HOME_KEYS | n << 32 | home's id`, for the remote
/// `r<n>`; id 0 is that remote's new rule. Local ids count up from 1 and never reach it.
pub(crate) const HOME_KEYS: u64 = 1 << 62;

/// The most rules of one remote the copy keeps, the longest a text of one is (a prompt, a
/// command, a notification, an agent's prompt), and the most blocks a Who names.
pub(crate) const MOST_RULES: usize = 500;
pub(crate) const RULE_TEXT_CHARS: usize = 64 * 1024;
const MOST_WHO: usize = 100;
/// The most schedules the copy keeps, and the longest a text of one is (its first line).
pub(crate) const MOST_SCHEDULES: usize = 200;
pub(crate) const SCHEDULE_TEXT_CHARS: usize = 300;
/// The longest a block's uid and a schedule's id may be, in bytes.
const UID_MOST: usize = 64;
const SCHEDULE_ID_MOST: usize = 16;

/// The key of home's rule `id` of remote `remote` (`r3`); None when it isn't `r<n>` with `1 ≤ n < 2²⁹`
/// or the id is 2³² or more.
pub(crate) fn home_key(remote: &str, id: u64) -> Option<u64> {
    let n: u64 = remote.strip_prefix('r')?.parse().ok()?;
    (1..1 << 29).contains(&n).then_some(())?;
    (id < 1 << 32).then_some(HOME_KEYS | n << 32 | id)
}

/// The remote and home's rule a key names; None below [`HOME_KEYS`].
pub(crate) fn key_home(key: u64) -> Option<(String, u64)> {
    (key >= HOME_KEYS).then(|| {
        (
            format!("r{}", (key & !HOME_KEYS) >> 32),
            key & (u32::MAX as u64),
        )
    })
}

/// A rule of home's for the remote's workspace, as its state said it, cleaned.
#[derive(Clone, Debug)]
pub(crate) struct HomeRule {
    /// Home's id.
    pub id: u64,
    /// The rule as the popup draws and edits it: its `id` is its popup key, `workspace` home's.
    pub rule: Automation,
    /// Cleaning cut or changed something of it: saved from here it would not be home's rule.
    pub cut: bool,
    /// Home's next run, Unix seconds.
    pub next: Option<i64>,
    pub sentence: String,
    /// Home's own item, sent back unchanged by Duplicate (its `when` and `do`).
    pub raw: Arc<AutomationView>,
}

/// A delivery an agent of home's asked for later.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HomeSchedule {
    pub id: String,
    pub by: Option<String>,
    pub to: String,
    pub text: String,
    pub next: i64,
    pub every: Option<u64>,
}

/// A uid of home's, if it is one a block can have: short and plain.
fn plain(uid: &str) -> Option<String> {
    (uid.len() <= UID_MOST && !uid.chars().any(char::is_control)).then(|| uid.to_string())
}

fn line(text: &str) -> String {
    clean_to(text, LINE_CHARS)
}

fn text(text: &str) -> String {
    clean_text(text, RULE_TEXT_CHARS)
}

fn optional(text: Option<String>) -> Option<String> {
    text.as_deref().map(line)
}

/// Home's rule `view`, cleaned, as the popup's `key`, and whether cleaning cut or changed
/// anything of it (a text over the cap, a control character, a Who of more blocks than the copy
/// keeps): None for one that doesn't parse, or whose uids are not a block's.
pub(crate) fn rule_of(view: &AutomationView, key: u64) -> Option<(Automation, bool)> {
    let when_home = serde_json::from_value::<When>(view.when.clone()).ok()?;
    let act_home = serde_json::from_value::<Do>(view.act.clone()).ok()?;
    let when = match when_home.clone() {
        When::Cron { expr } => When::Cron { expr: line(&expr) },
        When::Weekly { mut days, at } => {
            days.truncate(7);
            When::Weekly { days, at }
        }
        When::Monthly { mut days, at } => {
            days.truncate(31);
            When::Monthly { days, at }
        }
        When::Agent {
            event,
            who: Who::Blocks { blocks, all },
        } => When::Agent {
            event,
            who: Who::Blocks {
                blocks: blocks
                    .iter()
                    .take(MOST_WHO)
                    .map(|b| plain(b))
                    .collect::<Option<_>>()?,
                all,
            },
        },
        When::Agent {
            event,
            who: Who::Kind { kind },
        } => When::Agent {
            event,
            who: Who::Kind { kind: line(&kind) },
        },
        When::Command { terminal, failures } => When::Command {
            terminal: plain(&terminal)?,
            failures,
        },
        timed => timed,
    };
    let act = match act_home.clone() {
        Do::Prompt { block, text: said } => Do::Prompt {
            block: plain(&block)?,
            text: text(&said),
        },
        Do::Command {
            terminal,
            folder,
            text: said,
        } => Do::Command {
            terminal: match terminal {
                Some(t) => Some(plain(&t)?),
                None => None,
            },
            folder: optional(folder),
            text: text(&said),
        },
        Do::Agent {
            agent,
            folder,
            model,
            effort,
            prompt,
        } => Do::Agent {
            agent: line(&agent),
            folder: optional(folder),
            model: optional(model),
            effort: optional(effort),
            prompt: prompt.as_deref().map(text),
        },
        Do::Notify { text: said } => Do::Notify { text: text(&said) },
    };
    let last = view
        .last
        .as_ref()
        .and_then(|last| serde_json::from_value::<LastRun>(last.clone()).ok())
        .map(|last| LastRun {
            about: line(&last.about),
            ..last
        });
    let name = clean_to(&view.name, NAME_MAX);
    let cooldown = view.cooldown.unwrap_or(COOLDOWN);
    let cut = when != when_home || act != act_home || name != view.name || cooldown > COOLDOWN_MOST;
    Some((
        Automation {
            id: key,
            name,
            on: view.on,
            workspace: view.workspace.clone(),
            when,
            act,
            max_runs: view.max_runs,
            runs: view.runs,
            since: 0,
            last,
            opened: None,
            cooldown: cooldown.min(COOLDOWN_MOST),
        },
        cut,
    ))
}

impl RemoteSpace {
    /// Home's rules whose workspace is `home` (the share's), in home's order, at most
    /// [`MOST_RULES`]; a rule whose `Arc` didn't change is moved over as it was.
    pub(super) fn mirror_rules(&mut self, remote: &str, state: &State, home: Option<&str>) {
        let mut old: std::collections::HashMap<u64, HomeRule> = std::mem::take(&mut self.rules)
            .into_iter()
            .map(|r| (r.id, r))
            .collect();
        let mut seen = HashSet::new();
        self.rules = state
            .automations
            .iter()
            .filter(|view| home.is_some() && view.workspace.as_deref() == home)
            .filter_map(|view| {
                let key = home_key(remote, view.id)?;
                seen.insert(view.id).then_some(())?;
                if let Some(kept) = old.remove(&view.id).filter(|r| Arc::ptr_eq(&r.raw, view)) {
                    return Some(kept);
                }
                let (rule, cut) = rule_of(view, key)?;
                Some(HomeRule {
                    id: view.id,
                    rule,
                    cut,
                    next: view.next,
                    sentence: line(&view.sentence),
                    raw: view.clone(),
                })
            })
            .take(MOST_RULES)
            .collect();
        // What a rule had done when first met is seen; a rule gone is forgotten.
        self.rules_seen.retain(|id, _| seen.contains(id));
        for r in &self.rules {
            let last = r.rule.last.as_ref().map(|l| l.at);
            self.rules_seen.entry(r.id).or_insert(last);
        }
    }

    /// Everything home's rules have done is seen.
    fn see_runs(&mut self) {
        for r in &self.rules {
            self.rules_seen
                .insert(r.id, r.rule.last.as_ref().map(|l| l.at));
        }
    }

    /// Home's schedules, at most [`MOST_SCHEDULES`], in home's order.
    pub(super) fn mirror_schedules(&mut self, state: &State) {
        self.schedules = state
            .schedules
            .iter()
            .filter_map(|s| schedule_of(s))
            .take(MOST_SCHEDULES)
            .collect();
    }
}

/// Home's schedule `s`, cleaned: None for one whose id or block is not a plain id.
fn schedule_of(s: &ScheduleView) -> Option<HomeSchedule> {
    let id = (s.id.len() <= SCHEDULE_ID_MOST && !s.id.chars().any(char::is_control))
        .then(|| s.id.clone())?;
    Some(HomeSchedule {
        id,
        by: s.by.as_deref().and_then(plain),
        to: plain(&s.to)?,
        text: clean_to(
            s.text.lines().next().unwrap_or_default(),
            SCHEDULE_TEXT_CHARS,
        ),
        next: s.next,
        every: s.every,
    })
}

/// One group of the Automations list: a remote workspace's rules, and its agents' schedules.
pub(crate) struct RuleGroup {
    pub space: String,
    /// "On winx-desktop · API".
    pub title: String,
    /// Home's rules, then the new one while it is a draft.
    pub keys: Vec<u64>,
    /// "Rules for all of winx-desktop's workspaces aren't shown here."
    pub foot: String,
    /// The remote isn't live.
    pub dim: bool,
    /// Home's schedules: (id, first line, second line), as `schedule_rows` says them.
    pub schedules: Vec<(String, String, String)>,
}

/// What the tool of `Deed::SaveRule` is called at home.
const SAVE_TOOL: &str = "save_automation";

/// A `Then` that says home's refusal and gives its answer to `done`.
fn said_or(
    done: impl FnOnce(&mut Workspace, Value, &mut Window, &mut Context<Workspace>) + 'static,
) -> Then {
    Box::new(move |ws, answered, window, cx| match answered {
        Err(refusal) => ws.warn(refusal.sentence, window, cx),
        Ok(answer) => done(ws, answer, window, cx),
    })
}

/// The number in home's "Saved automation 7.".
fn saved_number(answer: &Value) -> Option<u64> {
    answer["text"]
        .as_str()?
        .strip_prefix("Saved automation ")?
        .trim_end_matches('.')
        .parse()
        .ok()
}

/// The draft as automations.json keeps it: name, on, workspace, when, do, max_runs, cooldown.
pub(crate) fn rule_json(a: &Automation) -> Value {
    let mut rule = json!({
        "name": a.name,
        "on": a.on,
        "when": a.when,
        "do": a.act,
        "cooldown": a.cooldown,
    });
    if let Some(workspace) = &a.workspace {
        rule["workspace"] = json!(workspace);
    }
    if let Some(max) = a.max_runs {
        rule["max_runs"] = json!(max);
    }
    rule
}

impl Workspace {
    /// The saved rule of `key`: this computer's, or the copy of home's.
    pub(crate) fn rule(&self, key: u64) -> Option<&Automation> {
        match key_home(key) {
            None => self.automations.get(key),
            Some(_) => self.home_rule(key).map(|(_, rule)| &rule.rule),
        }
    }

    /// The remote space and the copy of home's rule `key`.
    pub(crate) fn home_rule(&self, key: u64) -> Option<(&Space, &HomeRule)> {
        let (remote, id) = key_home(key)?;
        let space = self.space_named(&remote)?;
        let rule = space.remote.as_deref()?.rules.iter().find(|r| r.id == id)?;
        Some((space, rule))
    }

    /// The keys of every remote's rules, and of its new one while it is a draft, in the
    /// sidebar's order.
    pub(crate) fn home_keys(&self) -> Vec<u64> {
        self.order()
            .into_iter()
            .filter_map(|space| Some((space, space.remote.as_deref()?)))
            .flat_map(|(space, remote)| {
                let new = home_key(&space.id, 0).filter(|k| self.automation_drafts.contains_key(k));
                remote
                    .rules
                    .iter()
                    .filter_map(|r| home_key(&space.id, r.id))
                    .chain(new)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// One group of the list per remote workspace, in the sidebar's order.
    pub(crate) fn rule_groups(&self, cx: &App) -> Vec<RuleGroup> {
        self.order()
            .into_iter()
            .filter_map(|space| {
                let remote = space.remote.as_deref()?;
                let computer = crate::mirror::computer_of(remote);
                let new = home_key(&space.id, 0).filter(|k| self.automation_drafts.contains_key(k));
                let keys = remote
                    .rules
                    .iter()
                    .filter_map(|r| home_key(&space.id, r.id))
                    .chain(new)
                    .collect();
                Some(RuleGroup {
                    space: space.id.clone(),
                    title: format!("On {computer} · {}", clean_title(&space.name)),
                    keys,
                    foot: format!("Rules for all of {computer}'s workspaces aren't shown here."),
                    dim: remote.dim(),
                    schedules: self.home_schedule_rows(space, remote, cx),
                })
            })
            .collect()
    }

    /// Home's schedules as the Automations list says them: ("s3", "s3 from Claude Code (b7k2m) to
    /// k9x1d", "every 10 min · next 15:10 · Check the deploy").
    fn home_schedule_rows(
        &self,
        space: &Space,
        remote: &RemoteSpace,
        cx: &App,
    ) -> Vec<(String, String, String)> {
        let clock = |secs: i64| local(secs).format("%H:%M").to_string();
        remote
            .schedules
            .iter()
            .map(|s| {
                let first = match &s.by {
                    Some(by) => {
                        let who = match self.uid_title_in(&space.id, by, cx) {
                            Some(title) => format!("{title} ({by})"),
                            None => by.clone(),
                        };
                        let to = if *by == s.to { "itself" } else { s.to.as_str() };
                        format!("{} from {who} to {to}", s.id)
                    }
                    None => format!("{} to {}", s.id, s.to),
                };
                let when = match s.every {
                    Some(every) => format!("every {} min · next {}", every / 60, clock(s.next)),
                    None => format!("once · {}", clock(s.next)),
                };
                (s.id.clone(), first, format!("{when} · {}", s.text))
            })
            .collect()
    }

    /// How many of home's rules went other than Ran since the popup was last opened: a last run
    /// that is not the one then seen. (Home's clock is never compared with this one's.)
    pub(crate) fn home_unseen(&self) -> usize {
        self.spaces()
            .filter_map(|space| space.remote.as_deref())
            .flat_map(|remote| remote.rules.iter().map(move |r| (remote, r)))
            .filter(|(remote, r)| {
                r.rule.last.as_ref().is_some_and(|l| {
                    l.went != Went::Ran && remote.rules_seen.get(&r.id) != Some(&Some(l.at))
                })
            })
            .count()
    }

    /// The popup is opened: everything home's rules have done is seen.
    pub(super) fn see_home_runs(&mut self) {
        for space in std::iter::once(&mut self.space).chain(&mut self.others) {
            if let Some(remote) = space.remote.as_deref_mut() {
                remote.see_runs();
            }
        }
    }

    /// The group's +: a new rule of home's (a draft keyed `home_key(space, 0)`), its Where home's
    /// workspace, on, daily 09:00, Notify me, selected.
    pub(crate) fn new_home_automation(&mut self, space: &str, cx: &mut Context<Self>) {
        let Some(key) = home_key(space, 0) else {
            return;
        };
        let Some(workspace) = self
            .space_named(space)
            .and_then(|s| s.remote.as_deref())
            .and_then(|remote| remote.home_workspace())
        else {
            return;
        };
        let nine = chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap_or_default();
        self.automation_drafts
            .entry(key)
            .or_insert_with(|| Automation {
                id: key,
                name: String::new(),
                on: true,
                workspace: Some(workspace),
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
        self.automations_at = Some(key);
        cx.notify();
    }

    /// The remote, home's id and whether it runs a command, of the rule `key`.
    fn home_target(&self, key: u64) -> Option<(String, u64, bool)> {
        let (space, rule) = self.home_rule(key)?;
        let command = matches!(rule.rule.act, Do::Command { .. });
        Some((space.id.clone(), rule.id, command))
    }

    /// `sentence` said as a warning: at once when the caller has a window, else when the update
    /// under way is over.
    fn say(&mut self, sentence: String, window: Option<&mut Window>, cx: &mut Context<Self>) {
        match window {
            Some(window) => self.warn(sentence, window, cx),
            None => {
                let handle = self.window_handle;
                cx.spawn(async move |ws, cx| {
                    cx.update_window(handle, |_, window, cx| {
                        ws.update(cx, |ws, cx| ws.warn(sentence, window, cx)).ok();
                    })
                    .ok();
                })
                .detach();
            }
        }
    }

    /// The deed `deed` for home's tool: the rule asked first (it says why not, and nothing is
    /// sent), then home's call; home's refusal is said, its answer goes to `done`.
    fn home_call(
        &mut self,
        space: &str,
        deed: Deed,
        args: Value,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Workspace, Value, &mut Window, &mut Context<Workspace>) + 'static,
    ) {
        let then = said_or(done);
        self.home_call_then(space, deed, args, window, cx, then);
    }

    /// `home_call` with the whole answer left to `then`.
    fn home_call_then(
        &mut self,
        space: &str,
        deed: Deed,
        args: Value,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
        then: Then,
    ) {
        let Some(tool) = self.home_tool(space, deed, window, cx) else {
            return;
        };
        self.send_home(space, tool, args, then, cx);
    }

    /// `home_call` for the rule `key` of home's: the same deed sent twice before home answers the
    /// first (a second click) goes once.
    fn home_rule_call(
        &mut self,
        key: u64,
        deed: Deed,
        args: Value,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
        done: impl FnOnce(&mut Workspace, Value, &mut Window, &mut Context<Workspace>) + 'static,
    ) {
        let then = said_or(done);
        self.home_rule_call_then(key, deed, args, window, cx, then);
    }

    /// `home_rule_call` with the whole answer left to `then`.
    fn home_rule_call_then(
        &mut self,
        key: u64,
        deed: Deed,
        args: Value,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
        then: Then,
    ) {
        let Some((space, _)) = key_home(key) else {
            return;
        };
        let Some(tool) = self.home_tool(&space, deed, window, cx) else {
            return;
        };
        if !self.home_busy.insert((key, tool)) {
            return;
        }
        let then: Then = Box::new(move |ws, answered, window, cx| {
            ws.home_busy.remove(&(key, tool));
            then(ws, answered, window, cx)
        });
        self.send_home(&space, tool, args, then, cx);
    }

    /// The tool of `deed`, if home's rule lets it go (else the why not is said).
    fn home_tool(
        &mut self,
        space: &str,
        deed: Deed,
        window: Option<&mut Window>,
        cx: &mut Context<Self>,
    ) -> Option<&'static str> {
        let (tool, _) = deed.call()?;
        match self.why_not_in(space, deed) {
            Some(why) => {
                self.say(why, window, cx);
                None
            }
            None => Some(tool),
        }
    }

    fn send_home(
        &mut self,
        space: &str,
        tool: &'static str,
        args: Value,
        then: Then,
        cx: &mut Context<Self>,
    ) {
        let call = Call {
            tool: tool.into(),
            args,
            slow: false,
        };
        self.remote_call(space, call, then, cx);
    }

    /// Whether a Save of home's rule `key` waits for home's answer: the editor dims Save.
    pub(crate) fn home_saving(&self, key: u64) -> bool {
        self.home_busy.contains(&(key, SAVE_TOOL))
    }

    /// The switch of home's rule `key`: `set_automation` at home. The copy shows it when home's
    /// state does.
    pub(super) fn set_home_rule(&mut self, key: u64, on: bool, cx: &mut Context<Self>) {
        let Some((_, id, command)) = self.home_target(key) else {
            return;
        };
        let args = json!({ "automation": id, "on": on });
        self.home_rule_call(key, Deed::SetRule(command), args, None, cx, |_, _, _, _| {});
    }

    /// Run now of home's rule `key`: `run_automation` at home, where it runs.
    pub(super) fn run_home_rule(&mut self, key: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, id, command)) = self.home_target(key) else {
            return;
        };
        let args = json!({ "automation": id });
        self.home_rule_call(
            key,
            Deed::RunRule(command),
            args,
            Some(window),
            cx,
            |_, _, _, _| {},
        );
    }

    /// Duplicate of home's rule `key`: home saves an off copy of its own `when` and `do`, and its
    /// answer selects the copy.
    pub(super) fn duplicate_home_rule(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some((space, rule)) = self.home_rule(key) else {
            return;
        };
        let (space, command, raw) = (
            space.id.clone(),
            matches!(rule.rule.act, Do::Command { .. }),
            rule.raw.clone(),
        );
        let mut copy = json!({
            "name": copy_name(&raw.name),
            "on": false,
            "workspace": raw.workspace,
            "when": raw.when,
            "do": raw.act,
        });
        // Home's own numbers, where it has them: a null is not a number to its parser.
        for (key, value) in [("max_runs", raw.max_runs), ("cooldown", raw.cooldown)] {
            if let Some(value) = value {
                copy[key] = json!(value);
            }
        }
        let args = json!({ "automation": copy });
        let remote = space.clone();
        self.home_rule_call(
            key,
            Deed::SaveRule(command),
            args,
            None,
            cx,
            move |ws, answer, _, cx| {
                if let Some(key) = saved_number(&answer).and_then(|n| home_key(&remote, n)) {
                    ws.automations_at = Some(key);
                    cx.notify();
                }
            },
        );
    }

    /// Delete of home's rule `key`, once confirmed: `delete_automation` at home; the rule leaves
    /// with home's state.
    pub(super) fn delete_home_rule(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some((_, id, _)) = self.home_target(key) else {
            return;
        };
        let args = json!({ "automation": id });
        self.home_rule_call(
            key,
            Deed::DeleteRule,
            args,
            None,
            cx,
            move |ws, _, _, cx| {
                ws.automation_drafts.remove(&key);
                cx.notify();
            },
        );
    }

    /// The question before Delete of home's rule `key`: the rule's refusal, said, else None.
    pub(super) fn home_delete_refusal(&self, key: u64) -> Option<String> {
        let (space, _, _) = self.home_target(key)?;
        self.why_not_in(&space, Deed::DeleteRule)
    }

    /// Cancel of a home schedule: `cancel_schedule` at home.
    pub(crate) fn cancel_home_schedule(
        &mut self,
        space: &str,
        schedule: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let args = json!({ "schedule": schedule });
        self.home_call(
            space,
            Deed::CancelSchedule,
            args,
            Some(window),
            cx,
            |_, _, _, _| {},
        );
    }

    /// A remote went: its drafts, and the selection if it is one of its rules.
    pub(super) fn forget_home_rules(&mut self, remote: &str) {
        let of = |key: &u64| key_home(*key).is_some_and(|(r, _)| r == remote);
        self.automation_drafts.retain(|key, _| !of(key));
        self.home_busy.retain(|(key, _)| !of(key));
        if self.automations_at.as_ref().is_some_and(of) {
            self.automations_at = None;
        }
    }

    /// A home rule's `next` row words: "Off" when off, an event's short words, else its next
    /// time, else "Never".
    pub(super) fn home_next_words(&self, key: u64, now: SystemTime) -> String {
        let Some((space, rule)) = self.home_rule(key).filter(|(_, r)| r.rule.on) else {
            return "Off".into();
        };
        let kind = |id: &str| self.home_kind_title(&space.id, id);
        if let Some(event) = event_short(&rule.rule.when, &kind) {
            return event;
        }
        match rule.next {
            Some(next) => {
                let (next, now) = (local(next), DateTime::<Local>::from(now));
                time_words(next.naive_local(), now.naive_local())
            }
            None => "Never".into(),
        }
    }

    /// The title of the block `uid` as a rule of `home` (a remote space) names it, else as this
    /// computer's rules do.
    pub(super) fn uid_title_of(&self, home: Option<&Space>, uid: &str, cx: &App) -> Option<String> {
        match home {
            Some(space) => self.uid_title_in(&space.id, uid, cx),
            None => self.uid_title(uid, cx),
        }
    }

    /// Save of a home rule's draft: the form's rules, the rule's deed (`SaveRule(runs a command)`),
    /// then `save_automation` at home (`id` home's, left out for a new one). Ok: sent, the answer
    /// comes later (its draft goes with it). Err: why not, for the editor's red line.
    pub(crate) fn save_home_rule(
        &mut self,
        key: u64,
        cx: &mut Context<Self>,
    ) -> Result<u64, String> {
        let (space, id) = key_home(key).ok_or("It is gone.")?;
        let draft = self.automation_draft(key).cloned().ok_or("It is gone.")?;
        if let Some((space, rule)) = self.home_rule(key)
            && rule.cut
        {
            let computer = space
                .remote
                .as_deref()
                .map(crate::mirror::computer_of)
                .unwrap_or_default();
            return Err(format!(
                "This rule is longer than a mirror shows: edit it on {computer}."
            ));
        }
        if let Some(problem) = self.form_problem(&draft, Local::now().naive_local()) {
            return Err(problem);
        }
        let deed = Deed::SaveRule(matches!(draft.act, Do::Command { .. }));
        if let Some(why) = self.why_not_in(&space, deed) {
            return Err(why);
        }
        let mut args = json!({ "automation": rule_json(&draft) });
        if id != 0 {
            args["id"] = json!(id);
        }
        let remote = space.clone();
        let then: Then = Box::new(move |ws, answered, _, cx| {
            match answered {
                // Home's refusal is the red line of the form that still shows this rule.
                Err(refusal) => ws.home_save_refused(key, refusal.sentence),
                Ok(answer) => {
                    ws.automation_drafts.remove(&key);
                    // A new rule selects what home numbered it, if the person is still on the
                    // draft.
                    if id == 0
                        && ws.automations_at == Some(key)
                        && let Some(saved) =
                            saved_number(&answer).and_then(|n| home_key(&remote, n))
                    {
                        ws.automations_at = Some(saved);
                    }
                }
            }
            cx.notify();
        });
        self.home_rule_call_then(key, deed, args, None, cx, then);
        Ok(id)
    }

    /// Home's rules changed: the form that shows one of them, with no draft over it, shows it
    /// again (a Save's answer comes before the state that carries the saved rule).
    pub(super) fn reload_home_form(&mut self, remote: &str) {
        let shown = self.automation_form.as_ref().and_then(|f| f.for_id);
        if let Some(key) = shown
            && key_home(key).is_some_and(|(r, _)| r == remote)
            && !self.automation_drafts.contains_key(&key)
        {
            self.reload_automation_form();
        }
    }

    /// Home's terminals of the remote `space`'s workspace (agents only when `agents`), from its
    /// state: (uid, label), the label as the editor says a block.
    pub(crate) fn home_blocks(
        &self,
        space: &str,
        agents: bool,
        _cx: &App,
    ) -> Vec<(SharedString, SharedString)> {
        let Some(space) = self.space_named(space) else {
            return Vec::new();
        };
        let Some(remote) = space.remote.as_deref() else {
            return Vec::new();
        };
        let home = remote.home_workspace();
        remote
            .state
            .blocks
            .iter()
            .filter(|b| {
                b.kind == "terminal"
                    && !b.read_only
                    && home.as_deref() == Some(b.workspace.as_str())
                    && (!agents || b.agent.is_some())
            })
            .filter_map(|b| {
                let uid = plain(&b.id)?;
                let label = block_label(&clean_title(&b.title), &uid, &clean_title(&space.name));
                Some((uid.into(), label.into()))
            })
            .take(MOST_BLOCKS)
            .collect()
    }

    /// Home's agents (`RemoteSpace::agents`) of the remote `space`: (id, title).
    pub(crate) fn home_kinds(&self, space: &str) -> Vec<(SharedString, SharedString)> {
        self.space_named(space)
            .and_then(|s| s.remote.as_deref())
            .and_then(|r| r.agents.as_ref())
            .map(|agents| {
                agents
                    .iter()
                    .map(|a| (a.id.clone().into(), a.title.clone().into()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The folder of home's workspace the remote `space` mirrors: a path on the other computer.
    pub(super) fn home_folder(&self, space: &str) -> String {
        self.space_named(space)
            .and_then(|s| s.remote.as_deref())
            .and_then(|r| r.home.as_ref())
            .and_then(|h| h.folder.as_deref())
            .map(line)
            .unwrap_or_default()
    }

    /// Home's agent `id` by its title (else its id).
    pub(crate) fn home_kind_title(&self, space: &str, id: &str) -> String {
        self.space_named(space)
            .and_then(|s| s.remote.as_deref())
            .and_then(|r| r.agents.as_ref())
            .and_then(|agents| agents.iter().find(|a| a.id == id))
            .map_or_else(|| id.to_string(), |a| a.title.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn view(id: u64, workspace: &str) -> AutomationView {
        AutomationView {
            id,
            name: format!("Rule {id}"),
            on: true,
            workspace: Some(workspace.into()),
            when: json!({"daily": {"at": "09:00:00"}}),
            act: json!({"notify": {"text": "x"}}),
            cooldown: Some(COOLDOWN),
            ..Default::default()
        }
    }

    fn state_of(views: Vec<AutomationView>) -> State {
        State {
            automations: views.into_iter().map(Arc::new).collect(),
            ..Default::default()
        }
    }

    fn remote() -> RemoteSpace {
        RemoteSpace::new(crate::session::RemoteEntry::default())
    }

    #[test]
    fn keys_of_home_s_rules_never_meet_a_local_id() {
        let key = home_key("r1", 7).unwrap();
        assert_eq!(key, HOME_KEYS | 1 << 32 | 7);
        assert_eq!(key_home(key), Some(("r1".into(), 7)));
        assert_eq!(key_home(home_key("r3", 0).unwrap()), Some(("r3".into(), 0)));
        assert_eq!(key_home(41), None);
        assert_eq!(home_key("r1", 1 << 32), None);
        assert_eq!(home_key("w3", 1), None);
        assert_eq!(home_key("r0", 1), None);
        assert_eq!(home_key("r536870912", 1), None);
        assert!(home_key("r1", 0).unwrap() >= HOME_KEYS);
    }

    #[test]
    fn home_s_rules_are_cleaned_and_capped() {
        let mut views: Vec<AutomationView> = (1..=5_000).map(|id| view(id, "w1")).collect();
        views.extend((5_001..=5_003).map(|id| view(id, "w2")));
        let mut space = remote();
        space.mirror_rules("r1", &state_of(views), Some("w1"));
        assert_eq!(space.rules.len(), MOST_RULES);
        assert!(
            space
                .rules
                .iter()
                .all(|r| r.raw.workspace.as_deref() == Some("w1"))
        );
        assert_eq!(space.rules[0].id, 1);
        assert_eq!(space.rules[MOST_RULES - 1].id, MOST_RULES as u64);

        let mut hostile = view(1, "w1");
        hostile.act = json!({"notify": {"text": format!("a\x1b[31m\x07{}", "p".repeat(1 << 20))}});
        hostile.name = "n".repeat(200);
        let mut cron = view(2, "w1");
        cron.when = json!({"cron": {"expr": "*".repeat(10_000)}});
        let mut spiral = view(3, "w1");
        spiral.when = json!({"spiral": {}});
        let far = view(1 << 40, "w1");
        let mut crowd = view(4, "w1");
        crowd.when = json!({"agent": {"event": "idle", "who": {
            "blocks": (0..300).map(|n| format!("b{n}")).collect::<Vec<_>>()}}});
        let mut bad = view(5, "w1");
        bad.act = json!({"prompt": {"block": "ab\ncd", "text": "x"}});
        let mut space = remote();
        space.mirror_rules(
            "r1",
            &state_of(vec![hostile, cron, spiral, far, crowd, bad]),
            Some("w1"),
        );
        let ids: Vec<u64> = space.rules.iter().map(|r| r.id).collect();
        assert_eq!(ids, [1, 2, 4]);
        let Do::Notify { text } = &space.rules[0].rule.act else {
            panic!("a notification")
        };
        assert!(text.chars().count() <= RULE_TEXT_CHARS);
        assert!(
            !text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
        );
        assert!(space.rules[0].rule.name.chars().count() <= NAME_MAX);
        let When::Cron { expr } = &space.rules[1].rule.when else {
            panic!("a cron")
        };
        assert!(expr.chars().count() <= LINE_CHARS);
        let When::Agent {
            who: Who::Blocks { blocks, .. },
            ..
        } = &space.rules[2].rule.when
        else {
            panic!("blocks")
        };
        assert_eq!(blocks.len(), MOST_WHO);
        // The same Arcs again: each rule is kept as it was.
        let state = state_of(vec![view(1, "w1"), view(2, "w1")]);
        let mut space = remote();
        space.mirror_rules("r1", &state, Some("w1"));
        space.rules[0].rule.name = "kept as it was".into();
        space.mirror_rules("r1", &state, Some("w1"));
        assert_eq!(space.rules[0].rule.name, "kept as it was");
        assert!(Arc::ptr_eq(&space.rules[0].raw, &state.automations[0]));
        assert_eq!(space.rules[0].rule.id, home_key("r1", 1).unwrap());
    }

    #[test]
    fn home_s_schedules_are_cleaned_and_capped() {
        let schedule = |id: &str, to: &str| ScheduleView {
            id: id.into(),
            by: Some("b7k2m".into()),
            to: to.into(),
            text: "check".into(),
            next: 5,
            every: Some(600),
            until: 9,
        };
        let mut state = State {
            schedules: (1..=1_000)
                .map(|n| Arc::new(schedule(&format!("s{n}"), "k9x1d")))
                .collect(),
            ..Default::default()
        };
        let mut space = remote();
        space.mirror_schedules(&state);
        assert_eq!(space.schedules.len(), MOST_SCHEDULES);
        let mut long = schedule("s1", "k9x1d");
        long.text = format!("first\u{1b}[31m line{}\nsecond", "x".repeat(100_000));
        let mut by = schedule("s2", "k9x1d");
        by.by = Some("b".repeat(500));
        state.schedules = vec![
            Arc::new(long),
            Arc::new(by),
            Arc::new(schedule("s3\n", "k9x1d")),
            Arc::new(schedule("s4", &"t".repeat(500))),
        ];
        space.mirror_schedules(&state);
        let ids: Vec<&str> = space.schedules.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s1", "s2"]);
        let text = &space.schedules[0].text;
        assert!(text.chars().count() <= SCHEDULE_TEXT_CHARS && text.starts_with("first"));
        assert!(!text.chars().any(char::is_control));
        assert_eq!(space.schedules[1].by, None);
    }
}
