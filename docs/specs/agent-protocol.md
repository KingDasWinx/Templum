# Agent protocol: every task answered through Templum

- **Date:** 2026-10-20
- **Status:** Implemented, phase 1 (kanban card 69, "Comunicação entre agentes"; it covers card 68, "Integração com
  codex zoado", and the part of card 58, "opencode, cline e amp não injetando MCP templum", that isn't one agent's own
  plumbing). Phase 1 was planned whole in
  [docs/plans/2026-10-20-agent-protocol.md](../plans/2026-10-20-agent-protocol.md); phase 2 is outlined there, not
  done. [Calls made while implementing it](#calls-made-while-implementing-it) lists where the code settled what the
  text above left open.
- **Builds on:** [connections.md](connections.md) (Delegating, Hooks and agent state, An agent's state, Every agent
  that can take it), [agent-parity.md](agent-parity.md) (what each agent gets), [agent-tools.md](agent-tools.md),
  [ask-user.md](ask-user.md). Card 30's rule stays: a subagent tells its master only when its task ends, or when it is
  stuck.

## The problem

The person, in card 69: with Claude Code, handing work between agents works; with Codex, OpenCode, Cline, Amp and the
others it doesn't. The subagent finishes and the master isn't told; the master can only read what is on the
subagent's screen; a task doesn't reach the subagent; Templum's MCP server doesn't start; nothing comes back; the
subagent shows a question on its screen that no agent can read or answer. The ask: a rule that **every hand-off goes
through Templum's MCP** (or another sure channel), so that whatever the agent, it gets the task, works on it and
answers, and a subagent is told to send its answer back that way.

## How it works today, agent by agent

A task is always delivered the same way, whatever the agent: Templum types `[from <title> (<id>), task t7] Your task
is in the pasted text: do it.` as a paste, the task as a second paste, and Enter once the screen shows the paste and
stood still a look, 150 ms (an agent that reads a paste as keys takes an Enter among them for a new line), and not
once its person typed into it since (`send_task`, `crates/templum/src/workspace/delegate.rs`). Then it checks the
agent took it: its hooks say so (`Submitted`), or, without hooks, its screen moves. With neither within 3 seconds, a
second Enter goes only where it can't answer a dialog: the first changed nothing on the screen, or the paste went as
keys (Windows' console, a program that asked for no bracketed paste). `delegate` answers once it knows: the task
taken, or why not ("typed but not submitted" when neither Enter changed anything). What differs is what Templum hears
back:

- **An agent whose hooks speak** (`Pane::speaks`): its turn's end (`AgentEvent::Stopped`) carries its whole last
  message, which ends the task (`agent_event`, `crates/templum/src/workspace/bus.rs`); a permission prompt is heard
  (`NeedsYou`) and told to the master with the dialog's screen (Delegating 10).
- **Any other agent:** its screen is looked at once a second (`look_at_agents`,
  `crates/templum/src/workspace/activity.rs`). Still for 5 looks after it moved, the task **ends, as answered**, with
  the last 40 lines of the screen ("Its screen has been still for 5 seconds, and Templum heard no answer…").
- **`reply`** ends a task at once with the text the subagent sends, but nothing tells a subagent to call it: the task's
  lines don't, the server's instructions don't, and only `reply`'s own description does.

Checked on 2026-10-20 on this machine: `which` found Claude Code, Codex 0.154.0, OpenCode 1.19.0, Gemini CLI 0.63.0,
Copilot CLI 1.0.100, Crush 0.98.1, Grok Build 1.0.62, Pi 0.85.1 and Cursor CLI 2026.10.18; not Cline, Amp, Qwen
Code, Kimi Code, Kilo Code, Kiro CLI, Aider, Droid, Auggie, Goose or Mistral Vibe. Run in a scratch folder, nothing
written to an agent's files: `OPENCODE_CONFIG_CONTENT="$TEMPLUM_OPENCODE_CONFIG" opencode debug config` merges
Templum's server and plugin after the person's own, and `opencode mcp list` shows `templum connected`; `codex -c
'mcp_servers.probe.tools.reply.approval_mode="approve"' mcp list` is accepted, and a wrong value is refused
("expected one of `auto`, `prompt`, `writes`, `approve`"); `copilot --help` documents `--allow-tool='MyMCP(tool)'`;
`~/.gemini/extensions/templum` isn't there (the person never clicked Install), so Gemini CLI has neither the server nor
the hooks here; `grok`, `cursor-agent` and `crush` still take a server only from their own files (Cursor's
`--plugin-dir` and `--approve-mcps` aside).

| Agent | Templum's server | Turn's end heard | The answer the master gets | The subagent's dialogs | What goes wrong |
|---|---|---|---|---|---|
| Claude Code | `--mcp-config` (opened; typed: the shell function) | hooks in `--settings`: `Stop` | `last_assistant_message`, whole | `PermissionRequest`, `Notification` | Works. Each Templum tool asks "Allow mcp__templum__…?" until the person allows it |
| Codex | `-c mcp_servers.templum.*` (not under cmd, Windows PowerShell 5.1, WSL) | hooks in `-c` flags, only once the person trusted them ("Hooks need review") | `Stop`'s last message; before the trust, the screen's last 40 lines | `PermissionRequest`; its own `request_user_input` unchecked | Untrusted hooks or the hooks feature off: the screen's tail as its answer (card 68); a spawned Codex's prompt can wait on the trust question and be dropped after a minute |
| Gemini CLI | the extension, only after Settings ▸ Connections ▸ Install, and no stdio server in a folder it doesn't trust | the extension's `AfterAgent` | `prompt_response` | `Notification` `ToolPermission` | Without the extension (as here): no server, no hooks, the screen's tail |
| Copilot CLI | `--additional-mcp-config` | a plugin's `agentStop` | the last `assistant.message` of `events.jsonl` | `notification` `permission_prompt` | It asks the person before every MCP tool call, `reply` included |
| OpenCode | `OPENCODE_CONFIG_CONTENT` (checked above) | the JS plugin's `session.idle` | the session's last assistant text | `permission.asked`, `question.asked` | Its TUI was never checked by hand: the plugin's events, the paste and its Enter (card 58) |
| Kilo Code | `KILO_CONFIG_CONTENT` | the same plugin | the same | the same | From its source only: not installed |
| Qwen Code | `--mcp-config` | hooks in a system settings file Templum moves | `last_assistant_message` | `PermissionRequest` | Not installed; no hooks when its own system file exists |
| Pi | its extension registers the server only on Pi 0.99 and later (0.85.1 here: none) | the extension's `agent_settled` | the last assistant text | `ui_prompt_start` (no permission prompts) | No tools on 0.85.1: only its turn's end |
| Cursor CLI | none (its own files; a plugin with `--plugin-dir` not tried) | none | the screen's tail | none | Screen only |
| Grok Build | none (it reads Claude Code's `~/.claude.json` and a project's `.mcp.json`) | none (hooks in files only) | the screen's tail | none | Screen only |
| Amp | none, though it takes `--mcp-config '<json>'` | none (plugins, installed) | the screen's tail | — (it asks before no tool) | Card 58: no Templum tools; only being handed tasks works |
| Cline | none, though it takes `CLINE_MCP_SETTINGS_PATH` | none, though it takes `--hooks-dir` | the screen's tail | unconfirmed | Card 58: no Templum tools |
| Kiro CLI, Kimi Code, Mistral Vibe, Goose, Droid, Auggie, Crush | none for one session (agent-parity.md, The others) | none | the screen's tail | none heard | Screen only |
| Aider | impossible: no MCP | none | the screen's tail | — | Screen only, and no shell tool to run `templum` |
| The person's own | their own setup | none | the screen's tail | none | Screen only |

**What goes wrong, and why** (the root causes behind card 69's list):

1. **"The subagent finished and the master wasn't told."** A screen that never stays still for 5 seconds (a clock, a
   token counter, a status bar that redraws) never ends its task; one that pauses for 5 seconds while it still works
   (a long think with no spinner) ends it too early. Neither is the agent saying it is done.
2. **"The master can only read the screen."** For every agent without hooks the answer *is* the screen's last 40
   lines: cut, wrapped, and for a full-screen TUI (OpenCode, Crush) only what is drawn now.
3. **"It shows a question nobody can answer."** Without hooks a dialog is a still screen, so the task ends "answered"
   with the dialog as its text, and the master moves on while the subagent waits.
4. **"It doesn't reply."** Nothing tells it to: `reply` is only in the tool's own description.
5. **"Templum's server doesn't start."** For 12 of the 20 agents Templum gives none for one session; for the others it
   can still be missing (connections off, WSL, typed as `command opencode`, Gemini CLI without its extension or in an
   untrusted folder, Pi before 0.99), and nothing tells the master which.
6. **"The message doesn't reach the subagent."** Typing is the only way into an agent's TUI. It works where the screen
   shows the paste and the Enter is confirmed by a hook; it is blind elsewhere. This card keeps typing (there is no
   other way in) and makes every other step not depend on the screen.

## Goal

- Any agent that has Templum's tools answers its task with `reply`, and that answer, whole, is the task's end, whatever
  its screen does. It is told so in every task it gets.
- An agent without Templum's tools can answer the same way from its shell: `templum reply -`.
- The screen never ends the task of an agent that can answer: a still screen only tells the master, once, that the
  subagent went quiet without answering, with its screen, and the task stays open.
- A subagent that waits on a task of its own (it handed one to its own subagent) isn't taken for done when its turn
  ends.
- The master knows which agents have Templum's tools, and why one hasn't.
- `reply` asks no permission where Templum can say so for one session (Claude Code, Codex, Copilot CLI), so the
  protocol adds no dialog to every task.
- All of it is the same for every master: a call that waits gets the answer as its result, any other master gets it
  typed into its terminal (the Templum Agent in its chat), as today.

**Non-goals.** Reading an agent's dialogs off its screen (still guesswork: connections.md, A dialog Templum doesn't
hear). A channel into an agent's TUI other than typing. Giving one more agent its server (phase 2, one spike each).
Pre-allowing any Templum tool other than `reply`.

**Anti-goals.** A Claude Code that works today working worse: its turn's end still answers its task when it doesn't
reply. More notices than card 30 allows: one when a task ends, one when it gets stuck, and now one when it goes quiet
without answering, never more than once per task.

## The protocol

### 1. Every task says how to answer

The first line of a task, a change and a spawn's prompt asks for the answer:

```
[from Claude Code · api (b7k2m), task t7] Your task is in the pasted text: do it, then answer it with reply.
[from Claude Code · api (b7k2m), a change to task t7] The change is in the pasted text: do it, then answer the task with reply.
```

and the pasted text ends with how (`answer_how(7)`, one blank line before it):

```
(When task t7 is done, call Templum's reply tool once, with your whole answer as its text: that ends the task and hands your answer over. Without Templum's tools, run `templum reply -` in your shell, the answer on its standard input.)
```

The server's instructions (`initialize`) say it too, after "A task you hand over without wait…": "A task handed to
you begins [from …, task t7]: when it is done, answer it once with reply, your whole answer as its text. When you need
the person, ask with ask_user, not on your screen."

### 2. `reply` ends the task you are on

- `reply`'s `block` becomes optional. **Without it**, it answers the task the caller is on (`Pane::handed`), to
  whoever handed it: an agent's block, or the Templum Agent, which no block names. The caller's task ends there:
  calls of the asker that wait for it (`delegate` with `wait`, `wait_for_idle`, `wait_for` idle) get the text as
  their result; otherwise the asker hears the task's end as any end is told (Delegating 10: `[task t7 from Codex · api
  (k9x1d): done in 31 s, …] Its answer is in the pasted text.`, the Templum Agent in its chat). With no task: "You are on
  no task: reply answers the task an agent handed you; name a block to write to one."
- **With a block** that is the caller's asker, the same. With another block, as today: that block's calls waiting on
  the caller get the text, else it is typed into it as `[reply from <title> (<id>)] Its reply is in the pasted text.`;
  no task ends.
- The text goes whole: Templum cuts nothing of it. A blank one is refused: "reply needs your answer as text."
- Its description: "Answers the task you are on (it began [from <title> (<id>), task t7]): call it once, when you are
  done, with your whole answer as text. It ends the task, and the agent that handed it gets your text now. With block,
  writes to that connected agent instead: typed into its terminal after a line saying it is from you."
- **The command line:** `templum reply -` is `reply` without a block, its text read from standard input; `templum
  reply <block> -` reads it there too; `templum reply <block> <text>…` stays. `-` is never a block id (five letters and
  digits). Usage: `reply [<block>] <text>|- [--json]`.

### 3. What ends a task

In this order; the first that happens ends it, and nothing after it answers it again.

| What happens | The task |
|---|---|
| The subagent calls `reply` (tool or command line) | ends with its text: **done** |
| Its turn ends, heard by its hooks, with a message, and it has no task of its own handed out (4) | ends with that message: **done** (today's) |
| Its turn ends, heard by its hooks, without a message, and it has no task of its own handed out | ends: **failed**, "Its turn ended without an answer" (today's) |
| Its screen goes still and it has **Templum's tools** (8) and no hooks | stays open: **quiet** (5) |
| Its screen goes still and it has neither tools nor hooks | ends with its screen's last lines, as today: it has no other way to say it is done |
| It didn't take the task, its agent left, its block closed, the asker's `interrupt`, `stop_agent` or `close_block` | ends, as today |
| Its timeout passes | not an end, as today: the asker's waiting call hears it; the task's end still comes |

### 4. A turn that ends while the subagent waits on its own task

A subagent that handed a task of its own (`delegate`, `spawn_agent`'s prompt, to any agent; `Handed::by` is its
terminal) and whose turn ends is waiting for that task, as card 30 asks it to: the turn's end doesn't end its own task.
Templum types that other task's end into it when it comes (Delegating 10), it works on, and its `reply` (or the end of
a turn with nothing handed out any more) ends its task. Its screen going still meanwhile tells nothing either (5).

Two exceptions. A task whose agent went quiet (5) isn't waited for any more: its master was told, reads that agent's
screen and answers its own task with its turn's end, as it would if the sub-task were over; a `delegate` of it to that
agent again (a change), or anything its master writes into it (`send_keys`, `send_to_terminal`, a `reply` to it: its
dialog answered), starts the wait over. And the calls of other agents that wait for the subagent's turn
(`wait_for_idle` on an agent they didn't hand the task) don't wait for its task: the turn's end, or its screen going
still, answers them as it would any turn, with its last message or the still-screen text. The last message only where
its hooks refresh it at each turn's end: an agent without hooks may hold the answer of an earlier task, so for it they
get the still-screen text.

### 5. Quiet: a still screen and no answer

An agent that has Templum's tools but no hooks, on a task, whose screen stays still for 5 looks after it worked (the
same looks as today), went quiet without answering. Once per task (`Handed::quiet`; a change, or anything the asker
writes into it (keys, typed text, a reply), starts it over, and a screen that stays still after the writing tells it
again):

- the asker's calls that wait for the task get, as an error result (it is no answer), "k9x1d hasn't answered task t7:
  its screen has been still for 5 seconds. It may be done without replying, stopped, or asking its person. The task
  stays open, and its reply still ends it: wait_for_idle waits for it, send_keys answers a dialog, delegate to it
  again asks it for its answer, and interrupt ends it. The last lines of its screen:" and its last 40 lines;
- an asker with no call waiting gets the notice `[task t7 from Codex · api (k9x1d) hasn't answered: its screen is
  still] Its screen is in the pasted text.` and the last 20 lines, as a dialog's notice is typed (its level
  Waiting for the Templum Agent);
  the notices about a task come in batches, one message for what comes within 15 seconds of each other
  ([connections.md](connections.md), Delegating 10; [notice-batching.md](notice-batching.md));
- other agents' calls that wait for the subagent's turn (`wait_for_idle` on an agent they didn't hand the task) get
  today's still-screen text: for them, its turn ended.

Not while it has a task of its own handed out (4). The task stays handed over (the dot travels, others are still
refused until its timeout), and ends as 3 says: its reply, its agent leaving, its block closing.

### 6. Questions and dialogs in a subagent

- **Its own permission prompts**, where its hooks hear them: as today, `NeedsYou`, its yellow dot and its alert to
  the person, and once per wait `[task t7 from … waits for its person: Allow Bash?] Its screen is in the pasted
  text.` to its master, which can answer with `send_keys` (the wire is the person's consent).
- **Where no hook hears them:** the screen goes still, so 5's notice (with tools) or the task's end (without) carries
  the dialog as drawn, and the master can still answer it with `send_keys`, `send_to_terminal` or a `reply` to it; with
  tools the task stays open across that answer.
- **Its questions to the person:** the server's instructions ask for `ask_user`, whose request in the inbox
  Templum hears; as card 30 wants, its master isn't told of them.
- **`reply` itself asks nothing** (9).

### 7. The whole answer, and more than the screen

The answer is `reply`'s text, whole, or the hooks' last message, whole; the screen is the answer only for an agent
that has neither tools nor hooks. A waiting call gets it as its result; a notice types it as a paste, as today. Beyond
the answer the master already has `read_terminal` (up to 2000 lines of history and screen) and, in the notice of a
task's end, the transcript the subagent's hooks named (Claude Code, Codex, Copilot CLI). No new tool.

### 8. Templum's tools, checked

- Templum notes which terminals have its tools: an `initialize` from a `mcp` client (the bridge `templum mcp`, which
  every agent's server is) while connections are on records its connection and terminal; `templum/closed` for that
  connection drops it (`Workspace::tool_conns`, `Workspace::has_tools`). A nested `claude -p` an agent's shell runs
  counts for the block while it runs: harmless.
- `list_blocks` adds `, no Templum tools` to an agent's line when its terminal has none.
- `delegate`'s answer to an agent without them adds why, and what ends its task: "Handed to k9x1d as task t7. k9x1d has
  no Templum tools (<why>): it can still answer with `templum reply -` in its shell; else its turn's end or its screen
  going still ends the task." In WSL, where `TEMPLUM_BUS` and `templum` don't reach the shell, and for Aider, which has
  no shell tool, the clause about `templum reply -` is left out.
- **Why** (`no_tools_why`), the first that fits: "connections are off (Settings ▸ Connections ▸ Agents in Templum)";
  "it runs in WSL, where Templum can't give it its server"; Gemini CLI without the extension: "Templum's Gemini CLI
  extension isn't installed (Settings ▸ Connections ▸ Gemini CLI ▸ Install)"; Gemini CLI with it: "Gemini CLI starts no
  server in a folder it doesn't trust"; Pi: "Pi has MCP only from 0.99"; Aider: "Aider has no MCP"; Claude Code,
  Codex, Copilot CLI, OpenCode, Kilo Code, Qwen Code: "its server hasn't connected: it is still starting, its start
  failed, or it was started without Templum's words (command <agent>, a shell without Templum's integration)"; any
  other: "Templum can't give <title> its server for one session: add `templum mcp` to its own MCP servers (Settings ▸
  Connections ▸ Other agents)".

### 9. `reply` asks no permission

Templum lets `reply` run without asking, where it already gives the agent its words for one session, and only
`reply`: it does no more than the turn's end already does (hand the subagent's text to its master), so it gives no
power the agent doesn't have.

| Agent | How |
|---|---|
| Claude Code | `"permissions": { "allow": ["mcp__templum__reply"] }` in the settings file Templum gives with `--settings`, only when that file carries the hooks (the server comes with them) |
| Codex | `-c 'mcp_servers.templum.tools.reply.approval_mode="approve"'`, one more of the server's flags (and `TEMPLUM_CODEX_ARGS`) |
| Copilot CLI | `--allow-tool=templum(reply)`, one more of its words (typed in double quotes, `typed_path`'s rule for `(`) |

Gemini CLI and Qwen Code ask before an MCP tool unless the whole server is trusted, which would let every Templum tool
run unasked: left as a minor. OpenCode and Pi ask nothing before an MCP tool.

## What the master sees

| Master | The answer | A quiet or stuck subagent |
|---|---|---|
| Any agent waiting in a call (`delegate` with `wait`, `wait_for_idle`, `wait_for`; `templum delegate --wait`, `templum wait`) | the call's result | the call's result (quiet: an error result, the task still open) |
| Any agent in a terminal not waiting | typed into its terminal, `[task t7 from …: done …]` | typed, `[task t7 from … hasn't answered …]` or `[… waits for its person …]` |
| The Templum Agent | a notice in its chat, told to its engine | the same |

## Decisions

Made for card 69, the person away; each can be changed.

1. The rule is "**reply ends the task**", taught in every task's lines and in the server's instructions; the agent is
   asked, never forced: an agent that can't reply still ends by the next rules.
2. A hooked turn's end with a message still ends a task (no change for Claude Code), except while the agent waits on a
   task of its own.
3. The screen never ends the task of an agent with Templum's tools; it tells its master once ("quiet") and the task
   stays open. For an agent with neither tools nor hooks the screen stays the end: it has no other.
4. Quiet answers the asker's waiting call (an error result), so no master hangs until the timeout on an agent that
   finished without replying; the task stays open for its reply.
5. `reply` needs no block: the subagent never has to copy its master's id, and the Templum Agent, which no block names,
   is answered the same way. Its end is told in the same words as a turn's end.
6. The command line takes the answer on standard input (`-`): no quoting of a long answer in a shell.
7. "Has Templum's tools" is what Templum sees (an MCP connection from the terminal), not a table of agents: it covers
   the person's own agents and any setup.
8. Pre-allow `reply` only, and only for Claude Code, Codex and Copilot CLI, through the per-session words Templum
   already gives.
9. No new tool: the answer, the transcript in the end's notice and `read_terminal` cover "more than the screen".
10. Typing stays the only way into an agent; no screen reader of dialogs.
11. One more agent's server (Amp, Cline, Cursor CLI, …) and checking OpenCode's TUI by hand are phase 2, each after a
    spike with the agent installed, as card 44 did.

## Per agent, after phase 1

| Agent | Phase 1 gives it | Phase 2 | Left |
|---|---|---|---|
| Claude Code | the task's lines, `reply` unasked, its end unchanged | — | — |
| Codex | the same; with untrusted hooks its reply ends the task, never its screen | its `request_user_input` heard as a dialog | the hooks' trust question at its first start (Codex's own) |
| Copilot CLI | the same, `reply` unasked | — | — |
| OpenCode, Kilo Code | the task's lines and reply | OpenCode's TUI checked by hand: the plugin's events, the paste's Enter, its dialogs | Kilo Code untried |
| Qwen Code, Gemini CLI | the task's lines and reply | — | `reply` asks first (minor); Gemini CLI's extension still needs its Install |
| Pi | the task's lines; `templum reply -` from its shell on 0.85.1 | — | — |
| Amp, Cline | the task's lines, `templum reply -` from their shell, the master told why they have no tools | their server for one session (Amp `--mcp-config`, Cline `CLINE_MCP_SETTINGS_PATH`) and their turn's end (Amp's plugins, Cline's `--hooks-dir`) | — |
| Cursor CLI | the same as Amp | a plugin with its server and hooks (`--plugin-dir`, `--approve-mcps`) | — |
| Grok Build, Kiro CLI, Kimi Code, Vibe, Goose, Droid, Auggie, Crush | the same as Amp | when someone uses them: one spike each (agent-parity.md gap 10) | — |
| Aider | nothing new: screen only | — | impossible: no MCP, no shell tool |

## Testing

No test needs a real agent: CI (Linux, macOS, Windows) has none.

- **Fake agents in the workspace's tests** (`crates/templum/src/workspace/tests/`): a terminal block running `cat`
  (`agent_cat`) that Templum takes for an agent (`seen.agent`, `found`), whose MCP client and hooks the test plays
  through `Workspace::serve` with that terminal's id: `initialize` from an `mcp` hello (it has tools), `tools/call`
  (`reply`, `delegate`, `wait_for_idle`), `templum/event` (its turns), `templum/closed` (its server gone); its screen
  driven by writing into its terminal (`shows`, `ticks`, `screen_with`). The PTY-bound ones are `#[cfg(unix)]`, as
  their neighbors are; the rest run everywhere.
- **The command line** (`crates/templum-mcp/src/cli.rs`): `run_with(&["reply", "-"], …, "the answer")` against the
  test bus, as `plan -` is tested.
- **The words** (`templum-usage`, `templum-mcp`): Claude Code's settings file, Codex's flags and Copilot CLI's words
  carry `reply`'s permission; typed into each shell, Copilot's in double quotes.
- **What each test proves** is listed with its task in the plan: every one fails without its change.
- **By hand, once, where installed** (not in CI): Codex with the new flag still trusted its hooks; a Copilot CLI
  signed in calls `reply` unasked; an OpenCode subagent answers with `reply`.

## Calls made while implementing it

- **A still screen with no task behind it ends the turn's waits as before.** Quiet (5) is for an agent with Templum's
  tools, no hooks **and a task**; one with tools and no task whose screen goes still still answers the calls that wait
  on its turn (`wait_for_idle` from another agent) with the still-screen text, or they would hang until their timeout.
- **Quiet answers other agents' waits at every still screen**, and the asker's call or notice once per task (a change
  starts it over). An agent that waits for a task of its own is left alone, with or without Templum's tools.
- **Anything the asker writes into a quiet agent starts quiet over.** The master answers a dialog (6) with `send_keys`,
  `send_to_terminal` (also the Templum Agent's, whose asker is `ASKER`) or a `reply` to the agent that types the text in
  (not one that a waiting call takes). Each clears `quiet`, sets the agent to work again and starts its looks over
  (`Workspace::asker_wrote`, one helper all of them call): the master's turn ending with "answered, waiting" no longer
  ends its own task, and a screen still after the writing tells it once more. Another agent's writing changes nothing.
- **`wait_for_idle` from an agent that isn't the asker** answers at once when the subagent's turn already ended (idle)
  and its task stays open for one of its own or because it went quiet, as it would have at that turn's end (4). With
  its last message only for an agent whose hooks speak: one without them holds the answer of its last task, maybe an
  earlier one, and its still screen answers instead.
- **"Handed out" means since its own task was handed to it** (4): a task it handed out before, which nobody ended, is
  not what this one waits for, or a forgotten sub-task would keep every later task of that agent open.
- **`reply` to a block that is not the caller's asker** still sets the replier's last answer, as before; it ends no
  task.
- **`reply` to the caller's own block** is refused with "This block is yours: leave block out to answer the task you
  are on."
- **`no_tools_why`:** `<agent>` in the spec's wording is the agent's command (`command codex`); a Gemini CLI in an
  account is looked up in that account's folder, as the install notice does.
- **A task's text is longer than the screen now** (the lines plus how to answer): the tests that looked for a task's
  first line on the visible screen read the terminal's history (`history_with`).
- **The Templum Agent's own `delegate` says** "what it sends with its reply tool (Templum asks it to)": it has no
  `reply` of its own, its subagents do.
- **By hand, once:** Codex lists Templum's server with the new flag and refuses a wrong value (scratch `CODEX_HOME`);
  Copilot CLI's `--help` documents the `MyMCP(tool)` pattern the word uses. Not checked, as the Risks say: that Codex
  keeps trusting Templum's hooks with the extra flag, and a signed-in Copilot CLI calling `reply` unasked.

## Considered and left out

- **A new `finish_task` or `report` tool.** `reply` already answers the asker and ends the task; a second word for it
  would split the agents between two.
- **Ending only on `reply`**, never on a hooked turn's end: a Claude Code that forgets to reply would hang its master;
  its last message is its whole answer anyway.
- **Templum typing a reminder into a quiet subagent:** a dialog no hook hears may be on its screen, and a paste would
  answer it. The master, who sees the screen, asks with `delegate` (a change to its own task).
- **Writing long answers to a file** and typing its path: a waiting master gets the answer as its call's result,
  whatever its length, and a notice is a paste, which the agents' input boxes take (Claude Code shows a long one
  folded and sends it whole); a file would be one more place to clean. If an agent's box turns out to cut long
  pastes, that agent's notices get the file then.
- **A pull channel** (the subagent fetching its task by MCP): it still has to be woken by typing.
- **Pre-allowing every Templum tool**, or trusting the whole server in Gemini CLI and Qwen Code: the person's
  permission stays theirs for every tool but `reply`.
- **A notification to the person when an agent's server doesn't connect:** phase 2, with the person-facing look of
  "no Templum tools" on the block.

## Risks

- **An agent replies too early** ("I'll start now"): the task ends with that. The lines say "once, when it is done";
  the master can hand it more.
- **An agent with tools never replies and its screen never stays still:** its task never ends and its master hears
  nothing until its call's timeout. The same as today; the lines make it rare.
- **Codex's trust of Templum's hooks** might be keyed on all its flags: one more "Hooks need review" for everyone once.
  Checked by hand in the plan's task; if so, the change says so in its commit.
- **Copilot CLI's `templum(reply)` pattern** is from its `--help`, not a run: checked by hand where it is signed in.
- **A sandbox may keep `templum reply -` from the bus** (Codex's sandbox and network rules): Codex has the MCP tool, so
  only agents without tools rely on the command line.
