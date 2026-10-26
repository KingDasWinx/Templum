//! The requests inbox's model: what it lists, what is selected, and why each is asked
//! (docs/specs/ask-inbox.md).

use std::time::Duration;

use serde_json::json;
use templum_mcp::Reply;

use std::sync::mpsc::{Receiver, TryRecvError};

use gpui_kit::{ElementId, Focusable as _};

use super::bus::{after, answered, bus_note, call_later, me};
use super::*;
use crate::board::Assignee;
use crate::workspace::inbox::Why;
use crate::workspace::{Handed, Waiting};

/// A terminal opened apart from the others, at row `row` of the canvas.
pub(super) fn open_terminal(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    row: u8,
) -> BlockId {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open(
                Rect::new(40.0, 800.0 + f32::from(row) * 400.0, 400.0, 300.0),
                window,
                cx,
            );
            ws.space.appearing.clear();
            ws.space.canvas.top().unwrap()
        })
    })
    .unwrap()
}

/// The keys of the requests the inbox lists.
fn listed(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<u64> {
    ws.read_with(cx, |ws, _| ws.inbox().iter().map(|w| w.key).collect())
}

/// The key of the request the inbox has selected.
fn selected(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Option<u64> {
    ws.read_with(cx, |ws, _| ws.inbox_selected().map(|w| w.key))
}

fn open_inbox(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    at: Option<u64>,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_inbox(at, window, cx))
    })
    .unwrap();
}

/// The terminal `terminal` asks `ask_user` question `n` on connection `n`: its wait's key.
fn ask(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    terminal: &str,
    n: u64,
) -> u64 {
    let asked = json!({ "question": format!("Q{n}?"), "options": ["Yes"] });
    let _ = call_later(handle, ws, cx, terminal, n, "ask_user", asked);
    ws.read_with(cx, |ws, _| ws.waits.last().expect("a wait").key)
}

fn answer(ws: &Entity<Workspace>, cx: &mut TestAppContext, key: u64) {
    ws.update(cx, |ws, cx| ws.answer(key, Reply::ok("Yes"), cx));
}

/// The terminal `by` (wired to `block`) waits for a line of `block`'s screen that never comes: a
/// wait that is no request of the person's. Its key.
fn watching(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    by: &str,
    block: &str,
    conn: u64,
) -> u64 {
    let watch = json!({ "block": block, "pattern": "never", "wait": true, "timeout": 300 });
    let _ = call_later(handle, ws, cx, by, conn, "wait_for", watch);
    ws.read_with(cx, |ws, _| ws.waits.last().expect("a wait").key)
}

#[gpui_kit::test]
fn the_inbox_lists_this_workspaces_requests_oldest_first(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, a) = three_blocks(handle, &ws, cx);
    let b = open_terminal(handle, &ws, cx, 0);
    let c = open_terminal(handle, &ws, cx, 1);
    let d = open_terminal(handle, &ws, cx, 2);
    let e = open_terminal(handle, &ws, cx, 3);
    let (a_id, _) = me(&ws, cx, a);
    let (b_id, _) = me(&ws, cx, b);
    let (c_id, _) = me(&ws, cx, c);
    let (_, d_uid) = me(&ws, cx, d);
    let (e_id, _) = me(&ws, cx, e);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    // C waits for a line of D's screen, which never comes: not a request of the person's.
    ws.update(cx, |ws, cx| assert!(ws.connect(c, d, cx)));
    let _watching = call_later(
        handle,
        &ws,
        cx,
        &c_id,
        3,
        "wait_for",
        json!({ "block": d_uid, "pattern": "never", "wait": true, "timeout": 300 }),
    );
    // E asks from a workspace of its own.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.move_blocks(vec![e], None, window, cx))
    })
    .unwrap();
    let first = ask(handle, &ws, cx, &a_id, 1);
    let _ = call_later(handle, &ws, cx, &b_id, 2, "connect", json!({ "block": k }));
    let second = ws.read_with(cx, |ws, _| ws.waits.last().unwrap().key);
    ask(handle, &ws, cx, &e_id, 4);
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.waits
                .iter()
                .any(|w| matches!(w.what, Waiting::Event { .. })),
            "C's wait_for is a wait, not a request"
        );
        assert_eq!(ws.waits.len(), 4);
    });
    assert_eq!(listed(&ws, cx), [first, second]);
}

#[gpui_kit::test]
fn why_says_the_card_the_task_and_the_agents_words(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (_, board, early) = board_with_a_card(handle, &ws, cx);
    let (terminal, uid) = me(&ws, cx, a);
    // The card it works on is the newest one not in the last column; a card Done, though newer,
    // is finished.
    let (working, done) = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            let columns: Vec<u64> = b
                .board(board)
                .unwrap()
                .columns
                .iter()
                .map(|c| c.id)
                .collect();
            let working = b.add_card(board, columns[1]).unwrap();
            let done = b.add_card(board, columns[2]).unwrap();
            for (card, title) in [(early, "Old"), (working, "Parser"), (done, "Shipped")] {
                b.edit_card(board, card, |c| {
                    c.title = title.into();
                    c.assignee = Some(Assignee::Block(uid.clone()));
                });
            }
            (working, done)
        })
    });
    assert!(early < working && working < done);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a).unwrap().handed = Some(Handed {
            text: "Write the parser\nthen tests\nthen docs\nthen release".into(),
            ..Handed::new("x", Duration::from_secs(600))
        });
    });
    let asked = json!({ "question": "Which format?", "options": ["A"], "why": "Need the format" });
    let _ = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let why = ws.read_with(cx, |ws, _| ws.why(ws.inbox_selected().unwrap()));
    assert_eq!(
        why,
        Why {
            card: Some((working, "Parser".into())),
            task: Some("Write the parser\nthen tests\nthen docs".into()),
            agent: Some("Need the format".into()),
        }
    );
    // Nothing known, nothing said.
    let b = open_terminal(handle, &ws, cx, 0);
    let (other, _) = me(&ws, cx, b);
    let _ = call_later(
        handle,
        &ws,
        cx,
        &other,
        2,
        "ask_user",
        json!({ "question": "Q?" }),
    );
    let why = ws.read_with(cx, |ws, _| ws.why(ws.inbox().into_iter().last().unwrap()));
    assert_eq!(
        why,
        Why {
            card: None,
            task: None,
            agent: None
        }
    );
}

#[gpui_kit::test]
fn answering_the_selected_request_selects_the_next(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let keys: Vec<u64> = (1..=3)
        .map(|n| ask(handle, &ws, cx, &terminal, n))
        .collect();
    assert_eq!(
        selected(&ws, cx),
        Some(keys[0]),
        "the first, as none is chosen"
    );
    open_inbox(handle, &ws, cx, Some(keys[1]));
    assert_eq!(selected(&ws, cx), Some(keys[1]));
    answer(&ws, cx, keys[1]);
    assert_eq!(selected(&ws, cx), Some(keys[2]));
    answer(&ws, cx, keys[2]);
    assert_eq!(selected(&ws, cx), Some(keys[0]), "none after it: the first");
    // A new request joins the end and doesn't take the selection.
    let newer = ask(handle, &ws, cx, &terminal, 4);
    assert_eq!(listed(&ws, cx), [keys[0], newer]);
    assert_eq!(selected(&ws, cx), Some(keys[0]));
}

#[gpui_kit::test]
fn a_selected_requests_block_moved_away_leaves_the_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let a = first_id(&ws, cx);
    let b = open_terminal(handle, &ws, cx, 0);
    let c = open_terminal(handle, &ws, cx, 1);
    let keys: Vec<u64> = [a, b, c]
        .into_iter()
        .zip(1..)
        .map(|(id, n)| {
            let (terminal, _) = me(&ws, cx, id);
            ask(handle, &ws, cx, &terminal, n)
        })
        .collect();
    open_inbox(handle, &ws, cx, Some(keys[1]));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.move_blocks(vec![b], None, window, cx))
    })
    .unwrap();
    assert_eq!(listed(&ws, cx), [keys[0], keys[2]]);
    assert_eq!(selected(&ws, cx), Some(keys[2]), "the next");
    // Its block closes: the request ends with it, and nothing panics.
    open_inbox(handle, &ws, cx, Some(keys[2]));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(c, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    assert_eq!(listed(&ws, cx), [keys[0]]);
    assert_eq!(selected(&ws, cx), Some(keys[0]));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.waits.len(), 2, "A's, and B's in its workspace");
    });
}

#[gpui_kit::test]
fn the_inbox_and_the_git_popup_are_never_open_together(cx: &mut TestAppContext) {
    let repo = committed_repo();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    ws.read_with(cx, |ws, _| assert!(ws.git_open && !ws.inbox_open));
    open_inbox(handle, &ws, cx, None);
    ws.read_with(cx, |ws, _| assert!(ws.inbox_open && !ws.git_open));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.open_git(window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert!(ws.git_open && !ws.inbox_open));
    // The inbox's own toggle opens it, then closes it.
    for open in [true, false] {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.toggle_inbox(window, cx))
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.inbox_open, open));
    }
}

/// Another workspace's requests are not the shown one's: switching clears the selection, and a
/// request that is not listed (answered, or another workspace's) can't be selected. Nothing
/// ending elsewhere moves what the person is reading.
#[gpui_kit::test]
fn an_unrelated_wait_ending_leaves_the_selection(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let a = first_id(&ws, cx);
    let [b, c, d, e, f] = [0, 1, 2, 3, 4].map(|row| open_terminal(handle, &ws, cx, row));
    let ids = [a, b, c, d].map(|id| me(&ws, cx, id).0);
    let keys: Vec<u64> = ids
        .iter()
        .zip(1..)
        .map(|(terminal, n)| ask(handle, &ws, cx, terminal, n))
        .collect();
    let (watcher, _) = me(&ws, cx, e);
    let (_, watched) = me(&ws, cx, f);
    ws.update(cx, |ws, cx| assert!(ws.connect(e, f, cx)));
    let unrelated = watching(handle, &ws, cx, &watcher, &watched, 9);
    ws.read_with(cx, |ws, _| {
        let wait = ws.waits.iter().find(|w| w.key == unrelated).unwrap();
        assert!(matches!(wait.what, Waiting::Event { .. }), "it waits");
    });
    // C and D, and their requests, go to a workspace of their own.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.move_blocks(vec![c, d], None, window, cx))
    })
    .unwrap();
    open_inbox(handle, &ws, cx, Some(keys[1]));
    assert_eq!(selected(&ws, cx), Some(keys[1]));
    // Not listed here: the selection stays.
    open_inbox(handle, &ws, cx, Some(keys[2]));
    assert_eq!(selected(&ws, cx), Some(keys[1]));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(1, window, cx))
    })
    .unwrap();
    assert_eq!(listed(&ws, cx), [keys[2], keys[3]]);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.inbox_at, None, "switching clears it")
    });
    assert_eq!(selected(&ws, cx), Some(keys[2]), "the first there");
    ws.update(cx, |ws, cx| {
        ws.answer(unrelated, Reply::err("timed out"), cx)
    });
    assert_eq!(
        selected(&ws, cx),
        Some(keys[2]),
        "not the last: 12 is not 20"
    );
    // A request answered since can't be selected either.
    answer(&ws, cx, keys[2]);
    open_inbox(handle, &ws, cx, Some(keys[2]));
    assert_eq!(selected(&ws, cx), Some(keys[3]));
}

/// A block closes in a workspace not shown, while a `wait_for_idle` waits for it: that wait ends
/// inside the swap that shows its workspace for a moment, and the selection here stays.
#[gpui_kit::test]
fn a_block_closing_in_a_hidden_workspace_leaves_the_selection(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let a = first_id(&ws, cx);
    let b = open_terminal(handle, &ws, cx, 0);
    let c = open_terminal(handle, &ws, cx, 1);
    let keys: Vec<u64> = [a, b, c]
        .into_iter()
        .zip(1..)
        .map(|(id, n)| {
            let (terminal, _) = me(&ws, cx, id);
            ask(handle, &ws, cx, &terminal, n)
        })
        .collect();
    // The hidden workspace: Y opened X and waits for its turn.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx))
    })
    .unwrap();
    let (x, y) = (
        open_terminal(handle, &ws, cx, 0),
        open_terminal(handle, &ws, cx, 1),
    );
    let ((x_terminal, x_uid), (y_terminal, y_uid)) = (me(&ws, cx, x), me(&ws, cx, y));
    ws.update(cx, |ws, cx| {
        assert!(ws.connect(y, x, cx));
        let pane = ws.space.terminal_mut(x).unwrap();
        pane.seen.agent = Some("codex".into());
        pane.opened_by = Some(y_uid);
        pane.handed = Some(Handed::new(&y_terminal, Duration::from_secs(600)));
    });
    let idle = json!({ "block": x_uid });
    let _waiting = call_later(handle, &ws, cx, &y_terminal, 9, "wait_for_idle", idle);
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.waits
                .iter()
                .any(|w| matches!(w.what, Waiting::Turn { .. }))
        )
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx))
    })
    .unwrap();
    open_inbox(handle, &ws, cx, Some(keys[1]));
    let (failed, text) = super::bus::call(
        handle,
        &ws,
        cx,
        &y_terminal,
        "close_block",
        json!({ "block": x_uid }),
    );
    assert!(!failed, "{text}");
    ws.read_with(cx, |ws, _| {
        assert!(
            !ws.waits
                .iter()
                .any(|w| w.from == y_terminal && matches!(w.what, Waiting::Turn { .. })),
            "the wait ended"
        );
    });
    assert_eq!(selected(&ws, cx), Some(keys[1]));
    let _ = x_terminal;
}

/// Closing the inbox gives the keyboard back to the canvas only when the inbox had it.
#[gpui_kit::test]
fn closing_the_inbox_keeps_the_keyboard_where_it_went(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let toggle = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.toggle_inbox(window, cx))
        })
        .unwrap();
    };
    let focus = |cx: &mut TestAppContext, canvas: bool| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let handle = if canvas {
                    &ws.focus_handle
                } else {
                    &ws.inbox_focus
                };
                handle.contains_focused(window, cx)
            })
        })
        .unwrap()
    };
    // The inbox has the keyboard it took: closed, the canvas has it back.
    toggle(cx);
    assert!(focus(cx, false));
    toggle(cx);
    assert!(focus(cx, true));
    // The person clicked elsewhere (the settings field stands for it): closing leaves it there.
    toggle(cx);
    let elsewhere = ws.read_with(cx, |ws, _| ws.settings_focus.clone());
    cx.update_window(handle, |_, window, cx| elsewhere.focus(window, cx))
        .unwrap();
    toggle(cx);
    assert!(!focus(cx, true) && !focus(cx, false));
}

#[gpui_kit::test]
fn a_request_keeps_when_it_arrived(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let key = ask(handle, &ws, cx, &terminal, 1);
    super::bus::after(cx, 90_000);
    let waited = ws.read_with(cx, |ws, cx| {
        let wait = ws.inbox().into_iter().find(|w| w.key == key).unwrap();
        cx.background_executor().now() - wait.since
    });
    assert_eq!(waited, Duration::from_secs(90));
}

pub(super) fn name(text: String) -> ElementId {
    ElementId::Name(text.into())
}

/// The inbox open and drawn, long enough ago for presses and keys to count (`TOO_SOON`).
fn drawn_inbox(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            if !ws.inbox_open {
                ws.open_inbox(None, window, cx)
            }
        });
        window.render_frame(cx)
    })
    .unwrap();
    after(cx, 600);
}

/// Clicks the element `id`, the window drawn before and after.
pub(super) fn click_id(handle: AnyWindowHandle, cx: &mut TestAppContext, id: ElementId) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(id, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

pub(super) fn drawn(handle: AnyWindowHandle, cx: &mut TestAppContext, id: ElementId) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find(id).is_some()
    })
    .unwrap()
}

fn part(n: BlockId, what: &str) -> ElementId {
    crate::balloon::part(n.0 as usize, what)
}

pub(super) fn asks(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    terminal: &str,
    conn: u64,
) -> Receiver<String> {
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "No"] });
    call_later(handle, ws, cx, terminal, conn, "ask_user", asked)
}

#[test]
fn a_wait_is_told_in_minutes_and_hours() {
    use crate::inbox::age;
    let secs = Duration::from_secs;
    assert_eq!(age(secs(0)), "now");
    assert_eq!(age(secs(59)), "now");
    assert_eq!(age(secs(60)), "1m");
    assert_eq!(age(secs(125)), "2m");
    assert_eq!(age(secs(3600)), "1h");
    assert_eq!(age(secs(90_000)), "1d");
}

#[gpui_kit::test]
fn a_question_is_answered_from_the_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let answers = asks(handle, &ws, cx, &terminal, 1);
    drawn_inbox(handle, &ws, cx);
    click_id(handle, cx, part(a, "q0-option-1"));
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "Send answers");
    click_id(handle, cx, part(a, "send"));
    assert_eq!(answered(&answers), (false, "No".to_string()));
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
}

#[gpui_kit::test]
fn connect_is_allowed_from_the_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let (k, n) = ws.read_with(cx, |ws, _| {
        (ws.space.uids[&kanban].clone(), ws.space.uids[&note].clone())
    });
    let allow = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "connect",
        json!({ "block": k }),
    );
    let deny = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "connect",
        json!({ "block": n }),
    );
    drawn_inbox(handle, &ws, cx);
    click_id(handle, cx, part(a, "allow"));
    assert_eq!(answered(&allow), (false, format!("Connected to {k}.")));
    ws.read_with(cx, |ws, _| assert!(ws.space.wired(a, kanban)));
    // The next request is selected: Deny it.
    after(cx, 600);
    click_id(handle, cx, part(a, "deny"));
    let (failed, text) = answered(&deny);
    assert!(failed && text.contains("didn't connect you"), "{text}");
    ws.read_with(cx, |ws, _| assert!(!ws.space.wired(a, note)));
}

#[gpui_kit::test]
fn go_to_terminal_reveals_the_block_and_closes_the_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    ws.update(cx, |ws, _| ws.space.focused = Some(kanban));
    drawn_inbox(handle, &ws, cx);
    click_id(handle, cx, name("inbox-go".into()));
    ws.read_with(cx, |ws, _| {
        assert!(!ws.inbox_open, "closed");
        assert_eq!(ws.space.focused, Some(a), "revealed");
        assert_eq!(ws.waits.len(), 1, "the request waits");
    });
}

#[gpui_kit::test]
fn the_last_answer_shows_nothing_waits(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    drawn_inbox(handle, &ws, cx);
    assert!(!drawn(handle, cx, name("inbox-empty".into())));
    let key = ws.read_with(cx, |ws, _| ws.waits[0].key);
    ws.update(cx, |ws, cx| ws.answer(key, Reply::ok("Yes"), cx));
    assert!(drawn(handle, cx, name("inbox-empty".into())));
    ws.read_with(cx, |ws, _| assert!(ws.inbox_open, "still open"));
}

#[gpui_kit::test]
fn why_is_shown_when_something_is_known(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let _plain = asks(handle, &ws, cx, &terminal, 1);
    drawn_inbox(handle, &ws, cx);
    assert!(
        !drawn(handle, cx, name("inbox-why".into())),
        "nothing known"
    );
    let said = json!({ "question": "Which?", "options": ["A"], "why": "Need the format" });
    let _said = call_later(handle, &ws, cx, &terminal, 2, "ask_user", said);
    let second = ws.read_with(cx, |ws, _| ws.waits[1].key);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.select_request(second, window, cx))
    })
    .unwrap();
    assert!(
        drawn(handle, cx, name("inbox-why".into())),
        "the agent's words"
    );
}

/// A question whose client stopped waiting says so in the inbox, and its answer is still typed
/// into its terminal (Review Focus 3).
#[gpui_kit::test]
fn a_stopped_question_keeps_its_banner_in_the_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a).unwrap().seen.agent = Some("claude".into())
    });
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    drawn_inbox(handle, &ws, cx);
    assert!(!drawn(handle, cx, part(a, "stopped")));
    let cancel = json!({ "requestId": 1, "reason": "The person pressed Esc" });
    bus_note(handle, &ws, cx, 1, "notifications/cancelled", cancel);
    assert!(drawn(handle, cx, part(a, "stopped")), "the banner");
    click_id(handle, cx, part(a, "q0-option-0"));
    click_id(handle, cx, part(a, "send"));
    ws.read_with(cx, |ws, _| {
        assert!(ws.waits.is_empty());
        assert_eq!(ws.notices.len(), 1, "typed into its terminal");
        assert!(ws.notices[0].text.contains("Yes"), "{}", ws.notices[0].text);
    });
}

/// A press outside the inbox closes it, as any popup's backdrop does: the terminal under the press
/// never gets it, and the keyboard goes back to the canvas.
#[gpui_kit::test]
fn a_press_outside_the_inbox_closes_it_and_reaches_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
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
    // The block opened has the focus: it goes back to the asker's.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(a, window, cx))
    })
    .unwrap();
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    drawn_inbox(handle, &ws, cx);
    let focused_before = ws.read_with(cx, |ws, _| ws.space.focused);
    assert_eq!(focused_before, Some(a));
    let at = at(&ws, cx, (1050.0, 210.0));
    cx.update_window(handle, |_, window, cx| {
        click(window, cx, at, 1);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.inbox_open, "closed");
        assert_ne!(ws.space.focused, Some(b), "the terminal got nothing");
        assert_eq!(ws.space.focused, focused_before);
        assert_eq!(ws.waits.len(), 1, "the request still waits");
    });
    cx.update_window(handle, |_, window, cx| {
        assert!(ws.read(cx).inbox_keys(window, cx).is_none());
        assert!(
            ws.read(cx).focus_handle.is_focused(window),
            "the canvas has the keyboard"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_terminal_shows_a_notice_not_a_balloon(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let notice = name(format!("waiting-notice-{}", a.0));
    assert!(!drawn(handle, cx, notice.clone()), "nothing asked");
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    assert!(drawn(handle, cx, notice), "the notice");
    for what in ["waiting", "q0-option-0", "send", "dismiss"] {
        assert!(!drawn(handle, cx, part(a, what)), "no balloon: {what}");
    }
    ws.read_with(cx, |ws, _| {
        assert!(!ws.inbox_open, "nothing opened by itself")
    });
    assert_eq!(
        crate::inbox::notice_text(1),
        "Waiting for your answer",
        "what it reads"
    );
}

/// Two requests of one terminal are both listed, its notice says 2, and a click on it opens the
/// inbox at the oldest (Review Focus 5).
#[gpui_kit::test]
fn two_requests_of_one_terminal_count_on_its_notice(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let _one = asks(handle, &ws, cx, &terminal, 1);
    let _two = asks(handle, &ws, cx, &terminal, 2);
    let keys = listed(&ws, cx);
    assert_eq!(keys.len(), 2);
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.asking(&terminal).map(|(w, count)| (w.key, count)),
            Some((keys[0], 2))
        );
    });
    assert_eq!(
        crate::inbox::notice_text(2),
        "Waiting for your answer · 2",
        "what it reads"
    );
    // The inbox was left on the newer one; the notice opens it at the older.
    open_inbox(handle, &ws, cx, Some(keys[1]));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close_inbox(window, cx))
    })
    .unwrap();
    click_id(handle, cx, name(format!("waiting-notice-{}", a.0)));
    ws.read_with(cx, |ws, _| assert!(ws.inbox_open));
    assert_eq!(selected(&ws, cx), Some(keys[0]));
}

/// What the terminal of block `id` shows, once its program had time to echo.
#[cfg(unix)]
fn screen(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> String {
    std::thread::sleep(Duration::from_millis(300));
    ws.read_with(cx, |ws, _| {
        let mut snap = templum_terminal::Snapshot::default();
        snap.refresh(
            ws.space.terminal(id).unwrap().term.shared(),
            &ws.theme.palette,
        );
        snap.text()
    })
}

/// A request arriving while the person types in a terminal leaves the keyboard where it is: the
/// inbox closed, and open without the keyboard (Review Focus 1).
#[cfg(unix)]
#[gpui_kit::test]
fn a_request_arriving_never_takes_the_keyboard(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(a, window, cx));
        window.input("one", cx);
    })
    .unwrap();
    assert!(screen(&ws, cx, a).contains("one"));
    // Closed.
    let _first = asks(handle, &ws, cx, &terminal, 1);
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| window.input("two", cx))
        .unwrap();
    assert!(
        screen(&ws, cx, a).contains("onetwo"),
        "typed on: {}",
        screen(&ws, cx, a)
    );
    // Open, but the keyboard is the terminal's again after a click on it.
    open_inbox(handle, &ws, cx, None);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(a, window, cx));
    })
    .unwrap();
    let _second = asks(handle, &ws, cx, &terminal, 2);
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| window.input("three", cx))
        .unwrap();
    assert!(
        screen(&ws, cx, a).contains("onetwothree"),
        "{}",
        screen(&ws, cx, a)
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.waits.len(), 2, "nothing was answered")
    });
}

#[gpui_kit::test]
fn the_inbox_keys_answer_the_selected_request(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let first = asks(handle, &ws, cx, &terminal, 1);
    let second = asks(handle, &ws, cx, &terminal, 2);
    let keys = listed(&ws, cx);
    drawn_inbox(handle, &ws, cx);
    let press = |cx: &mut TestAppContext, keys: &[&str]| {
        cx.update_window(handle, |_, window, cx| {
            for key in keys {
                window.press(key, cx);
                window.render_frame(cx);
            }
        })
        .unwrap();
    };
    // ↓ highlights Yes, Space picks it, Enter sends.
    press(cx, &["down", "space", "enter"]);
    assert_eq!(answered(&first), (false, "Yes".to_string()));
    assert_eq!(second.try_recv(), Err(TryRecvError::Empty));
    assert_eq!(selected(&ws, cx), Some(keys[1]), "the next one");
    // Alt+↓ and Alt+↑ move in the list; Esc closes the inbox.
    let third = asks(handle, &ws, cx, &terminal, 3);
    let keys = listed(&ws, cx);
    press(cx, &["alt-down"]);
    assert_eq!(selected(&ws, cx), Some(keys[1]), "the last: no further");
    press(cx, &["alt-up"]);
    assert_eq!(selected(&ws, cx), Some(keys[0]));
    press(cx, &["alt-down"]);
    assert_eq!(selected(&ws, cx), Some(keys[1]));
    press(cx, &["escape"]);
    ws.read_with(cx, |ws, _| assert!(!ws.inbox_open));
    assert_eq!(
        third.try_recv(),
        Err(TryRecvError::Empty),
        "nothing answered"
    );
}

/// Esc in a field of the request closes the inbox too.
#[gpui_kit::test]
fn esc_in_a_field_closes_the_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let free = json!({ "questions": [{ "question": "Why?" }] });
    let _answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", free);
    drawn_inbox(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press("tab", cx);
        window.render_frame(cx);
        let field = match &ws.read(cx).waits[0].what {
            Waiting::Question { others, .. } => others[0].0.clone(),
            _ => panic!("a question"),
        };
        assert!(
            field.read(cx).focus_handle(cx).is_focused(window),
            "the field has the keyboard"
        );
        window.press("escape", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert!(!ws.inbox_open));
}

/// The theme picker takes no focus: with the inbox holding the keyboard, its keys are the
/// picker's, and the request waits.
#[gpui_kit::test]
fn the_theme_pickers_keys_are_its_own_while_the_inbox_has_the_keyboard(cx: &mut TestAppContext) {
    let config = tempfile::tempdir().unwrap();
    let (handle, ws) = open_with(cx, Some(config.path().to_path_buf()));
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let answers = asks(handle, &ws, cx, &terminal, 1);
    drawn_inbox(handle, &ws, cx);
    let theme = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.theme.name.clone());
    let before = theme(cx);
    cx.update_window(handle, |_, window, cx| {
        window.press(PICKER, cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.picker.is_some(), "the picker opened")
    });
    cx.update_window(handle, |_, window, cx| {
        window.press("down", cx);
        window.press("down", cx);
        window.press("enter", cx);
    })
    .unwrap();
    assert_ne!(theme(cx), before, "the theme moved");
    ws.read_with(cx, |ws, _| assert!(ws.picker.is_none(), "chosen"));
    assert_eq!(
        answers.try_recv(),
        Err(TryRecvError::Empty),
        "the question waits"
    );
    ws.read_with(cx, |ws, _| match &ws.waits[0].what {
        Waiting::Question { form, .. } => assert_eq!(form.highlight, None),
        _ => panic!("a question"),
    });
}

/// An editor's commands are nothing in the inbox: Ctrl+W doesn't close the tab of an editor on the
/// canvas.
#[gpui_kit::test]
fn an_editors_commands_are_nothing_while_the_inbox_has_the_keyboard(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let editor = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    let tabs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.editor(editor).unwrap().tabs.len())
    };
    assert_eq!(tabs(cx), 1);
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    drawn_inbox(handle, &ws, cx);
    ws.read_with(cx, |ws, _| assert!(ws.inbox_open));
    cx.update_window(handle, |_, window, cx| window.press("ctrl-w", cx))
        .unwrap();
    assert_eq!(tabs(cx), 1, "the editor's tab stays");
}

/// Dragging the body of a selected terminal that asks moves the group, as for any block; a click
/// on its notice opens the inbox and neither focuses nor selects the terminal.
#[gpui_kit::test]
fn a_selected_asking_terminal_moves_the_group_and_its_notice_only_opens_the_inbox(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let (kanban, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    ws.update(cx, |ws, _| {
        ws.space.focused = None;
        ws.space.selected = vec![kanban, a];
    });
    frame(handle, cx);
    let rects = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            [kanban, a].map(|id| ws.space.canvas.get(id).unwrap().rect)
        })
    };
    let before = rects(cx);
    // Well inside the terminal's body, away from the notice at its bottom center.
    let body = at(&ws, cx, (100.0, 500.0));
    cx.update_window(handle, |_, window, cx| {
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            body,
            (body.0 + 60.0, body.1),
        );
    })
    .unwrap();
    for (was, now) in before.iter().zip(rects(cx)) {
        assert!(
            now.x > was.x + 30.0,
            "moved by the drag: {was:?} -> {now:?}"
        );
    }
    // The notice: the inbox opens, and the terminal is neither focused nor picked anew.
    ws.update(cx, |ws, _| {
        ws.space.focused = Some(kanban);
        ws.space.selected.clear();
    });
    click_id(handle, cx, name(format!("waiting-notice-{}", a.0)));
    ws.read_with(cx, |ws, _| {
        assert!(ws.inbox_open);
        assert_eq!(ws.space.focused, Some(kanban), "not focused");
        assert!(ws.space.selected.is_empty(), "not selected");
    });
}

/// The popup occludes what it covers: a press on a row over a block reaches the row, not the
/// block, and the inbox keeps the keyboard.
#[gpui_kit::test]
fn a_press_on_a_row_over_a_block_doesnt_reach_the_block(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    let key = listed(&ws, cx)[0];
    // A block under the list's first row (the popup is at the canvas's bottom left).
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open(Rect::new(10.0, 280.0, 300.0, 300.0), window, cx);
            ws.space.appearing.clear();
            ws.space.focused = Some(kanban);
        })
    })
    .unwrap();
    drawn_inbox(handle, &ws, cx);
    click_id(handle, cx, name(format!("inbox-row-{key}")));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.focused,
            Some(kanban),
            "the block under it heard nothing"
        );
    });
    let has_keys = cx
        .update_window(handle, |_, window, cx| {
            ws.read(cx).inbox_keys(window, cx).is_some()
        })
        .unwrap();
    assert!(has_keys, "the inbox has the keyboard");
}

/// The request whose Other field had the keyboard ends (a deadline, its agent leaving): the
/// keyboard is the inbox's again, so Esc still closes it.
#[gpui_kit::test]
fn a_request_ending_under_a_field_gives_the_keyboard_back_to_the_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let free = json!({ "questions": [{ "question": "Why?" }] });
    let _answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", free);
    drawn_inbox(handle, &ws, cx);
    let key = ws.read_with(cx, |ws, _| ws.waits[0].key);
    cx.update_window(handle, |_, window, cx| {
        window.press("tab", cx);
        window.render_frame(cx);
    })
    .unwrap();
    // Withdrawn without the person.
    ws.update(cx, |ws, cx| {
        ws.answer(key, Reply::err("No one answered"), cx)
    });
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| window.press("escape", cx))
        .unwrap();
    ws.read_with(cx, |ws, _| assert!(!ws.inbox_open, "Esc reached the inbox"));
}

/// Typing in a request's Other field goes on when something else ends meanwhile: an unrelated
/// `wait_for`, or another terminal's request.
#[gpui_kit::test]
fn a_wait_ending_elsewhere_leaves_the_field_the_keyboard(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let b = open_terminal(handle, &ws, cx, 0);
    let (e, f) = (
        open_terminal(handle, &ws, cx, 1),
        open_terminal(handle, &ws, cx, 2),
    );
    let ((terminal, _), (other, _)) = (me(&ws, cx, a), me(&ws, cx, b));
    let (watcher, _) = me(&ws, cx, e);
    let (_, watched) = me(&ws, cx, f);
    ws.update(cx, |ws, cx| assert!(ws.connect(e, f, cx)));
    let free = json!({ "questions": [{ "question": "When?" }] });
    let _answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", free);
    let first = ws.read_with(cx, |ws, _| ws.waits[0].key);
    let unrelated = watching(handle, &ws, cx, &watcher, &watched, 2);
    let _theirs = asks(handle, &ws, cx, &other, 3);
    let theirs = ws.read_with(cx, |ws, _| ws.waits.last().unwrap().key);
    drawn_inbox(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press("tab", cx);
        window.render_frame(cx);
        window.input("ship on", cx);
    })
    .unwrap();
    let typed = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, cx| match &ws.waits[0].what {
            Waiting::Question { others, .. } => others[0].0.read(cx).value().to_string(),
            _ => panic!("a question"),
        })
    };
    assert_eq!(typed(cx), "ship on");
    for (ended, more) in [(unrelated, " fri"), (theirs, "day")] {
        ws.update(cx, |ws, cx| ws.answer(ended, Reply::err("ended"), cx));
        frame(handle, cx);
        cx.update_window(handle, |_, window, cx| window.input(more, cx))
            .unwrap();
        frame(handle, cx);
    }
    assert_eq!(typed(cx), "ship on friday", "every key went to the field");
    ws.read_with(cx, |ws, _| assert_eq!(ws.waits[0].key, first));
}

/// The badge reads the count, "99+" past 99.
#[test]
fn the_badge_reads_99_at_most() {
    use crate::inbox::badge_text;
    assert_eq!(
        [1, 2, 99, 100, 120].map(badge_text),
        ["1", "2", "99", "99+", "99+"]
    );
}

#[gpui_kit::test]
fn the_badge_counts_this_workspaces_requests(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let b = open_terminal(handle, &ws, cx, 0);
    let c = open_terminal(handle, &ws, cx, 1);
    let (a_id, _) = me(&ws, cx, a);
    let (b_id, _) = me(&ws, cx, b);
    let (c_id, _) = me(&ws, cx, c);
    let badge = name("inbox-badge".into());
    assert!(!drawn(handle, cx, badge.clone()), "none: no badge");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.move_blocks(vec![c], None, window, cx))
    })
    .unwrap();
    let _here = (
        asks(handle, &ws, cx, &a_id, 1),
        asks(handle, &ws, cx, &b_id, 2),
    );
    let _elsewhere = asks(handle, &ws, cx, &c_id, 3);
    assert_eq!(listed(&ws, cx).len(), 2, "this workspace's two");
    assert!(drawn(handle, cx, badge), "the badge");
}

/// Review Focus 4: a hundred and twenty requests: the badge says 99+, the list scrolls, the
/// selection stays in view.
#[gpui_kit::test]
fn many_requests_keep_the_selection_in_view(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    for conn in 1..=120 {
        let _ = asks(handle, &ws, cx, &terminal, conn);
    }
    let keys = listed(&ws, cx);
    assert_eq!(keys.len(), 120);
    assert_eq!(crate::inbox::badge_text(keys.len()), "99+");
    assert!(drawn(handle, cx, name("inbox-badge".into())));
    let in_view = |cx: &mut TestAppContext, key: u64| {
        cx.update_window(handle, |_, window, cx| {
            for _ in 0..3 {
                window.render_frame(cx);
            }
            let list = window.find("inbox-list").bounds();
            let row = window.find(name(format!("inbox-row-{key}"))).bounds();
            row.top() >= list.top() && row.bottom() <= list.bottom()
        })
        .unwrap()
    };
    // Opened on the last one, far below the first screenful.
    open_inbox(handle, &ws, cx, Some(keys[119]));
    assert!(in_view(cx, keys[119]), "the last");
    // Then up by the keys, past the top of what shows.
    cx.update_window(handle, |_, window, cx| {
        for _ in 0..30 {
            window.press("alt-up", cx);
            window.render_frame(cx);
        }
    })
    .unwrap();
    assert_eq!(selected(&ws, cx), Some(keys[89]));
    assert!(in_view(cx, keys[89]), "30 rows up");
}

#[test]
fn the_pulse_swells_and_fades_once_a_second() {
    use crate::inbox::pulse;
    let s = Duration::from_secs_f32;
    assert!(pulse(s(0.0)) < 0.01 && pulse(s(1.0)) < 0.01);
    assert!(pulse(s(0.5)) > 0.99);
}

#[gpui_kit::test]
fn a_new_request_pulses_the_button_for_a_while(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let b = open_terminal(handle, &ws, cx, 0);
    let (terminal, _) = me(&ws, cx, a);
    let (elsewhere, _) = me(&ws, cx, b);
    let pulsing = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, cx| {
            ws.inbox_pulsing(cx.background_executor().now())
        })
    };
    assert!(!pulsing(cx), "nothing yet");
    // A request in another workspace doesn't pulse this one's button.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.move_blocks(vec![b], None, window, cx))
    })
    .unwrap();
    let _far = asks(handle, &ws, cx, &elsewhere, 1);
    assert!(!pulsing(cx), "not for the others");
    let _answers = asks(handle, &ws, cx, &terminal, 2);
    assert!(pulsing(cx), "a request here");
    after(cx, 2900);
    assert!(pulsing(cx), "for a few seconds");
    after(cx, 200);
    assert!(!pulsing(cx), "then no frame is asked for any more");
    // Frames drawn while it lasts and after: the badge stays.
    assert!(drawn(handle, cx, name("inbox-badge".into())));
}

#[gpui_kit::test]
fn with_reduced_motion_the_button_never_pulses(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    no_motion(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    ws.read_with(cx, |ws, _| assert_eq!(ws.inbox_pulse, None));
    assert!(
        drawn(handle, cx, name("inbox-badge".into())),
        "the badge only"
    );
}

#[gpui_kit::test]
fn the_toggle_shows_the_count_when_the_sidebar_is_hidden(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    let (button, toggle) = (name("inbox-button".into()), name("sidebar-badge".into()));
    ws.update(cx, |ws, _| ws.sidebar = true);
    assert!(drawn(handle, cx, button.clone()));
    assert!(
        !drawn(handle, cx, toggle.clone()),
        "the sidebar has the badge"
    );
    ws.update(cx, |ws, _| ws.sidebar = false);
    assert!(!drawn(handle, cx, button), "no sidebar, no button");
    assert!(drawn(handle, cx, toggle), "the toggle has it");
}

#[gpui_kit::test]
fn the_button_toggles_the_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.sidebar = true);
    for open in [true, false] {
        click_id(handle, cx, name("inbox-button".into()));
        ws.read_with(cx, |ws, _| assert_eq!(ws.inbox_open, open));
    }
}

/// A request here with the window in front is told by the badge alone, in view or not; another
/// workspace's is a notification, whose click shows that workspace with its inbox open at it.
#[gpui_kit::test]
fn a_request_here_with_the_window_active_isnt_notified(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let b = open_terminal(handle, &ws, cx, 0);
    let ((terminal, _), (elsewhere, _)) = (me(&ws, cx, a), me(&ws, cx, b));
    super::terminals::set_active(handle, true, cx);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(a).unwrap().rect.x += 100_000.0;
        ws.space.focused = None;
    });
    let before = notification_count(handle, cx);
    let _here = asks(handle, &ws, cx, &terminal, 1);
    assert_eq!(
        notification_count(handle, cx),
        before,
        "here: the badge tells"
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.terminal(a).unwrap().alert,
            Some(crate::alert::Alert::Attention(Some(
                "Asks you: Ship it?".into()
            ))),
            "the block keeps the alert for the workspace's dot"
        )
    });
    // Another workspace's.
    let there = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.move_blocks(vec![b], None, window, cx);
                ws.others.last().unwrap().id.clone()
            })
        })
        .unwrap();
    // The move said so itself.
    after(cx, 600);
    frame(handle, cx);
    let before = notification_count(handle, cx);
    let _far = asks(handle, &ws, cx, &elsewhere, 2);
    assert_eq!(
        notification_count(handle, cx),
        before + 1,
        "there: a notification"
    );
    let key = ws.read_with(cx, |ws, _| ws.waits.last().unwrap().key);
    // Its click.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.show_request(&there, key, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, there, "that workspace");
        assert!(ws.inbox_open, "its inbox");
    });
    assert_eq!(selected(&ws, cx), Some(key), "at that request");
}

#[gpui_kit::test]
fn go_to_waiting_opens_the_inbox_when_a_request_waits(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let go = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.go_to_waiting(window, cx))
        })
        .unwrap()
    };
    go(cx);
    ws.read_with(cx, |ws, _| {
        assert!(!ws.inbox_open, "nothing waits: today's")
    });
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    go(cx);
    ws.read_with(cx, |ws, _| assert!(ws.inbox_open, "a request waits"));
}

/// A long preview wraps and shows twelve lines; Show more shows the rest, Show less cuts it again.
#[gpui_kit::test]
fn a_long_preview_wraps_and_shows_more_on_demand(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let tall: Vec<String> = (0..40).map(|i| format!("line {i}")).collect();
    let asked = json!({ "questions": [{ "question": "Which?", "options": [
        { "label": "Long", "preview": tall.join("\n") }, { "label": "Short" }
    ] }] });
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    ws.update(cx, |ws, _| {
        ws.git_size = Some(templum_canvas::Size::new(1100.0, 760.0))
    });
    drawn_inbox(handle, &ws, cx);
    let height = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            f32::from(window.find(part(a, "q0-preview")).bounds().size.height)
        })
        .unwrap()
    };
    let more = part(a, "q0-preview-more");
    // The link is under the fold of the request's body.
    let down = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.waits[0].body.handle.scroll_to_bottom());
        frame(handle, cx);
    };
    // 12 lines of 16, its padding of 8 above and below, its 1 px frame.
    assert!(height(cx) <= 12.0 * 16.0 + 16.0 + 2.0, "{}", height(cx));
    assert!(drawn(handle, cx, more.clone()), "Show more");
    down(cx);
    click_id(handle, cx, more.clone());
    assert!(height(cx) >= 40.0 * 16.0, "all 40: {}", height(cx));
    down(cx);
    click_id(handle, cx, more.clone());
    assert!(height(cx) <= 12.0 * 16.0 + 16.0 + 2.0, "cut again");
}

#[gpui_kit::test]
fn a_long_line_of_a_preview_wraps_and_a_short_preview_has_no_link(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let line = "word ".repeat(40);
    let asked = json!({ "questions": [{ "question": "Which?", "options": [
        { "label": "Wide", "preview": line.trim() }
    ] }] });
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    drawn_inbox(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let preview = window.find(part(a, "q0-preview")).bounds();
        let column = window.find(part(a, "body")).bounds();
        assert!(preview.right() <= column.right(), "no sideways scroll");
        assert!(preview.size.height > px(16.0 * 3.0), "wrapped: {preview:?}");
        assert!(window.try_find(part(a, "q0-preview-more")).is_none());
    })
    .unwrap();
}

/// A request missed in the workspace shown is found from another: its tile has the dot and
/// Ctrl+Shift+A goes to its block.
#[gpui_kit::test]
fn a_request_missed_here_is_found_from_another_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    super::terminals::set_active(handle, true, cx);
    let here = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    ws.update(cx, |ws, _| ws.space.focused = None);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_ne!(ws.space.id, here, "elsewhere now"));
    assert!(
        drawn(handle, cx, name(format!("workspace-alert-{here}"))),
        "the workspace's dot"
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.go_to_waiting(window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, here, "back where it waits");
        assert_eq!(ws.space.focused, Some(a));
    });
}

/// The click on a request's notification shows the workspace its block is in now.
#[gpui_kit::test]
fn a_notification_shows_the_workspace_the_block_moved_to(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let b = open_terminal(handle, &ws, cx, 0);
    let (terminal, _) = me(&ws, cx, b);
    let _ = a;
    let before = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let _answers = asks(handle, &ws, cx, &terminal, 1);
    let key = ws.read_with(cx, |ws, _| ws.waits[0].key);
    // The block moves on, to a workspace of its own, after the notification was made.
    let there = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.move_blocks(vec![b], None, window, cx);
                ws.others.last().unwrap().id.clone()
            })
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.show_request(&before, key, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, there, "where the block is");
        assert!(ws.inbox_open);
    });
    assert_eq!(selected(&ws, cx), Some(key), "listing the request");
}

/// A preview the estimate takes for short is never cut: wrapped, it is taller than 12 lines, and
/// it is shown whole, with no link. (The estimate is made to err: letters of one pixel.)
#[gpui_kit::test]
fn a_preview_the_estimate_calls_short_is_shown_whole(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    // 12 lines of 150 characters: 3 rows each on screen, though the estimate counts 1.
    let line = "word ".repeat(30);
    let text: Vec<&str> = (0..12).map(|_| line.trim()).collect();
    let asked = json!({ "questions": [{ "question": "Which?", "options": [
        { "label": "Wide", "preview": text.join("\n") }
    ] }] });
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    ws.update(cx, |ws, _| ws.cell.size.w = 1.0);
    drawn_inbox(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let preview = window.find(part(a, "q0-preview")).bounds();
        // 12 lines of 16 is where a cut would be.
        assert!(
            preview.size.height > px(12.0 * 16.0 + 16.0 + 2.0 + 16.0),
            "{preview:?}"
        );
        assert!(window.try_find(part(a, "q0-preview-more")).is_none());
    })
    .unwrap();
}

/// A request that is withdrawn (its deadline, its agent leaving) takes its alert with it: the
/// block's dot and Ctrl+Shift+A don't point at what no longer waits. Another alert stays.
#[gpui_kit::test]
fn a_withdrawn_request_takes_its_alert_with_it(cx: &mut TestAppContext) {
    use crate::alert::Alert;
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let alert = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(a).unwrap().alert.clone())
    };
    // Its deadline.
    let asked = json!({ "question": "Ship it?", "options": ["Yes"], "timeout": 1 });
    let _timed = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    assert_eq!(
        alert(cx),
        Some(Alert::Attention(Some("Asks you: Ship it?".into())))
    );
    after(cx, 1100);
    assert!(listed(&ws, cx).is_empty(), "withdrawn");
    assert_eq!(alert(cx), None, "its alert went");
    // Its agent leaves; a bell rung meanwhile stays.
    let _gone = asks(handle, &ws, cx, &terminal, 2);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a).unwrap().alert = Some(Alert::Attention(Some("a bell".into())))
    });
    bus_note(
        handle,
        &ws,
        cx,
        2,
        templum_mcp::bus::CLOSED,
        serde_json::Value::Null,
    );
    assert!(listed(&ws, cx).is_empty());
    assert_eq!(
        alert(cx),
        Some(Alert::Attention(Some("a bell".into()))),
        "not the request's"
    );
}

/// Answering one of a terminal's two requests leaves the alert for the other: after leaving the
/// workspace its tile still has the dot.
#[gpui_kit::test]
fn the_request_left_keeps_the_dot_of_its_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, a) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, a);
    let here = ws.read_with(cx, |ws, _| ws.space.id.clone());
    super::terminals::set_active(handle, true, cx);
    let _first = asks(handle, &ws, cx, &terminal, 1);
    let second = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "ask_user",
        json!({ "question": "Which?", "options": ["A"] }),
    );
    let keys = listed(&ws, cx);
    ws.update(cx, |ws, cx| ws.answer(keys[0], Reply::ok("Yes"), cx));
    ws.update(cx, |ws, _| {
        ws.seen_asking(&terminal);
        ws.space.focused = None;
    });
    assert_eq!(second.try_recv(), Err(TryRecvError::Empty));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.terminal(a).unwrap().alert,
            Some(crate::alert::Alert::Attention(Some(
                "Asks you: Which?".into()
            ))),
            "the other request's words"
        )
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx))
    })
    .unwrap();
    assert!(drawn(handle, cx, name(format!("workspace-alert-{here}"))));
}
