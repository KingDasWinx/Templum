# Settings panel

- **Date:** 2026-10-10
- **Status:** Implemented
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
- Per-block settings, theme editing (colors), and importing other formats.
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

**Shell** (applies to new terminals)

| Field | Control | Option |
|---|---|---|
| Program | Text; empty means the default shell (`$SHELL`, or PowerShell on Windows) | `shell.program` (empty removes the key) |
| Arguments | Text: separated by spaces, with `"…"` or `'…'` to keep spaces inside one argument | `shell.args` |
| Start folder | Dropdown: Inherit from the focused terminal, Home, A folder… With "A folder…", a text field takes the path | `shell.working_directory` |

**Shortcuts**

- **Rows:** one per command, 12 in the [keys](config.md#the-file) order including `settings` and `quit`. Each row
  shows the current shortcuts as gpui-kit `Kbd` keys (`Ctrl` `Shift` `T`), or "None".
- **Quit:** `quit` (Ctrl+Shift+Q, Cmd+Q on macOS) closes Templum on a second press within 2 s; the first shows
  "Press Ctrl+Shift+Q again to quit". Quitting closes every terminal, and Q sits next to W (close one terminal).
- **Recording:** clicking a row's shortcuts starts recording ("Press a shortcut…"). The next key press with a main
  key becomes the command's only shortcut.
  - Esc cancels.
  - Backspace leaves the command with no shortcut (`[]`).
  - Modifier keys alone are ignored until a main key comes.
  - Recording is the only state that takes keys before everything else: while recording, no shortcut runs and
    nothing reaches a terminal.
- **Taken shortcuts:** if the pressed shortcut belongs to another command, the row says "Ctrl+Shift+T is already
  New terminal" and keeps recording.
- **Several shortcuts:** a command with two shortcuts (zoom in has `Ctrl+=` and `Ctrl++`) gets one after recording.
  "Reset to default" brings back its defaults.

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

## Architecture

| File | Responsibility |
|---|---|
| `config.rs` | `Edit { table, key, value: Option<toml_edit::Value> }` and `save_edits(dir, &[Edit])`, which reads the file, applies the edits with `toml_edit` (creating a missing table, keeping CRLF and the BOM), refuses a file that doesn't parse, and writes. `save_theme` becomes one edit through it. |
| `keys.rs` | The `Settings` command (Ctrl+, / Cmd+,). `OpenConfig` gets no default. A key press becomes its shortcut text (`Keystroke::unparse`), and `Keymap::owner(&Keystroke)` tells which command a shortcut belongs to. |
| `settings.rs` (new) | Builds the `Settings` pages and fields from the active `Config`. Each field reads through the workspace entity and edits through it. Also has the monospaced-font list, `split_args` (quotes) and the shortcut row. |
| `workspace.rs` | The panel's open state, slide-in and focus handle. Edits are applied on the next frame (`pending`), saved 0.3 s after the last one (a replaced timer task) and flushed on close and on app quit. Also holds the save-error line and the recording state, which sees keys before shortcuts and terminals. |
| `title_bar.rs` | The ⚙ button. |

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
