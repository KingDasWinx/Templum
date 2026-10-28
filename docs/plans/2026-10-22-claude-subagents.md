# Claude Code's subagents on the canvas: a feasibility study

- **Date:** 2026-10-22
- **Status:** Study (kanban card 127, "Subagentes do Claude"). C1 and A1 are built, with the person's answers to the
  open questions: [docs/specs/claude-subagents.md](../specs/claude-subagents.md).
- **Card:** Claude Code has subagents of its own, created inside Claude. Could each one be seen on the canvas as if it
  were another agent, even though it is Claude Code's own?
- **Builds on:** [connections.md](../specs/connections.md) (hooks and agent state, spawning, delegating),
  [agent-protocol.md](../specs/agent-protocol.md) (what ends a task), [agent-tools.md](../specs/agent-tools.md) (roles
  and forks), [templum-agent.md](../specs/templum-agent.md) (the chat that already reads Claude Code's messages).

## Sources

Everything below was checked on 2026-10-22, against Claude Code **2.1.286** (installed) and the public repository's
CHANGELOG up to **2.1.301** (`/home/winx/Projects/claude-code` at 03b5892).

- **Official docs** (code.claude.com/docs/en, fetched as Markdown): `sub-agents` (foreground and background, nesting,
  the concurrent limit, resuming, forks, where transcripts are kept), `hooks` (common input fields, `SubagentStart`,
  `SubagentStop`, hooks inside subagents), `agent-teams` (teammates, split panes), `cross-session-messaging` (the
  session's inbox socket), `agent-view` (background sessions, `claude agents --json`).
- **Context7** (`/pjbrito/claude-code-docs`, an archive of Anthropic's docs): the Agent SDK's `SubagentStartHookInput`
  and `SubagentStopHookInput`, and its session storage, which names the `subagents/agent-<id>` subpath.
- **Claude Code's public repository:** its CHANGELOG, and the type declarations its mods are written against
  (`mods/types/claude-code.d.ts`: `BaseHookInput`, `SubagentStartHookInput`, `SubagentStopHookInput`,
  `BackgroundTaskSummary`, the `Agent` tool's input). The repository is "© Anthropic PBC. All rights reserved": it was
  read to learn formats, and nothing from it may be copied into Templum.
- **The installed `claude`:** `claude --help` (`--agent`, `--agents`, `--bg`, `--forward-subagent-text`, `agents`).
- **Files on this machine, read-only:** the layout and the field names of a few subagent transcripts under
  `~/.claude/projects/`. No content was copied; the tests of any feature write their own synthetic lines.

Each finding says whether it is **documented**, **observed** (seen in files or payloads, not promised anywhere) or
**inferred** (follows from the two, not seen). The [spike](#step-0-a-spike-before-any-code) checks the inferred ones.

## In short

- **Possible:** seeing every subagent a Claude Code in a Templum terminal starts, live, in a block of its own beside
  its parent: its task, its tool calls, whether it waits for a permission, and its final report. Claude Code tells
  Templum when one starts and stops (two hooks), every tool hook names the subagent it ran in, and each subagent writes
  its own transcript to a documented place.
- **Not possible:** typing into a subagent from Templum, answering its permission dialog from its block, or stopping
  it from there. A subagent runs inside its parent's `claude` process and has no terminal of its own; only Claude
  Code's own screen (its subagent panel) and the parent Claude (`SendMessage`, `TaskStop`) reach it. An indirect way to
  steer one exists (a message to the parent session's inbox socket, which the parent Claude passes on), but it is a
  message from "another session", it can't approve anything, and its wire format is undocumented.
- **Already there:** the other way round. Templum's agents can open real, interactive agents as blocks instead of
  hidden subagents (`spawn_agent` with `role` and `fork`, card 35), and Templum's server already tells Claude Code to
  prefer them.
- **A side effect found:** since Claude Code runs subagents in the background by default, a Claude Code that starts
  some ends its turn while they work. Templum then shows it at rest and ends a task handed to it with its interim
  message. The subagent hooks fix that too ([What Templum does with them today](#what-templum-does-with-them-today)).

## How Claude Code's subagents work

### What a subagent is

- **Documented.** A subagent is a worker Claude starts with its `Agent` tool (the CHANGELOG calls it the Task tool in
  older releases). It runs inside the same `claude` process, in its own context window, with its own system prompt,
  tools and permissions, and returns only its final report to its caller. Its requests count toward the same usage
  limits as the main conversation.
- **Kinds.** Built-in (`general-purpose`, `Explore`, `Plan`), custom ones defined in Markdown files with front matter
  (`.claude/agents/*.md` in a project, `~/.claude/agents/` for the person, plugins, managed settings) or as JSON with
  `--agents`, and **forks** (`subagent_type: "fork"`, or the person's `/subtask`), which inherit the whole conversation
  and its prompt cache. The `Agent` tool takes `description`, `prompt`, `subagent_type`, `model`,
  `run_in_background`, `name` and `isolation` (`worktree` or `remote`).
- **Background by default.** In an interactive session a subagent Claude spawns runs in the background, and its report
  reaches Claude later as a completion notification, in a new turn. Fork mode is on by default (interactive from
  2.1.232, everywhere from 2.1.302), and while it is on Claude can't wait for a subagent there.
- **Many and nested.** Up to 20 run at once by default (`CLAUDE_CODE_MAX_CONCURRENT_SUBAGENTS`); a subagent may start
  its own, three layers deep by default (`CLAUDE_CODE_MAX_SUBAGENT_SPAWN_DEPTH`). There is no limit on how many a
  session starts in all.
- **Resumable.** A finished subagent keeps an id; Claude resumes it with `SendMessage` (a new run under the same id),
  `Explore` and `Plan` excepted (one-shot, no id).
- **Permissions.** A background subagent's permission prompts show in the main session's screen, naming the subagent.
  An answer that lasts for the session applies to the whole session.

### Where it writes

- **Documented:** each subagent's transcript is `~/.claude/projects/<project>/<sessionId>/subagents/agent-<agentId>.jsonl`
  (the sub-agents page, "Resume subagents"; the Agent SDK's session storage names the same subpath). It lives through
  the main conversation's compaction and is deleted after `cleanupPeriodDays` (30 days by default). `~/.claude` is
  `CLAUDE_CONFIG_DIR` when set, which Templum's accounts use.
- **Observed:** beside it, `agent-<agentId>.meta.json` holds `agentType`, `description`, `toolUseId` (the id of the
  parent's `Agent` call), `spawnDepth` and two fields about the request. Each line of the `.jsonl` has the shape of
  the main transcript's lines, with `isSidechain: true`, the subagent's `agentId` and the parent's `sessionId`; the
  first is a `user` line whose content is the prompt, then `assistant` lines with `thinking`, `text` and `tool_use`
  blocks, `user` lines with `tool_result` blocks, and `attachment` lines. These are the same content blocks
  `templum-agent`'s `claude::parse` already reads from Claude Code's stream.
- **Observed:** in the parent's transcript, the `Agent` call is a `tool_use` whose input is the tool's parameters,
  and the result of a background one carries `toolUseResult` with `agentId`, `status: "async_launched"`,
  `resolvedModel` and an `outputFile`; the report comes later as a queued `<task-notification>` message.
- **Documented caveat:** transcripts are written asynchronously and may lag the conversation; for a final answer the
  hooks give `last_assistant_message` instead.

### The hooks around it

All documented on the hooks page unless marked.

| Hook | When | What it carries beyond the common fields |
|---|---|---|
| `SubagentStart` | Claude spawns a subagent, Claude resumes one, and each time an in-process teammate handles a message | `agent_id`, `agent_type`. **No** prompt, description or transcript path |
| `SubagentStop` | A subagent finishes; also Claude Code's internal agents (prompt suggestions, `/btw`), with an empty `agent_type` | `agent_id`, `agent_type`, `agent_transcript_path`, `last_assistant_message`, `stop_hook_active`, `background_tasks` |
| Every tool hook (`PreToolUse`, `PostToolUse`, `PostToolUseFailure`…) fired inside a subagent | as in the main conversation | `agent_id` and `agent_type` (absent on the main loop) |
| `Stop` | the main turn ends | `background_tasks`: the work still in flight, each with `type` (`subagent`, `shell`…), `status`, `description`, `agent_type`; and `session_crons` |
| `PreToolUse`/`PostToolUse` on the `Agent` tool, in the parent | the call starts and returns | the call's `tool_input` (`description`, `prompt`, `subagent_type`…) and its `tool_use_id`; **inferred** that `tool_response` carries the `agentId` the transcript shows |

The common fields (`session_id`, `transcript_path` of the main session, `cwd`, `prompt_id`, `permission_mode`)
come with each. Hooks from settings files and plugins run inside subagents too, so the `--settings` hooks Templum
gives Claude Code already hear every subagent's tools.

Two details for the spike: the docs' example `agent_id` is `agent-abc123` for `SubagentStart` and `def456` (with
`agent-def456.jsonl`) for `SubagentStop`, while the transcripts name it without the prefix; and whether a background
subagent's `PermissionRequest` carries its `agent_id` isn't said (the common fields say tool-context hooks do).

### Interacting with one

- **In Claude Code's own screen (documented):** the subagent panel under the prompt lists running subagents (a tree
  for nested ones); `↑`/`↓` and Enter open one's transcript, and what the person then types goes to that subagent
  (Ctrl+Enter sends it at once, from 2.1.286); `x` stops it. `/tasks` lists them and opens a finished one's transcript
  for 30 seconds.
- **From Claude:** `SendMessage` (by id or name) resumes or steers a subagent; `TaskStop` stops it.
- **From outside, there is no documented way to reach a subagent.** The nearest is the session's **inbox socket**
  (cross-session messaging, on by default from 2.1.224 on macOS and Linux, 2.1.234 on Windows): its path is exported
  to hooks and Bash as `CLAUDE_CODE_MESSAGING_SOCKET`, with `CLAUDE_CODE_MESSAGING_TOKEN`, and a script may post a
  message into the session. It reaches the main conversation, never a subagent; the receiving Claude is told it comes
  from another session, so it can't approve a permission or change settings; a session that bypasses permissions holds
  it for its person; and the docs give only the first, authentication line of the wire format.

### Neighbours that look alike

- **Agent teams** (experimental, `CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS=1`): teammates are separate Claude Code
  instances with a shared task list and mailboxes. In split-pane mode Claude Code opens each in a tmux or iTerm2 pane
  (not in VS Code's terminal, Windows Terminal or Ghostty); in-process mode, the default, shows them in the same panel
  as subagents. With teams on, a subagent Claude names becomes a teammate.
- **Background sessions** (agent view, research preview): `claude --bg` starts a whole session under a supervisor,
  `claude attach <id>` opens it in a terminal, and `claude agents --json` is the supported way to read their state
  (`working`, `blocked`, `done`…). Sessions, not subagents.
- **Mods** (early access): plugins whose TypeScript hooks hear the engine's events, a subagent's spawn and turns among
  them (`agent.spawn`, `turn.start` and `turn.complete` with an `agentId`). They load only where function hooks are
  enabled, and their API "may change between releases without notice".
- **`--forward-subagent-text`:** puts subagents' text and thinking into `-p`'s stream-json. Only the Templum Agent runs
  Claude Code that way, and it has no `Agent` tool (`--tools Read,Glob,Grep`), so it never starts subagents.

### What changes between versions

The CHANGELOG moves subagents' defaults often: nesting went from five layers to one to three between 2.1.172 and
2.1.219; background became the default; fork mode came on for the terminal in 2.1.232 and everywhere in 2.1.302;
`agent_id` in hook events, `SubagentStart`, `agent_transcript_path`, `last_assistant_message` and `background_tasks`
each arrived in a release of their own. Templum should rely on the hooks' fields and the documented transcript path,
treat `meta.json` as optional, and never on a default (foreground or background, a depth).

### The probe

Run on 2026-10-22 for the build (card 127), on Claude Code **2.1.286**: `claude -p` on haiku, in a scratch folder of
two small files, with every `TEMPLUM_*` and `CLAUDE_CODE_*` variable unset and Templum's `bin` off `PATH`,
`--setting-sources local` (none of the person's hooks or plugins), `--strict-mcp-config` with an empty config, and a
`--settings` file whose hook logged its stdin for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
`PostToolUseFailure`, `PermissionRequest`, `Notification`, `Stop`, `StopFailure`, `SubagentStart`, `SubagentStop`
and `SessionEnd`. One run: two `Agent` calls in one message, an `Explore` (no `run_in_background`) and a
`general-purpose` with `run_in_background: true` asked to `touch` a file. Shapes only, as seen (**observed**):

- **Both ran in the background**, `-p` and the `Explore` without the flag included: `PostToolUse` of `Agent` came at
  once with `tool_response` `{isAsync: true, status: "async_launched", agentId, description, prompt, resolvedModel,
  outputFile, canReadOutputFile}`. Per call: `PreToolUse` (`Agent`), then `SubagentStart`, then that `PostToolUse`.
- **`SubagentStart`**: the common fields (`session_id`, `transcript_path` of the main session, `cwd`, `prompt_id`)
  and `agent_id`, `agent_type`. The id is `a` and 16 hex digits, with no `agent-` prefix (the file name adds it).
- **Tool hooks inside a subagent** (`PreToolUse`, `PostToolUse`) carry `agent_id` and `agent_type`; the main loop's
  carry neither.
- **A background subagent's `PermissionRequest`** carries `agent_id` and `agent_type`, `tool_name`, `tool_input`,
  `permission_suggestions`, and **no** `tool_use_id`: a wait is told apart by the subagent, not by the call.
- **`SubagentStop`**: `agent_id`, `agent_type`, `agent_transcript_path`, `last_assistant_message`,
  `stop_hook_active`, `session_crons`, and `background_tasks`, which **still lists the stopping subagent** as
  `running`. `agent_transcript_path` is the main `transcript_path` without `.jsonl`, then
  `/subagents/agent-<agent_id>.jsonl`, as documented.
- **`Stop`**: `background_tasks` lists each subagent at work as `{id, type: "subagent", status: "running",
  description, agent_type}` (the id is the `agent_id`); `[]` once none is.
- **A report** comes as a turn of its own: a `UserPromptSubmit` whose `prompt` starts with `<task-notification>` and
  holds `<task-id>` (the `agent_id`), `<tool-use-id>`, `<output-file>`, `<status>completed</status>`, `<summary>` and
  a `<note>`; then that turn's `Stop`.
- **The interim answer, confirmed, and a race.** The first `Stop` came with both subagents in `background_tasks` and
  an interim message. Then the second subagent stopped while the turn taking the first's report ran: that turn's `Stop`
  had `background_tasks: []` and the message "Waiting for the background agent to complete…", and the real answer came
  one turn later, with the second report. `background_tasks` alone would have ended a task one turn early.
- **The transcript.** `agent-<id>.meta.json` beside it: `agentType`, `description`, `toolUseId`, `spawnDepth`,
  `requestShape`, `requestNonInteractive`. Its lines: a `user` line whose content is the prompt as a string, then
  `attachment` lines (environment, model, skills, date…), `assistant` lines with one content block each (`thinking`,
  `tool_use` or `text`), `user` lines with `tool_result` blocks; the last `assistant` line has
  `message.stop_reason: "end_turn"`.
- **In `-p`** the `touch` was refused without a dialog: `PermissionRequest` fired, then the subagent tried `Write`
  (another `PermissionRequest`), with no `PostToolUseFailure` between.
- **Not probed:** a foreground subagent (fork mode off), nesting, resuming, `/btw`, the interactive screen's dialogs
  and whether `Notification` names a subagent, and stopping one from Claude Code's panel.

## What Templum does with them today

- **Nothing shows them.** Templum's `--settings` (`templum-usage`'s `claude.rs`) has none of the subagent hooks;
  `hooks::claude` (`crates/templum-mcp/src/hooks.rs`) ignores an event's `agent_id`.
- **A subagent's tool moves its parent's dot.** Each `PostToolUse` inside a subagent becomes the parent's `Working`.
  That is behind two entries of [minors.md](../minors.md#connections): "Parallel subagents can turn a waiting Claude
  Code's dot back to Working" and "A Claude Code subagent's tool end may take a queued task into the wrong turn". Both
  asked whether the payload tells a subagent's tool apart: it does, with `agent_id`.
- **Background subagents end their parent's turn early (inferred, added to minors.md by this study).** A Claude Code
  that starts background subagents ends its turn while they work: its `Stop` comes, its dot goes to rest, and a task
  another agent handed it ends "done" with that turn's last message ("I've started three reviewers…",
  [agent-protocol.md](../specs/agent-protocol.md#3-what-ends-a-task)). The subagents' reports come in a later turn
  that answers nothing. `Stop`'s `background_tasks` says when subagents are still running.
- **Usage is right.** The context Templum reads from a transcript skips `isSidechain` lines, so subagents never count
  as the parent's context.
- **Real agents as blocks already exist.** Templum's MCP server tells Claude Code that its built-in subagents run
  hidden and to use them only when the person asks; `spawn_agent` opens an agent beside the caller, wired, with a
  `role` from `[roles]` (Claude Code's `subagent_type`) or as a `fork` of the caller's conversation, and its end comes
  back as `[task …]`, like a background subagent's notification ([agent-tools.md](../specs/agent-tools.md)).

## Options

Sizes: **S** is a few commits, one task of a plan; **M** is one plan of several tasks (a batch); **L** is several
batches.

### A. Watch each subagent in a block of its own (read-only)

What the card asks.

**What the person sees.** A Claude Code in a Templum terminal starts a subagent; a block opens beside it, in the
column on its right where `spawn_agent` already puts an agent's subagents (a nested one to the right of its own
parent), wired to it, titled with its type and description ("general-purpose · Review the parser changes"). The block
fills as the subagent works: its task, a line per tool call (`Bash  cargo test -p parser`, `Read  src/parse.rs`) with
the first line of its result or ✗ for an error, "thinking…", its text, and its final report. Its dot is the accent
while it works, yellow while a permission it asked waits in the parent's screen ("Waiting for you: answer in Claude
Code · api"), and nothing once done; its title says "done in 1 m 12 s" or "failed". It stays, done, until the person
closes it. A resumed subagent's block works again. Typing in it does nothing but say where to type.

**What Templum needs, either way:**

1. `templum-usage` (`claude.rs`): `SubagentStart` and `SubagentStop` join the hooks of Claude Code's settings file,
   so every Claude Code Templum opens or that is typed in its terminals has them.
2. `templum-mcp` (`hooks.rs`): the two hooks become events (the subagent's id and type; at its stop its transcript and
   last message, and the main transcript for its start, where its file is found); `Working`, `NeedsYou` and `Stopped`
   carry the `agent_id` they ran in, and `Stopped` the number of subagents `background_tasks` still lists.
3. `crates/templum` (`workspace/bus.rs`, `spawn.rs`'s placement): an event with an `agent_id` changes only that
   subagent's block, never the parent's dot; a `SubagentStart` with a type opens the block (an empty type is one of
   Claude Code's internal agents: ignored) or finds the one already open for that id; a `SubagentStop` marks it done.
4. Reading the transcript: its path is the main `transcript_path` without `.jsonl`, then
   `subagents/agent-<id>.jsonl` (until `SubagentStop` names it); it is read from where the last read stopped, a
   quarter of a second apart, only while its block is on screen or not yet done. The line format is read in a library
   crate, from `templum-agent`'s `claude::parse`, which already turns these content blocks into text, tool calls and
   tool results.

**Two ways to draw it:**

- **A1, a terminal block that prints it (M).** The block is an ordinary terminal running a new verb,
  `templum follow <transcript>`, which prints each line as above, in the terminal's colors, and ends when the
  subagent's stop is in the file (or its parent's session ends). Zoom, far-zoom silhouettes, selection, copy, scrolling,
  wires and closing come free; no new block kind. The verb is UI-free, beside the others in `templum-mcp`'s `cli.rs`
  (or `main.rs`), tested headlessly on a synthetic file. Its weak points: it looks like a terminal, not a chat; a
  restart restores a terminal as a shell, so subagent blocks are better not saved (or reopened on the finished file:
  [Open questions](#open-questions-for-the-person) 4).
- **A2, a block kind of its own (L).** A `Body::Subagent` that keeps a `templum_agent::Conversation` and draws its rows
  as the Templum Agent's chat does (`chat/rows.rs`), but at the canvas's zoom, with a level of detail far away, folded
  tool results, a `session::Item` and a menu arm. It reads like the chat the person already knows. The chat is drawn as
  window chrome at a fixed scale today, so drawing it at every zoom is new painting work, with `--bench-mixed` and
  `--bench-far` to run.

**Possible vs not.** Watching: yes, live to within the transcript's lag. Input, answering a dialog, stopping: no (see
[Interacting with one](#interacting-with-one)); the block points at the parent's screen. A Claude Code opened outside
Templum, a background session of agent view, or the Templum Agent: no blocks.

**Risks.** The transcript's line format is undocumented, though Templum already reads the main transcript's
(`templum-usage`, a task's end) and the path is documented; `meta.json` is neither, so the description is taken from
the parent's `Agent` call when it is missing. Twenty subagents at once are twenty blocks: placement must stay tidy
(the column), and each reader costs a small file read four times a second while it runs; off-screen blocks don't
paint. Privacy: the block shows what Claude Code's own screen shows, on the person's machine; Templum saves no
transcript content, only, at most, its path. Windows and macOS: only paths (the hook gives them; `CLAUDE_CONFIG_DIR`
is honored for free by using them), and a file another process appends is readable on all three.

### B. Real Templum agents instead of Claude's subagents

**What the person sees.** Each piece of delegated work is a full agent block: a `claude` in its own terminal, with its
dot, usage chip, model and effort, which the person can type into, answer, stop and resume. It is what `spawn_agent`
already gives; what is left:

- **B1, Claude Code's agent definitions as roles (S).** `spawn_agent(role: "code-reviewer")` with no
  `[roles.code-reviewer]` in `config.toml` opens `claude --agent code-reviewer`, which runs the whole session as that
  `.claude/agents/` definition (its prompt, tools, model). The person's subagent files then work for both.
- **B2, Claude Code without its `Agent` tool, by choice (S).** A setting that adds `--disallowedTools Agent` to the
  Claude Code Templum opens, so every delegation goes through `spawn_agent` and the canvas. Off by default.

**Possible vs not.** Fully interactive and visible, but these aren't Claude's own subagents: each is a whole session
that loads its system prompt, `CLAUDE.md` and MCP servers and keeps its own cache, so it costs more tokens and seconds
than a subagent (a fork subagent shares its parent's cache); its permission prompts are its own, not the parent's;
and with B2 Claude also loses its quick built-in `Explore` and `Plan`.

**Risks.** Token cost and clutter for small lookups; `--agent` follows Claude Code's definition format, which it owns.

### C. Other possibilities

- **C1, subagent-aware hooks without new blocks (S).** Steps 1 and 2 of A, and in `bus.rs`: a subagent's tool never
  moves its parent's dot (fixes both minors above); a turn that ends with subagents still in `background_tasks` keeps
  the parent Working ("Working: 2 subagents") and doesn't end a task handed to it, as a turn that ends while an agent
  waits on its own task already doesn't ([agent-protocol.md §4](../specs/agent-protocol.md#4-a-turn-that-ends-while-the-subagent-waits-on-its-own-task)).
  The person sees a Claude Code that is honestly busy while its subagents work, and a master gets the real answer.
- **C2, a list on the parent instead of blocks (S–M).** A chip on the parent's title ("3 subagents") with a popover:
  each one's type, description, running or done, time; a click opens A's block. Lighter when Claude starts many.
- **C3, steering a subagent through its parent (M, experimental).** Templum learns each session's inbox socket from
  its hooks (`CLAUDE_CODE_MESSAGING_SOCKET`), and text typed into a subagent's block is posted into the parent session
  as "for subagent a3e0…: …", which the parent Claude passes on with `SendMessage`. It costs a turn of the parent,
  counts as another session's message (never the person's approval), and needs the socket's message format, which the
  docs don't give: a spike first. The same socket could one day deliver Templum's tasks without typing into the TUI;
  that belongs to [agent-protocol.md](../specs/agent-protocol.md)'s phase 2, not here.
- **C4, agent teams' split panes as blocks (not recommended).** A `tmux` stand-in in `<data>/bin` (first on every
  terminal's `PATH`) could turn the panes Claude Code asks tmux for into blocks. Teammates are an experimental feature,
  not subagents, and the commands Claude Code sends tmux are an undocumented contract that a release can change.
- **C5, background sessions as blocks (M, a card of its own).** `claude agents --json` (supported) lists them; a block
  running `claude attach <id>` is the real session, interactive. Worth doing for agent view, but it isn't this card.
- **C6, a mod that streams subagent events (not now).** Live and exact, but early access, unstable by its own
  words, and TypeScript loaded into Claude Code.

### Side by side

| Option | The person sees | Can type into it | Size | Main risk |
|---|---|---|---|---|
| A1 | each subagent's activity, in a terminal-style block | no | M | undocumented line format |
| A2 | the same, as chat rows at every zoom | no | L | painting work and benches |
| B1, B2 | real agents as blocks, from Claude's definitions | yes | S each | cost; not Claude's own |
| C1 | the parent busy while its subagents work; right answers | — | S | none new |
| C2 | a list of subagents on the parent | no | S–M | — |
| C3 | typing to a subagent, through its parent | indirectly | M | undocumented socket format |
| C4 | teammates as blocks | yes | M | experimental, undocumented |
| C5 | agent view's sessions as blocks | yes | M | another card |

## Recommendation

### Step 0: a spike, before any code

A throwaway `--settings` with every hook logging its stdin (as batch 7's spikes did), a Claude Code in a Templum
terminal, and these cases: a foreground and a background `general-purpose`, an `Explore`, a fork (`/subtask`), a
nested subagent, a resumed one (`SendMessage`), two in parallel with one asking a permission, and `/btw`. It settles:
the `agent_id`'s exact form in each hook and in the file name; that the transcript path built from `transcript_path`
is the one `SubagentStop` names; when the file appears relative to `SubagentStart`; whether `PermissionRequest` and
`Notification` carry `agent_id` for a background subagent's prompt; what `PostToolUse` of `Agent` returns; and what
`background_tasks` holds while subagents run and after.

### Then, in order

1. **C1 first (S).** It fixes what is wrong today (a busy Claude Code shown at rest, a task ended with an interim
   message, a dot that flickers back to Working) and lays the hooks every other option needs. Small, testable with the
   fake hook events of `workspace/tests/`.
2. **A1 as the MVP (M).** It answers the card: every subagent visible on the canvas, live, beside its parent, at the
   cost of one new command-line verb and the hook events of C1, with no new block kind.
3. **Later, if the person wants more:** A2 if the terminal look isn't enough; B1 (cheap) so the person's
   `.claude/agents/` definitions open as real agents; C3 only after its spike shows the socket's format is usable;
   C5 as its own card.

Not recommended: C4 and C6, for the stability reasons above; B2 only as an opt-in setting.

## How it fits Templum's rules

- **No speculative structure:** C1 adds two hook events and fields that the first change uses; A1 adds one verb and
  no block kind; A2's `Body` arm comes only if it is chosen.
- **Crate boundaries:** reading hooks and transcripts stays in the library crates (`templum-mcp`, `templum-agent`,
  `templum-usage`), tested headlessly on synthetic lines; only placing and drawing blocks is in `crates/templum`.
- **Licensing:** formats come from the official docs and from observing files; Claude Code's repository is
  proprietary ("All rights reserved") and nothing is copied from it, its mods' types included.
- **Tests and benches:** each step ships its failing-first tests; A1 and A2 run `--bench-mixed` with a dozen subagent
  blocks on the canvas, A2 also `--bench-far`.
- **Templum's own words:** the server's instruction "Your built-in subagents run hidden inside you… use them only when
  the person asks" stops being true once A ships for Claude Code ([Open questions](#open-questions-for-the-person) 6).

## Open questions for the person

1. **Every subagent opens a block, or a list on the parent (C2) with a block on a click?** Claude Code may start up to
   20 at once.
2. **A finished subagent's block:** stays (done, greyed) until closed, closes itself after a minute, or one action
   closes all of a parent's finished ones?
3. **The quick built-ins** (`Explore`, `Plan`): blocks too, or only the others?
4. **After a restart:** subagent blocks gone, or back, read-only, on their finished transcripts (kept 30 days)?
5. **The look:** terminal-style text (A1, sooner) is enough, or the chat's look (A2, later and larger)?
6. **What Templum tells Claude Code:** once subagents are visible, keep telling it to avoid them unless asked, or let
   it use them for quick read-only work and keep `spawn_agent` for real work?
7. **B2:** would you want a switch that takes Claude Code's own subagents away so all delegation is on the canvas,
   knowing each delegated job then costs a whole session?
8. **C3:** worth a spike to try typing to a subagent through its parent, knowing the parent Claude relays it?
