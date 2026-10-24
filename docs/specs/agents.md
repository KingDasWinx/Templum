# AI agents from the canvas

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [context-menus.md](context-menus.md), [persistence.md](persistence.md) (detecting and resuming
  agents) and [workspaces.md](workspaces.md)
- **Parity:** what each agent below gets of Templum's server, hooks, usage and flags, and what is missing:
  [agent-parity.md](agent-parity.md)
- **Accounts** (kanban card 37): several logins of one agent (`claudinho`, a name the person types), each on its own folder, sharing what the
  person chooses: [accounts.md](accounts.md)
- **Shells** (kanban card 66): each agent, the person's own and accounts too, opens in Settings ▸ Shell's
  default or a shell of its own (`shell` in its `[agents]` entry, a Shell list in its dialog):
  [shells.md](shells.md#a-shell-for-each-agent)

## Goal

Templum is a cockpit for terminal AI agents. A right-click on the canvas opens any of the main ones directly, in the
project folder you pick, and each workspace remembers the folders you opened agents in.

**Success criteria**

1. **AI agent ▸** in the canvas menu lists every agent below; choosing one asks for a folder and opens it there.
2. Agents that aren't installed can't be chosen, and Settings ▸ Agents says how to install them.
3. Each workspace keeps its recent folders across restarts.
4. Every agent below is recognized in a terminal and comes back when the canvas is restored.
5. All tests in [Testing](#testing) pass in CI on the 3 OS.

## Out of scope

- Installing agents for the user, logging in, or passing them settings (except the status line Claude Code gets for
  its plan limits, [usage.md](usage.md), and the hooks and the MCP server of [connections.md](connections.md)).
- A per-agent setting for extra arguments.
- Agents that need a terminal of their own outside Templum (Warp), or that are dead (Roo Code, Plandex, Mentat).

## The agents

Researched on 2026-10-10 from each agent's docs, repository and package files. **Resume** is used only when the
agent documents it as limited to the current folder; any other resume would bring back another project's
conversation, so those agents start a new session on restore.

**One session per block.** Several blocks of one agent in a folder each get their own conversation back, by its id:

- Claude Code, Codex and Qwen Code: the id is read from their files while saving
  ([persistence.md](persistence.md#agents); Qwen keeps `~/.qwen/sessions/<pid>.json` as Claude Code does), and
  restored with `claude --resume <id>`, `codex resume <id>`, `qwen --resume <id>`.
- Gemini CLI, Grok Build, Pi and GitHub Copilot CLI (since card 44): opened from the menu, they get `--session-id <a new
  UUID>`, and the id is read back from the process's arguments (`--session-id` or the resume flag). Restored with
  `gemini --resume <id>`, `grok --resume <id>`, `pi --session-id <id>`, `copilot --session-id <id>` (Pi's and
  Copilot's open it, or start it when it has no message yet). Gemini deletes a
  session with no message when it exits, and can't resume it: Templum then types `gemini --session-id <id>` again.

The command in the table is for a block whose session isn't known (an agent started by hand, a canvas from before),
or when the user changed the resume command.

| Agent | Command | Resume on restore | Install (the menu shows it) | Recognized by |
|---|---|---|---|---|
| Claude Code | `claude` | `claude --continue` | `curl -fsSL https://claude.ai/install.sh \| bash` | `claude`; `@anthropic-ai/claude-code` |
| Codex | `codex` | `codex resume --last` | `npm install -g @openai/codex` | `codex`; `@openai/codex` |
| Gemini CLI | `gemini` | `gemini --resume` | `npm install -g @google/gemini-cli` | `gemini`; `@google/gemini-cli` |
| GitHub Copilot CLI | `copilot` | new session; by its id since card 44 (its `--continue` falls back to any folder's latest) | `curl -fsSL https://gh.io/copilot-install \| bash` | `copilot`; `@github/copilot` |
| Cursor CLI | `cursor-agent` | `cursor-agent --continue` since card 44 (its chats are kept per folder) | `curl https://cursor.com/install -fsS \| bash` | `cursor-agent` (its `agent` alias is too common a name) |
| OpenCode | `opencode` | `opencode --continue` | `curl -fsSL https://opencode.ai/install \| bash` | `opencode`; `opencode-ai` |
| Amp | `amp` | `amp threads continue --last` | `curl -fsSL https://ampcode.com/install.sh \| bash` | `amp`; `@sourcegraph/amp` |
| Kiro CLI | `kiro-cli chat` | `kiro-cli chat --resume` | `curl -fsSL https://cli.kiro.dev/install \| bash` | `kiro-cli` |
| Qwen Code | `qwen` | `qwen --continue` | `npm install -g @qwen-code/qwen-code@latest` | `qwen`; `@qwen-code/qwen-code` |
| Grok Build | `grok` | `grok --continue` | `curl -fsSL https://x.ai/cli/install.sh \| bash` | `grok`; `@xai-official/grok` |
| Kimi Code | `kimi` | `kimi --continue` | `curl -fsSL https://code.kimi.com/kimi-code/install.sh \| bash` | `kimi`, `kimi-code`; `@moonshot-ai/kimi-code` |
| Mistral Vibe | `vibe` | `vibe --continue` | `curl -LsSf https://mistral.ai/vibe/install.sh \| bash` | `vibe` (a Python script) |
| Aider | `aider` | `aider --restore-chat-history` | `curl -LsSf https://aider.chat/install.sh \| sh` | `aider` (a Python script) |
| Goose | `goose session` | new session | `download_cli.sh` from the goose releases | `goose` |
| Crush | `crush` | `crush --continue` since card 44 (the project's own `.crush` database) | `npm install -g @charmland/crush` | `crush`; `@charmland/crush` |
| Droid | `droid` | new session | `curl -fsSL https://app.factory.ai/cli \| sh` | `droid` |
| Auggie | `auggie` | new session | `npm install -g @augmentcode/auggie` | `auggie`; `@augmentcode/auggie` |
| Kilo Code | `kilo` | `kilo --continue` | `npm install -g @kilocode/cli` | `kilo`, `kilocode`, `.kilo`; `@kilocode/cli` |
| Cline | `cline` | new session | `npm install -g cline` | `cline`, `.cline` |
| Pi | `pi` | `pi --continue` | `curl -fsSL https://pi.dev/install.sh \| sh` | `pi`; `@earendil-works/pi-coding-agent` |

Considered and left out for now: JetBrains Junie, Codebuff, Continue CLI (`cn`), OpenHands, gptme, Open
Interpreter, Qodo Command. Each is one more row in `templum-terminal/src/agent.rs` when someone wants it.

**Recognizing an agent** in a terminal's processes (on top of [persistence.md](persistence.md#agents)):

- its process or script name: the program, a process title the agent sets, or a native child an npm wrapper starts;
- for a `node` or `bun` process, a piece of its npm package's path in the script argument;
- for a `node` or `bun` process, an npm bin link with no extension, by its name.

## A model and an effort

`spawn_agent`'s `model` and `effort` become words after the agent's command (`launch_args`), on a restore and Ctrl+Z
too ([connections.md](connections.md#spawning)). Read from each agent's `--help` (installed) or its reference (not
installed) on 2026-10-18; every row but the first three came with card 44, each with the long `--model` every one of
them takes (`-m` too, for most); `cargo test -p templum-terminal -- --ignored installed_agents` checks the installed
ones' `--help` again. An agent not listed takes neither, and one with no effort list takes only a model. Where
the audit's gap 2 differs (Droid's effort), this table wins.

| Agent | Model | Effort | Efforts |
|---|---|---|---|
| Claude Code | `--model` | `--effort` | low, medium, high, xhigh, max |
| Codex | `-m` | `-c model_reasoning_effort=` | low, medium, high, xhigh, max, ultra |
| Gemini CLI | `-m` | — | |
| GitHub Copilot CLI | `--model` | `--reasoning-effort` | none, minimal, low, medium, high, xhigh, max |
| Grok Build | `--model` | `--reasoning-effort` | none, minimal, low, medium, high, xhigh, max |
| Pi | `--model` | `--thinking` | off, minimal, low, medium, high, xhigh, max |
| Kiro CLI | `--model` | `--effort` | low, medium, high, xhigh, max |
| Cline | `--model` | `--thinking` | none, low, medium, high, xhigh |
| Aider | `--model` | `--reasoning-effort` | low, medium, high (it passes any word to the model's API: the three every reasoning model takes) |
| OpenCode, Kilo Code | `--model` (`provider/model`) | — (only `run --variant`, not the TUI) | |
| Cursor CLI | `--model` | — (written in the model, `[effort=high]`, which a model Templum types can't hold) | |
| Qwen Code | `--model` | — (only its settings, `model.reasoningEffort`) | |
| Kimi Code | `--model` | — (`--thinking` is on or off) | |
| Droid, Auggie | `--model` | — (a TUI's effort isn't confirmed) | |

Left out: Amp (`--mode` only), Goose (`GOOSE_MODEL`, a variable), Crush (its TUI takes no model), Mistral Vibe
(unconfirmed).

## Opening one

- **The menu:** right-click on the empty canvas, **AI agent ▸**. Installed agents come first; the others follow
  under **Not installed**, greyed out: Settings ▸ Agents shows each one's install command ("Not installed · *command*")
  under its name. The list scrolls.
- **Installed** means its program was found in PATH, in the folders where npm, bun, cargo, Deno, Volta, Go and nvm
  install programs, or among mise's and asdf's shims. An app started from a desktop menu may not get the shell's
  PATH, and version managers add theirs only in the shell. The menu looks when it opens; Settings ▸ Agents (and its
  Add an account dialog) when the panel (the dialog) opens and when the agents change, never at each frame: each
  look scans all those folders.
- **Choosing one** opens a dialog, "Open *agent*":
  - **The workspace's folder** (chosen in its edit dialog, [cockpit.md](cockpit.md)), when it has one, tagged
    "Workspace folder"; it isn't listed again among the recent ones;
  - **The home folder**, tagged "Personal space", to open an agent quickly outside any project; not shown when it is
    the workspace's folder or can't be found, and not listed again among the recent ones;
  - **Recent in this workspace:** up to 8 folders, newest first, each with its name and its path (home as `~`).
    A folder that no longer exists isn't listed. Clicking one opens the agent there.
  - **Choose a folder…:** the system's folder picker (the XDG desktop portal on Linux). If it can't open, a
    notification says to type a path.
  - **A path field:** `~` is the home folder. **Open** or Enter opens it; a path that isn't a folder says so and
    keeps the dialog open.
- **Opening** puts a terminal of 110 × 32 cells with its top-left corner where you right-clicked, starts its shell
  in the folder, and types the agent's command and Enter. When the agent exits, the shell is still there.
- The folder becomes the workspace's newest recent folder.

## While one runs

Each terminal knows its folder and its agent as the 5-second snapshot read them (`Pane::seen`; the folder also
follows each finished command, with shell integration).

- **Shift+Enter** is a new line in the prompt, not a send. An agent that turned on the kitty keyboard (Claude Code,
  Codex) hears Shift+Enter itself; elsewhere a terminal running an agent sends Esc then Enter (what Claude Code's
  `/terminal-setup` binds, and Alt+Enter sends). A shell still gets Enter.
- **Closing** a terminal running an agent, or a command (shell integration), asks first and says why. A shell
  that exited closes at once.
- **Find** (`find`, Ctrl+Shift+F, Cmd+F on macOS) opens a field over the terminal: the text typed, read literally
  (lowercase matches either case), is found in the whole history, selected and scrolled into view; Enter goes to
  the older match, Shift+Enter to the newer, wrapping; Esc closes it and keeps the match selected. Nothing typed in
  the field reaches the shell.

## Changing ours, adding yours

**Settings ▸ Agents** lists every agent: Templum's, then the user's. Under them, **Accounts** lists the accounts of
an agent ([accounts.md](accounts.md#settings--agents--accounts)), which aren't rows of this list.

- **A row** shows the agent's name and its command (the user's say "Yours"), then the shell it opens in when it
  has one (`claude · in PowerShell 7`; `(not available)` after a shell this computer lacks). It has:
  - a switch: whether the canvas's menu lists it;
  - **Edit**, which opens a dialog;
  - **↺**, for one of Templum's the user changed: it goes back to Templum's defaults.
- **Reset all** next to the heading puts every changed agent of Templum's back. The user's agents stay.
- **Add an agent** under the list opens the same dialog, empty.
- **The dialog** has Name, Command, Resume command and **Shell**: Default (Settings ▸ Shell's shell) or one of the
  shells found, which the agent then always opens in ([shells.md](shells.md#a-shell-for-each-agent)).
  - For one of Templum's, each field shows Templum's value under it.
  - An empty resume command means a new session when the canvas comes back.
  - Save needs a name and a command; otherwise a notification says which is missing and the dialog stays.
  - One of the user's has **Delete this agent**.
- **Restoring a canvas** types the resume command in effect, so a changed one is used.
- **Recognizing it** in a terminal: a user's agent by the program its command runs (`~/bin/My-Agent --tui` is
  `my-agent`); one of Templum's by its usual names, plus the program of a changed command (a wrapper).

**config.toml** holds them under `[agents]`, one entry per agent, keyed by id:

```toml
[agents]
claude = { command = "claude --model opus" }   # only what differs from Templum's
codex = { show = false }
my-agent = { name = "My agent", command = "my-agent --tui", resume = "my-agent --continue" }
claude-wsl = { name = "Claude Code (WSL)", command = "claude", shell = "WSL" }   # opens in a shell of its own
```

- `[agents.claude]` tables written by hand work too.
- For one of Templum's, each missing key keeps its default, and saving it back at its defaults removes its entry.
- An id is letters, digits, `-` and `_`. A new agent's id comes from its name (`My Agent` is `my-agent`, then
  `my-agent-2`), never one of Templum's.
- One of the user's needs `name` and `command`; a `command` that is set can't be empty. A file breaking these
  rules is reported like any other config error, and the last valid config stays.

## Files

A canvas file gets the workspace's recent folders, newest first:

```json
{ "version": 1, "camera": {…}, "items": […], "recent": ["/home/u/projects/app", "/srv/site"] }
```

Missing means none. Folders that aren't valid UTF-8 are left out. An older Templum ignores the field.

## Architecture

| File | Change |
|---|---|
| `templum-terminal/src/agent.rs` | Templum's agents as a table (`Spec`); `Detect` and `agent_of`, given the agents to look for; `installed` |
| `templum/src/agents.rs` (new) | The agents in effect (ours with `[agents]` changes, then the user's), the dialog's `Draft`, ids |
| `templum/src/config.rs` | `[agents]`, validated; an edit's key can be an agent's id |
| `templum/src/settings.rs` | The Agents section and its dialog |
| `templum/src/launcher.rs` (new) | The folder dialog; the system folder picker; typed paths |
| `templum/src/menu.rs` | **AI agent ▸** in the canvas menu |
| `templum/src/workspace.rs` | `launch_agent`; `Space::recent` and `remember` |
| `templum/src/session.rs` | `recent` in the canvas file |

## Testing

**Unit:**

- every agent's processes, as the research found them (npm, native, Python, renamed titles), and processes that
  aren't agents;
- every agent has a unique id, its program among its names, and a resume or start command;
- a resume not limited to the folder isn't used;
- programs are found only where they can run;
- typed paths expand `~`;
- recent folders: newest first, no repeats, at most 8;
- the canvas file keeps the recent folders, and an older file without them loads.

**Headless UI:**

- an agent opens in its folder, at its point, typed into its shell, and the folder becomes recent;
- right-click the canvas, **AI agent ▸** an agent, then a recent folder: the agent's terminal opens.
- the folder dialog offers the workspace's folder first, and not again among the recent ones.
- the folder dialog offers the home folder under it, once, and not when it is the workspace's folder.

**Manual (Hyprland):** the folder picker through the portal; Claude Code opened from the menu, quit and resumed.
