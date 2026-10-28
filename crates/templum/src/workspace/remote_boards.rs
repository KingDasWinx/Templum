//! A remote workspace's copy of home's boards (docs/specs/templum-to-templum.md, Boards): made
//! from home's state where it enters, bounded and cleaned before it is kept or drawn, never
//! saved. A kanban of a mirrored workspace is drawn from it by the kanban's own view.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use gpui_kit::{App, Context, Window};
use serde_json::{Value, json};
use templum_api::client::{Call, Command};
use templum_api::model::{CardView, State};
use templum_canvas::BlockId;

use super::bus::{clean_title, clean_to};
use super::remote::{Deed, Nay, RemoteSpace, Then, clean_text};
use super::{BoardAct, Workspace};
use crate::board::{
    Assignee, Attachment, Board, Boards, Card, Column, Comment, Draft, Label, Priority,
};
use crate::checklists::Item;
use crate::kanban::Kanban;

/// The most boards, columns of a board, cards of a column and attachments of a card the copy keeps.
pub(super) const MOST_BOARDS: usize = 1_000;
pub(super) const MOST_COLUMNS: usize = 100;
pub(super) const MOST_COLUMN_CARDS: usize = 500;
pub(super) const MOST_ATTACHMENTS: usize = 100;
/// A card's title: one line, in characters; its description, a comment's text and a checklist
/// item's text, in characters (a description and a comment keep their line breaks).
pub(super) const CARD_TITLE_CHARS: usize = 300;
pub(super) const DESCRIPTION_CHARS: usize = 65_536;
pub(super) const ITEM_CHARS: usize = 4_000;
pub(super) const COMMENT_CHARS: usize = 16_384;
/// The most checklist items of a card, and the newest comments, the copy keeps.
pub(super) const MOST_CARD_ITEMS: usize = 500;
pub(super) const MOST_COMMENTS: usize = 200;
/// The longest id of a block or an agent home names as a card's assignee, in bytes.
const ASSIGNEE_ID_MOST: usize = 64;

/// An id home names, kept only when it is short and has no control character.
fn plain_id(id: &str) -> Option<String> {
    (id.len() <= ASSIGNEE_ID_MOST && !id.chars().any(char::is_control)).then(|| id.to_string())
}

/// Home's card as the copy keeps it: every word cleaned and capped (a hostile home's card
/// reaches the dialog, the clipboard of Send to and the screen), labels only the six, a priority
/// only a known one, an assignee only a plain id, the newest comments, attachments by name.
pub(super) fn card_of(view: &CardView) -> Card {
    let mut labels = Vec::new();
    for name in &view.labels {
        if let Some(label) = Label::ALL.into_iter().find(|l| l.name() == name)
            && !labels.contains(&label)
        {
            labels.push(label);
        }
    }
    let assignee = view
        .assignee
        .as_ref()
        .and_then(|a| match (&a.block, &a.agent) {
            (Some(block), _) => plain_id(block).map(Assignee::Block),
            (None, Some(agent)) => plain_id(agent).map(Assignee::Agent),
            (None, None) => None,
        });
    let newest = view.comments.len().saturating_sub(MOST_COMMENTS);
    Card {
        id: view.id,
        title: clean_to(&view.title, CARD_TITLE_CHARS),
        description: clean_text(&view.description, DESCRIPTION_CHARS),
        labels,
        priority: view.priority.as_deref().and_then(Priority::named),
        assignee,
        checklist: view
            .checklist
            .iter()
            .take(MOST_CARD_ITEMS)
            .map(|i| Item {
                id: i.id,
                text: clean_to(&i.text, ITEM_CHARS),
                done: i.done,
            })
            .collect(),
        comments: view.comments[newest..]
            .iter()
            .map(|c| Comment {
                id: c.number,
                by: clean_title(&c.by),
                text: clean_text(&c.text, COMMENT_CHARS),
                at: c.at.max(0) as u64,
                edited: c.edited,
            })
            .collect(),
        // Shown by name and size only: never a path this computer opens.
        attachments: view
            .attachments
            .iter()
            .take(MOST_ATTACHMENTS)
            .map(|a| Attachment {
                name: clean_title(&a.name),
                size: a.size,
            })
            .collect(),
    }
}

impl RemoteSpace {
    /// The copy of home's boards, made again from `state`: each board's columns and cards as home
    /// lists them, a card once (at its first place) and only in a column of its own board, and
    /// only a card home's state holds. A card home didn't change since the state this one holds
    /// (the very same `Arc`) is moved over from the old copy as it is, not cleaned again, so this
    /// is called before `state` becomes `self.state`.
    pub(super) fn mirror_boards(&mut self, state: &State, now: Instant) {
        // A card an edit was laid over is cleaned again from home's state, not moved over.
        let laid = std::mem::take(&mut self.laid);
        let mut old: HashMap<u64, Card> = std::mem::take(&mut self.boards)
            .boards
            .into_iter()
            .flat_map(|b| b.columns)
            .flat_map(|c| c.cards)
            .map(|c| (c.id, c))
            .collect();
        let before: HashMap<u64, &Arc<CardView>> =
            self.state.cards.iter().map(|c| (c.id, c)).collect();
        let views: HashMap<u64, &Arc<CardView>> = state.cards.iter().map(|c| (c.id, c)).collect();
        let mut placed = HashSet::new();
        let boards = state
            .boards
            .iter()
            .take(MOST_BOARDS)
            .map(|board| Board {
                id: board.id,
                name: clean_title(&board.name),
                columns: board
                    .columns
                    .iter()
                    .take(MOST_COLUMNS)
                    .map(|column| Column {
                        id: column.id,
                        title: clean_title(&column.title),
                        cards: column
                            .cards
                            .iter()
                            .filter_map(|id| {
                                let view = views.get(id).filter(|v| v.board == board.id)?;
                                placed.insert(*id).then_some(())?;
                                let same = before.get(id).is_some_and(|b| Arc::ptr_eq(b, view))
                                    && !laid.contains(id);
                                Some(match old.remove(id).filter(|_| same) {
                                    Some(card) => card,
                                    None => card_of(view),
                                })
                            })
                            .take(MOST_COLUMN_CARDS)
                            .collect(),
                    })
                    .collect(),
            })
            .collect();
        self.boards = Boards {
            boards,
            ..Boards::default()
        };
        self.lay_holds(state, &views, now);
    }

    /// What was done here and home's state doesn't show yet stands over the copy: a card deleted
    /// is left out, a card moved is put where it was left. Each hold ends when home's state shows
    /// it (the card gone from it, or at the place the person left it), or after `HOLD`.
    fn lay_holds(&mut self, state: &State, views: &HashMap<u64, &Arc<CardView>>, now: Instant) {
        for card in self.cards_deleted.keys() {
            // True when home's state no longer holds the card: the same as what was done here.
            let gone = !views.contains_key(&card);
            if self
                .cards_deleted
                .shown(card, gone, now, |mine, home| mine == home)
                && !gone
            {
                for column in self.boards.boards.iter_mut().flat_map(|b| &mut b.columns) {
                    column.cards.retain(|c| c.id != card);
                }
            }
        }
        for card in self.cards_edited.keys() {
            let Some(draft) = self.cards_edited.get(&card).cloned() else {
                continue;
            };
            let Some(slot) = self
                .boards
                .boards
                .iter_mut()
                .flat_map(|b| &mut b.columns)
                .flat_map(|c| &mut c.cards)
                .find(|c| c.id == card)
            else {
                self.cards_edited.end(&card);
                continue;
            };
            // What the person changed, but not the items they added: home's own come with its
            // state, and a held edit never doubles them.
            let mut held = draft.clone();
            held.work
                .checklist
                .retain(|i| draft.base.checklist.iter().any(|b| b.id == i.id));
            let mut merged = slot.clone();
            held.merge_into(&mut merged);
            // Merging changes nothing of home's card: home shows it.
            let shown = merged == *slot;
            self.cards_edited.shown(card, draft, now, |_, _| shown);
            if self.cards_edited.get(&card).is_some() {
                *slot = merged;
                self.laid.insert(card);
            }
        }
        for card in self.cards_moved.keys() {
            let Some(view) = views.get(&card) else {
                self.cards_moved.end(&card);
                continue;
            };
            let home = home_place(state, view.board, card);
            let (column, before) = self.cards_moved.shown(card, home, now, |a, b| a == b);
            if (column, before) != home {
                self.boards
                    .move_card_before(view.board, card, column, before);
            }
        }
    }
}

/// Where home's state puts `card` of `board`: its column and the card after it (None: the last).
fn home_place(state: &State, board: u64, card: u64) -> (u64, Option<u64>) {
    state
        .boards
        .iter()
        .filter(|b| b.id == board)
        .flat_map(|b| &b.columns)
        .find_map(|column| {
            let at = column.cards.iter().position(|c| *c == card)?;
            Some((column.id, column.cards.get(at + 1).copied()))
        })
        .unwrap_or((0, None))
}

/// Where a drop puts `card` (into `column`, in the place of card `to`, the end without one) as
/// `Boards::move_card` puts it, said as the card that then follows it: `Some(None)` is the end of
/// the column; None when the drop changes nothing. A drop on a card further down its own column
/// lands after it, so "in the place of" is not "before".
pub(super) fn card_before(
    board: &Board,
    card: u64,
    column: u64,
    to: Option<u64>,
) -> Option<Option<u64>> {
    // Only the order matters: a board of bare cards is moved the way the real one would be.
    let order = |board: &Board| -> Vec<Vec<u64>> {
        board
            .columns
            .iter()
            .map(|c| c.cards.iter().map(|c| c.id).collect())
            .collect()
    };
    let mut stand_in = Boards {
        boards: vec![Board {
            id: board.id,
            name: String::new(),
            columns: board
                .columns
                .iter()
                .map(|c| Column {
                    id: c.id,
                    title: String::new(),
                    cards: c.cards.iter().map(|c| Card::new(c.id)).collect(),
                })
                .collect(),
        }],
        ..Boards::default()
    };
    let before = order(&stand_in.boards[0]);
    stand_in.move_card(board.id, card, column, to);
    let after = order(&stand_in.boards[0]);
    if before == after {
        return None;
    }
    let column = after.iter().find(|c| c.contains(&card))?;
    let at = column.iter().position(|c| *c == card)?;
    Some(column.get(at + 1).copied())
}

/// `card_before` for a column dropped in the place of column `to`: the column that then follows
/// it, `Some(None)` for the end; None when nothing changes.
pub(super) fn column_before(board: &Board, column: u64, to: u64) -> Option<Option<u64>> {
    let mut order: Vec<u64> = board.columns.iter().map(|c| c.id).collect();
    let (from, at) = (
        order.iter().position(|c| *c == column)?,
        order.iter().position(|c| *c == to)?,
    );
    if from == at {
        return None;
    }
    let moved = order.remove(from);
    order.insert(at, moved);
    Some(order.get(at + 1).copied())
}

/// Who a card is for, as home's `for` says it: a block's id, an agent's id, or `anyone`.
fn for_whom(assignee: &Option<Assignee>) -> String {
    match assignee {
        Some(Assignee::Block(id) | Assignee::Agent(id)) => id.clone(),
        None => "anyone".into(),
    }
}

/// `edit_card`'s arguments for only what the person changed between `draft.base` and
/// `draft.work` (what home applies as Done does here: the rest stays as home has it); None when
/// nothing changed.
pub(super) fn edit_args(card: u64, draft: &Draft) -> Option<Value> {
    let (base, work) = (&draft.base, &draft.work);
    let mut args = serde_json::Map::new();
    if work.title != base.title {
        args.insert("title".into(), json!(work.title));
    }
    if work.description != base.description {
        args.insert("description".into(), json!(work.description));
    }
    let labels: serde_json::Map<String, Value> = Label::ALL
        .into_iter()
        .filter(|l| base.labels.contains(l) != work.labels.contains(l))
        .map(|l| (l.name().to_string(), json!(work.labels.contains(&l))))
        .collect();
    if !labels.is_empty() {
        args.insert("labels".into(), Value::Object(labels));
    }
    if work.priority != base.priority {
        args.insert(
            "priority".into(),
            json!(work.priority.map_or("none", |p| p.name())),
        );
    }
    if work.assignee != base.assignee {
        args.insert("for".into(), json!(for_whom(&work.assignee)));
    }
    let added: Vec<String> = work
        .checklist
        .iter()
        .filter(|i| base.checklist.iter().all(|b| b.id != i.id))
        .map(|i| format!("[{}] {}", if i.done { "x" } else { " " }, i.text))
        .collect();
    if !added.is_empty() {
        args.insert("add_items".into(), json!(added.join("\n")));
    }
    let toggled = |done: bool| -> Vec<u64> {
        base.checklist
            .iter()
            .filter(|b| b.done != done)
            .filter(|b| {
                work.checklist
                    .iter()
                    .any(|w| w.id == b.id && w.done == done)
            })
            .map(|b| b.id)
            .collect()
    };
    let removed: Vec<u64> = base
        .checklist
        .iter()
        .filter(|b| work.checklist.iter().all(|w| w.id != b.id))
        .map(|b| b.id)
        .collect();
    for (key, ids) in [
        ("remove_items", removed),
        ("check", toggled(true)),
        ("uncheck", toggled(false)),
    ] {
        if !ids.is_empty() {
            args.insert(key.into(), json!(ids));
        }
    }
    if args.is_empty() {
        return None;
    }
    args.insert("card".into(), json!(card));
    Some(Value::Object(args))
}

/// `create_card`'s arguments for a card not made yet: its board, its column (`draft.at`), its
/// title and what is set of the rest. Its comments are sent after, once home says its number.
pub(super) fn create_args(board: u64, draft: &Draft) -> Value {
    let work = &draft.work;
    let mut args = json!({
        "board": board.to_string(),
        "column": draft.at.to_string(),
        "title": work.title,
    });
    if !work.description.is_empty() {
        args["description"] = json!(work.description);
    }
    if let Some(priority) = work.priority {
        args["priority"] = json!(priority.name());
    }
    if work.assignee.is_some() {
        args["for"] = json!(for_whom(&work.assignee));
    }
    if !work.labels.is_empty() {
        let names: Vec<&str> = work.labels.iter().map(|l| l.name()).collect();
        args["labels"] = json!(names);
    }
    if !work.checklist.is_empty() {
        let lines: Vec<String> = work
            .checklist
            .iter()
            .map(|i| format!("[{}] {}", if i.done { "x" } else { " " }, i.text))
            .collect();
        args["checklist"] = json!(lines.join("\n"));
    }
    args
}

/// The number of the card home made, from its words "Created card 77.".
pub(super) fn created_card(answer: &Value) -> Option<u64> {
    answer["text"]
        .as_str()?
        .strip_prefix("Created card ")?
        .strip_suffix('.')?
        .parse()
        .ok()
}

/// What an act of a person's does at home: the deed it asks the rule about, the call, the card
/// it holds over home's state, and where the hold goes.
struct Doing {
    deed: Deed,
    tool: &'static str,
    args: Value,
    hold: Option<Hold>,
    /// A mark in Ctrl+Z's stack (nothing at home undoes it, as showing a board isn't).
    mark: bool,
}

/// A card held over home's state until it shows it.
enum Hold {
    Moved(u64, (u64, Option<u64>)),
    Deleted(u64),
}

impl Hold {
    fn card(&self) -> u64 {
        match self {
            Hold::Moved(card, _) | Hold::Deleted(card) => *card,
        }
    }
}

impl Workspace {
    /// A person's act on a board of the mirrored workspace `space`: home's tool does it, asked of
    /// the rule first (a refusal says its words once, and nothing is sent). A card moved or
    /// deleted shows at once and stands over home's state until it shows it; everything else
    /// shows when home's state comes. Nothing is written here.
    pub(super) fn remote_board_act(
        &mut self,
        space: &str,
        act: BoardAct,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // (`act_on_boards` has checked the Boards dialog's workspace is the one shown; a card's
        // dialog acts on its own remote by id.)
        if let BoardAct::Finish { board, draft } = act {
            return self.remote_finish(space, board, draft, window, cx);
        }
        let Some(doing) = self.board_doing(space, &act) else {
            return;
        };
        let Some(remote) = self.space_named(space).and_then(|s| s.remote.as_deref()) else {
            return;
        };
        if let Some(nay) = remote.why_not(doing.deed) {
            let words = nay.words(&crate::mirror::computer_of(remote));
            return self.warn(words, window, cx);
        }
        let now = Instant::now();
        let held = doing.hold.as_ref().map(Hold::card);
        if let Some(remote) = self.remote_mut(space) {
            match doing.hold {
                Some(Hold::Moved(card, place)) => remote.cards_moved.hold(card, place, now),
                Some(Hold::Deleted(card)) => remote.cards_deleted.hold(card, true, now),
                None => {}
            }
        }
        if held.is_some() {
            self.remirror(space, now);
        }
        let call = Call {
            tool: doing.tool.into(),
            args: doing.args,
            slow: false,
        };
        let space_id = space.to_string();
        let then: Then = Box::new(move |ws, answered, window, cx| {
            let Err(refusal) = answered else {
                return;
            };
            // Home's state is the truth: what stood over it goes, and its last state is drawn.
            if let (Some(card), Some(remote)) = (held, ws.remote_mut(&space_id)) {
                remote.cards_moved.end(&card);
                remote.cards_deleted.end(&card);
            }
            ws.remirror(&space_id, Instant::now());
            ws.warn(refusal.sentence, window, cx);
        });
        self.remote_call(space, call, then, cx);
        if doing.mark && self.space.id == space {
            self.mark_remote();
        }
    }

    /// The call an act is, for the remote workspace `space`; None when it is none (a drop that
    /// changes nothing, a block home doesn't know, a draft: a mirror keeps none).
    fn board_doing(&self, space: &str, act: &BoardAct) -> Option<Doing> {
        let remote = self.space_named(space)?.remote.as_deref()?;
        let board_of = |board: u64| remote.boards.board(board);
        let doing = |deed, tool, args| Doing {
            deed,
            tool,
            args,
            hold: None,
            mark: true,
        };
        Some(match act {
            BoardAct::MoveCard {
                board,
                card,
                column,
                to,
            } => {
                let before = card_before(board_of(*board)?, *card, *column, *to)?;
                // An older home's `move_card` has no `before`: the card goes to the column's end.
                let before = before.filter(|_| remote.takes("move_card", "before"));
                let mut args = json!({ "card": card, "column": column.to_string() });
                if let Some(before) = before {
                    args["before"] = json!(before);
                }
                Doing {
                    hold: Some(Hold::Moved(*card, (*column, before))),
                    ..doing(Deed::MoveCard, "move_card", args)
                }
            }
            BoardAct::MoveColumn { board, column, to } => {
                let before = column_before(board_of(*board)?, *column, *to)?;
                let before = before.map_or(json!("end"), |c| json!(c));
                let args =
                    json!({ "board": board.to_string(), "column": column, "before": before });
                doing(Deed::EditColumn, "edit_column", args)
            }
            BoardAct::AddColumn { board } => doing(
                Deed::EditColumn,
                "edit_column",
                json!({ "board": board.to_string() }),
            ),
            BoardAct::RenameColumn {
                board,
                column,
                title,
            } => doing(
                Deed::EditColumn,
                "edit_column",
                json!({ "board": board.to_string(), "column": column, "title": title }),
            ),
            BoardAct::DeleteColumn { board, column } => doing(
                Deed::EditColumn,
                "edit_column",
                json!({ "board": board.to_string(), "column": column, "delete": true }),
            ),
            BoardAct::RenameBoard { board, name } => doing(
                Deed::EditBoard,
                "edit_board",
                json!({ "board": board.to_string(), "name": name }),
            ),
            BoardAct::DeleteBoard { board } => doing(
                Deed::EditBoard,
                "edit_board",
                json!({ "board": board.to_string(), "delete": true }),
            ),
            BoardAct::Show { block, board } => {
                let home = remote.home_of(*block)?;
                let args = match board {
                    Some(board) => json!({ "block": home, "board": board.to_string() }),
                    None => json!({ "block": home, "new": "" }),
                };
                Doing {
                    mark: false,
                    ..doing(Deed::ShowBoard, "show_board", args)
                }
            }
            BoardAct::DeleteCard { card, .. } => Doing {
                hold: Some(Hold::Deleted(*card)),
                ..doing(Deed::DeleteCard, "delete_card", json!({ "card": card }))
            },
            BoardAct::Comment { card, text, .. } => doing(
                Deed::CommentCard,
                "comment_card",
                json!({ "card": card, "text": text }),
            ),
            BoardAct::EditComment {
                card,
                comment,
                text,
                ..
            } => doing(
                Deed::EditComment,
                "edit_comment",
                json!({ "card": card, "comment": comment, "text": text }),
            ),
            BoardAct::DeleteComment { card, comment, .. } => doing(
                Deed::DeleteComment,
                "delete_comment",
                json!({ "card": card, "comment": comment }),
            ),
            // Done is `remote_finish`'s. A mirror keeps no draft, and a deleted comment's Undo is
            // home's Ctrl+Z.
            BoardAct::Finish { .. }
            | BoardAct::RestoreComment { .. }
            | BoardAct::KeepDraft(_)
            | BoardAct::DropDraft(_) => return None,
        })
    }

    /// Why Done on `draft` can't be done on the mirrored workspace `space`, in words: the rule
    /// says no to the deed, or home's `edit_card` lacks an argument the work needs. None for a
    /// local workspace, and when it can.
    pub(crate) fn finish_refusal(&self, space: &str, board: u64, draft: &Draft) -> Option<String> {
        let remote = self.space_named(space)?.remote.as_deref()?;
        let computer = crate::mirror::computer_of(remote);
        let deed = if draft.new {
            Deed::CreateCard
        } else {
            Deed::EditCard
        };
        if let Some(nay) = remote.why_not(deed) {
            return Some(nay.words(&computer));
        }
        let args = if draft.new {
            create_args(board, draft)
        } else {
            edit_args(draft.at, draft)?
        };
        let tool = if draft.new {
            "create_card"
        } else {
            "edit_card"
        };
        let lacks = args
            .as_object()
            .into_iter()
            .flatten()
            .any(|(key, _)| key != "card" && !remote.takes(tool, key));
        lacks.then(|| Nay::Older.words(&computer))
    }

    /// Why `deed` can't be done on the mirrored workspace `space`, in words.
    pub(crate) fn why_not_in(&self, space: &str, deed: Deed) -> Option<String> {
        let remote = self.space_named(space)?.remote.as_deref()?;
        let nay = remote.why_not(deed)?;
        Some(nay.words(&crate::mirror::computer_of(remote)))
    }

    /// Done on a card's dialog of the mirrored workspace `space`: home's `edit_card` with only
    /// what the person changed (shown at once, standing over home's state until it shows it), or
    /// its `create_card`, then a `comment_card` for each comment of the work once home says the
    /// card's number. A refusal says its words and gives the person's work back in the dialog:
    /// a mirror keeps no draft.
    fn remote_finish(
        &mut self,
        space: &str,
        board: u64,
        draft: Draft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(words) = self.finish_refusal(space, board, &draft) {
            return self.warn(words, window, cx);
        }
        let (tool, args) = if draft.new {
            ("create_card", create_args(board, &draft))
        } else {
            match edit_args(draft.at, &draft) {
                Some(args) => ("edit_card", args),
                None => return,
            }
        };
        let now = Instant::now();
        let card = draft.at;
        if !draft.new
            && let Some(remote) = self.remote_mut(space)
        {
            remote.cards_edited.hold(card, draft.clone(), now);
            self.remirror(space, now);
        }
        let space_id = space.to_string();
        let then: Then = Box::new(move |ws, answered, window, cx| match answered {
            Err(refusal) => {
                if let Some(remote) = ws.remote_mut(&space_id) {
                    remote.cards_edited.end(&card);
                }
                ws.remirror(&space_id, Instant::now());
                ws.warn(refusal.sentence, window, cx);
                // Its dialog is closed: the person's work comes back in a new one.
                if ws.card_work.is_none() {
                    crate::kanban::reopen_card(ws, &space_id, board, draft, window, cx);
                }
            }
            Ok(answer) => {
                if !draft.new {
                    return;
                }
                let Some(made) = created_card(&answer) else {
                    return;
                };
                for comment in &draft.work.comments {
                    let call = Call {
                        tool: "comment_card".into(),
                        args: json!({ "card": made, "text": comment.text }),
                        slow: false,
                    };
                    let then: Then = Box::new(|ws, answered, window, cx| {
                        if let Err(refusal) = answered {
                            ws.warn(refusal.sentence, window, cx);
                        }
                    });
                    ws.remote_call(&space_id, call, then, cx);
                }
            }
        });
        let call = Call {
            tool: tool.into(),
            args,
            slow: false,
        };
        self.remote_call(space, call, then, cx);
        if self.space.id == space {
            self.mark_remote();
        }
    }

    /// The sessions a card of the mirrored workspace `remote` may be given to: home's terminals
    /// that take input, by home's id of each and its title.
    pub(crate) fn home_sessions(&self, remote: &RemoteSpace) -> Vec<(String, String)> {
        let mut found: Vec<(BlockId, String, String)> = remote
            .homes
            .iter()
            .filter_map(|(home, id)| {
                let pane = self
                    .spaces()
                    .find(|s| s.remote.as_deref().is_some_and(|r| std::ptr::eq(r, remote)))?
                    .terminal(*id)?;
                let viewer = pane.remote.as_ref().filter(|r| !r.read_only)?;
                Some((*id, home.clone(), viewer.title.clone()))
            })
            .collect();
        found.sort_by_key(|(id, _, _)| id.0);
        found
            .into_iter()
            .map(|(_, home, title)| (home, title))
            .collect()
    }

    /// The terminals of the mirrored workspace shown that take input, by home's title for each:
    /// what a card's Send to ▸ lists.
    pub(crate) fn remote_terminals(&self) -> Vec<(BlockId, String)> {
        let mut found: Vec<(BlockId, String)> = self
            .space
            .blocks
            .iter()
            .filter_map(|(id, body)| {
                let remote = body.terminal()?.remote.as_ref()?;
                (!remote.read_only).then(|| (*id, remote.title.clone()))
            })
            .collect();
        found.sort_by_key(|(id, _)| id.0);
        found
    }

    /// Types `text` into the remote terminal `to` for the person to send (home pastes it, never
    /// presses Enter) and shows it. False when nothing was sent: the rule says no (said once), or
    /// the text is more than a mirrored terminal takes.
    pub(crate) fn remote_send_to(
        &mut self,
        to: BlockId,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some((space, block)) = self.typing_into(to, window, cx) else {
            return false;
        };
        if text.len() > templum_mcp::tools::MAX_TEXT {
            self.warn(
                "Send at most 256 KiB into a mirrored terminal.".into(),
                window,
                cx,
            );
            return false;
        }
        if let Some(pane) = self.space.terminal(to)
            && pane.term.scrolled_back()
        {
            pane.term.scroll(-(templum_terminal::VIEWER_HISTORY as i32));
        }
        self.mirror_send(
            &space,
            Command::Paste {
                block,
                text: text.to_string(),
            },
        );
        self.reveal(&space, to, window, cx);
        true
    }
}

impl Workspace {
    /// The boards a kanban draws: this computer's for a local one; a mirrored workspace's copy of
    /// home's for a remote one, which is drawn only while its workspace is shown (None else).
    pub(crate) fn boards_of(&self, kanban: &Kanban) -> Option<&Boards> {
        if kanban.remote {
            self.space.remote.as_deref().map(|r| &r.boards)
        } else {
            Some(&self.boards)
        }
    }

    /// The boards of workspace `space`: this computer's, or a mirrored workspace's copy.
    pub(crate) fn boards_in(&self, space: &str) -> Option<&Boards> {
        match self.space_named(space)?.remote.as_deref() {
            Some(remote) => Some(&remote.boards),
            None => Some(&self.boards),
        }
    }

    /// The title agents and cards show for block `uid` of workspace `space`: on this computer the
    /// block of that id in any workspace; on a mirrored workspace the block home calls `uid`.
    pub(crate) fn uid_title_in(&self, space: &str, uid: &str, cx: &App) -> Option<String> {
        let space = self.space_named(space)?;
        let Some(remote) = space.remote.as_deref() else {
            return self.uid_title(uid, cx);
        };
        let id = remote.homes.get(uid)?;
        Some(crate::block::title(self, space.blocks.get(id)?, cx).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use templum_api::model::{
        Assignee as HomeAssignee, AttachmentView, BoardView, ColumnView, CommentView, ItemView,
    };

    use crate::session::RemoteEntry;

    fn hostile() -> CardView {
        CardView {
            id: 40,
            board: 12,
            column: 30,
            title: format!("a\x1b[31m\nb{}", "t".repeat(1000)),
            description: format!("x\r\n\x07y{}", "d".repeat(100_000)),
            labels: ["red", "purple", "red"].map(String::from).to_vec(),
            priority: Some("urgent".into()),
            assignee: Some(HomeAssignee {
                block: Some("b1\n".into()),
                agent: None,
            }),
            checklist: (0..600)
                .map(|id| ItemView {
                    id,
                    text: "i".repeat(5000),
                    done: false,
                })
                .collect(),
            comments: (0..300)
                .map(|number| CommentView {
                    number,
                    by: "b1\nx".into(),
                    text: "c".repeat(20_000),
                    at: 5,
                    edited: false,
                })
                .collect(),
            attachments: (0..150)
                .map(|n| AttachmentView {
                    name: if n == 0 {
                        "../../.ssh/id_ed25519".into()
                    } else {
                        format!("f{n}.png")
                    },
                    size: 3,
                })
                .collect(),
        }
    }

    #[test]
    fn home_s_cards_are_cleaned_and_capped() {
        let card = card_of(&hostile());
        assert!(!card.title.contains(['\n', '\x1b']) && card.title.chars().count() <= 300);
        assert!(card.title.starts_with("a[31m b"), "{:?}", card.title);
        assert!(card.description.contains('\n'));
        assert!(!card.description.contains(['\r', '\x07']));
        assert_eq!(card.description.chars().count(), 65_536);
        assert_eq!(card.checklist.len(), 500);
        assert!(
            card.checklist
                .iter()
                .all(|i| i.text.chars().count() <= 4000)
        );
        assert_eq!(card.comments.len(), 200);
        assert_eq!(card.comments.last().unwrap().id, 299, "the newest are kept");
        assert!(
            card.comments
                .iter()
                .all(|c| c.text.chars().count() <= 16_384)
        );
        assert!(card.comments.iter().all(|c| !c.by.contains('\n')));
        assert_eq!(card.labels, [Label::Red]);
        assert_eq!(card.priority, None);
        assert_eq!(card.assignee, None);
        assert_eq!(card.attachments.len(), 100);
        assert!(card.attachments.iter().all(|a| !a.name.contains('\n')));
        // Shown by its name, one line; nothing here ever opens it.
        assert_eq!(card.attachments[0].name, "../../.ssh/id_ed25519");
        // A plain assignee stays.
        let mut view = hostile();
        view.assignee = Some(HomeAssignee {
            block: None,
            agent: Some("codex".into()),
        });
        assert_eq!(
            card_of(&view).assignee,
            Some(Assignee::Agent("codex".into()))
        );
    }

    /// A state with board 12 holding the cards `listed`, in its columns 30..; `views` are the
    /// cards home's state holds, each with its board.
    fn state(columns: Vec<Vec<u64>>, views: Vec<(u64, u64)>) -> State {
        State {
            boards: vec![Arc::new(BoardView {
                id: 12,
                name: "Sprint".into(),
                columns: columns
                    .into_iter()
                    .enumerate()
                    .map(|(n, cards)| ColumnView {
                        id: 30 + n as u64,
                        title: format!("c{n}"),
                        cards,
                    })
                    .collect(),
            })],
            cards: views
                .into_iter()
                .map(|(id, board)| {
                    Arc::new(CardView {
                        id,
                        board,
                        title: format!("card {id}"),
                        ..CardView::default()
                    })
                })
                .collect(),
            ..State::default()
        }
    }

    #[test]
    fn a_board_keeps_its_columns_and_cards_each_once_and_capped() {
        let mut columns = vec![vec![40, 40, 999]];
        columns[0].extend(100..700);
        columns.push(vec![41]);
        columns.push(vec![41, 77]);
        columns.extend((0..147).map(|_| Vec::new()));
        let mut views = vec![(40, 12), (41, 12), (77, 99)];
        views.extend((100..700).map(|id| (id, 12)));
        let mut remote = RemoteSpace::new(RemoteEntry::default());
        remote.mirror_boards(&state(columns, views), Instant::now());
        let board = &remote.boards.boards[0];
        assert_eq!(board.columns.len(), 100);
        let first: Vec<u64> = board.columns[0].cards.iter().map(|c| c.id).collect();
        assert_eq!(first.len(), 500);
        assert_eq!(first.iter().filter(|id| **id == 40).count(), 1, "40 once");
        assert!(!first.contains(&999), "an id home's state doesn't hold");
        let second: Vec<u64> = board.columns[1].cards.iter().map(|c| c.id).collect();
        assert_eq!(second, [41], "41 at its first place");
        assert!(
            board.columns[2].cards.is_empty(),
            "41 is not here, nor a stranger's 77"
        );
        // The boards themselves are capped.
        let mut many = state(Vec::new(), Vec::new());
        let one = many.boards[0].clone();
        many.boards = (0..1_200)
            .map(|id| {
                Arc::new(BoardView {
                    id,
                    ..(*one).clone()
                })
            })
            .collect();
        remote.mirror_boards(&many, Instant::now());
        assert_eq!(remote.boards.boards.len(), MOST_BOARDS);
    }

    /// A board whose columns hold the cards named by their ids.
    fn board_of(columns: &[(u64, &[u64])]) -> Board {
        Board {
            id: 12,
            name: String::new(),
            columns: columns
                .iter()
                .map(|(id, cards)| Column {
                    id: *id,
                    title: String::new(),
                    cards: cards.iter().map(|c| Card::new(*c)).collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn card_before_is_the_card_that_follows_where_the_drop_puts_it() {
        // To do [1, 2], Doing [3, 4, 5].
        let board = board_of(&[(30, &[1, 2]), (31, &[3, 4, 5])]);
        // Down its own column the card lands after the one it was dropped on.
        assert_eq!(card_before(&board, 4, 31, Some(5)), Some(None));
        assert_eq!(card_before(&board, 5, 31, Some(3)), Some(Some(3)));
        // From another column it takes the place of the card.
        assert_eq!(card_before(&board, 2, 31, Some(4)), Some(Some(4)));
        // On the column's empty space: the end.
        assert_eq!(card_before(&board, 2, 31, None), Some(None));
        assert_eq!(card_before(&board, 1, 31, None), Some(None));
        // Dropped on itself, or the last one on the end: nothing changes.
        assert_eq!(card_before(&board, 4, 31, Some(4)), None);
        assert_eq!(card_before(&board, 5, 31, None), None);
    }

    #[test]
    fn column_before_is_the_column_that_follows_where_the_drop_puts_it() {
        let board = board_of(&[(30, &[]), (31, &[]), (32, &[])]);
        // Done onto To do: it goes first, before To do.
        assert_eq!(column_before(&board, 32, 30), Some(Some(30)));
        // To do onto Done: down, so it lands last.
        assert_eq!(column_before(&board, 30, 32), Some(None));
        assert_eq!(column_before(&board, 30, 31), Some(Some(32)));
        assert_eq!(column_before(&board, 31, 31), None);
        assert_eq!(column_before(&board, 31, 99), None);
    }

    fn item(id: u64, text: &str, done: bool) -> Item {
        Item {
            id,
            text: text.into(),
            done,
        }
    }

    #[test]
    fn edit_args_say_only_what_changed() {
        let mut base = Card::new(40);
        base.title = "A".into();
        base.labels = vec![Label::Red];
        base.checklist = vec![item(13, "open", false), item(14, "done", true)];
        let mut draft = Draft {
            at: 40,
            new: false,
            base: base.clone(),
            work: base.clone(),
        };
        assert_eq!(edit_args(40, &draft), None, "nothing changed");
        draft.work.title = "B".into();
        draft.work.labels = vec![Label::Red, Label::Blue];
        draft.work.checklist = vec![item(13, "open", true), item(u64::MAX, "ship", false)];
        draft.work.priority = Some(Priority::High);
        draft.work.assignee = Some(Assignee::Block("b1".into()));
        assert_eq!(
            edit_args(40, &draft),
            Some(json!({
                "card": 40, "title": "B", "labels": {"blue": true}, "add_items": "[ ] ship",
                "remove_items": [14], "check": [13], "priority": "high", "for": "b1",
            }))
        );
        // Back to anyone, a blank description, a label off, an item unchecked.
        draft.base.assignee = Some(Assignee::Agent("codex".into()));
        draft.work.assignee = None;
        draft.base.description = "d".into();
        draft.work.description = String::new();
        draft.work.labels = vec![];
        draft.work.checklist = vec![item(13, "open", false), item(14, "done", false)];
        draft.work.priority = None;
        assert_eq!(
            edit_args(40, &draft),
            Some(json!({
                "card": 40, "title": "B", "description": "", "labels": {"red": false},
                "uncheck": [14], "for": "anyone",
            }))
        );
    }

    #[test]
    fn a_new_card_s_arguments_carry_what_it_has_and_home_s_answer_names_it() {
        let mut work = Card::new(0);
        work.title = "Login".into();
        work.labels = vec![Label::Red, Label::Blue];
        work.checklist = vec![item(u64::MAX, "a", true), item(u64::MAX - 1, "b", false)];
        let draft = Draft {
            at: 30,
            new: true,
            base: Card::new(0),
            work,
        };
        assert_eq!(
            create_args(12, &draft),
            json!({
                "board": "12", "column": "30", "title": "Login", "labels": ["red", "blue"],
                "checklist": "[x] a\n[ ] b",
            })
        );
        assert_eq!(created_card(&json!({"text": "Created card 77."})), Some(77));
        assert_eq!(created_card(&json!({"text": "Made card 77."})), None);
        assert_eq!(created_card(&json!({"text": "Created card x."})), None);
        assert_eq!(created_card(&json!({})), None);
    }

    #[test]
    fn a_card_home_didn_t_change_is_not_cleaned_again() {
        let mut remote = RemoteSpace::new(RemoteEntry::default());
        let mut first = state(vec![vec![40, 41]], vec![(40, 12), (41, 12)]);
        first.cards[0] = Arc::new(CardView {
            description: "kept text".into(),
            ..CardView::clone(&first.cards[0])
        });
        remote.mirror_boards(&first, Instant::now());
        remote.state = first.clone();
        let described = |remote: &RemoteSpace, id: u64| {
            remote.boards.boards[0].columns[0]
                .cards
                .iter()
                .find(|c| c.id == id)
                .unwrap()
                .description
                .as_ptr()
        };
        let before = described(&remote, 40);
        let mut next = first.clone();
        next.cards[0].clone_from(&first.cards[0]); // the same Arc
        next.cards[1] = Arc::new(CardView {
            id: 41,
            board: 12,
            title: "new".into(),
            description: "home's new text".into(),
            ..CardView::default()
        });
        remote.mirror_boards(&next, Instant::now());
        assert_eq!(described(&remote, 40), before, "moved over as it was");
        let card = &remote.boards.boards[0].columns[0].cards[1];
        assert_eq!(
            (card.title.as_str(), card.description.as_str()),
            ("new", "home's new text")
        );
    }
}
