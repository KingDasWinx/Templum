<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="crates/templum/icons/icon-dark.svg">
    <img src="crates/templum/icons/icon.svg" width="128" height="128" alt="">
  </picture>
</p>

<h1 align="center">Templum</h1>

<p align="center">
  <b>An infinite canvas for your terminals and your AI coding agents.</b><br>
  <a href="#see-it-work">See it work</a> · <a href="#install">Install</a> · <a href="#quick-start">Quick start</a> · <a href="docs/guide.md">Guide</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

Templum puts every terminal on one canvas you pan and zoom, like a whiteboard: place them anywhere, resize them,
group them by project. It is made for working with CLI agents such as Claude Code and Codex, side by side, wired to
each other and to the boards and notes they work from.

> **Early days (0.1).** Templum is young and moves fast. Bugs and ideas are welcome in the
> [issues](https://github.com/KingDasWinx/Templum/issues).

## What it does

- **Terminals on a canvas.** As many as you like, snapped to each other as you move them; pan, zoom, or fill the
  window with one block or a few tiled side by side.
- **A cockpit for AI agents.** Claude Code, Codex, Gemini CLI and 17 more open from a right-click, in the folder you
  pick. A dot on Claude Code's or Codex's title shows it working, or waiting for you.
- **Agents that work together.** Wire blocks with a cable: Claude Code, Codex, Gemini CLI and five more read and type
  into the terminals wired to them, work through a kanban's cards, hand tasks to each other and open new agents, in
  their own Git worktree if they want.
- **The Templum Agent.** Ask "a workspace for ~/proj with a kanban of 4 cards and a Claude Code per card", and it
  builds it on the canvas for you to watch.
- **Your plan limits in sight.** A Claude Code or Codex terminal shows its context and cost; the title bar shows your
  plan's 5-hour and weekly limits and warns you before you reach them.
- **Workspaces.** One canvas per project. Agents keep running in the workspaces you aren't looking at.
- **Everything beside the terminal.** Kanban boards, Markdown notes, checklists, a code editor, a Git popup that
  writes commit messages with AI, and pens and shapes to draw on the canvas.
- **Kept as you left it.** Every workspace and block comes back when Templum opens: terminals in their last folder,
  Claude Code, Codex and most other agents on their latest conversation.
- **Native and light.** Rust drawn on the GPU, no webview or Electron. macOS, Linux (Wayland and X11) and Windows.
  It follows your light or dark mode, takes any Alacritty theme, and updates itself.

## See it work

### Agents of different CLIs work together

A Claude Code opens an Amp beside itself and talks to it. Whichever CLI they run, agents reach each other through
Templum: they open one another, hand each other tasks and get the answers back.

https://github.com/user-attachments/assets/4d05f6c8-1ff0-4c1a-9b3b-ce302632231e

### An agent wired to a note, a checklist and a board

Draw a wire (the cable in the toolbar) from an agent to a block, and the agent can use it: it reads the note and adds
to it, ticks the checklist's items and adds new ones, and creates, moves and claims the board's cards. Here a Claude
Code works with all three at once.

https://github.com/user-attachments/assets/950bb59c-9abe-4028-bd13-c8561a8f18fb

### The Templum Agent opens a team

Ask the Templum Agent for a lead Claude Code and a few other agents (Amp, Codex and more), and it opens them on the
canvas. The lead splits the work and hands each agent its part; a dot runs along the wire while a task is out.

https://github.com/user-attachments/assets/d9dc5554-154f-4951-8be7-e11c805ed25b

### Fill the window with a grid

An agent and three terminals, selected and put in fullscreen, fill the window tiled side by side. Ctrl+Alt+arrows
moves the keyboard between tiles, and Ctrl+Shift+Enter puts every block back where it was on the canvas.

https://github.com/user-attachments/assets/325bb9ab-4feb-4a49-ab2a-3d0cf343dbde

### A code editor on the canvas

Ctrl+Shift+E opens a folder in an editor block: a file tree, tabs, syntax highlighting, search and replace, next to
the agents working on the same code.

https://github.com/user-attachments/assets/dbc8ff5f-f8fc-4b01-b0d1-fb0032f54e89

### Themes, settings and workspaces

The theme picker shows each theme on the whole window as you hover it. Settings apply as you change them. The
workspaces screen has a card per project, and makes new ones.

https://github.com/user-attachments/assets/ca179964-e74a-4a70-b458-4bc7cc08f062

## Install

### One command

**Linux and macOS**, in a terminal:

```sh
curl -fsSL https://github.com/KingDasWinx/Templum/releases/latest/download/install.sh | sh
```

**Windows**, in PowerShell:

```powershell
irm https://github.com/KingDasWinx/Templum/releases/latest/download/install.ps1 | iex
```

- **Linux:** Templum goes to `~/.local/bin/templum`, with an entry in your apps menu.
- **macOS:** `Templum.app` goes to Applications, and opens without the "can't verify" question.
- **Windows:** it runs Templum's setup for you only, without an administrator, with a Start menu shortcut.

Both check the download against its SHA-256 and install the newest release; `TEMPLUM_VERSION=0.1.2` (before `sh`,
or `$env:TEMPLUM_VERSION = "0.1.2"` before `irm`) installs another one. From then on, Templum updates itself.

### Arch Linux

[`templum-bin`](https://aur.archlinux.org/packages/templum-bin) is in the AUR:

```sh
yay -S templum-bin    # or: paru -S templum-bin
```

Your AUR helper updates it with your other packages.

### Or download a file

Every [release](https://github.com/KingDasWinx/Templum/releases/latest) has these files.

Each release's notes say what every file is for, and a `.sha256` beside each file has its checksum. The builds
aren't signed by an Apple or Microsoft account yet, so a downloaded file asks once:

- **macOS:** open Templum, choose *Done* when macOS says it can't verify it, then *System Settings ▸ Privacy &
  Security ▸ Open Anyway*. Or run `xattr -dr com.apple.quarantine /Applications/Templum.app`.
- **Windows:** on the SmartScreen prompt, *More info ▸ Run anyway*.

### From source

With [rustup](https://rustup.rs) and the prerequisites in [CONTRIBUTING.md](CONTRIBUTING.md#setup):
`git clone https://github.com/KingDasWinx/Templum && cd Templum && cargo run -p templum`.

## Quick start

1. **Open Templum.** The first time, a welcome lets you pick your theme, your shell and your AI agents.
2. **Open an agent.** Right-click the canvas, **AI agent ▸ Claude Code** (or any other), and pick its folder.
3. **Move around.** Drag the empty canvas or scroll to pan, Ctrl+scroll or pinch to zoom; drag a title bar to move
   a block, its bottom-right corner to resize it.
4. **Give it a board.** Ctrl+Shift+K adds a kanban block: create a board in it and write a few cards. Pick the
   cable in the toolbar and drag from the agent to the board to wire them, then ask the agent to work through the
   cards.
5. **Ask the Templum Agent.** Ctrl+Shift+Space (Cmd+Shift+Space on macOS), and say what you want on the canvas.

The keys worth knowing (Cmd instead of Ctrl on macOS, and Cmd+T for a new terminal):

| Keys | Action |
|---|---|
| Ctrl+Shift+T | New terminal |
| Ctrl+Shift+K / Ctrl+Shift+M / Ctrl+Shift+E | New kanban board / note / editor |
| Ctrl+Shift+G | The Git popup |
| Ctrl+Shift+Enter | Fill the window with the block, and back |
| Ctrl+Shift+1 | Fit every block in view |
| Ctrl+Shift+O | The workspaces screen |
| Ctrl+, | Settings |

Hold **Alt** (Option on macOS) to pan with a drag or zoom with the wheel anywhere, over blocks too.

## Learn more

- **[The guide](docs/guide.md):** every feature, every mouse action and shortcut, and `config.toml`.
- **[What changed](CHANGELOG.md)** in each release.
- **[The specs](docs/specs/)**: how each part behaves, in detail. **[ARCHITECTURE.md](docs/ARCHITECTURE.md)** and
  the [decision records](docs/adr/) explain how Templum is built: its terminals run on
  [`alacritty_terminal`](https://crates.io/crates/alacritty_terminal), its UI on [GPUI](https://www.gpui.rs), Zed's
  GPU framework, through [gpui-kit](https://github.com/longbridge/gpui-kit).

## Contributing

Contributions are welcome: read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues: see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). You may use, modify and sell Templum, as long as the copyright notice is kept.
