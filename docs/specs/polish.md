# Polish: opaque canvas, camera flights and a new settings panel

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [design-system.md](design-system.md) (the look, themes and motion) and [settings.md](settings.md)
  (the panel and its fields)

## Goal

Templum works, but it looks raw. This spec makes it feel finished, taking cues from
[The Maestri](https://www.themaestri.app/en#features): clear hierarchy, soft cards, pill buttons, and motion that
explains what happened. Colors still come from the active theme, so every theme gets the new look.

1. **No more glass.** The window is always opaque, and the canvas is a solid fill.
2. **A dot grid you can see.** It helps you keep your bearings while panning and zooming.
3. **The camera glides.** Zooming, resetting the zoom and framing every block animate instead of jumping.
4. **Blocks react.** A hovered block lifts, and the focused block gets a soft accent ring.
5. **Themes crossfade.** Switching or previewing a theme fades the canvas and the chrome.
6. **A settings panel that looks designed.** It keeps its place and behavior but gets Templum's own layout.

**Success criteria**

1. Every behavior below works on Linux, macOS and Windows. CI checks this on every OS, and Linux (Hyprland) is also
   checked by hand.
2. `cargo run --release -p templum -- --bench` still reports **p95 ≤ 16.7 ms**, flights included.
3. All tests in [Testing](#testing) pass in CI on the 3 OS.
4. `templum-canvas` and `templum-terminal` stay free of GPUI.

## Out of scope

- Workspaces and kanban blocks: they are the next two sub-projects, each with its own spec.
- Animating the terminal's own text (it never fades or slides) and gpui-kit's widgets during a theme change.
- A search box in the settings panel.

## Glass removed

- The window background is always `WindowBackgroundAppearance::Opaque`.
- `window.blur` and `window.opacity` are gone from `config.toml`, the template and the settings panel.
- **Old files keep loading.** `config.toml` rejects unknown keys, but the `[window]` table is still accepted and
  ignored, so a file written by an older Templum doesn't turn into an error.
- gpui-kit's own `background` becomes the opaque canvas color.

## Canvas

- **Fill:** the theme's `canvas` color, fully opaque.
- **Dots:** the same lattice as before (every 24 world units, thinned 4× at a time below 16 px), but now drawn as
  squares of `dot_size(zoom) = clamp(2 × zoom, 1.5, 3)` px centered on each lattice point, in the stronger `dot`
  color below. At most a quarter of the gap, so dots never touch.

## New theme tokens

Added to the derived colors of [design-system.md](design-system.md#derived-colors). `fg` is the theme's
foreground, and `c @ a` is color `c` with alpha `a`.

| Token | Value | Used by |
|---|---|---|
| `canvas` | unchanged formula, now an opaque color | the window fill |
| `dot` | `fg @ 0.30` for dark themes, `fg @ 0.22` for light ones (was 0.12) | the dot grid |
| `surface` | the theme background, opaque | block cards, settings panel, theme picker |
| `ring` | `accent @ 0.40` | the focused block's outer ring |
| `hover_border` | `fg @ 0.22` | a hovered block's border |
| `pressed` | `fg @ 0.14` | buttons while pressed |
| `card` | `fg @ 0.04` | settings cards, tabs and key pills |

## Motion

| What | Duration | Easing | Notes |
|---|---|---|---|
| Zoom step (keys, Ctrl+wheel, pinch) | 150 ms | ease-out cubic | The point under the pointer stays under it on every frame |
| Zoom reset, fit all | 150 ms | ease-out cubic | Pivots on the viewport center |
| Block hover lift | 120 ms | linear | Both ways: in and out |
| Theme change or preview | 200 ms | linear | Canvas, dots, chrome; terminal text switches at once |
| Settings panel slide | 200 ms | ease-out cubic | Unchanged |
| Block open and close | 280 / 180 ms | unchanged | Unchanged |

### Camera flights

- **The camera has a target and a view.** The target is where the camera is going, and it is what the zoom label
  shows, what is saved, and what clicks are measured against. The view is what is drawn this frame.
- **The view moves from where it is to the target.** `1/zoom` is interpolated linearly, which keeps every world
  point on a straight path on screen. The pivot's world point is interpolated between its spot in both cameras.
  For a zoom step it is the same point, so the content under the pointer never drifts.
- **Zooms on the way are snapped to 0.02 steps**, like block scales already are, so text is rasterized at only a
  few extra sizes.
- **Chained steps glide.** A new step starts from the view where it is and goes to the next target, so a fast
  Ctrl+wheel is one continuous glide.
- **Panning mid-flight:** wheel panning shifts both ends of the flight, so nothing jumps. Dragging the canvas ends
  the flight on its target first.

### Block hover and focus

- **Hover:**
  - the shadow goes from offset 4 / blur 12 to offset 7 / blur 20, or from 10 / 30 to 13 / 38 for the focused
    block;
  - the border blends from `hairline` to `hover_border`.
- **Focus:** the border is `accent`, and a second shadow with no blur and a spread of 2 (scaled by zoom) draws a
  `ring` around the card. It never changes the card's layout.
- **Title bar buttons** are pills, with `hover` behind them on hover and `pressed` while pressed.
- **The empty canvas** shows "Templum" large, a one-line subtitle, and the keys that open a terminal and the
  settings (from the active keymap), plus "Double-click the canvas".
- **Notifications** get the card radius (10 px).

## Settings panel

The panel keeps everything in [settings.md](settings.md) except its layout: 480 px wide (at most 60% of the
window), sliding in from the right, header with ✕, footer with the `config.toml` path, Edit button and save status.

- **Tabs:** a row of five pills under the header: Appearance, Font, Terminal, Shell, Shortcuts. The selected tab
  has the `card` fill and full-strength text. The panel opens on Appearance.
- **Content:** it scrolls. A large section heading (20 px, semibold), then the section's note when it has one
  (Shell: "Applies to new terminals"), then one rounded card with every field.
- **A field** is one row:
  - on the left, its title (13 px, semibold) and a one-line description in a dimmer color;
  - on the right, the control: a dropdown, switch, number or text field;
  - after it, a reset button that appears only while the value differs from the default.
- **Reset all** sits next to the heading while any field of the section differs from its default. It resets them
  together, in one event, so shortcuts reset at once may trade keys (as gpui-kit's section reset did).
- **Rows** are separated by hairlines inside the card.
- **Gone:** the search box and the section list on the left. The panel is too narrow for both.

## Architecture

| File | Change |
|---|---|
| `templum-canvas/src/dots.rs` | `dot_size(zoom)` |
| `templum-canvas/src/camera.rs` | The camera stores its zoom as a number, so a flight can be drawn between steps. `zoom_step`, `zoom_reset`, `fit` and `at` still land on steps. `Camera::unsnapped` builds a flight's frame. |
| `templum-canvas/src/flight.rs` (new) | `CameraFlight { from, to, pivot }`: `at(t)` and `pan_by` |
| `templum-canvas/src/canvas.rs` | `visible_from(camera, viewport)` |
| `templum/src/config.rs` | `[window]` swallowed; blur and opacity removed |
| `templum/src/theme.rs` | New tokens; `Ui` interpolates for the crossfade |
| `templum/src/motion.rs` | `FLIGHT`, `HOVER`, `THEME` |
| `templum/src/workspace.rs` | The view camera and its flight, the theme crossfade, the new empty-canvas hint |
| `templum/src/block.rs` | Hover lift, focus ring |
| `templum/src/settings.rs` | Tabs, cards and rows instead of gpui-kit's `Settings` |
| `templum/src/bench.rs` | Drives zoom and pan through the workspace, so flights are measured |

The crossfade and the hover lift use gpui-kit's `base::motion::transition`. No dependency is added.

## Testing

Tests are written first, as always.

**`templum-canvas`** (unit):

- `dot_size` at zoom 0.25, 1 and 3, and never more than a quarter of the gap at any zoom step.
- **Flights:**
  - the ends are exact;
  - a zoom step keeps the pointer's world point still on every frame;
  - zooms on the way are on 0.02 steps;
  - the pivot moves in a straight line;
  - `pan_by` shifts both ends.
- `Camera::unsnapped` keeps its zoom, and `zoom_step` from there lands on a step.

**`templum`** (unit):

- `[window]` tables load as the defaults.
- New tokens for Mocha and Latte.
- `Ui` interpolation at 0, 1 and halfway.
- The hover lift's shadow numbers.
- The hint's keys follow the keymap.

**`templum`** (headless UI):

- **Zoom:** a zoom key sets the target at once, the view gets there over the following frames, and the flight ends
  on the target.
- A wheel pan during a flight moves the view too.
- Fit all flies, then shows every block.
- A theme change crossfades: mid-fade the drawn colors differ from the theme's, and afterwards they match.
- **Settings:**
  - the panel opens on Appearance, and the tabs switch sections;
  - the Blink switch edits the config;
  - its reset button appears only while it differs from the default, and resetting removes the key.

**Performance:** `--bench` p95 ≤ 16.7 ms. The bench zooms and pans through flights.

**Manual check (Hyprland):**

- an old `config.toml` with `[window]` opens without an error;
- dots in Latte and Mocha at 25%, 100% and 300%;
- Ctrl+wheel over a block's corner keeps the corner under the pointer;
- Ctrl+0 and fit all glide;
- hover lift and focus ring;
- theme preview crossfade in the picker;
- every settings section next to a live terminal.

## Delivery

1. This spec.
2. Remove the glass.
3. Visible dots and the new tokens.
4. Camera flights.
5. Block and chrome polish.
6. Theme crossfade.
7. The settings panel's layout.

## Risks

- **Glyph sizes during flights.** Each flight rasterizes text at a few in-between sizes. If the bench goes over
  the bar, the step goes from 0.02 to 0.04 before the flight gets shorter.
- **Clicks during a flight** are measured against the target. A click in the 150 ms of a glide can land a few pixels
  off, which is accepted.
