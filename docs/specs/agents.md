# AI agents from the canvas

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [context-menus.md](context-menus.md), [persistence.md](persistence.md) (detecting and resuming
  agents) and [workspaces.md](workspaces.md)

## Goal

Templum is a cockpit for terminal AI agents. A right-click on the canvas opens any of the main ones directly, in the
project folder you pick, and each workspace remembers the folders you opened agents in.

**Success criteria**

1. **AI agent ▸** in the canvas menu lists every agent below; choosing one asks for a folder and opens it there.
2. Agents that aren't installed say how to install them.
3. Each workspace keeps its recent folders across restarts.
4. Every agent below is recognized in a terminal and comes back when the canvas is restored.
5. All tests in [Testing](#testing) pass in CI on the 3 OS.

## Out of scope

- Installing agents for the user, logging in, or passing them settings.
- A per-agent setting for extra arguments.
- Agents that need a terminal of their own outside Templum (Warp), or that are dead (Roo Code, Plandex, Mentat).

## The agents

Researched on 2026-10-10 from each agent's docs, repository and package files. **Resume** is used only when the
agent documents it as limited to the current folder; any other resume would bring back another project's
conversation, so those agents start a new session on restore.

| Agent | Command | Resume on restore | Install (the menu shows it) | Recognized by |
|---|---|---|---|---|
| Claude Code | `claude` | `claude --continue` | `curl -fsSL https://claude.ai/install.sh \| bash` | `claude`; `@anthropic-ai/claude-code` |
| Codex | `codex` | `codex resume --last` | `npm install -g @openai/codex` | `codex`; `@openai/codex` |
| Gemini CLI | `gemini` | `gemini --resume` | `npm install -g @google/gemini-cli` | `gemini`; `@google/gemini-cli` |
| GitHub Copilot CLI | `copilot` | new session | `curl -fsSL https://gh.io/copilot-install \| bash` | `copilot`; `@github/copilot` |
| Cursor CLI | `cursor-agent` | new session | `curl https://cursor.com/install -fsS \| bash` | `cursor-agent` (its `agent` alias is too common a name) |
| OpenCode | `opencode` | `opencode --continue` | `curl -fsSL https://opencode.ai/install \| bash` | `opencode`; `opencode-ai` |
| Amp | `amp` | `amp threads continue --last` | `curl -fsSL https://ampcode.com/install.sh \| bash` | `amp`; `@sourcegraph/amp` |
| Kiro CLI | `kiro-cli chat` | `kiro-cli chat --resume` | `curl -fsSL https://cli.kiro.dev/install \| bash` | `kiro-cli` |
| Qwen Code | `qwen` | `qwen --continue` | `npm install -g @qwen-code/qwen-code@latest` | `qwen`; `@qwen-code/qwen-code` |
| Grok Build | `grok` | `grok --continue` | `curl -fsSL https://x.ai/cli/install.sh \| bash` | `grok`; `@xai-official/grok` |
| Kimi Code | `kimi` | `kimi --continue` | `curl -fsSL https://code.kimi.com/kimi-code/install.sh \| bash` | `kimi`, `kimi-code`; `@moonshot-ai/kimi-code` |
| Mistral Vibe | `vibe` | `vibe --continue` | `curl -LsSf https://mistral.ai/vibe/install.sh \| bash` | `vibe` (a Python script) |
| Aider | `aider` | `aider --restore-chat-history` | `curl -LsSf https://aider.chat/install.sh \| sh` | `aider` (a Python script) |
| Goose | `goose session` | new session | `download_cli.sh` from the goose releases | `goose` |
| Crush | `crush` | new session | `npm install -g @charmland/crush` | `crush`; `@charmland/crush` |
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

## Opening one

- **The menu:** right-click on the empty canvas, **AI agent ▸**. Installed agents come first; the others follow
  under **Not installed**. The list scrolls.
- **Installed** means its program was found in PATH, in the folders where npm, bun, cargo, Deno, Volta, Go and nvm
  install programs, or among mise's and asdf's shims. An app started from a desktop menu may not get the shell's
  PATH, and version managers add theirs only in the shell.
- **Choosing one** opens a dialog, "Open *agent*":
  - when it isn't installed, a note with its install command, in the terminal's font;
  - **Recent in this workspace:** up to 8 folders, newest first, each with its name and its path (home as `~`).
    A folder that no longer exists isn't listed. Clicking one opens the agent there.
  - **Choose a folder…:** the system's folder picker (the XDG desktop portal on Linux). If it can't open, a
    notification says to type a path.
  - **A path field:** `~` is the home folder. **Open** or Enter opens it; a path that isn't a folder says so and
    keeps the dialog open.
- **Opening** puts a terminal of 110 × 32 cells with its top-left corner where you right-clicked, starts its shell
  in the folder, and types the agent's command and Enter. When the agent exits, the shell is still there.
- The folder becomes the workspace's newest recent folder.

## Files

A canvas file gets the workspace's recent folders, newest first:

```json
{ "version": 1, "camera": {…}, "items": […], "recent": ["/home/u/projects/app", "/srv/site"] }
```

Missing means none. Folders that aren't valid UTF-8 are left out. An older Templum ignores the field.

## Architecture

| File | Change |
|---|---|
| `templum-terminal/src/agent.rs` | The agents as a table (`Spec`), `Agent` an index into it; `agent_of` by names and packages; `installed` |
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

**Manual (Hyprland):** the folder picker through the portal; Claude Code opened from the menu, quit and resumed.
