# Templum to Templum, Batch 2b (Blocks, Wires and Agents on the Canvas) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer in a Templum agent block does the whole plan, then
> fresh Opus 5.5 reviewers review it in a loop until one passes it. The implementer **commits nothing**: everything
> stays uncommitted for the orchestrator.

**Goal:** Kanban card 171 part 2, batch 2, second of eight parts: on a remote workspace the person **adds** terminals,
kanbans and checklists, **opens home's agents**, **closes** blocks, **wires, unwires and names** wires, **hands an agent
a task**, and uses the **right-click menus and shortcuts**, each done at home through home's API; the new block gets the
keyboard, and Ctrl+Z stops at a **mark** instead of undoing an older move by surprise. Home gains `add_block` and
`open_agent`'s `x`, `y` and `connect`'s `label`.

**Architecture:** Home: `templum-mcp`'s control parser takes the new arguments; the glue places a block at
`Place::At(Point)` and names a wire through `rename_wire`. Mirror: each act asks the one rule (`Deed`, `why_not`), then
calls home with 2a's `Workspace::remote_call`; what the person sees at once (a closed block, a wire made or removed)
stands as a `Holds` over home's state; an ended hold re-mirrors home's last state (`Space::mirror_state`). Menus read
the rule's words per item (`Gates`) when they are built.

**Tech Stack:** Rust 2024; GPUI through gpui-kit `=0.7.3` (its menus are gpui-component's `PopupMenu`); 2a's client
(`Call`, `Answered`, `Tools`) and glue (`remote_call`, `Deed`, `Nay`, `Holds`).

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), "Batch 2: the whole workspace, live":
"Parts ▸ 2b", "What works on a remote workspace", "The canvas", "One rule for what the mirror can do", "What home's API
gains ▸ Grown arguments" (`add_block`, `open_agent`, `connect`), "The mirror's side", "Constraints (batch 2)", "Left to
the builder (batch 2)", the anti-goals and threat model rows marked *(batch 2)*, and "As built ▸ Batch 1c, 1d, 2a". The
2a plan's [Interfaces for later parts](2026-10-25-t2t-batch-2a-footing.md#interfaces-for-later-parts).

**Code it builds on:** `crates/templum-mcp/src/control.rs`; `crates/templum-api/src/registry.rs`;
`crates/templum/src/workspace/{remote,holds,history,blocks,checklist_blocks,note_tabs,wires,control,ops,spawn,input,
spaces,drawing}.rs`; `crates/templum/src/{menu/mod.rs,menu/build.rs,toolbar.rs,block.rs,launcher.rs,prompt.rs}`;
`crates/templum/src/workspace/tests/{remote,mirror_end_to_end,api,menus,wires,delegate,templum_agent}.rs`.

**Consumes (2a, as built):** `Workspace::remote_call(space, Call, Then, cx)` (`Then` =
`Box<dyn FnOnce(&mut Workspace, Answered, &mut Window, &mut Context<Workspace>)>`), the test-only `Workspace::calls`
(`Vec<(String, Call, Then)>`) and `answer_call(n, answered, window, cx)` (used when no client runs: the `offline`
harness); `Deed`, `Nay`, `Nay::words`, `RemoteSpace::why_not`, `Workspace::why_not`; `RemoteSpace::{tools, state, home,
homes, names, rects, held, me, status}`; `Holds<K, V>` and `HOLD`; `Workspace::warn` (pushes to `said` under test);
`crate::mirror::computer_of`; `heard`, `apply_remote`, `mirror_blocks`, `mirror_wires`, `expire_holds`; the tests'
`offline`, `live`, `apply`, `show_remote`, `terminals_of_home`, `terminal_of_home`, `pump_home`, `mirrored`.

## Global Constraints

- **MEMORY LIMITS on this machine:** **every** cargo command takes `-j 4`; **never two cargo commands at once**; never
  `cargo clean`; while working run only the targeted tests each step names; the full suite **once**, in Task 9.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-mcp` and `templum-api` stay UI-free. No
  `unsafe`. **No new dependency.**
- **The UI thread never waits on the network:** every act is `remote_call` with a `Then`; nothing awaits on the UI
  thread.
- **Home is the truth:** a block closed or a wire made or removed here stands over home's state until home's state shows
  it or `HOLD` (2 s) passes, then home's state wins (re-mirrored at once, even if no new state came); a refusal ends the
  hold at once and shows its sentence. Nothing is drawn for a block home hasn't added yet (no placeholder).
- **Never a relay:** no remote block, wire or label enters the mirror's stores (`boards.json`, `checklists.json`,
  `notes.json`, a canvas file), `Space::uids`, tools or API. The invariant guards stay `on_remote()` and keep refusing:
  `spawn_block_with`, `set_wire`, `set_label`, `begin_ink`, `Space::takes_blocks`, `switch_to`'s drawing-tool reset.
  `open(rect)` (bench and a new workspace's first terminal) refuses on a remote with `on_remote()`.
- **Values, verbatim:** `x`, `y` within ±10⁷ (`PLACE_MOST`, as `move_block`); a wire's label one line, at most 60
  characters (`LABEL_MAX`); `HOLD` 2 s; the new block gets the keyboard only within **5 s** of home's answer
  (`KEYBOARD_WAIT`); home's agents read at each connect and again when a menu opens **60 s** or more after the last read
  (`AGENTS_AGAIN`); at most **100** of home's agents (`MOST_AGENTS`), an agent id at most 64 bytes; a task at most
  256 KiB (`templum_mcp::tools::MAX_TEXT`).
- **Words, verbatim** (winx-desktop is `computer_of`, API home's workspace name):
  - "Notes are read-only on a mirror for now: add and edit notes on winx-desktop." (Add ▸ Note, the Note button and
    shortcut on a remote)
  - "Not on a mirrored workspace yet" (`NOT_YET`: Editor and everything else 2b leaves out)
  - "On a mirror, Ctrl+Z undoes moves only: undo the rest on winx-desktop."
  - The Open dialog: title "Open Claude Code on winx-desktop" (the agent's title); the workspace's folder tagged
    "Workspace folder"; the path field's placeholder "A folder on winx-desktop: inside the workspace's folder, or
    whole"; button **Open**. AI agent ▸ before home answered: "Asking winx-desktop…".
  - Hand a task…: the item **Hand a task…**; the dialog titled "Hand a task to Claude Code" (the agent's name), its
    description naming the block's title and the computer (its words are the builder's); button **Hand over**
    (Ctrl+Enter); a task past 256 KiB: "A task is at most 256 KiB: hand it in parts."
  - Closing a remote terminal whose state shows an agent: the local question, "Close the terminal?" (several: "Close
    N blocks?") with "Claude Code is running in it." (`alert::busy`'s words, home's agent name), button **Close**.
  - Home's parser: "\"x\" and \"y\" go together"; "Give near, or x and y: not both."; "\"x\" is within ±10000000";
    "\"label\" is a string"; "\"label\" is at most 60 characters: a few words."
  - Home's answers: "Connected b7k2m and k9x1d as \"deploys\"." (wired with a label); "Named the wire between b7k2m and
    k9x1d \"deploys\"." (a wired pair renamed); "Took the name off the wire between b7k2m and k9x1d." (blank).
- **Batch 1's and 2a's words stay;** every earlier test keeps passing except those this plan rewrites (named in their
  task).
- **Every logic change has a test that fails without it;** UI-only changes say how they were verified (the dialogs and
  the dimmed menu items' tooltips were not looked at on a display unless the implementer did: then a minors entry).
  Headless GPUI tests; keys in `keyring_core::mock::Store`; home on loopback in the same process; conditions with a
  10 s deadline, never a fixed sleep; a test that needs a shell's job control or an agent is `#[cfg(unix)]`. CI runs
  Linux, macOS and Windows with no agent CLI.
- **Every feature in the API:** the three new arguments and `open_agent`'s answer naming its block ship their lines in
  `docs/api.md` in their task (1, 2). The mirror's acts add nothing to the mirror's own API (never a relay).
- **Minors** go to `docs/minors.md` under `## Templum to Templum`; 2b's two fixes leave it (Task 8). Docs lines ≤ 120
  columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **The answer and the state race:** home's state may bring the new block before `add_block`/`open_agent` answers,
   or after. Either way the block gets the keyboard, unless the person focused something else since the act or 5 s
   passed since the answer. → Task 3 `the_new_block_gets_the_keyboard_whichever_comes_first`.
2. **Closing a big selection past the 64-call queue** (or the connection dropping mid-close): every refused close
   comes back at once, and the person reads the sentence **once** per close, not once per block. → Task 5
   `closing_many_blocks_that_home_refuses_says_it_once`.
3. **A home whose Templum is older** (batch 1: no `x`, `y`, `label` in `GET /v1/tools`): Add still works, placed by
   home; Rename wire… is dimmed with the "older" words; wiring still works. → Task 3
   `a_home_without_x_places_the_block_itself`, Task 6 `rename_is_dimmed_on_a_home_without_label`.
4. **Ctrl+Z after several acts and moves:** marks one after another are one stop; the Ctrl+Z that reaches a mark says
   so and takes it off, the next one undoes the older move; nothing is redone through a mark. → Task 3
   `ctrl_z_stops_once_at_the_mark_then_undoes_older_moves`.
5. **Hostile `list_agents` text** (thousands of lines, a 1 MB title, control characters, an id with quotes or past 64
   bytes): at most 100 agents, each id plain and short, each title one clean line. → Task 4
   `home_s_agent_list_is_capped_and_cleaned`.

---

### Task 1: Home places a block at `x`, `y`, and `open_agent` names its block

**Files:**
- Modify: `crates/templum-mcp/src/control.rs` (`AddBlock`, `Opening`, `list()`'s `add_block` and `open_agent`
  schemas, `parse`, a module-level `number` and `place`, its `mod tests`)
- Modify: `crates/templum/src/workspace/ops.rs` (`Place::At`, `Space::spot`), `crates/templum/src/workspace/control.rs`
  (`add_block_for_agent`, `open_agent_for_agent`, `control_call`'s `OpenAgent` arm),
  `crates/templum/src/workspace/spawn.rs` (`open_agent_in`'s `near`), every other `match` on `Place` the compiler names
- Modify: `docs/api.md` (Tools and aliases: a **Placing and naming** paragraph), `docs/specs/templum-agent.md` (the
  `add_block` and `open_agent` rows: `x`, `y`)
- Test: `crates/templum-mcp/src/control.rs` (`mod tests`), `crates/templum/src/workspace/tests/api.rs`

**Interfaces:**
- Produces, in `templum_mcp::control`:
  - `AddBlock::at: Option<(f32, f32)>` and `Opening::at: Option<(f32, f32)>`: the block's top-left in canvas units.
  - `fn number(args: &Value, key: &str) -> Result<Option<f32>, String>` (move_block's closure, moved out: null is
    None, a finite number is Some, else "\"{key}\" is a number"); `move_block` uses it.
  - `fn place(args: &Value) -> Result<Option<(f32, f32)>, String>`: both `x` and `y`, or neither ("\"x\" and \"y\" go
    together"); each within ±`PLACE_MOST` ("\"x\" is within ±10000000"); with `near` as well: "Give near, or x and y:
    not both.". `add_block` and `open_agent` parse it.
  - Both schemas gain `"x"` and `"y"` (`"type": "number"`, "Its top-left corner on the canvas, with y (x), in canvas
    units: it goes there, over what is there."); `add_block`'s description gains "; at x and y when given".
- Produces, in the glue: `Place::At(Point)`; `Space::spot` answers `Rect::new(p.x, p.y, size.w, size.h)` for it (as a
  local add at a click: it may cover a block); `open_agent_in` treats it as no `near`.
- `add_block_for_agent` and `open_agent_for_agent` take `add.at`/`opening.at` as `Place::At` (it wins: `near` was
  refused by the parser). `open_agent_for_agent` returns `(Outcome, Option<Target>)`: when the agent opened at once
  (`Outcome::Now`, not an error, no worktree) its target is `{ workspace: space, block: uid }`, so the API's answer
  says `"block"`. (The builder's call how the `open` closure hands its uid out: a shared `Rc<Cell<Option<String>>>`
  read after `in_worktree` returns is enough.)

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum-mcp/src/control.rs, mod tests
#[test]
fn add_block_and_open_agent_take_a_place() {
    let err = |name: &str, args: Value| parse(name, &args).unwrap_err();
    let base = json!({"workspace": "w3", "kind": "terminal"});
    let with = |extra: Value| { let mut a = base.clone(); a.as_object_mut().unwrap()
        .extend(extra.as_object().unwrap().clone()); a };
    assert_eq!(err("add_block", with(json!({"x": 10}))), "\"x\" and \"y\" go together");
    assert_eq!(err("add_block", with(json!({"x": 1e8, "y": 0}))), "\"x\" is within ±10000000");
    assert_eq!(err("add_block", with(json!({"x": "left", "y": 0}))), "\"x\" is a number");
    assert_eq!(err("add_block", with(json!({"x": 1, "y": 2, "near": "b7k2m"}))), "Give near, or x and y: not both.");
    let ControlCall::AddBlock(add) = parse("add_block", &with(json!({"x": -12.5, "y": 40}))).unwrap().call
        else { panic!() };
    assert_eq!(add.at, Some((-12.5, 40.0)));
    let opening = json!({"agent": "codex", "workspace": "w3", "x": 1e7, "y": -1e7});
    let ControlCall::OpenAgent(open) = parse("open_agent", &opening).unwrap().call else { panic!() };
    assert_eq!(open.at, Some((1e7, -1e7)));
    assert_eq!(err("open_agent", json!({"agent": "codex", "workspace": "w3", "y": 3})),
        "\"x\" and \"y\" go together");
}
```

`every_tool_parses_its_example_and_names_what_is_missing` and the `move_block` test keep passing (their `AddBlock`
literals gain `at: None`).

```rust
// crates/templum/src/workspace/tests/api.rs
#[gpui_kit::test]
fn add_block_goes_at_x_and_y(cx: &mut TestAppContext) {
    // with_data + turn_on + with_token(Control, None); first = the shown workspace's id;
    // tool("add_block", {"workspace": first, "kind": "kanban", "x": 1234.0, "y": -567.0}) is not refused, and the
    // block its answer's target names (line["result"]["target"]["block"]) has rect.origin() == (1234, -567).
}

#[cfg(unix)]
#[gpui_kit::test]
fn open_agent_at_x_and_y_answers_its_block(cx: &mut TestAppContext) {
    // As templum_agent.rs's open_agent tests: `cat_agents` (make it pub(super)), the workspace's folder a tempdir;
    // tool("open_agent", {"agent": "codex", "workspace": first, "x": 300.0, "y": 200.0}) through a Control token:
    // line["result"]["target"]["block"] is the uid of a new terminal whose rect.origin() == (300, 200).
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-mcp add_block_and_open_agent_take_a_place`
  → fails to compile (`at`).
- [ ] **Step 3: Implement** the interfaces above; write the `docs/api.md` paragraph (under the REST aliases table,
  before **Who acts.**):

  > **Placing and naming.** `add_block` and `open_agent` take `x` and `y`, the block's top-left in canvas units within
  > ±10⁷, both or neither and not with `near`: the block goes there, as one added at a click. `open_agent`'s answer
  > names its block when it opened at once (not in a new worktree).

  and the two `templum-agent.md` rows (`x`, `y`: "its top-left; both or neither, not with `near`").
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-mcp control` then `cargo test -j 4 -p templum add_block_goes_at`
  then `cargo test -j 4 -p templum open_agent_at` then `cargo test -j 4 -p templum workspace::tests::templum_agent` →
  all pass.

---

### Task 2: Home names a wire through `connect`'s `label`

**Files:**
- Modify: `crates/templum-mcp/src/control.rs` (`LABEL_MOST`, `ControlCall::Connect`, `parse`, `list()`'s `connect`
  schema, `rule`'s and `title`'s arms as the compiler asks, `mod tests`)
- Modify: `crates/templum-api/src/registry.rs` (`NOT_TARGETS` gains `"label"`, between `"kind"` and `"last_command"`)
- Modify: `crates/templum/src/workspace/wires.rs` (`LABEL_MAX` is `templum_mcp::control::LABEL_MOST`; `fn
  tidy_label(text: &str) -> Option<String>`, `rename_wire`'s normalizing, moved out), `crates/templum/src/workspace/
  control.rs` (`wire_for_agent` takes the label)
- Modify: `docs/api.md` (the **Placing and naming** paragraph gains `connect`'s `label`), `docs/specs/templum-agent.md`
  (the `connect` row), `docs/specs/connections.md` (a wire named through `connect`)
- Test: `crates/templum-mcp/src/control.rs` (`mod tests`), `crates/templum/src/workspace/tests/wires.rs`

**Interfaces:**
- Produces, in `templum_mcp::control`: `pub const LABEL_MOST: usize = 60;` `ControlCall::Connect { a, b, label:
  Option<String> }`: None when left out; `Some(text)` with its whitespace joined to one line (blank is `Some("")`:
  take the name off); not a string: "\"label\" is a string"; past 60 characters: "\"label\" is at most 60
  characters: a few words.". The schema's `label`: "The wire's name, one line, at most 60 characters; blank takes it
  off. On two blocks already wired it renames their wire." `connect`'s description gains "; with label, names it". Its
  rule is unchanged.
- Produces, in the glue: `pub(super) fn tidy_label(text: &str) -> Option<String>` (`wires.rs`: whitespace joined, at
  most `LABEL_MAX` characters, blank None); `wire_for_agent(a, b, on, label: Option<&str>, cx)`:
  - `connect` with a label on two blocks already wired: `rename_wire` (one Ctrl+Z step, as Rename wire…), answering
    "Named the wire between {a} and {b} \"{label}\"." or, blank, "Took the name off the wire between {a} and {b}."
    (also when the name was already that: the call is idempotent).
  - `connect` with a label on two blocks not wired: `connect` (one step), then the label written straight into
    `space.labels` by its key (no step: Ctrl+Z takes the wire back and the name is kept by its key, as every label is),
    answering "Connected {a} and {b} as \"{label}\"." (blank: today's "Connected {a} and {b}.").
  - Without a label: as today.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum-mcp/src/control.rs, mod tests
#[test]
fn connect_takes_a_label_of_one_line() {
    let label = |l: Value| match parse("connect", &json!({"a": "a1", "b": "b1", "label": l})) {
        Ok(Parsed { call: ControlCall::Connect { label, .. }, .. }) => Ok(label),
        Ok(other) => panic!("{other:?}"),
        Err(e) => Err(e),
    };
    assert_eq!(label(json!("  two \n words ")), Ok(Some("two words".into())));
    assert_eq!(label(json!("  ")), Ok(Some(String::new())));
    assert_eq!(label(json!(5)), Err("\"label\" is a string".into()));
    assert_eq!(label(json!("x".repeat(61))), Err("\"label\" is at most 60 characters: a few words.".into()));
    assert!(matches!(parse("connect", &json!({"a": "a1", "b": "b1"})).unwrap().call,
        ControlCall::Connect { label: None, .. }));
}
```

```rust
// crates/templum/src/workspace/tests/wires.rs
#[gpui_kit::test]
fn connect_names_a_wire_in_one_ctrl_z_step(cx: &mut TestAppContext) {
    // three_blocks (a kanban and a note: wiring them never asks); through the control server
    // (templum_agent::control): connect {a: kanban, b: note} → wired, undo.len() == n.
    // connect {a, b, label: "deploys"} → text "Named the wire between {a} and {b} \"deploys\".",
    //   space.label(wire) == Some("deploys"), undo.len() == n + 1; undo_canvas(false) → label None, still wired.
    // connect {a, b, label: ""} after naming it again → "Took the name off …", label None.
    // disconnect, then connect {a, b, label: "feeds"} → "Connected {a} and {b} as \"feeds\".", wired and labeled,
    //   one step more; undo_canvas(false) → not wired.
}
```

`every_argument_is_a_target_or_not` fails until `"label"` is in `NOT_TARGETS`, then passes.

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-mcp connect_takes_a_label` → fails to compile.
- [ ] **Step 3: Implement** the interfaces above; the docs: `docs/api.md`'s paragraph gains "`connect` takes `label`
  (one line, at most 60 characters; blank takes it off): on two blocks already wired it renames their wire, else it
  wires them with that name; either is one Ctrl+Z step."; the `templum-agent.md` row and a `connections.md` line say
  the same.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-mcp control` then `cargo test -j 4 -p templum-api registry` then
  `cargo test -j 4 -p templum workspace::tests::wires` → all pass.

---

### Task 3: Adding blocks on a remote, the undo mark, and the new block's keyboard

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Deed::call`'s three adds, `Nay::NotesReadOnly`,
  `RemoteSpace::{takes, awaited}`, `Awaited`, `KEYBOARD_WAIT`, `remote_add`, `awaiting`, `focus_awaited`,
  `apply_remote`'s end, `replace_code` resets `awaited`)
- Modify: `crates/templum/src/workspace/history.rs` (`Change::Mark`, `Workspace::mark_remote`, `undo_canvas`)
- Modify: `crates/templum/src/workspace/blocks.rs` (`open_shell`, `add_kanban_at`, `open` refuses `on_remote()`),
  `crates/templum/src/workspace/checklist_blocks.rs` (`new_checklist`, `add_checklist_at`: the latter has no guard today
  and would write a local checklist into the remote canvas once the menu opens there)
- Test: `crates/templum/src/workspace/remote.rs` (`mod tests`), `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `workspace/remote.rs`:
  - `Deed::call`: `AddTerminal` → `("add_block", {"workspace": "w", "kind": "terminal"})`, `AddKanban` → kind
    `kanban`, `AddChecklist` → kind `checklist`.
  - `Nay::NotesReadOnly`, words "Notes are read-only on a mirror for now: add and edit notes on {computer}.";
    `RemoteSpace::why_not(Deed::AddNote)` answers it first (before `NotYet`'s place in the order).
  - `RemoteSpace::takes(&self, tool: &str, argument: &str) -> bool`: home's tools unknown, or they take it (the
    rule's rung 2: an argument home lacks is left out when the act still means the same).
  - `pub(crate) struct Awaited { pub home: String, pub before: Option<BlockId>, pub at: Instant }`;
    `RemoteSpace::awaited: Option<Awaited>`; `const KEYBOARD_WAIT: Duration = Duration::from_secs(5)`.
  - `pub(super) fn remote_add(&mut self, kind: templum_mcp::control::Kind, at: Point, cx: &mut Context<Self>)`: for
    the remote shown, when `why_not` of its deed is None: `remote_call` of `add_block` with `workspace` home's
    workspace id (`remote.home`'s, else `entry.home`), `kind` (`terminal`, `kanban`, `checklist`), `x`/`y` from `at`
    when `takes("add_block", "x")`, and for a checklist `text` = `crate::checklists::NAME` (named as the local Add
    names one); `mark_remote()`; its `Then`: Ok → `awaiting`, Err → `warn` the sentence. Nothing is added here.
  - `fn awaiting(&mut self, space: &str, answered: &Value, before: Option<BlockId>, window, cx)`: home's `block` (a
    string ≤ 64 bytes with no control character, else nothing) becomes `awaited` with `at` now, unless the remote's
    `focused` is no longer `before`; then `focus_awaited`.
  - `pub(super) fn focus_awaited(&mut self, space: &str, now: Instant, window, cx)`: with the remote shown, its
    `focused` still `awaited.before`, `now` within `KEYBOARD_WAIT` of `at`, and `homes` holding the block on the
    canvas: `focus_block` it and clear `awaited`. Focus moved, the time passed, or the remote not shown: clear it.
    Otherwise keep waiting. `apply_remote` calls it at its end.
- Produces, in `history.rs`: `Change::Mark` (an act done at home that this Ctrl+Z can't take back; `ids()` empty);
  `pub(super) fn mark_remote(&mut self)`: keeps `Change::Mark` as `Then::New` (the redo stack is cleared), unless the
  top of the undo stack is already a mark. `undo_canvas` with a `Mark` on top (checked **before** the "gone" skip, which
  an empty `ids()` would take): pops it, `warn`s "On a mirror, Ctrl+Z undoes moves only: undo the rest on {computer}."
  and returns true. A redo never meets one (a mark goes nowhere when taken off).
- The entry points route on a remote (each keeps its local body unchanged): `open_shell(shell, at)` →
  `remote_add(Kind::Terminal, at or new_terminal_rect's origin)` (the shell is home's default: `shell` ignored);
  `add_kanban_at(at)` → `Kind::Kanban`; `new_kanban` reaches it; `add_checklist_at(at)` and `new_checklist(into)` →
  `Kind::Checklist` at `at` / `room_in_view(checklist::SIZE)`'s origin (`into` ignored). On a remote whose
  `why_not` is Some they do nothing (Task 8 tells a shortcut's refusal).
- Rewrites (2a's tests that used `AddTerminal` as the example of "not yet"): `the_one_rule_says_why_in_its_order` and
  `the_toolbar_dims_a_remote_s_controls_with_the_rule_s_words` use `Deed::AddEditor` ("Editor") instead;
  `a_remote_workspace_refuses_what_it_cant_do_yet` drops `new_terminal`, `new_kanban` and `new_checklist` (proved
  below) and keeps the rest.

- [ ] **Step 1: Write the failing tests** (the `offline` harness, `show_remote`, `live(ws, Scope::Control)`,
  `apply(…, terminals_of_home(&["b1"]))`: home's workspace `w1`; `p = Point::new(120.0, -40.0)`):

```rust
// workspace/remote.rs, mod tests
#[test]
fn notes_say_they_are_read_only_and_adds_are_home_s_calls() {
    // remote_at(…, Live) with me Control: why_not(AddNote) == Some(NotesReadOnly), its words for "winx-desktop" are
    // the Global Constraints' sentence; why_not(AddEditor) == Some(NotYet); AddTerminal, AddKanban, AddChecklist None.
    // every_deed_home_runs_has_a_scope_in_the_registry gains: the three adds are Scope::Control.
}
```

```rust
// tests/remote.rs
#[gpui_kit::test]
fn adding_on_a_remote_asks_home_at_the_point(cx: &mut TestAppContext) {
    // open_shell(None, Some(p)); add_kanban_at(p); add_checklist_at(p); new_terminal →
    // calls[0] == ("r1", Call { tool: "add_block", args: {"workspace":"w1","kind":"terminal","x":120.0,"y":-40.0},
    //   slow: false }); calls[1] kind "kanban"; calls[2] kind "checklist" with "text": checklists::NAME;
    // calls[3]'s x, y == new_terminal_rect's origin as it was before the call;
    // the canvas still has its one block; ws.checklists and ws.boards are as before (nothing local);
    // the undo stack is one step, matches!(…, [(_, Change::Mark)]) (four acts, one stop; `Change` has no PartialEq).
}

#[gpui_kit::test]
fn a_home_without_x_places_the_block_itself(cx: &mut TestAppContext) {
    // remote.tools = Tools::read([{"name":"add_block","inputSchema":{"properties":{"workspace":{},"kind":{}}}}]);
    // add_kanban_at(p) → calls[0].args == {"workspace":"w1","kind":"kanban"}.
}

#[gpui_kit::test]
fn the_new_block_gets_the_keyboard_whichever_comes_first(cx: &mut TestAppContext) {
    // a) open_shell(None, Some(p)); answer_call(0, Ok({"text":"Added …","block":"b9"})); apply(terminals_of_home
    //    (&["b1","b9"])) → ws.space.focused == Some(homes["b9"]).
    // b) again: apply(&["b1","b9","b10"]) first, then answer_call(0, Ok({"block":"b10"})) → focused is b10's block.
    // c) again: send, focus_block(b1's block), answer Ok b11, apply with b11 → focused stays b1's.
    // d) again: send, answer Ok b12, set awaited.at back 6 s, apply with b12 → focused unchanged; awaited None.
    // A refusal (answer_call Err("A token limited to some workspaces …")) → said.last() is it; nothing focused.
}

#[gpui_kit::test]
fn ctrl_z_stops_once_at_the_mark_then_undoes_older_moves(cx: &mut TestAppContext) {
    // b1's block moved by hand (placing + rect.x += 100 + end_placing): the undo stack is [Place];
    // open_shell(None, Some(p)); add_kanban_at(p) → [Place, Mark]; undo_canvas(false) → true, said.last() == "On a
    // mirror, Ctrl+Z undoes moves only: undo the rest on {computer}.", b1's block still moved, the stack [Place];
    // undo_canvas(false) again → b1's block back where it was; the redo stack holds the move only.
}

#[gpui_kit::test]
fn notes_and_editors_stay_off_a_remote(cx: &mut TestAppContext) {
    // live Control: new_note, add_note_at(p), new_editor send nothing (calls empty), add nothing, leave no mark.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum adding_on_a_remote` → fails (nothing sent).
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::remote` then `cargo test -j 4 -p templum
  workspace::tests::remote` then `cargo test -j 4 -p templum workspace::tests::history` → all pass.

---

### Task 4: Home's agents and the Open dialog

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Deed::OpenAgent`'s call, `HomeAgent`, `home_agents`,
  `MOST_AGENTS`, `AGENTS_AGAIN`, `RemoteSpace::{agents, agents_at, agents_asking}`, `read_home_agents`, `heard`'s
  `Update::Tools`, `home_folders`, `remote_open_agent`; `replace_code` resets the three)
- Modify: `crates/templum/src/workspace/control.rs` (`agent_list` becomes `pub(super)`, for the test that binds the
  parser to it), `crates/templum/src/launcher.rs` (`ask_home_folder`: the Open dialog)
- Modify: `crates/templum/src/menu/mod.rs` (`open_menu` lets `Target::Agents` open on a remote, reading home's agents
  again when stale; `items` gives home's agents for it), `crates/templum/src/menu/build.rs` (`home_agent_items`)
- Test: `crates/templum/src/workspace/remote.rs` (`mod tests`), `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `workspace/remote.rs`:
  - `Deed::OpenAgent` → `("open_agent", {"agent": "a", "workspace": "w"})` (Control).
  - `#[derive(Clone, Debug, PartialEq)] pub(crate) struct HomeAgent { pub id: String, pub title: String, pub
    installed: bool }`.
  - `pub(crate) fn home_agents(text: &str) -> Vec<HomeAgent>`: reads home's `list_agents` answer as home writes it
    (`agent_list`: per line `{id}  {title:?}  installed|not installed  …`): the id up to the first two spaces (at most
    64 bytes, no control or quote character, not empty, else the line is skipped); the title, the quoted string after
    it, unescaped (`\"` and `\\`), through `clean_title` (empty: the id); installed when what follows the title starts
    with `  installed`. At most `MOST_AGENTS` (100) agents.
  - `RemoteSpace::agents: Option<Vec<HomeAgent>>` (None: not read yet), `agents_at: Option<Instant>`,
    `agents_asking: bool`; `const AGENTS_AGAIN: Duration = Duration::from_secs(60)`.
  - `pub(super) fn read_home_agents(&mut self, space: &str, cx: &mut Context<Self>)`: unless one is asking,
    `remote_call` of `list_agents` `{}`; Ok → `agents` from `answered["text"]`, `agents_at` now; either way
    `agents_asking` false; a refusal says nothing. `heard` calls it after `Update::Tools` (each connect, once
    `GET /v1/me` answered); `open_menu` calls it for `Target::Agents` and `Target::Canvas` on a remote whose
    `agents_at` is None or `AGENTS_AGAIN` old.
  - `pub(crate) fn home_folders(&self) -> Vec<(String, Option<&'static str>)>`: for the remote shown, home's
    workspace's folder first, tagged `Some("Workspace folder")`, then each folder of home's terminals of that
    workspace (`remote.state.blocks`, kind `terminal`, `folder` Some), each once and not the workspace's, in home's
    back-to-front order; every path through `clean_to(_, 300)`.
  - `pub(crate) fn remote_open_agent(&mut self, agent: &str, folder: &str, at: Point, then: impl FnOnce(&mut
    Workspace, Result<(), String>, &mut Window, &mut Context<Workspace>) + 'static, cx: &mut Context<Self>)`:
    `open_agent` with `agent`, home's workspace id, `folder` when not blank, `x`/`y` when `takes("open_agent", "x")`;
    `mark_remote()`; Ok → `awaiting` (Task 3) then `then(Ok)`; Err → `then(Err(sentence))`.
- Produces, in `launcher.rs`: `pub(crate) fn ask_home_folder(ws: &Workspace, agent: HomeAgent, at: Point, window,
  cx)`: the dialog the Global Constraints' words describe, its rows from `home_folders()`, a path field; a row or the
  field's Enter calls `remote_open_agent`; the dialog shows that it is asking until home answers, closes on Ok
  (`window.close_dialog(cx)`) and shows the refusal's sentence in it on Err (it stays open). No Personal space, recent
  folders or picker. Its look is the builder's (theme tokens only).
- Produces, in the menus: AI agent ▸ on a remote lists `RemoteSpace::agents`: installed first, the others disabled
  under "Not installed", as `agent_items` does; None: one disabled "Asking {computer}…"; a choice opens
  `ask_home_folder`. (`fn home_agent_items(ws, menu, agents: Option<Vec<HomeAgent>>, computer: String, at: Point) ->
  PopupMenu` in `build.rs`; Task 8 uses it in the canvas menu.)

- [ ] **Step 1: Write the failing tests**

```rust
// workspace/remote.rs, mod tests
#[test]
fn home_s_agent_list_is_capped_and_cleaned() {
    // 150 lines "a{n}  \"Agent {n}\"  installed  no model  no effort" → 100 agents;
    // "codex  \"Co\\\"dex\"  not installed  no model  no effort" → title "Co\"dex", installed false;
    // an id of 65 bytes, an id with a quote, a line with no title, "" → skipped;
    // a title of 1 MB → one line, as clean_title cuts it; a title with "\u{7}" in it → no control character.
}
```

```rust
// tests/remote.rs
#[gpui_kit::test]
fn home_s_agent_list_reads_as_home_writes_it(cx: &mut TestAppContext) {
    // let text = ws.agent_list(); home_agents(&text) has, in order, every ws.agents entry's id and clean_title(title),
    // installed == templum_terminal::installed(entry.program()).
}

#[gpui_kit::test]
fn home_s_agents_are_read_at_connect_and_again_after_a_minute(cx: &mut TestAppContext) {
    // live Control; heard("r1", Update::Tools(Tools::read(&json!([])))) → calls[0].tool == "list_agents";
    // answer_call(0, Ok({"text": "codex  \"Codex\"  installed  takes a model  efforts high"})) → remote.agents ==
    // Some([HomeAgent{codex, Codex, true}]); open_menu(Target::Agents(p)) → the menu opened (ws.menu is Some), no new
    // call; agents_at set 61 s back, open_menu again → calls has one list_agents.
}

#[gpui_kit::test]
fn the_open_dialog_offers_the_workspace_s_folder_then_its_terminals_once(cx: &mut TestAppContext) {
    // apply a state whose w1 has folder "/srv/api" and terminals at "/srv/api", "/srv/api/web", "/srv/api/web",
    // "/srv/api/db" → home_folders() == [("/srv/api", Some("Workspace folder")), ("/srv/api/web", None),
    // ("/srv/api/db", None)] (in home's z order).
}

#[gpui_kit::test]
fn opening_an_agent_on_a_remote_sends_open_agent_and_hands_back_a_refusal(cx: &mut TestAppContext) {
    // remote_open_agent("codex", "web", p, then) → calls[0].args == {"agent":"codex","workspace":"w1",
    // "folder":"web","x":120.0,"y":-40.0}; the undo stack is one Mark;
    // answer_call(0, Err("That folder lies outside API's folder.")) → then got Err with that sentence;
    // again with folder "" → no "folder" key; answer Ok({"block":"b9"}) → then got Ok and awaited.home == "b9".
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum home_s_agent_list` → fails to compile.
- [ ] **Step 3: Implement** the interfaces above. The dialog is UI: say how it was checked (headlessly through
  `remote_open_agent`; on a display or not).
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::remote` then `cargo test -j 4 -p templum
  workspace::tests::remote` then `cargo test -j 4 -p templum workspace::tests::menus` → all pass.

---

### Task 5: Closing on a remote, held until home shows it

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Deed::Close`'s call, `RemoteSpace::closed`, `remote_close`,
  `close_remote_now`, `Viewers` built by `Workspace::viewers`, `Space::mirror_state`, `Workspace::remirror`,
  `apply_remote` through `mirror_state`, `mirror_blocks`' closed blocks, `expire_holds`)
- Modify: `crates/templum/src/workspace/blocks.rs` (`close_blocks` routes to `remote_close` on a remote),
  `crates/templum/src/block.rs` (the header's ✕ dimmed with `why_not(Deed::Close)` as its tooltip; the fullscreen
  button with `why_not(Deed::Grid)`)
- Test: `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `workspace/remote.rs`:
  - `Deed::Close` → `("close_block", {"block": "b"})` (Control).
  - `RemoteSpace::closed: Holds<BlockId, bool>` (true: closed here, home not caught up). A closed block keeps its
    `homes`/`names` entry while held; its `rects` entry and its move hold go.
  - `pub(super) fn remote_close(&mut self, ids: Vec<BlockId>, window, cx)`: with `why_not(Deed::Close)` None: the
    blocks among `ids` that are terminals whose `RemotePane::agent` is Some ask first, `prompt::confirm` with the local
    title ("Close the terminal?" / "Close N blocks?") and `alert::busy`'s "{agent} is running in it." (home's agent
    name; several: "{title}: {why}" each, joined), button **Close**; else `close_remote_now` at once.
  - `pub(super) fn close_remote_now(&mut self, ids: &[BlockId], window, cx)`: per block with a home id: `remote_call`
    `close_block {block}`, `remove_block` (its fade, as locally), `closed.hold(id, true, now)`; one `mark_remote()`.
    Each `Then` on Err: `closed.end(&id)`, `remirror`, and the first refusal of this close is `warn`ed, the others not
    (one `Rc<Cell<bool>>` per close).
  - `fn viewers(&self) -> Viewers` (what `apply_remote` builds today).
  - `fn mirror_state(&mut self /* Space */, remote: &mut RemoteSpace, state: &State, home: Option<&str>, make:
    &Viewers, (now, dragged): (Instant, &HashSet<BlockId>))`: `apply_remote`'s blocks filter (home's workspace) and
    `z` sort, then `mirror_blocks`, then `mirror_wires`; `apply_remote` calls it when its collections changed.
  - `pub(super) fn remirror(&mut self, now: Instant)`: the remote shown, `mirror_state` of its `remote.state` (a
    pointer copy) with `remote.home`'s id.
  - `expire_holds(now)` also expires `closed` (nothing kept); when any ended, `remirror(now)`; the due it returns is
    the soonest of every hold's.
- `mirror_blocks`, for each block home lists, picks its local id so (the algorithm the signatures don't fix):

```rust
let id = match remote.homes.get(&b.id).copied() {
    // Closed here, home not caught up: not drawn.
    Some(id) if remote.closed.shown(id, false, now, |a, b| a == b) => continue,
    Some(id) if self.canvas.get(id).is_some() => id,
    // Closed here and still home's past HOLD (or refused): it comes back as a new block.
    Some(stale) => { remote.names.remove(&stale); /* then added as a new one, below */ }
    None => /* added as a new one */,
};
```

  and a block home no longer lists ends its `closed` hold with the rest of its entries. The order loop skips held ones.

- [ ] **Step 1: Write the failing tests** (the `offline` harness, live Control, `apply(terminals_of_home(&["b1",
  "b2"]))`; `v1`, `v2` the viewers):

```rust
#[gpui_kit::test]
fn closing_on_a_remote_asks_home_and_the_block_leaves_at_once(cx: &mut TestAppContext) {
    // close(v1) → calls[0] == close_block {"block":"b1"}; the canvas has no v1; the undo stack is one Mark;
    // apply(state still with b1, a new Arc so it "changed") within HOLD → still no b1 block;
    // apply(state with b2 only) → homes has no "b1", closed.get(&v1) is None.
}

#[gpui_kit::test]
fn a_closed_block_home_still_lists_past_hold_comes_back(cx: &mut TestAppContext) {
    // close(v1); no new state; expire_holds(now + HOLD) → homes["b1"] is a block on the canvas again (a new id),
    // drawn at home's rectangle.
}

#[gpui_kit::test]
fn a_refused_close_brings_the_block_back_at_once(cx: &mut TestAppContext) {
    // close(v1); answer_call(0, Err("There's no block b1: overview lists them.")) → b1 is on the canvas again,
    // said.last() is the sentence.
}

#[gpui_kit::test]
fn closing_many_blocks_that_home_refuses_says_it_once(cx: &mut TestAppContext) {
    // terminals_of_home(&["b1","b2","b3"]); close_blocks(all three, Then::New) → three calls; answer each with
    // Err("winx-desktop is slow to answer: try again.") → all three back, said grew by one.
}

#[gpui_kit::test]
fn closing_a_remote_agent_asks_first(cx: &mut TestAppContext) {
    // terminal_of_home(Some(AgentView{agent:"Claude Code", state:"working", ..})); close(v) (not a key press: a
    // focused remote terminal sends its keys home): window.has_active_dialog; calls empty;
    // within("dialog").click("ok") → calls[0] is close_block. An idle remote terminal (no agent) closes with no
    // question.
}
```

`a_remote_workspace_refuses_what_it_cant_do_yet` drops `ws.close(viewer, …)` and `ws.close_terminal(…)`.

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum closing_on_a_remote` → fails.
- [ ] **Step 3: Implement** the interfaces above. The dimmed ✕ is UI: say how it was checked.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::tests::remote` then `cargo test -j 4 -p templum
  workspace::tests::blocks` then `cargo test -j 4 -p templum workspace::tests::mirror_end_to_end` → all pass.

---

### Task 6: Wires on a remote: the tool, the menus, names and Delete

**Files:**
- Modify: `crates/templum/src/workspace/holds.rs` (`keys`), `crates/templum/src/workspace/remote.rs` (`Deed::{Wire,
  Unwire, Label}`, `Deed::needs`, `why_not`'s rung 2 for arguments, `RemoteSpace::wires_held`, `remote_wire`,
  `remote_label`, `mirror_wires` with holds, `expire_holds`, `close_remote_now` ends holds of its wires)
- Modify: `crates/templum/src/workspace/wires.rs` (`connect`, `disconnect`, `rename_wire` route on a remote; `set_wire`
  and `set_label` keep their `on_remote()` refusal), `crates/templum/src/workspace/spaces.rs` (`switch_to` keeps the
  Wire tool on a remote whose `why_not(Deed::Wire)` is None)
- Test: `crates/templum/src/workspace/holds.rs` (`mod tests`), `crates/templum/src/workspace/remote.rs`
  (`mod tests`), `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `holds.rs`: `pub fn keys(&self) -> Vec<K>` (every key held, any order).
- Produces, in `workspace/remote.rs`:
  - `Deed::Wire` → `("connect", {"a": "a", "b": "b"})`; new `Deed::Unwire` → `("disconnect", {"a": "a", "b": "b"})`;
    new `Deed::Label` → `("connect", {"a": "a", "b": "b", "label": "l"})`; all Control.
  - `Deed::needs(self) -> &'static [&'static str]`: `Label` → `&["label"]`, the others `&[]`. `RemoteSpace::why_not`
    answers `Older` also when home's tools are known and its tool lacks one of `needs()`.
  - `RemoteSpace::wires_held: Holds<(BlockId, BlockId), bool>` (key: the two ends, the smaller first; true: wired
    here, false: unwired here).
  - `pub(super) fn remote_wire(&mut self, wire: Wire, on: bool, cx) -> bool`: with `why_not(Wire or Unwire)` None and
    both ends home's (`home_of`), not already so here: the wire is added to (or removed from) `space.wires` at once and
    held; `remote_call` `connect`/`disconnect {a, b}` (home's ids); `mark_remote()`; Err → hold ended, `remirror`, the
    sentence `warn`ed. True when sent.
  - `pub(super) fn remote_label(&mut self, wire: Wire, label: Option<String>, cx) -> bool`: with `why_not(Deed::Label)`
    None and the wire shown: `connect {a, b, label}` (`""` for None); `mark_remote()`; Err `warn`ed. Not held: the name
    shows when home's state brings it.
  - `mirror_wires(&mut self, remote: &mut RemoteSpace, state: &State, now: Instant)`: home's wires as today, then for
    each key of `wires_held` whose ends are both on the canvas: `shown(key, home_has, now, ==)` adds or removes it;
    a key with an end not on the canvas (closed here, gone) ends.
  - `expire_holds` expires `wires_held` too (then `remirror`).
- `connect(a, b, cx)` → `remote_wire(Wire{a,b}, true)`, `disconnect(wire, cx)` → `remote_wire(wire, false)`,
  `rename_wire(wire, text, cx)` → `remote_label(wire, tidy_label(text))` on a remote. The Wire tool (`end_wire`), the
  menus' Connect to ▸ / Disconnect from ▸ / Rename wire… / Disconnect, and a selected wire's Delete all reach them.

- [ ] **Step 1: Write the failing tests**

```rust
// holds.rs, mod tests
#[test]
fn keys_lists_what_is_held() { /* hold(1,…), hold(2,…), end(&1) → keys() == [2] */ }

// workspace/remote.rs, mod tests
#[test]
fn rename_is_dimmed_on_a_home_without_label() {
    // Live, me Control, tools = Tools::read([{"name":"connect","inputSchema":{"properties":{"a":{},"b":{}}}},
    // {"name":"disconnect"}]) → why_not(Label) == Some(Older), why_not(Wire) and why_not(Unwire) None.
    // every_deed_home_runs_has_a_scope_in_the_registry gains Wire, Unwire, Label: Control.
}
```

```rust
// tests/remote.rs (offline harness, live Control, apply(terminals_of_home(&["b1","b2"])), v1/v2 the viewers)
#[gpui_kit::test]
fn wiring_on_a_remote_asks_home_and_shows_at_once(cx: &mut TestAppContext) {
    // connect(v1, v2) → calls[0] == connect {"a":"b1","b":"b2"}; space.wires == [Wire{v1,v2}]; one Mark;
    // apply(state without the wire, changed) within HOLD → still wired; expire_holds(now + HOLD) → not wired.
    // Again connect; apply(state with home's wire b1–b2) → wired, wires_held empty.
}

#[gpui_kit::test]
fn unwiring_on_a_remote_and_its_refusal(cx: &mut TestAppContext) {
    // a state with the wire b1–b2; disconnect(wire) → calls[0] disconnect {a,b}; not wired at once;
    // answer_call(0, Err("b1 and b2 aren't wired.")) → wired again (home's), said.last() is the sentence.
}

#[gpui_kit::test]
fn renaming_a_remote_wire_asks_home_with_its_label(cx: &mut TestAppContext) {
    // the wire shown; rename_wire(wire, "  deploys  ") → calls[0] == connect {"a":"b1","b":"b2","label":"deploys"};
    // labels unchanged until apply(state whose wire has label "deploys") → space.label(wire) == Some("deploys").
    // rename_wire(wire, "") → label "".
}

#[gpui_kit::test]
fn a_held_wire_to_a_block_closed_here_goes_with_it(cx: &mut TestAppContext) {
    // connect(v1, v2); close(v2) → no wire drawn; expire_holds(now + HOLD) → no panic, wires_held empty.
}

#[gpui_kit::test]
fn the_wire_tool_works_on_a_remote_that_can_wire(cx: &mut TestAppContext) {
    // live Control: set_tool(Tool::Wire) → ws.tool == Tool::Wire; offline (status Disconnected) → set_tool refuses.
    // Wire picked on the local workspace, then switch_to the live remote → still Tool::Wire; to the remote with its
    // status Disconnected → Tool::Select.
}
```

`a_remote_workspace_refuses_what_it_cant_do_yet` drops `ws.set_tool(Tool::Wire, …)` and its `ws.tool != Tool::Wire`
assertion; its `set_wire`/`set_label` lines stay (the invariant guards still refuse).

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum wiring_on_a_remote` → fails.
- [ ] **Step 3: Implement** the interfaces above.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::holds` then `cargo test -j 4 -p templum
  workspace::remote` then `cargo test -j 4 -p templum workspace::tests::remote` then `cargo test -j 4 -p templum
  workspace::tests::wires` → all pass.

---

### Task 7: Hand a task…

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Deed::Delegate`, `hand_task`, `ask_task`),
  `crates/templum/src/prompt.rs` (`ask_lines`: a multi-line field, Ctrl+Enter confirms)
- Test: `crates/templum/src/workspace/remote.rs` (`mod tests`), `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Produces, in `workspace/remote.rs`:
  - new `Deed::Delegate` → `("delegate", {"block": "b", "task": "t"})` (Control).
  - `pub(crate) fn hand_task(&mut self, block: BlockId, task: String, window, cx)`: with `why_not(Deed::Delegate)` None
    and `block` home's: a task over `templum_mcp::tools::MAX_TEXT` bytes is `warn`ed "A task is at most 256 KiB: hand
    it in parts." and not sent; else `remote_call` `delegate {block: home id, task}`; no mark (not a canvas step);
    success says nothing (the task shows typed in the agent; its end is a notice); Err `warn`ed.
  - `pub(crate) fn ask_task(&mut self, block: BlockId, window, cx)`: the dialog (`prompt::ask_lines`) titled "Hand a
    task to {agent}" (`RemotePane::agent`'s name), naming the block's title and the computer, **Hand over**; its apply
    calls `hand_task` and closes. Task 8 puts it in the block menu.
- Produces, in `prompt.rs`: `pub(crate) fn ask_lines<V: 'static>(title, description, ok: &'static str, apply: impl
  Fn(String, &mut Window, &mut App) -> bool + 'static, window, cx: &mut Context<V>)`: `ask_text` with a multi-line
  `InputState` (about 6 rows) and Ctrl+Enter as OK.

- [ ] **Step 1: Write the failing tests**

```rust
// tests/remote.rs (offline harness, live Control, terminal_of_home(Some(agent "Codex")), v its viewer)
#[gpui_kit::test]
fn a_task_handed_on_a_remote_is_home_s_delegate(cx: &mut TestAppContext) {
    // hand_task(v, "Take card 4.\nThen review.") → calls[0] == delegate {"block":"b1","task":"Take card 4.\nThen
    // review."}; undo is empty; answer_call(0, Ok({"text":"Handed to b1 as task t1."})) → said unchanged;
    // hand_task(v, "x".repeat(MAX_TEXT + 1)) → no call, said.last() == "A task is at most 256 KiB: hand it in parts.".
}
```

`every_deed_home_runs_has_a_scope_in_the_registry` gains `Delegate`: Control.

- [ ] **Step 2: Run it to see it fail:** `cargo test -j 4 -p templum a_task_handed_on_a_remote` → fails to compile.
- [ ] **Step 3: Implement** the interfaces above. The dialog is UI: say how it was checked.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::remote` then `cargo test -j 4 -p templum
  workspace::tests::remote` → all pass.

---

### Task 8: The menus and shortcuts on a remote

**Files:**
- Modify: `crates/templum/src/menu/mod.rs` (`open_menu` refuses only `Target::Shells` and `Target::Ink` on a remote;
  `Gates`; `items` reads the gates and home's agents; `Items::Mirrored` for a card of home's)
- Modify: `crates/templum/src/menu/build.rs` (`gated_item`; `adding` on a remote; the terminal's items with Paste gated
  and **Hand a task…**; the block menu's tail gated; the wire's items gated)
- Modify: `crates/templum/src/toolbar.rs` (Terminal on a remote adds at once: `new_terminal`, no shells menu)
- Modify: `crates/templum/src/workspace/input.rs` (`run`), `crates/templum/src/workspace/remote.rs`
  (`RemoteSpace::shortcuts_told`, `command_deed`, `tell_shortcut`)
- Modify: `docs/minors.md` (delete the two entries below)
- Test: `crates/templum/src/workspace/tests/menus.rs`

**Interfaces:**
- Produces, in `menu/mod.rs`: `#[derive(Clone, Default)] pub(super) struct Gates(Vec<(Deed, SharedString)>)` with
  `fn of(ws: &Workspace, deeds: &[Deed]) -> Gates` (each deed whose `ws.why_not` is Some, with its words; empty on a
  local workspace) and `fn why(&self, deed: Deed) -> Option<SharedString>`. The variants that hold items a remote may
  refuse carry one, read in `items`.
- Produces, in `build.rs`: `fn gated_item(ws, label, icon, why: Option<SharedString>, act) -> PopupMenuItem`: `item`
  when `why` is None; else the label disabled, the words its tooltip (gpui-component's `PopupMenuItem` has no tooltip:
  a disabled `PopupMenuItem::element` whose element is the label with `.tooltip(Tooltip::new(why))`).
- What a remote's menus offer (their order as locally):
  - **The canvas:** AI agent ▸ (`home_agent_items`, Task 4; the whole submenu dimmed by `OpenAgent`'s words when
    refused); Add ▸ Terminal (an item: home's default shell, `open_shell(None, Some(at))`), Kanban board, Note (gated
    `AddNote`: the notes' words), Checklist, Editor (gated `AddEditor`); Notes… and Checklists… gated `OpenNote` /
    `OpenChecklist` (`NOT_YET`: they list this computer's own).
  - **A block:** a remote terminal's Copy (as locally), Paste gated `Type`, and **Hand a task…** (gated `Delegate`)
    when its `RemotePane::agent` is Some (`ask_task`); a card of home's (`Body::Mirrored`) has no items of its kind
    (`Items::Mirrored`); then Fullscreen gated `Grid`, Connect to ▸ gated `Wire`, Disconnect from ▸ gated `Unwire`,
    Move to workspace gated `MoveToWorkspace`, Close gated `Close`.
  - **A wire:** Rename wire… gated `Label`, Disconnect gated `Unwire`.
- `open_menu` on a remote also calls `read_home_agents` for `Canvas` and `Agents` when stale (Task 4).
- Produces, in `workspace/remote.rs`: `RemoteSpace::shortcuts_told: HashSet<String>`; `pub(super) fn
  command_deed(command: Command) -> Option<Deed>`: `NewTerminal` → `AddTerminal`, `CloseTerminal` → `Close`,
  `NewKanban` → `AddKanban`, `NewNote` → `AddNote`, `NewEditor` and `OpenFile` → `AddEditor`, `NewGit` → `Git`,
  `ToggleGrid` → `Grid`; every other command None. `pub(super) fn tell_shortcut(&mut self, words: String, window, cx)`:
  `warn`s the words the first time this remote is told them this run, else nothing. `run` on a remote: a command whose
  deed `why_not` refuses is told (`tell_shortcut`) and goes no further; one that is allowed runs its handler as now
  (which acts at home).
- `docs/minors.md`, deleted (fixed here): "A remote terminal has no right-click menu." and "A remote workspace's key
  shortcuts for what it can't do answer nothing."

- [ ] **Step 1: Write the failing tests** (`tests/menus.rs`; the `offline` harness of `tests/remote.rs` with its
  helpers made `pub(super)`: `offline`, `show_remote`, `live`, `apply`, `terminals_of_home`)

```rust
#[gpui_kit::test]
fn a_remote_s_menus_open_with_what_works_there(cx: &mut TestAppContext) {
    // live Control, home's terminal b1 shown: open_menu(Target::Canvas(p)) → ws.menu is Some, said empty;
    // open_block_menu(v1) → Some; a card of home's (a kanban in the state) → its block menu opens;
    // open_menu(Target::Ink(0)) → no menu, said.last() == NOT_YET.
}

#[gpui_kit::test]
fn the_canvas_menu_on_a_remote_adds_at_home_where_clicked(cx: &mut TestAppContext) {
    // as the_canvas_menu_adds_blocks_where_it_was_opened: right_click on empty canvas, Add ▸ (index 1) ▸ Kanban board
    // (index 1) → calls[0] == add_block {"workspace":"w1","kind":"kanban","x": world.x, "y": world.y};
    // right_click again, Add ▸ Note (index 2, dimmed) → no new call.
}

#[gpui_kit::test]
fn hand_a_task_is_in_a_remote_agent_s_menu(cx: &mut TestAppContext) {
    // terminal_of_home(Some(agent)): open_block_menu(v1), within("popup-menu").click(2) (Copy, Paste, Hand a task…)
    // → window.has_active_dialog(cx).
}

#[gpui_kit::test]
fn a_shortcut_a_remote_can_t_do_says_why_once(cx: &mut TestAppContext) {
    // live Control: run(Command::NewNote) twice → said gained exactly one "Notes are read-only on a mirror for now:
    // add and edit notes on {computer}."; run(Command::NewEditor) → one NOT_YET; run(Command::NewTerminal) → calls
    // has an add_block of kind terminal.
}
```

`a_remote_workspace_refuses_what_it_cant_do_yet` drops its `open_menu` line and `assert!(ws.menu.is_none(), …)`.

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum a_remote_s_menus_open` → fails (no menu).
- [ ] **Step 3: Implement** the interfaces above. The tooltips of dimmed items are UI: say how they were checked;
  if not on a display, a minors entry ("The dimmed items of a remote's menus were checked headlessly only").
- [ ] **Step 4: Run** `cargo test -j 4 -p templum workspace::tests::menus` then `cargo test -j 4 -p templum
  workspace::tests::remote` → all pass.

---

### Task 9: End to end, the docs, the full check and `--bench-mirror`

**Files:**
- Test: `crates/templum/src/workspace/tests/mirror_end_to_end.rs` (three cases), `crates/templum/src/workspace/tests/
  delegate.rs` (`agents_running_in(cx, command, data: Option<PathBuf>)`, which `agents_running` calls with None)
- Modify: `crates/templum/src/workspace/remote.rs` (`NOT_MIRRORED`: "Not mirrored yet: notes', checklists',
  editors' and boards' content (a card shows each), ink and shapes, scrolling back in a terminal, the mouse in a
  terminal. Not on a mirrored workspace yet: editors, adding notes, drawing, grid mode, Move to workspace, Copy and
  Paste of blocks, Stop, the Git popup.")
- Modify: `docs/api.md` (Templum to Templum ▸ 7. What a mirror can't do yet: the same list, and "Adding, closing and
  wiring blocks, opening home's agents and handing them tasks work from the mirror; Ctrl+Z there undoes moves only."),
  `docs/specs/templum-to-templum.md` (Status: "part 2b built (plan 2026-10-25-t2t-batch-2b-canvas)"; "As built ▸ Batch
  2b": what the builder chose or changed, and this plan's "Open for the orchestrator" as settled),
  `docs/specs/context-menus.md` (a remote's menus: what each offers, dimmed with the rule's words),
  `docs/specs/connections.md` (Hand a task… on a remote agent), `docs/ARCHITECTURE.md` (the mirror's acts:
  `remote_add`, `remote_close`, `remote_wire`; closed blocks and wires held; `Change::Mark`), `docs/minors.md`
- Note: the helpers that read `ws.space` (`me`, `screen_with`, `entered`) read home only while home's workspace is
  shown; once `r1` is shown, read home through `ws.spaces().find(|s| s.id == first)` (as `printed` does), or show the
  first workspace again for them. A share is a limited token: home's workspace needs a folder for a terminal or an
  agent (`confined_default`): give the first workspace `space.folder = Some(dir)` before `mirrored`.

- [ ] **Step 1: Write the end-to-end cases**

```rust
#[gpui_kit::test]
fn a_terminal_added_on_the_mirror_is_there_at_home_has_the_keyboard_and_closes_there(cx: &mut TestAppContext) {
    // with_data; the first workspace's folder a tempdir; mirrored(…). 1. r1's viewer of home's terminal moved by
    // 100 (placing / end_drag, as the first case does) and home's moved. 2. On r1: open_shell(None,
    // Some(Point::new(1234.0, 567.0))) → pump_home until the first workspace has a new terminal at origin
    // (1234, 567); pump_home until r1's focused block is homes[its uid]; r1's uids still empty. 3. undo_canvas(false)
    // → said.last() == "On a mirror, Ctrl+Z undoes moves only: undo the rest on {computer}."; home's terminal still
    // moved and the new one still there. 4. close(the new viewer) → pump_home until the first workspace no longer has
    // that terminal.
}

#[gpui_kit::test]
fn two_blocks_wired_and_named_on_the_mirror_are_wired_and_named_at_home(cx: &mut TestAppContext) {
    // with_data, three_blocks, mirrored; on r1: connect(viewer of the terminal, card of the kanban) → pump_home until
    // the first workspace wired(terminal, kanban); rename_wire(that wire, "deploys") → pump_home until its
    // label(Wire{terminal, kanban}) == Some("deploys"), and r1's label of its wire == Some("deploys").
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_task_handed_on_the_mirror_is_typed_at_home_and_its_end_shows_on_the_mirror(cx: &mut TestAppContext) {
    // agents_running_in(cx, None, Some(dir)) (theirs found as Codex), (worker, them) = me(theirs); the first
    // workspace's folder set; mirrored(…); on r1: hand_task(viewer of theirs, "Green.") → show the first workspace;
    // screen_with(theirs, "[from API · Itself, task t"); entered(theirs); event(worker, {"event":"submitted"});
    // event(worker, {"event":"stopped","message":"Green."}); pump_home (after(cx, 250) each round) until said has an
    // entry that starts with "{r1's name} on " and contains "task t1" and "Green.".
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::mirror_end_to_end` → all pass (fix what
  fails in the task that owns it).
- [ ] **Step 3: Write the docs** above. `docs/minors.md`: add what was deferred while building, at least: the Open
  dialog, the Hand a task… dialog and the dimmed menu items checked headlessly only (unless looked at on a display);
  "A kanban added from a mirror shows a new board" (`add_block` always shows one; locally a new kanban shows none;
  fix: 2d's All boards… on a remote, or `add_block` without a board); "A shared workspace with no folder refuses the
  mirror's terminals and agents" (home's confinement of a limited token says so in its words; fix: the Share… dialog
  says it when the workspace has none).
- [ ] **Step 4: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 5: The bench** (`mirror_blocks` and `mirror_wires` changed): `cargo run -j 4 --release -p templum --
  --bench-mirror` → `p95_ms` ≤ 16.7; report its JSON line.
- [ ] **Step 6: Leave uncommitted;** report the files changed, the checks' and the bench's results, what went to and
  left `docs/minors.md`, and how each UI-only change was verified.

---

## Interfaces for later parts

What 2b leaves for 2c–2h, by name (each defined in the task shown):

- **The rule** (Tasks 3, 6): `Deed::needs` and `RemoteSpace::takes(tool, argument)` are rung 2 for every later
  argument: 2c's `Deed::Mouse` needs `["mouse"]`; 2d's `move_card` `before` is left out by `takes` (a card at its
  column's end). `Nay::NotesReadOnly` is the notes' sentence 2e reuses for a key typed into a remote note. New deeds:
  `Unwire`, `Label`, `Delegate`.
- **Menus** (Task 8): `Gates::of(ws, deeds)` and `gated_item` for 2c (the terminal's mouse items), 2d (a card's and a
  column's menus, All boards…), 2e (a note's and a checklist's), 2g (the Git popup's menus). `Items::Mirrored` goes as
  2d/2e draw kanbans, checklists and notes live (their own items then, gated). `command_deed` gains 2c's and 2g's
  shortcuts (`NewGit` → a `Git` deed with its call in 2g).
- **Holds and re-mirroring** (Tasks 5, 6): `Holds::keys`; `Space::mirror_state` and `Workspace::remirror(now)` re-draw
  home's last state when a hold ends with no new state: 2d's card holds and 2e's item holds end through it (each adds
  its collection's pass to `mirror_state`). `expire_holds` is the one place every hold expires.
- **Acts at home and the keyboard** (Task 3): `remote_add(kind, at)` for 2g's "Open a terminal here" (it gains a
  `folder` argument then); `awaiting`/`focus_awaited` give any block home answers with the keyboard; `mark_remote()`
  for any later act that is a canvas step at home (none planned: cards and items keep their own undo).
- **Home's agents** (Task 4): `RemoteSpace::agents` (`HomeAgent`) for 2h's Agents popup (From: the agent's title) and
  the Automations editor's agent lists; `home_folders()` for 2g's folder menu (the workspace's folder and its
  terminals', each once).
- **Home's arguments** (Tasks 1, 2): `Place::At` for any later tool that places a block; `tidy_label` for any label.

## Open for the orchestrator

Each decided with the recommended option; the plan is written that way.

1. **The Ctrl+Z that reaches a mark says the sentence and takes the mark off;** the next Ctrl+Z undoes the older move,
   and marks one after another are one. A mark that stayed would make every older move unreachable. *Recommended.*
2. **Marks are left only by acts that are a canvas step at home** (adds, opening an agent, closes, wires, names); Hand
   a task… leaves none (nothing anywhere undoes it). *Recommended.*
3. **A mark is left when the act is sent,** not when home answers: it keeps the stack's order with moves made while a
   call is in flight; a refused act leaves a stop that undoes nothing, which the sentence explains. *Recommended.*
4. **`x` and `y` go together and exclude `near`** (refused, in words), so a call means one place. *Recommended.*
5. **`connect` with a label on two unwired blocks is one Ctrl+Z step** (the wire); the name is kept by its key, as
   every label is (Ctrl+Shift+Z brings both back). *Recommended.*
6. **`open_agent`'s answer names its block** (its target, as `add_block`'s does): without it the mirror can't tell
   which new block to give the keyboard. A home change with its `docs/api.md` line. *Recommended.*
7. **Home's agents are read from `list_agents`'s text** (its lines), not a new structured answer; a test binds the
   parser to home's `agent_list`, so the two can't drift. The other way is a JSON field in the answer of one tool.
   *Recommended.*
8. **Home's agents are read at `Update::Tools`** (each connect, once `GET /v1/me` answered), not at `Status::Live`,
   and again when a menu opens 60 s on; 1d's tests that hear `Live` and answer call 0 stay as they are.
   *Recommended.*
9. **A shortcut is told once per sentence per remote per run** (`shortcuts_told`), so a held-down key or a habit never
   stacks notifications. *Recommended.*
10. **The toolbar's Terminal adds at once on a remote** (home's default shell: there is no shells menu to show).
    *Recommended.*
11. **Notes… and Checklists… are dimmed in a remote's canvas menu** ("Not on a mirrored workspace yet"): they list this
    computer's own, which can't go into a remote workspace. *Recommended.*
12. **Labels aren't held:** a name shows when home's state brings it (a round trip); only wires made or removed are
    held, as the spec's Holds list says. *Recommended.*
13. **Hand a task…'s success says nothing:** the task shows typed in the agent's terminal and its end comes as a
    notice; a refusal is said. *Recommended.*
14. **A kanban added from a mirror shows the new board home makes** (To do, Doing, Review, Done), as `add_block`
    always does; locally a new kanban shows none. A minors entry; 2d's All boards… switches it. *Recommended.*

## Found against the code as built

- `open_agent`'s API answer carries no `block` (`control_call` answers its target None), so "the new block gets the
  keyboard" can't work for an opened agent without a home change: Task 1 adds it (Open question 6).
- `list_agents` answers text lines only: the mirror parses them (Open question 7).
- `add_checklist_at` has no rule guard today; reachable only once the canvas menu opens on a remote, it would write a
  local checklist into `checklists.json` and the remote canvas (an anti-goal): Task 3 routes it.
- gpui-component's `PopupMenuItem` has no tooltip: "dimmed with their reason as tooltip" needs an element item
  (`gated_item`), to be looked at on a display.
- A share is a limited token, so home refuses a terminal or an agent in a shared workspace that has no folder ("A
  token limited to some workspaces opens things inside their folders: this workspace has none."): the mirror shows
  the refusal; the spec doesn't say it. A minors entry.
- The spec's "comes back if home … still lists it 2 s later" needs a re-mirror when a hold ends with no new state:
  `drain_mirror` applies a state only when the client has a fresh one (`remirror`, Task 5).
