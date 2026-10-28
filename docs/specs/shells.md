# Shells

- **Date:** 2026-10-11; [A shell for each terminal](#a-shell-for-each-terminal) and
  [A shell for each agent](#a-shell-for-each-agent) 2026-10-20; [A scope for each terminal](#a-scope-for-each-terminal)
  2026-10-24
- **Status:** Implemented, *A shell for each terminal* too (kanban card 60,
  [plan](../plans/2026-10-20-shell-per-terminal.md)); *A shell for each agent* too (kanban card 66,
  [plan](../plans/2026-10-20-shell-per-agent.md)); *A scope for each terminal* too (kanban card 173)
- **Builds on:** [config.md](config.md) (`[shell]`), [settings.md](settings.md),
  [context-menus.md](context-menus.md), [persistence.md](persistence.md),
  [shell-integration.md](shell-integration.md), [agents.md](agents.md) and [accounts.md](accounts.md)

## Goal

On Windows a terminal ran Windows PowerShell 5.1, `alacritty_terminal`'s default when Templum names no program: a
plain prompt, few colors, and a poor fit for AI agents, which expect a POSIX shell. Templum now picks a better
default there, and the settings list the shells found on the computer, on every OS.

## The default

| OS | When `shell.program` is not set |
|---|---|
| Windows | The first found of **Git Bash** (`bash.exe --login -i`), **PowerShell 7** (`pwsh -NoLogo`), **Windows PowerShell** (`powershell -NoLogo`) |
| Linux, macOS | The user's login shell (`$SHELL`), as before |

## Finding shells

**Windows**, in this order (each only when its file exists):

| Shell | Where | Arguments |
|---|---|---|
| Git Bash | `bin\bash.exe` beside the `git` in PATH (`…\Git\cmd\git.exe`), else `Git\bin\bash.exe` under `%ProgramFiles%`, `%ProgramFiles(x86)%`, or `%LOCALAPPDATA%\Programs` | `--login -i` |
| PowerShell 7 | `pwsh` in PATH, else `%ProgramFiles%\PowerShell\7\pwsh.exe` | `-NoLogo` |
| Windows PowerShell | `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`, else `powershell` in PATH | `-NoLogo` |
| WSL | `%SystemRoot%\System32\wsl.exe` | none |
| Nushell | `nu` in PATH | none |
| Command Prompt | `%SystemRoot%\System32\cmd.exe` | none |

Git for Windows' login profile doesn't change folder (its `/etc/profile`, checked on 2026-10-11), so Git Bash starts
in the terminal's folder, which agents opened in a project need.

**Linux and macOS:** the shells `/etc/shells` lists that exist, once each by name, in its order, leaving out
`nologin`, `false`, `git-shell`, `rbash`, `tmux`, `screen` and `systemd-home-fallback-shell`; then the known shells it
doesn't name, and `-l` on macOS ([Finding shells, completed](#finding-shells-completed)).

## Settings

**Settings ▸ Shell** gets a **Shell** row above Program and Arguments:

- a list: **Default (*its name*)**, then every shell found;
- choosing one sets `shell.program` (its path) and `shell.args` (its arguments); **Default** removes both;
- a program typed by hand that isn't in the list shows as **Custom**;
- ↺ goes back to the default.

The shells are found when the panel opens. Like the rest of Shell, the choice applies to new terminals.

## Testing

**Unit (`templum-terminal`):** with a made-up Windows (paths and PATH):

- every shell found, in order, with its arguments, and Git Bash as the default;
- Git Bash beside the `git` in PATH;
- without Git Bash, PowerShell 7, then Windows PowerShell by name when nothing is found;
- `/etc/shells` read once per name, without the excluded ones and missing files.

**Headless UI:** choosing PowerShell 7 in the list sets the program and its arguments in config.toml, and ↺ removes
them.

**Manual (Windows):** a new terminal runs Git Bash in the right folder; an agent opened from the canvas runs in it;
the list shows the installed shells. Not checked yet: no Windows machine at hand.

## A shell for each terminal

Kanban card 60, from the person: *"Terminals with different shells: when creating a terminal, list the options and
let me pick one: in the right-click menu the terminal item gets a submenu that lists them, and in the action bar a
click on the terminal opens a small dropdown upward, like the agents'."*

### Goal

Settings ▸ Shell picks the shell of **every** new terminal. A person who works mostly in zsh but now and then wants
fish, Nushell, PowerShell 7 or WSL had to change the setting, open a terminal and change it back. Now each new
terminal can run any shell found on the computer, chosen where terminals are made, and keeps it across restarts.
The setting stays what it was: the default.

**Success criteria**

1. The toolbar's Terminal button and the canvas menu's **Add ▸ Terminal ▸** list the shells found; the one chosen
   runs in the new block, on Linux, macOS and Windows.
2. Ctrl+Shift+T (Cmd+T) and every other way a terminal opens still run the default, as before.
3. A terminal opened with a chosen shell comes back with it after a restart, an undo of its close, or a paste.
4. All tests in [Testing](#testing-1) pass in CI on the three OS, whatever shells the machine has.

**Not in scope:** a shell for agents (AI agent ▸ opens its agent in the default shell, as now), a shell argument for
the agents' `create_block`, new integration scripts, a setting for the menu, shells over SSH.

### Finding shells, completed

The list is `available_shells()`, the one Settings ▸ Shell shows, with two additions on Linux and macOS:

- **Known shells on the PATH.** After `/etc/shells`, each of `bash`, `zsh`, `fish`, `nu`, `pwsh` that it didn't
  name, where Templum finds programs (`templum_terminal::locate`: the PATH, the login shell's PATH, `~/.cargo/bin`…),
  by its path. Homebrew's fish and zsh, and Nushell from cargo, aren't in `/etc/shells` until someone runs `chsh`.
- **Login shells on macOS.** Every shell found gets `-l`, as the default (`$SHELL`, which alacritty starts as a login
  shell there): a shell started from Finder's Templum otherwise misses `/etc/zprofile`'s `path_helper` and the
  `brew shellenv` of `~/.zprofile`. bash, zsh, fish, pwsh, nu, csh, tcsh, ksh, dash and sh all take `-l`. A shell
  chosen in Settings before this change has no `-l`: Settings shows it as **Custom** until it is chosen again.

Labels stay as they are: Windows names (**Git Bash**, **PowerShell 7**, **Windows PowerShell**, **WSL**,
**Nushell**, **Command Prompt**), the program's file name elsewhere (**zsh**, **fish**, **nu**).

The list and the default's name are read when Templum starts and, as before, when Settings or the welcome opens
(`Workspace::shells`, `Workspace::default_shell`). Reading them costs a file read and a few `stat`s.

The login shell's PATH is read on a thread that starts with Templum, and a list read before it arrives may lack a shell
only it has. Nothing waits for it: the window must not be late because a startup file is slow. Where programs are
looked up (`program_dirs`) also lists the usual package managers' folders (Homebrew's on macOS and Linux, Nix's,
MacPorts', `~/.cargo/bin`), so Homebrew's fish from the Dock is found at once. A list read early
(`Workspace::shells_early`) is read again by the first menu opened after the PATH arrives. A restored terminal that
names a shell the list lacks opens the default and forgets it (see [minors](../minors.md), *A shell only the login
shell's PATH finds*).

### The menus

Both menus list the same items, built by one function:

| Item | Opens |
|---|---|
| **Default (*name*)** | What Ctrl+Shift+T opens: `shell.program` and `shell.args` when set, else the system's default. *Name* is the label of the shell found with that program and those arguments, else the program's file name; with no program set, `Workspace::default_shell` (the name Settings shows) |
| — | A separator, when other shells follow |
| *each shell found* | That shell, with its arguments. The one labelled as the default's *name* is left out: it is the first item already |

- **The toolbar's Terminal button** opens this menu at the click, as **AI agent** opens the agents: the menu is kept
  inside the window, so from the toolbar at the bottom it opens upward. The block opens where a new terminal goes
  (`new_terminal_rect`: the view's center, or beside the blocks there). The tooltip still names Ctrl+Shift+T, which
  opens the default without a menu. The menu opens even when the default is the only item: one more click, the
  same gesture every time.
- **The empty canvas's menu**: **Add ▸ Terminal** becomes **Add ▸ Terminal ▸**, a submenu with the items above;
  the block's top-left corner goes where the person right-clicked. The same in grid mode's **Add ▸** (a tile's
  menu, a gap's), where the block takes half of the tile, as now.
- The list scrolls past 420 px, as the agents' does (macOS lists eight shells).

### The block keeps its shell

- A terminal opened from a shell item remembers it (`Pane::shell`: the shell found, label, program and arguments).
  One opened with **Default** or any other way remembers nothing and follows the default.
- Its saved item gets `"shell"`: the shell's **label** (`"fish"`, `"PowerShell 7"`), written only when set; an older
  Templum ignores it. Undo of a close and Copy then Paste keep it: both go through `Pane::item` and `add_saved`.
  Move to workspace moves the running block, its shell with it.
- At startup and from a saved item (`add_saved`: undo, paste), the block starts the shell of that label among the
  shells found (`Workspace::shells`). **Safety:** the file names a shell, never a command, as it names an agent
  ([persistence.md](persistence.md)): an edited file can only pick a shell the computer lists.
- **A label not among the shells found** (uninstalled, another computer's, edited) starts the default instead, and
  the block forgets it: the canvas loses no block for a shell that is gone. A line goes to stderr; no notification.
- The block's title is the shell's file name until the shell sets one (`Terminal::title`), as now.

### Shell integration

Unchanged, and decided by the program's file name ([shell-integration.md](shell-integration.md#loading-the-scripts)):
bash (Git Bash too), zsh, fish, PowerShell 7 and Windows PowerShell get it, with `-l`/`--login` handled as now. The
others (sh, dash, ksh, csh, tcsh, Nushell, Command Prompt, WSL) open without it: no command marks or prompt jumps, no
"Finished after…" news, and the folder comes from the OS. `shell.integration` off turns it off for every shell.

### Agents typed in a chosen shell

What a new terminal's environment gets for the agents a person types there (`TEMPLUM_CLAUDE_ARGS`,
`TEMPLUM_CODEX_ARGS`…, [connections.md](connections.md#agents-typed-in-a-terminal)) depends on the shell: nothing in
WSL, no Codex words in Command Prompt or Windows PowerShell. It is now judged by **the terminal's own shell**, not the
default: a Windows PowerShell terminal opened from the menu while the default is Git Bash gets no Codex words. The
same for the agent's resume command typed into a restored terminal.

### Decisions

Taken by the planner (card 60 left them open):

1. **One list for Settings, the welcome and both menus** (`available_shells`), completed by known shells on the PATH
   and `-l` on macOS, rather than a second detection.
2. **A click on the toolbar's Terminal always opens the menu**, as AI agent does; the keyboard shortcut stays the
   one-step default. Rejected: a split button (one more control in the toolbar), a click that opens the default and
   a long press for the menu (hidden).
3. **Default first, by the name of what it opens**, the found shell of that name not repeated.
4. **The shell is the block's, saved with it by its label**, looked up among the shells found on restore; a default
   terminal saves nothing and follows Settings. Rejected: saving the program and its arguments, which would make the
   canvas file a command Templum runs at start.
5. **A shell not found falls back to the default** on restore, with no notification.
6. **No integration for more shells**: the shells most people use already have it; Nushell, cmd and WSL would each
   need their own script.
7. **The list is read at start and when Settings opens**, not at each menu: a shell installed while Templum runs
   shows after Settings opens or a restart ([minors.md](../minors.md#settings-panel)). Reading it at each menu would
   take the tests' made-up list away.
8. **Agents keep the default shell**; their resume and environment words follow the terminal's own shell.
9. **No new setting.**

### Testing

**Unit (`templum-terminal`):**

- `unix_shells` adds `bash`, `zsh`, `fish`, `nu`, `pwsh` found on a made-up PATH after `/etc/shells`, once each by
  name, by their path; a name `/etc/shells` lists isn't added again;
- with `login`, every shell found gets `-l`; without, none;
- `program_dirs` lists Homebrew's (Apple Silicon and Linuxbrew), Nix's and MacPorts' folders, so a shell from a
  package manager is found whatever PATH the app was started with.

**Headless UI (`workspace/tests/`)**, with a made-up list (`Workspace::shells`, `Workspace::default_shell`) whose
shells run the tests' quiet program, so no test depends on the machine's shells:

- the toolbar's Terminal opens a menu; its third item (after Default and the separator) opens a terminal that runs
  that shell (`Pane::shell`; on Unix its title is the program's name), the found shell named as the default being
  left out; Default opens one with no shell of its own;
- **Add ▸ Terminal ▸** a shell opens it at the right-click;
- **Default (*name*)**'s name: the configured shell's label, the file name of a program typed by hand, the system's
  name without a program;
- a terminal's saved item carries its shell's label; a canvas file whose `shell` is `sh` (**Command Prompt** on
  Windows) restores the block running it (the one test that reads the machine's own list, so it picks a shell that
  has no slow startup files, and is skipped where the list lacks it); a saved item whose label isn't found restores it running
  the default, its shell forgotten, at once even while the list is early; closing it and Ctrl+Z bring it back with
  it;
- a menu opened after the login shell's PATH arrived reads an early list again;
- `agent_env` for a terminal running `wsl.exe` gives no agent words while the default gives them; on Unix, with
  Windows PowerShell configured, a terminal running bash gets the Codex words;
- the existing tests that click the toolbar's Terminal or **Add ▸ Terminal** pick **Default** in the new menu.

**Manual (Linux):** the toolbar's Terminal opens the menu upward with the shells found; one picked runs in its block
(its title); after a restart it runs again. Windows (Git Bash, PowerShell 7, WSL, cmd) and macOS (`-l`, Homebrew's
fish) not checked by hand: no machine at hand.

## A shell for each agent

Kanban card 66, from the person: *"What if the user has a Claude in WSL and another Claude in PowerShell? Add an
agent should have an option to pick the shell the agent opens in, and it must work for our default agents too: the
shell picker could have Default, which follows the global shell chosen in Templum, but each agent could set its
own."*

### Goal

Every agent opened from the canvas ran in the default shell (Settings ▸ Shell; card 60's decision 8). A person whose
Claude Code lives in WSL and whose other agents live in PowerShell, or who wants Claude Code in fish and Codex in zsh,
had to change the global setting before opening each one. Now every entry of Settings ▸ Agents (Templum's agents,
the person's own, and accounts) has a **Shell**: **Default**, which follows Settings ▸ Shell as before, or one of the
shells found on the computer, the one it then always opens in. This reverses card 60's decisions 8 ("Agents keep the
default shell") and 9 ("No new setting").

**Success criteria**

1. The agent dialog (Edit and Add an agent) and the account dialog list **Default (*name*)** and every shell found;
   Save writes the choice to that entry of `[agents]`, Default removes it.
2. An agent with a shell opens in it wherever Templum opens agents: **AI agent ▸** (the canvas's menu, the
   toolbar's, grid mode's), `spawn_agent`, the Templum Agent, and an account's **Log in**. One without opens in the
   default, as now.
3. Its block keeps that shell after a restart, an undo of its close or a paste, whatever the agent's setting is by
   then (card 60's saved `shell`).
4. On Windows, an agent set to WSL can be chosen even when its program isn't on Windows' PATH, and opens in WSL in the
   folder chosen.
5. The tests below pass in CI on the three OS, whatever shells the machine has.

**Not in scope:** Templum's server, hooks, status line and usage for an agent inside WSL; seeing what runs inside WSL;
accounts in WSL ([What works in WSL](#what-works-in-wsl)); looking for the agent's program inside its shell; Generate
✨ (commit messages) running in the agent's shell; a `shell` argument for `spawn_agent` or `create_block`; showing
the shell in the canvas's menu.

### In config.toml

```toml
[agents]
claude = { shell = "PowerShell 7" }                                        # one of Templum's
claude-wsl = { name = "Claude Code (WSL)", command = "claude", shell = "WSL" }   # one of yours
claudinho = { of = "claude", shell = "Git Bash" }                          # an account
```

- `shell` is a shell's **label**, as the lists show it (card 60's labels: **Git Bash**, **PowerShell 7**, **Windows
  PowerShell**, **WSL**, **Nushell**, **Command Prompt** on Windows; the program's file name elsewhere: **zsh**,
  **fish**, **nu**). Missing or empty: Default.
- Any entry takes it. One of Templum's with only `shell` set is changed (↺ and Reset all take it back to Default);
  saving it back at Default and its other defaults removes its entry, as now. An account's is its own: it doesn't
  follow its agent's (an account entry copies the rest of its agent's).
- A label, never a program, as the canvas file names a block's shell: it is looked up among the shells found
  (`Workspace::shells`) and runs with that shell's arguments (`-l` on macOS, `--login -i` for Git Bash). The same name
  then works on every computer a `config.toml` is copied to.
- **A label not found here** (another computer's file, a shell uninstalled, a typo) opens the agent in the default
  shell, with a line on stderr and no notification, as a restored terminal's (card 60's decision 5). The label stays
  in the file: on the computer that has that shell it works again.

### Settings ▸ Agents

- **The agent dialog** (Edit, Add an agent) gets **Shell** under Resume command: a list button (`menu_button`, as
  Settings ▸ Shell's) showing the choice:
  - **Default (*name*)**, first: it follows Settings ▸ Shell; *name* is what the shells' menus call the default
    (`Workspace::default_shell_name`);
  - every shell found, by its label, the default's own included: choosing it pins the agent to it, and it stays when
    Settings ▸ Shell changes;
  - when the entry's label isn't among them: ***label* (not available)**, last and checked, so Save keeps it.
  - Under it, while the choice runs WSL: "In WSL the agent gets none of Templum's tools, hooks or status line, and
    isn't resumed after a restart."
- **The account dialog** gets the same list, in the language of its other choices (under the agent and the name of a
  new account, under Folder for an edited one). An account's list leaves WSL out
  ([What works in WSL](#what-works-in-wsl)).
- **A row** says its shell after the rest, when it has one: `claude · in PowerShell 7`, `Yours · claude · in WSL`,
  `Claude Code · type claudinho in any terminal · in Git Bash`; a label not found adds `(not available)`.
- The row's switch saves the whole entry: it keeps the shell (`Draft::of` and `AccountDraft::of` carry it).

### Opening an agent in its shell

- Every way Templum opens an agent goes through `Workspace::start_agent`: the canvas's and toolbar's **AI agent ▸**
  (`launch_agent`), `spawn_agent` and the Templum Agent (`open_agent_in`). It runs the entry's shell
  (`Workspace::agent_shell`: its label among the shells found; None for no label, a label not found, or WSL for an
  account), and the block keeps it (`Pane::shell`), saved by its label as card 60 does.
- What Templum types after the agent's command and puts in its block's environment is judged by that shell: the line
  (`typed_in`), the words for agents typed later (`agent_env`, card 60), and now an agent's own variables too
  (`agent_vars`: OpenCode's, Kilo Code's and Qwen Code's server, which were judged by the default shell). So a Codex
  set to Windows PowerShell gets none of the words 5.1 would break, and an agent set to WSL gets none of Templum's.
- **Log in** (Settings ▸ Agents ▸ Accounts) opens the account's shell too.
- **Restoring** a canvas, Ctrl+Z of a close and a paste run the **block's own** saved shell (card 60), not the agent's
  setting as it is now: the session an agent resumes was made in that shell's system (a Claude Code in WSL keeps its
  sessions in WSL's `~/.claude`, not Windows'). A block saved before this change has none: the default, as before.
- Ctrl+Shift+T, the toolbar's Terminal and **Add ▸ Terminal ▸** are unchanged.

### Installed

- **The same computer's shells** (bash, zsh, fish, Nushell, PowerShell, cmd, Git Bash) run the same programs:
  Templum looks for an agent's program as it does now (`templum_terminal::installed`: the PATH, the login shell's
  PATH, the package managers' folders), whatever shell the agent is set to.
- **WSL is another system**: its programs aren't on Windows' PATH, and looking inside it means starting it (seconds)
  at each menu. An agent that runs in WSL (its shell is WSL; or it has none and Settings ▸ Shell is WSL; the
  system's default never is) counts as installed (`Workspace::agent_installed`): the menus list it, its row shows its
  command, not the install line. When it isn't installed in WSL either, its block shows WSL's "command not found".
- The canvas's menu, the toolbar's, grid mode's, Settings ▸ Agents' rows and the welcome's agents page all ask
  `agent_installed`. The Templum Agent's `list_agents` keeps saying what this computer has.
- Generate ✨ runs an agent's command on the computer, never in its shell: an agent only WSL has writes no commit
  message, as now.

### What works in WSL

An agent set to WSL runs in `wsl.exe`: the default distribution, in its default shell.

| Works | Doesn't, and why |
|---|---|
| It opens in the folder chosen: `wsl.exe` starts in the Windows folder it is given as `/mnt/c/…` (a `\\wsl.localhost\…` folder as its Linux path; neither checked by hand) | **Templum's server, hooks, status line and usage**: `Workspace::gives` gives nothing under WSL. Templum's files are Windows paths, the bus is Windows' loopback, and none of the block's variables (`TEMPLUM_BUS`, `TEMPLUM_TERMINAL`…) cross into WSL ([minors](../minors.md#connections)) |
| Its command, with the model, effort and role words `spawn_agent`'s caller or a role gives (plain words, read the same by every shell) | **Templum doesn't see the agent**: WSL 2's processes aren't Windows'. No status dot, closing doesn't warn, the block forgets its agent at the next snapshot, so a restart brings back a WSL shell without the agent ([minors](../minors.md#programs-in-a-terminal)) |
| The block keeps WSL across restarts, undo and paste | **`spawn_agent` and the Templum Agent refuse it**: its prompt could never be typed, since Templum never sees it start. "Claude Code (WSL) runs in WSL, where Templum can't see it start nor type its prompt: open it from the canvas." |
| | **Accounts**: an account's folder reaches its agent through a variable, which doesn't cross into WSL; the agent would run on WSL's own login. An account's list leaves WSL out, and an account whose `shell` names WSL opens the default ([minors](../minors.md#agent-accounts)) |

**The card's case.** Claude Code set to PowerShell 7 (or Git Bash) keeps all of Templum: its server, hooks, status
line, resume by session, accounts. The WSL one is an agent of the person's own: **Add an agent**, Name "Claude Code
(WSL)", Command `claude`, Shell **WSL**.

### Decisions

Taken by the planner (card 66 left them open):

1. **The setting is each `[agents]` entry's `shell`, a label**, as the canvas file names a block's shell: one name for
   a shell everywhere, looked up among the shells found, never a command Templum runs.
2. **The list is Default, then every shell found**, the default's own included (pinning it is a real choice), as
   Settings ▸ Shell lists them.
3. **A label not found opens the default and stays in the file**, shown as "(not available)": a `config.toml` shared
   between computers keeps working on each. No notification, as card 60's restore.
4. **An account has its own shell**, Default when unset, not its agent's: it is its own row, and an account that
   followed a WSL agent would lose its folder.
5. **Installed is trusted in WSL** and looked up as now elsewhere. Rejected: asking the shell (`wsl.exe -e sh -lc
   'command -v claude'`), which starts WSL's VM at each menu and still guesses which distribution.
6. **WSL gets none of Templum's connections, and `spawn_agent` refuses an agent there**, rather than passing variables
   through `WSLENV` and translating every path, which no one could check without a Windows machine. Each limit is a
   minor with its fix.
7. **An account never runs in WSL** through its own setting (its list leaves WSL out; a hand-written one opens the
   default). An account opened with Default while Settings ▸ Shell is WSL still runs there, as before this change
   (a minor).
8. **A restored block runs its own saved shell**, not the agent's setting as it is then: the session it resumes lives
   in that shell's system.
9. **One place opens agents** (`start_agent`): the menus, `spawn_agent`, the Templum Agent and a fork all follow the
   setting with no new argument; Log in follows the account's.
10. **No shell in the canvas's menu** and no `shell` argument for `spawn_agent`: the agent's name says which one it is
    ("Claude Code (WSL)"), and its setting where it runs.
11. **Generate ✨ stays on the computer**: it runs the agent without a terminal.

### Testing

**Unit:**

- `templum-terminal`: `is_shell` names a program by its file stem, in any case, on every OS's path
  (`C:/Windows/System32/wsl.exe` is `wsl`, `PowerShell.exe` is `powershell`, `/usr/bin/wslu` isn't `wsl`).
- `agents.rs`: each entry's `shell` from `[agents]` (one of Templum's, changed; one of the person's; an account's own,
  not its agent's); `Draft` and `AccountDraft` keep it from `of` to `to_config`, Default with nothing else gives no
  entry; `to_value` writes it.
- `settings`: the Shell list (Default first, every shell, an account's without WSL, a label not found last); a row's
  description with its shell, and with "(not available)".

**Headless UI (`workspace/tests/`)**, with made-up shells (`Workspace::shells`) whose programs are the tests' quiet
one, or a link named `wsl` to it (Unix), so no test depends on the machine's shells:

- an agent set to a shell opens in it, its block keeps it and its saved item names it; another agent opens in the
  default; one whose label isn't found opens in the default; an account set to WSL opens in the default;
- an agent set to WSL gets no Templum flags on its line (Unix), and OpenCode set to WSL no server variable;
- an agent set to WSL counts as installed (the menu's list, the welcome's) when its program isn't found, and one in
  the default doesn't; with Settings ▸ Shell set to WSL every agent does;
- `spawn_agent` of an agent set to WSL is refused with the reason, and opens no block;
- the agent dialog's Shell list sets `shell` in config.toml and Default removes it; the row's switch keeps it;
- the account dialog's Shell list sets an account's `shell`.

**Manual (Linux):** Claude Code set to fish opens in fish (its block's title before the agent sets one), Codex in the
default zsh; after a restart Claude Code resumes in fish. Windows (PowerShell 7, Git Bash, WSL) not checked by hand: no
Windows machine at hand.

## A scope for each terminal

Kanban card 173, urgent: *"The whole Templum dies when a terminal runs out of memory."* On 2026-10-24 systemd-oomd
killed Templum's scope (`app-Hyprland-gtk-launch-*.scope`) three times, its memory pressure above 50% for more than
20 s: 270 to 370 processes each time, the window and every terminal and agent. Templum itself used about 200 MB of the
scope's 18.1 GB; the rest was agents' parallel Rust builds in its terminals. Every shell, and all it starts, lived in
Templum's own cgroup, and oomd ends a whole unit.

### On Linux

- Each terminal's shell is moved into a systemd user scope of its own, as GNOME Terminal's are (VTE's
  `vte-spawn-<uuid>.scope`): `templum-term-<id>.scope`, `<id>` being its `TEMPLUM_TERMINAL`, described as
  `Templum terminal <id> (<shell>)`. What the shell starts afterwards (agents, `cargo`, tests) is in it too. oomd then
  ends the scope of the terminal that uses the memory; Templum and the other terminals live. `systemd-cgls --user`
  shows them under `app.slice`.
- **How:** right after the PTY's child starts, a thread of its own (`templum-terminal`'s `scope.rs`) calls
  `org.freedesktop.systemd1.Manager.StartTransientUnit` on the user's session bus with the shell's pid (`PIDs`),
  `CollectMode=inactive-or-failed`, so a scope oomd ended doesn't stay as a failed unit, as
  `systemd-run --user --scope --collect` would. One connection serves the whole run (zbus, already in the tree for
  the portals).
- **A shell that ends at once:** systemd (261) misses the emptying of a scope whose only process ends while it is
  being moved, and keeps it running, empty, until logout (3 of 80 quick shells in a test). So when the shell is gone
  within a second of the call, the thread asks for the scope's `TasksCurrent` and stops it (`StopUnit`) when nothing
  runs in it.
- **Nothing waits for it:** opening a terminal costs what it did. Measured on 2026-10-24 (40 terminals running
  `sh -c 'printf ready; exec cat'`, dev build): `Terminal::spawn` 0.87 ms median before, 0.65 ms after; first output
  1.8 ms before, 1.6 ms after (noise); the shell is in its scope 2.2 ms (median, p95 3.7 ms) after the spawn began, the
  first terminal of a run a few ms later (it connects to the bus).
- **What the shell forks before that** stays in Templum's cgroup: at most the first commands of its startup files,
  which usually take longer than the move. An agent Templum types into a new terminal starts once the startup files
  ran, inside the scope. VTE has the same window.
- **No scope, as before:** without a session bus (zbus tries `DBUS_SESSION_BUS_ADDRESS`, else
  `/run/user/<uid>/bus`), without systemd on it, or when systemd refuses (Templum started from an SSH login, whose
  session the system's manager owns), the shell stays in Templum's cgroup. One line on stderr says why, the first time;
  no notification. macOS and Windows have no scopes.

### A terminal the system stopped

oomd ends a scope with SIGKILL. A terminal whose shell ends killed (`Terminal::killed`: SIGKILL, on Unix) closes its
block as any ended shell does, and a notification says *The terminal "<title>" was stopped by the system, most likely
for using too much memory* (also on stderr). Templum never kills the shell of a block it still shows, so the other
SIGKILL is a person's `kill -9` ([minors](../minors.md#programs-in-a-terminal)).

**Not in a scope yet:** the Templum Agent's engine and what its tools run, and the programs Templum runs without a
terminal (Git, Generate ✨), stay in Templum's cgroup ([minors](../minors.md#programs-in-a-terminal)).

### Decisions

1. **A D-Bus call from Templum, after the spawn**, as VTE does, rather than starting each shell as
   `systemd-run --user --scope -- <shell>`. `systemd-run` added 5 to 15 ms before every prompt (measured), needed to
   be found on the PATH, and, when systemd refused, failed the terminal instead of opening it as before; the call
   costs no waiting, falls back silently, and leaves the shell's command line, pid, shell integration and the agent
   and folder detection untouched. Rejected also: a sub-cgroup Templum makes, which needs a delegated cgroup an app's
   scope doesn't have.
2. **The scope's name carries the terminal's `TEMPLUM_TERMINAL`**, unique among every Templum running; the block's
   id is the UI's and `templum-terminal` stays UI-free.
3. **The notification guesses from the signal**: asking systemd how the scope ended needs its unit kept after it
   failed, and oomd's own marks (its journal line, the cgroup's `user.oomd_kill` attribute) are gone or racy by then.

### Testing

- `templum-terminal` (`tests/spawn.rs`, Linux): two terminals' shells are each in their `/templum-term-<id>.scope`
  (`/proc/<pid>/cgroup`), not the test's cgroup, waiting for it; SIGKILL to the first's scope, as oomd sends it
  (`systemctl --user kill`), ends that terminal (`killed`) and not the other. Skipped, with a line, when no systemd
  user manager answers or doesn't run the test (`systemctl --user is-system-running`, the test's own cgroup), as in CI
  containers.
- A shell that ends at once (`exit 0`, 20 of them) leaves no `templum-term-*` unit (fails without the stop: 1 to
  3 left).
- The fallback: the test runs itself again with `DBUS_SESSION_BUS_ADDRESS` naming a missing socket; there two
  terminals run and stay in its cgroup, and stderr has the line once.
- `a_killed_shell_says_so` (Unix): `kill -9 $$` is `killed`, `exit 3` isn't.
- Headless UI (`workspace/tests/terminals.rs`): a block's shell killed from outside closes it with one notification;
  one the person ends shows none (`persistence.rs`).
- **Manual (Linux)**, in a dev build (`cargo run -p templum -- --dev`), with oomd watching `app.slice` (`oomctl`):
  open two terminals; in each, `cat /proc/self/cgroup` names its own `templum-term-<id>.scope`. In one, run
  `stress-ng --vm 4 --vm-bytes 90% --vm-keep --timeout 120s` (Arch: `pacman -S stress-ng`) and watch
  `journalctl -f -u systemd-oomd` from a terminal outside Templum: after 20 s above 50% pressure oomd kills
  `templum-term-<id>.scope` alone; its block closes with the notification, the window and the other terminal stay.
  Without a hog: `systemctl --user kill --signal=SIGKILL templum-term-<id>.scope` does what oomd does. Not run by the
  implementer: `stress-ng` isn't installed here, and no GUI window was opened.

