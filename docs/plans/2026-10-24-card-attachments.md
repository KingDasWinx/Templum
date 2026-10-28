# Card Attachments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A kanban card holds files: attached in the open card (Attach, a drop, a pasted screenshot), copied into
Templum's data folder, shown as thumbnails and rows, counted on its face, and listed with their paths for agents.

**Architecture:** One more card field, `Card::attachments: Vec<Attachment>` (crates/templum/src/board.rs), merged at
Done like the checklist (`Draft::merge_into`). The files live in `<data>/attachments/<card id>/`, and every file
operation is a pure function of a new `crates/templum/src/attachments.rs` (copy in, write a pasted image, a free name,
sizes, the sweep). The dialog (crates/templum/src/kanban.rs `open_draft`) gets an Attachments section; its three ways
in end in one function, `attach`, that copies on the background executor and puts each result in the work. Nothing is
deleted until a start sweeps what no card or draft names (workspace/spaces.rs `load_spaces`). Agents read them through
the shared `cards_of` (workspace/ops.rs).

**Tech Stack:** Rust, GPUI (gpui-fast 0.1.8) + gpui-kit 0.7.3 (pinned): `prompt_for_paths`, `ExternalPaths` drops,
`ClipboardEntry::Image`, `img`, `open_with_system`. No new dependency.

**Spec:** [docs/specs/kanban.md, Attachments (kanban card 152)](../specs/kanban.md#attachments-kanban-card-152) (read
it first).

## Global Constraints

- 50 MB a file (`attachments::MAX = 50 * 1024 * 1024`); thumbnails up to 10 MB (`THUMB_MAX`); sizes shown with
  1 KB = 1024 bytes. Constants, no setting.
- Files at `<data>/attachments/<card id>/<name>`, `name` a plain file name (one component, not `.` or `..`, no `/`,
  `\` or NUL). No path is ever built from a name that isn't one.
- Nothing is deleted when an attachment leaves a card: only the start's sweep deletes, and only after `boards.json`
  loaded as `Loaded::Saved`.
- `boards.json` keeps `"version": 1`; the new key is `default` + `skip_serializing_if`; older files load unchanged.
- No copy or write on the UI thread but the sweep at start: painting never waits on a copy.
- Every logic change ships a test that fails without it: board.rs and attachments.rs unit tests (`tempfile`), headless
  GPUI tests in crates/templum/src/workspace/tests/attachments.rs (new, registered in tests/mod.rs, opened with a data
  folder: `open_in(cx, None, Some(dir))`). The test platform's `open_with_system` is `unimplemented!()`: no test
  clicks an attachment. Tests build paths with `Path::join` and compare `Path`s, never strings holding `/` (CI runs
  Windows). UI looks: say how verified. Minors to docs/minors.md; docs ≤ 120 columns.
- Commits on `development`, Conventional Commits. Full check before each commit:
  `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`. Task 1 paints
  the kanban: run `cargo run --release -p templum -- --bench-mixed` once and keep its JSON line for the pull request.

## Review Focus

1. A hand-edited `boards.json` whose attachment is named `"../../.ssh/id_rsa"` or `"a/b"`: the entry is dropped at
   read and the rest of the card loads. → Task 1 test `an_attachment_name_that_leaves_its_folder_is_dropped`.
2. A copy that ends after the dialog closed lands where the work went: Done → onto the card (a Ctrl+Z step of its
   own), Save draft → into the draft, Discard → nowhere. → Task 1 test `a_copy_that_ends_late_follows_the_work`.
3. The sweep keeps everything when `boards.json` was set aside (unreadable) or is missing, and keeps a new card's
   draft's files (its reserved id is no card on the board). → Task 2 test `the_sweep_runs_only_on_boards_read_cleanly`.
4. Ctrl+Z of Delete card brings the card back with its attachment's file still on disk; only a later start deletes
   it. → Task 2 test `ctrl_z_of_a_deleted_card_brings_its_file_back`.
5. A clipboard with text and an image pastes the text into the field and attaches nothing; one with only an image
   attaches it and leaves the field as it was. → Task 3 test `a_paste_with_text_pastes_the_text`.

---

### Task 1: Attach and remove files in the open card

**Files:**
- Create: `crates/templum/src/attachments.rs` (`mod attachments;` in main.rs)
- Modify: `crates/templum/src/board.rs` (`Card` ~61, `Card::new`, `Draft::merge_into` ~257, `Boards::finish_draft`
  ~505), `crates/templum/src/kanban.rs` (`meta_row` ~628: the paperclip; `Work` ~839; `finish_card` ~920;
  `open_draft` ~1403: the section, Attach, `submit` setting `done`), docs/specs/kanban.md (the open card's layout
  lists the section)
- Test: board.rs and attachments.rs `mod tests`; `crates/templum/src/workspace/tests/attachments.rs` (new)

**Interfaces:**
- board.rs: `pub(crate) struct Attachment { pub name: String, pub size: u64 }`, whose deserializing fails when `name`
  isn't `attachments::plain_name`, so `known` drops the entry; `Card::attachments: Vec<Attachment>` (`default`,
  `deserialize_with = "known"`, `skip_serializing_if = "Vec::is_empty"`). `merge_into` adds the work's new ones the
  card lacks and removes the ones the work dropped from the base, by name. `finish_draft` gives a new card
  `draft.work.id` when it isn't 0, else `next()` as today.
- attachments.rs:
  - `pub(crate) const MAX: u64`, `pub(crate) const THUMB_MAX: u64`
  - `pub(crate) fn plain_name(name: &str) -> bool`
  - `pub(crate) fn dir(data: &Path, card: u64) -> PathBuf` (`<data>/attachments/<card>`)
  - `pub(crate) fn copy_in(data: &Path, card: u64, from: &Path) -> Result<Attachment, String>`: refuses a folder, a
    file over `MAX` or one it can't read, saying why in a few words ("over 50 MB", "a folder", "couldn't read it:
    …"); takes a free name with `create_new` (" (2)" before the extension); removes the file when the copy fails.
  - `pub(crate) fn write_in(data: &Path, card: u64, name: &str, bytes: &[u8]) -> Result<Attachment, String>`
  - `pub(crate) fn size_text(bytes: u64) -> String`; `pub(crate) fn thumbnail(a: &Attachment) -> bool` (an image kind
    in any case, `size <= THUMB_MAX`)
- kanban.rs:
  - `Work` gains `copying: Vec<String>` (the names shown "Copying…"; `changed()` is true while it isn't empty) and
    `done: bool` (set by `submit`).
  - `fn card_id(ws: &mut Workspace, open: &Open) -> u64`: the card's id; a new card's is taken from `next_id` the
    first time and set in `now`'s and `opened`'s `base` and `work` alike.
  - `pub(crate) enum Source { File(PathBuf), Image(gpui_kit::Image) }` and `pub(crate) fn attach(ws: &mut Workspace,
    open: &Open, board: u64, from: Vec<Source>, window: &mut Window, cx: &mut Context<Workspace>)`: does nothing
    unless `ws.card_work` is `open` and `ws.data_dir` is Some; spawns the copies on the background executor; each
    result goes into `open` while `ws.card_work` is it (`Rc::ptr_eq`), else into the kept draft whose `work.id` is
    the card's, else, when `open.done` and the card exists, onto it through `edit_boards`, else nowhere. Refusals:
    one warning notification per call, "Not attached: video.mov (over 50 MB), build (a folder)."
  - Ids: `attach-files`, `card-attachments`, `attachment-{name}`, `attachment-remove-{name}`,
    `attachment-copying-{name}`; on the face `name("attachments", n, id)`.
  - With `ws.data_dir` None: no section.

- [ ] **Step 1: Failing tests:**
  - board.rs: `attachments_read_back_and_a_card_without_any_writes_no_key`;
    `an_attachment_name_that_leaves_its_folder_is_dropped` (Review Focus 1: `"../../.ssh/id_rsa"`, `"a/b"`, `"a\\b"`,
    `""`, `".."` dropped, `"ok.png"` kept, the card's title still read);
    `done_adds_and_removes_the_persons_attachments_and_keeps_the_rest`; `a_new_card_keeps_its_reserved_id_at_done`.
  - attachments.rs: `a_copy_keeps_the_name_and_takes_a_free_one_when_taken` (`x.png` twice → `x.png`, `x (2).png`;
    `Makefile` twice → `Makefile (2)`; each file's bytes are the source's);
    `a_file_over_50_mb_and_a_folder_are_refused_and_leave_nothing` (a sparse file, `File::set_len(MAX + 1)`);
    `a_pasted_image_is_written`; `sizes_read_in_bytes_kb_and_mb` (12 → "12 B", 48213 → "47 KB", 1258291 →
    "1.2 MB").
  - tests/attachments.rs: `attach_shows_a_thumbnail_and_a_row_and_done_puts_them_on_the_card` (open a card, click
    `attach-files`, `cx.simulate_path_prompt_response` with a temp `shot.png` and `notes.txt`, `run_until_parked` →
    `attachment-shot.png` and `attachment-notes.txt` drawn, the board's card has none; Done → it has both, their files
    are in `attachments::dir`, the face's `attachments-…` reads 2; one Ctrl+Z → none on the card);
    `removing_an_attachment_changes_the_board_only_at_done`; `a_new_cards_files_go_in_its_reserved_folder`;
    `a_refused_file_notifies_and_attaches_nothing` (`notification_count` + 1); `a_copy_that_ends_late_follows_the_work`
    (Review Focus 2: `attach`, then Done, Save draft or Discard before `run_until_parked`, in three runs);
    `without_a_data_folder_there_is_no_attachments_section` (`open(cx)`: no `attach-files`).
- [ ] **Step 2: Run** `cargo test -p templum attachment` → FAIL.
- [ ] **Step 3: Implement** the interfaces; the section as the spec draws it (thumbnails with `img(path)`,
  `ObjectFit::Cover` and `with_fallback`; rows; ✕ on hover; a click calls `cx.open_with_system`); the face's paperclip.
  Check by hand in `cargo run -p templum -- --dev`: light and dark themes, a long name, ten images, a click on an
  image, a PDF and a text file, the face at zoom 0.5 and 2.
- [ ] **Step 4: Run** → PASS; full check; `--bench-mixed` once.
- [ ] **Step 5: Commit** `feat(kanban): attach files to a card, shown as thumbnails and rows in the open card`.

### Task 2: Files no card holds are deleted at start

**Files:**
- Modify: `crates/templum/src/attachments.rs` (`sweep`), `crates/templum/src/workspace/spaces.rs` (`load_spaces` ~41,
  its `Loaded::Saved` arm), docs/specs/kanban.md (Files, if the build changed a detail)
- Test: attachments.rs `mod tests`; tests/attachments.rs

**Interfaces:**
- `pub(crate) fn sweep(data: &Path, boards: &Boards) -> io::Result<usize>`: in `<data>/attachments/`, removes every
  entry whose name is no card id named by a card or by a draft's `base` or `work`, and every entry of a named folder
  that its card (on the board, in a draft) doesn't list; an entry that is a link (`symlink_metadata`) is removed as a
  link, never followed. Says how many it removed; a missing folder is 0.
- Called once, on the UI thread, in `load_spaces` right after `Loaded::Saved`; never on `Moved`, `Unreadable` or
  `Nothing`. An error is printed (`eprintln!`), not shown.

- [ ] **Step 1: Failing tests:**
  - attachments.rs: `the_sweep_removes_what_no_card_or_draft_names` (a gone card's folder, a file its card no longer
    lists and a copy cut short are removed; a listed file and a new card draft's reserved folder stay; it returns 3).
  - tests/attachments.rs: `a_start_deletes_a_deleted_cards_files` (a card with a file, Delete card, the canvas saved,
    a new workspace on the same data folder → the card's folder is gone);
    `the_sweep_runs_only_on_boards_read_cleanly` (Review Focus 3: `boards.json` written as garbage → set aside, the
    files stay; `boards.json` removed → the files stay; a kept new-card draft's files stay);
    `ctrl_z_of_a_deleted_card_brings_its_file_back` (Review Focus 4: Delete card, Ctrl+Z → the card lists the
    attachment and its file exists).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `feat(kanban): files no card holds any more are deleted when Templum starts`.

### Task 3: Drop files on the open card, or paste a screenshot

**Files:**
- Modify: `crates/templum/src/kanban.rs` (`open_draft`: the body's `drag_over::<ExternalPaths>`,
  `on_drop::<ExternalPaths>`, and `capture_action` of gpui-kit's input `Paste`), docs/specs/kanban.md
- Test: kanban.rs `mod tests`; tests/attachments.rs

**Interfaces:**
- `fn pasted_image(item: &ClipboardItem) -> Option<Image>`: the item's image when it has one and no text that isn't
  blank. Pure.
- The body's capture of `Paste`: with `pasted_image` Some, `attach` with `Source::Image` (written as
  `Pasted image.<format.extension()>`) and `cx.stop_propagation()`; else nothing, and the field pastes.
- The drop: `attach` with a `Source::File` per path; a 2-wide accent ring on the body while files are over it.

- [ ] **Step 1: Failing tests:**
  - kanban.rs: `only_an_image_with_no_text_is_a_pasted_image` (an image alone → Some; an image and "x" → None; an
    image and "" → Some; text alone → None).
  - tests/attachments.rs: `pasting_a_screenshot_attaches_it_and_leaves_the_title` (`cx.write_to_clipboard` a PNG
    `Image`, press `PASTE` with the title focused, `run_until_parked` → `attachment-Pasted image.png` drawn, the
    title as it was); `a_paste_with_text_pastes_the_text` (Review Focus 5);
    `dropping_files_on_the_open_card_attaches_them` (`simulate_event` with `FileDropEvent::Entered` at the dialog's
    center, then `Submit`; if the harness swallows the drop as it does a card's (docs/minors.md, Kanban, "No headless
    test drops a card"), the test calls the drop's function instead, and a minor says the file drop is checked by
    hand).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**; check by hand on Hyprland: a drop from the file manager, `grim -g "$(slurp)" - | wl-copy`
  then Ctrl+V in the title, and copied text still pasting as text.
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `feat(kanban): drop files on an open card, or paste a screenshot, to attach them`.

### Task 4: Agents read the attachments

**Files:**
- Modify: `crates/templum/src/workspace/ops.rs` (`card_line` ~610, `card_whole` ~643),
  `crates/templum-mcp/src/tools.rs` (`list_cards`' description ~830), `crates/templum/src/board.rs` (`Card::text`
  ~194), `crates/templum/src/menu/mod.rs` (Send to, ~331), docs/specs/connections.md (`list_cards`),
  docs/specs/kanban.md (Status: implemented, with the four commits; Out of scope no longer lists attachments),
  docs/ARCHITECTURE.md (`attachments.rs`, the attachments folder)
- Test: tests/content.rs; board.rs `mod tests`

**Interfaces:**
- `card_line`: `  1 attachment` / `  {n} attachments` after the comments' count.
- `card_whole`: after the checklist, `\nattachments:` and per attachment `\n  {name}  {size_text}  {path}`, the path
  `attachments::dir(data, card).join(name)` written out whole; no part without a data folder.
- `Card::text(&self, data: Option<&Path>) -> String`: as today, then `\n\nAttachments:` and each path on its own line,
  when it has any and `data` is Some.

- [ ] **Step 1: Failing tests:**
  - tests/content.rs: `list_cards_lists_a_cards_attachments_with_their_paths` (a data folder; a card given `shot.png`
    through `attachments::copy_in` and `edit_boards` → the board's line has "1 attachment"; with `card`, a line with
    `shot.png`, its size and a path that exists and is `attachments::dir(..).join("shot.png")`).
  - board.rs: `a_cards_text_ends_with_its_attachments_paths` (and has none without a data folder).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**; the docs above.
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `feat(connections): agents see a card's attachments and the path to read each one`.
