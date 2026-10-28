# Templum to Templum, Batch 1a (Home's Side: Shares and the API They Need) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer does the whole plan, then fresh Opus 5.5 reviewers
> review it in a loop until one passes it. The implementer **commits nothing**: everything stays uncommitted for the
> orchestrator.

**Goal:** Kanban card 171 part 2, batch 1, first of four plans: everything **home** gains. The API grows what a mirror
needs (`GET /v1/me`, `shares` and `share` events, a block's `read_only`, a workspace's `icon`, a frame's `modes`, the
full key names for `terminal_input`, the mirror's session and `event: replaced`, the audit log's grouping), the share
tools (`share_workspace`, `revoke_share`) make a token limited to one workspace and its **connection code**, and the
person shares from the workspace's Edit… dialog and sees who mirrors it.

**Architecture:** A share is an API token whose `share` field (workspace, way, address) is kept in `api.json` beside
it; its id is `s<token id>`, so revoking the token is revoking the share. The connection code is a new UI-free module,
`templum_api::code` (encode here, decode for the mirror in plan 1b). The share tools are API-own tools served on the
UI thread like `save_automation`; the Share… dialog calls the same `Workspace::share_workspace`. `GET /v1/me` is
answered by the server from the token alone (no UI round trip: a mirror polls it). The key names are parsed once, in
`templum_terminal::named_key`, for both `send_keys` and `terminal_input`.

**Tech Stack:** Rust 2024; `templum-api` (hyper 1, tokio, serde, base64, ring); `templum-terminal`
(`alacritty_terminal` 0.26); GPUI through gpui-kit 0.7.3 (pinned) for the glue. One dependency is added to
`templum-api`: `sysinfo` 0.31 with the same features `templum-terminal` already builds (no new package in
`Cargo.lock`).

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), whole; this plan builds its
"Batches ▸ Batch 1 ▸ Home" line and "What home's API gains". Read also [docs/specs/api.md](../specs/api.md) (its As
built sections) and [docs/api.md](../api.md). The code it builds on: `crates/templum-api/src/{store,model,registry,
openapi,server,events,audit,terminals,tls}.rs`, `crates/templum-api/tests/common/mod.rs`,
`crates/templum-terminal/src/{input,screen,terminal}.rs`, `crates/templum/src/workspace/{api,api_state,delegate}.rs`,
`crates/templum/src/{space_dialog,sidebar,spaces_screen}.rs`, `crates/templum/src/settings/api.rs` (its token dialog
and shown-once dialog are the models for the Share… dialog), and the tests in
`crates/templum/src/workspace/tests/api.rs` (`three_blocks`, `with_token`, `with_data`, `turn_on`, `port`, `tool`,
`api_call`). The batch 2 plan, [2026-10-24-api-batch-2.md](2026-10-24-api-batch-2.md), shows the expected level.

**Later plans consume this one:** 1b (the mirror's client) uses `code::{decode, Code, Key, Trust}`, `MeView`,
`ModesView`, `State`'s new fields and the `REPLACED` event; 1c and 1d use the share tools in the end-to-end test.

## Global Constraints

- **MEMORY LIMITS on this machine** (systemd-oomd has killed the desktop app several times): **every** cargo command
  takes `-j 4`; **never two cargo commands at once** (no background build while a test runs); while working, run only
  the targeted tests each step names (`cargo test -j 4 -p <crate> --lib <module>::`, `cargo test -j 4 -p templum-api
  --test <file> <name>`, `cargo test -j 4 -p templum --bin templum workspace::tests::<file>::<name>`); the full suite
  **once**, in Task 10.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit. `templum-api` and `templum-terminal` stay UI-free
  and are tested headlessly. No `unsafe`.
- **Dependencies:** no new package in `Cargo.lock`. `templum-api` gains `sysinfo = { version = "0.31",
  default-features = false, features = ["system"] }` (the comment: "Already in the tree (templum-terminal's): the
  computer's name `GET /v1/me` gives."). `cargo deny check` passes unchanged.
- **Every feature in the API** (CLAUDE.md): every new route is a row of `registry::ROUTES`, every new tool is in
  `registry::tools()` with its scope, so both are in `GET /v1/tools` and the OpenAPI document; every new state field is
  in `openapi.rs`'s schemas; docs/api.md gets their lines in Task 10.
- **The token never leaks:** a share's token text exists only in the tool's answer (`code`), the Share… dialog until
  Done, and the code. Never in `api.json` (only its hash), a log, the audit log, `Debug`, an error or the state.
  `code::Key`'s `Debug` and `Display` print `tpl_…`. Task 5 adds the test that no formatted value carries `tpl_`.
- **The UI thread never waits on the network;** painting is not slowed: nothing here runs per frame except reading
  `Server::mirrored()` (a lock held for a copy) where the sidebar draws.
- **Values, verbatim:** keys per `terminal_input` call 1 to 256 (`registry::INPUT_KEYS`); `send_keys` keeps 16
  (`tools::MAX_KEYS`) and `KEY_GAP` (50 ms) between each; `terminal_input` waits `KEY_GAP` only after an `escape` with
  no modifier that another key follows; a share's name at most 60 characters (`store::NAME_MOST`), default "Another
  computer"; expiry choices 1 day, 7 days, **30 days** (default), 90 days, 1 year, Never; the code's prefix `tplc1_`;
  the pin 64 lowercase hex characters.
- **Words, verbatim** (the spec's): "Shared API as s4." (the workspace's name, the share's id); "A pinned share needs
  HTTPS with Templum's certificate: Settings ▸ API ▸ HTTPS."; "The address must start with https://."; "Let another
  Templum mirror this workspace: watch it live, move its blocks, answer its agents, type into its terminals.";
  "Watch, move blocks, answer requests and type into terminals. Anyone with the code can run commands on this
  computer."; "Watch, move blocks and answer requests. No typing."; "Sharing needs the API. **Turn it on** (this
  computer only, port 7411)."; "Sharing needs the API. **Turn it on** for every network, with Templum's
  certificate"; "Directly needs the API on every network with Templum's certificate. **Apply**"; "Paste it in the other
  Templum: sidebar ▸ Connect to a Templum… Templum keeps only a fingerprint of its key: this code can't be shown
  again."; "Mirrored now on Another computer (203.0.113.7)"; "Clients that pinned the old one stop connecting, 2
  mirrors connected directly among them: share again for them."; "share of API", "share of a deleted workspace".
- **Every logic change has a test that fails without it.** UI-only changes: say how they were verified.
- **CI** runs Linux, macOS and Windows with no agent CLIs: platform tests and their imports behind `cfg(unix)` /
  `cfg(target_os)`; text of fixed width is clipped (`w_full` + `overflow_hidden` + `truncate`); no test touches HOME,
  the real keyring or the network beyond loopback; waits are conditions with a 10 s deadline, never one fixed sleep.
- **Don't open a Templum window against the person's real data:** `--dev` and headless tests only.
- **Minors** go to `docs/minors.md`, under `## Templum to Templum` (a new section after `## API`), each with where it
  is, what a person sees and a way to fix it. Docs lines ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **A share of a workspace that is then deleted at home:** its token sees nothing (`workspaces: []` in
   `GET /v1/me`), Settings ▸ API says "share of a deleted workspace", and `revoke_share` still revokes it. → Task 4
   `me_says_a_deleted_workspace_as_none`; Task 9 `a_shares_line_names_its_workspace_or_says_it_is_gone`.
2. **Two computers holding one code:** the second's event stream makes home send `event: replaced` on the first's
   streams and close the first's other connections; a token Settings made is never treated so. → Task 7
   `a_new_mirror_session_replaces_the_old_one` and `a_settings_token_keeps_its_streams`.
3. **A Control share calling `terminal_input`, or a share revoking another share:** `403`; a share revoking itself:
   `200`. → Task 6 `revoke_share_takes_the_callers_own_or_needs_full`.
4. **A morning of typing through a mirror:** `audit.log` keeps one `terminal_input` line per token and minute with
   `more`, and every other call still has its own line. → Task 7 `typing_is_one_audit_line_a_minute`.
5. **The code shown once:** after Done, neither the dialog, `Api` nor `Debug` of anything holds the token; the state and
   `GET /v1/shares` never carry `tpl_`. → Task 6 `a_share_answers_its_code_once`; Task 8 (Step 4's check).

---

### Task 1: Every key a keyboard has, by name

**Files:**
- Modify: `crates/templum-terminal/src/input.rs` (`NamedKey`, `named_key`, `KEY_NAMES`), `src/lib.rs` (export
  `NamedKey`)
- Modify: `crates/templum-api/src/registry.rs` (`INPUT_KEYS`, `terminal_input`'s parse, schema and description)
- Modify: `crates/templum/src/workspace/delegate.rs` (`press_each`'s gaps; callers take `.input()`)
- Test: `input.rs`, `registry.rs`, `crates/templum/src/workspace/tests/api.rs`

**Interfaces:**
- Produces, in `templum_terminal`:
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct NamedKey { pub key: String, pub key_char: Option<String>, pub
    ctrl: bool, pub alt: bool, pub shift: bool }` and `impl NamedKey { pub fn input(&self) -> KeyInput<'_> }`.
  - `pub fn named_key(name: &str) -> Option<NamedKey>` (was `Option<KeyInput<'_>>`): `[ctrl+][alt+][shift+]<key>`,
    the three prefixes in any order, each at most once. After them `<key>` is: `enter`, `escape`, `tab`, `backspace`,
    `delete`, `insert`, `up`, `down`, `left`, `right`, `home`, `end`, `page_up` (key `pageup`), `page_down` (key
    `pagedown`), `space` (key_char `" "`), `f1`–`f12`; or exactly one character that isn't a control character. A
    character's key is its lowercase (`to_lowercase()`, one char) and an uppercase one sets `shift`; its `key_char` is
    the character (uppercased when `shift+` was named) unless Ctrl or Alt is held, then None (so Ctrl+Alt+letter is
    never read as AltGr's text by `encode_key`). A named key's `key_char` is None except `space`'s. `"+"` alone is the
    character `+`; `"ctrl++"` is Ctrl and `+`. Anything else is None.
  - `KEY_NAMES` becomes: `"[ctrl+][alt+][shift+] and enter, escape, tab, backspace, delete, insert, up, down, left,
    right, home, end, page_up, page_down, space, f1–f12, or one character (2, y, A, ç)"`.
- Produces, in `templum_api::registry`: `pub const INPUT_KEYS: usize = 256;` — `terminal_input`'s `keys` hold 1 to
  `INPUT_KEYS` ("\"keys\" holds 1 to 256 keys."); its schema's `maxItems` and its description say 256 and "50 ms after
  an escape that another key follows". `send_keys` (control tool) keeps `MAX_KEYS`.
- Produces, in `delegate.rs`: `press_each` waits `KEY_GAP` before key `i > 0` when `!person` (send_keys: every key,
  as now) or when key `i - 1` is exactly `"escape"` (terminal_input). One private `fn gap_before(previous: &str,
  person: bool) -> bool`.

- [ ] **Step 1: Write the failing tests** (`input.rs`; `fields(n)` = `(key, key_char, ctrl, alt, shift)` of
  `named_key(n).unwrap()`; `bytes(n, mode)` = `encode_key(&named_key(n).unwrap().input(), mode)`):

```rust
#[test]
fn names_carry_every_modifier_in_any_order() {
    assert_eq!(fields("ctrl+shift+p"), ("p".into(), None, true, false, true));
    assert_eq!(named_key("shift+ctrl+p"), named_key("ctrl+shift+p"));
    assert_eq!(fields("A"), ("a".into(), Some("A".into()), false, false, true));
    assert_eq!(fields("Ç"), ("ç".into(), Some("Ç".into()), false, false, true));
    assert_eq!(fields("@"), ("@".into(), Some("@".into()), false, false, false));
    assert_eq!(fields("ctrl+alt+a"), ("a".into(), None, true, true, false));
    assert_eq!(fields("+"), ("+".into(), Some("+".into()), false, false, false));
    assert_eq!(fields("ctrl++"), ("+".into(), None, true, false, false));
    assert_eq!(fields("page_up").0, "pageup");
    for bad in ["ctrl+ctrl+p", "ctrl+", "f13", "F5", "\u{7}", "hyper+a", "ab", ""] {
        assert_eq!(named_key(bad), None, "{bad:?}");
    }
    // Today's names mean what they meant.
    assert_eq!(fields("shift+tab"), ("tab".into(), None, false, false, true));
    assert_eq!(fields("ctrl+c"), ("c".into(), None, true, false, false));
    assert_eq!(fields("space").1.as_deref(), Some(" "));
}

#[test]
fn new_names_reach_the_program_as_keys_pressed_at_home() {
    let plain = TermMode::empty();
    assert_eq!(bytes("f5", plain), Some(b"\x1b[15~".to_vec()));
    assert_eq!(bytes("shift+f5", plain), Some(b"\x1b[15;2~".to_vec()));
    assert_eq!(bytes("f1", plain), Some(b"\x1bOP".to_vec()));
    assert_eq!(bytes("alt+left", plain), Some(b"\x1b[1;3D".to_vec()));
    assert_eq!(bytes("ctrl+up", TermMode::APP_CURSOR), Some(b"\x1b[1;5A".to_vec()));
    assert_eq!(bytes("ctrl+alt+delete", plain), Some(b"\x1b[3;7~".to_vec()));
    assert_eq!(bytes("insert", plain), Some(b"\x1b[2~".to_vec()));
    assert_eq!(bytes("page_down", plain), Some(b"\x1b[6~".to_vec()));
    assert_eq!(bytes("alt+x", plain), Some(b"\x1bx".to_vec()));
    // Claude Code's Shift+Enter and Ctrl+Shift+letter, in the kitty keyboard protocol.
    let kitty = TermMode::DISAMBIGUATE_ESC_CODES;
    assert_eq!(bytes("shift+enter", kitty), Some(b"\x1b[13;2u".to_vec()));
    assert_eq!(bytes("ctrl+shift+p", kitty), Some(b"\x1b[112;6u".to_vec()));
    // A character with no modifier is text: the press path types it.
    assert_eq!(bytes("ç", plain), None);
}
```

  (`registry.rs`) extend `terminal_input_parses_text_or_keys`: 256 keys → `Ok`, 257 → `Err("\"keys\" holds 1 to
  256 keys.")`, `["ctrl+shift+f5"]` → `Ok`, `["f13"]` → `Err` starting `"keys[0] isn't a key Templum knows. Keys:
  [ctrl+]"`.

  (`workspace/tests/api.rs`) a new `#[gpui_kit::test] fn terminal_input_waits_only_after_an_escape`: three_blocks; a
  full token; `n` = the terminal's `term.inputs()`; `tool("terminal_input", {"block": t, "keys": ["a", "b",
  "escape", "c"]})`: before the clock moves `inputs() == n + 3` and no answer yet; advance the clock by `KEY_GAP` and
  `run_until_parked`: `inputs() == n + 4` and the answer is "Pressed 4 keys in <t>.". Then `send_keys`' existing tests
  must still pass (a gap before every key).
- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-terminal --lib input::` and
  `cargo test -j 4 -p templum-api --lib registry::` → compile errors / FAIL.
- [ ] **Step 3: Implement** `NamedKey`, the parser, `INPUT_KEYS`, and the gap rule; update the two callers of
  `named_key` in `delegate.rs` and the one in `registry.rs` (`.is_none()` keeps working).
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-terminal --lib`, `cargo test -j 4 -p templum-api --lib registry::`,
  then `cargo test -j 4 -p templum --bin templum workspace::tests::api::terminal_input` and
  `workspace::tests::delegate` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 2: A frame says what the program took (`modes`)

**Files:**
- Modify: `crates/templum-terminal/src/screen.rs` (`Screen::modes`), `crates/templum-api/src/model.rs`
  (`ModesView`, `FrameView::modes`), `src/terminals.rs` (`frame`), `src/openapi.rs` (`Frame`'s `modes`)

**Interfaces:**
- Produces, in `templum_terminal::Screen`: `pub modes: Modes` — `mouse`, `kitty_keys`, `full_screen` read from
  `term.mode()` under the same `try_lock_unfair` as the rows (`typing` is always false here: a mirror doesn't need it,
  and a change of it alone must not send a frame).
- Produces, in `templum_api::model`: `#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)] pub struct
  ModesView { pub full_screen: bool, pub mouse: bool, pub kitty_keys: bool }`, each `#[serde(default,
  skip_serializing_if = "is_false")]`; `FrameView`'s `#[serde(default)] pub modes: ModesView`. Every frame carries
  it; a frame whose only change is the modes has `rows: []`.

- [ ] **Step 1: Write the failing tests:**

```rust
// screen.rs
#[test]
fn the_screen_says_what_the_program_took() {
    // headless 20x4; screen().modes == Modes::default(); feed "\x1b[?1049h\x1b[?1000h\x1b[>1u":
    // modes == Modes { full_screen: true, mouse: true, kitty_keys: true, typing: false }.
}

// terminals.rs
#[test]
fn modes_ride_on_every_frame_and_a_change_alone_sends_one() {
    // a; feed "\x1b[?1000h"; b: frame(Some(&a), &b) is Some, whole false, rows [], modes.mouse true;
    // serde_json::to_value(&b's frame)["modes"] == json!({"mouse": true}); a frame with no mode has "modes": {}.
}

// openapi.rs: the_frame_schema_matches_a_frame keeps passing with "modes": {"full_screen": true}; a frame without
// "modes" passes too (older homes); "modes": {"blink": true} fails.
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-terminal --lib screen::`,
  `cargo test -j 4 -p templum-api --lib terminals::` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-terminal --lib screen::`, `cargo test -j 4 -p templum-api --lib`,
  `cargo test -j 4 -p templum-api --test terminals` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 3: A share in the store and in the state

**Files:**
- Modify: `crates/templum-api/src/store.rs` (`Way`, `Share`, `Token::share`, `Store::{mint_share, tunnel}`,
  `share_problem`), `src/model.rs` (`WorkspaceView::icon`, `BlockView::read_only`, `ShareView`, `LastView`,
  `MeView`, `State::shares`, `diff`, `visible`), `src/openapi.rs` (the schemas)

**Interfaces:**
- Produces, in `templum_api::store`:
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] #[serde(rename_all = "lowercase")] pub enum
    Way { Public, Pinned }`.
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] pub struct Share { pub workspace: String, pub way: Way,
    pub address: String }` (`address` as the person typed it: `https://templum.example.com` or `192.168.1.5:7411`).
  - `Token`'s `#[serde(default, skip_serializing_if = "Option::is_none")] pub share: Option<Share>`; `impl Token { pub
    fn share_id(&self) -> Option<String> }` → `Some(format!("s{}", self.id))` for a share. (Every `Token { … }` literal
    in the tests gains `share: None`.)
  - `Store`'s `#[serde(default, skip_serializing_if = "Option::is_none")] pub tunnel: Option<String>`: the last
    tunnel address a share used, the Share… dialog's default.
  - `impl Store { pub fn mint_share(&mut self, share: Share, name: &str, scope: Scope, expires: Option<u64>, now: u64)
    -> Result<(u64, String), String> }`: `mint` with `workspaces: Some(vec![share.workspace.clone()])`, then the
    token's `share`; a public share's address goes to `tunnel`. Answers the token's id and its text. A scope other than
    Control or Full is `Err("A share is Control or Full.")`.
  - `pub fn share_problem(settings: &Settings, way: Way, address: &str) -> Option<&'static str>`: what keeps a share
    from working when its code is pasted; None when it fits. Pinned: `settings.tls != Tls::Own` → "A pinned share
    needs HTTPS with Templum's certificate: Settings ▸ API ▸ HTTPS."; `listen == Loopback` → "Directly needs the API on
    every network with Templum's certificate." (*the planner's call*: the tool needs a sentence for it too; the
    dialog's button fixes both). Public: the address, trimmed, must start with `https://`, or `http://` followed by
    `127.0.0.1`, `[::1]` or `localhost` (then `:` or `/` or the end), else "The address must start with https://.".
- Produces, in `templum_api::model`:
  - `WorkspaceView`'s `#[serde(default)] pub icon: Option<String>` (a Lucide name).
  - `BlockView`'s `#[serde(default, skip_serializing_if = "is_false")] pub read_only: bool`.
  - `#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)] pub struct ShareView { pub id: String, pub
    workspace: String, pub name: String, pub scope: Scope, pub created: u64, pub expires: Option<u64>, pub way: Way,
    pub address: String, pub last: Option<LastView>, pub mirrored: bool }` (`Way` gets `#[derive(Default)]` with
    `#[default] Public`); `pub struct LastView { pub at: u64, pub addr: String }` (same derives).
  - `State`'s `#[serde(default)] pub shares: Vec<ShareView>`. `diff` compares them as `"share"` (id `s4`), after
    `"automation"`. `visible`: an unlimited caller keeps every share; a limited one keeps only the share whose id is
    `s<caller.token>`.
  - `#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)] pub struct MeView { pub name: String, pub
    scope: Scope, pub workspaces: Option<Vec<String>>, pub expires: Option<u64>, pub share: Option<String>, pub
    computer: String, pub version: String, pub api: u32 }` and `impl MeView { pub fn of(token: &Token, computer: &str)
    -> MeView }` (version `env!("CARGO_PKG_VERSION")`, `api` 1).
- Produces, in `openapi.rs`: schemas `Share`, `Me`; `State`'s `shares`, `Workspace`'s `icon`, `Block`'s
  `read_only`.

- [ ] **Step 1: Write the failing tests:**

```rust
// store.rs
#[test]
fn a_share_is_kept_with_its_token() {
    // mint_share(Share { workspace: "w3", way: Pinned, address: "192.168.1.5:7411" }, "Another computer", Full,
    // Some(100), 1): the token has workspaces Some(["w3"]), share Some(..), share_id Some("s1"); the text starts
    // "tpl_" and verify() finds it; save + load round-trips it; a file written before (no "share", no "tunnel")
    // loads with None; mint_share(.., Scope::Read, ..) is Err("A share is Control or Full."); a Public share's
    // address becomes store.tunnel.
}

#[test]
fn a_share_must_fit_the_api() {
    // settings Loopback + Off: share_problem(Pinned, "x") = the HTTPS sentence; Loopback + Own: the "Directly
    // needs…" sentence; All + Own: None. Public: "https://t.example.com/p" None, "http://127.0.0.1:9000" None,
    // "http://localhost" None, "http://[::1]:1" None, "http://10.0.0.2:7411" and "templum.example.com" and
    // "http://localhost.evil.com" Some("The address must start with https://.").
}

// model.rs
#[test]
fn a_limited_caller_sees_only_its_own_share() {
    // a state with shares s1 (w3) and s2 (w3): Caller { token: 2, workspaces: Some(["w3"]) } keeps only s2;
    // Caller { token: 7, workspaces: Some(["w3"]) } keeps none; an unlimited caller keeps both.
}

#[test]
fn shares_are_on_the_stream_as_share_events() {
    // diff(a, b) where b adds s2 and changes s1.mirrored: two Delta { kind: "share", .. } puts; removing s1: a
    // delete with id "s1".
}

#[test]
fn new_fields_serialize_only_when_they_say_something() {
    // a BlockView with read_only false has no "read_only" key; true has "read_only": true; a WorkspaceView with
    // icon None serializes "icon": null; a State JSON without "shares" (an older home) deserializes with [].
}

// openapi.rs: the_state_schema_matches_the_state with a full_state() that has a share, an icon and a read_only
// block; check(&r("Me"), MeView::of(..)) is Ok.
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --lib store:: model:: openapi::` →
  compile errors.
- [ ] **Step 3: Implement.** Fix the `Token { … }` and `BlockView`/`WorkspaceView` literals the compiler names.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api --lib` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 4: Home answers `GET /v1/me` and `GET /v1/shares`, and fills the state

**Files:**
- Modify: `crates/templum-api/Cargo.toml` (`sysinfo`), `src/server.rs` (`Options::computer`, the `Me` route,
  `Server::mirrored`, telling the UI when an event stream opens or ends), `src/registry.rs` (`Does::Me`,
  `Resource::Shares`, their routes), `src/openapi.rs` (their operations)
- Modify: `crates/templum/src/workspace/api_state.rs` (`icon`, `read_only`, `shares`)
- Test: `crates/templum-api/tests/server.rs`, `tests/events.rs`, `crates/templum/src/workspace/tests/api.rs`

**Interfaces:**
- Consumes: Task 3.
- Produces, in `templum_api::server`: `Options`' `pub computer: String`, set in `Options::from` to
  `sysinfo::System::host_name().unwrap_or_default()` (tests' options may set anything).
- Produces: `route("GET", "/v1/me", Me)` (`Does::Me`, scope read): answered by `dispatch` from the token and
  `options.computer` with `MeView::of`, without asking the UI thread. `route("GET", "/v1/shares", Read(R::Shares))`:
  the state's `shares`, filtered as the state is.
- Produces: `impl Server { pub fn mirrored(&self) -> Vec<(u64, String)> }`: each token that holds an open event
  stream now, with the address of that stream's connection (`Conn::addr`), sorted by token id. An event stream's
  opening and its end each send the UI `CLIENTS` (`tell_ui`), as a client joining does, so the sidebar's marks and the
  Edit… dialog's rows change at once.
- Produces, in the glue's `api_state`: `WorkspaceView::icon` = `space.look.icon` as `space_dialog::icon_name`;
  `BlockView::read_only` = the terminal's `pane.follows.is_some()`; `State::shares` = one `ShareView` per token with a
  `share` (its id, the share's workspace even when that workspace is gone, name, scope, created, expires, way,
  address; `last` = the server's newer use, else the token's; `mirrored` = the token is in `server.mirrored()`).

- [ ] **Step 1: Write the failing tests:**

```rust
// tests/server.rs
#[test]
fn me_says_who_the_token_is_without_asking_the_ui() {
    // a token named "phone", Read, workspaces Some(["w3"]), expires Some(T); options.computer = "winx-desktop".
    // GET /v1/me -> 200 {"name":"phone","scope":"read","workspaces":["w3"],"expires":T,"share":null,
    // "computer":"winx-desktop","version":<CARGO_PKG_VERSION>,"api":1}; fake.asked() has no "templum/state".
    // A share token (share: Some(..), id 4) -> "share":"s4". An unlimited token -> "workspaces": null.
}

#[test]
fn me_says_a_deleted_workspace_as_none() {
    // the share token's workspaces set to Some([]) through server.set_tokens -> GET /v1/me "workspaces": [].
}

#[test]
fn shares_are_a_resource() {
    // the fake's state has shares s1, s2; an unlimited token's GET /v1/shares -> both; token 2 limited to w3 ->
    // only s2. GET /v1/openapi.json has "/v1/me" and "/v1/shares" with x-templum-scope "read".
}

// tests/events.rs
#[test]
fn mirrored_says_which_tokens_hold_an_event_stream() {
    // no stream: mirrored() == []; token 1 opens /v1/events -> wait_for mirrored() == [(1, "127.0.0.1")]; the
    // fake saw a CLIENTS notification after the open; drop the stream -> wait_for mirrored() == [] and one more
    // CLIENTS notification.
}

// workspace/tests/api.rs
#[gpui_kit::test]
fn the_state_has_icons_read_only_blocks_and_shares(cx: &mut TestAppContext) {
    // the shown workspace's look.icon = Some(IconName::Rocket) -> its WorkspaceView.icon == Some("rocket");
    // a subagent's viewer pane (follows Some) -> read_only true, the others false; a share minted with
    // store.mint_share(..) -> state.shares == [ShareView { id: "s<id>", workspace, name, scope, way, address,
    // mirrored: false, .. }]; no field of the state's JSON contains "tpl_".
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --test server me_`,
  `--test server shares_are`, `--test events mirrored`, `cargo test -j 4 -p templum --bin templum
  workspace::tests::api::the_state_has_icons` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests of Step 2, `cargo test -j 4 -p templum-api --lib registry:: openapi::` and
  `cargo test -j 4 -p templum-api --test events` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 5: The connection code

**Files:**
- Create: `crates/templum-api/src/code.rs`
- Modify: `crates/templum-api/src/lib.rs` (`pub mod code;`), `src/tls.rs` (`pin`)

**Interfaces:**
- Produces, in `templum_api::code` (the spec's "The connection code"; *the planner's call* for the names):
  - `pub const PREFIX: &str = "tplc1_";`
  - `#[derive(Clone, PartialEq, Eq)] pub struct Key(String)`: a token's text. `Key::new(text: String) -> Key`,
    `fn expose(&self) -> &str` (the only way to the text: its callers write it into a header, the keyring or a code).
    `Debug` and `Display` print `tpl_…`. No `Serialize`.
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub enum Trust { Pinned(String), Public, Loopback }` (`Pinned` holds the
    64 lowercase hex characters).
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub struct Code { pub url: String, pub key: Key, pub trust: Trust }`, `url`
    without a trailing `/` (the mirror calls `<url>/v1/…`).
  - `pub fn encode(url: &str, key: &Key, pin: Option<&str>) -> String`: `PREFIX` + base64url (no padding) of
    `{"url","token","pin"}` (`pin` left out when None).
  - `pub fn decode(text: &str) -> Result<Code, Wrong>`, the text trimmed. `#[derive(Clone, Copy, Debug, PartialEq,
    Eq)] pub enum Wrong { NotACode, CutShort, Newer, Plain }` with `impl Display` giving, in order: "This isn't a
    connection code: one starts with tplc1_.", "The code is cut short: copy it again whole.", "This code is from a
    newer Templum: update this one.", "This code's address isn't encrypted: Templum connects in plain HTTP only to this
    computer itself." Rules: `tplc<n>_` with `n > 1` is Newer; no `tplc1_` is NotACode; bad base64, bad JSON, a
    missing `url` or `token`, a token not starting with `tpl_`, a pin that isn't 64 hex characters, or a `url` that
    `hyper::Uri` can't read with a scheme and a host, is CutShort; unknown JSON fields are ignored. The trust is read
    from the code, never guessed: a pin → `Pinned` (its `url` must be `https`, else CutShort); `https` → `Public`;
    `http` to `127.0.0.1`, `::1` or `localhost` → `Loopback`; other `http` → Plain.
  - `impl Code { pub fn host(&self) -> String }` (host and port as the url has them: `templum.example.com`,
    `192.168.1.5:7411`) and `pub fn said(&self) -> String`: "templum.example.com · checks its public certificate",
    "192.168.1.5:7411 · pins Templum's certificate AB:CD:…:9F" (its first two and last pairs, uppercase), "127.0.0.1:7411
    · plain HTTP to this computer".
- Produces, in `tls.rs`: `pub fn pin(cert: &CertificateDer) -> String`: the SHA-256 in 64 lowercase hex characters
  (`fingerprint` without colons, lowercase).

- [ ] **Step 1: Write the failing tests** (`code.rs`):

```rust
#[test]
fn a_code_round_trips() {
    let key = Key::new("tpl_abc".into());
    let pin = "ab".repeat(32);
    let text = encode("https://192.168.1.5:7411", &key, Some(&pin));
    assert!(text.starts_with("tplc1_") && !text.contains('='));
    assert_eq!(decode(&format!("  {text}\n")), Ok(Code { url: "https://192.168.1.5:7411".into(), key: key.clone(),
        trust: Trust::Pinned(pin.clone()) }));
    // https://templum.example.com/base/ -> url "https://templum.example.com/base", Trust::Public;
    // http://127.0.0.1:7411 and http://localhost:9 and http://[::1]:9 -> Loopback.
}

#[test]
fn a_wrong_code_says_why() {
    // "hello" -> NotACode; text[..text.len() - 9] -> CutShort; "tplc2_xxx" -> Newer; a code for
    // "http://192.168.1.5:7411" -> Plain; a pinned code with url "http://…" -> CutShort; {"url":..,"token":"abc"}
    // -> CutShort; a JSON with an extra "name" field decodes.
}

#[test]
fn a_code_says_where_it_goes() {
    // Public: said() == "templum.example.com · checks its public certificate"; Pinned with pin "abcd…9f":
    // "192.168.1.5:7411 · pins Templum's certificate AB:CD:…:9F".
}

#[test]
fn the_key_never_shows() {
    let code = decode(&encode("https://t.example.com", &Key::new("tpl_SECRET".into()), None)).unwrap();
    for shown in [format!("{code:?}"), format!("{}", code.key), format!("{:?}", code.key)] {
        assert!(!shown.contains("SECRET"), "{shown}");
    }
    assert_eq!(code.key.expose(), "tpl_SECRET");
}

// tls.rs
#[test]
fn the_pin_is_the_fingerprint_in_lowercase_hex() {
    // pin(&c) == fingerprint(&c).replace(':', "").to_lowercase(), 64 characters.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --lib code:: tls::` → compile errors.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api --lib code:: tls::` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 6: The share tools

**Files:**
- Modify: `crates/templum-api/src/registry.rs` (`OWN`, `tools()`, `scope`, `nominal`, `Own`, `parse_own`, `TARGETS`/
  `NOT_TARGETS`, the aliases), `src/server.rs` (`tool_answer` passes `code` on), `src/openapi.rs`
- Modify: `crates/templum/src/workspace/api.rs` (`share_workspace`, `revoke_share`, their `api_own` arms)
- Test: `registry.rs`, `crates/templum-api/tests/server.rs`, `crates/templum/src/workspace/tests/api.rs`

**Interfaces:**
- Consumes: Task 3 (`mint_share`, `share_problem`, `Share`, `Way`), Task 5 (`code::encode`, `Key`, `tls::pin`).
- Produces, in `registry.rs`:
  - `"share_workspace"` and `"revoke_share"` in `OWN` and `tools()`. `share_workspace`: `workspace` (string, required),
    `name` (string, required), `scope` (`enum ["control","full"]`, required), `expires` (integer or null), `way` (`enum
    ["public","pinned"]`, required), `address` (string, required); description: "Shares a workspace with another
    Templum, as its Edit… dialog does: a token limited to it and the connection code, given once in the answer's
    code field." `revoke_share`: `share` (string; omitted: the calling token's own); description "Revokes a share: its
    mirror is cut at once."
  - `scope("share_workspace", _) == Ok(Scope::Full)`; `scope("revoke_share", _) == Ok(Scope::Read)` (any share may
    revoke itself; the glue asks Full of one revoking another); `nominal("revoke_share") == "full, or the share
    itself"`.
  - `Own::ShareWorkspace { workspace: String, name: String, scope: Scope, expires: Option<u64>, way: Way, address:
    String }`, `Own::RevokeShare { share: Option<String> }`, parsed with the parser's sentences ("\"scope\" is control
    or full.", "\"way\" is public or pinned.", "\"expires\" is a whole number or null.").
  - `"share"`, `"scope"`, `"expires"`, `"way"`, `"address"` in `NOT_TARGETS` (`workspace` is already a target, so a
    limited token can't name another workspace).
  - Aliases: `route("POST", "/v1/workspaces/{workspace}/shares", Tool("share_workspace"))`,
    `route("DELETE", "/v1/shares/{share}", Tool("revoke_share"))`.
- Produces, in `server.rs`: `tool_answer` copies `result["code"]`, when it is a string, into the answer's `code`.
- Produces, in the glue (`workspace/api.rs`):
  - `pub(crate) struct ShareDraft { pub workspace: String, pub name: String, pub scope: Scope, pub expires:
    Option<u64>, pub way: Way, pub address: String }`.
  - `pub(crate) fn share_workspace(&mut self, draft: ShareDraft, cx: &mut Context<Self>) -> Result<(String, String),
    String>`: the share's id and its code. Checks, in order: a workspace that isn't a local one (a later plan's remote
    workspace too) → `registry::no("workspace", id)`; `share_problem` → its sentence; the url: public → the address
    trimmed, a trailing `/` dropped; pinned → `https://<address>` (an address with a scheme is refused "Give Directly's
    address as host:port."); the pin = `tls::pin` of Templum's certificate (`tls::own(data)`); `mint_share`;
    `tokens_changed`. The code is built with `code::encode` from the minted text, never kept.
  - `api_own`'s `Own::ShareWorkspace` arm: a caller limited to some workspaces → `Err((codes::SCOPE,
    "share_workspace needs a Full token for all workspaces."))`; then `share_workspace`; the answer line is
    `answer_line` plus `result.code` and `result.target.workspace`: text `"Shared {name} as {id}."` with the
    workspace's name.
  - `Own::RevokeShare { share }`: None → the caller's own token must be a share (else `params("revoke_share needs
    \"share\": this token isn't a share.")`); `Some(s)` other than `s<caller.token>` → needs a Full caller for all
    workspaces (else `codes::SCOPE` "revoke_share needs a Full token for all workspaces."), and a share that doesn't
    exist is `not_found("share", s)` (`registry::no`'s path for `share` is `/v1/shares`); then `revoke_token` (its
    streams end through `set_tokens`); text "Revoked s4.". A share revoking itself answers before its streams end.

- [ ] **Step 1: Write the failing tests:**

```rust
// registry.rs
#[test]
fn the_share_tools_parse_and_route() {
    // parse_own("share_workspace", {workspace:"w3",name:"x",scope:"full",expires:null,way:"public",address:"https://t"})
    // == Own::ShareWorkspace{..}; scope "read" -> Err("\"scope\" is control or full."); way "lan" -> Err; find("POST",
    // "/v1/workspaces/w3/shares") -> Tool("share_workspace"), [("workspace","w3")]; find("DELETE","/v1/shares/s4")
    // -> Tool("revoke_share"), [("share","s4")]; scope("share_workspace", ..) == Full; scope("revoke_share", ..) == Read.
}

// workspace/tests/api.rs
#[gpui_kit::test]
fn a_share_answers_its_code_once(cx: &mut TestAppContext) {
    // with_data + turn_on with settings All + Tls::Own; a Full token for all workspaces. tool("share_workspace",
    // {workspace: <first id>, name: "Another computer", scope: "full", expires: null, way: "pinned", address:
    // "192.168.1.5:7411"}) -> text "Shared <name> as s<id>.", result.code decodes (code::decode) to url
    // "https://192.168.1.5:7411", Trust::Pinned(tls::pin of Templum's certificate), and its key verifies against the
    // store as a token with workspaces Some([first id]) and share Some(..). The store's file has no "tpl_"; the state
    // has one share and no "tpl_"; ws.api.shown is None.
    // Refusals (isError, a 422 over HTTP): way "pinned" with Tls::Off -> "A pinned share needs HTTPS with Templum's
    // certificate: Settings ▸ API ▸ HTTPS."; way "public" with address "templum.example.com" -> "The address must
    // start with https://.". Errors: a token limited to the first workspace -> SCOPE "share_workspace
    // needs a Full token for all workspaces."; a Control token -> SCOPE "share_workspace needs a Full token.".
}

#[gpui_kit::test]
fn revoke_share_takes_the_callers_own_or_needs_full(cx: &mut TestAppContext) {
    // two shares s<a> (Control) and s<b> (Full) of the first workspace, and a Full token for all workspaces.
    // As s<a>: revoke_share {} -> "Revoked s<a>.", the store no longer has token a. As s<b>: revoke_share
    // {"share": "s<c>"} for a third share -> SCOPE "revoke_share needs a Full token for all workspaces.". As the
    // Full token: {"share": "s<b>"} -> "Revoked s<b>."; {"share": "s999"} -> NOT_FOUND "There's no share s999:
    // GET /v1/shares lists them.". A Settings token (no share) calling revoke_share {} -> PARAMS
    // "revoke_share needs \"share\": this token isn't a share.".
}

// tests/server.rs
#[test]
fn a_tools_code_reaches_the_answer() {
    // the fake answers tools/call share_workspace with result {"content":[{"type":"text","text":"Shared API as s4."}],
    // "code":"tplc1_x"}: POST /v1/workspaces/w3/shares {..} with a Full token -> 200 {"text":"Shared API as s4.",
    // "code":"tplc1_x"}; audit.log has "share_workspace" and not "tplc1_x".
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --lib registry::`,
  `cargo test -j 4 -p templum-api --test server a_tools_code`, `cargo test -j 4 -p templum --bin templum
  workspace::tests::api::a_share workspace::tests::api::revoke_share` → FAIL.
- [ ] **Step 3: Implement.** (The fake in `tests/common/mod.rs` answers `tools/call share_workspace` with
  `"code": "tplc1_x"` beside its text, so the server test has something to pass on.)
- [ ] **Step 4: Run** the tests of Step 2 and `cargo test -j 4 -p templum-api --lib` → PASS
  (`every_argument_is_a_target_or_not` and `the_listed_scopes_are_the_nominal_ones` included).
- [ ] **Step 5: Leave uncommitted.**

### Task 7: The mirror's session, `replaced`, and the audit log's grouping

**Files:**
- Modify: `crates/templum-api/src/server.rs` (`MIRROR_HEADER`, `Conn::session`, replacing), `src/events.rs` (`Hub`'s
  streams of a connection; the `replaced` frame), `src/terminals.rs` (`Screens::end_on`), `src/audit.rs` (chatter),
  `src/lib.rs` (`REPLACED`)
- Test: `crates/templum-api/tests/events.rs`, `src/audit.rs`

**Interfaces:**
- Produces, in `templum_api`: `pub const MIRROR_HEADER: &str = "templum-mirror";` and `pub const REPLACED: &str =
  "replaced";` (the event's name; its `data` is `{}`).
- Produces, in `server.rs`: `Conn`'s `session: Option<String>`, set from each authenticated request's
  `Templum-Mirror` header (at most 64 characters, else ignored). When a token that `share`s opens `GET /v1/events`
  with a session S (its connection's): before the stream is opened, every event stream of that token on a connection
  whose session isn't S gets the frame `event: replaced` and ends (`Cell::push` then `Cell::end`, so the frame is sent
  before the body ends); its terminal streams on such connections end; every other connection of that token whose
  session isn't S is aborted. A token without a `share` keeps part 1's rules.
- Produces, in `audit.rs`: `const CHATTER: [&str; 2] = ["terminal_input", "/v1/me"];` a call line whose `what` is one
  of them is counted per (token, what) in a window of `WINDOW` (a minute), like noise: the first goes to `audit.log` at
  once, the rest are counted and one line with `more` is written when the window ends; it uses the noise path's
  table (capped at `COUNTED`) and its queue rules, but writes to `audit.log`. Every other call keeps its own line.

- [ ] **Step 1: Write the failing tests:**

```rust
// tests/events.rs (a share token: Token { share: Some(Share { .. }), .. })
#[test]
fn a_new_mirror_session_replaces_the_old_one() {
    // A: open /v1/events with "Templum-Mirror: one" -> snapshot. B: open /v1/events with "Templum-Mirror: two"
    // -> 200 snapshot; A's next event is "replaced" with data {}, then A has ended; B is still open (a publish
    // reaches B). A third request with session "one" on a new connection is served (it is a new call, not a stream).
}

#[test]
fn a_settings_token_keeps_its_streams() {
    // the same with a token whose share is None: both streams stay open; no "replaced".
}

// audit.rs
#[test]
fn typing_is_one_audit_line_a_minute() {
    // Audit::with_caps(dir, AUDIT_CAP, NOISE_CAP, 100 ms): write 50 "terminal_input" lines and 5 "/v1/me" lines for
    // token "laptop" and one "move_block" line; after the window (wait_for, deadline 10 s) audit.log has 1 + 1
    // "terminal_input" lines (the first, then one with "more": 49), 1 + 1 "/v1/me" lines (more 4), the "move_block"
    // line, and noise.log has none of them.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --test events replace`,
  `--test events settings_token`, `cargo test -j 4 -p templum-api --lib audit::` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api --lib`, `--test events` twice, `--test terminals`,
  `--test server` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 8: Sharing from the Edit… dialog

**Files:**
- Create: `crates/templum/src/share_dialog.rs` (the Share… dialog, and the code shown once)
- Modify: `crates/templum/src/space_dialog.rs` (the Sharing part), `crates/templum/src/main.rs` (`mod share_dialog;`),
  `crates/templum/src/workspace/api.rs` (`share_rows`, `fit_api`, `this_address`)
- Test: `crates/templum/src/workspace/tests/api.rs`

**Interfaces:**
- Consumes: Task 6 (`ShareDraft`, `share_workspace`), Task 3 (`share_problem`), Task 4 (`ShareView`s in the state).
- Produces, in the glue:
  - `pub(crate) fn share_rows(&self, space: &str) -> Vec<ShareView>`: the shares of workspace `space`, oldest first
    (from the same builder as the state's).
  - `pub(crate) fn fit_api(&mut self, way: Way, cx: &mut Context<Self>)`: the dialog's one button. API off → on
    (`enabled`), and for `Pinned` `listen = All`, `tls = Own`; on but Pinned misfit → `listen = All`, `tls = Own`
    (both through `api_edit`).
  - `pub(crate) fn this_address() -> Option<std::net::IpAddr>`: this computer's address on its main network: a UDP
    socket bound to `0.0.0.0:0`, `connect("1.1.1.1:9")` (no packet is sent), its `local_addr`; None on any error or
    a loopback answer (*the planner's call*: std only).
  - `pub(crate) fn sharing_words(share: &ShareView, now: u64) -> (String, String)`: the row's two dim lines: `"Full ·
    expires in 29 days"` (`settings::api::expiry_words`, "never expires", "expired 2 days ago"), and `"mirrored now from
    203.0.113.7"` / `"last used 3 h ago from 203.0.113.7"` (`used_words`) / `"never used"`.
- The Sharing part (below Folder, `label("Sharing")`), applied at once, not on Save: with no share, the sentence "Let
  another Templum mirror this workspace: watch it live, move its blocks, answer its agents, type into its terminals."
  and **Share…**; else one row per share (name; under it, dim, `sharing_words`, the expired line in yellow, the accent
  dot before "mirrored now"); **Revoke** on the right, at once (`revoke_token`). The dialog reads the rows from the
  workspace entity each render, so a mirror connecting shows at once.
- **Share…** opens a second dialog over the Edit… one (gpui-kit stacks dialogs: `Root::active_dialogs`):
  - Name (one line, required, at most 60 characters, default "Another computer"), Can (Full, default, with its
    sentence; Control, with its sentence), Expires (In 1 day, 7 days, 30 days default, 90 days, 1 year, Never), The
    other computer reaches this one (Through a tunnel or proxy, with Address default `store.tunnel` else empty with the
    placeholder `https://templum.example.com`; Directly, with Address default `"<this_address>:<port>"`). The way's
    default: `Tls::Own` and a listen other than Loopback pick Directly; anything else Through a tunnel.
  - Under the way: `share_problem`'s misfit in the spec's words with its one button (**Turn it on** / **Apply** →
    `fit_api`), or the address sentence (no button). **Make code** is disabled while a problem shows or the name is
    empty.
  - **Make code** → `share_workspace`; an Err shows under the fields; Ok replaces the dialog's content with the code,
    whole, in the terminal's font (clipped: `w_full` + `overflow_hidden`, wrapping by characters), **Copy**, and the
    sentence "Paste it in the other Templum: sidebar ▸ Connect to a Templum… Templum keeps only a fingerprint of its
    key: this code can't be shown again." **Done** closes it and drops the text (the view's `code: Option<String>` set
    to None before the dialog closes; nothing else holds it).

- [ ] **Step 1: Write the failing tests** (`workspace/tests/api.rs`):

```rust
#[gpui_kit::test]
fn the_sharing_rows_say_who_mirrors_and_when(cx: &mut TestAppContext) {
    // two shares of the first workspace and one of another; share_rows(first) has the two, oldest first.
    // sharing_words(ShareView { scope: Full, expires: Some(now + 29 d), last: None, mirrored: false, .. }, now) ==
    // ("Full · expires in 29 days", "never used"); mirrored true with last addr "203.0.113.7" -> second line
    // "mirrored now from 203.0.113.7"; Control, expires None, last 3 h ago -> ("Control · never expires",
    // "last used 3 h ago from 203.0.113.7").
}

#[gpui_kit::test]
fn the_dialogs_button_makes_the_api_fit(cx: &mut TestAppContext) {
    // with_data, the API off: fit_api(Public) -> enabled, listen Loopback, tls Off, a server listens;
    // fit_api(Pinned) -> listen All, tls Own; share_problem(.., Pinned, ..) is then None.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum
  workspace::tests::api::the_sharing workspace::tests::api::the_dialogs` → compile errors.
- [ ] **Step 3: Implement** the helpers, the Sharing part and the dialog.
- [ ] **Step 4: Run** the tests → PASS. Then **look at it**: `cargo run -j 4 -p templum -- --dev`, open a workspace's
  Edit…, Share…, try both ways with the API off and on, make a code, Copy, Done, see the row, Revoke. Say what each
  showed; with no display at hand, say so. Check by a test-only read that after Done no `Entity` of the dialog holds
  the code (the view's `code` is None).
- [ ] **Step 5: Leave uncommitted.**

### Task 9: Who mirrors: marks, Settings ▸ API, New certificate

**Files:**
- Modify: `crates/templum/src/sidebar.rs` (the cast mark), `crates/templum/src/spaces_screen.rs` (the card's mark),
  `crates/templum/src/settings/api.rs` (a share's line; New certificate's question)
- Test: `crates/templum/src/settings/api.rs` (unit), `crates/templum/src/workspace/tests/api.rs`

**Interfaces:**
- Consumes: Task 4 (`Server::mirrored`, `ShareView`), Task 3.
- Produces, in the glue: `pub(crate) fn mirrored_now(&self, space: &str) -> Vec<(String, String)>`: the names and
  addresses of the shares of `space` mirrored now, from the server (no state built: it runs where the sidebar draws).
- Produces, in `settings/api.rs`: `pub(crate) fn share_line(token: &Token, names: &HashMap<String, String>) ->
  Option<String>` → `"share of API"` (the workspace's name) or `"share of a deleted workspace"` (the share's workspace
  isn't among them), None for a token without a share; `pub(crate) fn renew_words(direct: usize) -> String` →
  "Clients that pinned the old one stop connecting." for 0, "Clients that pinned the old one stop connecting, 1 mirror
  connected directly among them: share again for it." for 1, the spec's sentence for 2 and more (`direct` = live
  tokens with a `Pinned` share).
- A tile of a workspace mirrored now: Lucide `cast` at its bottom-right (12 px, the accent), tooltip "Mirrored now on
  Another computer (203.0.113.7)" (several: one line each). The workspaces screen's card: the same mark and tooltip.
- Settings ▸ API's token row: its second line starts with `share_line` when Some, instead of its workspaces.

- [ ] **Step 1: Write the failing tests:**

```rust
// settings/api.rs
#[test]
fn a_shares_line_names_its_workspace_or_says_it_is_gone() {
    // a token with share workspace "w3" and names {"w3": "API"} -> Some("share of API"); names {} -> Some("share of a
    // deleted workspace"); share None -> None.
}

#[test]
fn new_certificate_counts_the_direct_mirrors() {
    // renew_words(0), (1), (2) as above.
}

// workspace/tests/api.rs
#[gpui_kit::test]
fn a_mirrored_workspace_is_named_with_its_mirror(cx: &mut TestAppContext) {
    // with_data + turn_on; a share of the first workspace; a thread opens its /v1/events (raw GET with the token,
    // as the_tick_lends_terminals_while_a_terminal_stream_is_open does); pump until mirrored_now(first) ==
    // [("Another computer", "127.0.0.1")]; mirrored_now(other) == []; drop the stream: pump until [].
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum --bin templum settings::api::` and
  `workspace::tests::api::a_mirrored` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the tests → PASS. Look at the mark with `--dev` and a `curl -N` on `/v1/events` with a share's
  token (or say there was no display).
- [ ] **Step 5: Leave uncommitted.**

### Task 10: Docs, minors, the full check and the benches

**Files:**
- Modify: `docs/api.md`, `docs/specs/templum-to-templum.md` (Status: "Batch 1a (home's side) implemented"; a new
  "As built ▸ Batch 1a" section with what the builder chose or changed), `docs/specs/api.md` (its Templum to Templum
  section: "home's side built, plan 2026-10-25-t2t-batch-1a-home.md"), `docs/specs/settings.md` (shares among the
  tokens; New certificate's count), `docs/specs/workspaces.md` and `docs/specs/cockpit.md` (the Edit… dialog's
  Sharing, the cast mark), `docs/ARCHITECTURE.md` (`templum-api`'s row: `code.rs`; the glue's line: shares and
  `share_dialog.rs`), `docs/minors.md`

**docs/api.md:**
1. **Resources:** `GET /v1/me` (who the token is: the spec's example), `GET /v1/shares`. **Tools:** `share_workspace`
   and `revoke_share` rows with their scopes; **Aliases:** `POST /v1/workspaces/{workspace}/shares`,
   `DELETE /v1/shares/{share}`.
2. **The state:** `shares` (the spec's example; a limited token sees only its own), a block's `read_only`, a
   workspace's `icon`; **Events:** the `share` event; `event: replaced` and the `Templum-Mirror` header (a share
   token's new session ends the others').
3. **Terminals, live:** a frame's `modes`; `terminal_input`'s keys: the full names (the spec's "A key's name" list
   and examples), 1 to 256 per call, 50 ms only after an Esc that another key follows; `send_keys` keeps 16.
4. **The audit log:** `terminal_input` and `GET /v1/me` grouped per token and minute with `more`.

**docs/minors.md**, a new `## Templum to Templum` section after `## API`; at least:
- `revoke_share` lists as "full, or the share itself" while its registry scope is read (the glue checks the rest).
  Fix: a scope that depends on the caller, in `registry::scope`.
- A shifted symbol with Ctrl or Alt reaches the kitty protocol with the symbol as its key (the spec's known limit).
  Fix: names carry the unshifted key too.
- Anything else deferred while building.

- [ ] **Step 1: Write the docs** above.
- [ ] **Step 2: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 3: The benches** (this touches `templum-terminal`), one after the other:
  `cargo run -j 4 --release -p templum -- --bench`, then `cargo run -j 4 --release -p templum -- --bench-mixed`: both
  `p95_ms` ≤ 16.7; report their JSON lines (no data folder, so no API: say so).
- [ ] **Step 4: Leave uncommitted;** report the files changed, the checks' and benches' results, and what went to
  docs/minors.md.

## Open for the orchestrator

1. **A pinned share while the API listens only on loopback** is refused by the tool with "Directly needs the API on
   every network with Templum's certificate." (the spec lists only two refusals). *Recommended:* keep: a code that
   can't connect must not be made.
2. **`revoke_share`'s registry scope is Read**, so a Control or Read share can revoke itself; the glue asks Full of
   one revoking another. *Recommended:* keep (minor noted).
