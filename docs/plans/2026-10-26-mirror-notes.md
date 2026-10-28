# Plan: notes on a mirror (kanban card 204)

- **Date:** 2026-10-26
- **Card:** 204, "Templum Mirror - falta integrar sistema de notas, user deve poder criar notas" and "user deve poder
  alem de ler: criar, editar, renomear, mudar a cor ou reabrir uma nota" (on a mirrored workspace the person must be
  able, besides reading, to make, edit, rename, recolor and reopen a note).
- **Design (what the person sees, the words):** [docs/specs/templum-to-templum.md, Notes on a mirror (kanban card
  204)](../specs/templum-to-templum.md#notes-on-a-mirror-kanban-card-204). This plan is how to build it. Where the two
  differ, the spec's words win and the plan's rules win.
- **For:** a Sonnet implementer and an Opus reviewer, each starting from this file alone. Every choice the planner made
  without the person is marked *the planner's call*.

## 0. Read first

1. `CLAUDE.md` (binding): crate boundaries, "Every feature in the API", tests that fail without their code, minors,
   no `unsafe`, the full check command, `--bench-mirror` for a change to a mirror's painting.
2. `docs/specs/templum-to-templum.md`: Batch 2 ("One rule for what the mirror can do", holds, Constraints, the threat
   model) and the new section "Notes on a mirror (kanban card 204)".
3. The two cards just done the same way: `git show aac555e0` (card 205: checklists on a mirror, the door
   `remote_checklist_act`, holds, `still_on`, the two-ends test) and `git show 090c6031` (card 203: an API-own tool,
   `Own::RecentFolders`, a test over real loopback).
4. The local notes: `crates/templum/src/notes.rs` (`Notes`, `Entry`, `set_text`, `rename`), `note.rs` (`Note`,
   `NoteTab`, `body`, `editor_box`, `empty`, `remote_empty`, `open_notes`, `note_rows`), `workspace/note_tabs.rs` (every
   local act), `workspace/ops.rs` (`note_replace`: how home writes a note through its editor, one Ctrl+Z step).
5. The mirror's notes today: `workspace/remote_content.rs` (`note_of`, `mirror_notes`, `remote_notes`,
   `tell_note_read_only`, `shown_notes`, `lay_note_tabs`, `remote_checklist_act`), `workspace/remote.rs`
   (`RemoteSpace`, `Deed`, `Deed::call`, `Deed::needs`, `RemoteSpace::why_not`, `Nay`, `NOT_MIRRORED`, `typing_into`,
   `remote_add_in`, `awaiting`, `focus_awaited`, `remote_call`, `expire_holds`, `remirror`, `apply_remote`,
   `still_shown`), `workspace/holds.rs`.
6. Home's API: `crates/templum-api/src/model.rs` (`NoteView`, `NOTE_TEXT_MOST`, `NOTES_TEXT_MOST`, `State::visible`),
   `crates/templum-api/src/registry.rs` (`OWN`, `TARGETS`, `NOT_TARGETS`, `Own`, `parse_own`, `tools()`, `scope`,
   `registry::no`), `crates/templum/src/workspace/api.rs` (`api_allows`, `api_own`, `answer_line`, `Target`),
   `workspace/api_state.rs` (`note_views`).
7. gpui-kit's editor, never guessed: `~/.cargo/registry/src/*/gpui-base-0.7.3/src/input/base/state.rs`: `set_value`
   (clears history, no `Change`), `replace` (one undo step, emits `InputEvent::Change` after the update's effects
   flush), `selected_range`/`set_selected_range` (UTF-8 byte offsets; `set_selected_range` scrolls to the caret through
   `move_to`), `scroll_offset`/`set_scroll_offset` (deferred to the next layout). Events are delivered after the
   update that emitted them, not inside it.

## 1. The feature in one paragraph

Home gains five API-own tools (`edit_note`, `new_note`, `open_note`, `close_note`, `delete_note`), a per-note text
revision `rev` in the state, and remembers the workspace a closed note was last open in (`closed_in`), so a share can
list and reopen the notes of its workspace. The mirror's note editors become editable: typing is local and instant,
the whole text goes home after a pause with the `rev` it was typed on, home writes it only on that `rev` (a
compare-and-swap), and when home's text moved on the mirror merges the two changes or, when they meet, keeps the
person's text as a new note. Tabs, rename, color, All notes…, Add ▸ Note and the empty block's buttons call home's
tools through one door, held over home's state as card 205's acts are.

## 2. Decisions (*the planner's call* each)

1. **API-own tools, not control tools.** These are the person's acts from another computer (as `git`, the keyboard
   and automations are), and a text compare-and-swap only makes sense for a client that reads the state. The Templum
   Agent keeps `append_note`; templum-mcp and docs/specs/templum-agent.md don't change.
2. **The whole text, not edits, crosses** (`edit_note`'s `text`), with the `rev` it was made on. Home computes the
   ranged change itself and writes it through its editor (one Ctrl+Z step, home's caret kept). At most 256 KiB, so a
   body stays under the server's 1 MiB even JSON-escaped.
3. **`rev` is a counter, not a hash:** O(1) at home, and monotonic, so the mirror can tell a stale state from a new one.
   `Notes` keeps the last one given out (in memory), seeded at launch with the Unix time in ms (as
   `Workspace::next_wait` is): a restarted Templum never reuses one. Each note's `rev` changes only when its text does.
4. **The mirror merges, home only compares.** Home never applies an edit to a text other than the one it was made on.
   The mirror's merge is a pure function in `notes.rs`, tested headlessly.
5. **A conflict keeps both:** home's text stays in the note; the person's becomes a new note in the same block (a new
   block where it was, when that is gone), named "{name} ({share})", `{share}` being the share's name from
   `GET /v1/me` (`RemoteSpace::me.name`): home's person knows the mirror by it.
6. **A note's sends are one at a time,** and the next waits until home's state shows the last (a `rev` greater than
   the one it was made on): the answer and the event stream are two connections.
7. **Closed notes join the state** (without text, with `workspace`), filtered per token by `State::visible`: All
   notes… on the mirror is live from the copy, with no read tool. Only notes closed since this version have a
   `closed_in`; older closed notes aren't listed to anyone through the API (they stay in home's own Notes dialog).
8. **No hold for typing.** Batch 2's 2 s hold would throw the person's text away; text stays until home has it, and
   the " •" after 2 s keeps "an act that seems done and isn't" honest.
9. **Closing a tab or a note block whose text isn't at home yet waits for it** (`RemoteSpace::closing`): no second
   send can be lost behind a close.

## 3. Home: `notes.rs`

### 3.1 `rev` and `closed_in`

- `Entry` gains `#[serde(skip)] pub rev: u64` and
  `#[serde(default, skip_serializing_if = "Option::is_none")] pub closed_in: Option<String>` ("the workspace whose block
  last showed it, while no block does"). `VERSION` stays 1 (an older Templum ignores the field).
- `Notes` gains `#[serde(skip)] pub rev: u64`, the last revision given out.
- `Notes::stamp(&mut self, seed: u64)`: `self.rev = seed` and every entry's `rev = seed`.
- `set_text`: when the text differs, `self.rev += 1; note.rev = self.rev` (besides `text` and `changed`).
  `touch(id)` does the same whatever the text (`edit_note`'s write, §5.4). `add` and `duplicate` give the new note
  `rev: self.rev` (no bump: a new id has no older text to meet).
- `close_in(&mut self, id, workspace: &str)` sets `closed_in`; `forget_workspace(&mut self, workspace: &str)` clears
  every `closed_in` naming it (workspace ids are reused: `session::next_id`).
- Where the workspace takes the loaded notes (`workspace/spaces.rs`, `match self.data_dir.as_deref().map(notes::load)`
  → `Loaded::Saved(notes)`): stamp `notes` with the Unix time in ms **before** `notes_saved` is cloned from it, so the
  first comparison doesn't see a change. Tests without a data folder keep rev 0.

### 3.2 The merge (UI-free, in `notes.rs`)

```rust
/// One change from `old` to `new`: the bytes `range` of `old` become `text`, the longest common start and end of the
/// two left out, both on character boundaries. None when they are equal.
pub(crate) struct Splice { pub range: Range<usize>, pub text: String }
pub(crate) fn splice(old: &str, new: &str) -> Option<Splice>;

/// `mine` and `home`, both made from `base`, as one text: their splices from `base` go together when at least one
/// byte of `base` lies between them; two insertions at the same place keep home's first. None: they overlap or
/// touch, a conflict.
pub(crate) fn merge(base: &str, mine: &str, home: &str) -> Option<String>;

/// Where `at`, a byte offset of the text before `splice`, is after it: before its range the same, after it moved by
/// its growth, inside it at the end of the new text.
pub(crate) fn shift(at: usize, splice: &Splice) -> usize;
```

`merge` rules, in order: `mine == base` → `home`; `home == base` or `home == mine` → `mine`; with `a = splice(base,
mine)`, `b = splice(base, home)`: `a.range.end < b.range.start` or `b.range.end < a.range.start` → both applied in
`base`'s coordinates (the later range first); both empty ranges at the same offset → `base[..p] + b.text + a.text +
base[p..]`; else None. The common start is computed first, the common end only over what is left of both (so they
never overlap), each backed off to a char boundary (`is_char_boundary`). `shift` with `at == range.start` stays (a
caret at an agent's append stays before it, as `append_note` leaves it locally).

### 3.3 Writing into an editor (`note.rs`)

`pub(crate) fn write(editor: &Entity<EditorState>, text: &str, window: &mut Window, cx: &mut App)`: with `s =
splice(value, text)` (nothing when None): keep `selected_range()` and `scroll_offset()`; `set_selected_range(s.range)`;
`replace(s.text)`; `set_selected_range(shift(kept.start)..shift(kept.end))`; `set_scroll_offset(kept_scroll)`. One
undo step; works on a read-only editor (gpui-base lifts read-only for programmatic edits: `with_edits_allowed`).
Used by home's `edit_note` and by the mirror for every write of home's text. If `set_scroll_offset` doesn't win over
`move_to`'s scroll (the minor "`append_note` scrolls the note back to the person's caret" says it may not), say so in
the review and keep the minor; don't fight gpui-base.

## 4. Home: the API (`templum-api`)

### 4.1 `model.rs`

- `NoteView` gains `#[serde(default)] pub rev: u64`, `#[serde(default)] pub changed: u64` and
  `#[serde(default)] pub workspace: Option<String>` (set only on a note no block shows). Its doc says so. An older home
  sends none: `rev` 0 (and it lacks `edit_note`, so nothing is sent on it).
- `State::visible`: a note is seen when `shown_notes` has it (as today) **or** its `workspace` is one the caller sees.
- Test (model.rs): `a_limited_caller_sees_the_closed_notes_of_its_workspaces_only`: a closed note of `w1` is seen by a
  `w1` token and not by a `w2` one; a shown note as today; an unlimited caller sees all. And in the existing parse
  test, a `note` event from an older home (no `rev`, `changed`, `workspace`) still reads.

### 4.2 `registry.rs`

- `OWN` gains `edit_note`, `new_note`, `open_note`, `close_note`, `delete_note` (11 → 16).
- `NOT_TARGETS` gains `rev` (every other argument name exists). `note` stays a non-target: `answer_request`'s `note` is
  free text, so the glue confines a note itself (§5.3).
- `Own` gains:
  ```rust
  EditNote { block: String, note: u64, name: Option<String>, color: Option<Option<String>>, text: Option<(String, u64)> },
  NewNote { block: String, name: Option<String>, text: Option<String> },
  OpenNote { note: u64, into: NoteInto },
  CloseNote { block: String, note: u64 },
  DeleteNote { note: u64 },
  pub enum NoteInto { Block(String), Workspace { workspace: String, at: Option<(f32, f32)> } }
  ```
- `parse_own` refusals (each a test): `edit_note` without any of `name`, `color`, `text`: "edit_note takes name, color,
  or text with rev"; `text` without `rev`, or `rev` without `text`: "edit_note's text and rev go together: rev is the
  note's rev in the state, which the text was made on"; a color not one of `red`, `yellow`, `green`, `cyan`, `blue`,
  `magenta`, `none`: "\"color\" is red, yellow, green, cyan, blue, magenta or none"; `text` over `NOTE_TEXT_MOST`
  bytes (also `new_note`'s): "A note through the API holds at most 256 KiB."; `open_note` with both or neither of
  `block` and `workspace`: "open_note takes block, or workspace"; `x`, `y` with `block`, or only one of them: "x and y go
  together, with workspace" (bounds ±10⁷ as `add_block`'s); `note` not a whole number: "\"note\" is a note's id, as the
  state shows it". `note` may come as a number or a numeric string, as `item` does.
- `scope`: `delete_note` → Full; the four others → Control (the `_ => Scope::Control` arm already gives it; add
  `delete_note` to the Full arm).
- `tools()`: one entry each, its description the docs/api.md row's words (§8.1), required: `edit_note` `block`, `note`;
  `new_note` `block`; `open_note` `note`; `close_note` `block`, `note`; `delete_note` `note`.
- Tests: extend the scope table test and the `parse_own` test as card 203 did (`git show 090c6031 --
  crates/templum-api/src/registry.rs`); the test that classifies every argument passes with `rev` added.

## 5. Home: the glue (`crates/templum`)

### 5.1 The state (`workspace/api_state.rs`, `note_views`)

Each `NoteView` of a shown note gets `rev: entry.rev`, `changed: entry.changed`, `workspace: None`. Then, after the
shown ones, every note with `closed_in` naming a local workspace that exists (`space_named`, `!is_remote()`) and that
no block shows: `NoteView { id, name, text: "", color, cut: false, rev, changed, workspace: closed_in }`, the most
recently changed first. They take nothing of the 8 MiB budget.

### 5.2 Marking a note closed

`note_tabs.rs`: `drop_if_blank(id)` becomes `note_closed(id)`: a blank note goes as today; any other gets
`notes.close_in(id, &self.space.id)`. Its two callers (`close_note_tab`, `blocks.rs` `remove_block`) act on
`self.space` (an API call reaches another workspace through `in_space`, which swaps it in). `spaces.rs`
`delete_space_as`: `self.notes.forget_workspace(&deleted)` beside `api_forget_workspace`. Closing a note's tab through
`close_note` (§5.4) goes through `note_closed` too.

### 5.3 Dispatch and confinement (`workspace/api.rs`)

- In the API call path, before the generic `api_own`, the five note tools go to `self.api_note(&caller, own, window,
  cx) -> Result<(Reply, Option<Target>), Refusal>`, answered with `answer_line(id, &reply, target.as_ref())` so the
  answer names the block (`new_note`, `close_note`, `edit_note`: the block given; `open_note`: the block it opened in,
  a new one included; `delete_note`: none). The mirror's `awaiting` reads `block` from it.
- `api_allows`: `block` and `workspace` are targets already. For `open_note` and `delete_note`, the note itself must be
  the token's (`note_seen`): open in a block the token sees (`note_open_at`, that block's uid in `seen.blocks`), or
  open nowhere with `closed_in` one of `seen.workspaces`, or the caller sees every workspace. Else
  `not_found("note", id)` ("There's no note 9…", `registry::no`). A token limited to some workspaces can't reach an
  unmarked closed note.
- Every tool on a block of a mirrored workspace at home is refused with `Space::mirror_words`, as `add_block_in` is.

### 5.4 The bodies (`workspace/note_tabs.rs`, beside the local acts)

None of them moves home's keyboard, selection or camera (as `add_block_in`); each that changes a tab list keeps
`note.active` pointing at the same note where it can.

- `note_edit`: the note must be a tab of `block` ("b7k2m shows no note 7."). `name` → `notes.rename` (blank:
  "Untitled", as locally); `color` → `notes.set_color` (`none` → None). `text`: refused when the note's `rev` isn't
  `rev` ("Note 7 changed since: its newer text and rev are in the state."), or when its current text is longer than
  `NOTE_TEXT_MOST` ("Note 7 is longer than the API shows (256 KiB): edit it in Templum on {computer}."); else
  `note::write(tab.editor, text)` and then `notes.set_text(id, editor's value, now)` at once (the editor's `Change`
  comes after this update: without it a second call in the same tick would pass the old `rev`), and the note gets a
  new `rev` even when the text was already that (`Notes::touch(id)`): a caller waiting for a newer `rev` always gets
  one. Answer: "Saved note 7." / "Renamed note 7 "Plan"." / "Colored note 7 blue." (the parts done, joined).
- `note_new`: `block` must be a note block (local, not remote). `notes.add(text or "", None, now)`, then
  `rename(name)` when given; a tab after the last, shown (`active`), no focus. Answer "Made note 12 in b7k2m."
- `note_open`: where the note is open, that tab is shown (`active`), nothing else; else into `Block` as a new tab
  shown; else `Workspace` → `add_block_in(space, NewBlock::Note { text: "", open: Some(id) }, place at x, y or
  default)`: `NewBlock::Note` gains `open: Option<u64>` (the note to show instead of a new one; `add_block` and every
  other caller pass None). Answer "Opened note 9 in b7k2m."
- `note_close`: the note must be a tab of `block`; `remove_tab`, then `note_closed`. Answer "Closed note 7 in b7k2m."
- `note_delete`: the existing `delete_note(id, window, cx)` (from every block too). Answer "Deleted note 7 "Plan"."

### 5.5 Tests (`workspace/tests/api.rs`, through the API as card 205's are)

1. `edit_note_writes_on_its_rev_only_and_keeps_home_s_caret`: a note block with "first\nsecond", home's caret at the
   end of "second"; `edit_note` with the state's `rev` and "zero\nfirst\nsecond" → the note's text and `notes.json`
   have it, the caret is still after "second", one Ctrl+Z of home's editor brings back the old text; the same call again
   (old `rev`) is refused with "changed since"; `append_note` (an agent) then `edit_note` with the `rev` read before it
   is refused (home's own writes move `rev`); name and color in one call; a note of another block ("shows no note");
   a note made longer than 256 KiB at home refuses `text` but takes `name`.
2. `new_close_open_and_delete_a_note_through_the_api`: `new_note` with a name and text → a shown tab, home's focus
   unchanged; `close_note` → the note stays, `closed_in` is the workspace, the state lists it with `workspace` and an
   empty text; a blank note closed goes; `open_note` with `block` reopens it there; with `workspace`, `x`, `y` in a new
   block at that point (its answer names the block); `delete_note` with a Control token is `403`, with Full deletes it.
3. `a_limited_token_reaches_only_its_workspace_s_notes`: a `w2` token's `open_note`/`delete_note` of a note closed in
   `w1`, of a note open in `w1`, and of a note with no `closed_in` are "no note"; its `edit_note` on `w1`'s block is "no
   block"; a `w1` token reopens `w1`'s closed note.
4. `a_deleted_workspace_s_closed_notes_are_no_new_workspace_s`: close a note in `w2`, delete `w2`, make a workspace
   (it gets `w2` again): the state lists the note with no `workspace` (not at all for a limited token).
5. In `notes.rs`'s tests: `set_text` moves `rev` only when the text changes; `stamp`; `add` doesn't bump; `close_in`,
   `forget_workspace`; `closed_in` reads back from the file and an older file without it reads. And `splice`, `merge`,
   `shift`: equal texts; disjoint changes both ways; an append at the end with an edit at the top; a deletion and an
   insertion apart; overlapping and touching changes → None; two insertions at one place → home's then mine; a
   multi-byte text ("ação", "日本") never cut inside a character; `shift` before, inside, after, at the start.

## 6. The mirror

### 6.1 The rule (`workspace/remote.rs`)

- `Deed` (each with its call in `Deed::call`):
  - `AddNote` → `("add_block", {"workspace": "w", "kind": "note"})`, and it is "older" also when home lacks
    `edit_note` (a home that could add a note the mirror can't type in): in `RemoteSpace::why_not`'s Older rung,
    `|| (deed == Deed::AddNote && !tools.has("edit_note"))`.
  - `EditNote` (typing, Rename, Color, the tabs' + and ×, the empty block's New note) → `("edit_note", {"block": "n",
    "note": 1, "name": "x"})`.
  - `OpenNote` (All notes…, the canvas menu's Notes…, the empty block's Open a note…, the header button) →
    `("open_note", {"note": 1, "block": "n"})`.
  - New `DeleteNote` → `("delete_note", {"note": 1})` (Full through `registry::scope`).
- Remove the early `Deed::AddNote | Deed::EditNote → Nay::NotesReadOnly`, `Nay::NotesReadOnly` and its words, and its
  arm in `typing_into`. `NOT_MIRRORED` drops "editing notes". `block::Gates::notes` asks `Deed::OpenNote`; the note
  block's menu `Gates` (`menu/mod.rs`, `Items::Note`) ask `[Deed::EditNote, Deed::OpenNote]`, All notes… gated by
  `OpenNote`, Rename… and Color ▸ by `EditNote`.
- `remote_add_in`: `Kind::Note => (Deed::AddNote, "note")` (no `text`: home names it Untitled). `new_note` and
  `add_note_at` (`note_tabs.rs`) call `remote_add(Kind::Note, at, cx)` on a mirror, as `add_checklist_at` does.
- The tests in `tests/remote.rs` that read `words(&remote, Deed::AddNote)` and the `Deed` list test change with it.

### 6.2 The copy (`remote_content.rs`)

- `note_of` keeps `rev: view.rev`, `changed: view.changed`. `RemoteSpace` gains `notes_cut: HashSet<u64>`: the notes
  whose text home cut or this cleaning cut (`mirror_notes` fills it). A cut note is never edited from the mirror.
- `mirror_notes(state, now)` lays the name and color holds (§6.6) at its end, as `mirror_checklists` lays
  `lay_item_holds`, with a `notes_laid` set re-made from home's state at the next copy. `remirror` calls it too, so an
  ended hold shows home's name again (its `made` is unused there: the texts it re-makes are the editors' already).
- The copy now holds closed notes too (no editor: `shown_notes` lists only tabs).

### 6.3 The editors (`remote_content.rs`, `remote_notes`)

- An editor made for a remote note subscribes (`cx.subscribe_in`, capturing the workspace id and the note id):
  `InputEvent::Change` → `remote_note_typed(space, note, Instant::now(), window, cx)`; `InputEvent::Blur` →
  `flush_note(space, note, window, cx)`.
- Home's text into an editor: for a note in `made` **with no `NoteEdit`** (§6.4), `note::write(editor, copy text)`
  instead of today's `set_value` and clipping. A note with a `NoteEdit` is never written here: `note_steps` decides.
- `editor_box`'s `readonly` becomes `note.remote && ws.note_locked(block, note).is_some()`, where `note_locked` answers
  the first of: the rule's `why_not(Deed::EditNote)` words; the cut note's sentence; "Keeping your version as a new
  note on {computer}…" while a copy is being made.
- The key capture in `note::body` (`would_type`) calls `tell_note_locked(block, note)` instead of
  `tell_note_read_only`: told once per (home's block, reason) through `RemoteSpace::told`, with the tags "not live"
  (told again after the remote was live in between, as `typing_into`'s), "older", "note cut", "note copying". "Not
  connected" says `NOT_CONNECTED` ("Not connected: what you type isn't sent."), the others the spec's sentences.

### 6.4 A note being typed in: `NoteEdit` (new file `workspace/remote_notes.rs`)

`RemoteSpace::edits: HashMap<u64, NoteEdit>` by home's note id, and `note_wake: Option<Task<()>>`.

```rust
pub(crate) struct NoteEdit {
    /// The editor typed in: kept here so the text outlives its tab (a note home closed).
    editor: Entity<EditorState>,
    /// Home's block it was last shown in (home's id), and its rectangle: where a copy goes when the note is gone.
    block: String,
    rect: Rect,
    /// Home's text the editor's text was made from, and its rev; None: the text home took from the last send, whose
    /// rev the next state brings (any rev above `after`).
    base: String,
    rev: Option<u64>,
    after: u64,
    /// The last key; the first key not sent yet; the first key home doesn't have yet (the " •" after HOLD).
    typed: Instant,
    unsent: Option<Instant>,
    since: Option<Instant>,
    /// The text sent, not answered yet, and `unsent` as it was when it went.
    sent: Option<(String, Instant)>,
    /// The last send was refused, and how many times in a row with home's state no newer.
    refused: Option<(Instant, u8)>,
    /// Send now: the editor lost the keyboard, or its tab or block is closing.
    flush: bool,
    /// The person's text is being made a new note at home; the copy was refused (tried again with the next state).
    copying: bool,
    stuck: bool,
}
```

Constants: `PAUSE = 500 ms`, `MOST_WAIT = 2 s` (at least one send that often while the person types), `holds::HOLD`
(2 s), `TRIES = 3`.

**`remote_note_typed(space, note, now)`** (a `Change`): with no `NoteEdit`: when the editor's value equals the copy's
text, nothing (it was a write of home's); else make one with `base` the copy's text, `rev: Some(copy rev)`, its block
and rect from the tab that shows it, `typed = now`, `unsent = since = Some(now)`. With one: `typed = now`, `unsent`
and `since` set where None. Then `note_steps`. (A write of the mirror's into an editor with a `NoteEdit` also emits
`Change` after it; at worst it delays a send by `PAUSE`, never sends wrong text: sends read the editor.)

**Clean text only:** `note_of`'s filter becomes `clean_note_text(&str) -> String` (line breaks and tabs kept, every
other control character dropped), used by `note_of` and by step 0 below, so the mirror's base, its sends and home's
copy are alike and a pasted carriage return can't make every later copy look like home's change.

**`note_steps(space, now, window, cx)`** runs for every `NoteEdit` of that remote, from: a `Change`, a blur, every
`apply_remote` of that remote (after `remote_notes`), every answer of its calls, and its wake. For one note, with
`mine` the editor's value and `copy` home's latest (the copy), in order:

0. When `clean_note_text(mine) != mine`, `note::write(editor, cleaned)` and take `mine` as the cleaned text.
1. `copying` → wait. `stuck` → wait (cleared by the next `apply_remote`).
2. **Gone:** the copy doesn't have it, no block of home's state shows it, or it is in `notes_cut`: when `mine ==
   base` (and nothing in flight), drop the edit; else **keep as a copy** (below), reason "gone" or "cut".
3. `sent.is_some()` → wait for the answer.
4. **Landed** (`rev` None): when `copy.rev > after`, home's text holds the send: set `rev = Some(copy.rev)` and go on
   as "home moved" with `base` the sent text (step 5 compares `copy.text` with it). Else wait.
5. **Home moved** (`copy.rev != rev` or `copy.text != base`): `merge(base, mine, copy.text)`:
   Some(r) → `note::write(editor, r)`, `base = copy.text`, `rev = Some(copy.rev)`, `refused = None`, `mine = r`;
   None → **keep as a copy**, reason "conflict"; stop.
6. **Clean** (`mine == base`): drop the edit (and fire a `closing` waiting on it, §6.5); stop.
7. Not live (`why_not(Deed::EditNote)` is `NotConnected`) → wait (nothing sent, nothing dropped).
8. `mine.len() > NOTE_TEXT_MOST` → tell once ("note too long") and wait.
9. A refusal stands while home's state is no newer: wait until `refused.at + HOLD`; then send again, and after `TRIES`
   refusals in a row tell its sentence once and wait for the next state or key (which reset the count).
10. Not due (`!flush && now < min(typed + PAUSE, unsent + MOST_WAIT)`) → wait.
11. **Send** `edit_note {block: the block whose tabs show the note in home's state, note, rev, text: mine}`; `sent =
    Some((mine, unsent))`, `unsent = None`, `flush = false`. Its `Then` (an edit no longer there: nothing): `Ok` →
    `after = rev.unwrap()`, `rev = None`, `base = sent text`, `sent = None`, `refused = None`, `since = unsent` (the
    first key typed after the send, or None: home has the rest). `Err` → `sent = None`, `refused = (now, count + 1)`,
    `unsent = min(unsent, the sent one)` (that text is unsent again). Both run `note_steps`.

After the loop, schedule `note_wake` (`cx.spawn_in(window, …)` with `cx.background_executor().timer`, replacing the
last) at the earliest due time over the edits: `typed + PAUSE`, `unsent + MOST_WAIT`, `since + HOLD` (to draw the
" •"), `refused.at + HOLD`; none when nothing is due. It calls `note_steps(space, Instant::now(), …)`. Tests call `note_steps` with a `now`
they choose (no sleeps).

**Keep as a copy** (reason conflict, gone or cut): `copying = true`, the editor turns read-only (§6.3). The block is
the edit's `block` when home's state still shows it as a note block: `new_note {block, name: "{name} ({share})",
text: mine}`; else `add_block {workspace, kind: "note", text: mine, x, y}` at `rect`'s top-left (home names it by its
first line: *the planner's call*, `add_block` has no `name`). `{name}` is the copy's name of the note (or the last one
known), `{share}` `me.name`. `Ok` → drop the edit; for a conflict and a cut note write the copy's text into the
editor (`note::write`); tell the spec's sentence for the reason. `Err` → `copying = false`, `stuck = true`, its
sentence told once. The text is in `NoteEdit::editor` throughout: nothing drops it but `Ok`, Delete and Forget.

**The " •":** `note.rs` `body` appends " •" to a remote tab's title when `ws.note_unsaved(note)`: an edit exists and
(`since` is at least `HOLD` old, or `copying`, or `stuck`).

`{share}` is `me.name`, or "another computer" before `GET /v1/me` answered.

Forgetting the remote drops its edits (deliberate). `delete_note` from the dialog drops that note's edit.

### 6.5 Tabs and menus: one door (`remote_notes.rs`)

```rust
pub(crate) enum NoteAct { New, Close { note: u64 }, Rename { note: u64, name: String },
                          Color { note: u64, color: Option<Label> }, Open { note: u64 }, Delete { note: u64 } }
pub(super) fn remote_note_act(&mut self, space: &str, block: Option<BlockId>, act: NoteAct, window, cx);
```

Like `remote_checklist_act`: ask the deed first (`New`, `Close`, `Rename`, `Color` → `EditNote`; `Open` → `OpenNote`;
`Delete` → `DeleteNote`), a refusal says its words and sends nothing; lay the hold; call; on `Err` end the hold,
`remirror`, warn the sentence; `mark_remote()` when the space is shown. A rename to the same name, or a color already
set, sends nothing; a blank rename sends "" (home names it Untitled).

| Act | Called from (each branches to the door on a remote block, as `new_checklist` does) | Home | Hold |
|---|---|---|---|
| `New` | the tabs' + (`new_note_tab`), the empty block's New note, the dialog's New note with a block | `new_note {block}` | none: `tab_awaited` |
| `Close` | the tabs' × (`close_note_tab`) | `close_note {block, note}` | `tabs_closed` |
| `Rename` | the tab's double click (`rename_note_tab` / `stop_note_rename`), the menu's Rename… | `edit_note {block, note, name}` | `notes_renamed` |
| `Color` | Color ▸ (`set_note_color`) | `edit_note {block, note, color}` | `notes_colored` |
| `Open` | the dialog's row of a closed note | `open_note {note, block}`, or without a block `{note, workspace, x, y}` (where a new note goes in the view, `room_in_view`) | none: `tab_awaited`, or `awaiting` for a new block |
| `Delete` | the dialog's trash (second click, as locally) | `delete_note {note}` | none (gone a round trip later) |

- **Holds** (`RemoteSpace`): `notes_renamed: Holds<u64, String>`, `notes_colored: Holds<u64, Option<Label>>`,
  `tabs_closed: Holds<u64, bool>`, laid in `mirror_notes` (names, colors) and `lay_note_tabs` (a held-closed note gets
  no tab), ended by home's state showing it (`Holds::shown`), a refusal, or `expire_holds` (add the three there).
- **Close with unsaved text:** when the note has a `NoteEdit`, `Close` doesn't call: it sets `flush` and pushes
  `Closing::Tab { block, note }` to `RemoteSpace::closing`; closing a note block (the remote close path) whose tabs
  have edits pushes `Closing::Block(block)` instead of closing. `note_steps` fires a closing once none of its notes has
  an edit. Until then the tab shows its " •".
- **`tab_awaited: Option<(BlockId, Vec<u64>, Option<BlockId>, Instant)>`** (the block, its tabs when asked, what had
  the keyboard, when): set on `New`'s and `Open`'s `Ok`. In `remote_notes`, after `lay_note_tabs`, the first id in that
  block's tabs that wasn't there is shown (`active`) and its editor focused when the keyboard is still where it was and
  less than 5 s passed (as `focus_awaited`); then it ends.
- **Captured workspace:** the rename field's events, the Rename… prompt and the Notes dialog's closures carry the
  workspace they were opened on, as card 205's fields do. Move `checklist.rs`'s `still_on` to `remote.rs` beside
  `still_shown` (as `Workspace::still_on`) and use it in both dialogs and the Rename… prompt: with a mirror involved
  (the dialog's or the one shown), they act only while their workspace is shown. The local `rename_note`'s and
  `new_note_tab`'s `on_remote()` guards become this.

### 6.6 All notes… on a mirror (`note.rs`)

`open_notes(space: String, into: Option<BlockId>, window, cx)` (the workspace, as `open_checklists` takes it; every
caller passes `ws.space.id.clone()`). Rows: from `ws.notes_in(space)` (new, as `checklists_in`: the copy on a mirror,
`ws.notes` else); on a mirror each row's detail is "{ago} · Open" for a note a block shows, "{ago} · Closed" else, then
its next line as locally (`ago` from home's `changed`; a skewed clock shows as such). A click: open → reveal its block
and show its tab (the mirror's own view, no call); closed → `NoteAct::Open`. Trash → `NoteAct::Delete` (Full; on a
Control share the click says "Needs a Full share: share again on winx-desktop."). New note → `NoteAct::New` with a
block, else `remote_add(Kind::Note, …)`.

`note.rs` `empty`: on a remote block, its two buttons call the same acts, dimmed with the rule's words when
`why_not(EditNote)` / `why_not(OpenNote)` (as `checklist.rs`'s `empty`). `remote_empty` goes.

### 6.7 What stays

Send to ▸, Copy all, Find, the shown tab being the mirror's own choice, a note in one tab at most, the copy's cleaning
and caps, `older_content` for a home before batch 2e. Nothing of home's ever enters `ws.notes` or `notes.json` (the
door and the edits act on `RemoteSpace` and home's tools only).

## 7. The mirror's tests

Headless (`workspace/tests/remote_content.rs` or a new `tests/remote_notes.rs` registered beside it; `live_content`,
`content_state`, `ws.calls` to answer calls, `after_hold()`), each failing without its code:

1. `typing_in_a_remote_note_goes_home_after_a_pause_with_home_s_rev`: type "x" into n1's note 9; `note_steps` before
   `PAUSE` calls nothing; at `PAUSE` one `edit_note {block: "n1", note: 9, rev, text}`; more keys while it is in
   flight call nothing; its `Ok` and a state with `rev + 1` and the sent text leave the note clean (no edit); the keys
   typed meanwhile go in the next send with that `rev`.
2. `keys_held_down_still_go_every_two_seconds`: a key every 400 ms → a send by `MOST_WAIT`.
3. `home_s_change_to_a_clean_note_keeps_the_caret`: caret after "one"; home's state inserts "zero " before it → the
   editor shows home's text and the caret moved by 5, and no `NoteEdit` is made nor anything sent at any later step
   (the write's own `Change`) (replaces `home_s_text_follows_into_a_remote_note_and_keeps_the_selection`).
4. `home_s_change_while_typing_waits_for_the_pause_and_merges`: dirty at the start; a state appends at the end → the
   editor unchanged until `note_steps` at `PAUSE`; then it holds both and the call carries the new `rev`.
5. `a_change_of_both_in_one_place_keeps_both_texts`: dirty in "second"; home's state changes "second" too → no
   `edit_note`; `new_note {block: "n1", name: "Plan (…share…)", text: mine}`; the editor is read-only and a key says
   "Keeping your version…"; on `Ok` the editor shows home's text and the conflict's sentence is said.
6. `a_refused_edit_is_sent_again_on_home_s_newer_text`: `Err` then a state with `rev + 1` → merged and sent with it,
   no copy; `Err` three times with no newer state → its sentence once, nothing more sent.
7. `a_state_older_than_the_answer_doesn_t_undo_the_person_s_text`: `Ok`, then a state still at the old `rev` with the
   old text → the editor keeps the person's text, nothing sent; then `rev + 1` → clean.
8. `a_note_closed_at_home_while_typing_becomes_a_new_note`: dirty; a state without note 9 in n1's tabs →
   `new_note {block: "n1", …, text: mine}`; with n1 gone too → `add_block {kind: "note", text: mine, x, y}` at its
   rect.
9. `a_cut_note_is_read_only_and_nothing_past_256_kib_is_sent`: `cut` → read-only, a key says the sentence once, no
   call; a clean note made longer than 256 KiB on the mirror → the "past 256 KiB" sentence, no call.
10. `typing_while_not_connected_sends_nothing_and_the_rest_goes_when_live`: dirty, then Reconnecting → no call, a key
    says "Not connected: what you type isn't sent." once; live again with a snapshot at the same `rev` → sent.
11. `the_tabs_rename_and_color_call_home_and_show_at_once` (replaces `a_remote_note_s_tabs_and_menu_dim_what_edits`):
    + → `new_note {block}`; × → `close_note` and the tab hides at once, back after `HOLD` or a refusal; a double click,
    typing and Enter → `edit_note {name}` shown at once; Color ▸ Blue → `edit_note {color: "blue"}` tinted at once;
    Ctrl+Z on the canvas then stops at the mark; Copy all and Send to as before.
12. `a_tab_with_unsaved_text_closes_once_home_has_it`: × on a dirty tab → `edit_note` (flushed), no `close_note` until
    its `Ok` and the state showing it; then `close_note`. The same for closing the block.
13. `the_new_tab_home_adds_is_shown_with_the_keyboard`: `new_note`'s `Ok`, then a state with note 12 in n1's tabs →
    note 12 shown and its editor focused; not when the person focused another block meanwhile.
14. `all_notes_on_a_mirror_lists_the_workspace_s_notes_and_reopens_a_closed_one` (replaces
    `a_remote_note_s_menu_opens_no_dialog_and_renames_nothing_of_the_mirror_s_own`): a state with closed note 5
    (`workspace: "w1"`) → the dialog's rows "Open" for 7 and 9, "Closed" for 5; a click on 5 → `open_note {note: 5,
    block: "n1"}`; on 7 → no call, its tab shown; the trash on Control → "Needs a Full share…", on Full →
    `delete_note {note: 5}`; New note → `new_note`; the canvas menu's Notes… → `open_note {note, workspace, x, y}`.
15. `a_local_notes_dialog_does_nothing_once_a_mirror_is_shown_and_back` (keeps and extends
    `new_note_from_a_local_notes_dialog_adds_nothing_once_a_mirror_is_shown`): a local dialog's row, New and trash with a
    mirror shown do nothing ("Another workspace is shown now…"), and a mirror's dialog with the local workspace shown
    calls nothing and changes `ws.notes` nowhere; a Rename… prompt opened on a mirror's note, the local workspace
    then shown, renames nothing.
16. `an_older_home_dims_every_note_edit`: tools with `add_block` and `edit_item` but no `edit_note` → a key says
    "…'s Templum is older…" once, Add ▸ Note, +, ×, Rename…, Color ▸, All notes… dimmed with it; the note still live.
17. `add_note_on_a_mirror_adds_a_note_block_at_home`: the toolbar's Note → `add_block {kind: "note", x, y}`; Add ▸
    Note at a click → at that point; its block takes the keyboard when home's state brings it.
18. Extend `acts_on_a_mirror_leave_its_notes_json_and_checklists_json_as_they_were` with typing, a rename, a new tab and
    a reopen: `notes.json` byte for byte and `ws.notes` unchanged (the mirror's own note 7 untouched).
19. `remote.rs`'s: the `Deed` list (with `DeleteNote`), the words of `AddNote` for an older home.

End to end (`workspace/tests/mirror_end_to_end.rs`, real loopback, one headless `Workspace` both ends, a Control
share unless said; `pump_home` with a deadline, `note_steps` given a `now` past the due time):

20. `a_note_typed_on_the_mirror_is_home_s_and_an_agent_s_append_merges`: type at the top of the mirror's note; send;
    home's note has it (and `notes.json` at home); then type again on the mirror, `append_note` at home before the
    mirror's step, pump until the mirror's copy shows the append, step → home's note holds both.
21. `a_note_changed_in_one_place_at_both_ends_keeps_both_texts`: dirty on the mirror; `note_replace` at home over the
    same words; pump until the copy shows it; step → home's block has a new note "{name} ({share})" with the mirror's
    text, the original has home's, and the mirror's editor shows home's.
22. `a_note_made_renamed_colored_closed_and_reopened_on_the_mirror_is_home_s`: Add ▸ Note at a point → home's block
    there; + in it; rename and color; × closes it at home (`closed_in` the workspace); All notes… lists it Closed and
    reopens it at home; on this Control share the trash says "Needs a Full share…" and deletes nothing; the
    mirror's `notes.json` is unchanged.

## 8. Docs

### 8.1 `docs/api.md`

- "with the eleven only the API has" → sixteen; rows:

  | `edit_note` | `block`, `note` (a note that block shows as a tab), then any of `name`, `color` (`red`, `yellow`, `green`, `cyan`, `blue`, `magenta` or `none`), `text` with `rev` | control |
  | `new_note` | `block`, then `name`, `text` | control |
  | `open_note` | `note`, then `block`, or `workspace` with `x`, `y` (both or neither) | control |
  | `close_note` | `block`, `note` | control |
  | `delete_note` | `note` | full |
- Scopes table, full: "…, deleting a note (`delete_note`), …".
- "Notes, checklists and ink." gains: "`notes` also holds each note no note block shows that was last open in a
  workspace (closed since Templum 0.x, the version this ships in), its `text` empty and `workspace` that workspace's
  id; a workspace-limited token sees those of its workspaces. Every note has `rev`, its text's revision (a number that
  grows each time its text changes and at each `edit_note` of its text, from the Unix time in milliseconds when Templum
  started, so a restarted Templum never reuses one), and `changed` (Unix seconds). `edit_note` renames a note (`name`, one line; blank is Untitled), tints it
  (`color`), or replaces its whole text (`text`, at most 256 KiB) when its `rev` is still `rev`, the one the state
  showed with the text the change was made on; else it is refused ("Note 7 changed since: its newer text and rev are in
  the state."), and so is a text for a note longer than 256 KiB, which the state shows cut: no change is written over
  a text the caller didn't see. The text goes in as one step of the note's Ctrl+Z, your caret kept. `new_note` adds a
  new note as a shown tab of a note block (`text` at most 256 KiB). `open_note` shows a note's tab where it is open,
  else opens it as a tab of `block`, or in a new note block of `workspace`. `close_note` closes a tab as its × does (a
  blank note goes). `delete_note` deletes a note for good, from every block, as the Notes dialog's trash does. A
  workspace-limited token reaches only the notes its blocks show and those last closed in its workspaces."
- Templum to Templum, "On a mirrored workspace": "editing notes (… read-only …)" and the stale "renaming a checklist
  and moving its items" go; add: "Notes are edited there as here: what you type goes to home after a pause, checked
  against home's text; a change home made in the same place keeps both texts, yours as a new note beside it."
- Section 7 "What a mirror can't do yet": drop item 1 and the stale item 2 (card 205 did checklists), renumber.

### 8.2 `docs/specs/api.md`

The API's own tools table gains the five rows (Arguments, Does, Scope, as §4.2 and §8.1; "kanban card 204"); the
`notes` bullet gains `rev`, `changed`, closed notes with `workspace`, and the limited token's rule.

### 8.3 `docs/specs/templum-to-templum.md`

The design is in "Notes on a mirror (kanban card 204)" already. As it lands: turn that section's italic first line into
"Built: plan …" and add what the builder chose or changed (as card 205's section); "Status" says built; "The canvas"
Adding bullet: Note adds a note block at home; "What works on a remote workspace" table gains "Types in, renames,
recolors a note; makes, closes, reopens one" → `edit_note`, `new_note`, `close_note`, `open_note`, Control, and
"Deletes a note" → `delete_note`, Full, and "Adds a note" joins the add row; the "Notes" bullets become the new
behaviour (keeping Send to, Copy all, Find); "One rule"'s ⓘ list drops "editing notes"; "What home's API gains" gets a
"Tools of kanban card 204" paragraph pointing at the section; "Later batches" marks editing notes done.

### 8.4 Others

- `docs/specs/cockpit.md` §5's "On a mirrored workspace" paragraph: editable, with the conflict rule in one sentence
  and a link to the section.
- `docs/ARCHITECTURE.md` (the `remote_content.rs` paragraph): the note editors are editable; `workspace/remote_notes.rs`
  keeps the edits being sent (`NoteEdit`, `note_steps`, the merge in `notes.rs`) and the door `remote_note_act`.

## 9. Minors (`docs/minors.md`)

- **Delete in the fixing commit,** if `note::write` keeps the scroll (verify by hand, §11): "A remote note's scroll may
  move when home's text changes." Otherwise keep it, its fix naming `note::write`.
- **Edit:** "Home's state copies every note's text…": its fix now keys the cached `Arc<NoteView>` by `Entry.rev`.
  "A remote note, a remote checklist and home's ink were checked headlessly only": the editable note, the " •", the
  conflict's notification, All notes…'s Closed rows join what to look at.
- **Add,** under Templum to Templum, each with where, what a person sees and a fix:
  1. Text of a mirror's note not at home yet is lost when Templum quits or the remote is forgotten
     (`remote_notes.rs`, `NoteEdit`): fix: list such notes in `may_close`'s question and in Forget…'s.
  2. Notes closed at home before card 204 can't be reopened from a mirror (`closed_in` is None): fix: set `closed_in`
     at load for notes a saved canvas names (the workspace of its file), or let the person pick a workspace in home's
     Notes dialog.
  3. Several carets collapse to one when home's text comes into a mirror's note (`note::write`): fix: shift every
     selection.
  4. An IME composition may be cut short when home's change is merged in while composing (gpui-base hides its marked
     range): fix: defer `note::write` while the editor has marked text, once gpui-base exposes it.
  5. Each send carries the whole note (up to 256 KiB every 0.5–2 s while typing): fix: send the splice with its
     `rev` when home can apply one.
  Add others the review defers, as always.

## 10. Order of steps

Each step ends with `cargo test -p <crate>` passing for what it touched; the full check at the end.

1. `notes.rs`: `rev`, `closed_in`, `stamp`, `close_in`, `forget_workspace`, `splice`, `merge`, `shift` with their
   tests (§3, §5.5 item 5). `note::write` (§3.3).
2. `templum-api`: `NoteView`'s fields and `State::visible` (§4.1); the registry's five tools (§4.2), with tests.
3. Home's glue (§5): the state's notes, `note_closed`, `forget_workspace` at delete, the seed at load, `api_note`,
   `note_seen`, the five bodies, `NewBlock::Note`'s `open`; tests §5.5 1–4. Docs §8.1, §8.2.
4. The mirror's rule (§6.1) and copy (§6.2): deeds, `Nay::NotesReadOnly` gone, `remote_add_in` for notes,
   `notes_cut`, holds laid in `mirror_notes`.
5. The editors and `NoteEdit` (§6.3, §6.4): new `workspace/remote_notes.rs`; tests §7 1–10.
6. The door, holds, closing, `tab_awaited`, the dialog, the empty block, `still_on` moved (§6.5, §6.6); tests §7
   11–19.
7. End to end §7 20–22.
8. Docs §8.3, §8.4; minors §9.
9. `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`;
   `cargo run --release -p templum -- --bench-mirror` (its JSON line for the pull request: the mirror's notes now
   paint a tab dot and run an editable editor).

## 11. Done when

- Every test above passes and fails without its code (say how you checked the riskiest ones: 5, 7, 12, 20, 21).
- The full check command passes; its result is reported as it is, failures included.
- By hand, two `--dev` Templums (or one sharing to itself): type in a mirrored note and see it at home; an agent's
  `append_note` at home comes in without moving the mirror's caret or scroll; a conflict makes the second note; × on
  a note just typed in closes after it is saved; the " •" shows while not connected. Say what was looked at.
- `--bench-mirror` `p95_ms` ≤ 16.7, its line pasted.

## 12. Risks, for the implementer and the reviewer

- **The editor writes** are the riskiest part: `replace` and `set_selected_range` move gpui-base's caret and scroll,
  its `Change` comes after the update, and an IME composition is invisible. Every write of home's text into an editor
  goes through `note::write`; a write into an editor with a `NoteEdit` happens only in `note_steps` step 5, at a pause.
- **The answer racing the stream:** the `rev: None` / `after` rule (§6.4 step 4) is what keeps a state that left home
  before the edit from undoing it; test 7 must fail if step 4 is replaced by "take the copy".
- **Home's own writes and `rev`:** `edit_note` must bump `rev` inside its own update (§5.4), and `append_note` /
  `replace_note` must move it before the next call is handled (test §5.5 1).
- **Ids are alike on both ends:** every act that names a note carries its workspace (the door's `space`, the dialog's
  and prompts' `still_on`); test 15 covers a local dialog over a mirror and the reverse.
- **Text never lost:** only `Ok` of a copy, Delete and Forget drop a `NoteEdit`; a reviewer should look for any other
  path that drops `RemoteSpace::edits` or `note_editors` entries an edit still needs (the edit holds its editor).
