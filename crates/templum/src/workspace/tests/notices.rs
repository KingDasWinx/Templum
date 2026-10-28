//! Notices that wait for the person's text and come in batches (docs/specs/notice-batching.md).
//! Unix only (`mod.rs`): `cat` stands in for the agents, as in `delegate.rs`.

use std::time::{Duration, Instant};

use serde_json::json;

use super::bus::{after, event, me, screen_with};
use super::delegate::{asker, done_task, notices, two_agents};
use super::*;
use crate::workspace::delegate::TYPING;

/// The person's keys reach the terminal block `id`.
fn focus(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(id, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
}

/// The person types `text` into the terminal block `id`.
fn types(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
    text: &str,
) {
    focus(handle, ws, cx, id);
    cx.simulate_input(handle, text);
}

/// The person presses `key` in the terminal block `id`.
fn presses(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
    key: &str,
) {
    focus(handle, ws, cx, id);
    cx.update_window(handle, |_, window, cx| window.press(key, cx))
        .unwrap();
}

/// Their last key was TYPING ago, as the looks go on: only the draft holds a notice back.
fn long_ago(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) {
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(id).unwrap();
        pane.keyed.set(Instant::now().checked_sub(TYPING));
    });
}

/// `seconds` of the agents' looks, one a second.
fn looks(cx: &mut TestAppContext, seconds: u64) {
    for _ in 0..seconds {
        after(cx, 1000);
    }
}

fn has_draft(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> bool {
    ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().has_draft())
}

fn chip(id: BlockId) -> gpui_kit::ElementId {
    gpui_kit::ElementId::Name(format!("answers-waiting-{}", id.0).into())
}

fn chip_drawn(handle: AnyWindowHandle, cx: &mut TestAppContext, id: BlockId) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(chip(id)).is_some()
    })
    .unwrap()
}

/// The asker `mine` (Claude Code) has a half-written message and an answer waiting behind it,
/// the person's last key long ago.
fn held(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Workspace>, BlockId, BlockId, String) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    asker(&ws, cx, mine);
    types(handle, &ws, cx, mine, "half a sen");
    done_task(handle, &ws, cx, &terminal, theirs, "One.");
    long_ago(&ws, cx, mine);
    looks(cx, 30);
    assert_eq!(
        notices(&ws, cx).len(),
        1,
        "held, however long ago they typed"
    );
    assert!(ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().sent.is_none()));
    (handle, ws, mine, theirs, terminal)
}

/// The notice goes in: a look later, it shows on the asker's screen and the queue is empty.
fn goes_in(ws: &Entity<Workspace>, cx: &mut TestAppContext, mine: BlockId) {
    long_ago(ws, cx, mine);
    looks(cx, 2);
    screen_with(ws, cx, mine, "[task t1 from ");
    assert!(notices(ws, cx).is_empty());
}

#[gpui_kit::test]
fn a_draft_holds_a_notice_however_long_ago_it_was_typed(cx: &mut TestAppContext) {
    let (handle, ws, mine, _, _) = held(cx);
    assert!(has_draft(&ws, cx, mine));
    assert!(chip_drawn(handle, cx, mine), "the chip says it waits");
    assert_eq!(crate::block::answers_text(1), "1 answer waiting");
    assert_eq!(crate::block::answers_text(3), "3 answers waiting");
}

/// Claude Code says `Submitted` at its Enter, and its Enter alone ends nothing: a `\` before it
/// makes a new line. The hook ends the draft.
#[gpui_kit::test]
fn sending_the_draft_lets_the_notice_in(cx: &mut TestAppContext) {
    let (handle, ws, mine, _, terminal) = held(cx);
    // Its hooks were heard.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().speaks = true
    });
    presses(handle, &ws, cx, mine, "enter");
    assert!(has_draft(&ws, cx, mine), "its hook says whether it went");
    let submitted = json!({ "event": "submitted", "at_enter": true });
    event(handle, &ws, cx, &terminal, submitted);
    assert!(!has_draft(&ws, cx, mine));
    event(
        handle,
        &ws,
        cx,
        &terminal,
        json!({ "event": "stopped", "message": null }),
    );
    goes_in(&ws, cx, mine);
    assert!(!chip_drawn(handle, cx, mine), "the chip is gone");
}

/// Claude Code whose hooks never reach Templum (no bus, `disableAllHooks`) says nothing of its
/// Enter: only the screen could end the draft, so the Enter does.
#[gpui_kit::test]
fn claude_code_without_hooks_is_freed_by_its_enter(cx: &mut TestAppContext) {
    let (handle, ws, mine, _, _) = held(cx);
    assert!(!ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().speaks));
    presses(handle, &ws, cx, mine, "enter");
    assert!(!has_draft(&ws, cx, mine));
    goes_in(&ws, cx, mine);
}

/// The key reaches the pane's handler alone: a `cat` would die of a Ctrl+C.
fn pane_key(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    mine: BlockId,
    key: &str,
    (ctrl, shift, alt): (bool, bool, bool),
) {
    ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(mine)
            .unwrap()
            .person_keyed(&templum_terminal::KeyInput {
                key,
                key_char: None,
                ctrl,
                alt,
                shift,
            })
    });
}

#[gpui_kit::test]
fn clearing_the_draft_lets_the_notice_in(cx: &mut TestAppContext) {
    let (_, ws, mine, _, _) = held(cx);
    pane_key(&ws, cx, mine, "c", (true, false, false));
    assert!(!has_draft(&ws, cx, mine));
    goes_in(&ws, cx, mine);
}

/// Ctrl+U deletes to the start of the line: a draft of two lines keeps the first.
#[gpui_kit::test]
fn ctrl_u_leaves_the_draft_to_the_screen(cx: &mut TestAppContext) {
    let (_, ws, mine, _, _) = held(cx);
    pane_key(&ws, cx, mine, "u", (true, false, false));
    assert!(has_draft(&ws, cx, mine));
}

/// Keys that put text in an empty input: the draft is there.
fn key_makes_a_draft(cx: &mut TestAppContext, key: &str, mods: (bool, bool, bool), draft: bool) {
    let (_, ws, mine, _) = two_agents(cx);
    asker(&ws, cx, mine);
    pane_key(&ws, cx, mine, key, mods);
    assert_eq!(has_draft(&ws, cx, mine), draft, "{key} {mods:?}");
}

#[gpui_kit::test]
fn tab_makes_a_draft_of_the_suggestion(cx: &mut TestAppContext) {
    key_makes_a_draft(cx, "tab", (false, false, false), true);
}

/// Shift+Tab cycles Claude Code's modes: it types nothing.
#[gpui_kit::test]
fn shift_tab_makes_no_draft(cx: &mut TestAppContext) {
    key_makes_a_draft(cx, "tab", (false, true, false), false);
}

#[gpui_kit::test]
fn up_recalls_history_into_a_draft(cx: &mut TestAppContext) {
    key_makes_a_draft(cx, "up", (false, false, false), true);
}

#[gpui_kit::test]
fn down_recalls_history_into_a_draft(cx: &mut TestAppContext) {
    key_makes_a_draft(cx, "down", (false, false, false), true);
}

#[gpui_kit::test]
fn ctrl_y_yanks_a_draft_back(cx: &mut TestAppContext) {
    key_makes_a_draft(cx, "y", (true, false, false), true);
}

#[gpui_kit::test]
fn alt_enter_is_a_new_line_in_a_draft(cx: &mut TestAppContext) {
    key_makes_a_draft(cx, "enter", (false, false, true), true);
}

/// Text typed for a dialog (a "1", a "y") answers it: no draft.
#[gpui_kit::test]
fn typing_for_a_dialog_makes_no_draft(cx: &mut TestAppContext) {
    let (handle, ws, mine, _) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    asker(&ws, cx, mine);
    let asks = json!({ "event": "needs_you", "message": "Allow Edit?" });
    event(handle, &ws, cx, &terminal, asks);
    types(handle, &ws, cx, mine, "1");
    assert!(!has_draft(&ws, cx, mine));
}

#[gpui_kit::test]
fn shift_enter_keeps_the_draft(cx: &mut TestAppContext) {
    let (handle, ws, mine, _, _) = held(cx);
    presses(handle, &ws, cx, mine, "shift-enter");
    assert!(has_draft(&ws, cx, mine), "a new line in the message");
    long_ago(&ws, cx, mine);
    looks(cx, 5);
    assert_eq!(notices(&ws, cx).len(), 1);
}

#[gpui_kit::test]
fn a_paste_is_a_draft(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = {
        let (handle, ws, mine, theirs) = two_agents(cx);
        asker(&ws, cx, mine);
        (handle, ws, mine, theirs)
    };
    let (terminal, _) = me(&ws, cx, mine);
    focus(handle, &ws, cx, mine);
    cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string("pasted text".into()));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.paste_clipboard(window, cx))
    })
    .unwrap();
    assert!(has_draft(&ws, cx, mine));
    done_task(handle, &ws, cx, &terminal, theirs, "One.");
    long_ago(&ws, cx, mine);
    looks(cx, 30);
    assert_eq!(notices(&ws, cx).len(), 1, "held");
}

/// The person's Enter answers the dialog the agent asks, not the prompt: what they had typed is
/// still there.
#[gpui_kit::test]
fn enter_on_a_dialog_keeps_the_draft(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    asker(&ws, cx, mine);
    types(handle, &ws, cx, mine, "half a sen");
    let asks = json!({ "event": "needs_you", "message": "Allow Edit?" });
    event(handle, &ws, cx, &terminal, asks);
    presses(handle, &ws, cx, mine, "enter");
    assert!(has_draft(&ws, cx, mine), "it answered the dialog");
    event(handle, &ws, cx, &terminal, json!({ "event": "working" }));
    done_task(handle, &ws, cx, &terminal, theirs, "One.");
    long_ago(&ws, cx, mine);
    looks(cx, 30);
    assert_eq!(notices(&ws, cx).len(), 1, "held");
}

/// Codex, Qwen and OpenCode say `Submitted` once a turn takes a prompt in, not at the Enter: the
/// person may be writing the next one meanwhile.
#[gpui_kit::test]
fn a_submit_that_comes_with_a_turn_keeps_the_draft(cx: &mut TestAppContext) {
    let (handle, ws, mine, _, terminal) = held(cx);
    event(handle, &ws, cx, &terminal, json!({ "event": "submitted" }));
    assert!(has_draft(&ws, cx, mine));
}

/// A new session, or an agent that quit, leaves no draft.
#[gpui_kit::test]
fn a_new_session_ends_the_draft(cx: &mut TestAppContext) {
    let (handle, ws, mine, _, terminal) = held(cx);
    event(handle, &ws, cx, &terminal, json!({ "event": "started" }));
    assert!(!has_draft(&ws, cx, mine));
}

#[gpui_kit::test]
fn an_agent_that_quit_leaves_no_draft(cx: &mut TestAppContext) {
    let (handle, ws, mine, _, terminal) = held(cx);
    event(handle, &ws, cx, &terminal, json!({ "event": "ended" }));
    assert!(!has_draft(&ws, cx, mine));
}

/// Backspace to empty tells nothing Templum can count; the screen does: the bare prompt.
#[gpui_kit::test]
fn an_input_seen_empty_ends_the_draft(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    asker(&ws, cx, mine);
    // `cat` echoes the prompt, and what the person types after it.
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(mine).unwrap().term.type_in("› ")
    });
    screen_with(&ws, cx, mine, "›");
    types(handle, &ws, cx, mine, "x");
    presses(handle, &ws, cx, mine, "backspace");
    assert!(has_draft(&ws, cx, mine), "the keys can't tell");
    done_task(handle, &ws, cx, &terminal, theirs, "One.");
    goes_in(&ws, cx, mine);
    assert!(!has_draft(&ws, cx, mine));
}

/// An agent without hooks (a plain one) sends with its Enter, and its screen frees the rest.
#[gpui_kit::test]
fn a_plain_agent_without_hooks_is_freed_by_its_enter(cx: &mut TestAppContext) {
    let (handle, ws, mine, theirs) = two_agents(cx);
    let (terminal, _) = me(&ws, cx, mine);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().seen.agent = Some("aider".into())
    });
    super::delegate::found(&ws, cx, mine);
    assert!(!ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().speaks));
    types(handle, &ws, cx, mine, "half a sen");
    done_task(handle, &ws, cx, &terminal, theirs, "One.");
    long_ago(&ws, cx, mine);
    looks(cx, 30);
    assert_eq!(notices(&ws, cx).len(), 1, "held");
    presses(handle, &ws, cx, mine, "enter");
    goes_in(&ws, cx, mine);
}

// Batches: tasks handed to `workers` by the asker `mine` (set on their panes directly: what
// matters here is when their ends are typed), ended with `finish_task` as a `reply` would.

use crate::workspace::delegate::{BATCH_MOST, BATCH_WAIT, NOTICE_GAP, NoticeKind};

/// `n` terminals, none an agent; the asker `mine` (Claude Code) handed each a task, ids from 1.
fn workers(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    mine: BlockId,
    n: u64,
) -> Vec<BlockId> {
    let (asker, _) = me(ws, cx, mine);
    (0..n)
        .map(|i| {
            cx.update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    ws.open(
                        Rect::new(40.0, 500.0 + 40.0 * i as f32, 300.0, 100.0),
                        window,
                        cx,
                    );
                    let id = ws.space.canvas.top().unwrap();
                    let handed = crate::workspace::Handed {
                        id: i + 1,
                        taken: true,
                        ..crate::workspace::Handed::new(&asker, Duration::from_secs(3600))
                    };
                    ws.space.terminal_mut(id).unwrap().handed = Some(handed);
                    ws.space.appearing.clear();
                    id
                })
            })
            .unwrap()
        })
        .collect()
}

/// The agent in the terminal block `worker` answers its task: `said`.
fn ends(ws: &Entity<Workspace>, cx: &mut TestAppContext, worker: BlockId, said: &str) {
    let (terminal, _) = me(ws, cx, worker);
    ws.update(cx, |ws, cx| {
        assert!(!ws.finish_task(&terminal, said, cx).is_error);
    });
}

/// What the asker's terminal shows of what was typed into it.
fn typed(ws: &Entity<Workspace>, cx: &mut TestAppContext, mine: BlockId) -> String {
    after(cx, 300);
    std::thread::sleep(Duration::from_millis(200));
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(mine).unwrap().screen().replace('\n', "")
    })
}

/// `shown` has `n` notices under one `title`. `cat` shows what it is typed twice, its echo and its
/// output (once its Enter went): the notices are `n` for each title, whatever came apart would
/// have a notice without a title.
#[track_caller]
fn one_paste(shown: &str, title: &str, n: usize) {
    let titles = shown.matches(title).count();
    assert!(titles >= 1, "{shown}");
    assert_eq!(shown.matches("[task t").count(), n * titles, "{shown}");
}

fn nothing_typed(ws: &Entity<Workspace>, cx: &mut TestAppContext, mine: BlockId) {
    assert!(ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().sent.is_none()));
}

/// An asker with `n` tasks out, nothing typed into it by the person.
fn asking(
    cx: &mut TestAppContext,
    n: u64,
) -> (AnyWindowHandle, Entity<Workspace>, BlockId, Vec<BlockId>) {
    let (handle, ws, mine, _) = two_agents(cx);
    asker(&ws, cx, mine);
    let tasks = workers(handle, &ws, cx, mine, n);
    (handle, ws, mine, tasks)
}

#[gpui_kit::test]
fn notices_wait_for_the_others_and_go_as_one(cx: &mut TestAppContext) {
    let (_, ws, mine, w) = asking(cx, 3);
    ends(&ws, cx, w[0], "First.");
    after(cx, 5000);
    ends(&ws, cx, w[1], "Second.");
    after(cx, 1000);
    assert_eq!(notices(&ws, cx).len(), 2, "queued, behind the third");
    nothing_typed(&ws, cx, mine);
    // The last one running ended: at once.
    ends(&ws, cx, w[2], "Third.");
    after(cx, 1000);
    assert!(notices(&ws, cx).is_empty());
    let shown = typed(&ws, cx, mine);
    assert!(
        shown.contains("[3 tasks ended] They are in the pasted text."),
        "{shown}"
    );
    let (a, b, c) = (
        shown.find("First.").expect(&shown),
        shown.find("Second.").expect(&shown),
        shown.find("Third.").expect(&shown),
    );
    assert!(a < b && b < c, "oldest first: {shown}");
    one_paste(&shown, "[3 tasks ended]", 3);
}

#[gpui_kit::test]
fn the_window_restarts_with_each_notice(cx: &mut TestAppContext) {
    let (_, ws, mine, w) = asking(cx, 4);
    ends(&ws, cx, w[0], "First.");
    looks(cx, 10);
    ends(&ws, cx, w[1], "Second.");
    // The first would go BATCH_WAIT after it; the second moved that.
    looks(cx, BATCH_WAIT.as_secs() - 1);
    assert_eq!(notices(&ws, cx).len(), 2);
    nothing_typed(&ws, cx, mine);
    looks(cx, 3);
    assert!(notices(&ws, cx).is_empty());
    let shown = typed(&ws, cx, mine);
    assert!(shown.contains("[2 tasks ended]"), "{shown}");
}

#[gpui_kit::test]
fn no_notice_waits_more_than_a_minute(cx: &mut TestAppContext) {
    let (_, ws, mine, w) = asking(cx, 8);
    // Six ends, ten seconds apart: each restarts the window, which never closes.
    for (i, &worker) in w[..6].iter().enumerate() {
        ends(&ws, cx, worker, &format!("Answer {i}."));
        looks(cx, 10);
    }
    // 60 s since the first end, queued a second after it: BATCH_MOST is not past yet, but is
    // two looks later.
    assert_eq!(BATCH_MOST.as_secs(), 60);
    nothing_typed(&ws, cx, mine);
    assert_eq!(notices(&ws, cx).len(), 6);
    looks(cx, 2);
    assert!(notices(&ws, cx).is_empty());
    let shown = typed(&ws, cx, mine);
    assert!(shown.contains("[6 tasks ended]"), "{shown}");
}

#[gpui_kit::test]
fn a_lone_task_ends_at_once(cx: &mut TestAppContext) {
    let (_, ws, mine, w) = asking(cx, 1);
    ends(&ws, cx, w[0], "Only.");
    after(cx, 1000);
    assert!(notices(&ws, cx).is_empty());
    let shown = typed(&ws, cx, mine);
    assert!(shown.contains("[task t1 from "), "{shown}");
    assert!(!shown.contains("tasks ended"), "no title for one: {shown}");
}

#[gpui_kit::test]
fn other_notices_dont_wait_for_the_window(cx: &mut TestAppContext) {
    let (_, ws, mine, _) = asking(cx, 2);
    let (terminal, _) = me(&ws, cx, mine);
    ws.update(cx, |ws, cx| {
        ws.queue_notice(
            terminal,
            "[scheduled s1] Its text.".into(),
            NoticeKind::Other,
            cx,
        )
    });
    assert!(notices(&ws, cx).is_empty(), "two tasks run, and it went");
    let shown = typed(&ws, cx, mine);
    assert!(shown.contains("[scheduled s1]"), "{shown}");
}

#[gpui_kit::test]
fn twenty_tasks_ending_together_make_one_batch(cx: &mut TestAppContext) {
    let (_, ws, mine, w) = asking(cx, 20);
    for (i, &worker) in w.iter().enumerate() {
        ends(&ws, cx, worker, &format!("Answer {i}."));
    }
    after(cx, 1000);
    assert!(notices(&ws, cx).is_empty());
    // The title counts the batch: 20 of 20 in one. Not `one_paste`: the paste is past the 1024
    // bytes macOS's tty keeps of a line, so `cat` shows only some of them there.
    let shown = typed(&ws, cx, mine);
    assert!(shown.contains("[20 tasks ended]"), "{shown}");
}

/// Its terminal is closed while its answers wait for the others: they are dropped.
#[gpui_kit::test]
fn a_batch_of_a_closed_terminal_is_dropped(cx: &mut TestAppContext) {
    let (handle, ws, mine, w) = asking(cx, 2);
    ends(&ws, cx, w[0], "First.");
    after(cx, 1000);
    assert_eq!(notices(&ws, cx).len(), 1, "waits for the second");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.close_now(&[mine], crate::workspace::history::Then::New, window, cx)
        })
    })
    .unwrap();
    looks(cx, 2);
    assert!(notices(&ws, cx).is_empty());
    ends(&ws, cx, w[1], "Second.");
    looks(cx, 2);
    assert!(notices(&ws, cx).is_empty());
}

/// Templum typed into the asker a moment ago: the answers that came meanwhile wait for the gap,
/// then go as one paste, not one each.
#[gpui_kit::test]
fn a_batch_waits_the_gap_as_one(cx: &mut TestAppContext) {
    let (_, ws, mine, w) = asking(cx, 3);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().sent = Some(Instant::now())
    });
    ends(&ws, cx, w[0], "First.");
    ends(&ws, cx, w[1], "Second.");
    after(cx, 1000);
    ends(&ws, cx, w[2], "Third.");
    looks(cx, 3);
    assert_eq!(notices(&ws, cx).len(), 3, "the gap");
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(mine).unwrap().sent = Instant::now().checked_sub(NOTICE_GAP)
    });
    looks(cx, 2);
    assert!(notices(&ws, cx).is_empty());
    let shown = typed(&ws, cx, mine);
    assert!(shown.contains("[3 tasks ended]"), "{shown}");
    one_paste(&shown, "[3 tasks ended]", 3);
}

/// A task waits for its person, and the others still run: its notice waits for them. The person
/// answers the dialog meanwhile: the asker isn't told it waits.
#[gpui_kit::test]
fn a_waiting_notice_whose_dialog_was_answered_is_dropped(cx: &mut TestAppContext) {
    let (handle, ws, mine, w) = asking(cx, 2);
    let (worker, _) = me(&ws, cx, w[0]);
    let asks = json!({ "event": "needs_you", "message": "Allow Edit?" });
    event(handle, &ws, cx, &worker, asks);
    after(cx, 400);
    assert_eq!(notices(&ws, cx).len(), 1, "queued, behind the other task");
    event(handle, &ws, cx, &worker, json!({ "event": "working" }));
    looks(cx, 2);
    assert!(notices(&ws, cx).is_empty(), "dropped");
    // The first task works on: the second's end goes with the window, alone.
    ends(&ws, cx, w[1], "Second.");
    looks(cx, BATCH_WAIT.as_secs() + 2);
    let shown = typed(&ws, cx, mine);
    assert!(shown.contains("[task t2 from "), "{shown}");
    assert!(!shown.contains("waits for its person"), "{shown}");
}
