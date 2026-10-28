# Claude Code's subagents on the canvas

- **Date:** 2026-10-22
- **Status:** Implemented (kanban card 127, "Subagentes do Claude"), from the study
  [docs/plans/2026-10-22-claude-subagents.md](../plans/2026-10-22-claude-subagents.md): its option C1 (a parent
  honestly busy while its subagents work) and A1 (each subagent in a terminal-style block of its own), with the
  person's answers to its open questions.
- **Builds on:** [connections.md](connections.md) (hooks and agent state, delegating),
  [agent-protocol.md](agent-protocol.md) (what ends a task).

## Goal

Claude Code starts subagents of its own with its `Agent` tool, inside its process, in the background by default. A
person watching the canvas should see each of them work, and an agent that handed that Claude Code a task should get
its real answer, not the message of the turn that started the subagents.

**Not in it:** typing into a subagent, answering its dialog from its block, or stopping it there (it has no terminal
of its own: the dialog is in its parent's screen, the study's [Interacting with one](../plans/2026-10-22-claude-subagents.md#interacting-with-one));
the chat look (the study's A2); a switch that takes Claude Code's subagents away (B2); steering one through its
parent (C3); subagents of a Claude Code opened outside Templum, or of the Templum Agent (which has no `Agent` tool).
Templum's instructions to Claude Code about subagents stay as they are.

## What the person sees

### The parent

A Claude Code whose turn ends while subagents it started still work stays at work: its dot keeps the accent, and its
tooltip says how many work, "Working: 2 subagents" ("Working: 1 subagent"); `list_blocks` says `, working: 2 Claude Code
subagents` (not the agents it handed tasks to, [agent-protocol.md](agent-protocol.md)'s subagents). Each subagent's
report comes back to it as a turn of its own; once the last one is in and that turn ends, it is at rest, and a task
another agent handed it ends with that turn's answer.

A permission dialog a subagent opens is in its parent's screen: the parent's dot turns yellow, "Waiting for you: Allow
Bash?", and stays so until that subagent's tool ends (or it stops): another subagent's tool, or the parent's own,
doesn't turn it back to Working.

### Its block

Each subagent of a Claude Code in a Templum terminal opens a block of its own as it starts, built-in ones (`Explore`,
`Plan`, `general-purpose`) as much as the person's `.claude/agents` ones and forks; Claude Code's own agents (prompt
suggestions, `/btw`), which have no type, don't. Accounts (`CLAUDE_CONFIG_DIR`) work the same: the transcript's path
comes from the hook.

- **Where.** Beside its parent, as `spawn_agent` places an agent's blocks: toward the blocks wired to the parent,
  down the column the ones it opened started, overlapping nothing (`Space::room_near`), 90 × 22 cells. Many at once
  go down that column, one under the other. It is wired to its parent. The keyboard, the selection and the camera
  stay where they are, and Ctrl+Z doesn't take it away: the person didn't open it.
- **What it shows.** A terminal block that runs nothing (`Terminal::viewer`, [The follower](#the-follower)), fed the
  subagent's transcript as it grows: a dim first line ("A subagent of Claude Code, read-only: what it asks is answered
  in its Claude Code's block."), then its task (bold "Task" and the prompt), "thinking…" (dim) for a thought, its words,
  each tool call as `● Bash  cargo test -p parser` (its command, file, pattern, URL… in one line of at most 100
  characters), each result's first line under it, `⎿ a.txt +1 lines` (dim) or `✗ Permission denied` (red), and once its
  last turn ended a dim `─ Done`. A message that resumes it shows as `› …`. Its title is its type and description,
  "Explore · List probe files", and "· done" once it ended; the description is the transcript's `meta.json`'s (observed,
  not documented), else the first line of its task. Zoom, the silhouettes far away, selection, copy, scrolling and find
  are a terminal's.
- **Its dot.** The accent while it works ("Working"); yellow while its dialog waits in its parent's screen ("Waiting
  for you: Allow Bash? Answer in its Claude Code."); grey once it finished ("Done").
- **Read-only.** It has no program: keys, typed text, pastes and resizes reach nothing, and its first line says where
  its dialogs are answered. The tools that type or press into a terminal (`send_to_terminal`, `send_keys`,
  `interrupt`, `stop_agent`, `delegate`, `reply` to it) refuse it: "*b7k2m* shows a Claude Code subagent's
  transcript, read-only: nothing typed or pressed there reaches the subagent. Its dialogs are answered in its Claude
  Code's block; read_terminal reads it." `read_terminal` and `wait_for` read it as any terminal, and `list_blocks`
  lists it, so its parent can read what it did, with `, a Claude Code subagent's transcript, read-only` after its
  wire.
- **Resumed.** A subagent started again (`SubagentStart` with the same id) works again in the same block.
- **Closing itself.** A minute after it finished (`SubagentStop`), or after its parent's session ended (`SessionEnd`,
  a new session, or the agent gone), its block closes without a question and without a Ctrl+Z step. **While the
  person reads it** (its workspace is on screen, and it is focused, the pointer is over it, it is selected, or its
  view is scrolled back from the end), it stays, and its minute starts over once they stop: nothing closes under the
  person's eyes. In a workspace not shown no one reads it, whatever focus or selection that workspace kept.
- **Closing its parent** closes its subagents' blocks with it; undoing that close brings back only the parent.
- **Never saved.** A subagent's block belongs to a live session: the canvas is saved without it and its wires
  (`Space::kept`), so a restart doesn't bring it back, and the snapshot doesn't look at it.

## Hooks

Templum's Claude Code settings file adds `SubagentStart` and `SubagentStop` to the hooks of
[connections.md](connections.md#hooks-and-agent-state). `templum hook claude` turns them into events
(`hooks::claude`, `crates/templum-mcp/src/hooks.rs`). The fields are the ones the study's
[probe](../plans/2026-10-22-claude-subagents.md#the-probe) saw on Claude Code 2.1.286.

| Hook | Event |
|---|---|
| any hook with `agent_id` and an empty `agent_type` | nothing: Claude Code's own agents (prompt suggestions, `/btw`) |
| `SubagentStart` | `SubagentStarted`: its `agent_id`, `agent_type`, and the session's `transcript_path` |
| `PostToolUse`, `PostToolUseFailure` with `agent_id` | `SubagentWorking`: never the parent's turn, also on `is_interrupt` |
| `PermissionRequest` with `agent_id` | `SubagentNeedsYou`, "Allow *tool*?" as the parent's own |
| `SubagentStop` | `SubagentStopped`; `reports` when its own `background_tasks` still lists it (it ran in the background, and its report comes as a turn) |
| `Stop` whose `background_tasks` lists a `subagent` | `AwaitsSubagents`, with their ids (a command left running isn't one) |
| `Stop` that lists none | `Stopped`, as before |
| `UserPromptSubmit` whose prompt starts with `<task-notification>` | `Reports`, with the ids of its `<task-id>`s: Claude Code took reports as a prompt of its own |

Any other hook with an `agent_id` says nothing.

## The parent's turn

Templum keeps, on the parent's pane (`Pane::subagents`), the subagents it heard start, each at work or **reported**:
done in the background, its report not yet taken.

- `SubagentStarted` adds one at work (a resumed one is at work again).
- `SubagentStopped` takes it off, or marks it reported when `reports` and it is in the list: one never heard to start
  (its session started again since, `/clear`, `/resume`) has no report anyone waits for.
- `Reports` takes off every one it names, at work or reported: its report is in (one stopped from Claude Code's panel
  may say nothing else). It starts a turn that isn't a prompt anyone typed: a task waiting for its Enter isn't taken
  by it.
- `AwaitsSubagents` keeps only the reported ones and those it names, which are at work.
- `Stopped` with an answer (Claude Code's `Stop`, which named none at work) keeps only the reported ones.
- A new session (`Started`) or the agent leaving forgets them all.

A turn's end (`Stopped`, `AwaitsSubagents`) while any is left isn't an answer: the agent stays Working (or waiting
for its person, when a subagent's dialog is open), nothing waiting on its turn is answered, and its task stays open;
its message is kept (`Pane::answer`). The turn a task was queued behind ends, as before, and the next turn's end is the
task's. Otherwise the turn's end is as before ([agent-protocol.md](agent-protocol.md#3-what-ends-a-task)). This covers
the race the probe saw: a subagent that stops while the turn taking another's report runs leaves its report to come,
so that turn's `Stop` (no subagent at work, an interim message) answers nothing yet.

A task taken while subagents were already there (an earlier task's, the person's) keeps their ids
(`Handed::subagents`): their reports aren't its own, so a turn's end with only those left answers it at once, while
the agent still awaits them (its dot, `list_blocks`).

A report may also never come as a turn of its own: Claude Code hands a background task's report to the turn under way
at its next tool's end (CHANGELOG 2.1.234), a nested subagent reports to the subagent that started it (2.1.259), and
one stopped from Claude Code's panel or with `TaskStop` reports nothing. So a turn's end that leaves only reported ones
(none at work) **settles** 5 seconds later (`GRACE`, `Pane::settles`) unless a turn starts first (`Submitted`,
`Reports`, the parent's own `Working` or dialog), as the report's turn does at once in the probe's race: the reported
ones are forgotten, and that end answers as a `Stopped` with its message would (the task ends with it, the agent is
at rest). Claude Code's `idle_prompt`, which from 2.1.288 doesn't come while background agents run, settles at once,
with the message of the turn that awaited them; a turn under way that ends without an answer (an error, an Esc)
answers so.

Between that turn's end and the next turn (`Pane::awaits_subagents`):

- no turn is under way, so a task typed then starts one at once instead of being queued behind one;
- its screen isn't looked at ([An agent's state](connections.md#an-agents-state)): its prompt sits still, and its
  subagents' hooks tell. Claude Code's `idle_prompt` meanwhile (a `Stopped` without an answer) ends nothing while
  one is at work (Claude Code before 2.1.288 sent it while they ran).

The dialog a subagent opens (`SubagentNeedsYou`) is remembered with its id (`Pane::asked_by`): only that subagent's
`SubagentWorking` or `SubagentStopped`, or that subagent leaving the list without a word of its own (its report taken,
a turn's end that no longer names it), ends the wait, never another's tool nor the parent's own `Working`; the
parent's own dialog is ended by its own `Working`, as before.

## The follower

`templum-agent`'s `follow.rs` (`Follower`, UI-free) reads the transcript from where it last stopped, a whole line at a
time (a half-written one waits), and turns what is new into the text the block shows; the file may not be there yet
when the block opens. It reads Claude Code's content blocks with `claude::parse`, the Templum Agent's reader of the
same blocks, and the task, thoughts and turn's end itself. Every text it takes from the transcript loses its control
characters but new lines and tabs, so an escape a tool's output holds can't drive the terminal (its title, its
clipboard). It opens the file as any reader, so Claude Code, which appends to it, is never kept from writing, on
Windows and macOS too.

The workspace (`workspace/subagents.rs`) builds the transcript's path from the session's `transcript_path` and the
subagent's id (`<transcript without .jsonl>/subagents/agent-<id>.jsonl`, documented), and opens no block for an id
that isn't letters, digits, `-` and `_`. The block's terminal runs nothing (`Terminal::viewer` in `templum-terminal`):
no process, no pseudoconsole, its folder its parent's. Every 250 ms the workspace runs the follower's read on the
background executor and feeds what is new to that terminal (`Terminal::feed`, with `\r\n` for its new lines, as a
program's output: its watches hear it); the reads stop once the block is gone. Before card 127's review the block ran
`templum follow`, Templum's own program without a window, which on Windows' release build (a GUI program, which
ConPTY doesn't give the block's console) would have shown nothing and outlived its block.

## Testing

- `hooks.rs`: `claude_codes_subagents_are_told_apart_from_its_own_turn`, on the probe's shapes.
- `crates/templum/src/workspace/tests/subagents.rs`, each hook input turned into its event as `templum hook claude`
  does: a task answered by the turn after the last report, the race included, an idle prompt ending nothing; a report
  taken inside a turn, settled by the grace or the idle prompt; a turn an Esc ended with reports left; an unknown
  subagent's stop; a task not held by subagents started before it; a report that doesn't take a task in; a subagent's
  dialog that only its own tool ends, or its leaving; a subagent's tool that doesn't take a queued task into the turn;
  one gone without a word that doesn't keep its parent busy; a still screen that neither ends nor sticks a parent
  awaiting its subagents; a block opened beside its parent, wired, overlapping nothing, outside Ctrl+Z and outside the
  saved canvas; one finished that closes a minute later, and not while read (on screen only); its dot for its dialog;
  its blocks closed with its parent and finished with its session; keys that don't reach it; what it shows of a
  transcript as it grows, with no process, and the tools that refuse it.
- `follow.rs`: a synthetic transcript, written in pieces, read as its task, its tools and its report, its title
  from its task or its `meta.json`; escapes in a tool's output never reach the terminal.
- `templum-terminal`'s `terminal.rs`: a viewer shows what it is fed, its title and watches included, and takes no
  input.
- **Not by hand:** the block on the canvas wasn't looked at with a real Claude Code. The workspace tests cover its
  placing, dot, closing, keys, refusals and what it shows of a synthetic transcript appended in a temporary folder;
  its look is a terminal's.

## Risks

- **Many subagents** are as many blocks down one column (Claude Code runs up to 20 at once). Each costs a file read
  four times a second while it lives, off the UI thread, and a terminal's grid (no process); off screen it isn't
  painted.

- **The report's prompt is Claude Code's own format** (`<task-notification>`, `<task-id>`), observed, not
  documented. If it changes, a report's turn is taken for a prompt anyone typed, and reported subagents are settled
  only by the grace or `idle_prompt`: in the probe's race a task would then end one turn early, with the interim
  message. The others are settled by `Stop`'s `background_tasks`, which is documented.
- **The grace is a guess.** A report Claude Code takes more than 5 seconds after a turn's end to start its turn for
  ends the task with that turn's interim message; in the probe the report's turn started at once.
- **A foreground subagent** (fork mode off) is inferred not to be in `background_tasks` as it stops, so its stop
  takes it off at once. Not probed.
