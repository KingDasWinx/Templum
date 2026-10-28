# Templum to Templum, Batch 1c (The Remote Workspace on the Canvas) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer does the whole plan, then fresh Opus 5.5 reviewers
> review it in a loop until one passes it. The implementer **commits nothing**: everything stays uncommitted for the
> orchestrator.

**Goal:** Kanban card 171 part 2, batch 1, third of four plans: on the **mirror**, Connect to a Templum… turns a code
into a **remote workspace** in the sidebar that shows home's workspace live: its blocks at home's rectangles, its
terminals as live viewers scaled to fit, its agents' dots and chips, "not mirrored yet" cards for the other kinds, a
banner that tells every state of the connection, and nothing of it ever saved as a canvas or seen by a local tool.
Acting from the mirror (typing, moving, answering) is plan 1d; this plan leaves a working view-only mirror.

**Architecture:** A remote workspace is a `Space` with `remote: Some(Box<RemoteSpace>)` (`workspace/remote.rs`): the
index keeps its connection (`session::RemoteEntry`), never a `<id>.json`. The workspace starts the client of plan 1b
(`templum_api::client::Mirror`) at the first remote, wakes on its updates through a futures channel (as the PTY wake
does) and applies them at most once per wake: statuses into `RemoteSpace`, the newest home state into the space's
blocks. Home's block ids live only in `RemoteSpace::homes`, never in `Space::uids`, so no local tool, the block
server or the control server can name a remote block. Terminals are viewers (no scrollback) fed by the client's
thread through `Feeder`s; the UI picks which to stream (at most 12) and paints each at the zoom step that fits home's
grid in the block.

**Tech Stack:** Rust 2024; GPUI through gpui-kit 0.7.3 (pinned); `templum-api`'s client (plan 1b); `templum-canvas`
(`ZOOM_STEPS`, `Camera`), `templum-terminal` (`Terminal::viewer`, `Feeder`).

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), whole; this plan builds "On the mirror:
connecting", "The remote workspace" (all but typing, moving, requests and Edit…), "When it isn't live", "Mirroring the
state" (its UI half), "Terminals" (the stream set and the scaled cells), "What the mirror's API shows" and "Persistence
on the mirror". The code it builds on: `crates/templum/src/workspace/{mod,spaces,persist,blocks,subagents,api_state,
control,input,render}.rs`, `crates/templum/src/{session,sidebar,spaces_screen,space_dialog,block,grid,keys,toolbar}.rs`,
`crates/templum/src/menu/`, and the tests' helpers in `crates/templum/src/workspace/tests/{mod,api}.rs`.

**Consumes:** plan 1a (`code::{decode, Code, Trust, Wrong}`, `MeView`, `WorkspaceView::icon`, `BlockView::read_only`,
`ModesView`, `share_workspace` refusing a non-local workspace); plan 1b (`client::{Mirror, Remote, KeySource, Keys,
Secrets, Kept, Command, Update, Status, Why, Bad, entry_name, SERVICE, SERVICE_DEV}`, `Feeder`, `State::apply`).

**Later plans consume:** 1d uses `RemoteSpace` (`homes`, `me`, `status`; 1d adds `held` and `told`), `Pane::remote`,
`Workspace::{mirror_send, on_remote}`, `remote::banner_words`, the Connect dialog's decode path (Paste a new code…).

## Global Constraints

- **MEMORY LIMITS on this machine** (systemd-oomd has killed the desktop app several times): **every** cargo command
  takes `-j 4`; **never two cargo commands at once**; while working, run only the targeted tests each step names
  (`cargo test -j 4 -p templum --bin templum workspace::tests::remote::<name>`, `cargo test -j 4 -p templum --bin
  templum <module>::`); the full suite **once**, in Task 8.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; what `templum-api` gains here (the mirror's API
  view, two `pub` re-exports) stays UI-free. No `unsafe`.
- **No new dependency.** `cargo deny check` passes unchanged.
- **Every feature in the API:** a remote workspace is in the mirror's state as a workspace with `remote` (Task 7); its
  blocks never are. docs/api.md's lines come with plan 1d's section; this plan adds the `remote` field's line.
- **The UI thread never waits** on the network or the keyring: it only calls `Mirror`'s non-blocking methods and
  drains on wake. **Painting is not slowed:** applying a state happens on wake, not per paint; choosing the stream set
  runs at most every 100 ms; a viewer is painted like any terminal.
- **Never saved, never a relay:** no `<r…>.json` is ever written or set aside; no note, checklist, board or card of
  home's reaches `notes.json`, `checklists.json` or `boards.json`; `Space::uids` of a remote workspace stays empty; a
  remote block never adds a Ctrl+Z step for what home did.
- **The key never leaks:** the code leaves the Connect dialog's field when Connect is pressed (success or failure);
  only `KeySource::Save` carries it to the client; the index keeps the entry's name, never a secret.
- **Words, verbatim** (the spec's): "Connect to a Templum…"; "The key is kept in this computer's keyring (Keychain /
  Credential Manager / the Secret Service)." (each platform names its own: macOS "the Keychain", Windows "the
  Credential Manager", others "the Secret Service"); "This computer has no keyring Templum can use: the connection
  lasts until Templum quits."; "Keep the key in a file only you can read"; "A copy of the data folder would carry it.";
  "Connecting…"; "Not mirrored yet: open it on winx-desktop."; "Read-only: a subagent's transcript"; "Only the live
  screen is mirrored for now."; "Not on a mirrored workspace yet"; every banner sentence of the spec's "When it isn't
  live" table, with home's computer and workspace names in them; "Disconnected. **Connect**"; "Forget API on
  winx-desktop?"; "Also revoke the share on winx-desktop"; "API is a mirror of winx-desktop: act on it there.";
  "API (mirror of winx-desktop)".
- **Every logic change has a test that fails without it;** UI-only changes: say how they were verified. Headless GPUI
  tests (`#[gpui_kit::test]`) with `with_data`; the mirror's keys in a `keyring_core::mock::Store`
  (`ws.keys = Keys::with_store(..)` before connecting); home is the same headless workspace's own API on loopback
  (`with_data` + `turn_on` + `port`): *the planner's call*, one workspace is both ends (its first workspace shared,
  the code connected as `r1`), as plan 1d's end-to-end test is, so "home moves its block" is moving the first
  workspace's block and no test needs a second window or leaves the process; waits are conditions with a 10 s deadline
  (`cx.run_until_parked()` in a wait loop), never one fixed sleep.
- **CI** runs Linux, macOS and Windows with no agent CLIs: `cfg` gating for platform tests and their imports; text of
  fixed width is clipped (`w_full` + `overflow_hidden` + `truncate`): the banner, the card, the dialog's lines.
- **Don't open a Templum window against the person's real data:** `--dev` (two dev instances need two data folders:
  `TEMPLUM_DATA_DIR` or what `--dev` documents) and headless tests only.
- **Minors** go to `docs/minors.md` under `## Templum to Templum`. Docs lines ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **Templum quits and starts again with a remote whose keyring is locked:** the remote shows, dimmed, "Templum
   couldn't read this connection's key from the keyring. **Try again**, or **Paste a new code…**", the canvas empty,
   nothing else breaks. → Task 2 `a_remote_without_its_key_says_so_at_start`.
2. **Home adds, moves and closes blocks while the mirror watches:** the mirror's blocks follow, a closed one goes, no
   undo step appears, `uids` stays empty. → Task 4 `home_s_blocks_appear_move_and_go`.
3. **Home's terminal grid is 200 × 60 and the mirror's block is small:** the viewer keeps home's grid and is painted
   at the largest zoom step that fits, never re-gridded by a local resize. → Task 5
   `a_viewer_keeps_home_s_grid_and_fits_its_block`.
4. **A local agent on the mirror calls `overview`, `read_terminal` on a remote block's id, or `add_block` into the
   remote workspace:** it sees no remote block and is refused by name. → Task 7 `no_local_tool_sees_a_remote_block`.
5. **Home deleted the workspace / the share expired / a second computer took the code:** the right sentence, no
   retry, Forget offered. → Task 6 `every_state_has_its_words`.

---

### Task 1: A remote workspace in the index, never saved as a canvas

**Files:**
- Create: `crates/templum/src/workspace/remote.rs` (`RemoteSpace`; `mod remote;` in `workspace/mod.rs`)
- Modify: `crates/templum/src/session.rs` (`RemoteEntry`, `Keeping`, `IndexEntry::remote`),
  `crates/templum/src/workspace/mod.rs` (`Space::remote`), `spaces.rs` (`load_spaces`, `index`, `delete_space`,
  `stop_space`/`can_stop`, `remote_id`), `persist.rs` (snapshot and save skip remote spaces)
- Test: create `crates/templum/src/workspace/tests/remote.rs` (`mod remote;` in `tests/mod.rs`), `session.rs` units

**Interfaces:**
- Produces, in `session.rs`:
  - `#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)] #[serde(rename_all = "lowercase")]
    pub(crate) enum Keeping { Keyring, File, #[default] None }`.
  - `#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)] pub(crate) struct RemoteEntry { pub url:
    String, pub way: String, pub pin: Option<String>, pub home: Option<String>, pub entry: String, pub key: Keeping,
    pub trust_added_authority: bool, pub access_id: Option<String>, pub proxy: Option<String>, pub camera:
    Option<SavedCamera> }` (`way`: `pinned`, `public` or `loopback`; `home`: home's workspace id; `entry`: the keyring
    entry's name; `proxy` without credentials). Every field `#[serde(default)]`.
  - `IndexEntry`'s `#[serde(default, skip_serializing_if = "Option::is_none", deserialize_with = "any_remote")] pub
    remote: Option<RemoteEntry>` (a value that doesn't read is None, as the look's keys are). `IndexEntry::icon` and
    `color` of a remote entry are the person's local look (None: home's).
- Produces, in `workspace/remote.rs`: `pub(crate) struct RemoteSpace { pub entry: RemoteEntry, pub status: Status,
  pub me: Option<MeView>, pub home: Option<WorkspaceView>, pub latency: Option<Duration>, pub live_at:
  Option<SystemTime>, pub since: Instant, pub homes: HashMap<String, BlockId> }`
  (`status` starts `Status::Connecting`; `since` is when `status` last changed, for "next in 8 s"), `impl RemoteSpace {
  pub fn new(entry: RemoteEntry) -> RemoteSpace; pub fn remote(&self, key: KeySource) -> client::Remote }` (the
  `Trust` from `way` and `pin`).
- Produces, on `Space`: `pub remote: Option<Box<RemoteSpace>>`; `fn is_remote(&self) -> bool`.
- Produces, on `Workspace`: `pub(super) fn remote_id(&self) -> String` (`r<n>`, the smallest `n ≥ 1` no workspace
  has); `load_spaces` makes a remote entry a `Space` with its `remote`, its name and look from the entry, no canvas
  read; `index()` writes `remote` back with `camera` from `space.canvas.camera`; `delete_space` of a remote never sets a
  file aside; a remote can't be stopped (`can_stop` false); `snapshot` and `save_canvas_now` skip remote spaces.

- [ ] **Step 1: Write the failing tests:**

```rust
// session.rs
#[test]
fn a_remote_entry_round_trips_and_an_odd_one_is_none() {
    // an Index with one plain entry and one remote entry (url, way "pinned", pin, home "w3", entry "remote-ab12cd34",
    // key Keyring, camera Some) serializes and reads back equal; "remote": 7 reads as None; an index written before
    // (no "remote") reads with None. The JSON holds no "tpl_".
}

// workspace/tests/remote.rs
#[gpui_kit::test]
fn a_remote_workspace_is_never_saved_as_a_canvas(cx: &mut TestAppContext) {
    // with_data; write an index whose second entry is remote ("r1", key None); start the workspace: two spaces, the
    // second is_remote, its blocks empty. Switch to it, move its camera, snapshot (as the 5 s save does) and quit
    // (save_canvas_now): no workspaces/r1.json exists; the index's r1 entry has the camera moved; remote_id() == "r2".
    // delete_space("r1"): no r1.json.bak, the index no longer lists it.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum session::` and
  `workspace::tests::remote::` → compile errors.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same, then `workspace::tests::spaces` and `workspace::tests::persist` (whatever files hold
  the workspaces' and saving's tests) → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 2: The client in the workspace

**Files:**
- Modify: `crates/templum/src/workspace/mod.rs` (`Workspace::{mirror, keys}`), `crates/templum/src/workspace/remote.rs`
  (`mirror`, `drain_mirror`, `connect_remotes`, `mirror_send`), `crates/templum/src/main.rs` (`keys` with
  `SERVICE_DEV` under `--dev`)
- Test: `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, on `Workspace`:
  - `pub(crate) mirror: Option<Mirror>` and `pub(crate) keys: Keys` (`Keys::system(SERVICE or SERVICE_DEV,
    data_dir)`; no data folder: `Keys::without_keyring(None)`).
  - `pub(super) fn mirror(&mut self, window: &mut Window, cx: &mut Context<Self>) -> &Mirror`: started at the first
    need with a wake that sends on a `futures::channel::mpsc::unbounded::<()>()`, and a `cx.spawn_in(window, …)` loop
    that, per wake, empties the channel, then calls `drain_mirror` once.
  - `pub(super) fn drain_mirror(&mut self, window: &mut Window, cx: &mut Context<Self>)`: each `(id, update)` into its
    remote space: `Status` → `status`, `since`, `live_at` (when it goes Live); `Me` → `me` and `entry.home` (its first
    workspace; the index written when it changed); `Latency` → `latency`; `Kept(k)` → `entry.key` (`Keyring`, `File`,
    `Memory` → `None`) and the index written; `Refused`/`Dropped` → plan 1d's handlers (here: a notification with the
    sentence); `Keyring(answers)` with id `""` → the Connect dialog's line (Task 3). Then, for each remote space,
    `mirror.state(id)` when Some → `apply_remote` (Task 4; until then it only sets `home` from the state's workspace).
    `cx.notify()` once.
  - `pub(super) fn connect_remotes(&mut self, window, cx)`: called once after `load_spaces`; each remote whose
    `entry.key` is `Keyring` or `File` → `mirror.connect(id, remote.remote(KeySource::Kept { entry, file }))`; `None`
    → `Status::Disconnected`.
  - `pub(crate) fn mirror_send(&self, id: &str, command: Command)` (nothing without a mirror).

- [ ] **Step 1: Write the failing tests** (`tests/remote.rs`; `both_ends(cx)` = a headless workspace `with_data` +
  `turn_on`, its `keys` a mock store, a share of its first workspace made with `share_workspace` (plan 1a; address
  `http://127.0.0.1:<port>`); `with_remote(ws, code, keeping)` writes an index entry `r1` made from the code (as
  Connect will, Task 3) and starts the workspace again from that index, the key saved in the mock store first when
  `keeping` is Keyring):

```rust
#[gpui_kit::test]
fn a_remote_connects_at_start_and_goes_live(cx: &mut TestAppContext) {
    // with_remote(both_ends(cx), code, Keyring): the remote's status is Connecting,
    // then (pump until) Live; me.computer is home's host name; entry.home == Some(<home's first workspace id>);
    // latency is Some.
}

#[gpui_kit::test]
fn a_remote_without_its_key_says_so_at_start(cx: &mut TestAppContext) {
    // entry.key Keyring but nothing in the mock store: pump until status == Cut(Why::Keyring); entry.key None:
    // status Disconnected at once, and mirror.connect was never called (no client thread: mirror is None).
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum workspace::tests::remote::`
  → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 3: Connect to a Templum…

**Files:**
- Create: `crates/templum/src/connect_dialog.rs` (`mod connect_dialog;` in `main.rs`)
- Modify: `crates/templum-api/src/client/mod.rs` (re-export `dial::{proxy_of, proxy_from_env}` as `pub`),
  `crates/templum/src/sidebar.rs` (the button), `crates/templum/src/spaces_screen.rs` (the header's button),
  `crates/templum/src/keys.rs` (`Command::ConnectTemplum`), `crates/templum/src/workspace/remote.rs`
  (`connect_code`, `connect_result`)
- Test: `crates/templum/src/workspace/tests/remote.rs`, `keys.rs`' tests

**Interfaces:**
- Produces, on `Workspace` (`remote.rs`):
  - `pub(crate) struct Connecting { pub code: Code, pub access: Option<(String, String)>, pub proxy: Option<String>,
    pub file: bool }` (`Debug` derived: `Code`'s key redacts; the Access secret is wrapped so it prints `…`).
  - `pub(crate) fn connect_code(&mut self, draft: Connecting, window, cx) -> Result<String, String>`: the proxy text
    through `client::proxy_of` (its Err is the dialog's); a new hidden `Space` with id `remote_id()`, name "Connecting…",
    `remote: Some(RemoteSpace::new(RemoteEntry { url, way, pin, entry: client::entry_name(), key: None, access_id,
    proxy, .. }))`; `mirror.connect(id, remote(KeySource::Save { entry, secrets: Secrets { token, access_secret,
    proxy_auth }, file }))`; the index written; answers the id. `pending_connect = Some(id)`.
  - `pub(crate) fn connect_result(&self) -> Option<Result<(), String>>`: for `pending_connect`: `Live` → Ok (the dialog
    closes, `switch_to` the remote, `pending_connect` None, and if the clipboard's text is still the code, it is
    replaced by an empty string); the first `Reconnecting` or `Cut` → Err with the banner's words for it (Task 6's
    `banner_words`; until Task 6, `format!("{why:?}")` is not allowed: use a placeholder sentence "Couldn't connect.",
    replaced in Task 6) and the remote is forgotten (`mirror.forget(id, false)`, the space removed, the index
    written); an AddedAuthority cut waits for the person's choice (Task 6). *The planner's call:* during Connect a home that doesn't answer is a failure shown in the dialog, not a
    remote left reconnecting.
  - `pub(crate) keyring: Option<bool>`: what `Mirror::probe` answered (None: not yet).
- The Connect dialog (`connect_dialog.rs`, opened by the sidebar's button, the workspaces screen's, and
  `connect_templum`): **Connection code** (focused); under it, as soon as it holds text, `code::decode`'s `said()` or
  its `Wrong`'s sentence; **More…** (folded): Cloudflare Access service token (Client ID, Client Secret), Proxy
  (prefilled with `proxy_from_env(host, std::env::var)` once a code is read); the key's line (`keyring`: Some(false) →
  "This computer has no keyring Templum can use: the connection lasts until Templum quits." and the switch "Keep the
  key in a file only you can read" with "A copy of the data folder would carry it."; else the keyring sentence for
  this platform); **Connect** (disabled until the code decodes) → "Connecting…", the field emptied, `connect_code`;
  the dialog polls `connect_result` on each render (the workspace notifies it).
- Sidebar: under **+**, a button with Lucide `plug` (`IconName::Plug`, or the nearest that exists in gpui-kit's
  `IconName`: check its source), tooltip "Connect to a Templum…". Workspaces screen: the same button beside "New
  workspace". Keymap: `Command::ConnectTemplum`, name `connect_templum`, label "Connect to a Templum…", no default key.

- [ ] **Step 1: Write the failing tests:**

```rust
// workspace/tests/remote.rs
#[gpui_kit::test]
fn a_code_becomes_a_live_remote_workspace(cx: &mut TestAppContext) {
    // both_ends(cx) gives a Loopback code; connect_code(Connecting { code: decode(code), ..
    // file: false }) -> Ok("r1"); pump until connect_result() == Some(Ok(())): the shown workspace is r1, its entry's
    // key is Keyring and the mock store holds the entry; the clipboard, which held the code, is empty; the index's
    // JSON has no "tpl_" and no Access secret.
}

#[gpui_kit::test]
fn a_code_that_cant_connect_leaves_nothing(cx: &mut TestAppContext) {
    // a code for http://127.0.0.1:<a closed port>: connect_result() becomes Some(Err(_)); no r1 space, the index
    // unchanged, the mock store has no entry left.
}

// keys.rs
#[test]
fn connect_templum_is_a_command_without_a_default_key() {
    // Command::ConnectTemplum's name is "connect_templum", its label "Connect to a Templum…", its default keys empty.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum workspace::tests::remote::a_
  keys::` → FAIL.
- [ ] **Step 3: Implement** the helpers, the dialog, the buttons and the command.
- [ ] **Step 4: Run** the tests → PASS. **Look at it** with two `--dev` instances on two data folders (one shares,
  one connects) or say there was no display.
- [ ] **Step 5: Leave uncommitted.**

### Task 4: Home's blocks on the mirror's canvas

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`apply_remote`), `crates/templum/src/workspace/mod.rs`
  (`Body::Mirrored`, `Mirrored`, `Pane::remote`, `RemotePane`), `crates/templum/src/block.rs` (the card, the dots and
  chips of a remote pane), the matches on `Body` the compiler names, `crates/templum/src/workspace/agents_view.rs`
  (`doing` of a remote pane)
- Test: `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `workspace/mod.rs`: `Body::Mirrored(Mirrored)` with `pub(crate) struct Mirrored { pub kind: String,
  pub title: String }` (the one `Body` arm batch 2 replaces kind by kind); `Pane`'s `pub remote: Option<RemotePane>`
  with `pub(crate) struct RemotePane { pub block: String, pub agent: Option<AgentView>, pub read_only: bool }`.
- Produces, on `Workspace`: `pub(super) fn apply_remote(&mut self, id: &str, state: State, window, cx)`: takes the
  state's workspace `entry.home` (none there: Task 6's Deleted handling; here: nothing applied) into `remote.home`, the
  space's name (home's) and look (the entry's icon/color, else home's color and `icon`); then home's blocks of that
  workspace, by `z`: a block not in `homes` is added, quietly (no undo step, no focus taken, no appearing animation,
  `uids` entry removed at once) — a `terminal` as a viewer pane (`Terminal::viewer(title, grid from the rect,
  palette, TermOptions { scrollback: 0, ..config.term_options() }, notify)`, its `remote = Some(RemotePane { block,
  agent, read_only })`), any other kind as `Body::Mirrored { kind, title }`; a known block takes home's rect, title and
  agent (the rect unless plan 1d holds it); a block no longer there is removed (no ghost, no undo step); the canvas's
  order follows home's `z`; the wires are home's (`a`/`b` mapped through `homes`, labels kept by home ids).
  First state of a remote with no `entry.camera`: the camera shows `state.view` when it is home's workspace (its x, y
  and zoom), else fits the blocks (`Camera::fit`); after that the camera is the person's.
- A remote pane's dot and chips come from `RemotePane::agent`: `Pane::agent()` gives its `agent`; `Workspace::doing`
  maps `state` (`needs_you` → `Doing::NeedsYou(asks)`, `starting`, `waiting_subagents` → `AwaitsSubagents(n)`,
  `working` → `Working(n)`, `done`, `idle`); the chips read `model`, `effort` and `usage` from it. A remote pane never
  runs the local agent watch (no hook, no process read).
- A `Body::Mirrored` block is drawn as a card in its place: its kind's icon (the same icons the add menu uses; a kind
  the mirror doesn't know: Lucide `box` and the kind's name), its title, and "Not mirrored yet: open it on
  winx-desktop." (home's computer).

- [ ] **Step 1: Write the failing tests:**

```rust
#[gpui_kit::test]
fn home_s_blocks_appear_move_and_go(cx: &mut TestAppContext) {
    // both_ends(cx) with three_blocks in its first workspace (a terminal, a note, a kanban) and a wire terminal–note;
    // the mirror live on it: the remote space has 3 blocks at home's rects (world coordinates equal), the terminal a
    // viewer pane with remote.block == home's uid, the note and the kanban Body::Mirrored { kind: "note" / "kanban" };
    // one wire; uids is empty; undo is empty. Home moves its terminal (move_block through its own API, or by hand
    // in the home workspace) -> pump until the mirror's rect follows. Home closes the note -> it goes on the mirror.
    // Home adds a checklist -> a Mirrored { kind: "checklist" } appears. No notes.json/checklists.json/boards.json
    // write on the mirror holds home's titles.
}

#[gpui_kit::test]
fn a_remote_agent_s_dot_comes_from_home(cx: &mut TestAppContext) {
    // apply_remote with a hand-made State whose terminal block has agent { agent: "claude", state: "needs_you",
    // asks: "Allow Bash?", model: "opus", .. }: doing(pane) == Doing::NeedsYou("Allow Bash?"), pane.agent() ==
    // Some("claude"); state "working" with subagents 2 -> Working(2).
}

#[gpui_kit::test]
fn the_first_camera_is_home_s_then_the_person_s(cx: &mut TestAppContext) {
    // a state whose view is home's workspace at (100, 50) zoom 2.0 -> the remote's camera there; the person pans; a
    // new state with another view leaves the camera where the person put it.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum
  workspace::tests::remote::home_s workspace::tests::remote::a_remote_agent workspace::tests::remote::the_first` →
  FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and `workspace::tests::agents` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 5: Terminals live: which stream, and how they fit

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`streamed`, `fit_zoom`, `stream_terminals`),
  `crates/templum/src/block.rs` (a remote viewer painted at `fit_zoom`; the read-only footer),
  `crates/templum/src/workspace/mod.rs` (`Pane::fit` leaves a remote viewer's grid alone; `Pane::grid_size`), the
  render path that already runs per frame (it calls `stream_terminals`), `input.rs` (the wheel's line)
- Test: `crates/templum/src/workspace/tests/remote.rs`, `remote.rs` units

**Interfaces:**
- Produces, in `workspace/remote.rs`:
  - `pub(crate) const MOST_STREAMS: usize = 12;` and `pub(crate) const STREAM_EVERY: Duration =
    Duration::from_millis(100);`
  - `pub(crate) fn streamed(blocks: &[(String, Rect)], center: Point, focused: Option<&str>) -> Vec<String>`: the
    focused one first (when among `blocks`), then the others by the distance from their rect's center to `center`, at
    most `MOST_STREAMS`. `blocks` are the remote terminals at Full detail that intersect the view.
  - `pub(crate) fn fit_zoom(zoom: f32, content: Size, grid: (u16, u16), cell: Size) -> f32`: `k = min(content.w /
    (cols × cell.w), content.h / (rows × cell.h))` (content in world units, `cell` at zoom 1); the largest of
    `ZOOM_STEPS` not above `zoom × k`, else the smallest step.
  - `pub(super) fn stream_terminals(&mut self, now: Instant)`: at most every `STREAM_EVERY`, and only when the set
    changed: the shown remote's `streamed(..)` as `Command::Terminals(vec![(home id, pane.term.feeder())])`; a remote
    that isn't shown, or isn't Live, gets `Terminals(vec![])` once.
- `Pane::fit` does nothing for a remote pane; `Pane::grid_size()` of a remote pane is `term.shared().grid_size()` (the
  frame's size, which the client's `Feeder::resize` sets).
- `block.rs`: a remote pane's body is `grid::body(…, zoom: fit_zoom(z, content, grid, cell), …)`, clipped to the
  block; a read-only remote pane shows "Read-only: a subagent's transcript" in its footer. The wheel over a remote
  terminal shows "Only the live screen is mirrored for now." once per terminal (a notification) and scrolls nothing.

- [ ] **Step 1: Write the failing tests:**

```rust
// remote.rs
#[test]
fn the_nearest_twelve_stream_and_the_focused_one_always() {
    // 15 rects on a line, center at the 8th: streamed(.., None) has 12, the 8th first, the farthest 3 left out;
    // with focused = the 15th: it is first and the 12th nearest is out.
}

#[test]
fn fit_zoom_takes_the_largest_step_that_fits() {
    // grid (80, 24), cell (8, 16): content exactly (640, 384) at zoom 1.0 -> 1.0; content (320, 192) -> 0.5;
    // content (330, 400) at zoom 2.0 -> k 0.515…, 2 × k = 1.03 -> 1.0; content (10, 10) -> 0.25.
}

// workspace/tests/remote.rs
#[gpui_kit::test]
fn a_viewer_keeps_home_s_grid_and_fits_its_block(cx: &mut TestAppContext) {
    // home's terminal resized to 100x30 (its block made big); mirror live, the remote shown: pump until home's
    // server terminal_streams() == 1 and the mirror's viewer grid_size() == (100, 30); the mirror resizes its block
    // by hand (as a resize gesture's end does) -> grid_size() still (100, 30) (Pane::fit did nothing); home prints
    // "hi" -> the viewer's screen shows it. Switch the mirror to another workspace -> home's terminal_streams() == 0.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum workspace::remote::
  workspace::tests::remote::a_viewer` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same → PASS; look at a remote terminal at several zooms with `--dev` (or say so).
- [ ] **Step 5: Leave uncommitted.**

### Task 6: The banner, the tile, and every state of the connection

**Files:**
- Create: `crates/templum/src/mirror.rs` (`mod mirror;` in `main.rs`: the banner, the remote mark, the stopped card,
  the Forget… dialog)
- Modify: `crates/templum/src/workspace/remote.rs` (`banner_words`, `Banner`, `forget_remote`), `sidebar.rs` (the
  remote tile: dimmed when not Live, its mark, its tooltip, its menu), `spaces_screen.rs` (the card's line),
  `crates/templum/src/workspace/render.rs` (the banner over the canvas's top edge, the veil), `block.rs` (grey dots
  when not Live), `connect_dialog.rs` (the failure words from `banner_words`)
- Test: `remote.rs` units, `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `workspace/remote.rs`:
  - `#[derive(Clone, Copy, Debug, PartialEq)] pub(crate) enum Act { TryNow, Paste, Forget, StartIt, Connect,
    ConnectHere, DontConnect, ConnectAnyway }` and `#[derive(Clone, Debug, PartialEq)] pub(crate) struct Banner { pub text: String, pub acts:
    Vec<Act>, pub dim: bool }`.
  - `pub(crate) fn banner_words(remote: &RemoteSpace, now: Instant, wall: SystemTime) -> Banner`, with `name` =
    home's workspace name (else "this workspace"), `computer` = `me.computer` (else the code's host), `host` the
    code's host (no port when it is the scheme's default):
    - Live: "Mirror of **API** on **winx-desktop** · Full · 84 ms" (scope from `me`; the latency when known), not dim;
      home's workspace `stopped` → "API is stopped on winx-desktop." with `[StartIt]`, not dim (the card in the
      canvas's middle says it too).
    - Connecting: "Connecting…".
    - Reconnecting: "Reconnecting to winx-desktop… try 3, next in 8 s. Live 2 min ago." (`next` minus the time
      since `since`, whole seconds, at least 0; "Live …" left out when it never was) `[TryNow]`.
    - Cut: Expired "This share of API expired. Share again on winx-desktop, then **Paste a new code…**" `[Paste,
      Forget]`; Revoked "winx-desktop revoked this share. Share again there, then **Paste a new code…**" `[Paste,
      Forget]`; Deleted "API was deleted on winx-desktop." `[Forget]`; Replaced "Another computer connected with this
      share's code, so this one stopped. Each computer needs its own share." `[ConnectHere, Forget]`; PinChanged the
      spec's sentence `[Paste, Forget]`; Certificate(bad) "templum.example.com's certificate isn't valid: it expired /
      it is for another name / no authority this computer trusts signed it. Fix it where the tunnel or proxy runs,
      then **Try again**." (+ " If it is Templum's own, share Directly instead: it is pinned." for `Alone`) `[TryNow]`;
      AddedAuthority the spec's sentence ("HTTPS to templum.example.com is signed by an authority this computer
      added, …") `[DontConnect, ConnectAnyway]`, its tooltip adding "or the server doesn't send its whole chain";
      Access the spec's sentence `[TryNow]`;
      NotTemplum "templum.example.com didn't answer as a Templum (HTTP 404, text/html). Check that the tunnel points
      at port 7411." `[TryNow]`; TooOld "winx-desktop runs a Templum that can't be mirrored: update it." `[TryNow]`;
      Proxy "The proxy proxy.corp:8080 refused the connection (407: it asks for a login Templum can't give)." (other
      statuses: "(HTTP 403)") `[TryNow]`; Keyring "Templum couldn't read this connection's key from the keyring.
      **Try again**, or **Paste a new code…**" `[TryNow, Paste]`.
    - Disconnected: "Disconnected. **Connect**" `[Connect]`.
    Every state but Live and Connecting is `dim`.
  - `pub(crate) fn forget_remote(&mut self, id: &str, revoke: bool, window, cx)`: `mirror.forget(id, revoke)`, the
    space removed (switching away first when shown), the index written.
- The banner: window chrome over the canvas's top edge at a fixed scale (like the Templum Agent's pill:
  `chat/look.rs`'s scale), clipped to the canvas's width; its text, its acts as buttons (TryNow → `Command::TryNow`;
  Connect and ConnectHere → `TryNow`; Paste → plan 1d's Paste a new code… (here: opens the Connect dialog); Forget →
  the Forget… dialog; StartIt → `Command::Show`, tooltip "Starts API on winx-desktop and shows it on its screen.";
  DontConnect → `Command::Disconnect`; ConnectAnyway → `entry.trust_added_authority = true`, the index written, and
  `mirror.connect` again with `trust_added` and `KeySource::Kept`), and **Disconnect** while Live or Reconnecting
  (`Command::Disconnect`). A new code (plan 1d's Paste a new code…) starts with `trust_added_authority` false: it asks
  again.
- During Connect (Task 3's dialog), an AddedAuthority failure shows the same sentence with **Don't connect** (the
  remote is forgotten, as any failure) and **Connect anyway** (the remote is kept, trusted, and the dialog waits for
  Live again) instead of forgetting at once.
- While `dim`: the tile is dimmed, a veil (the canvas's background at 50%) lies over the blocks, which keep their last
  frames, and the agents' dots are grey. Starting Templum: the tile shows "Connecting…" and the canvas is empty until
  the first state.
- The tile: home's look (Task 4), a small remote mark at its bottom-right (a Lucide icon that exists in `IconName`,
  e.g. `Globe`), tooltip "API, on winx-desktop · 84 ms"; it is dragged, Ctrl+Tab'd and Ctrl+1–9'd like any tile. Its
  right-click menu: Open, Disconnect (or Connect), Forget…. The workspaces screen's card: "Mirror of winx-desktop · 3
  terminals · 1 Claude Code working" (the agents from `RemotePane::agent`).
- The Forget… dialog: "Forget API on winx-desktop?" with the switch "Also revoke the share on winx-desktop" (on while
  Live, off and disabled otherwise) → `forget_remote(id, revoke)`.

- [ ] **Step 1: Write the failing tests:**

```rust
// remote.rs
#[test]
fn every_state_has_its_words() {
    // a RemoteSpace with home name "API", me.computer "winx-desktop", url "https://templum.example.com": one
    // assertion per row above: the exact text and acts. Reconnecting { attempt: 3, next: 10 s } with since 2 s ago
    // and live_at 2 min ago -> "Reconnecting to winx-desktop… try 3, next in 8 s. Live 2 min ago."; a pinned remote
    // shows the host:port; Live with latency 84 ms and scope Full -> "Mirror of API on winx-desktop · Full · 84 ms".
}

// workspace/tests/remote.rs
#[gpui_kit::test]
fn revoking_at_home_cuts_the_mirror_and_forget_cleans_up(cx: &mut TestAppContext) {
    // mirror live; home revokes the share (revoke_token) -> pump until the remote's banner_words is the Revoked
    // sentence and dim; forget_remote("r1", false): no r1 space, the index has no r1, the mock store has no entry.
}

#[gpui_kit::test]
fn a_stopped_home_workspace_offers_start(cx: &mut TestAppContext) {
    // home stops its first workspace (stop_now) -> the mirror's banner is "API is stopped on winx-desktop." with
    // StartIt, not dim; Command::Show (the act) -> home's workspace is shown and running (pump until not stopped).
}

#[gpui_kit::test]
fn disconnect_keeps_the_remote_dimmed(cx: &mut TestAppContext) {
    // Disconnect -> status Disconnected, home's client_count() reaches 0, banner "Disconnected. Connect"; Connect
    // (TryNow) -> Live again.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum workspace::remote::
  workspace::tests::remote::` → FAIL.
- [ ] **Step 3: Implement.** Replace Task 3's placeholder failure sentence with `banner_words`' text.
- [ ] **Step 4: Run** the same → PASS; look at the banner in its states with `--dev` (stop the home instance, revoke,
  delete) or say so.
- [ ] **Step 5: Leave uncommitted.**

### Task 7: What a remote workspace doesn't do yet, never a relay, and the mirror's API state

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`on_remote`, `NOT_YET`), the entry points it guards (the
  toolbar, the add menus and drops, the close button and `close_block`, the Wire tool and wiring, the drawing tools,
  grid mode, Move to workspace, Copy and Paste of blocks, Stop, the Git popup), `crates/templum/src/workspace/
  control.rs` (`overview`, `control_instructions`, a workspace argument naming a remote), `api_state.rs` (`remote`),
  `crates/templum-api/src/model.rs` (`RemoteView`, `WorkspaceView::remote`), `src/openapi.rs`
- Test: `crates/templum/src/workspace/tests/remote.rs`, `crates/templum-api/src/model.rs`

**Interfaces:**
- Produces, on `Workspace`: `pub(crate) fn on_remote(&self) -> bool` (the shown workspace is remote) and `pub(crate)
  const NOT_YET: &str = "Not on a mirrored workspace yet";` Each guarded action does nothing on a remote workspace and
  its control shows disabled with `NOT_YET` as its tooltip.
- Never a relay: `find_block` finds no remote block (its `uids` are empty, Task 1–4); `overview` and the control
  server's instructions list a remote workspace as "API (mirror of winx-desktop)" with no blocks; a tool whose
  `workspace` argument names a remote workspace (by id or name) is refused "API is a mirror of winx-desktop: act on it
  there." — one check where control tools resolve their workspace argument; `api_terminals` lends no remote viewer
  (no uid); the mirror's own `share_workspace` refuses a remote workspace (plan 1a's "isn't a local one").
- Produces, in `templum_api::model`: `#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)] pub struct
  RemoteView { pub computer: Option<String>, pub host: String, pub way: String, pub status: String, pub latency_ms:
  Option<u64> }` and `WorkspaceView`'s `#[serde(default, skip_serializing_if = "Option::is_none")] pub remote:
  Option<RemoteView>` (`status` = `Status::word`); the OpenAPI schema `Remote`. The glue's `api_state` fills it for
  remote workspaces; their blocks are never in `blocks` (no uid).

- [ ] **Step 1: Write the failing tests:**

```rust
// workspace/tests/remote.rs
#[gpui_kit::test]
fn a_remote_workspace_refuses_what_it_cant_do_yet(cx: &mut TestAppContext) {
    // the remote shown with home's 3 blocks: adding a terminal (the toolbar's action), a note, a kanban: still 3
    // blocks; close_block on the viewer: still there; toggling grid mode: grid None; starting a wire: no wire;
    // copy then paste of a block: still 3; move_blocks to another workspace: refused (still 3); stop_space(r1):
    // not stopped; opening the Git popup: space.git None.
}

#[gpui_kit::test]
fn no_local_tool_sees_a_remote_block(cx: &mut TestAppContext) {
    // as the Templum Agent (control_call with Actor::Agent): overview's text has "API (mirror of winx-desktop)" and
    // none of home's block titles or ids; read_terminal { block: <home's terminal uid> } -> the "There's no block"
    // refusal; add_block { workspace: "r1", kind: "note" } -> "API is a mirror of winx-desktop: act on it there.";
    // the same through the mirror's own API with a token: GET /v1/state has workspace r1 with remote { status:
    // "live", .. } and no block of it; api_terminals() lends no viewer.
}

// model.rs
#[test]
fn a_remote_workspace_says_where_it_mirrors_from() {
    // a WorkspaceView with remote Some(RemoteView { computer: Some("winx-desktop"), host: "templum.example.com",
    // way: "public", status: "live", latency_ms: Some(84) }) serializes as the spec's example; None leaves "remote"
    // out; the state schema accepts both.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum workspace::tests::remote::a_
  remote_workspace_refuses workspace::tests::remote::no_local`, `cargo test -j 4 -p templum-api --lib model::
  openapi::` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same and `workspace::tests::control` (or the file holding `overview`'s tests) → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 8: Docs, minors, the full check and the benches

**Files:**
- Modify: `docs/specs/templum-to-templum.md` (Status: "Batches 1a–1c implemented"; "As built ▸ Batch 1c": the
  `RemoteSpace`, the index's `remote` fields as built, `Body::Mirrored`, Connect's failure shown in the dialog,
  `fit_zoom`'s rule, anything else chosen or changed), `docs/specs/workspaces.md` and `docs/specs/cockpit.md` (remote
  tiles, the Connect button, the banner), `docs/ARCHITECTURE.md` (`workspace/remote.rs`, `mirror.rs`,
  `connect_dialog.rs`; a remote workspace is a `Space` with `remote`, never saved), `docs/api.md` (the state's
  `remote` on a workspace, one line: plan 1d writes the section for people), `docs/minors.md`

**docs/minors.md** (`## Templum to Templum`), at least:
- Home's wire labels are drawn but can't be edited on the mirror. Fix: batch 2's wires.
- Anything else deferred while building.

- [ ] **Step 1: Write the docs** above.
- [ ] **Step 2: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 3: The benches** (painting changed), one after the other: `cargo run -j 4 --release -p templum --
  --bench`, then `-- --bench-mixed`: both `p95_ms` ≤ 16.7; report their JSON lines.
- [ ] **Step 4: Leave uncommitted;** report the files changed, the checks' and benches' results, and what went to
  docs/minors.md.

## Open for the orchestrator

1. **A Connect that can't reach home fails in the dialog** (and leaves nothing), rather than adding a remote that
   keeps reconnecting. *Recommended:* keep: the spec says a failure stays in the dialog; a person who pastes a code
   expects it to work now or say why.
2. **Don't connect on an existing remote's banner disconnects it** (it stays, dimmed, until Connect or Forget); in
   the Connect dialog it forgets the new remote. *Recommended:* keep.
