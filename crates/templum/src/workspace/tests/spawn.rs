//! `spawn_agent`: an agent opened beside the caller (docs/specs/connections.md, Spawning). `cat`
//! stands in for the agents: every Unix has it, and it echoes what it is typed. Unix only (`mod.rs`).

use std::time::Duration;

use serde_json::json;
use templum_canvas::Wire;

use super::bus::{
    after, answer_soon, answered, call, call_later, event, me, result_of, screen_until, screen_with,
};
use super::delegate::{entered, found, prints, snapshot};
use super::*;

/// The prompt "say hi" went with its Enter: the terminal echoed it and `cat` printed it back,
/// maybe after its echo of the command typed before it (`cat` may start reading late).
#[track_caller]
fn said_twice(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) {
    screen_until(ws, cx, id, |shown| shown.matches("say hi").count() >= 2);
}

/// Looks whether the agent is ready for its prompt, READY_LOOK of the test's clock each, while
/// `waiting` holds; false if it still does after ten seconds: what a look reads (the shell starting
/// the agent, its output) comes in real time, slower on a busy machine.
fn typed(cx: &mut TestAppContext, waiting: impl Fn(&mut TestAppContext) -> bool) -> bool {
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while waiting(cx) {
        if std::time::Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
        after(cx, look);
    }
    true
}

/// The startup terminal at (40, 100), 400 × 300, the camera at 100% on the origin, and Claude Code
/// and Codex started as `cat` in front of the new terminals' shells (`agent_cat`).
pub(super) fn spawner(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Workspace>, BlockId) {
    let (handle, ws) = open(cx);
    let mine = first_id(&ws, cx);
    ws.update(cx, |ws, _| {
        // Its agents' input boxes bracket pastes.
        ws.command = Some(jobs("printf '\\033[?2004h'; cat; true"));
        ws.space.pristine = false;
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 1.0);
        ws.space.canvas.get_mut(mine).unwrap().rect = Rect::new(40.0, 100.0, 400.0, 300.0);
        ws.space.focused = Some(mine);
        for agent in &mut ws.agents {
            if agent.id == "claude" || agent.id == "codex" {
                agent.command = "cat".into();
            }
        }
    });
    (handle, ws, mine)
}

/// The block spawned beside `mine`: the other end of its newest wire.
pub(super) fn spawned(ws: &Entity<Workspace>, cx: &mut TestAppContext, mine: BlockId) -> BlockId {
    ws.read_with(cx, |ws, _| {
        let wire = ws
            .space
            .wires
            .iter()
            .rfind(|w| w.touches(mine))
            .expect("wired to the caller");
        if wire.a == mine { wire.b } else { wire.a }
    })
}

/// The Enter a typed prompt waits for.
fn enter(cx: &mut TestAppContext) {
    after(
        cx,
        crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
    );
}

#[gpui_kit::test]
fn spawn_agent_opens_a_wired_agent_beside_the_caller(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    // The person selected a wire.
    let wire = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open(Rect::new(40.0, 500.0, 400.0, 300.0), window, cx);
                let other = ws.space.canvas.top().unwrap();
                assert!(ws.connect(mine, other, cx));
                let wire = Wire::new(mine, other);
                (ws.space.focused, ws.space.selected_wire) = (Some(mine), wire);
                wire
            })
        })
        .unwrap();
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path() });
    let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    assert!(!failed, "{text}");
    let new = spawned(&ws, cx, mine);
    let (_, uid) = me(&ws, cx, new);
    assert!(
        text.contains(&uid) && text.contains(&folder.path().display().to_string()),
        "{text}"
    );
    ws.read_with(cx, |ws, _| {
        let rect = ws.space.canvas.get(new).unwrap().rect;
        assert!(
            rect.x > 440.0 && rect.y == 100.0,
            "beside the caller: {rect:?}"
        );
        assert_eq!(ws.space.focused, Some(mine), "the keyboard stays");
        assert_eq!(ws.space.selected_wire, wire, "the selection stays");
        assert_eq!(
            ws.space.terminal(new).unwrap().seen.agent.as_deref(),
            Some("codex")
        );
    });
    assert_eq!(
        folder_of(&ws, new, cx),
        folder.path().canonicalize().unwrap()
    );
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "spawn_agent",
        json!({ "agent": "nope" }),
    );
    assert!(failed && text.contains("no agent nope"), "{text}");
    let nowhere = json!({ "agent": "codex", "folder": "/no/such/folder" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", nowhere);
    assert!(failed && text.contains("isn't a folder"), "{text}");
}

/// Templum never sees an agent in WSL start, so it could not type its prompt: the Templum Agent's
/// open shares the refusal (`Workspace::spawn_problem`).
#[gpui_kit::test]
fn spawn_agent_refuses_an_agent_that_runs_in_wsl(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    super::agents::agent_shells(&ws, cx);
    ws.update(cx, |ws, _| {
        let claude = ws.agents.iter_mut().find(|a| a.id == "claude").unwrap();
        claude.shell = Some("WSL".into());
    });
    let blocks = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.blocks.len())
    };
    let before = blocks(&ws, cx);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "spawn_agent",
        json!({ "agent": "claude" }),
    );
    assert!(failed, "{text}");
    assert_eq!(
        text,
        "Claude Code runs in WSL, where Templum can't see it start nor type its prompt: open it from the canvas."
    );
    assert_eq!(blocks(&ws, cx), before, "no block opened");
}

/// The model and the effort go right after the agent's command (`cat` here), before what Templum
/// adds; anything it can't type safely opens nothing, a worktree included.
#[gpui_kit::test]
fn spawn_agent_types_its_model_and_effort_first(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path(), "model": "gpt-5.6-luna", "effort": "high" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    assert!(
        !failed && text.ends_with("connected to you, on gpt-5.6-luna (high)."),
        "{text}"
    );
    let new = spawned(&ws, cx, mine);
    screen_with(
        &ws,
        cx,
        new,
        "cat -m gpt-5.6-luna -c model_reasoning_effort=high",
    );
    ws.read_with(cx, |ws, _| {
        let p = ws.space.terminal(new).unwrap();
        assert_eq!(
            (
                p.opened_by.as_deref(),
                p.model.as_deref(),
                p.effort.as_deref()
            ),
            (Some(uid.as_str()), Some("gpt-5.6-luna"), Some("high"))
        );
    });
    let spawn =
        json!({ "agent": "claude", "folder": folder.path(), "model": "opus", "effort": "max" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    assert!(!failed, "{text}");
    let second = spawned(&ws, cx, mine);
    screen_with(&ws, cx, second, "cat --model opus --effort max");
    let repo = committed_repo();
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    for (bad, why) in [
        (
            json!({ "agent": "codex", "model": "opus;rm -rf ~" }),
            "letters, digits",
        ),
        (json!({ "agent": "gemini", "effort": "high" }), "no effort"),
        (
            json!({ "agent": "amp", "model": "gpt-5" }),
            "a model or an effort",
        ),
        (
            json!({ "agent": "codex", "effort": "extreme", "folder": repo.path(), "worktree": "w" }),
            "low, medium",
        ),
    ] {
        let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", bad.clone());
        assert!(failed && text.contains(why), "{bad}: {text}");
    }
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.space.blocks.len()),
        blocks,
        "nothing opened"
    );
    assert!(!repo.path().join(".worktrees").exists(), "no worktree made");
}

/// An agent closes the blocks it opened while it is wired to them, as one step of Ctrl+Z, which
/// brings one back still opened by it, its agent resumed with its model.
#[gpui_kit::test]
fn close_block_closes_only_what_the_caller_opened(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path(), "model": "gpt-5.6-luna" });
    assert!(!call(handle, &ws, cx, &terminal, "spawn_agent", spawn).0);
    let new = spawned(&ws, cx, mine);
    let (_, them) = me(&ws, cx, new);
    // The last snapshot found its session: Ctrl+Z resumes it, not the folder's latest (the
    // caller's, in the same folder).
    let session = "0d5e5f4b-1c2d-4e3f-8a9b-0c1d2e3f4a5b";
    ws.update(cx, |ws, _| {
        ws.space.saved = Some(
            serde_json::from_value(json!({
                "version": 1, "camera": { "x": 0, "y": 0, "zoom": 1 },
                "items": [{ "kind": "terminal", "uid": them, "rect": { "x": 0, "y": 0, "w": 1, "h": 1 },
                            "agent": "codex", "session": session }],
            }))
            .unwrap(),
        )
    });
    let close = json!({ "block": them });
    let (failed, text) = call(handle, &ws, cx, &terminal, "close_block", close.clone());
    assert!(
        !failed && text.starts_with(&format!("Closed {them}.")),
        "{text}"
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.block_by_uid(&them).is_none());
        // Its shell ends now, not after a fade a workspace not shown never draws.
        assert!(
            ws.space.ghosts.iter().all(|g| g.block.id != new),
            "no ghost"
        );
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| assert!(ws.undo_canvas(false, window, cx)))
    })
    .unwrap();
    let back = spawned(&ws, cx, mine);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.uids[&back], them);
        let pane = ws.space.terminal(back).unwrap();
        assert_eq!(pane.opened_by.as_deref(), Some(uid.as_str()));
    });
    screen_with(
        &ws,
        cx,
        back,
        &format!("codex resume {session} -m gpt-5.6-luna"),
    );
    // A block the person opened, the caller's own, and one no longer wired: the person's.
    let other = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open(Rect::new(40.0, 900.0, 400.0, 300.0), window, cx);
                let other = ws.space.canvas.top().unwrap();
                assert!(ws.connect(mine, other, cx));
                other
            })
        })
        .unwrap();
    let (_, theirs) = me(&ws, cx, other);
    for block in [&theirs, &uid] {
        let (failed, text) = call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": block }),
        );
        assert!(
            failed && text == format!("Only the person closes {block}: you didn't open it."),
            "{text}"
        );
    }
    ws.update(cx, |ws, cx| {
        assert!(ws.disconnect(Wire::new(mine, back).unwrap(), cx))
    });
    let (failed, text) = call(handle, &ws, cx, &terminal, "close_block", close);
    assert!(failed && text.contains("isn't connected to you"), "{text}");
}

/// A spawned Claude Code or Codex first asks whether to trust its folder (Codex also its hooks),
/// its cursor hidden: its prompt, whose Enter would answer for the person, waits until it brackets
/// pastes and shows its cursor, as its input box does. No hook speaks here: a Claude Code started
/// without Templum's hooks gets its prompt too.
#[gpui_kit::test]
fn a_prompt_waits_for_the_agents_input_to_take_pastes(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    for (conn, agent) in [(1, "claude"), (2, "codex")] {
        let folder = tempfile::tempdir().unwrap();
        let spawn = json!({ "agent": agent, "folder": folder.path(), "prompt": "say hi" });
        let (failed, said) = call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
        assert!(
            !failed
                && said.ends_with(&format!(
                    "Its prompt is typed once it is ready, as task t{conn}."
                )),
            "{said}"
        );
        let new = spawned(&ws, cx, mine);
        ws.read_with(cx, |ws, _| {
            let handed = ws.space.terminal(new).unwrap().handed.as_ref().unwrap();
            let usage = handed.usage.as_ref().unwrap();
            assert_eq!(
                (usage.cost_usd, usage.tokens),
                (Some(0.0), Some(0)),
                "its session is its task's"
            );
            assert_eq!(handed.text, "say hi", "the task as written");
        });
        // The prompt ends with how to answer it.
        ws.read_with(cx, |ws, _| {
            let prompt = ws.space.terminal(new).unwrap().prompt.as_ref().unwrap();
            let how = crate::workspace::delegate::answer_how(conn);
            assert!(prompt.text.ends_with(&how), "{agent}: {}", prompt.text);
        });
        in_front(&ws, cx, new, true);
        found(&ws, cx, new);
        let (theirs, uid) = me(&ws, cx, new);
        let idle = json!({ "block": uid });
        let waiting = call_later(handle, &ws, cx, &terminal, conn, "wait_for_idle", idle);
        let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
        let prompt = |cx: &mut TestAppContext| {
            ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some())
        };
        // Its trust question hides the cursor; and an input that doesn't bracket pastes isn't its
        // box.
        for bytes in [&b"\x1b[?25l\r"[..], b"\x1b[?25h\x1b[?2004l\r"] {
            prints(&ws, cx, new, bytes);
            for _ in 0..4 {
                std::thread::sleep(Duration::from_millis(100));
                after(cx, look);
            }
            assert!(prompt(cx), "{agent} typed into a question: {bytes:?}");
        }
        prints(&ws, cx, new, b"\x1b[?2004h\r");
        assert!(typed(cx, prompt), "{agent} never typed");
        // A task the caller hands over, under its own first line.
        screen_with(
            &ws,
            cx,
            new,
            &format!(
                ", task t{conn}] Your task is in the pasted text: do it, then answer it with reply."
            ),
        );
        screen_with(&ws, cx, new, "say hi");
        entered(&ws, cx, new);
        said_twice(&ws, cx, new);
        event(
            handle,
            &ws,
            cx,
            &theirs,
            json!({ "event": "submitted", "at_enter": true }),
        );
        event(
            handle,
            &ws,
            cx,
            &theirs,
            json!({ "event": "stopped", "message": "hi" }),
        );
        assert_eq!(answered(&waiting), (false, "hi".to_string()), "{agent}");
    }
}

/// A spawned agent's task gives no answer within the time a task gets: it may still work, so its
/// caller hears nothing of that, as of a task handed over without `wait`; only that it seems stuck,
/// its screen still for 30 seconds.
#[gpui_kit::test]
fn a_spawns_task_past_its_timeout_isnt_told(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    // Mine runs Claude Code and asks its person: the notice waits, to be read here.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.agent = Some("claude".into())
    });
    found(&ws, cx, mine);
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "needs_you", "message": "Allow Edit?" }),
    );
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    in_front(&ws, cx, new, true);
    found(&ws, cx, new);
    let (theirs, _) = me(&ws, cx, new);
    let waiting = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some())
    };
    assert!(typed(cx, waiting), "never typed");
    screen_with(&ws, cx, new, "say hi");
    entered(&ws, cx, new);
    event(handle, &ws, cx, &theirs, json!({ "event": "submitted" }));
    after(cx, templum_mcp::tools::TASK_WAIT * 1000);
    let told: Vec<String> = ws.read_with(cx, |ws, _| {
        ws.notices.iter().map(|n| n.text.clone()).collect()
    });
    assert!(
        told.len() == 1 && told[0].contains("waits for its person: it seems stuck"),
        "{told:?}"
    );
}

/// The person types into a spawned agent before its prompt is (its trust question answered): that
/// turn isn't its task, and its end answers nothing.
#[gpui_kit::test]
fn a_turn_before_a_spawns_prompt_is_typed_isnt_its_task(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "claude", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    let (theirs, uid) = me(&ws, cx, new);
    let idle = json!({ "block": uid });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "wait_for_idle", idle);
    event(
        handle,
        &ws,
        cx,
        &theirs,
        json!({ "event": "submitted", "at_enter": true }),
    );
    event(
        handle,
        &ws,
        cx,
        &theirs,
        json!({ "event": "stopped", "message": "the person's answer" }),
    );
    assert!(waiting.try_recv().is_err(), "answered by the person's turn");
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(new).unwrap();
        assert!(pane.prompt.is_some() && !pane.handed.as_ref().unwrap().taken);
    });
}

/// A minute after its spawn its person still hasn't answered what its agent asks: the prompt is
/// dropped, and its task ends showing the question.
#[gpui_kit::test]
fn a_prompt_not_typed_within_a_minute_ends_its_task_with_its_screen(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    in_front(&ws, cx, new, true);
    found(&ws, cx, new);
    let (_, uid) = me(&ws, cx, new);
    prints(&ws, cx, new, b"Trust these hooks?\x1b[?25l\r");
    screen_with(&ws, cx, new, "Trust these hooks?");
    let idle = json!({ "block": uid });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "wait_for_idle", idle);
    ws.update(cx, |ws, _| {
        let handed = ws.space.terminal_mut(new).unwrap().handed.as_mut().unwrap();
        handed.since -= crate::workspace::spawn::STARTS_WITHIN;
    });
    after(cx, crate::workspace::spawn::READY_LOOK.as_millis() as u64);
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(new).unwrap();
        assert!(pane.prompt.is_none() && pane.handed.is_none());
    });
    let (failed, text) = answered(&waiting);
    assert!(
        failed
            && text.starts_with(&format!("{uid} never got its prompt"))
            && text.contains("Trust these hooks?"),
        "{text}"
    );
}

/// A spawned agent's start moves its screen, then its trust question sits still: the screen's
/// looks tell nothing while its prompt waits, so its task isn't ended as at rest after 5 seconds,
/// and the minute still ends it, with the question.
#[gpui_kit::test]
fn the_looks_leave_a_spawn_whose_prompt_waits_alone(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "claude", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    in_front(&ws, cx, new, true);
    found(&ws, cx, new);
    let (_, uid) = me(&ws, cx, new);
    prints(&ws, cx, new, b"Trust this folder?\x1b[?25l\r");
    screen_with(&ws, cx, new, "Trust this folder?");
    let idle = json!({ "block": uid });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "wait_for_idle", idle);
    // As the looks would take its start for work.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(new).unwrap().state = Some(AgentState::Working)
    });
    for _ in 0..6 {
        after(cx, 1000);
    }
    assert!(waiting.try_recv().is_err(), "its task stays handed over");
    ws.update(cx, |ws, _| {
        let handed = ws.space.terminal_mut(new).unwrap().handed.as_mut().unwrap();
        handed.since -= crate::workspace::spawn::STARTS_WITHIN;
    });
    after(cx, crate::workspace::spawn::READY_LOOK.as_millis() as u64);
    let (failed, text) = answered(&waiting);
    assert!(
        failed && text.starts_with(&format!("{uid} never got its prompt")),
        "{text}"
    );
}

/// A snapshot taken while the new shell still starts (its rc files) finds no agent in it: the
/// Claude Code Templum typed there stays known, and its prompt is sent once its input is ready.
#[gpui_kit::test]
fn a_snapshot_before_the_agent_started_doesnt_stop_its_prompt(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "claude", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    snapshot(
        handle,
        &ws,
        cx,
        new,
        templum_terminal::Inspection::default(),
    );
    in_front(&ws, cx, new, true);
    // A second snapshot finds it.
    let p = ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().term.processes());
    let running = templum_terminal::Inspection {
        agent: Some("claude".into()),
        pid: p.foreground,
        ..Default::default()
    };
    snapshot(handle, &ws, cx, new, running);
    let waiting = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some())
    };
    assert!(typed(cx, waiting), "never typed");
    screen_with(&ws, cx, new, "say hi");
    entered(&ws, cx, new);
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(new).unwrap();
        assert!(pane.handed.is_some(), "not taken: {:?}", pane.screen());
    });
    said_twice(&ws, cx, new);
}

/// `wait_for_idle` right after a spawn waits for the prompt's answer, though a snapshot taken
/// while the new shell still started found no agent in it.
#[gpui_kit::test]
fn wait_for_idle_waits_on_an_agent_a_snapshot_found_no_trace_of_yet(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "claude", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    let (_, uid) = me(&ws, cx, new);
    snapshot(
        handle,
        &ws,
        cx,
        new,
        templum_terminal::Inspection::default(),
    );
    let idle = json!({ "block": uid });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "wait_for_idle", idle);
    if let Ok(line) = waiting.try_recv() {
        panic!("answered at once: {:?}", result_of(&line));
    }
}

/// An agent no snapshot found yet may never have started (its folder's trust declined, its
/// shell's startup asking something): typed text could run in its shell, which macOS can't tell
/// is in front until a snapshot found the agent. A reply to it is refused until then; a change to
/// the task its prompt hands it, until that prompt is typed.
#[gpui_kit::test]
fn nothing_is_typed_into_an_agent_no_snapshot_found_yet(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "claude", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    let (_, uid) = me(&ws, cx, new);
    snapshot(
        handle,
        &ws,
        cx,
        new,
        templum_terminal::Inspection::default(),
    );
    // Not the terminal's process in front, as macOS's shell under `login` isn't.
    in_front(&ws, cx, new, true);
    let reply = json!({ "block": uid, "text": "Done." });
    let (failed, text) = call(handle, &ws, cx, &terminal, "reply", reply);
    assert!(failed && text.contains("still starting"), "{text}");
    let task = json!({ "block": uid, "task": "go" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task);
    assert!(failed && text.contains("it is starting"), "{text}");
}

/// A task handed to an agent just spawned (no prompt), still starting: no snapshot found it yet
/// (Codex on Windows), or its screen isn't up (OpenCode, Copilot CLI: a paste typed then is
/// lost). It waits, as a spawn's prompt does, and is typed once the agent is ready.
#[gpui_kit::test]
fn a_task_for_an_agent_still_starting_is_typed_once_it_is_ready(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path() });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    let (worker, uid) = me(&ws, cx, new);
    // A snapshot before its process started: it found no agent there yet.
    snapshot(
        handle,
        &ws,
        cx,
        new,
        templum_terminal::Inspection::default(),
    );
    let task = json!({ "block": uid, "task": "say hi" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task);
    assert_eq!(
        (failed, text),
        (
            false,
            format!(
                "Handed to {uid} as task t1. It is starting: the task is typed once it is ready."
            )
        )
    );
    let waiting = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some())
    };
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    in_front(&ws, cx, new, true);
    for _ in 0..3 {
        after(cx, look);
    }
    assert!(waiting(cx), "no snapshot found it: nothing typed");
    let p = ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().term.processes());
    let running = templum_terminal::Inspection {
        agent: Some("codex".into()),
        pid: p.foreground,
        ..Default::default()
    };
    snapshot(handle, &ws, cx, new, running);
    assert!(typed(cx, waiting), "never typed");
    screen_with(&ws, cx, new, "say hi");
    entered(&ws, cx, new);
    said_twice(&ws, cx, new);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let task = ws.read_with(cx, |ws, _| {
        let handed = ws.space.terminal(new).unwrap().handed.as_ref();
        handed.map(|h| (h.id, h.taken))
    });
    assert_eq!(task, Some((1, true)), "its task, taken");
}

/// A task handed to an agent still starting that isn't ready a minute after: it ends, saying it
/// never got its task, with its screen.
#[gpui_kit::test]
fn a_task_for_an_agent_never_ready_ends_a_minute_after_it_was_handed(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path() });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    let (_, uid) = me(&ws, cx, new);
    let task = json!({ "block": uid, "task": "say hi" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task);
    assert!(!failed && text.contains("It is starting"), "{text}");
    let idle = json!({ "block": uid });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "wait_for_idle", idle);
    ws.update(cx, |ws, _| {
        let handed = ws.space.terminal_mut(new).unwrap().handed.as_mut().unwrap();
        handed.since -= crate::workspace::spawn::STARTS_WITHIN;
    });
    after(cx, crate::workspace::spawn::READY_LOOK.as_millis() as u64);
    let (failed, text) = answered(&waiting);
    assert!(
        failed
            && text.starts_with(&format!(
                "{uid} never got its task: a minute after it was handed,"
            )),
        "{text}"
    );
}

/// An agent spawned without a prompt that no snapshot found within a minute of its start never
/// started: a task handed to it is refused, as in a terminal running no agent, not kept for an
/// agent that will never be ready.
#[gpui_kit::test]
fn a_task_for_an_agent_that_never_started_is_refused(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path() });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    let (_, uid) = me(&ws, cx, new);
    snapshot(
        handle,
        &ws,
        cx,
        new,
        templum_terminal::Inspection::default(),
    );
    // Its cover came off; its command was typed a minute ago.
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(new).unwrap();
        pane.starting = None;
        pane.typed_agent = pane
            .typed_agent
            .map(|at| at - crate::workspace::spawn::STARTS_WITHIN);
    });
    let task = json!({ "block": uid, "task": "say hi" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task);
    assert!(failed && text.contains("runs no agent"), "{text}");
}

/// A starting agent whose hooks say it waits for its person: its prompt waits, as the dialog
/// would take it for its answer, and is typed once the dialog is gone.
#[gpui_kit::test]
fn a_prompt_waits_while_its_agent_asks_its_person(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    in_front(&ws, cx, new, true);
    found(&ws, cx, new);
    let set = |cx: &mut TestAppContext, state| {
        ws.update(cx, |ws, _| {
            ws.space.terminal_mut(new).unwrap().state = state
        })
    };
    set(
        cx,
        Some(AgentState::NeedsYou(Some("Trust these hooks?".into()))),
    );
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    let waiting = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some())
    };
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(100));
        after(cx, look);
    }
    assert!(waiting(cx), "typed into its dialog");
    set(cx, Some(AgentState::Idle));
    assert!(typed(cx, waiting), "never typed");
    screen_with(&ws, cx, new, "say hi");
    entered(&ws, cx, new);
    said_twice(&ws, cx, new);
}

/// A spawned agent no snapshot found a while after its spawn never started: its prompt and its
/// task go, whoever waits on it hears so, and an agent started there later by hand gets no prompt.
#[gpui_kit::test]
fn a_spawned_agent_that_never_started_is_let_go(cx: &mut TestAppContext) {
    use templum_terminal::Inspection;
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({
        "agent": "codex",
        "folder": folder.path(),
        "prompt": "say hi",
        "model": "gpt-5.6-luna",
    });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    let (_, uid) = me(&ws, cx, new);
    let idle = json!({ "block": uid });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "wait_for_idle", idle);
    // Spawned a while ago.
    ws.update(cx, |ws, _| {
        let handed = ws.space.terminal_mut(new).unwrap().handed.as_mut().unwrap();
        handed.since -= crate::workspace::spawn::STARTS_WITHIN;
    });
    snapshot(handle, &ws, cx, new, Inspection::default());
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(new).unwrap();
        assert!(pane.prompt.is_none() && pane.handed.is_none());
        assert_eq!(pane.seen.agent, None);
        // A schedule set now takes it for a terminal, not for an agent that may still start.
        assert_eq!(pane.agent(), None);
        assert_eq!((&pane.model, &pane.effort), (&None, &None));
    });
    let (failed, text) = answered(&waiting);
    assert!(
        failed && text.contains("ended before it answered"),
        "{text}"
    );
    // Codex, started there by hand, is found in front.
    in_front(&ws, cx, new, true);
    let p = ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().term.processes());
    let running = Inspection {
        agent: Some("codex".into()),
        pid: p.foreground.or(p.shell),
        ..Default::default()
    };
    snapshot(handle, &ws, cx, new, running);
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    for _ in 0..4 {
        std::thread::sleep(Duration::from_millis(50));
        after(cx, look);
    }
    enter(cx);
    std::thread::sleep(Duration::from_millis(300));
    let shown = ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().screen());
    assert!(!shown.contains("say hi"), "typed: {shown:?}");
    assert!(waiting.try_recv().is_err(), "answered once");
}

#[gpui_kit::test]
fn another_agents_prompt_waits_for_its_process_and_a_still_screen(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    in_front(&ws, cx, new, true);
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    let waiting = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some())
    };
    // No snapshot found its process yet: nothing typed, however still its screen.
    for _ in 0..3 {
        after(cx, look);
    }
    assert!(waiting(cx));
    // A snapshot found it in front.
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(new).unwrap();
        pane.seen.pid = pane.term.processes().foreground;
    });
    let mut looks = 0;
    while waiting(cx) {
        assert!(looks < 10, "never typed");
        // What the shell prints lands between two looks.
        std::thread::sleep(Duration::from_millis(100));
        after(cx, look);
        looks += 1;
    }
    assert!(looks >= 2, "typed once two looks saw the same screen");
    screen_with(&ws, cx, new, "say hi");
    entered(&ws, cx, new);
    said_twice(&ws, cx, new);
}

/// In grid mode the new agent takes half of the caller's tile, not of the one the person works in,
/// which keeps the keyboard.
#[gpui_kit::test]
fn in_grid_mode_a_spawned_agent_takes_half_of_the_callers_tile(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let other = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open(Rect::new(500.0, 100.0, 400.0, 300.0), window, cx);
                let other = ws.space.canvas.top().unwrap();
                ws.enter_grid(vec![mine, other], window, cx);
                other
            })
        })
        .unwrap();
    frame(handle, cx);
    let rect = |cx: &mut TestAppContext, id| {
        ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect)
    };
    let (half, theirs) = (rect(cx, mine), rect(cx, other));
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path() });
    let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    assert!(!failed, "{text}");
    let new = spawned(&ws, cx, mine);
    let r = rect(cx, new);
    assert!(
        r.x >= half.x && r.right() <= half.right() && r.y >= half.y && r.bottom() <= half.bottom(),
        "in the caller's half: {r:?} in {half:?}"
    );
    assert_eq!(rect(cx, other), theirs, "the person's tile stays whole");
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.focused, Some(other)));
}

/// A prompt the agent's screen doesn't show (a dialog that ignores pastes) gets no Enter, and
/// whoever waits on it hears at once that the agent didn't take it.
#[gpui_kit::test]
fn a_prompt_the_agent_didnt_take_ends_at_once(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    // Claude Code shows nothing it is typed: not even the task's first line, which the newline
    // starting its second paste would have `cat` print.
    ws.update(cx, |ws, _| {
        ws.command = Some(jobs(
            "stty -echo; printf '\\033[?2004h'; echo ready; cat >/dev/null; true",
        ))
    });
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "claude", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    let (_, uid) = me(&ws, cx, new);
    screen_with(&ws, cx, new, "ready");
    in_front(&ws, cx, new, true);
    let waiting = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "wait_for_idle",
        json!({ "block": uid }),
    );
    found(&ws, cx, new);
    after(cx, 5000);
    let (failed, text) = answered(&waiting);
    assert!(
        failed && text.starts_with(&format!("{uid} didn't take the task")),
        "{text}"
    );
}

/// An agent a snapshot saw that quit before its prompt was typed: its shell, in front, would run
/// the prompt as a command.
#[gpui_kit::test]
fn no_prompt_is_typed_into_the_shell_an_agent_left(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    // New terminals' shells with nothing in front.
    ws.update(cx, |ws, _| ws.command = Some(quiet()));
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path(), "prompt": "say hi" });
    call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    let new = spawned(&ws, cx, mine);
    in_front(&ws, cx, new, false);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(new).unwrap().seen.pid = Some(1)
    });
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    for _ in 0..4 {
        after(cx, look);
    }
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(new).unwrap().prompt.is_some(), "typed")
    });
}

#[gpui_kit::test]
fn spawn_agent_makes_its_worktree_first(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let spawn = json!({ "agent": "codex", "folder": repo.path(), "worktree": "fix-login" });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "spawn_agent", spawn);
    let (failed, text) = answer_soon(cx, &answers);
    let made = repo
        .path()
        .canonicalize()
        .unwrap()
        .join(".worktrees")
        .join("fix-login");
    assert!(!failed && made.is_dir(), "{text}");
    let new = spawned(&ws, cx, mine);
    assert_eq!(folder_of(&ws, new, cx), made);
    let plain = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": plain.path(), "worktree": "x" });
    let answers = call_later(handle, &ws, cx, &terminal, 2, "spawn_agent", spawn);
    let (failed, text) = answer_soon(cx, &answers);
    assert!(
        failed && text.contains("isn't in a Git repository"),
        "{text}"
    );
}

/// The new agent overlaps nothing on the canvas, drawings included, and goes the way of the
/// caller's connections: here its right holds a block, and its connection is far to its left.
#[gpui_kit::test]
fn a_spawned_agent_fits_where_nothing_is_toward_the_callers_connections(cx: &mut TestAppContext) {
    use templum_canvas::{Color, Drawing, ShapeKind};
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open(Rect::new(464.0, 100.0, 400.0, 300.0), window, cx);
            ws.open(Rect::new(-3000.0, 100.0, 400.0, 300.0), window, cx);
            let far = ws.space.canvas.top().unwrap();
            assert!(ws.connect(mine, far, cx));
            // A line where a block left of the caller would sit.
            ws.space.ink.push(Drawing::Shape {
                kind: ShapeKind::Line,
                from: Point::new(-200.0, 150.0),
                to: Point::new(-100.0, 150.0),
                width: 4.0,
                color: Color::Red,
            });
            ws.space.focused = Some(mine);
        })
    })
    .unwrap();
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path() });
    let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    assert!(!failed, "{text}");
    let new = spawned(&ws, cx, mine);
    ws.read_with(cx, |ws, _| {
        let rect = ws.space.canvas.get(new).unwrap().rect;
        let others = ws.space.canvas.blocks().iter().filter(|b| b.id != new);
        for other in others
            .map(|b| b.rect)
            .chain(ws.space.ink.iter().map(Drawing::bounds))
        {
            assert!(!rect.intersects(&other), "{rect:?} over {other:?}");
        }
        let caller = ws.space.canvas.get(mine).unwrap().rect;
        assert!(
            rect.x + rect.w / 2.0 < caller.x + caller.w / 2.0,
            "toward its connection, on its left: {rect:?}"
        );
    });
}

/// An agent's subagents line up down its right, tops level, [`templum_canvas::BESIDE`] apart; a
/// subagent's own agent goes on its right too, not back toward the agent that opened it.
#[gpui_kit::test]
fn spawned_agents_line_up_beside_their_opener(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "codex", "folder": folder.path() });
    let mut opened = Vec::new();
    for _ in 0..2 {
        let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", spawn.clone());
        assert!(!failed, "{text}");
        opened.push(spawned(&ws, cx, mine));
    }
    let rect = |cx: &mut TestAppContext, id| {
        ws.read_with(cx, |ws, _| ws.space.canvas.get(id).unwrap().rect)
    };
    let (caller, first, second) = (rect(cx, mine), rect(cx, opened[0]), rect(cx, opened[1]));
    let gap = templum_canvas::BESIDE;
    assert_eq!((first.x, first.y), (caller.right() + gap, caller.y));
    assert_eq!((second.x, second.y), (first.x, first.bottom() + gap));
    let (sub, _) = me(&ws, cx, opened[0]);
    let (failed, text) = call(handle, &ws, cx, &sub, "spawn_agent", spawn);
    assert!(!failed, "{text}");
    let own = spawned(&ws, cx, opened[0]);
    let own = rect(cx, own);
    assert_eq!((own.x, own.y), (first.right() + gap, first.y));
}

/// `spawn_agent` opens an account by its id, on its folder, with its agent's model.
#[gpui_kit::test]
fn spawn_agent_opens_an_account(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let mine = first_id(&ws, cx);
    super::agents::with_agents(
        &ws,
        cx,
        &format!(
            "[agents.claude]\ncommand = \"cat\"\n{}",
            super::agents::CLAUDE2
        ),
    );
    ws.update(cx, |ws, _| {
        ws.command = Some(jobs("echo \"dir=$CLAUDE_CONFIG_DIR;\"; cat; true"));
    });
    let (terminal, _) = me(&ws, cx, mine);
    let folder = tempfile::tempdir().unwrap();
    let spawn = json!({ "agent": "claude2", "folder": folder.path(), "model": "opus" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "spawn_agent", spawn);
    assert!(!failed, "{text}");
    let new = spawned(&ws, cx, mine);
    let account = data.path().join("accounts").join("claude2");
    screen_with(&ws, cx, new, &format!("dir={};", account.display()));
    screen_with(&ws, cx, new, "cat --model opus");
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(new).unwrap();
        assert_eq!(pane.account.as_deref(), Some("claude2"));
        assert_eq!(pane.agent(), Some("claude2"));
    });
}

/// `stop_agent` ends the program in a terminal the caller opened and what it left running, ends
/// the task it handed there without a notice, and keeps the shell (docs/specs/agent-tools.md,
/// stop_agent).
#[gpui_kit::test]
fn stop_agent_ends_what_the_caller_opened_and_what_it_left_running(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let dir = tempfile::tempdir().unwrap();
    let flag = dir.path().join("f");
    // Its "agent" (`cat`, in place of the `sh` that started it) leaves a job that would write a
    // file later; once they end, the shell waits in a builtin `read`, at its prompt with nothing
    // below it.
    ws.update(cx, |ws, _| {
        ws.command = Some(jobs(&format!(
            "printf '\\033[?2004h'; sh -c '(sleep 2 && touch {}) & exec cat'; while read -r x; do :; done",
            flag.display()
        )))
    });
    let spawn = json!({ "agent": "codex", "folder": dir.path(), "prompt": "go" });
    assert!(!call(handle, &ws, cx, &terminal, "spawn_agent", spawn).0);
    let new = spawned(&ws, cx, mine);
    let (_, them) = me(&ws, cx, new);
    in_front(&ws, cx, new, true);
    found(&ws, cx, new);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "stop_agent",
        json!({ "block": them }),
    );
    assert!(!failed, "{text}");
    assert!(
        text.starts_with(&format!("Stopped {them}: ended ")),
        "{text}"
    );
    assert!(text.ends_with("Its shell is at its prompt."), "{text}");
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(new).unwrap();
        assert!(
            pane.handed.is_none() && pane.prompt.is_none(),
            "its task ended"
        );
        assert!(!pane.term.exited(), "the shell stays");
    });
    std::thread::sleep(Duration::from_millis(2500));
    assert!(!flag.exists(), "its job wrote its file");
    in_front(&ws, cx, new, false);
    let again = call(
        handle,
        &ws,
        cx,
        &terminal,
        "stop_agent",
        json!({ "block": them }),
    );
    assert_eq!(
        again,
        (
            true,
            format!("Nothing runs in {them}: its shell is at its prompt.")
        )
    );
}

/// What an agent left running in a session of its own (as Claude Code's Bash tool does) still ends
/// with `stop_agent` once a snapshot found that agent gone, until the person types at the prompt.
#[gpui_kit::test]
fn stop_agent_ends_what_an_agent_that_quit_left_running(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let dir = tempfile::tempdir().unwrap();
    let (flag, stop) = (dir.path().join("f"), dir.path().join("stop"));
    // Its "agent" (`cat`) leaves a job a second later (start times are whole seconds), then quits
    // at Ctrl+D; the shell waits in a builtin `read`. The job writes its file once the test says
    // so, however long the stop took: only a job that outlived it writes (and it ends with the
    // folder, if the test fails first). `setsid` through Perl: macOS has no `setsid` command.
    const SETSID: &str = "perl -MPOSIX -e \"setsid; exec @ARGV\"";
    let job = format!(
        "until [ -e {} ] || [ ! -d {} ]; do sleep 0.1; done; touch {}",
        stop.display(),
        dir.path().display(),
        flag.display()
    );
    ws.update(cx, |ws, _| {
        ws.command = Some(jobs(&format!(
            "sh -c 'sleep 1.2; ({SETSID} sh -c \"{job}\" &); exec cat'; \
             while read -r x; do :; done"
        )))
    });
    let spawn = json!({ "agent": "codex", "folder": dir.path() });
    assert!(!call(handle, &ws, cx, &terminal, "spawn_agent", spawn).0);
    let new = spawned(&ws, cx, mine);
    let (_, them) = me(&ws, cx, new);
    in_front(&ws, cx, new, true);
    let p = ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().term.processes());
    let cat = templum_terminal::Detect {
        id: "codex".into(),
        names: vec!["sh".into(), "cat".into()],
        packages: Vec::new(),
        var: None,
    };
    let seen = templum_terminal::inspect(&[p], &[cat]).pop().unwrap();
    assert!(seen.started.is_some(), "{seen:?}");
    snapshot(handle, &ws, cx, new, seen);
    std::thread::sleep(Duration::from_millis(1500));
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(new).unwrap().term.write(&b"\x04"[..])
    });
    in_front(&ws, cx, new, false);
    snapshot(handle, &ws, cx, new, Default::default());
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "stop_agent",
        json!({ "block": them }),
    );
    assert!(!failed, "{text}");
    assert!(
        text.starts_with(&format!("Stopped {them}: ended ")) && text.contains("left running"),
        "{text}"
    );
    std::fs::write(&stop, "").unwrap();
    std::thread::sleep(Duration::from_millis(1000));
    assert!(!flag.exists(), "its job wrote its file");
    // Once the person typed at the prompt, what runs there is theirs.
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(new).unwrap();
        assert!(pane.agent_since().is_some());
        pane.person_typed();
        assert_eq!(pane.agent_since(), None);
    });
}

#[gpui_kit::test]
fn stop_agent_refuses_its_own_block_and_one_it_didnt_open(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let own = call(
        handle,
        &ws,
        cx,
        &terminal,
        "stop_agent",
        json!({ "block": uid }),
    );
    assert_eq!(
        own,
        (
            true,
            "This block is yours: stop_agent stops another block.".into()
        )
    );
    // A terminal the person opened, wired by them.
    let dir = tempfile::tempdir().unwrap();
    let other = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let id = ws
                    .terminal_beside(Some(mine), dir.path(), None, window, cx)
                    .unwrap();
                assert!(ws.connect(mine, id, cx));
                ws.space.uids[&id].clone()
            })
        })
        .unwrap();
    let theirs = call(
        handle,
        &ws,
        cx,
        &terminal,
        "stop_agent",
        json!({ "block": other }),
    );
    assert_eq!(
        theirs,
        (
            true,
            format!(
                "Only the person stops {other}: you didn't open it. interrupt presses Esc in it."
            )
        )
    );
}

/// A canvas the last snapshot saved: the terminal `uid` ran `agent` on `session`.
fn saved_with(uid: &str, agent: &str, session: &str) -> crate::session::Saved {
    serde_json::from_value(json!({
        "version": 1, "camera": { "x": 0, "y": 0, "zoom": 1 },
        "items": [{ "kind": "terminal", "uid": uid, "rect": { "x": 0, "y": 0, "w": 1, "h": 1 },
                    "agent": agent, "session": session }],
    }))
    .unwrap()
}

/// `spawn_agent` with `fork` types the caller's own agent's fork command on the session the last
/// snapshot found, and its prompt says it is a copy (docs/specs/agent-tools.md, spawn_agent with
/// fork).
#[gpui_kit::test]
fn a_fork_types_the_callers_fork_command_with_its_session(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let session = "0d5e5f4b-1c2d-4e3f-8a9b-0c1d2e3f4a5b";
    // The caller runs Claude Code, and the last snapshot found its session.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.agent = Some("claude".into());
        ws.space.saved = Some(saved_with(&uid, "claude", session));
    });
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "spawn_agent",
        json!({ "fork": true, "prompt": "go on" }),
    );
    assert!(
        !failed && text.starts_with("Opened a fork of your session as "),
        "{text}"
    );
    let new = spawned(&ws, cx, mine);
    screen_with(&ws, cx, new, &format!("--resume {session} --fork-session"));
    in_front(&ws, cx, new, true);
    found(&ws, cx, new);
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    let mut looks = 0;
    while ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some()) {
        assert!(looks < 20, "its prompt was never typed");
        std::thread::sleep(Duration::from_millis(100));
        after(cx, look);
        looks += 1;
    }
    enter(cx);
    screen_with(
        &ws,
        cx,
        new,
        &format!("You are a copy of the agent in {uid}"),
    );
}

#[gpui_kit::test]
fn a_fork_refuses_another_agent_and_a_session_not_found(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.agent = Some("claude".into())
    });
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &terminal,
            "spawn_agent",
            json!({ "fork": true })
        ),
        (
            true,
            "A fork needs a prompt: it tells the copy what to do.".into()
        )
    );
    let go = |agent: Option<&str>| match agent {
        Some(a) => json!({ "fork": true, "prompt": "x", "agent": a }),
        None => json!({ "fork": true, "prompt": "x" }),
    };
    assert_eq!(
        call(handle, &ws, cx, &terminal, "spawn_agent", go(None)),
        (
            true,
            "Templum doesn't know your session yet: try again in a few seconds.".into()
        )
    );
    assert_eq!(
        call(handle, &ws, cx, &terminal, "spawn_agent", go(Some("codex"))),
        (
            true,
            "A fork is a copy of your own agent, claude: leave agent out.".into()
        )
    );
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.agent = Some("gemini".into())
    });
    assert_eq!(
        call(handle, &ws, cx, &terminal, "spawn_agent", go(None)),
        (
            true,
            "Gemini CLI can't fork a session: forks are for Claude Code and Codex.".into()
        )
    );
}

/// A role opens its agent on its model and effort with its words, and its instructions go with
/// the prompt (docs/specs/agent-tools.md, Roles).
#[gpui_kit::test]
fn a_role_opens_its_agent_with_its_words_and_its_instructions_go_with_the_prompt(
    cx: &mut TestAppContext,
) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    ws.update(cx, |ws, _| {
        ws.config.roles.insert(
            "reviewer".into(),
            crate::config::Role {
                agent: "codex".into(),
                model: Some("gpt-5.6-luna".into()),
                effort: Some("high".into()),
                args: Some("-s read-only".into()),
                instructions: Some("Never edit files.".into()),
            },
        );
    });
    let told = ws.read_with(cx, |ws, cx| ws.instructions(&terminal, cx));
    assert!(
        told.contains(
            "list_blocks gives the current list. Roles for spawn_agent: reviewer (codex). When"
        ),
        "{told}"
    );
    let dir = tempfile::tempdir().unwrap();
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "spawn_agent",
        json!({ "role": "reviewer", "folder": dir.path(), "prompt": "review HEAD" }),
    );
    assert!(!failed, "{text}");
    let new = spawned(&ws, cx, mine);
    screen_with(
        &ws,
        cx,
        new,
        "-m gpt-5.6-luna -c model_reasoning_effort=high -s read-only",
    );
    in_front(&ws, cx, new, true);
    found(&ws, cx, new);
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    let mut looks = 0;
    while ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some()) {
        assert!(looks < 20, "its prompt was never typed");
        std::thread::sleep(Duration::from_millis(100));
        after(cx, look);
        looks += 1;
    }
    enter(cx);
    screen_with(&ws, cx, new, "as reviewer]");
    screen_with(&ws, cx, new, "Your role, reviewer:");
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.terminal(new).unwrap().role.as_deref(),
            Some("reviewer")
        )
    });
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &terminal,
            "spawn_agent",
            json!({ "role": "reviewer" })
        ),
        (
            true,
            "The role reviewer's instructions go with a prompt: give one.".into()
        )
    );
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &terminal,
            "spawn_agent",
            json!({ "role": "tester", "prompt": "x" })
        ),
        (true, "There's no role tester. Roles: reviewer.".into())
    );
}

/// Opens an agent in the worktree `name` of `repo`, as `spawn_agent` does: its block, its uid,
/// and the worktree's folder.
fn in_worktree(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    (terminal, mine): (&str, BlockId),
    repo: &std::path::Path,
    (conn, name): (u64, &str),
) -> (BlockId, String, std::path::PathBuf) {
    let spawn = json!({ "agent": "codex", "folder": repo, "worktree": name });
    let answers = call_later(handle, ws, cx, terminal, conn, "spawn_agent", spawn);
    let (failed, text) = answer_soon(cx, &answers);
    assert!(!failed, "{text}");
    let worker = spawned(ws, cx, mine);
    let (_, them) = me(ws, cx, worker);
    (worker, them, repo.join(".worktrees").join(name))
}

/// The inbox opened and drawn, long enough ago for presses to count; then the part `what` of the
/// request of block `mine` (its yes, allow, or its no, deny) pressed.
fn press_in_inbox(handle: AnyWindowHandle, cx: &mut TestAppContext, mine: BlockId, what: &str) {
    cx.update_window(handle, |_, window, cx| {
        inbox_open(window, cx);
        window.render_frame(cx)
    })
    .unwrap();
    after(cx, 600);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(crate::balloon::part(mine.0 as usize, what), cx);
    })
    .unwrap();
}

/// `merge_worktree` refuses while an agent works in the worktree or the person's branch has
/// changes, asks the person, then merges; `remove_worktree` removes a merged one at once
/// (docs/specs/agent-tools.md, remove_worktree and merge_worktree).
#[gpui_kit::test]
fn merge_worktree_asks_the_person_then_merges_and_remove_worktree_removes_it(
    cx: &mut TestAppContext,
) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let root = repo.path().canonicalize().unwrap();
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (1, "w"));
    std::fs::write(wt.join("a.txt"), "new\n").unwrap();
    git_run(&wt, &["commit", "-qam", "change"]);
    let merge = json!({ "worktree": "w", "folder": root });
    assert_eq!(
        call(handle, &ws, cx, &terminal, "merge_worktree", merge.clone()),
        (
            true,
            format!("{them} works in it: stop_agent or close_block it first.")
        )
    );
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    std::fs::write(root.join("a.txt"), "the person's\n").unwrap();
    let (failed, text) = call(handle, &ws, cx, &terminal, "merge_worktree", merge.clone());
    assert!(
        failed && text.starts_with("main has changes not committed in "),
        "{text}"
    );
    git_run(&root, &["checkout", "--", "a.txt"]);
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        5,
        "merge_worktree",
        merge.clone(),
    );
    assert!(answers.try_recv().is_err(), "it waits for the person");
    let line = ws.read_with(cx, |ws, cx| {
        ws.allow_line(ws.asking(&terminal).unwrap().0, cx)
    });
    assert!(
        line.ends_with(" wants to merge w (1 commits) into main."),
        "{line}"
    );
    press_in_inbox(handle, cx, mine, "allow");
    let (failed, text) = answer_soon(cx, &answers);
    assert_eq!(
        (failed, text.as_str()),
        (
            false,
            "Merged w into main (1 commit). remove_worktree removes it."
        )
    );
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "new\n"
    );
    assert_eq!(
        call(handle, &ws, cx, &terminal, "merge_worktree", merge),
        (true, "w has nothing main doesn't.".into())
    );
    let remove = json!({ "worktree": "w", "folder": root });
    let answers = call_later(handle, &ws, cx, &terminal, 6, "remove_worktree", remove);
    assert_eq!(
        answer_soon(cx, &answers),
        (false, "Removed the worktree w and its branch.".into())
    );
    assert!(!wt.exists());
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &terminal,
            "remove_worktree",
            json!({ "worktree": "w", "folder": root })
        ),
        (
            true,
            "There's no worktree w in this repository: git worktree list shows them.".into()
        )
    );
}

/// Work in a worktree is lost only once the person says Delete; Keep keeps it; a merge that
/// would conflict is aborted; a request waits its turn after the others of its block.
#[gpui_kit::test]
fn remove_worktree_with_work_in_it_asks_before_discarding_and_keep_keeps_it(
    cx: &mut TestAppContext,
) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let root = repo.path().canonicalize().unwrap();
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (1, "d"));
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    std::fs::write(wt.join("b.txt"), "work\n").unwrap();
    let remove = |discard: bool| json!({ "worktree": "d", "folder": root, "discard": discard });
    assert_eq!(
        call(handle, &ws, cx, &terminal, "remove_worktree", remove(false)),
        (
            true,
            "d has 1 changed files and 0 commits not merged into main: merge_worktree merges \
             them, discard: true loses them."
                .into()
        )
    );
    // A question first: the request waits its turn, "1 of 2".
    let question = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        4,
        "ask_user",
        json!({ "question": "Ready?", "options": ["Yes"] }),
    );
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        5,
        "remove_worktree",
        remove(true),
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.asking(&terminal).map(|(_, n)| n)),
        Some(2)
    );
    press_in_inbox(handle, cx, mine, "q0-option-0");
    press_in_inbox(handle, cx, mine, "send");
    assert_eq!(answer_soon(cx, &question), (false, "Yes".into()));
    let line = ws.read_with(cx, |ws, cx| {
        ws.allow_line(ws.asking(&terminal).unwrap().0, cx)
    });
    assert!(
        line.ends_with(
            " wants to delete the worktree d, with 1 changed files and 0 commits not merged."
        ),
        "{line}"
    );
    press_in_inbox(handle, cx, mine, "deny");
    assert_eq!(
        answer_soon(cx, &answers),
        (true, "The person said no.".into())
    );
    assert!(wt.join("b.txt").exists(), "kept");
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        6,
        "remove_worktree",
        remove(true),
    );
    press_in_inbox(handle, cx, mine, "allow");
    assert_eq!(
        answer_soon(cx, &answers),
        (false, "Removed the worktree d and its branch.".into())
    );
    assert!(!wt.exists());
    // A merge that would conflict is aborted, nothing left half done.
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (7, "c"));
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    std::fs::write(wt.join("a.txt"), "theirs\n").unwrap();
    git_run(&wt, &["commit", "-qam", "theirs"]);
    std::fs::write(root.join("a.txt"), "ours\n").unwrap();
    git_run(&root, &["commit", "-qam", "ours"]);
    let merge = json!({ "worktree": "c", "folder": root });
    let answers = call_later(handle, &ws, cx, &terminal, 8, "merge_worktree", merge);
    press_in_inbox(handle, cx, mine, "allow");
    assert_eq!(
        answer_soon(cx, &answers),
        (
            true,
            "The merge would conflict in a.txt: merge main into c and resolve them there, then \
             merge again."
                .into()
        )
    );
    assert!(!root.join(".git/MERGE_HEAD").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "ours\n"
    );
}

/// A worktree an agent took off its branch (a detached HEAD, another branch) is neither removed
/// nor merged: its commits, or that other branch, would be lost or left out.
#[gpui_kit::test]
fn a_worktree_off_its_branch_is_refused(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let root = repo.path().canonicalize().unwrap();
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (1, "x"));
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    git_run(&wt, &["checkout", "-q", "--detach"]);
    std::fs::write(wt.join("a.txt"), "detached\n").unwrap();
    git_run(&wt, &["commit", "-qam", "detached"]);
    let refused = "The worktree x has a detached HEAD, not its branch x: check out x there first.";
    for tool in ["remove_worktree", "merge_worktree"] {
        let args = json!({ "worktree": "x", "folder": root, "discard": true });
        let args = if tool == "merge_worktree" {
            json!({ "worktree": "x", "folder": root })
        } else {
            args
        };
        assert_eq!(
            call(handle, &ws, cx, &terminal, tool, args),
            (true, refused.to_string()),
            "{tool}"
        );
    }
    git_run(&wt, &["checkout", "-q", "-b", "other"]);
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &terminal,
            "merge_worktree",
            json!({ "worktree": "x", "folder": root })
        ),
        (
            true,
            "The worktree x is on other now, not on its branch x: check out x there first.".into()
        )
    );
    assert!(wt.exists());
}

/// What the person allowed is checked again when they answer: work made in the worktree since
/// the question isn't lost with it.
#[gpui_kit::test]
fn a_worktree_changed_since_the_question_isnt_deleted(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let root = repo.path().canonicalize().unwrap();
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (1, "d"));
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    std::fs::write(wt.join("b.txt"), "work\n").unwrap();
    let remove = json!({ "worktree": "d", "folder": root, "discard": true });
    let answers = call_later(handle, &ws, cx, &terminal, 5, "remove_worktree", remove);
    // More work while the person reads the question.
    std::fs::write(wt.join("c.txt"), "more work\n").unwrap();
    press_in_inbox(handle, cx, mine, "allow");
    assert_eq!(
        answer_soon(cx, &answers),
        (
            true,
            "d changed since the person was asked: ask again.".into()
        )
    );
    assert!(wt.join("c.txt").exists(), "kept");
}

/// A merge the person allowed into one branch doesn't go into another checked out there since.
#[gpui_kit::test]
fn a_merge_allowed_into_a_branch_isnt_made_into_another(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let root = repo.path().canonicalize().unwrap();
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (1, "m"));
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    std::fs::write(wt.join("a.txt"), "new\n").unwrap();
    git_run(&wt, &["commit", "-qam", "change"]);
    let merge = json!({ "worktree": "m", "folder": root });
    let answers = call_later(handle, &ws, cx, &terminal, 5, "merge_worktree", merge);
    // The person checks out another branch while they read the question.
    git_run(&root, &["checkout", "-q", "-b", "other"]);
    press_in_inbox(handle, cx, mine, "allow");
    assert_eq!(
        answer_soon(cx, &answers),
        (
            true,
            format!(
                "{} is on other now, not on main as the person was told: ask again.",
                root.display()
            )
        )
    );
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "one\n"
    );
}

/// Where the worktree goes back has a detached HEAD: neither removed (its commits would count as
/// merged into a HEAD a checkout leaves behind) nor merged.
#[gpui_kit::test]
fn a_worktree_going_back_to_a_detached_head_is_refused(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let root = repo.path().canonicalize().unwrap();
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (1, "h"));
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    std::fs::write(wt.join("a.txt"), "new\n").unwrap();
    git_run(&wt, &["commit", "-qam", "change"]);
    // The person's tree detached at the worktree's commit: nothing looks unmerged there.
    git_run(&root, &["checkout", "-q", "--detach", "h"]);
    let refused = format!(
        "{} has a detached HEAD: check out the branch h goes back to first.",
        root.display()
    );
    for tool in ["remove_worktree", "merge_worktree"] {
        let args = json!({ "worktree": "h", "folder": root });
        assert_eq!(
            call(handle, &ws, cx, &terminal, tool, args),
            (true, refused.clone()),
            "{tool}"
        );
    }
    assert!(wt.exists());
}

/// Removing a worktree deletes what git ignores in it: a `.env` is named, and goes only once the
/// person says Delete; a build folder goes without a word.
#[gpui_kit::test]
fn remove_worktree_names_the_ignored_files_it_would_delete(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let root = repo.path().canonicalize().unwrap();
    std::fs::write(root.join(".gitignore"), ".env\ntarget/\n").unwrap();
    git_run(&root, &["add", ".gitignore"]);
    git_run(&root, &["commit", "-qm", "Ignore"]);
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (1, "e"));
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    std::fs::write(wt.join(".env"), "SECRET=x\n").unwrap();
    std::fs::create_dir(wt.join("target")).unwrap();
    std::fs::write(wt.join("target").join("o"), "o\n").unwrap();
    let remove = json!({ "worktree": "e", "folder": root });
    assert_eq!(
        call(handle, &ws, cx, &terminal, "remove_worktree", remove),
        (
            true,
            "e has files git ignores, which removing it deletes: .env. discard: true deletes them."
                .into()
        )
    );
    assert!(wt.join(".env").exists());
    let discard = json!({ "worktree": "e", "folder": root, "discard": true });
    let answers = call_later(handle, &ws, cx, &terminal, 5, "remove_worktree", discard);
    let line = ws.read_with(cx, |ws, cx| {
        ws.allow_line(ws.asking(&terminal).unwrap().0, cx)
    });
    assert!(
        line.ends_with(
            " wants to delete the worktree e, with 0 changed files and 0 commits not merged, and \
             files git ignores: .env."
        ),
        "{line}"
    );
    press_in_inbox(handle, cx, mine, "allow");
    assert_eq!(
        answer_soon(cx, &answers),
        (false, "Removed the worktree e and its branch.".into())
    );
    assert!(!wt.exists());
}

/// A merge that would write over files git ignores in the person's tree is refused, naming the
/// first few of them.
#[gpui_kit::test]
fn merge_worktree_names_a_few_of_the_ignored_files_it_would_overwrite(cx: &mut TestAppContext) {
    let (handle, ws, mine) = spawner(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let repo = committed_repo();
    let root = repo.path().canonicalize().unwrap();
    std::fs::write(root.join(".gitignore"), "deps/\n").unwrap();
    git_run(&root, &["add", ".gitignore"]);
    git_run(&root, &["commit", "-qm", "Ignore"]);
    std::fs::create_dir(root.join("deps")).unwrap();
    std::fs::write(root.join("deps").join("x.js"), "the person's\n").unwrap();
    let (_, them, wt) = in_worktree(handle, &ws, cx, (&terminal, mine), &root, (1, "m"));
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": them })
        )
        .0
    );
    std::fs::create_dir(wt.join("deps")).unwrap();
    for i in 1..=7 {
        std::fs::write(wt.join("deps").join(format!("{i}.js")), "agent's\n").unwrap();
    }
    git_run(&wt, &["add", "-f", "deps"]);
    git_run(&wt, &["commit", "-qm", "Deps"]);
    let merge = json!({ "worktree": "m", "folder": root });
    assert_eq!(
        call(handle, &ws, cx, &terminal, "merge_worktree", merge),
        (
            true,
            format!(
                "The merge would overwrite deps/1.js, deps/2.js, deps/3.js, deps/4.js, deps/5.js \
                 and 2 more, ignored in {}: move it first.",
                root.display()
            )
        )
    );
}
