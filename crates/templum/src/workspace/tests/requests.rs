//! Requests that wait for the person: an agent's question (`ask_user`), its request to connect,
//! and the balloon they open (docs/specs/connections.md, Requests from agents).

use std::sync::mpsc::TryRecvError;

use gpui_kit::Focusable as _;
use gpui_kit::component::input::InputState;
use serde_json::{Value, json};

use super::bus::{after, answered, bus_note, call, call_later, event, me};
use super::*;
use crate::workspace::Waiting;

/// The newest wait's key.
fn last_wait(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> u64 {
    ws.read_with(cx, |ws, _| ws.waits.last().expect("a wait").key)
}

/// What the dot before terminal `id`'s title says.
fn dot_text(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> Option<String> {
    ws.read_with(cx, |ws, _| {
        crate::block::status_dot(ws, ws.space.terminal(id).unwrap())
            .map(|(_, text)| text.to_string())
    })
}

/// The person picks option `i` of question `q` of the question `key`.
fn pick(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    key: u64,
    q: usize,
    i: usize,
) {
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.pick_option(key, q, i, window, cx))
    })
    .unwrap();
}

/// The person presses Send on the question `key`, drawn (`shown`) before.
fn send(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, key: u64) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.send_question(key, window, cx))
    })
    .unwrap();
}

/// The inbox drawn, and long enough ago for presses to count (`TOO_SOON`).
fn shown(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        inbox_open(window, cx);
        window.render_frame(cx)
    })
    .unwrap();
    after(cx, 600);
}

/// The field of the question `key`: question `q`'s Other field, or with None its note.
fn field_of(ws: &Workspace, key: u64, field: Option<usize>) -> Entity<InputState> {
    let wait = ws.waits.iter().find(|w| w.key == key).expect("the wait");
    match (&wait.what, field) {
        (Waiting::Question { others, .. }, Some(q)) => others[q].0.clone(),
        (
            Waiting::Question {
                note: Some(note), ..
            },
            None,
        ) => note.0.clone(),
        _ => panic!("no such field"),
    }
}

/// The person types `text` in a field of the question `key` (`field_of`), the dialog drawn.
fn type_into(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    key: u64,
    field: Option<usize>,
    text: &str,
) {
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        let input = field_of(ws.read(cx), key, field);
        input.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        window.input(text, cx);
    })
    .unwrap();
}

/// Clicks the request part `what` of block `n` in the inbox, drawn first and scrolled into view.
fn click_part(handle: AnyWindowHandle, cx: &mut TestAppContext, n: usize, what: &str) {
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        scroll_to(window, cx, n, what);
        window.click(crate::balloon::part(n, what), cx);
    })
    .unwrap();
}

/// Scrolls the body of the request of block `n` until its element `what` shows: the body scrolls
/// over the foot, which stays.
fn scroll_to(window: &mut Window, cx: &mut gpui_kit::App, n: usize, what: &str) {
    window.render_frame(cx);
    let target = window.find(crate::balloon::part(n, what)).bounds();
    let balloon = window.find(crate::balloon::part(n, "body")).bounds();
    // In the foot, which stays in the popup: nothing to scroll. In the body, a part out of view
    // lies past the body's edge.
    let frame = window.find("inbox").bounds();
    if frame.contains(&target.origin)
        && frame.contains(&target.bottom_right())
        && (target.top() >= balloon.bottom() || target.bottom() <= balloon.top())
    {
        return;
    }
    let by = if target.bottom() > balloon.bottom() {
        balloon.bottom() - target.bottom() - px(8.0)
    } else if target.top() < balloon.top() {
        balloon.top() - target.top() + px(8.0)
    } else {
        return;
    };
    window.hover("inbox", cx);
    // At its left edge: a preview pane, on the right, keeps its own wheel.
    let at = (
        f32::from(balloon.left()) + 4.0,
        f32::from(balloon.center().y),
    );
    wheel(
        window,
        cx,
        at,
        ScrollDelta::Pixels(point(px(0.0), by)),
        false,
    );
    window.render_frame(cx);
}

/// Whether the inbox draws the request part `what` of block `n`.
fn drawn(handle: AnyWindowHandle, cx: &mut TestAppContext, n: usize, what: &str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        inbox_open(window, cx);
        window.render_frame(cx);
        window.try_find(crate::balloon::part(n, what)).is_some()
    })
    .unwrap()
}

/// The spec's call: two questions, the second a multiSelect.
fn two_questions() -> Value {
    json!({
        "questions": [
            {
                "question": "Which database should the service use?",
                "header": "Database",
                "options": [
                    { "label": "Postgres", "description": "What production runs",
                      "preview": "DATABASE_URL=postgres://…\npool = 10" },
                    { "label": "SQLite", "description": "One file, no server" }
                ]
            },
            {
                "question": "Which features go in the first release?",
                "header": "Features",
                "options": [{ "label": "Auth" }, { "label": "Search" }, { "label": "Export" }],
                "multiSelect": true
            }
        ]
    })
}

/// The JSON of an answer.
fn json_of(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| panic!("JSON: {text}"))
}

#[gpui_kit::test]
fn the_balloon_shows_the_oldest_request_and_counts_them(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    for (conn, question) in [(1, "One?"), (2, "Two?"), (3, "Three?")] {
        let asked = json!({ "question": question, "options": ["Yes"] });
        call_later(handle, &ws, cx, &terminal, conn, "ask_user", asked);
    }
    let shown = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.asking(&terminal)
                .map(|(wait, count)| (wait.summary(), count))
        })
    };
    assert_eq!(shown(cx), Some(("One?".to_string(), 3)));
    // The one shown is cancelled: it stays, its answer to be typed back.
    bus_note(
        handle,
        &ws,
        cx,
        1,
        "notifications/cancelled",
        json!({ "requestId": 1 }),
    );
    assert_eq!(shown(cx), Some(("One?".to_string(), 3)));
    // Its agent leaves: withdrawn, the next comes, counted again.
    bus_note(handle, &ws, cx, 1, templum_mcp::bus::CLOSED, Value::Null);
    assert_eq!(shown(cx), Some(("Two?".to_string(), 2)));
}

#[gpui_kit::test]
fn ask_user_waits_for_the_persons_answer(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let asked = json!({ "question": "Ship it?\nThe tests pass.", "options": ["Yes", "No"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    assert!(answers.try_recv().is_err(), "it waits for the person");
    assert_eq!(
        dot_text(&ws, cx, first).as_deref(),
        Some("Waiting for you: Ship it?")
    );
    let key = last_wait(&ws, cx);
    pick(handle, &ws, cx, key, 0, 0);
    assert!(answers.try_recv().is_err(), "a pick is not an answer");
    send(handle, &ws, cx, key);
    assert_eq!(answered(&answers), (false, "Yes".to_string()));
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
    assert_eq!(dot_text(&ws, cx, first), None);
}

#[gpui_kit::test]
fn a_question_without_options_takes_what_the_person_types(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let asked = json!({ "question": "When?" });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let key = last_wait(&ws, cx);
    let send = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.send_question(key, window, cx))
        })
        .unwrap();
    };
    type_into(handle, &ws, cx, key, Some(0), "   ");
    send(cx);
    assert!(answers.try_recv().is_err(), "a blank answer isn't sent");
    type_into(handle, &ws, cx, key, Some(0), "Friday");
    send(cx);
    assert_eq!(answered(&answers), (false, "Friday".to_string()));
}

#[gpui_kit::test]
fn a_question_no_one_answers_is_withdrawn_at_its_timeout(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let asked = json!({ "question": "Still there?", "options": ["Yes"], "timeout": 1 });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    after(cx, 900);
    assert!(answers.try_recv().is_err(), "not yet");
    after(cx, 200);
    let (failed, text) = answered(&answers);
    assert!(failed && text.contains("withdrawn"), "{text}");
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
}

#[gpui_kit::test]
fn an_answer_and_its_timeout_answer_once(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let asked = json!({ "question": "Now?", "options": ["Yes"], "timeout": 1 });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let key = last_wait(&ws, cx);
    pick(handle, &ws, cx, key, 0, 0);
    send(handle, &ws, cx, key);
    after(cx, 2000);
    assert_eq!(answered(&answers), (false, "Yes".to_string()));
    assert!(answers.try_recv().is_err(), "answered once");
}

/// The asking terminal runs an agent that doesn't take notices yet: a notice to it waits in
/// `Workspace::notices`, to be read.
fn agent_without_notices(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) {
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(id).unwrap().seen.agent = Some("claude".into())
    });
}

/// The notices waiting to be typed: to whom, and what.
fn notices(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<(String, String)> {
    ws.read_with(cx, |ws, _| {
        ws.notices
            .iter()
            .map(|n| (n.to.clone(), n.text.clone()))
            .collect()
    })
}

/// The oldest wait is a question its client stopped waiting for.
fn stopped(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    ws.read_with(cx, |ws, _| {
        matches!(
            ws.waits.first().map(|w| &w.what),
            Some(Waiting::Question { stopped: true, .. })
        )
    })
}

#[gpui_kit::test]
fn a_cancelled_question_stays_and_its_answer_is_typed_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    agent_without_notices(&ws, cx, mine);
    let n = mine.0 as usize;
    let question = json!({ "question": "Ship it?", "options": ["Yes", "No"] });
    let first = call_later(handle, &ws, cx, &terminal, 3, "ask_user", question.clone());
    let _second = call_later(handle, &ws, cx, &terminal, 4, "ask_user", question);
    // Request 3 of another connection is another call.
    bus_note(
        handle,
        &ws,
        cx,
        4,
        "notifications/cancelled",
        json!({ "requestId": 3 }),
    );
    assert!(!stopped(&ws, cx));
    assert!(!drawn(handle, cx, n, "stopped"));
    let cancel = json!({ "requestId": 3, "reason": "The person pressed Esc" });
    bus_note(handle, &ws, cx, 3, "notifications/cancelled", cancel);
    ws.read_with(cx, |ws, _| assert_eq!(ws.waits.len(), 2));
    assert!(stopped(&ws, cx));
    assert!(drawn(handle, cx, n, "stopped"), "the dialog says so");
    assert_eq!(first.try_recv(), Err(TryRecvError::Empty));
    click_part(handle, cx, n, "q0-option-0");
    click_part(handle, cx, n, "send");
    assert_eq!(
        notices(&ws, cx),
        [(
            terminal.clone(),
            "[answer to your question \"Ship it?\"] The answer is in the pasted text.\nYes"
                .to_string()
        )]
    );
    ws.read_with(cx, |ws, _| assert_eq!(ws.waits.len(), 1));
    assert_eq!(
        first.try_recv(),
        Err(TryRecvError::Disconnected),
        "MCP sends no answer to a cancelled call"
    );
}

#[gpui_kit::test]
fn dismissing_a_stopped_question_types_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    agent_without_notices(&ws, cx, mine);
    let n = mine.0 as usize;
    let asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    bus_note(
        handle,
        &ws,
        cx,
        1,
        "notifications/cancelled",
        json!({ "requestId": 1 }),
    );
    click_part(handle, cx, n, "dismiss");
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
    assert!(notices(&ws, cx).is_empty());
    assert_eq!(asked.try_recv(), Err(TryRecvError::Disconnected));
}

/// A cancelled question whose agent quits, or whose block closes, is withdrawn: nothing is typed,
/// nothing answered.
#[gpui_kit::test]
fn a_stopped_question_goes_with_its_agent(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    agent_without_notices(&ws, cx, mine);
    let asked = json!({ "question": "Ship it?", "options": ["Yes"] });
    let quits = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked.clone());
    bus_note(
        handle,
        &ws,
        cx,
        1,
        "notifications/cancelled",
        json!({ "requestId": 1 }),
    );
    bus_note(handle, &ws, cx, 1, templum_mcp::bus::CLOSED, Value::Null);
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
    assert!(notices(&ws, cx).is_empty());
    assert_eq!(quits.try_recv(), Err(TryRecvError::Disconnected));
    let closes = call_later(handle, &ws, cx, &terminal, 2, "ask_user", asked);
    bus_note(
        handle,
        &ws,
        cx,
        2,
        "notifications/cancelled",
        json!({ "requestId": 2 }),
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(mine, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
    assert!(notices(&ws, cx).is_empty());
    assert_eq!(
        closes.try_recv(),
        Err(TryRecvError::Disconnected),
        "no answer to a cancelled call"
    );
}

#[gpui_kit::test]
fn a_stopped_questions_deadline_drops_it_silently(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    agent_without_notices(&ws, cx, mine);
    let asked = json!({ "question": "Ship it?", "options": ["Yes"], "timeout": 1 });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    bus_note(
        handle,
        &ws,
        cx,
        1,
        "notifications/cancelled",
        json!({ "requestId": 1 }),
    );
    after(cx, 1100);
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
    assert!(notices(&ws, cx).is_empty());
    assert_eq!(answers.try_recv(), Err(TryRecvError::Disconnected));
}

#[gpui_kit::test]
fn an_agent_that_quits_takes_its_questions_away(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let ask = |text: &str| json!({ "question": text, "options": ["Yes"] });
    let _a = call_later(handle, &ws, cx, &terminal, 5, "ask_user", ask("One?"));
    let _b = call_later(handle, &ws, cx, &terminal, 5, "ask_user", ask("Two?"));
    let _c = call_later(handle, &ws, cx, &terminal, 6, "ask_user", ask("Three?"));
    bus_note(handle, &ws, cx, 5, templum_mcp::bus::CLOSED, Value::Null);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.waits.len(), 1);
        assert_eq!(ws.waits[0].conn, 6);
    });
    assert_eq!(
        dot_text(&ws, cx, first).as_deref(),
        Some("Waiting for you: Three?")
    );
}

#[gpui_kit::test]
fn closing_the_asking_block_withdraws_its_question(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let asked = json!({ "question": "Ship it?", "options": ["Yes"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(first, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    let (failed, text) = answered(&answers);
    assert!(failed && text.contains("closed"), "{text}");
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
}

/// The notification counts the call's other questions; it says the same as the alert.
#[gpui_kit::test]
fn a_notification_counts_the_other_questions(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    super::terminals::set_active(handle, false, cx);
    ws.update(cx, |ws, _| {
        ws.space.focused = None;
        ws.space.canvas.get_mut(mine).unwrap().rect.x += 100_000.0;
    });
    let before = notification_count(handle, cx);
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    assert_eq!(notification_count(handle, cx), before + 1);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().alert.clone()),
        Some(crate::alert::Alert::Attention(Some(
            "Asks you: Which database should the service use? (and 1 more)".into()
        )))
    );
}

/// A sound for a question or a request to connect, seen or not, at most once every 2 seconds.
#[gpui_kit::test]
fn a_request_chimes_once_every_two_seconds(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let chimed = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.last_chime);
    assert_eq!(chimed(cx), None);
    let asked = json!({ "question": "Ship it?", "options": ["Yes"] });
    let _one = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked.clone());
    let first = chimed(cx).expect("a sound");
    let _two = call_later(handle, &ws, cx, &terminal, 2, "ask_user", asked);
    assert_eq!(chimed(cx), Some(first), "not twice within 2 s");
    after(cx, 2100);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let _connect = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        3,
        "connect",
        json!({ "block": k }),
    );
    assert!(
        chimed(cx).is_some_and(|at| at > first),
        "a request to connect too"
    );
}

#[gpui_kit::test]
fn no_chime_with_the_sound_off(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    ws.update(cx, |ws, _| ws.config.connections.sound = false);
    let asked = json!({ "question": "Ship it?", "options": ["Yes"] });
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    ws.read_with(cx, |ws, _| assert_eq!(ws.last_chime, None));
}

/// The person doesn't see a question when the window is in the background: a notification says
/// what it asks. In front, zoomed out or not, the inbox's badge tells.
#[gpui_kit::test]
fn a_question_in_a_window_in_the_background_is_a_notification(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    super::terminals::set_active(handle, true, cx);
    // Zoomed out: the block is drawn as bars.
    ws.update(cx, |ws, _| {
        ws.space.focused = None;
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 0.33);
    });
    frame(handle, cx);
    let before = notification_count(handle, cx);
    let asked = json!({ "question": "Far?", "options": ["Yes"] });
    let _far = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    assert_eq!(
        notification_count(handle, cx),
        before,
        "the window is in front"
    );
    super::terminals::set_active(handle, false, cx);
    let asked = json!({ "question": "Away?", "options": ["Yes"] });
    let _away = call_later(handle, &ws, cx, &terminal, 2, "ask_user", asked);
    assert_eq!(notification_count(handle, cx), before + 1);
}

/// Claude Code asks its person to allow a Templum tool, then calls it: the person allowed it, and
/// the agent works again, though the tool may wait for minutes before its end says so.
#[gpui_kit::test]
fn a_templum_tool_the_person_allowed_works_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let hook =
        json!({ "hook_event_name": "PermissionRequest", "tool_name": "mcp__templum__ask_user" });
    let asks = serde_json::to_value(templum_mcp::hooks::claude(&hook).unwrap()).unwrap();
    event(handle, &ws, cx, &terminal, asks);
    let state = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(first).unwrap().state.clone())
    };
    let asking = state(cx);
    assert!(
        matches!(asking, Some(crate::workspace::AgentState::NeedsYou(_))),
        "{asking:?}"
    );
    // Another tool's call: the dialog may be another one's of its subagents.
    call_later(handle, &ws, cx, &terminal, 1, "whoami", json!({}));
    assert_eq!(state(cx), asking);
    let asked = json!({ "question": "Ship it?", "options": ["Yes"] });
    let _answer = call_later(handle, &ws, cx, &terminal, 2, "ask_user", asked);
    assert_eq!(state(cx), Some(crate::workspace::AgentState::Working));
}

/// Claude Code's own notification about the same dialog comes about six seconds after it opens:
/// the dot keeps its "Allow …?", and allowing the tool afterwards still makes it work again.
#[gpui_kit::test]
fn a_templum_tool_allowed_after_its_notification_works_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let hook = |input: Value| serde_json::to_value(templum_mcp::hooks::claude(&input)).unwrap();
    let tool = "mcp__templum__wait_for_idle";
    let asks = json!({ "hook_event_name": "PermissionRequest", "tool_name": tool });
    event(handle, &ws, cx, &terminal, hook(asks));
    let state = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.terminal(first).unwrap().state.clone())
    };
    let asking = state(cx);
    let notified = json!({
        "hook_event_name": "Notification",
        "notification_type": "permission_prompt",
        "message": format!("Claude needs your permission to use {tool}"),
    });
    event(handle, &ws, cx, &terminal, hook(notified.clone()));
    assert_eq!(state(cx), asking);
    let waits = json!({ "block": "k9x1d" });
    call_later(handle, &ws, cx, &terminal, 1, "wait_for_idle", waits);
    assert_eq!(state(cx), Some(crate::workspace::AgentState::Working));
    // The notification alone (its `PermissionRequest` hook didn't run) still says it waits.
    event(handle, &ws, cx, &terminal, hook(notified));
    assert_eq!(
        state(cx),
        Some(crate::workspace::AgentState::NeedsYou(None))
    );
}

/// A block in another workspace asks: the person can't see it, though its id there is the one of a
/// block in view here (ids count from 0 in each workspace).
#[gpui_kit::test]
fn a_question_from_another_workspace_is_a_notification(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let there = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.move_blocks(vec![note], None, window, cx);
                let there = ws.others.last().unwrap().id.clone();
                ws.move_blocks(vec![mine], Some(&there), window, cx);
                there
            })
        })
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.caller(&terminal),
            Some((there.clone(), kanban)),
            "the kanban's id here"
        )
    });
    let before = notification_count(handle, cx);
    let asked = json!({ "question": "Here?", "options": ["Yes"] });
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    assert_eq!(notification_count(handle, cx), before + 1);
    // Its tile's dot lights, and Ctrl+Shift+A goes there.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, _| ws.sidebar = true);
        window.render_frame(cx);
        let dot = format!("workspace-alert-{there}");
        assert!(
            window
                .try_find(gpui_kit::ElementId::Name(dot.into()))
                .is_some()
        );
        ws.update(cx, |ws, cx| ws.go_to_waiting(window, cx));
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, there);
        assert_eq!(
            ws.space.focused,
            Some(kanban),
            "the terminal, by its id there"
        );
    });
}

#[gpui_kit::test]
fn allow_wires_the_blocks_and_deny_refuses(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let (k, n) = ws.read_with(cx, |ws, _| {
        (ws.space.uids[&kanban].clone(), ws.space.uids[&note].clone())
    });
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "connect",
        json!({ "block": k }),
    );
    assert!(answers.try_recv().is_err(), "it waits for the person");
    assert_eq!(
        dot_text(&ws, cx, mine),
        Some(format!("Waiting for you: connect to {k}"))
    );
    let key = last_wait(&ws, cx);
    ws.update(cx, |ws, cx| ws.answer_allow(key, true, cx));
    assert_eq!(answered(&answers), (false, format!("Connected to {k}.")));
    ws.read_with(cx, |ws, _| assert!(ws.space.wired(mine, kanban)));
    // Wired as by hand: Ctrl+Z takes it back.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| assert!(ws.undo_canvas(false, window, cx)))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert!(!ws.space.wired(mine, kanban)));
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "connect",
        json!({ "block": n }),
    );
    let key = last_wait(&ws, cx);
    ws.update(cx, |ws, cx| ws.answer_allow(key, false, cx));
    let (failed, text) = answered(&answers);
    assert!(failed && text.contains("didn't connect you"), "{text}");
    ws.read_with(cx, |ws, _| assert!(!ws.space.wired(mine, note)));
}

/// The block asked for moved to another workspace while the person was asked: Allow can't wire
/// it from here.
#[gpui_kit::test]
fn allow_refuses_a_block_moved_to_another_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let ask = json!({ "block": k });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "connect", ask);
    let key = last_wait(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.move_blocks(vec![kanban], None, window, cx))
    })
    .unwrap();
    ws.update(cx, |ws, cx| ws.answer_allow(key, true, cx));
    let (failed, text) = answered(&answers);
    assert!(failed && text.contains("isn't in your workspace"), "{text}");
    ws.read_with(cx, |ws, _| {
        assert!(ws.spaces().all(|s| s.wires.is_empty()), "nothing wired")
    });
}

#[gpui_kit::test]
fn connect_answers_at_once_when_there_is_nothing_to_ask(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, mine) = three_blocks(handle, &ws, cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let (k, n) = ws.read_with(cx, |ws, _| {
        (ws.space.uids[&kanban].clone(), ws.space.uids[&note].clone())
    });
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "connect",
        json!({ "block": uid }),
    );
    assert!(failed && text.contains("your own block"), "{text}");
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "connect",
        json!({ "block": "zzzzz" }),
    );
    assert!(failed && text.contains("no block zzzzz"), "{text}");
    ws.update(cx, |ws, cx| assert!(ws.connect(mine, kanban, cx)));
    let (failed, text) = call(handle, &ws, cx, &terminal, "connect", json!({ "block": k }));
    assert!(!failed && text.contains("already connected"), "{text}");
    // A second request for one block while the first waits.
    let _first = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "connect",
        json!({ "block": n }),
    );
    let second = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "connect",
        json!({ "block": n }),
    );
    let (failed, text) = answered(&second);
    assert!(failed && text.contains("already asked"), "{text}");
}

#[gpui_kit::test]
fn closing_the_block_asked_for_withdraws_the_request(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "connect",
        json!({ "block": k }),
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.remove_block(kanban, window, cx))
    })
    .unwrap();
    frame(handle, cx);
    let (failed, text) = answered(&answers);
    assert!(failed && text.contains("was closed"), "{text}");
}

#[gpui_kit::test]
fn the_balloons_buttons_answer_a_question_then_a_request(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let n = mine.0 as usize;
    let part = crate::balloon::part;
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "Not yet"] });
    let question = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let request = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "connect",
        json!({ "block": k }),
    );
    let (first, second) = ws.read_with(cx, |ws, _| (ws.waits[0].key, ws.waits[1].key));
    let row = |key: u64| gpui_kit::ElementId::Name(format!("inbox-row-{key}").into());
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(row(first)).is_some(), "listed");
        assert!(window.try_find(row(second)).is_some(), "listed");
        window.click(part(n, "q0-option-1"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        question.try_recv(),
        Err(TryRecvError::Empty),
        "Send answers"
    );
    cx.update_window(handle, |_, window, cx| window.click(part(n, "send"), cx))
        .unwrap();
    assert_eq!(answered(&question), (false, "Not yet".to_string()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(row(first)).is_none(), "answered");
        assert!(window.try_find(row(second)).is_some(), "the last one");
        window.click(part(n, "allow"), cx);
    })
    .unwrap();
    assert_eq!(answered(&request), (false, format!("Connected to {k}.")));
    ws.read_with(cx, |ws, _| assert!(ws.space.wired(mine, kanban)));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("inbox-empty").is_some(),
            "nothing left to ask"
        );
    })
    .unwrap();
}

/// With the terminal in a selection, its balloon still takes its presses: the selected body's
/// group move leaves them alone.
#[gpui_kit::test]
fn the_balloons_buttons_answer_while_the_terminal_is_selected(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let n = mine.0 as usize;
    let request = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "connect",
        json!({ "block": k }),
    );
    shown(handle, cx);
    ws.update(cx, |ws, _| {
        ws.space.focused = None;
        ws.space.selected = vec![kanban, mine];
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(crate::balloon::part(n, "allow"), cx);
    })
    .unwrap();
    assert_eq!(answered(&request), (false, format!("Connected to {k}.")));
    ws.read_with(cx, |ws, _| assert!(ws.space.wired(mine, kanban)));
}

/// Far away the terminal is drawn without its balloon, so the selection's body keeps the press:
/// a drag on it moves the group.
#[gpui_kit::test]
fn a_far_selected_terminal_with_a_pending_request_moves_the_group(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let _request = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "connect",
        json!({ "block": k }),
    );
    shown(handle, cx);
    ws.update(cx, |ws, _| {
        // Closed: its backdrop would take the press.
        ws.inbox_open = false;
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 0.3);
        ws.space.flight = None;
        ws.space.focused = None;
        ws.space.selected = vec![kanban, mine];
    });
    frame(handle, cx);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window
                .try_find(gpui_kit::ElementId::Name(
                    format!("waiting-notice-{}", mine.0).into()
                ))
                .is_none()
        );
    })
    .unwrap();
    let rects = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            [kanban, mine].map(|id| ws.space.canvas.get(id).unwrap().rect)
        })
    };
    let before = rects(&ws, cx);
    // Well below the far label, in the terminal's body: 0.3 × (40 + 200, 400 + 150).
    let body = at(&ws, cx, (72.0, 165.0));
    cx.update_window(handle, |_, window, cx| {
        press_with(
            window,
            cx,
            MouseButton::Left,
            Modifiers::default(),
            body,
            (body.0 + 30.0, body.1),
        );
    })
    .unwrap();
    let after = rects(&ws, cx);
    for (was, now) in before.iter().zip(&after) {
        assert!(
            now.x > was.x + 50.0,
            "moved by the drag: {was:?} -> {now:?}"
        );
    }
}

/// A double click answers one request: once the question is answered, the next request's buttons
/// are drawn under the pointer, and the second click mustn't choose for the person.
#[gpui_kit::test]
fn a_double_click_answers_one_request(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let n = mine.0 as usize;
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "Not yet"] });
    let question = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let request = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "connect",
        json!({ "block": k }),
    );
    let at = |window: &Window, what: &str| {
        let c = window.find(crate::balloon::part(n, what)).bounds().center();
        (f32::from(c.x), f32::from(c.y))
    };
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let yes = at(window, "q0-option-0");
        click(window, cx, yes, 1);
        window.render_frame(cx);
        let send = at(window, "send");
        click(window, cx, send, 1);
        window.render_frame(cx);
        let allow = at(window, "allow");
        click(window, cx, allow, 2);
    })
    .unwrap();
    assert_eq!(answered(&question), (false, "Yes".to_string()));
    assert_eq!(request.try_recv(), Err(TryRecvError::Empty), "it waits");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let allow = at(window, "allow");
        click(window, cx, allow, 1);
    })
    .unwrap();
    assert_eq!(answered(&request), (false, format!("Connected to {k}.")));
}

/// A double click on an option under tabs picks it and shows the next question: the second click
/// mustn't pick the option of that question drawn under the pointer.
#[gpui_kit::test]
fn a_double_click_picks_one_option_under_tabs(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let _answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(3));
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let a = window
            .find(crate::balloon::part(n, "q0-option-0"))
            .bounds()
            .center();
        let at = (f32::from(a.x), f32::from(a.y));
        click(window, cx, at, 1);
        window.render_frame(cx);
        let next = window.find(crate::balloon::part(n, "q1-option-0")).bounds();
        assert!(next.contains(&a), "Q1's A is under the pointer");
        click(window, cx, at, 2);
    })
    .unwrap();
    assert_eq!(tab_of(&ws, cx), 1);
    assert!(drawn(handle, cx, n, "tab-0-answered"));
    assert!(!drawn(handle, cx, n, "tab-1-answered"), "Q1 has no answer");
}

/// A request withdrawn between a button's press and its release: the next one's button, drawn in
/// its place, isn't clicked by the release.
#[gpui_kit::test]
fn a_press_on_a_withdrawn_requests_button_answers_no_other(cx: &mut TestAppContext) {
    use gpui_kit::{MouseDownEvent, MouseUpEvent, PlatformInput};
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let asked = json!({ "question": "Ship it?", "options": ["Yes"] });
    // Three: the next one is selected as the first was, its Yes in the same place.
    let _first = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked.clone());
    let next = call_later(handle, &ws, cx, &terminal, 2, "ask_user", asked.clone());
    let _last = call_later(handle, &ws, cx, &terminal, 3, "ask_user", asked);
    let yes = |window: &mut Window| {
        window
            .find(crate::balloon::part(n, "q0-option-0"))
            .bounds()
            .center()
    };
    cx.update_window(handle, |_, window, cx| {
        inbox_open(window, cx);
        window.render_frame(cx);
        let position = yes(window);
        window.dispatch_event(
            PlatformInput::MouseDown(MouseDownEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
                first_mouse: false,
            }),
            cx,
        );
    })
    .unwrap();
    // Its agent leaves (a cancelled question would stay).
    bus_note(handle, &ws, cx, 1, templum_mcp::bus::CLOSED, Value::Null);
    // Long enough for a press on the next one to count, were it a click on it.
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let position = yes(window);
        window.dispatch_event(
            PlatformInput::MouseUp(MouseUpEvent {
                button: MouseButton::Left,
                position,
                modifiers: Modifiers::default(),
                click_count: 1,
            }),
            cx,
        );
    })
    .unwrap();
    assert_eq!(next.try_recv(), Err(TryRecvError::Empty), "it waits");
    ws.read_with(cx, |ws, _| match &ws.waits[0].what {
        Waiting::Question { form, .. } => assert!(!form.picked(0, 0), "nothing picked"),
        _ => panic!("a question"),
    });
}

/// Ctrl+L and Enter typed in the balloon's field while the asking terminal is focused: the field
/// takes them, the shell never sees them.
#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn keys_in_the_balloons_field_never_reach_the_terminal(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "ask_user",
        json!({ "question": "Name?" }),
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(mine, window, cx));
    })
    .unwrap();
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        let key = ws.read(cx).waits[0].key;
        let input = field_of(ws.read(cx), key, Some(0));
        input.update(cx, |input, cx| input.focus(window, cx));
        window.render_frame(cx);
        window.input("Ada", cx);
        window.press("ctrl-l", cx);
    })
    .unwrap();
    // A key of its own, as the platform sends it: what was typed is heard first.
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    assert_eq!(answered(&answers), (false, "Ada".to_string()));
    // `cat`'s tty would have echoed Ctrl+L as ^L.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let shown = ws.read_with(cx, |ws, _| {
        let mut snap = templum_terminal::Snapshot::default();
        snap.refresh(
            ws.space.terminal(mine).unwrap().term.shared(),
            &ws.theme.palette,
        );
        snap.text()
    });
    assert!(!shown.contains("^L") && !shown.contains("Ada"), "{shown:?}");
}

#[test]
fn deadline_takes_the_shorter() {
    use crate::workspace::waits::deadline;
    assert_eq!(deadline(None, 0), None);
    assert_eq!(deadline(Some(30), 0), Some(30));
    assert_eq!(deadline(None, 1), Some(60));
    assert_eq!(deadline(Some(120), 1), Some(60));
    assert_eq!(deadline(Some(30), 1), Some(30));
}

#[gpui_kit::test]
fn a_questions_call_answers_json(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    let key = last_wait(&ws, cx);
    for part in ["q0-option-1", "q1-option-0", "q1-option-2"] {
        click_part(handle, cx, n, part);
    }
    assert!(answers.try_recv().is_err(), "it waits for Send");
    type_into(handle, &ws, cx, key, None, "keep it small");
    click_part(handle, cx, n, "send");
    let (failed, text) = answered(&answers);
    assert!(!failed, "{text}");
    assert_eq!(
        json_of(&text),
        json!({
            "answers": {
                "Which database should the service use?": "SQLite",
                "Which features go in the first release?": ["Auth", "Export"],
            },
            "note": "keep it small",
        })
    );
}

#[gpui_kit::test]
fn send_waits_for_every_question(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    click_part(handle, cx, n, "q0-option-0");
    click_part(handle, cx, n, "send");
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "it waits");
    assert!(drawn(handle, cx, n, "send"));
}

#[gpui_kit::test]
fn todays_call_draws_todays_dialog(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "Not yet"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    for part in ["q0-option-0", "q0-option-1", "dismiss", "send"] {
        assert!(drawn(handle, cx, n, part), "{part}");
    }
    for part in ["q0-other", "note"] {
        assert!(!drawn(handle, cx, n, part), "{part}");
    }
    click_part(handle, cx, n, "q0-option-1");
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "Send answers");
    click_part(handle, cx, n, "send");
    assert_eq!(answered(&answers), (false, "Not yet".to_string()));
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "ask_user",
        json!({ "question": "When?" }),
    );
    let key = last_wait(&ws, cx);
    for part in ["q0-other", "send", "dismiss"] {
        assert!(drawn(handle, cx, n, part), "{part}");
    }
    assert!(!drawn(handle, cx, n, "note"));
    type_into(handle, &ws, cx, key, Some(0), "   ");
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    assert!(answers.try_recv().is_err(), "a blank answer isn't sent");
    type_into(handle, &ws, cx, key, Some(0), "Friday");
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    assert_eq!(answered(&answers), (false, "Friday".to_string()));
}

/// A click on an option, even of the only single-select question, only picks it: Send answers
/// (an accidental click mustn't).
#[gpui_kit::test]
fn a_click_on_an_option_only_picks_it_and_send_answers(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let asked = json!({ "questions": [{ "question": "When?", "options": ["Now", "Later"] }] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let key = last_wait(&ws, cx);
    type_into(handle, &ws, cx, key, None, "after lunch");
    click_part(handle, cx, n, "send");
    assert_eq!(
        answers.try_recv(),
        Err(TryRecvError::Empty),
        "nothing to send"
    );
    click_part(handle, cx, n, "q0-option-0");
    click_part(handle, cx, n, "q0-option-1");
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "picked only");
    click_part(handle, cx, n, "send");
    let (failed, text) = answered(&answers);
    assert!(!failed, "{text}");
    assert_eq!(
        json_of(&text),
        json!({ "answers": { "When?": "Later" }, "note": "after lunch" })
    );
}

#[gpui_kit::test]
fn dismiss_refuses_the_question(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    click_part(handle, cx, n, "dismiss");
    assert_eq!(
        answered(&answers),
        (
            true,
            "The person dismissed the question without answering.".to_string()
        )
    );
    ws.read_with(cx, |ws, _| assert!(ws.waits.is_empty()));
}

#[gpui_kit::test]
fn a_question_waits_without_a_deadline_by_default(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let asked = json!({ "question": "Ship it?", "options": ["Yes"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    after(cx, 7_200_000);
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "it waits");
    ws.read_with(cx, |ws, _| assert_eq!(ws.waits.len(), 1));
}

#[gpui_kit::test]
fn the_setting_sets_the_deadline_unless_the_call_is_shorter(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    ws.update(cx, |ws, _| ws.config.connections.question_timeout = 1);
    let ask = |timeout: Option<u64>| match timeout {
        Some(t) => json!({ "question": "Ship it?", "options": ["Yes"], "timeout": t }),
        None => json!({ "question": "Ship it?", "options": ["Yes"] }),
    };
    let short = call_later(handle, &ws, cx, &terminal, 1, "ask_user", ask(Some(30)));
    let long = call_later(handle, &ws, cx, &terminal, 2, "ask_user", ask(Some(120)));
    let none = call_later(handle, &ws, cx, &terminal, 3, "ask_user", ask(None));
    after(cx, 29_000);
    assert!(short.try_recv().is_err(), "not yet");
    after(cx, 2_000);
    let (failed, text) = answered(&short);
    assert!(failed && text.contains("within 30 seconds"), "{text}");
    after(cx, 28_000);
    assert!(
        long.try_recv().is_err() && none.try_recv().is_err(),
        "not yet"
    );
    after(cx, 2_000);
    for answers in [long, none] {
        let (failed, text) = answered(&answers);
        assert!(failed && text.contains("within 60 seconds"), "{text}");
    }
}

#[gpui_kit::test]
fn a_setting_changed_later_keeps_a_questions_deadline(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let asked = json!({ "question": "Ship it?", "options": ["Yes"] });
    let before = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked.clone());
    ws.update(cx, |ws, _| ws.config.connections.question_timeout = 1);
    after(cx, 90_000);
    assert_eq!(before.try_recv(), Err(TryRecvError::Empty), "it waits");
    let now = call_later(handle, &ws, cx, &terminal, 2, "ask_user", asked);
    after(cx, 61_000);
    let (failed, text) = answered(&now);
    assert!(failed && text.contains("withdrawn"), "{text}");
    assert_eq!(
        before.try_recv(),
        Err(TryRecvError::Empty),
        "it still waits"
    );
}

#[gpui_kit::test]
fn two_calls_from_one_block_keep_their_own_answers(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let asked = json!({ "questions": [{ "question": "Name?" }] });
    let first = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked.clone());
    let second = call_later(handle, &ws, cx, &terminal, 2, "ask_user", asked);
    let shown = ws.read_with(cx, |ws, _| ws.waits[0].key);
    type_into(handle, &ws, cx, shown, Some(0), "first");
    cx.update_window(handle, |_, window, cx| window.press("enter", cx))
        .unwrap();
    assert_eq!(
        json_of(&answered(&first).1),
        json!({ "answers": { "Name?": "first" } })
    );
    let next = ws.read_with(cx, |ws, cx| {
        let wait = &ws.waits[0];
        assert_ne!(wait.key, shown);
        field_of(ws, wait.key, Some(0)).read(cx).value().to_string()
    });
    assert_eq!(next, "", "its own field");
    let key = last_wait(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.send_question(key, window, cx))
    })
    .unwrap();
    assert_eq!(second.try_recv(), Err(TryRecvError::Empty), "nothing typed");
}

/// Hovering an option highlights it, and the preview beside the options is its own.
#[gpui_kit::test]
fn hovering_an_option_shows_its_preview(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    let preview = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| match &ws.waits[0].what {
            Waiting::Question { ask, form, .. } => (
                form.highlight,
                ask.preview_of(0, form).map(|p| p.map(str::to_string)),
            ),
            _ => panic!("a question"),
        })
    };
    assert!(drawn(handle, cx, n, "q0-preview"));
    assert!(!drawn(handle, cx, n, "q1-preview"), "no previews there");
    let (_, shown) = preview(cx);
    assert!(
        shown.flatten().unwrap().starts_with("DATABASE_URL"),
        "the first one's"
    );
    cx.update_window(handle, |_, window, cx| {
        scroll_to(window, cx, n, "q0-option-1");
        window.hover(crate::balloon::part(n, "q0-option-1"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        preview(cx),
        (Some(templum_mcp::ask::Row::Option(0, 1)), Some(None)),
        "No preview"
    );
}

/// The scroll of the newest wait's body, pixels.
fn body_scroll(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> f32 {
    ws.read_with(cx, |ws, _| {
        f32::from(ws.waits.last().unwrap().body.handle.offset().y)
    })
}

/// A question of thousands of words and thirty options keeps its header and its foot in
/// the balloon, which is no taller than 80% of its terminal's area, and scrolls its body between
/// them: an option far down is reached, picked and sent, the header and the foot where they were.
#[gpui_kit::test]
fn a_huge_request_scrolls_its_body_over_a_foot_that_stays(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let part = crate::balloon::part;
    let words = "A question that goes on about the database. ".repeat(120);
    let options: Vec<Value> = (0..30)
        .map(|i| {
            json!({
                "label": format!("Option {i} {}", "with a long label ".repeat(2)),
                "description": "and a description that says more. ".repeat(2),
            })
        })
        .collect();
    let asked = json!({ "questions": [{
        "question": words, "header": "Database", "options": options
    }] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    shown(handle, cx);
    let frames = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let at = |id: gpui_kit::ElementId| window.find(id).bounds();
            [at("inbox".into()), at(part(n, "body")), at(part(n, "send"))]
        })
        .unwrap()
    };
    let before = frames(cx);
    let [popup, body, send] = before;
    assert!(body.top() >= popup.top() && body.bottom() <= popup.bottom());
    assert!(send.top() >= body.bottom() - px(1.0) && send.bottom() <= popup.bottom());
    assert!(body.size.height > px(100.0), "the body keeps some room");
    // Far below the fold, in the body.
    assert!(
        !window_shows(handle, cx, n, "q0-option-29"),
        "the last option starts hidden"
    );
    click_part(handle, cx, n, "q0-option-29");
    assert!(body_scroll(&ws, cx) < -100.0, "the body scrolled");
    assert_eq!(frames(cx), before, "the foot stays");
    click_part(handle, cx, n, "send");
    let (failed, text) = answered(&answers);
    assert!(!failed, "{text}");
    let answer = json_of(&text)["answers"][&words]
        .as_str()
        .unwrap()
        .to_string();
    assert!(answer.starts_with("Option 29 "), "{answer}");
}

/// Whether the balloon's element `what` is inside its body's viewport.
fn window_shows(handle: AnyWindowHandle, cx: &mut TestAppContext, n: usize, what: &str) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let body = window.find(crate::balloon::part(n, "body")).bounds();
        let target = window.find(crate::balloon::part(n, what)).bounds();
        target.top() >= body.top() && target.bottom() <= body.bottom()
    })
    .unwrap()
}

/// Words with no space in them (a path, a hash) wrap by letters in the dialog's width: its
/// content never grows past the body, and the long option takes the lines it needs.
#[gpui_kit::test]
fn a_word_longer_than_the_dialog_wraps_inside_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let asked = json!({ "questions": [{
        "question": format!("Where is {}?", "a".repeat(400)),
        "header": "Path",
        "options": [
            { "label": "b".repeat(300), "description": "c".repeat(300) },
            { "label": "Short" },
        ],
    }] });
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let key = last_wait(&ws, cx);
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let at = |id: gpui_kit::ElementId| window.find(id).bounds();
        let (body, content) = (
            at(crate::balloon::part(n, "body")),
            at(("balloon-wait", key).into()),
        );
        assert!(
            content.left() >= body.left() && content.right() <= body.right(),
            "{content:?} in {body:?}"
        );
        let (long, short) = (
            at(crate::balloon::part(n, "q0-option-0")),
            at(crate::balloon::part(n, "q0-option-1")),
        );
        assert!(long.right() <= content.right(), "{long:?} in {content:?}");
        assert!(
            long.size.height > short.size.height * 4.0,
            "wrapped on many lines: {long:?} {short:?}"
        );
    })
    .unwrap();
}

/// A preview lies beside the options in a wide inbox and under them in a narrow one (the popup's
/// size at first leaves the request a column too narrow for two).
#[gpui_kit::test]
fn a_preview_goes_under_the_options_of_a_narrow_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    let places = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            inbox_open(window, cx);
            window.render_frame(cx);
            let at = |what: &str| window.find(crate::balloon::part(n, what)).bounds();
            (at("q0-option-1"), at("q0-preview"))
        })
        .unwrap()
    };
    let (option, preview) = places(cx);
    assert!(
        preview.top() >= option.bottom(),
        "under: {preview:?} {option:?}"
    );
    assert!(preview.left() <= option.left() + px(20.0), "{preview:?}");
    ws.update(cx, |ws, _| {
        ws.git_size = Some(templum_canvas::Size::new(1100.0, 540.0))
    });
    let (option, preview) = places(cx);
    assert!(
        preview.left() >= option.right(),
        "beside: {preview:?} {option:?}"
    );
}

/// The foot says how many of the questions have an answer, when there are several.
#[gpui_kit::test]
fn the_foot_counts_the_answers_of_several_questions(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    assert!(drawn(handle, cx, n, "progress"));
    let _one = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "ask_user",
        json!({ "questions": [{ "question": "Ship it?", "options": ["Yes"] }] }),
    );
    let key = last_wait(&ws, cx);
    ws.update(cx, |ws, _| ws.waits.retain(|w| w.key == key));
    assert!(!drawn(handle, cx, n, "progress"), "one question needs none");
}

/// The wheel over the preview of the newest question's first question, the pointer on the
/// balloon first (its block takes the wheel then), and how far the dialog's body scrolled.
fn wheel_over_preview(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    n: usize,
) -> f32 {
    cx.update_window(handle, |_, window, cx| {
        inbox_open(window, cx);
        window.render_frame(cx);
        window.hover("inbox", cx);
        let preview = window.find(crate::balloon::part(n, "q0-preview")).bounds();
        let at = (f32::from(preview.center().x), f32::from(preview.center().y));
        wheel(
            window,
            cx,
            at,
            ScrollDelta::Pixels(point(px(0.0), px(-40.0))),
            false,
        );
        window.render_frame(cx);
    })
    .unwrap();
    body_scroll(ws, cx)
}

/// The wheel over a preview, short or tall, scrolls the request's body: a preview has no scroll
/// of its own (Show more shows the rest).
#[gpui_kit::test]
fn the_wheel_over_a_preview_scrolls_the_requests_body(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    // Wide enough to lay the preview beside the options.
    ws.update(cx, |ws, _| {
        ws.git_size = Some(templum_canvas::Size::new(1100.0, 540.0))
    });
    let with = |preview: String| {
        let options: Vec<Value> = (0..12)
            .map(|i| json!({ "label": format!("Option {i}"), "preview": preview }))
            .collect();
        json!({ "questions": [{ "question": "Which?", "options": options }] })
    };
    let _short = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "ask_user",
        with("two\nlines".into()),
    );
    assert!(
        wheel_over_preview(handle, &ws, cx, n) < 0.0,
        "the body scrolled"
    );
    let tall: Vec<String> = (0..300).map(|i| format!("line {i}")).collect();
    let _tall = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "ask_user",
        with(tall.join("\n")),
    );
    ws.update(cx, |ws, _| {
        ws.waits.remove(0);
    });
    assert!(
        wheel_over_preview(handle, &ws, cx, n) < 0.0,
        "the body scrolled over a tall one too"
    );
}

/// The person works in block `id`, then opens the inbox (the keyboard is the inbox's), drawn long
/// enough ago for keys to count.
fn focus_dialog(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: BlockId,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.focus_block(id, window, cx);
            ws.open_inbox(None, window, cx)
        })
    })
    .unwrap();
    shown(handle, cx);
}

/// The person presses `keys`, the window drawn after each.
fn press(handle: AnyWindowHandle, cx: &mut TestAppContext, keys: &[&str]) {
    cx.update_window(handle, |_, window, cx| {
        for key in keys {
            window.press(key, cx);
            window.render_frame(cx);
        }
    })
    .unwrap();
}

/// The highlight of the newest question.
fn highlight(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Option<templum_mcp::ask::Row> {
    ws.read_with(cx, |ws, _| match &ws.waits.last().unwrap().what {
        Waiting::Question { form, .. } => form.highlight,
        _ => panic!("a question"),
    })
}

/// A `questions` call of `n` single-select questions ("Q0", …) of two options ("A", "B").
fn singles(n: usize) -> Value {
    let questions: Vec<Value> = (0..n)
        .map(|q| json!({ "question": format!("Q{q}"), "options": ["A", "B"] }))
        .collect();
    json!({ "questions": questions })
}

/// What the terminal of block `id` shows, once its program had time to echo.
#[cfg(unix)]
fn screen(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId) -> String {
    std::thread::sleep(std::time::Duration::from_millis(300));
    ws.read_with(cx, |ws, _| {
        let mut snap = templum_terminal::Snapshot::default();
        snap.refresh(
            ws.space.terminal(id).unwrap().term.shared(),
            &ws.theme.palette,
        );
        snap.text()
    })
}

#[gpui_kit::test]
fn the_keys_answer_a_question(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "No"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    focus_dialog(handle, &ws, cx, mine);
    // Enter picks No and puts the highlight on Send; the next Enter sends.
    press(handle, cx, &["down", "down", "enter"]);
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "picked only");
    assert_eq!(highlight(&ws, cx), Some(templum_mcp::ask::Row::Send));
    press(handle, cx, &["enter"]);
    assert_eq!(answered(&answers), (false, "No".to_string()));
}

/// The keys' highlight (↓ ↑ Tab) goes down a long list and back up with the row always in view:
/// the body scrolls to show it, once, and a wheel that scrolls it away after isn't fought.
#[gpui_kit::test]
fn the_keys_scroll_the_body_to_the_highlighted_row(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let options: Vec<Value> = (0..30)
        .map(|i| json!({ "label": format!("Option {i}"), "description": "More words." }))
        .collect();
    let asked = json!({ "questions": [{ "question": "Which?", "options": options }] });
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    focus_dialog(handle, &ws, cx, mine);
    let frames = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .unwrap()
    };
    assert!(
        !window_shows(handle, cx, n, "q0-option-29"),
        "it starts below"
    );
    for i in 0..30 {
        press(handle, cx, &["down"]);
        frames(cx);
        assert!(
            window_shows(handle, cx, n, &format!("q0-option-{i}")),
            "option {i} is highlighted and out of view"
        );
    }
    // Other, below the last option.
    press(handle, cx, &["tab"]);
    frames(cx);
    assert!(
        window_shows(handle, cx, n, "q0-other"),
        "Other is out of view"
    );
    // Back up.
    for i in (0..30).rev() {
        press(handle, cx, &["up"]);
        frames(cx);
        if i == 29 {
            continue;
        }
        assert!(
            window_shows(handle, cx, n, &format!("q0-option-{i}")),
            "going up, option {i} is out of view"
        );
    }
    // A wheel that scrolls the highlighted row away stays scrolled.
    let before = body_scroll(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.hover("inbox", cx);
        let body = window.find(crate::balloon::part(n, "body")).bounds();
        let at = (f32::from(body.left()) + 4.0, f32::from(body.center().y));
        wheel(
            window,
            cx,
            at,
            ScrollDelta::Pixels(point(px(0.0), px(-300.0))),
            false,
        );
    })
    .unwrap();
    frames(cx);
    assert!(body_scroll(&ws, cx) < before - 100.0, "the wheel scrolled");
}

/// Space picks without sending; Enter on the option already picked sends.
#[gpui_kit::test]
fn space_picks_and_enter_on_the_picked_option_sends(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "No"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    focus_dialog(handle, &ws, cx, mine);
    press(handle, cx, &["down", "space"]);
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "picked only");
    press(handle, cx, &["enter"]);
    assert_eq!(answered(&answers), (false, "Yes".to_string()));
}

/// No row is highlighted as the dialog opens: Space and Enter meant for the terminal answer
/// nothing.
#[gpui_kit::test]
fn enter_with_nothing_highlighted_does_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "No"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    focus_dialog(handle, &ws, cx, mine);
    press(handle, cx, &["enter", "space"]);
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "it waits");
    assert_eq!(highlight(&ws, cx), None);
}

#[gpui_kit::test]
fn space_checks_and_enter_sends_once_all_are_answered(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    focus_dialog(handle, &ws, cx, mine);
    // SQLite, then on to the second question.
    press(handle, cx, &["down", "down", "enter"]);
    assert_eq!(
        highlight(&ws, cx),
        Some(templum_mcp::ask::Row::Option(1, 0))
    );
    // Auth and Export.
    press(handle, cx, &["space", "down", "down", "space"]);
    assert_eq!(
        answers.try_recv(),
        Err(TryRecvError::Empty),
        "Space never sends"
    );
    press(handle, cx, &["enter"]);
    let (failed, text) = answered(&answers);
    assert!(!failed, "{text}");
    assert_eq!(
        json_of(&text),
        json!({ "answers": {
            "Which database should the service use?": "SQLite",
            "Which features go in the first release?": ["Auth", "Export"],
        } })
    );
}

#[gpui_kit::test]
fn enter_moves_to_the_next_unanswered_question(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(2));
    focus_dialog(handle, &ws, cx, mine);
    press(handle, cx, &["down", "enter"]);
    assert_eq!(
        highlight(&ws, cx),
        Some(templum_mcp::ask::Row::Option(1, 0))
    );
    assert_eq!(tab_of(&ws, cx), 1, "its tab shows");
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "it waits");
}

#[gpui_kit::test]
fn tab_reaches_other_and_the_note(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(1));
    let key = last_wait(&ws, cx);
    focus_dialog(handle, &ws, cx, mine);
    press(handle, cx, &["tab", "tab", "tab"]);
    cx.update_window(handle, |_, window, cx| {
        let other = field_of(ws.read(cx), key, Some(0));
        assert!(
            other.read(cx).focus_handle(cx).is_focused(window),
            "Other's field has the keyboard"
        );
        window.input("Friday", cx);
    })
    .unwrap();
    press(handle, cx, &["tab"]);
    cx.update_window(handle, |_, window, cx| {
        let note = field_of(ws.read(cx), key, None);
        assert!(
            note.read(cx).focus_handle(cx).is_focused(window),
            "then the note's"
        );
    })
    .unwrap();
    press(handle, cx, &["enter"]);
    assert_eq!(
        json_of(&answered(&answers).1),
        json!({ "answers": { "Q0": "Friday" } })
    );
}

/// Esc closes the inbox and gives the keyboard back to the terminal the person worked in; the
/// request waits, and the notice on its terminal opens the inbox again.
#[cfg(unix)]
#[gpui_kit::test]
fn esc_closes_the_inbox_and_gives_the_keyboard_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "No"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    focus_dialog(handle, &ws, cx, mine);
    press(handle, cx, &["escape"]);
    ws.read_with(cx, |ws, _| assert!(!ws.inbox_open));
    cx.update_window(handle, |_, window, cx| window.input("ls", cx))
        .unwrap();
    assert!(screen(&ws, cx, mine).contains("ls"), "the terminal's");
    press(handle, cx, &["down"]);
    assert!(
        screen(&ws, cx, mine).contains("^[[B"),
        "still the terminal's"
    );
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "it waits");
    assert_eq!(highlight(&ws, cx), None);
    // Its notice opens the inbox again, with the keyboard.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(
            gpui_kit::ElementId::Name(format!("waiting-notice-{n}").into()),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    after(cx, 600);
    press(handle, cx, &["down", "enter", "enter"]);
    assert_eq!(answered(&answers), (false, "Yes".to_string()));
}

/// A key or click meant for the terminal as the dialog appears doesn't answer it.
#[gpui_kit::test]
fn keys_and_clicks_too_soon_do_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "No"] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.focus_block(mine, window, cx);
            ws.open_inbox(None, window, cx)
        });
        window.render_frame(cx);
        window.press("down", cx);
        window.press("enter", cx);
        window.render_frame(cx);
        window.click(crate::balloon::part(n, "q0-option-0"), cx);
    })
    .unwrap();
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "too soon");
    after(cx, 600);
    // The pointer on the option highlights it.
    assert_eq!(
        highlight(&ws, cx),
        Some(templum_mcp::ask::Row::Option(0, 0))
    );
    press(handle, cx, &["enter", "enter"]);
    assert_eq!(answered(&answers), (false, "Yes".to_string()));
}

/// While the dialog has the keyboard, its keys never reach the terminal: arrows, Space, Enter.
#[cfg(unix)]
#[gpui_kit::test]
fn keys_never_reach_the_terminal_while_the_dialog_has_them(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    focus_dialog(handle, &ws, cx, mine);
    press(handle, cx, &["down", "space", "x"]);
    cx.update_window(handle, |_, window, cx| window.input("ls", cx))
        .unwrap();
    let shown = screen(&ws, cx, mine);
    assert!(
        !shown.contains("^[[B") && !shown.contains("ls") && !shown.contains('x'),
        "{shown:?}"
    );
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "it waits");
}

/// Space types in the dialog's fields: a free answer and the note keep their spaces.
#[gpui_kit::test]
fn space_types_in_a_field(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let asked = json!({ "questions": [{ "question": "Why?" }] });
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let key = last_wait(&ws, cx);
    focus_dialog(handle, &ws, cx, mine);
    press(
        handle,
        cx,
        &["tab", "a", "space", "b", "tab", "c", "space", "d"],
    );
    let texts = ws.read_with(cx, |ws, cx| {
        let text = |field| field_of(ws, key, field).read(cx).value().to_string();
        (text(Some(0)), text(None))
    });
    assert_eq!(texts, ("a b".to_string(), "c d".to_string()));
}

/// Enter in a field acts once: with a question still unanswered it moves there, and sends nothing.
#[gpui_kit::test]
fn enter_in_a_field_moves_on_once(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(2));
    let key = last_wait(&ws, cx);
    focus_dialog(handle, &ws, cx, mine);
    type_into(handle, &ws, cx, key, Some(0), "X");
    press(handle, cx, &["enter"]);
    assert_eq!(
        answers.try_recv(),
        Err(TryRecvError::Empty),
        "Q1 unanswered"
    );
    assert_eq!(
        highlight(&ws, cx),
        Some(templum_mcp::ask::Row::Option(1, 0))
    );
}

/// Enter in the note, no question answered: the highlight goes to the question; nothing is
/// picked nor sent.
#[gpui_kit::test]
fn enter_in_the_note_moves_on_once(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(1));
    let key = last_wait(&ws, cx);
    focus_dialog(handle, &ws, cx, mine);
    type_into(handle, &ws, cx, key, None, "n");
    press(handle, cx, &["enter"]);
    assert_eq!(
        answers.try_recv(),
        Err(TryRecvError::Empty),
        "Q0 unanswered"
    );
    assert_eq!(
        highlight(&ws, cx),
        Some(templum_mcp::ask::Row::Option(0, 0))
    );
}

/// A press in the request's field: the keys stay the inbox's once the highlight leaves the field.
#[gpui_kit::test]
fn a_field_that_takes_the_keyboard_keeps_the_inbox_keys(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    super::terminals::set_active(handle, true, cx);
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(1));
    let key = last_wait(&ws, cx);
    focus_dialog(handle, &ws, cx, kanban);
    type_into(handle, &ws, cx, key, Some(0), "X");
    frame(handle, cx);
    // Out of the field, onto B, picked; then sent.
    press(handle, cx, &["shift-tab", "space", "enter"]);
    assert_eq!(
        json_of(&answered(&answers).1),
        json!({ "answers": { "Q0": "B" } })
    );
}

/// A click in the dialog's field of a block that hadn't the keyboard leaves it in the field.
#[gpui_kit::test]
fn a_click_in_the_dialogs_field_keeps_the_keyboard_there(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    super::terminals::set_active(handle, true, cx);
    ws.update(cx, |ws, _| ws.config.canvas.focus_follows_pointer = false);
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(1));
    let key = last_wait(&ws, cx);
    focus_dialog(handle, &ws, cx, kanban);
    click_part(handle, cx, mine.0 as usize, "note");
    cx.update_window(handle, |_, window, cx| {
        let note = field_of(ws.read(cx), key, None);
        assert!(note.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

/// A plan is drawn rendered in its balloon; Reject with a comment, or the keys onto Approve,
/// answer it (docs/specs/agent-tools.md, propose_plan).
#[gpui_kit::test]
fn a_plan_is_approved_or_rejected_with_a_comment_from_its_dialog(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        3,
        "propose_plan",
        json!({ "plan": "ok" }),
    );
    focus_dialog(handle, &ws, cx, mine);
    // Comment, Reject, Approve.
    press(handle, cx, &["down", "down", "down", "enter"]);
    assert_eq!(answered(&answers), (false, "Approved.".into()));
    let plan = json!({ "plan": "## Steps\n1. Replace the reader threads\n2. Test", "title": "Move the bus" });
    let answers = call_later(handle, &ws, cx, &terminal, 4, "propose_plan", plan);
    assert!(drawn(handle, cx, n, "plan-body"), "the plan is drawn");
    assert!(drawn(handle, cx, n, "plan-title"));
    assert!(!drawn(handle, cx, n, "dismiss"), "Reject, not Dismiss");
    let key = last_wait(&ws, cx);
    type_into(handle, &ws, cx, key, None, "do 2 first");
    click_part(handle, cx, n, "plan-reject");
    assert_eq!(
        answered(&answers),
        (true, "Rejected.\nThe person's comment: do 2 first".into())
    );
}

#[gpui_kit::test]
fn a_cancelled_plan_is_typed_back_once_answered(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    agent_without_notices(&ws, cx, mine);
    let n = mine.0 as usize;
    let _first = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        3,
        "propose_plan",
        json!({ "plan": "## Steps" }),
    );
    bus_note(
        handle,
        &ws,
        cx,
        3,
        "notifications/cancelled",
        json!({ "requestId": 3 }),
    );
    assert!(stopped(&ws, cx));
    click_part(handle, cx, n, "plan-approve");
    assert_eq!(
        notices(&ws, cx),
        [(
            terminal.clone(),
            "[answer to your plan \"A plan\"] The answer is in the pasted text.\nApproved."
                .to_string()
        )]
    );
}

/// A plan, a question and a request to connect on one block wait in turn, listed, each
/// answered by its own buttons (Review Focus 5).
#[gpui_kit::test]
fn a_plan_a_question_and_a_request_wait_in_turn(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let n = mine.0 as usize;
    let part = crate::balloon::part;
    let plan = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "propose_plan",
        json!({ "plan": "## Steps" }),
    );
    let asked = json!({ "question": "Ship it?", "options": ["Yes", "Not yet"] });
    let question = call_later(handle, &ws, cx, &terminal, 2, "ask_user", asked);
    let request = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        3,
        "connect",
        json!({ "block": k }),
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.asking(&terminal).map(|(_, count)| count)),
        Some(3)
    );
    let keys: Vec<u64> = ws.read_with(cx, |ws, _| ws.waits.iter().map(|w| w.key).collect());
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for key in &keys {
            let row = gpui_kit::ElementId::Name(format!("inbox-row-{key}").into());
            assert!(window.try_find(row).is_some(), "listed");
        }
        window.click(part(n, "plan-approve"), cx);
    })
    .unwrap();
    assert_eq!(answered(&plan), (false, "Approved.".to_string()));
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(part(n, "plan-approve")).is_none());
        window.click(part(n, "q0-option-1"), cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        question.try_recv(),
        Err(TryRecvError::Empty),
        "Send answers"
    );
    cx.update_window(handle, |_, window, cx| window.click(part(n, "send"), cx))
        .unwrap();
    assert_eq!(answered(&question), (false, "Not yet".to_string()));
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(part(n, "allow"), cx);
    })
    .unwrap();
    assert_eq!(answered(&request), (false, format!("Connected to {k}.")));
}

/// A long plan scrolls in its frame, which takes at most 60% of the body's height.
#[gpui_kit::test]
fn a_long_plan_scrolls_in_its_frame(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let long: String = (1..=150).map(|i| format!("{i}. Step {i}\n")).collect();
    let _answers = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        3,
        "propose_plan",
        json!({ "plan": long }),
    );
    shown(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let frame = window.find(crate::balloon::part(n, "plan-body")).bounds();
        let balloon = window.find("inbox").bounds();
        assert!(
            frame.size.height <= balloon.size.height * 0.6 + px(1.0),
            "{frame:?} in {balloon:?}"
        );
        assert!(
            window
                .try_find(crate::balloon::part(n, "plan-approve"))
                .is_some(),
            "its buttons stay in the popup"
        );
    })
    .unwrap();
}

/// The wheel over a long plan scrolls the plan and leaves the dialog's body alone, even where
/// the body overflows too (a block this short): the plan's frame is sized from the body, not from
/// the whole balloon.
#[gpui_kit::test]
fn the_wheel_over_a_long_plan_scrolls_the_plan_not_the_body(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let long: String = (1..=150).map(|i| format!("{i}. Step {i}\n")).collect();
    let _plan = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        3,
        "propose_plan",
        json!({ "plan": long }),
    );
    shown(handle, cx);
    let key = last_wait(&ws, cx);
    let plan_scroll = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| match &ws.waits.last().unwrap().what {
            Waiting::Question { scroll, .. } => f32::from(scroll.handle.offset().y),
            _ => panic!("a plan"),
        })
    };
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let body = window.find(crate::balloon::part(n, "body")).bounds();
        let content = window.find(("balloon-wait", key)).bounds();
        let frame = window.find(crate::balloon::part(n, "plan-body")).bounds();
        assert!(
            content.size.height > body.size.height,
            "the body overflows too"
        );
        assert!(
            frame.size.height <= body.size.height * 0.6 + px(1.0),
            "{frame:?} in {body:?}"
        );
        window.hover("inbox", cx);
        let at = (f32::from(frame.center().x), f32::from(frame.center().y));
        wheel(
            window,
            cx,
            at,
            ScrollDelta::Pixels(point(px(0.0), px(-40.0))),
            false,
        );
        window.render_frame(cx);
    })
    .unwrap();
    assert!(plan_scroll(cx) < 0.0, "the plan scrolled");
    assert_eq!(body_scroll(&ws, cx), 0.0, "the body did not");
}

/// A plan the person doesn't see is a notification that says so: "Asks you to approve a plan: …".
#[gpui_kit::test]
fn a_plan_in_the_background_says_it_asks_for_an_approval(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    super::terminals::set_active(handle, false, cx);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(mine).unwrap().rect.x += 100_000.0;
        ws.space.focused = None;
    });
    let plan = json!({ "plan": "## Steps", "title": "Move the bus" });
    let _plan = call_later(handle, &ws, cx, &terminal, 1, "propose_plan", plan);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.space.terminal(mine).unwrap().alert.clone()),
        Some(crate::alert::Alert::Attention(Some(
            "Asks you to approve a plan: Move the bus".into()
        )))
    );
}

/// The question shown of the newest call (its lit tab).
fn tab_of(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> usize {
    ws.read_with(cx, |ws, _| match &ws.waits.last().unwrap().what {
        Waiting::Question { form, .. } => form.tab,
        _ => panic!("a question"),
    })
}

/// Several questions show one at a time, a tab each over it; a click on a tab shows its question.
/// One question shows no tab.
#[gpui_kit::test]
fn several_questions_show_one_at_a_time_under_tabs(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    for part in ["tab-0", "tab-1", "q0-option-0", "q0-other", "note", "send"] {
        assert!(drawn(handle, cx, n, part), "{part}");
    }
    assert!(
        !drawn(handle, cx, n, "q1-option-0"),
        "Features waits its tab"
    );
    click_part(handle, cx, n, "tab-1");
    assert_eq!(tab_of(&ws, cx), 1);
    for part in ["q1-option-0", "q1-other", "note", "send"] {
        assert!(drawn(handle, cx, n, part), "{part}");
    }
    assert!(!drawn(handle, cx, n, "q0-option-0"));
    click_part(handle, cx, n, "tab-0");
    assert!(drawn(handle, cx, n, "q0-option-0") && !drawn(handle, cx, n, "q1-option-0"));
    for (conn, asked) in [
        json!({ "questions": [{ "question": "Ship it?", "options": ["Yes"] }] }),
        json!({ "question": "Ship it?", "options": ["Yes"] }),
    ]
    .into_iter()
    .enumerate()
    {
        let _one = call_later(
            handle,
            &ws,
            cx,
            &terminal,
            conn as u64 + 2,
            "ask_user",
            asked,
        );
        let key = last_wait(&ws, cx);
        ws.update(cx, |ws, _| ws.waits.retain(|w| w.key == key));
        assert!(drawn(handle, cx, n, "q0-option-0"));
        assert!(!drawn(handle, cx, n, "tab-0"), "one question needs no tab");
    }
}

/// A tab's ring fills with a check once its question has an answer.
#[gpui_kit::test]
fn a_tab_says_whether_its_question_is_answered(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let _asked = call_later(handle, &ws, cx, &terminal, 1, "ask_user", two_questions());
    assert!(!drawn(handle, cx, n, "tab-0-answered") && !drawn(handle, cx, n, "tab-1-answered"));
    click_part(handle, cx, n, "tab-1");
    click_part(handle, cx, n, "q1-option-0");
    assert!(drawn(handle, cx, n, "tab-1-answered"), "Auth checked");
    assert!(!drawn(handle, cx, n, "tab-0-answered"));
    click_part(handle, cx, n, "q1-option-0");
    assert!(!drawn(handle, cx, n, "tab-1-answered"), "Auth unchecked");
}

/// What is typed in a tab (an Other text, a free answer, the note) stays when another tab shows,
/// and Send answers every question in the call's JSON.
#[gpui_kit::test]
fn answers_typed_in_a_tab_stay_when_another_shows(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let asked = json!({ "questions": [
        { "question": "Which?", "options": ["A", "B"] },
        { "question": "When?" }
    ] });
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    let key = last_wait(&ws, cx);
    type_into(handle, &ws, cx, key, Some(0), "C");
    type_into(handle, &ws, cx, key, None, "soon");
    click_part(handle, cx, n, "tab-1");
    type_into(handle, &ws, cx, key, Some(1), "Friday");
    click_part(handle, cx, n, "tab-0");
    let kept = ws.read_with(cx, |ws, cx| {
        let text = |field| field_of(ws, key, field).read(cx).value().to_string();
        (text(Some(0)), text(Some(1)), text(None))
    });
    assert_eq!(kept, ("C".into(), "Friday".into(), "soon".into()));
    assert!(drawn(handle, cx, n, "tab-0-answered") && drawn(handle, cx, n, "tab-1-answered"));
    click_part(handle, cx, n, "send");
    let (failed, text) = answered(&answers);
    assert!(!failed, "{text}");
    assert_eq!(
        json_of(&text),
        json!({ "answers": { "Which?": "C", "When?": "Friday" }, "note": "soon" })
    );
}

/// ← and → show the previous and the next question, wrapping; in a field they move in its text,
/// and do nothing at its start or its end (the field leaves them to its parents there).
#[gpui_kit::test]
fn left_and_right_turn_the_tabs_out_of_a_field(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(3));
    let key = last_wait(&ws, cx);
    focus_dialog(handle, &ws, cx, mine);
    press(handle, cx, &["right"]);
    assert_eq!(tab_of(&ws, cx), 1);
    press(handle, cx, &["left", "left"]);
    assert_eq!(tab_of(&ws, cx), 2, "it wraps");
    // ↓ highlights Q2's first option; → takes the highlight to Q0's.
    press(handle, cx, &["down", "right"]);
    assert_eq!(
        (tab_of(&ws, cx), highlight(&ws, cx)),
        (0, Some(templum_mcp::ask::Row::Option(0, 0)))
    );
    type_into(handle, &ws, cx, key, Some(0), "ab");
    let text = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, cx| {
            field_of(ws, key, Some(0)).read(cx).value().to_string()
        })
    };
    press(handle, cx, &["left", "left", "left", "x"]);
    assert_eq!(tab_of(&ws, cx), 0, "the field's, at its start too");
    assert_eq!(text(cx), "xab");
    press(handle, cx, &["right", "right", "right", "y"]);
    assert_eq!(tab_of(&ws, cx), 0, "at its end too");
    assert_eq!(text(cx), "xaby");
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "it waits");
}

/// A click that picks an option of a single-select question shows the next question without an
/// answer; a checkbox, or a call with every question answered, stays.
#[gpui_kit::test]
fn a_pick_shows_the_next_question_without_an_answer(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let answers = call_later(handle, &ws, cx, &terminal, 1, "ask_user", singles(3));
    click_part(handle, cx, n, "tab-1");
    click_part(handle, cx, n, "q1-option-1");
    assert_eq!(tab_of(&ws, cx), 2);
    click_part(handle, cx, n, "q2-option-0");
    assert_eq!(tab_of(&ws, cx), 0, "it wraps to the one left");
    click_part(handle, cx, n, "q0-option-1");
    assert_eq!(tab_of(&ws, cx), 0, "all answered: it stays");
    assert_eq!(answers.try_recv(), Err(TryRecvError::Empty), "Send answers");
    click_part(handle, cx, n, "send");
    assert_eq!(
        json_of(&answered(&answers).1),
        json!({ "answers": { "Q0": "B", "Q1": "B", "Q2": "A" } })
    );
    let _asked = call_later(handle, &ws, cx, &terminal, 2, "ask_user", two_questions());
    click_part(handle, cx, n, "tab-1");
    click_part(handle, cx, n, "q1-option-0");
    assert_eq!(tab_of(&ws, cx), 1, "a checkbox stays");
}

/// Another question shows from its top: the body scrolled down in a long one goes back up. A
/// call of one question has no tabs, and ← → leave its scroll alone.
#[gpui_kit::test]
fn a_question_turned_to_shows_from_its_top(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let options: Vec<Value> = (0..30).map(|i| json!(format!("Option {i}"))).collect();
    let long = json!({ "question": "Which?", "options": options });
    for (conn, questions) in [
        vec![
            long.clone(),
            json!({ "question": "When?", "options": ["Now"] }),
        ],
        vec![long],
    ]
    .into_iter()
    .enumerate()
    {
        let asked = json!({ "questions": questions });
        let _asked = call_later(
            handle,
            &ws,
            cx,
            &terminal,
            conn as u64 + 1,
            "ask_user",
            asked,
        );
        let key = last_wait(&ws, cx);
        ws.update(cx, |ws, _| ws.waits.retain(|w| w.key == key));
        focus_dialog(handle, &ws, cx, mine);
        press(handle, cx, &["down"; 30]);
        frame(handle, cx);
        let scrolled = body_scroll(&ws, cx);
        assert!(scrolled < -100.0, "the last option, far down: {scrolled}");
        press(handle, cx, &["right"]);
        if conn == 0 {
            assert_eq!((tab_of(&ws, cx), body_scroll(&ws, cx)), (1, 0.0));
        } else {
            assert_eq!(body_scroll(&ws, cx), scrolled, "no tab to turn to");
        }
    }
}

/// In an inbox too narrow for one row of tabs they wrap, none past the body's sides; in the
/// smallest one the body scrolls and every tab, option and Send is reached.
#[gpui_kit::test]
fn tabs_wrap_and_are_reached_in_a_small_inbox(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let n = mine.0 as usize;
    let questions: Vec<Value> = (0..6)
        .map(|q| {
            json!({ "question": format!("Q{q}"), "header": format!("Deployment {q}"),
                         "options": ["A", "B"] })
        })
        .collect();
    for (conn, (w, h)) in [(600.0, 420.0), (760.0, 540.0)].into_iter().enumerate() {
        ws.update(cx, |ws, _| {
            ws.git_size = Some(templum_canvas::Size::new(w, h))
        });
        let asked = json!({ "questions": questions });
        let answers = call_later(
            handle,
            &ws,
            cx,
            &terminal,
            conn as u64 + 1,
            "ask_user",
            asked,
        );
        let key = last_wait(&ws, cx);
        shown(handle, cx);
        let (content, tabs) = cx
            .update_window(handle, |_, window, cx| {
                window.render_frame(cx);
                let content = window.find(("balloon-wait", key)).bounds();
                let tabs: Vec<_> = (0..6)
                    .map(|q| {
                        window
                            .find(crate::balloon::part(n, &format!("tab-{q}")))
                            .bounds()
                    })
                    .collect();
                (content, tabs)
            })
            .unwrap();
        for tab in &tabs {
            assert!(
                tab.left() >= content.left() - px(1.0) && tab.right() <= content.right() + px(1.0),
                "{w} x {h}: {tab:?} in {content:?}"
            );
        }
        assert!(tabs[5].top() > tabs[0].top(), "{w} x {h}: on several lines");
        click_part(handle, cx, n, "tab-5");
        for q in [5, 0, 1, 2, 3, 4] {
            assert_eq!(tab_of(&ws, cx), q, "{w} x {h}");
            click_part(handle, cx, n, &format!("q{q}-option-1"));
        }
        click_part(handle, cx, n, "send");
        let (failed, text) = answered(&answers);
        assert!(!failed, "{text}");
        assert_eq!(json_of(&text)["answers"].as_object().unwrap().len(), 6);
    }
}
