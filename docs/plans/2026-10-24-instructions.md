# Instructions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The person writes instructions in Settings ▸ Instructions, for every workspace or one and every agent or
chosen ones, and the hooked agents get those that apply each time a session starts.

**Architecture:** `templum hook <agent>` already runs on every hooked agent's session start
(crates/templum-mcp/src/cli.rs `run` ~95, `hook_event` ~698, `send_event` ~763). Task 1 makes it, on a session's
start only, ask the bus `templum/instructions` on the connection it opens for the event and print the answer in the
agent's shape. Task 2 keeps the instructions in `<data>/instructions.json` (`crates/templum/src/instructions.rs`, as
`checklists.rs`) and answers that request in `Workspace::serve` (workspace/bus.rs ~170) with the framed text of those
that apply. Task 3 adds the Settings section that edits them.

**Tech Stack:** Rust, GPUI (gpui-fast 0.1.8) + gpui-kit 0.7.3 (pinned).

**Spec:** [docs/specs/instructions.md](../specs/instructions.md) (read it first).

## Global Constraints

- The hook waits for Templum's answer **2 seconds** at most (a constant); without an answer, or with an empty text,
  it prints nothing. It always exits 0.
- Only a session's start asks and prints; every other hook prints nothing, as today.
- templum-mcp stays UI-free: it knows the method, the request and each agent's output shape, never which instructions
  apply or their words (the templum crate's, beside `instructions()` and `flat` in workspace/bus.rs).
- Every logic change ships a test that fails without it: templum-mcp unit tests (cli.rs tests with `test_bus`,
  hooks.rs tests); GPUI test-support tests in a new `crates/templum/src/workspace/tests/instructions.rs`. UI looks:
  say how verified. Minors to docs/minors.md; docs ≤ 120 columns; no new dependency.
- Commits on `development`, Conventional Commits. Full check before each commit:
  `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.

## Review Focus

1. Templum gone, or hanging: the hook prints nothing and returns within the limit; the agent isn't held. → Task 1 test.
2. Only a session's start asks and prints: a `PermissionRequest` or a `Stop` still prints nothing. → Task 1 test.
3. A compaction (`source: compact`) sends no event (its turn keeps working) but gets the instructions. → Task 1 test.
4. One for `claude2` never reaches a plain Claude Code block; one for `claude` reaches `claude2`'s. → Task 2 test.
5. A deleted workspace, or chosen agents all removed: the instruction stays and applies nowhere. → Task 2 test.

---

### Task 1: The session's start asks Templum and prints its answer

**Files:**
- Modify: `crates/templum-mcp/src/hooks.rs` (beside `METHOD` ~7), `crates/templum-mcp/src/cli.rs` (`run` ~95,
  `hook_event` ~698, `send_event` ~763), docs/specs/connections.md (Hooks and agent state: a session's start asks
  and prints; The bus: a hook client's `templum/instructions` request; The `templum` command line: `templum hook`)
- Test: `crates/templum-mcp/src/hooks.rs` tests, `crates/templum-mcp/src/cli.rs` tests

**Interfaces:**
- Produces (hooks.rs):
  - `pub const INSTRUCTIONS: &str = "templum/instructions";` request params `{"agent": <the hook's agent>}`, result
    `{"text": <string, empty when none apply>}`.
  - `pub fn session_start(agent: &str, event: Option<&str>, input: &Value) -> bool`: true for `SessionStart` of any
    `source` from `claude`, `codex`, `gemini`, `qwen` (not one with an `agent_id`), and Copilot's `sessionStart`
    (`event` is its hook's command word); false for `opencode`, `pi` and everything else.
  - `pub fn start_output(agent: &str, text: &str) -> Option<String>`: the JSON line, `{"hookSpecificOutput":
    {"hookEventName":"SessionStart","additionalContext":text}}` for `claude`, `codex`, `gemini`, `qwen`;
    `{"additionalContext":text}` for `copilot`; None for any other agent or an empty `text`.
- Produces (cli.rs): `const ANSWER_WITHIN: Duration = Duration::from_secs(2)`; `hook_event` also returns the input
  it read (so `run` can test `session_start`); `fn instructions(stream: Stream, agent: &str, within: Duration) ->
  Option<String>` sends the request (id 1) after the event on the same connection, sets the read limit to what is left
  of `within`, reads lines until the answer with id 1, then `finish`es. `run` writes `start_output` to stdout when
  there is one. `within` is a parameter so the test runs with 200 ms.

- [ ] **Step 1: Failing tests:**
  - hooks.rs `each_agent_takes_context_its_own_way`: `start_output` gives the `hookSpecificOutput` line for claude,
    codex, gemini and qwen, the flat one for copilot, None for opencode, pi and an empty text; the text is JSON-escaped
    (`"a\nb \"q\""` round-trips through `serde_json`).
  - hooks.rs `only_a_session_start_is_one`: `session_start` true for each `source` of each agent (Copilot's three),
    false for `UserPromptSubmit`, `Stop`, a Claude Code hook with an `agent_id`, Copilot's `agentStop`, any opencode
    or pi line.
  - cli.rs `a_session_start_prints_templums_instructions`: a `test_bus` answering `templum/instructions` with `"Be
    brief."`; `templum hook claude` with a `SessionStart` `startup` → exit 0, stdout is the `hookSpecificOutput` line
    with `"Be brief."`, stderr empty; the bus got the `started` event, then the request with `{"agent":"claude"}`, in
    that order, from a `hook` client.
  - cli.rs `copilots_start_prints_its_own_shape`: `templum hook copilot sessionStart` → `{"additionalContext":…}`.
  - cli.rs `a_compaction_gets_the_instructions_again_and_no_event` (Review Focus 3): `SessionStart` `compact` → no
    `templum/event` reaches the bus, the request does, and stdout has the context.
  - cli.rs `no_instructions_print_nothing`: the answer's text is `""` → stdout empty.
  - cli.rs `a_templum_that_doesnt_answer_costs_at_most_the_limit` (Review Focus 1): a `test_bus` that never answers;
    the inner call with `within` 200 ms returns in under a second with None, and `run` prints nothing; with no
    `TEMPLUM_BUS` (`Env::default()`), `run` prints nothing at once.
  - cli.rs `only_a_session_start_asks_and_prints` (Review Focus 2): extend
    `a_hook_sends_its_event_prints_nothing_and_never_fails` with a bus that would answer instructions: `Stop` and
    `PermissionRequest` print nothing and no `templum/instructions` request arrives.
- [ ] **Step 2: Run** `cargo test -p templum-mcp` → FAIL.
- [ ] **Step 3: Implement** the interfaces above; update `run`'s doc comment ("prints nothing" but at a session's
  start). Specs: connections.md as listed in Files (the hook's one-second limit stays for the connection; the answer
  has 2 s).
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `feat(connections): Claude Code, Codex, Gemini CLI, Copilot CLI and Qwen Code ask Templum
  for your instructions as a session starts`.

### Task 2: Templum answers with the instructions that apply

**Files:**
- Create: `crates/templum/src/instructions.rs` (the model, `instructions.json`'s load and save, as
  `checklists.rs`), registered in main.rs
- Modify: `crates/templum/src/workspace/mod.rs` (fields beside `checklists_saved`/`checklists_frozen` ~1233 and
  their init ~1723), `crates/templum/src/workspace/spaces.rs` (the load at start, beside checklists' ~70),
  `crates/templum/src/workspace/persist.rs` (the snapshot ~436 and the quit save ~578), `crates/templum/src/
  workspace/bus.rs` (`serve`'s request arms ~170, `flat` ~1033), `crates/templum/src/workspace/content.rs` (`runs`
  ~134 becomes `pub(super)`), docs/specs/connections.md (which apply, the frame), docs/ARCHITECTURE.md (the new
  file; `instructions.json` among the app-wide files), docs/minors.md (Connections)
- Test: `crates/templum/src/instructions.rs` tests, `crates/templum/src/workspace/tests/instructions.rs` (new,
  registered in tests/mod.rs)

**Interfaces:**
- Consumes: Task 1's `templum_mcp::hooks::INSTRUCTIONS` and its request and result.
- Produces (instructions.rs):
  - `pub(crate) struct Instructions { version: u32, next_id: u64, instructions: Vec<Instruction> }` and
    `pub(crate) struct Instruction { id: u64, name: String, text: String, on: bool, workspace: Option<String>,
    agents: Option<Vec<String>> }` (serde: `on` defaults to true; `workspace` and `agents` skipped when None).
  - `fn add(&mut self, draft: Draft) -> u64`, `fn set(&mut self, id: u64, draft: Draft)`, `fn set_on(&mut self, id,
    on: bool)`, `fn remove(&mut self, id)`, `fn get(&self, id) -> Option<&Instruction>`; `pub(crate) struct Draft {
    name, text, workspace, agents }` (Task 3's editor fills it).
  - `fn applying<'a>(&'a self, workspace: &str, agent: impl Fn(&str) -> bool) -> Vec<&'a Instruction>`: on, text not
    blank, workspace None or `workspace`, agents None or one for which `agent(id)` holds; every-workspace ones first,
    then the workspace's, each in the list's order.
  - `path`, `load(dir) -> Loaded<Instructions>`, `save(dir, &Instructions, generation)`, as checklists'.
- Produces (workspace): `Workspace::standing: Instructions` (with `standing_saved`, `standing_frozen`), loaded,
  snapshotted and saved at quit as checklists are, with their notifications ("Couldn't read the instructions (…)").
  `fn standing_for(&self, terminal: &str, agent: &str) -> String` (workspace/bus.rs): empty for no block or
  connections off; the session's agent is `pane.agent()` when `self.runs(pane.agent(), agent)`, else `pane.account`
  when it is an account of `agent`, else `agent`; the instructions `applying(space.id, |id| self.runs(Some(&session),
  id))`, framed as the spec says. `fn invisible(c: char) -> bool` taken out of `flat`, and `fn clean_text(&str) ->
  String` (keeps line breaks, drops `invisible` and control characters but `\n` and `\t`, trims trailing blank lines).
  `serve` answers `INSTRUCTIONS` from a `hook` client with `{"text": standing_for(..)}` (`""` when off).

- [ ] **Step 1: Failing tests:**
  - instructions.rs `the_file_round_trips_and_an_older_one_loads`: a file without `on`, `workspace`, `agents` reads
    as on, every workspace, every agent; ids are never reused after a `remove`.
  - instructions.rs `every_workspace_comes_first_then_the_list`: A (W), B (every), C (W), D (every, off), E (every,
    blank text) → `applying("W", …)` is B, A, C; `applying("X", …)` is B.
  - tests/instructions.rs `a_session_gets_the_instructions_that_apply` (through `ask_as` as a `hook` client, the
    `templum/instructions` request): a Claude Code terminal in workspace W gets the frame, then `## B`, `## A`, `## C`
    with their texts; a terminal in another workspace gets only B.
  - `an_instruction_for_chosen_agents_reaches_only_them`: one for codex → the Claude Code terminal's text is `""`, a
    Codex terminal's has it; one for a person's `[agents]` entry running `claude` reaches the Claude Code terminal.
  - `an_accounts_instruction_is_its_own` (Review Focus 4): one for `claude2` → a plain Claude Code terminal gets `""`,
    a terminal of the `claude2` account (`Pane::account`) gets it; one for `claude` → the `claude2` terminal gets it.
  - `the_hooks_agent_names_a_session_templum_hasnt_seen`: a terminal with no seen agent asking as `codex` gets
    codex's; a terminal seen running Claude Code asking as `codex` (a stale look) gets codex's, not claude's.
  - `a_gone_workspace_or_agent_applies_nowhere` (Review Focus 5): an instruction for a workspace id no one has, and
    one whose only chosen agent was removed from `[agents]` → neither reaches any terminal; both are still in
    `ws.standing`.
  - `a_moved_block_gets_its_new_workspaces_instructions`: move the block (`take_block`/`with_space`) → its next
    answer has the new workspace's.
  - `none_while_connections_are_off_or_outside_a_block`: `[connections] enabled = false` → `""`; a terminal id that
    isn't a block → `""`; both answered at once.
  - `the_text_loses_what_the_editor_doesnt_show`: a text with U+200B and a tag character (U+E0041) between lines →
    answered without them, its line breaks kept; a name with a newline is one line.
  - `a_broken_instructions_file_is_never_written_over`: an unparsable `instructions.json` at start → kept as `.bak`
    with a notification; an unreadable one → not written by the snapshot or at quit.
- [ ] **Step 2: Run** `cargo test -p templum instructions` → FAIL.
- [ ] **Step 3: Implement** the interfaces above. Specs: connections.md (the answer, which apply, the frame; link to
  instructions.md), ARCHITECTURE.md. Add to docs/minors.md (Connections), each with where, what a person sees and a
  fix: Pi, OpenCode and Kilo Code get no instructions (Pi's extension could prepend them on `before_agent_start`,
  OpenCode's plugin through its system-prompt hook); Qwen Code's (and, unless run by hand, Copilot CLI's) output is
  from their docs only; Claude Code's subagents get none (`SubagentStart` takes `additionalContext`); a resume with
  none applying leaves an earlier set in the conversation (the hook knows `source`: say "none apply any more" on a
  resume).
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `feat(connections): agents get the instructions written for their agent and workspace when
  a session starts`.

### Task 3: Settings ▸ Instructions

**Files:**
- Create: `crates/templum/src/settings/instructions.rs` (the section's rows, "Add an instruction", the dialog), as
  `settings/agents.rs`
- Modify: `crates/templum/src/settings/mod.rs` (`Section` ~60: `Instructions` after `Agents`, its title, tab id
  `settings-tab-instructions`, its `note`; the page ~325), `crates/templum/src/settings/sections.rs` (its rows),
  `crates/templum/src/workspace/preferences.rs` (the edits, beside `save_agent` ~40), docs/specs/settings.md
  (Sections and fields: Instructions), docs/specs/instructions.md (Status: Implemented, with the three commits)
- Test: `crates/templum/src/workspace/tests/instructions.rs`

**Interfaces:**
- Consumes: Task 2's `Instructions`, `Draft`, `Workspace::standing`.
- Produces: `Workspace::save_instruction(&mut self, id: Option<u64>, draft: &Draft, cx) -> Result<u64, &'static
  str>` (refuses a blank name, a blank text, and `Some(vec![])` agents, with the words the notification shows);
  `delete_instruction(id, cx)`; `set_instruction_on(id, on, cx)`. Element ids: `instruction-on-{id}`,
  `edit-instruction-{id}`, `add-instruction`, `delete-instruction`. A row's description: "{where} · {agents}", where
  is "Every workspace", the workspace's name or "A deleted workspace", agents "Every agent" or the chosen titles
  joined by ", " (an id no longer an agent: "a removed agent").

- [ ] **Step 1: Failing tests:**
  - `saving_an_instruction_adds_it_last_and_applies_at_once`: `save_instruction(None, …)` twice → the list's order is
    theirs; a session's start answered right after has the new one.
  - `save_refuses_what_is_missing`: blank name, blank text, chosen agents with none → each refused with its words,
    nothing added.
  - `the_switch_turns_one_off_and_its_row_says_where_and_for_whom`: click `instruction-on-{id}` in the open panel → it
    is off, a session's start gets none of it; the row's description reads "api · Claude Code, Codex".
  - `delete_removes_only_that_one`.
  - `the_section_note_says_when_changes_reach_agents`: the Instructions tab shows the note's line.
- [ ] **Step 2: Run** → FAIL.
- [ ] **Step 3: Implement** the section and the dialog as the spec describes (gpui-kit `Input`, a dropdown, the
  agents' checkboxes, `Textarea`, as `agent_dialog`). Say in the commit's body how the dialog's look was verified (a
  run of `cargo run -p templum -- --dev`: add, edit, switch off, delete, with a Claude Code started after each change).
  Specs: settings.md; instructions.md's Status.
- [ ] **Step 4: Run** → PASS; full check.
- [ ] **Step 5: Commit** `feat(settings): write instructions for every agent or chosen ones, in every workspace or
  one`.
