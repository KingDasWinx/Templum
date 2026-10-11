# Configuration file

- **Date:** 2026-10-10
- **Status:** Implemented
- **Superseded in part by:** [polish.md](polish.md), which removed the glass (the window is always opaque).
- **Builds on:** [design-system.md](design-system.md), which created `config.toml` with `[theme]` only, and
  [walking-skeleton.md](walking-skeleton.md)

## Goal

Everything a user would want to tune lives in `config.toml` and takes effect as soon as the file is saved:
- the terminal font and its size;
- the window's glass and opacity;
- the light or dark mode;
- the shell and the folder it starts in, including "the folder of the focused terminal";
- the cursor and the scrollback;
- every keyboard shortcut.

No recompiling, no environment variables, no restart.

This is the first of two parts. The second part, a settings screen inside the app, comes later in its own spec and
edits this same file. Persistence of the canvas comes after both.

**Success criteria**

1. Every option below works on Linux (Wayland and X11), macOS and Windows. CI checks this on every OS, and Linux
   (Hyprland) is also checked by hand.
2. Editing and saving `config.toml` while Templum runs applies the change within one second, with no restart.
3. No content of `config.toml` can crash Templum or stop it from starting.
4. All tests in [Testing](#testing) pass in CI on the 3 OS, and `--bench` stays at p95 ≤ 16.7 ms.

## Out of scope

- **The settings screen** (part 2 of configuration).
- **Persistence of the canvas** (the next sub-project).
- Per-block settings (font, shell or theme per terminal).
- Font fallback lists, ligatures, line height.
- Mouse bindings.
- Shell integration through OSC 7.

## The file

The location is unchanged from [design-system.md](design-system.md#config-file):
- `$XDG_CONFIG_HOME/templum/config.toml` (falling back to `~/.config/templum/config.toml`) on Linux and macOS;
- `%APPDATA%\templum\config.toml` on Windows.

Every table and every key is optional. A missing key means its default. This is the complete file with the defaults
on Linux and Windows:

```toml
[font]
family = "JetBrains Mono"   # default: the first installed font of the built-in monospace list
size = 14                    # 6 to 72

[window]
blur = "auto"     # "auto" (blur where the system does) | true | false
opacity = 0.72    # opacity of the canvas, 0 to 1 (1 = solid)

[theme]
mode = "system"   # "system" | "light" | "dark"
light = "Catppuccin Latte"
dark = "Catppuccin Mocha"

[shell]
program = "fish"                # default: $SHELL, or PowerShell on Windows
args = ["--login"]              # default: none
working_directory = "inherit"   # "inherit" | "~" | a path

[cursor]
shape = "block"   # "block" | "bar" | "underline"
blink = false

[terminal]
scrollback = 10000   # lines of history per terminal, 0 to 100000

[keys]
new_terminal   = "ctrl-shift-t"
close_terminal = "ctrl-shift-w"
copy           = "ctrl-shift-c"
paste          = "ctrl-shift-v"
zoom_in        = ["ctrl-=", "ctrl-+"]
zoom_out       = "ctrl--"
zoom_reset     = "ctrl-0"
fit_all        = ["ctrl-shift-1", "ctrl-!"]
theme_picker   = "ctrl-shift-p"
open_config    = "ctrl-,"
```

On macOS the default shortcuts are `cmd-t`, `cmd-w`, `cmd-c`, `cmd-v`, `cmd-=` and `cmd-+`, `cmd--`, `cmd-0`,
`cmd-shift-1` and `cmd-!`, `cmd-shift-p` and `cmd-,`.

### Rules per table

- **`font`**
  - `family` must be an installed font. When it isn't, the built-in monospace list is used, with a warning (see
    [Errors](#errors)).
  - The `TEMPLUM_FONT` environment variable still works and wins over `family`, so existing setups keep working.
  - `size` is in points at zoom 1. It replaces today's constant 14.
- **`window`**
  - `blur = "auto"` is the [design-system.md](design-system.md#window) rule. `true` asks for a blurred background
    anywhere, for compositors Templum doesn't detect (niri, SwayFX). `false` keeps the window opaque everywhere.
  - `opacity` is the canvas alpha when the window is translucent; it replaces `CANVAS_ALPHA`. When the window is
    opaque, the canvas is drawn at 1. Blocks are always opaque.
- **`theme`**
  - `mode = "system"` follows the system, as today. `"light"` or `"dark"` pins that mode and ignores the system.
  - `light` and `dark` are as today; the theme picker still saves them.
- **`shell`**
  - `program` and `args` replace the login shell for new terminals.
  - `working_directory`:
    - `"inherit"` (the default) starts a new terminal in the current folder of the focused terminal. This covers
      every way of opening one: the shortcut, the **+** button and a double-click on the canvas. With no focused
      terminal, or when its folder can't be read, the terminal starts in the home folder.
    - `"~"` is the home folder.
    - Any other string is a path. A leading `~/` means the home folder.
- **`cursor`**
  - `shape` and `blink` are the default cursor. A program that asks for another cursor (DECSCUSR, like vim's bar in
    insert mode) still gets it.
- **`terminal`**
  - `scrollback` is the number of history lines each terminal keeps.
- **`keys`**
  - Each action takes one shortcut or a list. Writing an action replaces its defaults; `[]` disables it.
  - Shortcuts use GPUI's keystroke syntax (`ctrl-shift-t`, `cmd-,`, `alt-f4`).
  - `theme_picker` opens or closes the theme picker. While the picker is open, only `theme_picker` acts (it closes
    the picker, like Esc); other shortcuts wait.
  - `open_config` opens `config.toml` in the system's default editor. When the file doesn't exist, it first writes
    the commented template (see [Template](#template)).

### Template

`open_config` creates a missing `config.toml` from a template:
- the file above with every line commented out (`# size = 14`);
- a header line pointing to the README's "Configuration" section;
- the macOS shortcuts in the template when built for macOS.

Uncommenting a line and saving applies it.

## Live reload

Every 0.5 s, Templum compares `config.toml`'s modification time and size with the last ones it saw. When they
differ, it reads the file again and applies only what changed. Polling (rather than a file watcher) works the same
with editors that save by replacing the file (vim, VS Code) and with a `config.toml` that is a symlink into a
dotfiles repository.

| Option | When it applies |
|---|---|
| `font` | At once, in every terminal. Each block **keeps its size on the canvas**, and its terminal gets more or fewer columns and rows (never fewer than the existing minimum of 20×4). New blocks are sized for 80×24 at the new size. |
| `window` | At once: the window switches its background without reopening. |
| `theme` | At once. |
| `cursor` | At once, in every terminal. |
| `terminal.scrollback` | At once, in every terminal. Lowering it drops the oldest lines beyond the new limit. |
| `keys` | At once. |
| `shell` | In **new** terminals. Open terminals keep their shell. |

- **Creating or deleting the file:** a new `config.toml` is applied at once. Deleting it brings back the defaults.
- **Saving from the theme picker:** the picker changes the file, the next poll reads it, and nothing visible
  changes because the values are the same.
- **Blinking:** only the focused terminal's cursor blinks (0.5 s on, 0.5 s off), and it shows again at once on any
  key press. The other terminals show the hollow cursor, as today. This costs two frames per second, only while
  something blinks.
- **Shortcuts and the picker:** they do nothing while the theme picker is open. Today Ctrl+Shift+V pastes into the
  terminal behind the picker; this fixes that.

## Errors

Templum always starts and keeps running, whatever `config.toml` contains. Problems show as a gpui-kit notification
at the bottom right of the window, clear of the title bar's buttons (like today's "Couldn't start a shell"),
and go to stderr with the `templum:` prefix.

| Situation | Behavior |
|---|---|
| The file isn't valid TOML, a value has the wrong type, or there is an unknown table or key (a typo like `fnot`) | The **last valid config** stays in effect (the defaults at startup). The notification quotes the parser: `config.toml, line 4, column 1: unknown field "fnot", expected one of "font", "window", …` |
| A value is out of range: `font.size` outside 6–72, `window.opacity` outside 0–1, `terminal.scrollback` above 100000 | Same as above. The message names the key and its range. |
| A shortcut doesn't parse (`ctrl-shft-t`) | Same as above. The message names the action and the shortcut. |
| The file becomes valid again after an error | It applies, and an info notification says "config.toml loaded". After a reload that had no previous error, nothing is shown. |
| `font.family` isn't installed | The config applies with the built-in list, plus a warning: "Font "X" isn't installed; using "Y"". |
| The same shortcut is set for two actions | The config applies. The action listed first in the [keys](#the-file) order above wins, and a warning names the shortcut. |
| `shell.program` can't start | As today: no block is created, and the notification says "Couldn't start a shell: …". |
| `working_directory` is a path that isn't a folder | The terminal starts in the home folder, with a warning. |
| `"inherit"` and the focused terminal's folder can't be read (permission, process gone) | The terminal starts in the home folder, with no warning: there is nothing for the user to fix. |
| `open_config` has no config folder, or can't write the template | A notification with the reason. |

## Architecture

### `templum-terminal`

- **`SpawnOptions`** gets `working_directory: Option<PathBuf>`. `None` keeps today's home folder.
- **`Terminal::cwd() -> Option<PathBuf>`** is the shell's current folder, read through its process ID:
  - on Unix, the process asked is the leader of the PTY's foreground process group (`tcgetpgrp` through
    `rustix`), which is the shell the user types into even under macOS's `login` wrapper or in a nested shell; it
    falls back to the spawned child (`tty::Pty::child().id()`);
  - on Windows, the process asked is the spawned shell (`child_watcher().pid()`);
  - the folder comes from `sysinfo`, the same way on every OS.
- **`Terminal::set_options(TermOptions { scrollback, cursor })`** forwards to `Term::set_options`, which applies
  both to a running terminal. `SpawnOptions` gets the same `options` for new terminals.
- **The snapshot** records the cursor's shape (block, bar, underline, hollow block) and whether it blinks, as the
  terminal resolves them (the config default, unless a program asked for another one).

### `templum` (the app)

| File | Responsibility |
|---|---|
| `config.rs` | `Config` (serde: defaults, `deny_unknown_fields`) plus validation, which gives the errors of [Errors](#errors) with line and column. Also `Stamp` (modification time and size, for polling), the template, and the existing `save_theme` with `toml_edit`. `read`/`ThemeNames` are replaced by `Config`. |
| `keys.rs` (new) | `Command` (the 10 actions), the per-OS defaults, parsing `[keys]` into a keymap, and matching a key press against it. |
| `grid.rs` | The font size comes from the cell metrics instead of a constant, and the cursor is drawn as a block, a bar or an underline, blinking when asked. |
| `workspace.rs` | Holds the active `Config`, polls and reloads it, applies only the differences, opens terminals in the inherited folder, routes key presses through the keymap (before the terminal, and not at all while the picker is open), and shows the notifications. |
| `main.rs` | Reads `config.toml` before opening the window, so it opens with the configured background. |

**Shortcuts move from GPUI key bindings to the keymap.** GPUI can't remove one binding, only clear all of them, and
that would also drop gpui-kit's own bindings. So Templum's shortcuts are matched in the workspace's key handler
against its own keymap, which a reload simply replaces. The `tab` and `shift-tab` overrides of gpui-kit's root stay
as GPUI bindings.

**New dependencies:** `serde` (with `derive`) and `toml` to read, and `sysinfo` with `default-features = false`
and only the `system` feature, in `templum-terminal`. All three are already in `Cargo.lock`, and no new package is
downloaded. Saving stays with `toml_edit`.

## Testing

Tests are written first, as always. Everything runs in CI on Linux, macOS and Windows.

**`templum-terminal`:**

- spawning with a temporary `working_directory` makes `cwd()` report that folder (canonicalized, since macOS's
  temp folder is a symlink);
- `set_options` with a bar cursor shows in the snapshot's cursor shape;
- a lower scrollback is reflected in the history length.

**`templum`** (unit):

- **Config:**
  - the template, uncommented, parses to exactly the defaults;
  - an empty file gives the defaults;
  - an unknown key fails with its line and column;
  - each range limit fails with its key named;
  - an invalid shortcut fails with its action named;
  - a duplicated shortcut gives a warning, and the first action wins;
  - `working_directory` accepts `"inherit"`, `"~"`, `"~/x"` and an absolute path.
- **Polling:** `Stamp` notices an edit, a new file and a deleted file.
- **Keymap:**
  - the per-OS defaults match today's shortcuts;
  - a remapped action matches its new shortcut and no longer its old one;
  - `[]` disables an action.
- **Saving:** a CRLF `config.toml` keeps its CRLF line endings, and saving in dark mode writes `dark`. These are
  two of the deferred minors from the design review.

**`templum`** (headless UI, `#[gpui_kit::test]`, with a temporary config folder; the tests call the reload
directly instead of waiting for the poll):

- a reload with a bigger `font.size` changes the cell size, and the open terminal gets fewer columns while its
  block keeps its rect;
- `window.blur = false` makes the canvas opaque;
- `theme.mode = "dark"` pins the dark theme on a light system;
- a remapped `new_terminal` opens a terminal with the new shortcut and not with the old one;
- shortcuts do nothing while the theme picker is open;
- a new terminal starts in the focused terminal's folder (`inherit`);
- an invalid file keeps the previous config and shows a notification;
- `config::ensure_file` writes the template when the file is missing (GPUI's test platform can't open files, so the
  shortcut itself is checked by hand).

**Manual (Hyprland):**

- editing font, blur, opacity, mode, cursor and keys with the app open;
- `open_config` opening the editor;
- a new terminal inheriting the folder after `cd`.

## Delivery

Three pull requests, each green in CI and useful on its own:

1. **Terminal:** `working_directory`, `cwd()`, `set_options` (scrollback and cursor) and the cursor shape in the
   snapshot, all in `templum-terminal`, with its tests.
2. **Config:**
   - the typed `Config` and live reload;
   - font, window, theme mode, cursor, scrollback, shell and the inherited folder;
   - the error notifications.
3. **Shortcuts:**
   - the keymap and the two new actions (`theme_picker`, `open_config` with the template);
   - shortcuts disabled under the picker;
   - the CRLF and dark-mode save fixes;
   - docs: a "Configuration" section in the README, and ARCHITECTURE.md.

## Risks

- **Reading another process's folder** on macOS and Windows goes through `sysinfo`. It can fail for a shell running
  as another user (`sudo -s`) or one that already exited; the rule is then "home folder, no warning".
- **PowerShell doesn't move its process's folder.** `Set-Location`/`cd` only changes PowerShell's own location, so
  `"inherit"` follows `cd` with cmd.exe but not with PowerShell, Windows's default shell; new terminals then start
  where PowerShell started. Fixing it needs shell integration (OSC 7 or 9;9), which is out of scope.
- **ConPTY's PID** on Windows is the shell's when it starts directly. Under a wrapper (`cmd /c` starting
  PowerShell) it would be the wrapper's folder; the default PowerShell starts directly.
- **Fonts swapped at runtime** re-measure the cell and resize every terminal in one frame. With many terminals this
  is one slow frame, which is acceptable for a settings change.
- **Matching shortcuts ourselves** has to keep GPUI's handling of shifted keys (`ctrl-!` and `ctrl-shift-1`). The
  keymap uses GPUI's own `Keystroke` matching, and the tests cover both spellings.
