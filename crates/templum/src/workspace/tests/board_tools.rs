//! The board tools of the Templum API and the control server: the grown card arguments and the
//! tools that edit boards, columns and cards (Templum to Templum, part 2d).

use serde_json::{Value, json};
use templum_api::store::Scope;

use super::api::{said, tool, with_token};
use super::templum_agent::{control, control_later};
use super::*;
use crate::board::Label;

/// An unlimited Control token's answer to `name`: whether it refused, and its words.
fn act(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    name: &str,
    arguments: Value,
) -> (bool, String) {
    let token = with_token(ws, cx, Scope::Control, None);
    said(&tool(handle, ws, cx, &token, name, arguments))
}

/// A card added to the first column of `board`, called `title`.
fn card_in(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    board: u64,
    column: usize,
    title: &str,
) -> u64 {
    ws.update(cx, |ws, _| {
        let column = ws.boards.board(board).unwrap().columns[column].id;
        let id = ws.edit_boards(|b| b.add_card(board, column)).unwrap();
        ws.edit_boards(|b| b.edit_card(board, id, |c| c.title = title.into()));
        id
    })
}

fn column_titles(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    board: u64,
    column: usize,
) -> Vec<String> {
    ws.read_with(cx, |ws, _| {
        ws.boards.board(board).unwrap().columns[column]
            .cards
            .iter()
            .map(|c| c.title.clone())
            .collect()
    })
}

#[gpui_kit::test]
fn create_card_takes_labels(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, board, _) = board_with_a_card(handle, &ws, cx);
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "create_card",
        json!({"board": board.to_string(), "title": "Login", "labels": ["red", "blue"]}),
    );
    assert!(!refused, "{said}");
    let id: u64 = said
        .strip_prefix("Created card ")
        .and_then(|s| s.strip_suffix('.'))
        .expect("the mirror reads the number from these words")
        .parse()
        .unwrap();
    let labels = ws.read_with(cx, |ws, _| {
        ws.boards.card(board, id).unwrap().labels.clone()
    });
    assert_eq!(labels, [Label::Red, Label::Blue]);
}

#[gpui_kit::test]
fn edit_card_takes_a_title_a_description_labels_and_items_to_remove(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let items = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            b.edit_card(board, card, |c| {
                c.description = "d".into();
                c.labels = vec![Label::Red];
            });
            b.add_items(board, card, &[("one".into(), false), ("two".into(), false)])
        })
    });
    let steps = ws.read_with(cx, |ws, _| ws.boards_undo.len());
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_card",
        json!({"card": card, "title": "New", "description": "",
            "labels": {"red": false, "green": true}, "remove_items": [items[0]]}),
    );
    assert!(!refused, "{said}");
    assert_eq!(
        said,
        format!(
            "Card {card}: titled \"New\"; description cleared; label red off; label green on; \
             removed item {}.",
            items[0]
        )
    );
    ws.read_with(cx, |ws, _| {
        let c = ws.boards.card(board, card).unwrap();
        assert_eq!((c.title.as_str(), c.description.as_str()), ("New", ""));
        assert_eq!(c.labels, [Label::Green]);
        assert_eq!(
            c.checklist.iter().map(|i| i.id).collect::<Vec<_>>(),
            [items[1]]
        );
        assert_eq!(ws.boards_undo.len(), steps + 1, "one more Ctrl+Z step");
    });
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_card",
        json!({"card": card, "remove_items": [99]}),
    );
    assert!(refused);
    assert_eq!(
        said,
        format!("Card {card} has no item 99: list_cards with card shows its items.")
    );
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_card",
        json!({"card": card, "title": "  "}),
    );
    assert_eq!((refused, said.as_str()), (true, "A card needs a title."));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.boards.card(board, card).unwrap().title, "New")
    });
}

#[gpui_kit::test]
fn move_card_puts_a_card_just_before_another_of_its_column(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, board, a) = board_with_a_card(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.edit_card(board, a, |c| c.title = "A".into()))
    });
    let b = card_in(&ws, cx, board, 0, "B");
    let c = card_in(&ws, cx, board, 1, "C");
    let d = card_in(&ws, cx, board, 1, "D");
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "move_card",
        json!({"card": a, "column": "Doing", "before": d}),
    );
    assert_eq!(
        (refused, said),
        (false, format!("Moved card {a} to Doing, before card {d}."))
    );
    assert_eq!(column_titles(&ws, cx, board, 1), ["C", "A", "D"]);
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "move_card",
        json!({"card": a, "column": "Doing", "before": b}),
    );
    assert_eq!((refused, said), (true, format!("Card {b} isn't in Doing.")));
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "move_card",
        json!({"card": a, "column": "Doing", "before": a}),
    );
    assert_eq!(
        (refused, said.as_str()),
        (true, "A card can't go before itself.")
    );
    assert_eq!(
        column_titles(&ws, cx, board, 1),
        ["C", "A", "D"],
        "nothing moved"
    );
    let (refused, _) = act(
        handle,
        &ws,
        cx,
        "move_card",
        json!({"card": c, "column": "Doing"}),
    );
    assert!(!refused);
    assert_eq!(
        column_titles(&ws, cx, board, 1),
        ["A", "D", "C"],
        "the end, as before"
    );
}

/// The uid of kanban block `block`.
fn uid_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, block: BlockId) -> String {
    ws.read_with(cx, |ws, _| ws.space.uids[&block].clone())
}

#[gpui_kit::test]
fn delete_card_deletes_it_and_ctrl_z_brings_it_back(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let (refused, said) = act(handle, &ws, cx, "delete_card", json!({"card": card}));
    assert_eq!(
        (refused, said),
        (false, format!("Deleted card {card} \"New card\"."))
    );
    ws.read_with(cx, |ws, _| assert!(ws.boards.card(board, card).is_none()));
    cx.update(|cx| ws.update(cx, |ws, cx| ws.undo_boards(cx)));
    ws.read_with(cx, |ws, _| assert!(ws.boards.card(board, card).is_some()));
}

#[gpui_kit::test]
fn edit_board_renames_a_board_or_deletes_it_with_its_cards(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (block, board, _) = board_with_a_card(handle, &ws, cx);
    card_in(&ws, cx, board, 0, "B");
    card_in(&ws, cx, board, 1, "C");
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_board",
        json!({"board": board.to_string(), "name": "Sprint 2"}),
    );
    assert_eq!(
        (refused, said),
        (false, format!("Renamed board {board} to \"Sprint 2\"."))
    );
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_board",
        json!({"board": "Sprint 2", "delete": true}),
    );
    assert_eq!(
        (refused, said),
        (
            false,
            format!("Deleted board \"Sprint 2\" ({board}) and its 3 cards.")
        )
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards.board(board).is_none());
        assert!(ws.space.kanban(block).unwrap().shown(ws).is_none());
    });
    // One card, none: the words follow the count.
    let one = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("One"), &["A".into()]))
    });
    card_in(&ws, cx, one, 0, "x");
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "edit_board",
        json!({"board": one, "delete": true}),
    );
    assert_eq!(said, format!("Deleted board \"One\" ({one}) and its card."));
    let none = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("None"), &[]))
    });
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "edit_board",
        json!({"board": none, "delete": true}),
    );
    assert_eq!(said, format!("Deleted board \"None\" ({none})."));
}

#[gpui_kit::test]
fn edit_column_adds_renames_moves_and_deletes_columns(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, board, _) = board_with_a_card(handle, &ws, cx);
    let b = board.to_string();
    let columns = |ws: &Entity<Workspace>, cx: &mut TestAppContext| -> Vec<(u64, String)> {
        ws.read_with(cx, |ws, _| {
            ws.boards
                .board(board)
                .unwrap()
                .columns
                .iter()
                .map(|c| (c.id, c.title.clone()))
                .collect()
        })
    };
    let doing = columns(&ws, cx)[1].0;
    let done = columns(&ws, cx)[2].0;
    let (refused, said) = act(handle, &ws, cx, "edit_column", json!({"board": b}));
    assert!(!refused, "{said}");
    let added = columns(&ws, cx).last().unwrap().0;
    assert_eq!(
        said,
        format!("Added column {added} \"New column\" to board \"Board 1\".")
    );
    // A new one with a title, before Done.
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "edit_column",
        json!({"board": b, "title": "Review", "before": done}),
    );
    let review = columns(&ws, cx).iter().find(|c| c.1 == "Review").unwrap().0;
    assert_eq!(
        said,
        format!("Added column {review} \"Review\" to board \"Board 1\", before column {done}.")
    );
    let titles: Vec<String> = columns(&ws, cx).into_iter().map(|c| c.1).collect();
    assert_eq!(titles, ["To do", "Doing", "Review", "Done", "New column"]);
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "edit_column",
        json!({"board": b, "column": added, "title": "QA"}),
    );
    assert_eq!(said, format!("Renamed column {added} to \"QA\"."));
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "edit_column",
        json!({"board": b, "column": done, "before": "end"}),
    );
    assert_eq!(said, format!("Moved column {done} to the end."));
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "edit_column",
        json!({"board": b, "column": done, "before": doing}),
    );
    assert_eq!(said, format!("Moved column {done} before column {doing}."));
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_column",
        json!({"board": b, "column": done, "before": done}),
    );
    assert_eq!(
        (refused, said.as_str()),
        (true, "A column can't go before itself.")
    );
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_column",
        json!({"board": b, "column": 999, "title": "x"}),
    );
    assert!(
        refused && said.starts_with("There's no column 999."),
        "{said}"
    );
    let at = columns(&ws, cx).iter().position(|c| c.0 == doing).unwrap();
    for _ in 0..4 {
        card_in(&ws, cx, board, at, "x");
    }
    let doing_cards = ws.read_with(cx, |ws, _| {
        ws.boards
            .board(board)
            .unwrap()
            .columns
            .iter()
            .find(|c| c.id == doing)
            .unwrap()
            .cards
            .len()
    });
    assert_eq!(doing_cards, 4);
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "edit_column",
        json!({"board": b, "column": doing, "delete": true}),
    );
    assert_eq!(
        said,
        format!("Deleted column \"Doing\" ({doing}) and its 4 cards.")
    );
}

#[gpui_kit::test]
fn show_board_shows_a_board_or_makes_one_in_a_kanban(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let (block, _, _) = board_with_a_card(handle, &ws, cx);
    let uid = uid_of(&ws, cx, block);
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "show_board",
        json!({"block": uid, "new": ""}),
    );
    assert!(!refused, "{said}");
    let (made, titles) = ws.read_with(cx, |ws, _| {
        let id = ws.space.kanban(block).unwrap().board.unwrap();
        let board = ws.boards.board(id).unwrap();
        (
            (id, board.name.clone()),
            board
                .columns
                .iter()
                .map(|c| c.title.clone())
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(
        said,
        format!(
            "Made board \"{}\" ({}) and showed it in {uid}.",
            made.1, made.0
        )
    );
    assert_eq!(titles, ["To do", "Doing", "Done"]);
    let sprint = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("Sprint"), &["A".into()]))
    });
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "show_board",
        json!({"block": uid, "board": "Sprint"}),
    );
    assert_eq!(
        said,
        format!("Showing board \"Sprint\" ({sprint}) in {uid}.")
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.kanban(block).unwrap().board, Some(sprint))
    });
    // A new name, though a board of it exists, with its columns.
    let (_, said) = act(
        handle,
        &ws,
        cx,
        "show_board",
        json!({"block": uid, "new": "Sprint", "columns": ["A", "B"]}),
    );
    ws.read_with(cx, |ws, _| {
        let id = ws.space.kanban(block).unwrap().board.unwrap();
        assert_ne!(id, sprint, "{said}");
        let board = ws.boards.board(id).unwrap();
        assert_eq!(board.name, "Sprint");
        assert_eq!(
            board
                .columns
                .iter()
                .map(|c| c.title.as_str())
                .collect::<Vec<_>>(),
            ["A", "B"]
        );
    });
    let terminal = uid_of(&ws, cx, terminal);
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "show_board",
        json!({"block": terminal, "board": "Sprint"}),
    );
    assert_eq!(
        (refused, said),
        (true, format!("{terminal} isn't a kanban."))
    );
}

#[gpui_kit::test]
fn a_limited_token_reaches_only_the_boards_its_kanbans_show(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (block, board, _) = board_with_a_card(handle, &ws, cx);
    let uid = uid_of(&ws, cx, block);
    let secret = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("Secret plans"), &["To do".to_string()]))
    });
    let plan = card_in(&ws, cx, secret, 0, "Plan");
    let (_, text) = super::templum_agent::control(
        handle,
        &ws,
        cx,
        "create_workspace",
        json!({"name": "Other"}),
    );
    assert!(text.contains("Created workspace"), "{text}");
    let (_, text) = super::templum_agent::control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": "w2", "kind": "kanban"}),
    );
    let far = text
        .trim_end_matches('.')
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string();
    let limited = with_token(&ws, cx, Scope::Control, Some(vec![first.as_str()]));
    let secret_column = ws.read_with(cx, |ws, _| ws.boards.board(secret).unwrap().columns[0].id);
    for (name, arguments, word) in [
        ("delete_card", json!({"card": plan}), "card"),
        (
            "edit_board",
            json!({"board": secret, "name": "Mine"}),
            "board",
        ),
        (
            "edit_board",
            json!({"board": "Secret plans", "delete": true}),
            "board",
        ),
        (
            "edit_column",
            json!({"board": secret, "column": secret_column, "delete": true}),
            "board",
        ),
        (
            "show_board",
            json!({"block": uid, "board": secret}),
            "board",
        ),
        ("show_board", json!({"block": far, "board": board}), "block"),
    ] {
        let line = tool(handle, &ws, cx, &limited, name, arguments);
        let (code, message) = super::api::refused(&line);
        assert_eq!(
            code,
            Some(i64::from(templum_api::codes::NOT_FOUND)),
            "{name}: {message}"
        );
        assert!(
            message.starts_with(&format!("There's no {word}")),
            "{name}: {message}"
        );
    }
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.boards.board(secret).unwrap().name, "Secret plans");
        assert!(ws.boards.card(secret, plan).is_some());
        assert_eq!(ws.space.kanban(block).unwrap().board, Some(board));
    });
    let (refused, said) = {
        let line = tool(
            handle,
            &ws,
            cx,
            &limited,
            "show_board",
            json!({"block": uid, "new": "Mine"}),
        );
        said(&line)
    };
    assert!(!refused, "{said}");
    let seen = super::api::state(handle, &ws, cx, &limited);
    assert!(
        seen.boards.iter().any(|b| b.name == "Mine"),
        "the new board is in the token's state"
    );
    assert!(seen.boards.iter().all(|b| b.name != "Secret plans"));
}

/// The comment ids a card holds, in order.
fn comment_ids(ws: &Entity<Workspace>, cx: &mut TestAppContext, board: u64, card: u64) -> Vec<u64> {
    ws.read_with(cx, |ws, _| {
        ws.boards
            .card(board, card)
            .unwrap()
            .comments
            .iter()
            .map(|c| c.id)
            .collect()
    })
}

#[gpui_kit::test]
fn through_the_api_the_person_edits_their_comments_and_deletes_any(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, _, terminal) = three_blocks(handle, &ws, cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let uid = uid_of(&ws, cx, terminal);
    let (mine, theirs) = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            (
                b.add_comment(board, card, "you", "hello", 0).unwrap(),
                b.add_comment(board, card, &uid, "from an agent", 0)
                    .unwrap(),
            )
        })
    });
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_comment",
        json!({"card": card, "comment": mine, "text": "fixed"}),
    );
    assert_eq!(
        (refused, said),
        (false, format!("Edited comment {mine} on card {card}."))
    );
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "edit_comment",
        json!({"card": card, "comment": theirs, "text": "mine now"}),
    );
    assert!(refused);
    assert!(
        said.starts_with(&format!("Comment {theirs} is by {uid}"))
            && said.ends_with(", not you: you can only change your own."),
        "{said}"
    );
    let (refused, said) = act(
        handle,
        &ws,
        cx,
        "delete_comment",
        json!({"card": card, "comment": theirs}),
    );
    assert_eq!(
        (refused, said),
        (false, format!("Deleted comment {theirs} on card {card}."))
    );
    assert_eq!(comment_ids(&ws, cx, board, card), [mine]);
}

#[gpui_kit::test]
fn the_templum_agent_edits_and_deletes_only_its_own_comments(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let persons = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_comment(board, card, "you", "hello", 0))
            .unwrap()
    });
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "comment_card",
        json!({"card": card, "text": "noted"}),
    );
    assert!(!failed, "{said}");
    let own = *comment_ids(&ws, cx, board, card).last().unwrap();
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "edit_comment",
        json!({"card": card, "comment": own, "text": "noted!"}),
    );
    assert_eq!(
        (failed, said),
        (false, format!("Edited comment {own} on card {card}."))
    );
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "delete_comment",
        json!({"card": card, "comment": persons}),
    );
    assert_eq!(
        (failed, said),
        (
            true,
            format!("Comment {persons} is by the person, not you: you can only change your own.")
        )
    );
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "delete_comment",
        json!({"card": card, "comment": own}),
    );
    assert_eq!(
        (failed, said),
        (false, format!("Deleted comment {own} on card {card}."))
    );
    assert_eq!(comment_ids(&ws, cx, board, card), [persons]);
}

/// The approvals that wait: (said, details) of each.
fn waiting(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<(String, String)> {
    ws.read_with(cx, |ws, _| {
        ws.templum_agent
            .approvals
            .iter()
            .map(|a| (a.said.clone(), a.details.clone()))
            .collect()
    })
}

#[gpui_kit::test]
fn deleting_the_person_s_card_asks_and_its_own_doesnt(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let answers = control_later(handle, &ws, cx, 7, "delete_card", json!({"card": card}));
    assert!(answers.try_recv().is_err(), "it waits for the person");
    let asks = waiting(&ws, cx);
    assert_eq!(asks.len(), 1);
    assert_eq!(asks[0].0, format!("Delete card {card} \"New card\""));
    assert_eq!(asks[0].1, "On board \"Board 1\", in To do.");
    ws.read_with(cx, |ws, _| assert!(ws.boards.card(board, card).is_some()));
    let (_, said) = control(
        handle,
        &ws,
        cx,
        "create_card",
        json!({"board": board, "title": "Mine"}),
    );
    let own: u64 = said
        .trim_end_matches('.')
        .rsplit(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let (failed, said) = control(handle, &ws, cx, "delete_card", json!({"card": own}));
    assert_eq!(
        (failed, said),
        (false, format!("Deleted card {own} \"Mine\"."))
    );
    assert_eq!(waiting(&ws, cx).len(), 1, "no second approval");
}

#[gpui_kit::test]
fn deleting_a_board_it_made_asks_when_it_holds_the_person_s_cards(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": first, "kind": "kanban", "board": "Mine", "columns": ["A"]}),
    );
    assert!(!failed, "{said}");
    let board = ws.read_with(cx, |ws, _| ws.board_named("Mine").ok().unwrap());
    let column = ws.read_with(cx, |ws, _| ws.boards.board(board).unwrap().columns[0].id);
    card_in(&ws, cx, board, 0, "The person's");
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "edit_column",
        json!({"board": board, "column": column, "delete": true}),
    );
    assert!(answers.try_recv().is_err(), "the column asks");
    let answers = control_later(
        handle,
        &ws,
        cx,
        8,
        "edit_board",
        json!({"board": board, "delete": true}),
    );
    assert!(answers.try_recv().is_err(), "the board asks");
    assert_eq!(waiting(&ws, cx).len(), 2);
    ws.read_with(cx, |ws, _| assert!(ws.boards.board(board).is_some()));
}

#[gpui_kit::test]
fn deleting_a_board_or_a_column_with_the_person_s_cards_asks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let (_, board, _) = board_with_a_card(handle, &ws, cx);
    card_in(&ws, cx, board, 0, "B");
    let answers = control_later(
        handle,
        &ws,
        cx,
        7,
        "edit_board",
        json!({"board": board, "delete": true}),
    );
    assert!(answers.try_recv().is_err());
    assert_eq!(
        waiting(&ws, cx),
        [(
            "Delete board \"Board 1\"".to_string(),
            "Its 2 cards are deleted too.".to_string()
        )]
    );
    let todo = ws.read_with(cx, |ws, _| ws.boards.board(board).unwrap().columns[0].id);
    let answers = control_later(
        handle,
        &ws,
        cx,
        8,
        "edit_column",
        json!({"board": board, "column": todo, "delete": true}),
    );
    assert!(answers.try_recv().is_err());
    assert_eq!(
        waiting(&ws, cx)[1].0,
        "Delete column \"To do\" of \"Board 1\""
    );
    ws.read_with(cx, |ws, _| assert!(ws.boards.board(board).is_some()));
    // A board no kanban shows asks the same; an empty one of the person's goes at once.
    let hidden = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("Hidden"), &["A".into()]))
    });
    card_in(&ws, cx, hidden, 0, "x");
    let answers = control_later(
        handle,
        &ws,
        cx,
        9,
        "edit_board",
        json!({"board": hidden, "delete": true}),
    );
    assert!(answers.try_recv().is_err());
    assert_eq!(waiting(&ws, cx).len(), 3);
    let empty = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("Empty"), &["A".into()]))
    });
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "edit_board",
        json!({"board": empty, "delete": true}),
    );
    assert_eq!(
        (failed, said),
        (false, format!("Deleted board \"Empty\" ({empty})."))
    );
    // A board it made, with a card it made, goes at once.
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "add_block",
        json!({"workspace": first, "kind": "kanban", "board": "Mine", "columns": ["A"]}),
    );
    assert!(!failed, "{said}");
    let (_, said) = control(
        handle,
        &ws,
        cx,
        "create_card",
        json!({"board": "Mine", "title": "c"}),
    );
    assert!(said.starts_with("Created card"), "{said}");
    let (failed, said) = control(
        handle,
        &ws,
        cx,
        "edit_board",
        json!({"board": "Mine", "delete": true}),
    );
    assert!(
        !failed && said.starts_with("Deleted board \"Mine\""),
        "{said}"
    );
    assert_eq!(waiting(&ws, cx).len(), 3, "it asked nothing");
}
