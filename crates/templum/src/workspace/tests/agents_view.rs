//! The Agents view's model: what each agent does, its watch, and the rows it makes
//! (docs/specs/agents-view.md).

use std::time::{Duration, Instant};

use serde_json::json;
use templum_terminal::Outcome;

use super::bus::{call_later, event_message, me, screen_with};
use super::delegate::found;
use super::inbox::{asks, click_id, drawn, name, open_terminal};
use super::subagents::hook;
use super::terminals::set_active;
use super::*;
use crate::agents_view::{dot_color, empty_words, header_text, tone};
use crate::alert::Alert;
use crate::automations::Heard;
use crate::board::Assignee;
use crate::workspace::agents_view::{AgentRow, Doing, Show, Watch, by_state, hold, matches};
use crate::workspace::subagents::Follows;
use crate::workspace::{AgentState, Handed};

/// A terminal at row `row` of the canvas whose agent runs, as a snapshot found it: its block and
/// its terminal.
pub(super) fn agent(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    row: u8,
) -> (BlockId, String) {
    let id = open_terminal(handle, ws, cx, row);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().seen.agent = Some("claude".into())
    });
    found(ws, cx, id);
    (id, me(ws, cx, id).0)
}

/// `ws.doing` of the terminal block `id`.
fn doing(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Doing {
    ws.read_with(cx, |ws, _| ws.doing(ws.space.terminal(id).unwrap()))
}

#[test]
fn a_states_words() {
    assert_eq!(Doing::NeedsYou("Allow Bash?".into()).label(), "Needs you");
    assert_eq!(Doing::Working(0).label(), "Working");
    assert_eq!(Doing::Working(1).label(), "Working · 1 subagent");
    assert_eq!(Doing::Working(2).label(), "Working · 2 subagents");
    assert_eq!(Doing::AwaitsSubagents(1).label(), "Waiting for 1 subagent");
    assert_eq!(Doing::AwaitsSubagents(3).label(), "Waiting for 3 subagents");
    assert_eq!(
        (
            Doing::Done.label(),
            Doing::Idle.label(),
            Doing::Starting.label()
        ),
        ("Done".into(), "Idle".into(), "Starting".into())
    );
    assert!(Show::Working.shows(&Doing::AwaitsSubagents(1)) && Show::Idle.shows(&Doing::Starting));
    assert!(!Show::Done.shows(&Doing::Idle));
    for s in Show::ALL {
        assert_eq!(
            s.shows(&Doing::Working(0)),
            matches!(s, Show::All | Show::Working)
        );
    }
}

#[test]
fn the_watch_says_a_turns_end_and_a_new_need_once() {
    let now = Instant::now();
    let mut w = Watch::default();
    assert_eq!(
        w.see(Doing::NeedsYou("Allow?".into()), now),
        None,
        "first sight"
    );
    assert_eq!(
        w.see(Doing::NeedsYou("Other?".into()), now),
        None,
        "the same wait"
    );
    assert_eq!(w.see(Doing::Working(0), now), None);
    assert_eq!(w.see(Doing::AwaitsSubagents(1), now), None);
    assert_eq!(w.see(Doing::Idle, now), Some(Heard::Idle));
    assert_eq!(w.see(Doing::Idle, now), None);
    assert_eq!(
        w.see(Doing::NeedsYou(String::new()), now),
        Some(Heard::NeedsYou)
    );
    assert_eq!(
        w.see(Doing::Idle, now),
        None,
        "a wait that ends is no turn's end"
    );
}

#[gpui_kit::test]
fn a_rows_state_follows_its_dot(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (id, terminal) = agent(handle, &ws, cx, 0);
    let set =
        |ws: &Entity<Workspace>, cx: &mut TestAppContext, f: &dyn Fn(&mut super::super::Pane)| {
            ws.update(cx, |ws, _| f(ws.space.terminal_mut(id).unwrap()));
        };

    set(&ws, cx, &|p| p.state = Some(AgentState::Working));
    assert_eq!(doing(&ws, cx, id), Doing::Working(0));
    set(&ws, cx, &|p| {
        p.add_subagent("a1".into());
        p.add_subagent("a2".into());
    });
    assert_eq!(doing(&ws, cx, id), Doing::Working(2));
    set(&ws, cx, &|p| {
        p.awaits_subagents = true;
        p.subagents[0].reported = true;
    });
    assert_eq!(doing(&ws, cx, id), Doing::AwaitsSubagents(2));
    // A flag with no subagent left to wait for is plain work.
    set(&ws, cx, &|p| p.subagents.clear());
    assert_eq!(doing(&ws, cx, id), Doing::Working(0));
    set(&ws, cx, &|p| p.awaits_subagents = false);

    set(&ws, cx, &|p| {
        p.state = Some(AgentState::NeedsYou(Some("Allow Bash?".into())))
    });
    assert_eq!(doing(&ws, cx, id), Doing::NeedsYou("Allow Bash?".into()));

    set(&ws, cx, &|p| {
        p.state = Some(AgentState::Working);
        p.alert = Some(Alert::Attention(Some(
            "Claude needs your permission".into(),
        )));
    });
    assert_eq!(
        doing(&ws, cx, id),
        Doing::NeedsYou("Claude needs your permission".into())
    );

    // A command's news isn't the agent's.
    set(&ws, cx, &|p| {
        p.alert = Some(Alert::Finished(Outcome {
            code: Some(1),
            took: Duration::from_secs(9),
        }))
    });
    assert_eq!(doing(&ws, cx, id), Doing::Working(0));
    set(&ws, cx, &|p| p.alert = None);

    set(&ws, cx, &|p| p.state = Some(AgentState::Idle));
    assert_eq!(doing(&ws, cx, id), Doing::Idle);
    set(&ws, cx, &|p| p.state = None);
    assert_eq!(doing(&ws, cx, id), Doing::Idle);

    // The dot ignores a state its snapshot can't back.
    set(&ws, cx, &|p| {
        p.seen.agent = None;
        p.flags_for = Some("claude".into());
        p.state = Some(AgentState::Working);
    });
    assert_eq!(doing(&ws, cx, id), Doing::Idle);
    set(&ws, cx, &|p| p.seen.agent = Some("claude".into()));

    ws.update(cx, |ws, cx| ws.cover_start(id, "Claude Code".into(), cx));
    assert_eq!(doing(&ws, cx, id), Doing::Starting);
    set(&ws, cx, &|p| p.starting = None);

    // A request of its own, whatever its state says.
    let _ = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "ask_user",
        json!({ "question": "Ship it?", "options": ["Yes"] }),
    );
    assert_eq!(doing(&ws, cx, id), Doing::NeedsYou("Ship it?".into()));
    // A command's news doesn't hide it.
    set(&ws, cx, &|p| {
        p.alert = Some(Alert::Finished(Outcome {
            code: Some(1),
            took: Duration::from_secs(9),
        }))
    });
    assert_eq!(doing(&ws, cx, id), Doing::NeedsYou("Ship it?".into()));
}

/// A turn of the agent in `terminal` starts.
pub(super) fn begins(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    terminal: &str,
) {
    let input = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "go" });
    hook(handle, ws, cx, terminal, input);
}

/// A turn of the agent in `terminal` ends.
pub(super) fn ends(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    terminal: &str,
) {
    let input = json!({ "hook_event_name": "Stop", "last_assistant_message": "ok" });
    hook(handle, ws, cx, terminal, input);
}

/// What the row of the terminal block `id` shows, and since when.
fn row(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> (Doing, Instant) {
    ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(id)
            .unwrap()
            .watch
            .shown()
            .expect("watched")
    })
}

fn shown(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Doing {
    row(ws, cx, id).0
}

fn focus(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(id, window, cx))
    })
    .unwrap();
}

/// An agent in a workspace of its own, which is hidden again: the first workspace is shown.
fn hidden_agent(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (BlockId, String) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx))
    })
    .unwrap();
    let hidden = agent(handle, ws, cx, 0);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx))
    })
    .unwrap();
    hidden
}

/// Two agents `a` and `b`, `a` focused in the window in front, each at the end of a turn
/// the person did not see: `b` is Done.
pub(super) fn one_done(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> ((BlockId, String), (BlockId, String)) {
    set_active(handle, true, cx);
    let a = agent(handle, ws, cx, 0);
    let b = agent(handle, ws, cx, 1);
    focus(handle, ws, cx, a.0);
    for (_, terminal) in [&a, &b] {
        begins(handle, ws, cx, terminal);
    }
    frame(handle, cx);
    for (_, terminal) in [&a, &b] {
        ends(handle, ws, cx, terminal);
    }
    frame(handle, cx);
    (a, b)
}

#[gpui_kit::test]
fn a_turn_ending_unseen_marks_its_agent_done(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, b) = one_done(handle, &ws, cx);
    assert_eq!(shown(&ws, cx, b.0), Doing::Done);
    assert_eq!(shown(&ws, cx, a.0), Doing::Idle, "looked at while it ended");
    assert_eq!(ws.read_with(cx, |ws, _| ws.done_count()), 1);
}

#[gpui_kit::test]
fn looking_at_or_working_again_ends_done(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, b) = one_done(handle, &ws, cx);
    focus(handle, &ws, cx, b.0);
    frame(handle, cx);
    assert_eq!(shown(&ws, cx, b.0), Doing::Idle);
    assert_eq!(ws.read_with(cx, |ws, _| ws.done_count()), 0);

    // Done again; it works, and its next end is seen: it is focused.
    let c = agent(handle, &ws, cx, 2);
    focus(handle, &ws, cx, b.0);
    begins(handle, &ws, cx, &c.1);
    frame(handle, cx);
    ends(handle, &ws, cx, &c.1);
    frame(handle, cx);
    assert_eq!(shown(&ws, cx, c.0), Doing::Done);
    begins(handle, &ws, cx, &c.1);
    assert_eq!(shown(&ws, cx, c.0), Doing::Working(0));
    focus(handle, &ws, cx, c.0);
    ends(handle, &ws, cx, &c.1);
    assert_eq!(shown(&ws, cx, c.0), Doing::Idle);

    // The window in the background looks at nothing.
    set_active(handle, false, cx);
    begins(handle, &ws, cx, &c.1);
    ends(handle, &ws, cx, &c.1);
    assert_eq!(shown(&ws, cx, c.0), Doing::Done);
    set_active(handle, true, cx);
    frame(handle, cx);
    assert_eq!(shown(&ws, cx, c.0), Doing::Idle);
}

#[gpui_kit::test]
fn done_is_aged_from_the_event_not_the_next_frame(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    set_active(handle, true, cx);
    let a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    let c = agent(handle, &ws, cx, 2);
    focus(handle, &ws, cx, a.0);
    for (_, terminal) in [&b, &c] {
        begins(handle, &ws, cx, terminal);
    }
    frame(handle, cx);
    let since = row(&ws, cx, b.0).1;
    ws.update(cx, |ws, _| {
        ws.space
            .terminal_mut(b.0)
            .unwrap()
            .add_subagent("a1".into())
    });
    frame(handle, cx);
    assert_eq!(shown(&ws, cx, b.0), Doing::Working(1));
    assert_eq!(row(&ws, cx, b.0).1, since, "the same kind keeps its age");

    // No frame is drawn while the window is hidden.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(b.0).unwrap().subagents.clear()
    });
    ends(handle, &ws, cx, &b.1);
    cx.executor().advance_clock(Duration::from_millis(1_000));
    ends(handle, &ws, cx, &c.1);
    cx.executor().advance_clock(Duration::from_millis(1_000));
    frame(handle, cx);
    let now = cx.executor().now();
    assert_eq!(shown(&ws, cx, b.0), Doing::Done);
    assert!(row(&ws, cx, c.0).1 > row(&ws, cx, b.0).1);
    assert!(now - row(&ws, cx, b.0).1 >= Duration::from_secs(2));
}

#[gpui_kit::test]
fn an_agent_that_quits_takes_its_done_mark_with_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, b) = one_done(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(b.0).unwrap();
        pane.seen.agent = None;
        pane.flags_for = None;
    });
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(b.0).unwrap();
        assert!(pane.watch.was.is_none() && !pane.watch.done);
        assert_eq!(ws.done_count(), 0);
    });
    // A new agent in the same block starts with no mark.
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(b.0).unwrap();
        pane.seen.agent = Some("claude".into());
        pane.state = Some(AgentState::Idle);
    });
    frame(handle, cx);
    assert_eq!(shown(&ws, cx, b.0), Doing::Idle);
}

#[gpui_kit::test]
fn mark_all_seen_clears_every_done_mark(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, b) = one_done(handle, &ws, cx);
    // A hidden workspace's agent, Done too.
    let (hidden, _) = hidden_agent(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.others[0].terminal_mut(hidden).unwrap().state = Some(AgentState::Working)
    });
    frame(handle, cx);
    ws.update(cx, |ws, _| {
        ws.others[0].terminal_mut(hidden).unwrap().state = Some(AgentState::Idle)
    });
    frame(handle, cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.done_count()), 2);
    ws.update(cx, |ws, cx| ws.mark_all_seen(cx));
    assert_eq!(ws.read_with(cx, |ws, _| ws.done_count()), 0);
    assert_eq!(shown(&ws, cx, b.0), Doing::Idle);
    ws.read_with(cx, |ws, _| {
        let pane = ws.others[0].terminal(hidden).unwrap();
        assert_eq!(pane.watch.shown().unwrap().0, Doing::Idle);
    });

    // A stopped workspace's agents never count.
    ws.update(cx, |ws, _| {
        let pane = ws.others[0].terminal_mut(hidden).unwrap();
        pane.watch.done = true;
    });
    assert_eq!(ws.read_with(cx, |ws, _| ws.done_count()), 1);
    ws.update(cx, |ws, _| ws.others[0].stopped = true);
    assert_eq!(ws.read_with(cx, |ws, _| ws.done_count()), 0);
}

/// A row of terminal `terminal` titled `title` in workspace `space`, the rest empty.
fn made(space: &str, terminal: &str, title: &str, doing: Doing, since: Instant) -> AgentRow {
    AgentRow {
        space: space.into(),
        space_name: String::new(),
        terminal: terminal.into(),
        title: title.into(),
        agent: String::new(),
        doing,
        since,
        card: None,
        task: None,
        folder: None,
        context: None,
        dim: false,
    }
}

/// The terminals of `rows`.
fn terminals(rows: &[AgentRow]) -> Vec<&str> {
    rows.iter().map(|r| r.terminal.as_str()).collect()
}

/// The rows the popup lists now, as terminals.
fn listed(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<String> {
    ws.read_with(cx, |ws, _| {
        ws.listed_agents().into_iter().map(|r| r.terminal).collect()
    })
}

#[test]
fn rows_order_by_state_then_title() {
    let t0 = Instant::now();
    let s = Duration::from_secs(1);
    let mut rows = vec![
        made("g", "i3", "same", Doing::Idle, t0),
        made("g", "d1", "x", Doing::Done, t0),
        made("g", "k", "x", Doing::Working(0), t0),
        made("g", "i1", "beta", Doing::Idle, t0),
        made("g", "n2", "x", Doing::NeedsYou(String::new()), t0 + s),
        made("g", "w", "x", Doing::AwaitsSubagents(1), t0),
        made("g", "i4", "same", Doing::Idle, t0),
        made("g", "st", "gamma", Doing::Starting, t0),
        made("g", "d2", "x", Doing::Done, t0 + s),
        made("g", "i2", "Alpha", Doing::Idle, t0),
        made("g", "n1", "x", Doing::NeedsYou(String::new()), t0),
    ];
    // Both "same" ones fall back to their terminals.
    for row in &mut rows {
        match row.terminal.as_str() {
            "i3" => row.terminal = "t4".into(),
            "i4" => row.terminal = "t3".into(),
            _ => {}
        }
    }
    rows.sort_by(by_state);
    assert_eq!(
        terminals(&rows),
        [
            "n1", "n2", "d2", "d1", "w", "k", "i2", "i1", "st", "t3", "t4"
        ]
    );
}

#[test]
fn a_search_matches_title_agent_folder_workspace_card_and_task() {
    let mut row = made("s", "t", "Review", Doing::Idle, Instant::now());
    row.agent = "Codex".into();
    row.folder = Some("/home/me/parser".into());
    row.space_name = "Workspace 2".into();
    row.card = Some((152, "Attachments".into()));
    row.task = Some("fix the parser\nthen tests".into());
    for query in [
        "review",
        "codex",
        "parser",
        "workspace 2",
        "#152",
        "152",
        "attach",
        "then tests",
    ] {
        assert!(matches(&row, query), "{query}");
    }
    assert!(!matches(&row, "zzz"));
    let bare = made("s", "t", "Review", Doing::Idle, Instant::now());
    assert!(matches(&bare, "review") && !matches(&bare, "152") && !matches(&bare, "codex"));
}

#[gpui_kit::test]
fn rows_list_every_running_agent_but_stopped_ones_and_subagents(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    open_terminal(handle, &ws, cx, 2);
    // A subagent's block.
    let s = agent(handle, &ws, cx, 3);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(s.0).unwrap().follows = Some(Follows {
            parent: a.1.clone(),
            id: "s1".into(),
            done: None,
        })
    });
    let (_, x) = hidden_agent(handle, &ws, cx);
    let (stopped_id, y) = hidden_agent(handle, &ws, cx);
    let _ = stopped_id;
    ws.update(cx, |ws, _| {
        ws.others
            .iter_mut()
            .find(|s| s.terminals().any(|(_, p)| p.term.id() == y))
            .unwrap()
            .stopped = true
    });
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        let rows = ws.agent_rows();
        let mut listed = terminals(&rows);
        listed.sort_unstable();
        let mut want = [a.1.as_str(), b.1.as_str(), x.as_str()];
        want.sort_unstable();
        assert_eq!(listed, want);
        let row = rows.iter().find(|r| r.terminal == x).unwrap();
        assert_eq!(row.space_name, "Workspace 2");
    });
}

#[gpui_kit::test]
fn rows_group_by_workspace_shown_first(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let o = agent(handle, &ws, cx, 0);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx))
    })
    .unwrap();
    let k = agent(handle, &ws, cx, 0);
    let n = agent(handle, &ws, cx, 1);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(k.0).unwrap().state = Some(AgentState::Working);
        ws.space.terminal_mut(n.0).unwrap().state = Some(AgentState::NeedsYou(None));
    });
    frame(handle, cx);
    assert_eq!(listed(&ws, cx), [n.1.clone(), k.1.clone(), o.1.clone()]);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    assert_eq!(listed(&ws, cx), [o.1, n.1, k.1]);
}

#[gpui_kit::test]
fn the_card_and_the_task_flow_into_rows_and_filters_narrow_them(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    let c = agent(handle, &ws, cx, 2);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let uid = me(&ws, cx, a.0).1;
    ws.update(cx, |ws, _| {
        ws.edit_boards(|boards| {
            boards.edit_card(board, card, |c| {
                c.title = "Attachments".into();
                c.assignee = Some(Assignee::Block(uid.clone()));
            })
        });
        let pane = ws.space.terminal_mut(b.0).unwrap();
        pane.handed = Some(Handed {
            text: "fix the parser\nthen tests".into(),
            ..Handed::new("x", Duration::from_secs(600))
        });
        ws.space.terminal_mut(b.0).unwrap().state = Some(AgentState::Working);
    });
    // `c` finished unseen.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(c.0).unwrap().state = Some(AgentState::Working)
    });
    frame(handle, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(c.0).unwrap().state = Some(AgentState::Idle)
    });
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        let rows = ws.agent_rows();
        let of = |t: &str| rows.iter().find(|r| r.terminal == t).unwrap();
        assert_eq!(of(&a.1).card, Some((card, "Attachments".into())));
        assert_eq!(of(&b.1).task.as_deref(), Some("fix the parser\nthen tests"));
        assert_eq!(of(&c.1).doing, Doing::Done);
    });
    let query = |ws: &Entity<Workspace>, cx: &mut TestAppContext, q: &str| {
        ws.update(cx, |ws, _| ws.agents_query = q.into())
    };
    query(&ws, cx, "then tests");
    assert_eq!(listed(&ws, cx), std::slice::from_ref(&b.1));
    query(&ws, cx, "attach");
    assert_eq!(listed(&ws, cx), std::slice::from_ref(&a.1));
    query(&ws, cx, "");
    for (show, want) in [(Show::Working, &b.1), (Show::Done, &c.1)] {
        ws.update(cx, |ws, _| ws.agents_show = show);
        assert_eq!(listed(&ws, cx), std::slice::from_ref(want));
    }
    ws.update(cx, |ws, _| ws.agents_show = Show::All);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.counts_in(&ws.agent_rows())),
        [3, 0, 1, 1, 1]
    );
    query(&ws, cx, "then tests");
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.counts_in(&ws.agent_rows())),
        [1, 0, 1, 0, 0]
    );
}

#[test]
fn the_order_holds_while_the_pointer_is_over_the_list() {
    let t = Instant::now();
    let idle = |space: &str, terminal: &str| made(space, terminal, terminal, Doing::Idle, t);
    let fresh = vec![
        idle("g1", "a"),
        idle("g1", "c"),
        idle("g1", "e"),
        idle("g2", "d"),
    ];
    let held: Vec<String> = ["b", "c", "a", "d"].map(String::from).into();
    assert_eq!(terminals(&hold(fresh, &held)), ["c", "a", "e", "d"]);
}

#[gpui_kit::test]
fn the_listed_order_is_kept_under_the_pointer(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    // Same state and title: their terminals order them, as text ("…-100" before "…-94").
    let mut first = agent(handle, &ws, cx, 0);
    let mut second = agent(handle, &ws, cx, 1);
    if second.1 < first.1 {
        std::mem::swap(&mut first, &mut second);
    }
    // Listed as the popup does each frame; the pointer then comes over it.
    frame(handle, cx);
    assert_eq!(listed(&ws, cx), [first.1.clone(), second.1.clone()]);
    ws.update(cx, |ws, _| ws.agents_hover = true);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(second.0).unwrap().state = Some(AgentState::NeedsYou(None))
    });
    frame(handle, cx);
    assert_eq!(listed(&ws, cx), [first.1.clone(), second.1.clone()]);
    ws.update(cx, |ws, _| ws.agents_hover = false);
    assert_eq!(listed(&ws, cx), [second.1, first.1]);
}

#[gpui_kit::test]
fn a_moved_block_keeps_its_done_mark_in_its_new_group(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, b) = one_done(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.move_blocks(vec![b.0], None, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        let rows = ws.agent_rows();
        let row = rows.iter().find(|r| r.terminal == b.1).expect("listed");
        assert_eq!(row.doing, Doing::Done);
        let new = &ws.others[0];
        assert_eq!((&row.space, &row.space_name), (&new.id, &new.name));
        assert_ne!(row.space, ws.space.id);
        assert_eq!(rows.iter().filter(|r| r.space == row.space).count(), 1);
    });
}

#[cfg(target_os = "macos")]
const ALL_AGENTS: &str = "cmd-shift-l";
#[cfg(not(target_os = "macos"))]
const ALL_AGENTS: &str = "ctrl-shift-l";

pub(super) fn open_agents(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_agents(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

pub(super) fn press(handle: AnyWindowHandle, cx: &mut TestAppContext, key: &str) {
    cx.update_window(handle, |_, window, cx| {
        window.press(key, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// The terminal of the agent the popup has selected.
fn selected(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Option<String> {
    ws.read_with(cx, |ws, _| ws.agents_selected().map(|row| row.terminal))
}

fn keys(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    cx.update_window(handle, |_, window, cx| ws.read(cx).agents_keys(window, cx))
        .unwrap()
}

#[gpui_kit::test]
fn git_requests_and_agents_are_never_open_together(cx: &mut TestAppContext) {
    let repo = committed_repo();
    let (handle, ws) = open(cx);
    let only = |cx: &mut TestAppContext, git, inbox, agents| {
        ws.read_with(cx, |ws, _| {
            assert_eq!(
                (ws.git_open, ws.inbox_open, ws.agents_open),
                (git, inbox, agents)
            )
        })
    };
    open_git(handle, &ws, cx, repo.path());
    only(cx, true, false, false);
    open_agents(handle, &ws, cx);
    only(cx, false, false, true);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_inbox(None, window, cx))
    })
    .unwrap();
    only(cx, false, true, false);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_git(window, cx))
    })
    .unwrap();
    only(cx, true, false, false);
    for open in [true, false] {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.toggle_agents(window, cx))
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.agents_open, open));
    }
    // The shortcut works with the sidebar hidden too.
    ws.update(cx, |ws, _| ws.sidebar = false);
    press(handle, cx, ALL_AGENTS);
    only(cx, false, false, true);
    press(handle, cx, ALL_AGENTS);
    only(cx, false, false, false);
}

#[gpui_kit::test]
fn opening_gives_the_search_the_keyboard(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    // Right of the popup, which sits at the canvas's bottom left.
    let b = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open(Rect::new(900.0, 60.0, 300.0, 300.0), window, cx);
                ws.space.appearing.clear();
                ws.space.canvas.top().unwrap()
            })
        })
        .unwrap();
    // The block opened has the focus: it goes back to the first.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(a, window, cx))
    })
    .unwrap();
    open_agents(handle, &ws, cx);
    assert!(keys(handle, &ws, cx), "opened with the keyboard");
    cx.simulate_input(handle, "rev");
    ws.read_with(cx, |ws, _| assert_eq!(ws.agents_query, "rev"));
    // A press outside closes it, as any popup's backdrop does: the terminal under it gets nothing.
    let to = at(&ws, cx, (1050.0, 210.0));
    cx.update_window(handle, |_, window, cx| {
        click(window, cx, to, 1);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(a), "the terminal got nothing");
        assert!(!ws.agents_open, "the popup closed");
    });
    assert!(
        !keys(handle, &ws, cx),
        "the keyboard went back to the canvas"
    );
    // Closing never moves the canvas's focused block.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(b, window, cx))
    })
    .unwrap();
    open_agents(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close_agents(window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(b)));
    // Closed with the pointer over the list: reopened, the order is not held.
    ws.update(cx, |ws, _| ws.agents_hover = true);
    open_agents(handle, &ws, cx);
    ws.read_with(cx, |ws, _| assert!(!ws.agents_hover));
}

#[gpui_kit::test]
fn the_selection_follows_the_agent(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    let _c = agent(handle, &ws, cx, 2);
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_agent(&b.1, window, cx))
    })
    .unwrap();
    assert_eq!(selected(&ws, cx), Some(b.1.clone()));
    // It moves first, and stays selected.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(b.0).unwrap().state = Some(AgentState::NeedsYou(None))
    });
    frame(handle, cx);
    assert_eq!(selected(&ws, cx), Some(b.1.clone()));
    assert_eq!(listed(&ws, cx)[0], b.1);
    // Its block closes: the first row is selected.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(b.0, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    let first = listed(&ws, cx).into_iter().next();
    assert!(first.is_some() && first.as_ref() != Some(&b.1));
    assert_eq!(selected(&ws, cx), first);
    assert!(listed(&ws, cx).contains(&a.1));
    ws.update(cx, |ws, cx| ws.set_agents_show(Show::Done, cx));
    assert_eq!(selected(&ws, cx), None);
}

#[gpui_kit::test]
fn arrows_enter_and_esc(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    set_active(handle, true, cx);
    agent(handle, &ws, cx, 0);
    agent(handle, &ws, cx, 1);
    hidden_agent(handle, &ws, cx);
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    let rows = ws.read_with(cx, |ws, _| ws.listed_agents());
    assert_eq!(rows.len(), 3);
    let at = |cx: &mut TestAppContext| selected(&ws, cx);
    assert_eq!(at(cx), Some(rows[0].terminal.clone()));
    press(handle, cx, "down");
    press(handle, cx, "down");
    assert_eq!(at(cx), Some(rows[2].terminal.clone()), "across the header");
    press(handle, cx, "down");
    assert_eq!(at(cx), Some(rows[2].terminal.clone()), "no wrap");
    press(handle, cx, "up");
    assert_eq!(at(cx), Some(rows[1].terminal.clone()));
    press(handle, cx, "enter");
    ws.read_with(cx, |ws, _| {
        assert!(!ws.agents_open);
        let (space, id) = ws.caller(&rows[1].terminal).unwrap();
        assert_eq!(ws.space.id, space);
        assert_eq!(ws.space.focused, Some(id));
    });
    open_agents(handle, &ws, cx);
    cx.simulate_input(handle, "zz");
    press(handle, cx, "escape");
    ws.read_with(cx, |ws, _| {
        assert!(ws.agents_query.is_empty() && ws.agents_open)
    });
    press(handle, cx, "escape");
    ws.read_with(cx, |ws, _| assert!(!ws.agents_open));
}

#[gpui_kit::test]
fn go_to_terminal_clears_done(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    set_active(handle, true, cx);
    let (x, x_terminal) = hidden_agent(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.others[0].terminal_mut(x).unwrap().state = Some(AgentState::Working)
    });
    frame(handle, cx);
    ws.update(cx, |ws, _| {
        ws.others[0].terminal_mut(x).unwrap().state = Some(AgentState::Idle)
    });
    frame(handle, cx);
    assert_eq!(ws.read_with(cx, |ws, _| ws.done_count()), 1);
    open_agents(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.go_to_agent(&x_terminal, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.focused, Some(x));
        assert!(!ws.agents_open);
    });
    assert_eq!(shown(&ws, cx, x), Doing::Idle);
}

#[gpui_kit::test]
fn open_request_opens_the_inbox_at_its_request(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (x, x_terminal) = hidden_agent(handle, &ws, cx);
    let _answers = asks(handle, &ws, cx, &x_terminal, 1);
    let key = ws.read_with(cx, |ws, _| ws.waits.last().expect("a wait").key);
    open_agents(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_agent_request(&x_terminal, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(x).is_some(), "its workspace is shown");
        assert!(ws.inbox_open && !ws.agents_open);
        assert_eq!(ws.inbox_selected().map(|w| w.key), Some(key));
    });
}

#[gpui_kit::test]
fn the_button_counts_done_agents(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    set_active(handle, true, cx);
    assert!(drawn(handle, cx, name("agents-button".into())));
    assert!(!drawn(handle, cx, name("agents-badge".into())));
    let (x, _) = hidden_agent(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.others[0].terminal_mut(x).unwrap().state = Some(AgentState::Working)
    });
    frame(handle, cx);
    ws.update(cx, |ws, _| {
        ws.others[0].terminal_mut(x).unwrap().state = Some(AgentState::Idle)
    });
    assert!(drawn(handle, cx, name("agents-badge".into())));
    ws.update(cx, |ws, cx| ws.mark_all_seen(cx));
    assert!(!drawn(handle, cx, name("agents-badge".into())));
    click_id(handle, cx, name("agents-button".into()));
    ws.read_with(cx, |ws, _| assert!(ws.agents_open));
    click_id(handle, cx, name("agents-button".into()));
    ws.read_with(cx, |ws, _| assert!(!ws.agents_open));
}

/// A long folder or task is cut at the list's edge: the row never reaches into the details.
#[gpui_kit::test]
fn a_row_keeps_to_the_lists_width(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let a = agent(handle, &ws, cx, 0);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a.0).unwrap().seen.folder = Some("/a-long-folder".repeat(40).into())
    });
    open_agents(handle, &ws, cx);
    let width = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let row = window.try_find(name(format!("agents-row-{}", a.1)));
            row.expect("its row").bounds().size.width
        })
        .unwrap();
    assert!(width <= px(crate::agents_view::LIST_W), "{width:?}");
}

#[gpui_kit::test]
fn rows_details_and_buttons_are_drawn(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    set_active(handle, true, cx);
    let a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    let _answers = asks(handle, &ws, cx, &b.1, 1);
    ws.update(cx, |ws, _| {
        ws.space.terminal(a.0).unwrap().term.type_in("hello\r")
    });
    screen_with(&ws, cx, a.0, "hello");
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    let row = |t: &str| name(format!("agents-row-{t}"));
    assert!(drawn(handle, cx, row(&a.1)) && drawn(handle, cx, row(&b.1)));
    click_id(handle, cx, row(&b.1));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.agents_at.as_deref(), Some(b.1.as_str()))
    });
    assert!(drawn(handle, cx, name("agents-request".into())));
    assert!(drawn(handle, cx, name("agents-go".into())));
    click_id(handle, cx, row(&a.1));
    assert!(!drawn(handle, cx, name("agents-request".into())));
    assert!(drawn(handle, cx, name("agents-screen".into())));
    click_id(handle, cx, name("agents-go".into()));
    ws.read_with(cx, |ws, _| {
        assert!(!ws.agents_open);
        assert_eq!(ws.space.focused, Some(a.0));
    });

    // The chips narrow the list; Mark all seen clears what is Done.
    open_agents(handle, &ws, cx);
    click_id(handle, cx, name("agents-chip-Done".into()));
    ws.read_with(cx, |ws, _| assert_eq!(ws.agents_show, Show::Done));
    assert!(drawn(handle, cx, name("agents-empty".into())));
    focus(handle, &ws, cx, b.0);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a.0).unwrap().state = Some(AgentState::Working)
    });
    frame(handle, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a.0).unwrap().state = Some(AgentState::Idle)
    });
    assert!(drawn(handle, cx, name("agents-seen".into())));
    click_id(handle, cx, name("agents-seen".into()));
    assert_eq!(ws.read_with(cx, |ws, _| ws.done_count()), 0);
}

#[gpui_kit::test]
fn only_the_rows_in_view_are_drawn(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    for row in 0..30 {
        agent(handle, &ws, cx, row);
    }
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    let rows = listed(&ws, cx);
    assert_eq!(rows.len(), 30);
    let row = |t: &str| name(format!("agents-row-{t}"));
    assert!(drawn(handle, cx, row(&rows[0])));
    assert!(!drawn(handle, cx, row(&rows[29])), "below the fold");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_agent(&rows[29], window, cx))
    })
    .unwrap();
    frame(handle, cx);
    assert!(drawn(handle, cx, row(&rows[29])), "the list scrolled to it");
}

#[test]
fn a_group_header_counts_who_needs_you() {
    assert_eq!(header_text("Templum", 0), "Templum");
    assert_eq!(header_text("Templum", 1), "Templum · 1 needs you");
    assert_eq!(header_text("Templum", 2), "Templum · 2 need you");
}

#[test]
fn the_empty_list_says_whether_an_agent_runs() {
    assert_eq!(empty_words(0), "No agent is running");
    assert_eq!(empty_words(3), "No agent matches");
}

#[gpui_kit::test]
fn a_done_row_has_no_dot_but_its_words_are_accent(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, b) = one_done(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        let row = ws
            .agent_rows()
            .into_iter()
            .find(|r| r.terminal == b.1)
            .unwrap();
        assert_eq!(row.doing, Doing::Done);
        let pane = ws.pane(&b.1);
        assert_eq!(dot_color(ws, pane), None, "its block shows no dot");
        assert_eq!(tone(ws, &row, pane), Some(ws.ui.accent));
    });
}

#[gpui_kit::test]
fn a_selection_the_list_lost_stays_where_it_fell(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let _a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    let _c = agent(handle, &ws, cx, 2);
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_agent(&b.1, window, cx))
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(b.0, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    let shown = selected(&ws, cx).expect("the first row");
    // The other one needs the person: it moves first, and the selection stays.
    let (other, id) = ws.read_with(cx, |ws, _| {
        let row = ws
            .listed_agents()
            .into_iter()
            .find(|r| r.terminal != shown)
            .unwrap();
        (row.terminal.clone(), ws.caller(&row.terminal).unwrap().1)
    });
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().state = Some(AgentState::NeedsYou(None))
    });
    frame(handle, cx);
    assert_eq!(listed(&ws, cx)[0], other, "it moved first");
    assert_eq!(selected(&ws, cx), Some(shown));
}

/// With nothing chosen, the row shown first stays selected when another moves above it.
#[gpui_kit::test]
fn the_row_shown_stays_selected_as_the_order_changes(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let _a = agent(handle, &ws, cx, 0);
    let _b = agent(handle, &ws, cx, 1);
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    frame(handle, cx);
    let shown = selected(&ws, cx).expect("the first row");
    let id = ws.read_with(cx, |ws, _| {
        let other = ws.listed_agents().into_iter().find(|r| r.terminal != shown);
        ws.caller(&other.unwrap().terminal).unwrap().1
    });
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().state = Some(AgentState::NeedsYou(None))
    });
    frame(handle, cx);
    assert_ne!(listed(&ws, cx)[0], shown, "the other moved first");
    assert_eq!(selected(&ws, cx), Some(shown));
}

/// A turn ends in the bus's own call, before any frame: the watch has heard it by then.
#[gpui_kit::test]
fn the_bus_notes_a_turns_end_before_any_frame(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    set_active(handle, true, cx);
    let a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    focus(handle, &ws, cx, a.0);
    begins(handle, &ws, cx, &b.1);
    frame(handle, cx);
    let input = json!({ "hook_event_name": "Stop", "last_assistant_message": "ok" });
    let said = templum_mcp::hooks::claude(&input).expect("an event");
    let message = event_message(&b.1, serde_json::to_value(said).unwrap());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.serve(message, window, cx);
            let shown = ws.space.terminal(b.0).unwrap().watch.shown();
            assert!(matches!(shown, Some((Doing::Done, _))), "{shown:?}");
        })
    })
    .unwrap();
}

/// A look at the screens that finds an agent idle notes it, before any frame.
#[gpui_kit::test]
fn a_look_notes_a_turns_end_before_any_frame(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    set_active(handle, true, cx);
    let a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    focus(handle, &ws, cx, a.0);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(b.0).unwrap().state = Some(AgentState::Working)
    });
    frame(handle, cx);
    ws.update(cx, |ws, cx| {
        ws.space.terminal_mut(b.0).unwrap().state = Some(AgentState::Idle);
        ws.look_around(cx);
        let shown = ws.space.terminal(b.0).unwrap().watch.shown();
        assert!(matches!(shown, Some((Doing::Done, _))), "{shown:?}");
    });
}

#[gpui_kit::test]
fn a_done_row_draws_a_ring_and_a_working_row_a_dot(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, b) = one_done(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a.0).unwrap().state = Some(AgentState::Working)
    });
    open_agents(handle, &ws, cx);
    let id = |what: &str, t: &str| name(format!("agents-{what}-{t}"));
    assert!(
        drawn(handle, cx, id("ring", &b.1)),
        "Done has no dot of its block"
    );
    assert!(!drawn(handle, cx, id("dot", &b.1)));
    assert!(
        drawn(handle, cx, id("dot", &a.1)),
        "Working has its block's"
    );
    assert!(!drawn(handle, cx, id("ring", &a.1)));
}

#[gpui_kit::test]
fn a_choice_the_search_hid_comes_back_with_the_search(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let a = agent(handle, &ws, cx, 0);
    let b = agent(handle, &ws, cx, 1);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a.0).unwrap().handed = Some(Handed {
            text: "alpha".into(),
            ..Handed::new("x", Duration::from_secs(600))
        })
    });
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_agent(&b.1, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    // Typed: only a matches, and is shown selected.
    ws.update(cx, |ws, _| ws.agents_query = "alpha".into());
    frame(handle, cx);
    assert_eq!(selected(&ws, cx), Some(a.1.clone()));
    // Erased: the person's choice is theirs again.
    ws.update(cx, |ws, _| ws.agents_query.clear());
    frame(handle, cx);
    assert_eq!(selected(&ws, cx), Some(b.1.clone()));
}

// On a mirror: the agents of another computer (Templum to Templum, part 2h).

use super::remote::{answer, apply, offline, remote_of, sent, show_remote, terminal_of_home};
use crate::workspace::agents_view::{AGENT_SCREEN_AGAIN, SCREEN_LINES};
use crate::workspace::remote::HomeAgent;
use templum_api::store::Scope;

fn home_agent(state: &str) -> templum_api::model::AgentView {
    templum_api::model::AgentView {
        agent: "claude".into(),
        state: state.into(),
        model: Some("opus".into()),
        effort: Some("high".into()),
        role: Some("reviewer".into()),
        task: Some(templum_api::model::TaskView {
            id: "t3".into(),
            text: "Ship it.".into(),
            by: Some("api".into()),
            since: 1,
        }),
        ..Default::default()
    }
}

/// An offline mirror whose home runs Claude Code in `abcde`, in the state `state`, not shown.
fn mirrored_agent(
    cx: &mut TestAppContext,
    state: &str,
) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let (handle, ws, dir) = offline(cx);
    apply(handle, &ws, cx, terminal_of_home(Some(home_agent(state))));
    frame(handle, cx);
    (handle, ws, dir)
}

#[gpui_kit::test]
fn a_remote_agent_is_listed_under_its_computer_with_home_s_details(cx: &mut TestAppContext) {
    let (handle, ws, dir) = offline(cx);
    let _keep = dir;
    ws.update(cx, |ws, _| {
        let space = ws.others.iter_mut().find(|s| s.id == "r1").unwrap();
        space.remote.as_mut().unwrap().agents = Some(vec![HomeAgent {
            id: "claude".into(),
            title: "Claude Code".into(),
            installed: true,
        }]);
    });
    apply(
        handle,
        &ws,
        cx,
        terminal_of_home(Some(home_agent("working"))),
    );
    frame(handle, cx);
    let rows = ws.read_with(cx, |ws, _| ws.agent_rows());
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(
        (
            row.space.as_str(),
            row.space_name.as_str(),
            row.title.as_str(),
            row.agent.as_str(),
            row.task.as_deref(),
            row.dim
        ),
        (
            "r1",
            "API · 127.0.0.1:9",
            "claude",
            "Claude Code",
            Some("Ship it."),
            false
        )
    );
    // Go to terminal shows the remote workspace with the viewer focused.
    let terminal = row.terminal.clone();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.go_to_agent(&terminal, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, "r1");
        assert_eq!(ws.space.focused, Some(remote_of(ws).homes["abcde"]));
    });
    // Not live any more: still listed, dim.
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = templum_api::client::Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        }
    });
    let rows = ws.read_with(cx, |ws, _| ws.agent_rows());
    assert_eq!(rows.len(), 1);
    assert!(rows[0].dim);
}

#[gpui_kit::test]
fn a_remote_agent_is_done_here_until_looked_at_here_and_home_s_done_reads_idle(
    cx: &mut TestAppContext,
) {
    let (handle, ws, _dir) = offline(cx);
    set_active(handle, true, cx);
    let state = |cx: &mut TestAppContext, word: &str| {
        apply(handle, &ws, cx, terminal_of_home(Some(home_agent(word))));
        frame(handle, cx);
    };
    let doing_now = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.agent_rows().first().map(|r| r.doing.clone()))
    };
    state(cx, "working");
    state(cx, "idle");
    assert_eq!(doing_now(&ws, cx), Some(Doing::Done));
    ws.read_with(cx, |ws, _| assert_eq!(ws.done_count(), 1));
    // Looked at here: the viewer focused in the workspace shown.
    show_remote(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        let viewer = remote_of(ws).homes["abcde"];
        ws.space.focused = Some(viewer);
    });
    frame(handle, cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.done_count(), 0));
    assert_eq!(doing_now(&ws, cx), Some(Doing::Idle));
    // Home's own "done" is idle here; after working, it is this computer's watch that makes Done.
    state(cx, "working");
    state(cx, "done");
    ws.update(cx, |ws, _| ws.space.focused = None);
    state(cx, "working");
    state(cx, "done");
    assert_eq!(doing_now(&ws, cx), Some(Doing::Done));
    // Looked at, a state that is still "done" reads Idle.
    ws.update(cx, |ws, _| {
        let viewer = remote_of(ws).homes["abcde"];
        ws.space.focused = Some(viewer);
    });
    state(cx, "done");
    assert_eq!(doing_now(&ws, cx), Some(Doing::Idle));
}

#[gpui_kit::test]
fn a_screen_read_keeps_its_last_twelve_clean_lines(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = offline(cx);
    // The start's connecting (nothing to connect to) settles first.
    cx.run_until_parked();
    apply(
        handle,
        &ws,
        cx,
        terminal_of_home(Some(home_agent("working"))),
    );
    ws.update(cx, |ws, _| {
        if let Some(r) = ws
            .others
            .iter_mut()
            .find(|s| s.id == "r1")
            .and_then(|s| s.remote.as_mut())
        {
            r.status = templum_api::client::Status::Live;
            r.me = Some(templum_api::model::MeView {
                scope: Scope::Read,
                ..Default::default()
            });
        }
    });
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    let row = ws.read_with(cx, |ws, _| ws.agent_rows().remove(0));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_agent(&row.terminal, window, cx))
    })
    .unwrap();
    let reads = |cx: &mut TestAppContext| {
        sent(&ws, cx)
            .into_iter()
            .filter(|(tool, _)| tool == "read_terminal")
            .collect::<Vec<_>>()
    };
    assert_eq!(
        reads(cx),
        [(
            "read_terminal".to_string(),
            json!({ "block": "abcde", "lines": SCREEN_LINES })
        )]
    );
    let long: Vec<String> = (0..10_000)
        .map(|n| match n {
            9_995 => "red \u{1b}[31mtext".to_string(),
            n => format!("line {n}"),
        })
        .collect();
    answer(handle, &ws, cx, 0, Ok(json!({ "text": long.join("\n") })));
    ws.read_with(cx, |ws, _| {
        let (block, lines) = remote_of(ws).screen.clone().expect("a screen");
        assert_eq!(block, "abcde");
        assert_eq!(lines.len(), SCREEN_LINES);
        assert_eq!(lines.last().map(String::as_str), Some("line 9999"));
        assert!(
            lines.iter().all(|l| !l.chars().any(char::is_control)),
            "{lines:?}"
        );
    });
    // Again after a while, while the popup shows it.
    let again = |cx: &mut TestAppContext| {
        ws.update(cx, |ws, _| {
            if let Some(r) = ws
                .others
                .iter_mut()
                .find(|s| s.id == "r1")
                .and_then(|s| s.remote.as_mut())
            {
                r.status = templum_api::client::Status::Live;
            }
        });
        cx.executor().advance_clock(AGENT_SCREEN_AGAIN);
        cx.run_until_parked();
    };
    ws.update(cx, |ws, _| {
        ws.others
            .iter_mut()
            .find(|s| s.id == "r1")
            .unwrap()
            .remote
            .as_mut()
            .unwrap()
            .status = templum_api::client::Status::Live;
    });
    let before = reads(cx).len();
    again(cx);
    assert_eq!(reads(cx).len(), before + 1);
    // Closed: no more.
    let newest = reads_index(&ws, cx);
    answer(handle, &ws, cx, newest, Ok(json!({ "text": "x" })));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close_agents(window, cx))
    })
    .unwrap();
    let before = reads(cx).len();
    again(cx);
    assert_eq!(reads(cx).len(), before);
}

/// The index among the kept calls of the newest `read_terminal`.
fn reads_index(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> usize {
    ws.read_with(cx, |ws, _| {
        ws.calls
            .iter()
            .rposition(|(_, call, _)| call.tool == "read_terminal")
            .unwrap()
    })
}

#[gpui_kit::test]
fn a_streamed_remote_agent_s_screen_is_not_read(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = mirrored_agent(cx, "working");
    ws.update(cx, |ws, _| {
        let r = ws
            .others
            .iter_mut()
            .find(|s| s.id == "r1")
            .unwrap()
            .remote
            .as_mut()
            .unwrap();
        r.status = templum_api::client::Status::Live;
        r.me = Some(templum_api::model::MeView {
            scope: Scope::Read,
            ..Default::default()
        });
        let viewer = r.homes["abcde"];
        r.streaming = vec![("abcde".into(), viewer)];
    });
    open_agents(handle, &ws, cx);
    let row = ws.read_with(cx, |ws, _| ws.agent_rows().remove(0));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_agent(&row.terminal, window, cx))
    })
    .unwrap();
    assert!(sent(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn a_remote_agent_s_details_and_screen_draw(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = mirrored_agent(cx, "working");
    cx.run_until_parked();
    ws.update(cx, |ws, _| {
        let r = ws
            .others
            .iter_mut()
            .find(|s| s.id == "r1")
            .unwrap()
            .remote
            .as_mut()
            .unwrap();
        r.status = templum_api::client::Status::Live;
        r.me = Some(templum_api::model::MeView {
            scope: Scope::Read,
            ..Default::default()
        });
        r.screen = Some(("abcde".into(), vec!["$ cargo test".into(), "ok".into()]));
    });
    open_agents(handle, &ws, cx);
    let row = ws.read_with(cx, |ws, _| ws.agent_rows().remove(0));
    assert!(drawn(
        handle,
        cx,
        name(format!("agents-row-{}", row.terminal))
    ));
    assert!(drawn(handle, cx, name("agents-screen".into())));
}

#[gpui_kit::test]
fn a_task_time_of_home_far_before_the_epoch_draws_without_overflow(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = offline(cx);
    let mut agent = home_agent("working");
    agent.task.as_mut().unwrap().since = i64::MIN;
    apply(handle, &ws, cx, terminal_of_home(Some(agent)));
    frame(handle, cx);
    open_agents(handle, &ws, cx);
    let row = ws.read_with(cx, |ws, _| ws.agent_rows().remove(0));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_agent(&row.terminal, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}
