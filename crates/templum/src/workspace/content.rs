//! The content tools (docs/specs/connections.md, Tools, batch 4): the cards of a kanban wired to the
//! caller, a wired note, a wired checklist, a file opened in an editor.

use std::path::Path;

use gpui_kit::component::input::{Position, RopeExt as _};
use gpui_kit::{App, Context, Window};
use templum_mcp::{Reply, tools};

use super::bus::{flat, target};
use super::ops::{NewBlock, Place};
use super::{BlockId, Body, Space, Workspace};
use crate::board::{Assignee, Card};
use crate::checklists;
use crate::editor::{Content, Editor};

/// The caller as cards name it: its block id, and the agent it runs.
pub(super) type Caller = (String, Option<String>);

fn caller(space: &Space, me: BlockId) -> Caller {
    let uid = space.uids.get(&me).cloned().unwrap_or_default();
    let agent = space
        .terminal(me)
        .and_then(|p| p.agent())
        .map(str::to_string);
    (uid, agent)
}

impl Workspace {
    /// The caller's workspace and block: `call` found it.
    fn caller_space(&self, from: &str) -> (&Space, BlockId) {
        let (space_id, me) = self.caller(from).expect("call found the caller");
        (
            self.space_named(&space_id).expect("the caller's workspace"),
            me,
        )
    }

    /// The boards kanban blocks wired to `me` show, back to front, each once.
    fn wired_boards(&self, space: &Space, me: BlockId) -> Vec<u64> {
        let mut boards = Vec::new();
        for b in space.canvas.blocks() {
            if b.id != me
                && space.wired(me, b.id)
                && let Some(id) = space.kanban(b.id).and_then(|k| k.board)
                && self.boards.board(id).is_some()
                && !boards.contains(&id)
            {
                boards.push(id);
            }
        }
        boards
    }

    fn board_names(&self, boards: &[u64]) -> String {
        let names: Vec<String> = boards
            .iter()
            .filter_map(|b| self.boards.board(*b))
            .map(|b| format!("\"{}\" ({})", flat(&b.name), b.id))
            .collect();
        if names.is_empty() {
            "none".into()
        } else {
            names.join(", ")
        }
    }

    /// The board `name` names among those `me` may use; the only one when it is left out.
    fn board_for(&self, space: &Space, me: BlockId, name: Option<&str>) -> Result<u64, Reply> {
        let boards = self.wired_boards(space, me);
        match name {
            None => match boards.as_slice() {
                [one] => Ok(*one),
                [] => Err(Reply::err(
                    "No kanban connected to you shows a board. Ask the person with connect, or have \
                     them pick one.",
                )),
                _ => Err(Reply::err(format!(
                    "Several boards are connected to you: {}. Name one with board.",
                    self.board_names(&boards)
                ))),
            },
            Some(name) => {
                let lower = name.to_lowercase();
                boards
                    .iter()
                    .copied()
                    .find(|b| b.to_string() == name)
                    .or_else(|| {
                        boards.iter().copied().find(|b| {
                            self.boards
                                .board(*b)
                                .is_some_and(|x| x.name.trim().to_lowercase() == lower)
                        })
                    })
                    .ok_or_else(|| {
                        Reply::err(format!(
                            "No kanban connected to you shows board {name}. Connected boards: {}.",
                            self.board_names(&boards)
                        ))
                    })
            }
        }
    }

    /// The board holding `card`, when a kanban wired to `me` shows it. An unknown card and one
    /// elsewhere get the same answer.
    fn card_board(&self, space: &Space, me: BlockId, card: u64) -> Result<u64, Reply> {
        match self.boards.find_card(card) {
            Some((board, _)) if self.wired_boards(space, me).contains(&board) => Ok(board),
            _ => Err(Reply::err(format!(
                "No kanban connected to you has card {card}: list_cards lists them."
            ))),
        }
    }

    /// `b7k2m "Claude Code · api"`, `you`, or the id alone when no workspace has that block.
    pub(super) fn who(&self, uid: &str, cx: &App) -> String {
        if uid == "you" {
            return "you".into();
        }
        if uid == templum_agent::ASKER {
            return "Templum Agent".into();
        }
        match self.uid_title(uid, cx) {
            Some(title) => format!("{uid} {title:?}"),
            None => uid.to_string(),
        }
    }

    /// Whether a block running `pane_agent` runs `agent` to a card: the same agent, or one with
    /// its program (the first word of their commands, as `program_name` reads it). A card for an
    /// account is that account's only: its agent's other folders run the same program.
    pub(super) fn runs(&self, pane_agent: Option<&str>, agent: &str) -> bool {
        if self
            .agents
            .iter()
            .any(|a| a.id == agent && a.account.is_some())
        {
            return pane_agent == Some(agent);
        }
        let program = |id: &str| {
            self.agents
                .iter()
                .find(|a| a.id == id)
                .map(|a| templum_terminal::program_name(&a.command))
                .filter(|p| !p.is_empty())
        };
        pane_agent.is_some_and(|pane| {
            pane == agent || program(pane).is_some_and(|p| program(agent) == Some(p))
        })
    }

    /// Whether `assignee` is the caller's: its block, or an agent it runs.
    pub(super) fn is_callers(&self, assignee: &Assignee, caller: &Caller) -> bool {
        match assignee {
            Assignee::Block(uid) => *uid == caller.0,
            Assignee::Agent(id) => self.runs(caller.1.as_deref(), id),
        }
    }

    /// Ok when the caller may take `card`: it is for anyone, the caller's block, or an agent it
    /// runs; otherwise `claim_card`'s refusal, saying why.
    pub(super) fn may_take(&self, card: &Card, caller: &Caller, cx: &App) -> Result<(), Reply> {
        let id = card.id;
        match &card.assignee {
            Some(Assignee::Block(uid)) if *uid != caller.0 => Err(Reply::err(format!(
                "Card {id} is taken by {}. Ask the person to free it (the ✕ by its assignee in the \
                 open card), or take another card.",
                self.who(uid, cx)
            ))),
            Some(a @ Assignee::Agent(agent)) if !self.is_callers(a, caller) => {
                let wanted = self.agent_title(agent);
                Err(Reply::err(match &caller.1 {
                    None => format!(
                        "Card {id} is for any {wanted}, and Templum can't tell yet which agent you \
                         are: try again in a few seconds."
                    ),
                    Some(mine) => format!(
                        "Card {id} is for any {wanted}, and you are {}: take a card for you or for \
                         anyone.",
                        self.agent_title(mine)
                    ),
                }))
            }
            _ => Ok(()),
        }
    }

    /// Who a card is for, as `move_card` says it: `b7k2m "Codex · api"`, or `any codex`.
    pub(super) fn for_whom(&self, assignee: &Assignee, cx: &App) -> String {
        match assignee {
            Assignee::Block(uid) => self.who(uid, cx),
            Assignee::Agent(id) => format!("any {id}"),
        }
    }

    pub(super) fn list_cards(
        &self,
        from: &str,
        board: Option<&str>,
        card: Option<u64>,
        cx: &App,
    ) -> Reply {
        let (space, me) = self.caller_space(from);
        let caller = caller(space, me);
        let board = match card {
            Some(card) => self.card_board(space, me, card),
            None => self.board_for(space, me, board),
        };
        match board {
            Ok(board) => self.cards_of(board, card, Some(&caller), cx),
            Err(refused) => refused,
        }
    }

    /// Who `text` names for a card: anyone (None); an agent by its id or title in either case,
    /// matched first; or a terminal block by its id, in any workspace.
    pub(super) fn resolve_for(&self, text: &str) -> Result<Option<Assignee>, Reply> {
        let text = text.trim();
        if text.eq_ignore_ascii_case("anyone") {
            return Ok(None);
        }
        if let Some(agent) = self
            .agents
            .iter()
            .find(|a| a.id.eq_ignore_ascii_case(text) || a.title.eq_ignore_ascii_case(text))
        {
            return Ok(Some(Assignee::Agent(agent.id.clone())));
        }
        let terminal = self.spaces().any(|s| {
            s.block_by_uid(text)
                .is_some_and(|id| s.terminal(id).is_some())
        });
        if terminal {
            return Ok(Some(Assignee::Block(text.to_string())));
        }
        let ids: Vec<&str> = self.agents.iter().map(|a| a.id.as_str()).collect();
        Err(Reply::err(format!(
            "There's no agent or terminal block {text}. Agents: {}; or a terminal's block id, or \
             anyone.",
            ids.join(", ")
        )))
    }

    pub(super) fn create_card(
        &mut self,
        from: &str,
        new: tools::NewCard,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space, me) = self.caller_space(from);
        let board = match self.board_for(space, me, new.board.as_deref()) {
            Ok(b) => b,
            Err(refused) => return refused,
        };
        match self.card_new(board, new, cx) {
            Ok(id) => Reply::ok(format!("Created card {id}.")),
            Err(refused) => refused,
        }
    }

    /// Sets a card's priority and who it is for, adds items, checks and unchecks them, on a card
    /// of a kanban wired to the caller.
    pub(super) fn edit_card(
        &mut self,
        from: &str,
        edit: tools::CardEdit,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space, me) = self.caller_space(from);
        if let Err(refused) = self.card_board(space, me, edit.card) {
            return refused;
        }
        let caller = caller(space, me);
        self.card_edit(edit, Some(&caller), cx)
    }

    pub(super) fn move_card(
        &mut self,
        from: &str,
        card: u64,
        column: &str,
        before: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space, me) = self.caller_space(from);
        let board = match self.card_board(space, me, card) {
            Ok(b) => b,
            Err(refused) => return refused,
        };
        let Some(to) = self.boards.column_named(board, column) else {
            return self.no_column(board, column);
        };
        let caller = caller(space, me);
        let mine = caller.0.clone();
        // Its last column, or Review: the card is done, and says so only through a comment.
        let finished = self.boards.board(board).is_some_and(|b| {
            b.columns.last().is_some_and(|c| c.id == to)
                || b.columns
                    .iter()
                    .any(|c| c.id == to && c.title.trim().eq_ignore_ascii_case("review"))
        });
        let said = self
            .boards
            .card(board, card)
            .and_then(|c| c.comments.last())
            .is_some_and(|c| c.by == mine);
        // Moving stays open to every wired agent; it says when the card is someone else's.
        let theirs = self
            .boards
            .card(board, card)
            .and_then(|c| c.assignee.as_ref())
            .filter(|a| !self.is_callers(a, &caller))
            .map(|a| self.for_whom(a, cx));
        let moved = self.card_move(card, column, before, cx);
        if moved.is_error {
            return moved;
        }
        let mut text = moved.text;
        if finished && !said {
            text.push_str(
                " Say on it what you did and what the next one needs to know: comment_card.",
            );
        }
        if let Some(who) = theirs {
            text.push_str(&format!(" It is for {who}, not you."));
        }
        Reply::ok(text)
    }

    pub(super) fn comment_card(
        &mut self,
        from: &str,
        card: u64,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space, me) = self.caller_space(from);
        if let Err(refused) = self.card_board(space, me, card) {
            return refused;
        }
        let by = space.uids.get(&me).cloned().unwrap_or_default();
        self.card_comment(card, text, &by, cx)
    }

    /// The board holding `card`, when comment `comment` is on it and the caller wrote it: a
    /// comment is its writer's to change, so the person's and other agents' are refused.
    fn own_comment(&self, from: &str, card: u64, comment: u64, cx: &App) -> Result<u64, Reply> {
        let (space, me) = self.caller_space(from);
        self.card_board(space, me, card)?;
        let uid = space.uids.get(&me).map(String::as_str).unwrap_or_default();
        self.comment_of(card, comment, Some(uid), cx)
    }

    /// `edit_comment`: the caller's own comment gets `text`, one step of the kanban's Ctrl+Z.
    pub(super) fn edit_comment(
        &mut self,
        from: &str,
        card: u64,
        comment: u64,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        match self.own_comment(from, card, comment, cx) {
            Ok(board) => self.comment_edit(board, card, comment, text, cx),
            Err(refused) => refused,
        }
    }

    /// `delete_comment`: the caller's own comment goes, one step of the kanban's Ctrl+Z.
    pub(super) fn delete_comment(
        &mut self,
        from: &str,
        card: u64,
        comment: u64,
        cx: &mut Context<Self>,
    ) -> Reply {
        match self.own_comment(from, card, comment, cx) {
            Ok(board) => self.comment_delete(board, card, comment, cx),
            Err(refused) => refused,
        }
    }

    pub(super) fn claim_card(&mut self, from: &str, card: u64, cx: &mut Context<Self>) -> Reply {
        let (space, me) = self.caller_space(from);
        let board = match self.card_board(space, me, card) {
            Ok(b) => b,
            Err(refused) => return refused,
        };
        let caller = caller(space, me);
        let Some(c) = self.boards.card(board, card) else {
            return Reply::err(format!("Card {card} is gone."));
        };
        let mine = Assignee::Block(caller.0.clone());
        if c.assignee.as_ref() == Some(&mine) {
            return Reply::ok(format!("Card {card} is already yours."));
        }
        if let Err(refused) = self.may_take(c, &caller, cx) {
            return refused;
        }
        self.edit_boards(|b| b.edit_card(board, card, |c| c.assignee = Some(mine)));
        cx.notify();
        Reply::ok(format!("Card {card} is yours."))
    }

    /// The block `block` names, when it is wired to the caller: its workspace's id and its id.
    fn wired(&self, from: &str, block: &str) -> Result<(String, BlockId), Reply> {
        let (space, me) = self.caller_space(from);
        Ok((space.id.clone(), target(space, me, block)?))
    }

    pub(super) fn read_note(&self, from: &str, block: &str, cx: &App) -> Reply {
        match self.wired(from, block) {
            Ok((space, id)) => self.note_text((&space, id), block, cx),
            Err(refused) => refused,
        }
    }

    pub(super) fn append_note(
        &mut self,
        from: &str,
        block: &str,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        match self.wired(from, block) {
            Ok((space, id)) => self.note_append((&space, id), block, text, window, cx),
            Err(refused) => refused,
        }
    }

    /// `replace_note` from the terminal `from`: the note the wired note block `block` shows gets
    /// `text`, whole or in place of its one passage `old`; one step of the note's Ctrl+Z.
    pub(super) fn replace_note(
        &mut self,
        from: &str,
        block: &str,
        text: &str,
        old: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space, id) = match self.wired(from, block) {
            Ok(at) => at,
            Err(refused) => return refused,
        };
        if let Err(refused) = self.note_editor((&space, id), block) {
            return refused;
        }
        if let Err(refused) = self.note_replace(&space, id, text, old, window, cx) {
            return Reply::err(refused);
        }
        cx.notify();
        Reply::ok(match old {
            Some(_) => format!("Replaced 1 passage of the note in {block}."),
            None => format!("Replaced the note in {block}."),
        })
    }

    pub(super) fn read_checklist(&self, from: &str, block: &str) -> Reply {
        match self.wired(from, block) {
            Ok((space, id)) => self.checklist_text((&space, id), block),
            Err(refused) => refused,
        }
    }

    pub(super) fn check_item(
        &mut self,
        from: &str,
        block: &str,
        item: u64,
        done: bool,
        cx: &mut Context<Self>,
    ) -> Reply {
        match self.wired(from, block) {
            Ok((space, id)) => self.item_check((&space, id), block, item, done, cx),
            Err(refused) => refused,
        }
    }

    /// `edit_item` from the terminal `from`: see [`Workspace::item_text`].
    pub(super) fn edit_item(
        &mut self,
        from: &str,
        block: &str,
        item: u64,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        match self.wired(from, block) {
            Ok((space, id)) => self.item_text((&space, id), block, item, text, cx),
            Err(refused) => refused,
        }
    }

    /// `delete_item` from the terminal `from`: see [`Workspace::item_delete`].
    pub(super) fn remove_item(
        &mut self,
        from: &str,
        block: &str,
        item: u64,
        cx: &mut Context<Self>,
    ) -> Reply {
        match self.wired(from, block) {
            Ok((space, id)) => self.item_delete((&space, id), block, item, cx),
            Err(refused) => refused,
        }
    }

    pub(super) fn add_items(
        &mut self,
        from: &str,
        block: &str,
        text: &str,
        cx: &mut Context<Self>,
    ) -> Reply {
        match self.wired(from, block) {
            Ok((space, id)) => self.items_add((&space, id), block, text, cx),
            Err(refused) => refused,
        }
    }

    /// `create_block` from the terminal `from`: a note, a checklist or a terminal running a
    /// command beside the caller, wired to it (docs/specs/agent-tools.md, create_block).
    pub(super) fn create_block(
        &mut self,
        from: &str,
        args: tools::NewBlockArgs,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space, me) = self.caller_space(from);
        let space_id = space.id.clone();
        let opener = space.uids.get(&me).cloned();
        let (new, kind) = match args.kind {
            tools::BlockKind::Note => (
                NewBlock::Note {
                    text: args.text.unwrap_or_default(),
                    open: None,
                },
                "a note",
            ),
            tools::BlockKind::Checklist => (
                NewBlock::Checklist {
                    name: args.name.unwrap_or_default(),
                    items: checklists::parse_items(args.items.as_deref().unwrap_or_default()),
                },
                "a checklist",
            ),
            tools::BlockKind::Terminal => match space.folder_for(me, args.folder.as_deref()) {
                Ok(folder) => (
                    NewBlock::Terminal {
                        folder,
                        command: args.command.clone(),
                        shell: None,
                    },
                    "a terminal",
                ),
                Err(refused) => return Reply::err(refused),
            },
        };
        let folder = match &new {
            NewBlock::Terminal { folder, .. } => Some(folder.clone()),
            _ => None,
        };
        let id = match self.add_block_in(&space_id, new, Place::Near(me), opener, window, cx) {
            Ok(id) => id,
            Err(refused) => return Reply::err(refused),
        };
        let uid = self
            .in_space(&space_id, |ws| {
                ws.space.add_wire(me, id);
                ws.space.uids.get(&id).cloned()
            })
            .flatten()
            .unwrap_or_default();
        cx.notify();
        let mut said = format!("Opened {kind} as {uid}");
        if let Some(folder) = folder {
            said.push_str(&format!(" in {}", folder.display()));
        }
        if let Some(command) = &args.command {
            said.push_str(&format!(", running {command:?}"));
        }
        said.push_str(", connected to you.");
        if args.command.is_some() {
            said.push_str(&format!(
                " wait_for(block: \"{uid}\", pattern: …, lines: 50) waits for its line; stop_agent \
                 ends it."
            ));
        }
        Reply::ok(said)
    }

    /// `open_file` from `from`: the front-most wired editor whose folder holds the file, else a
    /// new editor beside the caller, wired to it. Neither the keyboard nor the camera moves; an
    /// editor that has the keyboard gives it to the tab opened.
    pub(super) fn open_file_for(
        &mut self,
        from: &str,
        path: &str,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Reply {
        let (space, me) = self.caller_space(from);
        let here = space.blocks[&me]
            .folder()
            .or_else(|| space.terminal(me)?.seen.folder.clone());
        let named = crate::launcher::expand(path);
        let given = match &here {
            Some(here) if named.is_relative() => here.join(named),
            _ => named,
        };
        if given.is_relative() {
            return Reply::err("Templum doesn't know your folder yet: give the file's whole path.");
        }
        let Ok(real) = std::fs::canonicalize(&given) else {
            return Reply::err(format!("There's no file {}.", given.display()));
        };
        if !real.is_file() {
            return Reply::err(format!("{} isn't a file.", given.display()));
        }
        match crate::editor::read_file(&real) {
            Err(why) => return Reply::err(why),
            Ok((Content::Binary { .. }, _)) => {
                return Reply::err(format!(
                    "{} is binary: an editor shows no text for it.",
                    given.display()
                ));
            }
            Ok(_) => {}
        }
        // The file as `folder` names it, when `folder` holds it. Canonical paths only to compare:
        // an editor's tabs are named under its folder as it was given (a link, Windows' `C:\`),
        // and a tab is found again, and saved with the block, by that name. Only the file's
        // folder is made canonical: a linked file (CLAUDE.md -> AGENTS.md) keeps its own name.
        let (dir, name) = (
            given.parent().and_then(|p| std::fs::canonicalize(p).ok()),
            given.file_name(),
        );
        let under = |folder: &Path| {
            let inside = dir
                .as_deref()?
                .strip_prefix(std::fs::canonicalize(folder).ok()?)
                .ok()?;
            Some(folder.join(inside).join(name?))
        };
        let wired = space
            .canvas
            .blocks()
            .iter()
            .rev()
            .map(|b| b.id)
            .filter(|id| *id != me && space.wired(me, *id))
            .find_map(|id| Some((id, under(&space.editor(id)?.folder)?)));
        let (folder, path) = here
            .and_then(|here| under(&here).map(|path| (here, path)))
            .unwrap_or_else(|| {
                (
                    given.parent().unwrap_or(&given).to_path_buf(),
                    given.clone(),
                )
            });
        let space_id = space.id.clone();
        let opened = self.in_space(&space_id, |ws| {
            let (id, path, new) = match wired {
                Some((id, path)) => (id, path, false),
                None => {
                    let anchor = ws.space.home(ws.space.canvas.get(me)?);
                    let rect = ws.space.room_beside(&[anchor], crate::editor::SIZE);
                    let kept = (
                        ws.space.focused,
                        ws.space.selected.clone(),
                        ws.space.selected_wire,
                    );
                    // In grid mode a new block takes half of the focused tile: the caller's.
                    ws.space.focused = Some(me);
                    ws.space.pristine = false;
                    let editor = Editor::new(folder.clone(), true, cx);
                    let id = ws.add_block(rect, Body::Editor(Box::new(editor)));
                    (ws.space.focused, ws.space.selected, ws.space.selected_wire) = kept;
                    ws.space.add_wire(me, id);
                    (id, path, true)
                }
            };
            let had_keys = ws.space.blocks.get(&id)?.has_focus(window, cx);
            let editor = ws.space.editor_mut(id)?;
            let i = editor.open_tab(&path, window, cx).ok()?;
            if let Some(line) = line {
                let at = Position {
                    line: line - 1,
                    character: 0,
                };
                editor.tabs[i].editor.update(cx, |e, cx| {
                    // Not set_cursor_position: that would take the keyboard.
                    let offset = e.text().position_to_offset(&at);
                    e.set_selected_range(offset..offset, cx);
                });
            }
            // A tab with no text to type in (a Markdown preview): the canvas takes the keys, as
            // `release_focus` does, rather than leave them in a hidden tab.
            if had_keys && !editor.focus(window, cx) {
                window.focus(&ws.focus_handle, cx);
            }
            Some((ws.space.uids.get(&id)?.clone(), path, new))
        });
        cx.notify();
        let at = line.map(|n| format!(" at line {n}")).unwrap_or_default();
        match opened.flatten() {
            Some((uid, path, false)) => {
                Reply::ok(format!("Opened {}{at} in {uid}.", path.display()))
            }
            Some((uid, path, true)) => Reply::ok(format!(
                "Opened {}{at} in a new editor {uid} beside you, connected to you.",
                path.display()
            )),
            None => Reply::err(format!("Couldn't open {}.", given.display())),
        }
    }
}
