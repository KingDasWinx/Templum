# Automations, Batch 2 (Events) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** a Sonnet 5.5 (high) implementer in a Templum block, then an Opus 5.5 (xhigh)
> reviewer, in a loop until the reviewer passes it. The implementer **commits nothing**: everything stays
> uncommitted for the orchestrator.

**Goal:** Kanban card 150, batch 2: automations that run **when an agent finishes its turn**, **when an agent needs
you**, or **when a command ends** (failures only, as an option), from one block, several (Any of them / All of them)
or any agent of a kind, with a cooldown per automation and per agent that fired it, a loop warning, and **the agent
that fired it** as a prompt's target, as where new blocks open and as what the notification's click shows.

**Architecture:** Batch 1 (a554ee4) left a pure layer (`crates/templum/src/automations/`), a runtime
(`workspace/automations.rs`: a 1-second look that runs only while a rule is on or a run held) and the popup
(`automations_view/`). Batch 2 adds no module and no loop. The Agents view's watch (`agents_view::Watch::see`, run by
`watch_agents` on every frame, bus message and screen look) reports a turn's end or a new need; `watch_agents` queues
it (only while an event rule is on) and the look acts on the queue, and on each command rule's terminal count, within
a second. A rule's run state splits into **slots**, one per block that fired it, so each agent's run is held or typed
apart. The Do of "the agent that fired it" is the saved block `"fired-it"`, swapped for the firing block's uid before
a run, so every batch-1 check (Target missing, busy, never into a shell) applies unchanged.

**Tech Stack:** Rust, GPUI (gpui-fast 0.1.8) + gpui-kit 0.7.3 (pinned): the controls batch 1 uses (`Input`,
`menu_button`, the `toggle` chip, `Tooltip`); chrono; serde (`untagged` for who).

**Spec:** [docs/specs/automations.md](../specs/automations.md): read Events (batch 2) with its designer's calls, The
agent that fired it, Start an agent, Notify me, Safety, the list's Run now, the editor's batch 2 lines, At the edges,
and **As built (batch 1)**. Code to read first: `crates/templum/src/automations/{mod,clock}.rs`,
`crates/templum/src/workspace/automations.rs`, `crates/templum/src/automations_view/{mod,editor}.rs`,
`crates/templum/src/workspace/agents_view.rs` (`Doing`, `Watch::see`, `watch_agents`),
`crates/templum/src/workspace/content.rs` (`Workspace::runs`), `crates/templum/src/workspace/waits.rs` (`look`: how
`wait_for(command_finished)` reads `term.status()`), and the tests' helpers in `workspace/tests/automations.rs`,
`tests/agents_view.rs` (`agent`, `begins`, `ends`), `tests/subagents.rs` (`hook`), `tests/delegate.rs` (`prints`,
`two_agents`), `tests/spawn.rs` (`spawner`), `tests/bus.rs` (`me`, `after`, `history_with`, `screen_until`).

## Global Constraints

- Only `crates/templum` changes code; no new crate, file or module (tests go in `workspace/tests/automations.rs` and
  `workspace/tests/agents_view.rs`); no `unsafe`; no new loop or timer.
- Keep to the spec's decisions and words. Verbatim: "When Claude Code (abc12) finishes its turn", "When any Codex in
  api needs you", "When a command fails in zsh (q3x9z)", "When an agent finishes its turn", "When an agent needs
  you", "When a command ends", "Who", "Chosen agents", "Any agent of a kind", "Any of them", "All of them",
  "Terminal", "Failures only", "Cooldown", "minutes", "The agent that fired it", "It acts on the agent that fires
  it", "Choose at least one agent.", "Choose a kind of agent.", "Choose a terminal.", "Choose an agent.", "The
  cooldown runs from 0 to 1440 minutes.", "Write the cooldown in minutes.", the four loop warnings, the Target
  missing words and the row's short words of the spec's Events and Safety sections, and the empty list's "Type a
  prompt, run a command, start an agent or notify you at a time you choose, or when an agent finishes."
- **Every logic change has a headless test that fails without it.** UI-only looks: say how they were verified.
- **Tests don't depend on HOME, config or installed agent CLIs** (CI has none, and runs macOS and Windows): agents
  are `agent()` blocks (`seen.agent = "claude"`) driven by hook events (`begins`, `ends`, `hook`); programs are
  `cat`/`true` stand-ins as `spawner` sets them; folders are `tempfile`. Tests that need a program in front of a
  shell, and the helpers only they use, are `#[cfg(unix)]`.
- **Stable under CPU load: wait on conditions, not single looks.** After `ends`, wait (with a deadline, as
  `ended_run` does) until the block's watch shows Idle; after `prints`, until its `term.status().finished` reaches
  the count; for a typed task, `taken`/`history_with`; never assert after one fixed sleep. A `look(cx)` only runs the
  look; what the look started (a task's answer) is waited for.
- **Fixed-width text is clipped:** the row's right-hand words get a max width and truncate; a block's toggle shows
  `block_label` (already cut); the loop warning wraps inside the column, never overflows.
- **Nothing runs per frame while the popup is closed:** `watch_agents` allocates nothing and scans no rule unless an
  agent's state changed in that call; words (`when_said`, the row's) are made only while the popup draws.
- **Any loop runs only while a rule needs it:** events ride batch 1's look, which runs only while a rule is on or a
  run is held; the queue fills only while an event rule is on and is emptied at every look.
- **Async callbacks handle the rule being saved, turned off or deleted meanwhile:** a task's answer for an event's
  slot writes only `last` (never counts) when the rule changed, never leaves its slot typing, and does nothing for a
  deleted rule; the notification's click handles a deleted rule and a closed block.
- Times are `SystemTime` (the cooldown counts a sleep); headless tests make a rule's cooldown pass by setting its
  `cooldown` to 0 or its `cooled` stamp in the past, never by sleeping.
- Theme tokens and the terminal palette only (warning: `palette.ansi[3]`, as batch 1's yellow); nothing animates.
- Minors deferred go to `docs/minors.md` under Automations; docs lines ≤ 120 columns.
- Before handing over: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test
  --workspace`; the reduced-env run
  `env -i PATH=~/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=~/.cargo RUSTUP_HOME=~/.rustup cargo test --workspace --no-fail-fast`;
  `cargo run --release -p templum -- --bench` and `-- --bench-mixed`, both `p95_ms` ≤ 16.7, their JSON lines
  reported.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. Two Claudes finish within the same second under "type into the agent that fired it": each gets its own prompt;
   none is dropped, none typed into the other. → Task 3, `two_agents_that_finish_together_each_get_their_prompt`.
2. The person is writing in one agent that fired it (a draft) while another also fires it: the first waits "you are
   typing in it" and is never pasted onto their text; the second isn't held behind it. → Task 3,
   `one_agents_held_run_holds_no_other_and_stop_after_drops_it`.
3. Templum starts (or a rule is turned on) while an agent already needs the person, or after a terminal's command
   failed: nothing fires for what happened before. → Task 2, `the_watch_says_a_turns_end_and_a_new_need_once`;
   Task 3, `an_event_while_it_is_off_is_not_replayed`; Task 4, `commands_that_ended_before_it_was_on_fire_nothing`.
4. A rule is edited and saved while its prompt to the agent that fired it is being typed: the answer writes only the
   last run, that agent's slot isn't stuck, and its next turn's end fires again. → Task 3,
   `an_answer_after_a_save_frees_the_agents_run`.
5. A chosen block of an Any or All set is closed: the row says Target missing, an event from the others is Skipped
   with those words, and nothing is typed. → Task 3, `a_chosen_agent_that_is_not_open_is_target_missing`.

---

### Task 1: The events in the rules, and their words

**Files:**
- Modify: `crates/templum/src/automations/mod.rs` (types, constants, `event_words`, `event_short`, tests)
- Modify: `crates/templum/src/automations/clock.rs` (`next_after`, `when_problem`, `when_words`, `list`)
- Modify: every `Automation { .. }` literal (`workspace/automations.rs::new_automation`, the tests' `notify()` in
  `automations/mod.rs` and `workspace/tests/automations.rs`): `cooldown: COOLDOWN`

**Interfaces:**
- Produces, in `crate::automations`:
  - `pub(crate) enum Heard { Idle, NeedsYou }`: `Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize`,
    `rename_all = "snake_case"` ("idle", "needs_you"). Also what `Watch::see` returns (Task 2).
  - `pub(crate) enum Who { Blocks { blocks: Vec<String>, all: bool }, Kind { kind: String } }`, `#[serde(untagged)]`,
    `all` with `#[serde(default, skip_serializing_if = "std::ops::Not::not")]` (as `board.rs` does).
  - `When::Agent { event: Heard, who: Who }` and `When::Command { terminal: String, failures: bool }` (`failures`
    defaulted and skipped when false); `impl When { pub(crate) fn is_event(&self) -> bool }`.
  - `pub(crate) const COOLDOWN: u32 = 5; pub(crate) const COOLDOWN_MOST: u32 = 1440;
    pub(crate) const FIRED: &str = "fired-it";`
  - `Automation.cooldown: u32`, `#[serde(default = "cooldown", skip_serializing_if = "is_cooldown")]` (fns returning
    `COOLDOWN` and testing `== COOLDOWN`).
  - `pub(crate) fn event_words(when: &When, block: &dyn Fn(&str) -> String, kind: &dyn Fn(&str) -> String,
    space: Option<&str>) -> Option<String>`: None for a time.
  - `pub(crate) fn event_short(when: &When, kind: &dyn Fn(&str) -> String) -> Option<String>`: None for a time.
  - `clock::list(items: &[String], and: &str) -> String` becomes `pub(super)`, its last joiner an argument ("and" at
    its two callers, "or" for Any).

- [ ] **Step 1: Write the failing tests** in `automations/mod.rs`'s tests:

```rust
const EVENTS: &str = r#"{ "version": 1, "next_id": 3, "automations": [
  { "id": 1, "when": { "agent": { "event": "idle", "who": { "blocks": ["abc12", "k9x1d"], "all": true } } },
    "do": { "prompt": { "block": "fired-it", "text": "Go on." } }, "cooldown": 10 },
  { "id": 2, "when": { "agent": { "event": "needs_you", "who": { "kind": "codex" } } },
    "do": { "notify": { "text": "Look" } } },
  { "id": 3, "when": { "command": { "terminal": "q3x9z", "failures": true } },
    "do": { "notify": { "text": "Broke" } } } ] }"#;

#[test]
fn an_event_rule_round_trips() {
    // load(EVENTS) from a tempdir, then:
    assert_eq!(first.when, When::Agent { event: Heard::Idle,
        who: Who::Blocks { blocks: vec!["abc12".into(), "k9x1d".into()], all: true } });
    assert_eq!((first.cooldown, &first.act), (10, &Do::Prompt { block: FIRED.into(), text: "Go on.".into() }));
    assert_eq!((second.when.clone(), second.cooldown),
        (When::Agent { event: Heard::NeedsYou, who: Who::Kind { kind: "codex".into() } }, COOLDOWN));
    assert_eq!(third.when, When::Command { terminal: "q3x9z".into(), failures: true });
    // save, then reload: equal; the text has `"cooldown": 10` once, no "all": false, no "failures": false.
}

#[test]
fn an_events_words() {
    // block: "abc12" -> "Claude Code (abc12)", "k9x1d" -> "Codex (k9x1d)", "q3x9z" -> "zsh (q3x9z)";
    // kind: "claude" -> "Claude Code", "codex" -> "Codex".
    // Idle [abc12]                 -> "When Claude Code (abc12) finishes its turn"
    // NeedsYou kind codex, Some("api") -> "When any Codex in api needs you"
    // Idle kind claude, None       -> "When any Claude Code finishes its turn"
    // Idle [abc12, k9x1d] any      -> "When Claude Code (abc12) or Codex (k9x1d) finishes its turn"
    // Idle [abc12, k9x1d, q3x9z]   -> "When Claude Code (abc12), Codex (k9x1d) or zsh (q3x9z) finishes its turn"
    // Idle [abc12, k9x1d] all      -> "When Claude Code (abc12) and Codex (k9x1d) have all finished their turns"
    // NeedsYou [abc12, k9x1d]      -> "When Claude Code (abc12) or Codex (k9x1d) needs you"
    // Command q3x9z / failures     -> "When a command ends in zsh (q3x9z)" / "When a command fails in zsh (q3x9z)"
    // When::Every { minutes: 10 }  -> None
}

#[test]
fn an_events_short_words() {
    // Idle [abc12] "on abc12's turn's end"; Idle [3 blocks] any "on a turn's end of 3 agents";
    // Idle [3 blocks] all "once all 3 agents finish"; Idle kind claude "on a Claude Code's turn's end";
    // NeedsYou [abc12] "when abc12 needs you"; NeedsYou [2 blocks] "when one of 2 agents needs you";
    // NeedsYou kind codex "when a Codex needs you"; Command "on a command's end in q3x9z";
    // failures "on a failure in q3x9z"; a time: None.
}
```

and in `clock.rs`'s tests:

```rust
#[test]
fn an_event_has_no_time() {
    let idle = When::Agent { event: Heard::Idle, who: Who::Blocks { blocks: vec!["abc12".into()], all: false } };
    assert_eq!(next_after(&idle, &utc("2027-01-04T08:00"), &utc("2027-01-04T08:00")), None);
    assert_eq!(when_problem(&idle, dt("2027-01-04T08:00")), None);
    assert_eq!(when_words(&idle, dt("2027-01-04T08:00").date()), "When abc12 finishes its turn");
}
```

- [ ] **Step 2: Run them to verify they fail:** `cargo test -p templum automations::` → compile errors (no `Heard`).
- [ ] **Step 3: Implement.** A one-block set, even with `all`, words as one block; All of them is words only for
  `Heard::Idle` (Needs you with `all` words as Any). The kind and workspace phrase: "any <kind>" plus " in <space>"
  when `space` is Some. `next_after`: `When::Agent { .. } | When::Command { .. } => None`. `when_problem`: return None
  first for an event (its checks are the Workspace's, Task 6). `when_words`: an event's arm is
  `super::event_words(when, &|u| u.to_string(), &|k| k.to_string(), None).unwrap_or_default()`.
- [ ] **Step 4: Run** `cargo test -p templum automations::` → PASS;
  `cargo test -p templum workspace::tests::automations` → PASS (batch 1 unchanged).
- [ ] **Step 5: Leave uncommitted.**

### Task 2: The watch hears a turn's end and a new need

**Files:**
- Modify: `crates/templum/src/workspace/agents_view.rs` (`Watch::see`, `watch_agents`)
- Modify: `crates/templum/src/workspace/mod.rs` (field `automation_heard`, its `Default` in the constructor beside
  `automation_runs`)
- Modify: `crates/templum/src/workspace/automations.rs` (`automations_listen`; the look takes the queue)
- Test: `crates/templum/src/workspace/tests/agents_view.rs`, `crates/templum/src/workspace/tests/automations.rs`

**Interfaces:**
- Consumes: `Heard` (Task 1).
- Produces:
  - `pub(super) fn see(&mut self, doing: Doing, now: Instant) -> Option<Heard>` on `Watch`: `Some(Idle)` exactly
    when it sets `done` (it was at work, `works()`, and is `Idle` now); `Some(NeedsYou)` when it was seen before in a
    state that isn't `NeedsYou` and is `NeedsYou` now; None otherwise, and on the first sight (`was` None).
  - `pub(crate) automation_heard: Vec<(String, Heard)>` on `Workspace`: (the block's uid, what it did), oldest first.
  - `fn automations_listen(&self) -> bool`: some rule that is on has `When::Agent`.

- [ ] **Step 1: Write the failing tests.**

```rust
// tests/agents_view.rs
#[test]
fn the_watch_says_a_turns_end_and_a_new_need_once() {
    let now = Instant::now();
    let mut w = Watch::default();
    assert_eq!(w.see(Doing::NeedsYou("Allow?".into()), now), None, "first sight");
    assert_eq!(w.see(Doing::NeedsYou("Other?".into()), now), None, "the same wait");
    assert_eq!(w.see(Doing::Working(0), now), None);
    assert_eq!(w.see(Doing::AwaitsSubagents(1), now), None);
    assert_eq!(w.see(Doing::Idle, now), Some(Heard::Idle));
    assert_eq!(w.see(Doing::Idle, now), None);
    assert_eq!(w.see(Doing::NeedsYou(String::new()), now), Some(Heard::NeedsYou));
    assert_eq!(w.see(Doing::Idle, now), None, "a wait that ends is no turn's end");
}

// tests/automations.rs, #[cfg(unix)]
#[gpui_kit::test]
fn a_turns_end_is_queued_only_while_an_event_rule_is_on(cx: &mut TestAppContext) {
    // agent A; a time rule on (notify): begins + ends -> automation_heard is empty.
    // add an on rule When::Agent { Idle, Blocks [A's uid] } (notify): begins + ends ->
    //   automation_heard == [(uid, Heard::Idle)] (read before any look);
    // look(cx) -> automation_heard is empty.
}
```

- [ ] **Step 2: Run them:** `cargo test -p templum the_watch_says a_turns_end_is_queued` → FAIL (no `Heard` returned,
  no field).
- [ ] **Step 3: Implement.** `watch_agents` keeps its `doings` as they are; in its write loop, when `see` returns
  Some, it pushes `(id, heard)` into a local `Vec` (empty, so free, on frames where nothing changed); after the loop,
  only if that Vec isn't empty and `automations_listen()`, it looks up each one's uid in its space's `uids` and
  appends to `automation_heard`. `look_automations` begins by clearing `automation_heard` (Task 3 takes it and acts
  on it instead), so the queue never outlives a look.
- [ ] **Step 4: Run** `cargo test -p templum workspace::tests::agents_view workspace::tests::automations` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 3: Agent events fire their automations

**Files:**
- Modify: `crates/templum/src/workspace/automations.rs`
- Test: `crates/templum/src/workspace/tests/automations.rs` (new tests; batch 1's uses of `run.held` move to slots)

**Interfaces:**
- Consumes: `Heard`, `Who`, `FIRED`, `COOLDOWN`, `event_words` (Task 1); `automation_heard` (Task 2);
  `Workspace::runs` (`content.rs`), `find_block`, `space_named`, `uid_title`, `agent_title`.
- Produces:

```rust
#[derive(Default)]
pub(crate) struct Run {
    pub next: Option<SystemTime>,
    /// Its runs under way: None its time's or Run now's, Some(uid) an event's, by the block that fired it.
    pub slots: BTreeMap<Option<String>, Slot>,
    /// When an event last started a run, by the block that fired it ("" for All of them): the cooldown.
    pub cooled: HashMap<String, SystemTime>,
    pub unseen: bool,
}

/// One run under way: batch 1's `held`, `typing` and `typing_counts`, moved off `Run`.
#[derive(Default)]
pub(crate) struct Slot { pub held: Option<Held>, pub typing: bool, pub typing_counts: bool }

impl Run {
    pub(crate) fn slot(&mut self, by: Option<&str>) -> &mut Slot;          // made when missing
    pub(crate) fn held(&self) -> Option<(Option<&str>, &Held)>;            // the first held, its own first
}

/// `a` with "the agent that fired it" made the block `by`.
fn for_firer(a: &Automation, by: Option<&str>) -> Automation;
/// The events of `heard` that fire a rule that is on, in the list's order: (rule, block that fired it, cooldown key).
fn events_due(&mut self, heard: &[(String, Heard)]) -> Vec<(u64, String, String)>;
/// The block `uid` is heard by `a`: in its workspace (when it has one), and chosen in `who` or of its kind.
fn hears(&self, a: &Automation, who: &Who, uid: &str) -> bool;
/// The block `uid` doesn't hold an All of them back: no agent there, idle, or not open.
fn rests(&self, uid: &str) -> bool;
/// The When's words, blocks as "Title (uid)", a kind by its title, a one-workspace rule's workspace by its name.
pub(crate) fn when_said(&self, a: &Automation, cx: &App) -> String;
```

  and these batch-1 signatures gain the block that fired it: `attempt(&mut self, id: u64, counts: bool,
  by: Option<&str>, window, cx)`, `run_ended(&mut self, id, went, about, counts, by: Option<&str>, cx)`,
  `try_do(&mut self, a: &Automation, by: Option<&str>, window, cx) -> Tried` (it takes the rule already made
  `for_firer`, no longer its id).

- [ ] **Step 1: Write the failing tests** (`#[cfg(unix)]`), with helpers:

```rust
/// A rule on `event` from `who` that does `act`, on, counted from now.
fn on_event(event: Heard, who: Who, act: Do) -> Automation;
fn chosen(uids: &[&str]) -> Who;                       // Who::Blocks, any
/// The agent in `terminal` (block `id`) works, then ends its turn; returns once its watch shows Idle.
fn turn(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId, terminal: &str);
fn runs_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, rule: u64) -> u32;
```

  - `a_turns_end_fires_its_rule_and_the_cooldown_holds_the_next`: agent A; rule Idle, `chosen([A])`, notify "Done".
    `turn` A, `look` → `last` = (Ran, "Notified."), `runs_of` 1, one notification more. `turn` A, `look` → runs 1,
    `last.at` unchanged, no notification more. Its `cooldown` set to 0, `turn` A, `look` → runs 2.
  - `each_agent_that_fires_it_has_its_own_cooldown`: A, B (rows 0, 1); rule Idle, `Who::Kind { kind: "claude" }`,
    notify. `turn` A, look → 1; `turn` B, look → 2; `turn` A, look → 2.
  - `a_need_fires_once_per_wait`: A; rule NeedsYou, `chosen([A])`, notify, cooldown 0. `hook(PermissionRequest,
    Bash)` as `tests/subagents.rs` does, look → 1; a second `PermissionRequest` (another tool), look → 1; `begins`,
    `PermissionRequest`, look → 2.
  - `an_event_is_heard_only_in_its_rules_workspace`: A in the shown workspace; `other = make_space()`; rule Kind
    claude with `workspace: Some(other)`, notify: `turn` A, look → `last` None. The same with `Some(shown)` → Ran.
  - `a_prompt_goes_to_the_agent_that_fired_it`: A, B; rule Idle Kind claude, `Do::Prompt { block: FIRED, text:
    "Go on." }`, named "Next". `turn` B, look → `history_with(B, "[from automation \"Next\", task t1]")`, `taken`
    → `last` = (Ran, "Typed into <B> as task t1."); A's screen unchanged. Then, unnamed, a new rule's task line is
    `[from automation "When any Claude Code finishes its turn", task t2]` (its words, `when_said`).
  - `two_agents_that_finish_together_each_get_their_prompt`: A, B; that rule; `turn` A and `turn` B, then one
    `look` → both histories show "[from automation \"Next\""; wait until `runs_of` is 2.
  - `all_of_them_fires_when_the_last_one_finishes`: A, B; rule Idle `Who::Blocks { [A, B], all: true }`, prompt
    FIRED. `begins` A and B; `ends` A, look → nothing typed, `last` None; `ends` B (wait for Idle), look → typed
    into B, not A.
  - `a_chosen_agent_that_is_not_open_is_target_missing`: A; rule Idle `chosen([A, "zzzzz"])`, notify:
    `automation_target` = Err("Target missing: zzzzz isn't open"); `turn` A, look → (Skipped, those words). The
    same with all; Kind "nosuch" → "Target missing: nosuch isn't in Settings ▸ Agents"; a rule of workspace
    `other` choosing A → "Target missing: <A> is in <shown's name>"; `rule(every(10), Do::Prompt { block: FIRED,
    .. })` → "Target missing: only an agent event has an agent that fired it".
  - `an_event_while_it_is_off_is_not_replayed`: rule off; `turn` A; `set_automation_on(true)`; look → `last` None.
  - `an_answer_after_a_save_frees_the_agents_run`: A; prompt FIRED rule; `turn` A, look; before `taken`, edit its
    name and `save_automation`; `taken` → `last` Ran, runs 0, `automation_runs[&rule].slots` empty. `turn` A, look,
    `taken` → runs 1 (the save cleared the cooldown). Then the same with `delete_automation` before the answer: wait
    until A's screen shows the task's Enter (`entered`), then a few `after` turns: nothing panics and
    `automation_runs` has no entry for it.
  - `one_agents_held_run_holds_no_other_and_stop_after_drops_it`: A, B; prompt FIRED rule, `max_runs: Some(1)`; A's
    `draft` set (`pane.draft.set(true)`). `turn` A and `turn` B, look → B typed; A held: `held()` names A with
    "Waiting for <A>: you are typing in it", `last_words` = "Waiting for <A>". `taken` B → runs 1, off, `about` ends
    "Ran 1 of 1 times, now off.", `held()` None, nothing typed into A.
  - `an_agents_run_held_for_an_hour_is_skipped`: a `Held` 61 minutes old in `slot(Some(<A>))` of a FIRED rule;
    look → (Skipped, "<A> stayed busy for an hour.").
  - batch 1's tests: `run.held` → `run.held().map(|(_, h)| ..)`, `.entry(id).or_default().held = Some(h)` →
    `.entry(id).or_default().slot(None).held = Some(h)`.

- [ ] **Step 2: Run them:** `cargo test -p templum workspace::tests::automations` → the new ones FAIL.
- [ ] **Step 3: Implement.**
  - `Run`/`Slot` as above; batch 1's sites act on `slot(None)`; `automations_active` and `look`'s "held" read
    `held()`; `run_automation_now`'s "trying" reads `slots.get(&None)`.
  - `run_ended(by)`: the slot `by` loses its held run and is removed unless typing; when `counts` turns the rule
    off (Stop after, a Once), every slot that isn't typing is removed too.
  - `set_automation_on` and `save_automation`: every slot loses its held run and `typing_counts`, slots not typing
    are removed, `cooled` is cleared.
  - `attempt(id, counts, by)`: `let a = for_firer(saved, by)`; Target missing, Hold and Later act on `slot(by)`.
    The Later callback reads and clears `slot(by).typing`; its "fell due meanwhile, runs at the next look" branch
    is for `by == None` only (an event that joined is dropped with the old rule).
  - `try_prompt`'s and `start_agent_for`'s `called` use `when_said` for an empty name.
  - `automation_target`: before the Do's checks, the When's (spec, Who and Where): each chosen block found in the
    rule's workspace (else "<uid> isn't open", or "<uid> is in <name>" when another workspace has it); a kind in
    `self.agents`; then `Do::Prompt { block }` with `block == FIRED` is present for `When::Agent`, else "only an agent
    event has an agent that fired it".
  - `last_words` and the hour's give-up name the target of `for_firer(a, by)` (`typed_into`).
  - `events_due`: for each rule that is on, in order, `When::Agent { event, who }`: the heard of kind `event` whose
    block it `hears`; for `Who::Blocks { all: true, .. }` with `Heard::Idle`: the last of them, once, key "", if every
    chosen block `rests`; otherwise each, key = its uid.
  - The look, after batch 1's own-slot handling:

```rust
// `fires` = self.events_due(&heard), made at the look's start from the queue it took.
// Each held event slot of a looked rule is tried again (or given up after HOLD_MOST), as the own slot is. Then:
for (id, by, key) in fires {
    let Some(minutes) = self.automations.get(id).map(|a| a.cooldown) else { continue };
    let run = self.automation_runs.entry(id).or_default();
    let cooldown = Duration::from_secs(60 * u64::from(minutes));
    if run.cooled.get(&key).is_some_and(|at| now.duration_since(*at).unwrap_or_default() < cooldown) {
        continue; // inside the cooldown: nothing, not written
    }
    let slot = run.slot(Some(&by));
    if slot.typing { slot.typing_counts = true; continue; }              // joins the run being typed
    if let Some(held) = &mut slot.held { held.counts = true; continue; } // joins the held run
    run.cooled.insert(key, now);
    self.attempt(id, true, Some(&by), window, cx);
}
```

- [ ] **Step 4: Run** `cargo test -p templum workspace::tests::automations` → PASS (batch 1's too).
- [ ] **Step 5: Leave uncommitted.**

### Task 4: A command ends

**Files:**
- Modify: `crates/templum/src/workspace/automations.rs` (`Run.finished`, `events_due`, `automation_target`, resets)
- Test: `crates/templum/src/workspace/tests/automations.rs`

**Interfaces:**
- Consumes: `When::Command` (Task 1), `events_due`, the slots and the cooldown (Task 3).
- Produces: `Run.finished: Option<u64>`: its terminal's `term.status().finished` at the last look.

- [ ] **Step 1: Write the failing tests** (`#[cfg(unix)]`; T a terminal that echoes, as `two_agents`' blocks;
  `prints(&ws, cx, T, b"\x1b]133;C\x07\x1b]133;D;2\x07\r")`, then wait until `status().finished` counts it):
  - `a_command_end_fires_once_for_the_last`: rule `When::Command { T, failures: false }`, notify, cooldown 0; a
    first `look` notes the count. Two commands end (D;0, D;0), wait for 2, look → `runs_of` 1. Also
    `automation_target` of `When::Command { "zzzzz" }` = Err("Target missing: zzzzz isn't open").
  - `failures_only_fires_on_a_failing_exit`: `failures: true`, cooldown 0, a first look; D;0, look → 0; D;2, look →
    1; a D with no code (`\x1b]133;D\x07`), look → 1.
  - `commands_that_ended_before_it_was_on_fire_nothing`: the rule off; D;1; `set_automation_on(true)`; look → 0;
    D;1, look → 1.
- [ ] **Step 2: Run them** → FAIL (nothing fires).
- [ ] **Step 3: Implement.** `events_due`, `When::Command { terminal, failures }`: the terminal's pane (none: note
  None, nothing); `let seen = run.finished.replace(count)`; it fires (key: the terminal's uid, by: the same) when
  `seen` is Some and `count > seen` and, for `failures`, `status.last`'s code is `Some(c)` with `c != 0`. A count
  lower than `seen` (a reopened terminal) is only noted. `set_automation_on` and `save_automation` set `finished` to
  None. `automation_target`: the command's terminal must be open.
- [ ] **Step 4: Run** `cargo test -p templum workspace::tests::automations` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 5: The block that fired it: beside it, the notification's click, Run now

**Files:**
- Modify: `crates/templum/src/workspace/automations.rs` (`try_do`, `command_in_new_terminal`, `start_agent_for`,
  `open_fired`, `run_automation_now`)
- Test: `crates/templum/src/workspace/tests/automations.rs`

**Interfaces:**
- Consumes: `try_do(a, by, ..)` (Task 3); `Place::Near`, `reveal`, `show_automation`.
- Produces: `pub(crate) fn open_fired(&mut self, id: u64, by: Option<String>, window: &mut Window,
  cx: &mut Context<Self>)`: the block `by` revealed (`self.reveal(&space, block, ..)`) when it is open; otherwise
  `show_automation(id)`. `command_in_new_terminal` and `start_agent_for` take a `place: Place`.

- [ ] **Step 1: Write the failing tests:**
  - `new_blocks_open_beside_the_agent_that_fired_it` (`#[cfg(unix)]`, `spawner`): A's rect moved far from the view's
    middle (e.g. at x 4000, y 3000). Rule Idle `chosen([A])`, `Do::Command { folder: tempdir, text: "true" }`, its
    workspace the shown one: `turn` A, look → a new terminal whose rect doesn't overlap A's and whose center is
    nearer A's center than the view's middle. Rule with `Do::Agent { agent: "claude", .. }`: the same for the agent
    it opens. (A rule that opens a block has one workspace and hears only it, so the agent that fired it is there;
    Run now, with none, keeps batch 1's `Place::Free`, which batch 1's tests cover.)
  - `a_notifications_click_shows_the_block_that_fired_it`: B a terminal of a second workspace (`make_space`, then
    `add_block_in` there); `open_fired(rule, Some(<B>))` → that workspace is shown and `space.focused == Some(B)`.
    `open_fired(rule, Some("zzzzz"))` and `open_fired(rule, None)` → `automations_open`, `automations_at ==
    Some(rule)`.
  - `run_now_does_nothing_for_the_agent_that_fired_it`: a prompt FIRED rule; `run_automation_now` → `last` None, no
    slot, nothing typed anywhere.
- [ ] **Step 2: Run them** → FAIL.
- [ ] **Step 3: Implement.** In `try_do`: `place` = `Place::Near(id)` when `by` is found in the rule's workspace,
  else `Place::Free`; it goes to `add_block_in` and `open_agent_in` (the kept terminal is reused as before).
  Notify's `on_click` calls `ws.open_fired(a_id, by.clone(), window, cx)`. `run_automation_now` returns at once for
  `Do::Prompt { block }` with `block == FIRED`. The click's wiring itself is one line: say so (no headless click on a
  notification).
- [ ] **Step 4: Run** `cargo test -p templum workspace::tests::automations` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 6: The list and the editor

**Files:**
- Modify: `crates/templum/src/workspace/automations.rs` (`next_words`, `automation_sentence`, `automation_problem`,
  `edits`, `save_automation`, `automation_loop`)
- Modify: `crates/templum/src/automations_view/editor.rs` (When kinds, who, command fields, Do's first choice,
  cooldown box, loop warning, preview)
- Modify: `crates/templum/src/automations_view/mod.rs` (row words, the next column's id and clip, Run now dimmed,
  the empty text)
- Test: `crates/templum/src/workspace/tests/automations.rs`

**Interfaces:**
- Consumes: `when_said` (Task 3), `event_short` (Task 1), `Workspace::runs`.
- Produces:
  - `pub(crate) fn automation_loop(&self, a: &Automation) -> Option<String>`: the spec's Safety words for (Idle,
    a prompt to a chosen block, to FIRED, or to a block whose agent the kind `runs`), (a kind, either event, Start
    an agent whose agent it `runs`), (a command's terminal = its command's terminal); None otherwise.
  - `preview_next(ws, id, a, now, cx: &App) -> String`: for an event "Runs " + `when_said` with its first letter
    lowered (batch 1's test `a_drafts_next_counts_from_now` passes its `cx`).
  - `Field::Cooldown` and `Form.cooldown: Entity<InputState>` (placeholder "5").
  - `when_kind` values `"idle"`, `"needs_you"`, `"command"`, which `set_when_kind` takes.

- [ ] **Step 1: Write the failing tests:**
  - `an_event_rules_words_in_the_list_and_the_editor`: A (title read with `uid_title`); rule Idle `chosen([A])`,
    notify "Hi": `next_words` = "on <A>'s turn's end"; `when_said` = "When <title> (<A>) finishes its turn";
    `automation_sentence` = that + `, notify you: "Hi".`; `preview_next` = "Runs when <title> (<A>) finishes its
    turn". With `Do::Prompt { FIRED, "Go on." }`: the sentence ends `type "Go on." into the agent that fired it.`.
  - `save_checks_an_event_draft`: `automation_problem` for `chosen([])` → "Choose at least one agent."; `Who::Kind {
    "" }` → "Choose a kind of agent."; `When::Command { "" }` → "Choose a terminal."; cooldown 1441 → "The cooldown
    runs from 0 to 1440 minutes."; `rule(every(10), Prompt { FIRED })` → "Choose an agent."; a valid event rule
    saves, its `cooldown` kept and its `cooled` empty.
  - `choosing_an_event_kind_keeps_who_and_drops_the_fired_target`: from Daily, `set_when_kind("idle")` → `Agent {
    Idle, Blocks { [], false } }`; who edited to `[A, B]`, all; `set_when_kind("needs_you")` → `[A, B]`, all false;
    Do set to `Prompt { FIRED }`; `set_when_kind("daily")` → its block ""; `set_when_kind("command")` → `Command {
    "", false }`.
  - `typing_a_cooldown_edits_the_draft`: `type_in(Field::Cooldown, "10")` → 10; "" → 5; "x" → Save says "Write the
    cooldown in minutes." (`automation-problem`).
  - `the_who_toggles_choose_blocks`: an Idle draft shown in the open popup; `click_id("automation-who-<A>")` → its
    who has A; again → none.
  - `the_loop_warning`: `automation_loop` gives "This can loop: its prompt ends a turn of <A>, which fires it again.
    The cooldown spaces the runs out." for Idle `chosen([A])` + prompt A and for Idle Kind claude + prompt A; the
    FIRED words for Idle + prompt FIRED; None for NeedsYou + prompt A; "This can loop: the agent it starts can fire it
    again, and each new agent has its own cooldown. Stop after bounds it." for Kind claude + `Do::Agent { "claude"
    }` on both events; "This can loop: its command ends in <T>, which fires it again." for `Command { T }` +
    `Do::Command { terminal: Some(T) }`; None for another terminal. Drawn: `automation-loop` holds it.
  - Run now dimmed is UI only (Task 5's `run_now_does_nothing_for_the_agent_that_fired_it` is its logic): say how it
    was verified.
  - `a_long_event_word_stays_in_its_row`: an agent entry's title made 120 characters; a Kind rule of it: the row ≤
    `LIST_W` (as `a_row_keeps_to_the_lists_width`) and `automation-next-<id>` ≤ `LAST_W` wide.
  - `the_empty_list_says_events_too`: no automations, popup open: the drawn text has "…, or when an agent finishes."
- [ ] **Step 2: Run them** → FAIL.
- [ ] **Step 3: Implement.**
  - `next_words`: an event rule that is on says `event_short` (kind titles by `agent_title`); off stays "Off".
  - `automation_sentence`, the row's line 2 and the task's name use `when_said`; a FIRED prompt is "type <quote>
    into the agent that fired it".
  - `automation_problem`, after `when_problem`: the who and terminal words above, the cooldown range, FIRED without
    `When::Agent` → "Choose an agent.". `edits` adds `cooldown`; `save_automation` copies it.
  - `set_when_kind`: "idle"/"needs_you" keep an agent event's who (Needs you: `all` false), else `chosen([])`;
    "command" keeps an earlier command's terminal, else `""`, `failures: false`; any kind but "idle"/"needs_you" turns
    a FIRED prompt's block to `""`. The When dropdown adds "When an agent finishes its turn", "When an agent needs
    you", "When a command ends".
  - Who: a `menu_button` "automation-who" (Chosen agents / Any agent of a kind; switching to a kind takes the first
    of `ws.agents`, or ""), then either a wrapping row of `toggle`s `automation-who-<uid>` labelled `block_label`
    (chosen blocks not offered stay, as `kept_label`), plus "automation-who-all" (Any of them / All of them) for Idle
    with two or more chosen, a click that leaves one block also clearing `all`; or `block_menu("automation-kind",
    agents by title, …, "<kind> (removed)")`. A command ends: "Terminal", `block_menu("automation-event-terminal",
    blocks(ws, a, false, cx), …)`, and `toggle("automation-failures", "Failures only")`.
  - The Do's Agent dropdown: `(FIRED, "The agent that fired it")` first for `When::Agent`.
  - Under the Do: `automation_loop`'s words in `div().id("automation-loop")`, yellow, small, `w_full`, wrapping.
  - After Stop after, for an event: "Cooldown", a 90 px box, "minutes". `form_edited(Field::Cooldown)`: empty →
    `COOLDOWN`, a whole number → it, else "Write the cooldown in minutes."; `load` shows `cooldown`.
  - The row: the next column `id("automation-next-<id>")`, `max_w(px(LAST_W))`, `truncate`; Run now of a saved FIRED
    prompt `opacity(0.4)`, tooltip "It acts on the agent that fires it", no `on_click`. The empty text as above.
  - Verify the look (spacing, the toggles' wrap, the warning's wrap) by the drawn tests above; no display at hand:
    say so in the As built.
- [ ] **Step 4: Run** `cargo test -p templum workspace::tests::automations` → PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 7: Docs, the full check and the benches

**Files:**
- Modify: `docs/specs/automations.md` (Status: batch 2 implemented, uncommitted until reviewed; **As built, Batch 2**:
  what was chosen where this plan left it open, and what differs)
- Modify: `docs/guide.md` (Automations: the three events, who, the cooldown, the agent that fired it)
- Modify: `docs/ARCHITECTURE.md` (the automations entry: events from `watch_agents`' queue and the terminals' counts,
  acted on at the look; slots per block that fired it)
- Modify: `docs/minors.md` (only what was deferred)

- [ ] **Step 1: Write the docs**, lines ≤ 120 columns.
- [ ] **Step 2: Full check:** `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test
  --workspace` → all pass.
- [ ] **Step 3: Reduced env:** `env -i PATH=~/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=~/.cargo
  RUSTUP_HOME=~/.rustup cargo test --workspace --no-fail-fast` → all pass. Run the automations tests three times in a
  row (`cargo test -p templum workspace::tests::automations`) while another `cargo build --release` loads the CPU:
  they pass every time.
- [ ] **Step 4: Benches:** `cargo run --release -p templum -- --bench` and `cargo run --release -p templum --
  --bench-mixed` → each JSON line's `p95_ms` ≤ 16.7; report both lines.
- [ ] **Step 5: Report** each command's result honestly, failures included. **Commit nothing.**
