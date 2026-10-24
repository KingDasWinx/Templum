# Settings panel

- **Date:** 2026-10-10
- **Status:** Implemented
- **Superseded in part by:** [polish.md](polish.md), which removed the glass (the window is always opaque) and
  replaced the layout (tabs and cards instead of gpui-kit's `Settings`, no search box).
- **Builds on:** [config.md](config.md) (part 1: every option in `config.toml`, applied live) and
  [design-system.md](design-system.md)

## Goal

Every option of [config.md](config.md) can be changed with clicks, in a panel that slides in from the right of the
window. The terminals stay visible and usable next to it, so each change is seen as it happens. The panel edits the
same `config.toml`: what one changes, the other shows. Comments in the file are kept.

**Success criteria**

1. Every field below changes its option on Linux, macOS and Windows. CI checks this on every OS, and Linux
   (Hyprland) is also checked by hand.
2. A change applies on the next frame. It is written to `config.toml` 0.3 s after the last change, or at once when
   the panel or Templum closes.
3. Writing never loses the user's comments, other keys, line endings or byte-order mark, and never overwrites a
   file that doesn't parse.
4. All tests in [Testing](#testing) pass in CI on the 3 OS, and `--bench` stays at p95 ≤ 16.7 ms.

## Out of scope

- Persistence of the canvas (the next sub-project).
- Per-block settings, and importing other formats. (Theme colors are edited since [themes.md](themes.md).)
- Undo history of settings changes. "Back to default" per field is in scope.

## The panel

### Opening and closing

- **Opens with:**
  - the new `settings` command (Ctrl+, on Linux and Windows, Cmd+, on macOS);
  - the new **⚙** button in the title bar, after the palette button (gpui-kit's `settings` icon).
- **Closes with:**
  - the same command;
  - the ✕ in its header;
  - Esc while the focus is inside the panel.
- **Movement:** it slides in from the right over 0.2 s and slides out the same way.
- **Commands:**
  - `open_config` (open `config.toml` in the system editor) loses its default shortcut and stays available in
    `[keys]`;
  - the panel's footer has an **Edit config.toml** button that runs it.

### Layout

- **Size:** 480 px wide, at most 60% of the window. The panel sits **over** the right of the canvas; the canvas and
  its blocks don't move.
- **Content:** gpui-kit's `Settings` component:
  - a section list on the left (Appearance, Font, Terminal, Shell, Shortcuts) with gpui-kit's search box on top,
    which filters fields by title and keywords;
  - the section's groups and fields on the right;
  - each field with gpui-kit's "reset to default".
- **Footer:**
  - the path of `config.toml`;
  - the **Edit config.toml** button;
  - the save status line ([Errors](#errors)).

### It doesn't block the app

- Clicking a terminal, typing into it, panning and zooming all keep working with the panel open.
- Esc goes to whatever has the focus. With a terminal focused, it goes to the terminal (vim keeps working), and the
  panel stays open.
- With the focus in the panel:
  - the theme picker gets its keys;
  - every shortcut runs, except copy, paste and close terminal, which act on the focused terminal and wait for it
    to have the keyboard.

## Sections and fields

Every field reads the active `Config`. A change applies at once, except in Shell, which applies to new terminals
(the section says so).

**Appearance**

| Field | Control | Option |
|---|---|---|
| Mode | Dropdown: System, Light, Dark | `theme.mode` |
| Light theme, Dark theme | Two dropdowns: the built-in themes and the user's `themes` folder | `theme.light`, `theme.dark` |
| Glass | Dropdown: Auto, On, Off | `window.blur` |
| Canvas opacity | Slider 0–100% (stored as 0.0–1.0); disabled while the window is opaque | `window.opacity` |

The theme picker in the title bar keeps working and writes the same keys.

Under these rows, a card for the theme in use: **Duplicate…** for a built-in one; **New theme…**, **Open file** and
its colors as swatches with color pickers for one of yours, written to its file ([themes.md](themes.md#making-a-theme),
[the color editor](themes.md#the-color-editor)).

**General**

| Field | Control | Option |
|---|---|---|
| Start Templum when you log in | Switch, off until someone turns it on; see [Start at login](#start-at-login) | — (the system's own entry, not `config.toml`) |

**Canvas**

| Field | Control | Option |
|---|---|---|
| Focus follows the pointer | Switch, on by default: the block under the pointer takes the keyboard once it moves ([navigation.md](navigation.md#the-keyboard-follows-the-pointer)); grid mode's tiles always do | `canvas.focus_follows_pointer` |

**Font**

| Field | Control | Option |
|---|---|---|
| Family | Scrollable dropdown: "Default (built-in list)" and the installed **monospaced** fonts | `font.family` (Default removes the key) |
| Size | Number 6–72, step 1 | `font.size` |
| Preview | A few lines of terminal output (`❯ cargo test`, colored) drawn with the chosen font, size and active theme | — |

GPUI doesn't say which fonts are monospaced. When the panel opens, it takes each installed family as monospaced
when "i" and "m" advance by the same width at 14 pt, within 0.01 px.

**Terminal**

| Field | Control | Option |
|---|---|---|
| Cursor | Dropdown: Block, Bar, Underline | `cursor.shape` |
| Blink | Switch | `cursor.blink` |
| Scrollback | Number 0–100000, step 1000 | `terminal.scrollback` |
| Copy selection automatically | Switch, on by default: text selected with the mouse goes to the clipboard as the button is let go, and a quiet "Copied" shows there for a moment ([walking-skeleton.md](walking-skeleton.md#mouse-and-trackpad)). Not in the welcome | `terminal.copy_on_select` |

**Shell** (applies to new terminals)

| Field | Control | Option |
|---|---|---|
| Program | Text; empty means the default shell (`$SHELL`, or PowerShell on Windows) | `shell.program` (empty removes the key) |
| Arguments | Text: separated by spaces, with `"…"` or `'…'` to keep spaces inside one argument | `shell.args` |
| Start folder | Dropdown: Inherit from the focused block, Home, A folder… With "A folder…", a text field takes the path | `shell.working_directory` |

**Connections** ([connections.md](connections.md#config), [ask-user.md](ask-user.md#settings))

| Field | Control | Option |
|---|---|---|
| Agents in Templum | Switch, on by default: agents opened from the canvas or typed in a terminal get Templum's server and hooks; off, the bus answers none of them, those already running included | `connections.enabled` |
| Questions wait | Number 0–1440 minutes, step 5: how long an agent's question waits for an answer; 0 waits until it is answered | `connections.question_timeout` |
| Sound | Switch, on by default: a short sound when an agent asks something | `connections.sound` |

**Agents** lists the agents ([agents.md](agents.md#changing-ours-adding-yours)), and under them their **Accounts**:
another login of an agent on its own folder, and what it shares with the others
([accounts.md](accounts.md#settings--agents--accounts)).

**Instructions** (after Agents) is the person's standing instructions for their agents, saved in `instructions.json`,
not `config.toml` ([instructions.md](instructions.md#settings--instructions-decided-with-the-person)): a row per
instruction (its name; under it where and for whom; a switch; **Edit**), "Add an instruction", and a dialog with Name,
Where, Agents and Text. A change applies at once: the next session's start reads it.

**Shortcuts**

- **Rows:** one per command, 12 in the [keys](config.md#the-file) order including `settings` and `quit`. Each row
  shows the current shortcuts as gpui-kit `Kbd` keys (`Ctrl` `Shift` `T`), or "None".
- **Quit:** `quit` (Ctrl+Shift+Q, Cmd+Q on macOS) closes Templum on a second press within 2 s; the first shows
  "Press Ctrl+Shift+Q again to quit". Quitting closes every terminal, and Q sits next to W (close one terminal).
- **Recording:** clicking a row's shortcuts starts recording ("Press a shortcut…"). The next key press with a main
  key becomes the command's only shortcut.
  - Esc cancels.
  - Backspace leaves the command with no shortcut (`[]`).
  - With Ctrl, Alt or Super (Cmd) held, Esc and Backspace are recorded like other keys.
  - Modifier keys alone are ignored until a main key comes.
  - A held key's repeats are ignored, so a shortcut just recorded doesn't run while it is still held.
  - Recording is the only state that takes keys before everything else: while recording, no shortcut runs and
    nothing reaches a terminal.
- **Taken shortcuts:** if the pressed shortcut belongs to another command, the row says "Ctrl+Shift+T is already
  New terminal" and keeps recording.
- **Several shortcuts:** a command with two shortcuts (zoom in has `Ctrl+=` and `Ctrl++`) gets one after recording.
  "Reset to default" brings back its defaults.
  - If another command has one of those defaults now, the reset is refused.
  - A notification says which command has it.
  - "Reset All" decides its resets together, so two commands that traded keys both get their defaults back.

## Start at login

Templum can start when the person logs in to the computer. The system's own entry for it is the only record: there
is no setting in `config.toml` that could drift from it, so removing the entry with the system's tools shows the
switch off.

- **Off for everyone who already uses Templum.** Nothing writes the entry on its own. The welcome
  ([welcome.md](welcome.md#pages)) offers it on a first run, on by default (but not over an entry the system's tools
  switched off, nor for `--dev`) and applied when the person has seen the option and the welcome closes;
  Settings ▸ General turns it on or off at any time.
- **Read** when the panel or the welcome opens (off the UI thread, since Windows reads the registry with a
  program), and set by each change, which is also written off the UI thread: the switch shows the new state at once
  and is disabled until the change's answer, and a failure puts the switch back to what the system has. A reading
  never lands over a change that started after it, nor while one runs: the change's own answer is the reading. The
  entry is written for the executable running: the AppImage file, not where it is mounted
  (`templum_update::Install`).
- **Where the entry is**, per system (`autostart.rs`):

| System | Entry | It starts |
|---|---|---|
| Linux | `$XDG_CONFIG_HOME/autostart/templum.desktop` (`~/.config` when unset or relative), with the name, comment and icon of the apps menu's `templum.desktop` | `Exec="<executable>"`, quoted as the Desktop Entry spec asks, and `TryExec=<executable>` so the desktop skips the entry when the file is gone |
| macOS | `~/Library/LaunchAgents/io.github.kingdaswinx.templum.plist` | `ProgramArguments` = the executable inside the bundle, `AssociatedBundleIdentifiers` = Templum's, so System Settings ▸ Login Items lists the item under Templum, `RunAtLoad`, only in a desktop session (`Aqua`) |
| Windows | the `Templum` value of `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, written with `reg.exe` from `%SystemRoot%\System32` (no console window) | the executable in quotes |

- **Plain tiling sessions don't run it.** The Linux entry is an XDG autostart file: GNOME, KDE, Xfce and the like
  run it, but Hyprland, sway and niri on their own don't (uwsm or `dex -a` does). On Linux the row says so, and
  there Templum is started from the compositor's own config (`exec-once = templum` in Hyprland).
- **Switched off by the system's tools:** a Linux file with `Hidden=true` or `X-GNOME-Autostart-enabled=false`,
  and a Windows entry that Task Manager or Settings ▸ Apps ▸ Startup switched off (its `StartupApproved` value),
  read as off. Turning it on writes the entry again and clears that value.
- **Run from the disk image** (macOS, or the copy macOS runs a downloaded app in): turning it on is refused with
  "Move Templum to Applications first", since the file the entry names would vanish. Turning it off still works.
- **`--dev`** (beside an installed Templum) has an entry of its own (`templum-dev.desktop`, `….templum.dev.plist`,
  `Templum (dev)`) that starts `templum --dev`, so it never overwrites or removes the installed one's.
- **Moved or reinstalled.** At startup, an entry that is on but whose executable no longer exists is written again
  for this one, off the UI thread. An entry that names a file that still exists is another Templum's (a build
  from Cargo beside the installed one, a portable copy, a test AppImage) and stays, so two never take it from each
  other. An entry that is off or switched off, one whose program can't be told, and a line a person added to it by
  hand, stay as they are. A second Templum open (it keeps nothing) never refreshes.
- **The bench** (`--bench…`) has no entry: it never touches the person's login items.
- **No entry** (no home folder, or the running executable can't be told): the row says "Templum can't add itself to
  this system's startup apps" and has no switch.
- **It fails** (a read-only folder, `reg.exe` refusing): the row's description says why, a notification shows
  the same, and the switch shows what the system has. Nothing crashes.

## How a change applies and is saved

1. **Edit:** a field change becomes an **edit**: "set `table.key` to a value", or "remove `table.key`" for "reset to
   default" (and for Default font or an empty program).
2. **Apply:**
   - the edit changes the active `Config` in memory;
   - the workspace applies it on its **next frame** through the same path as a reload of the file
     ([config.md](config.md#live-reload));
   - the next frame is needed because applying a font measures text in the window.
3. **Save:**
   - 0.3 s after the **last** edit, the pending edits are written in one go;
   - the file is **read again** at that moment and only the edited keys change: comments, other keys, CRLF line
     endings and a BOM are kept, so an edit made by hand in between stays;
   - afterwards the poll's stamp is updated, so the poll doesn't reload Templum's own write.
4. **Close:** closing the panel, or quitting Templum, writes pending edits at once.
5. **Hand edits:** an edit made by hand to the file while the panel is open is picked up by the poll, and the panel
   shows the new values, since it always reads the active `Config`.

## Errors

| Situation | Behavior |
|---|---|
| `config.toml` doesn't parse when saving | Nothing is written. The change stays in effect until Templum quits. A notification shows, and the footer keeps "Not saved: config.toml has an error on line N" until the file is fixed. |
| Writing fails (permission, disk) | The same, with the OS error. |
| No config folder (no HOME or APPDATA) | The footer says "No config folder: changes last until Templum quits". |
| A number beyond its limits | The number field doesn't go past them (6–72; 0–100000). |
| Arguments with an unclosed quote | The field shows "Unclosed quote" and doesn't apply until it is fixed. |
| Recording a taken shortcut | The row says whose it is and keeps recording. |
| The login entry can't be written (read-only folder, `reg.exe` fails) | The General row's description says why and a notification shows it; the switch shows what the system has. |

## Architecture

| File | Responsibility |
|---|---|
| `config.rs` | `Edit { table, key, value: Option<toml_edit::Value> }` and `save_edits(dir, &[Edit])`, which reads the file, applies the edits with `toml_edit` (creating a missing table, keeping CRLF and the BOM), refuses a file that doesn't parse, and writes. `save_theme` becomes one edit through it. |
| `keys.rs` | The `Settings` command (Ctrl+, / Cmd+,). `OpenConfig` gets no default. A key press becomes its shortcut text (`Keystroke::unparse`), and `Keymap::owner(&Keystroke)` tells which command a shortcut belongs to. |
| `settings.rs` (new) | Builds the `Settings` pages and fields from the active `Config`. Each field reads through the workspace entity and edits through it. Also has the monospaced-font list, `split_args` (quotes) and the shortcut row. |
| `workspace.rs` | The panel's open state, slide-in and focus handle. Edits are applied on the next frame (`pending`), saved 0.3 s after the last one (a replaced timer task) and flushed on close and on app quit. Also holds the save-error line and the recording state, which sees keys before shortcuts and terminals. |
| `title_bar.rs` | The ⚙ button. |
| `autostart.rs` (new) | The system's entry for starting at login: where it is per system, its text (`.desktop`, `.plist`, the `reg.exe` arguments), reading, writing and refreshing it. `State` is what the toggle shows. |
| `workspace/autostart.rs` (new) | Reading it when the panel or the welcome opens, setting it, refreshing it at startup. |
| `settings/general.rs` (new) | Settings ▸ General, and the row the welcome shares. |

No new dependency.

## Testing

Tests are written first, as always. Everything runs in CI on Linux, macOS and Windows.

**Unit:**

- **`save_edits`:**
  - sets a key in an existing table, creates a missing table, and removes a key;
  - keeps comments, other tables, CRLF and a BOM;
  - leaves a file that doesn't parse byte-for-byte unchanged;
  - `save_theme` still passes its tests through it.
- **Keys:**
  - `settings` defaults to `ctrl-,` (`cmd-,` on macOS), and `open_config` has no default;
  - `unparse` of a recorded keystroke parses back to the same keystroke;
  - `owner` finds the command that has a shortcut, and None for a free one.
- **`split_args`:** spaces, double and single quotes, an empty string, and an unclosed quote as an error.
- **The monospace rule:** equal advances are monospaced; different ones aren't.

**Headless UI** (`#[gpui_kit::test]`, temporary config folder; tests flush edits directly instead of waiting
0.3 s):

- Ctrl+, and the ⚙ open the panel; the ✕ and Ctrl+, again close it.
- Editing the font size applies on the next frame (cell size changes) and reaches the file after a flush.
- "Reset to default" removes the key from the file.
- A file that doesn't parse makes the footer show the save error, and the file stays unchanged.
- **Recording:**
  - a new shortcut applies and is saved;
  - a taken one keeps recording;
  - Esc cancels;
  - Backspace leaves the command with no shortcut.
- With the panel open, clicking a terminal and typing reaches the terminal.

**Start at login** (`autostart.rs` unit tests, `settings_panel.rs`, and `workspace/tests/welcome.rs`):

- the `.desktop` and `.plist` texts (`TryExec=`, `AssociatedBundleIdentifiers`), the `Exec=` quoting, the `reg.exe`
  program (System32's) and arguments and the parsing of its output, as pure functions;
- where each system keeps the entry, with and without `XDG_CONFIG_HOME` or a home;
- on, off, off again and read back in a temporary home (never the real one, nor the registry); removing the file
  by hand reads off; `Hidden=true` reads off and turning it on writes it again; an unwritable folder is an error;
- the status tells no entry from one the system switched off; a Templum run from the macOS disk image refuses to
  turn it on; the welcome's default (`State::offered`) is on only with no entry at all, and not for `--dev` or the
  disk image;
- the `--dev` entry doesn't touch the installed one's;
- the program an entry names is read back whatever it holds (spaces, quotes, `$`, backslashes, `%`, `&`, `<`), and
  one written with an escape the parsers don't decode (`\s`, `\n`, `\t`, `\r` in a `.desktop` file, `&#38;` in a
  LaunchAgent) can't be told, so startup leaves it alone;
- startup rewrites an entry that is on for an executable that is gone, and leaves alone one that names a file that
  exists, one that is off, switched off or edited by hand, and anything a second Templum finds;
- a reading never lands over a change: Settings opened from the welcome's last page shows what the welcome applied
  (on and off), and a reading fired just before a click doesn't undo it;
- headless: the General tab finds an entry made outside, turns it on and off (the switch follows at once, the system
  is written off the UI thread and the switch is disabled meanwhile), and says why it failed; the welcome's cases are in
  [welcome.md](welcome.md#testing).

**Manual (Hyprland):**

- every section against a running terminal;
- the slider over the glass;
- the font list and preview;
- recording shortcuts;
- a hand edit of the file showing up in the panel.

## Delivery

Three pull requests, each green in CI and useful on its own:

1. **The engine and the panel:**
   - `Edit` and `save_edits`;
   - pending edits, the debounced save and the flush;
   - the `settings` command and the ⚙;
   - the panel (open, close, slide, footer with path, Edit config.toml and the save line);
   - the **Appearance** and **Terminal** sections.
2. **Font and Shell:** the monospaced font list and the preview, `split_args`, and the start folder.
3. **Shortcuts:** recording, conflicts and reset, plus docs (README and ARCHITECTURE).

## Risks

- **gpui-kit's `Settings` component** owns its own state (`use_keyed_state`) and calls field setters without a
  `Window`. That is why edits apply on the next frame, from the workspace's render, rather than inside the setter.
- **Measuring "i" and "m"** for every installed family is one advance lookup pair per family. A few hundred
  families cost a few milliseconds, once per panel opening.
- **Esc inside gpui-kit inputs** may be handled by the input (blur) before the panel sees it. Pressing it again
  closes the panel.
