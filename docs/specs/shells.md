# Shells

- **Date:** 2026-10-11
- **Status:** Implemented
- **Builds on:** [config.md](config.md) (`[shell]`) and [settings.md](settings.md)

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
`nologin`, `false`, `git-shell`, `rbash`, `tmux` and `screen`.

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
