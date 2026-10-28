# Templum to Templum, Batch 1b (The Mirror's Client) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer does the whole plan, then fresh Opus 5.5 reviewers
> review it in a loop until one passes it. The implementer **commits nothing**: everything stays uncommitted for the
> orchestrator.

**Goal:** Kanban card 171 part 2, batch 1, second of four plans: the **mirror's client**, UI-free, in `templum-api`
("Templum's HTTP API, both ends"). One thread holds every remote: it reaches home through both ways in (a pinned
certificate, or the web's authorities with the private authority check), through an optional proxy, with the key from
the system's keyring (or a file, or memory); it follows home's state live, streams the terminals the UI asks for into
viewers, types, moves, answers, reconnects with backoff, and says plainly why it isn't live.

**Architecture:** A new module folder `crates/templum-api/src/client/`: `keys.rs` (the key's keeping), `trust.rs`
(rustls verifiers), `dial.rs` (TCP, the proxy's `CONNECT`, TLS), `http.rs` (hyper 1 client connections, calls and
their failures), `sse.rs` (the event reader), `frames.rs` (a frame to a viewer's bytes), `remote.rs` (one remote's
life) and `mod.rs` (`Mirror`: the thread, its single-threaded tokio runtime, the orders in and the updates out). The UI
thread never waits: it sends orders on a channel and drains updates and the newest state when woken. `templum-terminal`
gains a `Feeder`, so frames are fed into a viewer on the client's thread as a PTY reader would. Nothing here draws:
plan 1c wires it to the canvas.

**Tech Stack:** Rust 2024; tokio (current-thread runtime), hyper 1 (`client`, `http1`), tokio-rustls / rustls 0.23
with ring, `rustls-platform-verifier` 0.7.1 (new), `webpki-roots` 1 (in the tree), `keyring-core` 1.0.1 and its
three stores (new), serde_json, base64, ring.

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), whole; this plan builds "The connection
code" (decoding is plan 1a's `code::decode`), "Trust", "A private authority", "The client", "Mirroring the state"
(its client half), "Terminals" (frames to bytes, the stream set's client half), "The keyboard" (the queue), "Where the
key is kept", and the client half of "When it isn't live". Read also [docs/specs/api.md](../specs/api.md) (Events,
Batch 2's frame) and the code: `crates/templum-api/src/{code,model,server,events,terminals,tls,store}.rs`,
`crates/templum-api/tests/{common/mod,https,terminals,events}.rs` (the `Fake` home and its helpers),
`crates/templum-terminal/src/{terminal,screen,tap}.rs` (`Terminal::viewer`, `Terminal::feed`, `SharedTerm`).

**Consumes from plan 1a:** `code::{Code, Key, Trust, decode}`, `model::{MeView, ModesView, ShareView}` and
`FrameView::modes`, `REPLACED`, `MIRROR_HEADER`, `GET /v1/me`, `revoke_share`, `terminal_input` with up to
`registry::INPUT_KEYS` (256) keys, `store::private_write`/`private_dir`.

**Later plans consume:** 1c and 1d use `client::{Mirror, Remote, KeySource, Keys, Secrets, Kept, Command, Update,
Status, Why, Bad, About}`, `State::apply`, `templum_terminal::Feeder`.

## Global Constraints

- **MEMORY LIMITS on this machine** (systemd-oomd has killed the desktop app several times): **every** cargo command
  takes `-j 4`; **never two cargo commands at once**; while working, run only the targeted tests each step names
  (`cargo test -j 4 -p templum-api --lib client::<module>::`, `cargo test -j 4 -p templum-api --test mirror <name>`,
  `cargo test -j 4 -p templum-terminal --lib <module>::`); the full suite **once**, in Task 8.
- **Crate boundaries:** `templum-api` and `templum-terminal` stay UI-free; nothing here depends on gpui-kit. No
  `unsafe`.
- **Dependencies** (Task 3 and Task 4 add them; check each in `Cargo.lock`'s diff and with `cargo deny check`; all are
  MIT OR Apache-2.0, from the keyring-rs and rustls projects):
  - `rustls-platform-verifier = "0.7.1"` (it brings, already in the lock, `rustls-native-certs` and `openssl-probe` on
    Linux, `security-framework` on macOS, `windows-sys` on Windows; Android-only crates are not built).
  - `webpki-roots = "1"` (in the tree: ureq's).
  - `keyring-core = "1.0.1"`; `[target.'cfg(target_os = "macos")'.dependencies] apple-native-keyring-store = {
    version = "1.0.2", default-features = false, features = ["keychain"] }`; `[target.'cfg(windows)'.dependencies]
    windows-native-keyring-store = "1.1.0"`; `[target.'cfg(any(target_os = "linux", target_os = "freebsd",
    target_os = "openbsd", target_os = "netbsd"))'.dependencies] zbus-secret-service-keyring-store = { version =
    "1.0.1", features = ["rt-async-io-crypto-rust"] }` (it brings `secret-service` 5.2.0 on zbus 5 with async-io, which
    the tree builds, and RustCrypto's `aes` 0.9, `cbc` 0.2, `hkdf` 0.13 as second versions: expected).
  - hyper gains its `client` feature (`features = ["server", "client", "http1"]`; already compiled in the tree).
  - Each new line in `Cargo.toml` has the comment the crate's other lines have ("New: …" or "Already in the tree: …").
- **The UI thread never waits on the network or the keyring:** `Mirror`'s methods only push to a channel or take what
  a lock holds for a copy. The keyring is called on the runtime's blocking pool.
- **Memory is bounded:** per remote one newest state not yet taken; at most 12 terminal streams; nothing held between
  frames (each is fed as it is read); an event at most 16 MiB (`sse::CAP`); at most 1024 waiting keys per terminal;
  the updates queue at most 1024 (the oldest dropped); viewers are made without scrollback (plan 1c).
- **The key never leaks:** `Key` (plan 1a) and `Secrets` print `tpl_…` in `Debug`; no error, update, status, log line,
  panic message or environment variable holds a token, an Access secret or a proxy's password. Task 7's test formats
  every update, status and failure and finds the token's text nowhere, and every `tpl_` it finds followed by `…`.
- **What home sends is drawn, never run:** every control character (C0, DEL, C1) in a run's text or a title is dropped,
  a title is cut at 256 characters, a color is an index 0–255 or a strict `#rrggbb` (anything else is the default), a
  frame over 2000 × 1000 cells is refused (that stream ends and reopens).
- **No redirect is followed;** every request goes only to the code's host (and through the proxy when set).
- **Values, verbatim:** backoff 1 s doubling to 30 s, each wait ±20%, back to 1 s once a connection was live for a
  minute; a stream silent 45 s is dead; `GET /v1/me` when no call ran for 15 s; at most 12 terminal streams, 2 call
  connections, one event stream; keys at most 256 per call (`registry::INPUT_KEYS`), 1024 waiting; a paste at most
  256 KiB (`tools::MAX_TEXT`); connect, `CONNECT` and handshake 10 s each; headers `Authorization: Bearer`,
  `CF-Access-Client-Id`, `CF-Access-Client-Secret`, `User-Agent: Templum/<version> (mirror)`, `Templum-Mirror:
  <session>` (16 random base64url characters, new at each connect).
- **Every logic change has a test that fails without it.** Tests run on Linux, macOS and Windows with no agent CLI, no
  real keyring (keyring-core's `mock::Store`, or a file in a temporary folder), no network beyond loopback, no sleeps
  to synchronize (conditions with a 10 s deadline: `common::wait_for`).
- **Minors** go to `docs/minors.md` under `## Templum to Templum`. Docs lines ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **A laptop that sleeps with a remote live:** on waking the event stream is silent; within 45 s the remote is
   Reconnecting, drops every old connection, and comes back with a new session and `Last-Event-ID`. → Task 6
   `a_silent_stream_is_dead_and_the_remote_comes_back` and `the_event_stream_resumes_with_the_last_id`.
2. **Home sends a hostile frame** (ESC in a run, an OSC in the title, a 5000-column size, a color "red"): the viewer
   shows text, never runs a sequence, and the stream reopens after a refused size. → Task 1
   `what_home_sends_is_drawn_never_run`.
3. **A share revoked while the mirror types:** the call answers `401`, the remote is Cut(Revoked) and never retries,
   the waiting keys are dropped. → Task 7 `a_revoked_share_cuts_the_mirror_for_good`.
4. **The person types faster than home answers:** keys wait (one call in flight per terminal), go 256 at most per
   call in order, and past 1024 the new ones are dropped with `Update::Dropped`. → Task 7
   `keys_wait_their_turn_and_overflow_is_dropped`.
5. **A work computer behind a company proxy that reads HTTPS:** a certificate the system trusts only through an added
   authority is refused as `AddedAuthority`, and accepted only with `trust_added`. → Task 4
   `an_added_authority_is_refused_unless_trusted`.

---

### Task 1: A frame becomes a viewer's bytes, fed off the UI thread

**Files:**
- Modify: `crates/templum-terminal/src/terminal.rs` (`Feeder`, `Terminal::feeder`, `SharedTerm::grid_size`;
  `Terminal::feed` goes through a `Feeder`), `src/lib.rs` (export `Feeder`)
- Create: `crates/templum-api/src/client/mod.rs` (only `mod frames;` for now, `pub mod client;` in `lib.rs`),
  `crates/templum-api/src/client/frames.rs`
- Modify: `crates/templum-terminal/src/screen.rs` (its test-only `redraw` goes: `frames.rs`' test replaces it)

**Interfaces:**
- Produces, in `templum_terminal`: `#[derive(Clone)] pub struct Feeder` (a viewer's `SharedTerm` and its `Shared`;
  `Send + Sync`) with `pub fn feed(&self, bytes: &[u8])` (what `Terminal::feed` did: the tap's scanner, vte's parser
  under the lock, `wake`) and `pub fn resize(&self, cols: u16, rows: u16)` (the grid, under the lock, when it differs;
  the size the window reports follows); `impl Terminal { pub fn feeder(&self) -> Feeder }`; `impl SharedTerm { pub fn
  grid_size(&self) -> (u16, u16) }` (columns and screen lines, `lock_unfair`).
- Produces, in `client/frames.rs` (crate-private):
  - `pub(crate) const MOST: (usize, usize) = (2000, 1000);` (columns, rows).
  - `pub(crate) struct Painter { modes: Option<ModesView>, title: Option<String> }` (`Default`), one per stream:
    `pub(crate) fn paint(&mut self, frame: &FrameView, into: &Feeder) -> Result<(), &'static str>`: a size over `MOST`
    is `Err("too large")` and nothing is fed; a whole frame whose size differs from `into`'s grid resizes it first;
    then `into.feed(&self.bytes(frame))`.
  - `pub(crate) fn bytes(&mut self, frame: &FrameView) -> Vec<u8>`: first the modes that changed since the last
    frame (all set ones on the first): `CSI ?1049h`/`l` (full screen), `CSI ?1000h`/`l` (mouse), `CSI >1u` / `CSI <u`
    (kitty keys, pushed when it turns on, popped when it turns off); a whole frame then `CSI H CSI 2J`; each row
    (`y < size.rows`) `CSI y+1;1H CSI 0m CSI 2K`, then each run `CSI 0m` + its SGR (1 bold, 2 dim, 3 italic, 4
    underline, 7 inverse, `38;5;i` / `38;2;r;g;b`, `48;5;i` / `48;2;r;g;b`) + its text with every `char::is_control`
    character dropped; then `CSI 0m`; the cursor (None: `CSI ?25l`; else `CSI row+1;col+1H`, DECSCUSR `CSI n SP q`:
    block 2/1, underline 4/3, beam 6/5 steady/blinking, then `CSI ?25h`); the title when it changed: `OSC 2;<title>
    BEL` with controls dropped and at most 256 characters, a None after Some as `OSC 2; BEL` (a blank title reads
    as none). A color that isn't an index or `#rrggbb` (six hex digits) is the default (no SGR).

- [ ] **Step 1: Write the failing tests:**

```rust
// terminal.rs
#[test]
fn a_feeder_feeds_and_resizes_a_viewer_from_another_thread() {
    // v = Terminal::viewer("v", (20, 4), ..); let f = v.feeder(); std::thread::spawn(move || f.feed(b"hi")).join();
    // v.shared().lines(..) shows "hi"; v.feeder().resize(30, 6): v.shared().grid_size() == (30, 6).
}

// frames.rs (homes are headless or viewer terminals; `screen_of(&Terminal)` = shared().screen().unwrap())
#[test]
fn a_frame_painted_into_a_viewer_reads_back_the_same_screen() {
    // batch 2's round trip, moved here: a 30x5 headless term fed every style (colors of each kind, every flag,
    // hidden, wide, combining, a colored blank, a cursor shape, a title, a full last row, full screen and kitty keys
    // on); its whole frame (terminals::whole) painted by a fresh Painter into a 30x5 viewer:
    // screen_of(viewer) == screen_of(home). Then home fed "\r\nmore" and the changed-rows frame painted: equal again.
}

#[test]
fn what_home_sends_is_drawn_never_run() {
    // a FrameView with a run text "a\x1b]2;pwn\x07b\u{9b}31mc", title "t\x1b[2J" + 300 × "x", fg Color::Rgb("red"):
    // bytes() holds no 0x1b inside the run or the title (only the painter's own sequences: count of ESC equals the
    // count the same frame with clean text makes); the viewer's row 0 reads "a]2;pwnb31mc" with the default color;
    // its title is "t[2J" + 251 × "x" (256 characters).
    // size {cols: 5000, rows: 10}: paint() == Err("too large"), the viewer unchanged.
}

#[test]
fn a_whole_frame_of_a_new_size_resizes_the_viewer() {
    // a 10x3 viewer; a whole frame of 20x5 painted: grid_size() == (20, 5) and its rows are there.
}

#[test]
fn modes_are_sent_only_when_they_change() {
    // bytes(frame with kitty_keys) contains "\x1b[>1u"; the same frame again: no "\x1b[>1u"; kitty_keys off:
    // "\x1b[<u". The viewer's shared().modes().kitty_keys follows (true, then false).
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-terminal --lib terminal::a_feeder` and
  `cargo test -j 4 -p templum-api --lib client::frames::` → compile errors.
- [ ] **Step 3: Implement.** Remove `screen.rs`' test-only `redraw` and its round-trip test (moved, not lost).
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-terminal --lib` and `cargo test -j 4 -p templum-api --lib` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 2: Reading the event stream, and home's state applied

**Files:**
- Create: `crates/templum-api/src/client/sse.rs`
- Modify: `crates/templum-api/src/model.rs` (`State::apply`)

**Interfaces:**
- Produces, in `client/sse.rs`: `pub(crate) const CAP: usize = 16 * 1024 * 1024;`; `#[derive(Debug, PartialEq)]
  pub(crate) struct Event { pub event: String, pub data: String, pub id: Option<String> }`; `#[derive(Default)]
  pub(crate) struct Reader` with `pub(crate) fn push(&mut self, bytes: &[u8]) -> Result<Vec<Event>, TooLarge>`
  (`pub(crate) struct TooLarge;`) and `pub(crate) fn last_id(&self) -> Option<&str>`. The rules (the WHATWG
  event-stream format, the parts home uses): lines end in `\n`, `\r\n` or `\r`; a line starting with `:` is a comment
  (ignored); `field: value` (one space after the colon dropped), a line with no colon is that field with an empty
  value; `event` sets the name, `data` lines are joined with `\n`, `id` sets the id (and `last_id`) unless it holds a
  NUL; other fields are ignored; a blank line dispatches (nothing when no `data` came; the name defaults to
  `message`). More than `CAP` bytes held for one event (its data, plus the line not yet ended) is `Err(TooLarge)`.
- Produces, in `templum_api::model`: `impl State { pub fn apply(&mut self, event: &str, data: &serde_json::Value) ->
  Result<bool, String> }`: `snapshot` replaces the state (`data` is a State); `workspace`, `block`, `wire`,
  `request`, `board`, `card`, `automation`, `share`, `usage` take `{"op":"put","id","value"}` (replace the item with
  that id, or add it at the end) and `{"op":"delete","id"}` (remove it), ids as `diff` makes them (`WireView::id`, a
  number's text); `view` sets `view` (null: None). True when the state changed; any other event is `Ok(false)`; a
  malformed one is `Err` with what was wrong.

- [ ] **Step 1: Write the failing tests:**

```rust
// sse.rs
#[test]
fn events_are_read_across_any_chunking() {
    let text = b": ping\n\nid: a-1\nevent: block\ndata: {\"op\":\"put\"}\n\nid: a-2\r\nevent: frame\r\ndata: x\r\ndata: y\r\n\r\n";
    // pushed whole, then byte by byte: the same two events [block {"op":"put"} Some("a-1"), frame "x\ny" Some("a-2")];
    // last_id() == Some("a-2"); "data\n\n" gives one event with data "" and event "message"; "event: x\n\n" none.
}

#[test]
fn an_event_past_the_cap_is_too_large() {
    // a Reader fed "data: " then CAP bytes of 'x' without a newline -> Err(TooLarge); a fresh one fed 1 MiB events
    // ten times -> ten events, no error.
}

// model.rs
#[test]
fn a_mirror_applies_what_the_stream_sends() {
    // s = sample state; s.apply("block", {"op":"put","id":"b1","value":<b1 moved>}) -> Ok(true), b1's rect moved;
    // apply("block", {"op":"put","id":"zz",..}) adds it last; apply("wire", {"op":"delete","id":"a-b"}) removes it;
    // apply("share", put s4) adds a share; apply("view", null) -> view None; apply("snapshot", <other state>) == it;
    // apply("notice", {..}) == Ok(false); apply("block", {"op":"put"}) is Err.
    // After applying diff(a, b) as events onto a, a == b (diff's deltas, framed as the server frames them).
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --lib client::sse:: model::` →
  compile errors.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api --lib` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 3: Where the key is kept

**Files:**
- Modify: `crates/templum-api/Cargo.toml` (keyring-core and its three stores, as Global Constraints say)
- Create: `crates/templum-api/src/client/keys.rs`

**Interfaces:**
- Produces, in `templum_api::client` (re-exported from `keys.rs`):
  - `pub const SERVICE: &str = "Templum";` and `pub const SERVICE_DEV: &str = "Templum (dev)";`
  - `#[derive(Clone, PartialEq, Eq)] pub struct Secrets { pub token: Key, pub access_secret: Option<String>, pub
    proxy_auth: Option<String> }` (`proxy_auth` is `user:password`); `Debug` prints `Secrets { token: tpl_…, .. }`.
    Kept as one JSON value `{"token","access_secret","proxy_auth"}` (`to_json`/`from_json`, crate-private).
  - `#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Kept { Keyring, File, Memory }`.
  - `#[derive(Clone)] pub struct Keys` with:
    - `pub fn system(service: &str, data: Option<&Path>) -> Keys`: the system's store, made at its first use (on the
      caller's thread: the client calls it on its blocking pool), once (`OnceLock`): macOS
      `apple_native_keyring_store::keychain::Store::new()`, Windows `windows_native_keyring_store::Store::new()`,
      Linux and the BSDs `zbus_secret_service_keyring_store::Store::new()`; an Err or another platform: no keyring.
      `data` is the data folder, for the files (`<data>/remotes/`); None: no file can be kept.
    - `pub fn with_store(store: Arc<keyring_core::CredentialStore>, service: &str, data: Option<&Path>) -> Keys` (the
      tests: `keyring_core::mock::Store::new().unwrap()`) and `pub fn without_keyring(data: Option<&Path>) -> Keys`.
    - `pub fn answers(&self) -> bool`: a keyring is there and reading an entry that doesn't exist answers `NoEntry`.
    - `pub fn save(&self, entry: &str, secrets: &Secrets, file: bool) -> Result<Kept, String>`: `file` → `<data>/
      remotes/<entry>.key` through `store::private_dir` + `private_write` (`0600` in `0700`), `Kept::File`; else the
      keyring (`store.build(service, entry, None)?.set_password(json)`), `Kept::Keyring`; no keyring, or it refused:
      `Kept::Memory` (the caller keeps the secrets in memory). An Err only for a file that couldn't be written (its
      words name the folder, never the content).
    - `pub fn load(&self, entry: &str, file: bool) -> Result<Secrets, String>`: the words of an Err are the spec's
      "Templum couldn't read this connection's key from the keyring." (or "…from its file." for a file).
    - `pub fn delete(&self, entry: &str, file: bool)`: both places, errors ignored.
  - `pub fn entry_name() -> String`: `remote-` and 8 random characters `[a-z0-9]` (from `store::random_text`).

- [ ] **Step 1: Write the failing tests** (`keys.rs`; `mock()` = `Keys::with_store(mock::Store::new().unwrap(),
  SERVICE_DEV, Some(dir))`):

```rust
#[test]
fn a_key_round_trips_through_the_keyring() {
    // s = Secrets { token: Key::new("tpl_SECRET".into()), access_secret: Some("acc".into()), proxy_auth: None };
    // mock().save("remote-ab12cd34", &s, false) == Ok(Kept::Keyring); load(.., false) == Ok(s); delete(..);
    // load(..) is Err("Templum couldn't read this connection's key from the keyring."); no file under dir/remotes.
}

#[test]
fn without_a_keyring_the_key_stays_in_memory_unless_a_file_is_asked() {
    // Keys::without_keyring(Some(dir)).answers() == false; save(.., false) == Ok(Kept::Memory) and no file;
    // save(.., true) == Ok(Kept::File), the file exists, load(.., true) == Ok(s), delete removes it.
    // #[cfg(unix)]: the file's mode & 0o777 == 0o600 and the folder's == 0o700.
}

#[test]
fn a_refusing_keyring_is_memory_not_an_error() {
    // a mock entry whose credential set_error(Error::NoStorageAccess(..)) (mock::Cred::set_error, as keyring-core's
    // docs show) -> save(.., false) == Ok(Kept::Memory).
}

#[test]
fn secrets_never_show() {
    // format!("{s:?}") has neither "SECRET" nor "acc"; entry_name() matches remote-[a-z0-9]{8}.
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --lib client::keys::` → compile
  errors.
- [ ] **Step 3: Add the dependencies,** then `cargo tree -j 4 -p templum-api -e normal --target all -i secret-service`
  shows it only under the Linux/BSD store, and `cargo deny check` → ok (paste its summary in the report; a new
  license or advisory stops the task: report it instead of allowing it).
- [ ] **Step 4: Implement** `keys.rs`.
- [ ] **Step 5: Run** `cargo test -j 4 -p templum-api --lib client::keys::` → PASS.
- [ ] **Step 6: Leave uncommitted.**

### Task 4: Trust: the pin, the web's authorities, and an added one

**Files:**
- Modify: `crates/templum-api/Cargo.toml` (`rustls-platform-verifier`, `webpki-roots`), `src/store.rs` (`same`
  becomes `pub(crate)`)
- Create: `crates/templum-api/src/client/trust.rs`

**Interfaces:**
- Produces, in `client/trust.rs` (crate-private; *the planner's call*: each refusal is a `rustls::Error::General`
  holding one of the marks below, so the dialer can tell them apart after the handshake):
  - `pub(crate) const PIN: &str = "templum:pin"`, `ADDED: "templum:added"`, `EXPIRED: "templum:expired"`, `NAME:
    "templum:name"`, `ISSUER: "templum:issuer"`, `ALONE: "templum:issuer-alone"` (no authority known, and the server
    sent its certificate alone: maybe Templum's own), `OTHER: "templum:other"`.
  - `pub(crate) fn provider() -> Arc<CryptoProvider>` (ring's, as the server's).
  - `pub(crate) struct Pinned { pin: [u8; 32] }` with `pub(crate) fn new(hex: &str) -> Option<Pinned>`;
    `impl ServerCertVerifier`: the end entity's SHA-256 compared with `store::same` (every byte) → `Ok`, else
    `Err(General(PIN))`; names and dates not looked at; the handshake's signatures checked with
    `rustls::crypto::verify_tls12_signature` / `verify_tls13_signature` and the provider's algorithms.
  - `pub(crate) struct Checked { system: Arc<dyn ServerCertVerifier>, public: Arc<dyn ServerCertVerifier>, trust_added:
    bool }` with `pub(crate) fn new(system, public, trust_added) -> Checked`; `verify_server_cert`: `system` first —
    its `InvalidCertificate(Expired…)` → `EXPIRED`, `NotValidForName…` → `NAME`, `UnknownIssuer` → `ALONE` when
    `intermediates` is empty else `ISSUER`, anything else → `OTHER`; then, unless `trust_added`, `public` on the same
    chain: an Err → `ADDED`. Signatures by `system`.
  - `pub(crate) fn web() -> Result<Checked parts, String>`: `system` = `rustls_platform_verifier::Verifier::new(
    provider())`, `public` = `rustls::client::WebPkiServerVerifier::builder_with_provider(roots of
    webpki_roots::TLS_SERVER_ROOTS, provider()).build()`.
  - `pub(crate) fn config(verifier: Arc<dyn ServerCertVerifier>) -> Arc<ClientConfig>`: `builder_with_provider`,
    safe default versions (1.2 and 1.3), `dangerous().with_custom_certificate_verifier`, no client auth, ALPN
    `http/1.1`.

- [ ] **Step 1: Write the failing tests** (`trust.rs`; certificates made with rcgen: `ca(name)` an authority,
  `leaf(ca, "localhost", days)` a leaf it signs; `roots(&[ca])` a `WebPkiServerVerifier` that trusts them; `check(v,
  leaf, intermediates)` calls `verify_server_cert` for `ServerName "localhost"` at now):

```rust
#[test]
fn the_pin_is_the_certificates_identity() {
    // a self-signed cert for "localhost", 5 years: Pinned::new(&tls::pin(&cert)) accepts it for the name
    // "192.168.1.5" too (names aren't looked at); another cert -> Err(General(PIN)); Pinned::new("zz") is None.
    // verify_tls13_signature with a signature of other bytes -> Err.
}

#[test]
fn a_public_certificate_is_checked_twice() {
    // Checked::new(roots([a]), roots([a]), false) accepts leaf(a); an expired leaf(a, -1) -> General(EXPIRED);
    // leaf(a) for "other.example" -> General(NAME); leaf(b) with no intermediates -> General(ALONE), with one ->
    // General(ISSUER).
}

#[test]
fn an_added_authority_is_refused_unless_trusted() {
    // the "system" trusts a and c (a company's added authority), the "public" only a:
    // Checked::new(roots([a, c]), roots([a]), false) on leaf(c) -> Err(General(ADDED));
    // with trust_added true -> Ok.
}

#[test]
fn the_web_verifiers_build_on_this_computer() {
    // web() is Ok (the platform verifier and Mozilla's roots load; no network is asked).
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --lib client::trust::` → compile
  errors.
- [ ] **Step 3: Add the dependencies,** `cargo deny check` → ok.
- [ ] **Step 4: Implement.**
- [ ] **Step 5: Run** `cargo test -j 4 -p templum-api --lib client::trust::` → PASS.
- [ ] **Step 6: Leave uncommitted.**

### Task 5: Reaching home: the proxy, TLS, calls and what their failures mean

**Files:**
- Create: `crates/templum-api/src/client/dial.rs`, `crates/templum-api/src/client/http.rs`
- Modify: `crates/templum-api/Cargo.toml` (hyper's `client` feature), `src/client/mod.rs` (`Failure`, `Why`, `Bad`)

**Interfaces:**
- Produces, in `templum_api::client` (`mod.rs`):
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub enum Bad { Expired, Name, Issuer, Alone, Other }`.
  - `#[derive(Clone, Debug, PartialEq, Eq)] pub enum Why { Expired, Revoked, Deleted, Replaced, PinChanged,
    Certificate(Bad), AddedAuthority, Access, NotTemplum { status: u16, kind: String }, TooOld, Proxy { proxy: String,
    status: u16 }, Keyring }` — why a remote stopped and won't retry (the spec's table, rows 4 to 15).
  - `#[derive(Clone, Debug, PartialEq)] pub(crate) enum Failure { Down(String), Busy(Duration), Auth, Missing(String),
    Refused(String), Stop(Why), TooLarge }`: Down retries (refused, timed out, no route, a name that doesn't resolve,
    502, 503 without Templum's `busy`, 504, 530, an ended stream); Busy retries after its wait (429, 503 `busy`,
    `Retry-After` in seconds, else 1 s); Auth is a 401 (the remote decides Expired or Revoked); Missing is a 404 with
    Templum's body (its message); Refused a 422 or 403 with Templum's body (its message); Stop doesn't retry.
- Produces, in `client/dial.rs` (crate-private):
  - `pub(crate) struct Place { pub https: bool, pub host: String, pub port: u16, pub prefix: String }` and
    `pub(crate) fn place(url: &str) -> Option<Place>` (`hyper::Uri`; default ports 443/80; the path without a trailing
    `/`; an IPv6 host without its brackets).
  - `pub(crate) struct Proxy { pub host: String, pub port: u16, pub auth: Option<String> }`;
    `pub(crate) fn proxy_of(text: &str) -> Result<(String, Option<String>), String>`: `http://[user:pass@]host:port`
    → the address without credentials (`http://host:port`, what the index keeps) and the credentials (what the keyring
    keeps); "The proxy must look like http://proxy.corp:8080." otherwise.
  - `pub(crate) fn proxy_from_env(host: &str, var: impl Fn(&str) -> Option<String>) -> Option<String>`: `HTTPS_PROXY`,
    then `https_proxy`, then `ALL_PROXY`, then `all_proxy`; None when `NO_PROXY`/`no_proxy` names the host (`*`, the
    host, or a suffix `.example.com` / `example.com` matching it or its subdomains, comma-separated, ports ignored).
  - `pub(crate) async fn dial(place: &Place, tls: Option<Arc<ClientConfig>>, proxy: Option<&Proxy>) -> Result<Io,
    Failure>` where `pub(crate) type Io = Box<dyn hyper::rt::Read + hyper::rt::Write + Unpin + Send>` (tokio's IO
    through `hyper_util::rt::TokioIo`): TCP (10 s); through a proxy, `CONNECT host:port HTTP/1.1` with `Host` and,
    when `auth`, `Proxy-Authorization: Basic …`, its answer's head read up to 16 KiB within 10 s: 200 goes on, else
    `Stop(Why::Proxy { proxy: "proxy.corp:8080", status })`; then TLS when `tls` (10 s; `ServerName` from the host: a
    name or an IP, which sends no SNI) whose error carrying one of `trust`'s marks becomes `Stop(PinChanged)`,
    `Stop(AddedAuthority)`, `Stop(Certificate(Bad::…))`; other IO errors are `Down` with their kind.
- Produces, in `client/http.rs` (crate-private):
  - `pub(crate) struct Headers { pub key: Key, pub access: Option<(String, String)>, pub session: String, pub host:
    String }` and `fn apply(&self, request: &mut http::request::Builder)`: the headers of Global Constraints.
  - `pub(crate) struct Conn` (a hyper `http1::SendRequest` whose driver runs on the runtime): `pub(crate) async fn
    open(place, tls, proxy) -> Result<Conn, Failure>` and `pub(crate) async fn call(&mut self, method: &str, path:
    &str, body: Option<&Value>, headers: &Headers) -> Result<Value, Failure>` (`path` gets `place.prefix` in front;
    a body is `application/json`; the answer read whole up to 1 MiB).
  - `pub(crate) async fn stream(place, tls, proxy, path, headers, last_id: Option<&str>) -> Result<hyper::body::
    Incoming, Failure>` (its own connection; `Accept: text/event-stream`, `Last-Event-ID` when given).
  - `pub(crate) fn failure(status: u16, headers: &HeaderMap, body: &[u8], path: &str) -> Failure`: 401 → Auth; 429 →
    Busy; 503 with Templum's `{"error":{"code":"busy"}}` → Busy, other 502/503/504/530 → Down; a 3xx whose
    `Location` host ends in `cloudflareaccess.com`, or a 403 with a `cf-access-domain` header or a body mentioning
    `cloudflareaccess.com` → `Stop(Access)`; Templum's error body (`{"error":{"code","message"}}`): 404 → for
    `/v1/me` `Stop(TooOld)`, else Missing(message); 403/422 → Refused(message); anything else not Templum's (another
    3xx, HTML, a 404 without that body) → `Stop(NotTemplum { status, kind })` with `kind` the `Content-Type` before
    `;` (or "a redirect" for a 3xx, "no type" without one).

- [ ] **Step 1: Write the failing tests:**

```rust
// dial.rs
#[test]
fn places_and_proxies_are_read() {
    // place("https://templum.example.com/base/") == Place { https: true, host: "templum.example.com", port: 443,
    // prefix: "/base" }; place("https://[::1]:7411") host "::1"; place("http://127.0.0.1:7411") port 7411.
    // proxy_of("http://u:p@proxy.corp:8080") == Ok(("http://proxy.corp:8080", Some("u:p"))); proxy_of("proxy") is Err.
    // proxy_from_env("t.example.com", HTTPS_PROXY=http://p:3128) == Some(..); with NO_PROXY=".example.com" None;
    // NO_PROXY="other.com" Some; https_proxy alone used; ALL_PROXY used last.
}

#[tokio::test(flavor = "current_thread")]
async fn a_proxy_carries_the_connection_or_says_why_not() {
    // a std::thread proxy on 127.0.0.1:0 that reads the CONNECT head, asserts it names the target and carries
    // "Proxy-Authorization: Basic dTpw" (u:p), answers "HTTP/1.1 200 Connection established\r\n\r\n" and pipes to
    // a std::thread HTTP server answering every request with 200 {"ok":true}: Conn::open through the proxy, then
    // call("GET", "/v1/me", ..) == Ok(json!({"ok": true})). A proxy answering 407 -> Err(Failure::Stop(Why::Proxy
    // { proxy: "127.0.0.1:<port>", status: 407 })). A closed port -> Err(Failure::Down(_)).
}

// http.rs
#[test]
fn failures_say_what_happened() {
    // failure(401, ..) == Auth; (429, Retry-After: 7) == Busy(7 s); (503, {"error":{"code":"busy"}}) == Busy(1 s);
    // (503, text/html) == Down; (530, ..) == Down; (302, Location: https://x.cloudflareaccess.com/..) ==
    // Stop(Access); (302, Location: https://elsewhere) == Stop(NotTemplum { status: 302, kind: "a redirect" });
    // (404, text/html, "/v1/blocks") == Stop(NotTemplum { 404, "text/html" }); (404, Templum's not_found, "/v1/me")
    // == Stop(TooOld); (404, Templum's, "/v1/terminals/x/stream") == Missing(msg); (422, Templum's refused) ==
    // Refused(msg); (403, cf-access-domain: x) == Stop(Access).
}

#[test]
fn every_request_carries_the_mirrors_headers() {
    // Headers { key "tpl_x", access Some(("id", "sec")), session "s1", host "t.example.com" } applied: Authorization
    // "Bearer tpl_x", CF-Access-Client-Id "id", CF-Access-Client-Secret "sec", User-Agent starts "Templum/" and ends
    // "(mirror)", Templum-Mirror "s1", Host "t.example.com".
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --lib client::dial:: client::http::`
  → compile errors. (`#[tokio::test]` needs tokio's `macros` in `[dev-dependencies]`: `tokio = { version = "1",
  features = ["macros", "rt"] }`, already in the tree; or build a current-thread runtime by hand in the test.)
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** the same → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 6: One thread for every remote, and a remote's life

**Files:**
- Modify: `crates/templum-api/src/client/mod.rs` (`Mirror`, `Remote`, `KeySource`, `Command`, `Update`, `Status`,
  `About`)
- Create: `crates/templum-api/src/client/remote.rs`, `crates/templum-api/tests/mirror.rs`
- Modify: `crates/templum-api/tests/common/mod.rs` (`mirror_of(fake, …)`, `updates_until`)

**Interfaces:**
- Consumes: Tasks 2–5, plan 1a's `code::{Key, Trust}`, `MeView`, `REPLACED`.
- Produces, in `templum_api::client`:
  - `#[derive(Clone, Debug)] pub struct Remote { pub url: String, pub trust: Trust, pub trust_added: bool, pub
    access_id: Option<String>, pub proxy: Option<String>, pub key: KeySource }` (`proxy` without credentials: they are
    in the secrets).
  - `#[derive(Clone, Debug)] pub enum KeySource { Kept { entry: String, file: bool }, Save { entry: String, secrets:
    Secrets, file: bool }, Memory(Secrets) }`: load it, save it first (then `Update::Kept`), or use it as given.
  - `#[derive(Clone, Debug, PartialEq)] pub enum Status { Connecting, Live, Reconnecting { attempt: u32, next:
    Duration, live_at: Option<SystemTime> }, Cut(Why), Disconnected }` and `impl Status { pub fn word(&self) -> &'static
    str }`: `connecting`, `live`, `reconnecting`, `cut` (Expired, Revoked, Deleted, Replaced), `refused` (every other
    `Why`), `disconnected` (the mirror's API state's words).
  - `#[derive(Clone, Debug, PartialEq)] pub enum About { Keys(String), Move(String), Answer(String), Show }`.
  - `#[derive(Clone, Debug)] pub enum Update { Status(Status), Me(MeView), Latency(Duration), Kept(Kept), Refused {
    about: About, sentence: String }, Dropped(String), Keyring(bool) }`.
  - `pub enum Command { Disconnect, TryNow, Terminals(Vec<(String, Feeder)>), Keys { block: String, keys: Vec<String> },
    Paste { block: String, text: String }, Move { block: String, rect: Rect }, Answer { request: String, args: Value },
    Show }` (Task 7 serves the last five; here they are accepted and ignored).
  - `pub struct Mirror` (dropping it ends the thread and every connection), with:
    - `pub fn start(keys: Keys, wake: Arc<dyn Fn() + Send + Sync>) -> Mirror`: a thread `templum-mirror` with a
      current-thread tokio runtime (`enable_io`, `enable_time`); orders come on an unbounded channel.
    - `pub fn connect(&self, id: &str, remote: Remote)` (replaces a remote with that id), `pub fn send(&self, id: &str,
      command: Command)`, `pub fn forget(&self, id: &str, revoke: bool)` (Task 7: revoke first when asked), `pub fn
      probe(&self)` (answers `("", Update::Keyring(keys.answers()))`).
    - `pub fn updates(&self) -> Vec<(String, Update)>` (drains; at most 1024 kept, the oldest dropped) and `pub fn
      state(&self, id: &str) -> Option<State>` (the newest state not yet taken; taking it leaves None until a newer
      one). Each push calls `wake`.
- Produces, in `remote.rs` (crate-private), one task per remote: the key (loaded on the blocking pool; Err →
  `Cut(Keyring)`; `Save` → `keys.save`, then `Update::Kept`, and `Kept::Memory` keeps the secrets in the task); the
  verifier (`Trust::Pinned` → `Pinned`, `Public` → `Checked` from `trust::web()` with `trust_added`, `Loopback` → no
  TLS); then attempts, each with a new session:
  1. `Connecting` (the first) or `Reconnecting { attempt, next, live_at }`; every old connection dropped first.
  2. `GET /v1/me` on the calls connection → `Update::Me`; `workspaces == Some([])` → `Cut(Deleted)`.
  3. `GET /v1/events?view=true` with the last event id → `Live`; `snapshot` and state events go through
     `State::apply` into the newest-state slot (then `wake`); `replaced` → `Cut(Replaced)`; nothing read for 45 s →
     the attempt fails Down; 15 s with no call → `GET /v1/me` again (→ `Update::Latency` and `Update::Me`).
  4. A failure: Auth → `Cut(Expired)` when the last `Me.expires` has passed, else `Cut(Revoked)`; Stop(why) →
     `Cut(why)`; Down, Busy, TooLarge, an ended stream → wait and retry. Backoff: `pub(crate) fn backoff(attempt: u32,
     jitter: f64) -> Duration` = `min(30 s, 1 s × 2^(attempt−1)) × (1 + jitter)` with `jitter` in `[-0.2, 0.2]`
     (random), Busy waits at least its `Retry-After`; the attempt count goes back to 1 once a connection was live for
     a minute. `TryNow` cuts the wait (and starts a Disconnected or Cut remote again); `Disconnect` drops everything,
     `Status(Disconnected)`.
  - Every round trip (`me`, a call) sends `Update::Latency`.

- [ ] **Step 1: Add the helpers** to `tests/common/mod.rs`: `pub fn share_token(scope, workspace) -> (String, Token)`
  (a token with `share: Some(Share { workspace, way: Public, address })`), `pub fn mirror(keys: Keys) -> (Mirror,
  Arc<AtomicUsize>)` (its wake counts), `pub fn remote_for(fake: &Fake, text: &str) -> Remote` (Loopback, `KeySource::
  Memory`), `pub fn until(mirror: &Mirror, id: &str, what: &str, pick: impl FnMut(&Update) -> bool) -> Vec<Update>`
  (drains with `wait_for` until one update matches; returns all seen), and `pub fn fake_on(addr: SocketAddr, tokens:
  Vec<Token>) -> Fake` (a home started again on the address a dropped one had).
- [ ] **Step 2: Write the failing tests:**

```rust
// remote.rs
#[test]
fn backoff_doubles_to_thirty_seconds_with_a_fifth_either_way() {
    // backoff(1, 0.0) == 1 s, (2, 0.0) == 2 s, (5, 0.0) == 16 s, (6, 0.0) == 30 s, (40, 0.0) == 30 s;
    // backoff(3, 0.2) == 4.8 s, backoff(3, -0.2) == 3.2 s.
}

#[test]
fn the_event_stream_resumes_with_the_last_id() {
    // the request remote.rs builds for an attempt after events with id "r-7": path "/v1/events?view=true", header
    // Last-Event-ID "r-7" and a Templum-Mirror different from the first attempt's.
}

// tests/mirror.rs (a Fake home whose state is sample(); a share token limited to "w3")
#[test]
fn a_mirror_follows_home_live() {
    // mirror.connect("r1", remote_for(&fake, &text)): Status(Connecting), Me { computer, workspaces: ["w3"], share:
    // Some(..) }, Status(Live); state("r1") == sample().visible(w3 caller) (then None until a newer); publish a
    // state with w3's block moved -> state("r1") has it moved. The wake counter grew.
}

#[test]
fn the_key_is_saved_before_connecting() {
    // KeySource::Save { entry, secrets, file: false } with Keys::with_store(mock) -> Update::Kept(Keyring) then
    // Live; a Remote with KeySource::Kept { entry, file: false } on a second Mirror sharing the store -> Live.
    // KeySource::Kept with an entry that isn't there -> Status(Cut(Why::Keyring)).
}

#[test]
fn a_silent_stream_is_dead_and_the_remote_comes_back() {
    // a fake with heartbeat 60 s and the client's silence limit lowered for the test (remote.rs takes its limits
    // from a `Limits` it is given; Mirror::start uses Limits::default(), the test Mirror::start_with(.., Limits
    // { silence: 300 ms, me_every: 200 ms, .. })): after Live, no event for 300 ms -> Reconnecting { attempt: 1 }
    // then Live again.
}

#[test]
fn home_gone_then_back() {
    // after Live, drop the fake's server: Reconnecting { attempt: 1, .. }, then { attempt: 2, .. }; start a new
    // server on the same address (fake_on(addr)) -> Live; TryNow while Reconnecting retries at once.
}

#[test]
fn a_deleted_workspace_and_a_second_mirror_stop_for_good() {
    // set_tokens with the share's workspaces Some([]) and a TryNow -> Cut(Deleted), no Reconnecting after.
    // Two Mirrors with one code: the second Live; the first Status(Cut(Replaced)) and never Reconnecting.
}

#[test]
fn a_changed_pin_and_a_tunnel_that_isnt_templum_are_refused() {
    // a fake with TLS Own (as tests/https.rs makes it): Trust::Pinned(tls::pin of its cert) -> Live;
    // Trust::Pinned("00" × 32) -> Cut(PinChanged). A std::thread server answering "HTTP/1.1 404 Not Found",
    // text/html -> Cut(NotTemplum { status: 404, kind: "text/html" }).
}
```

- [ ] **Step 3: Run them to verify they fail:** `cargo test -j 4 -p templum-api --lib client::remote::` and
  `cargo test -j 4 -p templum-api --test mirror` → compile errors.
- [ ] **Step 4: Implement** `Mirror` and the remote's life (`pub fn start_with(keys, wake, limits: Limits)` is
  `#[doc(hidden)] pub` for the tests; `Limits { silence: 45 s, me_every: 15 s, live_resets: 60 s }`).
- [ ] **Step 5: Run** `cargo test -j 4 -p templum-api --lib client::` and `--test mirror` three times → PASS.
- [ ] **Step 6: Leave uncommitted.**

### Task 7: Terminals, typing, moves, answers, Forget

**Files:**
- Modify: `crates/templum-api/src/client/remote.rs`, `src/client/mod.rs`
- Test: `crates/templum-api/tests/mirror.rs`, `src/client/remote.rs`

**Interfaces:**
- Consumes: Task 1 (`Painter`, `Feeder`), Task 6.
- Produces, in `remote.rs`:
  - **Terminals:** `Command::Terminals(set)` keeps its first 12; a stream opens for each block in the set while Live
    (`GET /v1/terminals/{block}/stream`, its own connection, a `Painter` of its own), each `frame` event painted into
    its `Feeder` as it is read; one no longer in the set, or every one when the remote leaves Live, is dropped; one
    that ends or fails Down reopens after 1 s while it is wanted; Missing or Refused (the block left, or isn't a
    terminal) stops it until the set changes; a frame `paint` refuses, or `TooLarge`, ends that stream (it reopens).
  - **Typing:** per block a queue; `Command::Keys` adds keys (past 1024 waiting, the new ones are dropped and
    `Update::Dropped(block)` sent once per overflow), `Command::Paste` adds a paste after them; one call in flight per
    block, on the typing connection: the waiting keys (at most `registry::INPUT_KEYS`) as `terminal_input` `{block,
    keys}`, or a paste as `{block, text}`, in order. Refused → `Update::Refused { about: About::Keys(block),
    sentence }` (home's words, e.g. "terminal_input needs a Full token.") and that block's queue is emptied; Auth →
    the remote's Auth; Down → the queue is emptied (keys are never sent late).
  - **Calls** (the other calls connection, in order): `Move` → `move_block {block, x, y, w, h}`; `Answer` →
    `answer_request` with `{request}` and `args`' fields; `Show` → `show {workspace: <Me's first workspace>}`.
    Refused or Missing → `Update::Refused { about, sentence }`.
  - **Forget:** `Mirror::forget(id, revoke)`: when `revoke` and the remote is Live, `revoke_share {}` with its own
    token first (its result ignored); then the key is deleted (`keys.delete`, both places), every connection dropped
    and the task ends. No update follows.
- Produces: `POST /v1/tools/<name>` is the path of every call (the aliases are home's convenience).

- [ ] **Step 1: Write the failing tests** (`tests/mirror.rs`; a home viewer `h` (20×4) lent as `b7k2m` in `w3`):

```rust
#[test]
fn a_terminal_streams_into_its_viewer() {
    // a mirror viewer `m` (20x4); send Command::Terminals(vec![("b7k2m", m.feeder())]); h fed "hello" -> wait_for
    // m's screen row 0 == "hello"; h fed "\x1b]2;vim\x07" -> m's title "vim". Terminals(vec![]) -> the fake's
    // terminal_streams() == 0 (wait_for).
}

#[test]
fn at_most_twelve_terminals_stream() {
    // 14 terminals lent; Terminals with all 14 -> terminal_streams() reaches 12 and stays 12.
}

#[test]
fn keys_wait_their_turn_and_overflow_is_dropped() {
    // the fake holds its terminal_input answers until released (a gate in Fake: `fake.hold_calls(true)`):
    // Keys ["a"], then 300 keys, then 800 keys -> the fake saw one call ["a"]; release -> the next call has the
    // first 256 waiting keys, then 44, then the rest in order; the 1101st key overall was dropped: Update::Dropped
    // ("b7k2m"). Every call's "keys" concatenated == what was sent minus the dropped.
}

#[test]
fn moves_answers_and_show_are_calls() {
    // Move { "b7k2m", Rect { 10, 20, 300, 200 } } -> the fake saw move_block {"block":"b7k2m","x":10.0,"y":20.0,
    // "w":300.0,"h":200.0}; Answer { "r12", {"answers":[["Postgres"]]} } -> answer_request {"request":"r12",
    // "answers":[["Postgres"]]}; Show -> show {"workspace":"w3"}. A fake refusing move_block with 422 "No." ->
    // Update::Refused { about: Move("b7k2m"), sentence: "No." }.
}

#[test]
fn a_revoked_share_cuts_the_mirror_for_good() {
    // Live, keys queued while the fake holds calls; set_tokens(vec![]) -> Status(Cut(Revoked)). Then wait_for the
    // fake's streams() == 0 and its client_count() == 0 (the mirror opened nothing again), drain: no
    // Status(Reconnecting) among the updates; release the held calls: the fake saw no further terminal_input.
}

#[test]
fn forget_revokes_then_deletes_the_key() {
    // a share saved with KeySource::Save in a mock store; forget("r1", true) -> the fake saw revoke_share {}; the
    // entry is gone from the store (Keys::load is Err); updates() then has nothing for r1.
}

#[test]
fn nothing_shows_the_key() {
    // token text "tpl_SECRETSECRETSECRET"; collect every update of a run that goes Live, then Cut(Revoked), with a
    // proxy auth and an access secret set; plus format!("{:?}") of the Remote, KeySource::Save, Secrets, every Why
    // variant and Failure: none contains "SECRETSECRET", "acc-secret" or the proxy's password; every "tpl_" found is
    // followed by "…".
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -j 4 -p templum-api --test mirror` → FAIL.
- [ ] **Step 3: Implement.** (`Fake::hold_calls` is a test-only gate in `tests/common/mod.rs`: the drain thread waits
  on a `Condvar` before answering `tools/call terminal_input` while held.)
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api --test mirror` three times, `--lib client::` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 8: Docs, minors, the full check and the benches

**Files:**
- Modify: `docs/specs/templum-to-templum.md` (Status: "Batches 1a and 1b implemented (home's side; the mirror's
  client)"; "As built ▸ Batch 1b": the module layout, the trust marks, `Limits`, frames fed as read (nothing is held
  between frames), anything else chosen or changed), `docs/ARCHITECTURE.md` (`templum-api`'s row: "both ends" and
  `client/`; `templum-terminal`'s row: `Feeder`; the Runtime section: one `templum-mirror` thread with a
  current-thread runtime, orders in and updates out, the UI thread never waits), `docs/minors.md`
- docs/api.md waits for plan 1d (the person's section comes with the dialogs).

**docs/minors.md** (`## Templum to Templum`), at least:
- The system's proxy settings and PAC files aren't read, nor NTLM or Kerberos (the spec's call): a proxy that needs
  them refuses with 407. Fix: a later card.
- A frame is fed on the client's thread under the viewer's fair lock, which may wait for a paint's copy. Fix: if a
  bench shows it, feed with `try_lock` and keep one merged pending frame.
- Anything else deferred while building.

- [ ] **Step 1: Write the docs** above.
- [ ] **Step 2: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass (no Secret Service
  answers there: the tests must not need one).
- [ ] **Step 3: The benches** (`templum-terminal` changed), one after the other: `cargo run -j 4 --release -p templum
  -- --bench`, then `-- --bench-mixed`: both `p95_ms` ≤ 16.7; report their JSON lines.
- [ ] **Step 4: Leave uncommitted;** report the files changed, the `Cargo.lock` packages added, the checks' and benches'
  results, and what went to docs/minors.md.

## Open for the orchestrator

1. **Frames are fed as they are read** rather than merged into one pending frame per terminal (the spec's memory rule
   holds: nothing waits between frames). *Recommended:* keep; the minor above names the fallback.
2. **`Status::word` splits "cut" (home ended it) from "refused" (a check failed)** for the mirror's API state.
   *Recommended:* keep.
