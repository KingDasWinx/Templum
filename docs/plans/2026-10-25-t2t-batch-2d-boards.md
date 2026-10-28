# Templum to Templum, Batch 2d (Boards, Live and Acted On) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer in a Templum agent block does the whole plan, then
> fresh Opus 5.5 reviewers review it in a loop until one passes it. The implementer **commits nothing**: everything
> stays uncommitted for the orchestrator.

**Goal:** Kanban card 171 part 2, batch 2, fourth of eight parts: a kanban block of a mirrored workspace is **drawn
live from home's boards and cards** by the kanban's own view, and **everything a person does to a board works there,
done at home**: dragging cards and columns, adding, renaming and deleting columns, the card dialog (Done, comments,
Delete card), the header's All boards…, Rename board… and Delete board…, a card's Send to ▸. Home gains
`delete_card`, `edit_board`, `edit_column`, `show_board`, the grown arguments of `create_card`, `edit_card` and
`move_card`, and `edit_comment`/`delete_comment` in the control server.

**Architecture:** Home first (Tasks 1–3): the new control tools and arguments, parsed in `templum-mcp`, run by the glue
on `Workspace::boards`, their dangers in the control server's gate. Then the mirror: every write of a kanban (its view,
its menus, its card dialog) goes through one door, `Workspace::act_on_boards(space, BoardAct, …)` (Task 4): on this
computer it is today's `edit_boards` edit, on a mirrored workspace a `remote_call` of home's tool, asked of `why_not`
first. A remote's copy of home's boards is a `Boards` kept in `RemoteSpace::boards`, made from home's state where it
enters (cleaned, capped, a card rebuilt only when its `Arc` changed), never saved (Task 5); a remote kanban is
`Body::Kanban(Kanban { board, remote: true })`, which every local lookup (`Body::kanban()`) skips. Card moves, card
edits and card deletes stand over home's state as holds (`Holds`, 2 s) until home's state shows them (Tasks 6–7).

**Tech Stack:** Rust 2024; GPUI through gpui-kit `=0.7.3`; `templum-mcp` (control server, block server parsing);
`templum-api` (registry, `State`, `Arc` items, `model::same`); 2a's `remote_call`/`Then`/`Deed`/`Nay`/`why_not`/
`Holds`/`remirror`; 2b's `mark_remote`, `Gates`, `gated_item`, `still_shown`, `warn`.

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), "Batch 2: the whole workspace, live":
"Parts ▸ 2d", "What works on a remote workspace" (the cards row), "The canvas ▸ Holds" and "Undo", "Boards,
checklists, notes, ink ▸ Boards", "One rule for what the mirror can do", "What home's API gains" (new control tools,
"Into the control server", grown arguments, the registry's lists), "The mirror's side" (the copies' caps, never a
relay), "Constraints (batch 2)", the threat model rows marked *(batch 2)*, "As built ▸ 1c, 1d, 2a, 2b, 2c"; the 2a,
2b and 2c plans' "Interfaces for later parts".

**Code it builds on:** `crates/templum-mcp/src/{tools.rs,control.rs}`; `crates/templum-api/src/{registry.rs,model.rs}`;
`crates/templum/src/{board.rs,kanban.rs,block.rs,list_dialog.rs}`, `crates/templum/src/menu/{mod.rs,build.rs}`,
`crates/templum/src/workspace/{ops.rs,content.rs,control.rs,approvals.rs,bus.rs,blocks.rs,api.rs,persist.rs,remote.rs,
holds.rs,history.rs,mod.rs,input.rs}`, `crates/templum/src/bench.rs`; tests `crates/templum/src/workspace/tests/
{mod.rs,api.rs,kanban.rs,templum_agent.rs,remote.rs,menus.rs,mirror_end_to_end.rs}`.

**Consumes (as built):** `Boards::{board, card, find_card, column_named, add_board_with, add_column, rename_column,
remove_column, move_column, move_card, remove_card, add_comment, edit_comment, remove_comment, finish_draft,
keep_draft, drop_draft, next}`, `Draft::{merge_into, rebase}`, `Card::toggle_label`, `Label::{ALL, name}`,
`Priority::named`; `Workspace::{edit_boards, undo_boards, board_named, board_of, no_column, card_new, card_edit,
card_move, card_comment, show_board, new_board, delete_board, board_shown, card_shown, find_block, made, who,
uid_title}`; `templum_agent::Own::{cards, boards}`; the gate (`approvals.rs`: `gate`, `facts`, `approval_words`),
`Rule::{AskOwn, AskOwnWhenLosing}`; `registry::{TARGETS, NOT_TARGETS, scope}`; `RemoteSpace::{state, homes,
home_of, why_not, takes, agents, held}`, `Deed::{call, needs}`, `Nay::words`, `Workspace::{why_not, remote_call,
remote_mut, remirror, expire_holds, mark_remote, warn, still_shown, typing_into, mirror_send}`, `Command::Paste`,
`Holds::{hold, get, end, shown, expire, keys}`, `HOLD`; `bus::{clean_title, clean_to}`, `remote.rs`'s `clean_text`;
`menu::Gates`, `build.rs`'s `gated_item`; the tests' `open`, `with_data`, `with_token`, `tool`, `state`, `said`,
`refused`, `board_with_a_card`, `three_blocks`, `control`, `control_later`, `offline`, `live`, `show_remote`, `apply`,
`live_terminals`, `terminals_of_home`, `viewer`, `sent`, `answer`, `refusal`, `remote_of`, `shown_mirror_as`,
`pump_home`, `mirrored`, `rect_at`.

## Global Constraints

- **MEMORY LIMITS on this machine:** **every** cargo command takes `-j 4`; **never two cargo commands at once**; never
  `cargo clean`; while working run only the targeted tests each step names; the full suite **once**, in Task 8.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-mcp` and `templum-api` stay UI-free. No
  `unsafe`. **No new dependency.**
- **Home is the truth:** the mirror never edits a board; every act is home's tool through `remote_call`, asked of
  `RemoteSpace::why_not` first. What the mirror shows ahead of home is a hold that ends when home's state shows it, at
  a refusal, or after `HOLD` (2 s).
- **Nothing local written for a remote workspace:** no `Workspace::boards` edit, no `next()` of the local boards, no
  draft, no attachment folder read or written, no `boards.json` save for anything of a remote's; a remote's
  `Boards` lives only in `RemoteSpace::boards`. `Body::kanban()`/`kanban_mut()` answer None for a remote kanban, so no
  local lookup by board id (`board_open_at`, `board_shown`, `agent_sessions`, the kanban's Ctrl+Z, the API's state,
  `overview`) can meet home's ids, which in the end-to-end test equal the local ones.
- **Never a relay:** the mirror's API, control server and block server gain nothing; Send to ▸ on a remote card lists
  only the remote workspace's terminals.
- **What home sends is bounded and cleaned before it is kept or drawn** (`remote_boards.rs`, once where it enters):
  a card's title one line of at most 300 characters; its description `clean_text` (line breaks and tabs kept) of at most
  65,536 characters; at most 500 checklist items, each one line of at most 4,000 characters; the newest 200 comments,
  each `clean_text` of at most 16,384 characters, their author one line (`clean_title`); at most 100 attachments, their
  names one line (`clean_title`), never a path the mirror opens; labels only the six names, each once; a priority only
  `low`, `medium`, `high`; an assignee's block or agent id at most 64 bytes with no control character, else none; board
  names and column titles `clean_title`; at most 100 columns a board and 500 cards a column; a card listed twice, or in
  a column of another board, once at its first place; an id home's state doesn't hold is skipped.
- **The UI thread never waits on the network:** every act is `remote_call` (its `Then` runs when home answers).
- **The rule:** every button, item, drag and Done asks `why_not` of its deed; a dimmed control shows the words as its
  tooltip; a refused drop or Enter says them once (`warn`). New deeds, each `Deed::call` with home's tool:
  `MoveCard` (`move_card`), `CreateCard` (`create_card`), `EditCard` (`edit_card`), `CommentCard` (`comment_card`),
  `EditComment` (`edit_comment`), `DeleteComment` (`delete_comment`), `DeleteCard` (`delete_card`), `EditColumn`
  (`edit_column`), `EditBoard` (`edit_board`), `ShowBoard` (`show_board`); all take a Control share; Send to ▸ is
  `Deed::Type` (Full). A home whose `move_card` doesn't take `before` gets the card at its column's end (rung 2).
- **Values, verbatim:** `HOLD` 2 s; `MOST_COLUMNS` 100; `MOST_COLUMN_CARDS` 500; `CARD_TITLE_CHARS` 300;
  `DESCRIPTION_CHARS` 65,536; `MOST_CARD_ITEMS` 500; `ITEM_CHARS` 4,000; `MOST_COMMENTS` 200; `COMMENT_CHARS` 16,384;
  `MOST_ATTACHMENTS` 100; `templum_mcp::tools::LABELS` = `["red", "yellow", "green", "cyan", "blue", "magenta"]`;
  a new board from `show_board` without `columns`: To do, Doing, Done.
- **Words, verbatim** (winx-desktop is `computer_of`; ids as in the examples):
  - Mirror: "Attachments are on winx-desktop: open them there." (Attach, and opening an attachment, on a remote card);
    unchanged and reused: the four `Nay` sentences, "On a mirror, Ctrl+Z undoes moves only: undo the rest on
    winx-desktop.", "Another workspace is shown now: nothing was done.", "No board open here".
  - Home's answers: "Deleted card 41 \"Fix login\"."; "Renamed board 12 to \"Sprint 2\"."; "Deleted board \"Sprint\"
    (12) and its 3 cards." ("and its card" for one; "Deleted board \"Sprint\" (12)." for none); "Added column 31
    \"New column\" to board \"Sprint\"."; "Renamed column 31 to \"Review\"."; "Moved column 31 before column 33.";
    "Moved column 31 to the end."; "Deleted column \"Doing\" (31) and its 4 cards." (same count words as boards);
    "Showing board \"Sprint\" (12) in k9x1d."; "Made board \"Board 3\" (14) and showed it in k9x1d."; "Moved card 40
    to Doing, before card 41."; `edit_card`'s parts in this order: `titled "New title"`, `description set` or
    `description cleared`, `label red on`, `label blue off`, then today's (`priority …`, `for …`, `added item …`), then
    `removed item 7` / `removed items 7, 8`, then today's `checked …`, `unchecked …`.
  - Home's refusals: "Card 41 isn't in Doing." (`before` a card of another column; the column's title as the board
    has it); "A card can't go before itself."; "A column can't go before itself."; "A board needs a name."; "A column
    needs a title."; "A card needs a title." (today's, now also for `edit_card`'s `title`); "k9x1d isn't a kanban.";
    "Card 40 has no item 99: list_cards with card shows its items." (today's, now also for `remove_items`); "Comment 9
    is by b7k2m \"Codex\", not you: you can only change your own." (today's form; "the person" for a comment by you
    when the Templum Agent asks).
  - Home's parser: "\"labels\" is a list of red, yellow, green, cyan, blue or magenta"; "\"labels\" turns labels on or
    off: {\"red\": true, \"blue\": false}, of red, yellow, green, cyan, blue or magenta"; "edit_card needs something to
    change: title, description, priority, for, labels, add_items, remove_items, check or uncheck" (replaces today's
    list); "delete_card needs \"card\", a card's id"; "edit_board takes name or delete: true"; "edit_column needs
    title, before or delete for a column"; "delete goes alone: it takes the column and its cards away"; "delete needs
    \"column\""; "\"before\" is a column of the board, or \"end\""; "show_board takes board or new"; "\"columns\" go
    with new".
- **Batch 1's and 2a–2c's words stay;** every earlier test keeps passing except those this plan rewrites, named in
  their task (a kanban mirrored as a card becomes a live kanban; `NOT_MIRRORED`'s words; the edit_card parser's list).
- **Every logic change has a test that fails without it;** UI-only changes say how they were verified (the board, the
  card dialog and the dimmed controls on a remote were not looked at on a display unless the implementer did: then a
  minors entry). Headless GPUI tests; keys in `keyring_core::mock::Store`; home on loopback in the same process;
  conditions with a 10 s deadline, never a fixed sleep. CI runs Linux, macOS and Windows with no agent CLI.
- **Every feature in the API:** each new tool and argument ships its lines in `docs/api.md` (and `docs/specs/api.md`,
  `docs/specs/templum-agent.md`'s tools table, `docs/specs/connections.md`'s block-server table) in the task that adds
  it. The mirror's acts add nothing to the mirror's own API.
- **Minors** go to `docs/minors.md` (`## Templum to Templum`); "A kanban added from a mirror shows a new board." leaves
  it (Task 6). Docs lines ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **Home's ids equal the mirror's own** (the end-to-end test is one workspace for both ends, so home's board 12 *is*
   the mirror's board 12; a mirror with boards of its own collides by chance): no read of the remote kanban, its menus
   or its dialog may reach `Workspace::boards`, no write may reach it, and no local tool may see the remote kanban. →
   Task 5 `no_local_tool_sees_a_remote_board`; Task 8
   `acts_on_a_mirrored_board_leave_the_mirror_s_boards_json_as_it_was`.
2. **A hostile home's board** (ESC and `\r` in titles, a 1 MB description, 10,000 cards in one column, a card in two
   columns, unknown labels, an assignee `"b1\n"`, an attachment named `../../.ssh/id_ed25519`): capped and cleaned
   before it is kept; attachments never read from `<data>/attachments`. → Task 5 `home_s_cards_are_cleaned_and_capped`,
   `a_board_keeps_its_columns_and_cards_each_once_and_capped`; Task 7
   `a_remote_card_shows_its_attachments_by_name_only`.
3. **Home changes a card while the person edits it, or while a hold stands:** the dialog shows home's change (rebase),
   Done sends only what the person changed, a held edit never doubles the items it added, a held move shows until
   home's state agrees or 2 s pass. → Task 7 `done_on_a_remote_card_sends_only_what_changed_and_shows_it_at_once`;
   Task 6 `a_card_dropped_on_another_sends_move_card_before_it_and_shows_at_once`.
4. **A refusal, or the connection dropping, mid-act** (the column deleted at home, the card gone): the hold ends at
   once, home's state is drawn again, the sentence is said once; a refused Done keeps the person's work (the dialog
   reopens with it). → Task 6 `a_card_deleted_on_the_mirror_hides_until_home_drops_it`; Task 7
   `a_refused_done_reopens_the_card_with_what_the_person_made`.
5. **The shown workspace changes while a remote's dialog is open** (an agent's `show`): the card dialog acts on its own
   remote by id; the Boards dialog acts only while its workspace is shown. → Task 6
   `all_boards_lists_the_share_s_boards_and_shows_one_at_home` (its last step); Task 7
   `a_remote_card_s_dialog_keeps_no_draft_here` (its switch step).

---

### Task 1: Home's grown card arguments (`labels`, `title`, `description`, `remove_items`, `before`)

**Files:**
- Modify: `crates/templum-mcp/src/tools.rs` (`NewCard`, `CardEdit`, `Call::MoveCard`, `LABELS`, the three tools'
  schemas and descriptions, `parse`)
- Modify: `crates/templum/src/board.rs` (`Boards::move_card_before`), `crates/templum/src/workspace/ops.rs`
  (`card_new`, `card_edit`, `card_move`), `crates/templum/src/workspace/content.rs` (block server's `move_card`),
  `crates/templum/src/workspace/control.rs`, `crates/templum/src/workspace/bus.rs` (the `MoveCard` arms)
- Create: `crates/templum/src/workspace/tests/board_tools.rs` (`mod board_tools;` in `tests/mod.rs`)
- Docs: `docs/api.md` (a **Boards.** paragraph after "Placing and naming"), `docs/specs/api.md` (a bullet after
  `move_block`'s), `docs/specs/connections.md` (the `create_card`, `edit_card`, `move_card` rows)

**Interfaces:**
- Produces: `templum_mcp::tools::LABELS: [&str; 6]`; `NewCard.labels: Vec<String>`; `CardEdit.{title: Option<String>,
  description: Option<String>, labels: Vec<(String, bool)>, remove_items: Vec<u64>}` (`description: Some("")`
  clears; the block server reads it raw, not through `optional`);
  `Call::MoveCard { card, column, before: Option<u64> }`;
  `Boards::move_card_before(&mut self, board: u64, card: u64, column: u64, before: Option<u64>)` (the card leaves its
  place, then goes right before `before`, at the end when None); `Workspace::card_move(&mut self, card: u64, column:
  &str, before: Option<u64>, cx) -> Reply`.

- [ ] **Step 1: Write the failing tests.** In `tools.rs`'s tests:

```rust
#[test]
fn the_card_tools_take_labels_a_title_a_description_items_to_remove_and_before() {
    let labelled = json!({"title": "x", "labels": ["red", "blue"]});
    let Call::CreateCard(new) = parse("create_card", &labelled).unwrap() else { panic!() };
    assert_eq!(new.labels, ["red", "blue"]);
    assert!(parse("create_card", &json!({"title": "x", "labels": ["purple"]})).unwrap_err()
        .contains("\"labels\" is a list of red, yellow, green, cyan, blue or magenta"));
    let Call::EditCard(edit) = parse("edit_card", &json!({"card": 4, "title": "T", "description": "",
        "labels": {"red": false, "green": true}, "remove_items": [7, 8]})).unwrap() else { panic!() };
    assert_eq!((edit.title.as_deref(), edit.description.as_deref()), (Some("T"), Some("")));
    assert_eq!(edit.labels, [("red".to_string(), false), ("green".to_string(), true)]);
    assert_eq!(edit.remove_items, [7, 8]);
    assert!(parse("edit_card", &json!({"card": 4, "labels": {"pink": true}})).unwrap_err()
        .contains("\"labels\" turns labels on or off"));
    assert!(parse("edit_card", &json!({"card": 4})).unwrap_err().contains(
        "edit_card needs something to change: title, description, priority, for, labels, add_items, \
         remove_items, check or uncheck"));
    assert_eq!(parse("move_card", &json!({"card": 4, "column": "Done", "before": 9})).unwrap(),
        Call::MoveCard { card: 4, column: "Done".into(), before: Some(9) });
}
```

In `board.rs`'s tests: `the_block_server_s_labels_are_the_board_s` — `Label::ALL.map(Label::name) ==
templum_mcp::tools::LABELS`; `move_card_before_puts_it_right_before_the_card` — Doing `[C, D]`, To do `[A, B]`:
`move_card_before(b, A, doing, Some(D))` → Doing `[C, A, D]`; then `move_card_before(b, C, doing, Some(D))` → `[A, C,
D]` (a card moved down its own column lands before `before`, not after it); `None` → the end.

In `tests/board_tools.rs` (through the API, an unlimited Control token, `board_with_a_card` and more cards):

```rust
#[gpui_kit::test]
fn create_card_takes_labels(cx: &mut TestAppContext) {
    // tool(create_card {board, title: "Login", labels: ["red", "blue"]}) answers "Created card {id}." (exactly: the
    // mirror reads the number from these words, Task 7) and the card's labels are [Label::Red, Label::Blue].
}

#[gpui_kit::test]
fn edit_card_takes_a_title_a_description_labels_and_items_to_remove(cx: &mut TestAppContext) {
    // A card with description "d", labels [Red], items i1, i2. edit_card {card, title: "New", description: "",
    // labels: {"red": false, "green": true}, remove_items: [i1]} answers
    // format!("Card {c}: titled \"New\"; description cleared; label red off; label green on; removed item {i1}.")
    // and the card is so, in one more `boards_undo` step. remove_items [99] → said(...) refused with
    // "Card {c} has no item 99: list_cards with card shows its items." and the card unchanged;
    // title "  " → "A card needs a title."
}

#[gpui_kit::test]
fn move_card_puts_a_card_just_before_another_of_its_column(cx: &mut TestAppContext) {
    // To do [A, B], Doing [C, D]. move_card {card: A, column: "Doing", before: D} → "Moved card A to Doing, before
    // card D." and Doing is [C, A, D]. before: B (in To do) → "Card B isn't in Doing.", nothing moves. before: A on A
    // → "A card can't go before itself." Without before: the end, as today.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum-mcp tools` then `cargo test -j 4 -p templum board::tests
  workspace::tests::board_tools` → FAIL (no such fields, functions, words).
- [ ] **Step 3: Implement** the parser (labels as a list for `create_card`, an object of booleans for `edit_card`,
  both checked against `LABELS`; `remove_items` with `item_ids`; `before` with `id("before", "list_cards")`), the
  schemas (`labels` an array of the six or an object of booleans; `before` "A card of that column: the card goes just
  before it. Left out: the end."), and the descriptions (`edit_card`: "Sets a card's title, description, labels,
  priority and who it is for, adds, removes, checks and unchecks checklist items: one step of the person's Ctrl+Z.
  Check an item as you finish it."; `move_card`: "Moves a card to a column of its board, before another card of it or
  at its end." followed by today's " Done with your card: …" sentence unchanged, which `SHARED_WORDS` removes for the
  Templum Agent). The glue: `card_new` sets the labels; `card_edit` checks `check`, `uncheck` and `remove_items` ids
  together, then applies title, description, labels and removals in the same `edit_boards` step; `card_move` checks
  `before` (a card of column `to`, not `card`) and calls `move_card_before`.
- [ ] **Step 4: Run them** (same commands) → PASS; then `cargo test -j 4 -p templum workspace::tests::kanban
  workspace::tests::content` → PASS.
- [ ] **Step 5: Docs.** `docs/api.md` **Boards.**: "`create_card` takes `labels` (red, yellow, green, cyan, blue,
  magenta). `edit_card` also takes `title`, `description` (blank clears it), `labels`
  (`{"red": true, "blue": false}`: turned on, turned off; the rest keep) and `remove_items` (item ids). `move_card`
  takes `before`: a card of that column, which the card goes just before; left out, the column's end." The same in
  `docs/specs/api.md` and the block server's rows of `docs/specs/connections.md` (the agents' tools take them too).

---

### Task 2: Home's board tools (`delete_card`, `edit_board`, `edit_column`, `show_board`)

**Files:**
- Modify: `crates/templum-mcp/src/control.rs` (`Before`, four `ControlCall` variants, `list`, `parse`, `title`, `rule`)
- Modify: `crates/templum-api/src/registry.rs` (`NOT_TARGETS` gains `"before"`, `"delete"`, `"labels"`, `"new"`,
  `"remove_items"`, sorted)
- Modify: `crates/templum/src/board.rs` (`Boards::move_column_before`), `crates/templum/src/workspace/ops.rs` (the four
  handlers), `crates/templum/src/workspace/control.rs` (their arms and targets)
- Test: `crates/templum-mcp/src/control.rs` tests, `crates/templum-api/src/registry.rs` tests,
  `crates/templum/src/workspace/tests/board_tools.rs`
- Docs: `docs/api.md` (**Boards.**), `docs/specs/api.md`

**Interfaces:**
- Produces (templum-mcp): `pub enum Before { Column(u64), End }`; `ControlCall::DeleteCard { card: u64 }`,
  `ControlCall::EditBoard { board: String, name: Option<String>, delete: bool }` (exactly one of the two),
  `ControlCall::EditColumn { board: String, column: Option<u64>, title: Option<String>, before: Option<Before>,
  delete: bool }` (no `column`: a new column; `title` and `before` may go together; `delete` alone),
  `ControlCall::ShowBoard { block: String, board: Option<String>, new: Option<String>, columns: Vec<String> }`
  (exactly one of `board` and `new`; `new` keeps a blank name as `Some("")`, which is "Board n"). `board` takes a
  number or a string, as `add_block`'s does. `rule`: `DeleteCard` → `AskOwn`; `EditBoard`/`EditColumn` with
  `delete` → `AskOwnWhenLosing`; the rest `Free`. `title`: "Deleting a card", "Editing a board", "Editing a column",
  "Showing a board".
- Produces (glue, `ops.rs`): `card_delete(&mut self, card: u64, cx) -> Reply`; `board_change(&mut self, board: u64,
  name: Option<&str>, cx) -> Reply` (None: delete); `column_change(&mut self, board: u64, column: Option<u64>, title:
  Option<&str>, before: Option<Before>, delete: bool, cx) -> Reply`; `board_show(&mut self, block: &str, board:
  Option<u64>, new: Option<(&str, &[String])>, cx) -> (Reply, Option<u64>)` (the board made, for `made`);
  `Boards::move_column_before(&mut self, board: u64, column: u64, before: Option<u64>)`. Each is one `edit_boards`
  step (Ctrl+Z on a kanban at home undoes it); `board_show` sets the kanban's `Kanban { board: Some(id), .. }` in the
  block's own workspace (not a step of Ctrl+Z, as the person's showing isn't). Targets: the card's or board's kanban
  (`card_shown`/`board_shown`, read before a delete), the shown block for `show_board`.

- [ ] **Step 1: Write the failing tests.** `control.rs`: `the_board_tools_parse_and_say_what_is_missing` — each
  sentence of Global Constraints' parser list from its call (`edit_board {board: 1}`, `{board: 1, name: "x", delete:
  true}`, `edit_board {board: 1, name: " "}` → "A board needs a name.", `edit_column {board: 1, column: 3}`, `{board:
  1, column: 3, title: "x", delete: true}`, `{board: 1, delete: true}`, `{board: 1, before: "first"}`, `{board: 1,
  title: ""}` → "A column needs a title.", `show_board {block: "k"}`, `{block: "k", board: "1", new: "x"}`, `{block:
  "k", board: "1", columns: ["a"]}`, `delete_card {}`); `edit_column {board: 1, before: "end"}` is a new column at
  the end (`before: Some(Before::End)`, `column: None`). `danger_follows_the_table_and_reason_cant_lower_it` gains the
  four rules (`edit_board` rename `Free`, delete `AskOwnWhenLosing`; same for `edit_column`; `delete_card` `AskOwn`;
  `show_board` `Free`); `every_tool_parses_its_example_and_names_what_is_missing` gains the four examples. Registry:
  `the_board_tools_take_a_control_token` — `scope` of each (rename and delete alike) is `Scope::Control`, and
  `every_argument_is_a_target_or_not` passes. `tests/board_tools.rs`:

```rust
#[gpui_kit::test]
fn delete_card_deletes_it_and_ctrl_z_brings_it_back(cx: &mut TestAppContext) {
    // tool(delete_card {card}) → "Deleted card {c} \"New card\"."; the card is gone; ws.undo_boards(cx) brings it back.
}

#[gpui_kit::test]
fn edit_board_renames_a_board_or_deletes_it_with_its_cards(cx: &mut TestAppContext) {
    // name: "Sprint 2" → "Renamed board {b} to \"Sprint 2\"."; delete: true with 3 cards → "Deleted board \"Sprint 2\"
    // ({b}) and its 3 cards.", and the kanban showing it shows none (Kanban::shown is None).
}

#[gpui_kit::test]
fn edit_column_adds_renames_moves_and_deletes_columns(cx: &mut TestAppContext) {
    // {board} → "Added column {n} \"New column\" to board \"…\"." at the end; {board, title: "Review", before: done}
    // lands before Done; {board, column: n, title: "QA"} renames; {board, column: n, before: "end"} → "Moved column
    // {n} to the end."; {board, column: n, before: n} → "A column can't go before itself."; {board, column: 999,
    // title: "x"} → today's no_column words; {board, column: doing, delete: true} with 4 cards → "Deleted column
    // \"Doing\" ({doing}) and its 4 cards."
}

#[gpui_kit::test]
fn show_board_shows_a_board_or_makes_one_in_a_kanban(cx: &mut TestAppContext) {
    // {block: kanban's uid, board: "Sprint"} → "Showing board \"Sprint\" ({b}) in {uid}." and the kanban shows it;
    // {block, new: ""} → "Made board \"Board 2\" ({n}) and showed it in {uid}." with To do, Doing, Done;
    // {block, new: "Ops", columns: ["A", "B"]} makes "Ops" with A, B even when a board "Ops" exists;
    // {block: a terminal's uid, board: "Sprint"} → "{uid} isn't a kanban."
}

#[gpui_kit::test]
fn a_limited_token_reaches_only_the_boards_its_kanbans_show(cx: &mut TestAppContext) {
    // As a_limited_token_names_its_board_and_never_hears_of_others: a board "Secret plans" shown by no block of its
    // workspace. delete_card of a secret card, edit_board/edit_column of the secret board, show_board {board: secret}
    // in its own kanban: each NOT_FOUND, "There's no card …"/"There's no board …", nothing changed. show_board on a
    // kanban of another workspace: NOT_FOUND block. show_board {new: "Mine"} in its own kanban works, and the new
    // board is then in the token's state (state(…, &limited).boards).
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum-mcp control`, `cargo test -j 4 -p templum-api registry`,
  `cargo test -j 4 -p templum workspace::tests::board_tools` → FAIL.
- [ ] **Step 3: Implement** the four tools. Descriptions (`list`): `delete_card` "Deletes a card and its comments
  (Ctrl+Z on a kanban brings it back). Asks the person first when it isn't a card you made."; `edit_board` "Renames a
  board (name), or deletes it with its cards (delete: true; the kanbans that showed it show none; Ctrl+Z on a kanban
  brings it back). Deleting a board with cards that you didn't make asks the person first."; `edit_column` "Adds a
  column to a board (column left out: at the end, or before another), renames one (title), moves one (before: a column
  of the board, or \"end\") or deletes one with its cards (delete: true). Deleting a column with cards of a board you
  didn't make asks the person first."; `show_board` "Shows a board in a kanban block (board), or a new one (new: its
  name, blank for \"Board n\"; columns: its columns, To do, Doing and Done when left out)." Each with `reason` when it
  may ask (`delete_card`, `edit_board`, `edit_column`). The glue resolves `board` with `board_named` (the API's
  `api_allows` already turned a limited token's into the id it sees), `block` with `find_block` (a remote's blocks
  have no uid, so a mirrored kanban is never found), and refuses a `Before::Column` that isn't the board's with
  `no_column`. `control_call` keeps `made(actor, own.boards.push)` for a board `show_board` made.
- [ ] **Step 4: Run them** → PASS.
- [ ] **Step 5: Docs.** `docs/api.md` **Boards.** gains the four tools, each with its arguments, what it does and
  "control"; `docs/specs/api.md` likewise.

---

### Task 3: Comments in the control server, and the gate's board deletes

**Files:**
- Modify: `crates/templum-mcp/src/control.rs` (`SHARED` gains `"edit_comment"`, `"delete_comment"` after
  `"comment_card"`: `[&str; 20]`), `crates/templum-api/src/registry.rs` (`NOT_TARGETS` gains `"comment"`)
- Modify: `crates/templum/src/workspace/content.rs` (`edit_comment`/`delete_comment` keep `own_comment`, then call the
  shared bodies), `crates/templum/src/workspace/ops.rs` (`comment_edit`, `comment_delete`),
  `crates/templum/src/workspace/control.rs` (the two arms), `crates/templum/src/workspace/approvals.rs` (`BoardAsk`,
  `board_ask`, the gate's board path, `facts`)
- Test: `crates/templum-mcp/src/control.rs` tests, `crates/templum/src/workspace/tests/board_tools.rs`
- Docs: `docs/specs/templum-agent.md` (tools table), `docs/api.md`, `docs/specs/api.md`

**Interfaces:**
- Produces: `Workspace::comment_edit(&mut self, board: u64, card: u64, comment: u64, text: &str, cx) -> Reply`
  and `comment_delete(&mut self, board: u64, card: u64, comment: u64, cx) -> Reply` (today's bodies, moved);
  `Workspace::comment_of(&self, card: u64, comment: u64, by: Option<&str>, cx) -> Result<u64, Reply>` (the board;
  `by` Some: the comment must be by it, today's refusal words; None: any); the API's actor edits a comment by `you`
  and deletes any, the Templum Agent edits and deletes only its own (`templum_agent::ASKER`).
- Produces (`approvals.rs`): `struct BoardAsk { own: bool, losing: Option<String>, said: String, details: String }`;
  `fn board_ask(&self, call: &ControlCall) -> Option<BoardAsk>` for `DeleteCard` (own: `own.cards` has it; said
  `Delete card 41 "Fix login"`; details `On board "Sprint", in Doing.`), `EditBoard` with `delete` (own: `own.boards`
  has it; losing: build.rs's `cards(n)` words when it has cards; said `Delete board "Sprint"`), `EditColumn` with
  `delete` (own: the board is its own; losing: the column's cards; said `Delete column "Doing" of "Sprint"`); None for
  every other call and for a card or board that isn't there (it runs, and says so). `gate` asks `board_ask` before its
  block path: `rule.level(own, losing.is_some())` decides, and the approval shows `said` and `details` with no block.
  `facts` answers `said + "\n" + details` for these calls, so a card added or gone since the question shows as changed.

- [ ] **Step 1: Write the failing tests.** `control.rs`: `the_two_servers_share_the_comment_tools` — `known` both,
  examples `edit_comment {card: 4, comment: 9, text: "x"}` and `delete_comment {card: 4, comment: 9}` parse to
  `ControlCall::Block(..)`, `rule` `Free`, titles "Editing a comment", "Deleting a comment"; `SHARED.len() == 20`.
  `tests/board_tools.rs`:

```rust
#[gpui_kit::test]
fn through_the_api_the_person_edits_their_comments_and_deletes_any(cx: &mut TestAppContext) {
    // A card with comment 1 by "you" and comment 2 by an agent's uid. edit_comment {1, "fixed"} → "Edited comment 1
    // on card {c}."; edit_comment {2} → refused "Comment 2 is by {uid} …, not you: you can only change your own.";
    // delete_comment {2} → "Deleted comment 2 on card {c}.".
}

#[gpui_kit::test]
fn the_templum_agent_edits_and_deletes_only_its_own_comments(cx: &mut TestAppContext) {
    // control(comment_card) signs Templum Agent; control(edit_comment) of it works; control(delete_comment) of a "you"
    // comment is refused "Comment {n} is by the person, not you: you can only change your own."
}

#[gpui_kit::test]
fn deleting_the_person_s_card_asks_and_its_own_doesnt(cx: &mut TestAppContext) {
    // control_later(delete_card {card: person's}) → one approval, said "Delete card {c} \"New card\"", the card still
    // there; control(create_card) then control(delete_card) of that one → "Deleted card …" at once, no approval.
}

#[gpui_kit::test]
fn deleting_a_board_or_a_column_with_the_person_s_cards_asks(cx: &mut TestAppContext) {
    // edit_board {delete} of the person's board with 2 cards → approval said "Delete board \"…\"", details "Its 2 cards
    // are deleted too."; an empty board of the person's → deleted at once; a board the Templum Agent made (add_block
    // kanban, then create_card) → deleted at once. edit_column {delete} of the person's column with a card → asks.
    // A board no kanban shows asks the same (no block needed).
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum-mcp control` then `cargo test -j 4 -p templum
  workspace::tests::board_tools workspace::tests::content` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say; `content.rs`'s block-server tools keep their checks and words.
- [ ] **Step 4: Run them** → PASS; then `cargo test -j 4 -p templum workspace::tests::templum_agent` → PASS.
- [ ] **Step 5: Docs.** `templum-agent.md`'s tools table: the `create_card, edit_card, move_card, comment_card` row's
  arguments name the new ones; new rows `delete_card` (Ask own), `edit_board` and `edit_column` (Ask own for a delete
  with cards; – otherwise), `show_board` (–), `edit_comment`, `delete_comment` (its own; –), batch "171 2d"; the batch-3
  rows `rename_board, add_column, rename_column` and `delete_card, delete_column, delete_board` go (these replace
  them). `docs/api.md` **Boards.** and `docs/specs/api.md`: "`edit_comment` and `delete_comment` act as you: edit a
  comment by `you`, delete any."

---

### Task 4: One door for every write of a kanban

**Files:**
- Modify: `crates/templum/src/workspace/blocks.rs` (`BoardAct`, `Workspace::act_on_boards`, `Workspace::fresh_id`)
- Modify: `crates/templum/src/kanban.rs` (every `edit_boards`, `ws.boards.next()`, `keep_draft`, `drop_draft` of the
  view and the dialog: add column, both `on_drop`s of a column, a card's `on_drop`, `add_items`, `card_id`, the
  comment field and thread, Delete card, Discard, Save draft, Done; `Work` gains `space: String`),
  `crates/templum/src/menu/build.rs` (Rename board…, Delete board…, the column's Rename…/Delete, Delete card),
  `crates/templum/src/list_dialog.rs` callers in `kanban::open_boards`, `crates/templum/src/workspace/persist.rs`
  (the window's close keeps a draft only of a local card), `crates/templum/src/workspace/remote.rs`
  (`RemoteSpace::fresh_id`)
- Test: `crates/templum/src/workspace/tests/kanban.rs`, `crates/templum/src/workspace/remote.rs` tests

**Interfaces:**
- Produces:

```rust
/// What the person does to a kanban's boards: on this computer an edit of `Workspace::boards` (one Ctrl+Z step
/// each), on a mirrored workspace a call of home's.
pub(crate) enum BoardAct {
    /// `card` into `column`, in the place of card `to` (the end without one), as a drop puts it.
    MoveCard { board: u64, card: u64, column: u64, to: Option<u64> },
    /// `column` into the place of column `to`, as a drop puts it.
    MoveColumn { board: u64, column: u64, to: u64 },
    AddColumn { board: u64 },
    RenameColumn { board: u64, column: u64, title: String },
    DeleteColumn { board: u64, column: u64 },
    RenameBoard { board: u64, name: String },
    DeleteBoard { board: u64 },
    /// Kanban `block` shows `board`, or a new board (None).
    Show { block: BlockId, board: Option<u64> },
    DeleteCard { board: u64, card: u64 },
    /// Done on a card's dialog.
    Finish { board: u64, draft: Draft },
    Comment { board: u64, card: u64, text: String },
    EditComment { board: u64, card: u64, comment: u64, text: String },
    DeleteComment { board: u64, card: u64, comment: u64 },
    /// This computer only: a deleted comment's Undo, Save draft, Discard.
    RestoreComment { board: u64, card: u64, index: usize, comment: Comment },
    KeepDraft(Draft),
    DropDraft(u64),
}
```

  `Workspace::act_on_boards(&mut self, space: &str, act: BoardAct, window: &mut Window, cx: &mut Context<Self>)`:
  for a local `space`, today's edit (the same `Boards` call each site made), then `cx.notify()`; for a remote one
  nothing yet (Task 6 sends it home). `Workspace::fresh_id(&mut self, space: &str) -> u64`: `self.boards.next()` for
  a local space, `RemoteSpace::fresh_id()` for a remote one (ids counting down from `u64::MAX`, never home's).
  `kanban::Work.space`: the workspace the dialog was opened on (`ws.space.id`); the window's close keeps its draft only
  when that workspace is local.

- [ ] **Step 1: Write the failing tests.** `remote.rs` tests: `ids_handed_on_a_mirror_never_meet_home_s` —
  `RemoteSpace::fresh_id` answers `u64::MAX`, then `u64::MAX - 1`. `tests/kanban.rs`:

```rust
#[gpui_kit::test]
fn every_act_on_a_board_is_one_ctrl_z_step_through_one_door(cx: &mut TestAppContext) {
    // A local board: act_on_boards(first, each of MoveCard, MoveColumn, AddColumn, RenameColumn, DeleteColumn,
    // RenameBoard, DeleteCard, Comment, EditComment, DeleteComment) → boards as the old call left them, one
    // boards_undo step each; Show { block, board: None } shows a new board.
}

#[gpui_kit::test]
fn a_remote_workspace_s_board_acts_write_nothing_here(cx: &mut TestAppContext) {
    // offline() mirror with a data folder: act_on_boards("r1", …) for every act, and fresh_id("r1"): ws.boards and
    // boards.json unchanged, byte for byte (Task 6 replaces this test with the calls each sends).
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::kanban workspace::remote` → FAIL.
- [ ] **Step 3: Implement** `BoardAct`, `act_on_boards`, `fresh_id` and move every site onto them; `open_card` and
  `add_card` record `ws.space.id` in `Work.space`. Behaviour on this computer is unchanged.
- [ ] **Step 4: Run them, and every kanban test:** `cargo test -j 4 -p templum workspace::tests::kanban
  workspace::tests::card_draft workspace::tests::attachments workspace::tests::menus` → PASS.

---

### Task 5: A remote's boards, copied from home's state and drawn by the kanban's view

**Files:**
- Create: `crates/templum/src/workspace/remote_boards.rs` (`mod remote_boards;` in `workspace/mod.rs`): the caps,
  `card_of`, `RemoteSpace::mirror_boards`
- Modify: `crates/templum/src/kanban.rs` (`Kanban.remote`; `shown`/`title` read `ws.boards_of`; `body`, `board_view`,
  `column_view`, `card_view`, `meta_row`, `rows`, `add_card`, `open_card` and the dialog read the `&Boards` they are
  given or `ws.boards_in(&work.space)`; `assignee_label` and the comments' authors name blocks with `uid_title_in`)
- Modify: `crates/templum/src/workspace/mod.rs` (`Body::kanban()`/`kanban_mut()` None for `remote`; `boards_of`,
  `boards_in`, `uid_title_in`), `crates/templum/src/workspace/remote.rs` (`RemoteSpace.boards`; `remote_body` and
  `mirror_blocks` for kind `"kanban"`; `apply_remote` and `remirror` call `mirror_boards`; `NOT_MIRRORED`'s words),
  `crates/templum/src/menu/mod.rs` (the kanban, column and card items read `boards_in(&self.space.id)`)
- Test: `crates/templum/src/workspace/remote_boards.rs` tests; create `crates/templum/src/workspace/tests/
  remote_boards.rs` (`mod remote_boards;` in `tests/mod.rs`); rewrite the kanban arms of `tests/remote.rs`
  `home_s_blocks_appear_move_and_go`, `tests/mirror_end_to_end.rs` `a_workspace_mirrors_itself_over_loopback` (step 1:
  one viewer, one card, one live kanban), `tests/menus.rs` `a_remote_s_menus_open_with_what_works_there` (k1 is a
  kanban's menu), and any test pinning `NOT_MIRRORED`

**Interfaces:**
- Produces: `Kanban { pub board: Option<u64>, pub remote: bool }` (`remote`: a kanban of a mirrored workspace; its
  board is home's, in `RemoteSpace::boards`); `Workspace::boards_of(&self, kanban: &Kanban) -> Option<&Boards>`
  (local: `&self.boards`; remote: the shown remote's copy, None when the shown workspace isn't remote — a remote
  kanban is drawn only while its workspace is shown); `Workspace::boards_in(&self, space: &str) -> Option<&Boards>`;
  `Workspace::uid_title_in(&self, space: &str, uid: &str, cx: &App) -> Option<String>` (local: `uid_title`; remote:
  the title `block::title` draws for the block home calls `uid`); `kanban::assignee_label(ws, space: &str, assignee,
  cx)` (an agent on a remote: "Any {home's title of it from `RemoteSpace::agents`, else its id}").
- Produces (`remote_boards.rs`): the constants of Global Constraints; `pub(super) fn card_of(view: &CardView) ->
  Card`; `RemoteSpace.boards: Boards` (never saved; `drafts` always empty); `RemoteSpace::mirror_boards(&mut self,
  state: &State, now: Instant)`: rebuilds `self.boards` from `state.boards` and `state.cards` by the caps; a card whose
  `Arc` is the one `self.state.cards` holds for its id (`Arc::ptr_eq`) is moved over from the old copy as it is, not
  cleaned again (so it is called before `remote.state = state`); Tasks 6–7 add the holds over it.
- `apply_remote` calls `mirror_boards` when `!same(&state.boards, &remote.state.boards) || !same(&state.cards,
  &remote.state.cards)`; `remirror` always. `remote_body` makes `Body::Kanban(Kanban { board: b.board, remote: true
  })` for kind `"kanban"`; `mirror_blocks` keeps its `board` home's. `NOT_MIRRORED`: "Not mirrored yet: notes',
  checklists' and editors' content (a card shows each), ink and shapes. Not on a mirrored workspace yet: editors,
  adding notes, drawing, grid mode, Move to workspace, Copy and Paste of blocks, Stop, the Git popup, a card's
  attachments."

- [ ] **Step 1: Write the failing tests.** `remote_boards.rs`:

```rust
#[test]
fn home_s_cards_are_cleaned_and_capped() {
    // A CardView with title "a\x1b[31m\nb" + 1000×"t", description "x\r\n\x07y" + 100_000×"d", 600 items with
    // texts of 5000 chars, 300 comments of 20_000 chars by "b1\nx", labels ["red", "purple", "red"], priority
    // "urgent", assignee block "b1\n", 150 attachments one named "../../.ssh/id_ed25519".
    // card_of: title one line ≤ 300 chars, no ESC; description has "\n", no '\r' or '\x07', ≤ 65_536 chars;
    // 500 items each ≤ 4000 chars; the newest 200 comments (the last of home's), each ≤ 16_384 chars, by one line;
    // labels [Red]; priority None; assignee None; 100 attachments, names one line (shown only, never opened).
}

#[test]
fn a_board_keeps_its_columns_and_cards_each_once_and_capped() {
    // A board with 150 columns; its first column lists card 40 twice, 999 (not in cards) and 600 cards; card 41
    // listed in two columns; a card of another board listed here. mirror_boards: 100 columns; the first column
    // holds 40 once and 500 cards; 41 in its first column only; the stranger and 999 nowhere.
}

#[test]
fn a_card_home_didn_t_change_is_not_cleaned_again() {
    // mirror_boards(state) then, after remote.state = state, mirror_boards of a state with the same Arc for card 40
    // and a new one for 41: card 40's description has the same `as_ptr()` as before, card 41's is home's new text.
}
```

`tests/remote_boards.rs` (a helper `live_board(cx) -> (handle, ws, dir)`: `live_terminals(cx, &["b1"])` and a state
whose workspace w1 holds terminal b1 ("claude") and kanban `k1` showing board 12 "Sprint": To do (30) `[40]`, Doing
(31) `[41, 42]`, Done (32) `[]`; card 40 "Fix login", labels red, for block b1, two comments, one by "b1"; the
mirror's own `ws.boards` empty, and its `boards.json` written once):

```rust
#[gpui_kit::test]
fn a_remote_kanban_draws_home_s_board(cx: &mut TestAppContext) {
    // render_frame: ("columns", n) and "card-{n}-40", "card-{n}-41" are drawn; the block's body is
    // Body::Kanban(Kanban { board: Some(12), remote: true }); block::title is "Sprint"; "assignee-{n}-40" reads
    // "claude"; ws.boards.boards is empty and boards.json is as it was, byte for byte, after a save.
}

#[gpui_kit::test]
fn no_local_tool_sees_a_remote_board(cx: &mut TestAppContext) {
    // A local workspace's kanban shows a local board whose id is 12 too, named "Mine". With r1 shown:
    // ws.board_open_at(12) is the local workspace; ws.space.kanban(k1) is None; the local kanban's title (through
    // uid_title of its uid) is "Mine"; a local block given uid "b1" (others[0].uids) is not card 40's assignee label;
    // Ctrl+Z with k1 focused leaves ws.boards as it was.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::remote_boards workspace::tests::remote_boards` →
  FAIL.
- [ ] **Step 3: Implement** as the Interfaces say. The kanban's view and dialog read the copy and only the copy;
  thumbnails, `Card::text`'s attachment paths and opening a file stay for a local card (Task 7 draws a remote card's
  attachments). Run the rewritten tests: `cargo test -j 4 -p templum workspace::tests::remote
  workspace::tests::menus workspace::tests::mirror_end_to_end` → PASS.
- [ ] **Step 4: Run them** (Step 2's) → PASS; `cargo test -j 4 -p templum workspace::tests::kanban` → PASS.

---

### Task 6: Acting on a remote board: drags, columns, boards, Delete card, Send to

**Files:**
- Create: the remote arm in `crates/templum/src/workspace/remote_boards.rs` (`Workspace::remote_board_act`,
  `card_before`, `column_before`, the card holds over the copy)
- Modify: `crates/templum/src/workspace/blocks.rs` (`act_on_boards` sends a remote space's act to
  `remote_board_act`), `crates/templum/src/workspace/remote.rs` (the new `Deed`s and their `call`; `RemoteSpace`'s
  `cards_moved`, `cards_deleted`; `expire_holds` expires them and redraws through `remirror`),
  `crates/templum/src/kanban.rs` (the body's gates: Add a column, Add a card, the empty block's New board and Open a
  board… dimmed with the words; `open_boards` on a remote), `crates/templum/src/menu/{mod.rs,build.rs}` (`Gates` and
  `Gates::{of, why}` `pub(crate)`; the kanban, column and card items gated; Send to ▸ on a remote lists its
  terminals), `crates/templum/src/workspace/blocks.rs` (`send_to` on a remote: `remote_send_to`)
- Test: `crates/templum/src/workspace/remote_boards.rs` tests, `crates/templum/src/workspace/tests/remote_boards.rs`
  (replaces Task 4's `a_remote_workspace_s_board_acts_write_nothing_here`), `crates/templum/src/workspace/remote.rs`'s
  `every_deed_home_runs_has_a_scope_in_the_registry` (the new deeds)
- Docs: `docs/minors.md`

**Interfaces:**
- Consumes: Task 4's `BoardAct`; Task 5's copy; Tasks 1–2's tools.
- Produces: `pub(super) fn card_before(board: &Board, card: u64, column: u64, to: Option<u64>) -> Option<Option<u64>>`
  (where the drop puts the card, as `Boards::move_card` puts it, said as the card that then follows it: `Some(None)`
  the end; None when the drop changes nothing); `pub(super) fn column_before(board: &Board, column: u64, to: u64) ->
  Option<Option<u64>>` (the same for `move_column`); `RemoteSpace.cards_moved: Holds<u64, (u64, Option<u64>)>` (card →
  column and the card after it) and `RemoteSpace.cards_deleted: Holds<u64, bool>`, laid over the copy by
  `mirror_boards` (a deleted card left out; a moved card placed with `move_card_before`; each ends when home's state
  shows it, at a refusal, or after `HOLD`); `Workspace::remote_board_act(&mut self, space: &str, act: BoardAct, window,
  cx)`; `Workspace::remote_terminals(&self) -> Vec<(BlockId, String)>` (the shown remote's viewers that take input, by
  title) and `Workspace::remote_send_to(&mut self, to: BlockId, text: &str, window, cx) -> bool` (`typing_into`, then
  `Command::Paste`, at most 256 KiB, then reveal; false when nothing was sent).
- What each act sends (arguments exactly; `board` as its id's string, `column` of `move_card` too):

  | Act | Call | Held | Mark |
  |---|---|---|---|
  | `MoveCard` | `move_card {card, column, before}` (no `before` at the end, or for an older home) | `cards_moved` | yes |
  | `MoveColumn` | `edit_column {board, column, before}` (`before` a column id or `"end"`) | no | yes |
  | `AddColumn`, `RenameColumn`, `DeleteColumn` | `edit_column {board}`, `{…, column, title}`, `{…, column, delete: true}` | no | yes |
  | `RenameBoard`, `DeleteBoard` | `edit_board {board, name}`, `{board, delete: true}` | no | yes |
  | `Show` | `show_board {block: home's id, board}` or `{block, new: ""}` | no | no |
  | `DeleteCard` | `delete_card {card}` | `cards_deleted` | yes |

  The deed is asked first (`why_not`; refused: `warn` its words, nothing sent); a drop that changes nothing sends
  nothing; a hold is laid and the copy redrawn (`remirror`) before the call; a mark (`mark_remote`) only when `space`
  is the shown one. A refusal ends the act's hold, redraws home's last state and says the sentence. Task 7 fills
  `Finish` and the comments; `RestoreComment`, `KeepDraft`, `DropDraft` do nothing on a remote.
- The Boards dialog on a remote lists the copy's boards (their cards' count, no "Open in"), New board is `Show {
  board: None }`, a row `Show`, the trash `DeleteBoard`; each acts only while its workspace is still shown
  (`still_shown`).

- [ ] **Step 1: Write the failing tests.** `remote_boards.rs`: `card_before_is_the_card_that_follows_where_the_drop_
  puts_it` — Doing `[C, A, D]`: A dropped on D (down its own column) → `Some(None)` (`move_card` leaves `[C, D, A]`);
  D dropped on C → `Some(Some(C))`; B of To do dropped on A → `Some(Some(A))`; B dropped on Doing's empty space (`to`
  None) → `Some(None)`; A dropped on itself → None.
  `column_before_…` likewise. `tests/remote_boards.rs`:

```rust
#[gpui_kit::test]
fn a_card_dropped_on_another_sends_move_card_before_it_and_shows_at_once(cx: &mut TestAppContext) {
    // live_board; render; press on "card-{n}-40", move 10 px, move onto "card-{n}-42", release (a helper
    // `drop_on(handle, cx, from, to)` in tests/mod.rs: no sleeps). sent == [("move_card", {"card": 40, "column":
    // "31", "before": 42})]; the copy's Doing is [41, 40, 42] at once; ws.space.undo ends with Change::Mark;
    // ws.boards untouched. A state from home with Doing [41, 40, 42] ends the hold (cards_moved.get(&40) None).
    // Again with no state: expire_holds(now + HOLD) puts 40 back in To do. Again, answered with refusal("Card 42
    // isn't in Doing."): 40 back at once and said once.
}

#[gpui_kit::test]
fn columns_and_boards_are_edited_at_home(cx: &mut TestAppContext) {
    // + Add a column → ("edit_column", {"board": "12"}); the column menu's Rename… → {"board": "12", "column": 31,
    // "title": "Review"}; dragging Done's title onto To do → {"board": "12", "column": 32, "before": 30}; Delete of
    // Doing (2 cards) asks first, then {"board": "12", "column": 31, "delete": true}; Rename board… → ("edit_board",
    // {"board": "12", "name": "Sprint 2"}); Delete board… asks counting 3 cards, then {"board": "12", "delete":
    // true}. The copy is unchanged until home's state; ws.boards untouched.
}

#[gpui_kit::test]
fn all_boards_lists_the_share_s_boards_and_shows_one_at_home(cx: &mut TestAppContext) {
    // A second board 13 "Ops" in the state. kanban::open_boards(k1): rows 12 and 13 from the copy; a click on 13 →
    // ("show_board", {"block": "k1", "board": "13"}); New board → {"block": "k1", "new": ""}. A dialog opened, then
    // the shown workspace switched to the local one: its click sends nothing and says "Another workspace is shown
    // now: nothing was done."
}

#[gpui_kit::test]
fn a_card_deleted_on_the_mirror_hides_until_home_drops_it(cx: &mut TestAppContext) {
    // The card menu's Delete card asks, then ("delete_card", {"card": 40}); the copy lacks 40 at once; after HOLD
    // with no state it is back; a refusal brings it back at once and is said.
}

#[gpui_kit::test]
fn an_older_home_takes_the_card_at_its_column_s_end_and_dims_what_it_lacks(cx: &mut TestAppContext) {
    // Update::Tools whose move_card lacks before and with no delete_card/edit_column: the drop sends ("move_card",
    // {"card": 40, "column": "31"}) and holds 40 at Doing's end; why_not(Deed::DeleteCard) and Deed::EditColumn are
    // Nay::Older; + Add a column is dimmed; its click sends nothing.
}

#[gpui_kit::test]
fn send_to_on_a_remote_card_lists_its_terminals_and_pastes_there(cx: &mut TestAppContext) {
    // Full share: the card menu's Send to ▸ lists "claude" (b1's viewer) and no local terminal; remote_send_to(b1's
    // viewer, card's text) is true. Control share: Send to ▸ is a dimmed item, "Needs a Full share: share again on
    // …", and remote_send_to is false.
}

#[gpui_kit::test]
fn on_a_mirror_ctrl_z_after_a_card_act_stops_at_the_mark(cx: &mut TestAppContext) {
    // A block moved (held), then a card dropped: Ctrl+Z with k1 focused says "On a mirror, Ctrl+Z undoes moves only:
    // undo the rest on …" and moves no block; the next Ctrl+Z undoes the move.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::remote_boards workspace::tests::remote_boards
  workspace::remote` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say. Gates read once per frame in `kanban::body` for a remote kanban
  (`Gates::of(ws, &[Deed::EditColumn, Deed::CreateCard, Deed::MoveCard, Deed::ShowBoard])`): dimmed, with the words
  as tooltip, their clicks doing nothing; a refused drop says the words once. The kanban header's menu: All boards…
  (`ShowBoard`), Rename board… and Delete board… (`EditBoard`); a column's: Rename… and Delete (`EditColumn`), Add a
  card (`CreateCard`); a card's: Open, Send to ▸ (`Type`), Delete card (`DeleteCard`). `Card::text(None)` for a
  remote card's Send to (no paths of this computer's).
- [ ] **Step 4: Run them** → PASS; `cargo test -j 4 -p templum workspace::tests::kanban workspace::tests::menus` →
  PASS.
- [ ] **Step 5: Minors.** Delete "A kanban added from a mirror shows a new board." (its fix, All boards… on a remote,
  lands). Add: "A column dragged on a mirror moves when home's state comes" (`workspace/remote_boards.rs`,
  `remote_board_act`; a column move isn't held, so it shows a round trip later; fix: a `columns_moved` hold);
  "The board's dimmed controls and its menus on a remote were checked headlessly only" (`kanban.rs`, `menu/build.rs`;
  fix: look at them with two `--dev` instances, one a Control share, one an older home).

---

### Task 7: The card dialog on a remote

**Files:**
- Modify: `crates/templum/src/workspace/remote_boards.rs` (`edit_args`, `create_args`, `created_card`;
  `RemoteSpace.cards_edited`; `remote_board_act`'s `Finish`, `Comment`, `EditComment`, `DeleteComment`)
- Modify: `crates/templum/src/kanban.rs` (on a remote card: Save draft gone and `ask_to_leave` offers Save and Discard
  only; a deleted comment has no Undo row; attachments as names and sizes, no thumbnail, no opening, Attach dimmed with
  "Attachments are on winx-desktop: open them there.", files dropped or pasted on the dialog attach nothing; the
  assignee menu lists the remote's terminals, Anyone and Any session of home's agents; Done, Comment, a comment's
  edit and delete, and Delete card dimmed by their deeds' words), `crates/templum/src/workspace/remote.rs`
  (`expire_holds` for `cards_edited`)
- Test: `crates/templum/src/workspace/remote_boards.rs` tests, `crates/templum/src/workspace/tests/remote_boards.rs`
- Docs: `docs/minors.md`

**Interfaces:**
- Produces: `pub(super) fn edit_args(card: u64, draft: &Draft) -> Option<serde_json::Value>` (`edit_card`'s
  arguments for only what the person changed between `draft.base` and `draft.work`: `title`, `description`,
  `priority` (its name, or `"none"`), `for` (a block's id, an agent's id, or `"anyone"`), `labels` (each label whose
  being on changed), `add_items` (the new items, a line each, `[x] ` before a checked one), `remove_items`, `check`,
  `uncheck`; None when nothing changed); `pub(super) fn create_args(board: u64, draft: &Draft) -> serde_json::Value`
  (`create_card`'s `board`, `column` (the new card's `draft.at`), `title`, and what is set of `description`,
  `priority`, `for`, `labels`, `checklist`); `pub(super) fn created_card(answer: &serde_json::Value) -> Option<u64>`
  (the number of home's
  "Created card 77."); `RemoteSpace.cards_edited: Holds<u64, Draft>`, laid over the copy as `Draft::merge_into` with the
  items it adds left out (their ids are the mirror's own; they show when home's state brings them), ended when merging
  changes nothing of home's card, at a refusal, or after `HOLD`; a card the last build laid an edit over is cleaned
  again from home's `Arc` at the next, not moved over.
- `Finish` of a card that exists: Done's arguments go only for keys home takes (`takes("edit_card", key)`; one it
  lacks: the `Nay::Older` words, the dialog stays open, nothing sent); held in `cards_edited`. Of a new card:
  `create_card`, then, once home answers its number, one `comment_card {card, text}` per comment of the work, in
  order. A refused Done (either) says the sentence and, when no dialog is open, reopens the card's dialog with the
  person's work (a new card's comments included). `Comment` → `comment_card {card, text}`; `EditComment` →
  `edit_comment {card, comment, text}`; `DeleteComment` → `delete_comment {card, comment}`; none held (they show a
  round trip later); each leaves a mark. A new card's items and comments take `fresh_id` ids.

- [ ] **Step 1: Write the failing tests.** `remote_boards.rs`:

```rust
#[test]
fn edit_args_say_only_what_changed() {
    // base: title "A", labels [Red], items [13 open, 14 done], priority None, assignee None. work: title "B", labels
    // [Red, Blue], item 13 checked, 14 removed, a new item "ship" (id u64::MAX), priority High, assignee
    // Block("b1"). edit_args(40, &draft) == json!({"card": 40, "title": "B", "labels": {"blue": true}, "add_items":
    // "[ ] ship", "remove_items": [14], "check": [13], "priority": "high", "for": "b1"}); the same draft with work ==
    // base → None; assignee back to None → "for": "anyone".
}

#[test]
fn a_new_card_s_arguments_carry_what_it_has_and_home_s_answer_names_it() {
    // create_args(12, draft at column 30, title "Login", labels [Red, Blue], items "a" (done), "b") == json!({"board":
    // "12", "column": "30", "title": "Login", "labels": ["red", "blue"], "checklist": "[x] a\n[ ] b"});
    // created_card(&json!({"text": "Created card 77."})) == Some(77); a text that isn't home's → None.
}
```

`tests/remote_boards.rs`:

```rust
#[gpui_kit::test]
fn done_on_a_remote_card_sends_only_what_changed_and_shows_it_at_once(cx: &mut TestAppContext) {
    // open_card(k1's board, 40): the dialog's title shows "Fix login". Type a new title, toggle blue, check item 13;
    // a state from home meanwhile adds item 15 to card 40 (the dialog shows it). Done → sent == [("edit_card",
    // {"card": 40, "title": "Fixed", "labels": {"blue": true}, "check": [13]})]; the copy's card 40 is "Fixed" with
    // blue at once and item 15 once; a state from home showing it ends the hold.
}

#[gpui_kit::test]
fn a_new_remote_card_is_created_then_its_comments_are_sent(cx: &mut TestAppContext) {
    // Add a card in To do on k1, title "Login", labels red and blue, comment "First" (in the work, a fresh_id).
    // Done → calls[0] == ("create_card", {"board": "12", "column": "30", "title": "Login", "labels": ["red",
    // "blue"]}); answer(0, Ok(json!({"text": "Created card 77."}))) → the next call is ("comment_card", {"card": 77,
    // "text": "First"}). ws.boards untouched; no draft anywhere.
}

#[gpui_kit::test]
fn a_refused_done_reopens_the_card_with_what_the_person_made(cx: &mut TestAppContext) {
    // A new card "Login" with a comment: Done, then answer(0, refusal("That column is gone.")) → said once, and the
    // dialog is open again with title "Login" and the comment; Esc in it asks Save / Discard (no Save draft).
}

#[gpui_kit::test]
fn comments_on_a_remote_card_are_home_s_calls(cx: &mut TestAppContext) {
    // Comment "ok" → ("comment_card", {"card": 40, "text": "ok"}); editing the comment by "you" → ("edit_comment",
    // {"card": 40, "comment": c, "text": "ok!"}); deleting b1's → ("delete_comment", {"card": 40, "comment": c2}),
    // with no Undo row; Delete card in the dialog asks, then ("delete_card", {"card": 40}).
}

#[gpui_kit::test]
fn a_remote_card_s_dialog_keeps_no_draft_here(cx: &mut TestAppContext) {
    // With a data folder: change the title, then Esc → only Save and Discard; Discard → nothing sent. Change it again
    // and close the window (persist's keep) → ws.boards.drafts empty, boards.json as it was, byte for byte. Change it
    // again, switch the shown workspace to the local one (an agent's show), Done → still ("edit_card", {"card": 40,
    // …}) to r1, and the local boards untouched.
}

#[gpui_kit::test]
fn a_remote_card_shows_its_attachments_by_name_only(cx: &mut TestAppContext) {
    // Card 40 with attachment "shot.png" (12 KiB) at home, and a local card 40 whose <data>/attachments/40/shot.png
    // exists: the remote dialog shows "shot.png" and its size, no thumbnail element, no open on click; Attach is
    // dimmed with "Attachments are on … : open them there." (computer_of); a file dropped on the dialog attaches
    // nothing and sends nothing.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::remote_boards workspace::tests::remote_boards` →
  FAIL.
- [ ] **Step 3: Implement** as the Interfaces say.
- [ ] **Step 4: Run them** → PASS; `cargo test -j 4 -p templum workspace::tests::kanban workspace::tests::card_draft
  workspace::tests::attachments` → PASS.
- [ ] **Step 5: Minors.** Add: "A comment deleted on a mirror has no Undo" (`kanban.rs`, the thread; home's Ctrl+Z on
  its kanban brings it back; fix: `comment_card` of the text, which would lose its author and time, or a restore at
  home); "`GET /v1/tools` describes `edit_comment` and `delete_comment` as the Templum Agent's"
  (`templum-mcp/src/control.rs`, `SHARED`; through the API they edit the person's comments and delete any, as
  docs/api.md says; fix: the registry's own descriptions for the shared tools); "The card dialog on a remote was
  checked headlessly only" (`kanban.rs`, `open_draft`; fix: look at it with two `--dev` instances).

---

### Task 8: End to end, the bench, the docs and the full check

**Files:**
- Test: `crates/templum/src/workspace/tests/mirror_end_to_end.rs` (three cases),
  `crates/templum/src/workspace/tests/remote_boards.rs` (the two-ends case)
- Modify: `crates/templum/src/bench.rs` (`mirror_set_up`: the mixed cockpit's two kanbans each show a
  `sample_board`, so the mirror draws 12 cards in each, live; `--bench-mixed` unchanged)
- Docs: `docs/api.md` (Templum to Templum ▸ 7), `docs/specs/kanban.md` (a section "On a mirrored workspace (Templum
  to Templum, part 2d)": drawn from home's state, every act home's tool, the holds, no drafts, attachments by name, the
  assignee list), `docs/specs/templum-to-templum.md` (Status: "parts 2a, 2b, 2c and 2d built (plan …-2d-boards)";
  "As built ▸ Batch 2d": what the builder chose or changed, and this plan's "Open for the orchestrator" as settled),
  `docs/ARCHITECTURE.md` (`workspace/remote_boards.rs`: a remote's copy of home's boards and the acts on them;
  `act_on_boards`, the one door of a kanban's writes), `docs/minors.md`

- [ ] **Step 1: Write the end-to-end cases** (one workspace both ends; `board_with_a_card` before `mirrored`, then
  more cards so To do is `[A, B]` and Doing `[C, D]`; on `r1` the acts go through `act_on_boards("r1", …)`, the drop
  itself being Task 6's test):

```rust
#[gpui_kit::test]
fn a_card_dragged_on_the_mirror_lands_at_home_before_that_card(cx: &mut TestAppContext) {
    // act_on_boards("r1", MoveCard { board, card: A, column: doing, to: Some(D) }) → pump_home until ws.boards (home's)
    // Doing is [C, A, D]; r1's copy agrees and cards_moved is empty.
}

#[gpui_kit::test]
fn a_card_made_on_the_mirror_has_its_labels_and_its_comment_at_home(cx: &mut TestAppContext) {
    // Finish { board, draft: new in To do, title "Login", labels [Red, Blue], one comment "First" } → pump_home until
    // home's To do ends with "Login", labels [Red, Blue], comments [by "you", "First"].
}

#[gpui_kit::test]
fn a_column_added_and_renamed_on_the_mirror_is_at_home(cx: &mut TestAppContext) {
    // AddColumn → pump_home until home's board has "New column" last (and r1's copy has it); RenameColumn of its id to
    // "Review" → pump_home until home's is "Review".
}
```

`tests/remote_boards.rs`, two ends (`shown_mirror_as(cx, Scope::Control)`; home's kanban shows a board with two
columns and a card; the mirror's data folder's `boards.json` read before):

```rust
#[gpui_kit::test]
fn acts_on_a_mirrored_board_leave_the_mirror_s_boards_json_as_it_was(cx: &mut TestAppContext) {
    // On the mirror: MoveCard, AddColumn, RenameColumn, Finish (a new card with a comment), Comment, DeleteCard,
    // Show { board: None }; pump_home until home's boards show each; save the mirror (save_canvas_now and the
    // boards' save): the mirror's boards.json is byte for byte as before and its ws.boards has no board.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::mirror_end_to_end
  workspace::tests::remote_boards` → all pass (fix what fails in the task that owns it).
- [ ] **Step 3: Write the docs** above. `docs/api.md` ▸ 7: "board's" leaves "Not mirrored"; add "A kanban is live:
  dragging cards and columns, adding, renaming and deleting columns and boards, the card dialog and Send to work from
  the mirror, each done at home; a card you move, edit or delete shows at once and home's state wins after two
  seconds; a card's attachments show by name (open them on home), and a card has no drafts on a mirror."
  `docs/minors.md`:
  what Tasks 6–7 added and removed, and any UI checked only headlessly.
- [ ] **Step 4: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 5: The benches** (the mirror paints kanbans now): `cargo run -j 4 --release -p templum -- --bench` then
  `cargo run -j 4 --release -p templum -- --bench-mirror` → `p95_ms` ≤ 16.7 each; report both JSON lines, and 2c's
  `--bench-mirror` line beside the new one.
- [ ] **Step 6: Leave uncommitted;** report the files changed, the checks' and the benches' results, what went to and
  left `docs/minors.md`, and how each UI-only change was verified.

---

## Interfaces for later parts

What 2d leaves for 2e–2h, by name:

- **One door for a block's writes:** `BoardAct` + `Workspace::act_on_boards(space, act, …)` (local: the store's edit;
  remote: `remote_board_act`, asked of `why_not`, held, marked). 2e's checklist items follow the same shape
  (`check_item`, `add_items`, `edit_item`, `delete_item` from one door) instead of branching at each site.
- **A remote body that local lookups skip:** `Kanban::remote` with `Body::kanban()` None for it; 2e gives the
  checklist and note bodies the same mark, so `Body::checklist()`/`note()` never meet home's ids.
- **Reading a remote's copies:** `Workspace::boards_in(space)`, `boards_of(kanban)`, `uid_title_in(space, uid)`
  (2e's checklists and notes; 2h's Agents popup names home's blocks with `uid_title_in`).
- **Send to ▸ on a remote:** `Workspace::remote_terminals()` and `remote_send_to(to, text)` for 2e's notes and 2g's diff
  lines.
- **Cleaning home's text:** `remote_boards.rs`'s caps and `card_of` beside `remote.rs`'s `clean_text`; 2e's checklist
  items reuse `ITEM_CHARS` (4,000) and the copy's per-`Arc` rebuild of `mirror_boards`.
- **Holds over a copy:** `cards_moved`, `cards_deleted`, `cards_edited` are the pattern for 2e's `items_checked`: laid
  over the copy where it is built, ended by home's state, a refusal or `HOLD`, expired in `expire_holds`, redrawn by
  `remirror` (which now also rebuilds the boards).
- **Home:** `Boards::{move_card_before, move_column_before}`; `Workspace::{comment_edit, comment_delete, comment_of,
  card_delete, board_change, column_change, board_show}`; the gate's board path (`board_ask`), for any later control
  call whose target isn't a block; `templum_mcp::control::Before`, `tools::LABELS`.
- **Deeds:** `MoveCard`, `CreateCard`, `EditCard`, `CommentCard`, `EditComment`, `DeleteComment`, `DeleteCard`,
  `EditColumn`, `EditBoard`, `ShowBoard`. `menu::Gates` is `pub(crate)` for bodies' own controls.
- **Benches:** 2e's `--bench-mirror` compares with Task 8's line (kanbans live; notes and ink still cards).

## Open for the orchestrator

Each decided with the recommended option; the plan is written that way.

1. **A remote kanban is `Body::Kanban` with `Kanban::remote`,** and `Body::kanban()`/`kanban_mut()` answer None for
   it, instead of a body arm of its own: the spec's "the same Body, drawn by the same view" holds, and every local
   lookup by board id (`board_open_at`, `board_shown`, `agent_sessions`, Ctrl+Z on a kanban, `overview`, the API's
   state) skips home's ids by construction. *Recommended.*
2. **Every write of a kanban goes through one door** (`BoardAct`, `act_on_boards`), the local arm today's edits
   unchanged, instead of an `if remote` at each of ~15 sites of the view, the menus and the dialog. *Recommended.*
3. **`before` means "goes right before"** at home (`move_card_before`, `move_column_before`), and the mirror computes it
   from where the local drop puts the card (`card_before`), since a drop moving a card down its own column lands it
   *after* the card it was dropped on. *Recommended.*
4. **Holds:** card moves, card edits (Done) and card deletes are held, as the spec's holds and the brief's "moves/edits"
   say; a column drag, renames, new columns, new cards and comments, and a board shown show a round trip later (a
   minors entry for column drags). *Recommended.*
5. **Card, column and board acts leave an undo mark** (the spec's "any other act leaves a mark"; 2b's interface said
   none was planned): Ctrl+Z on a focused remote kanban then says the sentence instead of undoing an older block move
   by surprise. Showing a board leaves none (nothing at home undoes it). *Recommended.*
6. **A new card's number is read from home's answer** ("Created card 77."), the words bound by Task 1's test, as 2b
   reads `list_agents`, not a new field in the API's answer. *Recommended.*
7. **A refused Done reopens the card's dialog with the person's work** (when no dialog is open), since a mirror keeps
   no drafts and the work would be lost. *Recommended.*
8. **A comment deleted on a mirror has no Undo row:** re-adding it would change its author and time; home's Ctrl+Z
   brings it back (a minors entry). *Recommended.*
9. **`show_board`'s `new` takes a blank name for "Board n"** and To do, Doing, Done without `columns` (the person's New
   board); the mirror's New board sends `new: ""`. *Recommended.*
10. **The minor "A kanban added from a mirror shows a new board" closes with All boards…** (its own named fix): home's
    `add_block` still makes a board for an added kanban, which the person switches from the header. The other way, an
    `add_block` that adds a kanban with no board, changes what agents get. *Recommended.*
11. **Caps the spec doesn't give:** 100 columns a board, 500 cards a column, 100 attachments a card; the text caps are
    counted in characters, as `clean_text` counts (the spec's 64 KiB description and 16 KiB comments are 65,536 and
    16,384 characters). *Recommended.*
12. **The assignee list on a mirror:** the remote workspace's terminals, Anyone, and Any session of home's agents
    (`RemoteSpace::agents`), as locally the list has sessions and agents. *Recommended.*
13. **The grown arguments are the block server's too** (one parser): agents in terminals can set a card's title,
    description and labels, remove items and place a card before another. *Recommended.*
14. **Through the API `edit_comment` edits comments by `you` and `delete_comment` deletes any;** the Templum Agent
    edits and deletes only its own. `GET /v1/tools` still shows the control server's description (a minors entry).
    *Recommended.*
15. **The Templum Agent's danger for deletes uses a board path in the gate** (`board_ask`, no block needed; `own` from
    `own.cards`/`own.boards`), because the gate is block-based and a call with no block target runs without asking.
    *Recommended.*
16. **`edit_column` takes `title` and `before` together** (one Ctrl+Z step); `delete` goes alone. *Recommended.*

## Found against the code as built

- **The end-to-end test is one workspace for both ends:** home's boards are the mirror's own `Workspace::boards`, with
  the same ids, so "`boards.json` on the mirror unchanged, byte for byte" can't be shown there, and a view reading the
  local boards would pass it. The byte check runs with two ends (`shown_mirror_as`: two workspaces, two data
  folders, Task 8), and the headless tests give the mirror no boards of its own or colliding ones (Task 5).
- **Local lookups by board id walk every workspace** (`board_open_at`, `board_shown`, `agent_sessions`) and Ctrl+Z on
  a focused kanban undoes `Workspace::boards`: a remote `Body::Kanban` would reach them (Open 1).
- **`Kanban::title` and `block::title` take no workspace,** and `about` titles blocks of every workspace for agents:
  a remote kanban can't be told from a local one without a mark on the `Kanban` (Open 1).
- **The card dialog writes outside `edit_boards`:** `ws.boards.next()` for new items and comments, kept drafts, an
  attachment's reserved id, and the window's close keeping a draft (`persist.rs`): each would change the mirror's
  `boards.json` (Task 4).
- **The dialog draws thumbnails from `<data>/attachments/<card id>/`:** a remote card's id would show a local card's
  files (Task 7).
- **Card assignees and comment authors resolve uids in local workspaces** (`uid_title`), and Send to ▸ lists only
  `Space::terminals()`, which skips viewers (Tasks 5, 6).
- **The gate is block-based** (`asked_about`, `find_block`): a control call with no block target runs without asking,
  so `delete_card` would never ask (Open 15).
- **The local drop puts a card "in the place of" another,** which differs from "before" when it moves down its own
  column (Open 3).
- **The mixed cockpit's kanbans show no board,** so `--bench-mirror` would draw two empty kanbans (Task 8).
- **`edit_comment`/`delete_comment`'s descriptions speak for an agent** ("a comment you wrote … Not the person's"),
  and the registry serves them as they are (Open 14).
- **Shares are always limited,** so home's state holds only the boards a share's kanbans show: All boards… lists those,
  and `show_board` of another is "There's no board …" (as the spec's "the boards the share sees").
- **The spec's "Done sends … as it applies only that locally"** maps to `Draft::merge_into`'s fields one for one, except
  attachments (dimmed on a mirror) and comments (sent at once, as locally).
