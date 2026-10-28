//! The person's automations: their file, their times, what each Do does and the popup that lists
//! them (docs/specs/automations.md).

use std::time::{Duration, SystemTime};

use chrono::{Local, NaiveTime};
use serde_json::json;

#[cfg(unix)]
use super::agents_view::{agent, begins, ends};
use super::agents_view::{open_agents, press as press_key};
#[cfg(unix)]
use super::bus::{history_with, me, screen_until};
#[cfg(unix)]
use super::delegate::{entered, two_agents};
use super::inbox::{click_id, drawn, name};
#[cfg(unix)]
use super::spawn::spawner;
#[cfg(unix)]
use super::subagents::hook;
use super::*;
use crate::automations::{Automation, Automations, COOLDOWN, Do, FIRED, Heard, Went, When, Who};
use crate::session::Loaded;
#[cfg(unix)]
use crate::workspace::agents_view::Doing;
use crate::workspace::automations::{AUTOMATION_LOOK, unix};

/// A rule that notifies every day at 09:00, on, unnamed.
pub(super) fn notify(text: &str) -> Automation {
    Automation {
        id: 0,
        name: String::new(),
        on: true,
        workspace: None,
        when: When::Daily {
            at: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        },
        act: Do::Notify { text: text.into() },
        max_runs: None,
        runs: 0,
        since: 0,
        last: None,
        opened: None,
        cooldown: COOLDOWN,
    }
}

/// A rule that runs `act` `when`, on, counted from now.
fn rule(when: When, act: Do) -> Automation {
    Automation {
        when,
        act,
        since: unix(SystemTime::now()),
        ..notify("")
    }
}

fn every(minutes: u32) -> When {
    When::Every { minutes }
}

fn say(text: &str) -> Do {
    Do::Notify { text: text.into() }
}

/// Adds `a`; returns its id.
fn add(ws: &Entity<Workspace>, cx: &mut TestAppContext, a: Automation) -> u64 {
    ws.update(cx, |ws, cx| {
        let id = ws.automations.add(a);
        ws.look_at_automations(cx);
        id
    })
}

/// Its time came `secs` ago (the next look runs it): the way headless tests make a rule due.
fn due_since(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64, secs: u64) {
    let past = SystemTime::now() - Duration::from_secs(secs);
    ws.update(cx, |ws, _| {
        ws.automation_runs.entry(id).or_default().next = Some(past)
    });
}

fn due(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64) {
    due_since(ws, cx, id, 1);
}

/// One look of the automations.
fn look(cx: &mut TestAppContext) {
    cx.executor().advance_clock(AUTOMATION_LOOK);
    cx.run_until_parked();
}

fn last(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64) -> (Went, String) {
    ws.read_with(cx, |ws, _| {
        let last = ws
            .automations
            .get(id)
            .unwrap()
            .last
            .clone()
            .expect("it ran");
        (last.went, last.about)
    })
}

/// One snapshot period, with the background write finished.
fn tick(cx: &mut TestAppContext) {
    cx.executor().advance_clock(crate::session::EVERY);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_automations_are_saved_with_the_snapshot(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (_, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    ws.update(cx, |ws, _| ws.automations.add(notify("Stand-up")));
    tick(cx);
    let text = std::fs::read_to_string(crate::automations::path(dir.path())).unwrap();
    assert!(text.contains("Stand-up"), "{text}");
    // A restart reads it back.
    let (_, again) = open_in(cx, None, Some(dir.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        let saved = &ws.automations.automations;
        assert_eq!(saved.len(), 1);
        assert_eq!(
            saved[0].act,
            Do::Notify {
                text: "Stand-up".into()
            }
        );
    });
}

#[gpui_kit::test]
fn a_broken_automations_file_is_set_aside(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(crate::automations::path(dir.path()), "{").unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    assert!(dir.path().join("automations.json.bak").exists());
    assert_eq!(notification_count(handle, cx), 1);
    ws.read_with(cx, |ws, _| assert!(ws.automations.automations.is_empty()));
    ws.update(cx, |ws, cx| {
        ws.automations.add(notify("A"));
        ws.save_canvas_now(cx);
    });
    assert!(crate::automations::path(dir.path()).exists(), "a new file");
}

#[gpui_kit::test]
fn an_unreadable_automations_file_freezes_the_list(cx: &mut TestAppContext) {
    // A folder in the file's place can't be read, nor set aside.
    let dir = tempfile::tempdir().unwrap();
    let path = crate::automations::path(dir.path());
    std::fs::create_dir(&path).unwrap();
    assert!(matches!(
        crate::automations::load(dir.path()),
        Loaded::Unreadable(_)
    ));
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    assert_eq!(notification_count(handle, cx), 1);
    ws.update(cx, |ws, cx| {
        ws.automations.add(notify("A"));
        ws.save_canvas_now(cx);
    });
    tick(cx);
    assert!(path.is_dir(), "nothing was written over it");
}

#[gpui_kit::test]
fn notify_me_sends_its_notice_to_the_api(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let named = add(
        &ws,
        cx,
        Automation {
            name: "Morning review".into(),
            workspace: Some(first.clone()),
            ..notify("Look")
        },
    );
    due(&ws, cx, named);
    look(cx);
    assert_eq!(last(&ws, cx, named), (Went::Ran, "Notified.".into()));
    let unnamed = add(&ws, cx, rule(every(10), say("Again")));
    due(&ws, cx, unnamed);
    look(cx);
    let notices = ws.read_with(cx, |ws, _| ws.api.notices.clone());
    assert_eq!(notices.len(), 2, "{notices:?}");
    assert_eq!(
        notices[0],
        templum_api::model::Notice {
            text: "Morning review: Look".into(),
            workspace: Some(first),
            block: None,
            automation: Some(named),
        }
    );
    assert!(notices[1].text.ends_with(": Again"), "{}", notices[1].text);
    assert_eq!(notices[1].automation, Some(unnamed));
}

#[gpui_kit::test]
fn a_new_rule_saved_off_through_the_api_stays_off(cx: &mut TestAppContext) {
    use super::api::{said, tool, with_token};
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let full = with_token(&ws, cx, templum_api::store::Scope::Full, None);
    let rule = |on: Option<bool>| {
        let mut rule = json!({ "name": "Copy", "workspace": first,
            "when": { "daily": { "at": "09:00:00" } }, "do": { "notify": { "text": "x" } } });
        if let Some(on) = on {
            rule["on"] = json!(on);
        }
        json!({ "automation": rule })
    };
    let line = tool(handle, &ws, cx, &full, "save_automation", rule(Some(false)));
    assert!(!said(&line).0, "{line}");
    let line = tool(handle, &ws, cx, &full, "save_automation", rule(None));
    assert!(!said(&line).0, "{line}");
    ws.read_with(cx, |ws, _| {
        let (off, on) = (
            &ws.automations.automations[0],
            &ws.automations.automations[1],
        );
        assert!(!off.on && on.on);
        assert_eq!(ws.automation_runs[&off.id].next, None);
        assert!(ws.automation_runs[&on.id].next.is_some());
    });
}

#[gpui_kit::test]
fn a_due_automation_notifies_and_says_it_ran(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = add(&ws, cx, rule(every(10), say("Stand-up")));
    let before = notification_count(handle, cx);
    due(&ws, cx, id);
    look(cx);
    assert_eq!(notification_count(handle, cx), before + 1);
    assert_eq!(last(&ws, cx, id), (Went::Ran, "Notified.".into()));
    let ahead = ws.read_with(cx, |ws, _| ws.automation_runs[&id].next.unwrap());
    let ahead = ahead.duration_since(SystemTime::now()).unwrap();
    assert!(ahead > Duration::from_secs(9 * 60) && ahead <= Duration::from_secs(10 * 60));
    // Not again until its next time.
    look(cx);
    assert_eq!(notification_count(handle, cx), before + 1);
}

#[gpui_kit::test]
fn a_time_missed_while_closed_is_missed_not_run(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let days_ago = |n: i64| Local::now() - chrono::Duration::days(n);
    let midnight = When::Daily {
        at: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
    };
    let yesterday = When::Once {
        at: days_ago(1).naive_local(),
    };
    let mut list = Automations::default();
    for (when, since) in [(midnight, days_ago(2)), (yesterday, days_ago(3))] {
        let mut a = rule(when, say("Late"));
        a.since = since.timestamp();
        list.add(a);
    }
    crate::automations::save(dir.path(), &list, 1).unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    assert_eq!(
        notification_count(handle, cx),
        0,
        "a missed time doesn't run"
    );
    for id in [1, 2] {
        assert_eq!(
            last(&ws, cx, id),
            (Went::Missed, "Templum was closed.".into())
        );
    }
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.get(1).unwrap().on);
        assert!(
            !ws.automations.get(2).unwrap().on,
            "a Once that was missed is done"
        );
        assert!(ws.automation_runs[&1].next.unwrap() > SystemTime::now());
        assert_eq!(ws.automations_unseen(), 2);
    });
}

#[gpui_kit::test]
fn a_sleep_runs_a_missed_time_once(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = add(&ws, cx, rule(every(60), say("Hourly")));
    let before = notification_count(handle, cx);
    due_since(&ws, cx, id, 3 * 60 * 60);
    look(cx);
    look(cx);
    assert_eq!(notification_count(handle, cx), before + 1);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automation_runs[&id].next.unwrap() > SystemTime::now())
    });
}

#[gpui_kit::test]
fn stop_after_turns_it_off(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let mut twice = rule(every(10), say("Twice"));
    twice.max_runs = Some(2);
    let id = add(&ws, cx, twice);
    let before = notification_count(handle, cx);
    for _ in 0..2 {
        due(&ws, cx, id);
        look(cx);
    }
    ws.read_with(cx, |ws, _| {
        let a = ws.automations.get(id).unwrap();
        assert_eq!((a.runs, a.on), (2, false));
        assert_eq!(
            a.last.as_ref().unwrap().about,
            "Notified. Ran 2 of 2 times, now off."
        );
        assert_eq!(ws.next_words(id, SystemTime::now()), "Off");
    });
    due(&ws, cx, id);
    look(cx);
    assert_eq!(
        notification_count(handle, cx),
        before + 2,
        "a third look runs nothing"
    );
}

#[gpui_kit::test]
fn run_now_runs_an_off_automation_without_counting(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let mut off = rule(every(10), say("Now"));
    off.on = false;
    let id = add(&ws, cx, off);
    let before = notification_count(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(id, window, cx))
    })
    .unwrap();
    assert_eq!(notification_count(handle, cx), before + 1);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(id).unwrap().runs, 0);
        assert!(
            ws.automation_runs
                .get(&id)
                .is_none_or(|run| run.next.is_none())
        );
    });
}

#[gpui_kit::test]
fn a_missing_target_is_skipped_and_counted_unseen(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let prompt = Do::Prompt {
        block: "zzzzz".into(),
        text: "Hi".into(),
    };
    let id = add(&ws, cx, rule(every(10), prompt));
    let said = "Target missing: zzzzz isn't open";
    ws.read_with(cx, |ws, _| {
        let a = ws.automations.get(id).unwrap();
        assert_eq!(ws.automation_target(a), Err(said.to_string()));
    });
    due(&ws, cx, id);
    look(cx);
    assert_eq!(last(&ws, cx, id), (Went::Skipped, said.into()));
    ws.read_with(cx, |ws, _| assert_eq!(ws.automations_unseen(), 1));
}

#[gpui_kit::test]
fn a_stopped_workspace_skips_its_automations_until_it_starts(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.update(cx, |ws, _| ws.make_space());
    let name = ws.read_with(cx, |ws, _| {
        ws.spaces().find(|s| s.id == space).unwrap().name.clone()
    });
    let mut there = rule(every(10), say("There"));
    there.workspace = Some(space.clone());
    let id = add(&ws, cx, there);
    let before = notification_count(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.stop_now(&space, window, cx))
    })
    .unwrap();
    due(&ws, cx, id);
    look(cx);
    assert_eq!(
        last(&ws, cx, id),
        (Went::Skipped, format!("Target missing: {name} is stopped"))
    );
    assert_eq!(notification_count(handle, cx), before, "nothing ran");
    // Showing it starts it.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let at = ws.order().iter().position(|s| s.id == space).unwrap();
            ws.switch_to(at, window, cx)
        })
    })
    .unwrap();
    due(&ws, cx, id);
    look(cx);
    assert_eq!(last(&ws, cx, id), (Went::Ran, "Notified.".into()));
    assert_eq!(notification_count(handle, cx), before + 1);
}

#[gpui_kit::test]
fn an_unreadable_cron_never_runs_and_says_never(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut list = Automations::default();
    list.add(rule(
        When::Cron {
            expr: "0 25 * * *".into(),
        },
        say("Never"),
    ));
    list.add(rule(every(10), say("Often")));
    crate::automations::save(dir.path(), &list, 1).unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let before = notification_count(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automation_runs[&1].next, None);
        assert_eq!(ws.next_words(1, SystemTime::now()), "Never");
    });
    due(&ws, cx, 2);
    look(cx);
    assert_eq!(notification_count(handle, cx), before + 1);
    assert_eq!(last(&ws, cx, 2).0, Went::Ran);
}

/// A rule that types `text` into the agent block `uid`, named `name`.
#[cfg(unix)]
fn prompt(name: &str, uid: &str, text: &str) -> Automation {
    let act = Do::Prompt {
        block: uid.into(),
        text: text.into(),
    };
    Automation {
        name: name.into(),
        ..rule(every(10), act)
    }
}

/// The run of `id`: why it is held, if it is.
#[cfg(unix)]
fn held(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64) -> Option<String> {
    ws.read_with(cx, |ws, _| {
        let run = ws.automation_runs.get(&id)?;
        run.held().map(|(_, held)| held.why.clone())
    })
}

/// What the agent block `id` shows: its rows joined.
#[cfg(unix)]
fn shown(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> String {
    screen_until(ws, cx, id, |_| true)
}

/// The agent took the task Templum typed into block `id`: its Enter went and its screen moved,
/// as `hand_over` waits for, and the rule's run ended.
#[cfg(unix)]
fn taken(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId, rule: u64) {
    entered(ws, cx, id);
    ended_run(ws, cx, rule);
}

/// Waits until the rule's last run is written.
#[cfg(unix)]
fn ended_run(ws: &Entity<Workspace>, cx: &mut TestAppContext, rule: u64) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while ws.read_with(cx, |ws, _| ws.automations.get(rule).unwrap().last.is_none()) {
        assert!(std::time::Instant::now() < deadline, "not taken");
        std::thread::sleep(Duration::from_millis(10));
        super::bus::after(
            cx,
            crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
        );
    }
}

/// `taken`, for an agent whose hooks have spoken (`begins`, `ends`): they tell it submitted.
#[cfg(unix)]
fn taken_by_hook(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
    terminal: &str,
    rule: u64,
) {
    entered(ws, cx, id);
    super::bus::event(
        handle,
        ws,
        cx,
        terminal,
        serde_json::json!({ "event": "submitted" }),
    );
    ended_run(ws, cx, rule);
}

/// What Templum typed has been confirmed and the typing delays are over, as after a few seconds.
#[cfg(unix)]
fn rested(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) {
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(id).unwrap();
        pane.sent = None;
        pane.draft.set(false);
        pane.keyed.set(None);
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_prompt_is_handed_as_a_task_from_the_automation(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(
        &ws,
        cx,
        prompt("Morning review", &uid, "Review the open pull requests."),
    );
    due(&ws, cx, rule);
    look(cx);
    history_with(
        &ws,
        cx,
        id,
        "[from automation \"Morning review\", task t1] Your task is in the pasted text: do it, then answer it with reply.",
    );
    taken(&ws, cx, id, rule);
    assert_eq!(
        last(&ws, cx, rule),
        (Went::Ran, format!("Typed into {uid} as task t1."))
    );
    ws.read_with(cx, |ws, _| {
        let handed = ws
            .space
            .terminal(id)
            .unwrap()
            .handed
            .as_ref()
            .expect("on a task");
        assert_eq!(handed.by, "automation-1");
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_prompt_waits_while_its_agent_works(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    begins(handle, &ws, cx, &terminal);
    let before = shown(&ws, cx, id);
    due(&ws, cx, rule);
    look(cx);
    assert_eq!(
        held(&ws, cx, rule),
        Some(format!("Waiting for {uid}: it is working"))
    );
    assert_eq!(shown(&ws, cx, id), before, "nothing typed");
    ends(handle, &ws, cx, &terminal);
    rested(&ws, cx, id);
    look(cx);
    assert_eq!(held(&ws, cx, rule), None);
    history_with(&ws, cx, id, "Your task is in the pasted text");
    // A run held for an hour is given up.
    let late = add(&ws, cx, prompt("", &uid, "Again"));
    begins(handle, &ws, cx, &terminal);
    ws.update(cx, |ws, _| {
        let since = SystemTime::now() - Duration::from_secs(61 * 60);
        let held = crate::workspace::automations::Held {
            since,
            why: "Waiting".into(),
            counts: true,
        };
        ws.automation_runs.entry(late).or_default().slot(None).held = Some(held);
    });
    look(cx);
    assert_eq!(
        last(&ws, cx, late),
        (Went::Skipped, format!("{uid} stayed busy for an hour."))
    );
    assert_eq!(held(&ws, cx, late), None);
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_prompt_waits_while_its_person_types(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    // Their half-written message is in the agent's input.
    ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().draft.set(true));
    let before = shown(&ws, cx, id);
    due(&ws, cx, rule);
    look(cx);
    assert_eq!(
        held(&ws, cx, rule),
        Some(format!("Waiting for {uid}: you are typing in it"))
    );
    assert_eq!(
        shown(&ws, cx, id),
        before,
        "only their text is on the screen"
    );
    // They sent it.
    rested(&ws, cx, id);
    look(cx);
    history_with(&ws, cx, id, "from automation");
}

#[cfg(unix)]
#[gpui_kit::test]
fn two_prompts_for_one_agent_take_turns(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let first = add(&ws, cx, prompt("One", &uid, "First"));
    let second = add(&ws, cx, prompt("Two", &uid, "Second"));
    due(&ws, cx, first);
    due(&ws, cx, second);
    look(cx);
    history_with(&ws, cx, id, "from automation \"One\", task t1]");
    let why = held(&ws, cx, second).expect("the second waits");
    assert!(
        why == format!("Waiting for {uid}: it is on a task")
            || why == format!("Waiting for {uid}: Templum is typing into it"),
        "{why}"
    );
    assert!(
        !shown(&ws, cx, id).contains("Two"),
        "never two pastes into one input"
    );
    taken(&ws, cx, id, first);
    // The first task ends with its turn.
    begins(handle, &ws, cx, &terminal);
    ends(handle, &ws, cx, &terminal);
    rested(&ws, cx, id);
    look(cx);
    history_with(&ws, cx, id, "from automation \"Two\", task t2]");
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_prompt_never_goes_into_a_shell(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    // A terminal that runs no agent.
    let id = super::inbox::open_terminal(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "rm -rf /"));
    let before = shown(&ws, cx, id);
    due(&ws, cx, rule);
    look(cx);
    assert_eq!(
        last(&ws, cx, rule),
        (
            Went::Failed,
            format!("{uid} runs no agent now: a prompt is never typed into a shell.")
        )
    );
    assert_eq!(shown(&ws, cx, id), before);
}

/// A rule that runs `text` in a new terminal in `folder`, in the workspace `space`.
#[cfg(unix)]
fn command(space: &str, folder: &std::path::Path, text: &str) -> Automation {
    let act = Do::Command {
        terminal: None,
        folder: Some(folder.display().to_string()),
        text: text.into(),
    };
    Automation {
        workspace: Some(space.into()),
        ..rule(every(10), act)
    }
}

/// How many terminal blocks the shown workspace has.
#[cfg(unix)]
fn terminals(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> usize {
    ws.read_with(cx, |ws, _| ws.space.terminals().count())
}

/// The terminal block uid of the rule `id`'s kept terminal.
#[cfg(unix)]
fn kept(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64) -> String {
    ws.read_with(cx, |ws, _| {
        ws.automations
            .get(id)
            .unwrap()
            .opened
            .clone()
            .expect("kept one")
    })
}

/// Waits (real time, with the test's clock moved on) until the rule `id`'s last run says `about`.
#[cfg(unix)]
fn says(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64, about: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while last_about(ws, cx, id).as_deref() != Some(about) {
        assert!(
            std::time::Instant::now() < deadline,
            "{:?}",
            last_about(ws, cx, id)
        );
        std::thread::sleep(Duration::from_millis(10));
        super::bus::after(
            cx,
            crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
        );
    }
}

#[cfg(unix)]
fn last_about(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64) -> Option<String> {
    ws.read_with(cx, |ws, _| {
        ws.automations
            .get(id)
            .unwrap()
            .last
            .as_ref()
            .map(|l| l.about.clone())
    })
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_command_opens_a_terminal_and_reuses_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let folder = tempfile::tempdir().unwrap();
    let id = add(&ws, cx, command(&space, folder.path(), "echo automated"));
    let blocks = terminals(&ws, cx);
    due(&ws, cx, id);
    look(cx);
    assert_eq!(terminals(&ws, cx), blocks + 1);
    let uid = kept(&ws, cx, id);
    let (_, opened) = last(&ws, cx, id);
    assert!(opened.starts_with(&format!("Opened {uid} in ")), "{opened}");
    let at = ws.read_with(cx, |ws, _| ws.find_block(&uid).unwrap().1);
    screen_until(&ws, cx, at, |shown| shown.contains("echo automated"));
    // Its next run goes to the same terminal.
    due(&ws, cx, id);
    look(cx);
    says(&ws, cx, id, &format!("Typed into {uid}."));
    assert_eq!(terminals(&ws, cx), blocks + 1);
    // Another folder is not the terminal the old one kept: saved so, the next run opens a new one.
    let other = tempfile::tempdir().unwrap();
    let to_other = Do::Command {
        terminal: None,
        folder: Some(other.path().display().to_string()),
        text: "echo automated".into(),
    };
    ws.update(cx, |ws, cx| {
        ws.edit_automation(id, |a| a.act = to_other, cx);
        ws.save_automation(id, cx).unwrap();
    });
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.automations.get(id).unwrap().opened.clone()),
        None
    );
    let blocks_now = terminals(&ws, cx);
    due(&ws, cx, id);
    look(cx);
    assert_eq!(terminals(&ws, cx), blocks_now + 1, "a new terminal opened");
    assert_ne!(kept(&ws, cx, id), uid);
    let uid = kept(&ws, cx, id);
    let at = ws.read_with(cx, |ws, _| ws.find_block(&uid).unwrap().1);
    // Closed, a new one opens.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.block_close((&space, at), &uid, "", window, cx)
        })
    })
    .unwrap();
    let after_close = terminals(&ws, cx);
    due(&ws, cx, id);
    look(cx);
    assert_eq!(terminals(&ws, cx), after_close + 1);
    assert_ne!(kept(&ws, cx, id), uid);
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_command_waits_for_its_terminal(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let act = Do::Command {
        terminal: Some(uid.clone()),
        folder: None,
        text: "make test".into(),
    };
    let rule = add(&ws, cx, rule(every(10), act));
    let before = shown(&ws, cx, id);
    due(&ws, cx, rule);
    look(cx);
    assert_eq!(
        held(&ws, cx, rule),
        Some(format!("Waiting for {uid}: an agent runs in it"))
    );
    assert_eq!(shown(&ws, cx, id), before, "nothing typed");
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_missing_folder_fails(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let folder = tempfile::tempdir().unwrap();
    let id = add(&ws, cx, command(&space, &folder.path().join("gone"), "ls"));
    let blocks = terminals(&ws, cx);
    due(&ws, cx, id);
    look(cx);
    let (went, about) = last(&ws, cx, id);
    assert_eq!(went, Went::Failed);
    assert!(about.starts_with("No folder "), "{about}");
    assert_eq!(terminals(&ws, cx), blocks);
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_kept_terminal_running_an_agent_gets_no_command(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let folder = tempfile::tempdir().unwrap();
    let id = add(&ws, cx, command(&space, folder.path(), "echo automated"));
    due(&ws, cx, id);
    look(cx);
    let first = kept(&ws, cx, id);
    let at = ws.read_with(cx, |ws, _| ws.find_block(&first).unwrap().1);
    // The tty's echo and `cat`'s answer, both, so the screen holds still.
    screen_until(&ws, cx, at, |shown| {
        shown.matches("echo automated").count() >= 2
    });
    // The person started an agent in it since.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(at).unwrap().seen.agent = Some("claude".into())
    });
    super::delegate::found(&ws, cx, at);
    let before = shown(&ws, cx, at);
    let blocks = terminals(&ws, cx);
    due(&ws, cx, id);
    look(cx);
    assert_eq!(terminals(&ws, cx), blocks + 1, "another terminal opened");
    assert_ne!(kept(&ws, cx, id), first);
    assert_eq!(shown(&ws, cx, at), before, "the agent was typed nothing");
    let _ = handle;
}

#[cfg(unix)]
#[gpui_kit::test]
fn start_an_agent_opens_it_with_its_prompt(cx: &mut TestAppContext) {
    let (_, ws, mine) = spawner(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let folder = tempfile::tempdir().unwrap();
    let start = |effort: Option<&str>| {
        let act = Do::Agent {
            agent: "claude".into(),
            folder: Some(folder.path().display().to_string()),
            model: None,
            effort: effort.map(str::to_string),
            prompt: Some("Triage the new issues".into()),
        };
        Automation {
            workspace: Some(space.clone()),
            ..rule(every(10), act)
        }
    };
    let id = add(&ws, cx, start(None));
    due(&ws, cx, id);
    look(cx);
    let (went, about) = last(&ws, cx, id);
    assert!(about.starts_with("Opened Claude Code as "), "{about}");
    assert_eq!(went, Went::Ran);
    ws.read_with(cx, |ws, _| {
        let (_, pane) = ws
            .space
            .terminals()
            .find(|(block, _)| *block != mine)
            .expect("opened");
        assert!(pane.prompt.is_some(), "its prompt waits for it to be ready");
        assert_eq!(pane.handed.as_ref().unwrap().by, "automation-1");
    });
    // An effort it doesn't know: refused with the words `spawn_agent` gives.
    let refused = ws.read_with(cx, |ws, _| {
        let entry = ws.agents.iter().find(|a| a.id == "claude").unwrap();
        entry
            .launch_args_with(None, Some("turbo"), None)
            .unwrap_err()
    });
    let bad = add(&ws, cx, start(Some("turbo")));
    due(&ws, cx, bad);
    look(cx);
    assert_eq!(last(&ws, cx, bad), (Went::Failed, refused));
}

#[cfg(target_os = "macos")]
const AUTOMATIONS: &str = "cmd-shift-j";
#[cfg(not(target_os = "macos"))]
const AUTOMATIONS: &str = "ctrl-shift-j";
#[cfg(target_os = "macos")]
const ALL_AGENTS: &str = "cmd-shift-l";
#[cfg(not(target_os = "macos"))]
const ALL_AGENTS: &str = "ctrl-shift-l";

/// The popup open and drawn.
fn open_popup(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_automations(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

fn is_open(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    ws.read_with(cx, |ws, _| ws.automations_open)
}

#[gpui_kit::test]
fn one_popup_of_four_is_open(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    press_key(handle, cx, AUTOMATIONS);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations_open && !ws.agents_open && !ws.git_open && !ws.inbox_open)
    });
    press_key(handle, cx, ALL_AGENTS);
    ws.read_with(cx, |ws, _| assert!(ws.agents_open && !ws.automations_open));
    click_id(handle, cx, name("automations-button".into()));
    ws.read_with(cx, |ws, _| assert!(ws.automations_open && !ws.agents_open));
    press_key(handle, cx, AUTOMATIONS);
    assert!(!is_open(&ws, cx));
    // The inbox and Git close it as they open.
    open_popup(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_inbox(None, window, cx))
    })
    .unwrap();
    assert!(!is_open(&ws, cx));
    open_popup(handle, &ws, cx);
    open_agents(handle, &ws, cx);
    assert!(!is_open(&ws, cx));
}

#[gpui_kit::test]
fn rows_say_the_next_and_the_last_run(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let now = SystemTime::now();
    let mut off = rule(every(10), say("Off"));
    off.on = false;
    let off = add(&ws, cx, off);
    let past = add(
        &ws,
        cx,
        rule(
            When::Once {
                at: (Local::now() - chrono::Duration::days(1)).naive_local(),
            },
            say("Once"),
        ),
    );
    // Today at 23:59, unless it is later than that.
    let late = Local::now().date_naive().and_hms_opt(23, 59, 0).unwrap();
    let late_rule = add(&ws, cx, rule(When::Once { at: late }, say("Late")));
    ws.update(cx, |ws, _| {
        ws.automation_runs.insert(
            late_rule,
            crate::workspace::automations::Run {
                next: Some(SystemTime::from(late.and_local_timezone(Local).unwrap())),
                ..Default::default()
            },
        );
        ws.automation_runs.insert(past, Default::default());
    });
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.next_words(off, now), "Off");
        assert_eq!(ws.next_words(past, now), "Never");
        assert_eq!(ws.next_words(late_rule, now), "23:59");
        assert_eq!(
            ws.last_words(off, now),
            None,
            "nothing before the first run"
        );
    });
    // A failed run now: its time, with the word.
    ws.update(cx, |ws, _| {
        ws.automations.get_mut(off).unwrap().last = Some(crate::automations::LastRun {
            at: unix(now),
            went: Went::Failed,
            about: "No folder x.".into(),
        });
    });
    let clock = chrono::DateTime::<Local>::from(now)
        .format("%H:%M")
        .to_string();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.last_words(off, now), Some(format!("Failed {clock}")))
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_held_prompt_says_who_it_waits_for(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    begins(handle, &ws, cx, &terminal);
    due(&ws, cx, rule);
    look(cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.last_words(rule, SystemTime::now()),
            Some(format!("Waiting for {uid}"))
        )
    });
}

#[gpui_kit::test]
fn the_badge_counts_unseen_runs_until_the_popup_opens(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    super::terminals::set_active(handle, true, cx);
    let prompt = Do::Prompt {
        block: "zzzzz".into(),
        text: "Hi".into(),
    };
    let id = add(&ws, cx, rule(every(10), prompt));
    due(&ws, cx, id);
    look(cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.automations_unseen(), 1));
    assert!(drawn(handle, cx, name("automations-badge".into())));
    open_popup(handle, &ws, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.automations_unseen(), 0));
    assert!(!drawn(handle, cx, name("automations-badge".into())));
}

#[cfg(unix)]
#[gpui_kit::test]
fn asked_by_agents_lists_schedules_and_cancels_them(cx: &mut TestAppContext) {
    use super::spawn::spawner;
    let (handle, ws, mine) = spawner(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.agent = Some("claude".into())
    });
    let (failed, said) = super::bus::call(
        handle,
        &ws,
        cx,
        &terminal,
        "schedule",
        serde_json::json!({ "block": uid, "text": "tick", "every": 1 }),
    );
    assert!(!failed, "{said}");
    open_popup(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        let rows = ws.schedule_rows();
        assert_eq!(rows.len(), 1);
        assert!(rows[0].1.starts_with("s1 from "), "{}", rows[0].1);
        assert!(rows[0].1.ends_with("to itself"), "{}", rows[0].1);
        assert!(
            rows[0].2.starts_with("every 1 min · next "),
            "{}",
            rows[0].2
        );
    });
    assert!(drawn(handle, cx, name("schedule-cancel-1".into())));
    click_id(handle, cx, name("schedule-cancel-1".into()));
    ws.read_with(cx, |ws, _| assert!(ws.schedules.is_empty()));
}

#[gpui_kit::test]
fn duplicate_is_off_and_delete_asks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let mut named = rule(every(10), say("A"));
    named.name = "A".into();
    let id = add(&ws, cx, named);
    open_popup(handle, &ws, cx);
    let copy = ws
        .update(cx, |ws, cx| ws.duplicate_automation(id, cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        let copy = ws.automations.get(copy).unwrap();
        assert_eq!((copy.name.as_str(), copy.on), ("A (copy)", false));
        assert_eq!(ws.automation_selected(), Some(copy.id), "selected");
    });
    // Delete asks first; confirming removes the rule and its run.
    ws.update(cx, |ws, _| {
        ws.automation_runs.insert(id, Default::default())
    });
    use gpui_kit::component::WindowExt as _;
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.ask_delete_automation(id, window, cx));
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "it asks");
    })
    .unwrap();
    // Pressed until it takes: a dialog still opening ignores a press.
    for _ in 0..50 {
        cx.update_window(handle, |_, window, cx| {
            window.within("dialog").click("ok", cx);
            window.render_frame(cx);
        })
        .unwrap();
        if ws.read_with(cx, |ws, _| ws.automations.get(id).is_none()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        cx.run_until_parked();
    }
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.get(id).is_none(), "the rule is gone");
        assert!(!ws.automation_runs.contains_key(&id), "and its run");
        assert!(ws.automations.get(copy).is_some());
    });
}

#[gpui_kit::test]
fn arrows_select_and_esc_closes(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, b) = (
        add(&ws, cx, rule(every(10), say("A"))),
        add(&ws, cx, rule(every(10), say("B"))),
    );
    open_popup(handle, &ws, cx);
    let selected = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.automation_selected())
    };
    assert_eq!(selected(&ws, cx), Some(a));
    press_key(handle, cx, "up");
    assert_eq!(selected(&ws, cx), Some(a), "none before the first");
    press_key(handle, cx, "down");
    assert_eq!(selected(&ws, cx), Some(b));
    press_key(handle, cx, "down");
    assert_eq!(selected(&ws, cx), Some(b), "none past the last");
    press_key(handle, cx, "up");
    assert_eq!(selected(&ws, cx), Some(a));
    press_key(handle, cx, "escape");
    assert!(!is_open(&ws, cx));
}

/// A long name and a long When are cut at the list's edge: the row never reaches the editor.
#[gpui_kit::test]
fn a_row_keeps_to_the_lists_width(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let mut long = rule(
        When::Cron {
            expr: "0 9 * * 1-5".repeat(20),
        },
        say("A"),
    );
    long.name = "An automation with a very long name ".repeat(10);
    let id = add(&ws, cx, long);
    open_popup(handle, &ws, cx);
    let width = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window
                .try_find(name(format!("automation-row-{id}")))
                .expect("its row")
                .bounds()
                .size
                .width
        })
        .unwrap();
    assert!(width <= px(crate::agents_view::LIST_W), "{width:?}");
}

// The editor.

fn draft_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64) -> Automation {
    ws.read_with(cx, |ws, _| ws.automation_draft(id).unwrap().clone())
}

/// Saving `a` as a draft of `id` and what Save says.
fn saves(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64) -> Result<u64, String> {
    ws.update(cx, |ws, cx| ws.save_automation(id, cx))
}

#[gpui_kit::test]
fn save_checks_the_draft(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let seven = NaiveTime::from_hms_opt(7, 0, 0).unwrap();
    let base = |act: Do| Automation {
        workspace: Some(space.clone()),
        ..rule(When::Daily { at: seven }, act)
    };
    let id = add(&ws, cx, base(say("ok")));
    let problem = |ws: &Entity<Workspace>,
                   cx: &mut TestAppContext,
                   edit: Box<dyn FnOnce(&mut Automation)>| {
        ws.update(cx, |ws, cx| ws.edit_automation(id, edit, cx));
        let found = saves(ws, cx, id);
        ws.update(cx, |ws, cx| ws.revert_automation(id, cx));
        found
    };
    let set = |act: Do| -> Box<dyn FnOnce(&mut Automation)> { Box::new(move |a| a.act = act) };
    let prompt = |block: &str, text: &str| Do::Prompt {
        block: block.into(),
        text: text.into(),
    };
    let command = |terminal: Option<&str>, text: &str| Do::Command {
        terminal: terminal.map(str::to_string),
        folder: None,
        text: text.into(),
    };
    let agent = |agent: &str, effort: Option<&str>| Do::Agent {
        agent: agent.into(),
        folder: None,
        model: None,
        effort: effort.map(str::to_string),
        prompt: None,
    };
    type Edit = Box<dyn FnOnce(&mut Automation)>;
    let cases: Vec<(Edit, String)> = vec![
        (
            Box::new(|a| a.name = "x".repeat(61)),
            "A name is at most 60 characters.".into(),
        ),
        (
            Box::new(|a| {
                a.when = When::Weekly {
                    days: vec![],
                    at: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
                }
            }),
            "Choose at least one weekday.".into(),
        ),
        (
            Box::new(|a| {
                a.when = When::Cron {
                    expr: "0 25 * * *".into(),
                }
            }),
            "Hour: 25 is past 23.".into(),
        ),
        (set(prompt("", "go")), "Choose an agent.".into()),
        (set(prompt("abc12", " ")), "Write the prompt.".into()),
        (set(command(Some(""), "ls")), "Choose a terminal.".into()),
        (set(command(None, " ")), "Write the command.".into()),
        (set(agent("", None)), "Choose an agent.".into()),
        (set(say(" ").clone()), "Write the text.".into()),
        (
            Box::new(|a| {
                a.workspace = None;
                a.act = Do::Command {
                    terminal: None,
                    folder: None,
                    text: "ls".into(),
                };
            }),
            "Choose the workspace it opens in.".into(),
        ),
    ];
    for (edit, said) in cases {
        assert_eq!(problem(&ws, cx, edit), Err(said.clone()), "{said}");
    }
    // What `spawn_agent` says of an effort it doesn't know.
    let refused = ws.read_with(cx, |ws, _| {
        ws.agents
            .iter()
            .find(|a| a.id == "claude")
            .unwrap()
            .launch_args_with(None, Some("turbo"), None)
            .unwrap_err()
    });
    assert_eq!(
        problem(&ws, cx, set(agent("claude", Some("turbo")))),
        Err(refused)
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.automations.get(id).unwrap().act,
            say("ok"),
            "nothing was saved"
        )
    });
    // A good one saves.
    ws.update(cx, |ws, cx| {
        ws.edit_automation(id, |a| a.act = say("better"), cx)
    });
    assert_eq!(saves(&ws, cx, id), Ok(id));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(id).unwrap().act, say("better"))
    });
}

#[gpui_kit::test]
fn new_is_added_on_when_saved_and_revert_removes_it(cx: &mut TestAppContext) {
    use crate::workspace::automations::NEW;
    let (_, ws) = open(cx);
    ws.update(cx, |ws, cx| ws.new_automation(cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automation_selected(), Some(NEW));
        assert_eq!(ws.automation_ids(), [NEW]);
        assert!(ws.automations.automations.is_empty(), "not saved yet");
    });
    ws.update(cx, |ws, cx| ws.revert_automation(NEW, cx));
    ws.read_with(cx, |ws, _| assert!(ws.automation_ids().is_empty()));
    ws.update(cx, |ws, cx| {
        ws.new_automation(cx);
        ws.edit_automation(NEW, |a| a.act = say("Hi"), cx);
    });
    let id = saves(&ws, cx, NEW).unwrap();
    assert_ne!(id, NEW);
    ws.read_with(cx, |ws, _| {
        let a = ws.automations.get(id).unwrap();
        assert!(a.on && a.since > 0);
        assert!(ws.automation_runs[&id].next.is_some());
        assert!(!ws.automation_drafts.contains_key(&NEW));
        assert_eq!(ws.automation_selected(), Some(id));
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn saving_counts_from_now_and_drops_a_held_run(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    begins(handle, &ws, cx, &terminal);
    ws.update(cx, |ws, _| {
        let a = ws.automations.get_mut(rule).unwrap();
        (a.runs, a.since) = (3, 1);
    });
    due(&ws, cx, rule);
    look(cx);
    assert!(held(&ws, cx, rule).is_some());
    ws.update(cx, |ws, cx| {
        ws.edit_automation(
            rule,
            |a| {
                a.act = Do::Prompt {
                    block: uid.clone(),
                    text: "New prompt".into(),
                }
            },
            cx,
        )
    });
    saves(&ws, cx, rule).unwrap();
    assert_eq!(held(&ws, cx, rule), None);
    ws.read_with(cx, |ws, _| {
        let a = ws.automations.get(rule).unwrap();
        assert_eq!(a.runs, 0);
        assert!(a.since > 1_000_000);
    });
}

#[gpui_kit::test]
fn a_draft_stays_while_another_row_is_selected(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, b) = (
        add(&ws, cx, rule(every(10), say("A"))),
        add(&ws, cx, rule(every(10), say("B"))),
    );
    ws.update(cx, |ws, cx| {
        ws.edit_automation(a, |x| x.name = "Renamed".into(), cx)
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.select_automation(b, window, cx);
            ws.select_automation(a, window, cx);
        })
    })
    .unwrap();
    assert_eq!(draft_of(&ws, cx, a).name, "Renamed");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(a).unwrap().name, "")
    });
    ws.update(cx, |ws, cx| ws.revert_automation(a, cx));
    assert_eq!(draft_of(&ws, cx, a).name, "");
    // Edited back to what is saved, it is no draft.
    ws.update(cx, |ws, cx| {
        ws.edit_automation(a, |x| x.name = "Again".into(), cx);
        ws.edit_automation(a, |x| x.name = String::new(), cx);
    });
    ws.read_with(cx, |ws, _| assert!(ws.automation_drafts.is_empty()));
}

#[cfg(unix)]
#[gpui_kit::test]
fn the_preview_says_the_sentence(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let title = ws.read_with(cx, |ws, cx| ws.uid_title(&uid, cx).unwrap());
    let nine = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    let weekdays = When::Weekly {
        days: vec![1, 2, 3, 4, 5],
        at: nine,
    };
    let sentence = |ws: &Entity<Workspace>, cx: &mut TestAppContext, a: &Automation| {
        ws.read_with(cx, |ws, cx| ws.automation_sentence(a, cx))
    };
    let typed = rule(
        weekdays,
        Do::Prompt {
            block: uid.clone(),
            text: "review open PRs".into(),
        },
    );
    assert_eq!(
        sentence(&ws, cx, &typed),
        format!("Every weekday at 09:00, type \"review open PRs\" into {title} ({uid}).")
    );
    let five = NaiveTime::from_hms_opt(17, 0, 0).unwrap();
    let make = rule(
        When::Daily { at: five },
        Do::Command {
            terminal: None,
            folder: Some("~/api".into()),
            text: "make test".into(),
        },
    );
    assert_eq!(
        sentence(&ws, cx, &make),
        "Every day at 17:00, run \"make test\" in a new terminal in ~/api."
    );
    // Dated in the current year: its words carry no year.
    let today = Local::now().date_naive().and_time(five);
    let stand_up = rule(When::Once { at: today }, say("Stand-up"));
    let day = today.format("%-d %b");
    assert_eq!(
        sentence(&ws, cx, &stand_up),
        format!("On {day} at 17:00, notify you: \"Stand-up\".")
    );
}

/// Types into a box of the form and lets the change reach the draft.
fn type_in(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    field: crate::automations_view::editor::Field,
    value: &str,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.type_in(field, value, window, cx));
    })
    .unwrap();
    cx.run_until_parked();
}

#[gpui_kit::test]
fn typing_in_the_form_edits_the_draft(cx: &mut TestAppContext) {
    use crate::automations_view::editor::Field;
    let (handle, ws) = open(cx);
    let seven = NaiveTime::from_hms_opt(7, 0, 0).unwrap();
    let id = add(&ws, cx, rule(When::Daily { at: seven }, say("Hi")));
    open_popup(handle, &ws, cx);
    let dot = || name(format!("automation-draft-{id}"));
    assert!(!drawn(handle, cx, dot()), "loading the form makes no draft");
    type_in(handle, &ws, cx, Field::Time, "10:30");
    assert_eq!(
        draft_of(&ws, cx, id).when,
        When::Daily {
            at: NaiveTime::from_hms_opt(10, 30, 0).unwrap()
        }
    );
    assert!(
        drawn(handle, cx, dot()),
        "the row has a dot while it has a draft"
    );
    // What can't be read keeps the draft as it was, and Save says so.
    type_in(handle, &ws, cx, Field::Time, "25:99");
    assert_eq!(
        draft_of(&ws, cx, id).when,
        When::Daily {
            at: NaiveTime::from_hms_opt(10, 30, 0).unwrap()
        }
    );
    click_id(handle, cx, name("automation-save".into()));
    assert!(drawn(handle, cx, name("automation-problem".into())));
    // Typed back to what is saved, it is no draft again.
    type_in(handle, &ws, cx, Field::Time, "07:00");
    assert!(!drawn(handle, cx, dot()));
    // A problem of the draft is shown above the buttons.
    type_in(handle, &ws, cx, Field::Notify, " ");
    click_id(handle, cx, name("automation-save".into()));
    assert!(drawn(handle, cx, name("automation-problem".into())));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(id).unwrap().act, say("Hi"))
    });
    // Revert gives the saved rule back to the boxes.
    click_id(handle, cx, name("automation-revert".into()));
    cx.update_window(handle, |_, window, _| {
        window.refresh();
    })
    .unwrap();
    assert_eq!(draft_of(&ws, cx, id).act, say("Hi"));
    // Another kind of When, from the boxes.
    ws.update(cx, |ws, cx| ws.set_when_kind("cron", cx));
    assert!(matches!(draft_of(&ws, cx, id).when, When::Cron { .. }));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert!(drawn(handle, cx, name("automation-cron-next".into())));
    type_in(handle, &ws, cx, Field::Cron, "0 25 * * *");
    assert_eq!(
        draft_of(&ws, cx, id).when,
        When::Cron {
            expr: "0 25 * * *".into()
        }
    );
}

// The review's findings.

#[gpui_kit::test]
fn an_every_of_zero_minutes_never_runs_and_does_not_crash(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let mut list = Automations::default();
    list.add(rule(every(0), say("Zero")));
    list.add(rule(every(10), say("Often")));
    crate::automations::save(dir.path(), &list, 1).unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automation_runs[&1].next, None);
        assert_eq!(ws.next_words(1, SystemTime::now()), "Never");
    });
    let before = notification_count(handle, cx);
    due(&ws, cx, 2);
    look(cx);
    assert_eq!(
        notification_count(handle, cx),
        before + 1,
        "the other still runs"
    );
    // The editor's preview of a 0 doesn't crash either.
    open_popup(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.edit_automation(1, |a| a.when = every(0), cx)
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_held_run_now_of_an_off_rule_is_tried_again_and_does_not_count(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let mut off = prompt("", &uid, "Go");
    off.on = false;
    let rule = add(&ws, cx, off);
    begins(handle, &ws, cx, &terminal);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(rule, window, cx))
    })
    .unwrap();
    assert_eq!(
        held(&ws, cx, rule),
        Some(format!("Waiting for {uid}: it is working"))
    );
    ends(handle, &ws, cx, &terminal);
    rested(&ws, cx, id);
    look(cx);
    history_with(&ws, cx, id, "Your task is in the pasted text");
    taken_by_hook(handle, &ws, cx, id, &terminal, rule);
    assert_eq!(
        last(&ws, cx, rule).0,
        Went::Ran,
        "{:?}",
        last(&ws, cx, rule)
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(rule).unwrap().runs, 0)
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_held_run_now_of_an_off_rule_gives_up_after_an_hour(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let mut off = prompt("", &uid, "Go");
    off.on = false;
    let rule = add(&ws, cx, off);
    begins(handle, &ws, cx, &terminal);
    ws.update(cx, |ws, cx| {
        let since = SystemTime::now() - Duration::from_secs(61 * 60);
        let held = crate::workspace::automations::Held {
            since,
            why: "Waiting".into(),
            counts: false,
        };
        ws.automation_runs.entry(rule).or_default().slot(None).held = Some(held);
        ws.look_at_automations(cx);
    });
    look(cx);
    assert_eq!(
        last(&ws, cx, rule),
        (Went::Skipped, format!("{uid} stayed busy for an hour."))
    );
    assert_eq!(held(&ws, cx, rule), None);
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_answer_after_a_save_does_not_count_on_the_new_rule(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    ws.update(cx, |ws, _| ws.automations.get_mut(rule).unwrap().since = 1);
    due(&ws, cx, rule);
    look(cx);
    history_with(&ws, cx, id, "from automation");
    // Saved while the task is being confirmed: the rule counts from now.
    ws.update(cx, |ws, cx| {
        ws.edit_automation(
            rule,
            |a| {
                a.act = Do::Prompt {
                    block: uid.clone(),
                    text: "New".into(),
                }
            },
            cx,
        );
        ws.save_automation(rule, cx).unwrap();
    });
    taken(&ws, cx, id, rule);
    ws.read_with(cx, |ws, _| {
        let a = ws.automations.get(rule).unwrap();
        assert_eq!(a.runs, 0, "the old run is not the new rule's");
        assert_eq!(
            a.last.as_ref().unwrap().went,
            Went::Ran,
            "but it is the last thing that happened"
        );
    });
}

#[gpui_kit::test]
fn the_look_runs_only_while_something_is_on(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    ws.read_with(cx, |ws, _| {
        assert!(!ws.automating, "no automations, no look")
    });
    ws.update(cx, |ws, cx| {
        ws.new_automation(cx);
        ws.edit_automation(
            crate::workspace::automations::NEW,
            |a| a.act = say("Hi"),
            cx,
        );
    });
    let id = saves(&ws, cx, crate::workspace::automations::NEW).unwrap();
    ws.read_with(cx, |ws, _| assert!(ws.automating, "a rule on starts it"));
    ws.update(cx, |ws, cx| ws.set_automation_on(id, false, cx));
    look(cx);
    ws.read_with(cx, |ws, _| {
        assert!(!ws.automating, "nothing on, nothing held: it stops")
    });
    ws.update(cx, |ws, cx| ws.set_automation_on(id, true, cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.automating, "turned on, it starts again")
    });
}

#[gpui_kit::test]
fn the_boxes_a_rule_does_not_use_are_empty(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = add(&ws, cx, rule(every(10), say("X")));
    let act = Do::Command {
        terminal: None,
        folder: Some("/tmp".into()),
        text: "ls".into(),
    };
    let second = add(&ws, cx, rule(every(10), act));
    open_popup(handle, &ws, cx);
    assert_eq!(draft_of(&ws, cx, first).act, say("X"));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_automation(second, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    ws.update(cx, |ws, cx| ws.set_do_kind("notify", cx));
    assert_eq!(
        draft_of(&ws, cx, second).act,
        say(""),
        "not the first rule's \"X\""
    );
}

#[gpui_kit::test]
fn a_drafts_next_counts_from_now(cx: &mut TestAppContext) {
    use chrono::{DateTime, Local};
    let (_, ws) = open(cx);
    let now = SystemTime::now();
    let mut every_two_hours = rule(every(120), say("Hi"));
    every_two_hours.since = unix(now) - 100 * 60;
    let id = add(&ws, cx, every_two_hours);
    let words = |at: DateTime<Local>| {
        let now = DateTime::<Local>::from(now);
        format!(
            "Next: {}",
            crate::automations::clock::time_words(at.naive_local(), now.naive_local())
        )
    };
    let since = crate::workspace::automations::local(unix(now) - 100 * 60);
    let preview = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, cx| {
            let a = ws.automation_draft(id).unwrap();
            crate::automations_view::editor::preview_next(ws, id, a, now, cx)
        })
    };
    assert_eq!(
        preview(&ws, cx),
        words(since + chrono::Duration::minutes(120))
    );
    // A draft counts from now, as Save will.
    ws.update(cx, |ws, cx| {
        ws.edit_automation(id, |a| a.name = "Renamed".into(), cx)
    });
    assert_eq!(
        preview(&ws, cx),
        words(DateTime::<Local>::from(now) + chrono::Duration::minutes(120))
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_held_prompt_that_falls_due_again_counts_once(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    begins(handle, &ws, cx, &terminal);
    // Held by a Run now, which doesn't count...
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(rule, window, cx))
    })
    .unwrap();
    assert!(held(&ws, cx, rule).is_some());
    ws.read_with(cx, |ws, _| {
        assert!(!ws.automation_runs[&rule].held().unwrap().1.counts)
    });
    // ...until its time comes: the time joins it.
    due(&ws, cx, rule);
    look(cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automation_runs[&rule].held().unwrap().1.counts)
    });
    ends(handle, &ws, cx, &terminal);
    rested(&ws, cx, id);
    look(cx);
    history_with(&ws, cx, id, "from automation");
    taken_by_hook(handle, &ws, cx, id, &terminal, rule);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(rule).unwrap().runs, 1, "one run");
        assert_eq!(ws.next_task, 1, "one task");
    });
}

#[gpui_kit::test]
fn a_rule_with_no_since_counts_from_the_start_and_misses_nothing(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let midnight = When::Daily {
        at: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
    };
    let mut list = Automations::default();
    let mut a = rule(midnight, say("Hi"));
    a.since = 0;
    list.add(a);
    crate::automations::save(dir.path(), &list, 1).unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    assert_eq!(notification_count(handle, cx), 0);
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.automations.get(1).unwrap().last.is_none(),
            "nothing Missed"
        );
        assert!(ws.automation_runs[&1].next.unwrap() > SystemTime::now());
        assert!(ws.automations.get(1).unwrap().since > 0);
        assert_eq!(ws.automations_unseen(), 0);
    });
}

// Round 2 of the review.

#[cfg(unix)]
#[gpui_kit::test]
fn a_held_runs_last_line_says_why_it_waits(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    // An older run that went, which the held one must not borrow its words from.
    ws.update(cx, |ws, _| {
        ws.automations.get_mut(rule).unwrap().last = Some(crate::automations::LastRun {
            at: unix(SystemTime::now()),
            went: Went::Ran,
            about: "Typed into it.".into(),
        });
    });
    let line = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.automation_last_line(rule, SystemTime::now()))
    };
    assert!(line(&ws, cx).unwrap().ends_with("Typed into it."));
    begins(handle, &ws, cx, &terminal);
    due(&ws, cx, rule);
    look(cx);
    let why = format!("Waiting for {uid}: it is working");
    assert_eq!(line(&ws, cx), Some(why.clone()));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automation_held_why(rule), Some(why))
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_time_that_falls_due_while_the_task_is_typed_counts_once(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    // Run now types it, and doesn't count...
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(rule, window, cx))
    })
    .unwrap();
    history_with(&ws, cx, id, "from automation");
    // ...but its time comes while it is being typed: it joins the run.
    due(&ws, cx, rule);
    look(cx);
    // The look moved the clock on: its Enter went already.
    ended_run(&ws, cx, rule);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(rule).unwrap().runs, 1, "counted once");
        assert_eq!(ws.next_task, 1, "one task");
    });
}

#[test]
fn a_dropdown_label_keeps_the_blocks_id_whole() {
    use crate::automations_view::editor::block_label;
    let label = block_label(&"t".repeat(60), "abc12", "A workspace with a long name");
    assert!(label.contains(" (abc12) · "), "{label}");
    assert!(
        label.chars().count() <= 20 + " (abc12) · ".chars().count() + 12,
        "{label}"
    );
    assert!(label.starts_with('t') && label.ends_with('…'));
    assert_eq!(
        block_label("Claude Code", "abc12", "api"),
        "Claude Code (abc12) · api"
    );
}

#[gpui_kit::test]
fn choosing_once_gives_the_next_full_hour(cx: &mut TestAppContext) {
    use chrono::Timelike;
    let (handle, ws) = open(cx);
    // Midnight is past all day: the draft's own time never passes for the next hour.
    let id = add(&ws, cx, rule(When::Daily { at: NaiveTime::MIN }, say("Hi")));
    open_popup(handle, &ws, cx);
    ws.update(cx, |ws, cx| ws.set_when_kind("once", cx));
    let When::Once { at } = draft_of(&ws, cx, id).when else {
        panic!("a Once");
    };
    assert!(at > Local::now().naive_local(), "not past: {at}");
    assert_eq!((at.minute(), at.second()), (0, 0), "a full hour: {at}");
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_late_failure_of_a_rule_turned_off_meanwhile_reaches_the_badge(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _) = agent(handle, &ws, cx, 0);
    let (space, uid) = (
        ws.read_with(cx, |ws, _| ws.space.id.clone()),
        me(&ws, cx, id).1,
    );
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    due(&ws, cx, rule);
    look(cx);
    history_with(&ws, cx, id, "from automation");
    // Turned off while its task is being typed, then the agent's block closes: the answer fails.
    ws.update(cx, |ws, cx| ws.set_automation_on(rule, false, cx));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.block_close((&space, id), &uid, "", window, cx)
        })
    })
    .unwrap();
    ended_run(&ws, cx, rule);
    assert_eq!(last(&ws, cx, rule).0, Went::Failed);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations_unseen(), 1, "the badge counts it");
        assert_eq!(ws.automations.get(rule).unwrap().runs, 0);
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn editing_only_a_commands_text_keeps_its_terminal(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let folder = tempfile::tempdir().unwrap();
    let id = add(&ws, cx, command(&space, folder.path(), "echo one"));
    due(&ws, cx, id);
    look(cx);
    let first = kept(&ws, cx, id);
    let change = |ws: &Entity<Workspace>, cx: &mut TestAppContext, edit: fn(&mut Do)| {
        ws.update(cx, |ws, cx| {
            ws.edit_automation(id, |a| edit(&mut a.act), cx);
            ws.save_automation(id, cx).unwrap();
        });
    };
    change(&ws, cx, |act| {
        if let Do::Command { text, .. } = act {
            *text = "echo two".into();
        }
    });
    assert_eq!(kept(&ws, cx, id), first, "only the command changed");
    change(&ws, cx, |act| {
        if let Do::Command { folder, .. } = act {
            *folder = Some("/".into());
        }
    });
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(id).unwrap().opened, None)
    });
}

#[gpui_kit::test]
fn a_chosen_block_in_another_workspace_is_labelled_with_it(cx: &mut TestAppContext) {
    use crate::automations_view::editor::kept_label;
    let (handle, ws) = open(cx);
    let uid = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.new_space(window, cx);
                ws.open(Rect::new(0.0, 0.0, 600.0, 400.0), window, cx);
                let id = ws.space.canvas.top().unwrap();
                let uid = ws.space.uids[&id].clone();
                ws.switch_to(0, window, cx);
                uid
            })
        })
        .unwrap();
    let name = ws.read_with(cx, |ws, _| ws.others[0].name.clone());
    ws.read_with(cx, |ws, _| {
        assert_eq!(kept_label(ws, &uid), format!("{uid} (in {name})"));
        assert_eq!(kept_label(ws, "zzzzz"), "zzzzz (not open)");
    });
}

/// The new task is typed (the looks move the clock on), then the agent tells it submitted, until
/// `done` holds: a task Templum types waits for its Enter and its hook.
#[cfg(unix)]
fn confirm_until(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    terminal: &str,
    done: impl Fn(&Workspace) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    let mut typed = false;
    while !ws.read_with(cx, |ws, _| done(ws)) {
        assert!(std::time::Instant::now() < deadline, "never happened");
        typed = typed || ws.read_with(cx, |ws, _| ws.next_task >= 2);
        if typed {
            super::bus::event(
                handle,
                ws,
                cx,
                terminal,
                serde_json::json!({ "event": "submitted" }),
            );
        }
        std::thread::sleep(Duration::from_millis(10));
        let step = match typed {
            true => crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
            false => AUTOMATION_LOOK.as_millis() as u64,
        };
        super::bus::after(cx, step);
    }
}

/// Run now types a task; the rule is saved while it is being typed and its new time falls due in
/// that window. The old task's answer must not swallow it.
#[cfg(unix)]
fn saved_while_its_old_task_is_typed(
    cx: &mut TestAppContext,
    new_when: When,
    due_by: fn(&mut Automation),
) -> (AnyWindowHandle, Entity<Workspace>, String, BlockId, u64) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    let rule = add(&ws, cx, prompt("", &uid, "Go"));
    ws.update(cx, |ws, _| ws.automations.get_mut(rule).unwrap().since = 1);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(rule, window, cx))
    })
    .unwrap();
    history_with(&ws, cx, id, "from automation");
    ws.update(cx, |ws, cx| {
        ws.edit_automation(rule, |a| a.when = new_when, cx);
        ws.save_automation(rule, cx).unwrap();
        due_by(ws.automations.get_mut(rule).unwrap());
    });
    // The saved rule's time comes while the old task is still being typed.
    due(&ws, cx, rule);
    look(cx);
    ended_run(&ws, cx, rule);
    // The old task ends with its turn, so the agent takes the new one.
    begins(handle, &ws, cx, &terminal);
    ends(handle, &ws, cx, &terminal);
    rested(&ws, cx, id);
    (handle, ws, terminal, id, rule)
}

#[cfg(unix)]
#[gpui_kit::test]
fn the_due_time_of_a_rule_saved_while_its_old_task_is_typed_runs_once(cx: &mut TestAppContext) {
    let (handle, ws, terminal, _, rule) = saved_while_its_old_task_is_typed(cx, every(10), |_| {});
    confirm_until(handle, &ws, cx, &terminal, |ws| {
        ws.automations.get(rule).unwrap().runs == 1
    });
    look(cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(rule).unwrap().runs, 1, "run once");
        assert_eq!(ws.next_task, 2, "the old task and the new one");
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_once_saved_while_its_old_task_is_typed_runs_and_turns_off(cx: &mut TestAppContext) {
    let at = Local::now().naive_local() + chrono::Duration::minutes(2);
    // Its minute comes as if two minutes went by while the old task was typed.
    let (handle, ws, terminal, _, rule) =
        saved_while_its_old_task_is_typed(cx, When::Once { at }, |a| {
            a.since -= 600;
            a.when = When::Once {
                at: Local::now().naive_local() - chrono::Duration::seconds(1),
            };
        });
    confirm_until(handle, &ws, cx, &terminal, |ws| {
        !ws.automations.get(rule).unwrap().on
    });
    ws.read_with(cx, |ws, _| {
        let a = ws.automations.get(rule).unwrap();
        assert_eq!(a.runs, 1, "the Once ran");
        assert_eq!(ws.next_task, 2);
    });
}

/// A rule on `event` from `who` that does `act`, on, counted from now.
fn on_event(event: Heard, who: Who, act: Do) -> Automation {
    rule(When::Agent { event, who }, act)
}

/// Chosen blocks, any of them.
fn chosen(uids: &[&str]) -> Who {
    Who::Blocks {
        blocks: uids.iter().map(|u| u.to_string()).collect(),
        all: false,
    }
}

/// The agent in `terminal` (block `id`) ends its turn; returns once its watch shows Idle.
#[cfg(unix)]
fn finishes(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
    terminal: &str,
) {
    ends(handle, ws, cx, terminal);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ws.read_with(cx, |ws, _| {
        let watch = &ws.space.terminal(id).unwrap().watch;
        matches!(watch.was, Some((Doing::Idle, _)))
    }) {
        assert!(std::time::Instant::now() < deadline, "not idle");
        std::thread::sleep(Duration::from_millis(10));
        cx.run_until_parked();
    }
}

/// The agent in `terminal` (block `id`) works, then ends its turn.
#[cfg(unix)]
fn turn(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
    terminal: &str,
) {
    begins(handle, ws, cx, terminal);
    finishes(handle, ws, cx, id, terminal);
}

#[cfg(unix)]
fn runs_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, rule: u64) -> u32 {
    ws.read_with(cx, |ws, _| ws.automations.get(rule).unwrap().runs)
}

/// The block's uid, and its terminal.
#[cfg(unix)]
fn agent_at(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    row: u8,
) -> (BlockId, String, String) {
    let (id, terminal) = agent(handle, ws, cx, row);
    let (_, uid) = me(ws, cx, id);
    (id, terminal, uid)
}

/// The tasks Templum typed (`tasks` of them in all) are told submitted by their agents, the
/// looks moving the clock on, until `done` holds.
#[cfg(unix)]
fn confirmed(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    terminals: &[&str],
    tasks: u32,
    done: impl Fn(&Workspace) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ws.read_with(cx, |ws, _| done(ws)) {
        assert!(std::time::Instant::now() < deadline, "never happened");
        if ws.read_with(cx, |ws, _| ws.next_task >= u64::from(tasks)) {
            for terminal in terminals {
                let said = serde_json::json!({ "event": "submitted" });
                super::bus::event(handle, ws, cx, terminal, said);
            }
        }
        std::thread::sleep(Duration::from_millis(10));
        super::bus::after(
            cx,
            crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
        );
    }
}

/// A rule that types `text` into the agent that fired it, on `event` from `who`, named `name`.
#[cfg(unix)]
fn reply_to_firer(name: &str, event: Heard, who: Who) -> Automation {
    let act = Do::Prompt {
        block: FIRED.into(),
        text: "Go on.".into(),
    };
    Automation {
        name: name.into(),
        ..on_event(event, who, act)
    }
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_turns_end_is_queued_only_while_an_event_rule_is_on(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let (_, uid) = me(&ws, cx, id);
    add(&ws, cx, rule(every(10), say("A time")));
    turn(handle, &ws, cx, id, &terminal);
    ws.read_with(cx, |ws, _| assert!(ws.automation_heard.is_empty()));
    add(&ws, cx, on_event(Heard::Idle, chosen(&[&uid]), say("Done")));
    turn(handle, &ws, cx, id, &terminal);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automation_heard, [(uid.clone(), Heard::Idle)])
    });
    look(cx);
    ws.read_with(cx, |ws, _| assert!(ws.automation_heard.is_empty()));
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_turns_end_fires_its_rule_and_the_cooldown_holds_the_next(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal, uid) = agent_at(handle, &ws, cx, 0);
    let rule = add(&ws, cx, on_event(Heard::Idle, chosen(&[&uid]), say("Done")));
    let before = notification_count(handle, cx);
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    assert_eq!(last(&ws, cx, rule), (Went::Ran, "Notified.".into()));
    assert_eq!(runs_of(&ws, cx, rule), 1);
    assert_eq!(notification_count(handle, cx), before + 1);
    // Inside the cooldown: nothing, and nothing written.
    ws.update(cx, |ws, _| {
        ws.automations
            .get_mut(rule)
            .unwrap()
            .last
            .as_mut()
            .unwrap()
            .at = 7
    });
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1);
    assert_eq!(notification_count(handle, cx), before + 1);
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.automations.get(rule).unwrap().last.as_ref().unwrap().at,
            7
        );
    });
    ws.update(cx, |ws, _| {
        ws.automations.get_mut(rule).unwrap().cooldown = 0
    });
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 2);
}

#[cfg(unix)]
#[gpui_kit::test]
fn each_agent_that_fires_it_has_its_own_cooldown(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, a_terminal, _) = agent_at(handle, &ws, cx, 0);
    let (b, b_terminal, _) = agent_at(handle, &ws, cx, 1);
    let kind = Who::Kind {
        kind: "claude".into(),
    };
    let rule = add(&ws, cx, on_event(Heard::Idle, kind, say("Done")));
    turn(handle, &ws, cx, a, &a_terminal);
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1);
    turn(handle, &ws, cx, b, &b_terminal);
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 2);
    turn(handle, &ws, cx, a, &a_terminal);
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 2, "A is inside its own cooldown");
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_need_fires_once_per_wait(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, terminal, uid) = agent_at(handle, &ws, cx, 0);
    let mut asks = on_event(Heard::NeedsYou, chosen(&[&uid]), say("Look"));
    asks.cooldown = 0;
    let rule = add(&ws, cx, asks);
    let permission =
        |tool: &str| json!({ "hook_event_name": "PermissionRequest", "tool_name": tool });
    begins(handle, &ws, cx, &terminal);
    hook(handle, &ws, cx, &terminal, permission("Bash"));
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1);
    hook(handle, &ws, cx, &terminal, permission("Edit"));
    look(cx);
    assert_eq!(
        runs_of(&ws, cx, rule),
        1,
        "another request is the same wait"
    );
    begins(handle, &ws, cx, &terminal);
    hook(handle, &ws, cx, &terminal, permission("Bash"));
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 2);
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_event_is_heard_only_in_its_rules_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal, _) = agent_at(handle, &ws, cx, 0);
    let (other, shown) = ws.update(cx, |ws, _| (ws.make_space(), ws.space.id.clone()));
    let kind = || Who::Kind {
        kind: "claude".into(),
    };
    let mut elsewhere = on_event(Heard::Idle, kind(), say("Done"));
    elsewhere.workspace = Some(other);
    let rule = add(&ws, cx, elsewhere);
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.get(rule).unwrap().last.is_none())
    });
    let mut here = on_event(Heard::Idle, kind(), say("Done"));
    here.workspace = Some(shown);
    let rule = add(&ws, cx, here);
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    assert_eq!(last(&ws, cx, rule).0, Went::Ran);
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_prompt_goes_to_the_agent_that_fired_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, _, _) = agent_at(handle, &ws, cx, 0);
    let (b, b_terminal, b_uid) = agent_at(handle, &ws, cx, 1);
    let kind = || Who::Kind {
        kind: "claude".into(),
    };
    let rule = add(&ws, cx, reply_to_firer("Next", Heard::Idle, kind()));
    let a_before = shown(&ws, cx, a);
    turn(handle, &ws, cx, b, &b_terminal);
    look(cx);
    history_with(&ws, cx, b, "[from automation \"Next\", task t1]");
    taken_by_hook(handle, &ws, cx, b, &b_terminal, rule);
    assert_eq!(
        last(&ws, cx, rule),
        (Went::Ran, format!("Typed into {b_uid} as task t1."))
    );
    assert_eq!(shown(&ws, cx, a), a_before, "A was not told");
    // Unnamed, the task is from the rule's words.
    let unnamed = add(&ws, cx, reply_to_firer("", Heard::Idle, kind()));
    turn(handle, &ws, cx, b, &b_terminal);
    rested(&ws, cx, b);
    look(cx);
    let from = "[from automation \"When any Claude Code finishes its turn\", task t2]";
    history_with(&ws, cx, b, from);
    let _ = unnamed;
}

#[cfg(unix)]
#[gpui_kit::test]
fn two_agents_that_finish_together_each_get_their_prompt(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, a_terminal, _) = agent_at(handle, &ws, cx, 0);
    let (b, b_terminal, _) = agent_at(handle, &ws, cx, 1);
    let kind = Who::Kind {
        kind: "claude".into(),
    };
    let rule = add(&ws, cx, reply_to_firer("Next", Heard::Idle, kind));
    turn(handle, &ws, cx, a, &a_terminal);
    turn(handle, &ws, cx, b, &b_terminal);
    look(cx);
    for id in [a, b] {
        history_with(&ws, cx, id, "[from automation \"Next\"");
    }
    confirmed(handle, &ws, cx, &[&a_terminal, &b_terminal], 2, |ws| {
        ws.automations.get(rule).unwrap().runs == 2
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn all_of_them_fires_when_the_last_one_finishes(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, a_terminal, a_uid) = agent_at(handle, &ws, cx, 0);
    let (b, b_terminal, b_uid) = agent_at(handle, &ws, cx, 1);
    let both = Who::Blocks {
        blocks: vec![a_uid, b_uid],
        all: true,
    };
    let rule = add(&ws, cx, reply_to_firer("Both", Heard::Idle, both));
    begins(handle, &ws, cx, &a_terminal);
    begins(handle, &ws, cx, &b_terminal);
    let (a_before, b_before) = (shown(&ws, cx, a), shown(&ws, cx, b));
    finishes(handle, &ws, cx, a, &a_terminal);
    look(cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.get(rule).unwrap().last.is_none())
    });
    assert_eq!(shown(&ws, cx, b), b_before, "B still works");
    finishes(handle, &ws, cx, b, &b_terminal);
    look(cx);
    history_with(&ws, cx, b, "[from automation \"Both\"");
    assert_eq!(shown(&ws, cx, a), a_before, "typed into the last one only");
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_chosen_agent_that_is_not_open_is_target_missing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal, uid) = agent_at(handle, &ws, cx, 0);
    let missing = |why: &str| Err(format!("Target missing: {why}"));
    let target = |ws: &Entity<Workspace>, cx: &mut TestAppContext, a: &Automation| {
        ws.read_with(cx, |ws, _| ws.automation_target(a))
    };
    let notify_on = |who: Who| on_event(Heard::Idle, who, say("Done"));
    let first = notify_on(chosen(&[&uid, "zzzzz"]));
    assert_eq!(target(&ws, cx, &first), missing("zzzzz isn't open"));
    let rule = add(&ws, cx, first);
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    assert_eq!(
        last(&ws, cx, rule),
        (Went::Skipped, "Target missing: zzzzz isn't open".into())
    );
    // All of them: a block that isn't open doesn't hold the others back, but the rule is missing it.
    let all = Who::Blocks {
        blocks: vec![uid.clone(), "zzzzz".into()],
        all: true,
    };
    let rule = add(&ws, cx, notify_on(all));
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    assert_eq!(
        last(&ws, cx, rule),
        (Went::Skipped, "Target missing: zzzzz isn't open".into())
    );
    let nosuch = Who::Kind {
        kind: "nosuch".into(),
    };
    assert_eq!(
        target(&ws, cx, &notify_on(nosuch)),
        missing("nosuch isn't in Settings ▸ Agents")
    );
    let other = ws.update(cx, |ws, _| ws.make_space());
    let mut elsewhere = notify_on(chosen(&[&uid]));
    elsewhere.workspace = Some(other);
    let name = ws.read_with(cx, |ws, _| ws.space.name.clone());
    assert_eq!(
        target(&ws, cx, &elsewhere),
        missing(&format!("{uid} is in {name}"))
    );
    let fired = Do::Prompt {
        block: FIRED.into(),
        text: "Go".into(),
    };
    assert_eq!(
        target(&ws, cx, &rule_of_time(fired)),
        missing("only an agent event has an agent that fired it")
    );
}

#[cfg(unix)]
fn rule_of_time(act: Do) -> Automation {
    rule(every(10), act)
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_event_while_it_is_off_is_not_replayed(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal, uid) = agent_at(handle, &ws, cx, 0);
    let mut off = on_event(Heard::Idle, chosen(&[&uid]), say("Done"));
    off.on = false;
    let rule = add(&ws, cx, off);
    turn(handle, &ws, cx, id, &terminal);
    ws.update(cx, |ws, cx| ws.set_automation_on(rule, true, cx));
    look(cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.get(rule).unwrap().last.is_none())
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_answer_after_a_save_frees_the_agents_run(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal, uid) = agent_at(handle, &ws, cx, 0);
    let rule = add(&ws, cx, reply_to_firer("", Heard::Idle, chosen(&[&uid])));
    ws.update(cx, |ws, _| ws.automations.get_mut(rule).unwrap().since = 1);
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    history_with(&ws, cx, id, "from automation");
    // Saved while the task is being confirmed.
    ws.update(cx, |ws, cx| {
        ws.edit_automation(rule, |a| a.name = "Renamed".into(), cx);
        ws.save_automation(rule, cx).unwrap();
    });
    confirmed(handle, &ws, cx, &[&terminal], 1, |ws| {
        ws.automations.get(rule).unwrap().last.is_some()
    });
    ws.read_with(cx, |ws, _| {
        let a = ws.automations.get(rule).unwrap();
        assert_eq!((a.runs, a.last.as_ref().unwrap().went), (0, Went::Ran));
        assert!(
            ws.automation_runs[&rule].slots.is_empty(),
            "the slot is free"
        );
    });
    // Its next turn's end fires again: the save started the cooldown over.
    turn(handle, &ws, cx, id, &terminal);
    rested(&ws, cx, id);
    look(cx);
    confirmed(handle, &ws, cx, &[&terminal], 2, |ws| {
        ws.automations.get(rule).unwrap().runs == 1
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_answer_after_a_delete_does_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal, uid) = agent_at(handle, &ws, cx, 0);
    let rule = add(&ws, cx, reply_to_firer("", Heard::Idle, chosen(&[&uid])));
    turn(handle, &ws, cx, id, &terminal);
    look(cx);
    history_with(&ws, cx, id, "from automation");
    ws.update(cx, |ws, cx| ws.delete_automation(rule, cx));
    entered(&ws, cx, id);
    for _ in 0..3 {
        super::bus::after(
            cx,
            crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    ws.read_with(cx, |ws, _| {
        assert!(!ws.automation_runs.contains_key(&rule));
        assert!(ws.automations.get(rule).is_none());
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn one_agents_held_run_holds_no_other_and_stop_after_drops_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, a_terminal, a_uid) = agent_at(handle, &ws, cx, 0);
    let (b, b_terminal, b_uid) = agent_at(handle, &ws, cx, 1);
    let kind = Who::Kind {
        kind: "claude".into(),
    };
    let mut once = reply_to_firer("", Heard::Idle, kind);
    once.max_runs = Some(1);
    let rule = add(&ws, cx, once);
    let a_before = shown(&ws, cx, a);
    turn(handle, &ws, cx, a, &a_terminal);
    turn(handle, &ws, cx, b, &b_terminal);
    // Their half-written message is in A's input (a turn's start clears it).
    ws.read_with(cx, |ws, _| ws.space.terminal(a).unwrap().draft.set(true));
    look(cx);
    history_with(&ws, cx, b, "from automation");
    ws.read_with(cx, |ws, _| {
        let run = &ws.automation_runs[&rule];
        let (by, held) = run.held().expect("A's run waits");
        assert_eq!(by, Some(a_uid.as_str()));
        assert_eq!(
            held.why,
            format!("Waiting for {a_uid}: you are typing in it")
        );
        assert_eq!(
            ws.last_words(rule, SystemTime::now()),
            Some(format!("Waiting for {a_uid}"))
        );
    });
    confirmed(handle, &ws, cx, &[&b_terminal], 1, |ws| {
        ws.automations.get(rule).unwrap().last.is_some()
    });
    ws.read_with(cx, |ws, _| {
        let a = ws.automations.get(rule).unwrap();
        assert_eq!((a.runs, a.on), (1, false));
        assert!(
            a.last
                .as_ref()
                .unwrap()
                .about
                .ends_with("Ran 1 of 1 times, now off.")
        );
        assert!(ws.automation_runs[&rule].held().is_none());
        let _ = &b_uid;
    });
    assert_eq!(shown(&ws, cx, a), a_before, "nothing typed into A");
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_agents_run_held_for_an_hour_is_skipped(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, uid) = agent_at(handle, &ws, cx, 0);
    let rule = add(&ws, cx, reply_to_firer("", Heard::Idle, chosen(&[&uid])));
    ws.update(cx, |ws, _| {
        let since = SystemTime::now() - Duration::from_secs(61 * 60);
        let held = crate::workspace::automations::Held {
            since,
            why: "Waiting".into(),
            counts: true,
        };
        let run = ws.automation_runs.entry(rule).or_default();
        run.slot(Some(&uid)).held = Some(held);
    });
    look(cx);
    assert_eq!(
        last(&ws, cx, rule),
        (Went::Skipped, format!("{uid} stayed busy for an hour."))
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.automation_runs[&rule].slots.is_empty())
    });
}

/// How many commands finished in the terminal block `id`.
#[cfg(unix)]
fn finished(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> u64 {
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(id).unwrap().term.status().finished
    })
}

/// A command ends in the terminal block `id` with `code` (None: the shell said none), as shell
/// integration tells it; returns once the terminal counts it.
#[cfg(unix)]
fn command_ends(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId, code: Option<i32>) {
    let before = finished(ws, cx, id);
    let code = code.map_or(String::new(), |code| format!(";{code}"));
    let bytes = format!("\x1b]133;C\x07\x1b]133;D{code}\x07\r").into_bytes();
    ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.write(bytes));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while finished(ws, cx, id) == before {
        assert!(std::time::Instant::now() < deadline, "not counted");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// A rule that notifies when a command ends in `uid`, with no cooldown.
#[cfg(unix)]
fn on_command(uid: &str, failures: bool) -> Automation {
    let when = When::Command {
        terminal: uid.into(),
        failures,
    };
    Automation {
        cooldown: 0,
        ..rule(when, say("Ended"))
    }
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_command_end_fires_once_for_the_last(cx: &mut TestAppContext) {
    let (_, ws, terminal, _) = two_agents(cx);
    let (_, uid) = me(&ws, cx, terminal);
    let rule = add(&ws, cx, on_command(&uid, false));
    look(cx);
    // Two commands end between two looks: one run.
    command_ends(&ws, cx, terminal, Some(0));
    command_ends(&ws, cx, terminal, Some(0));
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1);
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1, "nothing new, nothing run");
    let gone = on_command("zzzzz", false);
    let why = ws.read_with(cx, |ws, _| ws.automation_target(&gone));
    assert_eq!(why, Err("Target missing: zzzzz isn't open".into()));
}

#[cfg(unix)]
#[gpui_kit::test]
fn failures_only_fires_on_a_failing_exit(cx: &mut TestAppContext) {
    let (_, ws, terminal, _) = two_agents(cx);
    let (_, uid) = me(&ws, cx, terminal);
    let rule = add(&ws, cx, on_command(&uid, true));
    look(cx);
    command_ends(&ws, cx, terminal, Some(0));
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 0, "it went well");
    command_ends(&ws, cx, terminal, Some(2));
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1);
    command_ends(&ws, cx, terminal, None);
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1, "no exit code is no failure");
}

#[cfg(unix)]
#[gpui_kit::test]
fn commands_that_ended_before_it_was_on_fire_nothing(cx: &mut TestAppContext) {
    let (_, ws, terminal, _) = two_agents(cx);
    let (_, uid) = me(&ws, cx, terminal);
    let mut off = on_command(&uid, true);
    off.on = false;
    let rule = add(&ws, cx, off);
    command_ends(&ws, cx, terminal, Some(1));
    ws.update(cx, |ws, cx| ws.set_automation_on(rule, true, cx));
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 0);
    command_ends(&ws, cx, terminal, Some(1));
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1);
}

/// The terminal blocks of the shown workspace.
#[cfg(unix)]
fn terminal_ids(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<BlockId> {
    ws.read_with(cx, |ws, _| ws.space.terminals().map(|(id, _)| id).collect())
}

/// The new terminal beside `a` made by one turn of its end: not over it, nearer it than the
/// middle of the view.
#[cfg(unix)]
fn opened_beside(ws: &Entity<Workspace>, cx: &mut TestAppContext, before: &[BlockId], a: BlockId) {
    let opened: Vec<BlockId> = (terminal_ids(ws, cx).into_iter())
        .filter(|id| !before.contains(id))
        .collect();
    assert_eq!(opened.len(), 1, "one block opened");
    ws.read_with(cx, |ws, _| {
        let (new, firer) = (
            ws.space.canvas.get(opened[0]).unwrap().rect,
            ws.space.canvas.get(a).unwrap().rect,
        );
        assert!(!new.intersects(&firer), "never over it");
        let middle = |r: crate::workspace::Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let apart =
            |p: (f32, f32), q: (f32, f32)| ((p.0 - q.0).powi(2) + (p.1 - q.1).powi(2)).sqrt();
        let (near, far) = (
            apart(middle(new), middle(firer)),
            apart(middle(new), (640.0, 360.0)),
        );
        assert!(
            near < far,
            "beside it ({near}), not at the view's middle ({far})"
        );
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn new_blocks_open_beside_the_agent_that_fired_it(cx: &mut TestAppContext) {
    let (handle, ws, _) = spawner(cx);
    let (a, a_terminal, a_uid) = agent_at(handle, &ws, cx, 0);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(a).unwrap().rect = Rect::new(4000.0, 3000.0, 400.0, 300.0)
    });
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let folder = tempfile::tempdir().unwrap();
    let opens = |act: Do| Automation {
        workspace: Some(space.clone()),
        cooldown: 0,
        ..on_event(Heard::Idle, chosen(&[&a_uid]), act)
    };
    let command = Do::Command {
        terminal: None,
        folder: Some(folder.path().display().to_string()),
        text: "true".into(),
    };
    let agent_do = Do::Agent {
        agent: "claude".into(),
        folder: Some(folder.path().display().to_string()),
        model: None,
        effort: None,
        prompt: None,
    };
    for act in [command, agent_do] {
        let before = terminal_ids(&ws, cx);
        let rule = add(&ws, cx, opens(act));
        turn(handle, &ws, cx, a, &a_terminal);
        look(cx);
        assert_eq!(last(&ws, cx, rule).0, Went::Ran);
        opened_beside(&ws, cx, &before, a);
        ws.update(cx, |ws, cx| ws.delete_automation(rule, cx));
    }
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_notifications_click_shows_the_block_that_fired_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let rule = add(&ws, cx, rule(every(10), say("Hi")));
    let (other, b, uid) = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.new_space(window, cx);
                ws.open(Rect::new(0.0, 0.0, 600.0, 400.0), window, cx);
                let b = ws.space.canvas.top().unwrap();
                let (other, uid) = (ws.space.id.clone(), ws.space.uids[&b].clone());
                ws.switch_to(0, window, cx);
                (other, b, uid)
            })
        })
        .unwrap();
    let shows = |ws: &Entity<Workspace>, cx: &mut TestAppContext, by: Option<&str>| {
        let by = by.map(str::to_string);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.open_fired(rule, by, window, cx))
        })
        .unwrap();
    };
    shows(&ws, cx, Some(&uid));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            (ws.space.id.as_str(), ws.space.focused),
            (other.as_str(), Some(b))
        );
        assert!(!ws.automations_open);
    });
    for by in [Some("zzzzz"), None] {
        shows(&ws, cx, by);
        ws.read_with(cx, |ws, _| {
            assert!(ws.automations_open);
            assert_eq!(ws.automations_at, Some(rule));
        });
        ws.update(cx, |ws, _| ws.automations_open = false);
    }
}

#[cfg(unix)]
#[gpui_kit::test]
fn run_now_does_nothing_for_the_agent_that_fired_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _, uid) = agent_at(handle, &ws, cx, 0);
    let rule = add(&ws, cx, reply_to_firer("", Heard::Idle, chosen(&[&uid])));
    let before = shown(&ws, cx, id);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(rule, window, cx))
    })
    .unwrap();
    // Its button on the selected one's card is dimmed and does nothing either.
    open_popup(handle, &ws, cx);
    click_id(handle, cx, name(format!("automation-run-{rule}")));
    look(cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.get(rule).unwrap().last.is_none());
        assert!(
            ws.automation_runs
                .get(&rule)
                .is_none_or(|run| run.slots.is_empty())
        );
    });
    assert_eq!(shown(&ws, cx, id), before);
}

// Batch 2's list and editor.

#[cfg(unix)]
#[gpui_kit::test]
fn an_event_rules_words_in_the_list_and_the_editor(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, _, uid) = agent_at(handle, &ws, cx, 0);
    let title = ws.read_with(cx, |ws, cx| ws.uid_title(&uid, cx).unwrap());
    let said = format!("When {title} ({uid}) finishes its turn");
    let rule = add(&ws, cx, on_event(Heard::Idle, chosen(&[&uid]), say("Hi")));
    let (words, sentence, preview) = ws.read_with(cx, |ws, cx| {
        let a = ws.automation_draft(rule).unwrap();
        (
            (ws.next_words(rule, SystemTime::now()), ws.when_said(a, cx)),
            ws.automation_sentence(a, cx),
            crate::automations_view::editor::preview_next(ws, rule, a, SystemTime::now(), cx),
        )
    });
    assert_eq!(words, (format!("on {uid}'s turn's end"), said.clone()));
    assert_eq!(sentence, format!("{said}, notify you: \"Hi\"."));
    let lowered = said.replacen("When", "when", 1);
    assert_eq!(preview, format!("Runs {lowered}"));
    let typed = Do::Prompt {
        block: FIRED.into(),
        text: "Go on.".into(),
    };
    ws.update(cx, |ws, cx| ws.edit_automation(rule, |a| a.act = typed, cx));
    let sentence = ws.read_with(cx, |ws, cx| {
        ws.automation_sentence(ws.automation_draft(rule).unwrap(), cx)
    });
    assert_eq!(
        sentence,
        format!("{said}, type \"Go on.\" into the agent that fired it.")
    );
    let _ = id;
}

#[gpui_kit::test]
fn save_checks_an_event_draft(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let id = add(&ws, cx, rule(every(10), say("ok")));
    let problem = |ws: &Entity<Workspace>, cx: &mut TestAppContext, edit: fn(&mut Automation)| {
        ws.update(cx, |ws, cx| ws.edit_automation(id, edit, cx));
        let found = saves(ws, cx, id);
        ws.update(cx, |ws, cx| ws.revert_automation(id, cx));
        found
    };
    let agents = |who: Who| When::Agent {
        event: Heard::Idle,
        who,
    };
    assert_eq!(
        problem(&ws, cx, |a| a.when = When::Agent {
            event: Heard::Idle,
            who: chosen(&[])
        }),
        Err("Choose at least one agent.".into())
    );
    let _ = agents;
    assert_eq!(
        problem(&ws, cx, |a| a.when = When::Agent {
            event: Heard::NeedsYou,
            who: Who::Kind {
                kind: String::new()
            }
        }),
        Err("Choose a kind of agent.".into())
    );
    assert_eq!(
        problem(&ws, cx, |a| a.when = When::Command {
            terminal: String::new(),
            failures: false
        }),
        Err("Choose a terminal.".into())
    );
    assert_eq!(
        problem(&ws, cx, |a| {
            a.when = When::Command {
                terminal: "abc12".into(),
                failures: false,
            };
            a.cooldown = 1441;
        }),
        Err("The cooldown runs from 0 to 1440 minutes.".into())
    );
    assert_eq!(
        problem(&ws, cx, |a| a.act = Do::Prompt {
            block: FIRED.into(),
            text: "Go".into()
        }),
        Err("Choose an agent.".into())
    );
    // A good one saves, its cooldown kept and its cooldowns started over.
    ws.update(cx, |ws, cx| {
        ws.automation_runs
            .entry(id)
            .or_default()
            .cooled
            .insert("abc12".into(), SystemTime::now());
        ws.edit_automation(
            id,
            |a| {
                a.when = When::Agent {
                    event: Heard::Idle,
                    who: chosen(&["abc12"]),
                };
                a.cooldown = 10;
            },
            cx,
        );
    });
    assert_eq!(saves(&ws, cx, id), Ok(id));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.get(id).unwrap().cooldown, 10);
        assert!(ws.automation_runs[&id].cooled.is_empty());
    });
}

#[gpui_kit::test]
fn choosing_an_event_kind_keeps_who_and_drops_the_fired_target(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let nine = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
    let id = add(&ws, cx, rule(When::Daily { at: nine }, say("Hi")));
    open_popup(handle, &ws, cx);
    let kind = |ws: &Entity<Workspace>, cx: &mut TestAppContext, kind: &'static str| {
        ws.update(cx, |ws, cx| ws.set_when_kind(kind, cx));
        draft_of(ws, cx, id)
    };
    assert_eq!(
        kind(&ws, cx, "idle").when,
        When::Agent {
            event: Heard::Idle,
            who: chosen(&[])
        }
    );
    let both = |event| When::Agent {
        event,
        who: Who::Blocks {
            blocks: vec!["abc12".into(), "k9x1d".into()],
            all: true,
        },
    };
    ws.update(cx, |ws, cx| {
        ws.edit_automation(id, |a| a.when = both(Heard::Idle), cx)
    });
    assert_eq!(
        kind(&ws, cx, "needs_you").when,
        When::Agent {
            event: Heard::NeedsYou,
            who: Who::Blocks {
                blocks: vec!["abc12".into(), "k9x1d".into()],
                all: false
            }
        },
        "All of them is for a turn's end"
    );
    let fired = Do::Prompt {
        block: FIRED.into(),
        text: "Go".into(),
    };
    ws.update(cx, |ws, cx| ws.edit_automation(id, |a| a.act = fired, cx));
    let daily = kind(&ws, cx, "daily");
    assert!(matches!(daily.when, When::Daily { .. }));
    assert_eq!(
        daily.act,
        Do::Prompt {
            block: String::new(),
            text: "Go".into()
        }
    );
    assert_eq!(
        kind(&ws, cx, "command").when,
        When::Command {
            terminal: String::new(),
            failures: false
        }
    );
    // Who comes back as a kind, from Settings ▸ Agents.
    kind(&ws, cx, "idle");
    ws.update(cx, |ws, cx| ws.set_who_kind("kind", cx));
    let first = ws.read_with(cx, |ws, _| ws.agents[0].id.clone());
    assert_eq!(
        draft_of(&ws, cx, id).when,
        When::Agent {
            event: Heard::Idle,
            who: Who::Kind { kind: first }
        }
    );
}

#[gpui_kit::test]
fn typing_a_cooldown_edits_the_draft(cx: &mut TestAppContext) {
    use crate::automations_view::editor::Field;
    let (handle, ws) = open(cx);
    let id = add(
        &ws,
        cx,
        on_event(Heard::Idle, chosen(&["abc12"]), say("Hi")),
    );
    open_popup(handle, &ws, cx);
    assert!(drawn(handle, cx, name("automation-sentence".into())));
    type_in(handle, &ws, cx, Field::Cooldown, "10");
    assert_eq!(draft_of(&ws, cx, id).cooldown, 10);
    type_in(handle, &ws, cx, Field::Cooldown, "");
    assert_eq!(draft_of(&ws, cx, id).cooldown, COOLDOWN);
    type_in(handle, &ws, cx, Field::Cooldown, "20");
    type_in(handle, &ws, cx, Field::Cooldown, "x");
    assert_eq!(
        draft_of(&ws, cx, id).cooldown,
        20,
        "what can't be read changes nothing"
    );
    click_id(handle, cx, name("automation-save".into()));
    assert!(drawn(handle, cx, name("automation-problem".into())));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.automations.get(id).unwrap().cooldown,
            COOLDOWN,
            "not saved"
        );
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn the_who_toggles_choose_blocks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, uid) = agent_at(handle, &ws, cx, 0);
    let id = add(&ws, cx, on_event(Heard::Idle, chosen(&[]), say("Hi")));
    open_popup(handle, &ws, cx);
    let who = |ws: &Entity<Workspace>, cx: &mut TestAppContext| match draft_of(ws, cx, id).when {
        When::Agent {
            who: Who::Blocks { blocks, .. },
            ..
        } => blocks,
        other => panic!("{other:?}"),
    };
    click_id(handle, cx, name(format!("automation-who-{uid}")));
    assert_eq!(who(&ws, cx), std::slice::from_ref(&uid));
    click_id(handle, cx, name(format!("automation-who-{uid}")));
    assert!(who(&ws, cx).is_empty());
}

#[cfg(unix)]
#[gpui_kit::test]
fn the_loop_warning(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, uid) = agent_at(handle, &ws, cx, 0);
    let loops = |ws: &Entity<Workspace>, cx: &mut TestAppContext, a: &Automation| {
        ws.read_with(cx, |ws, _| ws.automation_loop(a))
    };
    let prompt_to = |block: &str| Do::Prompt {
        block: block.into(),
        text: "Go".into(),
    };
    let claude = || Who::Kind {
        kind: "claude".into(),
    };
    let turn_end = format!(
        "This can loop: its prompt ends a turn of {uid}, which fires it again. The cooldown spaces the runs out."
    );
    for who in [chosen(&[&uid]), claude()] {
        let a = on_event(Heard::Idle, who, prompt_to(&uid));
        assert_eq!(loops(&ws, cx, &a), Some(turn_end.clone()));
    }
    let fired = on_event(Heard::Idle, chosen(&[&uid]), prompt_to(FIRED));
    assert_eq!(
        loops(&ws, cx, &fired).as_deref(),
        Some(
            "This can loop: its prompt ends a turn of the agent that fired it, which fires it again. The cooldown spaces the runs out."
        )
    );
    let needs = on_event(Heard::NeedsYou, chosen(&[&uid]), prompt_to(&uid));
    assert_eq!(
        loops(&ws, cx, &needs),
        None,
        "a prompt to an agent that needs you is held"
    );
    let needs_fired = on_event(Heard::NeedsYou, chosen(&[&uid]), prompt_to(FIRED));
    assert_eq!(
        loops(&ws, cx, &needs_fired),
        None,
        "nor to the one that needs you"
    );
    let starts = Do::Agent {
        agent: "claude".into(),
        folder: None,
        model: None,
        effort: None,
        prompt: None,
    };
    for event in [Heard::Idle, Heard::NeedsYou] {
        let a = on_event(event, claude(), starts.clone());
        assert_eq!(
            loops(&ws, cx, &a).as_deref(),
            Some(
                "This can loop: the agent it starts can fire it again, and each new agent has its own cooldown. Stop after bounds it."
            )
        );
    }
    let command = |when_in: &str, runs_in: &str| {
        rule(
            When::Command {
                terminal: when_in.into(),
                failures: false,
            },
            Do::Command {
                terminal: Some(runs_in.into()),
                folder: None,
                text: "make".into(),
            },
        )
    };
    assert_eq!(
        loops(&ws, cx, &command("q3x9z", "q3x9z")).as_deref(),
        Some("This can loop: its command ends in q3x9z, which fires it again.")
    );
    assert_eq!(loops(&ws, cx, &command("q3x9z", "k9x1d")), None);
    // Drawn under the Do.
    let id = add(&ws, cx, fired);
    open_popup(handle, &ws, cx);
    assert!(drawn(handle, cx, name("automation-loop".into())));
    ws.update(cx, |ws, cx| {
        ws.edit_automation(id, |a| a.act = say("Hi"), cx)
    });
    assert!(!drawn(handle, cx, name("automation-loop".into())));
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_long_event_word_stays_in_its_row(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        for entry in &mut ws.agents {
            if entry.id == "claude" {
                entry.title = "Claude Code with a very long title ".repeat(4);
            }
        }
    });
    let kind = Who::Kind {
        kind: "claude".into(),
    };
    let id = add(&ws, cx, on_event(Heard::Idle, kind, say("Hi")));
    open_popup(handle, &ws, cx);
    let (row, next) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let width = |window: &mut Window, what: String| {
                window
                    .try_find(name(what))
                    .expect("drawn")
                    .bounds()
                    .size
                    .width
            };
            (
                width(window, format!("automation-row-{id}")),
                width(window, format!("automation-next-{id}")),
            )
        })
        .unwrap();
    assert!(row <= px(crate::agents_view::LIST_W), "{row:?}");
    assert!(next <= px(150.0), "{next:?}");
}

#[gpui_kit::test]
fn the_actions_are_on_the_selected_ones_card_not_over_its_row(cx: &mut TestAppContext) {
    use crate::workspace::automations::NEW;
    let (handle, ws) = open(cx);
    let first = add(&ws, cx, rule(every(10), say("Hi")));
    let other = add(&ws, cx, rule(every(20), say("Ho")));
    open_popup(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let card = window.find("automation-summary").bounds();
        let row = window
            .find(name(format!("automation-row-{first}")))
            .bounds();
        for what in ["run", "duplicate", "delete"] {
            let button = window
                .find(name(format!("automation-{what}-{first}")))
                .bounds();
            assert!(
                button.is_contained_within(&card),
                "{what}: {button:?} {card:?}"
            );
            assert!(!button.intersects(&row), "{what} is over its row");
            let shown = window.try_find(name(format!("automation-{what}-{other}")));
            assert!(shown.is_none(), "{what} of a row not selected");
        }
    })
    .unwrap();
    // They still act on the selected one.
    click_id(handle, cx, name(format!("automation-duplicate-{first}")));
    ws.read_with(cx, |ws, _| assert_eq!(ws.automation_ids().len(), 3));
    // A draft never saved has nothing to run, copy or delete.
    ws.update(cx, |ws, cx| ws.new_automation(cx));
    assert!(drawn(handle, cx, name("automation-sentence".into())));
    assert!(!drawn(handle, cx, name(format!("automation-run-{NEW}"))));
}

#[gpui_kit::test]
fn the_empty_list_says_events_too(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_popup(handle, &ws, cx);
    assert!(drawn(handle, cx, name("automations-empty".into())));
    assert!(crate::automations_view::EMPTY_HINT.ends_with(", or when an agent finishes."));
}

#[cfg(unix)]
#[gpui_kit::test]
fn several_events_of_one_look_stop_at_stop_after(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, a_terminal, _) = agent_at(handle, &ws, cx, 0);
    let (b, b_terminal, _) = agent_at(handle, &ws, cx, 1);
    let kind = Who::Kind {
        kind: "claude".into(),
    };
    let mut once = on_event(Heard::Idle, kind, say("Done"));
    once.max_runs = Some(1);
    let rule = add(&ws, cx, once);
    let before = notification_count(handle, cx);
    turn(handle, &ws, cx, a, &a_terminal);
    turn(handle, &ws, cx, b, &b_terminal);
    look(cx);
    assert_eq!(runs_of(&ws, cx, rule), 1);
    assert_eq!(notification_count(handle, cx), before + 1);
    ws.read_with(cx, |ws, _| assert!(!ws.automations.get(rule).unwrap().on));
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_command_rule_hears_only_its_workspaces_terminal(cx: &mut TestAppContext) {
    let (_, ws, terminal, _) = two_agents(cx);
    let (_, uid) = me(&ws, cx, terminal);
    let other = ws.update(cx, |ws, _| ws.make_space());
    let mut elsewhere = on_command(&uid, false);
    elsewhere.workspace = Some(other);
    let rule = add(&ws, cx, elsewhere);
    look(cx);
    command_ends(&ws, cx, terminal, Some(1));
    look(cx);
    ws.read_with(cx, |ws, _| {
        let none = ws.automations.get(rule).unwrap().last.is_none();
        assert!(none, "not Skipped on every end");
        assert_eq!(ws.automations_unseen(), 0);
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_event_that_joins_a_run_saved_meanwhile_still_runs(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, a_terminal, a_uid) = agent_at(handle, &ws, cx, 0);
    let (b, b_terminal, b_uid) = agent_at(handle, &ws, cx, 1);
    let to_b = Do::Prompt {
        block: b_uid,
        text: "Go".into(),
    };
    let rule = add(&ws, cx, on_event(Heard::Idle, chosen(&[&a_uid]), to_b));
    ws.update(cx, |ws, _| ws.automations.get_mut(rule).unwrap().since = 1);
    turn(handle, &ws, cx, a, &a_terminal);
    look(cx);
    history_with(&ws, cx, b, "from automation");
    // Saved while B's task is being confirmed, and A finishes again: it joins the run.
    ws.update(cx, |ws, cx| {
        ws.edit_automation(rule, |a| a.name = "Renamed".into(), cx);
        ws.save_automation(rule, cx).unwrap();
    });
    turn(handle, &ws, cx, a, &a_terminal);
    look(cx);
    confirmed(handle, &ws, cx, &[&b_terminal], 1, |ws| {
        ws.automations.get(rule).unwrap().last.is_some()
    });
    assert_eq!(
        runs_of(&ws, cx, rule),
        0,
        "the old run is not the new rule's"
    );
    // B's task ends with its turn: the joined event runs.
    turn(handle, &ws, cx, b, &b_terminal);
    rested(&ws, cx, b);
    look(cx);
    confirmed(handle, &ws, cx, &[&b_terminal], 2, |ws| {
        ws.automations.get(rule).unwrap().runs == 1
    });
}

#[gpui_kit::test]
fn the_same_who_option_keeps_the_choice(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = add(
        &ws,
        cx,
        on_event(Heard::Idle, chosen(&["abc12"]), say("Hi")),
    );
    open_popup(handle, &ws, cx);
    ws.update(cx, |ws, cx| ws.set_who_kind("blocks", cx));
    assert_eq!(
        draft_of(&ws, cx, id).when,
        When::Agent {
            event: Heard::Idle,
            who: chosen(&["abc12"])
        }
    );
    let kind = |kind: &str| When::Agent {
        event: Heard::Idle,
        who: Who::Kind { kind: kind.into() },
    };
    ws.update(cx, |ws, cx| {
        ws.edit_automation(id, |a| a.when = kind("codex"), cx);
        ws.set_who_kind("kind", cx);
    });
    assert_eq!(draft_of(&ws, cx, id).when, kind("codex"));
}

#[cfg(unix)]
#[gpui_kit::test]
fn turning_a_command_rule_off_and_on_forgets_the_count(cx: &mut TestAppContext) {
    let (_, ws, terminal, _) = two_agents(cx);
    let (_, uid) = me(&ws, cx, terminal);
    let rule = add(&ws, cx, on_command(&uid, false));
    look(cx);
    ws.update(cx, |ws, cx| ws.set_automation_on(rule, false, cx));
    command_ends(&ws, cx, terminal, Some(0));
    ws.update(cx, |ws, cx| ws.set_automation_on(rule, true, cx));
    look(cx);
    assert_eq!(
        runs_of(&ws, cx, rule),
        0,
        "what ended while it was off is not heard"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_run_being_typed_counts_toward_stop_after(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, a_terminal, _) = agent_at(handle, &ws, cx, 0);
    let (b, b_terminal, _) = agent_at(handle, &ws, cx, 1);
    let kind = Who::Kind {
        kind: "claude".into(),
    };
    let mut once = reply_to_firer("", Heard::Idle, kind);
    once.max_runs = Some(1);
    let rule = add(&ws, cx, once);
    let b_before = shown(&ws, cx, b);
    turn(handle, &ws, cx, a, &a_terminal);
    turn(handle, &ws, cx, b, &b_terminal);
    // A's task is being typed when B's event comes in the same look: it starts no second run.
    look(cx);
    history_with(&ws, cx, a, "from automation");
    for _ in 0..2 {
        look(cx);
    }
    ws.read_with(cx, |ws, _| assert_eq!(ws.next_task, 1, "one task only"));
    assert_eq!(shown(&ws, cx, b), b_before, "nothing typed into B");
    confirmed(handle, &ws, cx, &[&a_terminal], 1, |ws| {
        ws.automations.get(rule).unwrap().runs == 1
    });
    ws.read_with(cx, |ws, _| assert_eq!(ws.next_task, 1));
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_event_that_reaches_a_rule_turned_off_runs_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, uid) = agent_at(handle, &ws, cx, 0);
    let mut off = on_event(Heard::Idle, chosen(&[&uid]), say("Done"));
    off.on = false;
    off.cooldown = 0;
    let rule = add(&ws, cx, off);
    let before = notification_count(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.fire_event(rule, &uid, uid.clone(), SystemTime::now(), window, cx)
        })
    })
    .unwrap();
    assert_eq!(notification_count(handle, cx), before);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.get(rule).unwrap().last.is_none());
        assert_eq!(ws.automations.get(rule).unwrap().runs, 0);
    });
}
