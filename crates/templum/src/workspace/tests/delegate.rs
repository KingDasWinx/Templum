//! Tasks handed between agents: `delegate`, `reply`, `wait_for_idle`, and the dot on the wire
//! (docs/specs/connections.md, Delegating).

use std::time::Instant;

use serde_json::json;
use templum_canvas::Wire;

use super::bus::{
    after, answered, call, call_later, event, has_server, me, result_of, screen_until, screen_with,
};
#[cfg(unix)]
use super::bus::{bus_note, history_with, server_gone};
use super::*;

/// Two terminals side by side and wired: mine at (40, 100) and theirs at (900, 100), 700 × 300
/// each, theirs running Codex as far as Templum knows; the camera at 100% on the origin.
pub(super) fn two_agents(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Workspace>, BlockId, BlockId) {
    agents_running(cx, None)
}

/// `two_agents`, theirs running `command` (`agent_cat` when None), each once its program is in
/// front, where a snapshot found theirs.
pub(super) fn agents_running(
    cx: &mut TestAppContext,
    command: Option<(String, Vec<String>)>,
) -> (AnyWindowHandle, Entity<Workspace>, BlockId, BlockId) {
    let (handle, ws) = open_as(cx, Some(agent_cat()), None, None);
    let mine = first_id(&ws, cx);
    let theirs = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                if command.is_some() {
                    ws.command = command;
                }
                ws.space.pristine = false;
                ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 1.0);
                ws.space.canvas.get_mut(mine).unwrap().rect = Rect::new(40.0, 100.0, 700.0, 300.0);
                ws.open(Rect::new(900.0, 100.0, 700.0, 300.0), window, cx);
                let theirs = ws.space.canvas.top().unwrap();
                ws.space.terminal_mut(theirs).unwrap().seen.agent = Some("codex".into());
                ws.space.appearing.clear();
                assert!(ws.connect(mine, theirs, cx));
                theirs
            })
        })
        .unwrap();
    frame(handle, cx);
    #[cfg(unix)]
    for id in [mine, theirs] {
        in_front(&ws, cx, id, true);
    }
    found(&ws, cx, theirs);
    (handle, ws, mine, theirs)
}

/// `two_agents`, theirs a `cat -v` that asked for bracketed pastes, as Claude Code does, and
/// echoes nothing: its screen shows the bytes Templum wrote, in order, ESC as "^[" and Enter as
/// "^M". The tty's echo would be a second writer, which `cat`'s lines cut into at any point.
#[cfg(unix)]
fn bracketed_cat(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Workspace>, BlockId, BlockId) {
    // One job: `cat` takes the place of the `sh` that set its tty, in front from the start.
    let cat = "sh -c 'stty -echo -icanon -icrnl; printf \"\\033[?2004h\"; exec cat -v'; true";
    let (handle, ws, mine, theirs) = agents_running(cx, Some(jobs(cat)));
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while !ws.read_with(cx, |ws, _| {
        ws.space.terminal(theirs).unwrap().term.bracketed()
    }) {
        assert!(Instant::now() < deadline, "no bracketed paste");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    (handle, ws, mine, theirs)
}

/// A third terminal at (40, 500), 700 × 300, running Codex as far as Templum knows, found in front
/// of its shell (Unix) and wired to `to`.
pub(super) fn another_agent(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    to: BlockId,
) -> BlockId {
    let id = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.open(Rect::new(40.0, 500.0, 700.0, 300.0), window, cx);
                let id = ws.space.canvas.top().unwrap();
                ws.space.terminal_mut(id).unwrap().seen.agent = Some("codex".into());
                ws.space.appearing.clear();
                assert!(ws.connect(id, to, cx));
                id
            })
        })
        .unwrap();
    frame(handle, cx);
    #[cfg(unix)]
    in_front(ws, cx, id, true);
    found(ws, cx, id);
    id
}

/// A snapshot found the agent of terminal block `id`: in front of its shell (Unix), or below it
/// (Windows, which has no process in front).
pub(super) fn found(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) {
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(id).unwrap();
        let p = pane.term.processes();
        pane.seen.pid = p.foreground.or(p.shell);
    });
}

/// A snapshot that found `found` in the terminal block `id`, as one taken every few seconds.
pub(super) fn snapshot(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
    found: templum_terminal::Inspection,
) {
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let saved: crate::session::Saved = serde_json::from_value(
        json!({ "version": 1, "camera": { "x": 0, "y": 0, "zoom": 1 }, "items": [] }),
    )
    .unwrap();
    let done = vec![(space, vec![(id, found)], saved, None)];
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.snapshot_done(done, None, None, None, None, window, cx)
        })
    })
    .unwrap();
}

/// The Enter of what Templum just typed into the terminal block `id`: once its program printed
/// since (in real time: a slow one would see its looks give up on a screen that hasn't moved yet),
/// its looks go a short while apart until its paste shows and stood still, and the Enter went.
pub(super) fn entered(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let (sent, printed) = ws.read_with(cx, |ws, _| {
            let pane = ws.space.terminal(id).unwrap();
            (pane.sent, pane.term.status().output)
        });
        if printed > sent {
            break;
        }
        assert!(Instant::now() < deadline, "nothing printed since");
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let inputs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.inputs())
    };
    let typed = inputs(cx);
    while inputs(cx) == typed {
        assert!(Instant::now() < deadline, "no Enter");
        std::thread::sleep(std::time::Duration::from_millis(10));
        after(
            cx,
            crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
        );
    }
}

/// `delegate` from the terminal `from` with `args`, answered once given: at once when refused,
/// else once the agent of block `theirs` took the task. Its screen is drawn in real time: once it
/// changed (Windows' `cmd` may start late), its looks go a short while apart: its paste shows, its
/// Enter goes, then its hooks say it took it (`hooks`), or its screen tells.
pub(super) fn hand_over(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    (from, theirs): (&str, BlockId),
    args: serde_json::Value,
    hooks: bool,
) -> (bool, String) {
    let (worker, _) = me(ws, cx, theirs);
    let before = screen_until(ws, cx, theirs, |_| true);
    let answers = call_later(handle, ws, cx, from, 99, "delegate", args);
    if let Ok(line) = answers.try_recv() {
        return result_of(&line);
    }
    screen_until(ws, cx, theirs, |shown| shown != before);
    if hooks {
        entered(ws, cx, theirs);
        event(handle, ws, cx, &worker, json!({ "event": "submitted" }));
    }
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Ok(line) = answers.try_recv() {
            return result_of(&line);
        }
        assert!(Instant::now() < deadline, "not answered");
        std::thread::sleep(std::time::Duration::from_millis(10));
        after(
            cx,
            crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
        );
    }
}

/// Mine runs Claude Code too, found by a snapshot: what comes back is typed into it.
#[cfg(unix)]
pub(super) fn asker(ws: &Entity<Workspace>, cx: &mut TestAppContext, mine: BlockId) {
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.agent = Some("claude".into())
    });
    found(ws, cx, mine);
}

/// The notices waiting to be typed, oldest first: to whom, and what.
#[cfg(unix)]
pub(super) fn notices(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<(String, String)> {
    ws.read_with(cx, |ws, _| {
        ws.notices
            .iter()
            .map(|n| (n.to.clone(), n.text.clone()))
            .collect()
    })
}

/// The terminal `from` hands block `theirs` the task `said` without waiting; its agent takes it
/// and ends with the answer `said`; then a second passes (what it spent is read then).
#[cfg(unix)]
pub(super) fn done_task(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    from: &str,
    theirs: BlockId,
    said: &str,
) {
    let (worker, them) = me(ws, cx, theirs);
    let task = json!({ "block": them, "task": said });
    let (failed, text) = hand_over(handle, ws, cx, (from, theirs), task, true);
    assert!(!failed, "{text}");
    event(
        handle,
        ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": said }),
    );
    after(cx, 1000);
}

/// A handed task keeps at most 2000 characters, cut between two characters.
#[test]
fn a_handed_tasks_text_is_cut_at_2000_characters() {
    let long = "é".repeat(2001);
    assert_eq!(crate::workspace::Handed::text_of(&long), "é".repeat(2000));
    assert_eq!(crate::workspace::Handed::text_of("short"), "short");
}

#[cfg(unix)]
#[gpui_kit::test]
fn delegate_types_the_header_the_task_and_enter(cx: &mut TestAppContext) {
    // Its pastes take no Enter for a new line.
    let (handle, ws, mine, theirs) = bracketed_cat(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    has_server(handle, &ws, cx, &worker, 7);
    let inputs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(theirs).unwrap().term.inputs())
    };
    let before = inputs(cx);
    let task = json!({ "block": them, "task": "fix the tests\nthen push" });
    // Answered once it took the task: `cat` has no hooks, its screen moved after the Enter.
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, false);
    assert_eq!(
        (failed, text),
        (false, format!("Handed to {them} as task t1."))
    );
    // Its first line, its text, one Enter.
    assert_eq!(inputs(cx), before + 3, "one Enter");
    ws.read_with(cx, |ws, _| {
        let handed = ws
            .space
            .terminal(theirs)
            .unwrap()
            .handed
            .as_ref()
            .expect("handed over");
        assert_eq!(handed.by, terminal);
        assert_eq!(handed.id, 1);
        assert_eq!(handed.text, "fix the tests\nthen push");
    });
    // What `cat` was given, in order: the header, a paste of its own, then the task's lines and
    // how to answer, then one Enter. The task is longer than the screen: its start is read in the
    // history.
    let given = format!(
        "({uid}), task t1] Your task is in the pasted text: do it, then answer it with reply.\
         ^[[201~^[[200~\nfix the tests\nthen push{}^[[201~^M",
        crate::workspace::delegate::answer_how(1)
    )
    .replace('\n', "");
    history_with(&ws, cx, theirs, &given);
    let shown = ws.read_with(cx, |ws, _| ws.space.terminal(theirs).unwrap().screen());
    let shown = shown.replace('\n', "");
    assert!(
        shown.starts_with("^[[200~[from ") && shown.ends_with(&given),
        "{shown}"
    );
}

/// Claude Code takes a long paste alone for data it was shown, not for what it is asked: the
/// first line goes as a paste of its own, which it reads as typed.
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_goes_as_its_first_line_then_its_text(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = bracketed_cat(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "line 1\nline 2\nline 3\nline 4" });
    call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    // Two pastes, the first line alone. The task is longer than the screen: its start is read in
    // the history.
    history_with(
        &ws,
        cx,
        theirs,
        &format!(
            "({uid}), task t1] Your task is in the pasted text: do it, then answer it with reply.\
             ^[[201~^[[200~line 1"
        ),
    );
}

/// The spec's words (docs/specs/agent-protocol.md, 1), word for word.
#[test]
fn how_to_answer_is_the_specs_text_for_its_task() {
    assert_eq!(
        crate::workspace::delegate::answer_how(7),
        "\n\n(When task t7 is done, call Templum's reply tool once, with your whole answer as its \
         text: that ends the task and hands your answer over. Without Templum's tools, run \
         `templum reply -` in your shell, the answer on its standard input.)"
    );
}

/// A task, and a change to it, ask for the answer through `reply` and end with how.
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_and_its_change_say_how_to_answer(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "find the bug" });
    call_later(handle, &ws, cx, &terminal, 1, "delegate", task.clone());
    let shown = screen_with(&ws, cx, theirs, "do it, then answer it with reply.");
    assert!(shown.contains("find the bug"), "{shown}");
    screen_with(&ws, cx, theirs, "call Templum's reply tool once");
    screen_with(&ws, cx, theirs, "run `templum reply -` in your shell");
    // A second delegate from the same agent is a change to task t1.
    call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "do it, then answer the task with reply.");
    screen_with(&ws, cx, theirs, "When task t1 is done");
}

/// An agent whose MCP client never said `initialize` has no Templum tools: the master is told why,
/// and what else ends the task (docs/specs/agent-protocol.md, 8).
#[gpui_kit::test]
fn delegate_says_why_an_agent_has_no_tools(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let hand = |cx: &mut TestAppContext| {
        // The task before it is over.
        ws.update(cx, |ws, _| {
            ws.space.terminal_mut(theirs).unwrap().handed = None
        });
        let task = json!({ "block": them, "task": "go" });
        hand_over(handle, &ws, cx, (&terminal, theirs), task, true)
    };
    let (failed, text) = hand(cx);
    assert!(!failed, "{text}");
    assert!(
        text.starts_with(&format!(
            "Handed to {them} as task t1. {them} has no Templum tools (its server hasn't connected: "
        )) && text.ends_with(
            "): it can still answer with `templum reply -` in its shell; else its turn's end or \
             its screen going still ends the task."
        ),
        "{text}"
    );
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().seen.agent = Some("amp".into())
    });
    let (_, text) = hand(cx);
    assert!(
        text.contains(
            "(Templum can't give Amp its server for one session: add `templum mcp` to its own MCP \
             servers (Settings ▸ Connections ▸ Other agents)):"
        ),
        "{text}"
    );
    // No `templum` to run in the agent's shell, in WSL (no `TEMPLUM_BUS` reaches it) or for Aider
    // (no shell tool): the end of its turn, or its screen going still, is all there is.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().seen.agent = Some("aider".into())
    });
    let (_, text) = hand(cx);
    assert!(
        text.ends_with(
            "(Aider has no MCP): its turn's end or its screen going still ends the task."
        ),
        "{text}"
    );
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(theirs).unwrap();
        pane.seen.agent = Some("claude".into());
        pane.shell = Some(templum_terminal::ShellOption {
            label: "wsl".into(),
            program: "C:/Windows/System32/wsl.exe".into(),
            args: Vec::new(),
        });
    });
    let (_, text) = hand(cx);
    assert!(
        text.ends_with(
            "(it runs in WSL, where Templum can't give it its server): its turn's end or its \
             screen going still ends the task."
        ),
        "{text}"
    );
    has_server(handle, &ws, cx, &worker, 7);
    let (failed, text) = hand(cx);
    assert_eq!(
        (failed, text.as_str()),
        (false, format!("Handed to {them} as task t5.").as_str())
    );
}

/// The first reason that fits (docs/specs/agent-protocol.md, 8).
#[gpui_kit::test]
fn the_reason_an_agent_has_no_tools_is_the_first_that_fits(cx: &mut TestAppContext) {
    let (_, ws, _, theirs) = two_agents(cx);
    let data = tempfile::tempdir().unwrap();
    let own = tempfile::tempdir().unwrap();
    crate::agents::OWN_FOLDERS.set(Some(own.path().to_path_buf()));
    ws.update(cx, |ws, _| ws.data_dir = Some(data.path().to_path_buf()));
    // Not the machine's default shell: a WSL one would be the first reason.
    let shell = |program: &str| templum_terminal::ShellOption {
        label: program.into(),
        program: program.into(),
        args: Vec::new(),
    };
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().shell = Some(shell("sh"))
    });
    let why = |cx: &mut TestAppContext, agent: &str| {
        ws.update(cx, |ws, _| {
            ws.space.terminal_mut(theirs).unwrap().seen.agent = Some(agent.into())
        });
        ws.read_with(cx, |ws, _| {
            ws.no_tools_why(ws.space.terminal(theirs).unwrap())
        })
    };
    assert!(
        why(cx, "claude").starts_with("its server hasn't connected: it is still starting"),
        "{}",
        why(cx, "claude")
    );
    assert_eq!(why(cx, "pi"), "Pi has MCP only from 0.99");
    assert_eq!(why(cx, "aider"), "Aider has no MCP");
    assert_eq!(
        why(cx, "gemini"),
        "Templum's Gemini CLI extension isn't installed (Settings ▸ Connections ▸ Gemini CLI ▸ Install)"
    );
    // Installed, Gemini CLI still starts no server in a folder it doesn't trust.
    let gemini = crate::agents::dirs(data.path()).gemini.unwrap();
    std::fs::create_dir_all(gemini.join("extensions").join("templum")).unwrap();
    assert_eq!(
        why(cx, "gemini"),
        "Gemini CLI starts no server in a folder it doesn't trust"
    );
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().shell = Some(shell("C:/Windows/System32/wsl.exe"))
    });
    assert_eq!(
        why(cx, "claude"),
        "it runs in WSL, where Templum can't give it its server"
    );
    ws.update(cx, |ws, _| ws.config.connections.enabled = false);
    assert_eq!(
        why(cx, "claude"),
        "connections are off (Settings ▸ Connections ▸ Agents in Templum)"
    );
}

/// A subagent that handed a task of its own and whose turn ends is waiting for that one, as card
/// 30 asks: its turn's end doesn't end its own task. Its `reply`, or the end of a turn with nothing
/// handed out any more, does (docs/specs/agent-protocol.md, 4).
#[cfg(unix)]
#[gpui_kit::test]
fn a_subagent_waiting_on_its_own_task_isnt_done(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (helper, other) = me(&ws, cx, third);
    let handed = |ws: &Entity<Workspace>, cx: &mut TestAppContext, id| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(id).unwrap().handed.as_ref().map(|h| h.id)
        })
    };
    // Mine hands theirs t1, which takes it and hands the third t2.
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    let task = json!({ "block": other, "task": "look" });
    let (failed, text) = hand_over(handle, &ws, cx, (&worker, third), task, true);
    assert!(!failed, "{text}");
    // Its turn ends while it waits for t2: t1 is not over, and mine is told nothing.
    let waiting = json!({ "event": "stopped", "message": "Waiting for the third." });
    event(handle, &ws, cx, &worker, waiting);
    after(cx, 1000);
    assert_eq!(handed(&ws, cx, theirs), Some(1));
    assert!(notices(&ws, cx).is_empty(), "{:?}", notices(&ws, cx));
    let shown = ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().screen());
    assert!(!shown.contains("[task t1 from "), "{shown}");
    // The third ends t2: theirs hears so (queued, it was typed into a moment ago).
    event(
        handle,
        &ws,
        cx,
        &helper,
        json!({ "event": "stopped", "message": "Found it." }),
    );
    after(cx, 1000);
    assert_eq!(handed(&ws, cx, third), None);
    let told = notices(&ws, cx);
    assert!(
        told.len() == 1 && told[0].0 == worker && told[0].1.starts_with("[task t2 from "),
        "{told:?}"
    );
    // Nothing is handed out any more: its next turn's end ends t1, once.
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let line = json!({ "event": "stopped", "message": "Line 42." });
    event(handle, &ws, cx, &worker, line);
    after(cx, 1000);
    assert_eq!(handed(&ws, cx, theirs), None);
    let shown = screen_with(&ws, cx, mine, "Line 42.");
    assert!(
        shown.contains("[task t1 from ") && shown.contains(": done in "),
        "{shown}"
    );
}

/// What a subagent handed out before its task came isn't what its task waits for: its turn's end
/// still answers it (the anti-goal of docs/specs/agent-protocol.md: a Claude Code that works today
/// works no worse).
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_handed_out_before_its_own_doesnt_keep_it_open(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (_, other) = me(&ws, cx, third);
    // Theirs handed the third a task long ago, which nobody ended.
    call_later(
        handle,
        &ws,
        cx,
        &worker,
        1,
        "delegate",
        json!({ "block": other, "task": "look" }),
    );
    // Then mine hands theirs one, which theirs answers with its turn's end.
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    let done = json!({ "event": "stopped", "message": "Done." });
    event(handle, &ws, cx, &worker, done);
    after(cx, 1000);
    let shown = screen_with(&ws, cx, mine, "Done.");
    assert!(shown.contains("[task t2 from "), "{shown}");
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(theirs).unwrap().handed.is_none())
    });
}

/// An agent without Templum's tools that waits on a task of its own (it ran `templum delegate`
/// from its shell) isn't ended by its still screen either (docs/specs/agent-protocol.md, 4).
#[cfg(unix)]
#[gpui_kit::test]
fn an_agent_without_tools_waiting_on_its_own_task_isnt_ended_by_its_screen(
    cx: &mut TestAppContext,
) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (_, other) = me(&ws, cx, third);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    call_later(
        handle,
        &ws,
        cx,
        &worker,
        1,
        "delegate",
        json!({ "block": other, "task": "look" }),
    );
    ticks(&ws, cx, theirs, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert!(delegated.try_recv().is_err(), "no answer");
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(theirs).unwrap().handed.is_some())
    });
}

#[gpui_kit::test]
fn delegate_refuses_what_it_mustnt_type_into(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go" });
    // Waiting for its person: the typed task would answer its question.
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "needs_you", "message": "Allow Bash?" }),
    );
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task.clone());
    assert!(failed && text.contains("waiting for its person"), "{text}");
    // Working on its own prompt: typed in, the task waits its turn, taken at its Enter.
    event(handle, &ws, cx, &worker, json!({ "event": "working" }));
    let at = (terminal.as_str(), theirs);
    let (failed, text) = hand_over(handle, &ws, cx, at, task.clone(), false);
    assert!(!failed, "{text}");
    // On the caller's own task: a change to it (another agent's: another_agents_task_is_still_refused).
    let (failed, text) = hand_over(handle, &ws, cx, at, task.clone(), false);
    assert_eq!(
        (failed, text),
        (false, format!("Typed into {them} as a change to task t1."))
    );
    // To itself, and to a terminal running no agent.
    let mine_task = json!({ "block": uid, "task": "go" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", mine_task);
    assert!(failed && text.contains("yours"), "{text}");
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(theirs).unwrap();
        pane.seen.agent = None;
        pane.handed = None;
    });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task);
    assert!(failed && text.contains("runs no agent"), "{text}");
}

#[gpui_kit::test]
fn a_delegation_that_waits_ends_with_the_turns_answer(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "run the tests", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    assert!(answers.try_recv().is_err(), "it waits for the answer");
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "All green." }),
    );
    assert_eq!(answered(&answers), (false, "All green.".to_string()));
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(theirs).unwrap().handed.is_none())
    });
}

#[gpui_kit::test]
fn reply_answers_the_task_before_its_turn_ends(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "find the bug", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let reply = json!({ "block": uid, "text": "Line 42." });
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply);
    assert_eq!((failed, text.as_str()), (false, "Answered task t1."));
    assert_eq!(answered(&answers), (false, "Line 42.".to_string()));
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(theirs).unwrap().handed.is_none())
    });
    // Its turn's end comes later: nothing waits for it any more.
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "Fixed it too." }),
    );
    assert!(answers.try_recv().is_err(), "answered once");
}

/// `reply` without a block ends the task its caller is on, once, with the whole text; a blank
/// one is refused and the task stays open (docs/specs/agent-protocol.md, 2).
#[cfg(unix)]
#[gpui_kit::test]
fn reply_without_a_block_answers_the_task_you_are_on(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let reply = |text: &str| json!({ "text": text });
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply("Done."));
    assert!(failed && text.starts_with("You are on no task"), "{text}");
    let task = json!({ "block": them, "task": "find the bug", "wait": true });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply("  "));
    assert_eq!(
        (failed, text.as_str()),
        (true, "reply needs your answer as text.")
    );
    assert!(waiting.try_recv().is_err(), "still open");
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply("Line 42."));
    assert_eq!((failed, text.as_str()), (false, "Answered task t1."));
    assert_eq!(answered(&waiting), (false, "Line 42.".to_string()));
    // Once: the task is over.
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply("Again."));
    assert!(failed && text.starts_with("You are on no task"), "{text}");
}

#[cfg(unix)]
#[gpui_kit::test]
fn reply_with_no_one_waiting_types_into_the_agent(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (_, uid) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let reply = json!({ "block": uid, "text": "Done." });
    // Mine runs no agent: the reply would run in its shell.
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply.clone());
    assert!(failed && text.contains("runs no agent"), "{text}");
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.agent = Some("claude".into())
    });
    found(&ws, cx, mine);
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply);
    assert_eq!((failed, text), (false, format!("Typed into {uid}.")));
    screen_with(&ws, cx, mine, "Done.");
    entered(&ws, cx, mine);
    screen_with(
        &ws,
        cx,
        mine,
        &format!("({them})] Its reply is in the pasted text. Done.[reply from"),
    );
}

/// A reply to the agent that handed the caller its task is the task's end: with no call of that
/// agent waiting, it is told as any end is, not typed as a reply.
#[cfg(unix)]
#[gpui_kit::test]
fn a_reply_with_no_one_waiting_is_the_tasks_end(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    let task = json!({ "block": them, "task": "find the bug" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &worker,
        "reply",
        json!({ "text": "Line 42." }),
    );
    assert_eq!((failed, text.as_str()), (false, "Answered task t1."));
    after(cx, 1000);
    // `cat` echoes what is typed: the notice, its first line and its answer as one line.
    let shown = screen_with(&ws, cx, mine, "Line 42.");
    assert!(
        shown.contains("[task t1 from ")
            && shown.contains(&format!("({them}): done in "))
            && shown.contains(" s] Its answer is in the pasted text. Line 42."),
        "{shown}"
    );
    assert!(!shown.contains("[reply from"), "{shown}");
    assert!(notices(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn wait_for_idle_waits_for_the_turn_and_gives_the_last_answer(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let idle = json!({ "block": them });
    let (failed, text) = call(handle, &ws, cx, &terminal, "wait_for_idle", idle.clone());
    assert!(!failed && text.contains("is idle"), "{text}");
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let answers = call_later(handle, &ws, cx, &terminal, 1, "wait_for_idle", idle.clone());
    assert!(
        answers.try_recv().is_err(),
        "it waits while the agent works"
    );
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "Merged." }),
    );
    assert_eq!(answered(&answers), (false, "Merged.".to_string()));
    let (failed, text) = call(handle, &ws, cx, &terminal, "wait_for_idle", idle);
    assert_eq!(
        (failed, text),
        (false, "Merged.".to_string()),
        "its last answer, at once"
    );
}

#[gpui_kit::test]
fn a_wait_that_times_out_gives_the_last_lines(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "think", "wait": true, "timeout": 2 });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    // The agent's screen shows the task in real time; the test's clock would otherwise run the
    // look for it out first, and its Enter would never go.
    screen_with(&ws, cx, theirs, "think");
    after(cx, 2000);
    let (failed, text) = answered(&answers);
    assert!(
        failed
            && text.contains(&format!("No answer from {them} within 2 seconds"))
            && text.contains("last lines"),
        "{text}"
    );
}

/// Codex never says it works: once a task's wait timed out, only the task, still unanswered, tells
/// that it does. `wait_for_idle` from the agent that handed it over waits for its answer, not
/// the turn before's.
#[gpui_kit::test]
fn wait_for_idle_waits_for_a_task_whose_wait_timed_out(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let stopped = |message: &str| json!({ "event": "stopped", "message": message });
    event(handle, &ws, cx, &worker, stopped("Old."));
    let task = json!({ "block": them, "task": "think", "wait": true, "timeout": 2 });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    // The agent's screen shows the task in real time; the test's clock would otherwise run the
    // look for it out first, and its Enter would never go.
    screen_with(&ws, cx, theirs, "think");
    after(cx, 2000);
    assert!(answered(&answers).0, "timed out");
    // Its deadline passed.
    ws.update(cx, |ws, _| {
        let handed = ws.space.terminal_mut(theirs).unwrap().handed.as_mut();
        let handed = handed.expect("unanswered");
        handed.until = handed.since;
    });
    let idle = json!({ "block": them });
    let waiting = call_later(handle, &ws, cx, &terminal, 2, "wait_for_idle", idle);
    assert!(waiting.try_recv().is_err(), "it still works on the task");
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    event(handle, &ws, cx, &worker, stopped("Done."));
    assert_eq!(answered(&waiting), (false, "Done.".to_string()));
}

#[gpui_kit::test]
fn a_turns_end_and_its_timeout_answer_once(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go", "wait": true, "timeout": 2 });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "Done." }),
    );
    after(cx, 3000);
    assert_eq!(answered(&answers), (false, "Done.".to_string()));
    assert!(answers.try_recv().is_err(), "answered once");
}

/// A task handed over without `wait` ends: the agent that handed it hears so in its terminal,
/// under a first line naming the task, its agent and its time.
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_handed_without_wait_comes_back_typed(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    // One word: a space where the echo's row ends is lost when the rows are joined.
    done_task(handle, &ws, cx, &terminal, theirs, "Green.");
    // `cat` echoes what is typed: the notice, its first line and its answer as one line.
    let shown = screen_with(&ws, cx, mine, "Green.");
    assert!(
        shown.contains("[task t1 from ")
            && shown.contains(&format!("({them}): done in "))
            && shown.contains(" s] Its answer is in the pasted text. Green."),
        "{shown}"
    );
    assert!(notices(&ws, cx).is_empty());
}

/// A call of the agent that handed the task over waits for it: that call gets the answer, and
/// nothing is typed. One its client gave up on (Claude Code past 2 minutes) waits no more.
#[cfg(unix)]
#[gpui_kit::test]
fn a_waiting_call_gets_the_answer_and_no_notice(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    let ends = |cx: &mut TestAppContext, said: &str| {
        screen_with(&ws, cx, theirs, said);
        entered(&ws, cx, theirs);
        event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
        event(
            handle,
            &ws,
            cx,
            &worker,
            json!({ "event": "stopped", "message": said }),
        );
    };
    // `delegate` with `wait`.
    let task = json!({ "block": them, "task": "one", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    ends(cx, "one");
    assert_eq!(answered(&answers), (false, "one".to_string()));
    after(cx, 1000);
    // `wait_for_idle` after one without.
    call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "delegate",
        json!({ "block": them, "task": "two" }),
    );
    let idle = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "wait_for_idle",
        json!({ "block": them }),
    );
    ends(cx, "two");
    assert_eq!(answered(&idle), (false, "two".to_string()));
    after(cx, 1000);
    assert!(notices(&ws, cx).is_empty());
    // Cancelled: its end comes back typed.
    let task = json!({ "block": them, "task": "three", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 3, "delegate", task);
    super::bus::bus_note(
        handle,
        &ws,
        cx,
        3,
        "notifications/cancelled",
        json!({ "requestId": 3 }),
    );
    ends(cx, "three");
    assert!(answers.try_recv().is_err(), "no one waits");
    after(cx, 1000);
    let shown = screen_with(&ws, cx, mine, "[task t3 from ");
    assert!(
        !shown.contains("[task t1") && !shown.contains("[task t2"),
        "{shown}"
    );
}

/// The agent that handed a task over isn't there to read its end: a script's `templum delegate`
/// in a shell, or an agent that quit. Nothing is typed into that shell: the notice is dropped.
#[cfg(unix)]
#[gpui_kit::test]
fn a_notice_never_goes_into_a_shell(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    // Mine runs no agent.
    done_task(handle, &ws, cx, &terminal, theirs, "One.");
    assert!(notices(&ws, cx).is_empty(), "dropped");
    // Its agent quit: its shell is in front, though the last snapshot still has it.
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(mine).unwrap();
        pane.seen.agent = Some("claude".into());
        pane.seen.pid = Some(1);
    });
    done_task(handle, &ws, cx, &terminal, theirs, "Two.");
    assert!(notices(&ws, cx).is_empty(), "dropped");
    // An agent again: typed, and that one only.
    found(&ws, cx, mine);
    done_task(handle, &ws, cx, &terminal, theirs, "Three.");
    let shown = screen_with(&ws, cx, mine, "[task t3 from ");
    assert!(
        !shown.contains("[task t1") && !shown.contains("[task t2"),
        "{shown}"
    );
}

/// What comes back waits while its asker can't take it: it asks its person, no snapshot found it
/// yet, or its person is typing into it. Oldest first, and the next only once the last one's Enter
/// can't be due any more. An agent that quit meanwhile gets nothing: its shell would run it.
#[cfg(unix)]
#[gpui_kit::test]
fn a_notice_waits_while_its_asker_asks_its_person(cx: &mut TestAppContext) {
    use crate::workspace::delegate::{CONFIRM, NOTICE_GAP, TYPING};
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    asker(&ws, cx, mine);
    let queued = |cx: &mut TestAppContext| -> Vec<String> {
        notices(&ws, cx)
            .iter()
            .map(|(_, text)| text.lines().last().unwrap().to_string())
            .collect()
    };
    let asks = json!({ "event": "needs_you", "message": "Allow Edit?" });
    event(handle, &ws, cx, &terminal, asks.clone());
    done_task(handle, &ws, cx, &terminal, theirs, "One.");
    done_task(handle, &ws, cx, &terminal, theirs, "Two.");
    assert_eq!(queued(cx), ["One.", "Two."]);
    // Answered, and restarted: no snapshot found its agent yet.
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "stopped", "message": null }),
    );
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.pid = None
    });
    after(cx, 1000);
    assert_eq!(queued(cx).len(), 2, "starting");
    // Found; its person types into it.
    found(&ws, cx, mine);
    ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().person_typed());
    after(cx, 1000);
    assert_eq!(queued(cx).len(), 2, "the person types");
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(mine).unwrap();
        pane.keyed.set(Instant::now().checked_sub(TYPING));
    });
    after(cx, 1000);
    // Both go as one paste: no other task of its agent runs (docs/specs/notice-batching.md).
    screen_with(&ws, cx, mine, "[2 tasks ended]");
    assert!(queued(cx).is_empty());
    // A third comes while their Enters may still be due: it waits.
    done_task(handle, &ws, cx, &terminal, theirs, "Three.");
    assert_eq!(queued(cx), ["Three."], "its Enter may still be due");
    after(cx, 1000);
    assert_eq!(queued(cx), ["Three."], "its Enter may still be due");
    // Its first Enter may have come up to ENTER_LOOKS looks after it was typed, its second CONFIRM
    // after that.
    let sent = |cx: &mut TestAppContext, ago| {
        ws.update(cx, |ws, _| {
            ws.space.terminal_mut(mine).unwrap().sent = Instant::now().checked_sub(ago)
        })
    };
    sent(cx, CONFIRM);
    after(cx, 1000);
    assert_eq!(queued(cx), ["Three."], "its second Enter may still be due");
    sent(cx, NOTICE_GAP);
    after(cx, 1000);
    screen_with(&ws, cx, mine, "[task t3 from ");
    assert!(queued(cx).is_empty());
    // It asks again, and quits while a notice waits: dropped.
    event(handle, &ws, cx, &terminal, asks);
    done_task(handle, &ws, cx, &terminal, theirs, "Four.");
    assert_eq!(queued(cx), ["Four."]);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.pid = Some(1)
    });
    after(cx, 1000);
    assert!(queued(cx).is_empty());
}

/// A reply typed into the asker a moment ago may still get its Enter, and a second one: a notice
/// waits as it does after another notice, and never shares that Enter.
#[cfg(unix)]
#[gpui_kit::test]
fn a_notice_waits_after_a_reply_typed_into_its_asker(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (worker, _) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    let reply = json!({ "block": uid, "text": "Hi." });
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply);
    assert!(!failed, "{text}");
    ws.update(cx, |ws, cx| {
        ws.queue_notice(
            terminal.clone(),
            "[task t9 from x: done in 1 s] y\nz".into(),
            crate::workspace::delegate::NoticeKind::Other,
            cx,
        )
    });
    after(cx, 1000);
    assert_eq!(
        notices(&ws, cx).len(),
        1,
        "the reply's Enter may still be due"
    );
}

/// What a task spent is what its agent's usage chip shows, since it was handed over: its tokens
/// (Codex), its cost when it gives one (Claude Code). Read again a second after its end; with a
/// data folder, from the agent's own files. Its transcript is the one its last turn's end named,
/// on one line. The test's clock stays under 5 s: a snapshot, with a data folder, would look for
/// real agents.
#[cfg(unix)]
#[gpui_kit::test]
fn the_end_says_what_it_spent_and_its_transcript(cx: &mut TestAppContext) {
    use templum_usage::Session;
    let spent_after = crate::workspace::delegate::SPENT_AFTER.as_millis() as u64;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    // Mine asks its person: the notices wait, to be read here.
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "needs_you", "message": "Allow Edit?" }),
    );
    let usage = |cx: &mut TestAppContext, u: Session| {
        ws.update(cx, |ws, _| {
            ws.space.terminal_mut(theirs).unwrap().usage = Some(u)
        })
    };
    let send = |cx: &mut TestAppContext, e: serde_json::Value| event(handle, &ws, cx, &worker, e);
    usage(
        cx,
        Session {
            tokens: Some(1000),
            ..Default::default()
        },
    );
    let task = json!({ "block": them, "task": "count" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    usage(
        cx,
        Session {
            tokens: Some(61231),
            ..Default::default()
        },
    );
    send(
        cx,
        json!({ "event": "stopped", "message": "Done.", "transcript": "/x/roll\nout\u{1b}.jsonl" }),
    );
    assert!(notices(&ws, cx).is_empty(), "read SPENT_AFTER later");
    after(cx, spent_after);
    let told = notices(&ws, cx);
    assert!(
        told[0].1.starts_with("[task t1 from ")
            && told[0].1.ends_with(
                " s, 60231 tokens, transcript /x/roll out.jsonl] Its answer is in the pasted text.\nDone."
            ),
        "{}",
        told[0].1
    );
    // Claude Code with its status line: its cost, which its file has a moment after its end.
    let data = tempfile::tempdir().unwrap();
    ws.update(cx, |ws, _| {
        ws.data_dir = Some(data.path().to_path_buf());
        let pane = ws.space.terminal_mut(theirs).unwrap();
        pane.seen.agent = Some("claude".into());
        pane.seen.started = Some(crate::notes::now());
    });
    usage(
        cx,
        Session {
            cost_usd: Some(0.10),
            tokens: Some(38069),
            ..Default::default()
        },
    );
    let task = json!({ "block": them, "task": "again" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    send(cx, json!({ "event": "stopped", "message": "Again." }));
    let relay = templum_usage::relay_dir(data.path());
    std::fs::create_dir_all(&relay).unwrap();
    let status = r#"{"cost":{"total_cost_usd":0.52},"context_window":{"total_input_tokens":38279,"total_output_tokens":40}}"#;
    std::fs::write(relay.join(format!("{worker}.json")), status).unwrap();
    after(cx, spent_after);
    let told = notices_until(&ws, cx, 2);
    assert!(
        told[1].1.ends_with(
            " s, $0.42, transcript /x/roll out.jsonl] Its answer is in the pasted text.\nAgain."
        ),
        "{}",
        told[1].1
    );
    // The next task, handed before a snapshot read its usage again, starts from that read: the
    // last task's calls aren't its own ($0.42), and nothing new read says no figure.
    let task = json!({ "block": them, "task": "more" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    send(cx, json!({ "event": "stopped", "message": "More." }));
    after(cx, spent_after);
    let told = notices_until(&ws, cx, 3);
    assert!(told[2].1.contains(" s, transcript "), "{}", told[2].1);
    ws.update(cx, |ws, _| ws.data_dir = None);
    // Its next session names its own.
    send(cx, json!({ "event": "started" }));
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(theirs).unwrap().transcript.is_none())
    });
}

/// The transcript a notice names is its agent's: one that left, with its hook or without a word
/// (a snapshot no longer finds it), takes it along, and an agent without hooks started next in
/// its terminal names none.
#[gpui_kit::test]
fn an_agent_that_left_takes_its_transcript_along(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    let transcript = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(theirs).unwrap().transcript.clone()
        })
    };
    let stopped = json!({ "event": "stopped", "message": "Done.", "transcript": "/t.jsonl" });
    event(handle, &ws, cx, &worker, stopped.clone());
    assert_eq!(transcript(cx).as_deref(), Some("/t.jsonl"));
    event(handle, &ws, cx, &worker, json!({ "event": "ended" }));
    assert_eq!(transcript(cx), None, "its session ended");
    event(handle, &ws, cx, &worker, stopped);
    snapshot(
        handle,
        &ws,
        cx,
        theirs,
        templum_terminal::Inspection::default(),
    );
    assert_eq!(transcript(cx), None, "a snapshot no longer finds it");
}

/// A task whose agent left, or whose block the person closed, comes back as failed, saying why.
/// One its asker ends itself (`interrupt`, `close_block`) comes back as nothing: it knows.
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_whose_agent_left_or_block_closed_comes_back_as_failed(cx: &mut TestAppContext) {
    use templum_terminal::Inspection;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let extra = another_agent(handle, &ws, cx, mine);
    let (terminal, uid) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let (_, other) = me(&ws, cx, extra);
    asker(&ws, cx, mine);
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "needs_you", "message": "Allow Edit?" }),
    );
    // Handed over: not refused. Its end comes before it took the task.
    let hand = |cx: &mut TestAppContext, block: &str| {
        let task = json!({ "block": block, "task": "go" });
        let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
        assert!(answers.try_recv().is_err(), "refused");
    };
    // Its agent left: a snapshot no longer finds it.
    hand(cx, &them);
    snapshot(handle, &ws, cx, theirs, Inspection::default());
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().seen.agent = Some("codex".into())
    });
    found(&ws, cx, theirs);
    // Interrupted by its asker.
    hand(cx, &them);
    let _stopped = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "interrupt",
        json!({ "block": them }),
    );
    // Closed by the person.
    hand(cx, &them);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(theirs, window, cx))
    })
    .unwrap();
    // Closed by its asker, which opened it.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(extra).unwrap().opened_by = Some(uid.clone())
    });
    hand(cx, &other);
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &terminal,
            "close_block",
            json!({ "block": other })
        )
        .0
    );
    after(cx, 1000);
    let told: Vec<String> = notices(&ws, cx).into_iter().map(|(_, t)| t).collect();
    assert_eq!(told.len(), 2, "t1 and t3 only: {told:?}");
    // Both ended in the same instant of the test's clock: found by their ids.
    let task = |id: &str| {
        told.iter()
            .find(|t| t.starts_with(&format!("[task {id} from ")))
            .unwrap()
    };
    assert!(
        task("t1").contains(": failed in ")
            && task("t1").ends_with(
                "] What happened is in the pasted text.\nIts agent ended before it answered."
            ),
        "{told:?}"
    );
    assert!(
        task("t3").ends_with(&format!("\n{them} was closed before it answered.")),
        "{told:?}"
    );
}

/// A task's agent starts waiting for its person: the agent that handed it hears so, with the
/// dialog as drawn, once per wait, what it asks on one line. Not while a call of its waits for
/// the task's end: it is inside that call.
#[cfg(unix)]
#[gpui_kit::test]
fn a_dialog_in_a_task_is_told_once(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "needs_you", "message": "Allow Edit?" }),
    );
    let send = |cx: &mut TestAppContext, e: serde_json::Value| event(handle, &ws, cx, &worker, e);
    let asks = |m: serde_json::Value| json!({ "event": "needs_you", "message": m });
    let told = |cx: &mut TestAppContext| -> Vec<String> {
        notices(&ws, cx).into_iter().map(|(_, t)| t).collect()
    };
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    send(cx, asks(json!("Allow Bash?")));
    shows(&ws, cx, theirs, "1. Yes, proceed (y)");
    // The same wait: its notification, and another message for it.
    send(cx, asks(serde_json::Value::Null));
    send(cx, asks(json!("Claude needs your permission")));
    after(cx, 300);
    let t = told(cx);
    assert_eq!(t.len(), 1, "{t:?}");
    assert!(
        t[0].starts_with("[task t1 from ")
            && t[0].contains(&format!(
                "({them}) waits for its person: Allow Bash?] Its screen is in the pasted text.\n"
            ))
            && t[0].contains("1. Yes, proceed (y)"),
        "{}",
        t[0]
    );
    // Allowed, it works, and asks again: a new wait.
    send(cx, json!({ "event": "working" }));
    send(cx, asks(json!("Allow\nEdit?")));
    after(cx, 300);
    let t = told(cx);
    assert!(
        t.len() == 2 && t[1].contains("waits for its person: Allow Edit?]"),
        "{t:?}"
    );
    // A call of its asker waits for the task's end.
    let _idle = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "wait_for_idle",
        json!({ "block": them }),
    );
    send(cx, json!({ "event": "working" }));
    send(cx, asks(json!("Allow Bash?")));
    after(cx, 300);
    assert_eq!(told(cx).len(), 2);
}

/// The agent that handed a task over waits for its agent to need its person
/// (`wait_for(needs_you)`): that call tells it the dialog, and nothing is typed.
#[cfg(unix)]
#[gpui_kit::test]
fn a_dialog_its_asker_waits_for_isnt_typed_too(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    // Mine asks its person: a notice would wait, to be read here.
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "needs_you", "message": "Allow Edit?" }),
    );
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    let needs_you = json!({ "block": them, "event": "needs_you" });
    let answers = call_later(handle, &ws, cx, &terminal, 2, "wait_for", needs_you);
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "needs_you", "message": "Allow Bash?" }),
    );
    after(cx, 300);
    let (failed, text) = answered(&answers);
    assert!(
        !failed && text.starts_with(&format!("{them} is waiting for its person: Allow Bash?")),
        "{text}"
    );
    assert!(notices(&ws, cx).is_empty(), "{:?}", notices(&ws, cx));
}

/// A task's agent asks its person through Templum (`ask_user`): its balloon asks, which the person
/// sees and the agent that handed it the task can't answer: that agent hears nothing.
#[cfg(unix)]
#[gpui_kit::test]
fn a_balloon_in_a_task_isnt_told(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    // Mine asks its person: the notice waits, to be read here.
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "needs_you", "message": "Allow Edit?" }),
    );
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "No"] });
    let _asked = call_later(handle, &ws, cx, &worker, 2, "ask_user", asked);
    after(cx, 300);
    let told = notices(&ws, cx);
    assert!(told.is_empty(), "{told:?}");
    let shown = ws.read_with(cx, |ws, _| ws.asking(&worker).is_some());
    assert!(shown, "the person sees it");
}

/// A task handed over without `wait` gives no answer within its timeout: it may still work, so its
/// asker hears nothing then, only its end later. One it waits for hears its timeout from its call.
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_past_its_timeout_comes_back_only_at_its_end(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "needs_you", "message": "Allow Edit?" }),
    );
    let told = |cx: &mut TestAppContext| -> Vec<String> {
        notices(&ws, cx).into_iter().map(|(_, t)| t).collect()
    };
    let task = json!({ "block": them, "task": "think", "timeout": 2 });
    call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    // Its Enter goes while the test's clock runs: the screen shows the task in real time.
    screen_with(&ws, cx, theirs, "think");
    after(cx, 2000);
    after(cx, 1000);
    assert!(told(cx).is_empty(), "{:?}", told(cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space
                .terminal(theirs)
                .unwrap()
                .handed
                .as_ref()
                .map(|h| h.id),
            Some(1)
        )
    });
    // Its end, later.
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "Late." }),
    );
    after(cx, 1000);
    let t = told(cx);
    assert!(
        t.len() == 1 && t[0].contains(": done in ") && t[0].ends_with("\nLate."),
        "{t:?}"
    );
    // With `wait`: its call says it timed out, and nothing is typed.
    let task = json!({ "block": them, "task": "ponder", "wait": true, "timeout": 2 });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "ponder");
    entered(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    after(cx, 2000);
    assert!(answered(&answers).0, "timed out");
    after(cx, 1000);
    assert_eq!(told(cx).len(), 1);
}

/// The agent whose task it is changes it while its agent works: typed at once, under a line naming
/// the task; the end of the turn that took the change answers it, and its timeout starts again.
/// Before a spawned agent got its prompt nothing goes: it would come first.
#[cfg(unix)]
#[gpui_kit::test]
fn a_change_from_the_asker_goes_into_its_task(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let send = |cx: &mut TestAppContext, e: serde_json::Value| event(handle, &ws, cx, &worker, e);
    let task = json!({ "block": them, "task": "say A" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    let change = json!({ "block": them, "task": "also say B", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 2, "delegate", change);
    screen_with(
        &ws,
        cx,
        theirs,
        &format!(
            "({uid}), a change to task t1] The change is in the pasted text: do it, then answer the task with reply."
        ),
    );
    ws.read_with(cx, |ws, _| {
        let handed = ws.space.terminal(theirs).unwrap().handed.as_ref().unwrap();
        assert!(handed.id == 1 && !handed.taken);
    });
    // The turn under way ends before taking it; Codex takes it into the next.
    send(cx, json!({ "event": "stopped", "message": "A" }));
    assert!(answers.try_recv().is_err(), "the turn before the change");
    send(cx, json!({ "event": "submitted" }));
    send(cx, json!({ "event": "stopped", "message": "B" }));
    assert_eq!(answered(&answers), (false, "B".to_string()));
    // Its timeout starts again from the change's.
    let task = json!({ "block": them, "task": "count", "timeout": 2 });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    // At work: it queues the change, taken at its Enter.
    let change = json!({ "block": them, "task": "count to 9", "timeout": 5 });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), change, false);
    assert_eq!(
        (failed, text),
        (false, format!("Typed into {them} as a change to task t2."))
    );
    ws.read_with(cx, |ws, _| {
        let handed = ws.space.terminal(theirs).unwrap().handed.as_ref().unwrap();
        assert!(handed.until >= handed.since + std::time::Duration::from_secs(5));
    });
    // A spawned agent whose prompt still waits.
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(theirs).unwrap();
        pane.handed = Some(crate::workspace::Handed {
            id: 7,
            ..crate::workspace::Handed::new(&terminal, std::time::Duration::from_secs(600))
        });
        pane.prompt = Some(crate::workspace::Prompt {
            text: "say hi".into(),
            screen: None,
            gated: false,
            delegated: false,
        });
    });
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "delegate",
        json!({ "block": them, "task": "say ho" }),
    );
    assert!(failed && text.contains("hasn't got task t7 yet"), "{text}");
}

/// Another agent's task: refused, whether it waits or not, and the task stays its asker's.
#[gpui_kit::test]
fn another_agents_task_is_still_refused(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    let (terminal, _) = me(&ws, cx, mine);
    let (other, _) = me(&ws, cx, third);
    let (_, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "a" });
    assert!(!hand_over(handle, &ws, cx, (&terminal, theirs), task, true).0);
    for wait in [false, true] {
        let task = json!({ "block": them, "task": "b", "wait": wait });
        let (failed, text) = call(handle, &ws, cx, &other, "delegate", task);
        assert!(
            failed && text.contains("already on a task another agent handed it"),
            "{text}"
        );
    }
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space
                .terminal(theirs)
                .unwrap()
                .handed
                .as_ref()
                .unwrap()
                .by,
            terminal
        )
    });
}

/// The notices once there are `n`: a read on the background executor may come a moment later.
#[cfg(unix)]
fn notices_until(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    n: usize,
) -> Vec<(String, String)> {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        let told = notices(ws, cx);
        if told.len() >= n {
            return told;
        }
        assert!(Instant::now() < deadline, "{told:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// The turn an agent was on when a task was typed into it ends first, and isn't the task's
/// answer. Codex says it took the task once a turn takes it in; Claude Code says so at its Enter
/// (`at_enter`) and queues it behind the turn under way, which a tool's end takes it into.
#[gpui_kit::test]
fn a_turns_end_before_its_task_was_taken_doesnt_answer_it(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let send = |cx: &mut TestAppContext, e: serde_json::Value| event(handle, &ws, cx, &worker, e);
    let submitted = |at_enter: bool| json!({ "event": "submitted", "at_enter": at_enter });
    let stopped = |message: &str| json!({ "event": "stopped", "message": message });
    let task = json!({ "block": them, "task": "say B", "wait": true });
    // Codex at work on its person's prompt: the turn under way ends before taking the task.
    send(cx, submitted(false));
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task.clone());
    send(cx, stopped("A"));
    assert!(answers.try_recv().is_err(), "the turn it was on");
    send(cx, submitted(false));
    send(cx, stopped("B"));
    assert_eq!(answered(&answers), (false, "B".to_string()));
    // Claude Code queues it at its Enter; the turn under way ends with no tool after it.
    send(cx, submitted(true));
    let answers = call_later(handle, &ws, cx, &terminal, 2, "delegate", task.clone());
    send(cx, submitted(true));
    send(cx, stopped("1 to 300"));
    assert!(answers.try_recv().is_err(), "the turn it was queued behind");
    send(cx, stopped("B"));
    assert_eq!(answered(&answers), (false, "B".to_string()));
    // A tool's end takes it into the turn under way, whose end answers it.
    send(cx, submitted(true));
    let answers = call_later(handle, &ws, cx, &terminal, 3, "delegate", task);
    send(cx, submitted(true));
    send(cx, json!({ "event": "working" }));
    send(cx, stopped("A\nB"));
    assert_eq!(answered(&answers), (false, "A\nB".to_string()));
}

#[gpui_kit::test]
fn closing_the_agent_worked_on_answers_its_wait(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(theirs, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    let (failed, text) = answered(&answers);
    assert!(
        failed && text.contains("was closed before it answered"),
        "{text}"
    );
}

#[gpui_kit::test]
fn a_dot_travels_only_while_a_task_is_handed_over(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let dots = |cx: &mut TestAppContext, at: Instant, still: bool| {
        ws.read_with(cx, |ws, _| ws.wire_dots(at, still))
    };
    assert!(
        dots(cx, Instant::now(), false).is_empty(),
        "nothing handed over"
    );
    call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "delegate",
        json!({ "block": them, "task": "go" }),
    );
    let since = ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(theirs)
            .unwrap()
            .handed
            .as_ref()
            .unwrap()
            .since
    });
    let trip = crate::wires::DOT_TRIP;
    // The wire runs from mine's right edge (740, 250) to theirs' left one (900, 250). The dot
    // leaves from the agent that handed the task over, is halfway at half a trip, and starts again.
    let near = |dots: Vec<Point>, x: f32| {
        dots.len() == 1 && (dots[0].x - x).abs() < 0.5 && (dots[0].y - 250.0).abs() < 0.5
    };
    assert!(near(dots(cx, since, false), 740.0));
    assert!(near(dots(cx, since + trip / 2, false), 820.0));
    assert!(near(dots(cx, since + trip, false), 740.0));
    assert!(
        near(dots(cx, since, true), 820.0),
        "still, at the middle, with reduced motion"
    );
    // Painted with the wires.
    frame(handle, cx);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "Done." }),
    );
    assert!(
        dots(cx, Instant::now(), false).is_empty(),
        "the answer came back"
    );
}

#[gpui_kit::test]
fn the_dot_goes_with_its_wire_and_the_answer_still_comes(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    ws.update(cx, |ws, cx| {
        assert!(ws.disconnect(Wire { a: mine, b: theirs }, cx))
    });
    ws.read_with(cx, |ws, _| {
        assert!(ws.wire_dots(Instant::now(), false).is_empty())
    });
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "Done." }),
    );
    assert_eq!(answered(&answers), (false, "Done.".to_string()));
}

/// Codex and Copilot CLI, reading a paste as keys (Windows' console passes no bracketed paste
/// on), take an Enter that comes with them, or just after, for a new line in it: the task sits
/// in their input. Nothing moved after it: a second Enter goes, and sends it. `delegate` answers
/// once it went.
#[cfg(unix)]
#[gpui_kit::test]
fn an_enter_taken_for_a_new_line_goes_again(cx: &mut TestAppContext) {
    // Its first line read, the second Enter starts it.
    let fake = "echo ready; sh -c 'read -r task; read -r enter; echo SUBMITTED; exec cat'; true";
    let (handle, ws, mine, theirs) = agents_running(cx, Some(jobs(fake)));
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    screen_with(&ws, cx, theirs, "ready");
    in_front(&ws, cx, theirs, true);
    found(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go on" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, false);
    assert!(
        !failed && text.starts_with(&format!("Handed to {them} as task t1.")),
        "{text}"
    );
    screen_with(&ws, cx, theirs, "SUBMITTED");
    let handed = ws.read_with(cx, |ws, _| {
        ws.space.terminal(theirs).unwrap().handed.is_some()
    });
    assert!(handed, "its task goes on");
}

/// An agent whose input never sends what it holds (an input not ready yet, a promo dialog over
/// it): two Enters change nothing on its screen. `delegate` says the task was typed but not
/// submitted, instead of handing it over, and ends it: no notice of that end comes besides.
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_two_enters_didnt_send_is_answered_as_not_taken(cx: &mut TestAppContext) {
    // It shows what is typed, an Enter as a carriage return, and sends nothing.
    let fake = "stty -icanon -icrnl -echoctl; echo ready; cat >/dev/null; true";
    let (handle, ws, mine, theirs) = agents_running(cx, Some(jobs(fake)));
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    screen_with(&ws, cx, theirs, "ready");
    in_front(&ws, cx, theirs, true);
    found(&ws, cx, theirs);
    let inputs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(theirs).unwrap().term.inputs())
    };
    let before = inputs(cx);
    let task = json!({ "block": them, "task": "go on" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, false);
    assert!(
        failed
            && text.starts_with(&format!(
                "{them} didn't take the task: typed but not submitted: two Enters changed \
                 nothing on its screen"
            ))
            && text.contains("go on"),
        "{text}"
    );
    assert_eq!(inputs(cx) - before, 4, "two pastes, two Enters");
    let handed = ws.read_with(cx, |ws, _| {
        ws.space.terminal(theirs).unwrap().handed.is_some()
    });
    assert!(!handed, "the task ended");
    after(cx, 2000);
    assert!(notices(&ws, cx).is_empty(), "{:?}", notices(&ws, cx));
    let shown = ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().screen());
    assert!(!shown.contains("[task t1 from"), "told twice: {shown}");
}

/// A screen the task doesn't show on (a dialog that ignores the paste, as Codex's approval does)
/// gets no Enter: it would pick the dialog's answer for the person. The task ends at once, and
/// the agent that handed it over hears so. A reply that isn't taken ends no task.
#[cfg(unix)]
#[gpui_kit::test]
fn no_enter_goes_to_a_screen_the_task_didnt_change(cx: &mut TestAppContext) {
    // A program that shows nothing it is typed; `cat` prints a line only once Enter sends it.
    let command = jobs("stty -echo; echo ready; cat; true");
    let (handle, ws, mine, theirs) = agents_running(cx, Some(command));
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    screen_with(&ws, cx, theirs, "ready");
    // `cat`, not `stty`, which the snapshot may have found in front.
    in_front(&ws, cx, theirs, true);
    found(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go on", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    after(cx, 5000);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let shown = ws.read_with(cx, |ws, _| ws.space.terminal(theirs).unwrap().screen());
    assert!(!shown.contains("go on"), "an Enter went: {shown:?}");
    let (failed, text) = answered(&answers);
    assert!(
        failed && text.starts_with(&format!("{them} didn't take the task")),
        "{text}"
    );
    let handed = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(theirs).unwrap().handed.is_some()
        })
    };
    assert!(!handed(cx), "the task ended");
    // A task another agent handed it stays, whatever happens to a reply typed into it.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().handed = Some(crate::workspace::Handed::new(
            "another",
            std::time::Duration::from_secs(600),
        ));
    });
    let reply = json!({ "block": them, "text": "Done." });
    let (failed, text) = call(handle, &ws, cx, &terminal, "reply", reply);
    assert!(!failed, "{text}");
    after(cx, 5000);
    assert!(handed(cx), "a reply ends no task");
}

/// A dialog that opens between the paste and its Enter (Claude Code asks to run a tool while it
/// works on a prompt of its own, its screen changing all along) gets no Enter: it would pick the
/// dialog's first answer for the person.
#[cfg(unix)]
#[gpui_kit::test]
fn no_enter_goes_to_an_agent_that_started_asking_its_person(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let task = json!({ "block": them, "task": "go on", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "go on");
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "needs_you", "message": "Allow Bash?" }),
    );
    after(cx, 5000);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let shown = screen_with(&ws, cx, theirs, "go on");
    assert_eq!(
        shown.matches("go on").count(),
        1,
        "an Enter went: {shown:?}"
    );
    let (failed, text) = answered(&answers);
    assert!(failed && text.contains("didn't take the task"), "{text}");
}

/// An agent that quit leaves its shell in front, where typed text would run as a command; until a
/// snapshot misses it (up to 5 s), Templum still takes it to run there.
#[cfg(unix)]
#[gpui_kit::test]
fn nothing_is_typed_into_the_shell_an_agent_left(cx: &mut TestAppContext) {
    // Its agent's job ends at an end of file; its shell (reading as `cat` does) is then in front.
    let (handle, ws, mine, theirs) = agents_running(cx, Some(jobs("cat; exec cat")));
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(theirs).unwrap().term.write(&b"\x04"[..])
    });
    in_front(&ws, cx, theirs, false);
    ws.read_with(cx, |ws, _| {
        let seen = &ws.space.terminal(theirs).unwrap().seen;
        assert!(seen.agent.is_some(), "no snapshot missed it yet");
    });
    // Before a snapshot found it (a Claude Code's prompt typed at its start), its shell's own
    // process in front tells.
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(theirs).unwrap();
        let pid = pane.seen.pid.take();
        assert!(pane.shell_in_front(), "its shell is in front");
        pane.seen.pid = pid;
    });
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task.clone());
    assert!(failed && text.contains("runs no agent"), "{text}");
    // An agent run in its shell's place (`exec claude`) is in front as the shell, where a
    // snapshot found it.
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(theirs).unwrap();
        pane.seen.pid = pane.term.processes().shell;
    });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
}

/// macOS starts a terminal's shell under `login`, and the shell takes a process group of its own:
/// once its agent quit, what is in front isn't the terminal's process. That the agent a snapshot
/// found is no longer in front tells.
#[cfg(unix)]
#[gpui_kit::test]
fn nothing_is_typed_into_a_shell_under_login_an_agent_left(cx: &mut TestAppContext) {
    // The first `sh` stands in for `login`, the second for the shell, its job for the agent.
    let shell = jobs(r#"sh -c 'set -m; sh -c "echo agent; exec cat"; exec cat'; true"#);
    let (handle, ws, mine, theirs) = agents_running(cx, Some(shell));
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    screen_with(&ws, cx, theirs, "agent");
    let front = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(theirs).unwrap().term.processes()
        })
    };
    let agent = front(cx).foreground;
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(theirs).unwrap();
        pane.seen.pid = agent;
        assert!(!pane.shell_in_front(), "the agent is in front");
        pane.term.write(&b"\x04"[..]);
    });
    let deadline = Instant::now() + std::time::Duration::from_secs(5);
    while front(cx).foreground == agent {
        assert!(Instant::now() < deadline, "the agent never quit");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let p = front(cx);
    assert_ne!(
        p.foreground, p.shell,
        "the shell isn't the terminal's process"
    );
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task);
    assert!(failed && text.contains("runs no agent"), "{text}");
}

/// An agent that goes without a word (Codex says nothing when it quits; one that crashed): the
/// first snapshot that misses its process ends its task.
#[gpui_kit::test]
fn an_agent_that_leaves_without_a_word_ends_its_task(cx: &mut TestAppContext) {
    use templum_terminal::Inspection;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let snapshot = |cx: &mut TestAppContext, found| snapshot(handle, &ws, cx, theirs, found);
    let running = Inspection {
        agent: Some("codex".into()),
        pid: Some(42),
        ..Default::default()
    };
    snapshot(cx, running);
    assert!(answers.try_recv().is_err(), "at work");
    snapshot(cx, Inspection::default());
    let (failed, text) = answered(&answers);
    assert!(
        failed && text.contains("ended before it answered"),
        "{text}"
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(theirs).unwrap().handed.is_none());
        assert!(
            ws.wire_dots(Instant::now(), false).is_empty(),
            "the dot stops"
        );
    });
}

#[gpui_kit::test]
fn wait_for_needs_you_now_or_when_it_asks(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let needs_you = json!({ "block": them, "event": "needs_you" });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "wait_for", needs_you.clone());
    assert!(answers.try_recv().is_err(), "it waits");
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "needs_you", "message": "Allow Bash?" }),
    );
    let asks = format!("{them} is waiting for its person: Allow Bash?");
    after(cx, 300);
    let (failed, text) = answered(&answers);
    assert!(!failed && text.starts_with(&asks), "{text}");
    let (failed, text) = call(handle, &ws, cx, &terminal, "wait_for", needs_you.clone());
    assert!(
        !failed && text.starts_with(&asks),
        "at once while it still asks: {text}"
    );
    // Its own question, in its balloon, is a wait for its person too.
    event(handle, &ws, cx, &worker, json!({ "event": "working" }));
    let answers = call_later(handle, &ws, cx, &terminal, 2, "wait_for", needs_you);
    let _question = call_later(
        handle,
        &ws,
        cx,
        &worker,
        3,
        "ask_user",
        json!({ "question": "Which branch?" }),
    );
    after(cx, 300);
    let (failed, text) = answered(&answers);
    let asks = format!("{them} is waiting for its person: Which branch?");
    assert!(!failed && text.starts_with(&asks), "{text}");
}

/// The dialog an agent waits on, as drawn: what answers it with `send_keys` reads its options.
/// Its hook comes before it is drawn (the hook may answer it).
#[cfg(unix)]
#[gpui_kit::test]
fn wait_for_needs_you_shows_the_dialog_as_drawn(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let needs_you = json!({ "block": them, "event": "needs_you" });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "wait_for", needs_you.clone());
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "needs_you", "message": "Allow Bash?" }),
    );
    shows(&ws, cx, theirs, "1. Yes, proceed (y)");
    after(cx, 300);
    let (failed, text) = answered(&answers);
    assert!(
        !failed
            && text.starts_with(&format!("{them} is waiting for its person: Allow Bash?"))
            && text.contains("1. Yes, proceed (y)"),
        "{text}"
    );
    let (failed, text) = call(handle, &ws, cx, &terminal, "wait_for", needs_you);
    assert!(
        !failed && text.contains("1. Yes, proceed (y)"),
        "at once too: {text}"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn delegate_into_a_dialog_is_refused_with_its_screen(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "needs_you", "message": "Allow Bash?" }),
    );
    shows(
        &ws,
        cx,
        theirs,
        "3. No, and tell Codex what to do differently (esc)",
    );
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = call(handle, &ws, cx, &terminal, "delegate", task);
    assert!(
        failed
            && text.contains("waiting for its person: Allow Bash?")
            && text.contains("send_keys")
            && text.contains("3. No, and tell Codex"),
        "{text}"
    );
}

#[gpui_kit::test]
fn wait_for_idle_is_wait_for_idle_and_the_rest_need_a_wire(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "wait_for",
        json!({ "block": them, "event": "idle" }),
    );
    assert!(!failed && text.contains("is idle"), "{text}");
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "wait_for",
        json!({ "block": uid, "event": "attention" }),
    );
    assert!(failed && text.contains("yours"), "{text}");
    ws.update(cx, |ws, cx| {
        ws.disconnect(Wire::new(mine, theirs).unwrap(), cx);
    });
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "wait_for",
        json!({ "block": them, "event": "attention" }),
    );
    assert!(failed && text.contains("isn't connected to you"), "{text}");
}

/// Keys go as a keyboard sends them, never as a paste: a dialog takes `2` as its option, where a
/// pasted one would land in the agent's input.
#[cfg(unix)]
#[gpui_kit::test]
fn send_keys_presses_keys_not_a_paste(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    // `cat` prints the sequence back: the terminal now brackets pastes, as Claude Code's does.
    prints(&ws, cx, theirs, b"\x1b[?2004hon\r");
    screen_with(&ws, cx, theirs, "2004honon");
    let keys = json!({ "block": them, "keys": ["2", "escape"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "send_keys", keys);
    let (failed, text) = heard(cx, &answers);
    assert!(
        !failed && text.starts_with(&format!("Pressed 2 keys in {them}.")),
        "{text}"
    );
    assert!(text.ends_with("2^["), "the screen after them: {text}");
    let shown = screen_with(&ws, cx, theirs, "2^[");
    assert!(!shown.contains("200~"), "never a paste: {shown}");
}

/// Keys go into an agent waiting for its person, as the person's: once its screen moves, it works.
#[cfg(unix)]
#[gpui_kit::test]
fn send_keys_takes_a_dialog_as_the_persons_keys(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
    event(handle, &ws, cx, &worker, asks);
    let keys = json!({ "block": them, "keys": ["2"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "send_keys", keys);
    let (failed, text) = heard(cx, &answers);
    assert!(
        !failed && text.starts_with(&format!("Pressed 1 key in {them}.")),
        "{text}"
    );
    ws.read_with(cx, |ws, _| {
        let (by, _) = ws
            .space
            .terminal(theirs)
            .unwrap()
            .keys_from
            .clone()
            .expect("who");
        assert!(by.ends_with(&format!("({uid})")), "{by}");
    });
    ticks(&ws, cx, theirs, 0, 4);
    assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Working));
}

#[cfg(unix)]
#[gpui_kit::test]
fn send_keys_refuses_its_own_block_an_unknown_key_and_another_agents_task(cx: &mut TestAppContext) {
    use crate::workspace::Handed;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let pressing = |block: &str, keys: serde_json::Value| json!({ "block": block, "keys": keys });
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "send_keys",
        pressing(&uid, json!(["y"])),
    );
    assert!(failed && text.contains("yours"), "{text}");
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "send_keys",
        pressing(&them, json!(["y", "foo"])),
    );
    assert!(
        failed && text.contains("\"foo\"") && text.contains("page_up"),
        "{text}"
    );
    let typed = ws.read_with(cx, |ws, _| ws.space.terminal(theirs).unwrap().typed.get());
    assert!(!typed, "no key went before the refusal");
    // On another agent's task: refused while it works, not while it waits for its person.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().handed = Some(Handed::new(
            "elsewhere",
            std::time::Duration::from_secs(600),
        ));
    });
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "send_keys",
        pressing(&them, json!(["y"])),
    );
    assert!(failed && text.contains("another agent"), "{text}");
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "needs_you", "message": "Allow Bash?" }),
    );
    let y = pressing(&them, json!(["y"]));
    let answers = call_later(handle, &ws, cx, &terminal, 2, "send_keys", y);
    assert!(
        !heard(cx, &answers).0,
        "it waits for its person: its dialog takes keys"
    );
    // Its block closes between two keys: the call ends there.
    let ab = pressing(&them, json!(["a", "b"]));
    let answers = call_later(handle, &ws, cx, &terminal, 3, "send_keys", ab);
    cx.run_until_parked();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.close_now(&[theirs], crate::workspace::history::Then::New, window, cx)
        })
    })
    .unwrap();
    let (failed, text) = heard(cx, &answers);
    assert!(failed && text.contains("closed"), "{text}");
}

#[cfg(unix)]
#[gpui_kit::test]
fn interrupt_presses_esc_and_ends_the_callers_task(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let stop = json!({ "block": them });
    let (failed, text) = call(handle, &ws, cx, &terminal, "interrupt", stop.clone());
    assert!(failed && text.contains("idle"), "{text}");
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "interrupt",
        json!({ "block": uid }),
    );
    assert!(failed && text.contains("yours"), "{text}");
    let task = json!({ "block": them, "task": "count to a million", "wait": true });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    // A spawn's prompt still waiting is that task: it goes with it.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().prompt = Some(crate::workspace::Prompt {
            text: "count to a million".into(),
            screen: None,
            gated: false,
            delegated: false,
        })
    });
    let stopped = call_later(handle, &ws, cx, &terminal, 2, "interrupt", stop.clone());
    assert_eq!(
        answered(&waiting),
        (true, "Interrupted by you.".to_string())
    );
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(theirs).unwrap();
        assert!(pane.handed.is_none() && pane.prompt.is_none())
    });
    let (failed, text) = heard(cx, &stopped);
    assert!(
        !failed
            && text.starts_with(&format!(
                "Pressed Esc in {them}, and ended the task you handed it."
            ))
            && text.contains("^["),
        "{text}"
    );
    // Another agent's task: only that agent stops it.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().handed = Some(crate::workspace::Handed::new(
            "elsewhere",
            std::time::Duration::from_secs(600),
        ));
    });
    let (failed, text) = call(handle, &ws, cx, &terminal, "interrupt", stop);
    assert!(failed && text.contains("another agent"), "{text}");
}

/// Keys from another block name it after the dot, in place of the usage chip, and in the dot's
/// tooltip, for ten seconds.
#[gpui_kit::test]
fn keys_name_who_pressed_them_for_ten_seconds(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, id);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().seen.agent = Some("codex".into())
    });
    event(handle, &ws, cx, &terminal, json!({ "event": "submitted" }));
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().keys_from =
            Some(("Claude Code (b7k2m)".into(), Instant::now()))
    });
    let tip = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            crate::block::status_dot(ws, ws.space.terminal(id).unwrap()).map(|(_, t)| t.to_string())
        })
    };
    let chip = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(("answered-by", id.0 as usize)).is_some()
        })
        .unwrap()
    };
    assert_eq!(
        tip(&ws, cx).as_deref(),
        Some("Working · answered by Claude Code (b7k2m)")
    );
    assert!(chip(cx));
    ws.update(cx, |ws, _| {
        let at = &mut ws
            .space
            .terminal_mut(id)
            .unwrap()
            .keys_from
            .as_mut()
            .unwrap()
            .1;
        *at = at.checked_sub(std::time::Duration::from_secs(11)).unwrap();
    });
    assert_eq!(tip(&ws, cx).as_deref(), Some("Working"));
    assert!(!chip(cx));
}

/// `cat` in block `id` prints `bytes` back, as a program would.
#[cfg(unix)]
pub(super) fn prints(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
    bytes: &'static [u8],
) {
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(id).unwrap().term.write(bytes.to_vec())
    });
}

/// The answer on `answers`, the test's clock running a look at a time: the terminal's own thread
/// reads what `cat` prints back.
#[cfg(unix)]
pub(super) fn heard(
    cx: &mut TestAppContext,
    answers: &std::sync::mpsc::Receiver<String>,
) -> (bool, String) {
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        after(cx, 250);
        if let Ok(line) = answers.try_recv() {
            return result_of(&line);
        }
        assert!(Instant::now() < deadline, "no answer");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[cfg(unix)]
#[gpui_kit::test]
fn wait_for_a_finished_command_or_a_call_for_attention(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let event = |name: &str| json!({ "block": them, "event": name });
    let finished = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "wait_for",
        event("command_finished"),
    );
    let attention = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "wait_for",
        event("attention"),
    );
    after(cx, 500);
    assert!(
        finished.try_recv().is_err() && attention.try_recv().is_err(),
        "nothing yet"
    );
    // A command starts and ends with code 3: OSC 133 C, then D;3.
    prints(&ws, cx, theirs, b"\x1b]133;C\x07\x1b]133;D;3\x07\r");
    let (failed, text) = heard(cx, &finished);
    assert!(
        !failed && text.starts_with(&format!("A command finished in {them}, with exit code 3.")),
        "{text}"
    );
    assert!(
        attention.try_recv().is_err(),
        "a command isn't a call for attention"
    );
    prints(&ws, cx, theirs, b"\x1b]777;notify;Build;done\x07\r");
    assert_eq!(
        heard(cx, &attention),
        (false, format!("{them} asks for attention: done"))
    );
    let bell = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        3,
        "wait_for",
        event("attention"),
    );
    prints(&ws, cx, theirs, b"\x07\r");
    assert_eq!(heard(cx, &bell), (false, format!("{them} rang its bell.")));
}

#[gpui_kit::test]
fn a_wait_for_times_out_goes_with_its_block_and_shows_no_balloon(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let soon = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "wait_for",
        json!({ "block": them, "event": "attention", "timeout": 1 }),
    );
    assert!(
        ws.read_with(cx, |ws, _| ws.asking(&terminal).is_none()),
        "a wait asks the person nothing"
    );
    after(cx, 1000);
    let (failed, text) = answered(&soon);
    assert!(
        failed
            && text.starts_with(&format!(
                "No call for attention in {them} within 1 seconds."
            )),
        "{text}"
    );
    let closed = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "wait_for",
        json!({ "block": them, "event": "command_finished" }),
    );
    // Its block goes, and no frame is drawn (a hidden window draws none): `take_block` asks for
    // none, where `remove_block` would, and a test draws each window that asked.
    let _gone = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.take_block(theirs, window, cx))
        })
        .unwrap();
    after(cx, 250);
    assert_eq!(
        answered(&closed),
        (true, format!("{them} was closed while you waited."))
    );
}

/// How many watches the terminal of block `id` has.
#[cfg(unix)]
fn watching(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> usize {
    ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.watching())
}

#[cfg(unix)]
#[gpui_kit::test]
fn wait_for_a_pattern_answers_on_the_printed_line(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        7,
        "wait_for",
        json!({ "block": them, "pattern": "Ready in \\d+" }),
    );
    after(cx, 250);
    assert!(answers.try_recv().is_err(), "nothing printed yet");
    assert_eq!(watching(&ws, cx, theirs), 1);
    prints(&ws, cx, theirs, b"starting\r  Ready in 312 ms\r");
    let (failed, text) = heard(cx, &answers);
    assert!(!failed, "{text}");
    assert!(
        text.starts_with(&format!(
            "{them} printed a line matching \"Ready in \\d+\":\n  Ready in 312 ms\nIts last lines:\n"
        )),
        "{text}"
    );
    assert_eq!(watching(&ws, cx, theirs), 0, "answered, its watch goes");
}

#[cfg(unix)]
#[gpui_kit::test]
fn wait_for_a_pattern_with_lines_answers_from_what_is_there(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    prints(&ws, cx, theirs, b"Local: http://localhost:5173\r");
    screen_until(&ws, cx, theirs, |shown| shown.contains("localhost:5173"));
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "wait_for",
        json!({ "block": them, "pattern": "Local:", "lines": 20 }),
    );
    assert!(
        !failed && text.contains("Local: http://localhost:5173"),
        "{text}"
    );
    assert_eq!(watching(&ws, cx, theirs), 0);
    let bad = call(
        handle,
        &ws,
        cx,
        &terminal,
        "wait_for",
        json!({ "block": them, "pattern": "(" }),
    );
    assert!(
        bad.0
            && bad
                .1
                .starts_with("\"pattern\" isn't a regular expression: "),
        "{bad:?}"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_pattern_wait_leaves_no_watch_behind(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let never = |timeout: u64| json!({ "block": them, "pattern": "never", "timeout": timeout });
    // Its timeout.
    let answers = call_later(handle, &ws, cx, &terminal, 8, "wait_for", never(1));
    assert_eq!(watching(&ws, cx, theirs), 1);
    after(cx, 1_100);
    let (failed, text) = answered(&answers);
    assert!(
        failed
            && text.starts_with(&format!(
                "No line matching \"never\" in {them} within 1 seconds. Its last lines:\n"
            )),
        "{text}"
    );
    assert_eq!(watching(&ws, cx, theirs), 0);
    // Its connection's end.
    let _answers = call_later(handle, &ws, cx, &terminal, 9, "wait_for", never(60));
    assert_eq!(watching(&ws, cx, theirs), 1);
    bus_note(handle, &ws, cx, 9, templum_mcp::bus::CLOSED, json!({}));
    assert_eq!(watching(&ws, cx, theirs), 0);
    // Its cancel.
    let _answers = call_later(handle, &ws, cx, &terminal, 10, "wait_for", never(60));
    assert_eq!(watching(&ws, cx, theirs), 1);
    bus_note(
        handle,
        &ws,
        cx,
        10,
        "notifications/cancelled",
        json!({ "requestId": 10 }),
    );
    assert_eq!(watching(&ws, cx, theirs), 0);
}

/// `cat` in block `id` prints `text` back, and the test waits until its screen shows it.
#[cfg(unix)]
fn shows(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId, text: &str) {
    let line = format!("{text}\r").into_bytes();
    ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.write(line));
    screen_with(ws, cx, id, text);
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_screen_at_work_says_working_and_one_gone_still_ends_its_task(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let state = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(theirs).unwrap().state.clone())
    };
    // The person types: their keys change the screen, and tell nothing.
    for i in 0..4 {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(theirs).unwrap().typed.set(true)
        });
        shows(&ws, cx, theirs, &format!("typed {i}"));
        after(cx, 1000);
    }
    assert_eq!(state(&ws, cx), None);
    // Its own output, a second apart: at work.
    for i in 0..4 {
        shows(&ws, cx, theirs, &format!("tick {i}"));
        after(cx, 1000);
    }
    assert_eq!(state(&ws, cx), Some(AgentState::Working));
    let idle = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "wait_for_idle",
        json!({ "block": them }),
    );
    assert!(idle.try_recv().is_err(), "it waits while the agent works");
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert_eq!(state(&ws, cx), Some(AgentState::Idle));
    let (failed, text) = answered(&idle);
    assert!(
        !failed
            && text.starts_with("Its screen has been still for 5 seconds")
            && text.contains("tick 3"),
        "{text}"
    );
}

/// Their agent's screen shows a tick a second apart, `n` times: at work.
#[cfg(unix)]
fn ticks(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId, from: usize, n: usize) {
    for i in from..from + n {
        shows(ws, cx, id, &format!("tick {i}"));
        after(cx, 1000);
    }
}

/// Their agent has Templum's tools and no hooks: its screen going still doesn't end the task it is
/// on. The master waiting for it hears so as an error, and the task stays open for its reply
/// (docs/specs/agent-protocol.md, 5).
#[cfg(unix)]
#[gpui_kit::test]
fn a_quiet_agent_keeps_its_task_and_its_waiting_master_hears_so(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    has_server(handle, &ws, cx, &worker, 7);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    ticks(&ws, cx, theirs, 0, 4);
    assert!(
        delegated.try_recv().is_err(),
        "it waits while the agent works"
    );
    for _ in 0..5 {
        after(cx, 1000);
    }
    let (failed, text) = answered(&delegated);
    assert!(
        failed
            && text.starts_with(&format!(
                "{them} hasn't answered task t1: its screen has been still for 5 seconds."
            ))
            && text.contains("delegate to it again asks it for its answer, and interrupt ends it.")
            && text.contains("tick 3"),
        "{text}"
    );
    let handed = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space
                .terminal(theirs)
                .unwrap()
                .handed
                .as_ref()
                .map(|h| h.id)
        })
    };
    assert_eq!(handed(&ws, cx), Some(1), "the task stays open");
    // wait_for_idle waits for its reply, and gets it.
    let idle = json!({ "block": them });
    let idle = call_later(handle, &ws, cx, &terminal, 2, "wait_for_idle", idle);
    after(cx, 1000);
    assert!(idle.try_recv().is_err(), "it waits for the reply");
    let reply = json!({ "text": "Line 42." });
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply);
    assert_eq!((failed, text.as_str()), (false, "Answered task t1."));
    assert_eq!(answered(&idle), (false, "Line 42.".to_string()));
    assert_eq!(handed(&ws, cx), None);
}

/// The quiet error says `interrupt` ends the task, and it does, with the agent idle.
#[cfg(unix)]
#[gpui_kit::test]
fn interrupt_ends_a_quiet_agents_task(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    has_server(handle, &ws, cx, &worker, 7);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    ticks(&ws, cx, theirs, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert!(answered(&delegated).0, "told it went quiet");
    assert_eq!(task_of(&ws, cx, theirs), Some(1));
    let stop = json!({ "block": them });
    let stopped = call_later(handle, &ws, cx, &terminal, 2, "interrupt", stop);
    let (failed, text) = heard(cx, &stopped);
    assert!(
        !failed
            && text.starts_with(&format!(
                "Pressed Esc in {them}, and ended the task you handed it."
            )),
        "{text}"
    );
    assert_eq!(task_of(&ws, cx, theirs), None);
}

/// A master with no call waiting is told once, as a dialog is; the reply then ends the task once,
/// and a later still screen tells nothing (docs/specs/agent-protocol.md, 5).
#[cfg(unix)]
#[gpui_kit::test]
fn a_quiet_agent_is_told_once_to_a_master_not_waiting(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    has_server(handle, &ws, cx, &worker, 7);
    call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "delegate",
        json!({ "block": them, "task": "go" }),
    );
    ticks(&ws, cx, theirs, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    // Typed into mine at once.
    assert!(notices(&ws, cx).is_empty(), "{:?}", notices(&ws, cx));
    history_with(
        &ws,
        cx,
        mine,
        "hasn't answered: its screen is still] Its screen is in the pasted text.",
    );
    // At work again and still again: no second notice, typed or waiting in the queue.
    let quiet = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let shown = ws.space.terminal(mine).unwrap().screen().replace('\n', "");
            shown
                .matches("hasn't answered: its screen is still]")
                .count()
        })
    };
    // `cat` shows it twice: as it is typed, and printed once its Enter went.
    entered(&ws, cx, mine);
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while quiet(&ws, cx) < 2 {
        assert!(Instant::now() < deadline, "the first one was typed");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let typed = quiet(&ws, cx);
    ticks(&ws, cx, theirs, 4, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert_eq!(quiet(&ws, cx), typed, "told once");
    assert!(notices(&ws, cx).is_empty(), "{:?}", notices(&ws, cx));
    // Its reply ends the task: one notice, the end.
    let reply = json!({ "text": "Line 42." });
    let (failed, text) = call(handle, &ws, cx, &worker, "reply", reply);
    assert_eq!((failed, text.as_str()), (false, "Answered task t1."));
    after(cx, 1000);
    let told = notices(&ws, cx);
    assert!(
        told.len() == 1
            && told[0].0 == terminal
            && told[0].1.starts_with("[task t1 from ")
            && told[0].1.contains(": done in ")
            && told[0].1.ends_with("Line 42."),
        "{told:?}"
    );
    // Nothing after the end.
    ticks(&ws, cx, theirs, 8, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert_eq!(notices(&ws, cx).len(), 1, "{:?}", notices(&ws, cx));
}

/// The master changes the task: a quiet agent that goes still again is told once more.
#[cfg(unix)]
#[gpui_kit::test]
fn a_change_starts_quiet_over(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    asker(&ws, cx, mine);
    has_server(handle, &ws, cx, &worker, 7);
    let task = json!({ "block": them, "task": "go" });
    call_later(handle, &ws, cx, &terminal, 1, "delegate", task.clone());
    ticks(&ws, cx, theirs, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    history_with(&ws, cx, mine, "hasn't answered: its screen is still]");
    assert!(notices(&ws, cx).is_empty(), "{:?}", notices(&ws, cx));
    // A change to t1.
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, false);
    assert_eq!(
        (failed, text),
        (false, format!("Typed into {them} as a change to task t1."))
    );
    ticks(&ws, cx, theirs, 4, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    let told = notices(&ws, cx);
    assert!(
        told.len() == 1 && told[0].1.contains("hasn't answered: its screen is still]"),
        "{told:?}"
    );
}

/// Its server gone (the bridge died): it has no Templum tools again, and its screen ends the task
/// as that of any agent without them.
#[cfg(unix)]
#[gpui_kit::test]
fn a_quiet_agent_without_tools_ends_its_task_by_its_screen(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    has_server(handle, &ws, cx, &worker, 7);
    server_gone(handle, &ws, cx, &worker, 7);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    ticks(&ws, cx, theirs, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    let (failed, text) = answered(&delegated);
    assert!(
        !failed && text.starts_with("Its screen has been still for 5 seconds"),
        "{text}"
    );
}

/// A subagent that waits on a task of its own (it handed one to a third) says nothing by a still
/// screen either (docs/specs/agent-protocol.md, 5).
#[cfg(unix)]
#[gpui_kit::test]
fn a_quiet_agent_waiting_on_its_own_task_says_nothing(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (_, other) = me(&ws, cx, third);
    has_server(handle, &ws, cx, &worker, 7);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    call_later(
        handle,
        &ws,
        cx,
        &worker,
        1,
        "delegate",
        json!({ "block": other, "task": "look" }),
    );
    ticks(&ws, cx, theirs, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert!(delegated.try_recv().is_err(), "no answer");
    assert!(notices(&ws, cx).is_empty(), "{:?}", notices(&ws, cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(theirs).unwrap().handed.is_some())
    });
}

/// The id of the task handed to terminal block `id`'s agent.
#[cfg(unix)]
fn task_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Option<u64> {
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(id).unwrap().handed.as_ref().map(|h| h.id)
    })
}

/// Whether the master of the task handed to terminal block `id`'s agent was told it went quiet.
#[cfg(unix)]
fn told_quiet(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Option<bool> {
    ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(id)
            .unwrap()
            .handed
            .as_ref()
            .map(|h| h.quiet)
    })
}

/// A → B → C: C (Templum's tools, no hooks) went quiet on the task B handed it, and B was told. It
/// stays on it, but B no longer waits for it: B's turn's end answers B's own task, which A hears.
/// A new `delegate` of B to C starts the wait over (docs/specs/agent-protocol.md, 4).
#[cfg(unix)]
#[gpui_kit::test]
fn a_sub_task_that_went_quiet_doesnt_keep_the_subagents_task_open(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (helper, other) = me(&ws, cx, third);
    has_server(handle, &ws, cx, &helper, 7);
    // A hands B t1, which takes it and hands C t2.
    let task = json!({ "block": them, "task": "go" });
    call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "go");
    entered(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let task = json!({ "block": other, "task": "look" });
    call_later(handle, &ws, cx, &worker, 1, "delegate", task);
    screen_with(&ws, cx, third, "look");
    entered(&ws, cx, third);
    // C goes still without a reply: B is told.
    ticks(&ws, cx, third, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert_eq!(told_quiet(&ws, cx, third), Some(true));
    // B hands C the task again: it waits for it again, and its turn's end answers nothing.
    let again = json!({ "block": other, "task": "again" });
    let (failed, text) = hand_over(handle, &ws, cx, (&worker, third), again, false);
    assert_eq!(
        (failed, text),
        (false, format!("Typed into {other} as a change to task t2."))
    );
    assert_eq!(told_quiet(&ws, cx, third), Some(false));
    let waiting = json!({ "event": "stopped", "message": "Waiting for the third." });
    event(handle, &ws, cx, &worker, waiting);
    after(cx, 1000);
    assert_eq!(task_of(&ws, cx, theirs), Some(1));
    // C goes still again and B is told again: now B's turn's end is its answer.
    ticks(&ws, cx, third, 4, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert_eq!(told_quiet(&ws, cx, third), Some(true));
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let line = json!({ "event": "stopped", "message": "Line 42." });
    event(handle, &ws, cx, &worker, line);
    after(cx, 1000);
    assert_eq!(task_of(&ws, cx, theirs), None);
    assert_eq!(task_of(&ws, cx, third), Some(2), "C is still on its task");
    let shown = screen_with(&ws, cx, mine, "Line 42.");
    assert!(
        shown.contains("[task t1 from ") && shown.contains(": done in "),
        "{shown}"
    );
}

/// A → B → C: B (hooks) took A's task and handed C (Templum's tools, no hooks) one; C went still
/// on a dialog and B was told.
#[cfg(unix)]
struct Quiet {
    handle: AnyWindowHandle,
    ws: Entity<Workspace>,
    mine: BlockId,
    theirs: BlockId,
    third: BlockId,
    /// B's terminal, and C's id as B names it.
    worker: String,
    other: String,
}

#[cfg(unix)]
fn quiet_subagent(cx: &mut TestAppContext) -> Quiet {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (helper, other) = me(&ws, cx, third);
    has_server(handle, &ws, cx, &helper, 7);
    // A hands B t1, which takes it and hands C t2; C goes still on a dialog: B is told.
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    call_later(
        handle,
        &ws,
        cx,
        &worker,
        1,
        "delegate",
        json!({ "block": other, "task": "look" }),
    );
    screen_with(&ws, cx, third, "look");
    entered(&ws, cx, third);
    ticks(&ws, cx, third, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert_eq!(told_quiet(&ws, cx, third), Some(true));
    Quiet {
        handle,
        ws,
        mine,
        theirs,
        third,
        worker,
        other,
    }
}

#[cfg(unix)]
impl Quiet {
    /// B wrote into C (its dialog answered): C waits for its task again, and B's turn ends
    /// ("waiting for C") without ending B's own task.
    fn master_waits_again(&self, cx: &mut TestAppContext) {
        use crate::workspace::AgentState;
        assert_eq!(told_quiet(&self.ws, cx, self.third), Some(false));
        assert_eq!(
            state_of(&self.ws, cx, self.third),
            Some(AgentState::Working)
        );
        let waiting =
            json!({ "event": "stopped", "message": "Answered C's dialog; waiting for C." });
        event(self.handle, &self.ws, cx, &self.worker, waiting);
        after(cx, 1000);
        assert_eq!(task_of(&self.ws, cx, self.theirs), Some(1));
    }
}

/// A → B → C: C went quiet on a dialog and B was told. B answers it with `send_keys`, and its turn
/// ends ("waiting for C"): the keys of the asker start C's wait over, so B's task stays open, and
/// a C whose screen stays still after them tells B again (docs/specs/agent-protocol.md, 5).
#[cfg(unix)]
#[gpui_kit::test]
fn the_masters_keys_start_a_quiet_agents_wait_over(cx: &mut TestAppContext) {
    let q = quiet_subagent(cx);
    let (handle, ws, theirs, third) = (q.handle, &q.ws, q.theirs, q.third);
    // B answers the dialog.
    let keys = json!({ "block": q.other, "keys": ["2"] });
    let answers = call_later(handle, ws, cx, &q.worker, 8, "send_keys", keys);
    assert!(!heard(cx, &answers).0);
    // Its turn ends: it waits for C again, so its own task stays.
    q.master_waits_again(cx);
    for _ in 0..2 {
        after(cx, 1000);
    }
    assert_eq!(
        told_quiet(ws, cx, third),
        Some(false),
        "not still for 5 looks yet"
    );
    // C's screen stays still after the keys: B is told again, and its next turn's end answers.
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while told_quiet(ws, cx, third) != Some(true) {
        assert!(Instant::now() < deadline, "told again");
        after(cx, 1000);
    }
    event(handle, ws, cx, &q.worker, json!({ "event": "submitted" }));
    let line = json!({ "event": "stopped", "message": "Line 42." });
    event(handle, ws, cx, &q.worker, line);
    after(cx, 1000);
    assert_eq!(task_of(ws, cx, theirs), None);
    assert_eq!(task_of(ws, cx, third), Some(2), "C is still on its task");
    let shown = screen_with(ws, cx, q.mine, "Line 42.");
    assert!(shown.contains("[task t1 from "), "{shown}");
}

/// The same with `send_to_terminal`: the master's "y" and Enter answer C's dialog as its keys do.
#[cfg(unix)]
#[gpui_kit::test]
fn the_masters_typed_text_starts_a_quiet_agents_wait_over(cx: &mut TestAppContext) {
    let q = quiet_subagent(cx);
    let typed = json!({ "block": q.other, "text": "y", "enter": true });
    let answers = call_later(q.handle, &q.ws, cx, &q.worker, 8, "send_to_terminal", typed);
    assert!(!heard(cx, &answers).0);
    q.master_waits_again(cx);
}

/// The same with `reply` to C: the master's message, typed into C, answers its dialog.
#[cfg(unix)]
#[gpui_kit::test]
fn the_masters_reply_to_a_quiet_agent_starts_its_wait_over(cx: &mut TestAppContext) {
    let q = quiet_subagent(cx);
    let said = json!({ "block": q.other, "text": "Take the second one." });
    let (failed, text) = call(q.handle, &q.ws, cx, &q.worker, "reply", said);
    assert!(!failed && text.starts_with("Typed into"), "{text}");
    entered(&q.ws, cx, q.third);
    q.master_waits_again(cx);
}

/// Another agent's `wait_for_idle` on a subagent whose turn already ended while it waits on a task
/// of its own is answered now, with the turn's message; its master's own wait stays
/// (docs/specs/agent-protocol.md, 4).
#[cfg(unix)]
#[gpui_kit::test]
fn waiting_for_a_subagent_that_waits_on_its_own_task_ended_its_turn(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (helper, other) = me(&ws, cx, third);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "go");
    entered(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    call_later(
        handle,
        &ws,
        cx,
        &worker,
        1,
        "delegate",
        json!({ "block": other, "task": "look" }),
    );
    let waiting = json!({ "event": "stopped", "message": "Waiting for the third." });
    event(handle, &ws, cx, &worker, waiting);
    after(cx, 1000);
    // C asks after the turn ended; A, its master, waits for the task.
    let idle = json!({ "block": them });
    let (failed, text) = call(handle, &ws, cx, &helper, "wait_for_idle", idle.clone());
    assert_eq!((failed, text.as_str()), (false, "Waiting for the third."));
    let masters = call_later(handle, &ws, cx, &terminal, 2, "wait_for_idle", idle);
    after(cx, 1000);
    assert!(masters.try_recv().is_err(), "its master's wait stays");
    assert!(delegated.try_recv().is_err());
    assert_eq!(task_of(&ws, cx, theirs), Some(1));
}

/// A subagent without hooks keeps the answer of its last task: another agent waiting for its turn,
/// which ended while it waits on a task of its own, gets its still screen, not that answer
/// (docs/specs/agent-protocol.md, 4).
#[cfg(unix)]
#[gpui_kit::test]
fn waiting_for_a_subagent_without_hooks_isnt_answered_with_an_earlier_task(
    cx: &mut TestAppContext,
) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (helper, other) = me(&ws, cx, third);
    has_server(handle, &ws, cx, &worker, 7);
    // B (tools, no hooks) answers t1 with `reply`, takes t2, hands C a sub-task and goes still.
    let task = json!({ "block": them, "task": "one", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "one");
    entered(&ws, cx, theirs);
    let answer = json!({ "text": "Answer to t1." });
    call(handle, &ws, cx, &worker, "reply", answer);
    assert_eq!(answered(&delegated), (false, "Answer to t1.".into()));
    let next = json!({ "block": them, "task": "two" });
    call_later(handle, &ws, cx, &terminal, 1, "delegate", next);
    screen_with(&ws, cx, theirs, "two");
    entered(&ws, cx, theirs);
    let sub = json!({ "block": other, "task": "look" });
    call_later(handle, &ws, cx, &worker, 1, "delegate", sub);
    ticks(&ws, cx, theirs, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert_eq!(task_of(&ws, cx, theirs), Some(2));
    let idle = json!({ "block": them });
    let (failed, text) = call(handle, &ws, cx, &helper, "wait_for_idle", idle);
    assert!(
        !failed && text.starts_with("Its screen has been still for 5 seconds"),
        "{text}"
    );
}

/// The same when it went quiet (no hooks): the others get its still screen at once.
#[cfg(unix)]
#[gpui_kit::test]
fn waiting_for_a_quiet_subagent_ended_its_turn(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (helper, _) = me(&ws, cx, third);
    has_server(handle, &ws, cx, &worker, 7);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    ticks(&ws, cx, theirs, 0, 4);
    for _ in 0..5 {
        after(cx, 1000);
    }
    assert_eq!(told_quiet(&ws, cx, theirs), Some(true));
    assert!(answered(&delegated).0, "its master was told");
    let idle = json!({ "block": them });
    let (failed, text) = call(handle, &ws, cx, &helper, "wait_for_idle", idle);
    assert!(
        !failed
            && text.starts_with("Its screen has been still for 5 seconds")
            && text.contains("tick 3"),
        "{text}"
    );
    assert_eq!(task_of(&ws, cx, theirs), Some(1));
}

/// A subagent that waits on a task of its own and whose turn ends keeps its task, but the other
/// agents waiting for its turn (`wait_for_idle`) hear the turn ended, as they do when it goes
/// quiet; its master's own wait stays (docs/specs/agent-protocol.md, 4).
#[cfg(unix)]
#[gpui_kit::test]
fn a_subagent_waiting_on_its_own_task_answers_the_others_waiting_for_its_turn(
    cx: &mut TestAppContext,
) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (helper, other) = me(&ws, cx, third);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "go");
    entered(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let task = json!({ "block": other, "task": "look" });
    call_later(handle, &ws, cx, &worker, 1, "delegate", task);
    // C waits for B's turn, as A does.
    let idle = json!({ "block": them });
    let idle = call_later(handle, &ws, cx, &helper, 8, "wait_for_idle", idle);
    assert!(idle.try_recv().is_err());
    let waiting = json!({ "event": "stopped", "message": "Waiting for the third." });
    event(handle, &ws, cx, &worker, waiting);
    after(cx, 1000);
    assert_eq!(
        answered(&idle),
        (false, "Waiting for the third.".to_string())
    );
    assert!(delegated.try_recv().is_err(), "its master's wait stays");
    assert_eq!(task_of(&ws, cx, theirs), Some(1));
}

/// The same when its screen goes still instead (it has no hooks).
#[cfg(unix)]
#[gpui_kit::test]
fn a_subagent_waiting_on_its_own_task_answers_the_others_by_its_still_screen(
    cx: &mut TestAppContext,
) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let third = another_agent(handle, &ws, cx, theirs);
    asker(&ws, cx, mine);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (helper, other) = me(&ws, cx, third);
    has_server(handle, &ws, cx, &worker, 7);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let task = json!({ "block": other, "task": "look" });
    call_later(handle, &ws, cx, &worker, 1, "delegate", task);
    ticks(&ws, cx, theirs, 0, 4);
    let idle = json!({ "block": them });
    let idle = call_later(handle, &ws, cx, &helper, 8, "wait_for_idle", idle);
    assert!(idle.try_recv().is_err());
    for _ in 0..5 {
        after(cx, 1000);
    }
    let (failed, text) = answered(&idle);
    assert!(
        !failed && text.starts_with("Its screen has been still for 5 seconds"),
        "{text}"
    );
    assert!(delegated.try_recv().is_err(), "its master's wait stays");
    assert_eq!(task_of(&ws, cx, theirs), Some(1));
}

/// The state of terminal block `id`'s agent.
fn state_of(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
) -> Option<crate::workspace::AgentState> {
    ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().state.clone())
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_turns_end_right_after_its_spinner_stays_idle(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    ticks(&ws, cx, theirs, 0, 4);
    assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Working));
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "All done." }),
    );
    for _ in 0..2 {
        after(cx, 1000);
        assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Idle));
    }
    let answer = call(
        handle,
        &ws,
        cx,
        &terminal,
        "wait_for_idle",
        json!({ "block": them }),
    );
    assert_eq!(answer, (false, "All done.".to_string()), "at once");
}

/// A dialog sat still for longer than a screen takes to say "at rest"; the person allowed the
/// Templum tool it asked for, which now runs: its agent works, and the stillness before says
/// nothing of it.
#[gpui_kit::test]
fn a_tool_allowed_after_a_still_dialog_works(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    let asks = json!({ "event": "needs_you", "message": "Allow mcp__templum__whoami?" });
    event(handle, &ws, cx, &worker, asks);
    for _ in 0..6 {
        after(cx, 1000);
    }
    assert!(matches!(
        state_of(&ws, cx, theirs),
        Some(AgentState::NeedsYou(_))
    ));
    call_later(handle, &ws, cx, &worker, 1, "whoami", json!({}));
    assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Working));
    after(cx, 1000);
    assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Working));
}

/// A dialog opens while the agent works: its dot stays yellow, and a task waiting on its turn
/// waits on, while the dialog sits still and while a subagent draws under it; once the person
/// typed into it (they answered), the screen at work says Working again.
#[cfg(unix)]
#[gpui_kit::test]
fn a_dialog_stays_yellow_until_its_person_answers(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    ticks(&ws, cx, theirs, 0, 4);
    assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Working));
    let idle = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "wait_for_idle",
        json!({ "block": them }),
    );
    // The person was typing as the dialog opened: those keys answer nothing.
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(theirs).unwrap().typed.set(true)
    });
    let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
    event(handle, &ws, cx, &worker, asks);
    let asking = Some(AgentState::NeedsYou(Some("Allow Bash?".into())));
    for _ in 0..6 {
        after(cx, 1000);
    }
    assert_eq!(state_of(&ws, cx, theirs), asking, "still");
    ticks(&ws, cx, theirs, 4, 6);
    assert_eq!(state_of(&ws, cx, theirs), asking, "a subagent draws");
    assert!(idle.try_recv().is_err(), "it still waits");
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(theirs).unwrap().typed.set(true)
    });
    ticks(&ws, cx, theirs, 10, 4);
    assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Working));
}

/// A resize redraws an agent at its new size: its looks don't take that for work (which, for one
/// whose hooks speak, would "seem stuck" 30 s later).
#[cfg(unix)]
#[gpui_kit::test]
fn resizing_an_idle_agent_isnt_work(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "started" }));
    for (i, width) in [400.0, 700.0, 400.0, 700.0].into_iter().enumerate() {
        ws.update(cx, |ws, _| {
            ws.fit_terminal(theirs, Rect::new(900.0, 100.0, width, 300.0))
        });
        // Its agent redraws at the new size.
        shows(&ws, cx, theirs, &format!("frame {i}"));
        after(cx, 1000);
    }
    assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Idle));
}

/// Its hooks spoke, and its screen went still while it worked (a dialog no hook tells of, a hang):
/// it seems stuck, waits for its person, keeps its task, and works again once its screen moves.
#[cfg(unix)]
#[gpui_kit::test]
fn a_hooked_agent_gone_still_keeps_its_task_and_says_so(cx: &mut TestAppContext) {
    use crate::workspace::{AgentState, activity::STUCK_TEXT};
    let (handle, ws, mine, theirs) = two_agents(cx);
    // A third agent waits for it to need its person: its asker hears so as a notice.
    let third = another_agent(handle, &ws, cx, theirs);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let (other, _) = me(&ws, cx, third);
    // Mine asks its person: the notice waits, to be read here.
    asker(&ws, cx, mine);
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "needs_you", "message": "Allow Edit?" }),
    );
    let task = json!({ "block": them, "task": "think" });
    let delegated = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "standard input.)");
    entered(&ws, cx, theirs);
    // Its screen settled before the looks start counting.
    screen_until(&ws, cx, theirs, |s| s.matches("think").count() >= 2);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let asks = json!({ "block": them, "event": "needs_you" });
    let needs_you = call_later(handle, &ws, cx, &other, 2, "wait_for", asks);
    // The first look sees the echo drawn before the hook: the screen is still from there.
    after(cx, 1000);
    let (failed, text) = answered(&delegated);
    assert!(
        !failed && text.starts_with(&format!("Handed to {them} as task t1.")),
        "{text}"
    );
    for _ in 0..29 {
        after(cx, 1000);
    }
    assert_eq!(
        state_of(&ws, cx, theirs),
        Some(AgentState::Working),
        "29 still looks"
    );
    after(cx, 1000);
    after(cx, 300);
    let stuck = Some(AgentState::NeedsYou(Some(STUCK_TEXT.into())));
    assert_eq!(state_of(&ws, cx, theirs), stuck);
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(theirs).unwrap();
        assert!(pane.handed.is_some(), "its task stays handed over");
        // Another wait: an approval written before it is asked again.
        assert_eq!(pane.dialogs, 1);
    });
    let (failed, text) = answered(&needs_you);
    assert!(!failed && text.contains("it seems stuck"), "{text}");
    let told = notices(&ws, cx);
    assert!(
        told.len() == 1
            && told[0].1.contains(&format!(
                "({them}) waits for its person: it seems stuck, its screen still for 30 seconds]"
            )),
        "{told:?}"
    );
    ticks(&ws, cx, theirs, 0, 4);
    assert_eq!(state_of(&ws, cx, theirs), Some(AgentState::Working));
}

/// The person scrolls an agent's screen, or its program hears their mouse: the screen changes,
/// and tells nothing. Nor does the mouse answer a dialog: it may only pass over a TUI that hears
/// every move.
#[cfg(unix)]
#[gpui_kit::test]
fn the_wheel_and_the_mouse_tell_nothing(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    let scroll = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            // Its left: its middle is past the window's right edge.
            let body = window.find(("body", theirs.0 as usize)).bounds().origin;
            let at = (f32::from(body.x) + 20., f32::from(body.y) + 20.);
            wheel(window, cx, at, ScrollDelta::Lines(point(0., -1.)), false);
        })
        .unwrap();
    };
    for i in 0..4 {
        scroll(cx);
        shows(&ws, cx, theirs, &format!("scrolled {i}"));
        after(cx, 1000);
    }
    assert_eq!(state_of(&ws, cx, theirs), None, "its history scrolls");
    prints(&ws, cx, theirs, b"\x1b[?1002h\x1b[?1006h\r");
    super::terminals::until("the mouse", handle, &ws, cx, |ws| {
        ws.space
            .terminal(theirs)
            .unwrap()
            .term
            .shared()
            .modes()
            .mouse
    });
    for i in 0..4 {
        scroll(cx);
        shows(&ws, cx, theirs, &format!("heard {i}"));
        after(cx, 1000);
    }
    assert_eq!(state_of(&ws, cx, theirs), None, "its program hears it");
    let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
    event(handle, &ws, cx, &worker, asks);
    scroll(cx);
    ticks(&ws, cx, theirs, 0, 4);
    assert!(
        matches!(state_of(&ws, cx, theirs), Some(AgentState::NeedsYou(_))),
        "the mouse answers no dialog"
    );
}

/// An agent whose hooks speak (Claude Code, Gemini CLI) says it took a task: its turn starts. With
/// no word from it, Templum presses Enter once more (the first may have joined the paste, as a
/// new line); still none, the task ends untaken, and the agent that handed it over hears so.
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_no_hook_confirms_gets_its_enter_again_then_ends(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "started" }));
    let task = json!({ "block": them, "task": "think", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "think");
    entered(&ws, cx, theirs);
    screen_until(&ws, cx, theirs, |s| s.matches("think").count() >= 2);
    let confirm = crate::workspace::delegate::CONFIRM.as_millis() as u64;
    after(cx, confirm);
    assert!(answers.try_recv().is_err(), "it gets its Enter again first");
    after(cx, confirm);
    let (failed, text) = answered(&answers);
    assert!(
        failed && text.contains("didn't take the task") && text.contains("think"),
        "{text}"
    );
    // Its input may still hold the task: typed again, it would go in twice.
    assert!(text.contains("clear its input (send_keys)"), "{text}");
}

/// The person types into the agent before its task's Enter: the Enter would send their half
/// line with the task. None goes, and the asker hears why.
#[cfg(unix)]
#[gpui_kit::test]
fn no_enter_once_the_person_typed_into_the_agent(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let inputs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(theirs).unwrap().term.inputs())
    };
    let task = json!({ "block": them, "task": "think" });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let typed = inputs(cx);
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(theirs).unwrap().person_typed()
    });
    after(
        cx,
        crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
    );
    let (failed, text) = answered(&answers);
    assert!(
        failed && text.contains("its person is typing in it"),
        "{text}"
    );
    assert_eq!(inputs(cx), typed, "no Enter");
}

/// A task's looks after its Enter stop once it ended and another came after it: they press no
/// Enter into the later task, nor end it as not taken.
#[cfg(unix)]
#[gpui_kit::test]
fn an_earlier_tasks_looks_leave_a_later_task_alone(cx: &mut TestAppContext) {
    let confirm = crate::workspace::delegate::CONFIRM.as_millis() as u64;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "started" }));
    let inputs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(theirs).unwrap().term.inputs())
    };
    let task = json!({ "block": them, "task": "think" });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "think");
    entered(&ws, cx, theirs);
    screen_until(&ws, cx, theirs, |s| s.matches("think").count() >= 2);
    // Task t1 ended, and task t2 came after it.
    ws.update(cx, |ws, _| {
        ws.space
            .terminal_mut(theirs)
            .unwrap()
            .handed
            .as_mut()
            .unwrap()
            .id = 2
    });
    let typed = inputs(cx);
    after(cx, 2 * confirm);
    assert_eq!(inputs(cx), typed, "no Enter into task t2");
    ws.read_with(cx, |ws, _| {
        let handed = ws.space.terminal(theirs).unwrap().handed.as_ref();
        assert_eq!(handed.map(|h| h.id), Some(2), "task t2 still handed over");
    });
    let (failed, text) = answered(&answers);
    assert!(failed && text.contains("before it took the task"), "{text}");
}

/// A turn's end (Claude Code's idle prompt comes a minute after its last turn) and a screen its
/// looks take for one at work confirm nothing: only the prompt taken does, and its turn's end then
/// answers the task.
#[cfg(unix)]
#[gpui_kit::test]
fn only_its_prompt_taken_confirms_a_task(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let confirm = crate::workspace::delegate::CONFIRM.as_millis() as u64;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "started" }));
    let task = json!({ "block": them, "task": "think", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    screen_with(&ws, cx, theirs, "think");
    entered(&ws, cx, theirs);
    screen_until(&ws, cx, theirs, |s| s.matches("think").count() >= 2);
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": null }),
    );
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().state = Some(AgentState::Working)
    });
    after(cx, 2 * confirm);
    let (failed, text) = answered(&answers);
    assert!(failed && text.contains("didn't take the task"), "{text}");
    // Idle again, its prompt taken: confirmed, and its turn's end answers the task.
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": null }),
    );
    let task = json!({ "block": them, "task": "ponder", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 2, "delegate", task);
    screen_with(&ws, cx, theirs, "ponder");
    entered(&ws, cx, theirs);
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "submitted", "at_enter": true }),
    );
    after(cx, 2 * confirm);
    assert!(answers.try_recv().is_err(), "taken: it works on it");
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "Pondered." }),
    );
    assert_eq!(answered(&answers), (false, "Pondered.".to_string()));
}

/// No word 3 s after the Enter: a screen that changed since just after it drew something no hook
/// tells of (a trust question), and the person typing there would send their own half line with
/// a second Enter. Neither gets one: the task ends at that look, with the screen as it is.
#[cfg(unix)]
#[gpui_kit::test]
fn no_second_enter_into_a_changed_screen_or_while_the_person_types(cx: &mut TestAppContext) {
    let confirm = crate::workspace::delegate::CONFIRM.as_millis() as u64;
    let enter_after = crate::workspace::delegate::ENTER_AFTER.as_millis() as u64;
    for case in ["a question drew", "the person types"] {
        let (handle, ws, mine, theirs) = two_agents(cx);
        let (terminal, _) = me(&ws, cx, mine);
        let (worker, them) = me(&ws, cx, theirs);
        event(handle, &ws, cx, &worker, json!({ "event": "started" }));
        let task = json!({ "block": them, "task": "think", "wait": true });
        let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
        screen_with(&ws, cx, theirs, "think");
        entered(&ws, cx, theirs);
        screen_until(&ws, cx, theirs, |s| s.matches("think").count() >= 2);
        // The screen just after the Enter, which a second one would need unchanged.
        after(cx, enter_after);
        if case == "a question drew" {
            shows(&ws, cx, theirs, "Trust these hooks?");
        } else {
            ws.read_with(cx, |ws, _| {
                ws.space.terminal(theirs).unwrap().person_typed()
            });
        }
        after(cx, confirm);
        // Answered at the first look: a second Enter would have waited 3 s more.
        let (failed, text) = answered(&answers);
        assert!(
            failed && text.contains("didn't take the task"),
            "{case}: {text}"
        );
        if case == "a question drew" {
            assert!(
                text.contains("Trust these hooks?"),
                "the screen as it is: {text}"
            );
        }
    }
}

/// A hook after the Enter (its turn started) confirms it; an agent at work queued the task; one
/// whose hooks never said a turn started (Codex, a `claude` typed by hand) can't confirm it, and
/// Templum never presses Enter blindly into it. None of them ends the task untaken.
#[cfg(unix)]
#[gpui_kit::test]
fn a_task_confirmed_queued_or_unconfirmable_stays_handed(cx: &mut TestAppContext) {
    let confirm = crate::workspace::delegate::CONFIRM.as_millis() as u64;
    for before in ["none", "started", "submitted"] {
        let (handle, ws, mine, theirs) = two_agents(cx);
        let (terminal, _) = me(&ws, cx, mine);
        let (worker, them) = me(&ws, cx, theirs);
        if before != "none" {
            event(handle, &ws, cx, &worker, json!({ "event": before }));
        }
        let task = json!({ "block": them, "task": "think", "wait": true });
        let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
        screen_with(&ws, cx, theirs, "think");
        entered(&ws, cx, theirs);
        if before == "started" {
            // Its turn starts: `UserPromptSubmit`.
            event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
        }
        after(cx, 2 * confirm);
        // The screen gone still may end a working agent's task: never as untaken.
        if let Ok(line) = answers.try_recv() {
            let (_, text) = result_of(&line);
            assert!(!text.contains("didn't take"), "{before}: {text}");
        }
    }
}
