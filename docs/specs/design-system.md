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
  canvas so they move with pan and zoom. When the dots would be closer than 16 px on screen, the spacing is
  multiplied by 4 until it is at least 16 px. At zoom 0.25 the dots are 24 px apart, and at 0.5 they are 48 px apart.
- The empty-canvas hint uses the theme's `hint` color.

### Blocks (continuous card)

- One surface: the header is the same color as the terminal, with no separate strip. Corner radius is 10 world
  units (scaled by zoom), and the background is the theme's background, fully opaque.
- **Title:** centered, in the system UI font, color `title` (focused) or `title_dim` (other blocks).
- **Close button (✕):** right side of the header. **Resize grip:** gpui-kit's resize-corner icon in the bottom-right corner.
  Both are rendered only while the pointer is over the block. They fade in over 120 ms, and fade-out is instant.
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

- The close button and grip fade in over 120 ms.
- **A new block pops in** over 0.28 s: it grows from 88% past 100% (about 101.6%) and settles, fading in.
- **A closed block** shrinks to 92% and fades out over 0.18 s. It leaves the canvas at once and is drawn as a ghost
  that clicks go through; its shell lives until the fade ends.
- Scales snap to 0.02 steps: a block is drawn at zoom × scale, so text is rasterized at only a few extra sizes.
- Nothing else animates (no animated zoom).

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
| `canvas` | `mix(background, black, 0.30)` for dark themes; `mix(background, black, 0.04)` for light ones |
| `dot` | `foreground @ 0.12` |
| `hairline` | `foreground @ 0.10` |
| `title` / `title_dim` | `foreground @ 0.60` / `foreground @ 0.40` |
| `hint` | `foreground @ 0.40` |
| `hover` | `foreground @ 0.08` (behind the hovered ✕) |
| `accent` | `normal.blue` |
| `selection` | (`colors.selection.background` if set, else `normal.blue`) `@ 0.35`, drawn over the cells |
| `cursor` | `colors.cursor.cursor` if set, else `foreground`; drawn as today (filled at 0.6 alpha when focused, outlined otherwise) |
| `marked` | `normal.yellow` (IME pre-edit text, on `background`, underlined) |
| `shadow` | `black @ 0.45` for dark themes; `black @ 0.15` for light ones |

The gpui-kit theme, which the picker and the title bar use, gets its colors from the active theme as well:

- `mode` is light or dark;
- `background` and `title_bar` are transparent: gpui-kit's root paints `background` under the whole window, and
  the glass must show through;
- `popover` is the theme background;
- `foreground` and `popover_foreground` are the theme foreground;
- `border`, `title_bar_border` and `window_border` are `hairline`;
- `muted_foreground` is `title_dim`;
- `accent` and `ring` are `accent`;
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
- **Loading:** the folder is read at startup and again every time the picker opens, so a downloaded theme appears
  without a restart.

### Light and dark

- Templum keeps two theme names: one for light mode and one for dark mode.
- At startup it uses `window.appearance()`. While running, `observe_window_appearance` swaps the active theme.
- `Light` and `VibrantLight` count as light; `Dark` and `VibrantDark` count as dark.

### Theme picker

- **The palette button opens a popover** under it, listing:
  - three groups of built-ins: "Catppuccin", "One" and "Apple";
  - then "My themes", only when the folder has valid themes, sorted by name.
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
- the ×4 rule gives at least 16 px at every `ZOOM_STEPS` entry;
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
- **Dot count at low zoom.** There are at most about 14,000 quads on a 2560×1440 screen. If the bench goes over the
  bar, the minimum spacing goes from 16 px to 24 px.
