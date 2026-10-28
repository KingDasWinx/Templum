# Templum to Templum, Batch 2h (Agents, Automations and the Inbox) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer in a Templum agent block does the whole plan, then
> fresh Opus 5.5 reviewers review it in a loop until one passes it. The implementer **commits nothing**: everything
> stays uncommitted for the orchestrator.

**Goal:** Kanban card 171 part 2, batch 2, the last of eight parts: the mirror's **Agents popup** lists home's agents
under "API · winx-desktop" with their task, model, role, usage and screen, and a Done of its own; the **Automations
popup** shows home's rules of the shared workspace and home's agents' schedules in a group of their own and edits,
switches, runs, duplicates and deletes them **at home** through home's API; **home's automations never run on the
mirror** (the person's hard rule) and the mirror's own rules never offer or target a remote workspace; the **inbox**
ages a remote request from home's `since`, lists one whose block has no viewer, and says how many more home has past
50. Home gains an agent's `task` and `role`, `schedules`, `cancel_schedule` and the notice of a rule's Notify me.
Batch 2 is then complete: the spec's Status, `docs/api.md` §7 and the guide say so.

**Architecture:** Home first (Tasks 1–2): new state fields and one new API tool, on the patterns of 2e (a collection
with its singular event) and 1a (an own tool). Then the rule on the mirror's own engine (Task 3): every door a local
rule could fire through is guarded and tested; home's rules never enter `Workspace::automations`. Then, on 2d's and
2e's pattern, `RemoteSpace` keeps cleaned, capped copies of home's rules and schedules made from home's state where it
enters (`workspace/remote_rules.rs`, Task 4). The Automations popup (Tasks 5–6) addresses a home rule by a popup key in
a range no local id reaches (`HOME_KEYS | n << 32 | home id`, as `RemoteSpace::fresh_id` keeps the mirror's own ids
apart): every function the popup calls with a key routes a home key to a call of home's (`remote_call`, asked of
`why_not_in` first) and a local one to today's code, so local behaviour and its 3,300 lines of tests stay. The Agents
popup (Task 7) already lists remote panes (`watch_agents` walks every space): it gains their group, their details from
`RemotePane::agent`, a screen read through `read_terminal`, and home's own `done` read as idle. The inbox (Task 8)
changes three spots of `remote.rs`/`waits.rs`. Task 9 adds the end-to-end cases and closes batch 2's docs.

**Tech Stack:** Rust 2024; GPUI through gpui-kit `=0.7.3` (only in `crates/templum`); `templum-api` (`State`, `Arc`
items, `visible`, `diff`, `parse`, the registry, `openapi`, `audit`, the client's `notice_of`); 2a's
`remote_call`/`Then`/`Deed`/`Nay`/`why_not_in`; 2b's `warn`, `still_shown`; 2d's `uid_title_in`, `boards_in`; serde.

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), "Batch 2: the whole workspace, live":
"The rule: home's automations run only at home" (the person's hard rule), "The Agents popup", "The Automations popup",
"The Requests inbox", "Notices", "One rule for what the mirror can do", "What works on a remote workspace" (the
automation and `cancel_schedule` rows), "What home's API gains" (`schedules`, an agent's `task` and `role`, `notice`'s
`automation`, a limited token's view, `cancel_schedule`), "The mirror's side" (caps, never a relay), "Constraints
(batch 2)", "Parts ▸ 2h", the threat model's *(batch 2)* rows, "As built ▸ 1d, 2a–2g"; the 2a–2g plans' "Interfaces
for later parts" (2a: notices gain `automation`, 2h's tools through `remote_call`; 2b: `RemoteSpace::agents` for the
Agents popup's From and the editor's lists; 2d: `uid_title_in`; 2e: a collection's shape). Also
[automations.md](../specs/automations.md), [agents-view.md](../specs/agents-view.md),
[ask-inbox.md](../specs/ask-inbox.md), [agent-tools.md](../specs/agent-tools.md#schedule-and-cancel_schedule).

**Code it builds on:** `crates/templum-api/src/{model.rs,openapi.rs,registry.rs,audit.rs,client/mod.rs}`;
`crates/templum/src/automations/mod.rs`, `crates/templum/src/automations_view/{mod.rs,editor.rs}`,
`crates/templum/src/{agents_view.rs,inbox.rs,usage.rs,block.rs,space_dialog.rs,share_dialog.rs}`,
`crates/templum/src/workspace/{automations.rs,schedules.rs,agents_view.rs,inbox.rs,waits.rs,bus.rs,api.rs,
api_state.rs,remote.rs,remote_boards.rs,spaces.rs,mod.rs}`; tests
`crates/templum/src/workspace/tests/{mod.rs,api.rs,automations.rs,agents_view.rs,remote.rs,schedules.rs,delegate.rs,
bus.rs,terminals.rs,mirror_end_to_end.rs}`.

**Consumes (as built):** `Automations::{add, get, get_mut, remove, duplicate}`, `automations::{Automation, Do, When,
Who, Heard, LastRun, Went, COOLDOWN, NAME_MAX, FIRED, event_words, event_short}`, `clock::{time_words, next_after,
when_problem}`; `Workspace::{automations, automation_runs, automation_drafts, automations_at, automation_form,
automation_heard, automating, start_automations, look_automations, look_at_automations, automation_ids,
automation_selected, automation_draft, edit_automation, save_automation, save_rule, revert_automation,
new_automation, duplicate_automation, ask_delete_automation, delete_automation, set_automation_on,
run_automation_now, automation_target, automation_problem, when_said, automation_sentence, automation_loop,
automation_folder_shown, next_words, last_words, automation_last_line, automations_unseen, open_automations,
show_automation, select_automation, schedules, schedule_add, schedule_rows, cancel_schedule_now, scheduling,
watch_agents, doing, agent_rows, listed_agents, done_count, select_agent, agents_at, agents_open, go_to_agent,
open_agent_request, cards_by_block, waits, wait_as, asker, orphaned, tell_asking, tell_folded, inbox, show_request,
go_to_request, api_notice, api_own, api_allows, api_scope, uid_title_in, boards_in, why_not_in, remote_call, warn,
read_home_agents, delete_space_as}`; `RemoteSpace::{state, home, entry, me, status, tools, agents, streaming, dim,
home_workspace, why_not, takes}`, `Deed::{call, needs}`, `Nay::words`, `RemotePane::{block, title, agent,
read_only}`, `remote.rs`'s `clean_text`, `LINE_CHARS`, `TEXT_CHARS`, `MOST_REQUESTS`, `tidy_block`, `remote_wait`,
`remote_requests`, `remote_notice`, `apply_remote`, `Space::{is_remote, mirror_title, mirror_of}`,
`mirror::computer_of`, `bus::{clean_title, clean_to}`, `Handed::{id, by, since, text}`, `Pane::role`,
`templum_api::ASKER` ("api"), `usage::chip`; the tests' `offline`, `live`, `show_remote`, `apply`,
`terminal_of_home`, `terminals_of_home`, `home_allows`, `offline_asked`, `state_asking`, `sent`, `answer`,
`refusal`, `said`, `hear`, `notice`, `remote_of`, `pump_home`, `pump_until`, `both_ends_as`, `with_remote`,
`shown_mirror_as`, `mirrored`, `three_blocks`, `agents_running_in`, `me`, `event`, `call`, `call_later`, `after`,
`set_active`, `frame`, `with_token`, `tool`, `turn_on`, `with_data`, `notify`, `rule`, `due`, `look`, `last`.

## Global Constraints

- **MEMORY LIMITS on this machine:** **every** cargo command takes `-j 4`; **never two cargo commands at once**; never
  `cargo clean`; never `git stash`; while working run only the targeted tests each step names; the full suite
  **once**, in Task 9.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-api`, `templum-mcp`, `templum-git` and
  `templum-canvas` stay UI-free. No `unsafe`. **No new dependency.**
- **THE RULE (the person's, hard):** home's automations run only at home. The mirror never copies a home rule into
  `Workspace::automations` or `automations.json`, never schedules (`automation_runs`), runs (`attempt`, `try_do`) or
  fires one (`automation_heard`); it changes them only through home's `save_automation`, `set_automation`,
  `run_automation`, `delete_automation`. Its own rules never list, default to or target a remote workspace; a stale
  one whose Where is a remote workspace is Skipped with the rule's words; its event rules never hear a remote agent or
  a notice of home's; home's agent schedules are listed and cancelled at home (`cancel_schedule`), never put in
  `Workspace::schedules`, never delivered. Every line has its own test (Task 3, and Tasks 5, 6 and 9).
- **Home is the truth:** what the mirror shows of home's rules, schedules and agents is home's state, cleaned and
  capped where it enters; an act is home's tool through `remote_call`, asked of `why_not_in(space, deed)` first; a
  refusal says its sentence (`warn`, or the editor's red line). No hold for a switched rule or a cancelled schedule
  (Open for the orchestrator, 2): they show a round trip later.
- **Never a relay:** the mirror's API state, control server and block server gain nothing of home's: `api_state`
  builds `schedules` from `Workspace::schedules` (this computer's only) and an agent's `task`/`role` from local panes
  only (remote blocks have no uid, so `block_views` skips them, as today).
- **Home's ids meet the mirror's:** in `mirror_end_to_end.rs` one workspace is both ends, so home's rule 1 and block
  `abc12` are also the mirror's own. A home rule is reached only by its popup key (`HOME_KEYS` and up); no local
  function (`automation_target`, `automation_problem`, `uid_title`, `find_block`, `automation_folder`,
  `run_automation_now`'s local arm, `save_rule`) may be called with a home rule's id or workspace.
- **What home sends is bounded and cleaned before it is kept or drawn** (`remote_rules.rs`, once where it enters):
  at most `MOST_RULES` = 500 rules a remote, only those whose `workspace` is the remote's home workspace and whose id
  is below 2³² and whose `when`/`do` parse; a name `clean_title` cut to `NAME_MAX` (60); a prompt, command, notify or
  agent prompt `clean_text(_, RULE_TEXT_CHARS)` (65,536); a uid, kind, agent, model, effort, folder or terminal one
  line `clean_to(_, LINE_CHARS)` (300), a uid that has a control character or is longer than 64 bytes drops the rule;
  a cron expression `clean_to(_, LINE_CHARS)`; at most 100 blocks in a Who; `last.about` and `sentence`
  `clean_to(_, LINE_CHARS)`. At most `MOST_SCHEDULES` = 200 schedules, `text` its first line cut to
  `SCHEDULE_TEXT_CHARS` = 300, ids plain (≤ 16 bytes for `s3`, ≤ 64 for a uid, no control character) else dropped.
  An agent's `task.text` `clean_text(_, TASK_CHARS)` (2,000), `task.id` plain ≤ 16 bytes (else no task), `task.by`
  plain ≤ 64 bytes (else None), `role` `clean_title`. A screen read: its last `SCREEN_LINES` (12) lines, each
  `clean_to(_, LINE_CHARS)`.
- **The UI thread never waits on the network:** every act is `remote_call` (its `Then` runs when home answers); the
  screen read is a `remote_call` paced by a timer task, never awaited on the UI thread.
- **The rule's words for the popups** (rung order as built: not yet, not connected, older, needs Full): new deeds
  `SetRule(bool)`, `RunRule(bool)`, `SaveRule(bool)` (true: the rule runs a command), `DeleteRule`,
  `CancelSchedule`; `SetRule(true)`/`RunRule(true)` take Full (home's glue asks Full for them, the registry's
  `scope` doesn't know), `SaveRule(true)` takes Full through the registry, `DeleteRule` Full, the rest Control.
- **Values, verbatim:** `HOME_KEYS` = 1 << 62; `MOST_RULES` 500; `RULE_TEXT_CHARS` 65,536; `MOST_SCHEDULES` 200;
  `SCHEDULE_TEXT_CHARS` 300; `TASK_CHARS` 2,000; `AGENT_SCREEN_AGAIN` 2 s; `AGE_MOST` 7 days; `SCREEN_LINES` 12
  (exists); `MOST_REQUESTS` 50 (exists).
- **Words, verbatim** (winx-desktop is `computer_of`, API the remote workspace's name):
  - The stale rule's run, and a local Save whose Where is a remote workspace: "Target missing: API is a mirror of
    winx-desktop: its automations run there." (the Save's red line drops "Target missing: ").
  - The Automations popup: a group's header "On winx-desktop · API"; under its rules, dim, "Rules for all of
    winx-desktop's workspaces aren't shown here."; under a home rule's editor foot "Saved from another computer, it
    turns off when this share ends. Save it on winx-desktop to keep it."; the fixed Where of a home rule "API on
    winx-desktop"; a home schedule's row "s3 from Claude Code (b7k2m) to k9x1d" (`to` is "itself" when it equals
    `by`), "s3 to k9x1d" when home names no `by`; its second line as the local rows' ("every 10 min · next 15:10 ·
    Check the deploy", "once · 15:10 · …").
  - The Agents popup: a remote group's header "API · winx-desktop" (with the existing " · 1 needs you"); a task handed
    through the API says "from the API".
  - The usage chip's tooltip of a remote terminal ends with a line "Effort: high".
  - The inbox: "And 3 more on winx-desktop." (one: "And 1 more on winx-desktop.").
  - Home: `cancel_schedule` answers "Cancelled s3."; one it doesn't have (or the token doesn't see) is "There's no
    schedule s3: GET /v1/state lists them."; Notify me's notice `text` is "{title}: {text}", `title` the rule's
    name, else its sentence.
  - The mirror's Edit… row "The share can": Full "Full: also type and click into terminals, discard Git changes,
    delete branches, stashes and automations"; Control "Control: watch and work with blocks, cards, checklists, Git
    and automations, open agents and hand them tasks, answer requests". Home's Share… invitation (`INVITE`): "Let
    another Templum mirror this workspace: watch it live and work with its blocks, cards, checklists, Git,
    automations and agents from there."
- **Batch 1's and 2a–2g's words and tests stay;** the only existing tests this plan rewrites are named in their task
  (the `Notice`, `AgentView` and `AgentRow` literals gain their new fields; the registry's own-tool and target counts).
- **Every logic change has a test that fails without it;** UI-only changes say how they were verified (the groups, the
  home editor, the Agents popup's remote details and the inbox line were not looked at on a display unless the
  implementer did: then a minors entry). Headless GPUI tests; keys in `keyring_core::mock::Store`; home on loopback in
  the same process; conditions with a 10 s deadline, never a fixed sleep. CI runs Linux, macOS and Windows with no
  agent CLI (fake agents as `delegate`'s tests do).
- **Every feature in the API:** each new state field and tool ships its lines in `docs/api.md` and
  `docs/specs/api.md` in the task that adds it. The mirror's acts add nothing to the mirror's own API.
- **Minors** go to `docs/minors.md` (`## Templum to Templum`); these leave it in the task that fixes them:
  "Automations default to, and offer, a remote workspace." (Task 3), "A remote terminal's effort has no place on its
  block." (Task 7), "A request whose block has no viewer on the mirror isn't listed there.", "Only the first 50
  requests of a mirrored workspace are listed.", "A mirrored request is aged from when the mirror heard of it." (Task
  8), "The Edit… row "The share can" and the Share… invitation keep batch 1's words." (Task 9). Docs lines ≤ 120
  columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **One process is both ends** (`mirror_end_to_end.rs`): home's rule 1 is the mirror's rule 1, home's `w1` and
   `abc12` are the mirror's own. A home key that slips into a local path runs the rule twice in a real two-computer
   setup while the one-process test still passes once, unless the test counts. → Task 5
   `a_home_key_never_reaches_the_local_engine`; Task 9
   `a_home_rule_switched_and_run_from_the_mirror_runs_at_home_once` counts home's notices of that rule (exactly 1).
2. **A hostile home's rules, schedules and agents** (5,000 rules; a 1 MB prompt with ESC and BEL; a `when` that
   doesn't parse; a rule id of 2⁴⁰; a cron expression of 10,000 characters; a uid with `\n`; 1,000 schedules of 100 KB;
   a task text of 1 MB; a `task.by` of 500 bytes; a screen of 10,000 lines): capped and cleaned before it is kept or
   drawn. → Task 4 `home_s_rules_are_cleaned_and_capped`, `home_s_schedules_are_cleaned_and_capped`,
   `home_s_task_and_role_are_cleaned_where_they_enter`; Task 7 `a_screen_read_keeps_its_last_twelve_clean_lines`.
3. **The remote drops, is forgotten or comes back as another `r1`** while the popup shows home's rule or a draft of
   one: acts say "Not connected." and send nothing; a forgotten remote's drafts and selection go; a new remote that
   takes the id `r1` inherits no draft. → Task 5 `a_home_rule_s_acts_ask_the_rule_first`,
   `a_forgotten_remote_takes_its_rules_and_drafts_with_it`.
4. **A Control share on a rule that runs a command:** its switch, Run now, Save and Delete are dimmed with "Needs a
   Full share: share again on winx-desktop." and send nothing, though `registry::scope` calls `set_automation` and
   `run_automation` Control; home's 403 sentence still shows if they differ. → Task 4
   `a_command_rule_takes_full_to_switch_run_save_or_delete`; Task 5 `a_home_rule_s_acts_ask_the_rule_first`.
5. **The editor's answer arrives after the person moved on** (another rule selected, the popup closed, another
   workspace shown): a Save's success drops only that key's draft; a refusal writes the red line only while the form
   still shows that key; a new rule's "Saved automation 7." selects home's 7 only if the person is still on the new
   draft. → Task 6 `a_home_save_s_answer_lands_on_its_own_key`.

---

### Task 1: Home's state gains an agent's task and role, and the schedules

**Files:**
- Modify: `crates/templum-api/src/model.rs` (`TaskView`, `ScheduleView`; `AgentView.{task, role}`;
  `State.schedules`; the event `schedule` in `keeps_items`, `parse`, `diff` (after `automation`), `from_snapshot`'s
  crowded list; `State::visible`)
- Modify: `crates/templum-api/src/openapi.rs` (`TaskView`, `ScheduleView` and `AgentView`'s `task`, `role` and
  `State`'s `schedules` in `schemas_with_views`; `full_state`)
- Modify: `crates/templum/src/workspace/api_state.rs` (`agent_view` fills `task` and `role`; `schedule_views`)
- Test: `crates/templum-api/src/model.rs` tests, `crates/templum-api/src/openapi.rs` tests,
  `crates/templum/src/workspace/tests/api.rs`
- Docs: `docs/api.md` (Resources' `GET /v1/state` list gains `schedules`; Events' names gain `schedule` (`s3`); a
  paragraph **Agents and schedules.**), `docs/specs/api.md` (the agent's fields, the collection)

**Interfaces:**
- Produces (`templum_api::model`, each `Clone, Debug, Default, PartialEq, Serialize, Deserialize`):

```rust
/// The task an agent works on: its id (`t7`), its text as home keeps it (2,000 characters), the block that
/// handed it (`api` for the API; None for a block the token doesn't see, an automation or the Templum Agent).
pub struct TaskView { pub id: String, pub text: String, pub by: Option<String>, pub since: i64 }
/// A delivery an agent asked for later (`schedule`): `by` the asking block (None as above), `to` the block it
/// goes to, `every` in seconds (None: once), `next` and `until` Unix seconds.
pub struct ScheduleView {
    pub id: String, pub by: Option<String>, pub to: String, pub text: String,
    pub next: i64, pub every: Option<u64>, pub until: i64,
}
```

  `AgentView` gains `#[serde(default)] pub task: Option<TaskView>` and `#[serde(default)] pub role: Option<String>`
  (null when none, as `model` is). `State` gains `#[serde(default)] pub schedules: Vec<Arc<ScheduleView>>`.
- `State::visible` for a limited caller: `schedules` whose `to` is a visible block, each with `by` None when `by`
  isn't a visible block (`Arc::new` of a changed clone, else the same `Arc`); a visible block whose
  `agent.task.by` is neither a visible block nor `crate::ASKER` gets `by` None the same way.
- `Workspace::agent_view` (home): `task: pane.handed.as_ref().filter(|h| !h.text.is_empty()).map(|h| TaskView { id:
  format!("t{}", h.id), text: h.text.clone(), by, since: unix_of(h.since, now, wall) })`, `by` = `"api"` when
  `h.by == templum_api::ASKER`, else the uid of `self.caller(&h.by)`'s block, else None; `role: pane.role.clone()`.
- `Workspace::schedule_views(&self) -> Vec<ScheduleView>` (home, `self.schedules` in order): `id: format!("s{}",
  s.id)`, `by` the uid of `self.caller(&s.by)`'s block (None for the Templum Agent's), `to: s.to`, `text: s.text`,
  `next`/`until` through `automations::unix`, `every: s.every.map(|e| e.as_secs())`. `state_of` sets `schedules:
  arcs(self.schedule_views())` (also in `api_scope`: a rule over a schedule reads it).

- [ ] **Step 1: Write the failing tests.** `model.rs`:

```rust
#[test]
fn a_limited_caller_sees_the_schedules_and_tasks_of_its_blocks_only() {
    // Blocks b1, b2 (w1) and c1 (w2). b1's agent has task { id "t3", by Some("c1") }, b2's task { by Some("api") }.
    // Schedules s1 { by Some("b1"), to "b2" }, s2 { by Some("c1"), to "b1" }, s3 { by Some("b1"), to "c1" }.
    // Limited to w1: schedules ids ["s1", "s2"], s2's by None, s1's by Some("b1") and the same Arc as the full
    // state's (Arc::ptr_eq); b1's task.by None, b2's Some("api"). Unlimited: all three, untouched.
}

#[test]
fn schedule_events_put_and_delete_and_old_homes_have_no_task() {
    // apply("schedule", put "s1") adds it; an equal put keeps the Arc and answers false; delete "s1" leaves none;
    // diff of two states that differ by a schedule is one Delta { kind: "schedule", id: "s1", .. }. A snapshot with
    // 10_001 schedules is Err(TooLarge). An AgentView read from JSON without "task" and "role" has both None.
}
```

`openapi.rs`: `full_state` gets a schedule and an agent with a task and a role; `the_state_schema_matches_the_state`
also checks an agent whose `task` and `role` are null.

`tests/api.rs`:

```rust
#[gpui_kit::test]
fn the_state_shows_an_agent_s_task_and_role_and_the_schedules(cx: &mut TestAppContext) {
    // agents_running_in (mine, theirs): theirs' pane.role = Some("reviewer"); mine delegates "Ship it." to theirs
    // (call_later from mine, as delegate's tests do) → state(read token): theirs' block's agent.task ==
    // TaskView { id: "t1", text: "Ship it.", by: Some(mine's uid), since: within 5 s of now } and role
    // Some("reviewer"); mine's agent.task None. A schedule added with schedule_add { by: mine's terminal, to:
    // theirs' uid, every: 600 s } is schedules[0] == { id "s1", by Some(mine's uid), to theirs' uid, every
    // Some(600) }, next and until as automations::unix of its times.
}

#[gpui_kit::test]
fn a_task_handed_through_the_api_is_by_api(cx: &mut TestAppContext) {
    // A Control token's delegate to theirs (tool "delegate") → its task.by == Some("api").
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum-api model::tests openapi::tests`, then `cargo test -j 4 -p
  templum workspace::tests::api::the_state_shows` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say. Every `AgentView { … }` literal in the tree gains `task: None,
  role: None` or `..Default::default()` (13 today).
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum-api client` → PASS.
- [ ] **Step 5: Docs.** `docs/api.md`, **Agents and schedules.** "An agent's block has `agent.task`, the task it
  works on: `{"id":"t7","text":"…","by":"b7k2m","since":1761560000}` (`text` at most 2,000 characters; `by` the block
  that handed it, `"api"` for the API, null for one the token doesn't see), and `agent.role`, the role it was opened
  with, or null. `schedules` holds the deliveries agents asked for (`schedule`):
  `{"id":"s3","by":"b7k2m","to":"k9x1d","text":"Check the deploy","next":1761562000,"every":600,"until":1762166800}`
  (`every` in seconds, null for once); a workspace-limited token sees those whose `to` block it sees, `by` null when it
  doesn't see that one." `docs/specs/api.md`: the same, as bullets.

---

### Task 2: Home's `cancel_schedule`, the notice of Notify me, a new rule saved off, `read_terminal` grouped

**Files:**
- Modify: `crates/templum-api/src/registry.rs` (`OWN` gains `"cancel_schedule"`: 10; `TARGETS` gains `"schedule"`: 12;
  its `tool(…)` entry; `Own::CancelSchedule { schedule: String }` in `parse_own`; `not_found`'s path for `schedule` is
  `/v1/state`; `scope` gives it Control by the own tools' default)
- Modify: `crates/templum-api/src/model.rs` (`Notice.automation`), `crates/templum-api/src/client/mod.rs` (`notice_of`
  keeps `automation`), `crates/templum-api/src/audit.rs` (`CHATTER` gains `"read_terminal"`: 5)
- Modify: `crates/templum/src/workspace/api.rs` (`api_allows`'s `"schedule"` target; `api_own`'s `CancelSchedule`),
  `crates/templum/src/workspace/automations.rs` (`try_do`'s `Do::Notify` also sends a notice; `save_rule` keeps a new
  rule's `on`)
- Test: `crates/templum-api/src/registry.rs` tests, `crates/templum-api/src/audit.rs` tests,
  `crates/templum-api/src/client/mod.rs` tests, `crates/templum/src/workspace/tests/{api.rs,automations.rs}`
- Docs: `docs/api.md` (the own tools' table: "with the ten only the API has", a `cancel_schedule` row; Events'
  `notice` sentence; The audit log: `read_terminal` grouped with `terminal_input`; `save_automation`'s row: "`on`
  (default true) for a new one"), `docs/specs/api.md`, `docs/specs/automations.md` (Notify me also sends a notice
  through the API)

**Interfaces:**
- Produces: `Notice { text, block, workspace, #[serde(default, skip_serializing_if = "Option::is_none")] automation:
  Option<u64> }` (every literal gains `automation: None` or `..Default::default()`). `client::notice_of` copies
  `automation` as it is.
- `registry::tools()` gains `tool("cancel_schedule", "Cancels a delivery an agent asked for later, as the Automations
  popup's Cancel does.", json!({ "schedule": { "type": "string", "description": "A schedule's id, as the state shows it
  (s3)." } }), &["schedule"])`. `Own::CancelSchedule { schedule }`: `schedule` a string, else
  "cancel_schedule needs \"schedule\", a string".
- `api_allows`, key `"schedule"`: the value read as `s<n>` (a leading `s` optional) must be the id of one of
  `seen.schedules`, else `not_found("schedule", "s<n>")`.
- `api_own`, `Own::CancelSchedule { schedule }`: `n` from `schedule.trim().trim_start_matches('s')`; a schedule
  `n` in `self.schedules` → `cancel_schedule_now(n, cx)`, `Reply::ok("Cancelled s{n}.")`; else `Err(not_found(
  "schedule", &format!("s{n}")))`.
- `try_do`, `Do::Notify`: after the notification, `self.api_notice(Notice { text: format!("{title}: {text}"),
  workspace: a.workspace.clone(), block: None, automation: Some(a.id) })` (`title` as the notification's). The hub
  already sends a notice only to streams that see its workspace (a rule for all workspaces: unlimited tokens).
- `save_rule`, `id == NEW`: `Automation { since: unix(now), ..draft }` (the draft's own `on`; the editor's New draft is
  on, so the editor is unchanged); `api_own`'s `SaveAutomation` keeps `rule.on` as parsed (serde's default is on).

- [ ] **Step 1: Write the failing tests.** `registry.rs`: `cancel_schedule` parses and is `Scope::Control`; without
  `schedule` it is the shape sentence; `own("cancel_schedule")`; `every_argument_is_a_target_or_not` passes with
  `"schedule"` in `TARGETS`. `audit.rs`: `read_terminal_calls_are_grouped_per_token_and_minute` (three
  `read_terminal` calls of one token in a window are one line with `more` 2, as the `terminal_input` test). `client`:
  `a_notice_keeps_its_automation` (`notice_of(r#"{"text":"a\nb","workspace":"w3","automation":4}"#)` →
  `Notice { text: "a", workspace: Some("w3"), automation: Some(4), block: None }`).

`tests/api.rs`:

```rust
#[gpui_kit::test]
fn cancel_schedule_cancels_a_schedule_the_token_sees(cx: &mut TestAppContext) {
    // agents_running_in; schedule_add s1 { to: theirs' uid } and s2 { to: a block of a second workspace }.
    // A Control token limited to the first workspace: cancel_schedule {"schedule": "s1"} → "Cancelled s1.", and
    // ws.schedules has only s2; {"schedule": "2"} → 404 "There's no schedule s2: GET /v1/state lists them.", s2
    // kept. A Read token → 403 "cancel_schedule needs a Control token.".
}
```

`tests/automations.rs`:

```rust
#[gpui_kit::test]
fn notify_me_sends_its_notice_to_the_api(cx: &mut TestAppContext) {
    // add(Automation { name: "Morning review", workspace: Some(first), ..notify("Look") }); due; look →
    // last == (Ran, "Notified."), and ws.api.notices.last() == Notice { text: "Morning review: Look",
    // workspace: Some(first), block: None, automation: Some(id) }. An unnamed one's text starts with its sentence.
}

#[gpui_kit::test]
fn a_new_rule_saved_off_through_the_api_stays_off(cx: &mut TestAppContext) {
    // save_automation through a Full token with {"automation": {"name": "Copy", "on": false, "workspace": first,
    // "when": {"daily": {"at": "09:00:00"}}, "do": {"notify": {"text": "x"}}}} → the rule is off and its run's next
    // is None; the same without "on" is on.
}
```

- [ ] **Step 2: Run them** (`cargo test -j 4 -p templum-api registry audit client`; `cargo test -j 4 -p templum
  workspace::tests::api::cancel_schedule workspace::tests::automations::notify_me
  workspace::tests::automations::a_new_rule_saved_off`) → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say.
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum-api` (the registry's counts) and `cargo test -j 4
  -p templum workspace::tests::automations` (the editor's New and Save unchanged) → PASS.
- [ ] **Step 5: Docs** as the Files say: the `cancel_schedule` row "`schedule` (`s3`) | Cancels it, as the
  Automations popup's Cancel does | control"; the notice sentence "…a watch set through the API names the watched
  block and its workspace, and an automation's Notify me sends `{"text":"Morning review: Look","workspace":"w3",
  "automation":1}` to the tokens that see its workspace (one for all workspaces: tokens for all workspaces)."

---

### Task 3: The rule on the mirror's own engine

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Space::automations_there`)
- Modify: `crates/templum/src/workspace/automations.rs` (`automation_target`, `automation_problem`,
  `new_automation`)
- Modify: `crates/templum/src/workspace/agents_view.rs` (`watch_agents`: heard only from a local space)
- Modify: `crates/templum/src/automations_view/editor.rs` (`blocks` skips remote spaces; the Where choices without
  remote workspaces, as `pub(crate) fn where_choices`)
- Create: `crates/templum/src/workspace/tests/remote_rules.rs` (and `mod remote_rules;` in `tests/mod.rs`)
- Docs: `docs/specs/automations.md` (a section "On a computer that mirrors another": the rule, a stale rule's
  words, Where), `docs/minors.md` ("Automations default to, and offer, a remote workspace." leaves it)

**Interfaces:**
- Produces: `Space::automations_there(&self) -> Option<String>`: "API is a mirror of winx-desktop: its automations run
  there." (`clean_title` of the name, `mirror_of`), None for a local space.
- `automation_target`: a rule whose `workspace` is a remote space → `missing(automations_there)` (before the stopped
  check), so every run (a time, an event, Run now, a time missed at start that then falls due) is Skipped with
  "Target missing: API is a mirror of winx-desktop: its automations run there." and its Do never runs.
- `automation_problem`: a rule whose `workspace` is a remote space → `Some(automations_there)` (Save refuses it, in the
  editor and through this computer's own `save_automation`, which shares `save_rule`).
- `new_automation`: `workspace` is the shown workspace when it is local, else the first local one in `order()`, else
  None.
- `watch_agents`: a `Heard` is pushed only for a pane of a space that `!is_remote()` (explicit; today it holds only
  because a remote block has no uid).
- `pub(crate) fn where_choices(ws: &Workspace, a: &Automation) -> Vec<(SharedString, SharedString)>`: today's list
  (`order()`, "All workspaces" unless it opens something) without remote spaces; a current value that is a remote
  space is kept with its `mirror_title()` ("API (mirror of winx-desktop)"), a missing one as "A deleted workspace".
  `blocks` (now `pub(crate)`) skips `space.is_remote()`.

- [ ] **Step 1: Write the failing tests** in `tests/remote_rules.rs` (each proves a way a rule could fire on the
  mirror runs nothing there):

```rust
#[gpui_kit::test]
fn a_home_rule_whose_time_came_runs_nothing_on_the_mirror(cx: &mut TestAppContext) {
    // offline + show_remote + live(Full) + entry.home "w1". apply a terminal_of_home state whose automations hold
    // AutomationView { id 1, on, workspace "w1", when {"daily":…}, do {"notify":{"text":"x"}}, next: now - 60 } and
    // an event rule { when {"agent":{"event":"idle","who":{"kind":"claude"}}} }. Advance AUTOMATION_LOOK ×3:
    // ws.automations.automations is empty, automation_runs is empty, automating false, automations_listen() false,
    // automations_unseen() 0, and dir has no automations.json.
}

#[gpui_kit::test]
fn a_local_event_rule_never_hears_a_remote_agent_or_a_notice(cx: &mut TestAppContext) {
    // offline mirror; a local rule (workspace None) When::Agent { Idle, Kind "claude" } → notify; one more for
    // NeedsYou. apply terminal_of_home(agent "claude" working), frame (set_active true), apply it idle, frame,
    // apply it needs_you, frame; hear(notice("[task t1 done] Green.", Some("abcde"))). look ×2: both rules' last
    // None, automation_heard empty, though automations_listen() is true.
}

#[gpui_kit::test]
fn a_stale_rule_for_a_remote_workspace_is_skipped_at_start_at_its_time_and_by_run_now(cx: &mut TestAppContext) {
    // A mirror whose automations.json (written into its data folder before it opens, as with_remote writes the
    // index) holds a Daily 00:00 notify rule for "r1" counted from two days ago, and a Command rule for "r1" with a
    // folder. At start: the first is Missed "Templum was closed." (nothing ran). apply terminal_of_home (name "API");
    // due(first); look → (Skipped, "Target missing: API is a mirror of 127.0.0.1:9: its automations run there.");
    // run_automation_now(second) → Skipped with the same words and no terminal opened anywhere (every space's block
    // count unchanged).
}

#[gpui_kit::test]
fn the_mirror_s_own_rules_never_list_target_or_default_to_a_remote_workspace(cx: &mut TestAppContext) {
    // offline, r1 shown (show_remote), terminals_of_home(["abcde"]) applied. new_automation → draft NEW's workspace
    // is the first local workspace's id. where_choices of it holds no "r1". A rule with workspace None and
    // Do::Command { terminal: Some("") }: blocks(ws, a, false) lists no remote viewer (none of r1's ids).
    // A draft with workspace Some("r1") → save_automation(NEW) is Err("API is a mirror of 127.0.0.1:9: its
    // automations run there."), and where_choices keeps "r1" as "API (mirror of 127.0.0.1:9)".
}

#[gpui_kit::test]
fn home_s_schedules_are_never_delivered_on_the_mirror(cx: &mut TestAppContext) {
    // offline; apply a state with ScheduleView { id "s1", by Some("abcde"), to "abcde", next: now - 60 }; after
    // SCHEDULE_LOOK ×2: ws.schedules is empty and scheduling false.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::remote_rules` → the stale, the default and
  the Save cases FAIL (the others may pass today by accident; they stay as the rule's proof).
- [ ] **Step 3: Implement** as the Interfaces say.
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum workspace::tests::automations
  workspace::tests::agents_view` → PASS.
- [ ] **Step 5: Docs** as the Files say; the minor leaves `docs/minors.md`.

---

### Task 4: The mirror's copies of home's rules and schedules, the deeds, a task and a role cleaned

**Files:**
- Create: `crates/templum/src/workspace/remote_rules.rs` (`mod remote_rules;` in `workspace/mod.rs`)
- Modify: `crates/templum/src/workspace/remote.rs` (`RemoteSpace.{rules, schedules}`; `apply_remote` calls
  `mirror_rules` when `automations` or `workspaces` changed and `mirror_schedules` when `schedules` changed;
  `tidy_block` cleans `agent.task` and `agent.role`; the new `Deed`s, their `call`s and Full in `why_not`)
- Test: `remote_rules.rs`'s own `mod tests`, `crates/templum/src/workspace/remote.rs` tests
  (`every_deed_home_runs_has_a_scope_in_the_registry` covers the new deeds), `tests/remote_rules.rs`

**Interfaces:**
- Produces (`workspace/remote_rules.rs`):

```rust
/// The Automations popup's keys of home's rules: HOME_KEYS | n << 32 | home's id, for the remote `r<n>`; id 0 is
/// that remote's new rule. Local ids count up from 1 and never reach it.
pub(crate) const HOME_KEYS: u64 = 1 << 62;
pub(crate) fn home_key(remote: &str, id: u64) -> Option<u64>;   // None: not `r<n>` with 1 ≤ n < 2^29, or id ≥ 2^32
pub(crate) fn key_home(key: u64) -> Option<(String, u64)>;      // None below HOME_KEYS
/// A rule of home's for the remote's workspace, as its state said it, cleaned.
pub(crate) struct HomeRule {
    /// Home's id.
    pub id: u64,
    /// The rule as the popup draws and edits it: its `id` is its popup key, `workspace` home's.
    pub rule: Automation,
    /// Home's next run, Unix seconds.
    pub next: Option<i64>,
    pub sentence: String,
    /// Home's own item, sent back unchanged by Duplicate (its `when` and `do`).
    pub raw: Arc<AutomationView>,
}
pub(crate) struct HomeSchedule {
    pub id: String, pub by: Option<String>, pub to: String, pub text: String,
    pub next: i64, pub every: Option<u64>,
}
pub(crate) const MOST_RULES: usize = 500;
pub(crate) const RULE_TEXT_CHARS: usize = 64 * 1024;
pub(crate) const MOST_SCHEDULES: usize = 200;
pub(crate) const SCHEDULE_TEXT_CHARS: usize = 300;
/// Home's rule `view`, cleaned, as the popup's `key`: None for one that doesn't parse or names a bad id.
pub(crate) fn rule_of(view: &AutomationView, key: u64) -> Option<Automation>;
impl RemoteSpace {
    /// Home's rules whose workspace is `home` (the share's), in home's order, at most MOST_RULES; a rule whose
    /// `Arc` didn't change is moved over as it was.
    pub(super) fn mirror_rules(&mut self, remote: &str, state: &State, home: Option<&str>);
    /// Home's schedules, at most MOST_SCHEDULES, in home's order.
    pub(super) fn mirror_schedules(&mut self, state: &State);
}
```

  `rule_of`: `when`/`do`/`last` through `serde_json::from_value` into `When`/`Do`/`LastRun`, `cooldown` default
  `COOLDOWN`, `runs`, `max_runs`, `since` 0, `opened` None, cleaned per Global Constraints. `RemoteSpace` gains
  `pub rules: Vec<HomeRule>` and `pub schedules: Vec<HomeSchedule>` (empty in `new`).
- `tidy_block` (each agent of home's): `task` cleaned per Global Constraints (`TASK_CHARS` = 2,000 in `remote.rs`),
  `role` `clean_title`.
- `Deed` gains `SetRule(bool)`, `RunRule(bool)`, `SaveRule(bool)`, `DeleteRule`, `CancelSchedule`; `call`:
  `("set_automation", {"automation": 1, "on": true})`, `("run_automation", {"automation": 1})`, `("save_automation",
  {"automation": {"do": {"command": {"text": "t"}}}})` for `SaveRule(true)` and `{"do": {"notify": {"text": "t"}}}`
  for `false`, `("delete_automation", {"automation": 1})`, `("cancel_schedule", {"schedule": "s1"})`; `why_not`'s
  `takes` is `Scope::Full` for `SetRule(true)` and `RunRule(true)` (as `Answer` raises it).

- [ ] **Step 1: Write the failing tests.** In `remote_rules.rs`:

```rust
#[test]
fn keys_of_home_s_rules_never_meet_a_local_id() {
    // home_key("r1", 7) == Some(HOME_KEYS | 1 << 32 | 7); key_home(that) == Some(("r1".into(), 7));
    // home_key("r3", 0) is r3's new rule; key_home(41) == None; home_key("r1", 1 << 32) == None;
    // home_key("w3", 1) == None; home_key("r0", 1) == None; every key ≥ HOME_KEYS.
}

#[test]
fn home_s_rules_are_cleaned_and_capped() {
    // A state of 5_000 views for "w1" and 3 for "w2": mirror_rules keeps MOST_RULES, all "w1", in home's order. A
    // view with a 1 MB prompt with ESC and BEL: its prompt has no control character but \n and \t and is at most
    // RULE_TEXT_CHARS; a name of 200 characters is ≤ NAME_MAX; a cron expr of 10_000 is ≤ LINE_CHARS; a view whose
    // "when" is {"spiral":{}} is left out; id 1 << 40 is left out; a Who of 300 blocks keeps 100; a uid "ab\ncd"
    // leaves the rule out. A second mirror_rules with the same Arcs keeps each HomeRule (its rule unchanged, raw
    // Arc::ptr_eq).
}

#[test]
fn home_s_schedules_are_cleaned_and_capped() {
    // 1_000 schedules: MOST_SCHEDULES kept; a 100 KB text with \u{1b} is its first line, ≤ 300, clean; an id
    // "s1\n" or a to of 500 bytes is left out; a by of 500 bytes is None.
}
```

In `remote.rs` tests: `a_command_rule_takes_full_to_switch_run_save_or_delete` (`remote_at(.., Status::Live)` with a
Control `me`: `why_not(SetRule(true))`, `RunRule(true)`, `SaveRule(true)`, `DeleteRule` are `Some(Nay::NeedsFull)`;
`SetRule(false)`, `RunRule(false)`, `SaveRule(false)`, `CancelSchedule` are None; tools without `cancel_schedule` make
`CancelSchedule` `Older`). In `tests/remote_rules.rs`:

```rust
#[gpui_kit::test]
fn home_s_task_and_role_are_cleaned_where_they_enter(cx: &mut TestAppContext) {
    // apply terminal_of_home(agent with task { id "t3", text: 1 MB of "a\x07", by Some(500 bytes) } and role
    // Some("re\nviewer")): the viewer's RemotePane.agent.task.text ≤ 2_000 characters with no BEL, by None,
    // role "re viewer" (clean_title). A task with id "t3\n" is None.
}

#[gpui_kit::test]
fn home_s_rules_and_schedules_are_copied_into_the_remote_never_into_the_mirror_s_own(cx: &mut TestAppContext) {
    // apply a state with two rules for "w1", one for "w2", and a schedule: remote_of(ws).rules ids [1, 2],
    // rules[0].rule.id == home_key("r1", 1), rules[0].rule.workspace == Some("w1"), schedules.len() == 1;
    // ws.automations and ws.schedules empty. A state without them empties both copies.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum remote_rules` and `cargo test -j 4 -p templum
  workspace::remote::tests` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say.
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum workspace::tests::remote` → PASS.

---

### Task 5: The Automations popup lists home's rules and schedules, and acts on them at home

**Files:**
- Modify: `crates/templum/src/workspace/remote_rules.rs` (the routing and the acts below)
- Modify: `crates/templum/src/workspace/automations.rs` (`automation_ids`, `automation_draft`, `edit_automation`,
  `set_automation_on`, `run_automation_now`, `duplicate_automation`, `ask_delete_automation`, `delete_automation`,
  `next_words`, `last_words`, `automations_unseen`, `open_automations`, `when_said`, `automation_sentence`,
  `automation_target` route a home key; `Automations::duplicate`'s name rule as `pub(crate) fn copy_name`)
- Modify: `crates/templum/src/automations/mod.rs` (`copy_name`)
- Modify: `crates/templum/src/automations_view/mod.rs` (the groups, home rows, home schedules with Cancel)
- Modify: `crates/templum/src/workspace/remote.rs` (`remote_notice`: a notice with `automation` opens the popup at its
  rule on a click), `crates/templum/src/workspace/spaces.rs` (`delete_space_as` of a remote drops its drafts and
  selection), `crates/templum/src/workspace/mod.rs` (`Workspace::automations_seen: i64`, set to the start's Unix
  seconds)
- Test: `crates/templum/src/workspace/tests/remote_rules.rs`

**Interfaces:**
- Produces (`impl Workspace`, in `remote_rules.rs`):

```rust
/// The saved rule of `key`: this computer's, or the copy of home's.
pub(crate) fn rule(&self, key: u64) -> Option<&Automation>;
/// The remote space and the copy of home's rule `key`.
pub(crate) fn home_rule(&self, key: u64) -> Option<(&Space, &HomeRule)>;
/// One group of the list per remote workspace, in the sidebar's order.
pub(crate) fn rule_groups(&self) -> Vec<RuleGroup>;
pub(crate) struct RuleGroup {
    pub space: String,
    /// "On winx-desktop · API".
    pub title: String,
    /// Home's rules, then the new one while it is a draft.
    pub keys: Vec<u64>,
    /// "Rules for all of winx-desktop's workspaces aren't shown here."
    pub foot: String,
    /// The remote isn't live.
    pub dim: bool,
    /// Home's schedules: (id, first line, second line), as `schedule_rows` says them.
    pub schedules: Vec<(String, String, String)>,
}
/// The group's +: a new rule of home's (a draft keyed `home_key(space, 0)`), its Where home's workspace, on, daily
/// 09:00, Notify me, selected.
pub(crate) fn new_home_automation(&mut self, space: &str, cx: &mut Context<Self>);
/// Cancel of a home schedule: `cancel_schedule` at home.
pub(crate) fn cancel_home_schedule(&mut self, space: &str, schedule: &str, window: &mut Window, cx: &mut Context<Self>);
/// How many of home's rules went other than Ran after `automations_seen`.
pub(crate) fn home_unseen(&self) -> usize;
```

- The routing, each at the top of the function, through `key_home`/`home_rule` (a local key keeps today's code):
  `automation_ids` = today's ids, then each `rule_groups()`'s `keys`; `automation_draft(key)` falls back to
  `self.rule(key)`; `edit_automation` compares with `self.rule(id)`; `next_words` of a home rule: "Off" when off, an
  event's `event_short` with home's kinds' titles, else `time_words` of `next`, else "Never"; `last_words` and
  `automation_last_line` read `self.rule(id)` (a home key has no `automation_runs`); `automation_target` answers
  `Ok(())` for a home key (home's `last.about` says why a run was skipped); `when_said` and `automation_sentence` of
  a home key name blocks with `uid_title_in(space, uid, cx)`, kinds with `RemoteSpace::agents`' titles (else the id),
  the workspace with the remote space's name; a saved home rule with no draft shows home's `sentence`.
- The acts on a home key: `set_automation_on` → `SetRule(command)`, `set_automation` `{automation: home id, on}`;
  `run_automation_now` → `RunRule(command)`, `run_automation`; `duplicate_automation` → `SaveRule(command)`,
  `save_automation` `{automation: {name: copy_name(raw.name), on: false, workspace, when: raw.when, do: raw.act,
  max_runs, cooldown}}`, its answer's "Saved automation N." selects `home_key(space, N)`; `ask_delete_automation`
  asks as locally, then `delete_automation` → `DeleteRule`, `delete_automation`, its draft dropped on success.
  `command` is `matches!(rule.act, Do::Command { .. })`. Each asks `why_not_in(space, deed)` first: refused → `warn`
  its words, nothing sent; home's refusal → `warn` its sentence. None touches `self.automations`,
  `automation_runs` or `look_at_automations`.
- `open_automations` also sets `automations_seen` to now (Unix seconds); the sidebar badge counts
  `automations_unseen() + home_unseen()`.
- `remote_notice`: a notice with `automation: Some(id)` opens, on its click, `show_automation(home_key(space, id))`.
- The list (`automations_view/mod.rs`): after this computer's rows and "Asked by agents", one section per
  `RuleGroup`: its title (`section`) with a + (`automation-new-{space}`, dimmed with the rule's words when
  `why_not_in(space, SaveRule(false))`), its rows (`automation_row` with the key: a home row's switch, Run now,
  Duplicate and Delete dimmed by their deeds' words as tooltips), its foot, dim; its schedules as the local ones,
  Cancel (`schedule-cancel-{space}-{id}`) dimmed by `CancelSchedule`'s words. A `dim` group is drawn at 50%
  opacity. A home row never shows the yellow "Target missing" line.

- [ ] **Step 1: Write the failing tests** (`tests/remote_rules.rs`, a shown live Full mirror with home's state
  applied, `sent`/`answer`/`refusal` for the calls):

```rust
#[gpui_kit::test]
fn home_s_rules_are_listed_in_their_group_after_the_mirror_s_own(cx: &mut TestAppContext) {
    // One local rule and home's rules 1, 2 for "w1": automation_ids() == [local, key1, key2]; rule_groups() is one
    // group { space "r1", title "On 127.0.0.1:9 · API", keys [key1, key2], foot "Rules for all of 127.0.0.1:9's
    // workspaces aren't shown here." }; next_words(key1) is time_words of home's next; a home rule last Failed at
    // now + 10 makes home_unseen() 1, open_automations makes it 0.
}

#[gpui_kit::test]
fn a_home_key_never_reaches_the_local_engine(cx: &mut TestAppContext) {
    // A local rule with id 1 and home's rule 1: set_automation_on(key1, false), run_automation_now(key1),
    // duplicate_automation(key1), delete_automation(key1) → sent() is [set_automation {automation 1, on false},
    // run_automation {automation 1}, save_automation {automation {name "… (copy)", on false, when: home's raw when,
    // do: home's raw do}}, delete_automation {automation 1}] (delete through ask_delete's confirm in the test:
    // call delete_automation directly); the local rule 1 is unchanged (on, no last), automation_runs has no key
    // ≥ HOME_KEYS, automations.json unchanged.
}

#[gpui_kit::test]
fn a_home_rule_s_acts_ask_the_rule_first(cx: &mut TestAppContext) {
    // Control share: a command rule's set/run/duplicate/delete send nothing; said() ends with "Needs a Full share:
    // share again on 127.0.0.1:9."; a notify rule's switch is sent. Offline (status Reconnecting): nothing is sent,
    // "Not connected.". A refusal answered to the switch → said ends with its sentence.
}

#[gpui_kit::test]
fn a_home_schedule_is_listed_and_cancelled_at_home(cx: &mut TestAppContext) {
    // State: blocks b7k2m "Claude Code" and k9x1d; schedule s3 { by b7k2m, to k9x1d, every 600 }. rule_groups()[0]
    // .schedules == [("s3", "s3 from Claude Code (b7k2m) to k9x1d", "every 10 min · next HH:MM · Check the deploy")];
    // cancel_home_schedule("r1", "s3") → sent cancel_schedule {"schedule": "s3"}; ws.schedules stays empty.
}

#[gpui_kit::test]
fn a_forgotten_remote_takes_its_rules_and_drafts_with_it(cx: &mut TestAppContext) {
    // A draft of key1 (edit_automation) selected; forget_remote("r1") → automation_drafts has no key ≥ HOME_KEYS,
    // automation_selected() is a local id or None, rule_groups() empty.
}

#[gpui_kit::test]
fn a_notice_of_a_home_rule_opens_the_popup_at_it_on_a_click(cx: &mut TestAppContext) {
    // hear(Notice { automation: Some(1), workspace "w1", text "Morning review: Look" }) → said ends "API on …:
    // Morning review: Look"; the notification's click (as the notices tests click one) opens the popup with
    // automation_selected() == home_key("r1", 1).
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::remote_rules` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say.
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum workspace::tests::automations` (no local test
  changed) → PASS.

---

### Task 6: The editor on a home rule, Save at home, and a new home rule

**Files:**
- Modify: `crates/templum/src/automations_view/editor.rs` (`blocks`, `kept_label`, the kinds and agents lists,
  `where_choices`, the sentence, next and last lines, the loop line, `save_form`, the foot line)
- Modify: `crates/templum/src/workspace/automations.rs` (`form_problem` out of `automation_problem`; `save_automation`
  of a home key; `automation_folder_shown` and `automation_loop` of a home key; `revert_automation` of a home key)
- Modify: `crates/templum/src/workspace/remote_rules.rs` (`save_home_rule`, `rule_json`, `home_blocks`, `home_kinds`)
- Test: `crates/templum/src/workspace/tests/remote_rules.rs`

**Interfaces:**
- Produces:

```rust
/// What stops `a` from being saved by the form's own rules (times, dates, required fields, the cooldown, a
/// workspace for what opens something), whatever computer it is for.
pub(crate) fn form_problem(&self, a: &Automation, now: NaiveDateTime) -> Option<String>;
/// Save of a home rule's draft: the form's rules, the rule's deed (`SaveRule(runs a command)`), then
/// `save_automation` at home (`id` home's, left out for a new one). Ok: sent. Err: why not (the red line).
pub(crate) fn save_home_rule(&mut self, key: u64, cx: &mut Context<Self>) -> Result<u64, String>;
/// The draft as automations.json keeps it: name, on, workspace, when, do, max_runs, cooldown.
pub(crate) fn rule_json(a: &Automation) -> serde_json::Value;
/// Home's terminals of the remote's workspace (agents only when `agents`), from its state: (uid, block_label).
pub(crate) fn home_blocks(&self, space: &str, agents: bool, cx: &App) -> Vec<(SharedString, SharedString)>;
/// Home's agents (`RemoteSpace::agents`): (id, title).
pub(crate) fn home_kinds(&self, space: &str) -> Vec<(SharedString, SharedString)>;
```

  `automation_problem` = `form_problem` + this computer's checks (the agent's launch arguments, a remote Where).
  `save_automation(key)` of a home key returns `save_home_rule(key)`; its answer, on the UI thread: Ok → that key's
  draft goes and the form reloads if it shows that key; a new rule's "Saved automation N." selects `home_key(space,
  N)` if `automations_at` is still the new key; Err(refusal) → `form.problem = Some(sentence)` only while
  `form.for_id == Some(key)`. `revert_automation` of a home key drops its draft (a new one is removed and the
  selection moves as locally).
- The editor on a home key (`key_home(id).is_some()`): Who's chips and Send a prompt's Agent from
  `home_blocks(space, true)`, A terminal and When a command ends' terminal from `home_blocks(space, false)`, Who's
  kinds and Start an agent's list from `home_kinds(space)`, a kept value `"{uid} (not open)"`; Where has the one
  choice (home's workspace, "API on winx-desktop"); the Folder's placeholder is home's workspace folder
  (`RemoteSpace::home.folder`, else blank); the loop line answers None for the two arms that read this computer's
  blocks or agents; Next is "Next: never, it is off" or `time_words` of home's `next` for a saved rule with no draft,
  else `preview_next`; the foot shows "Saved from another computer, it turns off when this share ends. Save it on
  winx-desktop to keep it." above Save, and Save is dimmed with `why_not_in(space, SaveRule(command))`'s words.

- [ ] **Step 1: Write the failing tests** (`tests/remote_rules.rs`):

```rust
#[gpui_kit::test]
fn a_home_rule_edited_on_the_mirror_is_saved_at_home(cx: &mut TestAppContext) {
    // Home's rule 1 (prompt to "b7k2m", "Go on.") selected; edit_automation(key1, name "Renamed"); save_automation
    // (key1) → sent save_automation {id 1, automation {name "Renamed", on, workspace "w1", when, do {prompt {block
    // "b7k2m", text "Go on."}}, max_runs, cooldown}}; answer Ok "Saved automation 1." → no draft of key1;
    // ws.automations unchanged.
}

#[gpui_kit::test]
fn a_new_home_rule_starts_on_home_s_workspace_and_lists_home_s_blocks(cx: &mut TestAppContext) {
    // terminals_of_home(["b7k2m" with agent "claude", "k9x1d"]), RemoteSpace::agents [claude "Claude Code"].
    // new_home_automation("r1") → draft key home_key("r1", 0), workspace Some("w1"), selected; where_choices ==
    // [("w1", "API on 127.0.0.1:9")]; blocks(ws, draft, true) == [("b7k2m", "b7k2m (b7k2m) · API")] (block_label),
    // with false both; home_kinds("r1") == [("claude", "Claude Code")]; no local block or Settings agent offered.
    // Save with a blank Notify text → Err("Write the text."), nothing sent; with text → sent without "id"; answer
    // "Saved automation 9." → automations_at == home_key("r1", 9).
}

#[gpui_kit::test]
fn a_command_rule_needs_a_full_share_to_be_saved_from_the_mirror(cx: &mut TestAppContext) {
    // Control share: a draft whose Do is a command → save_automation(key) Err("Needs a Full share: share again on
    // 127.0.0.1:9."), nothing sent.
}

#[gpui_kit::test]
fn a_home_save_s_answer_lands_on_its_own_key(cx: &mut TestAppContext) {
    // Draft of key1 saved (sent), then key2 selected; refusal("That automation is gone.") for the save → the
    // form's problem is None (it shows key2), key1's draft is kept; select key1, save again, refusal → problem is
    // the sentence.
}

#[gpui_kit::test]
fn a_rule_saved_and_run_from_the_mirror_is_home_s_alone(cx: &mut TestAppContext) {
    // Two workspaces over loopback (shown_mirror_as(Full)): on the mirror new_home_automation("r1"), Notify "Look",
    // name "From the mirror", save → pump_home until home's automations hold it (on, workspace home's) and the
    // mirror's copy lists it; the mirror's automations are empty and its data folder has no automations.json.
    // run_automation_now(its key) on the mirror → pump until home's last is (Ran, "Notified."); the mirror's
    // automation_runs stay empty and its said holds "API on …: From the mirror: Look" once.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::remote_rules` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say.
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum workspace::tests::automations` → PASS.
- [ ] **Step 5: Verify the look:** render the popup headlessly with home's group, a home rule's editor and a Control
  share (`window.render_frame`; `try_find` of `automation-new-r1`, `automation-row-{key1}`, `automation-save`):
  they draw. Not looked at on a display → a minors entry "The Automations popup's groups of home's rules and its editor
  on a home rule were checked headlessly only" (`automations_view/mod.rs`, `editor.rs`).

---

### Task 7: The Agents popup lists home's agents with their details and a Done of their own

**Files:**
- Modify: `crates/templum/src/workspace/agents_view.rs` (`doing`: home's `done` is idle; `AgentRow.dim`; `agent_rows`
  for a remote pane; the screen read and its timer)
- Modify: `crates/templum/src/workspace/inbox.rs` (`cards_of(boards)`; `cards_by_block` calls it)
- Modify: `crates/templum/src/agents_view.rs` (a dim group; the details of a remote agent)
- Modify: `crates/templum/src/workspace/remote.rs` (`RemoteSpace.{screen, screen_asking}`),
  `crates/templum/src/workspace/mod.rs` (`Workspace::screen_looking`), `crates/templum/src/usage.rs`
  (`with_effort`), `crates/templum/src/block.rs` (a remote chip's tooltip)
- Test: `crates/templum/src/workspace/tests/agents_view.rs` (a section "On a mirror"; `made` gains `dim: false`),
  `crates/templum/src/usage.rs` tests
- Docs: `docs/specs/agents-view.md` (a section "Agents of another computer"), `docs/minors.md` ("A remote terminal's
  effort has no place on its block." leaves it)

**Interfaces:**
- Produces: `AgentRow` gains `pub dim: bool`. `agent_rows`, a pane with `remote`: `space_name` "API · winx-desktop",
  `title` `remote.title`, `agent` home's title for `remote.agent.agent` (`RemoteSpace::agents`, else the id), `task`
  `remote.agent.task.text`, `card` `cards_of(&remote_space.boards).get(&remote.block)`, `folder` None, `dim`
  `remote_space.dim()`. `doing` of a remote pane maps home's `"done"` to `Doing::Idle` (this computer's watch makes
  its own Done, and the person looking here clears it as `watch_agents` does for a local one).
- `pub(crate) fn cards_of(boards: &Boards) -> HashMap<String, (u64, String)>` (today's body of `cards_by_block`).
- `RemoteSpace` gains `pub screen: Option<(String, Vec<String>)>` (home's block, its last lines) and `pub
  screen_asking: bool`. `pub(crate) const AGENT_SCREEN_AGAIN: Duration = Duration::from_secs(2)`.
  `Workspace::read_home_screen(&mut self, terminal: &str, cx: &mut Context<Self>)`: for the selected row's remote
  pane that isn't in `RemoteSpace::streaming` and whose remote asks nothing of `why_not(ScrollBack)`, with no read in
  flight: `read_terminal {block: home's id, lines: SCREEN_LINES}`; its answer's `text` → `screen_of`. `fn
  screen_of(text: &str) -> Vec<String>` (the last `SCREEN_LINES` lines, each `clean_to(_, LINE_CHARS)`).
  `select_agent` reads once and starts `look_at_home_screen` (a task like `look_at_automations`, `screen_looking` its
  flag) that reads every `AGENT_SCREEN_AGAIN` while `agents_open` and the selected row is such a pane, and ends
  otherwise.
- The details of a remote row (`agents_view.rs`'s `detail`, when `pane.remote` is Some): From's model · effort and
  Role from `remote.agent`; Task from `remote.agent.task` ("t3", its first `TASK_LINES` lines, "from {title}" by
  `uid_title_in(space, by)` else `by`, "from the API" for `"api"`, "{age} ago" from `since` by the wall clock);
  Subagents "{n} subagents" from `remote.agent.subagents` (no lines, Left out 13); Usage as today with
  `with_effort`; Screen the viewer's lines when streamed, else `RemoteSpace::screen` for that block. A `dim` group's
  header and rows draw at 50% opacity.
- `pub(crate) fn with_effort(tip: String, effort: Option<&str>) -> String` (`usage.rs`): the tip, then a line
  "Effort: {effort}" when some. `block.rs`'s usage chip of a remote pane uses it with `remote.agent.effort`.

- [ ] **Step 1: Write the failing tests.** `usage.rs`: `the_effort_ends_the_tooltip` (`with_effort("a".into(),
  Some("high")) == "a\nEffort: high"`, `None` unchanged). `tests/agents_view.rs`:

```rust
#[gpui_kit::test]
fn a_remote_agent_is_listed_under_its_computer_with_home_s_details(cx: &mut TestAppContext) {
    // offline mirror; RemoteSpace::agents [claude "Claude Code"]; apply terminal_of_home(agent "claude" working,
    // model "opus", effort "high", role "reviewer", task { "t3", "Ship it.", by Some("api") }); frame. agent_rows():
    // one row { space "r1", space_name "API · 127.0.0.1:9", title "claude", agent "Claude Code", task
    // Some("Ship it."), dim false }. go_to_agent(row.terminal) with a local workspace shown → r1 is shown and the
    // viewer is its focused block. Reconnecting → dim true, still listed.
}

#[gpui_kit::test]
fn a_remote_agent_is_done_here_until_looked_at_here_and_home_s_done_reads_idle(cx: &mut TestAppContext) {
    // set_active; a local block focused; apply working, frame, apply idle, frame → done_count() 1 and the row's
    // doing Done; show r1 and focus the viewer, frame → 0. Apply home's state "done" (home's own mark) after working:
    // the row is Done here only by this computer's watch (working → done here reads working → idle: Done); after it
    // is looked at, a state still "done" reads Idle.
}

#[gpui_kit::test]
fn a_screen_read_keeps_its_last_twelve_clean_lines(cx: &mut TestAppContext) {
    // offline, live Read share; agents popup open; select the remote row → sent() ends read_terminal {block "abcde",
    // lines 12}; answer {"text": 10_000 lines, one with "\u{1b}[31m"} → RemoteSpace::screen == ("abcde", the last 12,
    // no control character). advance AGENT_SCREEN_AGAIN → one more read; close the popup, advance → none.
    // Mark "abcde" streamed (RemoteSpace::streaming) → selecting reads nothing.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum usage::tests workspace::tests::agents_view` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say.
- [ ] **Step 4: Run them** → PASS.
- [ ] **Step 5: Verify the look** headlessly (`render_frame` with the popup open on a remote row: `agents-screen`
  and the Task section draw); a minors entry "The Agents popup's remote groups and details, and a remote chip's
  effort line, were checked headlessly only" (`agents_view.rs`, `block.rs`). The effort minor leaves `docs/minors.md`.

---

### Task 8: The inbox: home's age, a request with no viewer, and how many more

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`aged`, `AGE_MOST`; `remote_wait` takes home's `since` and keeps a
  request with no viewer; `remote_requests` counts the rest; `RemoteSpace.requests_more`)
- Modify: `crates/templum/src/workspace/waits.rs` (`request_space`; `orphaned`, `tell_asking`, `tell_folded`,
  `wait_as`'s arrival through it), `crates/templum/src/workspace/inbox.rs` (`inbox`, `show_request` through it;
  `inbox_more`),
  `crates/templum/src/workspace/bus.rs` (`notify_from`'s `me: Option<BlockId>`)
- Modify: `crates/templum/src/inbox.rs` (Go to terminal only with a block; the list's last line)
- Test: `crates/templum/src/workspace/tests/remote.rs` (after
  `a_request_is_the_one_shown_by_its_id_and_a_new_id_is_a_new_one`)
- Docs: `docs/specs/ask-inbox.md` ("Requests from a mirrored workspace": the three), `docs/minors.md` (the three leave
  it)

**Interfaces:**
- Produces: `pub(super) const AGE_MOST: Duration = Duration::from_secs(7 * 24 * 60 * 60)`; `pub(super) fn aged(since:
  i64, now: Instant, wall: i64) -> Instant` (`now` less `wall - since` seconds, clamped to `0..=AGE_MOST`; `now` when
  `checked_sub` fails). `remote_wait` sets the new wait's `since` to `aged(request.since, executor now, unix now)`.
- A request whose block has no viewer here is a wait with `from` "" (no terminal has it); `pub(crate) fn
  request_space(&self, wait: &Wait) -> Option<String>`: for a remote wait the space of its viewer (`asker`), else its
  `RemoteAsk::space` while that space exists; else `caller`'s. `inbox()`, `orphaned()` (a remote wait is withdrawn
  only when `request_space` is None), `show_request`, `wait_as`'s `inbox_arrived` check, `tell_asking` and
  `tell_folded` use it; `go_to_request` keeps `asker` (no block, no Go to terminal). `notify_from(space, me:
  Option<BlockId>, …)`: a click with `request` opens the inbox at it, else reveals `me` when some.
- `RemoteSpace` gains `pub requests_more: usize` (home's wanted requests past `MOST_REQUESTS`, set by
  `remote_requests`). `Workspace::inbox_more(&self) -> Option<String>`: for the shown remote with `requests_more > 0`,
  "And {n} more on {computer}."; the inbox draws it dim as the list's last line (`inbox-more`).

- [ ] **Step 1: Write the failing tests** (`tests/remote.rs`; `home_allows` and `offline_asked` as built):

```rust
#[test]
fn a_request_is_aged_from_home_s_since() {
    // aged(1000, now, 1120) == now - 120 s; aged(2000, now, 1000) == now (home's clock ahead); aged(0, now,
    // 10_000_000) == now - AGE_MOST.
}

#[gpui_kit::test]
fn a_mirrored_request_ages_from_home_s_since(cx: &mut TestAppContext) {
    // offline_asked(home_allows("r3", "connect", unix(now) - 300, …)) → the wait's since is within 2 s of
    // executor now - 300 s.
}

#[gpui_kit::test]
fn a_request_whose_block_has_no_viewer_is_listed_without_go_to_terminal(cx: &mut TestAppContext) {
    // offline_asked with a request whose block "zzzzz" isn't in home's blocks → inbox() lists it (one wait,
    // from ""), asker None, request_space Some("r1"); after drop_orphans (a frame) it is still listed; home's next
    // state without it withdraws it; rendering the inbox has no `inbox-go`.
}

#[gpui_kit::test]
fn past_fifty_requests_the_inbox_says_how_many_more(cx: &mut TestAppContext) {
    // 53 requests of home's → inbox() has 50, inbox_more() == Some("And 3 more on 127.0.0.1:9."); 51 → "And 1 more
    // on …"; 50 → None.
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::remote::tests workspace::tests::remote::a_request
  workspace::tests::remote::a_mirrored workspace::tests::remote::past_fifty` → FAIL.
- [ ] **Step 3: Implement** as the Interfaces say.
- [ ] **Step 4: Run them** → PASS; also `cargo test -j 4 -p templum workspace::tests::requests
  workspace::tests::inbox workspace::tests::remote` → PASS.
- [ ] **Step 5: Docs** as the Files say; the three minors leave `docs/minors.md`.

---

### Task 9: The end-to-end cases, the words, batch 2 closed, the full check and the benches

**Files:**
- Modify: `crates/templum/src/workspace/tests/mirror_end_to_end.rs` (a "Batch 2h" section)
- Modify: `crates/templum/src/space_dialog.rs` (the Edit… row's words), `crates/templum/src/share_dialog.rs`
  (`INVITE`)
- Docs: `docs/specs/templum-to-templum.md` (Status: "Batch 2 implemented (parts 2a–2h, plans 2026-10-25-t2t-batch-2a…
  2g and 2026-10-26-t2t-batch-2h-agents-automations-inbox)"; "The mirror's side"'s caps table gains "Automations | 500
  a remote; texts 64 KiB"; "As built ▸ Batch 2h": what the builder chose or changed, this plan's "Open for the
  orchestrator" as settled; "#### Batch 2 as built, in short", as batch 1's), `docs/api.md` (§3 "Connect (the
  mirror)" gains a closing paragraph **On a mirrored workspace.** that keeps what §7 says works today, and the
  automations sentence below; §7 "What a mirror can't do yet" lists only the 15 items of "Left out of batch 2", in
  people's words), `docs/guide.md` (line 70: "Its **Share…** lets another computer's Templum mirror the workspace live
  and work in it."), `docs/specs/automations.md`, `docs/specs/agents-view.md`, `docs/specs/ask-inbox.md` (as Tasks 3,
  7, 8 left them), `docs/ARCHITECTURE.md` (`workspace/remote_rules.rs`: the copies of home's rules and schedules, the
  popup's keys), `docs/minors.md` (the words' minor leaves it; the entries of Tasks 6 and 7)
- Test: `crates/templum/src/workspace/tests/mirror_end_to_end.rs`

**Interfaces:**
- Consumes: everything above; `mirrored`, `agents_running_in`, `me`, `event`, `call_later`, `after`, `pump_home`,
  `remote_of`, `set_active`, `frame`.

- [ ] **Step 1: Write the end-to-end tests** (one workspace both ends, over loopback):

```rust
// Batch 2h: agents, automations and the inbox.

#[gpui_kit::test]
fn a_home_rule_switched_and_run_from_the_mirror_runs_at_home_once(cx: &mut TestAppContext) {
    // Home's rule { name "Morning review", workspace first, Daily 09:00, Notify "Look" } added, then mirrored(Full);
    // pump until remote_of(ws).rules has it (key = home_key("r1", id)). set_automation_on(key, false) → pump until
    // home's rule is off and the copy too. run_automation_now(key) → pump until said has "{r1 name} on {computer}:
    // Morning review: Look", then 5 more rounds: ws.api.notices with automation Some(id) count exactly 1, home's
    // last is (Ran, "Notified."), automation_runs has no key ≥ HOME_KEYS.
}

#[gpui_kit::test]
fn a_home_rule_edited_and_duplicated_on_the_mirror_is_saved_at_home(cx: &mut TestAppContext) {
    // edit_automation(key, name "Evening review") + save_automation(key) → pump until home's rule is named so;
    // duplicate_automation(key) → pump until home has "Evening review (copy)", off.
}

#[gpui_kit::test]
fn a_home_schedule_is_listed_and_cancelled_from_the_mirror(cx: &mut TestAppContext) {
    // agents_running_in; schedule_add at home { by mine, to theirs, after 30 min }; mirrored; pump until r1's group
    // lists s1; cancel_home_schedule("r1", "s1") → pump until home's schedules are empty and the group's too.
}

#[gpui_kit::test]
fn a_task_handed_at_home_shows_in_the_mirror_s_agents_popup(cx: &mut TestAppContext) {
    // agents_running_in; mirrored; mine delegates "Ship it." to theirs at home (call_later), "submitted" →
    // pump until the r1 row of theirs' viewer has task Some("Ship it.") and its RemotePane.agent.task.by is mine's
    // uid.
}

#[gpui_kit::test]
fn a_remote_agent_is_done_on_the_mirror_until_its_block_is_looked_at(cx: &mut TestAppContext) {
    // agents_running_in; mirrored; set_active; a local block focused; theirs "submitted" then "stopped" at home →
    // pump until r1's row of theirs is Done and done_count() counts it; show r1, focus its viewer, frame → that
    // row is Idle.
}

#[gpui_kit::test]
fn the_mirror_s_own_new_rule_offers_no_remote_workspace(cx: &mut TestAppContext) {
    // mirrored (r1 shown); new_automation → NEW's workspace is the first (local) workspace's id; where_choices has
    // no "r1".
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum mirror_end_to_end` → PASS (a failure is a bug of Tasks 1–8:
  fix it there, with a headless test where the headless ones missed it).
- [ ] **Step 3: The words** in `space_dialog.rs` and `share_dialog.rs` as Global Constraints quote them (UI-only:
  verified by reading them against the spec and by `cargo test -j 4 -p templum space_dialog share_dialog` still
  passing); their minor leaves `docs/minors.md`.
- [ ] **Step 4: The docs** above. `docs/api.md` §3's paragraph also says: "Home's automations run only on home: the
  Automations popup shows home's rules of the shared workspace in a group of their own and switches, runs, edits,
  duplicates and deletes them there (a rule that runs a command, and a delete, need a Full share); this computer's
  rules never run against a mirrored workspace."
- [ ] **Step 5: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 6: The benches,** one at a time: `cargo run -j 4 --release -p templum -- --bench-mirror`, then
  `cargo run -j 4 --release -p templum -- --bench` (the block chip's tooltip is painting code) → each `p95_ms` ≤ 16.7;
  report both JSON lines.
- [ ] **Step 7: Leave uncommitted;** report the files changed, the checks' and the benches' results, what went to and
  left `docs/minors.md`, and confirm no existing automations test was edited (`git diff --stat --
  crates/templum/src/workspace/tests/automations.rs` shows only Task 2's two new tests).

---

## Interfaces for later parts

Batch 2 ends here. What a later card builds on:

- **Home's rules on a mirror:** `remote_rules::{HOME_KEYS, home_key, key_home, HomeRule, HomeSchedule, rule_of,
  RuleGroup}`, `Workspace::{rule, home_rule, rule_groups, new_home_automation, save_home_rule, rule_json,
  home_blocks, home_kinds, cancel_home_schedule, home_unseen, form_problem}`; the deeds `SetRule`, `RunRule`,
  `SaveRule`, `DeleteRule`, `CancelSchedule`.
- **The rule:** `Space::automations_there`; `automation_target`, `automation_problem` and `watch_agents` are where a
  remote workspace is kept out of this computer's engine.
- **Home's state:** `TaskView`, `ScheduleView`, `Notice.automation`; `cancel_schedule`.
- **The Agents popup:** `AgentRow.dim`, `cards_of`, `RemoteSpace::{screen, screen_asking}`, `read_home_screen`,
  `AGENT_SCREEN_AGAIN`, `usage::with_effort`. A later card that lists a remote agent's subagents (Left out 13) reads
  home's state for them.
- **The inbox:** `request_space`, `aged`, `RemoteSpace::requests_more`, `inbox_more`.

## Open for the orchestrator

Each decided with the recommended option; the plan is written that way.

1. **A home rule's popup key is a number in a range of its own** (`HOME_KEYS | n << 32 | id`), not a key enum: every
   local signature and the 3,300 lines of automations tests stay, as `RemoteSpace::fresh_id` keeps the mirror's own
   ids apart. *Recommended.*
2. **No hold for a switched home rule or a cancelled schedule:** the spec's holds name blocks, cards, items and wires;
   these show a round trip later. *Recommended.*
3. **Duplicate sends home's own `when` and `do` back unchanged** (`HomeRule::raw`), so nothing of home's is altered
   (2g's lesson); Save sends the form, the cleaned copy the person edited and saw. *Recommended.*
4. **Home's `save_automation` keeps a new rule's `on`** (default on): without it the mirror's Duplicate makes a copy
   that runs at once at home, beside the original. The editor's New draft is on, so nothing local changes.
   *Recommended.*
5. **The foot sentence shows under every home rule's editor:** home's state doesn't say which token saved a rule, and
   a Save from the mirror makes it the share's. *Recommended.*
6. **A new local rule starts on the shown workspace when it is local**, else on the first local one (the spec's "not
   the one shown" read for a mirror shown). *Recommended.*
7. **This computer refuses to save a rule whose Where is a remote workspace** (the editor and its own API, which share
   `save_rule`), with the rule's words; a stale one already saved is Skipped. *Recommended.*
8. **`read_terminal` calls are grouped in the audit log** like `terminal_input`: the Agents popup reads a remote
   agent's screen every 2 s. *Recommended.*
9. **A remote request's age:** home's `since` ahead of the mirror's clock reads as now; one older than 7 days reads as
   7 days. *Recommended.*
10. **A request with no viewer is a wait with an empty `from`**, its workspace from `RemoteAsk::space`; it notifies
    and its click opens the inbox at it. *Recommended.*
11. **The badge counts home's failed, skipped and missed runs whose `last.at` is after the popup was last opened**
    (home's clock against this computer's). *Recommended.*
12. **Caps the spec doesn't give:** 500 rules a remote, a rule's texts 64 KiB; the spec's table gains the row.
    *Recommended.*
13. **"A remote request is aged from home's `since`" is a headless case,** not in `mirror_end_to_end.rs`: in one
    process a request has the same age at both ends, so the case can't fail there. *Recommended.*
14. **"Nothing runs on the mirror and its `automations.json` holds no home rule" runs on two workspaces over
    loopback** (`shown_mirror_as`, Task 6), not in `mirror_end_to_end.rs`, where one workspace is both ends and home's
    rule is in that workspace's file by being home's. *Recommended.*
15. **A notice of a home rule opens the Automations popup at that rule** on a click, as a local Notify me's click does.
    *Recommended.*
