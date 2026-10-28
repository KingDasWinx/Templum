//! A kanban of a mirrored workspace: drawn from home's boards and cards, and everything done to it
//! done at home (Templum to Templum, part 2d).

use std::sync::Arc;

use serde_json::json;

use templum_api::model::{
    Assignee as HomeAssignee, AttachmentView, BlockView, BoardView, CardView, ColumnView,
    CommentView, ItemView, Rect as ApiRect, State,
};

use super::remote::{apply, draw, live_terminals, offline, show_remote, viewer};
use super::*;
use crate::board::Assignee;
use crate::kanban::Kanban;
use crate::workspace::Body;

/// Home's ids of the board its kanban `k1` shows: the board, its columns To do, Doing and Done,
/// and its cards 40 (in To do) and 41, 42 (in Doing).
#[derive(Clone, Copy)]
pub(super) struct Ids {
    pub board: u64,
    pub columns: [u64; 3],
    pub cards: [u64; 3],
}

pub(super) const HOME: Ids = Ids {
    board: 12,
    columns: [30, 31, 32],
    cards: [40, 41, 42],
};

fn card(ids: Ids, n: usize, title: &str) -> Arc<CardView> {
    Arc::new(CardView {
        id: ids.cards[n],
        board: ids.board,
        column: ids.columns[usize::from(n > 0)],
        title: title.into(),
        ..CardView::default()
    })
}

/// Home's state: its workspace `w1` with the terminal `b1` ("claude") and the kanban `k1` showing
/// board "Sprint", which has card 40 "Fix login" (red, for b1, two comments, one by b1) in To do and
/// two more in Doing.
pub(super) fn board_state(ids: Ids) -> State {
    let mut state = super::remote::terminals_of_home(&["b1"]);
    let mut terminal = BlockView::clone(&state.blocks[0]);
    terminal.title = "claude".into();
    // Out of the kanban's way, which is drawn whole in the window.
    terminal.rect.y = 600.0;
    let kanban = BlockView {
        id: "k1".into(),
        workspace: "w1".into(),
        kind: "kanban".into(),
        title: "Sprint".into(),
        z: 1,
        rect: ApiRect {
            x: 0.0,
            y: 0.0,
            w: 900.0,
            h: 540.0,
        },
        board: Some(ids.board),
        ..BlockView::default()
    };
    state.blocks = vec![Arc::new(terminal), Arc::new(kanban)];
    let mut fix = CardView::clone(&card(ids, 0, "Fix login"));
    fix.labels = vec!["red".into()];
    fix.checklist = vec![ItemView {
        id: 13,
        text: "Ship".into(),
        done: false,
    }];
    fix.attachments = vec![AttachmentView {
        name: "shot.png".into(),
        size: 12 * 1024,
    }];
    fix.assignee = Some(HomeAssignee {
        block: Some("b1".into()),
        agent: None,
    });
    fix.comments = ["b1", "you"]
        .iter()
        .enumerate()
        .map(|(n, by)| CommentView {
            number: 50 + n as u64,
            by: (*by).into(),
            text: format!("comment {n}"),
            at: 5,
            edited: false,
        })
        .collect();
    state.cards = vec![Arc::new(fix), card(ids, 1, "Second"), card(ids, 2, "Third")];
    state.boards = vec![Arc::new(BoardView {
        id: ids.board,
        name: "Sprint".into(),
        columns: vec![
            ColumnView {
                id: ids.columns[0],
                title: "To do".into(),
                cards: vec![ids.cards[0]],
            },
            ColumnView {
                id: ids.columns[1],
                title: "Doing".into(),
                cards: vec![ids.cards[1], ids.cards[2]],
            },
            ColumnView {
                id: ids.columns[2],
                title: "Done".into(),
                cards: vec![],
            },
        ],
    })];
    state
}

/// A shown, live (Control) mirror of home's `board_state(ids)`, the mirror's own boards empty and
/// its `boards.json` written once.
pub(super) fn live_board(
    cx: &mut TestAppContext,
    ids: Ids,
) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let (handle, ws, dir) = live_terminals(cx, &["b1"]);
    apply(handle, &ws, cx, board_state(ids));
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    draw(handle, cx);
    (handle, ws, dir)
}

/// The id of the element `{prefix}-{n}-{id}` the kanban's view draws.
pub(super) fn el(prefix: &str, n: BlockId, id: u64) -> gpui_kit::ElementId {
    gpui_kit::ElementId::Name(format!("{prefix}-{}-{id}", n.0).into())
}

/// Whether the element `{prefix}-{n}-{id}` was drawn in the last frame.
pub(super) fn drawn(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    prefix: &str,
    n: BlockId,
    id: u64,
) -> bool {
    cx.update_window(handle, |_, window, _| {
        let id = gpui_kit::ElementId::Name(format!("{prefix}-{}-{id}", n.0).into());
        window.try_find(id).is_some()
    })
    .unwrap()
}

#[gpui_kit::test]
fn a_remote_kanban_draws_home_s_board(cx: &mut TestAppContext) {
    let (handle, ws, dir) = live_board(cx, HOME);
    let k1 = viewer(&ws, cx, "k1");
    let before = std::fs::read(crate::board::path(dir.path())).ok();
    assert!(drawn(handle, cx, "card", k1, 40) && drawn(handle, cx, "card", k1, 41));
    assert!(drawn(handle, cx, "assignee", k1, 40));
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(("columns", k1.0 as usize)).is_some());
    })
    .unwrap();
    cx.update(|cx| {
        ws.read_with(cx, |ws, cx| {
            let body = ws.space.blocks.get(&k1).unwrap();
            assert!(matches!(
                body,
                Body::Kanban(Kanban {
                    board: Some(12),
                    remote: true
                })
            ));
            assert_eq!(crate::block::title(ws, body, cx), "Sprint");
            let label = |a: Assignee| crate::kanban::assignee_label(ws, "r1", &a, cx);
            assert_eq!(label(Assignee::Block("b1".into())), "claude");
            assert_eq!(label(Assignee::Block("zzzzz".into())), "zzzzz");
        })
    });
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    ws.read_with(cx, |ws, _| assert!(ws.boards.boards.is_empty()));
    assert_eq!(std::fs::read(crate::board::path(dir.path())).ok(), before);
}

#[gpui_kit::test]
fn no_local_tool_sees_a_remote_board(cx: &mut TestAppContext) {
    // A local workspace whose kanban shows a board whose ids are home's too, named "Mine".
    let (handle, ws, _dir) = offline(cx);
    let (local_block, board, card_id) = board_with_a_card(handle, &ws, cx);
    ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.rename_board(board, "Mine"));
        ws.edit_boards(|b| b.edit_card(board, card_id, |c| c.title = "Local".into()));
    });
    let local_space = ws.read_with(cx, |ws, _| ws.space.id.clone());
    let local_uid = ws.update(cx, |ws, _| ws.space.uids[&local_block].clone());
    show_remote(handle, &ws, cx);
    let ids = Ids {
        board,
        columns: [30, 31, 32],
        cards: [card_id, 41, 42],
    };
    ws.update(cx, |ws, _| {
        super::remote::live(ws, templum_api::store::Scope::Control);
        ws.space.remote.as_mut().unwrap().entry.home = Some("w1".into());
    });
    apply(handle, &ws, cx, board_state(ids));
    draw(handle, cx);
    let k1 = viewer(&ws, cx, "k1");
    let before = ws.read_with(cx, |ws, _| ws.boards.clone());
    cx.update(|cx| {
        ws.read_with(cx, |ws, cx| {
            // The board the local lookup finds is the local one's.
            assert_eq!(
                ws.board_open_at(board).map(|s| s.id.clone()),
                Some(local_space.clone())
            );
            assert!(
                ws.space.kanban(k1).is_none(),
                "no local tool sees the remote kanban"
            );
            // The remote's kanban draws home's title, the local one its own.
            let remote = ws.space.blocks.get(&k1).unwrap();
            assert_eq!(crate::block::title(ws, remote, cx), "Sprint");
            assert_eq!(ws.uid_title(&local_uid, cx).as_deref(), Some("Mine"));
            // Home's "b1" is a block of home's: never a local one's.
            let label =
                crate::kanban::assignee_label(ws, "r1", &Assignee::Block(local_uid.clone()), cx);
            assert_eq!(label, local_uid, "a local block's id names nothing at home");
        })
    });
    // Ctrl+Z with the remote kanban focused undoes none of this computer's boards.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, _| ws.space.focused = Some(k1));
        window.press(UNDO, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards == before, "this computer's boards as they were")
    });
}

// Acting on a remote board.

use std::time::Instant;
use templum_api::client::Tools;
use templum_api::store::Scope;

use super::remote::{answer, refusal, remote_of, said, sent};
use crate::workspace::BoardAct;
use crate::workspace::holds::HOLD;
use crate::workspace::remote::{Deed, Nay};

/// The cards of home's boards as the mirror's copy lists them: each column's card ids.
fn copy(ws: &Entity<Workspace>, cx: &mut TestAppContext, board: u64) -> Vec<Vec<u64>> {
    ws.read_with(cx, |ws, _| {
        remote_of(ws)
            .boards
            .board(board)
            .unwrap()
            .columns
            .iter()
            .map(|c| c.cards.iter().map(|c| c.id).collect())
            .collect()
    })
}

/// Home's state with its board's columns holding `columns`' cards.
fn state_with(columns: [Vec<u64>; 3]) -> State {
    let mut state = board_state(HOME);
    let board = BoardView {
        id: HOME.board,
        name: "Sprint".into(),
        columns: columns
            .into_iter()
            .enumerate()
            .map(|(n, cards)| ColumnView {
                id: HOME.columns[n],
                title: ["To do", "Doing", "Done"][n].into(),
                cards,
            })
            .collect(),
    };
    state.boards = vec![Arc::new(board)];
    state
}

fn last_undo_is_a_mark(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    ws.read_with(cx, |ws, _| {
        matches!(
            ws.space.undo.last(),
            Some((_, crate::workspace::history::Change::Mark))
        )
    })
}

#[gpui_kit::test]
fn a_card_dropped_on_another_sends_move_card_before_it_and_shows_at_once(cx: &mut TestAppContext) {
    let (handle, ws, dir) = live_board(cx, HOME);
    let n = viewer(&ws, cx, "k1");
    let boards_json = std::fs::read(crate::board::path(dir.path())).ok();
    drop_on(handle, cx, el("card", n, 40), el("card", n, 42));
    assert_eq!(
        sent(&ws, cx),
        [(
            "move_card".to_string(),
            json!({"card": 40, "column": "31", "before": 42})
        )]
    );
    assert_eq!(
        copy(&ws, cx, 12),
        [vec![], vec![41, 40, 42], vec![]],
        "at once"
    );
    assert!(last_undo_is_a_mark(&ws, cx));
    ws.read_with(cx, |ws, _| assert!(ws.boards.boards.is_empty()));
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    assert_eq!(
        std::fs::read(crate::board::path(dir.path())).ok(),
        boards_json
    );
    // Home's state showing it ends the hold.
    apply(
        handle,
        &ws,
        cx,
        state_with([vec![], vec![41, 40, 42], vec![]]),
    );
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).cards_moved.get(&40).is_none())
    });
    assert_eq!(copy(&ws, cx, 12), [vec![], vec![41, 40, 42], vec![]]);
}

#[gpui_kit::test]
fn a_card_moved_here_goes_back_after_the_hold_or_at_a_refusal(cx: &mut TestAppContext) {
    // No state comes: after HOLD home's boards are drawn again.
    let (handle, ws, _dir) = live_board(cx, HOME);
    let n = viewer(&ws, cx, "k1");
    drop_on(handle, cx, el("card", n, 40), el("card", n, 42));
    assert_eq!(copy(&ws, cx, 12)[1], [41, 40, 42]);
    ws.update(cx, |ws, _| {
        ws.expire_holds(Instant::now() + HOLD + std::time::Duration::from_millis(10))
    });
    assert_eq!(copy(&ws, cx, 12), [vec![40], vec![41, 42], vec![]]);
    // A refusal brings it back at once, and is said once.
    let (handle, ws, _dir) = live_board(cx, HOME);
    let n = viewer(&ws, cx, "k1");
    drop_on(handle, cx, el("card", n, 40), el("card", n, 42));
    answer(handle, &ws, cx, 0, refusal("Card 42 isn't in Doing."));
    assert_eq!(copy(&ws, cx, 12), [vec![40], vec![41, 42], vec![]]);
    assert_eq!(said(&ws, cx), ["Card 42 isn't in Doing."]);
}

#[gpui_kit::test]
fn columns_and_boards_are_edited_at_home(cx: &mut TestAppContext) {
    let (handle, ws, dir) = live_board(cx, HOME);
    let boards_json = std::fs::read(crate::board::path(dir.path())).ok();
    let act = |cx: &mut TestAppContext, act: BoardAct| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.act_on_boards("r1", act, window, cx))
        })
        .unwrap();
    };
    // The button's click is this act (it is past the window's edge here).
    act(cx, BoardAct::AddColumn { board: 12 });
    // Done's title dropped on To do's: the drop is this act.
    act(
        cx,
        BoardAct::MoveColumn {
            board: 12,
            column: 32,
            to: 30,
        },
    );
    act(
        cx,
        BoardAct::RenameColumn {
            board: 12,
            column: 31,
            title: "Review".into(),
        },
    );
    act(
        cx,
        BoardAct::DeleteColumn {
            board: 12,
            column: 31,
        },
    );
    act(
        cx,
        BoardAct::RenameBoard {
            board: 12,
            name: "Sprint 2".into(),
        },
    );
    act(cx, BoardAct::DeleteBoard { board: 12 });
    assert_eq!(
        sent(&ws, cx),
        [
            ("edit_column", json!({"board": "12"})),
            (
                "edit_column",
                json!({"board": "12", "column": 32, "before": 30})
            ),
            (
                "edit_column",
                json!({"board": "12", "column": 31, "title": "Review"})
            ),
            (
                "edit_column",
                json!({"board": "12", "column": 31, "delete": true})
            ),
            ("edit_board", json!({"board": "12", "name": "Sprint 2"})),
            ("edit_board", json!({"board": "12", "delete": true})),
        ]
        .map(|(tool, args)| (tool.to_string(), args))
    );
    // Nothing shows until home's state comes; nothing is written here.
    assert_eq!(copy(&ws, cx, 12), [vec![40], vec![41, 42], vec![]]);
    ws.read_with(cx, |ws, _| assert!(ws.boards.boards.is_empty()));
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    assert_eq!(
        std::fs::read(crate::board::path(dir.path())).ok(),
        boards_json
    );
    assert!(last_undo_is_a_mark(&ws, cx));
}

#[gpui_kit::test]
fn all_boards_lists_the_share_s_boards_and_shows_one_at_home(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_board(cx, HOME);
    let k1 = viewer(&ws, cx, "k1");
    let mut state = board_state(HOME);
    state.boards.push(Arc::new(BoardView {
        id: 13,
        name: "Ops".into(),
        columns: vec![],
    }));
    apply(handle, &ws, cx, state);
    let open = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |_, cx| {
                crate::kanban::open_boards("r1".into(), k1, window, cx)
            });
            window.render_frame(cx);
        })
        .unwrap()
    };
    open(cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find("board-row-12").is_some() && window.try_find("board-row-13").is_some()
        );
        window.click("board-row-13", cx);
    })
    .unwrap();
    open(cx);
    cx.update_window(handle, |_, window, cx| window.click("boards-new", cx))
        .unwrap();
    assert_eq!(
        sent(&ws, cx),
        [
            ("show_board", json!({"block": "k1", "board": "13"})),
            ("show_board", json!({"block": "k1", "new": ""})),
        ]
        .map(|(tool, args)| (tool.to_string(), args))
    );
    assert!(
        !last_undo_is_a_mark(&ws, cx),
        "showing a board leaves no mark"
    );
    // The dialog outliving its workspace: a click sends nothing and says so.
    open(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        if window.try_find("board-row-13").is_some() {
            window.click("board-row-13", cx);
        } else {
            ws.update(cx, |ws, cx| {
                let show = BoardAct::Show {
                    block: k1,
                    board: Some(13),
                };
                ws.act_on_boards("r1", show, window, cx)
            });
        }
    })
    .unwrap();
    assert_eq!(sent(&ws, cx).len(), 2, "nothing more was sent");
    assert_eq!(
        said(&ws, cx).last().map(String::as_str),
        Some("Another workspace is shown now: nothing was done.")
    );
}

#[gpui_kit::test]
fn a_boards_dialog_of_this_computer_does_nothing_once_a_mirror_is_shown(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_board(cx, HOME);
    // A board of this computer whose id is also a board's of home.
    let local = ws.update(cx, |ws, _| {
        ws.edit_boards(|b| b.add_board_with(Some("Local"), &["A".into()]))
    });
    let mut state = board_state(Ids {
        board: local,
        ..HOME
    });
    state.boards.push(Arc::new(BoardView {
        id: 99,
        name: "Ops".into(),
        columns: vec![],
    }));
    apply(handle, &ws, cx, state);
    let here = ws.read_with(cx, |ws, _| ws.order().iter().position(|s| s.id == "r1"));
    let here = here.expect("the mirror");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx));
        let space = ws.read_with(cx, |ws, _| ws.space.id.clone());
        ws.update(cx, |_, cx| {
            crate::kanban::open_boards(space, BlockId(0), window, cx)
        });
        window.render_frame(cx);
        // Another workspace is shown by the time the person clicks (an agent's `show`).
        ws.update(cx, |ws, cx| ws.switch_to(here, window, cx));
        window.render_frame(cx);
        assert!(
            window.try_find("board-row-99").is_none(),
            "the dialog keeps listing the boards it was opened on, not home's"
        );
        let trash = format!("board-delete-{local}");
        window.click(trash.clone(), cx);
        window.click(trash, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(
            ws.boards.board(local).is_some(),
            "this computer's board is kept"
        )
    });
    assert_eq!(
        said(&ws, cx).last().map(String::as_str),
        Some("Another workspace is shown now: nothing was done.")
    );
}

#[gpui_kit::test]
fn a_card_deleted_on_the_mirror_hides_until_home_drops_it(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_board(cx, HOME);
    let delete = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.act_on_boards(
                    "r1",
                    BoardAct::DeleteCard {
                        board: 12,
                        card: 40,
                    },
                    window,
                    cx,
                )
            })
        })
        .unwrap();
    };
    delete(&ws, cx);
    assert_eq!(
        sent(&ws, cx),
        [("delete_card".to_string(), json!({"card": 40}))]
    );
    assert_eq!(copy(&ws, cx, 12)[0], Vec::<u64>::new(), "gone at once");
    ws.update(cx, |ws, _| {
        ws.expire_holds(Instant::now() + HOLD + std::time::Duration::from_millis(10))
    });
    assert_eq!(
        copy(&ws, cx, 12)[0],
        [40],
        "home never dropped it: it is back"
    );
    // A refusal brings it back at once, and says why.
    delete(&ws, cx);
    answer(handle, &ws, cx, 1, refusal("Card 40 is gone."));
    assert_eq!(copy(&ws, cx, 12)[0], [40]);
    assert_eq!(said(&ws, cx), ["Card 40 is gone."]);
    // Home dropping it ends the hold for good.
    delete(&ws, cx);
    let mut state = state_with([vec![], vec![41, 42], vec![]]);
    state.cards.retain(|c| c.id != 40);
    apply(handle, &ws, cx, state);
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).cards_deleted.get(&40).is_none())
    });
    assert_eq!(copy(&ws, cx, 12)[0], Vec::<u64>::new());
}

#[gpui_kit::test]
fn an_older_home_takes_the_card_at_its_column_s_end_and_dims_what_it_lacks(
    cx: &mut TestAppContext,
) {
    let (handle, ws, _dir) = live_board(cx, HOME);
    let n = viewer(&ws, cx, "k1");
    ws.update(cx, |ws, _| {
        let tools = json!([
            { "name": "move_card", "inputSchema": { "properties": { "card": {}, "column": {} } } },
        ]);
        ws.space.remote.as_mut().unwrap().tools = Some(Tools::read(&tools));
    });
    drop_on(handle, cx, el("card", n, 40), el("card", n, 42));
    assert_eq!(
        sent(&ws, cx),
        [("move_card".to_string(), json!({"card": 40, "column": "31"}))]
    );
    assert_eq!(copy(&ws, cx, 12)[1], [41, 42, 40], "held at Doing's end");
    ws.read_with(cx, |ws, _| {
        let remote = remote_of(ws);
        assert_eq!(remote.why_not(Deed::DeleteCard), Some(Nay::Older));
        assert_eq!(remote.why_not(Deed::EditColumn), Some(Nay::Older));
        assert_eq!(remote.why_not(Deed::MoveCard), None);
    });
    draw(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click(el("add-card", n, 31), cx);
        ws.update(cx, |ws, cx| {
            ws.act_on_boards("r1", BoardAct::AddColumn { board: 12 }, window, cx)
        });
    })
    .unwrap();
    assert_eq!(sent(&ws, cx).len(), 1, "a dimmed control sends nothing");
    let said = said(&ws, cx);
    let said = said.last().map(String::as_str).unwrap_or_default();
    assert!(
        said.ends_with("'s Templum is older: update it to do this from here."),
        "{said}"
    );
}

#[gpui_kit::test]
fn send_to_on_a_remote_card_lists_its_terminals_and_pastes_there(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_board(cx, HOME);
    let (b1, _) = (viewer(&ws, cx, "b1"), ());
    // A Full share: home's terminal, by home's title; never a local one.
    ws.update(cx, |ws, _| super::remote::live(ws, Scope::Full));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.remote_terminals(), [(b1, "claude".to_string())])
    });
    let sent_to = ws.update(cx, |ws, _| ws.remote_terminals().len());
    assert_eq!(sent_to, 1);
    let ok = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.remote_send_to(b1, "Fix login", window, cx))
        })
        .unwrap();
    assert!(ok);
    // A Control share can't type: the item stands dimmed with the rule's words.
    ws.update(cx, |ws, _| super::remote::live(ws, Scope::Control));
    let why = ws.read_with(cx, |ws, _| ws.why_not(Deed::Type));
    assert!(
        why.as_deref()
            .is_some_and(|w| w.starts_with("Needs a Full share: share again on ")),
        "{why:?}"
    );
    let ok = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.remote_send_to(b1, "Fix login", window, cx))
        })
        .unwrap();
    assert!(!ok);
}

#[gpui_kit::test]
fn on_a_mirror_ctrl_z_after_a_card_act_stops_at_the_mark(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = live_board(cx, HOME);
    let (n, b1) = (viewer(&ws, cx, "k1"), viewer(&ws, cx, "b1"));
    // A block moved (held), then a card dropped.
    let moved = Rect::new(500.0, 420.0, 410.0, 310.0);
    let was = ws.read_with(cx, |ws, _| ws.space.canvas.get(b1).unwrap().rect);
    ws.update(cx, |ws, _| {
        ws.space.canvas.get_mut(b1).unwrap().rect = moved;
        ws.remote_moved(&[b1], Instant::now());
        ws.keep(
            crate::workspace::history::Change::Place(vec![(b1, was)]),
            crate::workspace::history::Then::New,
        );
    });
    drop_on(handle, cx, el("card", n, 40), el("card", n, 42));
    ws.update(cx, |ws, _| ws.space.focused = Some(n));
    cx.update_window(handle, |_, window, cx| window.press(UNDO, cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.canvas.get(b1).unwrap().rect,
            moved,
            "no block moved"
        );
        let said = ws.said.last().map(String::as_str).unwrap_or_default();
        assert!(
            said.starts_with("On a mirror, Ctrl+Z undoes moves only: undo the rest on "),
            "{said}"
        );
    });
    cx.update_window(handle, |_, window, cx| window.press(UNDO, cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.canvas.get(b1).unwrap().rect,
            was,
            "the move is undone"
        )
    });
}

// The card dialog on a remote.

use super::card_draft::{click_add, click_on, dialog_open, open_card, press_key};
use crate::board::{Draft, Label};

/// The dialog's work as it is now.
fn work(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Draft {
    ws.read_with(cx, |ws, _| {
        ws.card_work
            .as_ref()
            .expect("a card dialog is open")
            .borrow()
            .draft()
            .clone()
    })
}

/// The mirror's copy of card `card` of board 12.
fn copy_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, card: u64) -> crate::board::Card {
    ws.read_with(cx, |ws, _| {
        remote_of(ws)
            .boards
            .card(12, card)
            .expect("in the copy")
            .clone()
    })
}

/// A comment written in the dialog of a card that isn't made yet: it is in the work, as the
/// field's Comment button puts it (the dialog is taller on a mirror, with its attachments, and
/// the field is out of the test window).
fn comment_in_work(ws: &Entity<Workspace>, cx: &mut TestAppContext, text: &str) {
    ws.update(cx, |ws, _| {
        let id = ws.fresh_id("r1");
        let open = ws.card_work.clone().expect("a dialog is open");
        open.borrow_mut()
            .draft_mut()
            .work
            .comments
            .push(crate::board::Comment {
                id,
                by: "you".into(),
                text: text.into(),
                at: 0,
                edited: false,
            });
    });
}

/// The mirror is live again: an offline one hears its connection fail while the test types.
fn relive(ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    ws.update(cx, |ws, _| {
        let remote = ws.space_by_id_mut("r1").unwrap().remote.as_mut().unwrap();
        remote.status = templum_api::client::Status::Live;
        remote.me = Some(templum_api::model::MeView {
            scope: templum_api::store::Scope::Control,
            ..Default::default()
        });
    });
}

/// Replaces the card's title with `text`.
fn retitle(handle: AnyWindowHandle, cx: &mut TestAppContext, text: &str) {
    click_on(handle, cx, "card-title");
    press_key(handle, cx, "secondary-a");
    cx.simulate_input(handle, text);
}

#[gpui_kit::test]
fn done_on_a_remote_card_sends_only_what_changed_and_shows_it_at_once(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws, _dir) = live_board(cx, HOME);
    open_card(handle, &ws, cx, 12, 40);
    assert_eq!(work(&ws, cx).work.title, "Fix login");
    retitle(handle, cx, "Fixed");
    click_on(handle, cx, "label-blue");
    // Home adds an item meanwhile: the dialog shows it.
    let mut state = board_state(HOME);
    let mut changed = CardView::clone(&state.cards[0]);
    changed.checklist.push(ItemView {
        id: 15,
        text: "Test".into(),
        done: false,
    });
    state.cards[0] = Arc::new(changed);
    apply(handle, &ws, cx, state);
    draw(handle, cx);
    click_on(handle, cx, "card-item-box-13");
    assert!(
        work(&ws, cx).work.checklist.iter().any(|i| i.id == 15),
        "home's item shows"
    );
    relive(&ws, cx);
    click_on(handle, cx, "card-done");
    assert!(!dialog_open(handle, cx));
    assert_eq!(
        sent(&ws, cx),
        [(
            "edit_card".to_string(),
            json!({"card": 40, "title": "Fixed", "labels": {"blue": true}, "check": [13]})
        )]
    );
    let card = copy_of(&ws, cx, 40);
    assert_eq!(card.title, "Fixed");
    assert!(card.labels.contains(&Label::Blue));
    assert_eq!(
        card.checklist.iter().filter(|i| i.id == 15).count(),
        1,
        "once"
    );
    assert!(card.checklist.iter().any(|i| i.id == 13 && i.done));
    ws.read_with(cx, |ws, _| assert!(ws.boards.boards.is_empty()));
    // Home's state showing it ends the hold.
    let mut state = board_state(HOME);
    let mut shown = CardView::clone(&state.cards[0]);
    shown.title = "Fixed".into();
    shown.labels.push("blue".into());
    shown.checklist = vec![
        ItemView {
            id: 13,
            text: "Ship".into(),
            done: true,
        },
        ItemView {
            id: 15,
            text: "Test".into(),
            done: false,
        },
    ];
    state.cards[0] = Arc::new(shown);
    apply(handle, &ws, cx, state);
    ws.read_with(cx, |ws, _| {
        assert!(remote_of(ws).cards_edited.get(&40).is_none())
    });
    assert_eq!(copy_of(&ws, cx, 40).title, "Fixed");
}

#[gpui_kit::test]
fn a_new_remote_card_is_created_then_its_comments_are_sent(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws, dir) = live_board(cx, HOME);
    let n = viewer(&ws, cx, "k1");
    let boards_json = std::fs::read(crate::board::path(dir.path())).ok();
    click_add(handle, cx, n, 30);
    cx.simulate_input(handle, "Login");
    click_on(handle, cx, "label-red");
    click_on(handle, cx, "label-blue");
    comment_in_work(&ws, cx, "First");
    relive(&ws, cx);
    click_on(handle, cx, "card-done");
    assert_eq!(
        sent(&ws, cx),
        [(
            "create_card".to_string(),
            json!({"board": "12", "column": "30", "title": "Login", "labels": ["red", "blue"]})
        )]
    );
    answer(handle, &ws, cx, 0, Ok(json!({"text": "Created card 77."})));
    assert_eq!(
        sent(&ws, cx),
        [(
            "comment_card".to_string(),
            json!({"card": 77, "text": "First"})
        )],
        "the answered call is gone: the next is the comment"
    );
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards.boards.is_empty() && ws.boards.drafts.is_empty())
    });
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    assert_eq!(
        std::fs::read(crate::board::path(dir.path())).ok(),
        boards_json
    );
}

#[gpui_kit::test]
fn a_refused_done_reopens_the_card_with_what_the_person_made(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws, _dir) = live_board(cx, HOME);
    let n = viewer(&ws, cx, "k1");
    click_add(handle, cx, n, 30);
    cx.simulate_input(handle, "Login");
    comment_in_work(&ws, cx, "First");
    relive(&ws, cx);
    click_on(handle, cx, "card-done");
    assert!(!dialog_open(handle, cx));
    answer(handle, &ws, cx, 0, refusal("That column is gone."));
    draw(handle, cx);
    assert_eq!(said(&ws, cx), ["That column is gone."], "said once");
    assert!(dialog_open(handle, cx), "the dialog is open again");
    let again = work(&ws, cx);
    assert_eq!(again.work.title, "Login");
    assert_eq!(
        again
            .work
            .comments
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>(),
        ["First"]
    );
    // Esc, once something changed, asks Save or Discard: a mirror keeps no draft.
    retitle(handle, cx, "Login again");
    press_key(handle, cx, "escape");
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find("leave-save").is_some() && window.try_find("leave-discard").is_some()
        );
        assert!(window.try_find("leave-save-draft").is_none());
    })
    .unwrap();
}

#[gpui_kit::test]
fn comments_on_a_remote_card_are_home_s_calls(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws, _dir) = live_board(cx, HOME);
    open_card(handle, &ws, cx, 12, 40);
    // The comment field, the pencil and the trash of a comment are these acts (the field is out
    // of the test window on a mirror's tall dialog); a deleted comment has no Undo row.
    let act = |cx: &mut TestAppContext, act: BoardAct| {
        relive(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.act_on_boards("r1", act, window, cx))
        })
        .unwrap();
    };
    act(
        cx,
        BoardAct::Comment {
            board: 12,
            card: 40,
            text: "ok".into(),
        },
    );
    act(
        cx,
        BoardAct::EditComment {
            board: 12,
            card: 40,
            comment: 51,
            text: "ok!".into(),
        },
    );
    act(
        cx,
        BoardAct::DeleteComment {
            board: 12,
            card: 40,
            comment: 50,
        },
    );
    act(
        cx,
        BoardAct::RestoreComment {
            board: 12,
            card: 40,
            index: 0,
            comment: crate::board::Comment {
                id: 50,
                by: "b1".into(),
                text: "x".into(),
                at: 0,
                edited: false,
            },
        },
    );
    assert_eq!(
        sent(&ws, cx),
        [
            ("comment_card", json!({"card": 40, "text": "ok"})),
            (
                "edit_comment",
                json!({"card": 40, "comment": 51, "text": "ok!"})
            ),
            ("delete_comment", json!({"card": 40, "comment": 50})),
        ]
        .map(|(tool, args)| (tool.to_string(), args)),
        "an Undo of a deleted comment sends nothing: home's Ctrl+Z brings it back"
    );
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("comment-undo-50").is_none());
    })
    .unwrap();
    // Delete card in the dialog is home's `delete_card`.
    relive(&ws, cx);
    click_on(handle, cx, "delete-card");
    assert_eq!(
        sent(&ws, cx)[3],
        ("delete_card".to_string(), json!({"card": 40}))
    );
    assert!(!dialog_open(handle, cx));
}

#[gpui_kit::test]
fn a_remote_card_s_dialog_keeps_no_draft_here(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws, dir) = live_board(cx, HOME);
    let boards_json = std::fs::read(crate::board::path(dir.path())).ok();
    open_card(handle, &ws, cx, 12, 40);
    retitle(handle, cx, "Draft one");
    // Esc asks: Save and Discard only; Discard sends nothing.
    press_key(handle, cx, "escape");
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("leave-save-draft").is_none());
    })
    .unwrap();
    click_on(handle, cx, "leave-discard");
    assert!(!dialog_open(handle, cx));
    assert!(sent(&ws, cx).is_empty());
    // The window closing keeps no draft of it.
    open_card(handle, &ws, cx, 12, 40);
    retitle(handle, cx, "Draft two");
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    ws.read_with(cx, |ws, _| assert!(ws.boards.drafts.is_empty()));
    assert_eq!(
        std::fs::read(crate::board::path(dir.path())).ok(),
        boards_json
    );
    // Another workspace shown meanwhile (an agent's `show`): Done still goes to its own remote.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx));
    })
    .unwrap();
    relive(&ws, cx);
    click_on(handle, cx, "card-done");
    let calls = sent(&ws, cx);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "edit_card");
    assert_eq!(calls[0].1["card"], 40);
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards.boards.is_empty() && ws.boards.drafts.is_empty());
        assert_eq!(ws.calls[0].0, "r1");
    });
}

#[gpui_kit::test]
fn a_remote_card_shows_its_attachments_by_name_only(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws, dir) = live_board(cx, HOME);
    // A local card 40 has a file of the same name where a local card's are kept.
    let folder = dir.path().join("attachments").join("40");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("shot.png"), b"not home's").unwrap();
    open_card(handle, &ws, cx, 12, 40);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("attachment-shot.png").is_some(), "by name");
        assert!(
            window.try_find("attachment-remove-shot.png").is_none(),
            "no ✕ to remove it"
        );
        assert!(window.try_find("attach-files").is_some());
    })
    .unwrap();
    // Dropping or pasting a file attaches nothing here, and sends nothing.
    let before = std::fs::read_dir(&folder).unwrap().count();
    let open = ws.read_with(cx, |ws, _| ws.card_work.clone().unwrap());
    let dropped = dir.path().join("dropped.txt");
    std::fs::write(&dropped, b"x").unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let from = vec![crate::kanban::Source::File(dropped)];
            crate::kanban::attach(ws, &open, 12, from, window, cx)
        });
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(std::fs::read_dir(&folder).unwrap().count(), before);
    assert_eq!(copy_of(&ws, cx, 40).attachments.len(), 1);
    assert!(work(&ws, cx).work.attachments.len() == 1 && sent(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn done_needing_what_an_older_home_lacks_says_so_and_the_dialog_stays(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws, _dir) = live_board(cx, HOME);
    ws.update(cx, |ws, _| {
        // A home whose `edit_card` takes no `title`: the work can't go as it is.
        let tools = json!([
            { "name": "edit_card", "inputSchema": { "properties": { "card": {}, "priority": {} } } },
        ]);
        ws.space.remote.as_mut().unwrap().tools = Some(Tools::read(&tools));
    });
    open_card(handle, &ws, cx, 12, 40);
    retitle(handle, cx, "Fixed");
    relive(&ws, cx);
    click_on(handle, cx, "card-done");
    assert!(dialog_open(handle, cx), "the dialog stays open");
    assert!(sent(&ws, cx).is_empty(), "nothing is sent");
    let said = said(&ws, cx);
    assert!(
        said.last()
            .is_some_and(|s| s.ends_with("'s Templum is older: update it to do this from here.")),
        "{said:?}"
    );
}

// Two ends: home and a mirror of its own, each with its data folder.

use super::remote::{pump_home, shown_mirror_as};
use crate::board::{Card, Comment};

#[gpui_kit::test]
fn acts_on_a_mirrored_board_leave_the_mirror_s_boards_json_as_it_was(cx: &mut TestAppContext) {
    let ((home_handle, home), (handle, ws), _, [_home_dir, dir]) =
        shown_mirror_as(cx, Scope::Control);
    // Home's kanban shows a board with a card in To do and another in To do.
    let (kanban, board, a) = board_with_a_card(home_handle, &home, cx);
    let (todo, doing) = home.read_with(cx, |ws, _| {
        let columns = &ws.boards.board(board).unwrap().columns;
        (columns[0].id, columns[1].id)
    });
    let b = home
        .update(cx, |ws, _| ws.edit_boards(|b| b.add_card(board, todo)))
        .unwrap();
    let uid = home.read_with(cx, |ws, _| ws.space.uids[&kanban].clone());
    pump_home(&home, cx, "the board on the mirror", |cx| {
        ws.read_with(cx, |ws, _| {
            remote_of(ws).boards.card(board, b).is_some() && remote_of(ws).homes.contains_key(&uid)
        })
    });
    let mirror_kanban = viewer(&ws, cx, &uid);
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let json = crate::board::path(dir.path());
    let before = std::fs::read(&json).ok();
    let act = |cx: &mut TestAppContext, act: BoardAct| {
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.act_on_boards("r1", act, window, cx))
        })
        .unwrap();
    };
    let columns = |cx: &mut TestAppContext| -> Vec<(String, Vec<u64>)> {
        home.read_with(cx, |ws, _| {
            ws.boards
                .board(board)
                .unwrap()
                .columns
                .iter()
                .map(|c| (c.title.clone(), c.cards.iter().map(|c| c.id).collect()))
                .collect()
        })
    };
    act(
        cx,
        BoardAct::MoveCard {
            board,
            card: a,
            column: doing,
            to: None,
        },
    );
    pump_home(&home, cx, "the card moved at home", |cx| {
        columns(cx)[1].1 == [a]
    });
    act(cx, BoardAct::AddColumn { board });
    pump_home(&home, cx, "the new column", |cx| columns(cx).len() == 4);
    let added = home.read_with(cx, |ws, _| ws.boards.board(board).unwrap().columns[3].id);
    act(
        cx,
        BoardAct::RenameColumn {
            board,
            column: added,
            title: "QA".into(),
        },
    );
    pump_home(&home, cx, "the rename", |cx| columns(cx)[3].0 == "QA");
    let mut work = Card::new(0);
    work.title = "Login".into();
    work.comments = vec![Comment {
        id: u64::MAX,
        by: "you".into(),
        text: "First".into(),
        at: 0,
        edited: false,
    }];
    let draft = Draft {
        at: todo,
        new: true,
        base: Card::new(0),
        work,
    };
    act(cx, BoardAct::Finish { board, draft });
    pump_home(&home, cx, "the new card and its comment", |cx| {
        home.read_with(cx, |ws, _| {
            ws.boards.board(board).unwrap().columns[0]
                .cards
                .iter()
                .any(|c| c.title == "Login" && c.comments.len() == 1)
        })
    });
    act(
        cx,
        BoardAct::Comment {
            board,
            card: a,
            text: "noted".into(),
        },
    );
    pump_home(&home, cx, "the comment", |cx| {
        home.read_with(cx, |ws, _| {
            !ws.boards.card(board, a).unwrap().comments.is_empty()
        })
    });
    act(cx, BoardAct::DeleteCard { board, card: b });
    pump_home(&home, cx, "the card gone", |cx| {
        home.read_with(cx, |ws, _| ws.boards.card(board, b).is_none())
    });
    act(
        cx,
        BoardAct::Show {
            block: mirror_kanban,
            board: None,
        },
    );
    pump_home(&home, cx, "a new board shown", |cx| {
        home.read_with(cx, |ws, _| {
            ws.space.kanban(kanban).unwrap().board != Some(board)
        })
    });
    // The mirror wrote nothing of home's: no board, no draft, its file as it was.
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    ws.read_with(cx, |ws, _| {
        assert!(ws.boards.boards.is_empty() && ws.boards.drafts.is_empty())
    });
    assert_eq!(
        std::fs::read(&json).ok(),
        before,
        "boards.json byte for byte"
    );
}
