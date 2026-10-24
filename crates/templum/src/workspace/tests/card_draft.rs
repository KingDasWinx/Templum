//! The card dialog works on a copy: nothing reaches the board before Done, and a draft keeps it.

use super::*;
use crate::board::Priority;

/// A kanban block on a new board with no card: (block, board, first column).
pub(super) fn empty_board(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (BlockId, u64, u64) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_kanban_at(Point::new(900.0, 0.0), window, cx);
            let block = ws.space.canvas.top().unwrap();
            ws.new_board(block, cx);
            let board = ws.space.kanban(block).unwrap().board.unwrap();
            let todo = ws.boards.board(board).unwrap().columns[0].id;
            ws.space.appearing.clear();
            (block, board, todo)
        })
    })
    .unwrap()
}

pub(super) fn click_add(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    block: BlockId,
    column: u64,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let add = gpui_kit::ElementId::Name(format!("add-card-{}-{column}", block.0).into());
        window.click(add, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Presses `key`, then draws.
pub(super) fn press_key(handle: AnyWindowHandle, cx: &mut TestAppContext, key: &str) {
    cx.update_window(handle, |_, window, cx| {
        window.press(key, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

pub(super) fn click_on(handle: AnyWindowHandle, cx: &mut TestAppContext, what: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        window.click(what, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

pub(super) fn dialog_open(handle: AnyWindowHandle, cx: &mut TestAppContext) -> bool {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.try_find("card-done").is_some()
    })
    .unwrap()
}

pub(super) fn open_card(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    board: u64,
    card: u64,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::kanban::open_card(ws, board, card, window, cx)
        });
        window.render_frame(cx);
    })
    .unwrap();
}

pub(super) fn cards(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    board: u64,
) -> Vec<crate::board::Card> {
    ws.read_with(cx, |ws, _| {
        ws.boards
            .board(board)
            .unwrap()
            .columns
            .iter()
            .flat_map(|c| c.cards.clone())
            .collect()
    })
}

#[gpui_kit::test]
fn add_a_card_and_leave_without_typing_creates_nothing_and_keeps_no_draft(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (block, board, todo) = empty_board(handle, &ws, cx);
    click_add(handle, cx, block, todo);
    assert!(dialog_open(handle, cx), "it opens");
    assert!(cards(&ws, cx, board).is_empty(), "and creates nothing");
    press_key(handle, cx, "escape");
    assert!(!dialog_open(handle, cx), "Esc with no change just closes");
    assert!(cards(&ws, cx, board).is_empty());
    ws.read_with(cx, |ws, _| assert!(ws.boards.drafts.is_empty()));
}

#[gpui_kit::test]
fn done_creates_the_card_with_its_title_item_and_comment_as_one_undo_step(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (block, board, todo) = empty_board(handle, &ws, cx);
    click_add(handle, cx, block, todo);
    cx.simulate_input(handle, "Ship it");
    click_on(handle, cx, "card-item-field");
    cx.simulate_input(handle, "Build");
    press_key(handle, cx, "enter");
    click_on(handle, cx, "comment-field");
    cx.simulate_input(handle, "Hello");
    click_on(handle, cx, "add-comment");
    assert!(cards(&ws, cx, board).is_empty(), "nothing before Done");
    click_on(handle, cx, "card-done");
    assert!(!dialog_open(handle, cx));
    let made = cards(&ws, cx, board);
    assert_eq!(made.len(), 1);
    let card = &made[0];
    assert_eq!(card.title, "Ship it");
    assert_eq!(
        card.checklist
            .iter()
            .map(|i| i.text.as_str())
            .collect::<Vec<_>>(),
        ["Build"]
    );
    assert_eq!(
        card.comments
            .iter()
            .map(|c| (c.by.as_str(), c.text.as_str()))
            .collect::<Vec<_>>(),
        [("you", "Hello")]
    );
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    assert!(cards(&ws, cx, board).is_empty(), "one Ctrl+Z removes it");
}

#[gpui_kit::test]
fn a_new_cards_draft_is_kept_off_the_board_and_continued(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (block, board, todo) = empty_board(handle, &ws, cx);
    click_add(handle, cx, block, todo);
    cx.simulate_input(handle, "Later");
    press_key(handle, cx, "escape");
    assert!(dialog_open(handle, cx), "Esc with changes asks first");
    click_on(handle, cx, "leave-save-draft");
    assert!(!dialog_open(handle, cx));
    assert!(cards(&ws, cx, board).is_empty(), "a draft isn't a card");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.boards.draft(todo).unwrap().work.title, "Later");
    });
    // "Continue draft" opens it with what was typed; leaving it alone keeps it.
    click_add(handle, cx, block, todo);
    press_key(handle, cx, "escape");
    assert!(!dialog_open(handle, cx));
    ws.read_with(cx, |ws, _| assert!(ws.boards.draft(todo).is_some()));
    click_add(handle, cx, block, todo);
    click_on(handle, cx, "card-done");
    assert_eq!(cards(&ws, cx, board)[0].title, "Later");
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards.drafts.is_empty(), "Done drops it")
    });
}

#[gpui_kit::test]
fn discard_on_a_new_card_drops_its_draft_and_the_footer_button_does_too(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (block, board, todo) = empty_board(handle, &ws, cx);
    click_add(handle, cx, block, todo);
    cx.simulate_input(handle, "Gone");
    press_key(handle, cx, "escape");
    click_on(handle, cx, "leave-discard");
    assert!(!dialog_open(handle, cx));
    ws.read_with(cx, |ws, _| assert!(ws.boards.drafts.is_empty()));
    // The footer's left button on a new card discards, draft and all.
    click_add(handle, cx, block, todo);
    cx.simulate_input(handle, "Kept once");
    press_key(handle, cx, "escape");
    click_on(handle, cx, "leave-save-draft");
    click_add(handle, cx, block, todo);
    assert!(
        cx.update_window(handle, |_, window, _| window
            .try_find("delete-card")
            .is_none())
            .unwrap()
    );
    click_on(handle, cx, "discard-card");
    assert!(!dialog_open(handle, cx));
    assert!(cards(&ws, cx, board).is_empty());
    ws.read_with(cx, |ws, _| assert!(ws.boards.drafts.is_empty()));
}

#[gpui_kit::test]
fn leaving_an_existing_card_with_changes_asks_discard_keeps_the_card_save_draft_keeps_it_apart(
    cx: &mut TestAppContext,
) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (block, board, card) = board_with_a_card(handle, &ws, cx);
    let title = |cx: &mut TestAppContext| cards(&ws, cx, board)[0].title.clone();
    open_card(handle, &ws, cx, board, card);
    cx.simulate_input(handle, " edited");
    press_key(handle, cx, "escape");
    assert!(dialog_open(handle, cx), "the question stands over the card");
    press_key(handle, cx, "escape");
    assert!(
        dialog_open(handle, cx),
        "Esc in the question goes back to editing"
    );
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("leave-save").is_none())
    })
    .unwrap();
    press_key(handle, cx, "escape");
    click_on(handle, cx, "leave-discard");
    assert!(!dialog_open(handle, cx));
    assert_eq!(title(cx), "New card", "Discard leaves the card as it was");
    ws.read_with(cx, |ws, _| assert!(ws.boards.drafts.is_empty()));

    // Save draft: the board, which agents read, is unchanged; the face wears the mark.
    open_card(handle, &ws, cx, board, card);
    cx.simulate_input(handle, " edited");
    press_key(handle, cx, "escape");
    click_on(handle, cx, "leave-save-draft");
    assert!(!dialog_open(handle, cx));
    assert_eq!(title(cx), "New card");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let mark = gpui_kit::ElementId::Name(format!("draft-{}-{card}", block.0).into());
        assert!(
            window.try_find(mark).is_some(),
            "the face shows the draft mark"
        );
    })
    .unwrap();
    // Reopening shows the draft: Done applies it, and the mark goes.
    open_card(handle, &ws, cx, board, card);
    click_on(handle, cx, "card-done");
    assert!(title(cx).contains("edited"), "{}", title(cx));
    ws.read_with(cx, |ws, _| assert!(ws.boards.drafts.is_empty()));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let mark = gpui_kit::ElementId::Name(format!("draft-{}-{card}", block.0).into());
        assert!(window.try_find(mark).is_none());
    })
    .unwrap();

    // Save, from the question, is Done.
    open_card(handle, &ws, cx, board, card);
    click_on(handle, cx, "priority-high");
    press_key(handle, cx, "escape");
    click_on(handle, cx, "leave-save");
    assert!(!dialog_open(handle, cx));
    assert_eq!(cards(&ws, cx, board)[0].priority, Some(Priority::High));
}

#[gpui_kit::test]
fn an_agents_change_to_what_the_person_left_alone_survives_done(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    open_card(handle, &ws, cx, board, card);
    click_on(handle, cx, "label-red");
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            b.edit_card(board, card, |c| c.priority = Some(Priority::High));
            b.add_comment(board, card, "agent", "Seen", 1);
        })
    });
    click_on(handle, cx, "card-done");
    let c = &cards(&ws, cx, board)[0];
    assert_eq!(c.labels, [crate::board::Label::Red], "the person's change");
    assert_eq!(c.priority, Some(Priority::High), "the agent's change");
    assert_eq!(c.comments.len(), 1);
}

#[gpui_kit::test]
fn done_on_a_card_deleted_meanwhile_closes_without_panicking_or_bringing_it_back(
    cx: &mut TestAppContext,
) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    open_card(handle, &ws, cx, board, card);
    cx.simulate_input(handle, "!");
    ws.update(cx, |ws, _| ws.edit_boards(|b| b.remove_card(board, card)));
    click_on(handle, cx, "card-done");
    assert!(!dialog_open(handle, cx));
    assert!(cards(&ws, cx, board).is_empty());
}

#[gpui_kit::test]
fn a_click_acts_on_what_the_dialog_shows_after_an_agent_changed_it(cx: &mut TestAppContext) {
    use crate::board::{Assignee, Label};
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    let item = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_items(board, card, &[("Build".into(), false)]))[0]
    });
    open_card(handle, &ws, cx, board, card);
    // The agent ticks the item, claims the card, sets a priority and adds a label, all while
    // the dialog is open and showing the card before.
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| {
            b.set_item_done(board, card, item, true);
            b.edit_card(board, card, |c| {
                c.assignee = Some(Assignee::Block("zzzzz".into()));
                c.priority = Some(Priority::High);
                c.labels.push(Label::Green);
            });
        })
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    // The person undoes each in the dialog.
    cx.update_window(handle, |_, window, cx| {
        window.click(
            gpui_kit::ElementId::Name(format!("card-item-box-{item}").into()),
            cx,
        );
        window.render_frame(cx);
        window.click("unassign", cx);
        window.render_frame(cx);
        window.click("priority-none", cx);
        window.render_frame(cx);
        window.click("label-green", cx);
        window.render_frame(cx);
    })
    .unwrap();
    click_on(handle, cx, "card-done");
    let c = &cards(&ws, cx, board)[0];
    assert!(!c.checklist[0].done, "unticked");
    assert_eq!(c.assignee, None, "freed");
    assert_eq!(c.priority, None, "no priority");
    assert!(c.labels.is_empty(), "label off");
}

#[gpui_kit::test]
fn an_item_an_agent_adds_while_the_dialog_is_open_can_be_ticked(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    open_card(handle, &ws, cx, board, card);
    let item = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_items(board, card, &[("Docs".into(), false)]))[0]
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(
            gpui_kit::ElementId::Name(format!("card-item-box-{item}").into()),
            cx,
        );
        window.render_frame(cx);
    })
    .unwrap();
    click_on(handle, cx, "card-done");
    assert!(cards(&ws, cx, board)[0].checklist[0].done);
}

#[gpui_kit::test]
fn closing_the_window_with_a_card_dialog_open_keeps_what_was_typed_as_a_draft(
    cx: &mut TestAppContext,
) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (block, board, todo) = empty_board(handle, &ws, cx);
    let card = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_card(board, todo)).unwrap()
    });
    open_card(handle, &ws, cx, board, card);
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards.drafts.is_empty(), "nothing typed")
    });
    cx.simulate_input(handle, "typed ");
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    ws.read_with(cx, |ws, _| {
        let draft = ws.boards.draft(card).expect("a draft");
        assert!(draft.work.title.contains("typed"));
        assert_eq!(draft.base.title, "New card");
    });
    // Done closes the dialog: the window's closing keeps nothing of it.
    ws.update(cx, |ws, _| ws.boards.drop_draft(card));
    click_on(handle, cx, "card-done");
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    ws.read_with(cx, |ws, _| assert!(ws.boards.drafts.is_empty()));
    // A new card's dialog is kept too, for its column.
    click_add(handle, cx, block, todo);
    cx.simulate_input(handle, "Later");
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards.draft(todo).unwrap().work.title.contains("Later"));
    });
}

#[gpui_kit::test]
fn ctrl_z_keeps_the_drafts_kept_since_and_deleting_a_card_drops_its_draft(cx: &mut TestAppContext) {
    use crate::board::Draft;
    let (handle, ws) = open(cx);
    let (_, board, card) = board_with_a_card(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_comment(board, card, "you", "x", 1));
        // A draft kept after the step: undoing the step must not take it away.
        let base = ws.boards.card(board, card).unwrap().clone();
        let mut work = base.clone();
        work.title = "Mine".into();
        ws.boards.keep_draft(Draft {
            at: card,
            new: false,
            base,
            work,
        });
    });
    ws.update(cx, |ws, cx| assert!(ws.undo_boards(cx)));
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards.card(board, card).unwrap().comments.is_empty());
        assert_eq!(ws.boards.draft(card).unwrap().work.title, "Mine", "kept");
    });
    ws.update(cx, |ws, _| ws.edit_boards(|b| b.remove_card(board, card)));
    ws.read_with(cx, |ws, _| assert!(ws.boards.drafts.is_empty(), "pruned"));
}
