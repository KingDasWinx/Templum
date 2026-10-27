# Design system and themes

- **Date:** 2026-10-10
- **Status:** Implemented
- **Superseded in part by:** [polish.md](polish.md), which removed the glass (the window is always opaque),
  made the dots stronger and animated the zoom.
- **Builds on:** [walking-skeleton.md](walking-skeleton.md), [ARCHITECTURE.md](../ARCHITECTURE.md),
  [ADR-0002](../adr/0002-ui-framework.md)

## Goal

Templum gets a modern, minimalist look inspired by macOS: a translucent window with the desktop blurred behind it,
a subtle dot grid on the canvas, an integrated title bar, and terminals drawn as clean cards. It follows the system's
light or dark mode, and it gets a theme system: Catppuccin by default, other built-in themes, and any theme in
Alacritty's format dropped into a folder, all switchable from a picker in the title bar.

**Success criteria**

1. Every behavior below works on Linux (Wayland and X11), macOS and Windows. CI checks this on every OS, and Linux
   (Hyprland) is also checked by hand.
2. `cargo run --release -p templum -- --bench` still reports **p95 ≤ 16.7 ms** with the dot grid, shadows and
   translucency on.
3. All tests listed in [Testing](#testing) pass in CI on the 3 OS.
4. `templum-canvas` and `templum-terminal` stay free of GPUI.

## Out of scope

- **Config sub-project (next):** terminal font and size, forcing blur on or off, opacity, live reload of
  `config.toml`, pinning light or dark mode, a keyboard shortcut for the picker.
- **Later:** other theme formats (Ghostty, iTerm2), a theme per block, animated zoom, the system accent color.

## Look

### Window

- **Background.** The window is translucent and the desktop shows through blurred where the system blurs:
  macOS, Windows, and on Linux, KDE and Hyprland under Wayland. Everywhere else (GNOME, X11, unknown compositors) the
  window is opaque. The choice is a pure function of the OS, `XDG_CURRENT_DESKTOP` and `WAYLAND_DISPLAY`:

  | Platform | Window background |
  |---|---|
  | macOS, Windows | `WindowBackgroundAppearance::Blurred` |
  | Linux, Wayland, desktop contains `KDE` or `Hyprland` | `Blurred` |
  | Any other Linux | `Opaque` |

- **Title bar.** gpui-kit's `TitleBar` (34 px), built from `TitleBar::window_options()`, drawn on the glass with
  no fill of its own (the root paints the canvas fill under the whole window, title bar included; the dots start
  below the bar) and a hairline bottom border:
  - left: the traffic lights (macOS only);
  - center: "Templum" in the secondary text color;
  - right: the zoom level ("100%"; clicking it resets the zoom to 100%), the palette button (gpui-kit's palette icon; opens the theme picker) and **+**
    (opens a terminal at the viewport center, like Ctrl/Cmd+Shift+T);
  - on Windows and Linux, the minimize, maximize and close buttons sit at the far right.
- **Linux decorations.** The window requests client-side decorations. gpui-kit's root already wraps every window
  in `window_border()`, which draws the shadow and the resize edges. Without client-side decorations, KDE and
  GNOME would draw a second title bar.
- **The canvas** sits below the title bar. The workspace's `origin` already converts window coordinates to canvas
  coordinates, so it keeps working.

### Canvas

- Fill: the theme's `canvas` color (see [Derived colors](#derived-colors)). It is drawn at `CANVAS_ALPHA = 0.72` when
  the window is translucent, and fully opaque otherwise.
- **Dot grid:** dots of 1 px (constant on screen) in the theme's `dot` color, every 24 world units, fixed to the
  canvas so they move with pan and zoom. When the dots would be closer than 24 px on screen, the spacing is
  doubled until it is at least 24 px. At zoom 0.25 and 0.5 the dots are 24 px apart, as at zoom 1.
- The empty-canvas hint uses the theme's `hint` color.

### Blocks (continuous card)

- One surface: the header is the same color as the terminal, with no separate strip. Corner radius is 10 world
  units (scaled by zoom), and the background is the theme's background, fully opaque.
- **Title:** centered, in the system UI font, color `title` (focused) or `title_dim` (other blocks).
- **Close button (✕):** right side of the header, with Fullscreen and, left of it, **More (⋯)**: the same menu a
  right-click on the block opens ([context-menus.md](context-menus.md)), for whoever doesn't know the right-click is
  there. **Resize grip:** gpui-kit's resize-corner icon in the bottom-right corner.
  They are rendered only while the pointer is over the block. They fade in over 120 ms, and fade-out is instant.
  While shown, their hit areas are the same as today: the grip keeps `chrome::HANDLE`.
- **Border:** always 1 px. It uses the `hairline` color, or the `accent` color when the block is focused. Today the
  focused border grows to 2 px and shifts the grid by 1 px; that goes away.
- **Shadow**, scaled by zoom: unfocused, offset y 4 and blur 12; focused, offset y 10 and blur 30. The color is the
  theme's `shadow` color.

### Text

- **UI text** (title bar, block titles, hint, picker) uses the system UI font through gpui-kit's theme: SF Pro on
  macOS, Segoe UI on Windows, the desktop's font on Linux. No font is bundled.
- **Terminal text** keeps its current font selection (`TEMPLUM_FONT`, then the monospace list). Choosing it is the
  Config sub-project.

### Motion

- The header's buttons and the grip fade in over 120 ms.
- **A new block pops in** over 0.28 s: it grows from 88% past 100% (about 101.6%) and settles, fading in.
- **A closed block** shrinks to 92% and fades out over 0.18 s. It leaves the canvas at once and is drawn as a ghost
  that clicks go through; its shell lives until the fade ends.
- Scales snap to 0.02 steps: a block is drawn at zoom × scale, so text is rasterized at only a few extra sizes.
- Nothing else animates (no animated zoom).

### Scrollbars

Every list or body that scrolls draws the same scrollbar, `kanban::scrollbar_over` (or `kanban::scrollbar` inside a
strip of its own): gpui-kit's, over the list, never in it (in it, it would scroll away). Its track is `kanban::TRACK`
(16) wide at the right edge; its thumb is 6 wide (8 under the pointer or held), 48 long at the least and 4 in from
the edge, in the kit theme's scrollbar colors: no color of its own. On the canvas every size is the block's zoom's.
It shows while the pointer is over its track and while the content scrolls, fading in over 300 ms and out over
500 ms after the theme's 2 s idle, and only once the content overflows. Shown, its strip takes the presses: the list
or block under it hears them first in the capture phase, so a press still picks the block and the middle button
still pans the canvas. A terminal's history has its own rules for when it shows, the same look
([walking-skeleton.md](walking-skeleton.md#the-scrollbar-and-jump-to-the-bottom)).

### Text areas

*(Kanban card 258; the card dialog's description is the reference.)* A text area for real text (a description, not
a one-line comment) grows with its text, a line at a time, from a useful height (6 lines) to a cap (16), then
scrolls in itself: gpui-kit's `auto_grow`. Typing grows the dialog downward only. When the person may want it
larger, it has a **grip** in its bottom-right corner: gpui-kit's resize-corner icon at 10 in `title_dim`, in a 14
square 1 in from the corner, over the field (a press there is the grip's, not the text's), with the ns-resize cursor,
held through the drag. The drag moves the field's bottom by the pointer's way, between a least height (64) and what
its dialog's body can show at once; the field then keeps that height whatever its text, which scrolls in it. The
height set is window chrome: kept as dragged, written in the index as the drag ends, drawn clamped to a smaller
window, not in the Templum API.

### Popups

*(Kanban card 217.)* Every popup over the canvas — Git, Requests, Agents and Automations today, and any new modal of
their kind — follows this one standard, built from the helpers of `crates/templum/src/popup.rs`: a new popup calls
them and never redraws its own frame.

- **Place and layering** (`popup::slot`): over the canvas's bottom-left corner, `popup::GAP` (8 px) from the sidebar
  and from the window's bottom edge; window chrome, so the canvas's zoom never scales it. Only one is open at a time:
  opening one closes the others.
- **Size:** they share one, `Workspace::git_popup_size`: `git_block::SIZE` (760 × 540) until the person drags it, at
  least `git_block::MIN` (600 × 420), at most the room the window leaves (`git_room`), which wins when it is smaller
  than the minimum. What is drawn is clamped, the size set is kept: a window that grows back gives it back. The size
  set is window chrome, saved in the index as `git_size`; it is not in the Templum API.
- **Resize:** the free edges, the right and the top, and the square at the corner between them resize it like a
  window's border (`popup-resize-right`, `popup-resize-top`, `popup-resize-top-right`; 6 px outside the box, 2 px
  inside); the left edge rests against the sidebar and the bottom one against the window, so they have none. A drag
  writes the size in the index as it ends. No edges at full screen.
- **Frame** (`popup::frame`): `ui.surface`, a 1 px `ui.hairline` border, 12 px corners, the shadow (offset y 12, blur
  32, `ui.shadow`), a column: the header, then the body. It occludes the canvas, and the wheel stops in it.
- **Header** (`popup::header`), across the full width, `popup::HEADER` (55 px) high with a hairline under it, 12 px at
  its sides and 10 px between its parts: the icon's tile (`popup::tile`: 34 px, 9 px corners, the icon at 17 px in
  its color on 14% of it: the accent, the palette's yellow for Requests); the title (14 px, semibold) over a dim line
  of what it holds (`popup::subtitle`, 12 px `ui.title_dim`: "3 automations", "8 running · 1 needs you"; Git's is its
  branch line); the popup's own actions (one tinted primary, as Git's Sync or Automations' New, then 24 px icon
  buttons); **Full screen** (`Maximize2`, "Full screen"; `Minimize2`, "Exit full screen" while on); **Close**
  (`Close`, "Close (Esc)"). Ids: `{name}-header`, `{name}-close`, `popup-full`.
- **Body:** a list column at the left with a hairline at its right, and the selected item at the right (or one
  panel). The content of a detail or a form grows to `popup::CONTENT_MAX` (680 px) and stays centered past it
  (`popup::content`), so full screen never stretches a field across the window; a list keeps its width.
- **A list row** (card 219) lays its parts out side by side and truncates its words; nothing is ever drawn over them,
  no buttons that show on hover. The selected item's actions live on its card in the detail (Automations' Run now,
  Duplicate and Delete on the summary card).
- **Foot** (`popup::foot`): what stays under a scrolling body, a hairline over it and 10 px above and under its
  content, which has the body's side padding and width. Its buttons sit at the right, the secondary (ghost) before
  the primary (solid); a problem shows above them in the palette's red.
- **Full screen:** the header's toggle makes the popups of this place fill the room (the canvas beside the sidebar,
  less `GAP` on every side) until it is toggled back; it holds for every popup of the place, Git's included, for this
  session (`Workspace::popup_full`): window chrome, never saved, not in the API. The size set stays and comes back.
- **Closing:** a press anywhere outside the popup closes it, and nothing under it gets the press (`popup-backdrop`).
  The sidebar stays out of the backdrop: its buttons switch from one popup to another, and the open one's button
  closes it. Esc closes it, and so does Close (`Workspace::close_popup` closes whichever is open). Closing gives the
  keyboard back to the canvas when the popup had it.
- **A form** (`popup::section`, `popup::row`, `popup::line`): its fields grouped in sections, each one's name in
  small dim capitals (`inbox::section`, 11.5 px) over its rows; 18 px between sections, 8 px between rows. A row is
  its label in a column of its own (`popup::LABEL_W`, 92 px, 11.5 px `ui.title_dim`, on the middle of a 24 px small
  field) and the field beside it, 12 px apart. Related controls share a line (`popup::line`, 8 px apart: When and its
  time, Where with Stop after and Cooldown); a narrow column wraps them under the field, never under the label. A
  short choice's dropdown is as wide as its words; a text field, a block's dropdown and a text area take the line.
  What sums the form up without being a field (a sentence, the next run, the last one; where a request comes from)
  is a card above it: `ui.card`, 8 px corners, 12 px padding. The item's actions are 24 px icon buttons at the card's
  top right, 2 px apart, each named by its tooltip; the card's words sit beside them, never under them.
- **A detail** (card 222; Requests, Agents): what is read there, not typed, is laid out as a form is. A summary card
  first, its words in rows beside their labels (Requests' From and Where, Agents' Agent, State, Terminal and Where);
  then sections of labelled rows (Agents' Work and Cost). A name, a title or a folder is cut to one line; a text the
  person reads (a task, what an agent asks) wraps under its field. An action with words on the item shown (Go to
  terminal) is a button of the foot, the one Enter does solid. Its content and its foot share their sides: 16 px, as
  the Automations editor's (Requests keeps its request balloon's 18).
- **Filter chips** (Agents): over a list, 12 px from its sides as its rows' words; a chip is its name and its count, 6
  px at its sides, 2 px from the next; the chosen one on `ui.pressed`. They wrap when the list is narrow.
- **Colors:** the theme's tokens only (`ui.surface`, `hairline`, `hover`, `card`, `accent`, `title_dim`, `shadow`) and
  the palette's yellow and red for what waits or failed: a popup adds no color.

### Screens

*(Kanban cards 256 and 265.)* A full-window screen — the [Home](home.md) today — is a place, a peer of the
workspaces, not a popup over the canvas. It follows the Popups standard's type, radii and colors, at a page's scale
(`crates/templum/src/home.rs`):

- **Place and layering:** in the canvas's place, below the title bar and beside the sidebar (the whole width when the
  sidebar is hidden). While it is shown no canvas is drawn: the workspace shown last is held loaded and running, as a
  hidden one, none of its blocks laid out; the canvas's chrome (the toolbar, the zoom level, the Templum Agent's pill,
  a mirror's banner) goes with it. The sidebar marks it as it marks the workspace shown, and the title bar names it.
  Popups that belong to no workspace (Agents, Automations), Settings, what the title bar opens (the theme picker, the
  usage popover) and the menus are drawn over it; a workspace's own (Git, Requests) show that workspace first.
- **Switching:** a screen and a workspace swap as two workspaces do, plainly, without motion. It is left by choosing
  a workspace (its card, its tile, Ctrl+1–9, Ctrl+Tab); Esc doesn't leave it (on the Home it empties the search), and
  the item or command that shows it does nothing more there.
- **Ground:** `ui.canvas`, opaque, without the dots.
- **Column:** centered, `home::CONTENT_MAX` (1040 px) at most, 32 px at its sides (16 px when the room is under
  640 px), 40 px above and 48 px below. It scrolls whole, with the standard scrollbar over the screen's right edge.
- **Header:** the title, 22 px semibold in the foreground, over a dim line (13 px, `ui.title_dim`); the actions at its
  right, 8 px apart and 28 px high: one tinted primary (the accent at 16%, 24% under the pointer, its words in the
  accent, medium, 12 px at its sides, 8 px corners), ghost buttons (the `hover` fill under the pointer), then 28 px icon
  buttons named by their tooltips. A search field under it, 16 px below, the column's width, the kit's medium field
  (32 px) with the search icon.
- **Sections:** 28 px apart; each one's name in the popups' small dim capitals (`inbox::section`, 11.5 px), 12 px over
  its content.
- **Cards:** `ui.surface` on the ground, a 1 px `ui.hairline` border, 12 px corners (the popup frame's), 16 px padding
  (14 for a count's tile), the blocks' unfocused shadow (offset y 4, blur 12, `ui.shadow`), 16 px apart. Under the
  pointer the border is `hover_border`; the keyboard's highlight adds the `hover` fill; none is marked current (a
  screen shows no workspace). What is stopped or not live is drawn at 45%, as its sidebar tile.
  A grid has as many 220 px columns as fit, at most four.
- **Type:** a card's title 14 px semibold; its words 12 px `ui.title_dim`; small words 11.5 px; a count 20 px semibold.
  A name is cut to one line at its end, a path at its start (its last folder shows).
- **Colors:** tokens only (`ui.canvas`, `surface`, `card`, `hairline`, `hover`, `hover_border`, `accent`,
  `title`, `title_dim`, `shadow`); the palette's yellow for what waits, red for what went wrong, green for the dot of an
  agent at work (the sidebar's), the accent for what is done. A count at zero has no color: its tile is dim and takes no
  press.
- **Motion:** none: shown and left at once, as a workspace is.
- **Empty:** a list with nothing to show is one dim line ("No workspace matches “api”"); an optional section with
  nothing is left out.

## Themes

### Theme files

A theme is a TOML file in [Alacritty's color format](https://alacritty.org/config-alacritty.html#colors). Templum
reads only these keys and ignores everything else (`search`, `hints`, `footer_bar`, `dim`, `indexed_colors`, other
tables):

| Key | Required | If missing |
|---|---|---|
| `colors.primary.background`, `colors.primary.foreground` | yes | the file is rejected |
| `colors.normal.{black,red,green,yellow,blue,magenta,cyan,white}` | yes | the file is rejected |
| `colors.bright.{same 8}` | no | copied from `normal` |
| `colors.cursor.cursor` | no | the foreground |
| `colors.selection.background` | no | derived (see below) |
| `templum.{accent,canvas,dots}` | no | derived (see below); Templum's own table, see [themes.md](themes.md#theme-files) |

- Colors are strings `"#rrggbb"` or `"0xrrggbb"`, case-insensitive. Any other string (Alacritty's `"CellForeground"`
  and `"CellBackground"`, for example) counts as missing.
- `colors.cursor.text` and `colors.selection.text` are ignored: the cursor and the selection are drawn as translucent
  quads, and the text keeps its color.
- **Light or dark:** a theme is dark when its background's relative luminance (WCAG) is below 0.18, which is about
  CIE L\* 50.

### Derived colors

The interface is computed from the theme, with one formula for every theme. `mix(a, b, t)` is a linear blend in
sRGB, and `c @ a` is color `c` with alpha `a`.

| Token | Value |
|---|---|
| `canvas` | `templum.canvas` if set; else `mix(background, black, 0.30)` for dark themes, `mix(background, black, 0.04)` for light ones |
| `dot` | `templum.dots` if set (opaque); else `foreground @ 0.30` for dark themes, `foreground @ 0.22` for light ones |
| `hairline` | `foreground @ 0.10` |
| `title` / `title_dim` | `foreground @ 0.60` / `foreground @ 0.40` |
| `hint` | `foreground @ 0.40` |
| `hover` | `foreground @ 0.08` (behind the hovered ✕) |
| `accent` | `templum.accent` if set, else `normal.blue`; `ring` is the accent `@ 0.40` |
| `selection` | (`colors.selection.background` if set, else `normal.blue`) `@ 0.35`, drawn over the cells |
| `cursor` | `colors.cursor.cursor` if set, else `foreground`; drawn as today (filled at 0.6 alpha when focused, outlined otherwise) |
| `marked` | `normal.yellow` (IME pre-edit text, on `background`, underlined) |
| `shadow` | `black @ 0.45` for dark themes; `black @ 0.15` for light ones |

The gpui-kit theme, which the picker and the title bar use, gets its colors from the active theme as well:

- `mode` is light or dark;
- `background` is `canvas`: gpui-kit's root paints it under the whole window, the opaque canvas; `title_bar` is
  transparent, drawn on it;
- `popover` is the theme background;
- `foreground` and `popover_foreground` are the theme foreground;
- `border`, `title_bar_border` and `window_border` are `hairline`;
- `muted_foreground` is `title_dim`;
- `accent` and `ring` are `accent`; `accent_foreground`, the text on it (a selected row, a button under the pointer), is `Theme::on_accent`: the theme's background or foreground, whichever reads better on the accent, white or black when neither reaches 4.5:1; in a dark theme, of those four the ones that reach 4.5:1 on the accent, the one that reads best on the ghost buttons' hover (the accent at 50% over the background, as the kit draws it);
- `selection` is `selection`.

Templum never calls `Theme::sync_system_appearance`, so gpui-kit never overwrites these colors.

### Built-in themes

Eight themes are embedded in the binary from `crates/templum/themes/*.toml`. Each file starts with a comment
crediting its source and license.

| Name | Source |
|---|---|
| Catppuccin Latte, Catppuccin Frappé, Catppuccin Macchiato, Catppuccin Mocha | [catppuccin/alacritty](https://github.com/catppuccin/alacritty) (MIT) |
| One Light, One Dark | [alacritty/alacritty-theme](https://github.com/alacritty/alacritty-theme) `one_dark.toml` and `one_light.toml` (Apache-2.0; Atom's One themes are MIT) |
| Apple Light, Apple Dark | Apple's system colors; Light uses the increased-contrast variants so yellow and cyan stay readable on white |

**Defaults:** Catppuccin Latte in light mode, Catppuccin Mocha in dark mode.

### User themes

- **Folder:** the `themes` directory next to `config.toml` (see [Config file](#config-file)). Every `*.toml` file
  directly inside it is a theme.
- **Name:** the file name without `.toml`. If it matches a built-in name, the user's theme wins.
- **Making and editing:** Settings ▸ Appearance duplicates the theme in use into the folder and edits its colors;
  the file of the theme in use is read again when it is saved ([themes.md](themes.md)).
- **Loading:** the folder is read at startup and again every time the picker opens, so a downloaded theme appears
  without a restart.

### Light and dark

- Templum keeps two theme names: one for light mode and one for dark mode.
- At startup it uses `window.appearance()`. While running, `observe_window_appearance` swaps the active theme.
- `Light` and `VibrantLight` count as light; `Dark` and `VibrantDark` count as dark.

### Theme picker

- **The palette button opens a popover** under it, listing:
  - three groups of built-ins: "Catppuccin", "One" and "Apple";
  - then "My themes", only when the folder has valid themes, sorted by name;
  - last, "Get more themes…", which opens the gallery ([themes.md](themes.md#the-gallery)).
- **The active theme has a ✓.**
- **Previewing:** hovering an item, or moving to it with ↑/↓, applies it as a preview to the whole window, terminals
  included.
- **Choosing:** Enter or a click makes it the theme for the **current** mode. If the system is dark, it becomes the
  dark theme. Templum then saves `config.toml` and closes the popover.
- **Cancelling:** Esc or a click outside closes the popover and restores the theme that was active before.
- **Save problems:** if saving fails, the choice still applies until Templum quits, and the popover stays open
  showing one line at the bottom:
  - "Not saved: config.toml has an error on line N", when the file doesn't parse;
  - "Not saved: \<io error\>", for anything else.

### Config file

| OS | Path |
|---|---|
| Linux, macOS | `$XDG_CONFIG_HOME/templum/config.toml`, falling back to `~/.config/templum/config.toml` |
| Windows | `%APPDATA%\templum\config.toml` |

This spec creates the file with one table. The Config sub-project adds the rest.

```toml
[theme]
light = "Catppuccin Latte"
dark = "Catppuccin Mocha"
```

- **Reading** happens once, at startup. A missing file or a missing key means the default.
- **Writing:**
  - The picker changes only `theme.light` or `theme.dark`, through `toml_edit`, so comments, formatting and other
    tables survive.
  - It creates the folders and the file if they don't exist.
  - It never writes over a file that fails to parse.

### Colors that terminal apps ask for

Programs ask the terminal for its colors (OSC 4, 10 and 11). vim and neovim do this to pick a light or dark
background. They now get the active theme's colors instead of the constants used today.

## Architecture

### `templum-terminal`

- `palette.rs`: the `DEFAULT_FG` and `DEFAULT_BG` constants and the `ANSI` table become a `Palette` value:

  ```rust
  pub struct Palette { pub foreground: Rgb, pub background: Rgb, pub ansi: [Rgb; 16] }
  ```

  - `Palette::default()` keeps today's colors, so the crate's tests and behavior don't depend on the app.
  - `resolve` and `query` take `&Palette`. Indexes 16–255 (the color cube and grays) stay computed.
- `Snapshot::refresh(term, &Palette)`: the snapshot is rebuilt every frame already, so a new palette shows on the
  next frame.
- **`Terminal::set_palette(Palette)`:** each terminal keeps the palette it answers color queries with, behind a lock
  shared with its event listener on the PTY thread.

### `templum-canvas`

- `dots.rs` holds `dot_grid(camera) -> DotGrid { spacing, first }`: the spacing in world units after the ×4 rule,
  and the first visible dot's world position, aligned to multiples of the spacing. The painter stops at the
  viewport's edge, so the function doesn't need the viewport. It is pure and unit-tested.

### `templum` (the app)

| File | Responsibility |
|---|---|
| `theme.rs` (new) | `Theme` (name, `Palette`, cursor, selection, `is_dark`, derived tokens), parsing Alacritty TOML, the built-ins (`include_str!`), reading the user folder, `window_background(os, desktop, wayland)` |
| `themes/*.toml` (new) | The 8 built-in themes |
| `config.rs` (new) | Config path, reading `[theme]`, saving one key with `toml_edit` |
| `title_bar.rs` (new) | The integrated bar: zoom level, palette button, + |
| `theme_picker.rs` (new) | The popover: list, preview, choose, cancel, the save-error line |
| `workspace.rs` | Holds the light and dark theme names, the loaded themes, the preview and the active theme; observes the appearance; applies a theme; paints the dot grid and the hint |
| `block.rs`, `grid.rs` | Colors from the active theme; the continuous card; hover state for ✕ and grip |
| `main.rs` | Window options from `TitleBar::window_options()`, the window background, client decorations on Linux |

**New dependency:** `toml_edit`. It is already compiled in the tree (0.25), and it is used both to read theme files
and to edit `config.toml`. No other dependency is added.

## Data flow

1. **Startup:**
   - read `config.toml`;
   - load the built-ins and the user folder;
   - pick the light or dark theme from `window.appearance()`;
   - apply it.
2. **Appearance change:** pick the other theme, then apply it.
3. **Apply** means three things:
   - give the `Palette` to every terminal with `set_palette`;
   - set the gpui-kit theme colors;
   - call `cx.notify()`.

   Painting reads the active theme from the workspace.
4. **Picker:**
   - a hover or an arrow key sets `preview = Some(theme)`, then applies;
   - Esc clears the preview, then applies;
   - Enter sets the theme name for the current mode, saves, clears the preview and applies.
5. **New terminals** get the active palette when they are created.

## Error handling

Templum always starts: no theme or config problem stops it. Warnings go to stderr with the `templum:` prefix, as
today.

| Situation | Behavior |
|---|---|
| No `config.toml` | Defaults. The first save in the picker creates the folders and the file |
| `config.toml` doesn't parse | Defaults, plus a warning with the line number. The picker won't save over it (see [Theme picker](#theme-picker)) |
| `config.toml` names a theme that doesn't exist | That mode's default, plus a warning with the name |
| A theme file doesn't parse or misses a required key | Left out of the list, plus a warning with the file name and the missing key |
| The themes folder doesn't exist or can't be read | No user themes, and no warning when it doesn't exist |
| Saving fails (permissions, disk) | The choice applies until quit, and the picker shows the error line |

## Testing

Tests are written first, as always. Every item runs in CI on Linux, macOS and Windows.

**`templum-terminal`** (unit):

- `resolve` returns the given palette's red for `Named(Red)` and its background for `Named(Background)`;
- `query(257)` answers with the given palette's background;
- `Snapshot::refresh` with a non-default palette resolves cells with it.

**`templum-canvas`** (unit):

- spacing is 24 at zoom 1;
- the ×2 rule gives at least 24 px at every `ZOOM_STEPS` entry, and a Full HD screen at most about 3,700 dots;
- `first` is a multiple of the spacing;
- after a pan, the dots keep their world positions.

**`templum`** (unit):

- **Theme parsing:**
  - Catppuccin Mocha gives the exact `background`, `foreground` and the 16 ANSI colors;
  - `0x` colors are accepted;
  - a missing `bright` table copies `normal`;
  - a missing required key is rejected, and the error names the key;
  - `"CellForeground"` counts as missing;
  - unknown tables are ignored.
- **Derived colors:**
  - `is_dark` for Mocha and Latte;
  - `accent` is `normal.blue`;
  - the `selection` and `cursor` fallbacks;
  - `canvas` for a dark and a light theme.
- **Built-ins:** all 8 parse; the defaults are Latte (light) and Mocha (dark).
- **User folder:**
  - a user theme with a built-in's name wins;
  - a broken file is skipped and the others load;
  - files that aren't `.toml` are ignored.
- **Config:**
  - a missing file gives the defaults;
  - saving into a file with comments and another table keeps both and changes only the key;
  - saving into a file that doesn't parse returns an error and leaves the file byte-for-byte unchanged;
  - an unknown theme name falls back to the default.
- **`window_background`:** every row of the table in [Window](#window).

**`templum`** (headless UI, `#[gpui_kit::test]`, with a temporary config directory):

- **Picker:**
  - opening it lists the built-ins;
  - hovering an item changes the active palette;
  - Esc restores the previous palette;
  - Enter saves `config.toml` with the chosen name for the current mode.
- `Workspace::set_dark` swaps between the light and dark themes. GPUI's `simulate_appearance_change` is not
  public outside GPUI.
- A block's body bounds are the same focused and unfocused (the 1 px shift is gone).
- The ✕ element doesn't exist until the pointer moves over its block.

**Performance:** `--bench` p95 ≤ 16.7 ms, with the numbers in each PR that touches painting.

**Manual check (Hyprland):**

- glass and dots, light and dark;
- switching the system mode with `gsettings set org.gnome.desktop.interface color-scheme prefer-dark` (read through
  xdg-desktop-portal);
- picker preview, save and cancel;
- a theme downloaded from the alacritty-theme repository appears and applies.

Screenshots go in the PR.

## Delivery

Three pull requests, each green in CI and useful on its own:

1. **Themes underneath.** `Palette` in `templum-terminal`; `theme.rs` with the 8 built-ins and the user folder;
   `config.rs` reading `[theme]`; following the system's light or dark mode. Every hard-coded color is replaced, so
   Templum turns Catppuccin.
2. **The new look.** Window background per platform; `TitleBar` with the zoom level and **+**; client decorations on
   Linux; the dot grid; the continuous card (hover ✕ and grip, 1 px focus ring, shadows).
3. **The picker.** The palette button and the popover, preview, choose and cancel, saving with `toml_edit`, the save-error line.
   The README gets a "Themes" section (where the folder is, and how to use a theme from alacritty-theme), and
   ARCHITECTURE.md gets updated.

## Risks

- **Linux light/dark detection depends on xdg-desktop-portal.** GPUI reads the portal's `color-scheme`, and
  "no preference" or no portal means light. On the dev machine the portal currently answers "prefer light", so
  Templum will start in Latte until the system is set to dark.
- **Client-side decorations on Linux** are verified by hand only on Hyprland. GNOME and KDE behavior (resize edges,
  double-click to maximize) is compile-checked in CI and needs a tester.
- **Blur on Windows 10 vs 11.** GPUI's `Blurred` uses the accent-policy API, which works on both, but it is
  checked at compile time only.
- **Readability over busy wallpapers.** Blocks are opaque, so terminal text is never affected. `CANVAS_ALPHA` is
  one constant if the canvas needs to be more opaque.
- **Dot count at low zoom.** With 16 px and ×4 thinning a Full HD screen drew up to 8,200 dots and the bench went
  over the bar, so the minimum spacing is 24 px with ×2 thinning: at most about 3,700 dots on Full HD, 6,500 on
  2560×1440, painted as one layer.
