//! Claude Code's subagents (docs/specs/claude-subagents.md): their parent busy while they work, a
//! task answered by its real end, and a block of their own beside it. The hooks' inputs have the shapes Claude Code 2.1.286 sent
//! (docs/plans/2026-10-22-claude-subagents.md, The probe), turned into events as `templum hook
//! claude` does.

use serde_json::{Value, json};

use super::bus::{after, answered, call_later, event, me, screen_until};
#[cfg(unix)]
use super::delegate::hand_over;
use super::delegate::{entered, two_agents};
use super::*;

/// The Claude Code hook input `input`, from the agent in `terminal`, as `templum hook claude`
/// hands it to the workspace.
fn hook(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    terminal: &str,
    input: Value,
) {
    let said = templum_mcp::hooks::claude(&input).expect("an event");
    event(
        handle,
        ws,
        cx,
        terminal,
        serde_json::to_value(said).unwrap(),
    );
}

/// A subagent's start, its type `kind`.
fn started(id: &str, kind: &str) -> Value {
    json!({ "hook_event_name": "SubagentStart", "agent_id": id, "agent_type": kind,
        "transcript_path": "/p/s1.jsonl" })
}

/// `Stop`, its turn's answer `said`, while the subagents `ids` still work.
fn stop(said: &str, ids: &[&str]) -> Value {
    let tasks: Vec<Value> = ids
        .iter()
        .map(|id| json!({ "id": id, "type": "subagent", "status": "running" }))
        .collect();
    json!({ "hook_event_name": "Stop", "last_assistant_message": said,
        "background_tasks": tasks })
}

/// A background subagent's stop: it is still among the running as it stops, with `others`.
fn stopped(id: &str, others: &[&str]) -> Value {
    let tasks: Vec<Value> = std::iter::once(&id)
        .chain(others)
        .map(|id| json!({ "id": id, "type": "subagent", "status": "running" }))
        .collect();
    json!({ "hook_event_name": "SubagentStop", "agent_id": id, "agent_type": "Explore",
        "background_tasks": tasks })
}

/// The prompt Claude Code gives itself with a subagent's report.
fn report(id: &str) -> Value {
    let prompt = format!(
        "<task-notification>\n<task-id>{id}</task-id>\n<status>completed</status>\n\
         <summary>Agent finished</summary>\n</task-notification>"
    );
    json!({ "hook_event_name": "UserPromptSubmit", "prompt": prompt })
}

/// A tool's end in the subagent `id`, or in the agent itself.
fn tool(id: Option<&str>) -> Value {
    match id {
        Some(id) => json!({ "hook_event_name": "PostToolUse", "tool_name": "Read",
            "agent_id": id, "agent_type": "Explore" }),
        None => json!({ "hook_event_name": "PostToolUse", "tool_name": "Read" }),
    }
}

/// `delegate` with `wait` from `terminal` to block `theirs`, then Claude Code's `UserPromptSubmit`
/// of it once Templum typed it and pressed Enter: a hook heard before the task is typed doesn't
/// take it, and its screen (Windows' `cmd` runs it as a command) would say it wasn't taken.
fn handed(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    (terminal, theirs): (&str, BlockId),
    task: Value,
) -> std::sync::mpsc::Receiver<String> {
    let (worker, _) = me(ws, cx, theirs);
    let before = screen_until(ws, cx, theirs, |_| true);
    let answers = call_later(handle, ws, cx, terminal, 1, "delegate", task);
    screen_until(ws, cx, theirs, |shown| shown != before);
    entered(ws, cx, theirs);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "[from …] task" });
    hook(handle, ws, cx, &worker, prompt);
    answers
}

/// What the dot of terminal block `id` says.
fn dot(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Option<String> {
    ws.read_with(cx, |ws, _| {
        crate::block::status_dot(ws, ws.space.terminal(id).unwrap())
            .map(|(_, text)| text.to_string())
    })
}

/// A Claude Code whose turn ends while its background subagents work is still at work: the task
/// it was handed ends with the answer of the turn that takes the last report, not with the
/// interim message, also when a subagent stops during the turn that takes another's report and
/// that turn ends first (seen in the probe). Its prompt sitting idle meanwhile ends nothing.
#[gpui_kit::test]
fn a_task_ends_with_the_turn_after_its_subagents_reports(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "review the parser", "wait": true });
    let answers = handed(handle, &ws, cx, (&terminal, theirs), task);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, started("a2", "general-purpose"));
    hook(handle, &ws, cx, &worker, tool(Some("a1")));
    hook(
        handle,
        &ws,
        cx,
        &worker,
        stop("I've started two.", &["a1", "a2"]),
    );
    assert!(
        answers.try_recv().is_err(),
        "the interim message isn't its answer"
    );
    assert_eq!(
        dot(&ws, cx, theirs).as_deref(),
        Some("Working: 2 subagents")
    );
    let listed = ws.read_with(cx, |ws, _| {
        super::super::bus::agent_state(&ws.space, theirs)
    });
    assert_eq!(listed, ", working: 2 Claude Code subagents");
    let idle = json!({ "hook_event_name": "Notification", "notification_type": "idle_prompt" });
    hook(handle, &ws, cx, &worker, idle);
    assert!(
        answers.try_recv().is_err(),
        "an idle prompt while they work"
    );
    hook(handle, &ws, cx, &worker, stopped("a1", &["a2"]));
    hook(handle, &ws, cx, &worker, report("a1"));
    assert_eq!(dot(&ws, cx, theirs).as_deref(), Some("Working: 1 subagent"));
    // The other stops during the turn that takes the first's report, which ends before its own.
    hook(handle, &ws, cx, &worker, stopped("a2", &[]));
    hook(
        handle,
        &ws,
        cx,
        &worker,
        stop("Waiting for the other.", &[]),
    );
    assert!(answers.try_recv().is_err(), "a report is still to come");
    assert_eq!(dot(&ws, cx, theirs).as_deref(), Some("Working"));
    hook(handle, &ws, cx, &worker, report("a2"));
    after(cx, 6_000);
    if let Ok(early) = answers.try_recv() {
        panic!("the turn taking the report is under way, yet it answered: {early}");
    }
    hook(
        handle,
        &ws,
        cx,
        &worker,
        stop("Both found it: parse.rs:42.", &[]),
    );
    assert_eq!(
        answered(&answers),
        (false, "Both found it: parse.rs:42.".to_string())
    );
    assert_eq!(dot(&ws, cx, theirs), None, "at rest");
}

/// A report Claude Code took inside a turn (at a tool's end, as it does since 2.1.234) starts no
/// turn: the turn's end that leaves only reported subagents answers the task once no turn started
/// within the grace, or at once when its prompt sits idle.
#[gpui_kit::test]
fn a_report_taken_inside_a_turn_doesnt_keep_the_task_open(cx: &mut TestAppContext) {
    for idle in [false, true] {
        let (handle, ws, mine, theirs) = two_agents(cx);
        let (terminal, _) = me(&ws, cx, mine);
        let (worker, them) = me(&ws, cx, theirs);
        let task = json!({ "block": them, "task": "find it", "wait": true });
        let answers = handed(handle, &ws, cx, (&terminal, theirs), task);
        hook(handle, &ws, cx, &worker, started("a1", "Explore"));
        hook(handle, &ws, cx, &worker, tool(None));
        hook(handle, &ws, cx, &worker, stopped("a1", &[]));
        // Its report, taken at this tool's end.
        hook(handle, &ws, cx, &worker, tool(None));
        hook(handle, &ws, cx, &worker, stop("Found it", &[]));
        assert!(
            answers.try_recv().is_err(),
            "a report may still come as a turn"
        );
        if idle {
            let idle = json!({ "hook_event_name": "Notification",
                "notification_type": "idle_prompt" });
            hook(handle, &ws, cx, &worker, idle);
        } else {
            after(cx, 4_000);
            assert!(answers.try_recv().is_err(), "within the grace");
            after(cx, 1_500);
        }
        assert_eq!(
            answered(&answers),
            (false, "Found it".to_string()),
            "idle: {idle}"
        );
        assert_eq!(dot(&ws, cx, theirs), None, "at rest");
    }
}

/// A turn that ends without an answer (an Esc while a tool runs) with only reported subagents left
/// says so at once: not with an earlier turn's answer.
#[gpui_kit::test]
fn a_turn_ended_without_an_answer_says_so_with_reports_left(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "the person's" });
    hook(handle, &ws, cx, &worker, prompt);
    hook(handle, &ws, cx, &worker, stop("An earlier answer.", &[]));
    let task = json!({ "block": them, "task": "find it", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "[from …] find" });
    hook(handle, &ws, cx, &worker, prompt);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, stopped("a1", &[]));
    let esc = json!({ "hook_event_name": "PostToolUseFailure", "tool_name": "Bash",
        "is_interrupt": true });
    hook(handle, &ws, cx, &worker, esc);
    let (failed, text) = answered(&answers);
    assert!(
        failed && text.starts_with("Its turn ended without an answer"),
        "{text}"
    );
}

/// A subagent's stop Templum never heard start (a session started since, `/clear`) doesn't keep
/// its parent waiting for a report.
#[gpui_kit::test]
fn an_unknown_subagents_stop_awaits_no_report(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    hook(handle, &ws, cx, &worker, started("a9", "Explore"));
    let start = json!({ "hook_event_name": "SessionStart", "source": "clear" });
    hook(handle, &ws, cx, &worker, start);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "[from …] go" });
    hook(handle, &ws, cx, &worker, prompt);
    hook(handle, &ws, cx, &worker, stopped("a9", &[]));
    hook(handle, &ws, cx, &worker, stop("Done.", &[]));
    assert_eq!(answered(&answers), (false, "Done.".to_string()));
}

/// A task handed while subagents started before it still work is answered by its own turn's end
/// once only those are left: their reports aren't its own. One it started keeps it open.
#[gpui_kit::test]
fn a_task_isnt_held_by_subagents_started_before_it(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    hook(handle, &ws, cx, &worker, started("s1", "general-purpose"));
    hook(
        handle,
        &ws,
        cx,
        &worker,
        stop("Started a reviewer.", &["s1"]),
    );
    let task = json!({ "block": them, "task": "review the parser", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "[from …] review" });
    hook(handle, &ws, cx, &worker, prompt);
    hook(handle, &ws, cx, &worker, started("s2", "Explore"));
    hook(handle, &ws, cx, &worker, stop("Started s2.", &["s1", "s2"]));
    assert!(answers.try_recv().is_err(), "its own subagent works");
    hook(handle, &ws, cx, &worker, stopped("s2", &["s1"]));
    hook(handle, &ws, cx, &worker, report("s2"));
    hook(handle, &ws, cx, &worker, stop("Review: ok", &["s1"]));
    assert_eq!(answered(&answers), (false, "Review: ok".to_string()));
    assert_eq!(
        dot(&ws, cx, theirs).as_deref(),
        Some("Working: 1 subagent"),
        "it still awaits the earlier one"
    );
}

/// A subagent's report starts a turn of Claude Code's own: a task typed a moment before it isn't
/// taken by it (it waits for its own prompt's hook).
#[gpui_kit::test]
fn a_subagents_report_doesnt_take_a_task_in(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, stop("Started one.", &["a1"]));
    let task = json!({ "block": them, "task": "go", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    hook(handle, &ws, cx, &worker, stopped("a1", &[]));
    hook(handle, &ws, cx, &worker, report("a1"));
    let taken = ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(theirs)
            .unwrap()
            .handed
            .as_ref()
            .map(|h| h.taken)
    });
    assert_eq!(taken, Some(false));
    hook(handle, &ws, cx, &worker, stop("a1 found nothing.", &[]));
    assert!(
        answers.try_recv().is_err(),
        "the report's turn isn't the task's"
    );
}

/// A dialog a subagent opened in its parent's screen stays its parent's state until that
/// subagent's tool ends: another subagent's tool, or the parent's own, doesn't close it.
#[gpui_kit::test]
fn a_subagents_dialog_stays_until_its_own_tool_ends(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "go" });
    hook(handle, &ws, cx, &worker, prompt);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, started("a2", "Plan"));
    let asks = json!({ "hook_event_name": "PermissionRequest", "tool_name": "Bash",
        "agent_id": "a1", "agent_type": "Explore" });
    hook(handle, &ws, cx, &worker, asks);
    let waiting = Some("Waiting for you: Allow Bash?".to_string());
    assert_eq!(dot(&ws, cx, theirs), waiting);
    hook(handle, &ws, cx, &worker, tool(Some("a2")));
    assert_eq!(dot(&ws, cx, theirs), waiting, "another subagent's tool");
    hook(handle, &ws, cx, &worker, tool(None));
    assert_eq!(dot(&ws, cx, theirs), waiting, "its parent's own tool");
    hook(handle, &ws, cx, &worker, tool(Some("a1")));
    assert_eq!(
        dot(&ws, cx, theirs).as_deref(),
        Some("Working: 2 subagents")
    );
}

/// A dialog a subagent asked is over once that subagent leaves its parent's list without a word of
/// its own: a turn's end that no longer names it (stopped from Claude Code's panel), or its report
/// taken. Its parent doesn't stay waiting for its person.
#[gpui_kit::test]
fn a_gone_subagents_dialog_doesnt_keep_its_parent_waiting(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "go" });
    hook(handle, &ws, cx, &worker, prompt);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, started("a2", "Plan"));
    let asks = |id: &str| {
        json!({ "hook_event_name": "PermissionRequest", "tool_name": "Bash", "agent_id": id,
            "agent_type": "Explore" })
    };
    hook(handle, &ws, cx, &worker, asks("a1"));
    let waiting = Some("Waiting for you: Allow Bash?".to_string());
    assert_eq!(dot(&ws, cx, theirs), waiting);
    hook(handle, &ws, cx, &worker, stop("Started two.", &["a2"]));
    assert_eq!(
        dot(&ws, cx, theirs).as_deref(),
        Some("Working: 1 subagent"),
        "a1 is gone, and its dialog with it"
    );
    hook(handle, &ws, cx, &worker, asks("a2"));
    assert_eq!(dot(&ws, cx, theirs), waiting);
    hook(handle, &ws, cx, &worker, report("a2"));
    assert_eq!(
        dot(&ws, cx, theirs).as_deref(),
        Some("Working"),
        "its report taken"
    );
}

/// A subagent's tool end doesn't take a task Claude Code queued behind its turn into that turn:
/// the turn's end is still not the task's answer.
#[gpui_kit::test]
fn a_subagents_tool_end_doesnt_take_a_queued_task_in(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "the person's" });
    hook(handle, &ws, cx, &worker, prompt);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    // Its Enter, while the turn works: queued behind it.
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "[from …] go" });
    hook(handle, &ws, cx, &worker, prompt);
    hook(handle, &ws, cx, &worker, tool(Some("a1")));
    hook(handle, &ws, cx, &worker, stop("The person's answer.", &[]));
    assert!(answers.try_recv().is_err(), "the turn it was queued behind");
    hook(handle, &ws, cx, &worker, stop("The task's answer.", &[]));
    assert_eq!(
        answered(&answers),
        (false, "The task's answer.".to_string())
    );
}

/// A subagent stopped without a word (from Claude Code's panel) doesn't keep its parent busy: the
/// next turn's end, which names none at work, answers.
#[gpui_kit::test]
fn a_subagent_gone_without_a_word_doesnt_keep_its_parent_busy(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go", "wait": true });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "delegate", task);
    let prompt = json!({ "hook_event_name": "UserPromptSubmit", "prompt": "[from …] go" });
    hook(handle, &ws, cx, &worker, prompt);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, stop("Done.", &[]));
    assert_eq!(answered(&answers), (false, "Done.".to_string()));
}

/// While its subagents work after its turn ended, its still screen, typed into or not, neither
/// ends its task nor makes it seem stuck: their hooks tell.
#[cfg(unix)]
#[gpui_kit::test]
fn a_parent_awaiting_its_subagents_isnt_ended_by_its_screen(cx: &mut TestAppContext) {
    use crate::workspace::AgentState;
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (worker, them) = me(&ws, cx, theirs);
    let task = json!({ "block": them, "task": "go" });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, stop("Started one.", &["a1"]));
    // The person typed into it: its screen alone would rest it after five still looks.
    ws.update(cx, |ws, _| {
        ws.space.terminal(theirs).unwrap().typed.set(true)
    });
    for _ in 0..35 {
        after(cx, 1000);
    }
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(theirs).unwrap();
        assert_eq!(pane.state, Some(AgentState::Working));
        assert!(pane.handed.is_some(), "no answer from its screen");
    });
}

/// The blocks showing a subagent of the agent in `parent` (its terminal), in the order they
/// opened, with each one's subagent id.
fn followers(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    parent: &str,
) -> Vec<(BlockId, String)> {
    ws.read_with(cx, |ws, _| {
        let mut theirs: Vec<(BlockId, String)> = ws
            .space
            .terminals()
            .filter_map(|(id, p)| {
                let follows = p.follows.as_ref().filter(|f| f.parent == parent)?;
                Some((id, follows.id.clone()))
            })
            .collect();
        theirs.sort_by_key(|(id, _)| id.0);
        theirs
    })
}

#[test]
fn a_subagents_transcript_is_beside_the_sessions() {
    use crate::workspace::subagents::transcript_of;
    use std::path::Path;
    let main = Path::new("/home/u/.claude/projects/p").join("s1.jsonl");
    let theirs = transcript_of(main.to_str().unwrap(), "a80a32db516c45194").unwrap();
    assert_eq!(
        theirs,
        Path::new("/home/u/.claude/projects/p/s1")
            .join("subagents")
            .join("agent-a80a32db516c45194.jsonl")
    );
    for id in ["", "../../etc/passwd", "a/b", "a b"] {
        assert_eq!(
            transcript_of("/p/s1.jsonl", id),
            None,
            "{id:?} names no file of its"
        );
    }
}

/// A subagent's block runs nothing: it shows its transcript's lines as they are appended, what is
/// typed or pressed into it is refused, and it closes with nothing left to end.
#[gpui_kit::test]
fn a_subagents_block_shows_its_transcript_as_it_grows(cx: &mut TestAppContext) {
    use super::bus::call;
    use std::io::Write as _;
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("s1.jsonl");
    let start = json!({ "hook_event_name": "SubagentStart", "agent_id": "a1",
        "agent_type": "Explore", "transcript_path": main });
    hook(handle, &ws, cx, &worker, start);
    let block = followers(&ws, cx, &worker)[0].0;
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&block].clone());
    let shown = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let term = &ws.space.terminal(block).unwrap().term;
            (term.shared().lines(20).join("\n"), term.title())
        })
    };
    ws.read_with(cx, |ws, _| {
        let term = &ws.space.terminal(block).unwrap().term;
        assert_eq!(term.processes(), Default::default(), "no process");
        assert!(term.shells().is_empty());
    });
    let (text, title) = shown(cx);
    assert!(
        text.starts_with("A subagent of Claude Code, read-only"),
        "{text}"
    );
    assert_eq!(title, "Explore");
    let file = dir
        .path()
        .join("s1")
        .join("subagents")
        .join("agent-a1.jsonl");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let append = |line: Value| {
        let mut f = std::fs::File::options()
            .create(true)
            .append(true)
            .open(&file)
            .unwrap();
        writeln!(f, "{line}").unwrap();
    };
    append(json!({ "type": "user", "message": { "role": "user", "content": "List the files." } }));
    after(cx, 300);
    let (text, title) = shown(cx);
    assert!(text.contains("Task\nList the files."), "{text}");
    assert_eq!(title, "Explore · List the files.");
    append(
        json!({ "type": "assistant", "message": { "role": "assistant",
        "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Two files." }] } }),
    );
    after(cx, 300);
    let (text, _) = shown(cx);
    assert!(
        text.contains("Two files.") && text.ends_with("─ Done"),
        "{text}"
    );
    let (_, read) = call(
        handle,
        &ws,
        cx,
        &worker,
        "read_terminal",
        json!({ "block": uid }),
    );
    assert!(read.contains("Two files."), "{read}");
    let (_, listed) = call(handle, &ws, cx, &worker, "list_blocks", json!({}));
    let line = listed.lines().find(|l| l.starts_with(&uid)).unwrap();
    assert!(
        line.ends_with("a Claude Code subagent's transcript, read-only"),
        "{line}"
    );
    for (tool, args) in [
        (
            "send_to_terminal",
            json!({ "block": uid, "text": "hi", "enter": true }),
        ),
        ("send_keys", json!({ "block": uid, "keys": ["enter"] })),
        ("interrupt", json!({ "block": uid })),
        ("stop_agent", json!({ "block": uid })),
        ("delegate", json!({ "block": uid, "task": "go" })),
    ] {
        let (failed, text) = call(handle, &ws, cx, &worker, tool, args);
        assert!(failed && text.contains("read-only"), "{tool}: {text}");
    }
    hook(handle, &ws, cx, &worker, stopped("a1", &[]));
    after(cx, 61_000);
    assert!(followers(&ws, cx, &worker).is_empty(), "closed");
    append(json!({ "type": "user", "message": { "role": "user", "content": "Again." } }));
    after(cx, 1_000);
}

/// Each subagent opens a block beside its parent, wired to it, overlapping nothing; the
/// keyboard, the selection and Ctrl+Z stay as they were; a resumed one opens no second block.
/// The canvas is saved without them.
#[gpui_kit::test]
fn a_subagent_opens_beside_its_parent_wired_and_unsaved(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    let before = ws.read_with(cx, |ws, _| (ws.space.focused, ws.space.undo.len()));
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, started("a2", "general-purpose"));
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    let opened = followers(&ws, cx, &worker);
    let ids: Vec<&str> = opened.iter().map(|(_, id)| id.as_str()).collect();
    assert_eq!(ids, ["a1", "a2"]);
    ws.read_with(cx, |ws, _| {
        assert_eq!((ws.space.focused, ws.space.undo.len()), before);
        let rects: Vec<Rect> = ws.space.canvas.blocks().iter().map(|b| b.rect).collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.intersects(b), "{a:?} overlaps {b:?}");
            }
        }
        for (id, _) in &opened {
            assert!(
                ws.space
                    .wires
                    .iter()
                    .any(|w| w.touches(*id) && w.touches(theirs))
            );
            assert!(
                !ws.space
                    .wires
                    .iter()
                    .any(|w| w.touches(*id) && w.touches(mine))
            );
            assert!(!ws.space.kept(*id), "not saved");
        }
        assert!(ws.space.kept(theirs) && ws.space.kept(mine));
    });
    let (saved, _) = cx.update(|cx| {
        let ws = ws.read(cx);
        let (known, accounts) = (ws.known_agents(), ws.known_accounts());
        ws.space.layout(known, accounts, cx).snapshot(None)
    });
    assert_eq!(saved.items.len(), 2, "its two terminals, no subagent's");
    assert_eq!(saved.wires.len(), 1, "the wire between them");
    // In grid mode, its tiles are saved without theirs.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let all = ws.space.canvas.blocks().iter().map(|b| b.id).collect();
            ws.enter_grid(all, window, cx)
        })
    })
    .unwrap();
    let (saved, _) = cx.update(|cx| {
        let ws = ws.read(cx);
        assert_eq!(ws.space.grid.as_ref().unwrap().tiles.ids().len(), 4);
        let (known, accounts) = (ws.known_agents(), ws.known_accounts());
        ws.space.layout(known, accounts, cx).snapshot(None)
    });
    assert!(saved.grid.is_some(), "the grid is kept");
}

/// A finished subagent's block turns grey, and closes a minute later; while the person reads it
/// (focused, pointed at), it stays, and its minute starts over once they stop.
#[gpui_kit::test]
fn a_finished_subagent_closes_a_minute_later_unless_read(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &worker, started("a2", "Plan"));
    let opened = followers(&ws, cx, &worker);
    let (first, second) = (opened[0].0, opened[1].0);
    assert_eq!(dot(&ws, cx, first).as_deref(), Some("Working"));
    hook(handle, &ws, cx, &worker, stopped("a1", &["a2"]));
    hook(handle, &ws, cx, &worker, stopped("a2", &[]));
    assert_eq!(dot(&ws, cx, first).as_deref(), Some("Done"));
    // The person reads the second.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(second, window, cx))
    })
    .unwrap();
    after(cx, 59_000);
    assert_eq!(followers(&ws, cx, &worker).len(), 2, "a minute isn't over");
    after(cx, 2_000);
    let left: Vec<BlockId> = followers(&ws, cx, &worker).iter().map(|f| f.0).collect();
    assert_eq!(left, [second], "the one being read stays");
    ws.update(cx, |ws, _| {
        ws.space.focused = None;
        ws.space.hovered = Some(second);
    });
    after(cx, 61_000);
    assert_eq!(followers(&ws, cx, &worker).len(), 1, "pointed at");
    ws.update(cx, |ws, _| ws.space.hovered = None);
    after(cx, 1_000);
    assert_eq!(
        followers(&ws, cx, &worker).len(),
        1,
        "its minute starts over"
    );
    after(cx, 60_000);
    assert!(followers(&ws, cx, &worker).is_empty());
}

/// A finished subagent's block in a workspace not shown closes a minute later, though it was focused
/// and selected there when the person left it: no one reads it.
#[gpui_kit::test]
fn a_finished_subagent_in_a_hidden_workspace_closes(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    let block = followers(&ws, cx, &worker)[0].0;
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.focus_block(block, window, cx);
            ws.space.selected = vec![block];
            ws.new_space(window, cx);
        })
    })
    .unwrap();
    hook(handle, &ws, cx, &worker, stopped("a1", &[]));
    let open = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.spaces()
                .flat_map(|s| s.terminals())
                .any(|(_, p)| p.follows.is_some())
        })
    };
    after(cx, 59_000);
    assert!(open(cx), "a minute isn't over");
    after(cx, 2_000);
    assert!(!open(cx));
}

/// A subagent's dialog shows on its block too, with where to answer it.
#[gpui_kit::test]
fn a_subagents_block_says_where_its_dialog_is(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    let block = followers(&ws, cx, &worker)[0].0;
    let asks = json!({ "hook_event_name": "PermissionRequest", "tool_name": "Bash",
        "agent_id": "a1", "agent_type": "Explore" });
    hook(handle, &ws, cx, &worker, asks);
    assert_eq!(
        dot(&ws, cx, block).as_deref(),
        Some("Waiting for you: Allow Bash? Answer in its Claude Code.")
    );
    hook(handle, &ws, cx, &worker, tool(Some("a1")));
    assert_eq!(dot(&ws, cx, block).as_deref(), Some("Working"));
}

/// Closing the parent closes its subagents' blocks; its session ending finishes them.
#[gpui_kit::test]
fn a_subagents_block_goes_with_its_parent(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    let (asker, _) = me(&ws, cx, mine);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    hook(handle, &ws, cx, &asker, started("b1", "Plan"));
    let end = json!({ "hook_event_name": "SessionEnd", "reason": "other" });
    hook(handle, &ws, cx, &asker, end);
    let mine_left = followers(&ws, cx, &asker)[0].0;
    assert_eq!(dot(&ws, cx, mine_left).as_deref(), Some("Done"));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(theirs, window, cx))
    })
    .unwrap();
    assert!(followers(&ws, cx, &worker).is_empty());
    assert_eq!(followers(&ws, cx, &asker).len(), 1, "another's stay");
}

/// Typing into a subagent's block reaches nothing: it is read-only.
#[gpui_kit::test]
fn a_subagents_block_takes_no_keys(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = two_agents(cx);
    let (worker, _) = me(&ws, cx, theirs);
    hook(handle, &ws, cx, &worker, started("a1", "Explore"));
    let block = followers(&ws, cx, &worker)[0].0;
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(block, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_input(handle, "x");
    let typed = ws.read_with(cx, |ws, _| {
        ws.space.terminal(block).unwrap().term.had_input()
    });
    assert!(!typed);
}
