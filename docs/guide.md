# Using Templum

Everything Templum does, feature by feature. New here? Start with the [README](../README.md); the specs in
[specs/](specs/) go deeper still.

## The basics

The first time Templum opens, a welcome lets you pick a theme, a shell and your AI agents, and shows the keys worth
knowing. **Welcome** in the settings panel's footer shows it again.

| Mouse | Result |
|---|---|
| Drag a title bar / the bottom-right handle | Move / resize the block |
| Drag a block near another | It snaps to the other's edges or center within 8 px, and a line shows which |
| Scroll while dragging a block | The canvas moves under it, the block staying under the pointer |
| Drag empty canvas, or scroll over it | Pan |
| Hold Alt (Option on macOS), then drag or scroll anywhere | A drag pans and the wheel zooms, even over blocks: they don't react while Alt is held |
| Drag with the middle button, anywhere | Pan |
| Scroll over a terminal | Scroll its history; a scrollbar shows at its right while you scroll or point there |
| Drag a terminal's scrollbar, or click its track | Go to that place in its history |
| Click *120 lines below* in a scrolled-back terminal | Back to its live screen (typing goes back too) |
| Click, drag or scroll in a program that uses the mouse (htop, vim, Claude Code full screen) | The program gets it, its last columns too (no scrollbar over them); hold Shift to select text, or to scroll the history, instead |
| Ctrl+scroll, or pinch | Zoom, from 1% to 300%: far out, blocks show their titles, then only their shapes and a terminal's status dot |
| Drag in a terminal (double-click: word, triple-click: line) | Select text, copied to the clipboard as you let go (Settings ▸ Terminal turns that off) |
| Right-click empty canvas | Menu: **AI agent ▸** (Claude Code, Codex, Gemini CLI and 17 more) or **Add ▸** Terminal, Kanban board, Note, Checklist or Editor, where you clicked |
| Right-click a block, card, column, board or workspace tile | Its menu (copy, paste, move to another workspace, edit, stop, delete, close…) |
| With a drawing tool on, drag the canvas | Draw; Shift keeps lines at 45° and makes squares and circles |

| Linux / Windows | macOS | Action |
|---|---|---|
| Ctrl+Shift+T | Cmd+T | New terminal |
| Ctrl+Shift+K | Cmd+Shift+K | New kanban board block |
| Ctrl+Shift+M | Cmd+Shift+M | New note |
| Ctrl+Shift+E | Cmd+Shift+E | New editor (asks for a folder) |
| Ctrl+Shift+G | Cmd+Shift+G | Show or hide the Git popup |
| Ctrl+S | Cmd+S | Save the file (in an editor) |
| Ctrl+W | Ctrl+W | Close the file's tab (in an editor) |
| Ctrl+G | Cmd+G | Go to line (in an editor) |
| Ctrl+Shift+W | Cmd+W | Close the focused terminal |
| Ctrl+Shift+C / V | Cmd+C / V | Copy / paste |
| Ctrl+= / Ctrl+- | Cmd+= / Cmd+- | Zoom in / out |
| Ctrl+0 | Cmd+0 | Zoom 100% |
| Ctrl+Shift+1 | Cmd+Shift+1 | Fit all blocks |
| Ctrl+Shift+Enter | Cmd+Shift+Enter | Fullscreen: the focused block, or the selected ones tiled, fill the window; again to go back |
| Ctrl+, | Cmd+, | Settings |
| Ctrl+Shift+P | Cmd+Shift+P | Theme picker |
| Ctrl+Shift+Space | Cmd+Shift+Space | Ask the Templum Agent |
| Ctrl+Shift+A | Cmd+Shift+A | Go to the terminal waiting for you |
| Ctrl+Shift+F | Cmd+F | Find in the terminal |
| Ctrl+P | Cmd+P | Open a file by name (in an editor) |
| Ctrl+Tab / Ctrl+Shift+Tab | Ctrl+Tab / Ctrl+Shift+Tab | Next / previous workspace |
| Ctrl+1 … Ctrl+9 | Cmd+1 … Cmd+9 | Workspace 1 to 9 |
| Ctrl+Shift+N | Cmd+Shift+N | New workspace |
| Ctrl+Shift+O | Cmd+Shift+O | Home |
| Ctrl+Shift+Q, twice | Cmd+Q, twice | Quit (the first press only asks for the second) |

In a terminal taken over by a program (full screen, an AI agent, or one that asked for the mouse or the kitty
keyboard), zoom, settings, the theme picker and the prompt jumps leave their keys to it: Ctrl+- undoes in Claude Code.
Shift+Enter is a new line in Claude Code and Codex.

## Workspaces

Each workspace is a whole canvas of its own, such as one per project. The sidebar on the left shows them all: click
one to switch, drag one to reorder, **+** to add one. The button at the left of the title bar hides or shows the
sidebar. Terminals in the workspaces you aren't looking at keep running, so an agent keeps working while you look
elsewhere; a green dot on a tile means Claude Code or Codex is running in it.

- **Home** is where Templum starts, with the workspace you left loaded behind it (Esc goes back to it). It shows
  what waits for you across every workspace (agents that need you or finished, requests, automations that went
  wrong: press one to open it where it is), a card per workspace with what it holds ("3 terminals · 1 note", "2
  Claude Code running", "Opened 3 h ago"), and recent folders to start a workspace from. Type to find a workspace by
  name or folder, Up/Down and Enter to open it, drag cards to reorder. The house atop the sidebar, the workspace's
  name in the title bar and Ctrl+Shift+O open it again.
- **Edit…** (right-click a tile or a card) names a workspace and gives it a color, a Lucide icon or an image, and a
  folder: new terminals start there. Its **Share…** lets another computer's Templum mirror the workspace live and work in it.
- **Connect to a Templum…** (the plug under **+**) pastes a share's code and opens another computer's workspace here:
  its blocks and terminals live, where you can type, move blocks and answer its agents. See
  [api.md](api.md#templum-to-templum).
- **Stop** ends every program of a workspace (shells, agents, servers) and keeps its layout; opening it starts its
  terminals again, agents resumed. A stopped workspace stays stopped across restarts and costs nothing at launch.
- **Move to workspace ▸** (right-click a block or a selection) sends blocks to another workspace while they run: an
  agent keeps working there.

## Themes

Templum follows your system's light or dark mode. Click the **palette** button in the title bar to pick a theme: hovering or the
arrow keys preview it, and a click or Enter keeps it for the current mode. The default is Catppuccin: Latte in
light mode, Mocha in dark mode.

**Get more themes…**, at the bottom of the picker, opens a gallery of the themes of
[alacritty-theme](https://github.com/alacritty/alacritty-theme): hover one to see it on the whole window, click it to
install it. Any other theme in [Alacritty's format](https://alacritty.org/config-alacritty.html#colors) works too:
put its `.toml` file in the `themes` folder, and it shows up under "My themes" the next time you open the picker:

| OS | Folder |
|---|---|
| Linux, macOS | `~/.config/templum/themes/` (or `$XDG_CONFIG_HOME/templum/themes/`) |
| Windows | `%APPDATA%\templum\themes\` |

To make your own, open Settings ▸ Appearance: **Duplicate…** copies the theme in use into that folder, with
comments that say what each color paints. **Open file** opens it in an editor block, and saving it there (or in any
editor) shows the change at once. Besides Alacritty's colors, a theme can set three of Templum's:

```toml
[templum]
accent = "#ff8800"  # the focused block's border
canvas = "#101018"  # behind the blocks
dots = "#303040"    # the dot grid
```

Your choice is saved in `config.toml`, one folder up:

```toml
[theme]
light = "Catppuccin Latte"
dark = "Catppuccin Mocha"
```

## Settings

Press **Ctrl+,** (**Cmd+,** on macOS), or the ⚙ in the title bar, to open the settings panel on the right. It has
Appearance (light/dark mode and themes), Font (with a preview), Terminal (cursor and
scrollback, system notifications, copying a selection as you make it), Shell (for new terminals: pick one of the shells found on your computer, shell
integration), Tools (the `git` program), Agents, Usage (notifications about agents' plan limits) and Shortcuts. The terminals stay usable next to it, so you see each change
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

## Templum Agent

**Ctrl+Shift+Space** (**Cmd+Shift+Space** on macOS), or a click on the pill at the bottom of the canvas, asks the
Templum Agent: a Claude Code of yours, run in the background with your login, that builds and runs the canvas for
you. "Make a workspace for ~/proj with a kanban of 4 cards and a Claude Code per card" makes the workspace, the
board and its cards, and opens the agents wired to it with their prompts. It doesn't write code: every agent it opens
is a block you can watch, type into or stop. Each of its actions is a row in the chat; a click shows its block.
Closing a block where something of yours runs, typing into a terminal or pressing keys in one waits for your
**Allow once**, **Allow for this conversation** or **Deny**. Settings ▸ Templum Agent picks its model, its effort
and hides the pill. The conversation is kept across restarts; **New chat** starts another.

## Kanban boards

The toolbar's board button (or Ctrl+Shift+K) puts a kanban block on the canvas, Trello-style. Create a board
in it (it starts with To do, Doing and Done), or open one. Drag cards between columns and columns by their titles,
click a card to edit its title, description and colored labels, and right-click a column or the block to rename or
delete it. The description grows with what you write; drag the grip at its bottom-right corner to give it the height
you like, which every card's dialog keeps. The button at the left of the block's header opens the Boards dialog: find
a board, open it in the block, or delete it with its trash. Boards aren't tied to a canvas: any block, in any
workspace, can show any board, and blocks showing the same board stay in sync. Boards are kept in `boards.json`, next to
the `workspaces` folder.

## Notes

The toolbar's note button (or Ctrl+Shift+M) puts a note on the canvas. Write Markdown in it: headings, lists (Tab
indents an item), code. The eye button at the left of its header shows it rendered, and back; the first heading is
the note's title. Notes are kept app-wide, like boards: a note block shows some of them as tabs, closing a tab or a
block never loses one, and **Notes…** in the canvas menu finds every note again.

## Checklists

The toolbar's checklist button, or **Add ▸ Checklist** in the canvas menu, puts a checklist on the canvas: items you
tick off, with its progress in the title ("Deploy 2/5"). Checklists are kept app-wide too, and **Checklists…** in the
canvas menu finds them. An agent wired to one reads it, ticks items as it finishes them and adds new ones. See
[specs/checklist.md](specs/checklist.md).

## Fullscreen

**Ctrl+Shift+Enter** (**Cmd+Shift+Enter** on macOS), the button left of a block's close button, or **Fullscreen** in
its menu fills the window with the focused block, at its real size; with blocks selected (Shift+drag, Shift+click on
a title), they fill it tiled side by side. The same key goes back to the canvas, every block where it was. Ctrl+Alt+arrows
move the keyboard between tiles. See [specs/grid.md](specs/grid.md).

## Editor

The toolbar's editor button (or Ctrl+Shift+E) asks for a folder and puts an editor on it: a file tree on the
left (folders are read when you open them), tabs of open files, and a code editor with line numbers and syntax
highlighting for the usual languages. Ctrl+S saves (the file keeps its line endings), Ctrl+W closes the tab,
Ctrl+F searches, Ctrl+H replaces, Ctrl+G goes to a line; Markdown files get the note's preview button. A file
changed by another program is reloaded while you haven't edited it, and flagged with Reload / Keep mine when you
have. Right-click the tree for new files and folders, rename, delete, reveal in the file manager, and a terminal
in that folder beside the editor. Closing a block, a workspace or the window, or quitting, asks about unsaved
files first. A file that isn't UTF-8, or is over 4 MB, opens read-only, so its bytes never change; Tab indents
as the language wants it (a real tab in Makefiles and Go).

## Git

The Git button at the bottom of the sidebar (or Ctrl+Shift+G) opens the workspace's Git in a popup over the
canvas's bottom-left corner, with what GitHub Desktop and the editors' Git panels do; Esc or its ✕ closes it. Each
workspace has its own. The first time, it opens on the repository the focused terminal works in (else another
terminal's, else the workspace's folder, else the newest recent one; with none it asks for a folder). The ▾ beside
the repository's name switches to another: one a terminal of the workspace works in, a recent folder, or any
folder. **Changes** lists what is staged and what isn't; a row shows its diff, and its buttons stage, unstage,
discard (after asking) or open the file in an editor.
Below, a summary, a description, **Amend** and **Commit** (Ctrl+Enter from either field; with no author set, the
popup asks for `user.name` and `user.email`). **History** lists the commits with their files and diffs, filtered
as you type; right-click one to check it out, make a branch or a tag from it, revert it or copy its hash.
**Branches** checks out on a click, makes, renames, merges, deletes and publishes them, and offers **Stash and
switch** when local changes are in the way. **Stashes** stashes, applies, pops and drops. The top bar has the
branch, ↑↓ against its upstream, **Fetch**, **Pull** (fast-forward only; otherwise it offers Merge or Rebase) and
**Push** (a branch with no upstream asks before publishing); while one runs, its button cancels it. During a merge, rebase, cherry-pick or revert a banner
offers Abort and, once nothing conflicts, Continue; a conflicted file has Use ours, Use theirs and Mark resolved.

Everything runs through your own `git`, in the background, without ever waiting for input: a command that fails
shows git's message, with **Details** and **Run in a terminal**, which opens a terminal in the middle of the view
with the command typed in, for you to read and run, so your credential helper or SSH agent does what it does in any
terminal. The popup refreshes every 2 seconds while it is open, after each action, and when a terminal of the
workspace finishes a command in the repository, so `git add` in a terminal shows at once. Settings ▸ Tools names
the `git` program.

**Generate ✨**, beside the summary, has an AI agent write the commit message: Claude Code (`claude -p`) and
Codex (`codex exec`) work as they are, any other agent once its generate command is set in Settings ▸ Agents.
It gets the diff of what you commit (cut past 60 KB, the list of files always whole), the last 10 commit
summaries and your rules (Settings ▸ Git, or a repository's own from the ⚙ beside it), and only fills the
summary and the description, for you to read and commit. A second click stops it, and a message you typed
meanwhile is kept. The first time, it asks before sending your diff to the agent's provider. Settings ▸ Git
chooses the agent and its model, and **Try it** runs it on a sample change.

## The toolbar

The bar at the bottom of the canvas holds the tools: **Select** and **Hand** (move the canvas by dragging anywhere,
over blocks too; Esc goes back to Select), then a button for each kind of block (terminal, note, kanban board, editor) and
the AI agents, then the drawing tools.

## Drawing

The toolbar's pen, highlighter, shapes (line, arrow, rectangle, ellipse) and eraser draw on the canvas, over
the blocks. A pill above the toolbar picks the color (seven, from the theme) and the width. Esc goes back to
Select; then a click on a drawing selects it, a drag moves it, Delete removes it, and Ctrl+Z / Ctrl+Shift+Z
(Cmd on macOS) undo and redo while a drawing tool is on or a drawing is selected. Drawings are saved with the
canvas.

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
or a path you type. It opens a terminal there and starts the agent; agents you haven't installed are greyed out, and Settings ▸ Agents
shows their install command. **Settings ▸ Agents** changes any agent's command or resume command (↺ puts it back), hides
the ones you don't use from the menu, and adds your own agents. **Settings ▸ Agents ▸ Accounts** adds another
login of an agent by a name you type (`claudinho`, on your work plan): it is a command in every terminal of your
computer, and in the same menu. Choose what it shares with the first: settings, instructions, skills, plugins,
sessions, never the login, and Templum opens a terminal to log in ([specs/accounts.md](specs/accounts.md)). The full list, with how each one resumes, is in [specs/agents.md](specs/agents.md).

A terminal running Claude Code or Codex shows its session in its header: how much of the context is used, and the
cost or the tokens. The title bar shows each agent's plan limits in turn (the 5-hour and weekly windows of a Claude
or ChatGPT plan; **Settings ▸ Usage** picks which agents); click it for every agent's limits, when each resets, and
the open sessions. Templum asks Claude Code for its limits every 5 minutes (its `/usage`, which costs nothing). Templum tells
you when a limit nears its end and when one you reached resets (**Settings ▸ Usage**). Claude Code hands its limits
to its status line, so the Claude Code Templum opens gets `--settings` naming Templum's: your own status line still
shows. Templum reads only what the agents write, never their login. See [specs/usage.md](specs/usage.md).

## Agents that talk to the canvas

The Claude Code and Codex you open from **AI agent ▸** get Templum's MCP server, for that session only: your own
settings files stay as they are. Wire blocks together with the Wire tool (the cable in the toolbar) or a block's
**Connect to ▸**: an agent reads and types into the terminals wired to it. They can ask which block they are, list
the workspace's blocks, read and type into their own terminal, and send you a notification. An agent can ask you a
question, or to be wired to another block, in the requests inbox (the button above Git in the sidebar). The Agents
button above it (Ctrl+Shift+L) lists every agent of every workspace with its state, marks the ones that finished
while you weren't looking, and takes you to any of them. An agent can hand a task to an agent wired to it and get its
answer back (a dot travels along the wire meanwhile), and open another agent beside itself, in a new Git worktree if
it likes. An agent wired to a kanban reads its cards and creates, moves, comments on and claims them; it reads a
wired note and adds to it, and opens a file at a line in an editor. The dot before a Claude Code terminal's title
shows it working (accent) or waiting for you (yellow, "Waiting for you: …"). In any Templum terminal,
`templum blocks`, `templum whoami`, `templum read <block> -n 50`, `templum notify <text>`, `templum ask`,
`templum delegate`, `templum spawn`, `templum kanban`, `templum note` and `templum open` do the same from the
command line.
Every block has a short id (`b7k2m`) that lasts through restarts. Turn it off with `[connections] enabled = false`. See [specs/connections.md](specs/connections.md).

## The API

Settings ▸ **API** opens a door to your Templum for a phone, a dashboard or another computer: off until you turn it on,
and then it answers only to the **tokens** you make there (read, control or full; an expiry; optionally only some
workspaces). It serves every tool the Templum Agent has, the things a dashboard lists and a live stream of what changes.
Templum runs no server of its own: point a tunnel (cloudflared), a VPN or a proxy at its port, 7411. Off this computer
it needs HTTPS (Templum's certificate, with a fingerprint to pin, or yours). A title-bar pill shows while it is on, and
Settings lists who is connected. A terminal can be watched live (its screen, as it changes) and typed into through the API, and the canvas's camera follows too. See [api.md](api.md).

## Automations

The clock button in the sidebar's foot, above Git (Ctrl+Shift+J), opens **Automations**: rules that run at a time and
do one thing. **New** writes one: when it runs (once at a date and time, every N minutes or hours, every day at a time,
on chosen weekdays or days of the month, or a cron expression) and what it does (type a prompt into an agent, run a
command in a terminal or a new one, start an agent, or notify you). Each row tells when it runs next and how its last
run went. A prompt waits while its agent works, asks you something or while you type in it, and is never typed into a
shell; a time that passes while Templum is closed is marked Missed, not run. What agents asked for with `schedule` is
listed under **Asked by agents**, where you can cancel it.

A rule can also wait for an **event** instead of a time: **When an agent finishes its turn**, **When an agent needs
you**, or **When a command ends** (in a terminal you choose, all exits or failures only). **Who** is one agent, several
(**Any of them**, or for a turn's end **All of them**: the last to finish fires it once the others are idle) or **any
agent of a kind**, like every Claude Code. The rule's **Cooldown** (5 minutes unless you change it, 0 to 1440) is
counted for each agent that fires it, so an agent that finishes again soon doesn't run it twice. A prompt can go to
**The agent that fired it**: two Claudes that finish together each get their own, and one that is busy or has your
half-written message waits without holding up the other. New terminals and agents open beside the agent that fired
the rule, and a click on its notification shows that agent. The editor warns when a rule can loop (a prompt that ends
a turn of the agent it hears, an agent it starts that it hears again, a command that ends where it runs). Nothing runs
for what happened before the rule was turned on or Templum started.

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
settings. Turn the integration off in Settings ▸ Shell. Details: [specs/shell-integration.md](specs/shell-integration.md).

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
copy_on_select = true   # text selected with the mouse goes to the clipboard as you let go

[canvas]
focus_follows_pointer = true   # the block under the pointer takes the keyboard, no click needed

[usage]
reset = "reached"      # when a plan limit's reset is notified: "off" | "reached" | "half" | "always"
warn = 90              # notify once at this much of a limit: 0 (never) | 80 | 90 | 95
claude_limits = true   # Templum asks Claude Code for its limits
hide = ["codex"]       # agents whose limits the title bar doesn't show

[updates]
check = true   # look for a new version at start and once a day, and download it

[git]
program = "git"   # the Git popup's program: a name on PATH, or a path

[git.commit_message]   # Generate ✨: an AI agent writes the commit message
agent = "claude"       # an agent with a generate command (Settings ▸ Agents)
model = "haiku"
instructions = "Conventional Commits, English, imperative."

[keys]
new_terminal = "ctrl-shift-t"   # one shortcut, a list, or [] to disable
```

A mistake in the file (a typo, a value out of range) shows a notification saying where, and Templum keeps the
last working settings. On Windows, new terminals run Git Bash when Git for Windows is installed, else PowerShell 7,
else Windows PowerShell; Settings ▸ Shell lists every shell found (Git Bash, PowerShell 7, Windows PowerShell, WSL,
Nushell, Command Prompt). The full list is in [specs/config.md](specs/config.md).

## Updates

Templum looks for a new release at start and once a day, downloads the file made for your system and for the way
you installed it (an AppImage replaces the AppImage, the installer runs again, and so on), checks it against the
SHA-256 GitHub lists, and asks before restarting into it: **Restart to update** in the title bar. Templum installed
with a `.deb` or the release's `PKGBUILD` gets the install command typed in a terminal, for your password. Installed
from the AUR ([`templum-bin`](https://aur.archlinux.org/packages/templum-bin)), it is only told a version is out:
your AUR helper (`yay`, `paru`) updates it. Run from the disk image on macOS, it is only told too; a build from source
checks only on demand. **Settings ▸ Updates** checks on demand and turns the automatic check off
([specs/updates.md](specs/updates.md)).
