# Notice Batching Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Templum never types a notice over the person's unsent text, and the notices about tasks reach the agent that
handed them in batches.

**Architecture:** Notices queue in `Workspace::notices` and go out from `deliver_notices`, called at every agent look
(crates/templum/src/workspace/delegate.rs, mod.rs ~1625). Task 1 makes `Pane::takes_notice` wait for a *draft* (the
person's unsent text). Task 2 gives each queued notice a kind and holds task notices per recipient in a window,
typed as one paste when the window closes. Task 3 applies the same window to what the Templum Agent's engine is told.

**Tech Stack:** Rust, GPUI (gpui-fast 0.1.8) + gpui-kit 0.7.3 (pinned).

**Spec:** [docs/specs/notice-batching.md](../specs/notice-batching.md) (read it first).

## Global Constraints

- The window: **15 seconds**, restarted by each new task notice for the same agent; at once when that agent has no
  other task still running; never more than **60 seconds** for the oldest notice. Constants, no setting.
- Never type into an agent's input while it holds the person's unsent text; when the signals disagree, wait.
- Every logic change ships a test that fails without it (GPUI test-support tests in crates/templum/src/workspace/tests/,
  with the executor's test clock); UI looks: say how verified. Minors to docs/minors.md; docs ≤ 120 columns.
- Commits on `development`, Conventional Commits. Full check before each commit:
  `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

## Review Focus

1. Multi-line drafts: Shift+Enter (a new line in Claude Code/Codex) isn't a submit; the draft stays. → Task 1 test.
2. A paste by the person (Ctrl+Shift+V, middle click, drop) makes a draft like typed text. → Task 1 test.
3. The person's Enter while the agent waits for them (a dialog: `AgentState::NeedsYou`) answers the dialog, not a
   prompt: a draft typed before it stays a draft. → Task 1 test.
4. A held batch whose agent's terminal closes: the batch is dropped, its chip goes, nothing panics. → Task 2 test.
5. A batch due while Templum just typed into the agent (`NOTICE_GAP`) waits for the gap, then goes as one paste, not
   one per notice. → Task 2 test.

---

### Task 1: The person's draft holds notices back

**Files:**
- Modify: `crates/templum/src/workspace/mod.rs` (`Pane`), `crates/templum/src/workspace/delegate.rs`
  (`person_typed` ~365, `takes_notice` ~436), the places the person's keys and pastes reach an agent's terminal
  (workspace/input.rs: grep `person_typed`), the hook path that hears `Submitted` (workspace/bus.rs / activity.rs),
  `crates/templum/src/block.rs` (the chip), docs/specs/connections.md (Delegating, notices), docs/minors.md
- Test: `crates/templum/src/workspace/tests/notices.rs` (new, registered in tests/mod.rs)

**Interfaces:**
- Produces: `Pane::draft: Cell<bool>` (the person's unsent text is in its input) and `fn has_draft(&self) -> bool`;
  `takes_notice` returns `Some(false)` while `has_draft()`. The chip's id: `answers-waiting-{block}`, text
  "1 answer waiting" / "{n} answers waiting", shown while at least one queued notice for that terminal waits and its
  pane `has_draft()`.
- Signals (the builder fixes the details per agent family, with a test each): set by the person's printable keys and
  pastes into a terminal running an agent; cleared by the person's submitting Enter (Enter, not Shift+Enter, not while
  `NeedsYou`), a `Submitted` hook, Ctrl+C or Ctrl+U, and by an agent look that reads the agent's input line empty (the
  prompt marker with nothing after it; `SharedTerm::lines` already leaves the faint suggestion out).

- [ ] **Step 1: Failing tests** (tests/notices.rs; use the delegate tests' fake agents and notices):
  - `a_draft_holds_a_notice_however_long_ago_it_was_typed`: the person types "half a sen" into the asker; a task ends;
    advance 30 s of looks → the notice isn't typed; the chip reads "1 answer waiting".
  - `sending_the_draft_lets_the_notice_in`: then the person presses Enter → after `NOTICE_GAP` the notice is typed, the
    chip is gone.
  - `clearing_the_draft_lets_the_notice_in`: Ctrl+C instead of Enter → typed.
  - `an_input_seen_empty_ends_the_draft`: the person types "x", then Backspace; the screen shows the bare prompt →
    the next look types the notice.
  - `shift_enter_keeps_the_draft` (Review Focus 1), `a_paste_is_a_draft` (Review Focus 2),
    `enter_on_a_dialog_keeps_the_draft` (Review Focus 3).
  - `a_hooked_submit_ends_the_draft`: a `Submitted` hook from the asker clears it.
- [ ] **Step 2: Run** `cargo test -p templum notices` → FAIL.
- [ ] **Step 3: Implement** the interfaces above. Add to docs/minors.md (Connections) the reverse race the spec keeps:
  Templum pastes a notice and the person types before its Enter → no Enter, the paste stays with their keys
  (`send_task`, `Step::Typing`); a fix to consider later: re-queue it, accepting a duplicate if the person sends both.
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `fix(connections): an answer from another agent never goes in with the person's unsent text`.

### Task 2: Task notices arrive in batches

**Files:**
- Modify: `crates/templum/src/workspace/delegate.rs` (`Notice` ~101, `queue_notice` ~1519, `deliver_notices` ~1526,
  `tell_end` ~1424), `crates/templum/src/workspace/agent.rs` (`notice_to` ~113), the other `queue_notice` callers
  (schedules.rs, questions.rs: they queue `NoticeKind::Other`), docs/specs/connections.md, docs/specs/agent-protocol.md
- Test: tests/notices.rs

**Interfaces:**
- Consumes: Task 1's `takes_notice`.
- Produces:
  - `pub(crate) enum NoticeKind { Task(NoticeLevel), Other }`; `Notice { to, text, kind, at: Instant }`;
    `queue_notice(to, text, kind, cx)`; `notice_to` queues `NoticeKind::Task(level)`.
  - `const BATCH_WAIT: Duration = Duration::from_secs(15)`, `const BATCH_MOST: Duration = Duration::from_secs(60)`.
  - `fn still_running(&self, asker: &str) -> bool`: some terminal's `Handed` with `by == asker` is still awaited, or
    an end notice for `asker` is on its way (`tell_end`'s `SPENT_AFTER` delay: count them in a field
    `ending: HashMap<String, usize>`, up before the delay, down when queued).
  - A recipient's task notices go together when it `takes_notice` and: its newest task notice is `BATCH_WAIT` old, or
    its oldest is `BATCH_MOST` old, or `!still_running(to)`. `Other` notices go as soon as `takes_notice` allows.
  - One batch of several is one paste, one Enter: a title line, then the notices oldest first, a blank line between.
    Titles: all task ends → "[{n} tasks ended]"; mixed → "[{n} notices: {a} tasks ended, {b} wait for their person]"
    (the builder words the other kinds the same way; `NoticeLevel` tells them apart). A batch of one: as today.

- [ ] **Step 1: Failing tests:**
  - `notices_wait_for_the_others_and_go_as_one`: an asker with 3 tasks; two end 5 s apart → nothing typed; the third
    ends → one paste starting "[3 tasks ended]" with the three notices (the last running one ended: at once).
  - `the_window_restarts_with_each_notice`: 4 tasks; ends at 0 s and 10 s → nothing typed at 20 s; typed at 25 s (15 s
    after the second), as one paste of 2.
  - `no_notice_waits_more_than_a_minute`: 6 tasks ending every 10 s → a paste at 60 s with what came so far.
  - `a_lone_task_ends_at_once`: one task → typed with no wait, as today (no title).
  - `other_notices_dont_wait_for_the_window`: a `wait_for` with `wait: false` matching while tasks run → typed at once.
  - `twenty_tasks_ending_together_make_one_batch`: 20 tasks end within 1 s (their `SPENT_AFTER` delays overlapping) →
    one paste "[20 tasks ended]".
  - `a_batch_of_a_closed_terminal_is_dropped` (Review Focus 4), `a_batch_waits_the_gap_as_one` (Review Focus 5).
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** the interfaces above; specs: describe the window and the batch's shape where notices are
  described (connections.md Delegating 10, agent-protocol.md).
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `feat(connections): answers from many agents reach the one that handed them in one message`.

### Task 3: The Templum Agent's engine hears batches too

**Files:**
- Modify: `crates/templum/src/workspace/agent.rs` (`agent_notice` ~136, `tell_engine`), crates/templum/src/chat/mod.rs
  (state), docs/specs/templum-agent.md, docs/specs/notice-batching.md (Status: Implemented, with the commits)
- Test: `crates/templum/src/workspace/tests/templum_agent.rs`

**Interfaces:**
- Consumes: Task 2's `NoticeKind`, `BATCH_WAIT`, `BATCH_MOST`, `still_running(templum_agent::ASKER)`.
- Produces: `agent_notice` adds the chat row, `unread` and the run meter's count at once, as today, and holds only the
  engine message: a field on `TemplumAgent`, `told: Vec<(String, Instant)>`, flushed by the same rule from the look
  loop (`deliver_notices` or beside it) as one `tell_engine` call with Task 2's batch shape. No draft rule here.

- [ ] **Step 1: Failing tests:**
  - `the_chat_shows_each_notice_at_once_and_the_engine_hears_a_batch`: the Templum Agent with 3 tasks; two end →
    two rows at once, the fake engine got nothing; the third ends → the engine got one message starting
    "[3 tasks ended]".
  - `a_lone_notice_reaches_the_engine_at_once`.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement**; specs: templum-agent.md's notices; notice-batching.md's Status line with the three
  commits.
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `feat(agent): the Templum Agent hears its agents' answers in one message`.
