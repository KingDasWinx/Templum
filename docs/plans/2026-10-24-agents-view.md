# Agents View Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** a Sonnet 5.5 (high) implementer in a Templum block, then an Opus 5.5 (xhigh)
> reviewer, in a loop until the reviewer passes it. The implementer **commits nothing**: everything stays
> uncommitted for the orchestrator.

**Goal:** Kanban card 153, "a global view of all agents": an **Agents** popup, opened from a sidebar button above
Requests and from Ctrl+Shift+L, lists every running agent of every workspace with its state and age, marks the agents
that finished while the person wasn't looking (**Done**, counted on the button), and goes to any agent in one press.

**Architecture:** A new model layer, `crates/templum/src/workspace/agents_view.rs`, derives each agent's state from
the same inputs, in the same precedence, as its block's dot (`block::status_dot`); watches each agent's state
(`Pane::watch`) wherever Templum hears a change (a frame, a bus message, a look at the screens) to age states and mark
Done; and builds the grouped, ordered, filtered rows plus the popup's selection and keys. A new view,
`crates/templum/src/agents_view.rs`, draws the popup like `inbox.rs` (two columns, a virtual list) and the sidebar
button with its badge. The card rule moves out of `Workspace::why` so the inbox and the view share it.

**Tech Stack:** Rust, GPUI (gpui-fast 0.1.8) + gpui-kit 0.7.3 (pinned): `uniform_list`,
`UniformListScrollHandle`, `ScrollStrategy::Nearest`, `InputState`/`Input`, `capture_key_down`.

**Spec:** [docs/specs/agents-view.md](../specs/agents-view.md) (read it first; this plan argues from it). Model files
to read before starting: `crates/templum/src/inbox.rs`, `crates/templum/src/workspace/inbox.rs`,
`crates/templum/src/workspace/tests/inbox.rs`, `crates/templum/src/block.rs` (`status_dot`),
`crates/templum/src/gallery.rs` (`uniform_list`, its search field).

## Global Constraints

- Only `crates/templum` depends on the UI framework; no other crate changes.
- Every logic change ships a headless test in `crates/templum/src/workspace/tests/agents_view.rs` (new, declared in
  `tests/mod.rs` beside `mod inbox;`) that fails without it; UI-only looks: say how they were verified.
- No `unsafe`; no speculative structure; theme tokens only for colors; nothing animates.
- The popup is window chrome: the canvas's zoom never scales it.
- Words, verbatim from the spec: tooltip "Agents"; header "Agents"; close tooltip "Close (Esc)"; placeholder "Find an
  agent"; chips "All", "Needs you", "Working", "Done", "Idle"; "Mark all seen"; "Go to terminal"; "Open request";
  empty states "No agent is running", "No agent matches"; group header "*Name* · *n* need you"; states "Needs you",
  "Working", "Working · *n* subagents", "Waiting for 1 subagent", "Waiting for *n* subagents", "Done", "Idle",
  "Starting"; command `all_agents`, "Show all agents", `ctrl-shift-l` (`cmd-shift-l` on macOS).
- The list column is 340 px; rows come from `uniform_list`; the screen lines (12) are read for the selected agent only.
- Minors deferred go to docs/minors.md under their area; docs lines ≤ 120 columns.
- Before handing over, the full check:
  `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`,
  and the benches `cargo run --release -p templum -- --bench` and `-- --bench-mixed` (p95_ms ≤ 16.7).
- **Commit nothing.** Every "Step: leave uncommitted" means: run the task's tests, then go on.

## Review Focus

1. The selected agent's block closes (or its agent quits) while the popup is open: the first listed row is selected,
   nothing panics. → test in Task 4 (`the_selection_follows_the_agent`).
2. A Done agent's block moves to another workspace: it stays Done and its row moves to that workspace's group. →
   test in Task 3 (`a_moved_block_keeps_its_done_mark_in_its_new_group`).
3. A turn ends while no frame is drawn (the window hidden or minimized): its Done is aged from the event, and two such
   agents order newest first. → test in Task 2 (`done_is_aged_from_the_event_not_the_next_frame`).
4. A Done agent quits: it leaves the list and the badge; a new agent in the same block starts with no mark. → test
   in Task 2 (`an_agent_that_quits_takes_its_done_mark_with_it`).
5. A row under the pointer changes state: it keeps its place until the pointer leaves; a row that goes away leaves
   its place; a new one joins its group's end. → test in Task 3 (`the_order_holds_while_the_pointer_is_over_the_list`).

---

### Task 1: An agent's state, as its dot tells it

**Files:**
- Create: `crates/templum/src/workspace/agents_view.rs` (the model; module doc pointing at docs/specs/agents-view.md)
- Modify: `crates/templum/src/workspace/mod.rs` (`pub(crate) mod agents_view;` beside `pub(crate) mod inbox;`)
- Create: `crates/templum/src/workspace/tests/agents_view.rs`; modify `crates/templum/src/workspace/tests/mod.rs`
  (`mod agents_view;`)

**Interfaces:**
- Consumes: `Workspace::asking(&self, terminal: &str) -> Option<(&Wait, usize)>` (waits.rs), `Wait::summary()`,
  `Alert` (alert.rs), `Pane::{state, seen, starting, awaits_subagents, subagents, subagents_at_work(), alert, term}`.
- Produces:
  ```rust
  #[derive(Clone, Debug, PartialEq)]
  pub(crate) enum Doing {
      /// What it asks: a request's summary, an alert's text, its dialog's words; may be empty.
      NeedsYou(String),
      /// Its turn ended while these subagents work or owe a report (`Pane::subagents.len()`).
      AwaitsSubagents(usize),
      /// At work, with how many of its subagents work too.
      Working(usize),
      Done,
      Idle,
      Starting,
  }
  impl Doing {
      pub(crate) fn label(&self) -> String;   // the row's words, Global Constraints
      pub(crate) fn rank(&self) -> u8;        // NeedsYou 0, Done 1, AwaitsSubagents 2, Working 3, Starting|Idle 4
      pub(crate) fn works(&self) -> bool;     // Working | AwaitsSubagents
  }
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
  pub(crate) enum Show { #[default] All, NeedsYou, Working, Done, Idle }
  impl Show {
      pub(crate) const ALL: [Show; 5];
      pub(crate) fn name(self) -> &'static str;          // "All", "Needs you", "Working", "Done", "Idle"
      pub(crate) fn shows(self, doing: &Doing) -> bool;  // Working ⊇ AwaitsSubagents; Idle ⊇ Starting
  }
  impl Workspace {
      /// Never `Done`: that is the watch's (Task 2).
      pub(crate) fn doing(&self, pane: &Pane) -> Doing;
  }
  ```

- [ ] **Step 1: Write the failing tests** in `tests/agents_view.rs`. A helper opens a terminal at a row of the
  canvas and makes it an agent: copy `open_terminal` from `tests/inbox.rs` (or make that one `pub(super)`), then set
  `seen.agent = Some("claude".into())` and, on Unix, `found` it as `tests/delegate.rs`'s `agents_running` does, so a
  snapshot during `after` doesn't take the agent away. Name it
  `fn agent(handle, ws, cx, row: u8) -> (BlockId, String /* terminal */)`.
  - `#[test] fn a_states_words()`:
    ```rust
    assert_eq!(Doing::NeedsYou("Allow Bash?".into()).label(), "Needs you");
    assert_eq!(Doing::Working(0).label(), "Working");
    assert_eq!(Doing::Working(2).label(), "Working · 2 subagents");
    assert_eq!(Doing::AwaitsSubagents(1).label(), "Waiting for 1 subagent");
    assert_eq!(Doing::AwaitsSubagents(3).label(), "Waiting for 3 subagents");
    assert_eq!((Doing::Done.label(), Doing::Idle.label(), Doing::Starting.label()),
               ("Done".into(), "Idle".into(), "Starting".into()));
    assert!(Show::Working.shows(&Doing::AwaitsSubagents(1)) && Show::Idle.shows(&Doing::Starting));
    assert!(!Show::Done.shows(&Doing::Idle));
    for s in Show::ALL {
        assert_eq!(s.shows(&Doing::Working(0)), matches!(s, Show::All | Show::Working));
    }
    ```
  - `#[gpui_kit::test] fn a_rows_state_follows_its_dot(cx)`: one agent `(id, terminal)`; for each setup below,
    written straight into its `Pane` (`ws.space.terminal_mut(id)`), assert `ws.doing(pane)`:
    - `state = Some(Working)` → `Working(0)`; plus `add_subagent("a1")`, `add_subagent("a2")` → `Working(2)`;
    - then `awaits_subagents = true`, one of them `reported = true` → `AwaitsSubagents(2)`;
    - `state = Some(NeedsYou(Some("Allow Bash?".into())))` → `NeedsYou("Allow Bash?")`;
    - `state = Some(Working)`, `alert = Some(Alert::Attention(Some("Claude needs your permission".into())))` →
      `NeedsYou("Claude needs your permission")`;
    - `alert = Some(Alert::Finished(Outcome { code: Some(1), took: 9s }))`, `state = Some(Working)` → `Working(0)`
      (a command's news isn't the agent's);
    - `state = Some(Idle)` → `Idle`; `state = None` → `Idle`;
    - `seen.agent = None`, `flags_for = Some("claude".into())`, `state = Some(Working)` → `Idle` (the dot ignores a
      state its snapshot can't back);
    - `ws.cover_start(id, "Claude Code".into(), cx)` → `Starting`;
    - an `ask_user` from `terminal` (`call_later(..., "ask_user", json!({"question": "Ship it?", "options": ["Yes"]}))`,
      as `tests/inbox.rs`'s `asks`) with `state = Some(Working)` → `NeedsYou("Ship it?")` (the request's summary).
- [ ] **Step 2: Run them to see them fail**
  Run: `cargo test -p templum agents_view`
  Expected: FAIL to compile (`Doing`, `Show`, `doing` missing).
- [ ] **Step 3: Implement** `Doing`, `Show` and `Workspace::doing` in `workspace/agents_view.rs`. `doing` follows
  `block::status_dot`'s precedence exactly (read it first): a request waits (`asking`) unless the alert is a command's
  news → `NeedsYou(wait.summary())`; an `Alert::Attention` → `NeedsYou(alert.text())`; then, with `pane.state`
  filtered by `seen.agent.is_some()`: `NeedsYou(asks)` → `NeedsYou(asks or "")`; else `pane.starting.is_some()` →
  `Starting`; else `Working` → `AwaitsSubagents(pane.subagents.len())` when `awaits_subagents`, else
  `Working(pane.subagents_at_work())`; `Idle` or none → `Idle`.
- [ ] **Step 4: Run them to see them pass**
  Run: `cargo test -p templum agents_view`
  Expected: PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 2: The watch: ages and Done

**Files:**
- Modify: `crates/templum/src/workspace/agents_view.rs` (`Watch`, `watch_agents`, `done_count`, `mark_all_seen`)
- Modify: `crates/templum/src/workspace/mod.rs` (`Pane::watch`; the look loop at `~1636`),
  `crates/templum/src/workspace/blocks.rs` (`Pane` construction `~780`: `watch: Default::default()`),
  `crates/templum/src/workspace/render.rs` (after `self.hear(window, cx)` `~313`),
  `crates/templum/src/workspace/bus.rs` (`serve` `~164`)
- Test: `crates/templum/src/workspace/tests/agents_view.rs`

**Interfaces:**
- Consumes: `Doing`, `Workspace::doing` (Task 1).
- Produces:
  ```rust
  /// What the Agents view keeps of a terminal's agent between frames; in memory only.
  #[derive(Default)]
  pub(crate) struct Watch {
      /// Its state as last seen (never Done), and since when its kind holds.
      pub was: Option<(Doing, Instant)>,
      /// Its turn ended since the person last looked at its block.
      pub done: bool,
  }
  impl Watch {
      /// The state its row shows: `Done` when `done` and idle; None before the first watch.
      pub(crate) fn shown(&self) -> Option<(Doing, Instant)>;
  }
  impl Workspace {
      pub(super) fn watch_agents(&mut self, active: bool, now: Instant);
      pub(crate) fn done_count(&self) -> usize;
      pub(crate) fn mark_all_seen(&mut self, cx: &mut Context<Self>);
  }
  // Pane gains: pub watch: agents_view::Watch
  ```

- [ ] **Step 1: Write the failing tests.** Turn ends are driven by hooks, as `tests/subagents.rs` does: make its
  `hook` `pub(super)` and use `json!({"hook_event_name": "UserPromptSubmit", "prompt": "go"})` (a turn starts) and
  `json!({"hook_event_name": "Stop", "last_assistant_message": "ok"})` (it ends). Make the window active with
  `super::terminals::set_active(handle, true, cx)`. The focused block is `ws.space.focused`.
  - `a_turn_ending_unseen_marks_its_agent_done`: agents `a` and `b`; `a` focused. Both start a turn, `frame`, both
    end it, `frame`. Then:
    ```rust
    assert_eq!(shown(b), Doing::Done);            // shown(x) = pane.watch.shown().unwrap().0
    assert_eq!(shown(a), Doing::Idle, "looked at while it ended");
    assert_eq!(ws.done_count(), 1);
    ```
  - `looking_at_or_working_again_ends_done`: `b` Done as above; `ws.focus_block(b, ...)`, `frame` → `Idle`,
    `done_count() == 0`. Again Done; a new turn starts (`UserPromptSubmit`) → `shown(b)` is `Working(0)`, and after
    its end with `b` still focused → `Idle`. With the window inactive (`set_active(handle, false, cx)`) and `b`
    focused, a turn's end → `Done`; activating it and drawing a frame → `Idle`.
  - `done_is_aged_from_the_event_not_the_next_frame` (`since(x) = pane.watch.shown().unwrap().1`,
    `now = cx.executor().now()`): `b` and `c` start turns, `frame`; note `since(b)`; `b.add_subagent("a1")`, `frame`
    → `shown(b) == Working(1)` and `since(b)` unchanged (same kind). Then, with no frame drawn between: `b` ends;
    `after(cx, 1_000)`; `c` ends; `after(cx, 1_000)`; then `frame` → `since(c) > since(b)` and
    `now - since(b) >= 2s`.
  - `an_agent_that_quits_takes_its_done_mark_with_it`: `b` Done; its agent goes (`seen.agent = None`,
    `flags_for = None`), `frame` → `pane.watch.was.is_none()`, `!pane.watch.done`, `done_count() == 0`; it comes
    back (`seen.agent = Some(..)`, `state = Some(Idle)`), `frame` → `shown(b) == Idle`.
  - `mark_all_seen_clears_every_done_mark`: Done agents in the workspace shown and in a hidden one
    (`ws.new_space`, then `switch_to(0)`); `ws.mark_all_seen(cx)` → `done_count() == 0`, both `Idle`. A stopped
    workspace's agents never count (`space.stopped = true` on the hidden one with a Done agent → `done_count()`
    drops it).
- [ ] **Step 2: Run them to see them fail**
  Run: `cargo test -p templum agents_view`
  Expected: FAIL to compile (`Watch`, `watch_agents`, `done_count`, `mark_all_seen` missing).
- [ ] **Step 3: Implement.**
  - `watch_agents(active, now)`: first an immutable pass over every non-stopped space (`self.space` and
    `self.others`) collecting `(space index, BlockId, Option<Doing>)` for each terminal: `None` unless
    `pane.agent().is_some() && pane.follows.is_none()`. Then a mutable pass:
    - not an agent → `pane.watch = Watch::default()`;
    - first seen → `was = Some((doing, now))`;
    - another kind (`std::mem::discriminant`) → if the old one `works()` and the new is `Idle`, `done = true`; if the
      new one `works()`, `done = false`; `was = Some((doing, now))`;
    - the same kind → the payload changes, `since` stays;
    - `active` and the pane is `self.space`'s focused block → `done = false` (alert.rs's "looked at" rule, as `hear`).
  - `Watch::shown`: `(Done, since)` when `done` and `was` is `Idle`, else `was.clone()`.
  - `done_count`: panes of non-stopped spaces whose `watch.shown()` is `Done`. No other work: it runs every frame for
    the badge.
  - `mark_all_seen`: `done = false` on every pane; `cx.notify()`.
  - Call sites, each passing `cx.background_executor().now()`:
    - render.rs, right after `self.hear(window, cx)`: `self.watch_agents(window.is_window_active(), now)`;
    - bus.rs: rename today's `serve` to `serve_message` and add `pub(super) fn serve(&mut self, message, window, cx)`
      that calls it, then `self.watch_agents(window.is_window_active(), now)` (the bus loop and the tests' `event`
      both call `serve`);
    - mod.rs's look loop, after `ws.look_at_agents(cx)`: `ws.watch_agents(false, now)` (no window there; the next
      frame applies the looked-at rule before anything is painted).
- [ ] **Step 4: Run them to see them pass**
  Run: `cargo test -p templum agents_view`
  Expected: PASS; also `cargo test -p templum subagents bus inbox` still PASS (the `serve` split).
- [ ] **Step 5: Leave uncommitted.**

### Task 3: The rows: groups, order, filter, search, holding still

**Files:**
- Modify: `crates/templum/src/workspace/inbox.rs` (`cards_by_block`; `why` uses it)
- Modify: `crates/templum/src/workspace/agents_view.rs` (`AgentRow`, `agent_rows`, `listed_agents`,
  `agent_counts`, `hold`)
- Modify: `crates/templum/src/workspace/mod.rs` (fields `agents_show`, `agents_query`, `agents_hover`, `agents_held`
  and their defaults beside `inbox_*` `~1772`)
- Modify: `crates/templum/src/usage.rs` (`context_text`, extracted from `chip`)
- Test: `crates/templum/src/workspace/tests/agents_view.rs`

**Interfaces:**
- Consumes: `Doing`, `Show`, `Watch::shown` (Tasks 1–2); `Workspace::{order, agent_name, uid_of, caller}`.
- Produces:
  ```rust
  // workspace/inbox.rs: the inbox's card rule, once: per block uid, its newest card not in its board's last column.
  pub(crate) fn cards_by_block(&self) -> HashMap<String, (u64, String)>;
  // usage.rs: the chip's context part, "42%" or "84k ctx".
  pub(crate) fn context_text(s: &Session) -> Option<String>;
  // workspace/agents_view.rs
  #[derive(Clone, Debug)]
  pub(crate) struct AgentRow {
      pub space: String,            // its workspace's id
      pub space_name: String,
      pub id: BlockId,
      pub terminal: String,         // pane.term.id()
      pub title: String,            // pane.term.title()
      pub agent: String,            // agent_name, "" when unknown
      pub doing: Doing,             // Watch::shown()
      pub since: Instant,
      pub card: Option<(u64, String)>,
      pub task: Option<String>,     // Handed.text, whole; None when empty
      pub folder: Option<String>,   // term.cwd(), else seen.folder, displayed
      pub context: Option<String>,  // usage::context_text
  }
  impl Workspace {
      pub(crate) fn agent_rows(&self) -> Vec<AgentRow>;     // every agent watched, grouped, ordered
      pub(crate) fn listed_agents(&self) -> Vec<AgentRow>;  // + agents_show, agents_query, held while hovered
      pub(crate) fn agent_counts(&self) -> [usize; 5];      // per Show::ALL, of the rows the query matches
  }
  /// The order within a group: rank; NeedsYou longest waiting first, Done newest first; then the title
  /// (lowercased), then the terminal.
  pub(crate) fn by_state(a: &AgentRow, b: &AgentRow) -> std::cmp::Ordering;
  /// The row matches `query` (already lowercased, not empty): in its title, agent, folder, space_name, card
  /// (`"#{id} {title}"`) or task, lowercased.
  pub(crate) fn matches(row: &AgentRow, query: &str) -> bool;
  /// `rows` in the order `held` names (terminal ids), keeping each group together; rows `held` doesn't name go
  /// at their group's end, in `rows`' order.
  pub(crate) fn hold(rows: Vec<AgentRow>, held: &[String]) -> Vec<AgentRow>;
  // Workspace fields: agents_show: Show, agents_query: String, agents_hover: bool,
  //                   agents_held: RefCell<Vec<String>>
  ```

- [ ] **Step 1: Write the failing tests.** The pure ones build rows with a helper
  `fn row(space: &str, terminal: &str, title: &str, doing: Doing, since: Instant) -> AgentRow` (other fields empty).
  - `#[test] fn rows_order_by_state_then_title()`: with `t0 = Instant::now()` and `s = Duration::from_secs(1)`, rows
    `n1` NeedsYou at `t0`, `n2` NeedsYou at `t0 + s`, `d1` Done at `t0`, `d2` Done at `t0 + s`, `w`
    AwaitsSubagents(1), `k` Working(0), `i1` Idle titled "beta", `i2` Idle titled "Alpha", `st` Starting titled
    "gamma", `i3` and `i4` Idle both titled "same" (terminals "t4" and "t3"), shuffled, then
    `rows.sort_by(by_state)` → terminals `[n1, n2, d2, d1, w, k, i2, i1, st, t3, t4]`.
  - `#[test] fn a_search_matches_title_agent_folder_workspace_card_and_task()`: one row with title "Review", agent
    "Codex", folder "/home/me/parser", space_name "Workspace 2", card `(152, "Attachments")`, task
    "fix the parser\nthen tests". `matches` is true for `"review"`, `"codex"`, `"parser"`, `"workspace 2"`, `"#152"`,
    `"152"`, `"attach"`, `"then tests"`; false for `"zzz"`; a row without card or task matches `"review"` only by
    its title.
  - `rows_list_every_running_agent_but_stopped_ones_and_subagents`: in the workspace shown: agents `a`, `b`; a
    terminal with no agent; a subagent's block (a terminal whose `follows = Some(Follows { parent: a's terminal, id:
    "s1".into(), done: None })` and `seen.agent = Some("claude".into())`). A hidden workspace (`new_space`) with agent
    `x`; a second hidden one with agent `y`, then `stopped = true`. `frame`. Assert the rows' terminals are exactly
    `a`, `b`, `x` (as sets), and `x`'s row has `space_name == "Workspace 2"`.
  - `rows_group_by_workspace_shown_first`: workspace 1 (the sidebar's first) has agent `o` Idle; a new workspace
    (`new_space`, shown) has `k` Working and `n` NeedsYou; `frame` → `agent_rows()` terminals `[n, k, o]`; after
    `switch_to(0)` and a `frame` → `[o, n, k]`.
  - `the_card_and_the_task_flow_into_rows_and_filters_narrow_them`: agents `a`, `b`, `c`. A card titled
    "Attachments" assigned to `a`'s uid (`tests/mod.rs`'s `board_with_a_card`, then `edit_card` setting `title` and
    `assignee = Some(Assignee::Block(uid))`, as the inbox's `why` test does); `b` gets
    `handed = Some(Handed { text: "fix the parser\nthen tests".into(), ..Handed::new("x", 600s) })`. `frame` →
    `a`'s row `card == Some((card_id, "Attachments".into()))`, `b`'s `task` is the whole text. `agents_query =
    "then tests"` → `listed_agents()` is `[b]`; `"attach"` → `[a]`. With the query empty, `b` Working and `c` Done
    (made so as in Task 2's tests): `agents_show = Show::Working` → `[b]`; `Show::Done` → `[c]`;
    `agent_counts() == [3, 0, 1, 1, 1]`; with
    `agents_query = "then tests"`, `agent_counts() == [1, 0, 1, 0, 0]`.
  - `the_order_holds_while_the_pointer_is_over_the_list`: a pure `#[test]` of `hold`: fresh rows
    `[g1: a, c, e][g2: d]` (`b` gone, `e` new), held `[b, c, a, d]` → terminals `[c, a, e, d]`. And a gpui test:
    agents `a`, `b` Idle; `agents_hover = true`; `listed_agents()` once (it stores the order); `b` turns NeedsYou,
    `frame` → `listed_agents()` keeps the stored order; `agents_hover = false` → `b` first.
  - `a_moved_block_keeps_its_done_mark_in_its_new_group`: `b` Done; `ws.move_blocks(vec![b], None, window, cx)`
    (as `tests/inbox.rs`'s moved-away test), `frame` → `b`'s row is in the new workspace's group and is `Done`.
  - The existing `tests/inbox.rs::why_says_the_card_the_task_and_the_agents_words` keeps passing unchanged (the
    card rule moved, not changed).
- [ ] **Step 2: Run them to see them fail**
  Run: `cargo test -p templum agents_view`
  Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
  - `cards_by_block`: today's filter in `why` (every board, columns but the last, `Assignee::Block(uid)`), keeping
    the highest card id per uid. `why` becomes `self.cards_by_block().remove(&uid)`.
  - `agent_rows`: groups in the order `self.space` first, then `self.order()` without it; skipping stopped spaces;
    one row per terminal whose `watch.shown()` is Some (Task 2 watched it) and `follows` is None; cards from one
    `cards_by_block()` call; each group sorted with `by_state`.
  - `listed_agents`: `agent_rows()` filtered by `agents_show.shows(&row.doing)` and, with a query,
    `matches(row, &agents_query.to_lowercase())`; then, when `agents_hover`, `hold(rows, &agents_held.borrow())`;
    otherwise it stores the rows' terminals in `agents_held`. Groups are contiguous runs of the same `space`.
  - `agent_counts`: `Show::ALL.map(|s| …)` over `agent_rows()` matched by the query only.
- [ ] **Step 4: Run them to see them pass**
  Run: `cargo test -p templum agents_view inbox usage`
  Expected: PASS.
- [ ] **Step 5: Leave uncommitted.**

### Task 4: Opening, keys, selection, Go to terminal, Open request

**Files:**
- Modify: `crates/templum/src/keys.rs` (`Command::AllAgents`: the enum, `ALL` (now 40), `name` "all_agents",
  `title` "Show all agents", `defaults` `ctrl-shift-l` / `cmd-shift-l`, the `Keys` field `all_agents` and its
  get/get_mut arms, as `GoToWaiting`'s), `crates/templum/src/workspace/input.rs` (`command_key` `~736`:
  `Command::AllAgents => self.toggle_agents(window, cx)`)
- Modify: `crates/templum/src/workspace/agents_view.rs` (the popup's state and keys),
  `crates/templum/src/workspace/mod.rs` (fields `agents_open`, `agents_at`, `agents_search`,
  `_agents_search_sub`, `agents_list`), `crates/templum/src/workspace/inbox.rs` (`open_inbox`: `agents_open = false`),
  `crates/templum/src/workspace/git_ops.rs` (`open_git` `~48`: `agents_open = false`)
- Modify: `docs/specs/config.md` (`all_agents = "ctrl-shift-l"   # every agent, in one list` in `[keys]`, and
  `cmd-shift-l` in the macOS sentence)
- Test: `crates/templum/src/keys.rs` tests; `crates/templum/src/workspace/tests/agents_view.rs`

**Interfaces:**
- Consumes: `AgentRow`, `listed_agents` (Task 3); `Workspace::{reveal, show_request, asking, caller, focus_handle,
  command_key}`; `gpui_kit::component::input::{InputState, InputEvent}` (gallery.rs's search field).
- Produces:
  ```rust
  // Workspace fields
  pub(crate) agents_open: bool,
  pub(crate) agents_at: Option<String>,                 // the selected agent's terminal
  pub(crate) agents_search: Option<Entity<InputState>>, // made at the first open
  _agents_search_sub: Option<Subscription>,             // mirrors its text into agents_query
  pub(crate) agents_list: UniformListScrollHandle,
  // Workspace methods
  pub(crate) fn agents_selected(&self) -> Option<AgentRow>;   // agents_at if listed, else the first row
  pub(crate) fn open_agents(&mut self, window: &mut Window, cx: &mut Context<Self>);
  pub(crate) fn close_agents(&mut self, window: &mut Window, cx: &mut Context<Self>);
  pub(crate) fn toggle_agents(&mut self, window: &mut Window, cx: &mut Context<Self>);
  pub(crate) fn select_agent(&mut self, terminal: &str, window: &mut Window, cx: &mut Context<Self>);
  pub(crate) fn set_agents_show(&mut self, show: Show, cx: &mut Context<Self>);
  pub(crate) fn go_to_agent(&mut self, terminal: &str, window: &mut Window, cx: &mut Context<Self>);
  pub(crate) fn open_agent_request(&mut self, terminal: &str, window: &mut Window, cx: &mut Context<Self>);
  pub(crate) fn agents_keys(&self, window: &Window, cx: &App) -> bool;  // the search field has the keyboard
  pub(crate) fn on_agents_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>);
  // The list's items, shared with the view (Task 5): a header before each group's rows.
  pub(crate) enum Item { Header { space: String, name: String }, Row(AgentRow) }
  pub(crate) fn items(rows: Vec<AgentRow>) -> Vec<Item>;
  pub(crate) fn item_of(items: &[Item], terminal: &str) -> Option<usize>;
  ```

- [ ] **Step 1: Write the failing tests.**
  - keys.rs: `#[test] fn all_agents_is_ctrl_shift_l()`: `Command::AllAgents.defaults(false) == ["ctrl-shift-l"]`,
    `.defaults(true) == ["cmd-shift-l"]`, `.name() == "all_agents"`, and the default Linux keymap maps
    `typed("ctrl-shift-l")` to `Some(Command::AllAgents)` (as the neighbouring tests build it).
  - `git_requests_and_agents_are_never_open_together` (model: `tests/inbox.rs`'s Git/inbox test): `open_git` →
    `open_agents` → only Agents open; `open_inbox(None)` → only Requests; `open_git` → only Git; `toggle_agents`
    twice → open then closed. The shortcut: `window.press("ctrl-shift-l", cx)` (Linux keymap) opens it with the
    sidebar hidden (`ws.sidebar = false`) too.
  - `opening_gives_the_search_the_keyboard`: `open_agents` → `agents_keys(window, cx)`; `cx.simulate_input(handle,
    "rev")` → `agents_query == "rev"`; a click on the canvas over another block (as
    `a_press_outside_the_inbox_reaches_the_canvas_and_keeps_it_open`) → that block focused, `agents_open` still,
    `!agents_keys`; `close_agents` then leaves the keyboard on that block.
  - `the_selection_follows_the_agent`: agents `a`, `b`, `c` Idle; `open_agents`; `select_agent(b)`; `b` turns
    NeedsYou, `frame` (it moves first) → `agents_selected()` is still `b`; `remove_block(b)`, `frame` →
    `agents_selected()` is `listed_agents()[0]`, no panic; `set_agents_show(Show::Done)` with nothing Done →
    `None`.
  - `arrows_enter_and_esc`: two agents in the workspace shown, one in a hidden one; `open_agents`; with
    `rows = listed_agents()`: `press("down")` twice → `rows[2]` selected (across the header between the groups);
    `press("down")` again → still `rows[2]` (no wrap); `press("up")` → `rows[1]`; `press("enter")` →
    `!agents_open`, `rows[1]`'s workspace shown and its block focused. Reopen, `simulate_input("zz")`,
    `press("escape")` → `agents_query == ""` and still open; `press("escape")` → closed.
  - `go_to_terminal_clears_done`: a Done agent `x` in a hidden workspace; `go_to_agent(x)`, `frame` → its workspace
    shown, `space.focused == Some(x)`, `shown(x) == Idle`, `!agents_open`.
  - `open_request_opens_the_inbox_at_its_request`: in a hidden workspace an agent asks (`asks`), its key `k`;
    `open_agent_request(terminal)` → that workspace shown, `inbox_open`, `!agents_open`, `inbox_selected()` key `k`.
- [ ] **Step 2: Run them to see them fail**
  Run: `cargo test -p templum agents_view keys`
  Expected: FAIL to compile.
- [ ] **Step 3: Implement.**
  - `open_agents`: `agents_open = true`, `inbox_open = false`, `git_open = false`; makes the search field the first
    time (`InputState::new(window, cx).placeholder("Find an agent")`, subscribed to `InputEvent::Change` to copy its
    trimmed value into `agents_query` and `cx.notify()`); focuses it; scrolls the selected row into view
    (`agents_list.scroll_to_item(ix, ScrollStrategy::Nearest)`, `ix = item_of(&items(listed_agents()), terminal)`).
  - `close_agents`: as `close_inbox`: the canvas gets the keyboard back only if the field had it.
  - `select_agent`: `agents_at = Some(terminal)`, focus the field, `cx.notify()`. Arrow steps move `agents_at` over
    `listed_agents()` (none past the ends) and scroll to it.
  - `go_to_agent`: `caller(terminal)` → `(space, id)`; `close_agents`; `reveal(&space, id, window, cx)`.
  - `open_agent_request`: `asking(terminal)` → its oldest wait's key; `caller(terminal)` → its space;
    `show_request(&space, key, window, cx)` (which opens the inbox, closing Agents via `open_inbox`).
  - `on_agents_key`, registered by the view with `capture_key_down` on the popup (Task 5) so it runs before the
    field: plain `up`/`down` step; `enter` → `go_to_agent` on `agents_selected()`; `escape` → clear the field
    (`set_value("", window, cx)`) and `agents_query` when not empty, else `close_agents`; each handled key stops
    propagation. Other keys: the workspace's commands as `on_inbox_key` filters them (not `Copy`, `Paste`,
    `CloseTerminal`, `PreviousPrompt`, `NextPrompt`, `Find`) go to `command_key` and stop; anything else reaches the
    field.
  - config.md: add the `all_agents` line under `go_to_waiting` and `cmd-shift-l` to the macOS list.
- [ ] **Step 4: Run them to see them pass**
  Run: `cargo test -p templum agents_view keys config inbox git_`
  Expected: PASS (config.rs tests iterate `Command::ALL`: they must accept the new command unchanged or with its key
  added where they list keys).
- [ ] **Step 5: Leave uncommitted.**

### Task 5: The popup, the sidebar button and its badge

**Files:**
- Create: `crates/templum/src/agents_view.rs` (the view; module doc as `inbox.rs`'s); modify
  `crates/templum/src/main.rs` (`mod agents_view;` beside `mod inbox;`)
- Modify: `crates/templum/src/inbox.rs` (`section`, `dim` → `pub(crate)`; `badge` delegates to a new
  `pub(crate) fn count_badge(ws: &Workspace, count: usize, color: Hsla, id: &'static str) -> Option<AnyElement>`)
- Modify: `crates/templum/src/usage.rs` (`bar` → `pub(crate)`)
- Modify: `crates/templum/src/sidebar.rs` (the foot: `agents_view::button` above `inbox::button`; module doc)
- Modify: `crates/templum/src/workspace/render.rs` (`~735`: the Agents popup placed exactly as the inbox's, added to
  the same `.children(...)` layer)
- Modify docs: `docs/specs/workspaces.md` (The sidebar, "At the foot": Agents above Requests, its accent badge of
  Done agents, "99+", no pulse), `docs/specs/ask-inbox.md` (Non-goals: card 153 links to agents-view.md),
  `docs/guide.md` (`~269`: one sentence on the Agents button), `docs/ARCHITECTURE.md` (`~172`: `agents_view.rs` and
  `workspace/agents_view.rs` lines, as `inbox.rs`'s), `docs/specs/agents-view.md` (Status → Implemented, plus an
  "As built" section for what the builder chose)
- Test: `crates/templum/src/workspace/tests/agents_view.rs`

**Interfaces:**
- Consumes: everything above; `inbox::{age, badge_text, section, dim, count_badge}`, `usage::{chip, bar,
  context_text}`, `block::status_dot`, `balloon::ghost`, `git_block::widgets::icon_button`, `Handed`,
  `Pane::{subagents, follows, model, effort, role, usage, term.shared().lines(12)}`.
- Produces:
  ```rust
  pub(crate) fn popup(ws: &Workspace, w: f32, h: f32, window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement;
  pub(crate) fn button(ws: &Workspace, size: f32, cx: &mut Context<Workspace>) -> Div;
  ```
  Element ids the tests use: `"agents"` (popup), `"agents-button"`, `"agents-badge"`, `"agents-list"`,
  `"agents-row-{terminal}"`, `"agents-chip-{Show::name}"`, `"agents-seen"` (Mark all seen), `"agents-go"`,
  `"agents-request"`, `"agents-screen"`, `"agents-empty"`.

- [ ] **Step 1: Write the failing tests** (drawn, as `tests/inbox.rs`'s `drawn`, `click_id`):
  - `the_button_counts_done_agents`: no Done → `"agents-button"` drawn, `"agents-badge"` not; one Done in a hidden
    workspace → `"agents-badge"` drawn; `mark_all_seen` → gone. A click on `"agents-button"` toggles `agents_open`.
  - `rows_details_and_buttons_are_drawn`: two agents, one asking (`asks`); open; both `"agents-row-{t}"` drawn;
    clicking the asking one's row selects it (`agents_at`) and draws `"agents-request"` and `"agents-go"`; selecting
    the other → no `"agents-request"`; `"agents-screen"` drawn; `click_id("agents-go")` → `!agents_open` and its
    block focused. `click_id("agents-chip-Done")` → `agents_show == Show::Done`, `"agents-empty"` drawn; with a Done
    agent, `"agents-seen"` drawn and its click clears it.
  - `only_the_rows_in_view_are_drawn`: 30 agents in the workspace shown (open them with the `quiet()` command as
    `open_with` does, to keep the test light); open the popup; the first row is drawn, the 30th is not; select the
    30th (`select_agent` then a frame) → it is drawn (the list scrolled to it).
- [ ] **Step 2: Run them to see them fail**
  Run: `cargo test -p templum agents_view`
  Expected: FAIL (`agents_view::popup`/`button` missing, ids not drawn).
- [ ] **Step 3: Implement the view** (`crates/templum/src/agents_view.rs`), drawn like `inbox::popup`:
  - The frame: `inbox::popup`'s outer div (surface, hairline border, radius 12, shadow, `occlude`, wheel stopped),
    id `"agents"`, `.capture_key_down(cx.listener(… ws.on_agents_key …))`, no backdrop.
  - Left column, 340 px: header (`IconName::Bot`, "Agents", close button "Close (Esc)"), the `Input` for
    `agents_search`, the chips (`Show::ALL`, "*name* *count*" from `agent_counts()`, the chosen one `ui.pressed`,
    `flex_wrap`), `"agents-seen"` when `agents_show == Show::Done && done_count() > 0`, then the list:
    `uniform_list("agents-list", items.len(), …)` over `Rc<Vec<Item>>`, `items(listed_agents())` once per frame; a
    header says "*name* · *n* need you", `n` the group's NeedsYou rows in `agent_rows()` (the filter doesn't change
    it), the name alone when 0. Every item is one height (48 px); a header is
    dim, semibold, small, bottom-aligned. `.track_scroll(&ws.agents_list)`. On the list's container,
    `.on_hover(cx.listener(|ws, hovered: &bool, _, cx| { ws.agents_hover = *hovered; cx.notify() }))`.
  - A row (spec, A row): the dot from `block::status_dot(ws, pane)` (a hollow `title_dim` ring when None), the title,
    the agent dim, the context right; second line dim: the task's first line, else `#id title`, else the folder,
    and at the right `"{label} · {age}"` (`inbox::age(now - since)`), its color the dot's (Done: `ui.accent`). The
    selected row `ui.hover`; a click → `select_agent`.
  - Right column: sections with `inbox::section` / `inbox::dim` (From, State, Task, Card, Subagents, Usage, Screen,
    spec The details), scrolling over a foot holding `balloon::ghost("agents-go", "Go to terminal", …)` and, while
    `asking(terminal)`, `balloon::ghost("agents-request", "Open request", …)`. Usage: `usage::bar` of
    `context_percent` and `usage::chip`'s tooltip lines. Screen: `pane.term.shared().lines(12)` in a `"agents-screen"`
    box, the terminal's font family, each line `truncate`d. None selected: `"agents-empty"` with the spec's words.
  - `button`: as `inbox::button` without the pulse: id `"agents-button"`, `IconName::Bot`, tooltip "Agents",
    pressed fill while open, click → `toggle_agents`; `inbox::count_badge(ws, ws.done_count(), ws.ui.accent,
    "agents-badge")`.
  - render.rs: `let agents = self.agents_open.then(|| …)` placed like `inbox` (`git_popup_size`, left = sidebar width
    or 0, plus `GIT_GAP`, bottom `GIT_GAP`), into the same children list.
  - Docs as listed under Files.
- [ ] **Step 4: Run them to see them pass**
  Run: `cargo test -p templum agents_view inbox`
  Expected: PASS. Then look at it: `cargo run -p templum -- --dev` with three or more agents in two workspaces; check
  the badge after a turn ends in a hidden workspace, the chips, the search, the holding order under the pointer, the
  details, a light and a dark theme. Say in the hand-over what was looked at.
- [ ] **Step 5: Leave uncommitted.**

### Task 6: The full check and the benches

**Files:** none new; `docs/minors.md` for anything deferred (its area, where, what a person sees, a way to fix it).

- [ ] **Step 1: The full check**
  Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
  Expected: all PASS, no warnings. Report failures as they are.
- [ ] **Step 2: The benches** (the change paints the sidebar and a popup; CLAUDE.md, Performance)
  Run: `cargo run --release -p templum -- --bench` and `cargo run --release -p templum -- --bench-mixed`
  Expected: each prints a JSON line with `p95_ms` ≤ 16.7. Keep both lines verbatim for the pull request's
  description.
- [ ] **Step 3: Hand over, uncommitted:** the files changed, the test names added, the two JSON lines, what was looked
  at in the app, and any minors added.
