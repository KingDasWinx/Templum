//! The content tools: a wired kanban's cards, a wired note, a file opened in an editor, a wired
//! Git block's status (docs/specs/connections.md, batch 4).

use gpui_kit::Focusable as _;
use gpui_kit::component::input::EditorState;
use serde_json::json;
use templum_canvas::Wire;

use super::bus::{call, me, screen_with};
use super::*;

/// The startup terminal wired to a kanban block on a new board with one card ("New card") in To do:
/// (terminal's TEMPLUM_TERMINAL, its uid, the kanban block, the board, the card).
fn wired_board(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (String, String, BlockId, u64, u64) {
    let terminal = first_id(ws, cx);
    let (kanban, board, card) = board_with_a_card(handle, ws, cx);
    ws.update(cx, |ws, cx| assert!(ws.connect(terminal, kanban, cx)));
    let (me, uid) = me(ws, cx, terminal);
    (me, uid, kanban, board, card)
}

#[gpui_kit::test]
fn list_cards_shows_the_wired_board_and_a_card_whole(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, uid, _, board, card) = wired_board(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            b.edit_card(board, card, |c| {
                c.title = "Fix\nlogin".into();
                c.description = "It 500s.\nOn Safari.".into();
                c.assignee = Some(crate::board::Assignee::Block(uid.clone()));
            });
            b.add_comment(board, card, "you", "Please", 0)
        })
    });
    let (failed, text) = call(handle, &ws, cx, &me, "list_cards", json!({}));
    assert!(!failed, "{text}");
    assert!(text.starts_with("Board \"Board 1\" ("), "{text}");
    assert!(
        text.contains(&format!("\n  {card}  Fix login  assignee {uid}")),
        "one line per card: {text}"
    );
    assert!(text.contains("1 comment"), "{text}");
    assert!(
        text.contains("Doing (") && text.contains("no cards"),
        "{text}"
    );
    let (failed, whole) = call(handle, &ws, cx, &me, "list_cards", json!({ "card": card }));
    assert!(!failed, "{whole}");
    assert!(
        whole.contains("description:\n  It 500s.\n  On Safari."),
        "{whole}"
    );
    assert!(whole.contains("you") && whole.contains("Please"), "{whole}");
}

#[gpui_kit::test]
fn list_cards_lists_a_cards_attachments_with_their_paths(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let (me, _, _, board, card) = wired_board(handle, &ws, cx);
    let src = tempfile::tempdir().unwrap();
    let from = src.path().join("shot.png");
    std::fs::write(&from, vec![0u8; 48213]).unwrap();
    let shot = crate::attachments::copy_in(data.path(), card, &from).unwrap();
    let log = crate::attachments::copy_in(data.path(), card, &src.path().join("shot.png")).unwrap();
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.edit_card(board, card, |c| c.attachments = vec![shot.clone()]))
    });
    let (_, text) = call(handle, &ws, cx, &me, "list_cards", json!({}));
    assert!(text.contains("  1 attachment"), "{text}");
    assert!(!text.contains("1 attachments"), "{text}");
    let (failed, whole) = call(handle, &ws, cx, &me, "list_cards", json!({ "card": card }));
    assert!(!failed, "{whole}");
    let path = crate::attachments::dir(data.path(), card).join("shot.png");
    assert!(path.exists());
    assert!(
        whole.contains(&format!(
            "\nattachments:\n  shot.png  47 KB  {}",
            path.display()
        )),
        "{whole}"
    );
    // Two: the plural; the second under the name it was given.
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.edit_card(board, card, |c| c.attachments.push(log.clone())))
    });
    let (_, text) = call(handle, &ws, cx, &me, "list_cards", json!({}));
    assert!(text.contains("  2 attachments"), "{text}");
    let (_, whole) = call(handle, &ws, cx, &me, "list_cards", json!({ "card": card }));
    assert!(whole.contains("  shot (2).png  47 KB  "), "{whole}");
}

#[gpui_kit::test]
fn a_card_without_a_data_folder_lists_no_attachment_paths(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _, _, board, card) = wired_board(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            b.edit_card(board, card, |c| {
                c.attachments = vec![crate::board::Attachment {
                    name: "shot.png".into(),
                    size: 3,
                }]
            })
        })
    });
    let (_, whole) = call(handle, &ws, cx, &me, "list_cards", json!({ "card": card }));
    assert!(whole.contains("1 attachment"), "{whole}");
    assert!(!whole.contains("attachments:"), "{whole}");
}

/// An agent that moves a card to the board's last column, or to a Review one, is asked to say on
/// it what it did, unless the card's newest comment is its own.
#[gpui_kit::test]
fn finishing_a_card_asks_for_a_comment_on_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _, _, board, card) = wired_board(handle, &ws, cx);
    let review = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            // To do, Doing, Review, Done.
            let done = b.board(board)?.columns.last()?.id;
            let id = b.add_column(board)?;
            b.rename_column(board, id, "Review");
            b.move_column(board, id, done);
            Some(id)
        })
    });
    assert!(review.is_some());
    let to = |cx: &mut TestAppContext, column: &str| {
        let (failed, text) = call(
            handle,
            &ws,
            cx,
            &me,
            "move_card",
            json!({ "card": card, "column": column }),
        );
        assert!(!failed, "{text}");
        text
    };
    assert_eq!(to(cx, "Doing"), format!("Moved card {card} to Doing."));
    let ask = "Say on it what you did and what the next one needs to know: comment_card.";
    assert_eq!(
        to(cx, "review"),
        format!("Moved card {card} to review. {ask}")
    );
    assert_eq!(to(cx, "Done"), format!("Moved card {card} to Done. {ask}"));
    let comment = json!({ "card": card, "text": "Fixed the parser; its docs are left." });
    assert!(!call(handle, &ws, cx, &me, "comment_card", comment).0);
    assert_eq!(to(cx, "Done"), format!("Moved card {card} to Done."));
}

#[gpui_kit::test]
fn create_move_comment_and_claim_change_the_board(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, uid, _, board, card) = wired_board(handle, &ws, cx);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "create_card",
        json!({ "column": "doing", "title": "Write docs", "description": "README" }),
    );
    assert!(!failed, "{text}");
    // "Created card 7."
    let new: u64 = text
        .split_whitespace()
        .find_map(|w| w.trim_end_matches('.').parse().ok())
        .expect("its id");
    let (doing, done) = ws.read_with(cx, |ws, _| {
        let b = ws.boards.board(board).unwrap();
        (b.columns[1].id, b.columns[2].id)
    });
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.boards.find_card(new)),
        Some((board, doing))
    );
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "move_card",
        json!({ "card": card, "column": "DONE" }),
    );
    assert!(!failed, "{text}");
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.boards.find_card(card)),
        Some((board, done))
    );
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "move_card",
        json!({ "card": card, "column": "Review" }),
    );
    assert!(
        failed && text.contains("To do, Doing, Done"),
        "the columns listed: {text}"
    );
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &me,
            "comment_card",
            json!({ "card": card, "text": "Done." })
        )
        .0
    );
    assert!(!call(handle, &ws, cx, &me, "claim_card", json!({ "card": card })).0);
    ws.read_with(cx, |ws, _| {
        let c = ws.boards.card(board, card).unwrap();
        assert_eq!(c.assignee, Some(crate::board::Assignee::Block(uid.clone())));
        assert_eq!(
            (c.comments[0].by.as_str(), c.comments[0].text.as_str()),
            (uid.as_str(), "Done.")
        );
    });
    assert!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "comment_card",
            json!({ "card": card, "text": "  " })
        )
        .0,
        "blank"
    );
    assert!(
        call(handle, &ws, cx, &me, "create_card", json!({ "title": " " })).0,
        "blank"
    );
}

/// An agent fixes or removes only the comments it wrote, by the number `list_cards` and
/// `comment_card` give; each change is a step of the kanban's Ctrl+Z, and the open card shows it.
#[gpui_kit::test]
fn an_agent_edits_and_deletes_only_its_own_comments(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (me, uid, _, board, card) = wired_board(handle, &ws, cx);
    let (failed, said) = call(
        handle,
        &ws,
        cx,
        &me,
        "comment_card",
        json!({ "card": card, "text": "Fixed the parser." }),
    );
    assert!(!failed, "{said}");
    let mine: u64 = said
        .rsplit(' ')
        .next()
        .and_then(|w| w.trim_end_matches(").").parse().ok())
        .expect("comment_card says the comment's number");
    let (person, agent) = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            (
                b.add_comment(board, card, "you", "Please check", 1)
                    .unwrap(),
                b.add_comment(board, card, "zzzzz", "Seen", 2).unwrap(),
            )
        })
    });
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::kanban::open_card(ws, board, card, window, cx)
        });
    })
    .unwrap();
    let shown = |cx: &mut TestAppContext, comment: u64| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let id = gpui_kit::ElementId::Name(format!("comment-{comment}").into());
            window.try_find(id).is_some()
        })
        .unwrap()
    };
    let thread = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| {
            let c = ws.boards.card(board, card).unwrap();
            c.comments
                .iter()
                .map(|c| (c.id, c.by.clone(), c.text.clone(), c.edited))
                .collect::<Vec<_>>()
        })
    };
    assert!([mine, person, agent].into_iter().all(|c| shown(cx, c)));
    let (_, other, theirs) = board_with_a_card(handle, &ws, cx);
    let elsewhere = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_comment(other, theirs, &uid, "mine, but unwired", 0))
    });
    let before = thread(cx);
    let steps = ws.read_with(cx, |ws, _| ws.boards_undo.len());

    // The person's, another agent's, a missing one, a blank text and an unwired card: refused,
    // and nothing changes.
    let args = |comment: u64| json!({ "card": card, "comment": comment, "text": "Changed" });
    let (failed, text) = call(handle, &ws, cx, &me, "edit_comment", args(person));
    assert!(
        failed && text.contains("the person") && text.contains("your own"),
        "{text}"
    );
    let (failed, text) = call(handle, &ws, cx, &me, "edit_comment", args(agent));
    assert!(failed && text.contains("zzzzz"), "{text}");
    for comment in [person, agent] {
        let (failed, text) = call(
            handle,
            &ws,
            cx,
            &me,
            "delete_comment",
            json!({ "card": card, "comment": comment }),
        );
        assert!(failed && text.contains("not you"), "{text}");
    }
    let (failed, text) = call(handle, &ws, cx, &me, "edit_comment", args(99_999));
    assert!(failed && text.contains("no comment 99999"), "{text}");
    let blank = json!({ "card": card, "comment": mine, "text": "  " });
    let (failed, text) = call(handle, &ws, cx, &me, "edit_comment", blank);
    assert!(failed && text.contains("delete_comment"), "{text}");
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "delete_comment",
        json!({ "card": theirs, "comment": elsewhere.unwrap() }),
    );
    assert!(failed && text.contains("No kanban connected"), "{text}");
    assert_eq!(thread(cx), before);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.boards_undo.len()),
        steps,
        "a refusal is no step of Ctrl+Z"
    );

    // Its own: edited in place, by the same author and at the same time, and marked.
    let (failed, text) = call(handle, &ws, cx, &me, "edit_comment", args(mine));
    assert!(!failed, "{text}");
    let after = thread(cx);
    assert_eq!(after[0], (mine, uid.clone(), "Changed".to_string(), true));
    assert_eq!(after[1..], before[1..]);
    let (_, whole) = call(handle, &ws, cx, &me, "list_cards", json!({ "card": card }));
    assert!(
        whole.contains(&format!("comment {mine} by {uid}")) && whole.contains(", edited:"),
        "its number and that it was edited: {whole}"
    );
    assert!(shown(cx, mine), "the open card still shows it");

    // The same text again changes nothing, and says so: no edit, no step of Ctrl+Z.
    let steps = ws.read_with(cx, |ws, _| ws.boards_undo.len());
    let (failed, text) = call(handle, &ws, cx, &me, "edit_comment", args(mine));
    assert!(
        !failed && text == format!("Comment {mine} already says that."),
        "{text}"
    );
    assert_eq!(thread(cx), after);
    assert_eq!(ws.read_with(cx, |ws, _| ws.boards_undo.len()), steps);

    // Deleting it takes it off the open card at once.
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "delete_comment",
        json!({ "card": card, "comment": mine }),
    );
    assert!(!failed, "{text}");
    assert_eq!(thread(cx).len(), 2);
    assert!(!shown(cx, mine) && shown(cx, person) && shown(cx, agent));

    // Each was a step of Ctrl+Z: the delete first, then the edit.
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    assert_eq!(
        thread(cx)[0],
        (mine, uid.clone(), "Changed".to_string(), true)
    );
    assert!(shown(cx, mine));
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    assert_eq!(thread(cx), before);
}

#[gpui_kit::test]
fn claim_card_refuses_a_card_another_block_holds(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _, _, board, card) = wired_board(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            b.edit_card(board, card, |c| {
                c.assignee = Some(crate::board::Assignee::Block("zzzzz".into()))
            })
        })
    });
    let (failed, text) = call(handle, &ws, cx, &me, "claim_card", json!({ "card": card }));
    assert!(
        failed && text.contains("zzzzz") && text.contains("free"),
        "{text}"
    );
    assert_eq!(
        ws.read_with(cx, |ws, _| ws
            .boards
            .card(board, card)
            .unwrap()
            .assignee
            .clone()),
        Some(crate::board::Assignee::Block("zzzzz".into()))
    );
}

#[gpui_kit::test]
fn a_card_on_a_board_not_connected_is_refused(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _, _, _, _) = wired_board(handle, &ws, cx);
    // A second kanban block, not wired, on a board of its own.
    let (_, other, theirs) = board_with_a_card(handle, &ws, cx);
    let before = ws.read_with(cx, |ws, _| ws.boards.clone());
    for (tool, args) in [
        ("move_card", json!({ "card": theirs, "column": "Doing" })),
        ("comment_card", json!({ "card": theirs, "text": "hi" })),
        ("claim_card", json!({ "card": theirs })),
        ("list_cards", json!({ "card": theirs })),
        ("list_cards", json!({ "board": other })),
    ] {
        let (failed, text) = call(handle, &ws, cx, &me, tool, args);
        assert!(failed, "{tool}: {text}");
    }
    let (_, unknown) = call(
        handle,
        &ws,
        cx,
        &me,
        "claim_card",
        json!({ "card": 99_999 }),
    );
    let (_, unwired) = call(
        handle,
        &ws,
        cx,
        &me,
        "claim_card",
        json!({ "card": theirs }),
    );
    assert_eq!(
        unknown.replace("99999", "N"),
        unwired.replace(&theirs.to_string(), "N"),
        "says no more"
    );
    assert_eq!(ws.read_with(cx, |ws, _| ws.boards.clone()), before);
}

#[gpui_kit::test]
fn a_board_left_out_with_two_wired_is_refused_and_both_named(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _, _, board, _) = wired_board(handle, &ws, cx);
    let (second, other, _) = board_with_a_card(handle, &ws, cx);
    let terminal = ws.read_with(cx, |ws, _| ws.space.terminals().next().unwrap().0);
    ws.update(cx, |ws, cx| assert!(ws.connect(terminal, second, cx)));
    let (failed, text) = call(handle, &ws, cx, &me, "list_cards", json!({}));
    assert!(
        failed && text.contains(&format!("({board})")) && text.contains(&format!("({other})")),
        "{text}"
    );
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &me,
            "list_cards",
            json!({ "board": other })
        )
        .0
    );
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &me,
            "list_cards",
            json!({ "board": "board 1" })
        )
        .0,
        "by name, either case"
    );
}

#[gpui_kit::test]
fn ctrl_z_in_the_kanban_takes_back_an_agents_move(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _, kanban, board, card) = wired_board(handle, &ws, cx);
    let todo = ws.read_with(cx, |ws, _| ws.boards.board(board).unwrap().columns[0].id);
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &me,
            "move_card",
            json!({ "card": card, "column": "Doing" })
        )
        .0
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(kanban, window, cx));
        window.press(UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.boards.find_card(card)),
        Some((board, todo))
    );
}

#[gpui_kit::test]
fn the_content_tools_are_refused_without_a_wire(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (kanban, note, terminal) = three_blocks(handle, &ws, cx);
    let (me, _) = me(&ws, cx, terminal);
    let uid = |id| ws.read_with(cx, |ws, _| ws.space.uids[&id].clone());
    let (kanban_uid, note_uid) = (uid(kanban), uid(note));
    for (tool, args) in [
        ("list_cards", json!({})),
        ("create_card", json!({ "title": "x" })),
        ("read_note", json!({ "block": note_uid })),
        ("append_note", json!({ "block": note_uid, "text": "x" })),
        ("read_note", json!({ "block": kanban_uid })),
    ] {
        let (failed, text) = call(handle, &ws, cx, &me, tool, args.clone());
        assert!(failed, "{tool} {args}: {text}");
    }
}

#[gpui_kit::test]
fn the_checklist_tools_are_refused_without_a_wire(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let (me, block, uid, _, build, _) = wired_checklist(handle, &ws, cx);
    ws.update(cx, |ws, cx| {
        assert!(ws.disconnect(Wire::new(terminal, block).unwrap(), cx))
    });
    for (tool, args) in [
        ("read_checklist", json!({ "block": uid })),
        ("check_item", json!({ "block": uid, "item": build })),
        ("add_items", json!({ "block": uid, "text": "x" })),
    ] {
        let (failed, text) = call(handle, &ws, cx, &me, tool, args.clone());
        assert!(
            failed && text.contains("isn't connected to you"),
            "{tool} {args}: {text}"
        );
    }
}

#[gpui_kit::test]
fn the_instructions_say_what_to_do_with_a_kanban_and_a_note(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let (me, _) = me(&ws, cx, terminal);
    let text = ws.read_with(cx, |ws, cx| ws.instructions(&me, cx));
    // Batch 8's sentence about tasks comes after it.
    assert!(
        text.contains(
            "When a connected kanban has your task, claim its card (claim_card: a card for another \
             agent isn't yours) and move it to Doing when you start; check its checklist's items as \
             you finish them (edit_card); when you are done, comment on it what you did and what the \
             next agent needs to know (comment_card), then move it to Review (Done when the board has \
             no Review); write what the next agent needs to know in the connected note too, and check \
             the items of a connected checklist as you finish them. For work of several steps, open \
             your own checklist with create_block and check its items as you go."
        ),
        "{text}"
    );
}

/// The startup terminal wired to the note "hello" of `three_blocks`: (TEMPLUM_TERMINAL, note
/// block, note's uid).
fn wired_note(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (String, BlockId, String) {
    let (_, note, terminal) = three_blocks(handle, ws, cx);
    ws.update(cx, |ws, cx| assert!(ws.connect(terminal, note, cx)));
    let (me, _) = me(ws, cx, terminal);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&note].clone());
    (me, note, uid)
}

fn note_editor(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    note: BlockId,
) -> Entity<EditorState> {
    ws.read_with(cx, |ws, _| {
        ws.space.blocks[&note]
            .note()
            .unwrap()
            .tab()
            .unwrap()
            .editor
            .clone()
    })
}

#[gpui_kit::test]
fn read_note_reads_what_the_note_shows(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _, uid) = wired_note(handle, &ws, cx);
    assert_eq!(
        call(handle, &ws, cx, &me, "read_note", json!({ "block": uid })),
        (false, "hello".to_string())
    );
    let terminal = ws.read_with(cx, |ws, _| ws.space.terminals().next().unwrap().0);
    let (_, mine) = super::bus::me(&ws, cx, terminal);
    let (failed, text) = call(handle, &ws, cx, &me, "read_note", json!({ "block": mine }));
    assert!(failed && text.contains("isn't a note"), "{text}");
}

#[gpui_kit::test]
fn append_note_shows_at_once_keeps_the_caret_and_one_ctrl_z_takes_it_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, note, uid) = wired_note(handle, &ws, cx);
    let editor = note_editor(&ws, cx, note);
    // The person is in the note, the caret after "he".
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(note, window, cx));
        editor.update(cx, |e, cx| e.set_selected_range(2..2, cx));
    })
    .unwrap();
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "append_note",
        json!({ "block": uid, "text": "Next: tests" }),
    );
    assert!(!failed, "{text}");
    cx.run_until_parked();
    let id = ws.read_with(cx, |ws, _| {
        ws.space.blocks[&note].note().unwrap().tab().unwrap().id
    });
    editor.read_with(cx, |e, _| {
        assert_eq!(e.value().as_ref(), "hello\n\nNext: tests");
        assert_eq!(e.selected_range(), 2..2, "the caret stays");
    });
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.notes.get(id).unwrap().text.clone()),
        "hello\n\nNext: tests"
    );
    cx.update_window(handle, |_, window, cx| {
        window.press(UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
    editor.read_with(cx, |e, _| assert_eq!(e.value().as_ref(), "hello"));
    // A note ending in a newline gets one more; an empty one none.
    editor.update(cx, |e, cx| e.set_selected_range(5..5, cx));
    cx.update_window(handle, |_, window, cx| {
        editor.update(cx, |e, cx| e.insert("\n", window, cx));
    })
    .unwrap();
    call(
        handle,
        &ws,
        cx,
        &me,
        "append_note",
        json!({ "block": uid, "text": "x" }),
    );
    editor.read_with(cx, |e, _| assert_eq!(e.value().as_ref(), "hello\n\nx"));
    assert!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "append_note",
            json!({ "block": uid, "text": " \n" })
        )
        .0,
        "blank"
    );
}

/// A folder with a.rs (five lines) and b.bin (binary), canonicalized.
fn a_project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.rs"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
    std::fs::write(dir.path().join("b.bin"), [0u8, 159, 146, 150, 0, 1]).unwrap();
    let path = std::fs::canonicalize(dir.path()).unwrap();
    (dir, path)
}

/// A terminal started in `folder`, focused.
fn terminal_in(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    folder: &std::path::Path,
) -> BlockId {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let rect = Rect::new(40.0, 400.0, 600.0, 300.0);
            let id = ws
                .spawn_block(rect, Some(folder.to_path_buf()), window, cx)
                .unwrap();
            ws.space.appearing.clear();
            ws.focus_block(id, window, cx);
            id
        })
    })
    .unwrap()
}

#[gpui_kit::test]
fn open_file_lands_at_the_line_in_the_wired_editor(cx: &mut TestAppContext) {
    let (_dir, folder) = a_project();
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let editor = open_editor(handle, &ws, cx, &folder);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            assert!(ws.connect(terminal, editor, cx));
            ws.focus_block(terminal, window, cx);
            ws.space.flight = None;
        })
    })
    .unwrap();
    let camera = ws.read_with(cx, |ws, _| ws.space.canvas.camera);
    let (me, _) = me(&ws, cx, terminal);
    let path = folder.join("a.rs");
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "open_file",
        json!({ "path": path, "line": 3 }),
    );
    assert!(!failed, "{text}");
    cx.update_window(handle, |_, window, cx| {
        assert!(
            ws.read(cx).focus_handle.is_focused(window),
            "the keys stay with the canvas"
        )
    })
    .unwrap();
    ws.read_with(cx, |ws, cx| {
        let e = ws.space.editor(editor).unwrap();
        let tab = e.active_tab().unwrap();
        assert_eq!(tab.path, path);
        assert_eq!(tab.editor.read(cx).cursor_position().line, 2, "line 3");
        assert_eq!(ws.space.focused, Some(terminal), "the keyboard stays");
        assert!(ws.space.flight.is_none(), "the camera stays");
        assert_eq!(ws.space.canvas.camera, camera);
    });
}

#[gpui_kit::test]
fn open_file_with_no_wired_editor_opens_one_beside_the_caller(cx: &mut TestAppContext) {
    let (_dir, folder) = a_project();
    let (handle, ws) = open(cx);
    // An editor holding the file, but not wired: not the one used.
    let unwired = open_editor(handle, &ws, cx, &folder);
    // The caller's folder can't hold the file's (the startup terminal's, home, holds the
    // temporary folder on Windows).
    let elsewhere = tempfile::tempdir().unwrap();
    let terminal = terminal_in(handle, &ws, cx, elsewhere.path());
    let (me, _) = me(&ws, cx, terminal);
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "open_file",
        json!({ "path": folder.join("a.rs"), "line": 2 }),
    );
    assert!(!failed && text.contains("new editor"), "{text}");
    cx.update_window(handle, |_, window, cx| {
        assert!(
            ws.read(cx).focus_handle.is_focused(window),
            "the keys stay with the canvas"
        )
    })
    .unwrap();
    let new = ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.blocks.len(), blocks + 1);
        let wire = ws
            .space
            .wires
            .iter()
            .rfind(|w| w.touches(terminal))
            .expect("wired to the caller");
        let new = if wire.a == terminal { wire.b } else { wire.a };
        assert_ne!(new, unwired);
        assert_eq!(
            ws.space.editor(new).unwrap().folder,
            folder,
            "the file's folder"
        );
        assert_eq!(ws.space.focused, Some(terminal), "the keyboard stays");
        new
    });
    // One Ctrl+Z closes it with its wire.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.undo_canvas(false, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.space.blocks.contains_key(&new));
        assert!(!ws.space.wires.iter().any(|w| w.touches(new)));
    });
}

/// A relative path is read from the caller's folder, `..` and all.
#[gpui_kit::test]
fn open_file_reads_a_relative_path_from_the_callers_folder(cx: &mut TestAppContext) {
    let (dir, folder) = a_project();
    std::fs::create_dir(folder.join("sub")).unwrap();
    let (handle, ws) = open(cx);
    // The plain path: cmd.exe can't start in a canonical \\?\ one.
    let terminal = terminal_in(handle, &ws, cx, dir.path());
    let (me, _) = me(&ws, cx, terminal);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "open_file",
        json!({ "path": "sub/../a.rs", "line": 2 }),
    );
    assert!(!failed, "{text}");
    ws.read_with(cx, |ws, cx| {
        let wire = ws
            .space
            .wires
            .iter()
            .rfind(|w| w.touches(terminal))
            .expect("a new editor, wired to the caller");
        let new = if wire.a == terminal { wire.b } else { wire.a };
        let e = ws.space.editor(new).unwrap();
        let tab = e.active_tab().unwrap();
        assert!(tab.path.starts_with(&e.folder), "{:?}", tab.path);
        assert_eq!(
            std::fs::canonicalize(&tab.path).unwrap(),
            folder.join("a.rs")
        );
        assert_eq!(tab.editor.read(cx).cursor_position().line, 1, "line 2");
    });
}

/// A file named by its real path shows in the tab an editor on a linked folder already has.
#[cfg(unix)]
#[gpui_kit::test]
fn open_file_shows_the_tab_already_open_under_a_linked_folder(cx: &mut TestAppContext) {
    let (_dir, folder) = a_project();
    let links = tempfile::tempdir().unwrap();
    let link = links.path().join("project");
    std::os::unix::fs::symlink(&folder, &link).unwrap();
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let editor = open_editor(handle, &ws, cx, &link);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            assert!(ws.connect(terminal, editor, cx));
            let e = ws.space.editor_mut(editor).unwrap();
            e.open_tab(&link.join("a.rs"), window, cx).unwrap();
        })
    })
    .unwrap();
    let (me, _) = me(&ws, cx, terminal);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "open_file",
        json!({ "path": folder.join("a.rs"), "line": 2 }),
    );
    assert!(!failed, "{text}");
    ws.read_with(cx, |ws, cx| {
        let e = ws.space.editor(editor).unwrap();
        assert_eq!(e.tabs.len(), 1, "the tab already open, no second one");
        assert_eq!(e.active, Some(0));
        assert_eq!(e.tabs[0].editor.read(cx).cursor_position().line, 1);
    });
    assert!(
        text.contains(&link.join("a.rs").display().to_string()),
        "as the editor names it: {text}"
    );
}

/// A linked file (CLAUDE.md -> AGENTS.md) shows the tab open under its own name.
#[cfg(unix)]
#[gpui_kit::test]
fn open_file_shows_the_tab_already_open_under_a_linked_files_name(cx: &mut TestAppContext) {
    let (dir, folder) = a_project();
    std::fs::write(folder.join("AGENTS.md"), "# Agents\n").unwrap();
    std::os::unix::fs::symlink("AGENTS.md", folder.join("CLAUDE.md")).unwrap();
    let (handle, ws) = open(cx);
    let terminal = terminal_in(handle, &ws, cx, dir.path());
    let editor = open_editor(handle, &ws, cx, &folder);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            assert!(ws.connect(terminal, editor, cx));
            let e = ws.space.editor_mut(editor).unwrap();
            e.open_tab(&folder.join("CLAUDE.md"), window, cx).unwrap();
        })
    })
    .unwrap();
    let (me, _) = me(&ws, cx, terminal);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "open_file",
        json!({ "path": "CLAUDE.md" }),
    );
    assert!(!failed, "{text}");
    ws.read_with(cx, |ws, _| {
        let e = ws.space.editor(editor).unwrap();
        let tabs: Vec<_> = e.tabs.iter().map(|t| t.path.clone()).collect();
        assert_eq!(tabs, [folder.join("CLAUDE.md")], "no second tab");
        assert_eq!(e.active, Some(0));
    });
}

#[gpui_kit::test]
fn open_file_refuses_what_it_cant_open_and_opens_no_block(cx: &mut TestAppContext) {
    let (_dir, folder) = a_project();
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let (me, _) = me(&ws, cx, terminal);
    let blocks = ws.read_with(cx, |ws, _| ws.space.blocks.len());
    for path in [folder.join("nope.rs"), folder.clone(), folder.join("b.bin")] {
        let (failed, text) = call(handle, &ws, cx, &me, "open_file", json!({ "path": path }));
        assert!(failed, "{}: {text}", path.display());
    }
    assert_eq!(ws.read_with(cx, |ws, _| ws.space.blocks.len()), blocks);
}

#[gpui_kit::test]
fn an_editor_with_the_keyboard_gives_it_to_the_tab_opened(cx: &mut TestAppContext) {
    let (_dir, folder) = a_project();
    std::fs::write(folder.join("c.rs"), "c\n").unwrap();
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let editor = open_editor(handle, &ws, cx, &folder);
    let (me, _) = me(&ws, cx, terminal);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            assert!(ws.connect(terminal, editor, cx));
            let e = ws.space.editor_mut(editor).unwrap();
            e.open_tab(&folder.join("c.rs"), window, cx).unwrap();
            assert!(ws.space.editor(editor).unwrap().focus(window, cx));
        })
    })
    .unwrap();
    call(
        handle,
        &ws,
        cx,
        &me,
        "open_file",
        json!({ "path": folder.join("a.rs") }),
    );
    cx.update_window(handle, |_, window, cx| {
        let ws = ws.read(cx);
        let tab = ws.space.editor(editor).unwrap().active_tab().unwrap();
        assert!(tab.path.ends_with("a.rs"));
        // The active tab's own editor has the keys (not the hidden c.rs tab's).
        assert!(
            tab.editor.read(cx).focus_handle(cx).is_focused(window),
            "the new tab has the keys"
        );
    })
    .unwrap();
}

/// An editor with the keyboard shows a file it has open rendered (a Markdown preview): no text
/// to type in, so the canvas takes the keys rather than leave them in the tab now hidden.
#[gpui_kit::test]
fn an_editor_with_the_keyboard_showing_a_preview_gives_it_to_the_canvas(cx: &mut TestAppContext) {
    let (_dir, folder) = a_project();
    std::fs::write(folder.join("c.md"), "# c\n").unwrap();
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let editor = open_editor(handle, &ws, cx, &folder);
    let (me, _) = me(&ws, cx, terminal);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            assert!(ws.connect(terminal, editor, cx));
            let e = ws.space.editor_mut(editor).unwrap();
            let md = e.open_tab(&folder.join("c.md"), window, cx).unwrap();
            e.tabs[md].preview = true;
            e.open_tab(&folder.join("a.rs"), window, cx).unwrap();
            assert!(ws.space.editor(editor).unwrap().focus(window, cx));
        })
    })
    .unwrap();
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "open_file",
        json!({ "path": folder.join("c.md") }),
    );
    assert!(!failed, "{text}");
    cx.update_window(handle, |_, window, cx| {
        let ws = ws.read(cx);
        let e = ws.space.editor(editor).unwrap();
        assert!(e.active_tab().unwrap().preview);
        assert!(!e.has_focus(window, cx), "no keys in the hidden a.rs tab");
        assert!(ws.focus_handle.is_focused(window), "the canvas has them");
    })
    .unwrap();
}

/// The startup terminal wired to a checklist block showing "Deploy": "Build" (checked) and "Run
/// the tests": (TEMPLUM_TERMINAL, block, its uid, the checklist, Build, Run the tests).
fn wired_checklist(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (String, BlockId, String, u64, u64, u64) {
    let terminal = first_id(ws, cx);
    let (block, list, build, tests) = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let list = ws.checklists.add(1);
                ws.checklists.rename(list, "Deploy", 1);
                let build = ws
                    .checklists
                    .add_item(list, None, "Build", true, 1)
                    .unwrap();
                let tests = ws
                    .checklists
                    .add_item(list, None, "Run the tests", false, 1);
                let item = crate::session::Item::Checklist(crate::session::ChecklistItem {
                    uid: String::new(),
                    rect: Rect::new(900.0, 40.0, 320.0, 360.0).into(),
                    checklist: Some(list),
                    checklists: Vec::new(),
                    active: 0,
                });
                ws.quiet = true;
                let block = ws.add_saved(&item, window, cx).unwrap();
                ws.quiet = false;
                ws.space.appearing.clear();
                assert!(ws.connect(terminal, block, cx));
                (block, list, build, tests.unwrap())
            })
        })
        .unwrap();
    let (me, _) = me(ws, cx, terminal);
    let uid = ws.read_with(cx, |ws, _| ws.space.uids[&block].clone());
    (me, block, uid, list, build, tests)
}

fn items_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, list: u64) -> Vec<(String, bool)> {
    ws.read_with(cx, |ws, _| {
        let list = ws.checklists.get(list).unwrap();
        list.items
            .iter()
            .map(|i| (i.text.clone(), i.done))
            .collect()
    })
}

#[gpui_kit::test]
fn read_checklist_reads_the_checklist_on_screen(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _, uid, _, build, tests) = wired_checklist(handle, &ws, cx);
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "read_checklist",
            json!({ "block": uid })
        ),
        (
            false,
            format!("Deploy (1/2)\n{build} [x] Build\n{tests} [ ] Run the tests")
        )
    );
    let (_, blocks) = call(handle, &ws, cx, &me, "list_blocks", json!({}));
    assert!(blocks.contains("checklist"), "{blocks}");
    let terminal = ws.read_with(cx, |ws, _| ws.space.terminals().next().unwrap().0);
    let (_, mine) = super::bus::me(&ws, cx, terminal);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "read_checklist",
        json!({ "block": mine }),
    );
    assert!(failed && text.contains("isn't a checklist"), "{text}");
}

#[gpui_kit::test]
fn check_item_and_add_items_show_at_once_and_ctrl_z_takes_them_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, block, uid, list, _, tests) = wired_checklist(handle, &ws, cx);
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "check_item",
            json!({ "block": uid, "item": tests })
        ),
        (false, format!("Item {tests} is checked."))
    );
    assert!(items_of(&ws, cx, list)[1].1);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "check_item",
        json!({ "block": uid, "item": 999 }),
    );
    assert!(failed, "{text}");
    assert_eq!(
        text,
        "There's no item 999 in \"Deploy\". read_checklist shows its items."
    );
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "add_items",
        json!({ "block": uid, "text": "- [ ] Ship\n[x] Tag" }),
    );
    assert!(!failed, "{text}");
    let ids: Vec<u64> = ws.read_with(cx, |ws, _| {
        ws.checklists.get(list).unwrap().items[2..]
            .iter()
            .map(|i| i.id)
            .collect()
    });
    assert_eq!(text, format!("Added items {}, {}.", ids[0], ids[1]));
    assert_eq!(
        items_of(&ws, cx, list)[2..],
        [("Ship".to_string(), false), ("Tag".to_string(), true)]
    );
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "add_items",
        json!({ "block": uid, "text": " \n- " }),
    );
    assert!(failed && text == "There's nothing to add.", "{text}");
    // Each call is one step.
    let undo = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.focus_block(block, window, cx));
            window.press(super::UNDO, cx);
            window.render_frame(cx);
        })
        .unwrap();
    };
    undo(cx);
    assert_eq!(items_of(&ws, cx, list).len(), 2, "Ship and Tag gone");
    assert!(items_of(&ws, cx, list)[1].1);
    undo(cx);
    assert!(!items_of(&ws, cx, list)[1].1, "unchecked");
}

/// Review focus: an agent's change while the person edits an item keeps the field (its text and
/// caret), shows the new items under it, and the person's typing after it is its own step.
#[gpui_kit::test]
fn add_items_while_the_person_edits_keeps_their_field(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, block, uid, list, build, _) = wired_checklist(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.edit_field(block, crate::checklist::Field::Item(build), window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    let input = ws.read_with(cx, |ws, _| {
        let c = ws.space.blocks[&block].checklist().unwrap();
        c.editing.as_ref().unwrap().input.clone()
    });
    cx.simulate_input(handle, "!");
    input.update(cx, |i, cx| i.set_selected_range(2..2, cx));
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "add_items",
        json!({ "block": uid, "text": "Ship" }),
    );
    assert!(!failed, "{text}");
    input.read_with(cx, |i, _| {
        assert_eq!(i.value().as_ref(), "Build!");
        assert_eq!(i.cursor(), 2);
    });
    let texts = |cx: &mut TestAppContext| {
        items_of(&ws, cx, list)
            .into_iter()
            .map(|(t, _)| t)
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(cx), ["Build!", "Run the tests", "Ship"]);
    // Typed after the agent's change: a step of its own, taken back before the agent's.
    input.update(cx, |i, cx| {
        let end = i.value().len();
        i.set_selected_range(end..end, cx)
    });
    cx.simulate_input(handle, "?");
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        ws.update(cx, |ws, cx| ws.focus_block(block, window, cx));
        window.press(super::UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(texts(cx), ["Build!", "Run the tests", "Ship"]);
}

/// A new item left empty goes from the undo steps an agent's change took while it was edited:
/// Ctrl+Z never brings back an empty row nobody edits.
#[gpui_kit::test]
fn undoing_an_agents_add_during_a_new_items_edit_brings_back_no_empty_item(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let (me, block, uid, list, _, _) = wired_checklist(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_item_after(block, list, None, window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "add_items",
        json!({ "block": uid, "text": "Ship" }),
    );
    assert!(!failed, "{text}");
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        ws.update(cx, |ws, cx| ws.focus_block(block, window, cx));
        window.press(super::UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
    let texts: Vec<_> = items_of(&ws, cx, list)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert_eq!(texts, ["Build", "Run the tests"]);
    assert!(
        !ws.update(cx, |ws, cx| ws.undo_checklists(cx)),
        "the new item's own step changes nothing now: skipped"
    );
}

/// A new item typed in after an agent's change while it was empty: Ctrl+Z takes back the typing
/// and the agent's change without an empty row nobody edits.
#[gpui_kit::test]
fn undoing_typing_after_an_agents_add_brings_back_no_empty_item(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, block, uid, list, _, _) = wired_checklist(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_item_after(block, list, None, window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "add_items",
        json!({ "block": uid, "text": "Ship" }),
    );
    assert!(!failed, "{text}");
    cx.simulate_input(handle, "abc");
    let texts = |cx: &mut TestAppContext| {
        items_of(&ws, cx, list)
            .into_iter()
            .map(|(t, _)| t)
            .collect::<Vec<_>>()
    };
    assert_eq!(texts(cx), ["Build", "Run the tests", "abc", "Ship"]);
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
        ws.update(cx, |ws, cx| ws.focus_block(block, window, cx));
        window.press(super::UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(texts(cx), ["Build", "Run the tests", "Ship"]);
    cx.update_window(handle, |_, window, cx| {
        window.press(super::UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert_eq!(texts(cx), ["Build", "Run the tests"]);
}

#[gpui_kit::test]
fn create_and_edit_card_set_priority_who_and_checklist(cx: &mut TestAppContext) {
    use crate::board::{Assignee, Priority};
    let (handle, ws) = open(cx);
    let (me, uid, _, board, _) = wired_board(handle, &ws, cx);
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "create_card",
        json!({ "title": "v2", "priority": "High", "for": "codex", "checklist": "- Model\n- [x] Tools" }),
    );
    assert!(!failed, "{text}");
    let card: u64 = text
        .trim_start_matches("Created card ")
        .trim_end_matches('.')
        .parse()
        .unwrap();
    let (model, tools) = ws.read_with(cx, |ws, _| {
        let c = ws.boards.card(board, card).unwrap();
        assert_eq!(
            (c.priority, c.assignee.clone()),
            (Some(Priority::High), Some(Assignee::Agent("codex".into())))
        );
        (c.checklist[0].id, c.checklist[1].id)
    });
    let (_, list) = call(handle, &ws, cx, &me, "list_cards", json!({}));
    assert!(
        list.contains(&format!(
            "{card}  v2  priority high  for any codex  checklist 1/2"
        )),
        "{list}"
    );
    let (_, whole) = call(handle, &ws, cx, &me, "list_cards", json!({ "card": card }));
    assert!(
        whole.contains(&format!(
            "checklist:\n  {model} [ ] Model\n  {tools} [x] Tools"
        )),
        "{whole}"
    );
    let before = ws.read_with(cx, |ws, _| ws.boards.clone());
    for (args, says) in [
        (
            json!({ "card": card, "for": "nobody" }),
            "There's no agent or terminal block nobody".to_string(),
        ),
        (
            json!({ "card": card, "check": 99_999 }),
            format!("Card {card} has no item 99999"),
        ),
    ] {
        let (failed, text) = call(handle, &ws, cx, &me, "edit_card", args);
        assert!(failed && text.contains(&says), "{text}");
    }
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.boards.clone()),
        before,
        "a refusal changes nothing"
    );
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "edit_card",
        json!({ "card": card, "priority": "none", "for": "claude code", "add_items": "Docs",
                "check": model, "uncheck": [tools] }),
    );
    assert!(!failed, "{text}");
    let docs = ws.read_with(cx, |ws, _| {
        ws.boards.card(board, card).unwrap().checklist[2].id
    });
    assert_eq!(
        text,
        format!(
            "Card {card}: priority none; for any claude; added item {docs}; checked {model}; unchecked {tools}."
        )
    );
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    // Ctrl+Z keeps next_id: ids are never given out twice.
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.boards.boards.clone()),
        before.boards,
        "one Ctrl+Z step"
    );
    let (failed, _) = call(
        handle,
        &ws,
        cx,
        &me,
        "edit_card",
        json!({ "card": card, "for": "Anyone" }),
    );
    assert!(!failed);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws
            .boards
            .card(board, card)
            .unwrap()
            .assignee
            .clone()),
        None
    );
    // A block's id names that block (creating is never gated: Task 7 gates edit_card's `for`).
    let (_, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "create_card",
        json!({ "title": "mine", "for": uid }),
    );
    let mine: u64 = text
        .trim_start_matches("Created card ")
        .trim_end_matches('.')
        .parse()
        .unwrap();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws
            .boards
            .card(board, mine)
            .unwrap()
            .assignee
            .clone()),
        Some(Assignee::Block(uid.clone()))
    );
}

#[gpui_kit::test]
fn claim_card_follows_who_a_card_is_for(cx: &mut TestAppContext) {
    use crate::board::Assignee;
    let (handle, ws) = open(cx);
    let terminal = first_id(&ws, cx);
    let (me, uid, _, board, card) = wired_board(handle, &ws, cx);
    let set = |cx: &mut TestAppContext, a: Option<Assignee>| {
        ws.update(cx, |ws, _| {
            ws.edit_boards(|b| b.edit_card(board, card, |c| c.assignee = a))
        })
    };
    let runs = |cx: &mut TestAppContext, agent: Option<&str>| {
        ws.update(cx, |ws, _| {
            ws.space.terminal_mut(terminal).unwrap().seen.agent = agent.map(str::to_string)
        })
    };
    let claim =
        |cx: &mut TestAppContext| call(handle, &ws, cx, &me, "claim_card", json!({ "card": card }));
    set(cx, Some(Assignee::Agent("codex".into())));
    runs(cx, None);
    let (failed, text) = claim(cx);
    assert!(
        failed
            && text
                == format!(
                    "Card {card} is for any Codex, and Templum can't tell yet which agent you are: \
                     try again in a few seconds."
                ),
        "{text}"
    );
    runs(cx, Some("claude"));
    let (failed, text) = claim(cx);
    assert!(
        failed
            && text
                == format!(
                    "Card {card} is for any Codex, and you are Claude Code: take a card for you or \
                     for anyone."
                ),
        "{text}"
    );
    runs(cx, Some("codex"));
    let (_, list) = call(handle, &ws, cx, &me, "list_cards", json!({}));
    assert!(list.contains("for any codex (you)"), "{list}");
    assert_eq!(claim(cx), (false, format!("Card {card} is yours.")));
    assert_eq!(
        ws.read_with(cx, |ws, _| ws
            .boards
            .card(board, card)
            .unwrap()
            .assignee
            .clone()),
        Some(Assignee::Block(uid.clone()))
    );
    let (_, list) = call(handle, &ws, cx, &me, "list_cards", json!({}));
    assert!(
        list.contains(&format!("assignee {uid} ")) && list.contains("(you)"),
        "{list}"
    );
    // move_card stays open, and says whose card it is.
    set(cx, Some(Assignee::Agent("claude".into())));
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "move_card",
        json!({ "card": card, "column": "Doing" }),
    );
    assert!(
        !failed && text.ends_with(" It is for any claude, not you."),
        "{text}"
    );
    // edit_card's `for` follows claim_card: no card for another agent given to itself...
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "edit_card",
        json!({ "card": card, "for": uid }),
    );
    assert!(
        failed && text.contains("is for any Claude Code, and you are Codex"),
        "{text}"
    );
    // ...and no card another block holds freed or handed on; its checklist stays open.
    set(cx, Some(Assignee::Block("zzzzz".into())));
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "edit_card",
        json!({ "card": card, "for": "anyone" }),
    );
    assert!(failed && text.contains("taken by zzzzz"), "{text}");
    let (_, text) = call(
        handle,
        &ws,
        cx,
        &me,
        "move_card",
        json!({ "card": card, "column": "To do" }),
    );
    assert!(text.ends_with(" It is for zzzzz, not you."), "{text}");
    assert!(
        !call(
            handle,
            &ws,
            cx,
            &me,
            "edit_card",
            json!({ "card": card, "add_items": "x" })
        )
        .0
    );
    // A person's agent that runs a built-in's program is that agent to a card, by a path too.
    ws.update(cx, |ws, _| {
        let mut work = ws.agents.iter().find(|a| a.id == "codex").unwrap().clone();
        (work.id, work.title, work.builtin) = ("codex-work".into(), "Codex work".into(), None);
        work.command = "~/bin/Codex.exe --profile work".into();
        ws.agents.push(work);
    });
    set(cx, Some(Assignee::Agent("codex-work".into())));
    assert_eq!(claim(cx), (false, format!("Card {card} is yours.")));
}

/// A card for an account is that account's only; one for its agent is any of the agent's blocks,
/// the account's included.
#[gpui_kit::test]
fn a_card_for_an_account_is_its_own(cx: &mut TestAppContext) {
    use crate::board::Assignee;
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    super::agents::with_agents(&ws, cx, super::agents::CLAUDE2);
    let terminal = first_id(&ws, cx);
    let (me, _, _, board, card) = wired_board(handle, &ws, cx);
    let set = |cx: &mut TestAppContext, a: Assignee| {
        ws.update(cx, |ws, _| {
            ws.edit_boards(|b| b.edit_card(board, card, |c| c.assignee = Some(a)))
        })
    };
    let runs = |cx: &mut TestAppContext, agent: &str| {
        ws.update(cx, |ws, _| {
            ws.space.terminal_mut(terminal).unwrap().seen.agent = Some(agent.to_string())
        })
    };
    let claim =
        |cx: &mut TestAppContext| call(handle, &ws, cx, &me, "claim_card", json!({ "card": card }));
    set(cx, Assignee::Agent("claude2".into()));
    runs(cx, "claude");
    let (failed, text) = claim(cx);
    assert!(
        failed,
        "Claude Code's own folder isn't Claude Code 2: {text}"
    );
    runs(cx, "claude2");
    assert_eq!(claim(cx), (false, format!("Card {card} is yours.")));
    set(cx, Assignee::Agent("claude".into()));
    assert_eq!(
        claim(cx),
        (false, format!("Card {card} is yours.")),
        "any Claude Code: its accounts run it"
    );
}

/// The block at the other end of `mine`'s newest wire.
fn newest_block(ws: &Entity<Workspace>, cx: &mut TestAppContext, mine: BlockId) -> BlockId {
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

/// `create_block` opens a note, a checklist or a terminal running a command beside the caller,
/// wired to it, the keyboard and the camera kept; one Ctrl+Z closes it (docs/specs/agent-tools.md,
/// create_block).
#[gpui_kit::test]
fn create_block_opens_each_kind_beside_the_caller_wired_to_it(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let mine = first_id(&ws, cx);
    let (terminal, uid) = me(&ws, cx, mine);
    let kept = ws.read_with(cx, |ws, _| (ws.space.focused, ws.space.canvas.camera));
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "create_block",
        json!({ "kind": "checklist", "name": "Plan", "items": "- [ ] Read\n- [x] Plan" }),
    );
    assert!(
        !failed
            && text.starts_with("Opened a checklist as ")
            && text.ends_with(", connected to you."),
        "{text}"
    );
    let list = newest_block(&ws, cx, mine);
    ws.read_with(cx, |ws, _| {
        let shown = ws.space.blocks[&list].checklist().unwrap().list.unwrap();
        let l = ws.checklists.get(shown).unwrap();
        assert_eq!(l.name_shown(), "Plan");
        let items: Vec<(&str, bool)> = l.items.iter().map(|i| (i.text.as_str(), i.done)).collect();
        assert_eq!(items, [("Read", false), ("Plan", true)]);
        assert!(ws.space.wired(mine, list));
        assert_eq!(
            (ws.space.focused, ws.space.canvas.camera),
            kept,
            "keyboard and camera stay"
        );
        let (a, b) = (
            ws.space.canvas.get(mine).unwrap().rect,
            ws.space.canvas.get(list).unwrap().rect,
        );
        assert!(!a.intersects(&b), "beside, covering nothing");
    });
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "create_block",
        json!({ "kind": "note", "text": "notes" }),
    );
    assert!(!failed && text.starts_with("Opened a note as "), "{text}");
    let note = newest_block(&ws, cx, mine);
    ws.read_with(cx, |ws, cx| {
        let editor = ws.space.blocks[&note]
            .note()
            .unwrap()
            .tab()
            .unwrap()
            .editor
            .clone();
        assert_eq!(editor.read(cx).value(), "notes");
        assert_eq!(ws.space.focused, kept.0);
    });
    let dir = tempfile::tempdir().unwrap();
    let (failed, text) = call(
        handle,
        &ws,
        cx,
        &terminal,
        "create_block",
        json!({ "kind": "terminal", "folder": dir.path(), "command": "echo hello-from-create" }),
    );
    assert!(
        !failed && text.contains("running \"echo hello-from-create\", connected to you."),
        "{text}"
    );
    assert!(text.contains("stop_agent ends it."), "{text}");
    let term = newest_block(&ws, cx, mine);
    screen_with(&ws, cx, term, "hello-from-create");
    ws.read_with(cx, |ws, _| {
        let pane = ws.space.terminal(term).unwrap();
        assert_eq!(
            pane.opened_by.as_deref(),
            Some(uid.as_str()),
            "the caller may stop and close it"
        );
    });
    // One Ctrl+Z closes the newest block and its wire.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| assert!(ws.undo_canvas(false, window, cx)))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.canvas.get(term).is_none() && !ws.space.wired(mine, term));
        assert!(ws.space.canvas.get(note).is_some(), "one step each");
    });
}

#[gpui_kit::test]
fn create_block_refuses_what_its_kind_doesnt_take(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let mine = first_id(&ws, cx);
    let (terminal, _) = me(&ws, cx, mine);
    let blocks = ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len());
    let missing = std::env::temp_dir().join("templum-no-such-folder");
    for (args, says) in [
        (
            json!({ "kind": "terminal", "folder": missing }),
            format!("{} isn't a folder.", missing.display()),
        ),
        (
            json!({ "kind": "note", "items": "x" }),
            "\"name\" and \"items\" are for a checklist".to_string(),
        ),
    ] {
        assert_eq!(
            call(handle, &ws, cx, &terminal, "create_block", args),
            (true, says)
        );
    }
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len()),
        blocks,
        "nothing opened"
    );
}

/// The person puts `text` in the note block `note` (its editor `editor` has the keyboard after).
fn set_note(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    editor: &Entity<EditorState>,
    note: BlockId,
    cx: &mut TestAppContext,
    text: &str,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(note, window, cx));
        editor.update(cx, |e, cx| {
            let end = e.value().len();
            e.set_selected_range(0..end, cx);
            e.replace(text.to_string(), window, cx);
        });
    })
    .unwrap();
    cx.run_until_parked();
}

fn note_text(editor: &Entity<EditorState>, cx: &mut TestAppContext) -> String {
    editor.read_with(cx, |e, _| e.value().to_string())
}

/// Ctrl+Z in the note that has the keyboard.
fn undo_note(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        window.press(UNDO, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// `replace_note` replaces the whole text, or its one passage `old`, as one step of the note's
/// Ctrl+Z each (docs/specs/agent-tools.md, Fixing a note or a checklist).
#[gpui_kit::test]
fn replace_note_replaces_the_text_or_one_passage_and_ctrl_z_takes_it_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, note, uid) = wired_note(handle, &ws, cx);
    let editor = note_editor(&ws, cx, note);
    set_note(
        handle,
        &ws,
        &editor,
        note,
        cx,
        "Plan\n- build\n- build\n- ship",
    );
    let replace = |old: Option<&str>, text: &str| match old {
        Some(old) => json!({ "block": uid, "old": old, "text": text }),
        None => json!({ "block": uid, "text": text }),
    };
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "replace_note",
            replace(Some("- build"), "x")
        ),
        (
            true,
            "\"- build\" is in the note 2 times: give more of the text around it.".into()
        )
    );
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "replace_note",
            replace(Some("nope"), "x")
        ),
        (
            true,
            "\"nope\" isn't in the note: read_note shows it.".into()
        )
    );
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "replace_note",
            replace(Some("- ship"), "- ship it")
        ),
        (false, format!("Replaced 1 passage of the note in {uid}."))
    );
    assert_eq!(note_text(&editor, cx), "Plan\n- build\n- build\n- ship it");
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "replace_note",
            replace(None, "All new")
        ),
        (false, format!("Replaced the note in {uid}."))
    );
    assert_eq!(note_text(&editor, cx), "All new");
    assert_eq!(
        call(handle, &ws, cx, &me, "replace_note", replace(None, " ")),
        (
            true,
            "That would empty the note: the person does that.".into()
        )
    );
    // The whole text as the passage: the same.
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "replace_note",
            replace(Some("All new"), " ")
        ),
        (
            true,
            "That would empty the note: the person does that.".into()
        )
    );
    let id = ws.read_with(cx, |ws, _| {
        ws.space.blocks[&note].note().unwrap().tab().unwrap().id
    });
    cx.run_until_parked();
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.notes.get(id).unwrap().text.clone()),
        "All new",
        "kept in the notes"
    );
    undo_note(handle, cx);
    assert_eq!(note_text(&editor, cx), "Plan\n- build\n- build\n- ship it");
    undo_note(handle, cx);
    assert_eq!(note_text(&editor, cx), "Plan\n- build\n- build\n- ship");
}

/// `edit_item` and `delete_item` fix a checklist, one step of Ctrl+Z each.
#[gpui_kit::test]
fn edit_item_and_delete_item_fix_a_checklist_one_ctrl_z_step_each(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (me, _block, uid, list, build, tests) = wired_checklist(handle, &ws, cx);
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "edit_item",
            json!({ "block": uid, "item": build, "text": "Build it" })
        ),
        (false, format!("Item {build} is now \"Build it\"."))
    );
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "delete_item",
            json!({ "block": uid, "item": tests })
        ),
        (false, format!("Deleted item {tests}."))
    );
    assert_eq!(items_of(&ws, cx, list), [("Build it".to_string(), true)]);
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "edit_item",
            json!({ "block": uid, "item": 999, "text": "x" })
        ),
        (
            true,
            "There's no item 999 in \"Deploy\". read_checklist shows its items.".into()
        )
    );
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "delete_item",
            json!({ "block": uid, "item": 999 })
        ),
        (
            true,
            "There's no item 999 in \"Deploy\". read_checklist shows its items.".into()
        )
    );
    assert_eq!(
        call(
            handle,
            &ws,
            cx,
            &me,
            "edit_item",
            json!({ "block": uid, "item": build, "text": " " })
        ),
        (true, "delete_item removes an item".into())
    );
    ws.update(cx, |ws, cx| assert!(ws.undo_checklists(cx)));
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.checklists.get(list).unwrap().item(tests).is_some(),
            "the delete taken back"
        )
    });
    ws.update(cx, |ws, cx| assert!(ws.undo_checklists(cx)));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.checklists.get(list).unwrap().item(build).unwrap().text,
            "Build"
        )
    });
}
