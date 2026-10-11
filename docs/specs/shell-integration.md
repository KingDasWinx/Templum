# Shell integration

- **Date:** 2026-10-11
- **Status:** Implemented (part 1: status, folder, attention). Part 2, command marks, is below.
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
- `"inherit"` follows `cd` in PowerShell, whose `cd` doesn't move its process.

## What Templum hears

The PTY's output passes through a scanner before alacritty parses it (`templum-terminal`'s `tap.rs`). It only
listens: every byte still reaches the terminal, which ignores these sequences.

| Sequence | Meaning |
|---|---|
| `OSC 133 ; C` | A command starts (its output follows) |
| `OSC 133 ; D [; code]` | It ended, with its exit code. A D without a C (an empty line, Ctrl+C) is no command |
| `OSC 7 ; file://host/path` | The current folder (percent-encoded; `kitty-shell-cwd://` as is) |
| `OSC 9 ; 9 ; path` | The current folder, ConEmu's and Windows Terminal's way |
| BEL | Attention: no text |
| `OSC 9 ; text` | Attention, with a text (iTerm2's notifications; other `9;n` numbers are ConEmu commands, ignored) |
| `OSC 777 ; notify ; title ; body` | Attention, with a text |

An OSC ends where vte ends it (BEL, ESC, CAN, SUB). Sequences of other numbers are skipped as they come, never
collected, and one of ours longer than 4 KiB is dropped.

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

- bash: a function first in `PROMPT_COMMAND` sends D (the status it got, given back to what runs next) and OSC 7;
  `PS0` sends C (bash 4.4 or newer; older bash reports the folder only).
- zsh: `precmd` and `preexec` hooks, added before the `.zshrc`'s.
- fish: handlers for `fish_preexec`, `fish_postexec` and `fish_prompt`.
- PowerShell: the prompt function is wrapped; the prompt it wraps still sees `$?` and `$LASTEXITCODE`. With
  PSReadLine, reading the command line is wrapped to send C; without it, only the folder is reported.

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
  workspace, the block focused, the camera on it.
- With Templum in the background and `terminal.notifications` on, the same as a system notification. A click
  brings Templum to the front on the block.

The dot before a terminal's title, with a tooltip saying why: yellow, it asked for attention; green or red, a long
command ended (the alert); then the accent color while a command runs; then red while the last command failed.

Agents: Codex rings the bell by default. Claude Code notifies only in the terminals it knows; set
`"preferredNotifChannel": "terminal_bell"` (or `"iterm2"`, OSC 9 with a text) in its settings.

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
- how each shell is started, its login form, and the arguments that leave it alone;
- `install` writes each script, rewrites a changed one only, and leaves no temporary file;
- real shells in a PTY, each with its own home whose startup file moves it to a folder: that folder is reported,
  then `false` ends with 1 and `cd` with 0 and its folder. bash, a login bash, zsh, fish and PowerShell 7; a shell
  that isn't installed is skipped.

**Headless UI:**

- a bell in another block gives it an alert (no system notification with the window active), focusing it clears
  it, and a bell in the focused block is no news;
- in the background, a bell is a system notification ("cat", "Needs your attention"), none while
  `terminal.notifications` is off, and no second one before the block is seen;
- on Windows, WSL's `bash.exe` isn't taken for Git Bash;
- the two switches turn their keys off in `config.toml`, and ↺ removes them;
- with the integration, a new bash terminal has it; with `shell.integration` off, a new one doesn't.

**By hand:** Linux (Arch): see the PR. Windows (Git Bash, PowerShell 7, Windows PowerShell through ConPTY) and macOS
weren't tried yet.

## Part 2: command marks

Not built yet. Each command gets a mark in the terminal's gutter, green or red by its exit code, and a shortcut
jumps to the previous or next command. The mark must live in the grid, so it follows scrolling, the history limit
and resizing; one way is for the scanner to put an OSC 8 hyperlink of its own (`templum://cmd/<n>`) where a command
starts, which alacritty keeps on the cells.
