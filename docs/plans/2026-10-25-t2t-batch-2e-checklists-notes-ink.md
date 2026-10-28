# Templum to Templum, Batch 2e (Checklists, Notes and Ink, Live) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer in a Templum agent block does the whole plan, then
> fresh Opus 5.5 reviewers review it in a loop until one passes it. The implementer **commits nothing**: everything
> stays uncommitted for the orchestrator.

**Goal:** Kanban card 171 part 2, batch 2, fifth of eight parts: a checklist, a note and the ink and shapes of a
mirrored workspace are **drawn live from home's state** by the views a local block uses; a checklist's items are
**checked, added, edited and deleted from the mirror, done at home**; a note is **read-only** there (selecting, Copy,
Copy all, Find and Send to work); home's drawings are painted above the blocks and can't be picked. Home's state gains
`notes`, `checklists`, `drawings` and the blocks' `notes` and `checklist`; `edit_item` and `delete_item` move into the
control server.

**Architecture:** Home first (Tasks 1–2): three new collections in `templum_api::model::State` (built by `api_state`,
filtered for a limited token by `State::visible`, diffed and parsed as every collection is) and the two item tools in
the control server, whose bodies become `Workspace` operations both servers call. Then the mirror, on 2d's pattern:
`RemoteSpace` keeps copies of home's notes and checklists (`Notes`, `Checklists`), made from home's state where it
enters (cleaned, capped, an item rebuilt only when its `Arc` changed), never saved; home's drawings become the remote
`Space::ink` (a remote space is never saved), painted by the local painter (Task 3). A remote note or checklist is
`Body::Note(Note { remote: true, .. })` / `Body::Checklist(Checklist { remote: true, .. })`, which `Body::note()` and
`Body::checklist()` skip, so no local lookup by id meets home's ids (Task 4). A note's editors are made where a window
is (`apply_remote`), kept in `RemoteSpace::note_editors`, and laid into the blocks' tabs by a pass that needs none, so
`remirror` can lay them too; each editor is `readonly`. Every item act goes through one door,
`Workspace::act_on_checklist(space, block, ChecklistAct, …)`: on this computer today's edit, on a mirrored workspace
home's tool through `remote_call`, asked of `why_not` first, a check, an edit and a delete held over home's state (Task
5).

**Tech Stack:** Rust 2024; GPUI through gpui-kit `=0.7.3` (gpui-component's `Editor::readonly`,
`EditorState::set_value`);
`templum-api` (`State`, `Arc` items, `model::same`, `visible`, `diff`, `parse`, the client's `Weight` through
`keeps_items`, `openapi`); `templum-mcp` (control server, block server parsing); 2a's `remote_call`/`Then`/`Deed`/`Nay`/
`why_not`/`Holds`/`remirror`; 2b's `mark_remote`, `Gates`, `gated_item`, `warn`; 2d's `remote_boards.rs` pattern
(`card_of`, `mirror_boards`, `lay_holds`, `laid`, `boards_of`, `boards_in`, `remote_terminals`, `remote_send_to`).

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), "Batch 2: the whole workspace, live":
"Parts ▸ 2e", "What works on a remote workspace" (the checklist row), "The canvas ▸ Adding, Holds, Undo", "Boards,
checklists, notes, ink ▸ Checklists, Notes, Ink and shapes", "One rule for what the mirror can do" (rung 2's older
home), "What home's API gains" (`notes`, `checklists`, `drawings`, the blocks' `notes` and `checklist`, a limited
token's view, "Into the control server"), "The mirror's side" (the copies' caps, never a relay), "Constraints (batch
2)", the threat model rows marked *(batch 2)*, "As built ▸ 1c, 1d, 2a, 2b, 2c, 2d"; the 2a–2d plans' "Interfaces for
later parts".

**Code it builds on:** `crates/templum-api/src/{model.rs,openapi.rs,registry.rs,client/mod.rs}`;
`crates/templum-mcp/src/{control.rs,tools.rs}`; `crates/templum/src/{note.rs,notes.rs,checklist.rs,checklists.rs,ink.rs,
block.rs,tab_strip.rs,session.rs,bench.rs}`, `crates/templum/src/menu/{mod.rs,build.rs}`,
`crates/templum/src/workspace/{api_state.rs,ops.rs,content.rs,control.rs,approvals.rs,note_tabs.rs,
checklist_blocks.rs,drawing.rs,input.rs,remote.rs,remote_boards.rs,holds.rs,history.rs,mod.rs}`; tests
`crates/templum/src/workspace/tests/{mod.rs,api.rs,templum_agent.rs,checklists.rs,notes.rs,remote.rs,remote_boards.rs,
menus.rs,mirror_end_to_end.rs}`.

**Consumes (as built):** `Notes::{get, name, set_text}`, `notes::{Entry, UNTITLED}`, `Checklists::{get, add_item,
set_text, set_done, remove_item}`, `List::{item, name_shown, title, progress}`, `checklists::{parse_items, NAME}`,
`note::{editor, Note, NoteTab}`, `checklist::{Checklist, Editing, Field}`; `Workspace::{edit_checklist, toggle_item,
delete_item, item_edit, item_check, items_add, list_of, no_item, checklist_text, note_open_at, checklist_open_at,
agent_changed, edit_field, stop_editing, field_enter, backspace_at_start, show_note_tab, close_note_tab,
new_note_tab, rename_note_tab, on_block, find_block, about}`; `session::InkItem::{of, drawing}`; `ink::paint`,
`ink::Cache`; `control::{SHARED, SHARED_WORDS, rule, title, Rule::AskOwn}`, the gate (`approvals.rs`: `gate`,
`called_block`, `approval_words`, `facts`, `own_block`); `registry::{scope, NOT_TARGETS}`; `RemoteSpace::{state,
homes, home_of, why_not, takes, tools, told, held, fresh_id}`, `Deed::{call, needs}`, `Nay::words`,
`Workspace::{why_not, why_not_in, remote_call, remote_mut, remirror, expire_holds, mark_remote, warn, still_shown,
send_to, remote_terminals, remote_send_to, space_named, boards_of}`, `remote.rs`'s `clean_text`, `MOST_FAR`,
`remote_boards.rs`'s `ITEM_CHARS`, `bus::{clean_title, clean_to}`, `mirror::computer_of`, `Holds::{hold, get, end,
shown, expire, keys}`, `HOLD`, `menu::Gates`, `build.rs`'s `gated_item`, `block::{Gates, header_button}`; the tests'
`open`, `with_data`, `with_token`, `tool`, `state`, `said`, `three_blocks`, `control`, `offline`, `live`, `show_remote`,
`apply`, `live_terminals`, `terminals_of_home`, `viewer`, `sent`, `answer`, `refusal`, `remote_of`, `shown_mirror_as`,
`pump_home`, `mirrored`, `draw`, `drawn`, `el`.

## Global Constraints

- **MEMORY LIMITS on this machine:** **every** cargo command takes `-j 4`; **never two cargo commands at once**; never
  `cargo clean`; never `git stash`; while working run only the targeted tests each step names; the full suite
  **once**, in Task 6.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-api`, `templum-mcp` and `templum-canvas`
  stay UI-free. No `unsafe`. **No new dependency.**
- **Home is the truth:** the mirror never edits a note, a checklist or a drawing; every item act is home's tool through
  `remote_call`, asked of `RemoteSpace::why_not` first. What the mirror shows ahead of home is a hold that ends when
  home's state shows it, at a refusal, or after `HOLD` (2 s).
- **Nothing local written for a remote workspace:** no `Workspace::notes` or `Workspace::checklists` edit, no
  `next_id`, no `checklists_undo` step, no `notes.json`/`checklists.json` save for anything of a remote's; a remote's
  copies live only in `RemoteSpace::{notes, checklists}` and its drawings only in the remote `Space::ink`, which
  `persist.rs` never saves (`space.is_remote()` is skipped). A remote note's editors have no `InputEvent::Change`
  subscription. `Body::note()`/`note_mut()`/`checklist()`/`checklist_mut()` answer None for a remote one, so
  `note_open_at`, `checklist_open_at`, `agent_changed`, `delete_note`, `delete_checklist`, Ctrl+Z on a focused
  checklist (`undo_checklists`), the Notes and Checklists dialogs and the API's state can't meet home's ids, which in
  the end-to-end test equal the local ones.
- **Never a relay:** the mirror's API, control server and block server gain nothing: `api_state` builds `notes` and
  `checklists` from `Body::note()`/`checklist()` (local only) and `drawings` from local spaces only
  (`!space.is_remote()`). Send to ▸ on a remote note lists only the remote workspace's terminals (2d's `send_to_menu`).
- **What home sends is bounded and cleaned before it is kept, drawn or made an editor** (`remote_content.rs`, once
  where it enters): at most `MOST_NOTES` = 1,000 notes; a note's text `clean_text` (line breaks and tabs kept) of at
  most `NOTE_CHARS` = 262,144 characters, its name `clean_title`, its color one of the six labels else none; at most
  `MOST_CHECKLISTS` = 1,000 checklists of at most `MOST_LIST_ITEMS` = 2,000 items, an item one line (`clean_to`) of at
  most `ITEM_CHARS` = 4,000 characters, a name `clean_title`; drawings through `InkItem::drawing` (an unknown kind or
  a stroke under 2 points dropped, a number that isn't finite drops the drawing, an unknown color is `foreground`, a
  width clamped to 1–64), a point or end beyond `MOST_FAR` (±10⁷) drops the drawing, then home's caps again
  (`DRAWINGS_MOST` 2,000 and `POINTS_MOST` 100,000, the newest kept). A note's tab lists only a note the copy holds,
  each note in one tab at most across the workspace (home's own rule), so a hostile home can't make thousands of
  editors of one note.
- **The UI thread never waits on the network:** every act is `remote_call` (its `Then` runs when home answers).
- **The rule:** every check box, ✕, field and menu item of a remote checklist or note asks `why_not` of its deed; a
  dimmed control shows the words as its tooltip; a refused act says them once (`warn`). New deeds with a call:
  `CheckItem` (`check_item`), `AddItems` (`add_items`), `EditItem` (`edit_item`), `DeleteItem` (`delete_item`), all
  Control. New deeds without one: `EditNote` (`Nay::NotesReadOnly`, as `AddNote`), `EditChecklist` (renaming a
  checklist, moving its items: `Nay::NotYet`). `Deed::Draw` answers a new `Nay::Drawing`. All checklists… is the
  existing `Deed::OpenChecklist`.
- **Values, verbatim:** `HOLD` 2 s; home: `templum_api::model::NOTE_TEXT_MOST` = 262,144 bytes (a note's text cut at a
  character's boundary at or below it, `cut` true), `DRAWINGS_MOST` = 2,000, `POINTS_MOST` = 100,000 (a stroke counts
  its points, a shape 2); mirror: `MOST_NOTES` 1,000, `NOTE_CHARS` 262,144, `MOST_CHECKLISTS` 1,000, `MOST_LIST_ITEMS`
  2,000, `ITEM_CHARS` 4,000 (2d's), `MOST_FAR` 1e7 (1c's); the bench's strokes: 200 of 50 points.
- **Words, verbatim** (winx-desktop is `computer_of`; ids as in the examples):
  - Mirror: "Notes are read-only on a mirror for now: edit them on winx-desktop." (a key that would type in a remote
    note, once per note block per run); "… The rest is on winx-desktop." (the last line of a note home cut, after a
    blank line); "No checklist open here: open one on winx-desktop." (a remote checklist block showing none); "No note
    open here: open one on winx-desktop." (a remote note block with no tab); "Drawing on a mirror isn't there yet:
    draw on winx-desktop." (`Nay::Drawing`: the pen, highlighter, shapes and eraser); "winx-desktop's Templum is older:
    update it to see this here." (a note or checklist block of a home whose `GET /v1/tools` lacks `edit_item`);
    unchanged and reused: "Notes are read-only on a mirror for now: add and edit notes on winx-desktop."
    (`Nay::NotesReadOnly`: Add ▸ Note, and now a note's Rename…, Color ▸, All notes… and its tabs' + and ✕), "Not on a
    mirrored workspace yet" (renaming a checklist, dragging its items, All checklists…), the other `Nay` sentences,
    "On a mirror, Ctrl+Z undoes moves only: undo the rest on winx-desktop.".
  - `NOT_MIRRORED` becomes: "Not mirrored yet: editors' content (a card shows each). Not on a mirrored workspace yet:
    editors, editing notes, renaming a checklist and moving its items, drawing, grid mode, Move to workspace, Copy and
    Paste of blocks, Stop, the Git popup, a card's attachments."
  - Home's answers and refusals (today's, now also the control server's): "Item 13 is now \"Tag it\"."; "Deleted item
    13."; "delete_item removes an item" (a blank `edit_item`); "There's no item 99 in \"Release\". read_checklist shows
    its items."; "c1 isn't a checklist."; "c1 shows no checklist.". The approval of the Templum Agent's `delete_item`:
    said `Delete item 13 of "Release"`, details `"Tag it"`. Running titles: "Editing an item", "Deleting an item".
- **Batch 1's and 2a–2d's words stay;** every earlier test keeps passing except those this plan rewrites, named in
  their task (a note or checklist mirrored as a card becomes a live block; `NOT_MIRRORED`'s words; `SHARED`'s count).
- **Every logic change has a test that fails without it;** UI-only changes say how they were verified (a remote note,
  checklist, the dimmed controls and home's ink were not looked at on a display unless the implementer did: then a
  minors entry). Headless GPUI tests; keys in `keyring_core::mock::Store`; home on loopback in the same process;
  conditions with a 10 s deadline, never a fixed sleep. CI runs Linux, macOS and Windows with no agent CLI.
- **Every feature in the API:** each new state field and each tool moved ships its lines in `docs/api.md`,
  `docs/specs/api.md` and `docs/specs/templum-agent.md` (the control tools table and Shared operations) in the task
  that adds it. The mirror's acts add nothing to the mirror's own API.
- **Minors** go to `docs/minors.md` (`## Templum to Templum`); "A kanban's header button All boards isn't dimmed on a
  mirror that can't show boards." leaves it (Task 4). Docs lines ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **Home's ids equal the mirror's own** (the end-to-end test is one workspace for both ends, so home's note 7 *is* the
   mirror's note 7; a mirror with notes and checklists of its own collides by chance): no read of a remote note or
   checklist, its menus or its fields may reach `Workspace::notes`/`checklists`, no write may reach them, and no local
   tool (the dialogs, `note_open_at`, `checklist_open_at`, Ctrl+Z on a checklist, the API's state) may see the remote
   block. → Task 4 `no_local_tool_sees_a_remote_note_or_checklist`; Task 6
   `acts_on_a_mirror_leave_its_notes_json_and_checklists_json_as_they_were`.
2. **A hostile home's content** (a 1 MB note with ESC, BEL and `\r`; 5,000 notes; one note in the tabs of every block;
   a checklist of 10,000 items of 10,000 characters; a drawing with a point at 1e30, a width of 1,000, the color
   "chartreuse", the kind "spiral"; 5,000 drawings of 1M points): capped and cleaned before it is kept, drawn or made
   an editor. → Task 3 `home_s_notes_are_cleaned_and_capped`, `home_s_checklists_are_cleaned_and_capped`,
   `home_s_drawings_are_cleaned_and_capped`; Task 4 `a_note_is_in_one_tab_at_most_on_a_mirror`.
3. **Home changes an item while the person edits it, or while a hold stands:** the field keeps the person's text;
   Enter or blur sends the whole value once (never per key); a held check shows until home's state agrees or 2 s pass;
   a refusal ends the hold at once and says its sentence. → Task 5
   `an_item_edited_on_the_mirror_goes_once_on_enter_and_esc_drops_it`,
   `a_box_checked_on_the_mirror_sends_check_item_and_shows_at_once`.
4. **Home types in a note the person reads or selects on the mirror:** the editor follows home's text and keeps the
   selection (clipped), nothing is written here, and typing on the mirror says the sentence once and changes nothing.
   → Task 4 `home_s_text_follows_into_a_remote_note_and_keeps_the_selection`,
   `a_remote_note_shows_home_s_text_read_only`.
5. **The shown workspace changes while a remote item's field is open** (an agent's `show`, a click on the sidebar):
   the field's blur sends the edit to its own remote by id, never to the shown workspace. → Task 5
   `an_item_field_left_by_switching_workspaces_sends_to_its_own_remote`.

---

### Task 1: Home's state gains notes, checklists, drawings and the blocks' notes and checklist

**Files:**
- Modify: `crates/templum-api/src/model.rs` (`NoteView`, `ChecklistView`, `DrawingsView`, `DrawingView`, `NotesShown`;
  `State.{notes, checklists, drawings}`; `BlockView.{notes, checklist}`; `NOTE_TEXT_MOST`, `DRAWINGS_MOST`,
  `POINTS_MOST`, `newest_drawings`; `State::visible`, `from_snapshot`'s crowded list, `keeps_items`, `parse`, `diff`)
- Modify: `crates/templum-api/src/openapi.rs` (the `State` schema's three lists; `NoteView`, `ChecklistView`,
  `DrawingsView`, `Drawing` and `BlockView`'s optional `notes` and `checklist` in `schemas_with_views`, beside
  `read_only`; `full_state`)
- Modify: `crates/templum/src/workspace/api_state.rs` (`note_views`, `checklist_views`, `drawing_views`; `block_views`
  fills `notes` and `checklist`), `crates/templum/src/session.rs` (`impl From<&InkItem> for DrawingView` and
  `impl From<&DrawingView> for InkItem`, field for field)
- Test: `crates/templum-api/src/model.rs` tests, `crates/templum-api/src/openapi.rs` tests,
  `crates/templum/src/workspace/tests/api.rs`
- Docs: `docs/api.md` (Resources: `GET /v1/state`'s list; Scopes ▸ Workspace-limited tokens; Events' list of names; a
  **Notes, checklists and ink.** paragraph after **Boards.**), `docs/specs/api.md` (the state's collections and a
  block's fields)

**Interfaces:**
- Produces (`templum_api::model`, each `Clone, Debug, Default, PartialEq, Serialize, Deserialize`):

```rust
/// A note a note block shows: its text cut at NOTE_TEXT_MOST bytes, `cut` true past it.
pub struct NoteView { pub id: u64, pub name: String, pub text: String, pub color: Option<String>, pub cut: bool }
/// A checklist a checklist block shows.
pub struct ChecklistView { pub id: u64, pub name: String, pub items: Vec<ItemView> }
/// One workspace's ink and shapes, in paint order: `id` is the workspace's.
pub struct DrawingsView { pub id: String, pub drawings: Vec<DrawingView> }
/// A drawing as the canvas file keeps it (`session::InkItem`, same serde attributes): a `stroke` with `points`, or a
/// `line`, `arrow`, `rect` or `ellipse` with `from` and `to`.
pub struct DrawingView {
    pub kind: String, pub color: String, pub width: f32,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")] pub highlight: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")] pub points: Vec<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub from: Option<[f32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")] pub to: Option<[f32; 2]>,
}
/// A note block's tabs (note ids) and the one on screen.
pub struct NotesShown { pub tabs: Vec<u64>, pub shown: Option<u64> }
```

  `State` gains `#[serde(default)] pub notes: Vec<Arc<NoteView>>`, `checklists: Vec<Arc<ChecklistView>>`, `drawings:
  Vec<Arc<DrawingsView>>`; `BlockView` gains `#[serde(default, skip_serializing_if = "Option::is_none")] pub notes:
  Option<NotesShown>` (every note block, its tabs maybe empty) and `checklist: Option<u64>` (a checklist block that
  shows one). Events `note` (id: the note's), `checklist` (the checklist's), `drawing` (the workspace's), each in
  `keeps_items`, `parse` and `diff` (after `card`, before `automation`), each counted in `from_snapshot`'s crowded list
  (`COLLECTION_MOST`). `pub const NOTE_TEXT_MOST: usize = 256 * 1024; pub const DRAWINGS_MOST: usize = 2_000; pub
  const POINTS_MOST: usize = 100_000;` `pub fn newest_drawings<T>(drawings: &[T], points: impl Fn(&T) -> usize) ->
  &[T]` (the longest tail with at most `DRAWINGS_MOST` items whose points sum to at most `POINTS_MOST`; a single
  drawing past `POINTS_MOST` is left out with everything older).
- `State::visible` for a limited caller: `notes` whose id is in a visible block's `notes.tabs`; `checklists` whose id
  is a visible block's `checklist`; `drawings` whose `id` the caller `sees`.
- `api_state` (home): `notes`: every note a local note block shows (`Body::note()`, spaces in `order()`, tab order,
  each id once), `NoteView { name, text: cut, color: label's name, cut }`; `checklists`: every checklist a local
  checklist block shows (`Body::checklist()`), `name` its `name_shown()`, items as `ItemView`; `drawings`: one
  `DrawingsView` per local space (`!is_remote()`) with ink, `newest_drawings(&space.ink, …)` mapped through
  `InkItem::of` → `DrawingView`; `block_views`: `notes: body.note().map(|n| NotesShown { tabs: n.ids(), shown:
  n.tab().map(|t| t.id) })`, `checklist: body.checklist().and_then(|c| c.list).filter(|l|
  self.checklists.get(*l).is_some())`.

- [ ] **Step 1: Write the failing tests.** `model.rs`:

```rust
#[test]
fn a_limited_caller_sees_the_notes_checklists_and_drawings_of_its_workspaces_only() {
    // Blocks n1 (w1, notes {tabs [7, 9], shown 9}), c1 (w1, checklist 12), n2 (w2, notes {tabs [8]}), c2 (w2,
    // checklist 13); notes 7, 8, 9; checklists 12, 13; drawings "w1", "w2". A caller limited to w1 sees notes [7, 9],
    // checklists [12], drawings ["w1"]; an unlimited one sees all three of each.
}

#[test]
fn note_checklist_and_drawing_events_put_and_delete() {
    // apply("note", put 7) adds it; a second equal put keeps the same Arc (Arc::ptr_eq) and answers false;
    // apply("drawing", put "w1") then delete "w1" leaves none; a put whose value's id isn't the event's is Malformed;
    // diff of two states that differ by a checklist answers one Delta { kind: "checklist", id: "12", .. }.
    // from_snapshot of a state with 10_001 notes is Err(TooLarge).
}

#[test]
fn the_newest_drawings_are_kept_within_both_caps() {
    // 2_500 items of 1 point: the last 2_000. [60_000, 30_000, 20_000] points: the last two. [150_000]: none.
    // [10, 150_000, 10]: [10] (the last only: the big one and everything older go).
}
```

`openapi.rs`: `full_state` gets a note, a checklist, a drawings item (a stroke and an arrow) and a block with `notes`
and `checklist`; `the_state_schema_matches_the_state` also checks a state whose block has neither (they are optional)
and a drawing without `highlight`, `points`, `from`, `to`.

`tests/api.rs`:

```rust
#[gpui_kit::test]
fn the_state_shows_notes_checklists_and_drawings(cx: &mut TestAppContext) {
    // three_blocks (note "hello" in the note block, a second tab with a 300 KiB note of 'é'), a checklist block
    // (add_block_in NewBlock::Checklist "Release": "Tag it", "[x] Ship"), a stroke in ws.space.ink. state(read token):
    // the note block's `notes` == NotesShown { tabs: [hello, big], shown: Some(big) }; the checklist block's
    // `checklist` == Some(list); the terminal has neither; notes[1].cut and its text is ≤ 262_144 bytes, ends on a
    // whole 'é', and is a prefix of the note; checklists[0] == { name "Release", items [Tag it, Ship (done)] };
    // drawings == [{ id: the workspace's, drawings: [the stroke as InkItem::of says it] }].
}

#[gpui_kit::test]
fn a_limited_token_sees_no_note_shown_only_in_another_workspace(cx: &mut TestAppContext) {
    // A second workspace with its own note block (note 21) and checklist (list 22) and a stroke. A token limited to
    // the first sees neither 21, 22 nor the second workspace's drawings item; a token for all sees them.
}

#[gpui_kit::test]
fn home_s_drawings_are_capped_at_the_newest(cx: &mut TestAppContext) {
    // 2_100 strokes of 2 points: the state's item holds the last 2_000, in paint order. Then 3 strokes of 40_000
    // points: the last 2.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum-api model::tests openapi::tests` then `cargo test -j 4 -p
  templum workspace::tests::api` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say. The client's `Weight` counts the new events by itself
  (`keeps_items`); no client change beyond it.
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum-api client` → PASS.
- [ ] **Step 5: Docs.** `docs/api.md`: add `notes`, `checklists`, `drawings` to `GET /v1/state`'s list and `note`,
  `checklist`, `drawing` (its id the workspace's) to the event names; Workspace-limited tokens: "…the notes and
  checklists a note or checklist block of theirs shows, the drawings of those workspaces…"; **Notes, checklists and
  ink.** "`notes` holds the notes a note block shows, its `text` cut at 256 KiB (`cut` true then); a note block has
  `notes`: `{"tabs": [7, 9], "shown": 7}`. `checklists` holds the checklists a checklist block shows; a checklist
  block that shows one has `checklist`: its id. `drawings` holds one item per workspace with ink, its id the
  workspace's: its drawings in paint order, as the canvas file keeps them (`kind` `stroke` with `points`, or `line`,
  `arrow`, `rect`, `ellipse` with `from` and `to`; `color` by name; `width`; `highlight`), at most 2,000 drawings and
  100,000 points (a shape counts 2), the oldest left out past either." `docs/specs/api.md`: the same, as bullets.

---

### Task 2: `edit_item` and `delete_item` in the control server

**Files:**
- Modify: `crates/templum-mcp/src/control.rs` (`SHARED` gains `"edit_item"`, `"delete_item"`: 22; `rule`:
  `ControlCall::Block(Call::DeleteItem { .. })` is `Rule::AskOwn`; `title`)
- Modify: `crates/templum/src/workspace/ops.rs` (`item_text`, `item_delete`), `crates/templum/src/workspace/content.rs`
  (the block server's `edit_item` and `remove_item` call them after `wired`), `crates/templum/src/workspace/control.rs`
  (two `control_call` arms through `on_block`), `crates/templum/src/workspace/approvals.rs` (`called_block`,
  `approval_words`, `facts`)
- Test: `crates/templum-mcp/src/control.rs` tests, `crates/templum-api/src/registry.rs` tests,
  `crates/templum/src/workspace/tests/{templum_agent.rs,api.rs}`
- Docs: `docs/api.md` (the Scopes table needs nothing; **Notes, checklists and ink.** gains the two tools),
  `docs/specs/templum-agent.md` (a row `edit_item`, `delete_item` | `block`, `item`, and `text` | As the block
  server's, on any checklist block | –; Ask own for `delete_item` | 171 2e; the batch-3 row and the Shared operations
  row lose them), `docs/specs/connections.md` (the two block-server rows: "(the control server's too, on any checklist
  block: 171 2e)"), `docs/specs/api.md`

**Interfaces:**
- Produces: `Workspace::item_text(&mut self, at: (&str, BlockId), block: &str, item: u64, text: &str, cx) -> Reply`
  (the checklist `block` shows; `text` trimmed; blank → "delete_item removes an item"; no item → `no_item`; else
  `item_edit`, `agent_changed`, "Item 13 is now \"Tag it\"."); `Workspace::item_delete(&mut self, at: (&str, BlockId),
  block: &str, item: u64, cx) -> Reply` (no item → `no_item`; else `delete_item`, `agent_changed`, "Deleted item 13.").
  Both use `list_of(at, block)` for "c1 isn't a checklist." and "c1 shows no checklist.".
- The gate: `called_block` answers `Call::DeleteItem { block, .. }`'s block; `approval_words` says
  `(format!("Delete item {item} of {name:?}"), format!("{text:?}"))` (name `name_shown()`); `facts` of a `DeleteItem`
  adds the item's text, so an item edited since asks again. Through the API the call runs at once (the token is the
  person's hand), scope Control (`registry::scope` from `Rule::AskOwn`).

- [ ] **Step 1: Write the failing tests.** `control.rs`: rename `the_two_servers_share_the_comment_tools` to
  `the_two_servers_share_the_comment_and_item_tools`: `SHARED.len() == 22`; `edit_item` (`{"block": "c1", "item": 13,
  "text": "x"}`) parses to `ControlCall::Block(Call::EditItem { .. })`, `Rule::Free`, title "Editing an item";
  `delete_item` → `Rule::AskOwn`, "Deleting an item". `every_tool_parses_its_example_and_names_what_is_missing` gains
  both examples. `registry.rs` `scope_follows_the_registry`: both are `Scope::Control`. `tests/templum_agent.rs`:

```rust
#[gpui_kit::test]
fn the_templum_agent_edits_and_deletes_items_of_any_checklist(cx: &mut TestAppContext) {
    // The person's checklist block c ("Release": 13 "Tag it", 14 "Ship"), wired to nothing. control("edit_item",
    // {block, item: 13, text: " Tag v1 "}) → "Item 13 is now \"Tag v1\"." and the item's text; one Ctrl+Z step.
    // control("delete_item", {block, item: 14}) waits: its approval says "Delete item 14 of \"Release\"", details
    // "\"Ship\""; the item is edited to "Ship it" before Allow → the approval is asked again (changed). A checklist
    // block the agent made (add_block) → delete_item runs at once.
}
```

`tests/api.rs`:

```rust
#[gpui_kit::test]
fn edit_item_and_delete_item_act_as_you_on_any_checklist_block(cx: &mut TestAppContext) {
    // A Control token, no wire: tool("edit_item", {block, item: 13, text: "x"}) changes it, no approval;
    // {text: "  "} → refused "delete_item removes an item"; tool("delete_item", {block, item: 99}) → refused "There's
    // no item 99 in \"Release\". read_checklist shows its items."; a terminal's id → "b… isn't a checklist.". A token
    // limited to another workspace naming this block → 404 not found. A Read token → 403 scope.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum-mcp control` then `cargo test -j 4 -p templum-api registry` then
  `cargo test -j 4 -p templum workspace::tests::templum_agent workspace::tests::api` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say. The block server's behaviour is unchanged (its `wired` check, then
  the same operations).
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum workspace::tests::content
  workspace::tests::checklists` → PASS.
- [ ] **Step 5: Docs** as listed. `docs/api.md`: "`edit_item` (`block`, `item`, `text`) and `delete_item` (`block`,
  `item`; control) change or delete an item of the checklist a checklist block shows, any block your token sees; each
  is one step of the checklist's Ctrl+Z."

---

### Task 3: The mirror's copies of home's notes, checklists and drawings

**Files:**
- Create: `crates/templum/src/workspace/remote_content.rs` (`mod remote_content;` in `workspace/mod.rs`): the caps,
  `note_of`, `list_of`, `drawings_of`, `RemoteSpace::{mirror_notes, mirror_checklists}`, `Space::mirror_drawings`
- Modify: `crates/templum/src/workspace/remote.rs` (`RemoteSpace.{notes, checklists, note_editors, items_checked,
  items_edited, items_deleted, lists_laid}`; `apply_remote` calls the three when their collection isn't `same`;
  `remirror` calls `mirror_checklists`; `Nay::Drawing` and `why_not(Deed::Draw)`; `NOT_MIRRORED`),
  `crates/templum/src/workspace/drawing.rs` (`ink_at` answers None on a remote workspace)
- Test: `crates/templum/src/workspace/remote_content.rs` tests; create
  `crates/templum/src/workspace/tests/remote_content.rs` (`mod remote_content;` in `tests/mod.rs`)

**Interfaces:**
- Produces (`remote_content.rs`): the constants of Global Constraints; `pub(super) fn note_of(view: &NoteView,
  computer: &str) -> notes::Entry` (a cut note — home's `cut`, or one this cleaning cut — ends with "\n\n… The rest is
  on {computer}."; `changed` 0); `pub(super) fn list_of(view: &ChecklistView) -> checklists::List`; `pub(super) fn
  drawings_of(views: &[DrawingView]) -> Vec<Drawing>` (each through `InkItem::from(view).drawing()`, then `MOST_FAR`,
  then `newest_drawings`); `RemoteSpace.notes: Notes`, `RemoteSpace.checklists: Checklists` (never saved);
  `RemoteSpace::mirror_notes(&mut self, state: &State)` and `RemoteSpace::mirror_checklists(&mut self, state: &State,
  now: Instant)` (rebuilt from `state` by the caps; an item whose `Arc` is the one `self.state` holds for its id is
  moved over as it is, except a checklist in `lists_laid`; called before `remote.state = state`; Task 5 lays the item
  holds); `Space::mirror_drawings(&mut self, state: &State, home: Option<&str>)` (`self.ink` becomes `drawings_of` of
  the item whose id is `home`, empty without one; `selected_ink` None).
- `RemoteSpace` also gains, for Tasks 4–5: `note_editors: HashMap<u64, Entity<EditorState>>`, `items_checked:
  Holds<(u64, u64), bool>`, `items_edited: Holds<(u64, u64), String>`, `items_deleted: Holds<(u64, u64), bool>`
  (keys: checklist, item), `lists_laid: HashSet<u64>`.
- `Nay::Drawing` → "Drawing on a mirror isn't there yet: draw on {computer}."; `RemoteSpace::why_not` answers it for
  `Deed::Draw` before `NotYet`. `Workspace::ink_at` answers None while `on_remote()`: no pick, no move, no ink menu
  (a right-click there opens the canvas menu).

- [ ] **Step 1: Write the failing tests.** `remote_content.rs`:

```rust
#[test]
fn home_s_notes_are_cleaned_and_capped() {
    // NoteView { name "a\x1b[31m\nb" + 200×"n", text "x\r\n\x07\ty" + 300_000×"t", color "purple", cut false }:
    // note_of(.., "winx-desktop"): name one line, no ESC; text keeps "\n" and "\t", no '\r' or '\x07', its first
    // 262_144 characters then "\n\n… The rest is on winx-desktop."; color None. A view with cut true and a short text
    // ends with the sentence too; "blue" is Some(Label::Blue). mirror_notes of 1_500 notes keeps the first 1_000.
}

#[test]
fn home_s_checklists_are_cleaned_and_capped() {
    // ChecklistView { name "R\nel", items: 3_000 of 5_000 chars with "\x1b" } → list_of: name one line; 2_000 items,
    // each one line of at most 4_000 characters, ids and done as home's. mirror_checklists of 1_200 keeps 1_000.
}

#[test]
fn home_s_drawings_are_cleaned_and_capped() {
    // [stroke of 1 point, stroke with a point [1e30, 0] (past ±10⁷), stroke with a point [1e39, 0] (infinite as
    // f32), kind "spiral", rect without `to`, stroke width 1000 color "chartreuse" [[0,0],[5,5]]] → drawings_of: one
    // stroke, width 64, Color::Foreground. 2_500 strokes of 2 points → the last 2_000.
}

#[test]
fn a_note_or_checklist_home_didn_t_change_is_not_cleaned_again() {
    // mirror_notes(s1); remote.state = s1; mirror_notes(s2) with note 7's Arc the same and note 9's new: note 7's
    // text keeps its as_ptr(); note 9 is home's new text. Likewise mirror_checklists for list 12 and 13.
}
```

`tests/remote_content.rs` (a helper `content_state() -> State`: workspace w1 with terminal b1 "claude", note block n1
`notes {tabs [7, 9], shown 9}`, checklist block c1 `checklist 12`; notes 7 "Plan" "first\nsecond", 9 "Ideas"
"one"; checklist 12 "Release": 13 "Tag it", 14 "Ship" (done); drawings w1: a stroke `[[0,0],[100,100]]` blue and a
rect; and `live_content(cx) -> (handle, ws, dir)`: `live_terminals(cx, &["b1"])` then `apply(content_state())`, the
mirror's own `notes.json` and `checklists.json` written once):

```rust
#[gpui_kit::test]
fn home_s_ink_is_drawn_on_the_mirror_and_can_t_be_picked(cx: &mut TestAppContext) {
    // r1's space.ink == drawings_of(home's two); ws.ink_at((50, 50)) is None; a press there with the Select tool picks
    // nothing (selected_ink None); a right-click there opens Target::Canvas's menu. A state whose drawings item is
    // gone empties r1's ink.
}

#[gpui_kit::test]
fn the_drawing_tools_say_drawing_isn_t_on_a_mirror_yet(cx: &mut TestAppContext) {
    // why_not(Deed::Draw) == "Drawing on a mirror isn't there yet: draw on …" (computer_of); set_tool(Pen) leaves
    // the tool Select; on a local workspace why_not(Deed::Draw) is None.
}

#[gpui_kit::test]
fn a_mirror_s_state_never_holds_a_remote_workspace_s_ink(cx: &mut TestAppContext) {
    // ws.api_state(cx).drawings has no item "r1" (nor any whose drawings are home's).
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::remote_content workspace::tests::remote_content` →
  FAIL.
- [ ] **Step 3: Implement** as the Interfaces say. Rewrite any test that pins `NOT_MIRRORED` or `Deed::Draw`'s words.
- [ ] **Step 4: Run them** → PASS; `cargo test -j 4 -p templum workspace::tests::remote workspace::tests::drawing` →
  PASS.

---

### Task 4: A remote note and checklist drawn live

**Files:**
- Modify: `crates/templum/src/note.rs` (`Note.remote`; `Note::title(&self, ws: &Workspace)`; `body` reads names
  through `ws.notes_of(note)`, draws `editor_box` with `readonly(note.remote)`, a remote note without a tab says the
  sentence instead of `empty`, and a remote note's body hears key-downs (capture phase) to tell typing once;
  `pub(crate) fn would_type(keystroke: &Keystroke) -> bool`)
- Modify: `crates/templum/src/checklist.rs` (`Checklist.remote`; `title`, `body`, `list_view`, `row` read
  `ws.checklists_of(checklist)`; a remote block showing none says the sentence; no `on_drag`, no name click on a
  remote one; until Task 5 wires them, a remote checklist's check box, ✕, item click and + Add an item do nothing, so
  no click can write home's ids into this computer's checklists)
- Modify: `crates/templum/src/tab_strip.rs` (`tab_strip` takes `locked: Option<SharedString>`: the + and each ✕ dimmed
  with the words as tooltip and doing nothing, a double-click renames nothing; `note.rs` passes the `EditNote` words for
  a remote note, `editor.rs` None)
- Modify: `crates/templum/src/block.rs` (`Gates` gains `boards`, `notes`, `checklists`: `why_not` of `ShowBoard`,
  `EditNote`, `OpenChecklist`; `header_button` takes `why: Option<String>`: dimmed, the words its tooltip, its click
  nothing; the note's tint and `title` read the copy; a remote Note or Checklist of an older home draws
  `mirrored_card` with the older words; `mirrored_card` takes its kind, title and sentence, and its element id
  `("mirrored", n)`)
- Modify: `crates/templum/src/workspace/mod.rs` (`Body::note()`/`note_mut()`/`checklist()`/`checklist_mut()` None for a
  remote one; `Space::note_view_mut(id)` and `Space::checklist_view(_mut)(id)` answer any), `remote_content.rs`
  (`Workspace::{notes_of, notes_in, checklists_of, checklists_in, older_content, remote_notes,
  tell_note_read_only}`, `Space::lay_note_tabs`), `remote.rs` (`remote_body` for `"note"` and `"checklist"`;
  `mirror_blocks` keeps a remote checklist's `list` home's; `apply_remote` calls `remote_notes` when blocks or notes
  changed; `remirror` calls `lay_note_tabs`; `Deed::{EditNote, EditChecklist}`; `why_not`: `AddNote | EditNote` →
  `NotesReadOnly`), `note_tabs.rs` (`show_note_tab` through `note_view_mut`, so a remote note's tab is the mirror's
  choice), `crates/templum/src/menu/{mod.rs,build.rs}` (`Items::Note` reads the copy for a remote note and gates
  Rename…, Color ▸ and All notes… by `EditNote`; `Items::Checklist` gates All checklists… by `OpenChecklist`)
- Test: `crates/templum/src/note.rs` tests, `crates/templum/src/workspace/tests/remote_content.rs`; rewrite the
  note/checklist arms of `tests/remote.rs` `home_s_blocks_appear_move_and_go`, `tests/mirror_end_to_end.rs`
  `a_workspace_mirrors_itself_over_loopback` (three_blocks' note is a live note on r1) and `tests/menus.rs`'s remote
  menu test where a note was a card
- Docs: `docs/minors.md`

**Interfaces:**
- Produces: `Note { pub tabs, pub active, pub renaming, pub remote: bool }`, `Checklist { pub list, pub editing, pub
  scroll, pub remote: bool }` (`remote`: of a mirrored workspace; ids are home's, in `RemoteSpace::{notes,
  checklists}`); `Workspace::notes_of(&self, note: &Note) -> Option<&Notes>` and `checklists_of(&self, c: &Checklist)
  -> Option<&Checklists>` (local: this computer's; remote: the shown remote's copy, None when the shown workspace isn't
  remote); `notes_in(&self, space: &str) -> Option<&Notes>`, `checklists_in(&self, space: &str) ->
  Option<&Checklists>`; `Workspace::older_content(&self) -> Option<String>` (the shown remote's tools are read and
  lack `edit_item`: the older words); `Workspace::remote_notes(&mut self, id: &str, window, cx)` (for remote `id`: one
  readonly editor per note a note block of home's workspace shows, made with `note::editor`, no subscription, in
  `note_editors`; an editor whose value isn't the copy's text gets `set_value` with its selection kept, clipped; editors
  of notes no longer shown dropped; then `lay_note_tabs`); `Space::lay_note_tabs(&mut self, remote: &RemoteSpace,
  state: &State)` (each remote note block's tabs: home's `notes.tabs` for its home id, in home's back-to-front order,
  each note once across the workspace and only one with an editor; a block whose tabs were empty starts on home's
  `shown`, else keeps its shown note by id, clamped); `Workspace::tell_note_read_only(&mut self, block: BlockId,
  window, cx)` (once per home block per run, through `RemoteSpace::told`); `note::would_type` (a key with `key_char`
  and no Ctrl, Alt or the platform key; Enter, Backspace, Delete, Tab; the platform's paste and cut: true).
- `remote_body`: `"note"` → `Body::Note(Note { remote: true, ..Default::default() })` (tabs laid later);
  `"checklist"` → `Body::Checklist(Checklist { list: b.checklist, editing: None, scroll: Default::default(), remote:
  true })`; `mirror_blocks` sets a remote checklist's `list` to `b.checklist` each time.

- [ ] **Step 1: Write the failing tests.** `note.rs`: `would_type_is_a_key_that_types_or_pastes` — "a", "shift-A",
  "enter", "backspace", "tab", "secondary-v" (the platform's) → true; "ctrl-t", "alt-1", "f5", "escape", "secondary-c",
  "pageup" → false. `tests/remote_content.rs`:

```rust
#[gpui_kit::test]
fn a_remote_note_shows_home_s_text_read_only(cx: &mut TestAppContext) {
    // live_content; n1's body is Body::Note with remote true; its tabs are [7, 9], the shown one 9, its editor's value
    // "one"; block::title is "Ideas". Focus n1 and simulate typing "x": the value is still "one", ws.notes (the
    // mirror's own) unchanged, said once "Notes are read-only on a mirror for now: edit them on …"; a second key says
    // nothing more. Ctrl+F reaches the editor's find (searchable) — assert only that no sentence is said for it.
}

#[gpui_kit::test]
fn a_remote_note_s_shown_tab_is_the_mirror_s_own(cx: &mut TestAppContext) {
    // show_note_tab(n1, 0): shown 7. A state where home shows 7 → still 7; home shows 9 again → still 7. A state where
    // home's block drops note 7 from its tabs → tabs [9], shown 9.
}

#[gpui_kit::test]
fn home_s_text_follows_into_a_remote_note_and_keeps_the_selection(cx: &mut TestAppContext) {
    // Select 0..3 of note 7's editor ("fir"); a state whose note 7 is "first\nsecond\nthird" (a new Arc): the editor
    // shows it, the selection is still 0..3; a state shortening it to "fi": selection clipped to 0..2.
}

#[gpui_kit::test]
fn a_note_home_cut_ends_with_the_rest_is_on_home(cx: &mut TestAppContext) {
    // Note 9 with cut true: its editor's value ends with "\n\n… The rest is on …" (computer_of).
}

#[gpui_kit::test]
fn a_note_is_in_one_tab_at_most_on_a_mirror(cx: &mut TestAppContext) {
    // Home lists note 7 in n1's tabs twice and in a second note block n2's: n1's tabs [7, 9], n2's []; n2 says "No
    // note open here: open one on …"; note_editors holds 2 editors. A tab naming a note the state lacks is left out.
}

#[gpui_kit::test]
fn a_remote_note_s_menu_dims_what_edits_and_sends_to_home_s_terminals(cx: &mut TestAppContext) {
    // n1's block menu: Rename…, Color and All notes… are dimmed items whose tooltip is "Notes are read-only on a mirror
    // for now: add and edit notes on …"; Copy all copies "one"; Send to ▸ lists "claude" (b1) and no local terminal
    // (Full share; Control: the dimmed Send to). The header's All notes button is dimmed with the same words; the
    // tab strip's + and ✕ are dimmed and their clicks change nothing.
}

#[gpui_kit::test]
fn a_remote_checklist_draws_home_s_items(cx: &mut TestAppContext) {
    // c1 is Body::Checklist with remote true, list Some(12); "checklist-box-{n}-13", "-14" and "checklist-text-{n}-13"
    // drawn; block::title "Release 1/2"; the header's All checklists button is dimmed with "Not on a mirrored workspace
    // yet". A state whose c1 shows no checklist: "No checklist open here: open one on …" and no New checklist button.
}

#[gpui_kit::test]
fn no_local_tool_sees_a_remote_note_or_checklist(cx: &mut TestAppContext) {
    // The mirror's own first workspace has a note block showing a local note 7 "Mine" and a checklist block showing a
    // local list 12 "Local". With r1 shown: note_open_at(7) and checklist_open_at(12) are the local blocks; the Notes
    // and Checklists dialogs' rows say "Mine" and "Local" only; Ctrl+Z with c1 focused leaves ws.checklists as it was
    // (it reaches the canvas's undo); ws.api_state(cx) holds notes [7 "Mine"] and checklists [12 "Local"] only, and
    // no block of r1.
}

#[gpui_kit::test]
fn an_older_home_s_notes_and_checklists_stay_cards(cx: &mut TestAppContext) {
    // Tools read without edit_item: n1 and c1 draw the element ("mirrored", n) saying "…'s Templum is older: update it
    // to see this here." and no "note-editor"/"checklist-list" element. Tools unknown (None): live as before.
}

#[gpui_kit::test]
fn a_kanban_s_all_boards_is_dimmed_when_the_mirror_can_t_show_boards(cx: &mut TestAppContext) {
    // live_board, then tools without show_board: the header's All boards button is dimmed with the Older words and
    // its click opens no dialog (the minor's fix).
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum note::tests workspace::tests::remote_content` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say. The view, menu and title of a remote block read the copy and only
  the copy; every local path keeps `Body::note()`/`checklist()`. Rewrite the tests named under Files.
- [ ] **Step 4: Run them** → PASS; `cargo test -j 4 -p templum workspace::tests::notes workspace::tests::checklists
  workspace::tests::remote workspace::tests::remote_boards workspace::tests::menus workspace::tests::editor_tabs` →
  PASS.
- [ ] **Step 5: Minors.** Delete "A kanban's header button All boards isn't dimmed on a mirror that can't show boards."
  Add: "A remote note's scroll may move when home's text changes" (`workspace/remote_content.rs`, `remote_notes`;
  `set_value` keeps the selection, not the scroll; fix: keep the editor's scroll offset across the set);
  "A remote note, a remote checklist and home's ink were checked headlessly only" (`note.rs`, `checklist.rs`,
  `ink.rs`; fix: look at them with two `--dev` instances, one an older home).

---

### Task 5: Acting on a remote checklist: check, add, edit, delete

**Files:**
- Modify: `crates/templum/src/workspace/checklist_blocks.rs` (`ChecklistAct`, `Workspace::act_on_checklist`; the
  field on a remote: `edit_field` reads the copy and takes `Field::New`, `field_changed` writes nothing, Enter and
  blur send through the door with the workspace the field was opened on, `escape_field` drops it, `backspace_at_start`
  does nothing), `crates/templum/src/checklist.rs` (`Field::New`; the check box and ✕ call `act_on_checklist`; "+ Add
  an item" on a remote opens `Field::New`, drawn as a field row after the items; Esc calls `escape_field`; on a remote
  the controls whose deed is refused are dimmed with the words, read once a frame through `menu::Gates`)
- Modify: `crates/templum/src/workspace/remote_content.rs` (`Workspace::remote_checklist_act`; `lay_item_holds` in
  `mirror_checklists`), `crates/templum/src/workspace/remote.rs` (`Deed::{CheckItem, AddItems, EditItem,
  DeleteItem}` and their `call`; `expire_holds` expires the three item holds and redraws through `remirror`)
- Test: `crates/templum/src/workspace/tests/{checklists.rs,remote_content.rs}`,
  `crates/templum/src/workspace/remote.rs`'s `every_deed_home_runs_has_a_scope_in_the_registry`

**Interfaces:**
- Produces:

```rust
/// What the person does to a checklist block's items: on this computer an edit of `Workspace::checklists` (one Ctrl+Z
/// step each), on a mirrored workspace a call of home's.
pub(crate) enum ChecklistAct {
    /// The check box: checked, or unchecked.
    Toggle { item: u64 },
    /// A row's ×.
    Delete { item: u64 },
    /// An item's whole text (a mirror's field sends it once, on Enter or when it loses the keyboard).
    Edit { item: u64, text: String },
    /// New items at the end, a line each (a mirror's + Add an item).
    Add { text: String },
}
```

  `Workspace::act_on_checklist(&mut self, space: &str, block: BlockId, act: ChecklistAct, window, cx)`: the block's
  checklist in workspace `space` (`checklist_view`); on this computer `Toggle` is today's `toggle_item`, `Delete`
  today's `delete_item`, `Edit` `item_edit`, `Add` each `parse_items` line through `edit_checklist`'s `add_item` at the
  end (one step), then `cx.notify()`; on a mirrored workspace `remote_checklist_act`. The view's check box and ✕ go
  through it on both.
- `Workspace::remote_checklist_act(&mut self, space: &str, block: BlockId, act: ChecklistAct, window, cx)`: the deed is
  asked first (refused: `warn` its words, nothing sent); home's block id is `home_of(block)`; then:

  | Act | Call | Held |
  |---|---|---|
  | `Toggle` | `check_item {block, item, done: !shown}` (`shown`: the copy's, holds included) | `items_checked[(list, item)] = done` |
  | `Delete` | `delete_item {block, item}` | `items_deleted[(list, item)] = true` |
  | `Edit` | `edit_item {block, item, text: text.trim()}`; nothing when blank or the copy's text already | `items_edited[(list, item)] = text.trim()` |
  | `Add` | `add_items {block, text}`; nothing when blank | no |

  A hold is laid and the copy redrawn (`remirror`) before the call; every act leaves an undo mark (`mark_remote`) when
  `space` is the shown one. A refusal ends the act's hold, redraws home's last state and says the sentence.
  `lay_item_holds` (end of `mirror_checklists`): a deleted item left out until home's list lacks it; a checked item's
  `done` and an edited item's `text` the held ones until home's equal them; each ends then, at a refusal, or after
  `HOLD`; a list a hold was laid over goes into `lists_laid` (rebuilt from home's `Arc` next time).
- The field on a remote: `Field::Item(id)` starts with the copy's text, `Field::New` empty; no key is written anywhere;
  Enter on `Item` sends `Edit` and stops; Enter on `New` with text sends `Add` and opens a new `Field::New`, blank
  stops; blur sends what Enter would (without reopening) and stops; Esc (`escape_field`) drops the field, nothing sent.
  The field's event handlers carry the workspace it was opened on and act there by id. A remote checklist's name is not
  clickable and its rows can't be dragged (`EditChecklist`: "Not on a mirrored workspace yet" as the name's tooltip).

- [ ] **Step 1: Write the failing tests.** `tests/checklists.rs`:

```rust
#[gpui_kit::test]
fn every_item_act_is_one_ctrl_z_step_through_one_door(cx: &mut TestAppContext) {
    // A local checklist (two_items): act_on_checklist(first, block, Toggle { a }) checks a; Delete { b } deletes it;
    // Edit { a, "A" }; Add { "c\n[x] d" } adds c and d (checked) at the end; each is one undo_checklists step.
}
```

`remote.rs`: `every_deed_home_runs_has_a_scope_in_the_registry` gains the four deeds (all Control).
`tests/remote_content.rs`:

```rust
#[gpui_kit::test]
fn a_box_checked_on_the_mirror_sends_check_item_and_shows_at_once(cx: &mut TestAppContext) {
    // live_content; click "checklist-box-{n}-13": sent == [("check_item", {"block": "c1", "item": 13, "done":
    // true})]; the copy's 13 is done at once; ws.space.undo ends with Change::Mark; ws.checklists untouched. A state
    // whose 13 is done ends the hold. Again with no state: expire_holds(now + HOLD) unchecks it. Again, answered with
    // refusal("There's no item 13 in \"Release\"."): unchecked at once, said once.
}

#[gpui_kit::test]
fn an_item_deleted_on_the_mirror_hides_until_home_drops_it(cx: &mut TestAppContext) {
    // ✕ of 14 → ("delete_item", {"block": "c1", "item": 14}); the copy lacks 14 at once; back after HOLD with no
    // state; a refusal brings it back at once and is said.
}

#[gpui_kit::test]
fn an_item_edited_on_the_mirror_goes_once_on_enter_and_esc_drops_it(cx: &mut TestAppContext) {
    // Click 13's text, type " v1": nothing sent, the copy and ws.checklists unchanged while typing; a state from home
    // meanwhile renames 14 (the field keeps "Tag it v1"); Enter → sent == [("edit_item", {"block": "c1", "item": 13,
    // "text": "Tag it v1"})], the copy shows it at once (held). Click 13 again, type "x", Esc → nothing more sent, the
    // copy still "Tag it v1". Click it, select all and delete (blank), Enter → nothing sent, the item stays.
}

#[gpui_kit::test]
fn add_an_item_on_the_mirror_sends_add_items_and_opens_the_next(cx: &mut TestAppContext) {
    // Click ("checklist-add", n): a field opens, no item added here; type "Release notes", Enter → ("add_items",
    // {"block": "c1", "text": "Release notes"}) and a new empty field; Enter on it → nothing sent, the field closes.
    // Type "x" in a new field and click elsewhere (blur) → ("add_items", {"block": "c1", "text": "x"}).
}

#[gpui_kit::test]
fn an_item_field_left_by_switching_workspaces_sends_to_its_own_remote(cx: &mut TestAppContext) {
    // Click 13's text on r1, type " v2", switch the shown workspace to the local one (an agent's show): the blur sends
    // ("edit_item", {"block": "c1", "item": 13, "text": "Tag it v2"}) to "r1" (ws.calls' space), and the local
    // workspace's checklists are untouched.
}

#[gpui_kit::test]
fn a_remote_checklist_dims_what_it_can_t_do(cx: &mut TestAppContext) {
    // Not live: the check box, ✕ and + Add an item are dimmed with "Not connected." and their clicks send nothing.
    // Live: the name's click opens no field (its tooltip "Not on a mirrored workspace yet"); a row can't be dragged
    // (drop_on 13 onto 14 sends nothing and keeps the order); the block menu's All checklists… is a dimmed item.
}

#[gpui_kit::test]
fn on_a_mirror_ctrl_z_after_an_item_act_stops_at_the_mark(cx: &mut TestAppContext) {
    // A block moved (held), then 13 checked: Ctrl+Z with c1 focused says "On a mirror, Ctrl+Z undoes moves only: undo
    // the rest on …" and moves no block; the next Ctrl+Z undoes the move.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::checklists workspace::tests::remote_content
  workspace::remote` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say. Behaviour on this computer is unchanged (the per-key writes, Enter
  adding the next item after this one, Backspace at an empty item's start).
- [ ] **Step 4: Run them** → PASS; `cargo test -j 4 -p templum workspace::tests::checklists workspace::tests::history`
  → PASS.

---

### Task 6: End to end, the bench, the docs and the full check

**Files:**
- Test: `crates/templum/src/workspace/tests/mirror_end_to_end.rs` (three cases),
  `crates/templum/src/workspace/tests/remote_content.rs` (the two-ends case)
- Modify: `crates/templum/src/bench.rs` (`strokes(ws)`: the 200 strokes of 50 points `run` draws, moved into a function
  `run` and `mirror_set_up` both call; `mirror_set_up` draws them on home's first workspace before sharing, so the
  mirror paints them; the module doc says so)
- Docs: `docs/api.md` (Templum to Templum ▸ 7), `docs/specs/checklist.md` (a section "On a mirrored workspace (Templum
  to Templum, part 2e)": drawn from home's state, check/add/edit/delete as home's tools, the holds, the field sending
  once, what is dimmed), `docs/specs/cockpit.md` (notes and ink on a mirror: read-only notes, the tab the mirror's own,
  home's ink painted and not pickable), `docs/specs/templum-to-templum.md` (Status: "parts 2a–2e built (plan
  …-2e-checklists-notes-ink)"; "As built ▸ Batch 2e": what the builder chose or changed, and this plan's "Open for
  the orchestrator" as settled), `docs/ARCHITECTURE.md` (`workspace/remote_content.rs`: a remote's copies of home's
  notes, checklists and drawings and the acts on its checklists; `act_on_checklist`, the one door of a checklist's item
  writes), `docs/minors.md`

- [ ] **Step 1: Write the end-to-end cases** (one workspace both ends; home's blocks made before `mirrored`; on `r1`
  the acts go through `act_on_checklist("r1", …)`, the clicks being Task 5's tests):

```rust
#[gpui_kit::test]
fn an_item_checked_added_edited_and_deleted_on_the_mirror_is_at_home(cx: &mut TestAppContext) {
    // add_block_in(first, NewBlock::Checklist "Release": "Tag it", "Ship"), mirrored, pump_home until r1's copy has
    // the list. On r1's checklist block: Toggle { tag } → pump_home until home's Tag it is done; Add { "Notes" } →
    // home's items end with "Notes"; Edit { ship, "Ship it" } → home's is "Ship it"; Delete { tag } → home lacks it.
    // r1's copy agrees and its three item holds are empty.
}

#[gpui_kit::test]
fn a_note_typed_at_home_shows_on_the_mirror(cx: &mut TestAppContext) {
    // three_blocks (note "hello"), mirrored. Home's note editor set_value("hello there") (its Change writes the
    // notes, as typing does) → pump_home until r1's note block's shown editor reads "hello there" and r1's copy too.
}

#[gpui_kit::test]
fn a_stroke_drawn_at_home_shows_on_the_mirror(cx: &mut TestAppContext) {
    // mirrored, then a stroke pushed onto the first workspace's ink (as the pen ends one) → pump_home until r1's
    // space.ink == [that stroke]; ws.ink_at on it, with r1 shown, is None.
}
```

`tests/remote_content.rs`, two ends (`shown_mirror_as(cx, Scope::Control)`: home's three_blocks note "hello"; home
adds a checklist block "Release" with two items; the mirror's `notes.json` and `checklists.json` read before):

```rust
#[gpui_kit::test]
fn acts_on_a_mirror_leave_its_notes_json_and_checklists_json_as_they_were(cx: &mut TestAppContext) {
    // On the mirror: Toggle, Add, Edit, Delete through act_on_checklist; home types into its note and draws a stroke;
    // pump_home until the mirror shows each (home's list, the note's text, r1's ink). Save the mirror
    // (save_canvas_now and the stores' save): its notes.json and checklists.json are byte for byte as before, and its
    // ws.notes and ws.checklists are as before.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::mirror_end_to_end
  workspace::tests::remote_content` → all pass (fix what fails in the task that owns it).
- [ ] **Step 3: Write the docs** above. `docs/api.md` ▸ 7: the first sentence becomes "Not mirrored: an editor's
  content (a card names it)."; "adding notes (notes are read-only on a mirror for now)" becomes "editing notes (a note
  is read-only and live there: select, copy and find in it, send it to a terminal)"; add "A checklist is live: its
  items are checked, added, edited and deleted from the mirror, each done at home (an item's text goes when you press
  Enter or leave it); a check, an edit or a delete shows at once and home's state wins after two seconds. Renaming a
  checklist and moving its items stay on home. Home's ink and shapes are drawn, not picked: draw on home." and
  "Drawing" stays in the list of what a mirror can't do.
- [ ] **Step 4: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 5: The benches** (the mirror paints notes, checklists and home's ink now): `cargo run -j 4 --release -p
  templum -- --bench` then `cargo run -j 4 --release -p templum -- --bench-mirror` → `p95_ms` ≤ 16.7 each; report both
  JSON lines, and 2d's `--bench-mirror` line beside the new one.
- [ ] **Step 6: Leave uncommitted;** report the files changed, the checks' and the benches' results, what went to and
  left `docs/minors.md`, and how each UI-only change was verified.

---

## Interfaces for later parts

What 2e leaves for 2f–2h and the later cards, by name:

- **Home's state:** `NoteView`, `ChecklistView`, `DrawingsView`, `DrawingView`, `NotesShown`, `NOTE_TEXT_MOST`,
  `DRAWINGS_MOST`, `POINTS_MOST`, `newest_drawings`; the events `note`, `checklist`, `drawing`; `State::visible`'s
  rule (a note or checklist by the visible blocks that show it, drawings by workspace). 2h's `schedules` and the
  agent's `task` follow the same shape (a collection with its singular event, `keeps_items`, `parse`, `diff`, the
  crowded list, an openapi schema in `schemas_with_views`).
- **Home's operations without a caller:** `Workspace::{item_text, item_delete}` beside `item_check` and `items_add`;
  the gate's `called_block` and `approval_words` for a block-targeted Ask own.
- **A remote body that local lookups skip:** `Note::remote`, `Checklist::remote` (with 2d's `Kanban::remote`);
  `Space::{note_view_mut, checklist_view, checklist_view_mut}` answer any; `Body::{note, checklist}` local only.
- **Reading a remote's copies:** `Workspace::{notes_of, notes_in, checklists_of, checklists_in}` beside 2d's
  `boards_of`, `boards_in`; `RemoteSpace::{notes, checklists, note_editors}`; home's drawings are the remote
  `Space::ink`.
- **One door for a checklist's item writes:** `ChecklistAct` + `act_on_checklist`; the item holds `items_checked`,
  `items_edited`, `items_deleted` laid by `lay_item_holds` and expired in `expire_holds`, as 2d's card holds.
- **An older home's content:** `Workspace::older_content()` (tools lack `edit_item`) and `mirrored_card(kind, title,
  sentence)`; 2g's Git popup and 2h's popups can say "older" the same way when a tool is missing.
- **Dimmed header buttons and tabs:** `header_button`'s `why`, `block::Gates::{boards, notes, checklists}`,
  `tab_strip`'s `locked`.
- **Typing that isn't sent:** `note::would_type` and `tell_note_read_only` (once per block per run,
  `RemoteSpace::told`).
- **Later cards:** editing notes from a mirror replaces the readonly editor and `tell_note_read_only`; drawing from a
  mirror makes `ink_at` and `begin_ink` ask a deed instead of `on_remote()`; renaming a checklist and moving its items
  add `ChecklistAct` variants.
- **Benches:** 2f's and 2g's `--bench-mirror` compare with Task 6's line (notes, checklists and 200 strokes live).

## Open for the orchestrator

Each decided with the recommended option; the plan is written that way.

1. **A remote note or checklist is `Body::Note`/`Body::Checklist` with a `remote` flag,** which `Body::note()`/
   `checklist()` skip, as 2d's kanban: the spec's "the same Body, drawn by the same view" holds and every local lookup
   by id skips home's ids by construction. *Recommended.*
2. **Home's `notes` and `checklists` hold only those a block shows** (the spec's words), unlike `boards`, which holds
   every board: less to send, and a limited token's view is the same filter. *Recommended.*
3. **`checklist` is left out (not null) on a checklist block that shows none,** and `notes` is on every note block
   (its tabs maybe empty): serde's `skip_serializing_if`, documented as such. *Recommended.*
4. **An older home is told by its tools** (`GET /v1/tools` lacks `edit_item`, which 2e adds to the control server),
   not by the state: a checklist block that shows none and one of an older home look the same in the state. Tools not
   read dim nothing (2a's rule): the blocks are live. *Recommended.*
5. **Holds:** a check (the spec's), an edit and a delete of an item stand over home's state, as 2d held card edits and
   deletes, so an edit sent on Enter doesn't flicker back for a round trip; added items show a round trip later.
   *Recommended.*
6. **+ Add an item on a mirror opens a field for a new item:** Enter sends `add_items` and opens the next field (a list
   is typed as locally); a blank Enter closes it; blur sends; Esc drops. `add_items` only appends, so Enter in an
   existing item sends its edit and stops (locally it opens a new item after it). *Recommended.*
7. **A blank edit on a mirror sends nothing and the item stays** (✕ deletes): locally an emptied item goes, but an
   accidental select-all and blur would delete home's item. Backspace at an empty field's start does nothing there.
   *Recommended.*
8. **Item acts leave an undo mark,** as card acts do (2d): Ctrl+Z on a focused remote checklist says the sentence
   instead of undoing an older block move. *Recommended.*
9. **A cut note's sentence is appended to the copy's text** ("\n\n… The rest is on winx-desktop."), so the editor shows
   it at the end and Copy all and Send to carry it, which tells the reader the text is partial; no footer element.
   *Recommended.*
10. **Home's drawings are the remote `Space::ink`,** painted by `ink::paint` unchanged; a remote space is never saved
    and its ink tools are guarded, so nothing writes it but the copy. *Recommended.*
11. **`ink_at` answers None on a mirror:** one guard covers picking, moving, the ink menu and the eraser's hit, and a
    right-click there opens the canvas menu. *Recommended.*
12. **A shape counts 2 points toward the 100,000;** the caps keep the newest (the spec's "the oldest left out").
    *Recommended.*
13. **A remote note's editor keeps the person's selection (clipped) when home's text changes;** its scroll may move (a
    minors entry). *Recommended.*
14. **"Says once" for typing in a remote note is once per note block per run,** through `RemoteSpace::told`, as
    typing's own sentences are. *Recommended.*
15. **Text caps on the mirror count characters, as 2d's** (a note's 256 KiB is 262,144 characters there; home cuts at
    262,144 bytes, so a home's text is never cut again). *Recommended.*
16. **Each of home's notes is in one tab at most across a mirrored workspace** (home's own rule, kept against a hostile
    home), and only notes a block shows get an editor. *Recommended.*
17. **`DrawingView` mirrors `session::InkItem` field for field** with `From` both ways, not one shared type: the
    canvas file's format stays the canvas file's. Home leaves `highlight: false` out, as the file does (the spec's
    example shows it). *Recommended.*
18. **A remote note block with no tab says "No note open here: open one on winx-desktop.",** as the checklist's
    sentence (the spec gives the checklist's only). *Recommended.*
19. **The Templum Agent's `delete_item` asks with "Delete item 13 of \"Release\"" and the item's text,** its facts
    holding the text so an item edited since asks again. *Recommended.*
20. **The minor "A kanban's header button All boards isn't dimmed…" closes here,** since `header_button` gains the
    rule's words for All notes and All checklists anyway. *Recommended.*

## Found against the code as built

- **The end-to-end test is one workspace for both ends:** home's notes and checklists are the mirror's own stores,
  with the same ids, so "`notes.json` and `checklists.json` on the mirror unchanged" can't be shown there. The byte
  check runs with two ends (`shown_mirror_as`, Task 6), as 2d's did for `boards.json`.
- **A checklist block that shows none and an older home's look the same in the state,** so the spec's rung 2 ("A state
  without batch 2's fields shows notes and checklists as batch 1's card") is told by home's tools (Open 4).
- **The client's weight cap is below the notes' cap:** `WEIGHT_MOST` is 4 × 16 MiB = 64 MiB of event data per
  remote, while 1,000 notes of 256 KiB are 256 MiB. A home whose shared notes weigh past 64 MiB is "larger than
  Templum keeps" before the copy's caps apply. Kept: memory stays bounded, and a share sees only its workspace's notes.
- **A note's editor needs a window** (`EditorState::new(window, cx)`), and `remirror`, which redraws home's state
  after a hold, has none: the editors live in `RemoteSpace::note_editors`, made in `apply_remote`, and a pass that
  needs no window lays them into the blocks' tabs (`lay_note_tabs`), so a block `remirror` brings back has its tabs.
- **The checklist's field writes every key to `Workspace::checklists`** (`field_changed`, `stop_editing`'s blank-item
  cleanup, `backspace_at_start`), and its view-state lookups go through `Body::checklist()`: on a mirror the field
  writes nothing and sends once (Task 5), and its lookups use `checklist_view`, which answers a remote block too.
- **`edit_item` and `delete_item` exist only in the block server, behind a wire** (`content.rs`'s
  `wired_checklist`): their bodies become `item_text`/`item_delete`, which take a block, as `item_check` does.
- **The gate's `called_block` knows only terminal-targeting calls:** `delete_item`'s Ask own needs its block there and
  its own approval words, or it would run without asking (as `delete_card` would have in 2d).
- **A checklist's `name` can be blank;** the state carries `name_shown()` ("Checklist"), what the block shows.
- **Home's drawings format is the canvas file's `InkItem`,** which already cleans what it reads (`InkItem::drawing`);
  the mirror reuses it and adds the ±10⁷ bound and the caps. The spec's example shows `"highlight": false` on a stroke,
  which the file leaves out (Open 17).
- **Find in a note is gpui-kit's editor's own** (`EditorState::new` is searchable; Ctrl+F is bound by the editor): a
  readonly editor keeps it, so nothing is built for Find. **Send to ▸** on a remote note already lists the remote's
  terminals (2d's `send_to_menu`).
- **The mixed cockpit has no strokes** and its note is empty: `--bench-mirror` gains the ink bench's 200 strokes on
  home's workspace (Task 6); the note stays as the mixed scenario has it.
