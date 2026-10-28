# Agent usage and plan limits

- **Date:** 2026-10-14
- **Status:** Batch 1 (Claude Code, Codex) implemented; batches 2 to 4 planned
- **Builds on:** [agents.md](agents.md) (detecting agents, opening them), [shell-integration.md](shell-integration.md)
  (notifications), [settings.md](settings.md) and [config.md](config.md)

## Goal

Templum is a cockpit for CLI agents, and agents run on budgets: a context window that fills up, tokens, a cost, and
a subscription's rate limits (Claude Pro/Max and ChatGPT plans have a 5-hour window and a weekly one). A person
sees, without leaving the canvas, how much of each is used, when each limit resets, and is told when a limit nears
its end and when it has reset.

**Success criteria**

1. A terminal running Claude Code or Codex shows its session's usage in its header.
2. The title bar shows the tightest known plan limit; clicking it lists every agent's limits and the open sessions.
3. A notification says when a window crosses the warning line and when a window that was reached resets.
4. **Settings ▸ Usage** changes both notifications and turns the Claude Code status line off.
5. Templum never reads an agent's login credentials.
6. All tests in [Testing](#testing) pass in CI on the 3 OS.

## Out of scope

- Reading an agent's OAuth token or credential file to call a private API. Anthropic's terms forbid it for Claude
  Code (code.claude.com/docs/en/legal-and-compliance), and the other agents' private APIs are undocumented. A
  limit shows only when the agent hands it out through a documented channel or its own session files.
- Editing the user's Claude Code settings files.
- Computing a cost from a price table: an agent's own figure is shown, or tokens.
- History or charts of past usage.

## Where the data comes from

Researched on 2026-10-14 from each agent's docs and source, and checked against real files (Claude Code 2.1.286,
codex-cli 0.154.0).

| Batch | Agent | Session usage | Plan limits |
|---|---|---|---|
| 1 | Claude Code | Its status line (below); without it, the transcript | Its status line's `rate_limits` (Pro/Max, Claude Code 2.1.80+) |
| 1 | Codex | `token_count` events in its rollout files | Their `rate_limits` |
| 2 | Pi, Grok Build, Gemini CLI, Qwen Code, Kimi Code, Mistral Vibe, Droid, Amp, Cline, Aider | Their JSON/JSONL session files (Aider: `--analytics-log`) | — |
| 3 | OpenCode, Kilo Code, Crush, Goose | Their SQLite databases | — |
| 4 | Copilot CLI, Kiro CLI, Auggie, Amp | — (Copilot: its OpenTelemetry file) | Their official CLIs |
| — | Cursor CLI | Nothing on disk | Only a private API |

### Claude Code

Claude Code hands its status line command a JSON document on stdin
([statusline docs](https://code.claude.com/docs/en/statusline)): `session_id`, `model`, `workspace`, `cost`
(`total_cost_usd`), `context_window` (`used_percentage`, `context_window_size`, token totals) and, for Pro/Max
subscribers after the first answer, `rate_limits.five_hour` and `rate_limits.seven_day`, each
`{used_percentage, resets_at}` (epoch seconds). It runs the command again after each answer and when a window's
`resets_at` passes.

- **Only the Claude Code Templum types** (the canvas menu's **AI agent ▸**, and resuming on restore) gets
  `--settings <data>/claude-statusline.json`. That file sets `statusLine` to Templum's own program,
  `templum --claude-statusline <data>`, the **relay** (the data folder is named: Claude Code's environment may say
  another one; a program replaced by an upgrade while Templum runs is named by its path). `--settings` lasts one
  session and writes nothing. When `[connections]` is on, the same file also holds Templum's hooks
  ([connections.md](connections.md#hooks-and-agent-state)), and `claude_limits = false` drops only the status line.
  A `claude` typed by hand gets nothing added.
- **The relay** reads the document and writes it as is to `<data>/usage/claude/<terminal>.json`, where
  `<terminal>` is the terminal's `TEMPLUM_TERMINAL` environment variable (every terminal gets a unique one). Then it
  runs the user's own status line, if any, with the same input, and prints what it prints: `--settings` replaces
  the user's `statusLine` for that session. The user's is the first `statusLine` found in the project's
  `.claude/settings.local.json`, the project's `.claude/settings.json`, then `settings.json` in `$CLAUDE_CONFIG_DIR`
  or `~/.claude`. One that runs the relay itself is skipped. The relay always exits 0.
- **Typed path:** the settings file's path is typed as is, in double quotes when it has a space (macOS's
  `Application Support`), with `/` for `\` on Windows. A path with `"`, `$`, `%`, `!` or a backtick gets no flag.
- **Without the relay** (typed by hand): `~/.claude/sessions/<pid>.json` (Claude Code's own, undocumented) names the
  process's session; the last answer in its transcript,
  `~/.claude/projects/<folder with every non-alphanumeric as ->/<session>.jsonl`, gives the context in tokens
  (`input_tokens + cache_creation_input_tokens + cache_read_input_tokens`), without a percentage.
- **Stale files:** a relay file older than the agent's process (2 s of slack) is ignored. Relay files older than 8
  days are deleted when Templum starts.
- **Plan limits** are the account's: they come from the newest relay file that has them, or from asking Claude
  Code, whichever is newer. With several accounts of an agent ([accounts.md](accounts.md#usage-and-limits)),
  each account's limits are its own row.
- **Asking Claude Code:** the status line doesn't carry `rate_limits` for every plan (a Max plan's had none on
  2.1.286), so while `claude_limits` is on, Templum runs
  `claude --setting-sources local -p /usage --output-format stream-json --verbose --no-session-persistence` in an
  empty folder, `<data>/usage/probe` (30 s at most). `/usage` asks nothing of the model and costs nothing; with only
  that folder's settings, the user's hooks and plugins don't run; Claude Code reads its own login. Its assistant line
  carries `usage_report.rate_limits.limits[]`, each `{kind, group, percent, resets_at (ISO 8601), scope}`: `session`
  is the 5-hour window, `weekly_all` the week, `weekly_scoped` one model's week (`scope.model.display_name`). An API
  key has none. The program is the one Claude Code's command in effect names, found like any agent's.
- **When it asks:** for the agent's own folder and for each Claude Code account with a login (its `.claude.json` in
  its folder), whether it ran this run or not. At once when Templum starts, then when the meter turns to it, when
  the popover opens, when a Claude Code session of it ends a turn (that is when its usage changes), and every minute
  while the meter shows that one agent alone; otherwise every 5 minutes. Never the same account more than once a
  minute, and one probe at a time, the account asked for first. What it finds shows in the meter at once, not at the
  next read.

### Codex

Rollouts: `$CODEX_HOME` or `~/.codex`, `sessions/YYYY/MM/DD/rollout-*.jsonl`. The first line is `session_meta` with
the session's `cwd` and `timestamp`. `event_msg` lines of type `token_count` carry `info` (`total_token_usage`,
`last_token_usage`, `model_context_window`; may be null) and `rate_limits`
(`{limit_id, primary, secondary, rate_limit_reached_type}`, each window
`{used_percent, window_minutes, resets_at}`, may be null).

- **Which file is a process's:** the newest whose `cwd` is the process's folder and whose `timestamp` is no earlier
  than 5 s before the process started, in the folders of the process's start date (UTC) and a day either side. Two
  Codex in one folder each get their own file, newest process first.
- **Session:** the last `token_count` with `info`: tokens `total_token_usage.total_tokens`, context
  `last_token_usage.input_tokens / model_context_window`.
- **Plan limits:** the last `rate_limits` whose `limit_id` is `codex` (or absent), in the newest of the last week's
  rollouts that has them (of the 10 newest); a window missing a field is skipped.
- **On Linux** the rollout a Codex process, or its child (npm's `codex` starts the native one), has open
  (`/proc/<pid>/fd`) is its own, even one it resumed long ago.

## What a person sees

### On a terminal

A terminal running an agent with known usage shows a chip in its header's left side, after the status dot (Full
zoom only): the context used and the cost, `42% · $3.10`; tokens when there is no cost, `18% · 1.2M`; the context in
tokens when its size is unknown, `84k ctx`. Its tooltip spells it out: model, context, tokens, cost (an estimate at
API prices, which is not what a subscription pays).

### In the title bar

- **The meter**, left of the zoom level: one agent's limits at a time, its short name (an account's own name, so
  they are told apart: `Claude Code` is `Claude`, `Claude Code 2` is `Claude 2`, `Work` stays `Work`; a name over
  12 characters is cut with `…`) and each of its windows for every model, shortest first, with a small bar and,
  dim, when it resets: `Claude 5h 27% ▮ 4h 20m 7d 18% ▮ 3d 4h`.
  Bars are yellow from the warning line, red from 95%. With several agents known, the next one fades in every 8
  seconds or so (at the next read), never while the pointer is over it, and a dot per agent says which is shown. An
  agent hidden in **Settings ▸ Usage** isn't shown there. Hidden when no limit is known. A window whose reset time
  has passed is gone.
- **Clicking it** opens **Agent usage** under it: each agent's windows (bar, percentage, when it resets, as a
  countdown in at most two units: `in 25 min`, `in 4 hours 20 min`, `in 3 days 4 hours`), then **Open sessions**:
  every terminal with usage, in every workspace, as agent · folder · context · cost or tokens. Clicking a session
  reveals its terminal. Clicking outside or Esc closes it.

### Notifications

Delivered like a terminal's news: in the app when Templum's window is active, also as a system notification when
it isn't. Clicking one reveals a terminal running that agent.

- **Near a limit:** `Claude Code: 90% of the 5-hour limit used` / `Resets in 4 hours`. Once per window (a window is
  the same until its reset time moves by more than 10 minutes), when its use first reaches the warning line.
- **Reset:** `Claude Code's 5-hour limit has reset`. When the reset time passes, if the rule allows it given the
  highest use seen in that window:

| `reset` | Notifies when the window's peak was |
|---|---|
| `off` | never |
| `reached` (default) | 95% or more |
| `half` | 50% or more |
| `always` | above 0% |

95% counts as reached because an agent may stop reporting just under 100%.

Only limits read after Templum started count: an old 92% doesn't warn on every launch, and a reset that passed
while Templum was closed isn't announced.

### Settings ▸ Usage

| Row | Control | Key |
|---|---|---|
| Limit reset notifications | Off / When a limit was reached / After half of it / Always | `usage.reset` |
| Warn near a limit | Off / 80% / 90% / 95% | `usage.warn` |
| Claude Code limits | switch: Templum asks Claude Code for its limits, and adds its status line to the Claude Code it opens | `usage.claude_limits` |
| *Agent* in the title bar | switch, one per agent with limits (Claude Code, Codex) | `usage.hide` |

```toml
[usage]
reset = "reached"      # "off", "reached", "half" or "always"
warn = 90              # 0 (off), 80, 90 or 95
claude_limits = true
hide = ["codex"]       # agents whose limits the title bar doesn't show
```

`warn` outside those values and an unknown `reset` are config errors.

## Later batches

Each gets its own plan in `docs/plans/` when its turn comes; every format is checked against the agent's source and a
real file before its reader is written, since several weren't.

- **Batch 2, session files:** Pi (`~/.pi/agent/sessions/`, documented, with cost), Grok Build (`~/.grok/sessions/`,
  `updates.jsonl`), Gemini CLI (`~/.gemini/tmp/<project>/chats/`), Qwen Code (`~/.qwen/projects/`), Kimi Code
  (`~/.kimi-code/sessions/`, `wire.jsonl`), Mistral Vibe (`~/.vibe/logs/session/`), Droid (`~/.factory/sessions/`), Amp
  (`~/.local/share/amp/threads/`), Cline (`~/.cline/data/sessions/`); Aider through `--analytics-log` added to the
  command Templum types. One reader per agent in `templum-usage`, `Proc` to `Session`.
- **Batch 3, SQLite** (`rusqlite`, bundled, in `templum-usage` only; read-only): OpenCode and Kilo Code (their
  `session` table), Crush (`<project>/.crush/crush.db`), Goose (`sessions.db`).
- **Batch 4, plan limits through official commands**, polled every few minutes while that agent runs: Copilot CLI
  (`account.getQuota` over `copilot --headless --stdio`; also its OpenTelemetry file for the session), Kiro CLI
  (`/usage`), Auggie (`account status --json`), Amp (`amp usage`).
- **Cursor CLI:** nothing on disk, its quota only through a private API.

## Architecture

| File | Change |
|---|---|
| `templum-terminal/src/terminal.rs` | `Inspection` keeps the agent's `pid` and `started`; each terminal has an `id`, exported as `TEMPLUM_TERMINAL` |
| `templum-usage` (new crate, no UI) | `Session`, `Window`, `Limits`, `Proc`; `watch.rs` (the notification rules); `claude.rs` (relay, settings file, readers); `codex.rs` (rollouts); `read` |
| `templum/src/main.rs` | `--claude-statusline` runs the relay before any window |
| `templum/src/workspace/usage.rs` (new) | The typed `--settings`; reading usage after each snapshot; notifications |
| `templum/src/usage.rs` (new) | The meter, the popover, the chip, reset times |
| `templum/src/config.rs`, `settings/` | `[usage]`; Settings ▸ Usage |

Usage is read in the background after each 5-second snapshot, from the processes the snapshot found, so the
process table is read once. Without a data folder (`--bench`) nothing is read.

## Testing

**Unit (`templum-usage`):** warn once per window and again in the next; each reset rule; no event for a reset seen
already passed or for limits older than Templum; the relay keeps its input, prints the user's status line, prefers
`settings.local.json`, never runs itself, writes nothing for a bad terminal id; a stale relay file is ignored; the
newest relay file with limits wins; the transcript's last main answer; the typed path's quoting; pruning; Codex:
two processes in one folder, a file older than its process, null `info`, other `limit_id`, null windows.

**Unit (`templum`):** `[usage]` parses and rejects bad values; the template has it; the chip's text; reset times;
window labels.

**Headless UI:** every terminal has its own `TEMPLUM_TERMINAL`; a snapshot hands each terminal its agent's process;
Claude Code opened with a data folder gets `--settings`, not with `claude_limits = false`, Codex never; a window at
92% while Templum is in the background is one system notification, not two; the meter is hidden without limits,
opens the popover, and a session row reveals its terminal; Settings ▸ Usage writes `claude_limits = false`.

**Manual (Hyprland):** Claude Code from the menu shows the chip and, on Pro/Max, the meter; `claude` typed by hand
shows tokens; Codex shows its limits; the warning with `warn = 80` on an account past it; Claude Code's status
line with and without one of the user's own.
