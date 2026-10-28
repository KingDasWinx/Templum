# Templum to Templum, Batch 1d (Acting From the Mirror, the End-to-End Test, `--bench-mirror`) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer does the whole plan, then fresh Opus 5.5 reviewers
> review it in a loop until one passes it. The implementer **commits nothing**: everything stays uncommitted for the
> orchestrator.

**Goal:** Kanban card 171 part 2, batch 1, last of four plans: the person's case, whole. From the mirror they **type**
into home's terminals with a real keyboard, **move and resize** home's blocks (Ctrl+Z included), **answer** home's
agents' requests from the inbox (with the sound and the notification a local one has), and look after the connection
(**Edit…**, **Paste a new code…**, the round trip, the coming expiry). Then the end-to-end test the orchestrator asked
for, `--bench-mirror`, and the docs for people.

**Architecture:** All glue in `crates/templum`, on plan 1c's `RemoteSpace`, with plan 1b's `Command`s. A key on a
focused remote terminal becomes its name (`templum_terminal::key_name`, the inverse of plan 1a's `named_key`) and goes
to the client's per-terminal queue; home encodes it in the terminal's live modes. A drag follows the pointer locally
and sends one `move_block` per moved block on release; the mirror's rectangle stands until home's state shows it, or
2 s. Home's requests become `Wait`s marked remote, so the inbox draws them unchanged; answering one sends
`answer_request` instead of answering locally.

**Tech Stack:** Rust 2024; GPUI through gpui-kit 0.7.3 (pinned); `templum-api`'s client (plan 1b); `templum-mcp`'s
`Ask` (the inbox's questions).

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), whole; this plan builds "The remote
workspace ▸ Typing, Moving and resizing, Requests, Right-click the tile ▸ Edit…", the banner's round trip and expiry,
"The keyboard" (the mirror's half), "Moving and resizing", "Requests", the end-to-end test of "Constraints ▸ Tests",
`--bench-mirror`, and the docs of "Every feature in the API" and "Specs to update". The code it builds on:
`crates/templum/src/workspace/{input,history,waits,questions,inbox,remote,api}.rs`, `crates/templum/src/{keys,bench,
main,space_dialog,connect_dialog,mirror,chime}.rs`, `crates/templum-terminal/src/input.rs`, and the tests' helpers
in `crates/templum/src/workspace/tests/{mod,api,remote,inbox}.rs`.

**Consumes:** plan 1a (`named_key`, `NamedKey`, `INPUT_KEYS`, `share_workspace`, `revoke_token`); plan 1b
(`Mirror`, `Command::{Keys, Paste, Move, Answer}`, `Update::{Refused, Dropped}`, `About`, `KeySource`, `Keys`); plan
1c (`RemoteSpace`, `Pane::remote`, `apply_remote`, `banner_words`, `on_remote`, `mirror_send`, the Connect dialog,
`Body::Mirrored`).

## Global Constraints

- **MEMORY LIMITS on this machine** (systemd-oomd has killed the desktop app several times): **every** cargo command
  takes `-j 4`; **never two cargo commands at once**; while working, run only the targeted tests each step names; the
  full suite **once**, in Task 7.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-terminal` gains `key_name` UI-free. No
  `unsafe`. **No new dependency;** `cargo deny check` passes unchanged.
- **The UI thread never waits on the network;** typing, moves and answers are `Mirror::send` (a channel push).
  **Painting is not slowed:** nothing here runs per frame but reading `RemoteSpace` fields.
- **Keys are dropped, never queued, while the remote isn't Live** ("Not connected: what you type isn't sent."); never
  sent late after a reconnect.
- **Home is the truth:** the mirror's rectangle for a moved block stands at most 2 s after the release; a refusal puts
  the block back where home has it.
- **Values, verbatim:** keys per call at most 256, 1024 waiting (the client's); a paste at most 256 KiB
  (`tools::MAX_TEXT`); the moved rectangle stands until home shows it or 2 s (`HOLD`); the round trip turns yellow past
  250 ms; the expiry is told 3 days before.
- **Words, verbatim** (the spec's): "Typing needs a Full share: share again on winx-desktop."; "Not connected: what you
  type isn't sent."; "Typing faster than winx-desktop answers: some keys were dropped."; "Paste at most 256 KiB into a
  mirrored terminal."; "Each key waits about 300 ms for winx-desktop's answer."; "· ends in 3 days: share again on
  winx-desktop"; "named on winx-desktop"; "Paste a new code…"; a request's notification titled "API on
  winx-desktop".
- **Every logic change has a test that fails without it;** UI-only changes: say how they were verified. Headless GPUI
  tests; keys in `keyring_core::mock::Store`; home on loopback in the same process; conditions with a 10 s deadline,
  never one fixed sleep; a test that needs a shell to echo is `#[cfg(unix)]`.
- **CI** runs Linux, macOS and Windows with no agent CLIs: `cfg` gating for platform tests and their imports; text of
  fixed width is clipped (`w_full` + `overflow_hidden` + `truncate`).
- **Don't open a Templum window against the person's real data:** `--dev` and headless tests only; the bench uses a
  temporary data folder it removes.
- **Minors** go to `docs/minors.md` under `## Templum to Templum`. Docs lines ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **Esc then j typed quickly in vim through a tunnel:** both reach home in one call, and home still reads Esc alone
   (its 50 ms after an unmodified Esc). → Task 1 `typed_keys_reach_home_in_order`.
2. **A drag, then home moves the same block a second later:** the mirror shows its own rectangle until home's state
   shows it (or 2 s), then whatever home says. → Task 2 `a_moved_block_stands_until_home_shows_it`.
3. **A question answered at home while the mirror shows it:** it leaves the mirror's inbox on the next delete event,
   and nothing is sent. → Task 3 `a_request_answered_elsewhere_leaves`.
4. **AltGr, dead keys and IME on the mirror:** the characters arrive as typed (one-character names), never as
   Ctrl+Alt combinations. → Task 1 `names_are_what_the_keyboard_typed`.
5. **The connection drops mid-drag or mid-answer:** moving and answering are disabled while not Live; a release
   while offline sends nothing and the block goes back to home's rectangle. → Task 2
   `nothing_is_moved_while_not_live`.

---

### Task 1: Typing into home's terminals

**Files:**
- Modify: `crates/templum-terminal/src/input.rs` (`key_name`, `text_names`), `src/lib.rs` (exports)
- Modify: `crates/templum/src/workspace/input.rs` (the key, text and paste paths of a focused remote pane),
  `crates/templum/src/workspace/remote.rs` (`remote_type`, `told`, the `Refused`/`Dropped` handlers)
- Test: `input.rs` (templum-terminal), `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `templum_terminal`:
  - `pub fn key_name(key: &KeyInput) -> Option<String>`: what `named_key` reads back as the same key. A key typing a
    printable character without Ctrl and Alt — or with both and a `key_char` other than its key (AltGr, the rule
    `encode_key` has) — is that character (`"A"`, `"@"`, `"ç"`); else the prefixes `ctrl+`, `alt+`, `shift+` in that
    order, then the key's name (`pageup` → `page_up`, `pagedown` → `page_down`, `enter`, `escape`, `tab`,
    `backspace`, `delete`, `insert`, arrows, `home`, `end`, `space`, `f1`–`f12`, or its one character). Any other key
    (`f13`, a media key, a lone modifier) is None: it isn't sent.
  - `pub fn text_names(text: &str) -> Vec<String>`: each character that isn't a control character, as its own name
    (IME commits and dead keys' results).
- Produces, in the glue:
  - `pub(super) fn remote_type(&mut self, keys: Vec<String>, window, cx)`: for the focused remote pane (its block's
    home id), unless read-only: not Live → told once until Live again: "Not connected: what you type isn't sent."
    (nothing sent); `me.scope == Control` → told once per terminal "Typing needs a Full share: share again on
    winx-desktop." (nothing sent); else `Command::Keys { block, keys }`. A paste: over `MAX_TEXT` bytes → "Paste at
    most 256 KiB into a mirrored terminal."; else `Command::Paste { block, text }`.
  - `RemoteSpace`'s `pub told: HashSet<(String, &'static str)>` (block, sentence); "told" is a notification.
  - `Update::Dropped(block)` → "Typing faster than winx-desktop answers: some keys were dropped."; `Update::Refused {
    about: Keys(block), sentence }` → its sentence; both as notifications.
- The paths in `workspace/input.rs`: `on_key_down`'s terminal branch builds its `KeyInput` exactly as now (Shift+Enter's
  newline rule, `option_types_text`) and, for a remote pane, sends `key_name(&input)` instead of `term.key` (stopping
  propagation when a name was sent, so the text path doesn't send it twice); the text input path sends `text_names`;
  `paste_clipboard` sends a paste. Templum's own shortcuts are matched first, as for any terminal, and those that yield
  to programs yield by the viewer's modes, which home's frames set (plan 1b's `Painter`).

- [ ] **Step 1: Write the failing tests:**

```rust
// templum-terminal input.rs
#[test]
fn names_are_what_the_keyboard_typed() {
    let k = |key, key_char, ctrl, alt, shift| KeyInput { key, key_char, ctrl, alt, shift };
    assert_eq!(key_name(&k("a", Some("A"), false, false, true)).as_deref(), Some("A"));
    assert_eq!(key_name(&k("q", Some("@"), true, true, false)).as_deref(), Some("@")); // AltGr on Windows
    assert_eq!(key_name(&k("p", None, true, false, true)).as_deref(), Some("ctrl+shift+p"));
    assert_eq!(key_name(&k("pageup", None, false, false, false)).as_deref(), Some("page_up"));
    assert_eq!(key_name(&k("f5", None, false, false, true)).as_deref(), Some("shift+f5"));
    assert_eq!(key_name(&k("left", None, false, true, false)).as_deref(), Some("alt+left"));
    assert_eq!(key_name(&k("f13", None, false, false, false)), None);
    assert_eq!(text_names("çã\u{7}!"), vec!["ç", "ã", "!"]);
}

#[test]
fn a_name_reads_back_as_the_same_key() {
    // for each KeyInput above and enter, shift+tab, ctrl+c, escape, space, ctrl+alt+delete: encode_key(&named_key(
    // &key_name(&k).unwrap()).unwrap().input(), mode) == encode_key(&k, mode) for mode in [empty, APP_CURSOR,
    // DISAMBIGUATE_ESC_CODES, REPORT_ALL_KEYS_AS_ESC].
}

// workspace/tests/remote.rs
#[gpui_kit::test]
fn typed_keys_reach_home_in_order(cx: &mut TestAppContext) {
    // plan 1c's home + live mirror (a Full share); the remote viewer focused; simulate KeyDown "escape", then "j",
    // then typing "ok" through the text path: home's API saw terminal_input calls whose keys concatenated are
    // ["escape", "j", "o", "k"] (home's own pane: inputs() grew by 4 after the clock moved past KEY_GAP).
}

#[gpui_kit::test]
fn typing_says_why_it_isnt_sent(cx: &mut TestAppContext) {
    // a Control share: a key -> one notification "Typing needs a Full share: share again on winx-desktop.", a second
    // key no second notification, home saw no terminal_input. A Full share, Disconnected: "Not connected: what you
    // type isn't sent." once. A 257 KiB paste: "Paste at most 256 KiB into a mirrored terminal.". An
    // Update::Dropped("b") drained -> the too-fast sentence.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-terminal --lib input::` and
  `cargo test -j 4 -p templum --bin templum workspace::tests::remote::typ` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and `workspace::tests::terminals` (local typing unchanged) → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 2: Moving and resizing home's blocks

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`HOLD`, `RemoteSpace::held`, `remote_moved`, the hold in
  `apply_remote`, the `Refused` handler), `crates/templum/src/workspace/input.rs` (`end_drag`; a drag doesn't start on
  a remote that isn't Live), `crates/templum/src/workspace/history.rs` (`undo_canvas` on a remote space)
- Test: `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `remote.rs`: `pub(crate) const HOLD: Duration = Duration::from_secs(2);`; `RemoteSpace`'s `pub held:
  HashMap<BlockId, (Rect, Instant)>`; `pub(super) fn remote_moved(&mut self, ids: &[BlockId], now: Instant)`: for each
  block of the shown remote whose rect differs from home's last, `Command::Move { block, rect }` and `held` gets
  `(rect, now)`. `apply_remote` keeps a held block's rect while home's rect differs from the held one (by more than
  0.5) and `now - at < HOLD`; then home's, and the hold goes.
- Hooks: `end_drag` of a `Move`, `MoveGroup` or `Resize` gesture on a remote space calls `remote_moved` with the blocks
  it moved (one `move_block` per block; nothing during the drag); `undo_canvas` applying `Change::Place` on a remote
  space calls it with those blocks (Ctrl+Z and Ctrl+Shift+Z are moves home takes). Snapping and the local undo steps
  work as on any canvas.
- `Update::Refused { about: Move(block), sentence }` → the hold goes, the block takes home's last rect, and the sentence
  is a notification.
- While the remote isn't Live, a drag or resize doesn't start (the pointer does nothing on the blocks but select and
  focus), and Ctrl+Z on it does nothing.

- [ ] **Step 1: Write the failing tests:**

```rust
#[gpui_kit::test]
fn a_moved_block_stands_until_home_shows_it(cx: &mut TestAppContext) {
    // home + live mirror. The mirror moves the remote terminal's rect by (100, 0) and calls the release path
    // (end_drag with a Move gesture, as workspace/tests/selection.rs drives drags): home's API saw one move_block
    // {block, x, y, w, h} with the new rect; before home's next state the mirror keeps its rect; pump until home's
    // own block has the new rect and the mirror's hold is gone. Advance the clock past HOLD with a state where home
    // has another rect (home moved it too): the mirror shows home's.
}

#[gpui_kit::test]
fn undo_on_the_mirror_moves_home_back(cx: &mut TestAppContext) {
    // after the move above, Ctrl+Z on the mirror -> a second move_block with the old rect; home's block back.
}

#[gpui_kit::test]
fn a_refused_move_goes_back(cx: &mut TestAppContext) {
    // drain an Update::Refused { about: About::Move(<home id>), sentence: "No." } after a local move: the block's
    // rect is home's last, held is empty, a notification "No.".
}

#[gpui_kit::test]
fn nothing_is_moved_while_not_live(cx: &mut TestAppContext) {
    // status Reconnecting: a drag attempt leaves the rect; Ctrl+Z does nothing; home saw no move_block.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum
  workspace::tests::remote::a_moved workspace::tests::remote::undo_on workspace::tests::remote::a_refused
  workspace::tests::remote::nothing_is` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same, `workspace::tests::history` and `workspace::tests::selection` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 3: Home's requests in the mirror's inbox

**Files:**
- Modify: `crates/templum/src/workspace/waits.rs` (`Wait::remote`, `Approved::Remote`, `Asked` for a remote),
  `crates/templum/src/workspace/questions.rs` and `waits.rs` (`answer_question`, `dismiss_now`,
  `dismiss_question`, `answer_allow` of a remote wait), `crates/templum/src/workspace/remote.rs` (`remote_requests`,
  the `Refused` handler), the request's notification title, `crates/templum/src/inbox.rs` and `balloon.rs` (a
  request whose scope is above the share's: its buttons disabled)
- Test: `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `waits.rs`: `Wait`'s `pub remote: Option<RemoteAsk>` with `pub(crate) struct RemoteAsk { pub space:
  String, pub request: String, pub scope: Scope, pub sent: bool }`; `Approved::Remote { yes: String, no: String }`
  (its buttons are home's words).
- Produces, in `remote.rs`: `pub(super) fn remote_requests(&mut self, id: &str, requests: &[RequestView], window, cx)`,
  called by `apply_remote` with the state's requests of home's workspace: a request id not yet a wait → `wait(Asked {
  conn: 0, id: Value::Null, reply: <an mpsc Sender whose receiver is dropped>, from: <the term id of the viewer pane
  of request.block> }, what, None, …)` with `remote` set (a question: `Waiting::Question` with an `Ask` built from its
  `questions` (`json: true`, `why` from `why.agent`); a plan: `templum_mcp::ask::plan(title, text)`; `connect`,
  `remove_worktree`, `merge_worktree`: `Waiting::Allow { then: Approved::Remote { yes, no } }` from `buttons`); a wait
  whose request is no longer in the state is removed without an answer (no reply is sent); a request whose block has
  no viewer pane here is skipped. These waits go through the same `wait()` as local ones, so the inbox lists them, the
  Requests button counts them while the remote is shown, and they chime and notify as a local one does, the
  notification titled "API on winx-desktop".
- Answering: `answer_question(key, answers, note)` of a remote wait sends `Command::Answer { request, args:
  {"answers": answers, "note": note} }` (no `note` when empty); `dismiss_now`/`dismiss_question` send `{"dismiss":
  true}`; `answer_allow(key, yes)` sends `{"allow": yes}`; the wait is marked `sent` and hidden from the inbox until
  home's state removes it. Not Live: the inbox's buttons are disabled for remote waits. `Update::Refused { about:
  Answer(request), sentence }` → `sent` false again (it shows) and the sentence is a notification. A request whose
  `scope` is above the share's (`me.scope`): its buttons disabled with the tooltip "Needs a Full share: share again on
  winx-desktop.".

- [ ] **Step 1: Write the failing tests** (home makes a question as `workspace/tests/inbox.rs` makes one, from its
  terminal's agent; a plan; a request to connect):

```rust
#[gpui_kit::test]
fn a_question_at_home_is_answered_from_the_mirror(cx: &mut TestAppContext) {
    // home + live mirror; home asks "Which database?" with options Postgres, SQLite from its terminal. Pump until
    // the mirror's inbox() has it (its from is the remote viewer's term id; remote.request == "r<n>"); the mirror
    // answers [["Postgres"]] (answer_question) -> home's wait is answered and its agent's reply holds "Postgres";
    // the mirror's wait leaves on the next state.
}

#[gpui_kit::test]
fn a_request_answered_elsewhere_leaves(cx: &mut TestAppContext) {
    // the same question answered at home by hand -> the mirror's wait goes; the mirror sent nothing (home's API saw
    // no answer_request).
}

#[gpui_kit::test]
fn allow_and_plans_cross_too(cx: &mut TestAppContext) {
    // a request to connect at home -> the mirror's wait is Allow with Approved::Remote { yes: "Allow", no: "Deny" };
    // answer_allow(key, true) -> home wired the two blocks. A plan -> Approve from the mirror approves it at home.
}

#[gpui_kit::test]
fn a_mirrored_request_rings_as_a_local_one(cx: &mut TestAppContext) {
    // a new remote request while the remote isn't shown: the chime was asked (the counter the inbox tests read) and
    // the notification's title is "<home workspace> on <computer>".
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum
  workspace::tests::remote::a_question workspace::tests::remote::a_request workspace::tests::remote::allow
  workspace::tests::remote::a_mirrored` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same, `workspace::tests::inbox` and `workspace::tests::api::answer` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 4: Edit…, Paste a new code…, the round trip and the coming expiry

**Files:**
- Modify: `crates/templum/src/space_dialog.rs` (a remote's Edit…), `crates/templum/src/connect_dialog.rs` (Paste a
  new code… for an existing remote), `crates/templum/src/workspace/remote.rs` (`replace_code`, `banner_words`'
  round trip and expiry, `NOT_MIRRORED`), `crates/templum/src/mirror.rs` (the ⓘ, the yellow round trip),
  `crates/templum/src/sidebar.rs` (the tile's menu gains Edit…), `crates/templum-api/src/client/mod.rs`
  (`Mirror::drop_key`)
- Test: `crates/templum/src/workspace/tests/remote.rs`, `remote.rs` units

**Interfaces:**
- Produces, in `templum_api::client`: `impl Mirror { pub fn drop_key(&self, entry: &str, file: bool) }` (deletes a key
  on the blocking pool; nothing else).
- Produces, in `remote.rs`:
  - `pub(crate) const NOT_MIRRORED: &str = "Not mirrored yet: notes', checklists', editors' and boards' content (a
    card shows each), ink and shapes, scrolling back in a terminal, the mouse in a terminal. Not on a mirrored
    workspace yet: adding, closing and wiring blocks, drawing, grid mode, Move to workspace, Copy and Paste of blocks,
    Stop, the Git popup.";` (the ⓘ's tooltip).
  - `pub(crate) fn replace_code(&mut self, id: &str, draft: Connecting, window, cx) -> Result<(), String>`: the remote
    `id` gets the new code's url, way, pin, a new `entry_name()`, `trust_added_authority` false, the draft's Access ID
    and proxy; `mirror.connect(id, …KeySource::Save { .. })` replaces the running one; once `Kept` arrives, the old
    entry is dropped (`drop_key(old, old_file)`). Its blocks are replaced by the new home state as any state is.
  - `banner_words` (Live) gains, after the round trip: the latency over 250 ms makes the banner's round trip yellow,
    with the tooltip "Each key waits about 300 ms for winx-desktop's answer." (the latency rounded to tens); within 3
    days of `me.expires`: " · ends in 3 days: share again on winx-desktop" (`settings::api::span` of what is left:
    "ends in 5 h" under a day).
- A remote's Edit… (the tile's menu: Open, Edit…, Disconnect or Connect, Forget…): home's name, dim "named on
  winx-desktop" (no Name field); Color and Icon (the local look, saved in the index's `icon`/`color`; none: home's);
  **Connection**: the address (url), the way (`Code::said` of it), "Trusts an authority this computer added" when
  `trust_added_authority`, the Access Client ID when set, the proxy when set, what the share can do ("Full: watch, move
  blocks, answer requests and type into terminals" / "Control: watch, move blocks and answer requests"), when it
  expires (`expiry_words`), the round trip; **Paste a new code…** (the Connect dialog for this remote, calling
  `replace_code`). No Folder, no Sharing.

- [ ] **Step 1: Write the failing tests:**

```rust
// remote.rs
#[test]
fn the_banner_tells_a_slow_round_trip_and_a_coming_expiry() {
    // Live, latency 84 ms, expires in 30 days: "Mirror of API on winx-desktop · Full · 84 ms", not slow;
    // latency 312 ms: slow, its tooltip "Each key waits about 310 ms for winx-desktop's answer."; expires in
    // 2 days 3 h: ends with " · ends in 3 days: share again on winx-desktop"; in 5 h: " · ends in 5 h: share again on
    // winx-desktop". (Banner gains `pub slow: Option<String>`, the tooltip.)
}

// workspace/tests/remote.rs
#[gpui_kit::test]
fn a_new_code_replaces_the_connection_and_its_key(cx: &mut TestAppContext) {
    // home + live mirror; home revokes the share and makes a new one: the mirror is Cut(Revoked); replace_code("r1",
    // <new code>) -> pump until Live; the mock store has the new entry and not the old; the index's entry names the
    // new one; trust_added_authority false.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum workspace::remote::
  workspace::tests::remote::a_new_code` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same → PASS; look at Edit… and Paste a new code… with `--dev` (or say so).
- [ ] **Step 5: Leave uncommitted.**

### Task 5: The end-to-end test

**Files:**
- Create: `crates/templum/src/workspace/tests/mirror_end_to_end.rs` (`mod mirror_end_to_end;` in `tests/mod.rs`)

**Interfaces:**
- Consumes: everything above. One headless workspace is both ends (*the orchestrator's call*): its API on loopback
  (`with_data` + `turn_on`), a Full share of its first workspace made with `share_workspace` (Through a tunnel, address
  `http://127.0.0.1:<port>`), and that code connected with `connect_code` into `r1` (mock keys). The share is limited
  to the first workspace, so `r1` (a workspace of the same app) is never in what the mirror sees.

- [ ] **Step 1: Write the tests:**

```rust
#[gpui_kit::test]
fn a_workspace_mirrors_itself_over_loopback(cx: &mut TestAppContext) {
    // three_blocks in the first workspace (terminal, note, kanban) and a question asked from its terminal.
    // 1. r1 goes Live; its blocks are at the first workspace's rects (one viewer pane, two Mirrored cards).
    // 2. Moving r1's terminal (remote_moved after a rect change) moves the first workspace's block (pump until).
    // 3. The question shows in r1's inbox; answering it there answers it in the first workspace.
    // 4. Revoke in the first workspace's Edit… dialog (revoke_token of the share) -> r1 is Cut(Revoked) and dim.
    // 5. Never saved, never a relay: after a snapshot and save_canvas_now no workspaces/r1.json; notes.json,
    //    checklists.json and boards.json hold nothing new; overview (Templum Agent) and GET /v1/state with a
    //    Settings token list r1 without blocks; find_block(<the first workspace's terminal uid>) finds the first
    //    workspace's block, never r1's; r1's uids are empty.
}

#[cfg(unix)]
#[gpui_kit::test]
fn what_home_s_shell_prints_shows_on_the_mirror_and_keys_reach_it(cx: &mut TestAppContext) {
    // Workspace::with(Some(("sh", vec![])), ..) so the first workspace's terminal runs sh; r1 Live and shown, its
    // viewer streaming. Home's shell gets `printf 'from-home\n'` (typed at home): pump until r1's viewer screen shows
    // "from-home". On r1, the viewer focused, keys typed through the key path: "echo", " ", "from-mirror", enter ->
    // pump until home's terminal text (read_terminal) shows "from-mirror" on its own line, and r1's viewer too.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum --bin templum workspace::tests::mirror_end_to_end::` → they
  should pass on the code of Tasks 1–4 and plans 1a–1c; a failure is a bug in that code: fix it there (with its own
  failing unit test first), never by loosening this test.
- [ ] **Step 3: Run them three times** in a row → PASS each time (no flake).
- [ ] **Step 4: Leave uncommitted.**

### Task 6: `--bench-mirror`

**Files:**
- Modify: `crates/templum/src/main.rs` (the flag; a temporary data folder for it; no welcome, no autostart),
  `crates/templum/src/bench.rs` (`run_mirror`)

**Interfaces:**
- `--bench-mirror`: one Templum as both ends. The data folder is `std::env::temp_dir()/templum-bench-mirror-<pid>`
  (made empty at start, removed when the bench ends, also on a failure); the first-run welcome and the autostart are
  off as for every bench; the window is the bench's (no throttling when unfocused).
- `pub(crate) fn run_mirror(window: AnyWindowHandle, ws: Entity<Workspace>, cx: &mut App)`: builds the
  `--bench-mixed` cockpit in the first workspace (the same code `run(.., mixed: true, ..)` uses: share it, don't copy
  it), turns the API on (loopback, plain HTTP), makes a Full share of the first workspace through `share_workspace`
  (address `http://127.0.0.1:<port>`), connects it as `r1` with `Keys::without_keyring(None)` and `KeySource::Memory`
  (no keyring is touched), switches to `r1`, waits (with a 30 s deadline) until it is Live and streams every terminal
  on screen, makes home's terminals print continuously (as `--bench` makes its terminals print), then measures frames
  exactly as `run` does and prints the same JSON line with `"bench":"mirror"` (and the same fields: `p95_ms`, …). A
  deadline missed prints `{"bench":"mirror","error":"…"}` and exits non-zero.

- [ ] **Step 1: Implement** (UI-only: verified by running it).
- [ ] **Step 2: Run** `cargo run -j 4 --release -p templum -- --bench-mirror` → one JSON line, `p95_ms` ≤ 16.7, the
  temporary folder gone afterwards (`ls $TMPDIR/templum-bench-mirror-*` finds nothing). Report the line. A p95 over
  16.7: find the cost (a profile or timing logs), fix it, and say what it was.
- [ ] **Step 3: Leave uncommitted.**

### Task 7: Docs, minors, the full check and every bench

**Files:**
- Modify: `docs/api.md` (a "Templum to Templum" section for people), `docs/specs/templum-to-templum.md` (Status:
  "Batch 1 implemented (plans 2026-10-25-t2t-batch-1a…1d)"; "As built ▸ Batch 1d", and a short "As built ▸ Batch 1"
  summary of what differs from the design across the four plans), `docs/specs/api.md` (its Templum to Templum section:
  batch 1 built), `docs/specs/workspaces.md`, `docs/specs/cockpit.md` (remote tiles' Edit…), `docs/specs/settings.md`
  (shares among the tokens, if 1a didn't), `docs/specs/ask-inbox.md` (requests from a mirrored workspace),
  `docs/ARCHITECTURE.md` (typing, moves and requests on a remote; `--bench-mirror`), `CLAUDE.md` (the Performance line
  gains `--bench-mirror` for a change to the mirror's painting or its client), `docs/guide.md` (two lines: share a
  workspace, connect to a Templum), `docs/minors.md`

**docs/api.md, "Templum to Templum"** (for people, after the reference's sections; every step tried with two `--dev`
instances, or say it wasn't):
1. What it is: another computer mirrors one workspace live; home is the truth; nothing runs on the mirror.
2. **Share** (home): Edit… ▸ Share…, the fields, Full vs Control, the expiry, the two ways in and what the API must be
   for each, Make code, the code shown once, Revoke, the cast mark; `share_workspace`/`revoke_share` for scripts.
3. **cloudflared** (the person's case): `cloudflared tunnel --url http://127.0.0.1:7411` (or a named tunnel to the
   API's port), share Through a tunnel with the tunnel's `https://` address; Cloudflare Access with a service token
   (recommended), and where the mirror takes the Client ID and Secret (Connect ▸ More…).
4. **Connect** (the mirror): sidebar ▸ Connect to a Templum…, paste, what the line under the field says, More… (Access,
   Proxy), where the key is kept (the keyring; the file option and its warning).
5. **What each way in checks:** pinned (the certificate's SHA-256; a New certificate at home needs a new code), public
   (the system's authorities, then Mozilla's: a company proxy that reads HTTPS is refused unless you accept it for that
   remote), loopback (plain HTTP to this computer: an `ssh -L` forward).
6. **A proxy:** typed in More… or from `HTTPS_PROXY`/`ALL_PROXY` (and `NO_PROXY`), Basic credentials only; NTLM,
   Kerberos, PAC and the system's proxy settings aren't supported.
7. **When it isn't live:** the banner's states in a short table (what retries, what needs a new code).
8. **What a mirror can't do yet:** the ⓘ's list; and the security notes: a Full code runs commands at home; keep agents
   you don't trust away from a mirror with a Full share; the tunnel's provider sees the traffic; short expiries.

**docs/minors.md** (`## Templum to Templum`), at least:
- A selection of several blocks moved on the mirror is one Ctrl+Z step per block at home (the spec's known limit).
  Fix: a `move_block` taking several blocks.
- A request whose block has no viewer on the mirror (not a terminal) isn't listed there. Fix: list it under the
  workspace with no block to go to.
- Shift+Enter in an agent without the kitty keyboard reaches home as Enter (home's newline rule is its local keyboard's,
  not `terminal_input`'s). Fix: apply the rule in `press_each` for `terminal_input`.
- Anything else deferred while building.

- [ ] **Step 1: Write the docs** above.
- [ ] **Step 2: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 3: The benches**, one after the other: `cargo run -j 4 --release -p templum -- --bench`, `-- --bench-mixed`,
  `-- --bench-mirror`: each `p95_ms` ≤ 16.7; report the three JSON lines.
- [ ] **Step 4: Leave uncommitted;** report the files changed, the checks' and benches' results, and what went to
  docs/minors.md.

## Open for the orchestrator

1. **The end-to-end test runs one workspace as both ends** (the spec's "in one process"), and its shell half is
   `#[cfg(unix)]` (a shell that echoes; Windows CI runs the rest). *Recommended:* keep.
2. **A request from a block that isn't a terminal on the mirror is skipped** (a minor); agents' requests always come
   from their terminal. *Recommended:* keep.
