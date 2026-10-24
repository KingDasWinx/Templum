# Agent tools: what Claude Code does that Templum's agents couldn't

- **Date:** 2026-10-18
- **Status:** Implemented (kanban card 35, "Aprimorando ferramentas do Templum"):
  [docs/plans/2026-10-18-agent-tools.md](../plans/2026-10-18-agent-tools.md)
- **Builds on:** [connections.md](connections.md) (the bus, the tools, tasks, the notices typed back, spawning, the
  inbox), [ask-user.md](ask-user.md) (the dialog `propose_plan` reuses), [agents.md](agents.md) (the agents, their
  model and effort), [agent-parity.md](agent-parity.md) (which agent gets what), [checklist.md](checklist.md),
  [cockpit.md](cockpit.md) (notes), [git.md](git.md) (worktrees)
- **Shares operations with:** kanban card 35's sibling, card 43 ("Templum Agent", a second MCP server that controls
  all of Templum, [templum-agent.md](templum-agent.md) on its branch): see [Shared operations](#shared-operations)

## Goal

The tests of 16 October showed holes in what an agent can do on the canvas: `interrupt` presses Esc, and a `sleep`
the stopped agent had left in the background woke up later and wrote a file; an agent can wait for another to stop
or for a command to end, but not for "the server is up"; it can only use blocks the person wired to it, so it can't
open its own plan as a checklist; and it can add to a note or a checklist but not fix one. Next to those, Claude Code
has tools Templum's agents lack: forking a session, saved roles for subagents, a plan the person approves, a clock,
a command left running in the background, and leaving a worktree.

This card gives every agent connected to Templum's MCP server those tools, as blocks the person sees on the canvas.
Every tool is a tool of the server ([connections.md](connections.md#the-mcp-server)), so it works the same for
Claude Code, Codex, Gemini CLI, Copilot CLI, OpenCode, Kilo Code, Qwen Code and Pi
([agent-parity.md](agent-parity.md)); each has a `templum` verb for the agents without the server. Forking is the
one exception: it needs the agent's own flag ([Every agent](#every-agent)).

**Success criteria**

1. `stop_agent` on an agent the caller opened ends it and every process it started, background ones included; a
   `sleep 30 && touch f` it started is gone, and `f` is never written.
2. `wait_for` with a `pattern` answers when a line matching it is printed in the terminal ("Ready in", `ERROR|
   Traceback`), with the line; with `lines`, also from what is already there; with `wait: false`, the match comes back
   typed into the caller's terminal while it keeps working.
3. `create_block` opens a note, a checklist or a terminal (with a command running in it, such as a dev server)
   beside the caller, wired to it; one Ctrl+Z closes it.
4. `replace_note` replaces a note's text, or one passage of it; `edit_item` and `delete_item` fix a checklist; each
   is one Ctrl+Z step.
5. `spawn_agent` with `fork` opens a copy of the caller's own agent on its conversation so far (Claude Code and
   Codex).
6. A role saved in `config.toml` (`[roles.reviewer]`) opens its agent with its model, effort, words and instructions:
   `spawn_agent(role: "reviewer", prompt: …)`.
7. `propose_plan` shows a long Markdown plan in the requests inbox with Approve, Reject and a comment, and returns the
   person's decision.
8. `schedule` hands a text to a wired agent or reminds the caller, at a time or every few minutes, until it is
   cancelled; the person sees the caller's schedules on its block and can cancel them.
9. `remove_worktree` removes a worktree Templum made (asking the person when it would lose work), and
   `merge_worktree` merges its branch back after the person allows it.
10. Every check of [Testing](#testing) passes on the three OS; the batches that paint or touch `templum-terminal`
    keep `--bench` and `--bench-mixed` at **p95 ≤ 16.7 ms**.

## Out of scope

Later work, which the card says can wait:

- **Review findings with their own UI** (Claude Code's ReportFindings): an agent already makes one card per finding
  with `create_card` (priority by severity, `file:line` and what fails in the description); `create_card`'s
  description says so. No new tool.
- **An HTML or browser block**, a page an agent writes and the person sees.
- **Push notifications to the phone** (PushNotification).
- **Workflows**: a script that fans tasks out to many agents.

Also left out, each a call made here ([Calls made](#calls-made-for-card-35)):

- Schedules and watches saved across a restart, and cron's syntax.
- A role editor in Settings (roles are `config.toml`'s, which Settings ▸ Config opens).
- `create_block` of a kanban or an editor (card 43 adds those kinds to the shared operation; the block server can
  take them later).
- Forking for Pi, Grok Build, OpenCode, Kilo Code, Gemini CLI, Copilot CLI, Qwen Code and the others ([Every
  agent](#every-agent)).
- `stop_agent` on a block the caller didn't open: refused, as `close_block` is.

## Words

- **The caller, wired, opened by:** as in [connections.md](connections.md#words). A block is **opened by** an agent
  when that agent's `spawn_agent` or `create_block` made it (`Pane::opened_by`, its block id).
- **A watch:** a `wait_for` with a `pattern` and `wait: false`: it keeps looking while the caller works, and its
  match is typed back.
- **A schedule:** a text Templum delivers later, once or every few minutes, from the caller to a block. Named `s1`,
  `s2`…, counting from 1 at each start, as tasks are.
- **A role:** a saved way to open an agent: which agent, its model and effort, words for its command line, and
  instructions typed with its prompt.

## The tools

Each tool below names its arguments and answers. Refusals are error results with a sentence that says what to do,
as every tool's ([connections.md](connections.md#the-mcp-server)). "Wired" checks are made at each call.

### `stop_agent`

*Batch 1.* What Claude Code's TaskStop does for a background task: ends it, and what it started.

- **Arguments:** `block`.
- **Does:** ends the program running in the terminal `block` (its agent, or the command `create_block` started there),
  everything below it, and what it left running after its parent quit. The shell stays, at its prompt; the block stays
  open. On Unix each process gets SIGTERM (an agent saves its session on it), and what is left 2 seconds later SIGKILL;
  on Windows they are ended at once.
- **Which processes** (`templum_terminal::strays`, read before any signal, while the children still name their
  parents):
  - **the program:** the process in front of the shell (Unix: the leader of the PTY's foreground process group, when it
    isn't the shell); on Windows, which can't tell who is in front, the agent's process the last snapshot found
    (`Pane::seen.pid`) while it is still that process (its start time, `seen.started`) below the shell, else the
    shell's oldest child; and every process below it;
  - **its orphans:** every process of the user whose environment's `TEMPLUM_TERMINAL` is this terminal's and whose
    parent isn't the terminal's shell nor a process below it: what a tool's shell left behind when it quit. Claude
    Code's Bash tool runs its shell as the leader of a session of its own, so a `( sleep 60 && touch f ) &` it leaves
    is reparented to init (or `systemd --user`) in that session: neither below the terminal's shell nor in its session,
    but it carries the variable (checked by hand on 2026-10-18). Only one started after the program (its start time
    in a later second; with no program in front, after its agent's, as the last snapshot found it, or as the one
    before found the agent that has since quit, until the person types into the block (`Pane::quit`); neither known,
    none): a daemon the shell's startup files or the person started before it (an `ssh-agent`, a tmux server, a
    browser a dev server opened before) stays;
  - **never** the shell: the terminal's process, and on macOS, where that is `login`, the shell it runs (its child), and
    on Windows, where it is Git for Windows' `bin\bash.exe`, the `usr\bin\bash.exe` it starts; what runs between them
    and the program, nor a job the person started from the shell's prompt (a child of the shell); a process whose
    environment can't be read is no orphan.
- **Nothing in front and no orphan:** "Nothing runs in k9x1d: its shell is at its prompt." With nothing in front but
  orphans left (its agent quit and left a job), they are ended, and the answer says `ended 2 processes it left
  running`.
- **Its task:** the task the caller handed it ends at once, its waits answered "Stopped by you.", with no notice,
  as `interrupt` and `close_block` end one ([connections.md](connections.md#delegating) 12). A task another agent
  handed it ends as when its agent leaves (`agent_left`: a `failed` notice to that agent).
- **Answer:** `Stopped k9x1d: ended Claude Code and 3 processes it started. Its shell is at its prompt.` (the count
  leaves the program itself out; "and nothing else" for none; with no program, `Stopped k9x1d: ended 2 processes it
  left running. Its shell is at its prompt.`).
- **Refused:**
  - the caller's own block: "This block is yours: stop_agent stops another block.";
  - a block not wired, or not a terminal (as every tool);
  - a block the caller didn't open: "Only the person stops k9x1d: you didn't open it. interrupt presses Esc in it.";
  - nothing runs in it and nothing it left runs (above), or its shell exited: "Nothing runs in k9x1d: its shell is at
    its prompt.""
- **`interrupt`** stays as it is: Esc, the agent lives on.

### `wait_for` with a pattern

*Batch 1 (waiting); batch 4 (`wait: false`).* What Claude Code's Monitor does with `tail -f log | grep`.

- **Arguments, new:** `pattern`, a regular expression (Rust's `regex` syntax, as `grep -E` writes most of it: `Ready
  in`, `ERROR|Traceback`, `(?i)listening on`); `event: "output"`, which `pattern` implies when `event` is left out;
  `lines`, 0 to 2000, 0 unless given: how many of the lines already in the terminal are looked at first; `wait`,
  true unless given (batch 4).
- **A line** is what a program printed up to a line feed, as the PTY wrote it: Templum's reader thread
  (`templum-terminal`'s `tap.rs`) keeps the line being printed while a watch is set, its escape sequences dropped and a
  `\r` before the line feed too, and tests it against each watch's pattern at its end. It sees every line, however
  fast they come or however far they scroll, and none twice. A full-screen program's redraws (an agent's own screen, a
  TUI) aren't lines: `idle` and `needs_you` wait for agents. **On Windows** Templum reads ConPTY's drawing of the
  screen, not the program's bytes: lines mostly come with their line feeds, but a resize or a full redraw may show a
  line again or join two. There a watch is best effort ([minors](../minors.md#connections)).
- **`lines`:** with 20, the last 20 lines of the terminal's history and screen, as `read_terminal` reads them, are
  tested first, the oldest first; a match there answers at once. For a command started a moment ago (`create_block`
  with a `command`) that may have printed its line before the call came.
- **Answer:** `t4q8p printed a line matching "Ready in":` then the line, then `Its last lines:` and the last 20 lines
  of its screen. The first match answers; the watch is then removed.
- **The timeout** (300 s unless asked, at most 3600) answers as `wait_for`'s do: `No line matching "Ready in" in t4q8p
  within 300 seconds. Its last lines:` and its last 40 lines. A closed block, the connection's end and a cancel answer
  or drop it as every `wait_for` ([connections.md](connections.md#tools)).
- **Refused:** a `pattern` that isn't a regular expression (`"pattern" isn't a regular expression: <the regex error's
  first line>`), one over 1000 characters, a blank one, `pattern` with an event other than `output`, `output` without
  a pattern, `lines` with no pattern; and as `wait_for` refuses today (not wired, the caller's own block, not a
  terminal).
- **`wait: false`** (batch 4): answers at once, `Watching t4q8p for "ERROR" as w3: a match is typed into your
  terminal.`; the first match comes back as a notice, typed as a task's end is ([connections.md](connections.md#delegating)
  10: once the caller takes typing, oldest first): `[watch w3 on t4q8p: a line matches "ERROR"] The line is in the pasted
  text.` and, on the next lines, the line, `Its last lines:` and 20 lines. Its timeout, or its block closing, ends it
  with a notice too: `[watch w3 on t4q8p ended: no line matched "ERROR" in 300 s]` (or `t4q8p was closed`). A watch
  answers once, as Monitor's single-notification form: the caller sets another to keep watching. The caller's own
  block leaving drops its watches silently. Watches are kept in `Workspace::watches`, never saved.
- The command line: `templum wait <block> --match <pattern> [--lines <n>] [--timeout <s>]`; a script waits, so the
  verb has no `wait: false`.

### `create_block`

*Batch 1.* What TodoWrite gives Claude Code (its own plan), and Claude Code's `run_in_background`, as blocks the
person sees.

- **Arguments:** `kind` (`note`, `checklist` or `terminal`; required); for a note `text`; for a checklist `name` and
  `items` (an item per line, read as `add_items` reads them: a leading `- ` or `* `, then `[ ]` or `[x]`, dropped);
  for a terminal `folder` (relative to the caller's, the caller's when left out) and `command`.
- **Where:** beside the caller where it covers no block nor drawing, toward the blocks wired to it, as `spawn_agent`
  places an agent ([connections.md](connections.md#spawning)); wired to the caller. The keyboard, the selection and the
  camera stay where they were; in grid mode it takes half of the caller's tile. One Ctrl+Z closes it and its wire.
- **A note:** a new note (`notes.json`) with `text`, shown in a new note block, named by its first line with text
  (Untitled when blank; the person renames it). A blank text gives an empty note, which stays while its block shows it.
- **A checklist:** a new checklist (`checklists.json`) named `name` ("Checklist" when blank) with its items, shown in
  a new checklist block.
- **A terminal:** a new terminal of the default size in `folder`, with the shell's environment every terminal gets;
  with `command`, the command is typed and run (Enter) once the shell reads it, as an agent's command is
  (`start_agent`). Its `opened_by` is the caller, so the caller may `stop_agent` its command and `close_block` it. It
  runs until stopped or closed: a terminal on the canvas is always "in the background". `wait_for` with `pattern` and
  `lines` waits for its line; `read_terminal` reads it.
- **Answer:** `Opened a checklist as k2m9x, connected to you.`; for a terminal `Opened a terminal as k2m9x in
  /home/u/app, running "npm run dev", connected to you.` and, with a command, `wait_for(block: "k2m9x", pattern: …,
  lines: 50) waits for its line; stop_agent ends it.`
- **Refused:** an unknown kind (`kind is note, checklist or terminal`); `text` for a checklist or a terminal, `items`
  or `name` for anything but a checklist, `folder` or `command` for anything but a terminal (each says which kind takes
  it); a folder that isn't one; a `command` over 4096 bytes or with a line break (it is typed into a shell: one line,
  as the person would type it); Templum doesn't know the caller's folder and none is given.
- **The instructions** ([connections.md](connections.md#the-mcp-server)) gain, after "check the items of a connected
  checklist as you finish them": "For work of several steps, open your own checklist with create_block and check its
  items as you go."
- The command line: `templum new note [<text>] | new checklist <name> [--items <text>] | new terminal [--folder <f>]
  [--command <c>]`.

### Fixing a note or a checklist

*Batch 1.* Each is one step of Ctrl+Z, shown at once, the person's caret and selection kept, as `append_note` and
`check_item` are ([checklist.md](checklist.md#agents)).

| Tool | Arguments | Does | Answer |
|---|---|---|---|
| `replace_note` | `block`, `text`, `old` | Without `old`: the note's whole text becomes `text` (a blank `text` is refused: "That would empty the note: the person does that."). With `old`: its one occurrence becomes `text` (blank deletes it, unless that leaves the note blank: refused the same); none, or more than one, is refused: "\"…\" isn't in the note: read_note shows it." / "\"…\" is in the note 3 times: give more of the text around it." | `Replaced the note in k2m9x.` / `Replaced 1 passage of the note in k2m9x.` |
| `edit_item` | `block`, `item`, `text` | The item's text becomes `text`; blank is refused ("delete_item removes an item") | `Item 41 is now "Build".` |
| `delete_item` | `block`, `item` | Removes the item (`Workspace::delete_item`, the row's ×) | `Deleted item 41.` |

An item the checklist doesn't have refuses as `check_item` does: `There's no item 99 in "Plan". read_checklist shows
its items.` The command line: `templum note replace <block> <text> [--old <text>]`, `templum checklist edit <block>
<item> <text> | delete <block> <item>`.

### `spawn_agent` with `fork`

*Batch 2.* Claude Code's `claude --resume <session> --fork-session`: a new agent that starts with the caller's whole
conversation.

- **Arguments, new:** `fork` (true or false; false unless given). With it `agent` may be left out: a fork is always
  of the caller's own agent (an account too: a `claude2` forks in `claude2`'s folder); naming another agent is
  refused ("A fork is a copy of your own agent, claude: leave agent out."). A fork needs a `prompt`, which tells the
  copy it is one and what to do ("A fork needs a prompt: it tells the copy what to do."). `model`, `effort`, `folder`
  and `worktree` work as for any spawn.
- **The session forked** is the caller's, as the last snapshot found it (`Space::saved`'s item for its block: the id
  `templum-usage` reads from Claude Code's files and Codex's rollouts). None found yet: "Templum doesn't know your
  session yet: try again in a few seconds."
- **What is typed** instead of the agent's command: its fork command for that session, then the model and effort
  words, then what Templum adds (connections.md's flags), as a resume is typed on restore:

  | Agent | Fork command |
  |---|---|
  | Claude Code | `claude --resume <id> --fork-session` |
  | Codex | `codex fork -C . <id>` |

  A field `fork_id` of `templum-terminal`'s agent table holds each, as `resume_id` does; an agent without one can't
  fork: "Gemini CLI can't fork a session: forks are for Claude Code and Codex." ([Every agent](#every-agent)). The
  fork's command replaces the agent's start command in `start_agent`, which otherwise ignores the command it is given
  for one of Templum's agents.
- **Another folder:** both find the session from any folder, so `folder` and `worktree` work for a fork as for any
  spawn (spike results below). Codex asks which folder a fork works in when it isn't the session's, Enter keeping the
  session's; `-C .` names the shell's, where Templum opened it, and Codex then asks nothing.
- **Its prompt** goes as a spawn's ([connections.md](connections.md#spawning)), its pasted text starting with "You are
  a copy of the agent in b7k2m, started from its conversation so far, in a block of your own." so the copy doesn't take
  itself for the original.
- **Answer:** as `spawn_agent`'s, `Opened a fork of your session as k9x1d in /home/u/app, connected to you. Its
  prompt is typed once it is ready, as task t8.`
- **Saved:** nothing new. Restored, the fork resumes its own session: Claude Code's and Codex's ids come from their
  files (`sessions/<pid>.json`, the rollouts), not from the arguments, so the snapshot finds the fork's new id. Batch
  2's first task checks it by hand.
- The command line: `templum spawn [<agent>] --fork …`.
- **Spike results** (by hand, 2026-10-18, Claude Code 2.1.286 and Codex 0.154, in scratch folders):
  1. `claude --resume <id> --fork-session` opens the conversation from a worktree of the session's repository and
     from a folder outside it: the fork answered a word told to the original.
  2. Forking while the original is mid-turn, as a fork always is (its caller waits in `spawn_agent`): a Claude Code
     session whose last entry was a running tool call forked and answered a word told to the original. Codex takes
     options after `codex fork -C . <id>` (`-c …` parsed, an unknown flag refused), so Templum's flags may follow.
  3. `codex fork <id>` from another folder shows "Choose working directory to fork this session" (the session's or the
     current one; Enter keeps the session's); `codex fork -C . <id>` opens the conversation in the current folder at
     once. Templum types the second.
  4. The fork has its own session: Claude Code's `sessions/<pid>.json` names a new id (not the one after `--resume`),
     and Codex's process holds a new rollout file. The snapshot reads those first, so a restore resumes the fork.
     Claude Code writes the fork's transcript with its first message (the prompt Templum types).

### Roles

*Batch 2.* Claude Code's `subagent_type`: an agent opened for a job ("reviewer", "tester") with what that job needs.

- **In `config.toml`**, under `[roles]`, one table per role, keyed by its id (letters, digits, `-` and `_`):

  ```toml
  [roles.reviewer]
  agent = "claude"                      # an id of Settings ▸ Agents, an account too (claude2)
  model = "opus"                        # optional, as spawn_agent's model
  effort = "high"                       # optional, one of its agent's efforts
  args = "--tools Read,Grep,Glob,Bash"  # optional: its agent's own flags, typed after its command
  instructions = """
  You review code: read the diff, run the tests, report each finding as a card. Never edit files.
  """
  ```

  `agent` is required, `args` one line. A file breaking these is reported like any other config error, and the last
  valid config stays ([config.md](config.md)). The template gains a commented example.
- **"Allowed tools"** are the agent's own flags, written by the person in `args`, since each agent restricts its tools
  its own way: Claude Code `--tools` and `--disallowedTools`, or `--agent <one of its subagents>`; Codex `-s
  read-only`; Copilot CLI `--deny-tool`. Templum types them as written, as it types an `[agents]` command: they are the
  person's.
- **`spawn_agent(role: "reviewer", prompt: …)`** opens the role's agent with its model, effort and words. A call's own
  `agent`, `model` and `effort` win over the role's. The instructions go with the prompt, for every agent: the
  prompt's pasted text is `Your role, reviewer:` and the instructions, a blank line, `Your task:` and the prompt. A role
  with instructions and no prompt is refused ("The role reviewer's instructions go with a prompt: give one."). The
  task's first line names it: `[from Claude Code · api (b7k2m), task t8, as reviewer]`.
- **Unknown:** "There's no role tester. Roles: reviewer." (or "No roles are saved: [roles] in config.toml holds
  them.").
- **Kept:** the block remembers its role (`Pane::role`, saved as the terminal item's `role`); a restore or Ctrl+Z types
  the role's `args` again with its model and effort (a role removed since gives none). The instructions aren't typed
  again: the resumed conversation has them.
- **The instructions** gain, when there are roles: "Roles for spawn_agent: reviewer (claude), tester (codex)." (ids and
  agents, at most 10, then "…").
- The command line: `templum spawn [<agent>] --role <role> …`.

### `propose_plan`

*Batch 3.* Claude Code's EnterPlanMode and ExitPlanMode: a long plan the person reads, then approves or sends back.

- **Arguments:** `plan` (Markdown, required, not blank, at most 256 KB), `title` (optional; shown up to 60 characters),
  `timeout` (as `ask_user`'s: none unless given or set by `[connections] question_timeout`, the shorter winning).
- **The dialog** is `ask_user`'s request in the inbox ([ask-user.md](ask-user.md#the-dialog)) holding a plan instead
  of questions:

  ```
  From Claude Code · api  Why Releasing 1.2                          Plan to approve
  ─────────────────────────────────────────────────────────────────────────────
   [Plan]  Move the bus to tokio
   ┌─────────────────────────────────────────────────────────────────────┐
   │ ## Steps                                                            │
   │ 1. Replace the reader threads…                                      │
   │ …                                                                   │
   └─────────────────────────────────────────────────────────────────────┘
   Comment [ optional                                                   ]
  ─────────────────────────────────────────────────────────────────────────────
   Reject                                                          [✓ Approve]
  ```

  in the frame of ask_user's dialog since card 81: the foot stays, the body over it scrolls

  the header chip "Plan" and the title (or "A plan" when none); the plan rendered as Markdown by `markdown::preview`
  (headings, lists, code), in a frame on the canvas color that scrolls on its own, at most 60% of the body's height
  (the popup less its foot); the wheel over it scrolls it when it overflows, not the body; then the Comment field
  (the note of `ask_user`); then **Reject** on the left and **Approve** on the right. Its keys are `ask_user`'s: ↓ and
  Tab move through Comment, Reject and Approve, Enter on Approve or Reject presses it, Esc closes the inbox; presses
  within 0.5 s of the inbox drawing it are ignored. The sound, the notification ("Asks you to approve a plan:
  *title*"), the alert, its row in the list, the deadline, a cancel typed back,
  and the connection's end are a question's.
- **Answer:** approved, a result `Approved.`; rejected, an error result `Rejected.`; either followed by
  `\nThe person's comment: …` when they wrote one. (`templum plan` exits 1 on a rejection: `templum plan p.md && …`.)
- **When the client stops waiting:** the answer is typed into the agent's terminal as a question's is,
  `[answer to your plan "Move the bus to tokio"] The answer is in the pasted text.` and `Approved.` or `Rejected.`
  with the comment.
- **In `templum-mcp`:** an `Ask` whose `plan` is set: one implicit single-select question (Approve, Reject) and the
  note, so `Form` keeps the picking, the highlight and the answer, headless and unit-tested; the inbox draws a plan
  instead of its questions.
- The command line: `templum plan <file> [--title <t>]` (`-` reads stdin).

### `schedule` and `cancel_schedule`

*Batch 4.* Claude Code's ScheduleWakeup and CronCreate: Templum already types into agents; this adds the clock.

- **`schedule` arguments:** `block` (a wired block running an agent, or the caller's own), `text` (not blank), and when:
  `at` (`HH:MM`, 24-hour, the computer's local time: the next time the clock shows it, today or tomorrow), `after`
  (minutes, 1 to 10080), `every` (minutes, 1 to 10080). One of `at` and `after`, or `every` alone, which first comes
  `every` minutes from now; `every` with `at` or `after` repeats from that first time. Neither is refused ("schedule
  needs at, after or every"), and so are `at` with `after`. A block running no agent, a wired one or the caller's own (a
  shell running `templum schedule`, which would never take its notices), is refused: "k9x1d runs no agent: schedules go
  to agents. To run a command, open a terminal with create_block's command." Templum already knows when an agent takes
  a task; a shell never says when a line typed into it would run as the person meant.
- **When it comes** Templum delivers `text`, from the caller, as if the caller sent it then:
  - to an agent: a new task, as `delegate` without `wait` types one (its first line `[from Claude Code · api (b7k2m),
    task t9, scheduled s3]`), whose end comes back to the caller typed. A schedule never changes a task: an agent still
    on a task, the caller's included (one an earlier time of this schedule handed), doesn't take it: "k9x1d is on a task
    (t7): a schedule hands a task only to an agent that has none." Nor the shell its agent left (a snapshot finds none
    there, or its shell is in front), where the text would run as a command: "k9x1d runs no agent now: a schedule for an
    agent never types into its shell.";
  - to the caller's own block: a notice, `[scheduled s3] Its text is in the pasted text.` and the text, typed once the
    caller takes typing ([connections.md](connections.md#delegating) 10).
  It goes through the same checks as the tool it stands for, at that moment: a block no longer wired, an agent waiting
  for its person or on another agent's task, a closed block. A refusal comes back to the caller as a notice,
  `[schedule s3 to k9x1d didn't go: <the refusal's sentence>]`; a closed block ends the schedule, the others try again
  at its next time (a one-time schedule ends).
- **How long:** a one-time schedule ends when it went; one with `every` after 7 days (Claude Code's limit for its
  cron), told by a notice `[schedule s3 ended: 7 days]`. All end when Templum quits: they aren't saved, as tasks
  aren't. The caller's block closing ends its schedules.
- **Answer:** `Scheduled s3: to k9x1d at 15:00 (in 42 minutes), then every 10 minutes until 25 Oct 15:00.`
- **`cancel_schedule`** (`schedule`: its id): `Cancelled s3.`; only the caller's own: "s3 isn't one of your
  schedules." `whoami` lists the caller's schedules, a line each: `schedule s3: to k9x1d every 10 min, next 15:10:
  <the text's first line, 60 characters>`.
- **What the person sees:** a clock and the count after the caller's title (`◷ 2`, `title_dim`, at the Full level),
  whose tooltip lists them as `whoami` does; the block's menu gains **Cancel schedules ▸**, one row per schedule and
  **All**. A schedule that delivers shows on the target as any task or typed text.
- **The clock:** a schedule's times are the computer's clock (`SystemTime`), looked at every 15 seconds while there
  are schedules: Templum's timers count only time awake, and a computer asleep at a schedule's time would deliver it
  late by as long as it slept. A time missed while asleep goes at the first look after waking, once; a repeating one
  goes on `every` minutes from then. `at` becomes a moment of the local clock for that day (`chrono::Local`), so a
  daylight-saving change is counted.
- The command line: `templum schedule <block> <text> [--at HH:MM] [--after <min>] [--every <min>]`, `templum
  unschedule <id>`.

### `remove_worktree` and `merge_worktree`

*Batch 5.* Claude Code's ExitWorktree, for the worktrees `spawn_agent`'s `worktree` makes.

- **Which:** `worktree`, the name `spawn_agent` was given: `.worktrees/<name>` of the main working tree of the
  repository `folder` is in (`folder` as `spawn_agent`'s: relative to the caller's, the caller's when left out; the
  caller may itself be in a worktree). A name Templum couldn't have made, or no such worktree: "There's no worktree
  feature-x in this repository: git worktree list shows them."
- **Into:** the branch checked out in `folder`'s working tree, where the caller works, not the main working tree's: a
  worktree spawned from a worktree branched from that one's HEAD, so it goes back there. "Merged" means merged into
  that branch. With no branch there (a detached HEAD) both are refused: "/home/u/app has a detached HEAD: check out
  the branch feature-x goes back to first."
- **Nobody may work in it:** a terminal block whose folder is in it refuses both: "k9x1d works in it: stop_agent or
  close_block it first." (any workspace).
- **`remove_worktree`** (`worktree`, `folder`, `discard` false unless given): with no changes and its branch merged into
  the main working tree's branch, removes the worktree and deletes its branch at once: `Removed the worktree feature-x
  and its branch.` With changes or commits not merged, refused unless `discard`: "feature-x has 2 changed files and 3
  commits not merged into development: merge_worktree merges them, discard: true loses them." So are files git ignores
  there, which `git worktree remove` deletes, but for what a build or a test run fills again: the folders
  `node_modules`, `target`, `build`, `dist`, `out`, `.next`, `.nuxt`, `.svelte-kit`, `.angular`, `.turbo`,
  `.parcel-cache`, `.venv`, `venv`, `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache`, `.tox`, `.nox`,
  `.hypothesis`, `coverage`, `htmlcov`, `.nyc_output`, `.gradle`, `.dart_tool`, `.cache` and `*.egg-info` git ignores
  whole, and what is inside one that ignores itself with a `*` in its own `.gitignore` (pytest's, ruff's, a
  virtualenv's), not what git ignores in a tracked folder of such a name (a `.env` in Go's `build/ci/`); the files
  `npm-debug.log`, `yarn-error.log`, `yarn-debug.log`, `pnpm-debug.log`, `.eslintcache` and `.coverage`: "feature-x
  has files git ignores, which removing it deletes: .env. discard: true deletes them." With `discard`, the person is
  asked in the inbox first ([Asking the person](#asking-the-person)): "*Claude Code · api* wants to delete the
  worktree feature-x, with 2 changed files and 3 commits not merged, and files git ignores: .env." (the last part with
  such files only) **Delete** and **Keep**. Deleted: `git worktree remove --force` then `git branch -D`.
- **`merge_worktree`** (`worktree`, `folder`): the person is asked first, always: "*Claude Code · api* wants to merge
  feature-x (3 commits) into development." **Merge** and **Cancel**. Allowed, and still the commits and the branch the
  person was told of ("/home/u/app is on main now, not on development as the person was told: ask again."), Templum runs
  `git merge --no-edit <branch>` in `folder`'s working tree (the branch's full name, `refs/heads/<branch>`, when a tag
  has its name). Refused before asking: changes not committed in the worktree ("Commit them first: a merge takes
  commits."); changes not committed to tracked files where it merges into, which a merge would mix with and an aborted
  one may not give back ("development has changes not committed in /home/u/app: commit or stash them first."); nothing
  to merge ("feature-x has nothing development doesn't."); a merge, rebase or cherry-pick already under way there; a
  file git ignores there that the worktree's branch tracks (a `.env` added with `git add -f`), which git would write
  over without a word ("The merge would overwrite .env, ignored in /home/u/app: move it first.", the first five files
  and how many more), checked again when the person says Merge. Other untracked files don't refuse it: git refuses
  itself when the merge would write over one. A conflict aborts the merge (`git merge --abort`) and answers with the
  files: "The merge would conflict in src/a.rs, src/b.rs: merge development into feature-x and resolve them there, then
  merge again." Merged: `Merged feature-x into development (3 commits). remove_worktree removes it.` The worktree stays.
- The command line: `templum worktree remove <name> [--folder <f>] [--discard] | merge <name> [--folder <f>]`.

### Asking the person

`connect`'s request ([connections.md](connections.md#requests-from-agents)) becomes one of a kind: a sentence and two
buttons, in the inbox, with its sound, notification, alert, queue, timeout (30 minutes unless asked) and cancel.
`merge_worktree` and a discarding `remove_worktree` use it with their own words; Deny (Keep, Cancel) answers "The
person said no." and nothing runs. The keyboard doesn't answer it, as `connect`'s.

## Permissions

The rules stay today's ([connections.md](connections.md#tools)): a tool reads or writes the caller's own block and
the blocks wired to it; what a block opened on its own the caller may close; what can't be taken back asks the person.

| Tool | May touch | Asks the person |
|---|---|---|
| `stop_agent` | a wired terminal the caller opened (`spawn_agent`, `create_block`); never its own | no (the block stays; Ctrl+Z isn't needed) |
| `wait_for` with `pattern` | a wired terminal; not its own | no |
| `create_block` | a new block, wired to the caller | no: it is on the canvas, Ctrl+Z closes it |
| `replace_note`, `edit_item`, `delete_item` | a wired note or checklist | no: one Ctrl+Z step |
| `spawn_agent` with `fork` or `role` | a new block, as `spawn_agent` | no |
| `propose_plan` | the caller's own request in the inbox | it is the person's answer |
| `schedule` | a wired block running an agent, or its own; checked again when it goes | no: shown on its block, cancelled from its menu |
| `cancel_schedule` | the caller's own schedules | no |
| `remove_worktree` | a worktree no block works in | only with `discard` |
| `merge_worktree` | the same | always |

## Every agent

Every tool here is a tool of Templum's MCP server, the same list for every caller, so any agent that has the server
gets all of them; an agent without it runs the `templum` verbs from its shell tool. What differs is what an agent's own
command line takes:

| | Claude Code | Codex | Gemini CLI | Copilot CLI | OpenCode, Kilo Code | Qwen Code | Pi | Grok Build | The others |
|---|---|---|---|---|---|---|---|---|---|
| Every tool but fork | yes | yes | yes, with its extension | yes | yes | yes | yes (0.99 and later; the verbs before) | the verbs | the verbs |
| Forked by `spawn_agent` | yes | yes | no fork flag | no fork flag | no: Templum can't know their session's id | no fork flag | not yet: `pi --fork <id>` takes no `--session-id`, so Templum couldn't find the fork's session to resume it | not yet: Templum reads a Grok session's id from its arguments, and a fork's carry the original's first (`--resume <original> --fork-session <new>`) | no |
| A role's model and effort | yes | yes | model | yes | model | model | yes | yes | as [agents.md](agents.md#a-model-and-an-effort) |
| A role's instructions | typed with the prompt, for every agent | | | | | | | | |
| `stop_agent`, `wait_for` patterns, schedules into it | any program, any agent (they act on its processes and its terminal) | | | | | | | | |

The fork flags were read from the installed `--help`s on 2026-10-18 (Claude Code 2.1.286 `--fork-session`; Codex
0.154 `codex fork [SESSION_ID]`; OpenCode `--fork` with `--session`; Pi `--fork <path|id>`; Grok Build
`--fork-session <uuid>` with `--resume`); Gemini CLI, Copilot CLI and Qwen Code list none.

## Shared operations

Card 43 adds a second server, the Templum Agent's `templum control`, which needs the same workspace operations. Each
operation is **one `Workspace` method that takes its target (a workspace, a block, a folder), never a caller**, in
`crates/templum/src/workspace/ops.rs` (new), so neither server duplicates it. The block server's tool checks the
wire and the opener, then calls it; the control server checks its gate, then calls it. Whichever card lands first adds
the method; the other calls it, and a task that adds one first looks for it on `development`.

| Operation | Does | Card 35's tool | Card 43's tool | Added by |
|---|---|---|---|---|
| `add_block_in(space, NewBlock, Place, opener) -> Result<BlockId, String>` | A block of a kind at a place where it covers nothing; `opener` its `opened_by` (a block id or `templum-agent`). `NewBlock::{Note { text }, Checklist { name, items }, Terminal { folder, command }}`; `Place::Near(BlockId)` (toward its wires, as `room_near`). The caller wires it | `create_block` | `add_block`, `run` | card 35; card 43 adds `NewBlock::{Kanban, Editor}` and `Place::{Free, At}` |
| `stop_in(space, block) -> Result<Stopped, String>` | Ends what runs in the terminal and its processes (`stop_agent` above); `Stopped { program, processes }` | `stop_agent` | `stop_agent` | card 35 |
| `note_replace(space, block, text, old)`, `item_edit(list, item, text) -> bool`; `delete_item(list, item)` (there already: the row's ×) | The note and checklist fixes above, as Ctrl+Z steps | `replace_note`, `edit_item`, `delete_item` | `replace_note`, `edit_item`, `delete_item` | card 35 |
| `worktree_state(folder, name)`, `worktree_remove(folder, name, discard)`, `worktree_merge(folder, name)` | Run on the background executor through `templum-git` (`Repo::worktree_state`, `remove_worktree`, `merge`) | `remove_worktree`, `merge_worktree` | `remove_worktree`, `merge_worktree` | card 35 |
| `fork_command(entry, session) -> Option<String>` (in `agents.rs`, through `templum_terminal::Agent::fork_session`) | The words that start a fork (Claude Code, Codex) | `spawn_agent` with `fork` | `open_agent` with `fork`, if it takes one | card 35 |
| `Role` (`config.rs`) and `Role::spawn(&self, id, tools::Spawn) -> Result<tools::Spawn, String>` | A role's agent, model, effort, words and prompt over a spawn's own | `spawn_agent` with `role` | `open_agent`'s `role` | card 35 |
| `ask::Ask::plan(title, markdown)` and the inbox's plan | A plan to approve | `propose_plan` | the plan card (its batch 4) | card 35 |
| `schedule_add(Schedule) -> u64`, `schedule_cancel(id)`, `Schedule { by, to, text, next, every, until }` | Deliveries later; `by` an asker (a terminal id, or `templum-agent`) | `schedule`, `cancel_schedule` | `schedule` (its batch 4) | card 35 |
| `watch_add(terminal, Pattern) -> Option<u64>` over `templum_terminal::Terminal::watch`; `Watch { by, of, pattern, until }` in `Workspace::watches` for `wait: false` | A pattern looked for in a terminal's lines | `wait_for` with `pattern` | `wait_for`'s `pattern` | card 35 |
| `open_agent_in(space, entry, folder, spawn, place, wire_to, opener)` | `spawn_agent`'s `open_agent` without a caller's block | `spawn_agent` (a fork goes through it) | `open_agent` | card 43, from `spawn.rs`; until it lands, card 35's fork goes through `spawn.rs`'s `open_agent`, changing only the command it types |

Notes for card 43:

- `NewBlock::Terminal` has **no `background` field**: every terminal on the canvas keeps running, so a command is
  always "in the background". The control server's `run` with `background: false` is `add_block_in`, then a
  `wait_for(command_finished)` on it.
- The asker of a schedule, a watch and a task is a string: a terminal's `TEMPLUM_TERMINAL`, or card 43's reserved
  `templum-agent`. Card 35 delivers a notice only to a terminal; card 43 routes `templum-agent`'s to its chat.
- The approval of the block server is the inbox; the control server's is its own gate. The operations above never
  ask anyone.
- `add_block_in` takes an `opener` (card 43's spec has none yet): the block's `opened_by`, a block id or
  `templum-agent`, which `stop_agent` and `close_block` check. The file is card 43's name for it, `workspace/ops.rs`.

## The command line

New verbs, each one tool call ([connections.md](connections.md#the-templum-command-line)), each with `--json`:

| Command | Tool | Batch |
|---|---|---|
| `templum stop <block>` | `stop_agent` | 1 |
| `templum wait <block> --match <pattern> [--lines <n>]` | `wait_for` with `pattern` | 1 |
| `templum new note [<text>] \| checklist <name> [--items <text>] \| terminal [--folder <f>] [--command <c>]` | `create_block` | 1 |
| `templum note replace <block> <text> [--old <text>]` | `replace_note` | 1 |
| `templum checklist edit <block> <item> <text> \| delete <block> <item>` | `edit_item`, `delete_item` | 1 |
| `templum spawn [<agent>] [--fork] [--role <role>] …` | `spawn_agent` | 2 |
| `templum plan <file> [--title <t>]` | `propose_plan` | 3 |
| `templum schedule <block> <text> [--at HH:MM] [--after <min>] [--every <min>]`, `templum unschedule <id>` | `schedule`, `cancel_schedule` | 4 |
| `templum worktree remove <name> [--folder <f>] [--discard] \| merge <name> [--folder <f>]` | `remove_worktree`, `merge_worktree` | 5 |

## Config

```toml
[roles]
## Ways spawn_agent opens an agent by name (docs/specs/agent-tools.md#roles): its agent, and
## optionally its model, effort, args (the agent's own flags) and instructions. For example:
## reviewer = { agent = "claude", model = "opus", effort = "high", args = "--tools Read,Grep,Glob,Bash", instructions = "You review code and report each finding as a card. Never edit files." }
```

The table stays uncommented and empty, and the example is one inline table: the template's rule is that its `# `
lines are the defaults and its `## key = value` lines examples that work once uncommented, which a commented
`[roles.reviewer]` table can't keep (its keys would fall into the table above it).

A change applies to the next `spawn_agent`, as every config change does to what comes after it.

## Files

- **Terminal items** gain `role` (the role it was opened as), written only when set; an older file has none and an
  older Templum ignores it.
- Watches and schedules aren't saved; nothing else is new.

## Architecture

| File | Change | Batch |
|---|---|---|
| `templum-terminal/src/terminal.rs` | `strays(terminal, shell, program) -> Vec<Running>`: the program, below it, and the block's orphans by their `TEMPLUM_TERMINAL`; `terminate(&[Running])`: SIGTERM (Unix), TerminateProcess (Windows) | 1 |
| `templum-terminal/src/tap.rs`, `terminal.rs`; `Cargo.toml` (`regex`) | the watches: `Pattern` (a `regex::Regex` with a size limit), `Terminal::watch`, `hit`, `unwatch`; the line kept while one is set, its escapes dropped | 1 |
| `templum-mcp/src/tools.rs` | `stop_agent`, `create_block`, `replace_note`, `edit_item`, `delete_item` (1); `wait_for`'s `pattern`, `lines`, `Event::Output` (1) and `wait` (4); `spawn_agent`'s `fork` and `role` (2); `propose_plan` (3); `schedule`, `cancel_schedule` (4); `remove_worktree`, `merge_worktree` (5); `create_card`'s sentence on findings (1) | 1–5 |
| `templum-mcp/src/cli.rs` | the verbs above | 1–5 |
| `templum-mcp/src/ask.rs` | `Ask::plan`, its answer and notice | 3 |
| `templum/src/workspace/ops.rs` (new) | [Shared operations](#shared-operations) | 1–5 |
| `templum/src/workspace/bus.rs` | the calls; the instructions' new sentences; `whoami`'s schedules | 1, 2, 4 |
| `templum/src/workspace/delegate.rs`, `spawn.rs` | `stop_agent`; a fork and a role in `open_agent`; the `[from …, as reviewer]` and `scheduled s3` lines | 1, 2, 4 |
| `templum/src/workspace/content.rs` | `create_block`, `replace_note`, `edit_item`, `delete_item` | 1 |
| `templum/src/workspace/waits.rs` | `Waiting::Event` with a watch; `Waiting::Allow` (connect's request made general); a plan's question | 1, 3, 5 |
| `templum/src/workspace/schedules.rs` (new) | `Workspace::schedules`, `watches` delivered by their timers | 4 |
| `templum-terminal/src/agent.rs`; `templum/src/agents.rs` | `Spec::fork_id`, `Agent::fork_session`; `AgentEntry::fork_command` | 2 |
| `templum/src/config.rs`, `session.rs`, `workspace/persist.rs` | `[roles]`; `role` on terminal items, typed again on resume | 2 |
| `templum/src/balloon.rs`, `inbox.rs`, `workspace/questions.rs` | the plan's dialog and the inbox; the general request's words | 3, 5 |
| `templum/src/block.rs`, `menu/` | the schedules' clock and **Cancel schedules ▸** | 4 |
| `templum-git/src/repo.rs` | `Repo::worktree_state` (changed files, commits not merged), `remove_worktree` | 5 |

No new crate in the tree: `regex` 1.13 is already compiled for gpui; `templum-terminal` names it directly.

## Testing

**Unit (`templum-mcp`):** each new tool parses its example and names a missing or bad argument (`kind`, `pattern`
with another event, `lines` without a pattern, `at` and `after` together, an `at` that isn't `HH:MM`, `fork` with
`agent`); the verbs send their calls; `Ask::plan`'s form answers `Approved.` and `Rejected.` with the comment, and its
notice.

**Unit (`templum-terminal`):** `strays` finds a program's children and an orphan in a session of its own (`setsid`)
that carries the terminal's variable, and leaves the shell and a job started beside the program; `terminate` ends them
(Unix); a watch
sees a line split across reads, drops escapes and the `\r`, sees none twice, matches lines past the scrollback, and
nothing is kept without a watch; `fork_session` for Claude Code and Codex, none for the others.

**Unit (`templum-git`):** a worktree's changed files and commits not merged; removing one; a merge that would
conflict leaves the main working tree as it was.

**App (`workspace/tests/`):** `stop_agent` ends a job and its background `sleep … && touch f` (no `f` after the
sleep's time), ends the caller's task "Stopped by you." without a notice, keeps the shell, and refuses a block the
caller didn't open and its own; `wait_for` with `pattern` answers on a printed line, at once from `lines`, at its
timeout with the last lines, and leaves no watch behind; `wait: false` types its notice once; `create_block` opens
each kind beside the caller, wired, the keyboard and camera kept, one Ctrl+Z closing it, a terminal running its
command; `replace_note` (whole, a passage, not found, twice) and Ctrl+Z; `edit_item`, `delete_item` and Ctrl+Z; a fork
types Claude Code's and Codex's fork command with the snapshot's session, refuses without one, without a prompt and
for another agent; a role types
its model, effort and words, its instructions with the prompt, and again on restore without the instructions;
`propose_plan`'s Approve, Reject with a comment, keys, a cancel typed back; a schedule goes at its time to an agent and
the caller, one to a terminal running no agent is refused, a refusal comes back (an agent on a task included),
`every` repeats and ends after 7 days, a time missed while asleep goes once, a cancel, the clock on the block and its
menu; `remove_worktree` and `merge_worktree` with the inbox's buttons, their refusals, a conflict aborted.

**By hand:** Claude Code stopping a spawned Codex that left `sleep 60 && touch ~/x &` running (no `~/x`); a dev server
from `create_block` and a `wait_for` on "ready"; Claude Code forking itself, the fork knowing the conversation; a
reviewer role with `--tools`; a plan approved and one rejected; a schedule every minute into a Codex; merging a
spawned agent's worktree. On Windows: `stop_agent` on a Codex and its children.

**Bench:** batch 1 touches `templum-terminal`'s reader thread and batches 3 and 4 paint: their pull requests carry
`--bench` and `--bench-mixed`.

## Delivery

One pull request per batch, in this order (the problems already seen first, then the most value for the least
work), each in [the plan](../plans/2026-10-18-agent-tools.md):

1. **Problems already seen:** `stop_agent`, `wait_for` with a pattern, `create_block` (the background terminal
   included), `replace_note`, `edit_item`, `delete_item`.
2. **Agents like Claude Code's subagents:** fork, roles.
3. **A plan the person approves:** `propose_plan`.
4. **Later:** `schedule`, `cancel_schedule`, `wait_for` with `wait: false`.
5. **Worktrees:** `remove_worktree`, `merge_worktree`, the general request in the inbox.

## Risks

| Risk | What we do |
|---|---|
| `stop_agent` kills a process the person started in that terminal | Only blocks the caller opened; never the shell, what is above the program, or a job started from the shell's prompt; an orphan is ended only when its parent is gone |
| A detached process wipes its environment (`env -i`) and escapes | Documented: a program that hides from its terminal on purpose; the person sees it in their own tools |
| `stop_agent` ends a daemon the person or the shell's startup started from that terminal (ssh-agent, tmux) | An orphan is ended only when it started after the program (or its agent); a GUI app the program itself opened (a browser `vite --open` launched) is still its orphan and ends with it |
| `stop_agent` ends a daemon the agent started on demand, shared with the person's other work (a `gpg-agent`, an `ssh` ControlPersist master, an `sccache` or Gradle server, a browser `xdg-open` started) | Documented: it started after the agent and carries the terminal's variable, so it is the agent's orphan; the next use starts it again (a key's passphrase asked again, a new ssh connection, a cold build cache), and a browser it started closes with its windows |
| An agent killed before it saves its session | SIGTERM first, SIGKILL 2 s later; Claude Code and Codex write their transcripts as they go |
| A pid taken again between reading the tree and ending it | `Running` keeps each process's start time, checked before it is ended (`kill_left`'s rule, `terminate` too) |
| A watch costs the reader thread | Lines are kept only while a watch is set; the regex crate runs in linear time; a line is cut at 64 KB |
| A pattern that never matches holds a call | Its timeout (at most 3600 s); `wait: false` doesn't hold the call |
| An agent opens blocks without end | Each is on the canvas and wired to it; the person closes them, Ctrl+Z takes each back |
| A fork takes itself for the original (both answer the same tasks) | Its first pasted line says it is a copy and names the original's block |
| Claude Code can't find a session from another folder | Checked by hand first; refused there if so |
| A role's `args` break the agent's command line | They are the person's words, typed as an `[agents]` command is; the agent says what is wrong in its block |
| A schedule types into an agent at a bad moment | The same checks as `delegate` at that moment; a refusal comes back to its caller; never into a dialog |
| A schedule's text runs as a command in a shell, at a moment nobody chose | Schedules go only to agents: one to a terminal running none is refused when it is set, and one whose agent left is refused when it comes, never typed into the shell |
| Schedules pile up unseen | The clock on the caller's block, its menu, `whoami`; 7 days at most; none survive a restart |
| A merge into the person's branch | Asked every time in the inbox; refused while the worktree, or the working tree it merges into, has uncommitted changes; a conflict is aborted, never left half done |
| Card 43 and card 35 change the same code at once | [Shared operations](#shared-operations) names who adds what; tasks look on `development` first and favor `ops.rs` |

## Calls made for card 35

The person was away and asked for the recommended option each time; each can be changed.

- **`stop_agent` ends the program, not only its turn**, and keeps the shell and the block. Killing only an agent's
  children would kill its MCP servers too. Only on blocks the caller opened, like `close_block`; no approval.
- **What `stop_agent` ends** is the program in front, what runs below it, and the orphans that carry the block's
  `TEMPLUM_TERMINAL`: a background command whose parent quit is found, even in a session of its own, while the shell,
  `login` and the person's jobs stay.
- **Lines for `wait_for` are read off the PTY stream**, not the grid: no line is missed or seen twice, past the
  scrollback too. Patterns are Rust regular expressions. A match answers once; `lines` looks back for a command that
  printed its line before the call; `wait: false` comes back as a notice, once.
- **`create_block` makes notes, checklists and terminals**; a terminal with a command is the background terminal (no
  separate tool, no `background` flag). New blocks are wired to the caller and closed by Ctrl+Z; only terminals keep
  `opened_by`, so an agent can't `close_block` a note it made.
- **`replace_note` takes an optional `old`** (one exact passage), as an editor's replace does; emptying a note is the
  person's.
- **Fork** for Claude Code and Codex, whose sessions Templum finds in their files, so a restored fork resumes its
  own; Pi and Grok Build wait until Templum can name a fork's session. A fork needs a prompt, whose first words tell
  the copy it is one; the session is the last snapshot's.
- **Roles live in `config.toml`**, written by the person; their "allowed tools" are the agent's own flags in `args`;
  instructions go with the prompt for every agent (no per-agent system prompt), so a role with instructions needs a
  prompt. The call's own agent, model and effort win.
- **`propose_plan` is `ask_user`'s dialog** with a Markdown body, Approve and Reject; Reject is an error result, so a
  script's `templum plan … &&` stops.
- **Schedules:** `at` (`HH:MM`), `after` and `every` in minutes, no cron; only to agents and the caller, delivered as
  the tool they stand for would at that moment; 7 days at most; not saved; the person sees and cancels them on the
  caller's block.
- **Worktrees:** removing a clean, merged worktree runs at once; discarding work and merging ask the person every
  time; a merge goes into the branch of the caller's folder (or `folder`'s), never over changes not committed there,
  and a conflict is aborted.
- **ReportFindings** is `create_card`, one card per finding; no new tool.
- **Shared operations** in one new file, `workspace/ops.rs`. Card 43's names are kept where its spec gave
  them (`add_block_in`, `NewBlock`, `Place`, `open_agent_in`); where a tool's method already has the tool's name
  (`replace_note`, `stop_agent` in the tool layer), the operation is named as card 43 names its own (`note_text`,
  `item_check`): `note_replace`, `item_edit`, `stop_in`, `worktree_remove`.

## Considered and left out

- **`stop_agent` as Ctrl+C twice** (Claude Code quits on it): Codex and the others differ, and a background command
  survives it.
- **`templum_terminal::tree` of the shell** (what a closing workspace ends): it ends the shell too, misses an orphan in a
  session of its own (Claude Code's tool shells), and on macOS takes `login`'s zsh for a child to end.
- **Process groups, cgroups or Windows Job Objects** to hold an agent's processes: each OS its own, and a program
  started before Templum set it up escapes anyway; the variable every process of the block inherits is one rule for
  all three.
- **Matching patterns against the screen's text**, as `read_terminal` reads it: lines scroll away between two looks,
  past the scrollback's end nothing tells how far, and a line seen in two looks would count twice.
- **A role's instructions as Claude Code's `--append-system-prompt-file`** and Codex's `-c developer_instructions`:
  better for those two, but two ways to do one thing, and nothing for the others.
- **A Settings section for roles:** `config.toml` is enough for the few roles a person keeps; Settings can list them
  later.
- **Asking the person before `stop_agent` on a block the caller didn't open:** `close_block` doesn't ask either, it
  refuses; `interrupt` covers the rest.
