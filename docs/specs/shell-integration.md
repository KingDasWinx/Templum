# Shell integration

- **Date:** 2026-10-11
- **Status:** Implemented
- **Builds on:** [config.md](config.md) (`"inherit"`), [shells.md](shells.md) and [workspaces.md](workspaces.md)

## Goal

A shell knows things a terminal can't see: which folder it is in, and when a command starts, ends and whether it
failed. Like Ghostty, Kitty, WezTerm and VS Code, Templum loads a small script into bash, zsh, fish and PowerShell
that tells it, with standard escape sequences. Users change neither their shell nor their files. For the agent
cockpit, it means:

- a block shows when its command runs, and when the last one failed;
- a terminal you aren't looking at that ends a long command, or that asks for attention (an agent waiting for you),
  gets a dot on its block and on its workspace's tile, and a notification; with Templum in the background, a
  system notification too, which brings you to the block when clicked;
- `"inherit"` follows `cd` in PowerShell, whose `cd` doesn't move its process;
- each command gets a mark in the terminal's margin, green or red by how it ended, and a shortcut jumps from
  prompt to prompt.

## What Templum hears

The PTY's output passes through a scanner before alacritty parses it (`templum-terminal`'s `tap.rs`). Every byte
still reaches the terminal, which ignores these sequences; the scanner only adds the prompt marks (below).

| Sequence | Meaning |
|---|---|
| `OSC 133 ; A` | A prompt starts |
| `OSC 133 ; C` | A command starts (its output follows) |
| `OSC 133 ; D [; code]` | It ended, with its exit code. A D without a C (an empty line, Ctrl+C) is no command |
| `OSC 7 ; file://host/path` | The current folder (percent-encoded; `kitty-shell-cwd://` as is) |
| `OSC 9 ; 9 ; path` | The current folder, ConEmu's and Windows Terminal's way |
| BEL | Attention: no text |
| `OSC 9 ; text` | Attention, with a text (iTerm2's notifications; other `9;n` numbers are ConEmu commands, ignored) |
| `OSC 777 ; notify ; title ; body` | Attention, with a text |

An OSC ends where vte ends it (BEL, ESC, CAN, SUB). Sequences of other numbers are skipped as they come, never
collected, and one of ours longer than 4 KiB is dropped.

A shell can be marked twice: fish 4 marks its prompts and commands itself, and so do some prompt themes and other
terminals' scripts a user sources. A second A on the same line as the open prompt's is the same prompt; a second C
while a command runs, and a D with none running, are the same command's.

The reported folder is trusted while the shell is in the foreground (on Windows, which has no foreground process,
always) and is a folder that exists. While another program runs in front (ssh, a nested shell), the system's answer
for that program is used, as before.

## Loading the scripts

`templum_terminal::install` writes the scripts to `shell-integration/` in the data folder at startup (only those
that changed). A new terminal running one of these shells, from Settings ▸ Shell or the default, starts like this:

| Shell | How | The user's files |
|---|---|---|
| bash (and Git Bash) | `bash --init-file templum.bash` | The script reads what bash would have: `/etc/bash.bashrc` and `~/.bashrc`; for a login shell (`-l`, `--login`, Git Bash's default), which ignores `--init-file`, Templum leaves the flag out and the script reads `/etc/profile` and the first of `~/.bash_profile`, `~/.bash_login`, `~/.profile` |
| zsh | `ZDOTDIR` points to Templum's `.zshenv` (the user's own kept in `TEMPLUM_ZDOTDIR`) | It puts `ZDOTDIR` back, reads the user's `.zshenv`; zsh then reads `.zprofile`, `.zshrc` and `.zlogin` as usual |
| fish | `fish --init-command 'source templum.fish'` | Read before, by fish itself |
| PowerShell 7, Windows PowerShell | `-NoExit -Command` reading `templum.ps1` as a script block | The profile, by PowerShell itself. A script block isn't a script file, so no execution policy stops it |

A shell started to run something else is left alone: bash and zsh with `-c`, a script or no startup files
(`--norc`, `-f`), fish with `-c`, PowerShell with `-Command`, `-File`, `-EncodedCommand` or `-NonInteractive`.
Other shells (Nushell, cmd, WSL) start as before.

Each script changes nothing but this:

- bash: a function first in `PROMPT_COMMAND` sends D (the status it got, given back to what runs next), OSC 7 and
  A; `PS0` sends C (bash 4.4 or newer; older bash reports prompts and the folder only). With
  [ble.sh](https://github.com/akinomyoga/ble.sh), which puts `PROMPT_COMMAND` back as it was when it attaches,
  the function is a `blehook PRECMD` instead.
- zsh: `precmd` and `preexec` hooks, added before the `.zshrc`'s.
- fish: handlers for `fish_preexec`, `fish_postexec` and `fish_prompt`.
- PowerShell: the prompt function is wrapped; the prompt it wraps still sees `$?` and `$LASTEXITCODE`. With
  PSReadLine, reading the command line is wrapped to send C; without it, prompts and the folder are reported.
- All four ([connections.md](connections.md#agents-typed-in-a-terminal)): with `TEMPLUM_CLAUDE_ARGS` or
  `TEMPLUM_CODEX_ARGS` set, a function `claude` or `codex` that puts Templum's arguments before the person's, unless
  the person has a function of that name. In PowerShell the prompt reports the agent's exit code, which `$?` after a
  function doesn't tell, and the prompt it wraps sees `$?` false when it isn't 0.
- All four ([accounts.md](accounts.md#typed-in-a-terminal)): a function per line of `TEMPLUM_ACCOUNTS`, named by the
  account's id (`claudinho`), unless the person has a function of that name. It runs the account's agent by its
  name, so the `claude` function above applies, with the account's variable set to its folder for that command only.

Prompts themselves (`PS1` and the like) aren't changed: A is sent just before the shell draws one.

New terminals also get `TERM_PROGRAM=templum`, not the variable of the terminal Templum was started from.

The bash and PowerShell scripts follow VS Code's (MIT). Ghostty's and Kitty's are GPL-3 and weren't used.

## News and notifications

Every frame, Templum reads each terminal's counts, in every workspace (a hidden workspace's terminal wakes the UI
for these). News is a command of 5 seconds or more that ended, or a call for attention, from a terminal nobody is
looking at: not the focused one of the shown workspace while the window is active.

- The block gets an alert; focusing it there clears it.
- The workspace's tile gets a yellow dot while one of its terminals has an alert.
- A notification at the bottom right: the terminal's title (and the workspace's name, with several), then
  "Finished after 2 min", "Failed (exit 1) after 12 s", the agent's text, or "Needs your attention". Only the
  first news until the block is seen: a terminal that rings on and on doesn't flood. A click shows the block: its
  workspace, the block focused, the camera moved just enough to show it whole.
- **`go_to_waiting`** (Ctrl+Shift+A, Cmd+Shift+A on macOS) opens the requests inbox
  ([ask-inbox.md](ask-inbox.md)) when a request of an agent waits in the workspace shown. Else it shows the
  terminal that asked for attention: this workspace's first, then the others' in the sidebar's order, the oldest
  block first. Going there clears its alert, so the next press shows the next one; with none waiting, a
  notification says so.
- With Templum in the background and `terminal.notifications` on, the same as a system notification. A click
  brings Templum to the front on the block.

The dot before a terminal's title, with a tooltip saying why: yellow, it asked for attention; green or red, a long
command ended (the alert); then the accent color while a command runs; then red while the last command failed.

Agents: Codex rings the bell by default. Claude Code notifies only in the terminals it knows; set
`"preferredNotifChannel": "terminal_bell"` (or `"iterm2"`, OSC 9 with a text) in its settings.

## Command marks

From a prompt's A to its command's C, the scanner adds a hyperlink of its own to the stream:
`OSC 8 ;; templum:cmd/<n>` after the A (after its ST, when it ends with one), `OSC 8 ;;` after the C. alacritty
keeps a hyperlink on each cell printed meanwhile: the prompt and the command typed at it. The mark is in the
grid, so it moves with the text when it scrolls, falls out of the history with it, survives resizing, and is gone
after `clear`. Templum doesn't draw hyperlinks, and Ctrl+click treats the text under its own as text.

## Links

Ctrl+click (Cmd+click on macOS) in a terminal opens what is under the pointer (`templum-terminal`'s `link.rs`,
read from the grid under the lock, on the click only):

- a hyperlink a program marked (OSC 8): a web page in the browser, a `file://` one in an editor;
- else the run of non-blank characters around the pointer, across wrapped lines: a URL opens in the browser; a
  path (`src/main.rs`, `src/main.rs:42`, `src/main.rs:42:7`, `a.ts(12,5)`, `/abs/x`, `~/x`) that is a file, found
  from the terminal's folder when relative, opens in an editor beside the terminal at that line. The editor's
  folder is the nearest one above the file with a `.git` entry, else the file's folder.

A plain click selects as before. Nothing is drawn under a link.

- **In the margin:** a dot left of the line where a prompt starts (the first line with its mark), green when its
  command ended with 0, red when it failed, dim while it runs or when the shell didn't say the code. A prompt with
  no command run from it (Ctrl+C, an empty line, the one being typed at) has none.
- **Jumping:** **Previous prompt** (`previous_prompt`, Ctrl+Shift+↑; Cmd+↑ on macOS) scrolls the focused
  terminal so that the prompt above the top of the view is at the top; **Next prompt** (`next_prompt`,
  Ctrl+Shift+↓; Cmd+↓) the one below, down to the bottom. Nothing happens in a full-screen program, or without
  prompts there. Like copy and paste, they need a terminal focused.

To make room for what it adds, the scanner reads at most a quarter of alacritty's buffer at a time: the rewritten
bytes then fit, and none wait for the PTY's next output.

## Settings

| Key | Default | Settings | Applies |
|---|---|---|---|
| `shell.integration` | `true` | Shell ▸ Shell integration | To new terminals |
| `terminal.notifications` | `true` | Terminal ▸ System notifications | At once |

## Testing

**Unit (`templum-terminal`):**

- the scanner: a command's start and end with its code, options after the code, no code, a D without a C;
  everything split at every byte boundary gives the same; a 100 KB OSC 52 isn't kept; look-alike numbers
  (1337, 77, 13) aren't ours; an endless sequence of ours is dropped; ESC ends an OSC as in vte; folders from
  `file://`, `kitty-shell-cwd://`, `9;9`, and Windows forms (`/C:/x`, Git Bash's `/c/x`); ConEmu's 9;4 is no
  notification; notification texts, cut at 200 characters;
- the bell asks for attention and wakes the UI at once, past the coalescing;
- the reported folder counts with the shell in front, not with another program there or once it is gone;
- prompt marks: our hyperlink from A to C, after an ST's backslash (also split between reads), none when ESC
  starts another sequence, none for a C without a prompt; the same however the reads split; a second
  integration's A and C (fish 4) are the same prompt and command, and Enter on an empty line makes a new prompt;
- in a grid: a prompt's first line shows its command's state, its other lines don't, also when its first line
  went into the history; jumps go prompt by prompt, up to the first and down to the bottom;
- how each shell is started, its login form, and the arguments that leave it alone; on Windows, WSL's `bash.exe`
  isn't taken for Git Bash;
- `install` writes each script, rewrites a changed one only, and leaves no temporary file;
- real shells in a PTY, each with its own home whose startup file moves it to a folder: that folder is reported,
  `false` ends with 1, its line is marked so, there is a prompt to jump to, and `cd` ends with 0 and its folder.
  bash, a login bash, bash with ble.sh, zsh, fish and PowerShell 7; a shell (or ble.sh) that isn't installed is
  skipped.

**Headless UI:**

- a bell in another block gives it an alert (no system notification with the window active), focusing it clears
  it, and a bell in the focused block is no news;
- in the background, a bell is a system notification ("cat", "Needs your attention"), none while
  `terminal.notifications` is off, and no second one before the block is seen;
- the two switches turn their keys off in `config.toml`, and ↺ removes them;
- with the integration, a new bash terminal has it; with `shell.integration` off, a new one doesn't;
- in bash, after `seq 100`, Previous prompt puts its prompt at the top (marked as ended with 0), Next prompt goes
  back to the bottom;
- the margin dot's color for each state.

**By hand:** not yet. Windows (Git Bash, PowerShell 7, Windows PowerShell through ConPTY) and macOS weren't tried.
