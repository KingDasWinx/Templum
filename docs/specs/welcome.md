# Welcome

- **Date:** 2026-10-14
- **Status:** Implemented
- **Builds on:** [settings.md](settings.md) and [polish.md](polish.md) (the panel's rows and edits),
  [persistence.md](persistence.md) (what a first run is) and [agents.md](agents.md)

## Goal

On a new install, Templum opened one terminal and nothing told the person what it can do: the themes, the shells
found on their computer, shell integration, the agents in the canvas menu, plan limits, notifications and updates
all wait in Settings, where a newcomer doesn't know to look. The welcome shows them once, in five short pages over the
canvas, and every choice applies at once.

## When it shows

- **A first run only:** the data folder has no `workspaces/index.json` and no saved canvas for the first workspace
  (`session::dir`: `~/.local/share/templum` on Linux, `~/Library/Application Support/Templum` on macOS,
  `%APPDATA%\templum` on Windows). It is the check that opened the first terminal (`workspace/spaces.rs`). No other
  flag is kept: once it closes, the next snapshot or quitting writes the canvas, and it doesn't show again. While a
  first run's welcome is open nothing is written, so quitting from it leaves a first run for next time.
- **Not** when a saved file was there but couldn't be read (a damaged canvas or workspace list starts as a first run
  did, with a terminal), nor without a data folder (`--bench`).
- **Again** from Settings: **Welcome**, in the panel's footer, closes the panel and opens it.
- The first terminal waits for it: closing it opens the terminal, centered, in the shell picked. Every later start
  opens on the [Home](home.md) instead.
- It is not in `config.toml`: that file holds preferences people edit, and it rejects unknown keys, so an older
  Templum would report a new one as an error.

## Pages

Steps at the top (Welcome · Look · Shell · Agents · Ready) show where one is and jump to a page. The footer has
**Back** (not on the first page), **Skip** (not on the last) and **Next**, which is **Get started** on the first and
**Start using Templum** on the last.

| Page | What it holds |
|---|---|
| Welcome | What Templum is, in three cards: terminals on a canvas, a cockpit for agents, notes, kanban and Git |
| Look | Mode; the themes of the mode in effect as cards in their own colors (a click makes it that mode's theme); the font's family, size and preview |
| Shell | The default shell and each one found, as a list; the start folder; shell integration |
| Agents | The agents found on this computer (looked for once, when it opens), each with its menu switch and Edit; the agent that writes commit messages and its model; plan limits |
| Ready | System notifications, checking for updates and **Start Templum when you log in**; the keys worth knowing, from the keymap in effect |

- Every control is the panel's (`settings/sections.rs`, picked by key) or edits the same keys, through
  `Workspace::edit`: a change applies on the next frame and is saved 0.3 s later.
- Closing applies what is pending and writes it at once, so the first terminal gets the shell just picked.
- Text is in English, like the rest of the interface.
- **Start at login** ([settings.md](settings.md#start-at-login)) is the one choice that waits for the welcome to
  close, since it touches the system rather than `config.toml`:
  - in a first run it shows on when the system has no entry at all, and pressing **Start using Templum** applies
    that. It shows off, and stays off, for an entry the system's tools switched off (`Hidden=true`, Task
    Manager), for a `--dev` build, and for a Templum run from the macOS disk image, which couldn't keep one;
  - nobody gets it without seeing the option: once the last page has been shown, the default applies whichever way
    the welcome closes (Start using Templum, Skip or Esc), since the person saw it on. **Skip** and Esc before the
    last page was ever shown leave it off;
  - switching it, either way, is a choice that applies when the welcome closes by any means;
  - opened again from Settings, it shows what the system has and changes nothing unless the person switches it.

## Keys and pointer

- **Esc** skips it. **Enter** goes to the next page while the card has the keyboard (not from a field).
- What the title bar opens over it has its keys first: the theme picker its arrows, Enter and Esc; the usage
  popover Esc.
- Settings, the theme picker and Quit still run (holding Quit is no second press). Opening Settings closes the
  welcome: the panel has everything. So does showing another workspace or a block (a notification's link): the
  keyboard goes there.
- It is a modal: nothing under it gets the pointer, and a press beside the card closes nothing. The title bar stays
  usable, and what it opens (the theme picker) is drawn above it; the Home never shows over it: opening it closes the
  welcome ([home.md](home.md#when-it-shows)).
- Fading out, it lets the canvas beside the card have the pointer at once; the card still takes its presses, so a
  second click on Skip doesn't reach the canvas.

## Look

- The card (at most 760 × 600, 48 px clear of the window) rises 24 px as it fades in, over 200 ms (`motion::slid`);
  each page fades in, sliding 24 px from the side it comes from (`with_animation`, keyed by page).
- **Frosted canvas.** GPUI can't blur what is drawn behind an element (its scene has no filter primitive), but a
  shadow is a true Gaussian blur of its shape, painted whole under a transparent element. So:
  1. the canvas fades under a veil of its own color;
  2. each block on screen becomes a soft shape where it is: the card's color, a little lighter on dark themes;
     white on light ones;
  3. three large glows in the theme's blue, magenta and cyan are drawn over them, so an empty canvas isn't bare.

## Architecture

| File | Change |
|---|---|
| `welcome.rs` (new) | `Welcome`, `Page`; opening, closing, pages and keys (`impl Workspace`); the frosted backdrop, the card, the steps, the first page and the keys |
| `settings/welcome.rs` (new) | Each page's groups: the theme and shell cards, and the panel's rows picked by `Row::key`; the Ready page also has the start-at-login row (`settings/general.rs`) |
| `settings/mod.rs` | `card` (rows in one card) shared with the panel; the **Welcome** button |
| `workspace/spaces.rs` | A first run opens the welcome; `open_first_terminal`; a damaged file is no first run for it; showing another workspace closes it; the workspace list isn't written while a first run's welcome is open |
| `workspace/persist.rs`, `workspace/view.rs` | No canvas written while a first run's welcome is open; showing a block closes it |
| `workspace/preferences.rs` | `load_lists` (themes, fonts, shells) shared with the panel; `apply_and_save` |
| `workspace/render.rs` | The welcome's fade, drawn under what the title bar opens |

## Testing

Headless UI (`workspace/tests/welcome.rs`):

- a first run opens it with no block; Esc closes it and opens the centered terminal, focused;
- a saved canvas, a damaged one and no data folder show no welcome;
- Next, Back, Enter and the steps move between pages, and the last page's button closes it;
- a theme card sets the theme of the mode in effect, and only that mode's themes are shown;
- a shell card sets `shell.program` and `shell.args`, Default removes them, and a choice followed at once by Esc
  is in effect and written when the first terminal opens;
- the Shell page shows only the start folder and integration rows, and its switch turns integration off;
- the Agents page lists the agents found, or says none were;
- Settings' **Welcome** opens it without a new terminal;
- start at login shows on in a first run with no entry and is applied by any way of closing once the last page was
  shown, not by Skip or Esc before it; it shows off for an entry the system switched off and for `--dev`; a switch
  made in the welcome is applied by any way; opened again, it shows the system's state and changes only what was
  switched;
- quitting from a first run's welcome writes nothing, and the next start welcomes again;
- a new workspace over it closes it, and the first terminal is in the first workspace;
- holding the quit shortcut over it is no second press; a double-click on Skip opens one terminal;
- the theme picker and the usage popover over it get Esc, and the welcome stays.

Manual (Hyprland), with fresh folders (`XDG_DATA_HOME=$(mktemp -d) XDG_CONFIG_HOME=$(mktemp -d) cargo run -p
templum`): the pages in light and dark, the frosted blocks (opened again from Settings over a canvas with blocks),
a theme changing behind it, Esc and Enter.
