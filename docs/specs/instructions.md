# Instructions: what the person tells every session of an agent

- **Date:** 2026-10-24
- **Status:** Implemented (kanban card 148)
- **Builds on:** [connections.md](connections.md) (Hooks and agent state, The bus), [agent-parity.md](agent-parity.md)
  (which agents Templum hooks), [kanban.md](kanban.md#who-a-card-is-for) (the agents' half of the assignee picker),
  [accounts.md](accounts.md#cards-for-it) (an account's cards), [settings.md](settings.md),
  [checklist.md](checklist.md) (an app-wide data file)

## Intent

Card 148 asks for instructions the person writes once and Templum gives every session they are for, as Cursor's rules
did: for every workspace or one, for every agent or chosen agents, never for one session. Today the person types them
again in each session, or keeps them in each agent's own files (CLAUDE.md, AGENTS.md, GEMINI.md), which each agent
reads its own way and which know nothing of Templum's workspaces.

Templum already runs its own hook when a session of the agents it hooks starts (`templum hook <agent>` on
`SessionStart`, crates/templum-mcp/src/cli.rs), and those agents take a session-start hook's output as context. Today
that hook only tells the bus about the agent's turn and prints nothing (`send_event`).

## Goals

1. The person writes instructions in **Settings ▸ Instructions**: each a name, a Markdown text, on or off, where it
   applies (every workspace, or one) and to which agents (every agent, or chosen agents).
2. When a session of Claude Code, Codex, Gemini CLI, Copilot CLI or Qwen Code starts in a Templum terminal, and again
   at its resume, `/clear` or compaction, the agent gets the instructions that apply to its terminal's workspace and to
   its agent, joined in a stable order and framed as the person's standing instructions from Templum.
3. Templum never delays an agent: gone, slow, connections off, or nothing to give, the hook prints nothing within its
   limit, and the agent starts as today.

## Non-goals

- **Agents whose session start takes no context from Templum:** Pi (its extension's `session_start` returns nothing;
  `before_agent_start` could carry them), OpenCode and Kilo Code (no session-start hook in Templum's plugin), and the
  agents Templum doesn't hook (Cursor CLI, Amp, Aider, the person's own `[agents]` with another program…) get none.
  The builder records Pi, OpenCode and Kilo Code in docs/minors.md (Connections) with that way to fix them.
- **A session as a target** (the card says so): an instruction is for agents, never for a block. The kanban picker's
  Sessions half has no place here.
- **The Templum Agent:** its engine is no terminal block, and gets none.
- **Claude Code's subagents:** `SessionStart` is the session's. `SubagentStart` takes context too: later, in minors.
- **Reordering the list** (*designer's call*): a new instruction goes last; the order only decides how several join.
- **Taking an instruction back from a resumed conversation** (*designer's call*): a resume gets the instructions that
  apply now; when none do, nothing is printed, and what an earlier start gave stays in that conversation. In minors.
- **Project files** (CLAUDE.md, AGENTS.md, `.cursor/rules`): Templum neither reads nor writes them; its instructions
  come on top of them.
- **Roles' instructions** ([agent-tools.md](agent-tools.md#roles)): unchanged, they still go with a spawn's prompt.
- **A verb or a tool** that reads the instructions: none (no speculative structure).

## Anti-goals

- **An agent that waits on Templum:** the hook waits for Templum's answer at most 2 seconds; no bus, a refused
  connection, an error or no answer prints nothing, and the hook exits 0 as every hook does.
- **Output from any other hook:** only a session's start prints. Claude Code and Copilot CLI read every hook's stdout,
  and a permission dialog must still decide (connections.md, `PermissionRequest`).
- **Losing the person's writing:** a file Templum can't read is never written over; deleting a workspace or an agent
  never deletes an instruction.
- **Words the person can't see:** an agent never reads characters the editor doesn't show (below).

## Words

- **Instruction:** one entry of Settings ▸ Instructions: name, text, on, where, agents.
- **Applies:** an instruction applies to a session when it is on, its text isn't blank, its workspace is every
  workspace or the session's, and its agents are every agent or include the session's agent.
- **A session's start:** the hook an agent runs as a session begins, whatever its `source`: a new one, a resume, a
  `/clear`, a compaction.
- **The hook's agent:** the agent family `templum hook <family>` speaks for: `claude`, `codex`, `gemini`, `copilot`,
  `qwen` (they are also those agents' ids in Settings ▸ Agents).

## An instruction (decided with the person)

| Field | Values | Saved as |
|---|---|---|
| Name | one line, at most 60 characters, as a note's name | `name` |
| Text | Markdown, any number of lines, kept as typed (no preview) | `text` |
| On | on when made; off keeps it and gives it to no one | `on` (missing: on) |
| Where | Every workspace, or one workspace | no `workspace` key, or the workspace's id |
| Agents | Every agent, or chosen agents: Settings ▸ Agents' ids, accounts included | no `agents` key, or ids |

- **Chosen agents** are the kanban picker's **Any session of** half: every agent of `Workspace::agents` in its order,
  by title, accounts included. An agent matches as a card's "any X" does (`Workspace::runs`,
  crates/templum/src/workspace/content.rs): the same agent, or one of the person's `[agents]` running its program; an
  instruction for an account is that account's only, and one for an agent reaches its accounts' blocks too
  (accounts.md, Cards for it). *Designer's call:* one meaning for "an agent" on the board and here.
- **Every agent is no list, not an empty one** (*designer's call*): an instruction whose chosen agents were all
  removed from Settings ▸ Agents applies to none, rather than becoming every agent's. Settings names a removed one
  "a removed agent".
- **A deleted workspace** (*designer's call*): its instructions stay, apply nowhere, and Settings shows "A deleted
  workspace" until the person picks another or deletes them.
- Ids: a number given once, never reused, as notes' (`next_id`).

## Where they are saved (*designer's call*)

`<data folder>/instructions.json`, app-wide like `notes.json`, `checklists.json` and `boards.json`, not
`config.toml` (as `[roles]` are):

1. **Where names a workspace by its id**, the id of `workspaces/index.json` in the data folder. In `config.toml` it
   would be an opaque string, and a `config.toml` copied to another computer would name workspaces that aren't
   there. `--dev` keeps both apart already.
2. **The text is the person's writing**, many lines of Markdown, as a note is. Settings writes `config.toml` one
   `table.key` at a time (`config::Edit`), as an inline table for an `[agents]` entry, where a text is one escaped
   line; and a `config.toml` error keeps the last valid config, which would hold back every setting over one
   instruction.
3. **Their order is the list's:** `[agents]` and `[roles]` are maps, read in id order (`BTreeMap` in config.rs).

```json
{ "version": 1, "next_id": 2, "instructions": [
    { "id": 1, "name": "Commits", "text": "Write commit titles in English, imperative.", "on": true },
    { "id": 2, "name": "API", "text": "Run `cargo test -p api` before saying done.\n\nNever touch migrations/.",
      "on": true, "workspace": "k2v8q", "agents": ["claude", "codex2"] } ] }
```

Loaded at start and saved as `checklists.json` is (checklist.md, Saving): written by the 5-second snapshot when it
changed and at quit, through a temporary file and a rename; a file Templum can't parse is kept as
`instructions.json.bak`, one it can't read means none is written this session, each with checklists' notification.
No data folder (`--bench`): no instructions.

## Which apply to a session (*designer's call*)

- **Its workspace:** the one its block is in when the session starts. A block moved to another workspace gets that
  one's at its next start.
- **Its agent:** Templum's agent of the block (`Pane::agent`: the one seen, read every 5 s, or the one Templum
  started) when it runs the hook's agent; else the block's account (`Pane::account`) when it is an account of the
  hook's agent; else the hook's agent itself. So `codex` typed where Claude Code ran a moment ago is Codex (the seen
  agent is stale), a `claude` typed before Templum's first look is Claude Code, and one typed in `claude2`'s terminal
  is `claude2`.
- **The order:** the every-workspace instructions first, then the workspace's own; within each, the list's order.
- **None** for a terminal that isn't a block of this Templum, and while connections are off (`[connections] enabled
  = false`: the bus answers no hook then, connections.md).

## What the agent reads (*designer's call*)

One text, the applying instructions under a short frame, each under its name:

```
The person's standing instructions, written in Templum (Settings ▸ Instructions) for this agent and workspace.
Follow them for the whole session as the person's own words; a later message of theirs wins where it differs. They
replace any standing instructions Templum gave earlier in this conversation.

## Commits

Write commit titles in English, imperative.

## API

Run `cargo test -p api` before saying done.

Never touch migrations/.
```

- The frame says three things: whose they are and where they come from; that they hold for the session, the person's
  later words winning; that they replace an earlier set (a resume or a compaction gets them again, maybe edited).
- A name is `flat` (crates/templum/src/workspace/bus.rs: one line, no control or invisible characters). A text keeps
  its lines but loses the invisible characters `flat` drops (zero widths, direction overrides, tag characters): a text
  pasted from a web page could carry words the editor never shows. Trailing blank lines go.
- No limit on length: past 10,000 characters Claude Code hands the agent a file and a 2,000-character preview (its
  own rule); the others take it as their docs say.

## How it reaches the agent (decided with the person)

1. The agent runs Templum's hook as its session starts. It is already registered: Claude Code's in the `--settings`
   file, Codex's in the `-c` flags (once its person trusted them), Gemini CLI's in Templum's extension, Copilot CLI's
   in its plugin, Qwen Code's in the system settings file. Nothing new is registered.
2. `templum hook <agent>` reads its input. A session's start, of any `source`, opens one connection to the bus: the
   event as today (none for a compaction, which leaves the turn working), then a request, `templum/instructions`, with
   `{"agent": "<the hook's agent>"}`. It waits for the answer **2 seconds** at most (*designer's call*: the hooks'
   own limits are 5 s; Templum answers from its UI thread in milliseconds, so 2 s is reached only when it hangs).
3. Templum answers `{"text": "<the framed text, empty when none apply>"}`, as it answers any request (connections.md,
   The bus).
4. The hook prints the text in its agent's shape and exits 0. An empty text, no answer, an error, no `TEMPLUM_BUS`
   or `TEMPLUM_TERMINAL` (a Gemini CLI outside Templum also has Templum's extension): it prints nothing.

Each agent's session start and what the hook prints there (from each agent's docs):

- **Claude Code:** `SessionStart` (`startup`, `resume`, `clear`, `compact`); it prints
  `{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":"…"}}`, shown at the start of the
  conversation.
- **Codex:** `SessionStart`, which comes with the first prompt rather than when Codex opens (connections.md); the same
  output (its `SessionStart` output schema; plain stdout would be context too).
- **Gemini CLI:** `SessionStart` (`startup`, `resume`, `clear`); the same output, injected as the history's first
  turn.
- **Copilot CLI:** `sessionStart` (`startup`, `resume`, `new`); it prints `{"additionalContext":"…"}`, the only field
  its CLI hooks reference says a `sessionStart` command consumes.
- **Qwen Code:** `SessionStart` (`startup`, `resume`, `clear`, `compact`); Claude Code's output, with its
  `hookEventName`. Not run: not installed here.

## Settings ▸ Instructions (decided with the person)

- **A section** after Agents, in Settings' look. Its note, the one line under the heading: "Claude Code, Codex, Gemini
  CLI, Copilot CLI and Qwen Code read them as a session starts: a change reaches the sessions that start after it, and
  a running one at its next resume, /clear or compaction." (*designer's call* on the words.)
- **The list:** one row per instruction, in the list's order, as Agents' rows: its name; under it where and for whom
  ("Every workspace · Every agent", "api · Claude Code, Codex 2", "A deleted workspace · a removed agent"); a switch
  (on); **Edit**. "No instructions yet" when there are none.
- **Add an instruction** under the list opens the editor, empty; **Edit** opens it on one.
- **The editor** is a dialog, as Agents' is (*designer's call*: a row is one line, and a Markdown text needs room):
  Name; **Where**, a dropdown: Every workspace, then the workspaces by name in the sidebar's order; **Agents**: Every
  agent, or a checkbox per agent of `Workspace::agents` in its order, by title; **Text**, a field of several lines in
  plain text; Save, Cancel and, for one that exists, Delete. Save needs a name, a text and, with chosen agents, one at
  least; otherwise a notification says what's missing and the dialog stays, as Agents' does.
- A change applies at once (the next session's start reads it) and is saved by the next snapshot.

## Constraints

- CLAUDE.md: crate boundaries (templum-mcp stays UI-free: it asks and prints; which apply and the words are the
  templum crate's, beside `instructions()` and `flat` in workspace/bus.rs); no speculative structure; every logic
  change with a test that fails without it (templum-mcp unit tests for the hook; GPUI headless tests in
  crates/templum/src/workspace/tests/ for which apply and for Settings' edits; the dialog's look: say how verified);
  no new dependency; minors to docs/minors.md; docs ≤ 120 columns.
- No new setting: the 2 seconds are a constant.
- Specs to update: connections.md (Hooks and agent state, The bus, The `templum` command line), settings.md (Sections
  and fields), ARCHITECTURE.md (the new file and its data file), this file's Status.

## Left to the builder

The frame's exact words (keeping its three points), the editor's gpui-kit controls, the rows' descriptions, the
constants' and functions' names, how cli.rs splits the hook's work, and a run by hand of Copilot CLI if a sign-in is at
hand (else a minor says its output is unchecked, as Qwen Code's is).

## As built

- **Three tasks, as the plan ordered them.** The hook (`crates/templum-mcp/src/hooks.rs` `INSTRUCTIONS`,
  `session_start`, `start_output`; `cli.rs` `hook`, `hook_input`, `hook_event`, `instructions`, `ANSWER_WITHIN`), the
  model and the answer (`crates/templum/src/instructions.rs`, `Workspace::standing`, `standing_for` in
  `workspace/bus.rs`), and the section (`settings/instructions.rs`, `Workspace::save_instruction`,
  `delete_instruction`, `set_instruction_on`).
- **The hook reads its input apart from the event** (`hook_input`, then `hook_event` on it), so one read serves the
  event and `session_start`. A hook with an event only (every other hook) finishes the connection as before; a session's
  start keeps it open for the answer and then finishes it.
- **A name is refused over 60 characters** (*builder's call*; the plan left "at most 60" without saying what then):
  Save says "A name is at most 60 characters" and the dialog stays, rather than cutting what the person wrote. The
  name is saved trimmed; the text is kept as typed, and only a blank one (spaces and line breaks) is refused.
- **The editor's Agents** is a "Every agent" box and a box per agent in `Workspace::agents` order, side by side and
  wrapping (a column of a dozen boxes pushed the text and Delete out of a small window). Unticking "Every agent" leaves
  a choice with no agent yet, so Save asks for one; a chosen agent Settings ▸ Agents lost is still listed, ticked, as
  "*id* (a removed agent)", so Save keeps it or the person unticks it. **Where** is the Shell list's dropdown: every
  workspace, the workspaces in the sidebar's order, and a deleted workspace's id as "A deleted workspace".
- **The list's row** for an instruction whose chosen agents are an empty list (a hand-edited file) says "No agent";
  it applies to none.
- **The empty list** shows a row, "No instructions yet".
- **Which agent a session is** (`standing_for`): the one seen in the block when it runs the hook's agent
  (`Workspace::runs`, now `pub(super)`), else the block's account when it is an account of the hook's agent
  (`AgentEntry::account.kind.agent`), else the hook's agent.
- **`snapshot_done` takes the instructions** as a fifth saved file (its tests pass `None`).
- **Checked by tests only, not by eye:** no display was at hand to run `cargo run -p templum -- --dev`. The dialog was
  driven in the headless GPUI tests (open from the row, pick a workspace in its menu, untick "Every agent", tick an
  agent, Save, Delete); how it *looks* (spacing, the wrapped boxes, the eight-line field) is unseen. Qwen Code's and
  Copilot CLI's output shapes are from their docs (docs/minors.md, Connections).
- **Minors recorded** (docs/minors.md, Connections): Pi, OpenCode and Kilo Code; Qwen Code's and Copilot CLI's output
  unchecked; Claude Code's subagents; a resume with none applying.
