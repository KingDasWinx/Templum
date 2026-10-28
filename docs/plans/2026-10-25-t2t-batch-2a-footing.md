# Templum to Templum, Batch 2a (The Mirror's Footing) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer in a Templum agent block does the whole plan, then
> fresh Opus 5.5 reviewers review it in a loop until one passes it. The implementer **commits nothing**: everything
> stays uncommitted for the orchestrator.

**Goal:** Kanban card 171 part 2, batch 2, first of eight parts: the footing every later part acts through. The
mirror's client gains **answered calls** (any tool, its answer handed to the UI without the UI waiting), a **bounded
queue** (64, a block's newest move only), a **slow connection** for long calls, **11 terminal streams**, **home's
tools** read at each connect and **notices**; taking a remote's state **copies pointers, not bytes**. The glue gains
**the one rule** that says what the mirror can do and why not, **holds made general**, home's notices as
**notifications** with their cap, and the Share… dialog's new words.

**Architecture:** `templum-api`: the state's collections hold `Arc` items (serde's `rc`, already on in the tree), so a
take shares every item that didn't change; the client's coordinator owns a queue of calls per attempt and feeds two
lanes (the calls connection and a slow one) one call at a time, answering each through a one-shot. `crates/templum`:
`Workspace::remote_call` awaits that one-shot in a gpui task; `Deed` and `RemoteSpace::why_not` replace the blanket
`on_remote` refusals of what a person does; `Holds<K, V>` (`workspace/holds.rs`) is batch 1's move hold, generic.

**Tech Stack:** Rust 2024; GPUI through gpui-kit (pinned); tokio's `sync::oneshot` (the `sync` feature is on);
`templum-api`'s client (batch 1b) and glue (batches 1c, 1d).

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), "Batch 2: the whole workspace, live":
"Parts ▸ 2a", "One rule for what the mirror can do", "The mirror's side", "Notices", "The canvas ▸ Holds",
"Constraints (batch 2)", "Left to the builder (batch 2)", the threat model's *(batch 2)* rows and "As built ▸ Batch
1a–1d". The code it builds on: `crates/templum-api/src/{model.rs,client/{mod,remote,http}.rs}`,
`crates/templum-api/tests/{mirror.rs,common/mod.rs}`, `crates/templum/src/workspace/{remote,api_state,history,
input,selection,blocks,files,git_ops,spaces,note_tabs,checklist_blocks,tiling,drawing,waits,agent,delegate,api}.rs`,
`crates/templum/src/{toolbar,share_dialog}.rs`, `crates/templum/src/workspace/tests/{remote,mirror_end_to_end,api}.rs`.

**Consumes:** batch 1b (`Mirror`, `Command`, `Update`, `About`, `Shared`, `Life`, `calls_task`, `Conn::call`,
`failure`), batch 1c (`RemoteSpace`, `apply_remote`, `mirror_blocks`, `tidy`, `on_remote`, `NOT_YET`), batch 1d
(`remote_moved`, `held`, `rect_shown`, `expire_holds`, `remote_answer`, `remote_blocked`, `typing_at`,
`remote_tells`, `fold_tells`, `heard`, `said`, the end-to-end test's `mirrored`).

## Global Constraints

- **MEMORY LIMITS on this machine:** **every** cargo command takes `-j 4`; **never two cargo commands at once**; never
  `cargo clean`; while working run only the targeted tests each step names; the full suite **once**, in Task 7.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-api` stays UI-free. No `unsafe`. **No new
  dependency:** serde's `rc` feature is already enabled in the tree (`cargo tree -e features -i serde` shows it), so
  naming it in `crates/templum-api/Cargo.toml` adds no crate; `cargo deny check` passes unchanged.
- **The UI thread never waits on the network:** `Mirror::call` returns a future the glue awaits in a gpui task
  (`cx.spawn` + `self.window_handle`, as `api_say` does); nothing awaits it on the UI thread.
- **Home is the truth:** what the mirror shows ahead of home (a hold) lasts `HOLD` (2 s) at most; a refusal ends it at
  once and shows its sentence. **Home's automations run only at home** (nothing in 2a touches them).
- **Never a relay:** no remote item enters the mirror's stores, tools or API; the invariant guards (`spawn_block_with`,
  `set_wire`, `set_label`, `begin_ink`, `Space::takes_blocks`) stay as they are.
- **Values, verbatim:** 64 calls waiting per remote; 11 terminal streams (was 12); the slow connection's limit 10
  minutes; an answer at most 1 MiB, 8 MiB for `git` and `read_terminal`; home's tools at most 500, a tool's or an
  argument's name at most 64 bytes; a notice's text its first line, at most 300 characters; notices 3 in 60 s per
  remote, then one "N more notices from …" a minute at most; `HOLD` 2 s.
- **Words, verbatim** (winx-desktop is home's computer name, `computer_of`): "Not connected."; "Couldn't reach the
  other computer."; "winx-desktop is slow to answer: try again."; "winx-desktop didn't answer in time: it may still be
  running there."; "winx-desktop's Templum is older: update it to do this from here."; "Needs a Full share: share
  again on winx-desktop."; "Not on a mirrored workspace yet"; a notice's notification titled "API on winx-desktop"
  (the remote workspace's name, " on ", the computer); "1 more notice from winx-desktop", "N more notices from
  winx-desktop". The Share… dialog: **Control** "Watch and work with blocks, cards, checklists, Git and automations,
  open agents and hand them tasks, answer requests. No typing into terminals; agents it opens can run commands."
  **Full** "Also type and click into terminals, discard Git changes, delete branches, stashes and automations. Anyone
  with the code can run commands on this computer."
- **Batch 1's words stay:** "Not connected: what you type isn't sent."; "Typing needs a Full share: share again on
  winx-desktop."; every batch 1 test keeps passing except those this plan rewrites (named in their task).
- **Every logic change has a test that fails without it;** UI-only changes say how they were verified. Headless GPUI
  tests; keys in `keyring_core::mock::Store`; home on loopback in the same process; conditions with a 10 s deadline,
  never a fixed sleep; a test that needs a shell is `#[cfg(unix)]`. CI runs Linux, macOS and Windows with no agent CLI.
- **Every feature in the API:** 2a adds no tool and no state field at home; the one change to what home sends (a
  watch's notice names its block and workspace) gets its line in `docs/api.md`.
- **Minors** go to `docs/minors.md` under `## Templum to Templum`; the three 2a fixes leave it (Task 7). Docs lines ≤
  120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **A slow call the tunnel cuts** (Cloudflare answers 524 after 100 s, or closes the connection): the person reads
   "winx-desktop didn't answer in time: it may still be running there." and the remote stays Live; it is never cut as
   "didn't answer as a Templum". → Task 2 `a_slow_call_a_tunnel_cuts_is_told_and_never_cuts_the_remote`.
2. **The connection drops with calls waiting:** each waiting and in-flight call is answered once, "Couldn't reach the
   other computer.", and none is sent by the next attempt. → Task 2
   `calls_waiting_when_the_attempt_drops_are_refused_once`.
3. **Home's tools not read yet** (Live before `GET /v1/tools` answers, or its read failed): nothing is dimmed as
   older. → Task 4 `the_one_rule_says_why_in_its_order`.
4. **A notice about a block the mirror doesn't draw** (a card, or past the 500): it still shows; its click shows the
   workspace. → Task 6 `a_notice_about_a_block_not_drawn_here_still_shows`.
5. **An answer that lands late** (its request already left the inbox, or its remote was forgotten): nothing changes,
   nothing panics. → Task 2 `an_answer_after_its_request_left_changes_nothing`.

---

### Task 1: The state handed out cheaply

**Files:**
- Modify: `crates/templum-api/Cargo.toml` (serde `features = ["derive", "rc"]`), `crates/templum-api/src/model.rs`
  (`State`'s collections, `Edit::run`, `edit`, `same`), `crates/templum-api/src/client/mod.rs` (`Shared::take`;
  `Mirror::state` calls it)
- Modify: `crates/templum/src/workspace/api_state.rs` (home builds `Arc` items),
  `crates/templum/src/workspace/remote.rs` (`RemoteSpace::state`, `apply_remote`, `tidy` → `tidy_block` at use)
- Modify (compile only): every `State { … vec![…] }` and `.blocks.push(…)` the compiler names, in `templum-api`'s
  `events.rs`, `server.rs`, `openapi.rs`, `tests/common/mod.rs`, `tests/events.rs`, and the glue's tests
- Test: `crates/templum-api/src/client/mod.rs` (`mod tests`), `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `templum_api::model`:
  - `State`'s `workspaces`, `blocks`, `wires`, `requests`, `boards`, `cards`, `automations`, `shares` become
    `Vec<Arc<T>>` (`std::sync::Arc`); `app`, `usage`, `view` stay as they are. Serialized and read exactly as before.
  - `Edit::run` keeps an item's `Arc` when the put equals it, replaces it with a new `Arc` when it differs, pushes a new
    `Arc` for a new id; `State::replace` (a snapshot) is unchanged.
  - `pub fn same<T>(a: &[Arc<T>], b: &[Arc<T>]) -> bool`: the same length and every item `Arc::ptr_eq`, in order.
- Produces, in `templum_api::client`: `Shared::take(&self, id: &str) -> Option<State>` (what `Mirror::state` did: the
  newest state when fresh, as a clone that copies pointers).
- Produces, in the glue: `RemoteSpace::state: State`, home's state as last taken (its items the client's own);
  `fn tidy_block(b: &BlockView) -> BlockView`, a cleaned copy (what `tidy` did to a block: title, kind, the agent's
  words); `tidy` goes. Home's workspace name is cleaned (`clean_title`) where `apply_remote` takes it, a wire's label in
  `mirror_wires`.
- `apply_remote(id, state)`: when `same` holds for `workspaces`, `blocks`, `wires` and `requests` against
  `remote.state`, it runs none of `mirror_blocks`, `mirror_wires`, `remote_requests` (a change of cards, boards,
  automations or shares alone costs nothing); otherwise all three, as now (they depend on each other: a wire or a
  request needs its block mirrored). The name, look and first camera run as now. Then `remote.state = state`.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum-api/src/client/mod.rs, mod tests
#[test]
fn a_take_shares_every_item_that_did_not_change() {
    let shared = Shared::for_tests();
    shared.inner().current.insert("r1".into(), 1);
    let block = |id: &str, title: &str| Arc::new(BlockView {
        id: id.into(), workspace: "w3".into(), kind: "terminal".into(), title: title.into(), ..Default::default()
    });
    let snapshot = serde_json::to_value(State {
        blocks: vec![block("b1", "one"), block("b2", "two")],
        cards: vec![Arc::new(CardView { id: 40, board: 3, title: "Ship".into(), ..Default::default() })],
        ..Default::default()
    }).unwrap();
    shared.apply("r1", 1, "snapshot", &snapshot, 100).unwrap();
    let first = shared.take("r1").unwrap();
    let put = |b: Arc<BlockView>| json!({ "op": "put", "id": b.id.clone(), "value": &*b });
    shared.apply("r1", 1, "block", &put(block("b1", "new")), 50).unwrap();
    shared.apply("r1", 1, "block", &put(block("b2", "two")), 50).unwrap(); // the same item again
    let second = shared.take("r1").unwrap();
    assert_eq!(second.blocks[0].title, "new");
    assert!(!Arc::ptr_eq(&first.blocks[0], &second.blocks[0]));
    assert!(Arc::ptr_eq(&first.blocks[1], &second.blocks[1]), "an equal put keeps the item");
    assert!(Arc::ptr_eq(&first.cards[0], &second.cards[0]));
    assert!(shared.take("r1").is_none(), "nothing newer");
}
```

(`CardView` may lack `Default`: build it with its fields, as `model::tests` does. `model::same` gets one line in this
test: `assert!(model::same(&first.cards, &second.cards) && !model::same(&first.blocks, &second.blocks))`.)

```rust
// crates/templum/src/workspace/tests/remote.rs
/// A take whose blocks, wires, requests and workspaces are the last take's own leaves the canvas alone.
#[gpui_kit::test]
fn a_take_with_home_s_blocks_unchanged_rebuilds_none(cx: &mut TestAppContext) {
    // offline(); apply terminal_of_home(None) → the viewer; set its pane's `remote.title` to "local".
    // Apply a clone of that state with one card pushed (`state.cards.push(Arc::new(card))`):
    //   the title is still "local" (nothing rebuilt).
    // Apply a state whose block has a new title (a new Arc): the title is home's again.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-api --lib a_take_shares` → fails to compile
  (`take`, `Arc` items). The glue test can't compile either until Step 3.
- [ ] **Step 3: Implement** the interfaces above. Home's `api_state` wraps each item (`.map(Arc::new)` before
  `.collect()`); `State::visible` compiles as it is (`.cloned()` clones the `Arc`). A test that applied the same state
  object twice and expected the second to act now meets the skip: give it a fresh state. The existing
  `home_s_words_are_one_short_line_wherever_they_reach` proves the cleaning moved, not lost.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api` then `cargo test -j 4 -p templum workspace::tests::remote` and
  `cargo test -j 4 -p templum workspace::tests::mirror_end_to_end` → all pass.

---

### Task 2: Answered calls, the bounded queue, the slow connection and 11 streams

**Files:**
- Modify: `crates/templum-api/src/client/mod.rs` (`Call`, `Refusal`, `Answered`, `Command::Call`, `Mirror::call`,
  `About`), `client/remote.rs` (the queue, the lanes, `STREAMS_MOST`), `client/http.rs` (`call_within`, the limits,
  524)
- Modify: `crates/templum-api/tests/common/mod.rs` (`Fake::hold_tool`, `held`, `release_tool`)
- Modify: `crates/templum/src/workspace/remote.rs` (`remote_call`, `Then`, `remote_answer`, `remote_act`'s `StartIt`,
  `refused`, `MOST_STREAMS`), `crates/templum/src/workspace/mod.rs` (the test door beside `said`)
- Test: `crates/templum-api/tests/mirror.rs`, `client/remote.rs` and `client/http.rs` (`mod tests`),
  `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `templum_api::client`:
  - `pub struct Call { pub tool: String, pub args: Value, pub slow: bool }` (`slow`: on the slow connection).
  - `#[derive(Clone, Debug, PartialEq)] pub struct Refusal { pub sentence: String }`; `pub type Answered =
    Result<Value, Refusal>` (home's JSON: `{"text": …}` and what it touched).
  - `Command::Call { call: Call, answer: tokio::sync::oneshot::Sender<Answered> }`; `Command::Answer` and
    `Command::Show` go (they become calls). `About` is `Keys(String)`, `Move(String)` and `Key` (was `Show`, which only
    `prepare` used for a key it can't keep).
  - `Mirror::call(&self, id: &str, call: Call) -> impl Future<Output = Answered> + Send + 'static`: sends the command
    **at once** (before the future is polled), then awaits the one-shot; a dropped sender (the remote forgotten or
    unknown) is `Err("Couldn't reach the other computer.")`.
- The coordinator (`client/remote.rs`): `STREAMS_MOST = 11`; `CALLS_WAITING_MOST = 64`. The attempt owns
  `jobs: VecDeque<Job>` (`Job { call: Call, to: To }`, `enum To { Move(String), Answer(oneshot::Sender<Answered>) }`)
  and, per lane, the `To` of the one call on it. `Command::Move` is a job (`move_block`, the quick lane) whose waiting
  twin for the same block is replaced in place; `Command::Call` is a job. `pump_calls` hands the next waiting job of a
  free lane to its task. The quick lane is `calls_task` as now (`GET /v1/me`, then `GET /v1/tools` once, Task 3, then
  calls; 30 s each); the slow lane is `calls_task` with `Lane::Slow` (no `/v1/me`, `SLOW_CALL`), spawned at the
  attempt's first slow job, so an idle mirror holds 15 connections. `Msg::Called` names its lane.
  - Not live: `Err("Not connected.")` at once (a move: `Update::Refused { About::Move, "Not connected." }`, as now).
  - 64 waiting: a new job is refused at once: "{computer} is slow to answer: try again." (`me.computer`, else "The
    other computer"); a move that replaces its twin is never refused.
  - `drop_attempt`: every waiting and in-flight job is answered "Couldn't reach the other computer." (a move through
    `Update::Refused`), once.
  - An answer: `Ok(json)` (the quick lane also says `Latency`); `Refused`/`Missing` → `Err(Refusal { sentence })`; on
    the quick lane `Down`/`Busy`/`TooLarge` → "Couldn't reach the other computer."; `Auth` and `Stop` end the attempt as
    now. **On the slow lane** every failure but `Auth`, `Refused` and `Missing` (a timeout, a closed connection,
    `Stop`) is "{computer} didn't answer in time: it may still be running there." and drops the slow connection
    only; the remote stays as it is.
- `client/http.rs`: `Conn::call_within(&mut self, method, path, body, headers, limit: Duration, most: usize)`; `call`
  is `call_within(…, CALL, ANSWER_MOST)`; `pub(crate) const SLOW_CALL: Duration` (600 s), `LARGE_ANSWER_MOST` (8 MiB),
  for jobs whose tool is `git` or `read_terminal`; `failure`: 524 joins 502, 504, 530 as `Down`.
- The fake home (`tests/common/mod.rs`): `Fake::hold_tool(&self, name: &str)` parks the `tools/call`s of that tool;
  `Fake::held(&self) -> usize` counts them; `Fake::release_tool(&self)` answers each "did {name}" and stops holding.
- Produces, in the glue:
  - `pub(crate) type Then = Box<dyn FnOnce(&mut Workspace, Answered, &mut Window, &mut Context<Workspace>)>`.
  - `pub(crate) fn remote_call(&mut self, space: &str, call: Call, then: Then, cx: &mut Context<Self>)`: with a client,
    `mirror.call`, awaited in a task that runs `then` through `self.window_handle`, the refusal's sentence first
    through `clean_to(…, 200)`; without one, `then(Err("Not connected."))` in the same way, except under `cfg(test)`
    where the call is kept in `Workspace::calls: Vec<(String, Call, Then)>` (beside `said`) for
    `#[cfg(test)] pub(crate) fn answer_call(&mut self, n: usize, answer: Answered, window, cx)` to answer.
  - `remote_answer` calls `answer_request` through `remote_call`; its `then`, on `Err`, finds the wait by its key (none:
    nothing) and sets `sent = false` and warns the sentence. `Act::StartIt` calls `show` with `{"workspace":
    entry.home}` (no home yet: warns "Not connected."); its `then` warns a refusal. `refused` loses its
    `About::Answer` branch; `About::Key` warns as `Show` did. `MOST_STREAMS = 11`.

- [ ] **Step 1: Write the failing tests** (`tests/mirror.rs`, with `live_on` and `calls_of`; `block_on` is
  `futures::executor::block_on`; `call(tool, args, slow)` builds a `Call`):

```rust
#[test]
fn a_call_is_answered_with_home_s_json() {
    // Full share of w3, live: block_on(mirror.call("r1", call("show", json!({"workspace": "w3"}), false)))
    //   == Ok(v) with v["text"] == "did show"; calls_of(&fake, "show") == [json!({"workspace": "w3"})].
}

#[test]
fn a_refused_call_says_home_s_words() {
    // close_block (the fake refuses it "No."): Err(Refusal { sentence: "No.".into() }).
}

#[test]
fn past_sixty_four_waiting_calls_are_refused_at_once() {
    // fake.hold_tool("show"); one call; wait_for held() == 1; 64 more (futures kept, not awaited);
    // the 66th: block_on → Err(Refusal { sentence: format!("{computer} is slow to answer: try again.") }),
    // computer being the fake's `me.computer`. release_tool(); the first future is Ok.
}

#[test]
fn a_block_s_waiting_moves_keep_only_the_newest() {
    // hold_tool("show"); a show; wait_for held() == 1; Command::Move b7k2m at x = 1.0, 2.0, then 3.0; release_tool();
    // wait_for one move_block: calls_of(&fake, "move_block") == [json!({"block": "b7k2m", "x": 3.0, "y": …, …})].
}

#[test]
fn a_quick_call_is_answered_while_a_slow_one_runs() {
    // hold_tool("show"); let slow = mirror.call("r1", call("show", …, true)); wait_for held() == 1;
    // block_on(mirror.call("r1", call("move_block", json!({"block": "b7k2m", "x": 1.0}), false))) is Ok while
    // held() == 1; release_tool(); block_on(slow) is Ok.
}

#[test]
fn calls_waiting_when_the_attempt_drops_are_refused_once() {
    // hold_tool("show"); three calls; wait_for held() == 1; Command::Disconnect;
    // each future → Err("Couldn't reach the other computer."); Command::TryNow, live again; release_tool();
    // after a barrier (`barrier(&mirror)`), calls_of(&fake, "show").len() == 1 (none was sent again).
}
```

Rewrite, same file: `moves_answers_and_show_are_calls` → `moves_and_calls_reach_home` (the move and its refusal as
now; the answer and the show through `mirror.call`); `a_command_while_not_live_is_refused_or_dropped` (the show:
`block_on(mirror.call(…))` == `Err("Not connected.")`); `at_most_twelve_terminals_stream` →
`at_most_eleven_terminals_stream` (11).

```rust
// client/remote.rs, mod tests (as a_move_home_answers_busy_to_is_made_again drives a Life on a raw server)
#[test]
fn a_slow_call_a_tunnel_cuts_is_told_and_never_cuts_the_remote() {
    // A raw server that answers POST /v1/tools/git with "HTTP/1.1 524" and no Templum body;
    // a live Life; a slow Call of git: its answer is Err(format!("{computer} didn't answer in time: it may still be
    // running there.")), and no Msg::Lost / no status but Live is said.
}
// Rewrite a_call_in_flight_when_the_attempt_is_dropped_is_refused with two Command::Call jobs.

// client/http.rs, failures_say_what_happened: assert!(matches!(failure(524, &with(&[]), b"", "/v1/tools/git"),
//     Failure::Down(_)));
```

```rust
// crates/templum/src/workspace/tests/remote.rs (offline(): no client, so calls land in ws.calls)
#[gpui_kit::test]
fn an_answer_home_refuses_shows_its_request_again(cx: &mut TestAppContext) {
    // Replaces the About::Answer halves of a_request_the_share_cant_answer_is_dim_and_answers_nothing and
    // an_answer_lost_with_the_connection_shows_its_request_again: a listed request answered → ws.calls ==
    // [("r1", Call { tool: "answer_request", args: {"request": …, "answers": …}, slow: false })] and the inbox empty;
    // answer_call(0, Err(Refusal { sentence: "Already answered." })) → the inbox shows it again and
    // said.last() == "Already answered.".
}

#[gpui_kit::test]
fn an_answer_after_its_request_left_changes_nothing(cx: &mut TestAppContext) {
    // Answer a request; apply a state without it (the wait goes); answer_call(0, Err(…)) → no panic, waits empty.
}

#[gpui_kit::test]
fn start_it_asks_home_to_show_its_workspace(cx: &mut TestAppContext) {
    // remote_act("r1", Act::StartIt) with entry.home = Some("w3") → ws.calls[0] is show {"workspace": "w3"};
    // answer_call(0, Err("No.")) → said.last() == "No.".
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-api --test mirror` → fails to compile
  (`Call`, `Mirror::call`, `hold_tool`).
- [ ] **Step 3: Implement** the interfaces above. `the_nearest_twelve_stream_and_the_focused_one_always` becomes
  eleven.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api` then `cargo test -j 4 -p templum workspace::tests::remote` and
  `cargo test -j 4 -p templum workspace::tests::mirror_end_to_end` → all pass.

---

### Task 3: Home's tools and notices reach the UI, and a watch's notice names its block

**Files:**
- Modify: `crates/templum-api/src/client/mod.rs` (`Tools`, `Update::Tools`, `Update::Notice`), `client/remote.rs`
  (`calls_task` reads `GET /v1/tools` once; `Life::event` hears `notice`)
- Modify (home): `crates/templum/src/workspace/agent.rs` (`notice_to`), `delegate.rs` (`push_notice`), `api.rs`
  (`api_notice_about`)
- Modify: `docs/api.md` (the `notice` event's line)
- Test: `client/mod.rs` (`mod tests`), `crates/templum-api/tests/mirror.rs`, `crates/templum/src/workspace/tests/api.rs`

**Interfaces:**
- Produces, in `templum_api::client`:
  - `#[derive(Clone, Debug, Default, PartialEq)] pub struct Tools` (by name, each tool's argument names):
    `pub fn read(list: &Value) -> Tools` (`GET /v1/tools`' array: each item's `name` and the keys of its
    `inputSchema.properties`; at most `TOOLS_MOST` (500) tools; a tool or argument whose name passes `NAME_MOST` (64
    bytes) or has a control character is left out; anything not an array is none), `pub fn has(&self, tool: &str) ->
    bool`, `pub fn takes(&self, tool: &str, argument: &str) -> bool` (2b's rung-2 arguments).
  - `Update::Tools(Tools)`: after the first `GET /v1/me` of each attempt the calls task asks `GET /v1/tools` once
    (`Msg::Tools { epoch, result }`); a failure says nothing (tools stay unknown).
  - `Update::Notice(Notice)` (`templum_api::model::Notice`) for each `notice` event: `text` its first line, at most
    `NOTICE_CHARS` (300) characters; `block` and `workspace` kept only when at most 64 bytes with no control
    character. A malformed notice is dropped (it is not state: the stream goes on).
- Home: a notice for the API's asker names the terminal it is about, task or not:
  `api_notice_about(&mut self, text: String, terminal: Option<&str>)`; `notice_to` sends a notice for
  `templum_api::ASKER` straight there with `Some(terminal)`; `push_notice`'s API branch passes its `about`'s terminal.
  So a watch set through the API (`wait_for` with `wait: false`) carries the watched block and its workspace, and a
  token limited to that workspace (a share) hears it (`Caller::sees(None)` is false for a limited token).

- [ ] **Step 1: Write the failing tests**

```rust
// client/mod.rs, mod tests
#[test]
fn home_s_tools_are_capped_and_read_by_name_and_argument() {
    let mut list: Vec<Value> = (0..600)
        .map(|i| json!({ "name": format!("t{i}"), "inputSchema": { "properties": { "a": {}, "x".repeat(65): {} } } }))
        .collect();
    list.insert(0, json!({ "name": "y".repeat(65) }));
    let tools = Tools::read(&Value::Array(list));
    assert!(tools.has("t0") && tools.has("t498") && !tools.has("t500") && !tools.has(&"y".repeat(65)));
    assert!(tools.takes("t0", "a") && !tools.takes("t0", "b") && !tools.takes("t0", &"x".repeat(65)));
    assert_eq!(Tools::read(&json!({"tools": []})), Tools::default());
}
```

```rust
// tests/mirror.rs
#[test]
fn home_s_tools_come_at_each_connect() {
    // live_on; until Update::Tools(t) with t.has("move_block") && t.takes("move_block", "x") && !t.has("nope");
    // Command::Disconnect, Command::TryNow: another Update::Tools comes.
}

#[test]
fn a_notice_reaches_the_ui_as_its_first_line() {
    // live_on (share of w3); fake.server.notice(Notice { text: format!("{}\nsecond", "é".repeat(400)),
    //   block: Some("b7k2m".into()), workspace: Some("w3".into()) });
    // until Update::Notice(n) with n.text == "é".repeat(300) && n.block.as_deref() == Some("b7k2m").
}
```

```rust
// crates/templum/src/workspace/tests/api.rs
#[cfg(unix)]
#[gpui_kit::test]
fn a_watch_set_through_the_api_names_its_block_and_workspace(cx: &mut TestAppContext) {
    // A terminal running cat and `prints`, as a_watch_without_wait_types_its_match_back_once has them;
    // with_token(Control, None); tool("wait_for", {"block": uid, "pattern": "ERROR", "wait": false}) answers
    // "… a match comes back as a notice event."; prints b"ERROR one\r"; after a few `after(cx, 250)`:
    // ws.api.notices[0].block == Some(uid) and .workspace == Some(the terminal's workspace id).
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-api --lib home_s_tools` → fails to compile.
- [ ] **Step 3: Implement** the interfaces above; `docs/api.md`'s `event: notice` sentence gains: "A watch set through
  the API (`wait_for` with `wait: false`) names the watched block and its workspace, so a token limited to that
  workspace hears it."
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api` then `cargo test -j 4 -p templum workspace::tests::api` → all
  pass.

---

### Task 4: The one rule for what the mirror can do

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Deed`, `Nay`, `RemoteSpace::{why_not, tools}`,
  `Workspace::why_not`, `heard`'s `Update::Tools`, `typing_at`, `remote_blocked`; `remote_frozen` goes; `on_remote`'s
  doc says it guards what a remote must never hold), `crates/templum/src/toolbar.rs` (`gated`, `button`)
- Modify (each guard named below): `workspace/{files,git_ops,spaces,blocks,note_tabs,checklist_blocks,selection,
  tiling,drawing,input,history}.rs`
- Test: `crates/templum/src/workspace/remote.rs` (`mod tests`), `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in the glue (`workspace/remote.rs`):

```rust
/// What the person does on a remote workspace (docs/specs/templum-to-templum.md, One rule): each is a call of
/// home's, or not on a mirror yet. A later part gives a deed its call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Deed {
    Type, Move, Answer(Scope), Start,
    AddTerminal, AddNote, AddChecklist, AddKanban, AddEditor, OpenAgent, OpenNote, OpenChecklist,
    Close, Wire, Draw, Grid, MoveToWorkspace, CopyBlocks, Git,
}

/// Why a deed can't be done now, in the order the rule asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Nay { NotYet, NotConnected, Older, NeedsFull }
```

  - `Deed::call(self) -> Option<(&'static str, Value)>`: the tool and arguments enough for `registry::scope`: `Type`
    `terminal_input` `{"block":"b","keys":["a"]}`; `Move` `move_block` `{"block":"b","x":0.0,"y":0.0}`; `Answer(_)`
    `answer_request` `{"request":"r1"}`; `Start` `show` `{"workspace":"w"}`; every other deed None in 2a.
  - `RemoteSpace::why_not(&self, deed: Deed) -> Option<Nay>`: no call → `NotYet`; not `Status::Live` →
    `NotConnected`; `tools` known and without the tool → `Older` (unknown tools dim nothing); the scope
    `registry::scope(tool, &args)` (for `Answer(s)`, the greater of it and `s`) above `me.scope` (or no `me`) →
    `NeedsFull`; else None.
  - `Nay::words(self, computer: &str) -> String`: the Global Constraints' four sentences (`NotYet` is `NOT_YET`).
  - `Workspace::why_not(&self, deed: Deed) -> Option<String>`: for the shown workspace, None when it is local, else
    its remote's `Nay` in words (`computer_of`).
  - `RemoteSpace::tools: Option<Tools>`, set by `Update::Tools`, None again in `replace_code`.
- The guards, from blanket to the rule (each keeps doing nothing when refused, as now): toolbar Wire `Wire`, Terminal
  `AddTerminal`, Note `AddNote`, Checklist `AddChecklist`, Kanban `AddKanban`, Editor `AddEditor`, AI agent
  `OpenAgent`, Pen/Highlighter/Shapes/Eraser `Draw`; `files.rs` `new_editor`, `ask_editor_folder`, `add_editor_at`
  `AddEditor`; `git_ops.rs` `toggle_git`, `open_git` `Git`; `spaces.rs` `move_blocks` `MoveToWorkspace`;
  `blocks.rs` `add_kanban_at` `AddKanban`, `open_shell` and `open` `AddTerminal`, `close_blocks` `Close`;
  `note_tabs.rs` `new_note`, `add_note_at` `AddNote`, `open_note` `OpenNote`; `checklist_blocks.rs` `new_checklist`
  `AddChecklist`, `open_checklist` `OpenChecklist`; `selection.rs` `copy_blocks`, `paste_blocks` `CopyBlocks`;
  `tiling.rs` `toggle_grid` `Grid`; `drawing.rs` `set_tool` (`Wire` for the Wire tool, else `Draw`) and
  `toggle_shapes` `Draw`; `remote_frozen`'s callers (`input.rs` ×2, `selection.rs`, `history.rs`) `Move`.
  `remote_blocked(wait)` is `why_not(Deed::Answer(ask.scope))` in words; `typing_at` asks `Deed::Type` and keeps
  batch 1's own sentences for `NotConnected` and `NeedsFull` (an `Older` is its words, told once).
- Stay `on_remote()`, as invariants: `spawn_block_with`, `set_wire`, `set_label`, `begin_ink`, `switch_to`'s tool
  reset (`spaces.rs`), and `open_menu`'s refusal (2b opens the menus).
- `toolbar.rs`: `fn gated(ws: &Workspace, deed: Deed, tip: impl Into<SharedString>) -> (SharedString, bool)` (the
  rule's words and dimmed, or the tip and not); `button` dims on that flag, not on `tip == NOT_YET`.

- [ ] **Step 1: Write the failing tests**

```rust
// workspace/remote.rs, mod tests (remote_at(url, status) is there)
#[test]
fn the_one_rule_says_why_in_its_order() {
    let mut remote = remote_at("https://winx-desktop.example.com", Status::Reconnecting { attempt: 1,
        next: Duration::from_secs(1), live_at: None });
    let words = |r: &RemoteSpace, d| r.why_not(d).map(|n| n.words("winx-desktop"));
    assert_eq!(words(&remote, Deed::AddTerminal).as_deref(), Some("Not on a mirrored workspace yet"));
    assert_eq!(words(&remote, Deed::Move).as_deref(), Some("Not connected."));
    remote.status = Status::Live;
    remote.me = Some(MeView { scope: Scope::Control, ..me() }); // a MeView as the module's tests make one
    assert_eq!(remote.why_not(Deed::Move), None, "tools unknown: nothing is older");
    assert_eq!(words(&remote, Deed::Type).as_deref(), Some("Needs a Full share: share again on winx-desktop."));
    assert_eq!(remote.why_not(Deed::Answer(Scope::Control)), None);
    assert_eq!(remote.why_not(Deed::Answer(Scope::Full)), Some(Nay::NeedsFull));
    remote.tools = Some(Tools::read(&json!([{ "name": "show" }])));
    assert_eq!(words(&remote, Deed::Move).as_deref(),
        Some("winx-desktop's Templum is older: update it to do this from here."));
    assert_eq!(remote.why_not(Deed::Start), None);
}

#[test]
fn every_deed_home_runs_has_a_scope_in_the_registry() {
    // For Type, Move, Answer(Scope::Control), Start: registry::scope(tool, &args) is Ok; Type's is Full, Move's and
    // Start's Control.
}
```

```rust
// tests/remote.rs
#[gpui_kit::test]
fn the_toolbar_dims_a_remote_s_controls_with_the_rule_s_words(cx: &mut TestAppContext) {
    // offline, r1 shown: crate::toolbar::gated(ws, Deed::AddTerminal, "Terminal") == ("Not on a mirrored workspace
    // yet", true); on a local workspace == ("Terminal", false). (`gated` becomes pub(crate) for it.)
}
```

`a_remote_workspace_refuses_what_it_cant_do_yet`, `typing_says_why_it_isnt_sent`,
`a_request_the_share_cant_answer_is_dim_and_answers_nothing` (its scope half) and `nothing_is_moved_while_not_live`
pass unchanged: they prove the guards still refuse.

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum the_one_rule` → fails to compile.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::remote` then `cargo test -j 4 -p templum
  workspace::tests::remote` → all pass.

---

### Task 5: Holds made general

**Files:**
- Create: `crates/templum/src/workspace/holds.rs` (`Holds`, `HOLD`, with its `mod tests`)
- Modify: `crates/templum/src/workspace/mod.rs` (`mod holds;`), `crates/templum/src/workspace/remote.rs`
  (`RemoteSpace::held`, `rect_shown`, `remote_moved`, `expire_holds`, `refused`, `mirror_blocks`'s gone blocks;
  `HOLD` re-exported for its users)

**Interfaces:**
- Produces, in `workspace/holds.rs`:

```rust
/// What the person changed here and sees at once, standing over home's state until home's shows it or HOLD passes
/// (docs/specs/templum-to-templum.md, The canvas ▸ Holds): a block moved, and from 2b on a block closed, a wire, a
/// card moved, an item checked.
pub(crate) struct Holds<K, V> { held: HashMap<K, (V, Instant)> }

impl<K: Copy + Eq + Hash, V: Clone> Holds<K, V> {
    pub fn hold(&mut self, key: K, value: V, now: Instant);
    /// What was sent for `key` and not yet shown by home.
    pub fn get(&self, key: &K) -> Option<&V>;
    /// A refusal, or the thing gone: the hold ends at once.
    pub fn end(&mut self, key: &K) -> Option<V>;
    /// What to show for `key`, home having `home`: the value held while it is younger than HOLD and `same` says home's
    /// isn't it yet; else home's, and the hold ends.
    pub fn shown(&mut self, key: K, home: V, now: Instant, same: impl Fn(&V, &V) -> bool) -> V;
    /// The holds older than HOLD end, except those `kept` keeps: the keys that ended, and when the next one is due.
    pub fn expire(&mut self, now: Instant, kept: impl Fn(&K) -> bool) -> (Vec<K>, Option<Instant>);
}
```

- `RemoteSpace::held: Holds<BlockId, Rect>`; `rect_shown` is `held.shown(id, home, now, same_rect)` (and keeps
  `rects`); `expire_holds` is `held.expire(now, |id| placed has it)` and puts each ended block at `rects`; nothing a
  person sees changes.

- [ ] **Step 1: Write the failing tests** (`holds.rs`, `mod tests`, with `let t = Instant::now()` and plain `u64` keys
  and values; `same` is `==`):

```rust
#[test]
fn a_hold_stands_until_home_shows_it_or_hold_passes() {
    // hold(1, 10, t); shown(1, 5, t + 1 s) == 10; shown(1, 10, t + 1.5 s) == 10 and get(&1) is None (home showed it);
    // hold(2, 20, t); shown(2, 5, t + HOLD) == 5 and get(&2) is None.
}

#[test]
fn a_refusal_ends_a_hold_at_once() {
    // hold(1, 10, t); end(&1) == Some(10); shown(1, 5, t) == 5.
}

#[test]
fn expire_spares_what_is_kept_and_says_when_the_next_is_due() {
    // hold(1, 10, t); hold(2, 20, t + 1 s); hold(3, 30, t);
    // expire(t + HOLD, |k| *k == 3) == (vec![1], Some(t + 1 s + HOLD)); get(&3) is Some.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum workspace::holds` → fails to compile.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::holds` then `cargo test -j 4 -p templum
  workspace::tests::remote` (the move holds: `a_moved_block_stands_until_home_shows_it`,
  `a_block_in_the_hand_isn_t_taken_from_it_by_home_s_state`, `a_refused_move_goes_back`,
  `a_block_grabbed_again_isn_t_taken_back_by_the_old_hold`) → all pass.

---

### Task 6: Home's notices as notifications, and the end-to-end watch

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Budget`, `RemoteSpace::{requests_budget, notice_budget,
  notices_folded}`, `remote_tells`/`fold_tells` on `Budget`, `heard`'s `Update::Notice`, `remote_notice`,
  `fold_notices`, `drain_mirror`)
- Test: `crates/templum/src/workspace/tests/remote.rs`, `crates/templum/src/workspace/tests/mirror_end_to_end.rs`

**Interfaces:**
- Produces, in the glue:
  - `struct Budget { at: VecDeque<Instant>, folded_at: Option<Instant> }` with `fn tell(&mut self, now: Instant) ->
    bool` (at most `MOST_NOTICES` in `NOTICE_SPAN`) and `fn fold(&mut self, now: Instant) -> bool` (one a span):
    `RemoteSpace`'s `notices` and `folded_at` become `requests_budget`; `notice_budget` and `notices_folded: usize`
    are the notices' own (a burst of requests never swallows a task's end).
  - `pub(super) fn remote_notice(&mut self, id: &str, notice: Notice, window, cx)`: a notice whose `workspace` isn't
    the remote's (`entry.home`) is ignored; else, when `notice_budget.tell`, a notification titled "{space.name} on
    {computer}", its body `clean_to(text, 300)`, delivered as `delivery(window)` says, whose click reveals the block
    (`reveal`) when `homes` has it and else shows the workspace (`switch_to` its place in `order()`); otherwise
    `notices_folded += 1`. Under `cfg(test)` each one told is pushed to `said` as "{title}: {text}".
  - `pub(super) fn fold_notices(&mut self, id: &str, window, cx)`: with `notices_folded > 0` and `notice_budget.fold`,
    one notification, same title, "1 more notice from {computer}" or "{n} more notices from {computer}", and the count
    starts again. `drain_mirror` calls it for each remote after the updates.

- [ ] **Step 1: Write the failing tests** (offline harness; `notice(text, block)` builds a `Notice` for `entry.home`;
  the remote's name and `computer(ws)` as the module's tests have them):

```rust
#[gpui_kit::test]
fn home_s_notices_are_notifications_titled_by_the_workspace_and_its_computer(cx: &mut TestAppContext) {
    // heard("r1", Update::Notice(notice("[task t7 done] Green.", None))) →
    //   said.last() == format!("{name} on {computer}: [task t7 done] Green.");
    // a notice whose workspace is another of home's → said unchanged.
}

#[gpui_kit::test]
fn a_burst_of_notices_is_three_and_one_of_how_many_more(cx: &mut TestAppContext) {
    // five notices heard, then fold_notices("r1") → said gains 3 notices then "{name} on {computer}: 2 more notices
    // from {computer}"; two more notices and fold_notices at once → nothing more is said (one fold a span).
}

#[gpui_kit::test]
fn a_notice_about_a_block_not_drawn_here_still_shows(cx: &mut TestAppContext) {
    // notice(…, Some("zzzzz")) (no such block in homes) → told, as above.
}
```

```rust
// mirror_end_to_end.rs
#[cfg(unix)]
#[gpui_kit::test]
fn a_watch_set_at_home_through_the_api_shows_its_notice_on_the_mirror(cx: &mut TestAppContext) {
    // As what_home_s_shell_prints_shows_on_the_mirror_and_keys_reach_it: open_as with `sh`, mirrored(…);
    // a Settings token (with_token(Control, None)) sets tool("wait_for", {"block": uid, "pattern": "DEPLOYED",
    // "wait": false, "timeout": 300}); home's shell runs "printf 'DEP%s\n' LOYED\r";
    // pump_home with after(cx, 250) each round until said has an entry that starts with format!("{name} on ") and
    // contains "a line matches \"DEPLOYED\"".
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum home_s_notices` → fails (`Update::Notice` not
  handled).
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::tests::remote` (`a_flood_of_requests_is_a_few_notices_
  and_one_of_how_many_more` and `a_home_that_makes_a_request_every_state_is_one_more_notice_a_span` prove `Budget`)
  then `cargo test -j 4 -p templum workspace::tests::mirror_end_to_end` → all pass.

---

### Task 7: The Share… dialog's words, the docs, the minors, the full check and `--bench-mirror`

**Files:**
- Modify: `crates/templum/src/share_dialog.rs` (`CONTROL_SAYS`, `FULL_SAYS`: the Global Constraints' words)
- Modify: `docs/api.md` (Templum to Templum ▸ Share ▸ **Can:** the same words for a share),
  `docs/specs/templum-to-templum.md` (Status: "Batch 2: part 2a built (plan 2026-10-25-t2t-batch-2a-footing)";
  "As built ▸ Batch 2a": what the builder chose or changed, with this plan's "Open for the orchestrator" as settled),
  `docs/ARCHITECTURE.md` (the client's lanes, answered calls, home's tools and notices, the shared state; the glue's
  `Deed`/`why_not` and `workspace/holds.rs`), `docs/minors.md`

**docs/minors.md:**
- Delete (fixed here): "The whole `State` is cloned on each wake of a mirror." (Task 1); "The calls channel of a
  remote is unbounded." (Task 2); "A key that can't be saved to its file says it as a refusal 'about Show'." (Task 2,
  `About::Key`).
- Add, if built as planned: "Notices folded past the cap are counted until the next wake a minute on" (`fold_notices`
  runs at a drain: a quiet home's count waits for its next update; fix: a one-shot timer at `folded_at + NOTICE_SPAN`).
  And anything else deferred while building.

- [ ] **Step 1: Write the words and docs** above. The Share… dialog's words are UI-only: say they were read in
  `share_dialog.rs` against the spec, and that the dialog was not looked at on a display.
- [ ] **Step 2: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 3: The bench** (the mirror's client changed): `cargo run -j 4 --release -p templum -- --bench-mirror` →
  `p95_ms` ≤ 16.7; report its JSON line.
- [ ] **Step 4: Leave uncommitted;** report the files changed, the checks' and the bench's results, and what went to
  docs/minors.md.

---

## Interfaces for later parts

What 2a leaves for 2b–2h, by name (each defined in the task shown):

- **Calls** (Task 2): `client::{Call, Refusal, Answered}`, `Mirror::call`; the glue's `Workspace::remote_call(space,
  Call, Then, cx)` and, for headless tests, `Workspace::calls` + `answer_call(n, answer, window, cx)`. 2b's adds,
  closes, wires and `delegate`, 2d's card and column tools, 2e's item tools, 2h's automation tools and
  `cancel_schedule` all go through `remote_call`. `Call { slow: true }` is 2g's lane for `git`'s `fetch`, `pull`,
  `push`, `sync`, `publish` and `message`; `git` and `read_terminal` answers already get 8 MiB. `Refusal` gains
  `details` in 2f/2g when home's error body has it.
- **The rule** (Task 4): a later part makes a deed work by giving `Deed::call` its tool and example arguments (2b:
  `AddTerminal`, `AddKanban`, `AddChecklist` → `add_block`; `OpenAgent` → `open_agent`; `Close` → `close_block`;
  `Wire` → `connect`; 2g: `Git` → `git`), adds the deeds it needs (2b `Delegate`; 2c `Mouse`, `ScrollBack`; 2d the
  card deeds), and asks `why_not` from every control, menu item and shortcut. Rung 2's arguments are
  `RemoteSpace::tools` + `Tools::takes(tool, argument)` (2b's `x`, `y`). 2b replaces `open_menu`'s blanket refusal
  with menus whose items ask the rule, and tells a refused shortcut once.
- **Holds** (Task 5): `Holds<K, V>` for 2b's closed blocks and wires, 2d's moved cards, 2e's checked items; a refusal
  is `end`, the frame's expiry `expire`.
- **The shared state** (Task 1): `RemoteSpace::state` is home's state as last taken; 2d/2e/2h keep their copies of
  cards, notes, checklists, drawings and schedules by comparing each item with it (`Arc::ptr_eq` by id) and
  rebuilding only what changed; `model::same` skips a whole collection. New collections (2e's `notes`, `checklists`,
  `drawings`, 2h's `schedules`) are `Vec<Arc<T>>` like the rest, added to `State::keeps_items`, `from_snapshot`'s
  crowding check and `parse`.
- **Notices** (Tasks 3, 6): `Update::Notice`; `remote_notice` shows 2b's task ends and 2h's automations' notices
  (`Notice` gains `automation` there). `Budget` is the rate of any remote's notifications.
- **The undo mark** is 2b's (see Open for the orchestrator, 2): `history::Change` gains it with the first act that
  leaves it.

## Open for the orchestrator

Each decided with the recommended option; the plan is written that way.

1. **The rule asks "not yet" first,** before "not connected" (the spec lists it fourth). A control that can't work on a
   mirror at all says so even offline, instead of "Not connected." that would never come true. *Recommended:* keep.
2. **The undo mark moves to 2b.** No act of 2a's but a move happens on a remote (typing, answering and Start aren't
   canvas steps), so `Change`'s mark would be built and never made: dead code that `clippy -D warnings` refuses. 2b
   adds it with its first act (an add, a close, a wire), and Ctrl+Z stopping at it with the sentence. *Recommended.*
3. **Home is not "nothing" in 2a:** a watch set through the API sent its notice with no block and no workspace, which
   a share never hears, so the end-to-end test the spec asks for couldn't pass. One change at home (Task 3), with its
   docs line. *Recommended.*
4. **`State`'s items are `Arc` at home too** (serde's `rc`, already on in the tree): one type both ends, home's
   per-token views get cheaper, nothing on the wire changes. The other way, a mirror-only state type, would duplicate
   every collection's parsing. *Recommended.*
5. **"Only what changed" is per collection for batch 1's three** (blocks ≤ 500, wires ≤ 200, requests ≤ 50, bounded
   and dependent on each other) and per item from 2d on for what those parts keep. *Recommended.*
6. **A slow call never cuts the remote, and 524 is "down":** through Cloudflare a call past 100 s is a 524 with no
   Templum body, which the client read as "didn't answer as a Templum" and would have cut the remote for. *Recommended.*
7. **Notices have their own budget** (the requests' numbers, counted apart), so a burst of requests never folds a
   task's end. *Recommended.*
8. **Answers and Start become answered calls,** `Command::Answer`/`Show` and `About::Answer`/`Show` go (`About::Key`
   names the key's refusal, fixing a minor): one way to call home, and `remote_call` has real users in 2a.
   *Recommended.*
9. **The mirror's Edit… row "The share can"** keeps batch 1's words until 2h, when the mirror does what the new words
   say; the Share… dialog (home's) changes now, since a share token can already do it all through the API.
   *Recommended.*
10. **Home's tools unknown** (not read yet, or the read failed) dim nothing as older; home refuses in its words if it
    lacks the tool. *Recommended.*
