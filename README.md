# Templum

An infinite canvas of terminals. Spawn as many terminals as you like, place them anywhere,
resize them like blocks, and pan and zoom around your workspace.

> **Status: pre-alpha.** Usable as a canvas of terminals; no session saving or settings yet.

## Why

- **Native and light.** Pure Rust, drawn directly on the GPU. No webview, no Electron.
- **Cross-platform.** macOS, Linux (Wayland and X11) and Windows.
- **Built on proven parts.** Terminal emulation by [`alacritty_terminal`](https://crates.io/crates/alacritty_terminal),
  UI by [GPUI](https://www.gpui.rs) (Zed's GPU framework) through [gpui-kit](https://github.com/longbridge/gpui-kit).
  See [the architecture](docs/ARCHITECTURE.md) and the [decision records](docs/adr/).

## Build from source

You need [rustup](https://rustup.rs); the pinned toolchain installs itself.
Platform prerequisites are listed in [CONTRIBUTING.md](CONTRIBUTING.md).

```sh
git clone https://github.com/KingDasWinx/Templum
cd Templum
cargo run -p templum
```

## Using it

| Mouse | Result |
|---|---|
| Double-click empty canvas | New terminal there |
| Drag a title bar / the bottom-right handle | Move / resize the block |
| Drag empty canvas, or scroll over it | Pan |
| Hold Alt (Option on macOS), then drag or scroll anywhere | Pan, even over blocks: they don't react while Alt is held |
| Drag with the middle button, anywhere | Pan |
| Scroll over a terminal | Scroll its history |
| Ctrl+scroll, or pinch | Zoom |
| Drag in a terminal (double-click: word, triple-click: line) | Select text |
| Right-click empty canvas | Menu: **AI agent ▸** (Claude Code, Codex, Gemini CLI and 17 more) or **Add ▸** Terminal or Kanban board, where you clicked |
| Right-click a block, card, column, board or workspace tile | Its menu (copy, paste, rename, delete, close…) |

| Linux / Windows | macOS | Action |
|---|---|---|
| Ctrl+Shift+T | Cmd+T | New terminal |
| Ctrl+Shift+K | Cmd+Shift+K | New kanban board block |
| Ctrl+Shift+W | Cmd+W | Close the focused terminal |
| Ctrl+Shift+C / V | Cmd+C / V | Copy / paste |
| Ctrl+= / Ctrl+- | Cmd+= / Cmd+- | Zoom in / out |
| Ctrl+0 | Cmd+0 | Zoom 100% |
| Ctrl+Shift+1 | Cmd+Shift+1 | Fit all blocks |
| Ctrl+, | Cmd+, | Settings |
| Ctrl+Tab / Ctrl+Shift+Tab | Ctrl+Tab / Ctrl+Shift+Tab | Next / previous workspace |
| Ctrl+1 … Ctrl+9 | Cmd+1 … Cmd+9 | Workspace 1 to 9 |
| Ctrl+Shift+N | Cmd+Shift+N | New workspace |
| Ctrl+Shift+Q, twice | Cmd+Q, twice | Quit (the first press only asks for the second) |

## Workspaces

Each workspace is a whole canvas of its own, such as one per project. The sidebar on the left shows them all: click
one to switch, **+** to add one, and right-click a tile to rename or delete it. The button at the left of the title
bar hides or shows the sidebar. Terminals in the workspaces you aren't looking at keep running, so an agent keeps
working while you look elsewhere; a green dot on a tile means Claude Code or Codex is running in it.

## Themes

Templum follows your system's light or dark mode. Click the **palette** button in the title bar to pick a theme: hovering or the
arrow keys preview it, and a click or Enter keeps it for the current mode. The default is Catppuccin: Latte in
light mode, Mocha in dark mode.

Any theme in [Alacritty's format](https://github.com/alacritty/alacritty-theme) works. Put its `.toml` file in the
`themes` folder, and it shows up under "My themes" the next time you open the picker:

| OS | Folder |
|---|---|
| Linux, macOS | `~/.config/templum/themes/` (or `$XDG_CONFIG_HOME/templum/themes/`) |
| Windows | `%APPDATA%\templum\themes\` |

Your choice is saved in `config.toml`, one folder up:

```toml
[theme]
light = "Catppuccin Latte"
dark = "Catppuccin Mocha"
```

## Settings

Press **Ctrl+,** (**Cmd+,** on macOS), or the ⚙ in the title bar, to open the settings panel on the right. It has
Appearance (light/dark mode and themes), Font (with a preview), Terminal (cursor and
scrollback, system notifications), Shell (for new terminals: pick one of the shells found on your computer, shell
integration), Agents and Shortcuts. The terminals stay usable next to it, so you see each change
as you make it.

- Every change applies at once and is saved to `config.toml` a moment later, keeping your comments. Numbers apply
  when you press Enter or leave the field. A field's ↺ button puts it back to its default, and **Reset all** does
  the whole section, by removing their lines from the file.
- To change a shortcut, click it and press the new one, with Ctrl, Alt or Super (Cmd on macOS), or an F key. Esc
  cancels, Backspace leaves the command without a shortcut, and a shortcut another command already has is refused
  with its name. A reset that would give a shortcut to two commands is refused the same way.
- Shortcuts keep working while the panel has the focus, except copy, paste and close terminal, which need a terminal
  to have it.
- Edits you make by hand to `config.toml` show up in the panel.

## Kanban boards

The board button in the title bar (or Ctrl+Shift+K) puts a kanban block on the canvas, Trello-style. Pick a board
in it or create one: it starts with To do, Doing and Done. Drag cards between columns and columns by their titles,
click a card to edit its title, description and colored labels, and right-click a column or a board to rename or
delete it. Boards aren't tied to a canvas: any block, in any workspace, can show any board (the ⇅ button at the left
of its header switches), and blocks showing the same board stay in sync. Boards are kept in `boards.json`, next to
the `workspaces` folder.

## Your canvas is kept

Templum saves every workspace as you go (every few seconds, and when it quits) and brings them back when it opens:
the view, every terminal's place and size, and a new shell in each terminal's last folder. What was on screen
doesn't come back. The files are in `~/.local/share/templum/workspaces/`
(`~/Library/Application Support/Templum/workspaces/` on macOS, `%APPDATA%\templum\workspaces\` on Windows): one
per workspace, and `index.json` listing them. If Templum can't read a file, it keeps it as `.bak` and starts that
canvas fresh.

If an AI agent was open in a terminal, Templum brings it back there: Claude Code with `claude --continue`, Codex
with `codex resume --last`, and so on for the 20 agents it knows, continuing that folder's latest conversation (or
a new one, for agents that can't resume by folder).

## AI agents

Right-click the canvas, **AI agent ▸**, and pick one: Claude Code, Codex, Gemini CLI, GitHub Copilot CLI, Cursor
CLI, OpenCode, Amp, Kiro, Qwen Code, Grok Build, Kimi Code, Mistral Vibe, Aider, Goose, Crush, Droid, Auggie, Kilo
Code, Cline or Pi. Templum asks which folder it runs in: one of this workspace's recent folders, a folder you choose,
or a path you type. It opens a terminal there and starts the agent; agents you haven't installed show their install
command instead. **Settings ▸ Agents** changes any agent's command or resume command (↺ puts it back), hides
the ones you don't use from the menu, and adds your own agents. The full list, with how each one resumes, is in [docs/specs/agents.md](docs/specs/agents.md).

## Shell integration

Templum loads a small script into bash, zsh, fish and PowerShell (Git Bash and Windows PowerShell included), as
Ghostty, Kitty and VS Code do, so the shell tells it its folder and when each command starts and ends. Your shell and
its files stay as they are: they are read as usual. With it:

- the dot before a terminal's title shows a command running, and red when the last one failed;
- a terminal you aren't looking at that ends a command of 5 seconds or more, or asks for attention (a bell, or an
  agent's notification), gets a dot on its block and on its workspace's tile, and a notification; click it to go
  there. With Templum in the background it's also a system notification (Settings ▸ Terminal ▸ System
  notifications);
- new terminals open in the focused one's folder in PowerShell too;
- each command gets a dot in the terminal's left margin, green or red by how it ended, and **Ctrl+Shift+↑** /
  **Ctrl+Shift+↓** (**Cmd+↑** / **Cmd+↓** on macOS) jump to the previous or next command's prompt.

Codex rings the bell when it needs you. For Claude Code, set `"preferredNotifChannel": "terminal_bell"` in its
settings. Turn the integration off in Settings ▸ Shell. Details: [docs/specs/shell-integration.md](docs/specs/shell-integration.md).

## Configuration

Everything is in `config.toml`, in the folder from [Themes](#themes) (`~/.config/templum/` on Linux and macOS,
`%APPDATA%\templum\` on Windows). Press **Ctrl+,** (**Cmd+,** on macOS), or the ⚙ in the title bar, to open the
settings panel; every change applies at once and is saved to the file, comments included. Its **Edit config.toml**
button opens the file in your editor: if it doesn't exist yet, Templum writes it first with every option commented
out. Uncomment a line and save, and it applies at once.

```toml
[font]
family = "JetBrains Mono"
size = 14

[theme]
mode = "system"   # "system" | "light" | "dark"

[shell]
program = "fish"
args = ["--login"]
working_directory = "inherit"   # "inherit" (the focused terminal's folder) | "~" | a path
integration = true              # see Shell integration

[cursor]
shape = "block"   # "block" | "bar" | "underline"
blink = false

[terminal]
scrollback = 10000
notifications = true   # system notifications while Templum is in the background

[keys]
new_terminal = "ctrl-shift-t"   # one shortcut, a list, or [] to disable
```

A mistake in the file (a typo, a value out of range) shows a notification saying where, and Templum keeps the
last working settings. On Windows, new terminals run Git Bash when Git for Windows is installed, else PowerShell 7,
else Windows PowerShell; Settings ▸ Shell lists every shell found (Git Bash, PowerShell 7, Windows PowerShell, WSL,
Nushell, Command Prompt). The full list is in [docs/specs/config.md](docs/specs/config.md).

## Pre-built binaries

There are no releases yet. When they arrive on the [releases page](https://github.com/KingDasWinx/Templum/releases),
they will be **unsigned** at first:

- **macOS:** right-click the app → *Open* → *Open* (only needed the first time).
- **Windows:** on the SmartScreen prompt, click *More info* → *Run anyway*.

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues: see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). You may use, modify and sell Templum, as long as the copyright notice is kept.
