//! The automations of a mirrored workspace (Templum to Templum, part 2h): home's run only at home,
//! and this computer's own never list, target or hear a remote workspace.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use chrono::NaiveTime;
use serde_json::json;
use templum_api::client::{Keys, SERVICE_DEV, entry_name};
use templum_api::code;
use templum_api::model::{AgentView, AutomationView, ScheduleView, State};
use templum_api::store::Scope;

use super::remote::{
    answer, apply, hear, live, notice, offline, pump_home, refusal, said, sent, show_remote,
    shown_mirror_as, terminal_of_home, terminals_of_home,
};
use super::terminals::set_active;
use super::*;
use crate::automations::{Automation, Automations, COOLDOWN, Do, Heard, Went, When, Who};
use crate::session::{self, IndexEntry, Keeping, RemoteEntry};
use crate::workspace::automations::{AUTOMATION_LOOK, NEW, unix};
use crate::workspace::remote_rules::{HOME_KEYS, home_key};
use crate::workspace::schedules::SCHEDULE_LOOK;

/// Home's rule `id` for its workspace `w1`: a daily Notify, due `due` seconds ago when some.
fn rule_of_home(id: u64, due: Option<i64>) -> AutomationView {
    AutomationView {
        id,
        name: format!("Rule {id}"),
        on: true,
        workspace: Some("w1".into()),
        when: json!({"daily": {"at": "09:00:00"}}),
        act: json!({"notify": {"text": "x"}}),
        cooldown: Some(COOLDOWN),
        next: due.map(|ago| unix(SystemTime::now()) - ago),
        sentence: "Every day at 09:00, notify you".into(),
        ..Default::default()
    }
}

fn state_with(rules: Vec<AutomationView>) -> State {
    State {
        automations: rules.into_iter().map(Arc::new).collect(),
        ..terminal_of_home(None)
    }
}

fn looks(cx: &mut TestAppContext, n: u32) {
    for _ in 0..n {
        cx.executor().advance_clock(AUTOMATION_LOOK);
        cx.run_until_parked();
    }
}

fn agent(state: &str) -> AgentView {
    AgentView {
        agent: "claude".into(),
        state: state.into(),
        ..Default::default()
    }
}

#[gpui_kit::test]
fn a_home_rule_whose_time_came_runs_nothing_on_the_mirror(cx: &mut TestAppContext) {
    let (handle, ws, dir) = offline(cx);
    show_remote(handle, &ws, cx);
    ws.update(cx, |ws, _| live(ws, Scope::Full));
    let event = AutomationView {
        id: 2,
        when: json!({"agent": {"event": "idle", "who": {"kind": "claude"}}}),
        next: None,
        ..rule_of_home(2, None)
    };
    apply(
        handle,
        &ws,
        cx,
        state_with(vec![rule_of_home(1, Some(60)), event]),
    );
    looks(cx, 3);
    cx.executor().advance_clock(crate::session::EVERY);
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.automations.is_empty());
        assert!(ws.automation_runs.is_empty());
        assert!(!ws.automating);
        assert!(!ws.automations_listen());
        assert_eq!(ws.automations_unseen(), 0);
        assert!(ws.api.notices.is_empty());
    });
    if let Ok(text) = std::fs::read_to_string(crate::automations::path(dir.path())) {
        assert!(text.contains("\"automations\": []"), "{text}");
    }
}

#[gpui_kit::test]
fn a_local_event_rule_never_hears_a_remote_agent_or_a_notice(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = offline(cx);
    let event = |event: Heard| Automation {
        id: 0,
        name: String::new(),
        on: true,
        workspace: None,
        when: When::Agent {
            event,
            who: Who::Kind {
                kind: "claude".into(),
            },
        },
        act: Do::Notify { text: "x".into() },
        max_runs: None,
        runs: 0,
        since: unix(SystemTime::now()),
        last: None,
        opened: None,
        cooldown: COOLDOWN,
    };
    let ids = ws.update(cx, |ws, cx| {
        let ids = [
            ws.automations.add(event(Heard::Idle)),
            ws.automations.add(event(Heard::NeedsYou)),
        ];
        ws.look_at_automations(cx);
        ids
    });
    show_remote(handle, &ws, cx);
    set_active(handle, true, cx);
    for state in ["working", "idle", "needs_you"] {
        apply(handle, &ws, cx, terminal_of_home(Some(agent(state))));
        frame(handle, cx);
    }
    hear(
        handle,
        &ws,
        cx,
        notice("[task t1 done] Green.", Some("abcde")),
    );
    looks(cx, 2);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations_listen());
        assert!(ws.automation_heard.is_empty());
        for id in ids {
            assert_eq!(ws.automations.get(id).unwrap().last, None);
        }
    });
}

/// A mirror whose `automations.json` holds `rules` before it opens, as an older Templum left it.
fn offline_with_rules(
    cx: &mut TestAppContext,
    rules: Vec<Automation>,
) -> (AnyWindowHandle, Entity<Workspace>) {
    let key = code::Key::new("tpl_NOTUSED".into());
    let code = code::decode(&code::encode("http://127.0.0.1:9", &key, None)).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = keyring_core::mock::Store::new().unwrap();
    let keys = Keys::with_store(store, SERVICE_DEV, Some(dir.path()));
    let entry = RemoteEntry {
        key: Keeping::None,
        ..RemoteEntry::of(&code, entry_name())
    };
    let mut index = session::Index::first_run();
    index.workspaces.push(IndexEntry {
        id: "r1".into(),
        name: "Connecting…".into(),
        remote: Some(entry),
        ..IndexEntry::default()
    });
    session::save_index(dir.path(), &index).unwrap();
    let next_id = rules.len() as u64;
    let file = Automations {
        version: crate::automations::VERSION,
        next_id,
        automations: rules,
    };
    crate::automations::save(dir.path(), &file, 1).unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.keep()));
    ws.update(cx, |ws, _| ws.keys = keys);
    (handle, ws)
}

#[gpui_kit::test]
fn a_stale_rule_for_a_remote_workspace_is_skipped_at_start_at_its_time_and_by_run_now(
    cx: &mut TestAppContext,
) {
    let two_days_ago = unix(SystemTime::now()) - 2 * 24 * 60 * 60;
    let stale = |id: u64, act: Do| Automation {
        id,
        name: String::new(),
        on: true,
        workspace: Some("r1".into()),
        when: When::Daily {
            at: NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
        },
        act,
        max_runs: None,
        runs: 0,
        since: two_days_ago,
        last: None,
        opened: None,
        cooldown: COOLDOWN,
    };
    let (handle, ws) = offline_with_rules(
        cx,
        vec![
            stale(1, Do::Notify { text: "x".into() }),
            stale(
                2,
                Do::Command {
                    terminal: None,
                    text: "ls".into(),
                    folder: Some("/tmp".into()),
                },
            ),
        ],
    );
    // Missed at start: nothing ran.
    ws.read_with(cx, |ws, _| {
        let last = ws.automations.get(1).unwrap().last.clone().unwrap();
        assert_eq!(
            (last.went, last.about.as_str()),
            (Went::Missed, "Templum was closed.")
        );
    });
    apply(handle, &ws, cx, terminal_of_home(None));
    let blocks = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.spaces()
                .map(|s| s.canvas.blocks().len())
                .collect::<Vec<_>>()
        })
    };
    let before = blocks(&ws, cx);
    ws.update(cx, |ws, _| {
        ws.automation_runs.entry(1).or_default().next =
            Some(SystemTime::now() - Duration::from_secs(1));
    });
    looks(cx, 1);
    let words = "Target missing: API is a mirror of 127.0.0.1:9: its automations run there.";
    ws.read_with(cx, |ws, _| {
        let last = ws.automations.get(1).unwrap().last.clone().unwrap();
        assert_eq!((last.went, last.about.as_str()), (Went::Skipped, words));
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(2, window, cx))
    })
    .unwrap();
    looks(cx, 1);
    ws.read_with(cx, |ws, _| {
        let last = ws.automations.get(2).unwrap().last.clone().unwrap();
        assert_eq!((last.went, last.about.as_str()), (Went::Skipped, words));
    });
    assert_eq!(blocks(&ws, cx), before);
}

#[gpui_kit::test]
fn the_mirror_s_own_rules_never_list_target_or_default_to_a_remote_workspace(
    cx: &mut TestAppContext,
) {
    let (handle, ws, _dir) = offline(cx);
    show_remote(handle, &ws, cx);
    apply(handle, &ws, cx, terminals_of_home(&["abcde"]));
    ws.update(cx, |ws, cx| ws.new_automation(cx));
    let first_local = ws.read_with(cx, |ws, _| {
        ws.order()
            .iter()
            .find(|s| !s.is_remote())
            .unwrap()
            .id
            .clone()
    });
    let draft = ws.read_with(cx, |ws, _| ws.automation_drafts[&NEW].clone());
    assert_eq!(draft.workspace.as_deref(), Some(first_local.as_str()));
    ws.read_with(cx, |ws, _| {
        let choices = crate::automations_view::editor::where_choices(ws, &draft);
        assert!(
            choices.iter().all(|(id, _)| id.as_ref() != "r1"),
            "{choices:?}"
        );
    });
    // A rule for every workspace, typing into a terminal: no viewer of the remote is offered.
    let everywhere = Automation {
        workspace: None,
        act: Do::Command {
            terminal: Some(String::new()),
            text: "ls".into(),
            folder: None,
        },
        ..draft.clone()
    };
    ws.read_with(cx, |ws, cx| {
        for agents in [false, true] {
            let offered = crate::automations_view::editor::blocks(ws, &everywhere, agents, cx);
            let remote_uids: Vec<_> = ws
                .spaces()
                .filter(|s| s.is_remote())
                .flat_map(|s| s.uids.values().cloned())
                .collect();
            assert!(
                offered
                    .iter()
                    .all(|(uid, _)| !remote_uids.contains(&uid.to_string())),
                "{offered:?}"
            );
        }
    });
    // A draft for the remote workspace is refused, and its choice keeps the mirror's name.
    ws.update(cx, |ws, cx| {
        ws.edit_automation(
            NEW,
            |a| {
                a.workspace = Some("r1".into());
                a.act = Do::Notify { text: "x".into() };
            },
            cx,
        );
        assert_eq!(
            ws.save_automation(NEW, cx),
            Err("API is a mirror of 127.0.0.1:9: its automations run there.".to_string())
        );
        assert!(ws.automations.automations.is_empty());
    });
    ws.read_with(cx, |ws, _| {
        let draft = ws.automation_drafts[&NEW].clone();
        let choices = crate::automations_view::editor::where_choices(ws, &draft);
        assert!(
            choices.iter().any(|(id, label)| id.as_ref() == "r1"
                && label.as_ref() == "API (mirror of 127.0.0.1:9)"),
            "{choices:?}"
        );
    });
}

#[gpui_kit::test]
fn home_s_schedules_are_never_delivered_on_the_mirror(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = offline(cx);
    let state = State {
        schedules: vec![Arc::new(ScheduleView {
            id: "s1".into(),
            by: Some("abcde".into()),
            to: "abcde".into(),
            text: "x".into(),
            next: unix(SystemTime::now()) - 60,
            ..Default::default()
        })],
        ..terminal_of_home(None)
    };
    apply(handle, &ws, cx, state);
    for _ in 0..2 {
        cx.executor().advance_clock(SCHEDULE_LOOK);
        cx.run_until_parked();
    }
    ws.read_with(cx, |ws, _| {
        assert!(ws.schedules.is_empty());
        assert!(!ws.scheduling);
        assert!(ws.notices.is_empty());
    });
}

#[gpui_kit::test]
fn home_s_task_and_role_are_cleaned_where_they_enter(cx: &mut TestAppContext) {
    use templum_api::model::TaskView;
    let (handle, ws, _dir) = offline(cx);
    show_remote(handle, &ws, cx);
    let agent = |id: &str| AgentView {
        task: Some(TaskView {
            id: id.into(),
            text: "a\x07".repeat(1 << 20),
            by: Some("b".repeat(500)),
            since: 1,
        }),
        role: Some("re\nviewer".into()),
        ..agent("working")
    };
    apply(handle, &ws, cx, terminal_of_home(Some(agent("t3"))));
    let pane = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let viewer = super::remote::remote_of(ws).homes["abcde"];
            ws.spaces()
                .find(|s| s.id == "r1")
                .and_then(|s| s.terminal(viewer))
                .and_then(|p| p.remote.as_ref().and_then(|r| r.agent.clone()))
                .unwrap()
        })
    };
    let kept = pane(&ws, cx);
    let task = kept.task.unwrap();
    assert!(task.text.chars().count() <= crate::workspace::remote::TASK_CHARS);
    assert!(!task.text.contains('\u{7}'));
    assert_eq!((task.id.as_str(), task.by), ("t3", None));
    assert_eq!(kept.role.as_deref(), Some("re viewer"));
    apply(handle, &ws, cx, terminal_of_home(Some(agent("t3\n"))));
    assert_eq!(pane(&ws, cx).task, None);
}

#[gpui_kit::test]
fn home_s_rules_and_schedules_are_copied_into_the_remote_never_into_the_mirror_s_own(
    cx: &mut TestAppContext,
) {
    use crate::workspace::remote_rules::home_key;
    let (handle, ws, _dir) = offline(cx);
    let mut other = rule_of_home(3, None);
    other.workspace = Some("w2".into());
    let schedule = ScheduleView {
        id: "s1".into(),
        by: Some("abcde".into()),
        to: "abcde".into(),
        text: "x".into(),
        next: 5,
        ..Default::default()
    };
    let state = State {
        schedules: vec![Arc::new(schedule)],
        ..state_with(vec![rule_of_home(1, None), rule_of_home(2, None), other])
    };
    apply(handle, &ws, cx, state);
    ws.read_with(cx, |ws, _| {
        let remote = super::remote::remote_of(ws);
        assert_eq!(
            remote.rules.iter().map(|r| r.id).collect::<Vec<_>>(),
            [1, 2]
        );
        assert_eq!(remote.rules[0].rule.id, home_key("r1", 1).unwrap());
        assert_eq!(remote.rules[0].rule.workspace.as_deref(), Some("w1"));
        assert_eq!(remote.schedules.len(), 1);
        assert!(ws.automations.automations.is_empty());
        assert!(ws.schedules.is_empty());
    });
    apply(handle, &ws, cx, terminal_of_home(None));
    ws.read_with(cx, |ws, _| {
        let remote = super::remote::remote_of(ws);
        assert!(remote.rules.is_empty() && remote.schedules.is_empty());
    });
}

/// A shown, live mirror of home's `w1` on a share of `scope`, with `rules` applied.
fn mirror_with(
    cx: &mut TestAppContext,
    scope: Scope,
    rules: Vec<AutomationView>,
) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let (handle, ws, dir) = offline(cx);
    show_remote(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        live(ws, scope);
        ws.space.remote.as_mut().unwrap().entry.home = Some("w1".into());
    });
    apply(handle, &ws, cx, state_with(rules));
    (handle, ws, dir)
}

fn key(id: u64) -> u64 {
    home_key("r1", id).unwrap()
}

fn open_popup(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_automations(window, cx))
    })
    .unwrap();
}

/// The popup selects `key` and draws it, so its form shows it.
fn show_rule(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, key: u64) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_automation(key, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

fn local_rule() -> Automation {
    Automation {
        id: 0,
        name: "Mine".into(),
        on: true,
        workspace: None,
        when: When::Daily {
            at: NaiveTime::from_hms_opt(9, 0, 0).unwrap(),
        },
        act: Do::Notify {
            text: "mine".into(),
        },
        max_runs: None,
        runs: 0,
        since: unix(SystemTime::now()),
        last: None,
        opened: None,
        cooldown: COOLDOWN,
    }
}

#[gpui_kit::test]
fn home_s_rules_are_listed_in_their_group_after_the_mirror_s_own(cx: &mut TestAppContext) {
    let mut failed = rule_of_home(1, Some(-3600));
    failed.last =
        Some(json!({"at": unix(SystemTime::now()) + 10, "went": "failed", "about": "no"}));
    let (handle, ws, _dir) = mirror_with(
        cx,
        Scope::Full,
        vec![rule_of_home(1, Some(-3600)), rule_of_home(2, None)],
    );
    // What home had done when it was first met is seen; the run after it is not.
    ws.read_with(cx, |ws, _| assert_eq!(ws.home_unseen(), 0));
    apply(
        handle,
        &ws,
        cx,
        state_with(vec![failed, rule_of_home(2, None)]),
    );
    let local = ws.update(cx, |ws, _| ws.automations.add(local_rule()));
    ws.read_with(cx, |ws, cx| {
        assert_eq!(ws.automation_ids(), [local, key(1), key(2)]);
        let groups = ws.rule_groups(cx);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].space, "r1");
        assert_eq!(groups[0].title, "On 127.0.0.1:9 · API");
        assert_eq!(groups[0].keys, [key(1), key(2)]);
        assert_eq!(
            groups[0].foot,
            "Rules for all of 127.0.0.1:9's workspaces aren't shown here."
        );
        let now = SystemTime::now();
        let next = ws.home_rule(key(1)).unwrap().1.next.unwrap();
        let want = crate::automations::clock::time_words(
            crate::workspace::automations::local(next).naive_local(),
            chrono::DateTime::<chrono::Local>::from(now).naive_local(),
        );
        assert_eq!(ws.next_words(key(1), now), want);
        assert_eq!(ws.next_words(key(2), now), "Never");
        assert_eq!(ws.home_unseen(), 1);
        assert_eq!(ws.automations_unseen(), 0);
    });
    open_popup(handle, &ws, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.home_unseen(), 0));
}

#[gpui_kit::test]
fn a_home_key_never_reaches_the_local_engine(cx: &mut TestAppContext) {
    let (handle, ws, dir) = mirror_with(cx, Scope::Full, vec![rule_of_home(1, None)]);
    let local = ws.update(cx, |ws, _| ws.automations.add(local_rule()));
    assert_eq!(local, 1, "home's rule 1 is also this computer's rule 1");
    let before = ws.read_with(cx, |ws, _| ws.automations.automations.clone());
    ws.update(cx, |ws, cx| {
        ws.set_automation_on(key(1), false, cx);
        assert_eq!(ws.duplicate_automation(key(1), cx), None);
        ws.delete_automation(key(1), cx);
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(key(1), window, cx))
    })
    .unwrap();
    let raw = rule_of_home(1, None);
    assert_eq!(
        sent(&ws, cx),
        [
            (
                "set_automation".to_string(),
                json!({"automation": 1, "on": false})
            ),
            (
                "save_automation".to_string(),
                json!({"automation": {
                    "name": "Rule 1 (copy)", "on": false, "workspace": "w1",
                    "when": raw.when, "do": raw.act, "cooldown": COOLDOWN,
                }})
            ),
            ("delete_automation".to_string(), json!({"automation": 1})),
            ("run_automation".to_string(), json!({"automation": 1})),
        ]
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations.automations, before);
        assert!(ws.automation_runs.keys().all(|k| *k < HOME_KEYS));
    });
    looks(cx, 2);
    assert!(
        !crate::automations::path(dir.path()).exists() || {
            let text = std::fs::read_to_string(crate::automations::path(dir.path())).unwrap();
            !text.contains("Rule 1")
        }
    );
}

fn command_rule(id: u64) -> AutomationView {
    AutomationView {
        act: json!({"command": {"text": "ls", "folder": "/tmp"}}),
        ..rule_of_home(id, None)
    }
}

#[gpui_kit::test]
fn a_home_rule_s_acts_ask_the_rule_first(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = mirror_with(
        cx,
        Scope::Control,
        vec![command_rule(1), rule_of_home(2, None)],
    );
    let act = |ws: &Entity<Workspace>, cx: &mut TestAppContext, key: u64| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.set_automation_on(key, false, cx);
                ws.duplicate_automation(key, cx);
                ws.delete_automation(key, cx);
                ws.run_automation_now(key, window, cx);
            })
        })
        .unwrap();
        cx.run_until_parked();
    };
    // A command rule needs a Full share: nothing is sent, and the words are said.
    act(&ws, cx, key(1));
    assert!(sent(&ws, cx).is_empty());
    let said_now = said(&ws, cx);
    assert!(
        said_now
            .iter()
            .all(|line| line == "Needs a Full share: share again on 127.0.0.1:9."),
        "{said_now:?}"
    );
    assert!(!said_now.is_empty());
    // A notify rule's switch, run and duplicate go; its delete needs Full. (The offline client
    // fails to connect while the test runs: the share is live again.)
    ws.update(cx, |ws, _| live(ws, Scope::Control));
    act(&ws, cx, key(2));
    let tools: Vec<String> = sent(&ws, cx).into_iter().map(|(tool, _)| tool).collect();
    assert_eq!(
        tools,
        ["set_automation", "save_automation", "run_automation"]
    );
    // A refusal answered to the switch is said.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.warn("x".into(), window, cx))
    })
    .unwrap();
    answer(handle, &ws, cx, 0, refusal("That automation is gone."));
    assert_eq!(
        said(&ws, cx).last().map(String::as_str),
        Some("That automation is gone.")
    );
    // Not connected: nothing is sent.
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = templum_api::client::Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        }
    });
    let sent_before = sent(&ws, cx).len();
    act(&ws, cx, key(2));
    assert_eq!(sent(&ws, cx).len(), sent_before);
    assert_eq!(
        said(&ws, cx).last().map(String::as_str),
        Some("Not connected.")
    );
}

#[gpui_kit::test]
fn a_home_schedule_is_listed_and_cancelled_at_home(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = mirror_with(cx, Scope::Full, vec![]);
    let mut state = terminals_of_home(&["b7k2m", "k9x1d"]);
    state.blocks = state
        .blocks
        .iter()
        .map(|b| {
            let mut b = (**b).clone();
            if b.id == "b7k2m" {
                b.title = "Claude Code".into();
            }
            Arc::new(b)
        })
        .collect();
    let next = unix(SystemTime::now()) + 600;
    state.schedules = vec![Arc::new(ScheduleView {
        id: "s3".into(),
        by: Some("b7k2m".into()),
        to: "k9x1d".into(),
        text: "Check the deploy\nsecond line".into(),
        next,
        every: Some(600),
        until: next + 1000,
    })];
    apply(handle, &ws, cx, state);
    let clock = crate::workspace::automations::local(next)
        .format("%H:%M")
        .to_string();
    ws.read_with(cx, |ws, cx| {
        let groups = ws.rule_groups(cx);
        assert_eq!(
            groups[0].schedules,
            [(
                "s3".to_string(),
                "s3 from Claude Code (b7k2m) to k9x1d".to_string(),
                format!("every 10 min · next {clock} · Check the deploy"),
            )]
        );
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.cancel_home_schedule("r1", "s3", window, cx))
    })
    .unwrap();
    assert_eq!(
        sent(&ws, cx),
        [("cancel_schedule".to_string(), json!({"schedule": "s3"}))]
    );
    ws.read_with(cx, |ws, _| assert!(ws.schedules.is_empty()));
}

#[gpui_kit::test]
fn a_forgotten_remote_takes_its_rules_and_drafts_with_it(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = mirror_with(cx, Scope::Full, vec![rule_of_home(1, None)]);
    ws.update(cx, |ws, cx| {
        ws.edit_automation(key(1), |a| a.name = "Edited".into(), cx);
        ws.automations_at = Some(key(1));
        assert!(ws.automation_drafts.contains_key(&key(1)));
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.forget_remote("r1", false, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, cx| {
        assert!(ws.automation_drafts.keys().all(|k| *k < HOME_KEYS));
        assert!(ws.automation_selected().is_none_or(|id| id < HOME_KEYS));
        assert!(ws.rule_groups(cx).is_empty());
    });
}

#[gpui_kit::test]
fn a_notice_of_a_home_rule_opens_the_popup_at_it_on_a_click(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = mirror_with(cx, Scope::Full, vec![rule_of_home(1, None)]);
    let heard = templum_api::model::Notice {
        automation: Some(1),
        ..notice("Morning review: Look", None)
    };
    hear(handle, &ws, cx, heard);
    assert_eq!(
        said(&ws, cx).last().map(String::as_str),
        Some("API on 127.0.0.1:9: Morning review: Look")
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_home_notice("r1", None, Some(1), window, cx)
        })
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations_open);
        assert_eq!(ws.automation_selected(), Some(key(1)));
    });
}

fn save(ws: &Entity<Workspace>, cx: &mut TestAppContext, key: u64) -> Result<u64, String> {
    ws.update(cx, |ws, cx| ws.save_automation(key, cx))
}

#[gpui_kit::test]
fn a_home_rule_edited_on_the_mirror_is_saved_at_home(cx: &mut TestAppContext) {
    let prompt = AutomationView {
        act: json!({"prompt": {"block": "b7k2m", "text": "Go on."}}),
        ..rule_of_home(1, None)
    };
    let (handle, ws, _dir) = mirror_with(cx, Scope::Full, vec![prompt]);
    ws.update(cx, |ws, cx| {
        ws.edit_automation(key(1), |a| a.name = "Renamed".into(), cx)
    });
    assert_eq!(save(&ws, cx, key(1)), Ok(1));
    assert_eq!(
        sent(&ws, cx),
        [(
            "save_automation".to_string(),
            json!({"id": 1, "automation": {
                "name": "Renamed", "on": true, "workspace": "w1",
                "when": {"daily": {"at": "09:00:00"}},
                "do": {"prompt": {"block": "b7k2m", "text": "Go on."}},
                "cooldown": COOLDOWN,
            }})
        )]
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.automation_drafts.contains_key(&key(1)))
    });
    answer(
        handle,
        &ws,
        cx,
        0,
        Ok(json!({"text": "Saved automation 1."})),
    );
    ws.read_with(cx, |ws, _| {
        assert!(!ws.automation_drafts.contains_key(&key(1)));
        assert!(ws.automations.automations.is_empty());
    });
}

#[gpui_kit::test]
fn a_new_home_rule_starts_on_home_s_workspace_and_lists_home_s_blocks(cx: &mut TestAppContext) {
    use crate::workspace::remote::HomeAgent;
    let (handle, ws, _dir) = offline(cx);
    show_remote(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        live(ws, Scope::Full);
        let remote = ws.space.remote.as_mut().unwrap();
        remote.entry.home = Some("w1".into());
        remote.agents = Some(vec![HomeAgent {
            id: "claude".into(),
            title: "Claude Code".into(),
            installed: true,
        }]);
    });
    let mut state = terminals_of_home(&["b7k2m", "k9x1d"]);
    state.blocks = state
        .blocks
        .iter()
        .map(|b| {
            let mut b = (**b).clone();
            if b.id == "b7k2m" {
                b.agent = Some(agent("working"));
            }
            Arc::new(b)
        })
        .collect();
    apply(handle, &ws, cx, state);
    let new = home_key("r1", 0).unwrap();
    ws.update(cx, |ws, cx| ws.new_home_automation("r1", cx));
    ws.read_with(cx, |ws, cx| {
        let draft = ws.automation_draft(new).unwrap();
        assert_eq!(draft.workspace.as_deref(), Some("w1"));
        assert_eq!(ws.automation_selected(), Some(new));
        let editor = crate::automations_view::editor::where_choices;
        let places: Vec<_> = editor(ws, draft)
            .into_iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect();
        assert_eq!(
            places,
            [("w1".to_string(), "API on 127.0.0.1:9".to_string())]
        );
        let labels = |agents: bool| {
            crate::automations_view::editor::blocks(ws, draft, agents, cx)
                .into_iter()
                .map(|(v, l)| (v.to_string(), l.to_string()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            labels(true),
            [("b7k2m".to_string(), "b7k2m (b7k2m) · API".to_string())]
        );
        assert_eq!(labels(false).len(), 2);
        let kinds: Vec<_> = ws
            .home_kinds("r1")
            .into_iter()
            .map(|(v, l)| (v.to_string(), l.to_string()))
            .collect();
        assert_eq!(kinds, [("claude".to_string(), "Claude Code".to_string())]);
    });
    // A blank text is the form's refusal; with text it is sent, without an id.
    assert_eq!(save(&ws, cx, new), Err("Write the text.".to_string()));
    assert!(sent(&ws, cx).is_empty());
    ws.update(cx, |ws, cx| {
        ws.edit_automation(
            new,
            |a| {
                a.act = Do::Notify {
                    text: "Look".into(),
                }
            },
            cx,
        )
    });
    assert_eq!(save(&ws, cx, new), Ok(0));
    let calls = sent(&ws, cx);
    assert_eq!(calls[0].0, "save_automation");
    assert!(calls[0].1.get("id").is_none(), "{:?}", calls[0].1);
    answer(
        handle,
        &ws,
        cx,
        0,
        Ok(json!({"text": "Saved automation 9."})),
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.automations_at, home_key("r1", 9));
        assert!(!ws.automation_drafts.contains_key(&new));
    });
}

#[gpui_kit::test]
fn a_command_rule_needs_a_full_share_to_be_saved_from_the_mirror(cx: &mut TestAppContext) {
    let (_, ws, _dir) = mirror_with(cx, Scope::Control, vec![]);
    let new = home_key("r1", 0).unwrap();
    ws.update(cx, |ws, cx| {
        ws.new_home_automation("r1", cx);
        ws.edit_automation(
            new,
            |a| {
                a.act = Do::Command {
                    terminal: None,
                    folder: None,
                    text: "ls".into(),
                }
            },
            cx,
        );
    });
    assert_eq!(
        save(&ws, cx, new),
        Err("Needs a Full share: share again on 127.0.0.1:9.".to_string())
    );
    assert!(sent(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn a_home_save_s_answer_lands_on_its_own_key(cx: &mut TestAppContext) {
    use super::inbox::{drawn, name};
    let (handle, ws, _dir) = mirror_with(
        cx,
        Scope::Full,
        vec![rule_of_home(1, None), rule_of_home(2, None)],
    );
    open_popup(handle, &ws, cx);
    show_rule(handle, &ws, cx, key(1));
    ws.update(cx, |ws, cx| {
        ws.edit_automation(key(1), |a| a.name = "One".into(), cx)
    });
    assert_eq!(save(&ws, cx, key(1)), Ok(1));
    // The person moves on to rule 2 before home answers.
    show_rule(handle, &ws, cx, key(2));
    answer(handle, &ws, cx, 0, refusal("That automation is gone."));
    assert!(!drawn(
        handle,
        cx,
        super::inbox::name("automation-problem".into())
    ));
    ws.read_with(cx, |ws, _| {
        assert!(ws.automation_drafts.contains_key(&key(1)))
    });
    // On rule 1 again, the same refusal is its red line.
    show_rule(handle, &ws, cx, key(1));
    assert_eq!(save(&ws, cx, key(1)), Ok(1));
    answer(handle, &ws, cx, 0, refusal("That automation is gone."));
    assert!(drawn(handle, cx, name("automation-problem".into())));
}

#[gpui_kit::test]
fn a_home_group_and_the_editor_on_a_home_rule_draw(cx: &mut TestAppContext) {
    use super::inbox::{drawn, name};
    for scope in [Scope::Full, Scope::Control] {
        let (handle, ws, _dir) = mirror_with(cx, scope, vec![command_rule(1)]);
        open_popup(handle, &ws, cx);
        show_rule(handle, &ws, cx, key(1));
        for id in [
            "automation-new-r1".to_string(),
            format!("automation-row-{}", key(1)),
            format!("automation-switch-{}", key(1)),
            "automation-save".to_string(),
            "automation-elsewhere".to_string(),
            "automation-where".to_string(),
        ] {
            assert!(drawn(handle, cx, name(id.clone())), "{id}");
        }
    }
}

#[gpui_kit::test]
fn a_rule_saved_and_run_from_the_mirror_is_home_s_alone(cx: &mut TestAppContext) {
    let ((home_handle, home), (handle, ws), _, [_home_dir, dir]) = shown_mirror_as(cx, Scope::Full);
    let new = home_key("r1", 0).unwrap();
    ws.update(cx, |ws, cx| {
        ws.new_home_automation("r1", cx);
        ws.edit_automation(
            new,
            |a| {
                a.name = "From the mirror".into();
                a.act = Do::Notify {
                    text: "Look".into(),
                };
            },
            cx,
        );
    });
    assert!(save(&ws, cx, new).is_ok());
    pump_home(&home, cx, "home to hold the rule", |cx| {
        let at_home = home.read_with(cx, |home, _| {
            home.automations
                .automations
                .iter()
                .any(|a| a.name == "From the mirror" && a.on)
        });
        at_home && ws.read_with(cx, |ws, _| !super::remote::remote_of(ws).rules.is_empty())
    });
    let (made, home_workspace) = home.read_with(cx, |home, _| {
        let rule = &home.automations.automations[0];
        (rule.id, rule.workspace.clone())
    });
    assert_eq!(
        home_workspace.as_deref(),
        Some(home.read_with(cx, |h, _| h.space.id.clone()).as_str())
    );
    let copied = key(made);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.automations.is_empty());
        assert!(ws.rule(copied).is_some());
    });
    // Run now on the mirror runs it at home.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.run_automation_now(copied, window, cx))
    })
    .unwrap();
    pump_home(&home, cx, "home to have run it", |cx| {
        home.read_with(cx, |home, _| {
            home.automations.get(made).is_some_and(|a| {
                a.last
                    .as_ref()
                    .is_some_and(|l| (l.went, l.about.as_str()) == (Went::Ran, "Notified."))
            })
        })
    });
    let heard = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.said
                .iter()
                .filter(|s| s.ends_with("From the mirror: Look"))
                .count()
        })
    };
    pump_home(&home, cx, "the mirror to hear it", |cx| heard(&ws, cx) == 1);
    for _ in 0..5 {
        pump_home(&home, cx, "a round", |_| true);
    }
    assert_eq!(heard(&ws, cx), 1);
    ws.read_with(cx, |ws, _| {
        assert!(ws.automation_runs.is_empty());
        assert!(ws.automations.automations.is_empty());
    });
    assert!(!crate::automations::path(dir.path()).exists());
    let _ = (home_handle, handle);
}

#[gpui_kit::test]
fn this_computer_s_own_api_saves_no_rule_for_a_remote_workspace(cx: &mut TestAppContext) {
    use super::api::{said, tool, with_token};
    let (handle, ws, _dir) = offline(cx);
    show_remote(handle, &ws, cx);
    apply(handle, &ws, cx, terminal_of_home(None));
    let full = with_token(&ws, cx, Scope::Full, None);
    let rule = json!({ "name": "x", "workspace": "r1",
        "when": { "daily": { "at": "09:00:00" } }, "do": { "notify": { "text": "x" } } });
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "save_automation",
        json!({ "automation": rule }),
    );
    let (refused, words) = (line["error"].is_object() || said(&line).0, line.to_string());
    assert!(refused, "{words}");
    assert!(words.contains("its automations run there"), "{words}");
    ws.read_with(cx, |ws, _| assert!(ws.automations.automations.is_empty()));
}

#[gpui_kit::test]
fn a_run_failing_at_home_counts_whatever_home_s_clock_says(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = mirror_with(cx, Scope::Full, vec![rule_of_home(1, None)]);
    open_popup(handle, &ws, cx);
    // Home's clock is ten minutes behind this one's.
    let behind = unix(SystemTime::now()) - 600;
    let last = |went: &str| Some(json!({"at": behind, "went": went, "about": "x"}));
    let mut ran = rule_of_home(1, None);
    ran.last = last("ran");
    apply(handle, &ws, cx, state_with(vec![ran]));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.home_unseen(), 0, "a run is no news")
    });
    let mut failed = rule_of_home(1, None);
    failed.last = last("failed");
    apply(handle, &ws, cx, state_with(vec![failed]));
    ws.read_with(cx, |ws, _| assert_eq!(ws.home_unseen(), 1));
    open_popup(handle, &ws, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.home_unseen(), 0));
}

#[gpui_kit::test]
fn a_second_click_on_a_home_rule_s_acts_sends_nothing_before_home_answers(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = mirror_with(cx, Scope::Full, vec![rule_of_home(1, None)]);
    // A new rule: Save twice makes one rule at home.
    let new = home_key("r1", 0).unwrap();
    ws.update(cx, |ws, cx| {
        ws.new_home_automation("r1", cx);
        ws.edit_automation(
            new,
            |a| {
                a.name = "New".into();
                a.act = Do::Notify {
                    text: "Look".into(),
                };
            },
            cx,
        );
    });
    assert_eq!(save(&ws, cx, new), Ok(0));
    ws.read_with(cx, |ws, _| assert!(ws.home_saving(new)));
    assert!(save(&ws, cx, new).is_ok());
    assert_eq!(sent(&ws, cx).len(), 1);
    answer(
        handle,
        &ws,
        cx,
        0,
        Ok(json!({"text": "Saved automation 2."})),
    );
    ws.read_with(cx, |ws, _| assert!(!ws.home_saving(new)));
    // The others: a switch, a duplicate, a run and a delete go once each.
    let act = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                for _ in 0..2 {
                    ws.set_automation_on(key(1), false, cx);
                    ws.duplicate_automation(key(1), cx);
                    ws.run_automation_now(key(1), window, cx);
                    ws.delete_automation(key(1), cx);
                }
            })
        })
        .unwrap();
    };
    act(&ws, cx);
    let tools = |ws: &Entity<Workspace>, cx: &mut TestAppContext| -> Vec<String> {
        sent(ws, cx).into_iter().map(|(tool, _)| tool).collect()
    };
    let once = [
        "set_automation",
        "save_automation",
        "run_automation",
        "delete_automation",
    ];
    assert_eq!(tools(&ws, cx), once);
    // Answered, each can go again.
    for _ in 0..4 {
        answer(handle, &ws, cx, 0, Ok(json!({"text": "Done."})));
    }
    act(&ws, cx);
    assert_eq!(tools(&ws, cx), once);
}

#[gpui_kit::test]
fn a_home_rule_the_copy_had_to_cut_is_not_saved_from_the_mirror(cx: &mut TestAppContext) {
    let long = AutomationView {
        act: json!({"notify": {"text": "p".repeat(70_000)}}),
        ..rule_of_home(1, None)
    };
    let crlf = AutomationView {
        act: json!({"notify": {"text": "a\r\nb"}}),
        ..rule_of_home(2, None)
    };
    let (_handle, ws, _dir) = mirror_with(cx, Scope::Full, vec![long, crlf, rule_of_home(3, None)]);
    for id in [1, 2] {
        ws.update(cx, |ws, cx| {
            ws.edit_automation(key(id), |a| a.name = "Renamed".into(), cx)
        });
        assert_eq!(
            save(&ws, cx, key(id)),
            Err("This rule is longer than a mirror shows: edit it on 127.0.0.1:9.".to_string())
        );
    }
    assert!(sent(&ws, cx).is_empty());
    // A rule that was not cut is saved.
    ws.update(cx, |ws, cx| {
        ws.edit_automation(key(3), |a| a.name = "Renamed".into(), cx)
    });
    assert_eq!(save(&ws, cx, key(3)), Ok(3));
    assert_eq!(sent(&ws, cx).len(), 1);
}
