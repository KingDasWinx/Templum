//! The Templum API's glue (docs/specs/api.md): the calls its server hands the UI thread, the
//! checks of a token's scope and workspaces, the state, and the notices.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use serde_json::{Value, json};
use templum_api::store::Scope;
use templum_api::{Store, Token, codes};
use templum_mcp::{Hello, Incoming, Message};

#[cfg(unix)]
use super::bus::{after, event, screen_with};
use super::bus::{answered, call_later, me};
#[cfg(unix)]
use super::delegate::{agents_running, entered, prints};
use super::*;
use crate::workspace::AgentState;
use templum_canvas::{Color, Drawing, ShapeKind};

/// A token of `scope` (and `workspaces`) the workspace's store keeps, as Settings would make one.
pub(super) fn with_token(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    scope: Scope,
    workspaces: Option<Vec<&str>>,
) -> Token {
    ws.update(cx, |ws, _| {
        let store = ws.api.store.get_or_insert_with(Store::default);
        let workspaces = workspaces.map(|w| w.into_iter().map(String::from).collect());
        store.mint("phone", scope, None, workspaces, 0).unwrap();
        store.tokens.last().unwrap().clone()
    })
}

/// The channel the answer to `method` comes on, as the API's server asks it: the message of
/// connection `conn`, from the token `token`.
pub(super) fn api_later(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
    conn: u64,
    method: &str,
    params: Value,
) -> Receiver<String> {
    let (reply, answers) = std::sync::mpsc::channel();
    let message = Message {
        hello: Hello {
            terminal: token.id.to_string(),
            client: "api".into(),
            control: None,
        },
        conn,
        incoming: Incoming::Request {
            id: json!(conn),
            method: method.into(),
            params,
        },
        reply,
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.serve(message, window, cx))
    })
    .unwrap();
    answers
}

/// The answer line to `method`, asked at once.
pub(super) fn api_call(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
    method: &str,
    params: Value,
) -> Value {
    let answers = api_later(handle, ws, cx, token, 900, method, params);
    serde_json::from_str(&answers.try_recv().expect("answered at once")).unwrap()
}

/// A tool's answer line.
pub(super) fn tool(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
    name: &str,
    arguments: Value,
) -> Value {
    api_call(
        handle,
        ws,
        cx,
        token,
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    )
}

/// The text of a tool's answer, and whether it refused.
pub(super) fn said(line: &Value) -> (bool, String) {
    let result = &line["result"];
    (
        result["isError"] == true,
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

/// The state, as the token sees it.
pub(super) fn state(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
) -> templum_api::State {
    let line = api_call(handle, ws, cx, token, templum_api::STATE, Value::Null);
    serde_json::from_value(line["result"].clone()).unwrap_or_else(|_| panic!("{line}"))
}

#[gpui_kit::test]
fn the_state_shows_notes_checklists_and_drawings(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    let big = "é".repeat(150_000);
    let big_id = ws.update(cx, |ws, _| {
        ws.notes
            .add(big.clone(), Some(crate::board::Label::Blue), 0)
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let tab = ws.note_tab(big_id, window, cx).unwrap();
            let note = ws
                .space
                .blocks
                .get_mut(&note)
                .and_then(|b| b.note_mut())
                .unwrap();
            note.tabs.push(tab);
            note.active = 1;
        })
    })
    .unwrap();
    let (_, list) = checklist_block(handle, &ws, cx, "Release", &["Tag it", "[x] Ship"]);
    ws.update(cx, |ws, _| {
        ws.space.ink.push(Drawing::Stroke {
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
            width: 2.0,
            color: Color::Blue,
            highlight: false,
        });
    });
    let token = with_token(&ws, cx, Scope::Read, None);
    let seen = state(handle, &ws, cx, &token);
    let uid = |id: BlockId| ws.read_with(cx, |ws, _| ws.space.uids[&id].clone());
    let block = |uid: &str| seen.blocks.iter().find(|b| b.id == uid).unwrap();
    let (hello_id, shown) = ws.read_with(cx, |ws, _| {
        let note = ws.space.blocks[&note].note().unwrap();
        (note.tabs[0].id, note.tab().map(|t| t.id))
    });
    assert_eq!(
        block(&uid(note)).notes,
        Some(templum_api::model::NotesShown {
            tabs: vec![hello_id, big_id],
            shown
        })
    );
    assert_eq!(shown, Some(big_id));
    assert_eq!(block(&uid(terminal)).notes, None);
    assert_eq!(block(&uid(terminal)).checklist, None);
    let checklist_uid = seen.blocks.iter().find(|b| b.kind == "checklist").unwrap();
    assert_eq!(checklist_uid.checklist, Some(list));
    assert_eq!(seen.notes.len(), 2);
    let cut = seen.notes.iter().find(|n| n.id == big_id).unwrap();
    assert!(cut.cut && cut.text.len() <= templum_api::model::NOTE_TEXT_MOST);
    assert!(cut.text.chars().all(|c| c == 'é') && big.starts_with(&cut.text));
    assert_eq!(cut.color.as_deref(), Some("blue"));
    assert_eq!(seen.checklists.len(), 1);
    let release = &seen.checklists[0];
    assert_eq!(release.name, "Release");
    assert_eq!(
        release
            .items
            .iter()
            .map(|i| (i.text.as_str(), i.done))
            .collect::<Vec<_>>(),
        [("Tag it", false), ("Ship", true)]
    );
    let id = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let want = ws.read_with(cx, |ws, _| {
        templum_api::model::DrawingView::from(&crate::session::InkItem::of(&ws.space.ink[0]))
    });
    assert_eq!(seen.drawings.len(), 1);
    assert_eq!(seen.drawings[0].id, id);
    assert_eq!(seen.drawings[0].drawings, [want]);
}

#[gpui_kit::test]
fn the_notes_texts_of_a_state_stay_within_a_total_budget(cx: &mut TestAppContext) {
    use templum_api::model::{NOTE_TEXT_MOST, NOTES_TEXT_MOST, json_len};
    let (handle, ws) = open(cx);
    let (_, note, _) = three_blocks(handle, &ws, cx);
    // A control character is six bytes as JSON: six notes of 1.5 MiB pass 8 MiB.
    let text = "\u{1}".repeat(NOTE_TEXT_MOST);
    let ids: Vec<u64> = (0..6)
        .map(|_| ws.update(cx, |ws, _| ws.notes.add(text.clone(), None, 0)))
        .collect();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            for id in &ids {
                let tab = ws.note_tab(*id, window, cx).unwrap();
                ws.space
                    .blocks
                    .get_mut(&note)
                    .unwrap()
                    .note_mut()
                    .unwrap()
                    .tabs
                    .push(tab);
            }
        })
    })
    .unwrap();
    let (full, scope) = ws.read_with(cx, |ws, cx| (ws.api_state(cx), ws.api_scope(cx)));
    let mine: Vec<_> = full.notes.iter().filter(|n| ids.contains(&n.id)).collect();
    assert_eq!(mine.len(), 6);
    let sent: usize = full.notes.iter().map(|n| json_len(&n.text)).sum();
    assert!(sent <= NOTES_TEXT_MOST, "{sent}");
    let kept: Vec<_> = mine.iter().filter(|n| !n.text.is_empty()).collect();
    let left: Vec<_> = mine.iter().filter(|n| n.text.is_empty()).collect();
    assert_eq!((kept.len(), left.len()), (5, 1));
    assert!(left[0].cut && kept.iter().all(|n| n.cut == (n.text.len() < text.len())));
    // The rule over blocks and workspaces reads none of the content.
    assert!(!scope.blocks.is_empty());
    assert!(scope.notes.is_empty() && scope.checklists.is_empty() && scope.drawings.is_empty());
}

#[gpui_kit::test]
fn a_limited_token_sees_no_note_shown_only_in_another_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (second, _) = another_workspace(handle, &ws, cx);
    super::templum_agent::control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({ "workspace": &second, "kind": "note", "text": "elsewhere" }),
    );
    super::templum_agent::control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({ "workspace": &second, "kind": "checklist", "text": "Far\nOne" }),
    );
    ws.update(cx, |ws, _| {
        ws.in_space(&second, |ws| {
            ws.space.ink.push(Drawing::Shape {
                kind: ShapeKind::Line,
                from: Point::new(0.0, 0.0),
                to: Point::new(5.0, 5.0),
                width: 2.0,
                color: Color::Red,
            });
        })
    });
    let limited = with_token(&ws, cx, Scope::Read, Some(vec![first.as_str()]));
    let all = with_token(&ws, cx, Scope::Read, None);
    let seen = state(handle, &ws, cx, &limited);
    assert!(seen.notes.is_empty() && seen.checklists.is_empty() && seen.drawings.is_empty());
    let everything = state(handle, &ws, cx, &all);
    assert_eq!(everything.notes.len(), 1);
    assert_eq!(everything.checklists.len(), 1);
    assert_eq!(everything.drawings.len(), 1);
    assert_eq!(everything.drawings[0].id, second);
}

#[gpui_kit::test]
fn home_s_drawings_are_capped_at_the_newest(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let stroke = |n: usize, x: f32| Drawing::Stroke {
        points: (0..n).map(|i| Point::new(x, i as f32)).collect(),
        width: 2.0,
        color: Color::Blue,
        highlight: false,
    };
    ws.update(cx, |ws, _| {
        ws.space.ink = (0..2_100).map(|i| stroke(2, i as f32)).collect();
    });
    let token = with_token(&ws, cx, Scope::Read, None);
    let seen = state(handle, &ws, cx, &token);
    let drawings = &seen.drawings[0].drawings;
    assert_eq!(drawings.len(), 2_000);
    assert_eq!(
        drawings[0].points[0][0], 100.0,
        "the oldest 100 are left out"
    );
    assert_eq!(drawings[1_999].points[0][0], 2_099.0, "in paint order");
    ws.update(cx, |ws, _| {
        ws.space.ink = (0..3).map(|i| stroke(40_000, i as f32)).collect();
    });
    let seen = state(handle, &ws, cx, &token);
    let drawings = &seen.drawings[0].drawings;
    assert_eq!(drawings.len(), 2);
    assert_eq!(drawings[0].points[0][0], 1.0);
}

#[gpui_kit::test]
fn the_state_shows_workspaces_blocks_agents_and_cards(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, terminal) = three_blocks(handle, &ws, cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let token = with_token(&ws, cx, Scope::Read, None);
    let seen = state(handle, &ws, cx, &token);
    assert_eq!(seen.app.api, 1);
    assert_eq!(seen.app.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(seen.workspaces.len(), 1);
    assert!(seen.workspaces[0].shown && !seen.workspaces[0].stopped);
    assert_eq!(
        seen.app.shown.as_deref(),
        Some(seen.workspaces[0].id.as_str())
    );
    let (canvas, uids) = ws.read_with(cx, |ws, _| {
        (ws.space.canvas.blocks().to_vec(), ws.space.uids.clone())
    });
    // Back to front, with the canvas's rects.
    assert_eq!(seen.blocks.len(), canvas.len());
    for (z, (view, block)) in seen.blocks.iter().zip(&canvas).enumerate() {
        assert_eq!((view.id.as_str(), view.z), (uids[&block.id].as_str(), z));
        assert_eq!(
            (view.rect.x, view.rect.y, view.rect.w, view.rect.h),
            (block.rect.x, block.rect.y, block.rect.w, block.rect.h)
        );
    }
    let kind = |id: BlockId| seen.blocks.iter().find(|b| b.id == uids[&id]).unwrap();
    assert_eq!(
        (kind(terminal).kind.as_str(), kind(kanban).kind.as_str()),
        ("terminal", "kanban")
    );
    assert_eq!(
        seen.boards.iter().find(|b| b.id == board).unwrap().columns[0].cards,
        [card]
    );
    let view = seen.cards.iter().find(|c| c.id == card).unwrap();
    assert_eq!(view.board, board);
    assert_eq!(
        view.column,
        seen.boards.iter().find(|b| b.id == board).unwrap().columns[0].id
    );
}

#[gpui_kit::test]
fn the_state_has_icons_read_only_blocks_and_shares(cx: &mut TestAppContext) {
    use templum_api::store::{Share, Way};
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    ws.update(cx, |ws, _| {
        ws.space.look.icon = Some(gpui_kit::assets::IconName::Rocket);
        ws.space.terminal_mut(terminal).unwrap().follows =
            Some(crate::workspace::subagents::Follows {
                parent: "p".into(),
                id: "s1".into(),
                done: None,
            });
    });
    let token = with_token(&ws, cx, Scope::Full, None);
    let share_id = ws.update(cx, |ws, _| {
        let store = ws.api.store.as_mut().unwrap();
        let (id, text) = store
            .mint_share(
                Share {
                    workspace: first.clone(),
                    way: Way::Pinned,
                    address: "192.168.1.5:7411".into(),
                },
                "Another computer",
                Scope::Full,
                None,
                5,
            )
            .unwrap();
        assert!(text.starts_with("tpl_"));
        id
    });
    let seen = state(handle, &ws, cx, &token);
    assert_eq!(seen.workspaces[0].icon.as_deref(), Some("rocket"));
    let uids = ws.read_with(cx, |ws, _| ws.space.uids.clone());
    let read_only = |id: BlockId| {
        seen.blocks
            .iter()
            .find(|b| b.id == uids[&id])
            .unwrap()
            .read_only
    };
    assert!(read_only(terminal) && !read_only(note));
    assert_eq!(
        seen.shares,
        [Arc::new(templum_api::model::ShareView {
            id: format!("s{share_id}"),
            workspace: first,
            name: "Another computer".into(),
            scope: Scope::Full,
            created: 5,
            expires: None,
            way: Way::Pinned,
            address: "192.168.1.5:7411".into(),
            last: None,
            mirrored: false,
        })]
    );
    let json = serde_json::to_string(&seen).unwrap();
    assert!(!json.contains("tpl_"), "no token text in the state");
}

#[gpui_kit::test]
fn an_agents_state_is_the_agents_views(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (a, b) = super::agents_view::one_done(handle, &ws, cx);
    let token = with_token(&ws, cx, Scope::Read, None);
    let (uid_a, uid_b) = ws.read_with(cx, |ws, _| {
        (ws.space.uids[&a.0].clone(), ws.space.uids[&b.0].clone())
    });
    let agent_of = |seen: &templum_api::State, uid: &str| {
        seen.blocks
            .iter()
            .find(|x| x.id == uid)
            .unwrap()
            .agent
            .clone()
            .unwrap()
    };
    let watch = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.update(cx, |ws, cx| {
            ws.watch_agents(false, cx.background_executor().now())
        })
    };
    // Ended while not looked at: done. Looked at: idle.
    let seen = state(handle, &ws, cx, &token);
    assert_eq!(agent_of(&seen, &uid_b).state, "done");
    assert!(agent_of(&seen, &uid_b).since.is_some());
    assert_eq!(agent_of(&seen, &uid_a).state, "idle");
    assert_eq!(agent_of(&seen, &uid_a).agent, "claude");
    // At work, then asking: needs you, with what it asks.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a.0).unwrap().state = Some(AgentState::Working)
    });
    watch(&ws, cx);
    assert_eq!(
        agent_of(&state(handle, &ws, cx, &token), &uid_a).state,
        "working"
    );
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(a.0).unwrap().state =
            Some(AgentState::NeedsYou(Some("Allow Bash?".into())))
    });
    watch(&ws, cx);
    let asking = agent_of(&state(handle, &ws, cx, &token), &uid_a);
    assert_eq!(
        (asking.state.as_str(), asking.asks.as_deref()),
        ("needs_you", Some("Allow Bash?"))
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn the_state_shows_an_agent_s_task_and_role_and_the_schedules(cx: &mut TestAppContext) {
    use super::delegate::{agents_running_in, hand_over};
    let (handle, ws, mine, theirs) = agents_running_in(cx, None, None);
    let (terminal, uid) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(theirs).unwrap().role = Some("reviewer".into())
    });
    let task = json!({ "block": them, "task": "Ship it." });
    let (failed, text) = hand_over(handle, &ws, cx, (&terminal, theirs), task, true);
    assert!(!failed, "{text}");
    let at = std::time::SystemTime::now();
    ws.update(cx, |ws, cx| {
        ws.schedule_add(
            crate::workspace::schedules::Schedule {
                id: 0,
                by: terminal.clone(),
                to: them.clone(),
                text: "Check the deploy".into(),
                next: at,
                every: Some(std::time::Duration::from_secs(600)),
                until: at,
            },
            cx,
        );
    });
    let token = with_token(&ws, cx, Scope::Read, None);
    let seen = state(handle, &ws, cx, &token);
    let agent = |uid: &str| {
        seen.blocks
            .iter()
            .find(|b| b.id == uid)
            .and_then(|b| b.agent.clone())
            .unwrap()
    };
    let task = agent(&them).task.unwrap();
    assert_eq!(
        (task.id.as_str(), task.text.as_str(), task.by.as_deref()),
        ("t1", "Ship it.", Some(uid.as_str()))
    );
    let now = crate::workspace::automations::unix(std::time::SystemTime::now());
    assert!((now - task.since).abs() < 5, "{} vs {now}", task.since);
    assert_eq!(agent(&them).role.as_deref(), Some("reviewer"));
    assert_eq!(
        seen.blocks
            .iter()
            .find(|b| b.id == uid)
            .and_then(|b| b.agent.as_ref())
            .and_then(|a| a.task.clone()),
        None
    );
    let unix = crate::workspace::automations::unix;
    assert_eq!(
        seen.schedules
            .iter()
            .map(|s| (**s).clone())
            .collect::<Vec<_>>(),
        [templum_api::model::ScheduleView {
            id: "s1".into(),
            by: Some(uid),
            to: them,
            text: "Check the deploy".into(),
            next: unix(at),
            every: Some(600),
            until: unix(at),
        }]
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_task_handed_through_the_api_is_by_api(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = agents_running(cx, None);
    let (worker, them) = me(&ws, cx, theirs);
    let token = with_token(&ws, cx, Scope::Control, None);
    let _ = api_later(
        handle,
        &ws,
        cx,
        &token,
        7,
        "tools/call",
        json!({ "name": "delegate", "arguments": { "block": them, "task": "Green." } }),
    );
    screen_with(&ws, cx, theirs, "[from API · phone, task t");
    entered(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    let seen = state(handle, &ws, cx, &token);
    let task = seen
        .blocks
        .iter()
        .find(|b| b.id == them)
        .and_then(|b| b.agent.as_ref())
        .and_then(|a| a.task.clone())
        .unwrap();
    assert_eq!(
        (task.text.as_str(), task.by.as_deref()),
        ("Green.", Some("api"))
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn cancel_schedule_cancels_a_schedule_the_token_sees(cx: &mut TestAppContext) {
    use super::delegate::agents_running_in;
    let (handle, ws, mine, theirs) = agents_running_in(cx, None, None);
    let (terminal, _) = me(&ws, cx, mine);
    let (_, them) = me(&ws, cx, theirs);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, elsewhere) = another_workspace(handle, &ws, cx);
    let at = std::time::SystemTime::now();
    let schedule = |to: &str| crate::workspace::schedules::Schedule {
        id: 0,
        by: terminal.clone(),
        to: to.into(),
        text: "x".into(),
        next: at,
        every: None,
        until: at,
    };
    ws.update(cx, |ws, cx| {
        ws.schedule_add(schedule(&them), cx);
        ws.schedule_add(schedule(&elsewhere), cx);
    });
    let token = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str()]));
    let ids = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.schedules.iter().map(|s| s.id).collect::<Vec<_>>()
        })
    };
    let line = tool(
        handle,
        &ws,
        cx,
        &token,
        "cancel_schedule",
        json!({ "schedule": "s1" }),
    );
    assert_eq!(said(&line), (false, "Cancelled s1.".to_string()), "{line}");
    assert_eq!(ids(&ws, cx), [2]);
    let line = tool(
        handle,
        &ws,
        cx,
        &token,
        "cancel_schedule",
        json!({ "schedule": "2" }),
    );
    assert_eq!(
        line["error"]["message"],
        "There's no schedule s2: GET /v1/state lists them."
    );
    assert_eq!(ids(&ws, cx), [2]);
    let read = with_token(&ws, cx, Scope::Read, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &read,
        "cancel_schedule",
        json!({ "schedule": "s2" }),
    );
    assert_eq!(
        line["error"]["message"],
        "cancel_schedule needs a Control token."
    );
    assert_eq!(ids(&ws, cx), [2]);
}

/// `recent_folders` (kanban card 203): the workspace's recent folders, newest first, for a Read
/// token. A token limited to it gets only those inside its folder, links followed, as their real
/// paths, each once: never a folder it couldn't open, nor a link's own path outside. A workspace
/// it doesn't see is one that doesn't exist.
#[gpui_kit::test]
fn recent_folders_are_confined_to_a_limited_token_s_folders(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (project, elsewhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let root = dunce::canonicalize(project.path()).unwrap();
    let away = dunce::canonicalize(elsewhere.path()).unwrap();
    let sub = root.join("sub");
    std::fs::create_dir(&sub).unwrap();
    let recent = vec![away.clone(), sub.clone(), root.join("gone"), root.clone()];
    // A link inside the folder that leads out of it, and one outside that leads into it.
    #[cfg(unix)]
    let recent = {
        std::os::unix::fs::symlink(&away, root.join("out")).unwrap();
        std::os::unix::fs::symlink(&sub, away.join("in")).unwrap();
        [recent, vec![root.join("out"), away.join("in")]].concat()
    };
    let id = ws.update(cx, |ws, _| {
        ws.space.folder = Some(root.clone());
        ws.space.recent = recent.clone();
        ws.space.id.clone()
    });
    let (other, _) = another_workspace(handle, &ws, cx);
    let lines = |folders: &[&std::path::Path]| {
        let lines: Vec<String> = folders.iter().map(|f| f.display().to_string()).collect();
        (false, lines.join("\n"))
    };
    let all = with_token(&ws, cx, Scope::Read, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &all,
        "recent_folders",
        json!({ "workspace": id }),
    );
    let every: Vec<&std::path::Path> = recent.iter().map(|f| f.as_path()).collect();
    assert_eq!(said(&line), lines(&every), "{line}");
    let limited = with_token(&ws, cx, Scope::Read, Some(vec![id.as_str()]));
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "recent_folders",
        json!({ "workspace": id }),
    );
    assert_eq!(said(&line), lines(&[&sub, &root]), "{line}");
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "recent_folders",
        json!({ "workspace": other }),
    );
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::NOT_FOUND)),
            format!("There's no workspace {other}: GET /v1/workspaces lists them.")
        )
    );
    // A workspace with no folder gives a limited token nothing.
    ws.update(cx, |ws, _| ws.space.folder = None);
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "recent_folders",
        json!({ "workspace": id }),
    );
    assert_eq!(said(&line), (false, String::new()), "{line}");
}

#[gpui_kit::test]
fn a_question_waits_in_the_state(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, uid) = me(&ws, cx, first);
    let token = with_token(&ws, cx, Scope::Read, None);
    let asked = json!({ "questions": [{ "question": "Which database?", "header": "DB",
        "options": [{ "label": "Postgres" }, { "label": "SQLite" }] }] });
    let waiting = call_later(handle, &ws, cx, &terminal, 1, "ask_user", asked);
    assert!(waiting.try_recv().is_err());
    let seen = state(handle, &ws, cx, &token);
    assert_eq!(seen.requests.len(), 1);
    let request = &seen.requests[0];
    assert!(request.id.starts_with('r'));
    assert_eq!(
        (
            request.kind.as_str(),
            request.block.as_deref(),
            request.scope
        ),
        ("question", Some(uid.as_str()), Scope::Control)
    );
    assert_eq!(request.questions[0].question, "Which database?");
    assert_eq!(
        request.questions[0]
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        ["Postgres", "SQLite"]
    );
    let plan = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "propose_plan",
        json!({ "plan": "Do the thing." }),
    );
    assert!(plan.try_recv().is_err());
    let seen = state(handle, &ws, cx, &token);
    let plan = seen.requests.iter().find(|r| r.kind == "plan").unwrap();
    assert_eq!(plan.plan.as_ref().unwrap().text, "Do the thing.");
}

#[gpui_kit::test]
fn a_read_token_cant_act_and_a_control_token_cant_type(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, terminal) = three_blocks(handle, &ws, cx);
    let (_, board, _) = board_with_a_card(handle, &ws, cx);
    let read = with_token(&ws, cx, Scope::Read, None);
    let control = with_token(&ws, cx, Scope::Control, None);
    let cards = ws.read_with(cx, |ws, _| {
        ws.boards.board(board).unwrap().columns[0].cards.len()
    });
    let line = tool(
        handle,
        &ws,
        cx,
        &read,
        "create_card",
        json!({ "board": board.to_string(), "title": "x" }),
    );
    assert_eq!(
        (
            line["error"]["code"].as_i64(),
            line["error"]["message"].as_str()
        ),
        (
            Some(i64::from(codes::SCOPE)),
            Some("create_card needs a Control token.")
        )
    );
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&terminal].clone());
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "send_to_terminal",
        json!({ "block": uid, "text": "ls" }),
    );
    assert_eq!(
        (
            line["error"]["code"].as_i64(),
            line["error"]["message"].as_str()
        ),
        (
            Some(i64::from(codes::SCOPE)),
            Some("send_to_terminal needs a Full token.")
        )
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.boards.board(board).unwrap().columns[0]
            .cards
            .len()),
        cards
    );
    let _ = kanban;
    // A read token reads.
    let line = tool(handle, &ws, cx, &read, "overview", json!({}));
    assert!(!said(&line).0 && said(&line).1.contains(&uid));
}

#[gpui_kit::test]
fn a_limited_token_sees_and_touches_only_its_workspaces(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (first_space, first_uid) = ws.read_with(cx, |ws, _| {
        (ws.space.id.clone(), ws.space.uids[&first].clone())
    });
    let (_, text) = super::templum_agent::control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({ "name": "Other" }),
    );
    assert!(text.contains("Created workspace"), "{text}");
    let (_, text) = super::templum_agent::control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({ "workspace": "w2", "kind": "terminal" }),
    );
    let far = text
        .trim_end_matches('.')
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string();
    let token = with_token(&ws, cx, Scope::Control, Some(vec![first_space.as_str()]));
    let seen = state(handle, &ws, cx, &token);
    assert_eq!(
        seen.workspaces
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>(),
        [first_space.as_str()]
    );
    assert!(
        seen.blocks.iter().all(|b| b.workspace == first_space)
            && seen.blocks.iter().any(|b| b.id == first_uid)
    );
    assert!(seen.blocks.iter().all(|b| b.id != far));
    // What is outside is answered as what doesn't exist.
    let refused = |line: &Value| {
        (
            line["error"]["code"].as_i64(),
            line["error"]["message"].as_str().map(String::from),
        )
    };
    let outside = tool(
        handle,
        &ws,
        cx,
        &token,
        "read_terminal",
        json!({ "block": far }),
    );
    let nothing = tool(
        handle,
        &ws,
        cx,
        &token,
        "read_terminal",
        json!({ "block": "zzzzz" }),
    );
    assert_eq!(refused(&outside).0, Some(i64::from(codes::NOT_FOUND)));
    assert_eq!(
        refused(&outside).1,
        Some(format!(
            "There's no block {far}: GET /v1/blocks lists them."
        ))
    );
    assert_eq!(
        refused(&nothing).1,
        Some("There's no block zzzzz: GET /v1/blocks lists them.".to_string())
    );
    let made = tool(
        handle,
        &ws,
        cx,
        &token,
        "create_workspace",
        json!({ "name": "Mine" }),
    );
    assert_eq!(refused(&made).0, Some(i64::from(codes::SCOPE)));
    let workspace = tool(
        handle,
        &ws,
        cx,
        &token,
        "show",
        json!({ "workspace": "w2" }),
    );
    assert_eq!(refused(&workspace).0, Some(i64::from(codes::NOT_FOUND)));
    // Moved into its workspace, the block is its own.
    let (_, text) =
        super::templum_agent::control(handle, &ws, cx, "show", json!({ "workspace": "w2" }));
    assert!(text.contains("Showing"), "{text}");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let id = ws.space.block_by_uid(&far).unwrap();
            ws.move_blocks(vec![id], Some(&first_space), window, cx);
        })
    })
    .unwrap();
    let inside = tool(
        handle,
        &ws,
        cx,
        &token,
        "read_terminal",
        json!({ "block": far }),
    );
    assert!(inside["error"].is_null(), "{inside}");
}

#[gpui_kit::test]
fn an_api_call_skips_the_gate_and_acts_as_the_person(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let full = with_token(&ws, cx, Scope::Full, None);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&terminal].clone());
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "send_to_terminal",
        json!({ "block": uid, "text": "ls" }),
    );
    assert!(!said(&line).0, "{line}");
    ws.read_with(cx, |ws, _| {
        assert!(ws.templum_agent.approvals.is_empty(), "no approval waits")
    });
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "comment_card",
        json!({ "card": card, "text": "hi" }),
    );
    assert!(!said(&line).0, "{line}");
    let by = ws.read_with(cx, |ws, _| {
        ws.boards.board(board).unwrap().columns[0].cards[0]
            .comments
            .last()
            .unwrap()
            .by
            .clone()
    });
    assert_eq!(by, "you");
    let rows = ws.read_with(cx, |ws, _| ws.templum_agent.conversation.rows.len());
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "create_card",
        json!({ "board": board.to_string(), "title": "n" }),
    );
    assert!(!said(&line).0);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.templum_agent.conversation.rows.len()),
        rows,
        "the chat is untouched"
    );
    assert!(ws.read_with(cx, |ws, _| {
        ws.templum_agent.conversation.own.cards.is_empty()
    }));
}

#[gpui_kit::test]
fn add_block_goes_at_x_and_y(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let token = with_token(&ws, cx, Scope::Control, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &token,
        "add_block",
        json!({ "workspace": first, "kind": "kanban", "x": 1234.0, "y": -567.0 }),
    );
    assert!(!said(&line).0, "{line}");
    let uid = line["result"]["target"]["block"].as_str().unwrap();
    ws.read_with(cx, |ws, _| {
        let id = ws.space.block_by_uid(uid).unwrap();
        let at = ws.space.canvas.get(id).unwrap().rect.origin();
        assert_eq!((at.x, at.y), (1234.0, -567.0));
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn open_agent_at_x_and_y_answers_its_block(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    super::templum_agent::cat_agents(&ws, cx);
    let folder = tempfile::tempdir().unwrap();
    let first = ws.update(cx, |ws, _| {
        ws.space.folder = Some(folder.path().to_path_buf());
        ws.space.id.clone()
    });
    let token = with_token(&ws, cx, Scope::Control, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &token,
        "open_agent",
        json!({ "agent": "codex", "workspace": first, "x": 300.0, "y": 200.0 }),
    );
    assert!(!said(&line).0, "{line}");
    let uid = line["result"]["target"]["block"].as_str().unwrap();
    ws.read_with(cx, |ws, _| {
        let id = ws.space.block_by_uid(uid).unwrap();
        assert!(ws.space.terminal(id).is_some());
        let at = ws.space.canvas.get(id).unwrap().rect.origin();
        assert_eq!((at.x, at.y), (300.0, 200.0));
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_task_from_the_api_is_typed_from_it_and_its_end_is_a_notice(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = agents_running(cx, None);
    let (worker, them) = me(&ws, cx, theirs);
    let token = with_token(&ws, cx, Scope::Control, None);
    // Answered once the agent takes the task: the call isn't waited for here.
    let _ = api_later(
        handle,
        &ws,
        cx,
        &token,
        7,
        "tools/call",
        json!({
            "name": "delegate", "arguments": { "block": them, "task": "Green." }
        }),
    );
    screen_with(&ws, cx, theirs, "[from API · phone, task t");
    entered(&ws, cx, theirs);
    event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    event(
        handle,
        &ws,
        cx,
        &worker,
        json!({ "event": "stopped", "message": "Green." }),
    );
    after(cx, 1000);
    let notices = ws.read_with(cx, |ws, _| ws.api.notices.clone());
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].text.contains("task t1") && notices[0].text.contains("Green."),
        "{}",
        notices[0].text
    );
    assert_eq!(notices[0].block.as_deref(), Some(them.as_str()));
    assert!(notices[0].workspace.is_some());
    ws.read_with(cx, |ws, _| {
        assert!(ws.notices.is_empty(), "nothing is queued to type")
    });
}

/// A watch set through the API carries the watched block and its workspace, so a token limited to
/// that workspace hears its notice.
#[cfg(unix)]
#[gpui_kit::test]
fn a_watch_set_through_the_api_names_its_block_and_workspace(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = agents_running(cx, None);
    let (_, them) = me(&ws, cx, theirs);
    let token = with_token(&ws, cx, Scope::Control, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &token,
        "wait_for",
        json!({ "block": them, "pattern": "ERROR", "wait": false, "timeout": 300 }),
    );
    let (refused, text) = said(&line);
    assert!(
        !refused && text.contains("a match comes back as a notice event."),
        "{text}"
    );
    prints(&ws, cx, theirs, b"ERROR one\r");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while ws.read_with(cx, |ws, _| ws.api.notices.is_empty()) {
        after(cx, 250);
        assert!(std::time::Instant::now() < deadline, "no notice came");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let notices = ws.read_with(cx, |ws, _| ws.api.notices.clone());
    assert_eq!(
        notices[0].block.as_deref(),
        Some(them.as_str()),
        "{notices:?}"
    );
    let workspace = ws.read_with(cx, |ws, _| ws.space.id.clone());
    assert_eq!(notices[0].workspace.as_deref(), Some(workspace.as_str()));
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_closed_api_call_drops_its_wait(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = agents_running(cx, None);
    let (_, them) = me(&ws, cx, theirs);
    let token = with_token(&ws, cx, Scope::Control, None);
    let waiting = api_later(
        handle,
        &ws,
        cx,
        &token,
        5,
        "tools/call",
        json!({
            "name": "delegate", "arguments": { "block": them, "task": "Slow.", "wait": true }
        }),
    );
    assert!(waiting.try_recv().is_err(), "it waits");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.waits.len(), 1);
        assert!(ws.space.terminal(theirs).unwrap().handed.is_some());
    });
    let (reply, _) = std::sync::mpsc::channel();
    let message = Message {
        hello: Hello {
            terminal: token.id.to_string(),
            client: "api".into(),
            control: None,
        },
        conn: 5,
        incoming: Incoming::Notification {
            method: templum_mcp::bus::CLOSED.into(),
            params: Value::Null,
        },
        reply,
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.serve(message, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.waits.is_empty(), "the wait is gone");
        assert!(
            ws.space.terminal(theirs).unwrap().handed.is_none(),
            "and so is the task's master"
        );
    });
}

#[gpui_kit::test]
fn a_revoked_token_is_answered_auth(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let token = with_token(&ws, cx, Scope::Read, None);
    assert!(tool(handle, &ws, cx, &token, "overview", json!({}))["error"].is_null());
    ws.update(cx, |ws, _| ws.api.store.as_mut().unwrap().revoke_all());
    let line = tool(handle, &ws, cx, &token, "overview", json!({}));
    assert_eq!(line["error"]["code"].as_i64(), Some(i64::from(codes::AUTH)));
    assert_eq!(line["error"]["message"], "This token was revoked.");
    let line = api_call(handle, &ws, cx, &token, templum_api::STATE, Value::Null);
    assert_eq!(line["error"]["code"].as_i64(), Some(i64::from(codes::AUTH)));
}

/// The ids of the requests in the state.
fn requests(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
) -> Vec<String> {
    state(handle, ws, cx, token)
        .requests
        .into_iter()
        .map(|r| r.id.clone())
        .collect()
}

/// `answer_request`'s answer line.
fn answer_request(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
    arguments: Value,
) -> Value {
    tool(handle, ws, cx, token, "answer_request", arguments)
}

fn question() -> Value {
    json!({ "questions": [{ "question": "Which database?", "header": "DB",
        "options": [{ "label": "Postgres" }, { "label": "SQLite" }] }] })
}

#[gpui_kit::test]
fn answering_a_question_from_the_api(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let token = with_token(&ws, cx, Scope::Control, None);
    let asker = call_later(handle, &ws, cx, &terminal, 1, "ask_user", question());
    let id = requests(handle, &ws, cx, &token).remove(0);
    let line = answer_request(
        handle,
        &ws,
        cx,
        &token,
        json!({ "request": id, "answers": [["Postgres"]] }),
    );
    assert_eq!(said(&line), (false, "Answered.".to_string()));
    let (failed, text) = answered(&asker);
    assert!(!failed);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({ "answers": { "Which database?": "Postgres" } })
    );
    assert!(requests(handle, &ws, cx, &token).is_empty());
    let again = answer_request(
        handle,
        &ws,
        cx,
        &token,
        json!({ "request": id, "answers": [["Postgres"]] }),
    );
    assert_eq!(
        again["error"]["code"].as_i64(),
        Some(i64::from(codes::NOT_FOUND))
    );
    assert_eq!(
        again["error"]["message"],
        format!("There's no request {id}: GET /v1/requests lists them.")
    );
}

#[gpui_kit::test]
fn a_wrong_answer_is_refused_and_the_request_stays(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let token = with_token(&ws, cx, Scope::Control, None);
    let asker = call_later(handle, &ws, cx, &terminal, 1, "ask_user", question());
    let id = requests(handle, &ws, cx, &token).remove(0);
    let line = answer_request(
        handle,
        &ws,
        cx,
        &token,
        json!({ "request": id, "answers": [["Postgres", "SQLite"]] }),
    );
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(templum_mcp::rpc::PARAMS))
    );
    assert_eq!(line["error"]["message"], "Question 1 takes one answer.");
    let line = answer_request(
        handle,
        &ws,
        cx,
        &token,
        json!({ "request": id, "allow": true }),
    );
    assert_eq!(
        line["error"]["message"],
        "A question takes answers or dismiss."
    );
    assert_eq!(requests(handle, &ws, cx, &token), [id], "it still waits");
    assert!(asker.try_recv().is_err());
}

#[gpui_kit::test]
fn a_plan_is_rejected_with_a_comment(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, first);
    let token = with_token(&ws, cx, Scope::Control, None);
    let asker = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        1,
        "propose_plan",
        json!({ "plan": "Everything." }),
    );
    let id = requests(handle, &ws, cx, &token).remove(0);
    let line = answer_request(
        handle,
        &ws,
        cx,
        &token,
        json!({ "request": id, "answers": [["Reject"]], "note": "too big" }),
    );
    assert_eq!(said(&line), (false, "Answered.".to_string()));
    assert_eq!(
        answered(&asker),
        (true, "Rejected.\nThe person's comment: too big".to_string())
    );
}

#[gpui_kit::test]
fn dismiss_and_allow(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let token = with_token(&ws, cx, Scope::Control, None);
    let asker = call_later(handle, &ws, cx, &terminal, 1, "ask_user", question());
    let id = requests(handle, &ws, cx, &token).remove(0);
    let line = answer_request(
        handle,
        &ws,
        cx,
        &token,
        json!({ "request": id, "dismiss": true }),
    );
    assert!(!said(&line).0, "{line}");
    assert_eq!(
        answered(&asker),
        (
            true,
            "The person dismissed the question without answering.".to_string()
        )
    );
    // A request to connect is answered with allow.
    let k = ws.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    let connecting = call_later(
        handle,
        &ws,
        cx,
        &terminal,
        2,
        "connect",
        json!({ "block": k }),
    );
    let request = state(handle, &ws, cx, &token).requests.remove(0);
    assert_eq!(
        (request.kind.as_str(), request.buttons.clone()),
        ("connect", Some(["Allow".to_string(), "Deny".to_string()]))
    );
    let line = answer_request(
        handle,
        &ws,
        cx,
        &token,
        json!({ "request": request.id, "answers": [["x"]] }),
    );
    assert_eq!(line["error"]["message"], "This request takes allow.");
    let line = answer_request(
        handle,
        &ws,
        cx,
        &token,
        json!({ "request": request.id, "allow": true }),
    );
    assert!(!said(&line).0, "{line}");
    assert!(!answered(&connecting).0);
    ws.read_with(cx, |ws, _| assert!(ws.space.wired(mine, kanban)));
}

#[gpui_kit::test]
fn an_approval_needs_a_full_token(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&terminal].clone());
    let control = with_token(&ws, cx, Scope::Control, None);
    let full = with_token(&ws, cx, Scope::Full, None);
    let waiting = super::templum_agent::control_later(
        handle,
        &ws,
        cx,
        7,
        "send_to_terminal",
        json!({ "block": uid, "text": "ls", "enter": false }),
    );
    assert!(waiting.try_recv().is_err(), "it waits for approval");
    let request = state(handle, &ws, cx, &full).requests.remove(0);
    assert!(request.id.starts_with('a'), "{}", request.id);
    assert_eq!(
        (
            request.kind.as_str(),
            request.tool.as_deref(),
            request.scope
        ),
        ("approval", Some("send_to_terminal"), Scope::Full)
    );
    assert!(request.said.is_some());
    // A limited token never sees it; a Control token sees it and can't answer it.
    let limited = with_token(&ws, cx, Scope::Full, Some(vec!["w1"]));
    assert!(state(handle, &ws, cx, &limited).requests.is_empty());
    let line = answer_request(
        handle,
        &ws,
        cx,
        &control,
        json!({ "request": request.id, "allow": true }),
    );
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::SCOPE))
    );
    assert!(waiting.try_recv().is_err());
    let line = answer_request(
        handle,
        &ws,
        cx,
        &full,
        json!({ "request": request.id, "allow": true }),
    );
    assert_eq!(said(&line), (false, "Answered.".to_string()), "{line}");
    cx.run_until_parked();
    let (failed, _) = super::bus::result_of(&waiting.try_recv().expect("the call ran"));
    assert!(!failed);
    assert!(state(handle, &ws, cx, &full).requests.is_empty());
}

#[gpui_kit::test]
fn automations_from_the_api(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let control = with_token(&ws, cx, Scope::Control, None);
    let full = with_token(&ws, cx, Scope::Full, None);
    let notify = json!({ "name": "Ping", "workspace": space,
        "when": { "every": { "minutes": 10 } }, "do": { "notify": { "text": "hello" } } });
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "save_automation",
        json!({ "automation": notify }),
    );
    assert_eq!(
        said(&line),
        (false, "Saved automation 1.".to_string()),
        "{line}"
    );
    let listed = state(handle, &ws, cx, &control).automations;
    assert_eq!(
        (listed[0].id, listed[0].name.as_str(), listed[0].on),
        (1, "Ping", true)
    );
    assert!(listed[0].sentence.contains("10"), "{}", listed[0].sentence);
    assert_eq!(listed[0].act, json!({ "notify": { "text": "hello" } }));
    assert!(listed[0].next.is_some());
    // Replaced, not added.
    let again = json!({ "name": "Pong", "workspace": space,
        "when": { "every": { "minutes": 5 } }, "do": { "notify": { "text": "hi" } } });
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "save_automation",
        json!({ "automation": again, "id": 1 }),
    );
    assert_eq!(said(&line).1, "Saved automation 1.");
    assert_eq!(state(handle, &ws, cx, &control).automations.len(), 1);
    // The editor's checks and words.
    let blank = json!({ "name": "x", "workspace": space, "when": { "every": { "minutes": 5 } }, "do": { "notify": { "text": " " } } });
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "save_automation",
        json!({ "automation": blank }),
    );
    assert_eq!(line["error"]["message"], "Write the text.");
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "set_automation",
        json!({ "automation": 1, "on": false }),
    );
    assert_eq!(said(&line).1, "Turned automation 1 off.");
    assert!(!state(handle, &ws, cx, &control).automations[0].on);
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "run_automation",
        json!({ "automation": 1 }),
    );
    assert_eq!(said(&line).1, "Ran automation 1.", "{line}");
    assert!(
        state(handle, &ws, cx, &control).automations[0]
            .last
            .is_some()
    );
    // A command is a Full token's; deleting is too.
    let command = json!({ "name": "Run", "workspace": space,
        "when": { "every": { "minutes": 5 } }, "do": { "command": { "text": "ls" } } });
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "save_automation",
        json!({ "automation": command }),
    );
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::SCOPE))
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "delete_automation",
        json!({ "automation": 1 }),
    );
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::SCOPE))
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "delete_automation",
        json!({ "automation": 1 }),
    );
    assert_eq!(said(&line).1, "Deleted automation 1.");
    assert!(state(handle, &ws, cx, &full).automations.is_empty());
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "delete_automation",
        json!({ "automation": 1 }),
    );
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::NOT_FOUND))
    );
}

/// An event rule saved through the API shows its cooldown in the state; a time rule has none.
#[gpui_kit::test]
fn an_event_rules_cooldown_is_in_the_state(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let control = with_token(&ws, cx, Scope::Control, None);
    let event = json!({ "name": "Done", "workspace": space, "cooldown": 3,
        "when": { "agent": { "event": "idle", "who": { "kind": "claude" } } },
        "do": { "notify": { "text": "a Claude finished" } } });
    let time = json!({ "name": "Ping", "workspace": space,
        "when": { "every": { "minutes": 10 } }, "do": { "notify": { "text": "hello" } } });
    for rule in [event, time] {
        let line = tool(
            handle,
            &ws,
            cx,
            &control,
            "save_automation",
            json!({ "automation": rule }),
        );
        assert!(!said(&line).0, "{line}");
    }
    let listed = state(handle, &ws, cx, &control).automations;
    assert_eq!(
        listed[0].when,
        json!({ "agent": { "event": "idle", "who": { "kind": "claude" } } })
    );
    assert_eq!((listed[0].cooldown, listed[1].cooldown), (Some(3), None));
}

#[gpui_kit::test]
fn a_limited_token_saves_automations_only_for_its_workspaces(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let all = with_token(&ws, cx, Scope::Control, None);
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![space.as_str()]));
    let rule = |workspace: Option<&str>| {
        let mut rule = json!({ "name": "Ping", "when": { "every": { "minutes": 5 } }, "do": { "notify": { "text": "hi" } } });
        if let Some(w) = workspace {
            rule["workspace"] = json!(w);
        }
        json!({ "automation": rule })
    };
    // For all workspaces, or for one it doesn't see: no.
    let line = tool(handle, &ws, cx, &limited, "save_automation", rule(None));
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::SCOPE))
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "save_automation",
        rule(Some("w9")),
    );
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::NOT_FOUND))
    );
    // Its own workspace: yes. And another's rule can't be replaced or run by it.
    assert!(
        !said(&tool(
            handle,
            &ws,
            cx,
            &limited,
            "save_automation",
            rule(Some(&space))
        ))
        .0
    );
    assert!(!said(&tool(handle, &ws, cx, &all, "save_automation", rule(None))).0);
    let theirs = state(handle, &ws, cx, &all)
        .automations
        .iter()
        .find(|a| a.workspace.is_none())
        .unwrap()
        .id;
    let limited_full = with_token(&ws, cx, Scope::Full, Some(vec![space.as_str()]));
    for (name, arguments) in [
        ("run_automation", json!({ "automation": theirs })),
        ("delete_automation", json!({ "automation": theirs })),
    ] {
        let line = tool(handle, &ws, cx, &limited_full, name, arguments);
        assert_eq!(
            line["error"]["code"].as_i64(),
            Some(i64::from(codes::NOT_FOUND)),
            "{name}"
        );
    }
    let mut replace = rule(Some(&space));
    replace["id"] = json!(theirs);
    let line = tool(handle, &ws, cx, &limited, "save_automation", replace);
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::NOT_FOUND))
    );
    // And it can't type into a block it doesn't see.
    let mut typed = rule(Some(&space));
    typed["automation"]["do"] = json!({ "prompt": { "block": "zzzzz", "text": "x" } });
    let line = tool(handle, &ws, cx, &limited, "save_automation", typed);
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::NOT_FOUND))
    );
}

// Turning it on: the store, the server, the tick, Settings ▸ API and the pill.

use std::time::Duration;
use templum_api::store::{Listen, hash};

/// A workspace with a data folder (the API keeps its file there).
pub(super) fn with_data(
    cx: &mut TestAppContext,
) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    (handle, ws, dir)
}

/// The API on, on an ephemeral port of this computer.
pub(super) fn turn_on(ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    ws.update(cx, |ws, cx| {
        ws.api_edit(
            |s| {
                s.enabled = true;
                s.port = 0;
            },
            cx,
        )
    });
}

pub(super) fn port(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> u16 {
    ws.read_with(cx, |ws, _| {
        ws.api.server.as_ref().expect("listening").addr().port()
    })
}

/// The status of `GET path` with `token` (none: no header), over a plain connection. Only what the
/// server alone answers (401, 404) is asked: the UI thread isn't pumped while this waits.
fn status(port: u16, path: &str, token: Option<&str>) -> u16 {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(10)))
        .unwrap();
    let auth = token
        .map(|t| format!("Authorization: Bearer {t}\r\n"))
        .unwrap_or_default();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: x\r\n{auth}Connection: close\r\n\r\n"
    )
    .unwrap();
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    answer
        .split(' ')
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

fn pill(handle: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find("api-pill").is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
fn turning_the_api_on_listens_and_off_stops(cx: &mut TestAppContext) {
    let (handle, ws, dir) = with_data(cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.api.server.is_none() && ws.api.store.is_some())
    });
    assert!(!pill(handle, cx), "off, no pill");
    turn_on(&ws, cx);
    assert_ne!(port(&ws, cx), 0);
    let file =
        std::fs::read_to_string(templum_api::store::folder(dir.path()).join("api.json")).unwrap();
    assert!(file.contains("\"enabled\": true"), "{file}");
    assert!(pill(handle, cx), "the pill shows the door is open");
    ws.update(cx, |ws, cx| ws.api_edit(|s| s.enabled = false, cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.api.server.is_none() && ws.api.error.is_none())
    });
    assert!(!pill(handle, cx));
    // Settings read the same file at the next start.
    drop((handle, ws));
}

#[gpui_kit::test]
fn a_new_token_is_shown_once_and_saved_as_a_hash(cx: &mut TestAppContext) {
    let (_, ws, dir) = with_data(cx);
    turn_on(&ws, cx);
    ws.update(cx, |ws, cx| {
        ws.mint_token("phone", Scope::Control, None, None, cx)
    })
    .unwrap();
    let text = ws
        .read_with(cx, |ws, _| ws.api.shown.clone())
        .expect("shown once");
    assert!(text.starts_with("tpl_"));
    let file =
        std::fs::read_to_string(templum_api::store::folder(dir.path()).join("api.json")).unwrap();
    assert!(
        file.contains(&hash(&text)) && !file.contains(&text[4..]),
        "{file}"
    );
    ws.update(cx, |ws, _| ws.token_seen());
    ws.read_with(cx, |ws, _| assert!(ws.api.shown.is_none()));
    let port = port(&ws, cx);
    assert_eq!(
        status(port, "/v1/nope", Some(&text)),
        404,
        "the server knows the token"
    );
    assert_eq!(status(port, "/v1/nope", None), 401);
    let blank = ws.update(cx, |ws, cx| {
        ws.mint_token("  ", Scope::Read, None, None, cx)
    });
    assert_eq!(blank, Err("A token needs a name.".to_string()));
}

#[gpui_kit::test]
fn revoke_and_revoke_all(cx: &mut TestAppContext) {
    let (_, ws, _dir) = with_data(cx);
    turn_on(&ws, cx);
    let mut texts = Vec::new();
    for name in ["a", "b"] {
        ws.update(cx, |ws, cx| {
            ws.mint_token(name, Scope::Read, None, None, cx)
        })
        .unwrap();
        texts.push(ws.read_with(cx, |ws, _| ws.api.shown.clone()).unwrap());
    }
    let (first, port) = (
        ws.read_with(cx, |ws, _| ws.api.store.as_ref().unwrap().tokens[0].id),
        port(&ws, cx),
    );
    ws.update(cx, |ws, cx| ws.revoke_token(first, cx));
    assert_eq!(status(port, "/v1/nope", Some(&texts[0])), 401);
    assert_eq!(status(port, "/v1/nope", Some(&texts[1])), 404);
    ws.update(cx, |ws, cx| ws.revoke_all_tokens(cx));
    assert_eq!(status(port, "/v1/nope", Some(&texts[1])), 401);
    ws.read_with(cx, |ws, _| {
        assert!(ws.api.store.as_ref().unwrap().tokens.is_empty())
    });
}

#[gpui_kit::test]
fn plain_http_is_never_offered_off_loopback(cx: &mut TestAppContext) {
    let (_, ws, _dir) = with_data(cx);
    ws.update(cx, |ws, cx| {
        ws.api_edit(
            |s| {
                s.enabled = true;
                s.port = 0;
                s.listen = Listen::All;
            },
            cx,
        )
    });
    ws.read_with(cx, |ws, _| {
        let settings = &ws.api.store.as_ref().unwrap().settings;
        assert_eq!(
            settings.tls,
            templum_api::store::Tls::Own,
            "HTTPS is on, not offered off"
        );
        assert!(ws.api.error.is_none(), "{:?}", ws.api.error);
        assert!(ws.api.server.as_ref().unwrap().addr().ip().is_unspecified());
        assert!(
            ws.api.fingerprint.as_ref().is_some_and(|f| f.len() == 95),
            "its fingerprint is shown"
        );
    });
}

#[gpui_kit::test]
fn a_port_in_use_says_so(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = with_data(cx);
    let held = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let busy = held.local_addr().unwrap().port();
    ws.update(cx, |ws, cx| {
        ws.api_edit(
            |s| {
                s.enabled = true;
                s.port = busy;
            },
            cx,
        )
    });
    ws.read_with(cx, |ws, _| {
        assert!(ws.api.server.is_none());
        assert_eq!(
            ws.api.error.as_deref(),
            Some(format!("Port {busy} is in use: choose another port.").as_str())
        );
    });
    assert!(pill(handle, cx), "it reads API off");
}

#[gpui_kit::test]
fn last_uses_are_kept_at_the_snapshot(cx: &mut TestAppContext) {
    let (handle, ws, dir) = with_data(cx);
    turn_on(&ws, cx);
    ws.update(cx, |ws, cx| {
        ws.mint_token("phone", Scope::Read, None, None, cx)
    })
    .unwrap();
    let text = ws.read_with(cx, |ws, _| ws.api.shown.clone()).unwrap();
    assert_eq!(status(port(&ws, cx), "/v1/nope", Some(&text)), 404);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.snapshot(window, cx))
    })
    .unwrap();
    let last = ws
        .read_with(cx, |ws, _| {
            ws.api.store.as_ref().unwrap().tokens[0].last.clone()
        })
        .expect("a last use");
    assert_eq!(last.addr, "127.0.0.1");
    let file =
        std::fs::read_to_string(templum_api::store::folder(dir.path()).join("api.json")).unwrap();
    assert!(file.contains("\"addr\": \"127.0.0.1\""), "saved: {file}");
    // A second use within the minute is kept, not written (the file is synced to disk).
    assert_eq!(status(port(&ws, cx), "/v1/nope", Some(&text)), 404);
    let snapshot = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.snapshot(window, cx))
        })
        .unwrap()
    };
    snapshot(cx);
    ws.read_with(cx, |ws, _| assert!(ws.api.dirty, "waiting for its minute"));
    cx.executor().advance_clock(Duration::from_secs(61));
    snapshot(cx);
    ws.read_with(cx, |ws, _| assert!(!ws.api.dirty, "a minute on, written"));
    // And at quit, whatever is left.
    assert_eq!(status(port(&ws, cx), "/v1/nope", Some(&text)), 404);
    snapshot(cx);
    ws.read_with(cx, |ws, _| assert!(ws.api.dirty));
    ws.update(cx, |ws, _| ws.flush_api());
    ws.read_with(cx, |ws, _| assert!(!ws.api.dirty, "flushed"));
}

#[gpui_kit::test]
fn a_use_since_the_last_snapshot_is_written_at_quit(cx: &mut TestAppContext) {
    let (_, ws, dir) = with_data(cx);
    turn_on(&ws, cx);
    ws.update(cx, |ws, cx| {
        ws.mint_token("phone", Scope::Read, None, None, cx)
    })
    .unwrap();
    let text = ws.read_with(cx, |ws, _| ws.api.shown.clone()).unwrap();
    assert_eq!(status(port(&ws, cx), "/v1/nope", Some(&text)), 404);
    // No snapshot ran: the use is only in the server. Quitting takes it.
    ws.update(cx, |ws, _| ws.flush_api());
    let file =
        std::fs::read_to_string(templum_api::store::folder(dir.path()).join("api.json")).unwrap();
    assert!(file.contains("\"addr\": \"127.0.0.1\""), "saved: {file}");
}

#[gpui_kit::test]
fn the_pill_counts_the_clients_the_server_tells_of(cx: &mut TestAppContext) {
    use std::io::{Read, Write};
    let (_, ws, _dir) = with_data(cx);
    turn_on(&ws, cx);
    ws.update(cx, |ws, cx| {
        ws.mint_token("phone", Scope::Read, None, None, cx)
    })
    .unwrap();
    let text = ws.read_with(cx, |ws, _| ws.api.shown.clone()).unwrap();
    let clients = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.api.clients);
    assert_eq!(clients(cx), 0);
    // A connection kept open after its answer: a client.
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port(&ws, cx))).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET /v1/nope HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n\r\n"
    )
    .unwrap();
    let mut head = [0u8; 12];
    stream.read_exact(&mut head).unwrap();
    wait_until(cx, "the pill to count it", |cx| clients(cx) == 1);
    drop(stream);
    wait_until(cx, "the pill to see it go", |cx| clients(cx) == 0);
}

/// Pumps the UI thread until `condition` holds (the server's messages come on its channel).
fn wait_until(
    cx: &mut TestAppContext,
    what: &str,
    condition: impl Fn(&mut TestAppContext) -> bool,
) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        cx.run_until_parked();
        if condition(cx) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "timed out waiting for {what}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[gpui_kit::test]
fn an_api_field_is_taken_on_enter_not_at_each_key(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = with_data(cx);
    turn_on(&ws, cx);
    let port = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.api.store.as_ref().unwrap().settings.port)
    };
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-api", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        window.click("field-api.port", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(port(cx), 0);
    cx.simulate_input(handle, "47311");
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    assert_eq!(port(cx), 0, "typed, not taken");
    cx.simulate_keystrokes(handle, "enter");
    assert_eq!(port(cx), 47311, "Enter takes it");
}

/// Settings ▸ API with the port field focused and `typed` in it.
fn type_a_port(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    typed: &str,
) {
    // Focus listeners (a field's blur) only hear an active window.
    super::terminals::set_active(handle, true, cx);
    turn_on(ws, cx);
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-api", cx);
        window.render_frame(cx);
        window.render_frame(cx);
        window.click("field-api.port", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.simulate_input(handle, typed);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[gpui_kit::test]
fn an_api_field_is_taken_when_the_keyboard_leaves_it(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = with_data(cx);
    type_a_port(handle, &ws, cx, "47312");
    let port = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.api.store.as_ref().unwrap().settings.port)
    };
    assert_eq!(port(cx), 0, "typed, not taken");
    cx.update_window(handle, |_, window, cx| {
        // The next field takes the keyboard from this one, as a mouse down on it does.
        window.click("menu-api-listen", cx);
        window.blur(cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(port(cx), 47312, "Blur takes it");
}

#[gpui_kit::test]
fn a_port_that_is_refused_is_put_back_and_said(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = with_data(cx);
    type_a_port(handle, &ws, cx, "80");
    let port = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.api.store.as_ref().unwrap().settings.port)
    };
    cx.simulate_keystrokes(handle, "enter");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find(gpui_kit::ElementId::Name("commit-api-port-refused".into()))
                .is_some(),
            "says why"
        );
    })
    .unwrap();
    assert_eq!(port(cx), 0, "unchanged");
    // The field shows the setting again: what is typed next is added to it, not to the "80".
    cx.simulate_input(handle, "47313");
    cx.simulate_keystrokes(handle, "enter");
    assert_eq!(port(cx), 47313);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window
                .try_find(gpui_kit::ElementId::Name("commit-api-port-refused".into()))
                .is_none(),
            "taken: nothing to say"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn an_edit_that_changes_nothing_restarts_nothing(cx: &mut TestAppContext) {
    let (_, ws, _dir) = with_data(cx);
    turn_on(&ws, cx);
    let before = port(&ws, cx);
    ws.update(cx, |ws, cx| ws.api_edit(|s| s.port = 0, cx));
    assert_eq!(port(&ws, cx), before, "the same server, on the same port");
}

#[gpui_kit::test]
fn the_tick_runs_only_while_a_stream_is_open(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = with_data(cx);
    turn_on(&ws, cx);
    let token = with_token(&ws, cx, Scope::Read, None);
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| assert!(!ws.api.ticking, "no stream, no tick"));
    // A stream opened: the server says so, and the tick starts.
    let (reply, _) = std::sync::mpsc::channel();
    let message = Message {
        hello: Hello {
            terminal: token.id.to_string(),
            client: "api".into(),
            control: None,
        },
        conn: 3,
        incoming: Incoming::Notification {
            method: templum_api::WATCH.into(),
            params: Value::Null,
        },
        reply,
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.serve(message, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert!(ws.api.ticking));
    // At its next look there is no stream: it ends.
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.api.ticking, "it ended at the first look with none")
    });
}

#[gpui_kit::test]
fn settings_show_the_api_its_fields_and_its_tokens(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = with_data(cx);
    turn_on(&ws, cx);
    ws.update(cx, |ws, cx| {
        ws.mint_token("phone", Scope::Read, None, None, cx)
    })
    .unwrap();
    let id = ws.read_with(cx, |ws, _| ws.api.store.as_ref().unwrap().tokens[0].id);
    cx.update_window(handle, |_, window, cx| window.click("settings-button", cx))
        .unwrap();
    settle_slide(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("settings-tab-api", cx);
        window.render_frame(cx);
        for found in [
            "field-api.enabled",
            "field-api.listen",
            "field-api.port",
            "field-api.tls",
            "api-new-token",
            "api-revoke-all",
            "api-listening",
        ] {
            assert!(window.try_find(found).is_some(), "{found}");
        }
        // Its row is below the fold of the test window: found, not clicked.
        assert!(
            window
                .try_find(gpui_kit::ElementId::Name(format!("api-revoke-{id}").into()))
                .is_some()
        );
    })
    .unwrap();
}

// The review's findings: what a limited token or a big number could reach.

pub(super) fn refused(line: &Value) -> (Option<i64>, String) {
    (
        line["error"]["code"].as_i64(),
        line["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

/// The first workspace, with a second one and a block in it: (the second's id, the block's uid).
fn another_workspace(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (String, String) {
    super::templum_agent::control(
        handle,
        ws,
        cx,
        "create_workspace",
        json!({ "name": "Other" }),
    );
    let (_, text) = super::templum_agent::control(
        handle,
        ws,
        cx,
        "add_block",
        json!({ "workspace": "w2", "kind": "terminal" }),
    );
    (
        "w2".to_string(),
        text.trim_end_matches('.')
            .rsplit(' ')
            .next()
            .unwrap()
            .to_string(),
    )
}

#[gpui_kit::test]
fn a_limited_token_cant_overview_what_it_doesnt_see(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, far) = another_workspace(handle, &ws, cx);
    let limited = with_token(&ws, cx, Scope::Read, Some(vec![first.as_str()]));
    let line = tool(handle, &ws, cx, &limited, "overview", json!({}));
    let (code, message) = refused(&line);
    assert_eq!(code, Some(i64::from(codes::SCOPE)), "{line}");
    assert!(
        message.contains("GET /v1/state") && !message.contains(&far),
        "{message}"
    );
    // An unlimited token still has it.
    let all = with_token(&ws, cx, Scope::Read, None);
    assert!(
        said(&tool(handle, &ws, cx, &all, "overview", json!({})))
            .1
            .contains(&far)
    );
}

#[gpui_kit::test]
fn a_limited_token_names_its_board_and_never_hears_of_others(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let secret = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("Secret plans"), &["To do".to_string()]))
    });
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str()]));
    for (name, arguments) in [
        ("list_cards", json!({})),
        ("create_card", json!({ "title": "x" })),
    ] {
        let line = tool(handle, &ws, cx, &limited, name, arguments);
        let (code, message) = refused(&line);
        assert_eq!(
            code,
            Some(i64::from(templum_mcp::rpc::PARAMS)),
            "{name}: {line}"
        );
        assert!(
            message.contains("board") && !message.contains("Secret"),
            "{name}: {message}"
        );
    }
    // A board it doesn't see is a board that doesn't exist, by id and by name.
    for unseen in [secret.to_string(), "Secret plans".to_string()] {
        let line = tool(
            handle,
            &ws,
            cx,
            &limited,
            "create_card",
            json!({ "board": unseen, "title": "x" }),
        );
        let (code, message) = refused(&line);
        assert_eq!(code, Some(i64::from(codes::NOT_FOUND)));
        assert!(message.starts_with("There's no board"), "{message}");
    }
    assert!(ws.read_with(cx, |ws, _| {
        ws.boards.board(secret).unwrap().columns[0].cards.is_empty()
    }));
    // Its own works; so does a card.
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "create_card",
        json!({ "board": board.to_string(), "title": "x" }),
    );
    assert!(!said(&line).0 && line["error"].is_null(), "{line}");
    assert!(
        !said(&tool(
            handle,
            &ws,
            cx,
            &limited,
            "list_cards",
            json!({ "card": card })
        ))
        .0
    );
}

#[gpui_kit::test]
fn a_big_move_is_clamped_to_the_biggest_grid(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&terminal].clone());
    let token = with_token(&ws, cx, Scope::Control, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &token,
        "move_block",
        json!({ "block": uid, "w": 1e9, "h": 1e9 }),
    );
    assert!(!said(&line).0 && line["error"].is_null(), "{line}");
    let (rect, grid, max) = ws.read_with(cx, |ws, _| {
        let max = templum_canvas::chrome::block_size(
            crate::workspace::MAX_COLS,
            crate::workspace::MAX_ROWS,
            ws.cell.size,
        );
        (
            ws.space.canvas.get(terminal).unwrap().rect,
            ws.space.terminal(terminal).unwrap().term.grid_size(),
            max,
        )
    });
    assert_eq!((rect.w, rect.h), (max.w, max.h));
    assert!(
        grid.0 <= crate::workspace::MAX_COLS && grid.1 <= crate::workspace::MAX_ROWS,
        "{grid:?}"
    );
    // And a place far beyond any canvas is refused as a number, not stored.
    let line = tool(
        handle,
        &ws,
        cx,
        &token,
        "move_block",
        json!({ "block": uid, "x": 1e12 }),
    );
    assert_eq!(
        refused(&line).0,
        Some(i64::from(templum_mcp::rpc::PARAMS)),
        "{line}"
    );
}

#[gpui_kit::test]
fn a_command_automation_is_a_full_tokens_whoever_touches_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let control = with_token(&ws, cx, Scope::Control, None);
    let full = with_token(&ws, cx, Scope::Full, None);
    let rule = |act: Value| json!({ "name": "R", "workspace": space, "when": { "every": { "minutes": 5 } }, "do": act });
    let command = json!({ "command": { "text": "ls" } });
    let notify = json!({ "notify": { "text": "hi" } });
    assert!(
        !said(&tool(
            handle,
            &ws,
            cx,
            &full,
            "save_automation",
            json!({ "automation": rule(command) })
        ))
        .0
    );
    assert!(
        !said(&tool(
            handle,
            &ws,
            cx,
            &control,
            "save_automation",
            json!({ "automation": rule(notify.clone()) })
        ))
        .0
    );
    // 1 is the command, 2 the notice.
    for (name, arguments) in [
        ("set_automation", json!({ "automation": 1, "on": false })),
        ("run_automation", json!({ "automation": 1 })),
        (
            "save_automation",
            json!({ "automation": rule(notify.clone()), "id": 1 }),
        ),
    ] {
        let line = tool(handle, &ws, cx, &control, name, arguments);
        let (code, message) = refused(&line);
        assert_eq!(code, Some(i64::from(codes::SCOPE)), "{name}: {line}");
        assert_eq!(message, format!("{name} needs a Full token."));
    }
    ws.read_with(cx, |ws, _| {
        assert!(ws.automations.get(1).unwrap().on, "untouched")
    });
    // The notice is a control token's.
    for (name, arguments) in [
        ("set_automation", json!({ "automation": 2, "on": false })),
        ("run_automation", json!({ "automation": 2 })),
        (
            "save_automation",
            json!({ "automation": rule(notify), "id": 2 }),
        ),
    ] {
        assert!(
            tool(handle, &ws, cx, &control, name, arguments)["error"].is_null(),
            "{name}"
        );
    }
    // A full token still does it all.
    assert!(
        !said(&tool(
            handle,
            &ws,
            cx,
            &full,
            "set_automation",
            json!({ "automation": 1, "on": false })
        ))
        .0
    );
}

#[gpui_kit::test]
fn a_name_that_is_another_workspaces_id_resolves_as_the_tool_does(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, far) = another_workspace(handle, &ws, cx);
    // The first workspace is named "w2", which is the id of the other.
    ws.update(cx, |ws, _| ws.space.name = "w2".into());
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str()]));
    let before = ws.read_with(cx, |ws, _| {
        ws.space_named("w2").unwrap().canvas.blocks().len()
    });
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "add_block",
        json!({ "workspace": "w2", "kind": "note" }),
    );
    assert_eq!(
        refused(&line).0,
        Some(i64::from(codes::NOT_FOUND)),
        "{line}"
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws
            .space_named("w2")
            .unwrap()
            .canvas
            .blocks()
            .len()),
        before
    );
    let _ = far;
    // The same for a board whose name is another board's id.
    let (_, mine, _) = board_with_a_card(handle, &ws, cx);
    let theirs = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("Theirs"), &["To do".to_string()]))
    });
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            b.boards.iter_mut().find(|x| x.id == mine).unwrap().name = theirs.to_string()
        })
    });
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "create_card",
        json!({ "board": theirs.to_string(), "title": "x" }),
    );
    assert_eq!(
        refused(&line).0,
        Some(i64::from(codes::NOT_FOUND)),
        "{line}"
    );
    let in_theirs = ws.read_with(cx, |ws, _| {
        ws.boards.board(theirs).unwrap().columns[0].cards.len()
    });
    let in_mine = ws.read_with(cx, |ws, _| {
        ws.boards.board(mine).unwrap().columns[0].cards.len()
    });
    assert_eq!(
        (in_theirs, in_mine),
        (0, 1),
        "the id wins in the tool, so a board it doesn't see isn't touched"
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_limited_tokens_folder_stays_inside_its_workspaces(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (home, elsewhere) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::create_dir(home.path().join("inside")).unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), home.path().join("link")).unwrap();
    ws.update(cx, |ws, _| {
        ws.space.folder = Some(home.path().to_path_buf())
    });
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str()]));
    let blocks = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len());
    let n = blocks(cx);
    for outside in [
        elsewhere.path().display().to_string(),
        "..".to_string(),
        format!(
            "{}/../{}",
            home.path().display(),
            elsewhere.path().file_name().unwrap().to_string_lossy()
        ),
        "link".to_string(),
        "/".to_string(),
    ] {
        for (kind, extra) in [("terminal", json!({})), ("editor", json!({}))] {
            let mut args = json!({ "workspace": first, "kind": kind, "folder": outside });
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let line = tool(handle, &ws, cx, &limited, "add_block", args);
            assert_eq!(
                refused(&line).0,
                Some(i64::from(codes::SCOPE)),
                "{kind} {outside}: {line}"
            );
        }
        let line = tool(
            handle,
            &ws,
            cx,
            &limited,
            "open_agent",
            json!({ "agent": "claude", "workspace": first, "folder": outside }),
        );
        assert_eq!(
            refused(&line).0,
            Some(i64::from(codes::SCOPE)),
            "agent {outside}: {line}"
        );
    }
    assert_eq!(blocks(cx), n, "nothing opened");
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "add_block",
        json!({ "workspace": first, "kind": "editor", "folder": "inside" }),
    );
    assert!(line["error"].is_null() && !said(&line).0, "{line}");
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "add_block",
        json!({ "workspace": first, "kind": "editor" }),
    );
    assert!(
        line["error"].is_null() && !said(&line).0,
        "without a folder it is the workspace's: {line}"
    );
    // A workspace with no folder gives a terminal nowhere inside to start.
    ws.update(cx, |ws, _| ws.space.folder = None);
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "add_block",
        json!({ "workspace": first, "kind": "terminal" }),
    );
    assert_eq!(refused(&line).0, Some(i64::from(codes::SCOPE)), "{line}");
    // An unlimited token goes anywhere.
    let all = with_token(&ws, cx, Scope::Control, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &all,
        "add_block",
        json!({ "workspace": first, "kind": "editor", "folder": elsewhere.path().display().to_string() }),
    );
    assert!(line["error"].is_null() && !said(&line).0, "{line}");
}

#[gpui_kit::test]
fn a_limited_tokens_rule_keeps_its_folder_as_the_real_path_and_inside(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let home = tempfile::tempdir().unwrap();
    // The simplified form, as the rule keeps it (`\\?\C:\x` on Windows is not one a shell can start in).
    let real = dunce::canonicalize(home.path()).unwrap();
    std::fs::create_dir(home.path().join("sub")).unwrap();
    ws.update(cx, |ws, _| ws.space.folder = Some(real.clone()));
    let limited = with_token(&ws, cx, Scope::Full, Some(vec![first.as_str()]));
    let rule = |act: Value| {
        json!({ "automation": { "name": "R", "workspace": first,
            "when": { "every": { "minutes": 5 } }, "do": act } })
    };
    let stored = |cx: &mut TestAppContext, id: u64| {
        ws.read_with(cx, |ws, _| match &ws.automations.get(id).unwrap().act {
            crate::automations::Do::Command { folder, .. }
            | crate::automations::Do::Agent { folder, .. } => folder.clone(),
            other => panic!("{other:?}"),
        })
    };
    // "." is the workspace's folder, written down as the real path: what was checked is what runs.
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "save_automation",
        rule(json!({ "command": { "folder": ".", "text": "ls" } })),
    );
    assert!(!said(&line).0 && line["error"].is_null(), "{line}");
    assert_eq!(stored(cx, 1), Some(real.display().to_string()));
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "save_automation",
        rule(json!({ "agent": { "agent": "claude", "folder": "sub" } })),
    );
    assert!(line["error"].is_null(), "{line}");
    assert_eq!(stored(cx, 2), Some(real.join("sub").display().to_string()));
    #[cfg(unix)]
    {
        let elsewhere = tempfile::tempdir().unwrap();
        // A link swapped in for the folder after the save: the next run is skipped.
        std::fs::remove_dir(home.path().join("sub")).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), home.path().join("sub")).unwrap();
        let line = tool(
            handle,
            &ws,
            cx,
            &limited,
            "run_automation",
            json!({ "automation": 2 }),
        );
        let (_, said) = said(&line);
        assert!(said.contains("Target missing"), "{said}");
        ws.read_with(cx, |ws, _| {
            let last = ws.automations.get(2).unwrap().last.clone().unwrap();
            assert_eq!(last.went, crate::automations::Went::Skipped);
        });
    }
    // A rule that names no folder, in a workspace that has none, has nowhere inside to start.
    ws.update(cx, |ws, _| ws.space.folder = None);
    for act in [
        json!({ "command": { "text": "ls" } }),
        json!({ "agent": { "agent": "claude" } }),
    ] {
        let line = tool(handle, &ws, cx, &limited, "save_automation", rule(act));
        assert_eq!(refused(&line).0, Some(i64::from(codes::SCOPE)), "{line}");
    }
}

/// `open_agent` with a worktree, as a token limited to `first` asks it: the answer once git ran.
#[cfg(unix)]
fn worktree_for(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    limited: &Token,
    first: &str,
) -> Value {
    let args = json!({ "agent": "claude", "workspace": first, "worktree": "feature" });
    let answers = api_later(
        handle,
        ws,
        cx,
        limited,
        7,
        "tools/call",
        json!({ "name": "open_agent", "arguments": args }),
    );
    serde_json::from_str(&super::bus::answer_line_soon(cx, &answers)).unwrap()
}

/// A worktree refused for a limited token: `403 scope`, like any folder outside its workspaces.
#[cfg(unix)]
fn assert_outside(line: &Value) {
    let (code, text) = refused(line);
    assert_eq!(code, Some(i64::from(codes::SCOPE)), "{line}");
    assert!(text.contains("main tree"), "{text}");
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_limited_tokens_worktree_is_made_only_where_git_says_the_main_tree_is_inside(
    cx: &mut TestAppContext,
) {
    let (handle, ws, _) = super::spawn::spawner(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str()]));
    // The workspace's folder is a linked worktree of a repository whose main tree lies elsewhere.
    let (main, sides) = (committed_repo(), tempfile::tempdir().unwrap());
    let linked = sides.path().canonicalize().unwrap().join("linked");
    git_run(
        main.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ],
    );
    ws.update(cx, |ws, _| ws.space.folder = Some(linked.clone()));
    assert_outside(&worktree_for(handle, &ws, cx, &limited, &first));
    assert!(!main.path().join(".worktrees").exists(), "nothing made");
    // A repository whose main tree is the workspace's folder: the worktree is made.
    let inside = committed_repo();
    ws.update(cx, |ws, _| {
        ws.space.folder = Some(inside.path().canonicalize().unwrap())
    });
    let (failed, text) = said(&worktree_for(handle, &ws, cx, &limited, &first));
    assert!(!failed, "{text}");
    assert!(inside.path().join(".worktrees/feature").is_dir());
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_planted_git_file_cant_move_a_limited_tokens_worktree_outside(cx: &mut TestAppContext) {
    let (handle, ws, _) = super::spawn::spawner(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str()]));
    // The folder's `.git` file leads to a git directory made inside it, which names (commondir)
    // a repository outside: git works in that one, so that is where the worktree would go.
    let (outside, home) = (committed_repo(), tempfile::tempdir().unwrap());
    let (outside_git, home) = (
        outside.path().canonicalize().unwrap().join(".git"),
        home.keep().canonicalize().unwrap(),
    );
    let fake = home.join("fake/.git/worktrees/wt");
    std::fs::create_dir_all(&fake).unwrap();
    std::fs::write(fake.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(
        fake.join("commondir"),
        format!("{}\n", outside_git.display()),
    )
    .unwrap();
    std::fs::write(
        fake.join("gitdir"),
        format!("{}\n", home.join(".git").display()),
    )
    .unwrap();
    std::fs::write(home.join(".git"), format!("gitdir: {}\n", fake.display())).unwrap();
    assert_eq!(
        git_run(&home, &["worktree", "list", "--porcelain"])
            .lines()
            .next(),
        Some(
            format!(
                "worktree {}",
                outside.path().canonicalize().unwrap().display()
            )
            .as_str()
        ),
        "git follows the commondir"
    );
    ws.update(cx, |ws, _| ws.space.folder = Some(home.clone()));
    assert_outside(&worktree_for(handle, &ws, cx, &limited, &first));
    assert!(!outside.path().join(".worktrees").exists(), "nothing made");
}

#[gpui_kit::test]
fn a_deleted_workspace_leaves_its_tokens_lists(cx: &mut TestAppContext) {
    let (handle, ws, dir) = with_data(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    another_workspace(handle, &ws, cx);
    let token = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str(), "w2"]));
    let id = token.id;
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.delete_space("w2", window, cx))
    })
    .unwrap();
    let kept = ws.read_with(cx, |ws, _| {
        ws.api
            .store
            .as_ref()
            .unwrap()
            .tokens
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .workspaces
            .clone()
    });
    assert_eq!(kept, Some(vec![first.clone()]), "w2 is gone from the list");
    // The deleted canvas's `.bak` would keep its id taken: remove it, so a new workspace really
    // takes the freed id, and is not the token's.
    let _ = std::fs::remove_file(crate::session::path(dir.path(), "w2").with_extension("json.bak"));
    super::templum_agent::control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({ "name": "New" }),
    );
    let token = ws.read_with(cx, |ws, _| {
        assert!(ws.spaces().any(|s| s.id == "w2"), "w2 is reused");
        ws.api
            .store
            .as_ref()
            .unwrap()
            .tokens
            .iter()
            .find(|t| t.id == id)
            .unwrap()
            .clone()
    });
    let seen = state(handle, &ws, cx, &token);
    assert_eq!(
        seen.workspaces
            .iter()
            .map(|w| w.id.as_str())
            .collect::<Vec<_>>(),
        [first.as_str()]
    );
}

/// Saves a notify rule named `name` as `token`, in `space`: its number.
fn save_rule_as(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
    space: &str,
    name: &str,
) -> u64 {
    let rule = json!({ "name": name, "workspace": space,
        "when": { "every": { "minutes": 10 } }, "do": { "notify": { "text": "hi" } } });
    let line = tool(
        handle,
        ws,
        cx,
        token,
        "save_automation",
        json!({ "automation": rule }),
    );
    let (failed, text) = said(&line);
    assert!(!failed, "{line}");
    text.trim_start_matches("Saved automation ")
        .trim_end_matches('.')
        .parse()
        .unwrap()
}

fn rule_on(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64) -> bool {
    ws.read_with(cx, |ws, _| ws.automations.get(id).unwrap().on)
}

#[gpui_kit::test]
fn the_automations_of_a_revoked_or_expired_token_are_turned_off(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let tokens: Vec<Token> = (0..4)
        .map(|_| with_token(&ws, cx, Scope::Control, None))
        .collect();
    let rules: Vec<u64> = tokens
        .iter()
        .enumerate()
        .map(|(n, t)| save_rule_as(handle, &ws, cx, t, &space, &format!("R{n}")))
        .collect();
    ws.read_with(cx, |ws, _| {
        let saved = &ws.api.store.as_ref().unwrap().saved_by;
        for (rule, token) in rules.iter().zip(&tokens) {
            assert_eq!(saved.get(rule), Some(&token.id));
        }
    });
    assert!(rules.iter().all(|r| rule_on(&ws, cx, *r)));
    // Revoke: only that token's rule goes off, and the person is told.
    ws.update(cx, |ws, cx| ws.revoke_token(tokens[0].id, cx));
    assert_eq!(
        rules
            .iter()
            .map(|r| rule_on(&ws, cx, *r))
            .collect::<Vec<_>>(),
        [false, true, true, true]
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.api.said,
            ["Automation R0, saved by token 'phone', was turned off."]
        );
        assert!(
            !ws.api
                .store
                .as_ref()
                .unwrap()
                .saved_by
                .contains_key(&rules[0])
        );
    });
    // Expiry: the sweep at the snapshot finds it.
    ws.update(cx, |ws, _| {
        let store = ws.api.store.as_mut().unwrap();
        store
            .tokens
            .iter_mut()
            .find(|t| t.id == tokens[1].id)
            .unwrap()
            .expires = Some(1);
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.snapshot(window, cx))
    })
    .unwrap();
    assert_eq!(
        rules
            .iter()
            .map(|r| rule_on(&ws, cx, *r))
            .collect::<Vec<_>>(),
        [false, false, true, true]
    );
    // The person's own edit makes a rule theirs: it outlives the token.
    ws.update(cx, |ws, _| ws.api_unconfine(rules[2]));
    ws.update(cx, |ws, cx| ws.revoke_all_tokens(cx));
    assert_eq!(
        rules
            .iter()
            .map(|r| rule_on(&ws, cx, *r))
            .collect::<Vec<_>>(),
        [false, false, true, false],
        "Revoke all turns off the rest, not the person's"
    );
    ws.read_with(cx, |ws, _| assert_eq!(ws.api.said.len(), 3));
}

#[gpui_kit::test]
fn a_revoked_tokens_rule_doesnt_run_before_the_sweep_turns_it_off(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let token = with_token(&ws, cx, Scope::Control, None);
    let rule = save_rule_as(handle, &ws, cx, &token, &space, "R");
    let target = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.automation_target(ws.automations.get(rule).unwrap())
        })
    };
    assert_eq!(target(cx), Ok(()));
    // Revoked, and the sweep (every few seconds) hasn't looked yet.
    ws.update(cx, |ws, _| {
        ws.api.store.as_mut().unwrap().revoke(token.id);
    });
    assert_eq!(
        target(cx),
        Err("Target missing: its token was revoked".to_string())
    );
    // The person's own edit makes it theirs again.
    ws.update(cx, |ws, _| ws.api_unconfine(rule));
    assert_eq!(target(cx), Ok(()));
}

#[gpui_kit::test]
fn a_rule_the_token_turned_off_is_told_of_when_it_is_revoked(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let token = with_token(&ws, cx, Scope::Control, None);
    let rule = save_rule_as(handle, &ws, cx, &token, &space, "R");
    ws.update(cx, |ws, cx| ws.set_automation_on(rule, false, cx));
    assert!(!rule_on(&ws, cx, rule));
    ws.update(cx, |ws, cx| ws.revoke_token(token.id, cx));
    assert!(!rule_on(&ws, cx, rule), "it stays off");
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.api.said,
            ["Automation R, saved by token 'phone', stays off."]
        );
        assert!(ws.api.store.as_ref().unwrap().saved_by.is_empty());
    });
}

#[gpui_kit::test]
fn entries_for_automations_that_dont_exist_are_dropped_at_load(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // `automations.json` was set aside and the numbers started again: only 1 exists now.
    std::fs::write(
        crate::automations::path(dir.path()),
        r#"{ "version": 1, "next_id": 2, "automations": [
            { "id": 1, "name": "R", "on": false, "when": { "every": { "minutes": 10 } },
              "do": { "notify": { "text": "hi" } }, "runs": 0, "since": 0 } ] }"#,
    )
    .unwrap();
    let mut store = Store::default();
    for id in [1, 7] {
        store.confined.insert(id, vec!["/home/x".into()]);
        store.saved_by.insert(id, 3);
    }
    templum_api::store::save(dir.path(), &store).unwrap();
    let (_, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let kept = ws.read_with(cx, |ws, _| {
        let store = ws.api.store.as_ref().unwrap();
        (
            store.confined.keys().copied().collect::<Vec<_>>(),
            store.saved_by.keys().copied().collect::<Vec<_>>(),
        )
    });
    assert_eq!(kept, (vec![1], vec![1]));
    // And the file says so too.
    let saved = templum_api::store::load(dir.path()).unwrap();
    assert_eq!(saved.confined.keys().copied().collect::<Vec<_>>(), [1]);
}

#[gpui_kit::test]
fn restarting_the_server_keeps_the_uses_it_holds(cx: &mut TestAppContext) {
    let (_, ws, _dir) = with_data(cx);
    turn_on(&ws, cx);
    ws.update(cx, |ws, cx| {
        ws.mint_token("phone", Scope::Read, None, None, cx)
    })
    .unwrap();
    let text = ws.read_with(cx, |ws, _| ws.api.shown.clone()).unwrap();
    assert_eq!(status(port(&ws, cx), "/v1/nope", Some(&text)), 404);
    ws.update(cx, |ws, _| ws.start_api());
    let last = ws.read_with(cx, |ws, _| {
        ws.api.store.as_ref().unwrap().tokens[0].last.clone()
    });
    assert_eq!(last.map(|u| u.addr), Some("127.0.0.1".to_string()));
}

#[test]
fn a_path_on_a_share_of_another_computer_is_refused_unless_a_folder_is_on_it() {
    use crate::workspace::api::refuses_share;
    use std::path::Prefix::{DeviceNS, Disk, UNC, Verbatim, VerbatimDisk, VerbatimUNC};
    let share =
        |server: &'static str, name: &'static str| Some(UNC(server.as_ref(), name.as_ref()));
    let attacker = share("attacker", "share");
    let disk = Some(Disk(b'C'));
    assert!(
        refuses_share(attacker, &[disk]),
        "a share, and no folder on it"
    );
    assert!(refuses_share(attacker, &[]));
    assert!(
        refuses_share(attacker, &[share("attacker", "other")]),
        "another share"
    );
    assert!(
        refuses_share(attacker, &[share("friend", "share")]),
        "another server"
    );
    assert!(
        !refuses_share(attacker, &[disk, share("ATTACKER", "Share")]),
        "a folder on the same share, in either case"
    );
    assert!(
        !refuses_share(
            Some(VerbatimUNC("attacker".as_ref(), "share".as_ref())),
            &[attacker]
        ),
        "verbatim or not, it is the same share"
    );
    assert!(refuses_share(
        Some(VerbatimUNC("attacker".as_ref(), "share".as_ref())),
        &[disk]
    ));
    assert!(
        refuses_share(Some(DeviceNS("pipe".as_ref())), &[disk]),
        "a device namespace"
    );
    // std matches `\\?\UNC\` case-sensitively: `\\?\unc\attacker\share` and
    // `\\?\GLOBALROOT\Device\Mup\…` parse as a plain `Verbatim` prefix, and still reach a share.
    assert!(
        refuses_share(Some(Verbatim("unc".as_ref())), &[disk]),
        "a lowercase verbatim UNC"
    );
    assert!(
        refuses_share(Some(Verbatim("GLOBALROOT".as_ref())), &[disk]),
        "the object manager's way to the redirector"
    );
    assert!(refuses_share(Some(Verbatim("unc".as_ref())), &[]));
    assert!(
        refuses_share(Some(Verbatim("unc".as_ref())), &[attacker]),
        "a UNC root is not the verbatim name"
    );
    assert!(
        !refuses_share(
            Some(Verbatim("GLOBALROOT".as_ref())),
            &[Some(Verbatim("globalroot".as_ref()))]
        ),
        "unless a folder is on the same one"
    );
    assert!(!refuses_share(disk, &[]), "a drive is the computer's own");
    assert!(!refuses_share(Some(VerbatimDisk(b'C')), &[]));
    assert!(!refuses_share(None, &[]), "a path with no prefix");
}

// Terminal streams: the terminals lent to the server, and the tick that lends them again.

#[gpui_kit::test]
fn the_terminals_are_lent_with_their_workspaces(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let (first, uid, kanban_uid, note_uid) = ws.read_with(cx, |ws, _| {
        (
            ws.space.id.clone(),
            ws.space.uids[&terminal].clone(),
            ws.space.uids[&kanban].clone(),
            ws.space.uids[&note].clone(),
        )
    });
    let (second, other) = another_workspace(handle, &ws, cx);
    let lent = ws.read_with(cx, |ws, _| ws.api_terminals());
    let mut found: Vec<(&str, &str)> = lent
        .iter()
        .map(|t| (t.block.as_str(), t.workspace.as_str()))
        .collect();
    found.sort_unstable();
    let mut wanted = vec![
        (uid.as_str(), first.as_str()),
        (other.as_str(), second.as_str()),
    ];
    wanted.sort_unstable();
    assert_eq!(found, wanted, "the terminals and no other block");
    assert!(
        !found
            .iter()
            .any(|(b, _)| *b == kanban_uid || *b == note_uid)
    );
    // What is lent is the pane's own grid.
    ws.read_with(cx, |ws, _| {
        let pane = ws.space_named(&first).unwrap().terminal(terminal).unwrap();
        let lent = lent.iter().find(|t| t.block == uid).unwrap();
        assert!(lent.term.same(pane.term.shared()));
    });
}

#[gpui_kit::test]
fn a_terminal_stream_is_checked_on_the_ui_thread(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    let (first, uid, note_uid) = ws.read_with(cx, |ws, _| {
        (
            ws.space.id.clone(),
            ws.space.uids[&terminal].clone(),
            ws.space.uids[&note].clone(),
        )
    });
    let (_, far) = another_workspace(handle, &ws, cx);
    let token = with_token(&ws, cx, Scope::Read, None);
    let ask = |cx: &mut TestAppContext, token: &Token, block: &str| {
        api_call(
            handle,
            &ws,
            cx,
            token,
            templum_api::TERMINAL,
            json!({ "block": block }),
        )
    };
    let line = ask(cx, &token, &uid);
    assert_eq!(said(&line), (false, format!("Streaming {uid}.")));
    let line = ask(cx, &token, &note_uid);
    assert_eq!(said(&line), (true, format!("{note_uid} isn't a terminal.")));
    let line = ask(cx, &token, "zzzzz");
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::NOT_FOUND)),
            "There's no block zzzzz: GET /v1/blocks lists them.".to_string()
        )
    );
    // A block outside the token's workspaces is answered as one that isn't there.
    let limited = with_token(&ws, cx, Scope::Read, Some(vec![first.as_str()]));
    let line = ask(cx, &limited, &far);
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::NOT_FOUND)),
            format!("There's no block {far}: GET /v1/blocks lists them.")
        )
    );
    assert!(
        !said(&ask(cx, &limited, &uid)).0,
        "its own terminal is fine"
    );
    ws.update(cx, |ws, _| ws.api.store.as_mut().unwrap().revoke_all());
    let line = ask(cx, &token, &uid);
    assert_eq!(line["error"]["code"].as_i64(), Some(i64::from(codes::AUTH)));
}

#[gpui_kit::test]
fn the_tick_lends_terminals_while_a_terminal_stream_is_open(cx: &mut TestAppContext) {
    use std::io::{Read, Write};
    let (handle, ws, _dir) = with_data(cx);
    turn_on(&ws, cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&terminal].clone());
    ws.update(cx, |ws, cx| {
        ws.mint_token("phone", Scope::Read, None, None, cx)
    })
    .unwrap();
    let text = ws.read_with(cx, |ws, _| ws.api.shown.clone()).unwrap();
    let port = port(&ws, cx);
    let (done, ended) = std::sync::mpsc::channel::<String>();
    let (headed, head) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        write!(
            stream,
            "GET /v1/terminals/{uid}/stream HTTP/1.1\r\nHost: x\r\n\
             Authorization: Bearer {text}\r\n\r\n"
        )
        .unwrap();
        let mut all = String::new();
        let mut byte = [0];
        while !all.ends_with("\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
            all.push(byte[0].into());
        }
        let _ = headed.send(());
        let _ = stream.read_to_string(&mut all);
        let _ = done.send(all);
    });
    let streams = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.api.server.as_ref().unwrap().terminal_streams()
        })
    };
    // The stream counts before the UI thread answers, and the server lets it through only after
    // (a stream opening also starts the tick): wait for the answer's head, sent once it did.
    wait_until(cx, "the stream to open", |_| head.try_recv().is_ok());
    ws.read_with(cx, |ws, _| {
        assert!(ws.api.ticking, "the answer starts the tick")
    });
    assert_eq!(streams(cx), 1);
    assert!(ws.update(cx, |ws, cx| ws.api_tick_once(cx)));
    assert_eq!(streams(cx), 1, "the terminal is still there");
    // The person closes the block: the tick lends the terminals without it, and the stream ends.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.close_now(
                &[terminal],
                crate::workspace::history::Then::New,
                window,
                cx,
            )
        })
    })
    .unwrap();
    assert!(
        ws.update(cx, |ws, cx| ws.api_tick_once(cx)),
        "it lent the table, which no longer has the block"
    );
    wait_until(cx, "the stream to end", |cx| streams(cx) == 0);
    let answer = ended
        .recv_timeout(Duration::from_secs(10))
        .expect("the client reads the end of the stream");
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    assert!(
        !ws.update(cx, |ws, cx| ws.api_tick_once(cx)),
        "no stream of either kind: the tick is over"
    );
    ws.read_with(cx, |ws, _| assert!(!ws.api.ticking));
}

// Input: `terminal_input` is the person's keyboard.

#[gpui_kit::test]
fn terminal_input_waits_only_after_an_escape(cx: &mut TestAppContext) {
    use crate::workspace::delegate::KEY_GAP;
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&terminal].clone());
    let token = with_token(&ws, cx, Scope::Full, None);
    let inputs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(terminal).unwrap().term.inputs()
        })
    };
    let n = inputs(cx);
    let answers = api_later(
        handle,
        &ws,
        cx,
        &token,
        903,
        "tools/call",
        json!({ "name": "terminal_input", "arguments": { "block": uid, "keys": ["a", "b", "escape", "c"] } }),
    );
    cx.run_until_parked();
    assert_eq!(inputs(cx), n + 3, "a, b and Esc back to back; c waits");
    assert!(
        answers.try_recv().is_err(),
        "no answer before the clock moves"
    );
    cx.executor()
        .advance_clock(KEY_GAP + Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(inputs(cx), n + 4);
    let line: Value = serde_json::from_str(&answers.try_recv().expect("answered")).unwrap();
    assert_eq!(said(&line), (false, format!("Pressed 4 keys in {uid}.")));
}

#[gpui_kit::test]
fn terminal_input_pastes_and_presses_as_the_keyboard(cx: &mut TestAppContext) {
    use crate::workspace::delegate::KEY_GAP;
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    let (first, uid, note_uid) = ws.read_with(cx, |ws, _| {
        (
            ws.space.id.clone(),
            ws.space.uids[&terminal].clone(),
            ws.space.uids[&note].clone(),
        )
    });
    let token = with_token(&ws, cx, Scope::Full, None);
    let inputs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(terminal).unwrap().term.inputs()
        })
    };
    let n = inputs(cx);
    let input = |cx: &mut TestAppContext, token: &Token, arguments: Value| {
        tool(handle, &ws, cx, token, "terminal_input", arguments)
    };
    let line = input(cx, &token, json!({ "block": uid, "text": "echo hi" }));
    assert_eq!(said(&line), (false, format!("Pasted into {uid}.")));
    assert_eq!(inputs(cx), n + 1, "one paste");
    // Esc and the key after it come KEY_GAP apart, and the answer after the last.
    let answers = api_later(
        handle,
        &ws,
        cx,
        &token,
        901,
        "tools/call",
        json!({ "name": "terminal_input", "arguments": { "block": uid, "keys": ["escape", "j"] } }),
    );
    cx.run_until_parked();
    assert!(
        answers.try_recv().is_err(),
        "no answer before the clock moves"
    );
    assert_eq!(inputs(cx), n + 2, "Esc pressed at once, alone");
    cx.executor()
        .advance_clock(KEY_GAP + Duration::from_millis(1));
    cx.run_until_parked();
    let line: Value = serde_json::from_str(&answers.try_recv().expect("answered")).unwrap();
    assert_eq!(said(&line), (false, format!("Pressed 2 keys in {uid}.")));
    assert_eq!(inputs(cx), n + 3);
    let by = ws.read_with(cx, |ws, _| {
        ws.space.terminal(terminal).unwrap().keys_from.clone()
    });
    assert_eq!(by.map(|(by, _)| by).as_deref(), Some("API · phone"));
    // One key says so.
    let answers = api_later(
        handle,
        &ws,
        cx,
        &token,
        902,
        "tools/call",
        json!({ "name": "terminal_input", "arguments": { "block": uid, "keys": ["enter"] } }),
    );
    cx.run_until_parked();
    let line: Value = serde_json::from_str(&answers.try_recv().expect("answered")).unwrap();
    assert_eq!(said(&line), (false, format!("Pressed 1 key in {uid}.")));
    // What isn't a terminal, a Control token, and a block out of the token's sight.
    let line = input(cx, &token, json!({ "block": note_uid, "text": "x" }));
    assert_eq!(said(&line), (true, format!("{note_uid} isn't a terminal.")));
    let control = with_token(&ws, cx, Scope::Control, None);
    let line = input(cx, &control, json!({ "block": uid, "text": "x" }));
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::SCOPE)),
            "terminal_input needs a Full token.".to_string()
        )
    );
    let (second, _) = another_workspace(handle, &ws, cx);
    assert_ne!(second, first);
    let limited = with_token(&ws, cx, Scope::Full, Some(vec![second.as_str()]));
    let line = input(cx, &limited, json!({ "block": uid, "text": "x" }));
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::NOT_FOUND)),
            format!("There's no block {uid}: GET /v1/blocks lists them.")
        )
    );
    assert_eq!(inputs(cx), n + 4, "none of those reached the terminal");
}

#[gpui_kit::test]
fn terminal_input_sends_the_mouse_the_program_asked_for(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&terminal].clone());
    let token = with_token(&ws, cx, Scope::Full, None);
    let inputs = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(terminal).unwrap().term.inputs()
        })
    };
    let mouse = |cx: &mut TestAppContext, token: &Token, events: Value| {
        tool(
            handle,
            &ws,
            cx,
            token,
            "terminal_input",
            json!({ "block": uid, "mouse": events }),
        )
    };
    let click = json!([
        { "kind": "press", "button": "left", "row": 1, "col": 1 },
        { "kind": "release", "button": "left", "row": 1, "col": 1 },
    ]);
    let n = inputs(cx);
    // Before the program asked: nothing is taken, and that is not an error.
    let line = mouse(cx, &token, click.clone());
    assert_eq!(
        said(&line),
        (false, format!("Sent 0 mouse events to {uid}."))
    );
    assert_eq!(inputs(cx), n);
    // As a program's output asks for it.
    ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(terminal)
            .unwrap()
            .term
            .feed(b"\x1b[?1000h\x1b[?1006h")
    });
    let line = mouse(cx, &token, click);
    assert_eq!(
        said(&line),
        (false, format!("Sent 2 mouse events to {uid}."))
    );
    assert_eq!(inputs(cx), n + 2);
    let by = ws.read_with(cx, |ws, _| {
        ws.space.terminal(terminal).unwrap().keys_from.clone()
    });
    assert_eq!(by.map(|(by, _)| by).as_deref(), Some("API · phone"));
    // A move with no button is every motion, which only `?1003` asks for.
    let hover = json!([{ "kind": "move", "button": null, "row": 0, "col": 0 }]);
    let line = mouse(cx, &token, hover.clone());
    assert_eq!(
        said(&line),
        (false, format!("Sent 0 mouse events to {uid}."))
    );
    ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(terminal)
            .unwrap()
            .term
            .feed(b"\x1b[?1003h")
    });
    let line = mouse(cx, &token, hover.clone());
    assert_eq!(
        said(&line),
        (false, format!("Sent 1 mouse event to {uid}."))
    );
    // A Control token may not.
    let control = with_token(&ws, cx, Scope::Control, None);
    let line = mouse(cx, &control, hover);
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::SCOPE)),
            "terminal_input needs a Full token.".to_string()
        )
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn input_to_an_ended_shell_is_refused(cx: &mut TestAppContext) {
    // A shell that ends at its first line.
    let ends = Some(("sh".to_string(), vec!["-c".into(), "read line".into()]));
    let (handle, ws) = open_as(cx, ends, None, None);
    let id = first_id(&ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&id].clone());
    let token = with_token(&ws, cx, Scope::Full, None);
    // Typed past the API: the draw that ends every update would close the block of an ended
    // shell.
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(id).unwrap().term.write(&b"\n"[..])
    });
    // `read_with` never draws: the block stays until the shell is seen to have ended.
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.exited()) {
        assert!(
            std::time::Instant::now() < deadline,
            "the shell never ended"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let line = tool(
        handle,
        &ws,
        cx,
        &token,
        "terminal_input",
        json!({ "block": uid, "text": "ls" }),
    );
    assert_eq!(
        said(&line),
        (
            true,
            format!("{uid}'s shell has ended: nothing typed reaches it.")
        )
    );
}

/// What the API's `terminal_input` does to the agent in `theirs`, as the person's own typing
/// does: a draft, and no notice typed over it.
#[cfg(unix)]
#[gpui_kit::test]
fn terminal_input_keys_are_the_persons_typing_for_the_draft(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = super::delegate::two_agents(cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&theirs].clone());
    let token = with_token(&ws, cx, Scope::Full, None);
    let state = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let pane = ws.space.terminal(theirs).unwrap();
            (pane.has_draft(), pane.takes_notice())
        })
    };
    assert_eq!(state(cx), (false, Some(true)));
    let mut conn = 910;
    let mut press = |cx: &mut TestAppContext, key: &str| {
        conn += 1;
        api_later(
            handle,
            &ws,
            cx,
            &token,
            conn,
            "tools/call",
            json!({ "name": "terminal_input", "arguments": { "block": uid, "keys": [key] } }),
        );
        cx.run_until_parked();
    };
    press(cx, "a");
    assert_eq!(state(cx), (true, Some(false)), "half a message is typed");
    // Another agent's Enter sends the draft.
    press(cx, "enter");
    assert!(!state(cx).0);
    // A key that types nothing leaves a draft as it was.
    press(cx, "a");
    press(cx, "left");
    assert!(state(cx).0);
}

/// The paste is the person's text too, and it says who typed it.
#[cfg(unix)]
#[gpui_kit::test]
fn terminal_input_text_is_the_persons_and_says_who(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = super::delegate::two_agents(cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&theirs].clone());
    let token = with_token(&ws, cx, Scope::Full, None);
    tool(
        handle,
        &ws,
        cx,
        &token,
        "terminal_input",
        json!({ "block": uid, "text": "please also ch" }),
    );
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(theirs).unwrap();
        assert!(pane.has_draft());
        assert_eq!(
            pane.keys_from.as_ref().map(|(by, _)| by.as_str()),
            Some("API · phone")
        );
        assert_eq!(pane.takes_notice(), Some(false));
    });
}

// The finer canvas: the camera in the state, and the tick that follows a drag.

#[gpui_kit::test]
fn the_state_has_the_shown_workspaces_view(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let token = with_token(&ws, cx, Scope::Read, None);
    // Before the first paint the canvas has no size: no view.
    ws.update(cx, |ws, _| {
        ws.api.viewport = templum_canvas::Size::default()
    });
    assert_eq!(state(handle, &ws, cx, &token).view, None);
    ws.update(cx, |ws, _| {
        ws.space.canvas.camera = Camera::at(Point::new(100.0, 50.0), 2.0);
        ws.api.viewport = templum_canvas::Size::new(800.0, 600.0);
    });
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    assert_eq!(
        state(handle, &ws, cx, &token).view,
        Some(templum_api::model::ViewView {
            workspace: first.clone(),
            x: 100.0,
            y: 50.0,
            w: 400.0,
            h: 300.0,
            zoom: 2.0,
        })
    );
    // A token that doesn't see the shown workspace doesn't see its camera either.
    let (second, _) = another_workspace(handle, &ws, cx);
    let shown = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let hidden = if shown == second { first } else { second };
    let other = with_token(&ws, cx, Scope::Read, Some(vec![hidden.as_str()]));
    assert_eq!(state(handle, &ws, cx, &other).view, None);
    // The paint sets the size.
    ws.update(cx, |ws, _| {
        ws.api.viewport = templum_canvas::Size::default()
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    let size = ws.read_with(cx, |ws, _| ws.api.viewport);
    assert!(size.w > 0.0 && size.h > 0.0, "{size:?}");
}

#[gpui_kit::test]
fn the_state_has_no_view_in_grid_mode(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let token = with_token(&ws, cx, Scope::Read, None);
    let id = first_id(&ws, cx);
    ws.update(cx, |ws, _| {
        ws.api.viewport = templum_canvas::Size::new(800.0, 600.0)
    });
    assert!(state(handle, &ws, cx, &token).view.is_some());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.enter_grid(vec![id], window, cx))
    })
    .unwrap();
    assert!(ws.read_with(cx, |ws, _| ws.space.grid.is_some()));
    assert_eq!(
        state(handle, &ws, cx, &token).view,
        None,
        "tiles, not a camera"
    );
}

#[gpui_kit::test]
fn the_tick_is_fast_for_a_moment_after_the_camera_moves_without_a_drag(cx: &mut TestAppContext) {
    use crate::workspace::api::MOVED_FOR;
    let (_, ws) = open(cx);
    let (slow, fast) = (Duration::from_millis(250), Duration::from_millis(100));
    // The clock is the test's: a stalled machine can't make a moment pass.
    let wait = |cx: &mut TestAppContext, later: Duration| {
        ws.read_with(cx, |ws, _| ws.api_tick(ws.api.moved_at.unwrap() + later))
    };
    let now = std::time::Instant::now();
    assert_eq!(
        (
            ws.read_with(cx, |ws, _| ws.api_tick(now)),
            ws.read_with(cx, |ws, _| ws.drag.is_none())
        ),
        (slow, true)
    );
    // A touchpad pan: no drag.
    ws.update(cx, |ws, _| ws.api.moved_at = None);
    ws.update(cx, |ws, _| ws.pan_by(Point::new(5.0, 5.0)));
    assert_eq!(wait(cx, Duration::ZERO), fast);
    assert_eq!(wait(cx, MOVED_FOR - Duration::from_millis(1)), fast);
    // A while later it is slow again.
    assert_eq!(wait(cx, MOVED_FOR), slow);
    // A zoom (the wheel, a pinch) moves it too.
    ws.update(cx, |ws, _| ws.api.moved_at = None);
    let to = ws.read_with(cx, |ws, _| ws.space.canvas.camera);
    let to = Camera::at(Point::new(to.origin.x + 50.0, to.origin.y), to.zoom());
    ws.update(cx, |ws, cx| ws.fly(to, Point::new(0.0, 0.0), cx));
    assert_eq!(wait(cx, Duration::ZERO), fast);
}

#[gpui_kit::test]
fn the_tick_is_faster_while_the_canvas_moves(cx: &mut TestAppContext) {
    use crate::workspace::Drag;
    use templum_canvas::{Edges, Gesture, Size};
    let (_, ws) = open(cx);
    let id = first_id(&ws, cx);
    let wait = |cx: &mut TestAppContext, drag: Option<Drag>| {
        ws.update(cx, |ws, _| {
            ws.drag = drag;
            ws.api_tick(std::time::Instant::now())
        })
    };
    let (slow, fast) = (Duration::from_millis(250), Duration::from_millis(100));
    assert_eq!(wait(cx, None), slow);
    let resize = Gesture::Resize {
        id,
        edges: Edges::RIGHT,
        grab: Point::new(0.0, 0.0),
        min: Size::new(10.0, 10.0),
    };
    assert_eq!(wait(cx, Some(Drag::Canvas(resize))), fast);
    let pan = Gesture::Pan {
        last: Point::new(0.0, 0.0),
    };
    assert_eq!(wait(cx, Some(Drag::Canvas(pan))), fast);
    let held = Drag::Held {
        gesture: Gesture::Move {
            id,
            grab: Point::new(0.0, 0.0),
        },
        from: Point::new(0.0, 0.0),
        pointer: Point::new(0.0, 0.0),
        scrolled: None,
        picks: None,
        carried: false,
    };
    assert_eq!(wait(cx, Some(held)), fast);
    assert_eq!(wait(cx, Some(Drag::Select(id))), slow);
    wait(cx, None);
}

// Shares: a token limited to one workspace and its connection code.

/// A share of `workspace` in the store, as Share… makes one: its token and its id.
fn make_share(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    workspace: &str,
    scope: Scope,
) -> (Token, String) {
    use templum_api::store::{Share, Way};
    ws.update(cx, |ws, _| {
        let store = ws.api.store.get_or_insert_with(Store::default);
        let share = Share {
            workspace: workspace.into(),
            way: Way::Public,
            address: "https://t.example.com".into(),
        };
        let (id, _) = store
            .mint_share(share, "Another computer", scope, None, 0)
            .unwrap();
        let token = store.tokens.iter().find(|t| t.id == id).unwrap().clone();
        let share_id = token.share_id().unwrap();
        (token, share_id)
    })
}

#[gpui_kit::test]
fn a_share_answers_its_code_once(cx: &mut TestAppContext) {
    use templum_api::code::{self, Trust};
    use templum_api::store::{Listen, Tls, verify};
    let (handle, ws, dir) = with_data(cx);
    let (first, name) = ws.read_with(cx, |ws, _| (ws.space.id.clone(), ws.space.name.clone()));
    let full = with_token(&ws, cx, Scope::Full, None);
    // The API fits a pinned share (no server runs: nothing here listens).
    let settings = |ws: &Entity<Workspace>, cx: &mut TestAppContext, tls: Tls| {
        ws.update(cx, |ws, _| {
            let s = &mut ws.api.store.as_mut().unwrap().settings;
            (s.listen, s.tls) = (Listen::All, tls);
        })
    };
    settings(&ws, cx, Tls::Own);
    let args = |way: &str, address: &str| {
        json!({"workspace": first, "name": "Another computer", "scope": "full",
            "expires": null, "way": way, "address": address})
    };
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "share_workspace",
        args("pinned", "192.168.1.5:7411"),
    );
    let share_id = {
        let (is_error, text) = said(&line);
        assert!(!is_error, "{text}");
        text.strip_prefix(&format!("Shared {name} as "))
            .and_then(|rest| rest.strip_suffix('.'))
            .unwrap_or_else(|| panic!("{text}"))
            .to_string()
    };
    assert_eq!(line["result"]["target"]["workspace"], json!(first));
    let decoded = code::decode(line["result"]["code"].as_str().unwrap()).unwrap();
    let pin = templum_api::tls::pin(&templum_api::tls::own(dir.path()).unwrap().chain[0]);
    assert_eq!(decoded.url, "https://192.168.1.5:7411");
    assert_eq!(decoded.trust, Trust::Pinned(pin));
    let token = ws.read_with(cx, |ws, _| {
        let store = ws.api.store.as_ref().unwrap();
        verify(&store.tokens, decoded.key.expose(), 1).cloned()
    });
    let token = token.expect("the code's key is a token of the store");
    assert_eq!(token.workspaces, Some(vec![first.clone()]));
    assert_eq!(token.share_id().as_deref(), Some(share_id.as_str()));
    // Only its hash is kept; the person's dialog has nothing to show.
    let file = std::fs::read_to_string(templum_api::store::folder(dir.path()).join("api.json"));
    assert!(!file.unwrap().contains("tpl_"));
    let seen = state(handle, &ws, cx, &full);
    assert_eq!(seen.shares.len(), 1);
    assert!(!serde_json::to_string(&seen).unwrap().contains("tpl_"));
    ws.read_with(cx, |ws, _| assert!(ws.api.shown.is_none()));
    // A tunnel's address is kept for the next share.
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "share_workspace",
        args("public", " https://t.example.com/ "),
    );
    assert!(!said(&line).0, "{line}");
    let decoded = code::decode(line["result"]["code"].as_str().unwrap()).unwrap();
    assert_eq!(
        (decoded.url.as_str(), decoded.trust),
        ("https://t.example.com", Trust::Public)
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.api.store.as_ref().unwrap().tunnel.as_deref(),
            Some("https://t.example.com/")
        )
    });
    // Refusals say what to fix; nothing is made.
    let shares = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.share_views().len())
    };
    assert_eq!(shares(&ws, cx), 2);
    settings(&ws, cx, Tls::Off);
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "share_workspace",
        args("pinned", "192.168.1.5:7411"),
    );
    assert_eq!(
        said(&line),
        (
            true,
            "A pinned share needs HTTPS with Templum's certificate: Settings ▸ API ▸ HTTPS.".into()
        )
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "share_workspace",
        args("public", "templum.example.com"),
    );
    assert_eq!(
        said(&line),
        (true, "The address must start with https://.".into())
    );
    settings(&ws, cx, Tls::Own);
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "share_workspace",
        args("pinned", "https://x:1"),
    );
    assert_eq!(
        said(&line),
        (true, "Give Directly's address as host:port.".into())
    );
    // A code that can't be made is this address's fault, said in home's words (not the mirror's).
    for (way, address) in [("pinned", "a b:1"), ("public", "https://")] {
        let line = tool(
            handle,
            &ws,
            cx,
            &full,
            "share_workspace",
            args(way, address),
        );
        assert_eq!(
            said(&line),
            (true, "Templum can't make a code for this address.".into()),
            "{address}"
        );
    }
    // A login in the address would read as the host and be another.
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "share_workspace",
        args("public", "https://templum.example.com@evil.com"),
    );
    assert_eq!(
        said(&line),
        (
            true,
            "The address is a host alone: no login, query or fragment.".into()
        )
    );
    // An expiry already past makes no share.
    let mut past = args("pinned", "192.168.1.5:7411");
    past["expires"] = json!(1);
    let line = tool(handle, &ws, cx, &full, "share_workspace", past);
    assert_eq!(
        said(&line),
        (
            true,
            "The expiry is already past: choose a later time.".into()
        )
    );
    assert_eq!(shares(&ws, cx), 2, "a refusal makes no token");
    // Who may share: a token for all workspaces, Full.
    let limited = with_token(&ws, cx, Scope::Full, Some(vec![first.as_str()]));
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "share_workspace",
        args("public", "https://t.example.com"),
    );
    assert_eq!(
        (
            line["error"]["code"].as_i64(),
            line["error"]["message"].as_str()
        ),
        (
            Some(i64::from(codes::SCOPE)),
            Some("share_workspace needs a Full token for all workspaces.")
        )
    );
    let control = with_token(&ws, cx, Scope::Control, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "share_workspace",
        args("public", "https://t.example.com"),
    );
    assert_eq!(
        line["error"]["message"].as_str(),
        Some("share_workspace needs a Full token.")
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "share_workspace",
        json!({"workspace": "nowhere", "name": "x", "scope": "full", "way": "public", "address": "https://t.example.com"}),
    );
    assert_eq!(
        line["error"]["code"].as_i64(),
        Some(i64::from(codes::NOT_FOUND))
    );
    assert_eq!(shares(&ws, cx), 2);
}

#[gpui_kit::test]
fn revoke_share_takes_the_callers_own_or_needs_full(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = with_data(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let full = with_token(&ws, cx, Scope::Full, None);
    let (a, a_id) = make_share(&ws, cx, &first, Scope::Control);
    let (b, b_id) = make_share(&ws, cx, &first, Scope::Full);
    let (_, c_id) = make_share(&ws, cx, &first, Scope::Full);
    let held = |ws: &Entity<Workspace>, cx: &mut TestAppContext, id: u64| {
        ws.read_with(cx, |ws, _| {
            ws.api
                .store
                .as_ref()
                .unwrap()
                .tokens
                .iter()
                .any(|t| t.id == id)
        })
    };
    // A share revokes itself, Control or not.
    let line = tool(handle, &ws, cx, &a, "revoke_share", json!({}));
    assert_eq!(said(&line), (false, format!("Revoked {a_id}.")));
    assert!(!held(&ws, cx, a.id));
    // But not another: that takes a Full token for all workspaces.
    let line = tool(handle, &ws, cx, &b, "revoke_share", json!({"share": c_id}));
    assert_eq!(
        (
            line["error"]["code"].as_i64(),
            line["error"]["message"].as_str()
        ),
        (
            Some(i64::from(codes::SCOPE)),
            Some("revoke_share needs a Full token for all workspaces.")
        )
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "revoke_share",
        json!({"share": b_id}),
    );
    assert_eq!(said(&line), (false, format!("Revoked {b_id}.")));
    assert!(!held(&ws, cx, b.id));
    let line = tool(
        handle,
        &ws,
        cx,
        &full,
        "revoke_share",
        json!({"share": "s999"}),
    );
    assert_eq!(
        (
            line["error"]["code"].as_i64(),
            line["error"]["message"].as_str()
        ),
        (
            Some(i64::from(codes::NOT_FOUND)),
            Some("There's no share s999: GET /v1/shares lists them.")
        )
    );
    // A token Settings made is no share.
    let line = tool(handle, &ws, cx, &full, "revoke_share", json!({}));
    assert_eq!(
        (
            line["error"]["code"].as_i64(),
            line["error"]["message"].as_str()
        ),
        (
            Some(i64::from(rpc_params())),
            Some("revoke_share needs \"share\": this token isn't a share.")
        )
    );
    assert!(held(&ws, cx, full.id));
}

fn rpc_params() -> i32 {
    templum_mcp::rpc::PARAMS
}

#[gpui_kit::test]
fn the_sharing_rows_say_who_mirrors_and_when(cx: &mut TestAppContext) {
    use crate::share_dialog::sharing_words;
    use templum_api::model::{LastView, ShareView};
    let (_handle, ws, _dir) = with_data(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, a) = make_share(&ws, cx, &first, Scope::Control);
    let (_, b) = make_share(&ws, cx, &first, Scope::Full);
    let (_, elsewhere) = make_share(&ws, cx, "w9", Scope::Full);
    ws.read_with(cx, |ws, _| {
        let ids: Vec<_> = ws.share_rows(&first).into_iter().map(|s| s.id).collect();
        assert_eq!(
            ids,
            [a.clone(), b.clone()],
            "oldest first, of this workspace only"
        );
        assert_eq!(ws.share_rows("w9").len(), 1);
        assert!(ws.share_rows("w9").iter().all(|s| s.id == elsewhere));
    });
    let now = 1_000_000;
    let day = 86_400;
    let share = |scope, expires, last: Option<(u64, &str)>, mirrored| ShareView {
        scope,
        expires,
        last: last.map(|(at, addr)| LastView {
            at,
            addr: addr.into(),
        }),
        mirrored,
        ..ShareView::default()
    };
    assert_eq!(
        sharing_words(
            &share(Scope::Full, Some(now + 29 * day), None, false),
            "",
            now
        ),
        (
            "Full · expires in 29 days".to_string(),
            "never used".to_string()
        )
    );
    assert_eq!(
        sharing_words(
            &share(
                Scope::Full,
                Some(now + day),
                Some((now, "198.51.100.2")),
                true
            ),
            "203.0.113.7",
            now
        )
        .1,
        "mirrored now from 203.0.113.7",
        "the stream's address, not the last call's"
    );
    assert_eq!(
        sharing_words(
            &share(
                Scope::Control,
                None,
                Some((now - 3 * 3600, "203.0.113.7")),
                false
            ),
            "",
            now
        ),
        (
            "Control · never expires".to_string(),
            "last used 3 h ago from 203.0.113.7".to_string()
        )
    );
    assert_eq!(
        sharing_words(
            &share(Scope::Full, Some(now - 2 * day), None, false),
            "",
            now
        )
        .0,
        "Full · expired 2 days ago"
    );
}

#[gpui_kit::test]
fn a_share_is_a_workspaces_only_while_its_token_reaches_it(cx: &mut TestAppContext) {
    let (_handle, ws, _dir) = with_data(cx);
    let (token, _) = make_share(&ws, cx, "w9", Scope::Full);
    ws.read_with(cx, |ws, _| assert_eq!(ws.share_rows("w9").len(), 1));
    // The workspace was deleted before it was ever saved; its id comes round again.
    ws.update(cx, |ws, cx| ws.api_forget_workspace("w9", cx));
    ws.read_with(cx, |ws, _| {
        let kept = ws.api.store.as_ref().unwrap().tokens[0].clone();
        assert_eq!(kept.id, token.id);
        assert_eq!(kept.share.as_ref().unwrap().workspace, "w9");
        assert!(ws.share_rows("w9").is_empty(), "the new w9 isn't its");
    });
}

#[gpui_kit::test]
fn the_dialogs_button_makes_the_api_fit(cx: &mut TestAppContext) {
    use templum_api::store::{Listen, Tls, Way, share_problem};
    let (_handle, ws, _dir) = with_data(cx);
    // An ephemeral port, the API off: the dialog's button turns it on.
    ws.update(cx, |ws, cx| ws.api_edit(|s| s.port = 0, cx));
    let settings = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            (
                ws.api.store.as_ref().unwrap().settings.clone(),
                ws.api.server.is_some(),
            )
        })
    };
    let (before, listening) = settings(&ws, cx);
    assert!(!before.enabled && !listening);
    ws.update(cx, |ws, cx| ws.fit_api(Way::Public, cx));
    let (s, listening) = settings(&ws, cx);
    assert!(s.enabled && s.listen == Listen::Loopback && s.tls == Tls::Off && listening);
    assert_eq!(
        share_problem(&s, Way::Public, "https://t.example.com"),
        None
    );
    assert!(
        share_problem(&s, Way::Pinned, "x:1").is_some(),
        "loopback and plain don't fit"
    );
    // On, but not for a direct share: the same button applies what is missing.
    ws.update(cx, |ws, cx| ws.fit_api(Way::Pinned, cx));
    let (s, listening) = settings(&ws, cx);
    assert!(s.listen == Listen::All && s.tls == Tls::Own && listening);
    assert_eq!(share_problem(&s, Way::Pinned, "192.168.1.5:7411"), None);
}

#[gpui_kit::test]
fn the_share_dialog_makes_a_code_shows_it_once_and_drops_it(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use templum_api::code;
    no_motion(cx);
    let (handle, ws, _dir) = with_data(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    // An ephemeral port, and a tunnel remembered from an earlier share.
    ws.update(cx, |ws, cx| {
        ws.api_edit(|s| s.port = 0, cx);
        ws.api.store.as_mut().unwrap().tunnel = Some("https://t.example.com".into());
    });
    let shares = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.share_views().len())
    };
    cx.update_window(handle, |_, window, cx| {
        let flow = ws
            .update(cx, |ws, cx| {
                crate::share_dialog::open(ws, &first, window, cx)
            })
            .expect("a data folder");
        window.render_frame(cx);
        // The API is off: the dialog says so, and Make code is not pressable.
        assert!(window.try_find("share-problem").is_some());
        window.click("share-make", cx);
        window.render_frame(cx);
        assert_eq!(ws.read(cx).share_views().len(), 0);
        assert!(flow.read(cx).code().is_none());
        // Turn it on: nothing is wrong now.
        window.click("share-fix", cx);
        window.render_frame(cx);
        assert!(ws.read(cx).api.server.is_some());
        assert!(window.try_find("share-problem").is_none());
        window.click("share-make", cx);
        window.render_frame(cx);
        assert!(window.try_find("share-code").is_some(), "the code is shown");
        assert!(window.try_find("share-paste-it").is_some());
        let text = flow.read(cx).code().expect("made").to_string();
        let decoded = code::decode(&text).unwrap();
        assert_eq!(decoded.url, "https://t.example.com");
        assert_eq!(ws.read(cx).share_rows(&first).len(), 1);
        // Done: the dialog closes and the text is gone from it.
        window.click("share-done", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        assert!(
            flow.read(cx).code().is_none(),
            "nothing holds the code after Done"
        );
        assert!(ws.read(cx).api.shown.is_none());
    })
    .unwrap();
    assert_eq!(shares(&ws, cx), 1);
}

#[gpui_kit::test]
fn the_share_dialog_keeps_blocking_when_the_api_failed_to_start(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = with_data(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    // The port is taken by someone else.
    let taken = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let busy = taken.local_addr().unwrap().port();
    ws.update(cx, |ws, cx| {
        ws.api_edit(|s| s.port = busy, cx);
        ws.api.store.as_mut().unwrap().tunnel = Some("https://t.example.com".into());
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::share_dialog::open(ws, &first, window, cx)
        })
        .expect("a data folder");
        window.render_frame(cx);
        window.click("share-fix", cx);
        window.render_frame(cx);
        assert!(ws.read(cx).api.server.is_none());
        assert!(ws.read(cx).api.error.is_some(), "the server could not bind");
        // The problem line stays, with the reason, and Make code makes nothing.
        assert!(window.try_find("share-problem").is_some());
        window.click("share-make", cx);
        window.render_frame(cx);
        assert_eq!(ws.read(cx).share_views().len(), 0);
        assert!(window.try_find("share-code").is_none());
    })
    .unwrap();
    drop(taken);
}

#[gpui_kit::test]
fn the_edit_dialog_lists_shares_and_revokes_at_once(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = with_data(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (token, share) = make_share(&ws, cx, &first, Scope::Full);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::space_dialog::open(ws, &first, window, cx)
        });
        window.render_frame(cx);
        assert!(window.try_find("space-share-invite").is_none());
        assert!(window.try_find(format!("space-share-{share}")).is_some());
        assert!(
            window.try_find("space-share").is_some(),
            "Share… for one more"
        );
        // Revoke needs no question and no Save.
        window.click(format!("space-revoke-{share}"), cx);
        window.render_frame(cx);
        assert!(window.try_find(format!("space-share-{share}")).is_none());
        assert!(window.try_find("space-share-invite").is_some());
        assert!(
            ws.read(cx)
                .api
                .store
                .as_ref()
                .unwrap()
                .tokens
                .iter()
                .all(|t| t.id != token.id)
        );
        // Share… opens a second dialog over this one.
        window.click("space-share", cx);
        window.render_frame(cx);
        assert!(window.try_find("share-make").is_some());
        window.click("share-cancel", cx);
        window.render_frame(cx);
        assert!(
            window.try_find("space-share").is_some(),
            "the Edit… dialog is still there"
        );
        window.click("cancel", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_mirrored_workspace_is_named_with_its_mirror(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    use std::io::{Read, Write};
    use templum_api::store::{Share, Way};
    let (handle, ws, _dir) = with_data(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    // A share of the first workspace, before the server starts with the tokens.
    let text = ws.update(cx, |ws, _| {
        let share = Share {
            workspace: first.clone(),
            way: Way::Public,
            address: "https://t.example.com".into(),
        };
        let store = ws.api.store.get_or_insert_with(Store::default);
        store
            .mint_share(share, "Another computer", Scope::Full, None, 0)
            .unwrap()
            .1
    });
    turn_on(&ws, cx);
    let port = port(&ws, cx);
    let (opened, open) = std::sync::mpsc::channel::<()>();
    let (close, closing) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        write!(
            stream,
            "GET /v1/events HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer {text}\r\n\r\n"
        )
        .unwrap();
        // Read until the snapshot is in: the stream is open.
        let mut all = String::new();
        let mut buf = [0u8; 4096];
        while !all.contains("event: snapshot") {
            let n = stream.read(&mut buf).unwrap();
            assert!(n > 0, "the stream ended early: {all}");
            all.push_str(&String::from_utf8_lossy(&buf[..n]));
        }
        let _ = opened.send(());
        // Held open until the test says; then the client leaves.
        let _ = closing.recv();
    });
    let mirrors =
        |cx: &mut TestAppContext, space: &str| ws.read_with(cx, |ws, _| ws.mirrored_now(space));
    wait_until(cx, "the mirror to be named", |cx| {
        !mirrors(cx, &first).is_empty()
    });
    open.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        mirrors(cx, &first),
        [("Another computer".to_string(), "127.0.0.1".to_string())]
    );
    assert!(mirrors(cx, "w9").is_empty());
    // The state says it too, for a client that reads shares.
    let line = ws.read_with(cx, |ws, _| ws.share_views());
    assert!(line[0].mirrored);
    // And the sidebar's tile wears the cast mark.
    let mark = format!("workspace-mirrored-{first}");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(mark.clone()).is_some());
    })
    .unwrap();
    let _ = close.send(());
    wait_until(cx, "the mirror to leave", |cx| {
        mirrors(cx, &first).is_empty()
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(mark.clone()).is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_share_of_a_deleted_workspace_sees_nothing_and_can_still_be_revoked(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = with_data(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (token, share) = make_share(&ws, cx, &first, Scope::Full);
    ws.update(cx, |ws, cx| ws.api_forget_workspace(&first, cx));
    let kept = ws.read_with(cx, |ws, _| ws.api.store.as_ref().unwrap().tokens[0].clone());
    assert_eq!(
        kept.workspaces,
        Some(vec![]),
        "it is nobody's workspace now"
    );
    let seen = state(handle, &ws, cx, &kept);
    assert!(seen.workspaces.is_empty() && seen.blocks.is_empty());
    // It still has its share, naming the workspace that was, for Settings to say so.
    assert_eq!(seen.shares.len(), 1);
    assert_eq!(seen.shares[0].workspace, first);
    let names = std::collections::HashMap::new();
    assert_eq!(
        crate::settings::api::share_line(&kept, &names).as_deref(),
        Some("share of a deleted workspace")
    );
    let line = tool(handle, &ws, cx, &kept, "revoke_share", json!({}));
    assert_eq!(said(&line), (false, format!("Revoked {share}.")));
    assert!(ws.read_with(cx, |ws, _| {
        ws.api
            .store
            .as_ref()
            .unwrap()
            .tokens
            .iter()
            .all(|t| t.id != token.id)
    }));
}

#[gpui_kit::test]
fn edit_item_and_delete_item_act_as_you_on_any_checklist_block(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (block, list) = checklist_block(handle, &ws, cx, "Release", &["Tag it", "Ship"]);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&block].clone());
    let terminal = ws.read_with(cx, |ws, _| {
        let id = ws.space.canvas.blocks()[0].id;
        ws.space.uids[&id].clone()
    });
    let tag = ws.read_with(cx, |ws, _| ws.checklists.get(list).unwrap().items[0].id);
    let control = with_token(&ws, cx, Scope::Control, None);
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "edit_item",
        json!({"block": uid, "item": tag, "text": "x"}),
    );
    assert_eq!(
        said(&line),
        (false, format!("Item {tag} is now \"x\".")),
        "{line}"
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.checklists.get(list).unwrap().items[0]
            .text
            .clone()),
        "x"
    );
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals.is_empty()));
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "edit_item",
        json!({"block": uid, "item": tag, "text": "  "}),
    );
    assert_eq!(
        said(&line),
        (true, "delete_item removes an item".to_string()),
        "{line}"
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "delete_item",
        json!({"block": uid, "item": 99}),
    );
    assert_eq!(
        said(&line),
        (
            true,
            "There's no item 99 in \"Release\". read_checklist shows its items.".to_string()
        )
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "delete_item",
        json!({"block": terminal, "item": 1}),
    );
    assert_eq!(
        said(&line),
        (true, format!("{terminal} isn't a checklist.")),
        "{line}"
    );
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "delete_item",
        json!({"block": uid, "item": tag}),
    );
    assert_eq!(
        said(&line),
        (false, format!("Deleted item {tag}.")),
        "{line}"
    );
    assert!(
        ws.read_with(cx, |ws, _| ws.templum_agent.approvals.is_empty()),
        "the token is your hand"
    );

    let (second, _) = another_workspace(handle, &ws, cx);
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![second.as_str()]));
    let line = tool(
        handle,
        &ws,
        cx,
        &limited,
        "edit_item",
        json!({"block": uid, "item": tag, "text": "y"}),
    );
    assert_eq!(
        refused(&line).0,
        Some(i64::from(codes::NOT_FOUND)),
        "{line}"
    );
    let read = with_token(&ws, cx, Scope::Read, Some(vec![first.as_str()]));
    let line = tool(
        handle,
        &ws,
        cx,
        &read,
        "edit_item",
        json!({"block": uid, "item": tag, "text": "y"}),
    );
    assert_eq!(refused(&line).0, Some(i64::from(codes::SCOPE)), "{line}");
}

#[gpui_kit::test]
fn edit_checklist_renames_and_moves_and_new_checklist_fills_an_empty_block(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (block, list) = checklist_block(handle, &ws, cx, "Release", &["Tag it", "Ship", "Tell"]);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&block].clone());
    let ids = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let l = ws.checklists.get(list).unwrap();
            (
                l.name.clone(),
                l.items.iter().map(|i| i.id).collect::<Vec<_>>(),
            )
        })
    };
    let (_, items) = ids(&ws, cx);
    let (tag, ship, tell) = (items[0], items[1], items[2]);
    let control = with_token(&ws, cx, Scope::Control, None);
    let call = |cx: &mut TestAppContext, name: &str, args: Value| {
        said(&tool(handle, &ws, cx, &control, name, args))
    };
    assert_eq!(
        call(
            cx,
            "edit_checklist",
            json!({"block": uid, "name": " Release 2 ", "item": tell, "before": tag})
        ),
        (
            false,
            format!("The checklist is \"Release 2\". Item {tell} is before item {tag}.")
        )
    );
    assert_eq!(
        ids(&ws, cx),
        ("Release 2".to_string(), vec![tell, tag, ship])
    );
    assert_eq!(
        call(
            cx,
            "edit_checklist",
            json!({"block": uid, "item": tell, "before": "end"})
        ),
        (false, format!("Item {tell} is at the end."))
    );
    assert_eq!(ids(&ws, cx).1, [tag, ship, tell]);
    assert_eq!(
        call(
            cx,
            "edit_checklist",
            json!({"block": uid, "item": tag, "before": 99})
        ),
        (
            true,
            "There's no item 99 in \"Release 2\". read_checklist shows its items.".to_string()
        )
    );
    assert_eq!(ids(&ws, cx).1, [tag, ship, tell], "nothing moved");
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals.is_empty()));
    // Each is one step of the checklist's Ctrl+Z.
    ws.update(cx, |ws, cx| assert!(ws.undo_checklists(cx)));
    assert_eq!(ids(&ws, cx).1, [tell, tag, ship]);
    ws.update(cx, |ws, cx| assert!(ws.undo_checklists(cx)));
    assert_eq!(ids(&ws, cx), ("Release".to_string(), vec![tag, ship, tell]));
    // A block that shows one keeps it; one that shows none shows a new one.
    let line = call(cx, "new_checklist", json!({"block": uid}));
    assert_eq!(
        line,
        (
            true,
            format!("{uid} shows a checklist already: add_block opens a block with a new one.")
        )
    );
    ws.update(cx, |ws, cx| {
        ws.checklists.remove(list);
        cx.notify()
    });
    let (made, text) = call(cx, "new_checklist", json!({"block": uid, "name": "Plan"}));
    let shown = ws.read_with(cx, |ws, _| {
        ws.space.blocks[&block].checklist().unwrap().list
    });
    let new = shown.expect("a checklist");
    assert_eq!(
        (made, text),
        (
            false,
            format!("Made checklist \"Plan\" ({new}) and showed it in {uid}.")
        )
    );
    assert_ne!(new, list);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.checklists.get(new).unwrap().name.clone()),
        "Plan"
    );
    // A limited token reaches only the blocks of its workspaces; a read token, none of these.
    let (second, _) = another_workspace(handle, &ws, cx);
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![second.as_str()]));
    for (name, args) in [
        ("edit_checklist", json!({"block": uid, "name": "x"})),
        ("new_checklist", json!({"block": uid})),
    ] {
        let line = tool(handle, &ws, cx, &limited, name, args);
        assert_eq!(
            refused(&line).0,
            Some(i64::from(codes::NOT_FOUND)),
            "{line}"
        );
    }
    let read = with_token(&ws, cx, Scope::Read, Some(vec![first.as_str()]));
    let line = tool(
        handle,
        &ws,
        cx,
        &read,
        "edit_checklist",
        json!({"block": uid, "name": "x"}),
    );
    assert_eq!(refused(&line).0, Some(i64::from(codes::SCOPE)), "{line}");
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.checklists.get(new).unwrap().name.clone()),
        "Plan"
    );
}

/// The note block `block`'s editor on screen.
fn shown_editor(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    block: BlockId,
) -> Entity<gpui_kit::component::input::EditorState> {
    ws.read_with(cx, |ws, _| {
        ws.space.blocks[&block]
            .note()
            .unwrap()
            .tab()
            .unwrap()
            .editor
            .clone()
    })
}

/// Note `id`'s `rev` as the state shows it to `token`.
fn note_rev(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
    id: u64,
) -> u64 {
    state(handle, ws, cx, token)
        .notes
        .iter()
        .find(|n| n.id == id)
        .map_or(0, |n| n.rev)
}

#[gpui_kit::test]
fn edit_note_writes_on_its_rev_only_and_keeps_home_s_caret(cx: &mut TestAppContext) {
    use templum_api::model::NOTE_TEXT_MOST;
    let (handle, ws) = open(cx);
    let (_, note, _) = three_blocks(handle, &ws, cx);
    let (uid, id) = ws.read_with(cx, |ws, _| {
        (
            ws.space.uids[&note].clone(),
            ws.space.blocks[&note].note().unwrap().tab().unwrap().id,
        )
    });
    let editor = shown_editor(&ws, cx, note);
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |e, cx| {
            e.set_value("first\nsecond", window, cx);
            let end = e.value().len();
            e.set_selected_range(end..end, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    let control = with_token(&ws, cx, Scope::Control, None);
    let rev = note_rev(handle, &ws, cx, &control, id);
    let edit = |cx: &mut TestAppContext, args: Value| {
        said(&tool(handle, &ws, cx, &control, "edit_note", args))
    };
    let answer = edit(
        cx,
        json!({"block": uid, "note": id, "text": "zero\nfirst\nsecond", "rev": rev}),
    );
    assert_eq!(answer, (false, format!("Saved note {id}.")));
    cx.run_until_parked();
    editor.read_with(cx, |e, _| {
        assert_eq!(e.value().as_ref(), "zero\nfirst\nsecond");
        assert_eq!(
            e.selected_range(),
            17..17,
            "home's caret is still after second"
        );
    });
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.notes.get(id).unwrap().text.clone()),
        "zero\nfirst\nsecond"
    );
    let newer = note_rev(handle, &ws, cx, &control, id);
    assert!(newer > rev, "{newer} > {rev}");
    // The same call again, on the old rev, is refused and writes nothing.
    assert_eq!(
        edit(
            cx,
            json!({"block": uid, "note": id, "text": "other", "rev": rev})
        ),
        (
            true,
            format!("Note {id} changed since: its newer text and rev are in the state.")
        )
    );
    editor.read_with(cx, |e, _| {
        assert_eq!(e.value().as_ref(), "zero\nfirst\nsecond")
    });
    // One Ctrl+Z of home's editor brings the old text back.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(note, window, cx));
        window.press(UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
    editor.read_with(cx, |e, _| assert_eq!(e.value().as_ref(), "first\nsecond"));
    cx.run_until_parked();
    // An agent's own write moves the rev too: a text made before it is refused.
    let before = note_rev(handle, &ws, cx, &control, id);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let space = ws.space.id.clone();
            ws.note_replace(&space, note, "agent wrote", None, window, cx)
                .unwrap()
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert!(note_rev(handle, &ws, cx, &control, id) > before);
    assert!(
        edit(
            cx,
            json!({"block": uid, "note": id, "text": "late", "rev": before})
        )
        .0,
        "an agent's write moved the rev"
    );
    // A name and a color in one call.
    assert_eq!(
        edit(
            cx,
            json!({"block": uid, "note": id, "name": "Plan", "color": "blue"})
        ),
        (
            false,
            format!("Renamed note {id} \"Plan\". Colored note {id} blue.")
        )
    );
    ws.read_with(cx, |ws, _| {
        let entry = ws.notes.get(id).unwrap();
        assert_eq!(
            (entry.name.as_str(), entry.color),
            ("Plan", Some(crate::board::Label::Blue))
        );
    });
    assert_eq!(
        edit(cx, json!({"block": uid, "note": id, "color": "none"})),
        (false, format!("Cleared the color of note {id}."))
    );
    // A note of another block.
    let other = ws.update(cx, |ws, _| ws.notes.add("elsewhere".into(), None, 0));
    let (failed, text) = edit(cx, json!({"block": uid, "note": other, "name": "x"}));
    assert!(
        failed && text == format!("{uid} shows no note {other}."),
        "{text}"
    );
    // A note made longer than 256 KiB at home refuses its text but takes a name.
    let long = ws.update(cx, |ws, _| {
        ws.notes.add("x".repeat(NOTE_TEXT_MOST + 1), None, 0)
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let tab = ws.note_tab(long, window, cx).unwrap();
            let shown = ws.space.blocks.get_mut(&note).unwrap().note_mut().unwrap();
            shown.tabs.push(tab);
        })
    })
    .unwrap();
    let rev = note_rev(handle, &ws, cx, &control, long);
    let (failed, text) = edit(
        cx,
        json!({"block": uid, "note": long, "text": "short", "rev": rev}),
    );
    assert!(
        failed && text.contains("longer than the API shows"),
        "{text}"
    );
    assert_eq!(
        edit(cx, json!({"block": uid, "note": long, "name": "Big"})),
        (false, format!("Renamed note {long} \"Big\"."))
    );
}

#[gpui_kit::test]
fn new_close_open_and_delete_a_note_through_the_api(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    let (uid, space) = ws.read_with(cx, |ws, _| {
        (ws.space.uids[&note].clone(), ws.space.id.clone())
    });
    let kept = ws.read_with(cx, |ws, _| ws.space.focused);
    let control = with_token(&ws, cx, Scope::Control, None);
    let full = with_token(&ws, cx, Scope::Full, None);
    let call = |cx: &mut TestAppContext, token: &Token, name: &str, args: Value| {
        tool(handle, &ws, cx, token, name, args)
    };
    let line = call(
        cx,
        &control,
        "new_note",
        json!({"block": uid, "name": "Plan", "text": "first line"}),
    );
    let (failed, text) = said(&line);
    assert!(!failed, "{text}");
    assert_eq!(
        line["result"]["target"],
        json!({"workspace": space, "block": uid})
    );
    let (made, tabs) = ws.read_with(cx, |ws, _| {
        let shown = ws.space.blocks[&note].note().unwrap();
        (shown.tab().unwrap().id, shown.tabs.len())
    });
    assert_eq!((text, tabs), (format!("Made note {made} in {uid}."), 2));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.notes.name(made), "Plan");
        assert_eq!(ws.notes.get(made).unwrap().text, "first line");
        assert_eq!(ws.space.focused, kept, "home's keyboard didn't move");
    });
    // Closing keeps the note, marked as last open here.
    let (failed, text) = said(&call(
        cx,
        &control,
        "close_note",
        json!({"block": uid, "note": made}),
    ));
    assert_eq!(
        (failed, text),
        (false, format!("Closed note {made} in {uid}."))
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.notes.get(made).unwrap().closed_in.as_deref(),
            Some(space.as_str())
        );
        assert_eq!(ws.space.blocks[&note].note().unwrap().tabs.len(), 1);
    });
    let seen = state(handle, &ws, cx, &control);
    let listed = seen.notes.iter().find(|n| n.id == made).unwrap();
    assert_eq!(
        (
            listed.workspace.as_deref(),
            listed.text.as_str(),
            listed.name.as_str()
        ),
        (Some(space.as_str()), "", "Plan")
    );
    assert!(listed.rev > 0 || ws.read_with(cx, |ws, _| ws.notes.rev == 0));
    // A blank note closed goes.
    let blank = said(&call(cx, &control, "new_note", json!({"block": uid}))).1;
    let blank: u64 = blank.split(' ').nth(2).unwrap().parse().unwrap();
    call(
        cx,
        &control,
        "close_note",
        json!({"block": uid, "note": blank}),
    );
    ws.read_with(cx, |ws, _| assert!(ws.notes.get(blank).is_none()));
    // open_note reopens it in a block.
    let (failed, text) = said(&call(
        cx,
        &control,
        "open_note",
        json!({"note": made, "block": uid}),
    ));
    assert_eq!(
        (failed, text),
        (false, format!("Opened note {made} in {uid}."))
    );
    let active = ws.read_with(cx, |ws, _| {
        ws.space.blocks[&note].note().unwrap().tab().unwrap().id
    });
    assert_eq!(active, made);
    // Open somewhere: just shown.
    call(
        cx,
        &control,
        "close_note",
        json!({"block": uid, "note": made}),
    );
    let line = call(
        cx,
        &control,
        "open_note",
        json!({"note": made, "workspace": space, "x": 900, "y": 700}),
    );
    let (failed, text) = said(&line);
    assert!(!failed, "{text}");
    let new_uid = line["result"]["target"]["block"]
        .as_str()
        .unwrap()
        .to_string();
    assert_ne!(new_uid, uid);
    assert_eq!(text, format!("Opened note {made} in {new_uid}."));
    let at = ws.read_with(cx, |ws, _| {
        let block = ws.space.block_by_uid(&new_uid).unwrap();
        let rect = ws.space.canvas.get(block).unwrap().rect;
        (
            (rect.x, rect.y),
            ws.space.blocks[&block].note().unwrap().ids(),
        )
    });
    assert_eq!((at.0, at.1.clone()), ((900.0, 700.0), vec![made]));
    // Open: showing the tab where it is.
    let (_, text) = said(&call(
        cx,
        &control,
        "open_note",
        json!({"note": made, "block": uid}),
    ));
    assert_eq!(text, format!("Opened note {made} in {new_uid}."));
    // Deleting is Full only.
    let refused = call(cx, &control, "delete_note", json!({"note": made}));
    assert_eq!(refused["error"]["code"], codes::SCOPE);
    ws.read_with(cx, |ws, _| assert!(ws.notes.get(made).is_some()));
    let (failed, text) = said(&call(cx, &full, "delete_note", json!({"note": made})));
    assert_eq!(
        (failed, text),
        (false, format!("Deleted note {made} \"Plan\"."))
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.notes.get(made).is_none());
        let block = ws.space.block_by_uid(&new_uid).unwrap();
        assert!(ws.space.blocks[&block].note().unwrap().tabs.is_empty());
    });
    let _ = terminal;
}

#[gpui_kit::test]
fn a_limited_token_reaches_only_its_workspace_s_notes(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, note, _) = three_blocks(handle, &ws, cx);
    let uid1 = ws.read_with(cx, |ws, _| ws.space.uids[&note].clone());
    let (second, _) = another_workspace(handle, &ws, cx);
    let (_, text) = super::templum_agent::control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({ "workspace": &second, "kind": "note", "text": "elsewhere" }),
    );
    let uid2 = text
        .trim_end_matches('.')
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string();
    let (open1, open2, closed1, unmarked) = ws.update(cx, |ws, _| {
        let open1 = ws.space.blocks[&note].note().unwrap().tab().unwrap().id;
        let open2 = ws
            .in_space(&second, |ws| {
                ws.space
                    .blocks
                    .values()
                    .find_map(|b| b.note()?.tab().map(|t| t.id))
            })
            .unwrap()
            .unwrap();
        let closed1 = ws.notes.add("closed in one".into(), None, 1);
        ws.notes.close_in(closed1, &first);
        let unmarked = ws.notes.add("closed long ago".into(), None, 1);
        (open1, open2, closed1, unmarked)
    });
    let two = with_token(&ws, cx, Scope::Full, Some(vec![second.as_str()]));
    let one = with_token(&ws, cx, Scope::Full, Some(vec![first.as_str()]));
    let call = |cx: &mut TestAppContext, token: &Token, name: &str, args: Value| {
        said(&tool(handle, &ws, cx, token, name, args))
    };
    // What a refusal says: the words of the error.
    let refusal = |cx: &mut TestAppContext, token: &Token, name: &str, args: Value| {
        let line = tool(handle, &ws, cx, token, name, args);
        line["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    // A token of w2 reaches none of w1's notes, however they are named.
    for id in [closed1, open1, unmarked] {
        for name in ["open_note", "delete_note"] {
            let words = refusal(cx, &two, name, json!({"note": id, "workspace": second}));
            assert_eq!(
                words,
                templum_api::registry::no("note", &id.to_string()),
                "{name} {id}"
            );
        }
    }
    ws.read_with(cx, |ws, _| {
        assert!(ws.notes.get(closed1).is_some() && ws.notes.get(open1).is_some());
    });
    assert_eq!(
        refusal(
            cx,
            &two,
            "edit_note",
            json!({"block": uid1, "note": open1, "name": "x"})
        ),
        templum_api::registry::no("block", &uid1)
    );
    // w1's own: the closed one reopens; the unmarked one isn't reachable; w1's can't reach w2's.
    let (failed, text) = call(
        cx,
        &one,
        "open_note",
        json!({"note": closed1, "block": uid1}),
    );
    assert!(!failed, "{text}");
    let words = refusal(
        cx,
        &one,
        "open_note",
        json!({"note": unmarked, "block": uid1}),
    );
    assert_eq!(
        words,
        templum_api::registry::no("note", &unmarked.to_string())
    );
    let words = refusal(cx, &one, "open_note", json!({"note": open2, "block": uid1}));
    assert_eq!(words, templum_api::registry::no("note", &open2.to_string()));
    let _ = uid2;
    // An unlimited token reaches the unmarked one.
    let all = with_token(&ws, cx, Scope::Full, None);
    let (failed, _) = call(
        cx,
        &all,
        "open_note",
        json!({"note": unmarked, "block": uid1}),
    );
    assert!(!failed);
}

#[gpui_kit::test]
fn a_deleted_workspace_s_closed_notes_are_no_new_workspace_s(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (second, _) = another_workspace(handle, &ws, cx);
    let note = ws.update(cx, |ws, _| {
        let id = ws.notes.add("was in w2".into(), None, 1);
        ws.notes.close_in(id, &second);
        id
    });
    let all = with_token(&ws, cx, Scope::Read, None);
    let seen = state(handle, &ws, cx, &all);
    assert_eq!(
        seen.notes
            .iter()
            .find(|n| n.id == note)
            .unwrap()
            .workspace
            .as_deref(),
        Some(second.as_str())
    );
    // The workspace goes; a new one gets its id again.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.delete_space_as(&second, false, window, cx))
    })
    .unwrap();
    super::templum_agent::control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({"name": "Again"}),
    );
    let ids: Vec<String> = ws.read_with(cx, |ws, _| {
        ws.order().iter().map(|s| s.id.clone()).collect()
    });
    assert!(ids.contains(&second), "{ids:?}");
    let limited = with_token(&ws, cx, Scope::Read, Some(vec![second.as_str()]));
    let seen = state(handle, &ws, cx, &all);
    assert_eq!(
        seen.notes
            .iter()
            .find(|n| n.id == note)
            .map(|n| n.workspace.clone()),
        None,
        "the note isn't listed under the new w2"
    );
    assert!(state(handle, &ws, cx, &limited).notes.is_empty());
    let _ = first;
}

#[gpui_kit::test]
fn a_note_opened_again_is_closed_in_no_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (second, _) = another_workspace(handle, &ws, cx);
    // Closed in the first workspace, then opened in the second, which is then deleted with the
    // tab open: the note is open nowhere, and no share of the first one may see it.
    let note = ws.update(cx, |ws, _| {
        let id = ws.notes.add("private".into(), None, 1);
        ws.notes.close_in(id, &first);
        id
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.switch_to(1, window, cx);
            ws.open_note(note, None, window, cx);
        })
    })
    .unwrap();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.notes.get(note).unwrap().closed_in.clone()),
        None
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.switch_to(0, window, cx);
            ws.delete_space_as(&second, false, window, cx)
        })
    })
    .unwrap();
    let limited = with_token(&ws, cx, Scope::Read, Some(vec![first.as_str()]));
    assert!(
        state(handle, &ws, cx, &limited)
            .notes
            .iter()
            .all(|n| n.id != note),
        "the first workspace's share doesn't list it"
    );
    let control = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str()]));
    let line = tool(
        handle,
        &ws,
        cx,
        &control,
        "open_note",
        json!({"note": note, "workspace": first}),
    );
    assert_eq!(
        line["error"]["message"].as_str(),
        Some(templum_api::registry::no("note", &note.to_string()).as_str())
    );
}
