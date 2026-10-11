//! Kanban boards: columns of cards, every edit, and `boards.json` (see docs/specs/kanban.md).
//! Boards live outside the canvases: any kanban block, in any workspace, shows one by its id.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};

use crate::session::{self, Loaded};

/// The file format's version: raised only when an older Templum couldn't read the new files.
pub(crate) const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Boards {
    pub version: u32,
    /// The last id given out: ids are unique across boards, columns and cards, never reused.
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub boards: Vec<Board>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Board {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub columns: Vec<Column>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Column {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub cards: Vec<Card>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Card {
    pub id: u64,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(
        default,
        deserialize_with = "known_labels",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub labels: Vec<Label>,
}

/// A card's label: one of six colors, drawn with the theme's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Label {
    Red,
    Yellow,
    Green,
    Cyan,
    Blue,
    Magenta,
}

impl Label {
    pub const ALL: [Label; 6] = [
        Label::Red,
        Label::Yellow,
        Label::Green,
        Label::Cyan,
        Label::Blue,
        Label::Magenta,
    ];

    /// Its color in the theme's 16 ANSI colors.
    pub fn ansi(self) -> usize {
        match self {
            Label::Red => 1,
            Label::Green => 2,
            Label::Yellow => 3,
            Label::Blue => 4,
            Label::Magenta => 5,
            Label::Cyan => 6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Label::Red => "red",
            Label::Yellow => "yellow",
            Label::Green => "green",
            Label::Cyan => "cyan",
            Label::Blue => "blue",
            Label::Magenta => "magenta",
        }
    }
}

/// Labels this Templum knows; others (from an edited file) are dropped.
fn known_labels<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Label>, D::Error> {
    let names = Vec::<String>::deserialize(d)?;
    Ok(names
        .iter()
        .filter_map(|n| Label::ALL.into_iter().find(|l| l.name() == n))
        .collect())
}

impl Default for Boards {
    fn default() -> Self {
        Boards {
            version: VERSION,
            next_id: 0,
            boards: Vec::new(),
        }
    }
}

impl Board {
    pub fn card_count(&self) -> usize {
        self.columns.iter().map(|c| c.cards.len()).sum()
    }
}

impl Boards {
    fn next(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub fn board(&self, id: u64) -> Option<&Board> {
        self.boards.iter().find(|b| b.id == id)
    }

    fn board_mut(&mut self, id: u64) -> Option<&mut Board> {
        self.boards.iter_mut().find(|b| b.id == id)
    }

    /// "Board n" with To do, Doing and Done; returns its id.
    pub fn add_board(&mut self) -> u64 {
        let name = (self.boards.len() + 1..)
            .map(|n| format!("Board {n}"))
            .find(|name| !self.boards.iter().any(|b| b.name == *name))
            .expect("an unused name");
        let id = self.next();
        let columns = ["To do", "Doing", "Done"]
            .into_iter()
            .map(|title| Column {
                id: self.next(),
                title: title.into(),
                cards: Vec::new(),
            })
            .collect();
        self.boards.push(Board { id, name, columns });
        id
    }

    pub fn rename_board(&mut self, id: u64, name: &str) {
        if let Some(board) = self.board_mut(id) {
            board.name = name.to_string();
        }
    }

    pub fn remove_board(&mut self, id: u64) {
        self.boards.retain(|b| b.id != id);
    }

    /// "New column" after the last one; returns its id.
    pub fn add_column(&mut self, board: u64) -> Option<u64> {
        let id = self.next();
        let board = self.board_mut(board)?;
        board.columns.push(Column {
            id,
            title: "New column".into(),
            cards: Vec::new(),
        });
        Some(id)
    }

    fn column_mut(&mut self, board: u64, column: u64) -> Option<&mut Column> {
        self.board_mut(board)?
            .columns
            .iter_mut()
            .find(|c| c.id == column)
    }

    pub fn rename_column(&mut self, board: u64, column: u64, title: &str) {
        if let Some(column) = self.column_mut(board, column) {
            column.title = title.to_string();
        }
    }

    /// Removes a column and its cards.
    pub fn remove_column(&mut self, board: u64, column: u64) {
        if let Some(board) = self.board_mut(board) {
            board.columns.retain(|c| c.id != column);
        }
    }

    /// Puts `column` in the place of column `to`.
    pub fn move_column(&mut self, board: u64, column: u64, to: u64) {
        let Some(board) = self.board_mut(board) else {
            return;
        };
        let (Some(from), Some(at)) = (
            board.columns.iter().position(|c| c.id == column),
            board.columns.iter().position(|c| c.id == to),
        ) else {
            return;
        };
        let moved = board.columns.remove(from);
        board.columns.insert(at, moved);
    }

    /// "New card" at the end of `column`; returns its id.
    pub fn add_card(&mut self, board: u64, column: u64) -> Option<u64> {
        let id = self.next();
        self.column_mut(board, column)?.cards.push(Card {
            id,
            title: "New card".into(),
            description: String::new(),
            labels: Vec::new(),
        });
        Some(id)
    }

    pub fn card(&self, board: u64, card: u64) -> Option<&Card> {
        self.board(board)?
            .columns
            .iter()
            .flat_map(|c| &c.cards)
            .find(|c| c.id == card)
    }

    pub fn edit_card(&mut self, board: u64, card: u64, edit: impl FnOnce(&mut Card)) {
        if let Some(card) = self.board_mut(board).and_then(|b| {
            b.columns
                .iter_mut()
                .flat_map(|c| &mut c.cards)
                .find(|c| c.id == card)
        }) {
            edit(card);
        }
    }

    pub fn toggle_label(&mut self, board: u64, card: u64, label: Label) {
        self.edit_card(board, card, |card| {
            if let Some(i) = card.labels.iter().position(|l| *l == label) {
                card.labels.remove(i);
            } else {
                card.labels.push(label);
            }
        });
    }

    pub fn remove_card(&mut self, board: u64, card: u64) {
        if let Some(board) = self.board_mut(board) {
            for column in &mut board.columns {
                column.cards.retain(|c| c.id != card);
            }
        }
    }

    /// Moves `card` into `column`: into the place of card `to`, or at the end without one.
    pub fn move_card(&mut self, board: u64, card: u64, column: u64, to: Option<u64>) {
        let Some(board) = self.board_mut(board) else {
            return;
        };
        let Some(target) = board.columns.iter().position(|c| c.id == column) else {
            return;
        };
        let at = to.and_then(|to| board.columns[target].cards.iter().position(|c| c.id == to));
        let Some(moved) = board.columns.iter_mut().find_map(|c| {
            let i = c.cards.iter().position(|x| x.id == card)?;
            Some(c.cards.remove(i))
        }) else {
            return;
        };
        let cards = &mut board.columns[target].cards;
        let at = at.unwrap_or(cards.len()).min(cards.len());
        cards.insert(at, moved);
    }
}

pub(crate) fn path(dir: &Path) -> PathBuf {
    dir.join("boards.json")
}

pub(crate) fn load(dir: &Path) -> Loaded<Boards> {
    match session::read::<Boards>(&path(dir)) {
        Loaded::Saved(mut boards) => {
            // An edited file's next_id may lag behind its ids: never give one out twice.
            let ids = boards.boards.iter().flat_map(|b| {
                std::iter::once(b.id).chain(
                    b.columns
                        .iter()
                        .flat_map(|c| std::iter::once(c.id).chain(c.cards.iter().map(|x| x.id))),
                )
            });
            boards.next_id = ids.fold(boards.next_id, u64::max);
            Loaded::Saved(boards)
        }
        other => other,
    }
}

/// Writes the boards; like canvases, a write older than the last one done is dropped.
pub(crate) fn save(dir: &Path, boards: &Boards, generation: u64) -> io::Result<()> {
    let text = serde_json::to_string_pretty(boards).map_err(io::Error::other)?;
    session::write_newest(&path(dir), &text, generation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(boards: &Boards, board: u64, column: usize) -> Vec<String> {
        boards.board(board).unwrap().columns[column]
            .cards
            .iter()
            .map(|c| c.title.clone())
            .collect()
    }

    /// A board with cards A, B, C in its first column: (boards, board, column ids, card ids).
    fn abc() -> (Boards, u64, [u64; 3], [u64; 3]) {
        let mut boards = Boards::default();
        let board = boards.add_board();
        let columns: Vec<u64> = boards
            .board(board)
            .unwrap()
            .columns
            .iter()
            .map(|c| c.id)
            .collect();
        let cards = ["A", "B", "C"].map(|title| {
            let id = boards.add_card(board, columns[0]).unwrap();
            boards.edit_card(board, id, |c| c.title = title.to_string());
            id
        });
        (boards, board, columns.try_into().unwrap(), cards)
    }

    #[test]
    fn a_new_board_has_three_columns_and_a_free_name() {
        let mut boards = Boards::default();
        let first = boards.add_board();
        let board = boards.board(first).unwrap();
        assert_eq!(board.name, "Board 1");
        let names: Vec<&str> = board.columns.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(names, ["To do", "Doing", "Done"]);
        boards.rename_board(first, "Board 2");
        let second = boards.add_board();
        assert_eq!(
            boards.board(second).unwrap().name,
            "Board 3",
            "Board 2 is taken"
        );
    }

    #[test]
    fn ids_are_never_reused() {
        let (mut boards, board, columns, cards) = abc();
        let mut seen: Vec<u64> = [board].into_iter().chain(columns).chain(cards).collect();
        boards.remove_board(board);
        let again = boards.add_board();
        seen.push(again);
        seen.extend(boards.board(again).unwrap().columns.iter().map(|c| c.id));
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), seen.len(), "{seen:?}");
    }

    #[test]
    fn a_card_takes_the_place_of_the_card_it_is_dropped_on() {
        let (mut boards, board, columns, [a, b, c]) = abc();
        boards.move_card(board, a, columns[0], Some(c));
        assert_eq!(titles(&boards, board, 0), ["B", "C", "A"], "down: after it");
        boards.move_card(board, a, columns[0], Some(b));
        assert_eq!(titles(&boards, board, 0), ["A", "B", "C"], "up: before it");
        boards.move_card(board, b, columns[1], None);
        assert_eq!(titles(&boards, board, 0), ["A", "C"]);
        assert_eq!(
            titles(&boards, board, 1),
            ["B"],
            "to the end of another column"
        );
        boards.move_card(board, c, columns[1], Some(b));
        assert_eq!(
            titles(&boards, board, 1),
            ["C", "B"],
            "before a card of another column"
        );
        boards.move_card(board, a, columns[0], None);
        assert_eq!(
            titles(&boards, board, 0),
            ["A"],
            "onto its own column's end"
        );
    }

    #[test]
    fn a_column_takes_the_place_of_the_column_it_is_dropped_on() {
        let (mut boards, board, [todo, doing, done], _) = abc();
        boards.move_column(board, todo, done);
        let order: Vec<u64> = boards
            .board(board)
            .unwrap()
            .columns
            .iter()
            .map(|c| c.id)
            .collect();
        assert_eq!(order, [doing, done, todo]);
    }

    #[test]
    fn removing_a_column_removes_its_cards() {
        let (mut boards, board, columns, cards) = abc();
        boards.remove_column(board, columns[0]);
        assert_eq!(boards.board(board).unwrap().columns.len(), 2);
        assert!(boards.card(board, cards[0]).is_none());
        assert_eq!(boards.board(board).unwrap().card_count(), 0);
    }

    #[test]
    fn labels_toggle_and_cards_edit() {
        let (mut boards, board, _, cards) = abc();
        boards.toggle_label(board, cards[1], Label::Green);
        boards.toggle_label(board, cards[1], Label::Red);
        boards.toggle_label(board, cards[1], Label::Green);
        boards.edit_card(board, cards[1], |c| c.description = "why".into());
        let card = boards.card(board, cards[1]).unwrap();
        assert_eq!(card.labels, [Label::Red]);
        assert_eq!(card.description, "why");
        boards.remove_card(board, cards[1]);
        assert!(boards.card(board, cards[1]).is_none());
    }

    #[test]
    fn an_older_write_never_replaces_a_newer_one() {
        let d = tempfile::tempdir().unwrap();
        let (older, _, _, _) = abc();
        let mut newer = older.clone();
        newer.add_board();
        save(d.path(), &newer, 7).unwrap();
        save(d.path(), &older, 6).unwrap();
        assert_eq!(load(d.path()), Loaded::Saved(newer));
    }

    #[test]
    fn boards_read_back_equal_and_unknown_labels_go() {
        let (mut boards, board, _, cards) = abc();
        boards.toggle_label(board, cards[0], Label::Cyan);
        let d = tempfile::tempdir().unwrap();
        save(d.path(), &boards, 1).unwrap();
        assert_eq!(load(d.path()), Loaded::Saved(boards));
        let text = r#"{"version":1,"boards":[{"id":1,"name":"B","columns":[{"id":2,"title":"T",
            "cards":[{"id":3,"title":"x","labels":["green","plaid"]}]}]}]}"#;
        std::fs::write(path(d.path()), text).unwrap();
        let Loaded::Saved(read) = load(d.path()) else {
            panic!("not read");
        };
        assert_eq!(read.boards[0].columns[0].cards[0].labels, [Label::Green]);
        assert_eq!(read.next_id, 3, "missing: after the largest id");
    }
}
