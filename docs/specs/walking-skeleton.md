# Walking skeleton

- **Date:** 2026-10-10
- **Status:** Draft
- **Builds on:** [ARCHITECTURE.md](../ARCHITECTURE.md), [ADR-0001](../adr/0001-pure-rust-gpu-no-webview.md),
  [ADR-0002](../adr/0002-ui-framework.md), and the prototype on branch `spike/gpui`

## Goal

The first version of Templum you can use every day. You open it and get a terminal. From there you can create more
terminals anywhere on the infinite canvas, move, resize and close them, pan and zoom around, type into them, scroll
their history, select and copy text, and paste.

**Success criteria**

1. Every behavior in [Behavior](#behavior) works on Linux (Wayland and X11), macOS and Windows. CI checks this on
   every OS; Linux is also checked by hand.
2. `cargo run --release -p templum -- --bench` reports **p95 ≤ 16.7 ms**, the same bar as the spike.
3. All tests listed in [Testing](#testing) pass in CI on the 3 OS.
4. No crate other than `templum` depends on GPUI. `templum-canvas` and `templum-terminal` don't depend on each
   other.

## Out of scope

The next plans pick these up:

- **Next phase:** saving and restoring the workspace, and configuration (font, theme, keybindings).
- **Later:**
  - hyperlinks and search;
  - built-in box-drawing glyphs;
  - inheriting the focused terminal's working directory;
  - OSC 52 clipboard, which needs a security review;
  - bell.

Level-of-detail drawing at low zoom, once listed under Later, is specified in [level-of-detail.md](level-of-detail.md).

## Architecture

```
crates/
  templum-canvas/    pure geometry and interaction logic. No dependencies.
  templum-terminal/  shell in a PTY, keys, colors, selection, snapshots. Depends on alacritty_terminal only.
  templum/           GPUI app: views, painting, input routing. Depends on gpui-kit and both crates above.
```

All coordinates in `templum-canvas` are **world units**: logical pixels at zoom 1.

### `templum-canvas`

| Item | Responsibility |
|---|---|
| `Point`, `Rect` | `f32` geometry: `contains`, `intersects`, `union`, `right`, `bottom`. |
| `Camera` | `origin` is the world point at the viewport's top-left. `zoom` is one of `ZOOM_STEPS` (0.01 … 3.0, 23 steps, 1.0 included; 0.25 … 3.0 until card 227). See the camera methods below. |
| `BlockId` | Opaque `u64`, never reused within a session. |
| `Canvas` | Holds `camera` and the blocks (back-to-front, each `{ id, rect }`). See the canvas methods below. |
| `Gesture` | `Pan { last }`, `Move { id, grab }` and `Resize { id, grab }`. `update(&mut Canvas, pointer_screen) -> Option<(BlockId, Rect)>` returns the new rect of a resized block, and resize clamps to the minimum size. |
| `chrome` | Block frame metrics in world units: `TITLE_H = 28`, `PAD = 6`, `HANDLE = 14`. `MIN_COLS = 20`, `MIN_ROWS = 4`. See the chrome functions below. |

`Camera` methods:

- `to_world(screen)` and `to_screen(world)`.
- `pan_by(screen_delta)`.
- `zoom_step(delta, anchor_screen)`: keeps the world point under the anchor fixed.
- `zoom_reset(anchor_screen)`: goes back to 100%.
- `fit(bounds, viewport)`: picks the largest zoom step that shows `bounds` with a 32 px screen margin, centered.
  With no blocks it resets to 100%.

`Canvas` methods:

- `add(rect) -> BlockId` puts the block on top.
- `remove(id)`, `raise(id)`, `get(id)`.
- `visible(viewport_size)`: the blocks intersecting the viewport, back-to-front.
- `bounds()`: the union of all block rects.
- `place_new(size, near) -> Rect`: starts at `near` and shifts by (32, 32) while another block's top-left is within
  8 units.

`chrome` functions:

- `content_rect(block_rect)`: the block minus the title bar and padding.
- `grid_size(content, cell) -> (cols, rows)`.
- `block_size(cols, rows, cell)`: the inverse of `grid_size`.

### `templum-terminal`

| Item | Responsibility |
|---|---|
| `Terminal::spawn(opts, notify) -> io::Result<Terminal>` | Starts the user's default shell in a PTY using alacritty_terminal's own `tty` and `EventLoop`: the login shell on Unix, PowerShell on Windows. See the spawn details below. |
| `key(&KeyInput) -> bool` | Encodes a key press xterm-style and writes it, returning `true` when it consumed the key. See key encoding below. |
| `paste(text)` | Bracketed paste when the app enabled it, with any `ESC` stripped from the payload. Otherwise newlines become `\r`. |
| `write(bytes)` | Raw input, used by the IME commit path. Like `key` and `paste`, it scrolls the view back to the bottom. |
| `resize(cols, rows, cell_px)` | Resizes the grid and the PTY. A no-op if unchanged. |
| `scroll(lines)` | In the alternate screen with alternate-scroll mode set, it sends ↑/↓ keys. Otherwise it scrolls the history. |
| `select_start(cell, side, kind)`, `select_update(cell, side)`, `select_clear()`, `selection_text()` | `kind` is `Simple`, `Word` or `Line`, using alacritty's `Selection`. |
| `title() -> String` | The last OSC 0/2 title. With none, the shell's name: `$SHELL` basename, or `powershell` on Windows. |
| `exited() -> bool` | True once the shell has exited. |
| `Snapshot::refresh(&mut self, &Terminal) -> bool` | Copies the visible cells without ever blocking painting. See the snapshot details below. |
| `palette` | Alacritty's default colors (ported with attribution) plus the xterm 256 palette, resolving OSC-overridden colors first. |

`Terminal::spawn` details:

- `opts`: `cols`, `rows`, `cell_px`, and an optional command for tests.
- `notify: Arc<dyn Fn() + Send + Sync>` is called when there is new output, a title change or an exit. It is
  **coalesced**: after one call, nothing more is sent until the UI calls `rearm()`, so heavy output costs one redraw
  per frame.
- It answers the shell's queries itself, off the UI thread: `PtyWrite`, `ColorRequest` (OSC 10/11/4) and
  `TextAreaSizeRequest`.

Key encoding for `key()`:

- Covered: Enter, Backspace, Tab and Shift+Tab, Esc, arrows, Home/End, PageUp/PageDown, Insert/Delete and F1–F12,
  each with xterm modifier parameters (`CSI 1;5C` for Ctrl+→), app-cursor mode, Ctrl+letter and Alt+key.
- `key()` returns `false` for printable text. That text reaches the terminal through the IME path instead, which
  keeps dead keys and compose working.

`Snapshot::refresh` details:

- It uses `try_lock`. If the PTY thread holds the lock, the previous snapshot is kept, for at most 100 ms.
- The output is rows of text runs (fg `Rgb`, bold, italic, dim), background spans, selected spans and the cursor.
- It contains no GPUI types.

### `templum` (the app)

| File | Responsibility |
|---|---|
| `main.rs` | App init, keybindings, the window, and the hidden `--bench` flag. |
| `workspace.rs` | `Workspace` view, listed below. |
| `block.rs` | The block frame built from gpui-kit components, listed below. |
| `grid.rs` | Picks the font and paints a `Snapshot`, listed below. |
| `bench.rs` | The spike's benchmark: 20 terminals, 5 running `yes`, 10 s of animated pan and zoom, one JSON line on stdout. |

`Workspace` (in `workspace.rs`):

- Owns the `Canvas`, `HashMap<BlockId, Terminal>`, one `Snapshot` per block, the focused block, the active `Gesture`
  and the IME state.
- Handles actions, background mouse events and scrolling.
- Implements `EntityInputHandler`, the IME and text path.

The block frame (in `block.rs`):

- Title text, a × button, a resize handle with the resize cursor, and the body.
- Positioned at `camera.to_screen(rect)`; every size is multiplied by zoom.
- Element IDs for tests: `("block", id)`, `("title", id)`, `("close", id)`, `("handle", id)`, `("body", id)`.

Painting (in `grid.rs`):

- Font: the first available of a monospace list; otherwise the first installed font. It never panics.
- Cell metrics come from the font. Width is the advance of `m`; height is `ceil(ascent + descent)` at size 14.
- A `Snapshot` is painted with `shape_line(…, force_width = cell width)`, the selection highlight, the cursor
  (block when focused, outline when not) and IME pre-edit text.

## Behavior

### Mouse and trackpad

| Input | Result |
|---|---|
| Double-click empty canvas | New 80×24 terminal with its top-left at the pointer, focused |
| Click a block | Focus and raise it |
| Drag the title bar | Move the block |
| Drag the bottom-right handle | Resize the block. The PTY gets the new grid size, clamped to 20×4 |
| Drag empty canvas | Pan |
| Drag in a terminal body | Select text. Double-click selects a word, triple-click selects a line. Letting go copies the selection to the clipboard (`terminal.copy_on_select`, on by default) |
| Wheel or two-finger scroll over a body | Scroll that terminal (see `Terminal::scroll`) |
| Two-finger scroll over empty canvas or a title bar | Pan |
| A mouse's wheel over empty canvas or a title bar | Zoom one step around the pointer (pans with a drawing tool) |
| Ctrl+wheel anywhere, or pinch | Zoom one step around the pointer |
| × button | Close the block, killing its shell |

Drags keep working when the pointer leaves the window, until the button is released.

**Copy on select** (kanban card 119): the selection is copied once per gesture, as the button is let go (not while
it moves; a release the window never heard, at the next move), through the same clipboard as Ctrl+Shift+C; a click that selects nothing copies nothing, and a selection
made otherwise (Find's match, an agent's output) isn't copied. Only the clipboard: Templum writes no PRIMARY
selection on Linux (its middle button pans the canvas, it doesn't paste). Settings ▸ Terminal ▸ Copy selection
automatically turns it off.

A quiet **"Copied"** says it happened (kanban card 125), not a notification: a pill with a check, in the block's
surface and hairline colors, where the button was let go, part of the terminal's body, so it moves and zooms with the
block. It sits above the pointer (below it near the top), grows away from the nearer side and stays inside the body,
even when the drag ended outside it. It fades in over 120 ms, stays, and fades out over the last 180 ms of 1.2 s
(`motion::COPIED`); the next copy starts it over where that one was let go, so there is one at a time. A selection of
several lines says how many ("Copied 3 lines"), counting up to the last line with text. It has no listener: clicks
and the pointer's text cursor go through it to the grid. Ctrl+Shift+C and the menu's Copy show nothing: the person
asked for those copies; the pill answers the one they didn't ask for.

### Keyboard

On Linux and Windows the app keybindings use Ctrl+Shift, so plain Ctrl keys still reach the shell. On macOS they
use Cmd.

| Linux / Windows | macOS | Action |
|---|---|---|
| Ctrl+Shift+T | Cmd+T | New terminal at the viewport center, or beside what is there ([Spawning](connections.md#spawning)) |
| Ctrl+Shift+W | Cmd+W | Close the focused terminal |
| Ctrl+Shift+C | Cmd+C | Copy the selection; does nothing without one |
| Ctrl+Shift+V | Cmd+V | Paste |
| Ctrl+= / Ctrl+- | Cmd+= / Cmd+- | Zoom in / out around the viewport center |
| Ctrl+0 | Cmd+0 | Zoom 100% around the viewport center |
| Ctrl+Shift+1 | Cmd+Shift+1 | Fit all blocks |

Every other key goes to the focused terminal.

### Lifecycle

- **Startup:** one terminal, centered in the viewport at zoom 100%, focused.
- **Title bar:** shows `Terminal::title()`.
- **Shell exits:** its block is removed on the next frame. The focus goes to the top-most remaining block.
- **Empty canvas:** a centered hint reads *"Double-click or press Ctrl+Shift+T to open a terminal"* (Cmd+T on
  macOS).
- **Closing the window:** quits the app and kills every shell.

## Data flow

1. The shell writes. The PTY thread parses the output into `Term` and calls `notify` (coalesced).
2. `notify` sends on a channel to a task spawned by `Workspace`. The task calls `cx.notify()`, which schedules a
   render.
3. `Workspace::render`:
   - calls `rearm()`;
   - removes the blocks whose `exited()` is true;
   - takes `canvas.visible(viewport)` and builds one frame element per visible block, with titles from `title()`.
4. Each body's paint closure calls `Snapshot::refresh`. If it kept stale cells, it requests another animation frame.
   Then it paints the cells.

## Error handling

| Situation | Behavior |
|---|---|
| Spawning the shell fails | No block is created, a gpui-kit notification shows the error, and the error is logged to stderr |
| No font from the monospace list is installed | Fall back to the first installed font; never panic |
| Input to a terminal whose shell already exited | Ignored; the block disappears on the next frame |
| Clipboard unavailable or not text | Paste does nothing |

## Testing

Every item runs in CI on Linux, macOS and Windows.

**`templum-canvas`** (unit tests):
- `to_world`/`to_screen` round trip.
- Zoom keeps the anchor fixed and clamps at both ends.
- `fit` with one and with many blocks, and with none.
- `add`/`remove`/`raise` order.
- `visible` culling.
- `Gesture` move, resize with min-size clamp, and pan.
- `room_at`: at the viewport center, or beside what is there.
- `grid_size`/`block_size` round trip.

**`templum-terminal`:**
- **Unit, keys:** the xterm table, including F1–F12, modifier parameters, app-cursor mode, Ctrl+letter, Alt+key,
  and printable text returning `false`.
- **Unit, palette:** xterm 256 colors and named colors.
- **Unit, paste:** bracketed, ESC stripped; plain, newlines become `\r`.
- **Unit, snapshot:** from a `Term` fed bytes with no PTY. It covers colors, inverse, dim, wide characters, cursor
  position, trailing-blank trimming and selection spans.
- **Integration:** spawn a real command (`sh -c 'printf hello'` on Unix, `cmd /C echo hello` on Windows), wait for
  `exited()`, then check that the snapshot contains `hello`, and that `title()` falls back to a non-empty name.

**`templum`** (headless UI tests with gpui-kit's `#[gpui_kit::test]` harness, test-support feature as a
dev-dependency):
- double-click on empty canvas creates a block, and Ctrl+Shift+T creates one at the viewport center;
- clicking `("close", id)` removes the block, and so does Ctrl+Shift+W;
- dragging `("title", id)` moves the block;
- dragging `("handle", id)` resizes it, and its terminal's grid size changes;
- a selection dragged or double-clicked in a terminal lands in the test harness's clipboard as the button is let go,
  not while it moves, and at the next move after a release the window never heard; a click copies nothing; with `terminal.copy_on_select` off nothing is copied; it is on by
  default; each copy shows `("copied", id)` inside the body by where it was let go (above the pointer at the bottom
  right, below it at the top left), with the count of lines with text, gone after `motion::COPIED` on the test
  clock and started over by the next copy; a click or a copy turned off shows none;
- scrolling over empty canvas pans the camera;
- the zoom keybindings change the zoom step, and fit-all frames every block.

**Performance:** the `--bench` p95 ≤ 16.7 ms check is required on pull requests that touch painting or the terminal
crate (AGENTS.md says so). The numbers go in the PR description.

**Manual check on UI pull requests:** ABNT2 dead keys (´ + a → á), IME composition, and trackpad pan and pinch.

## Delivery

Four pull requests, each green in CI and mergeable on its own:

1. **`templum-canvas`** and its tests. Nothing uses it yet.
2. **`templum-terminal`** and its tests. Nothing uses it yet.
3. **The app:** `Workspace`, block frames, grid painting, create/move/resize/close/pan/zoom/scroll/type, lifecycle,
   `--bench`, and the headless UI tests.
4. **Selection, copy and paste, shell titles, and the zoom and fit keybindings**, with their tests.

## Risks

- **Keybinding names for shifted digits.** GPUI may report Ctrl+Shift+1 as `ctrl-!` on some layouts. Bind both
  names, and check on US and ABNT2.
- **Headless UI tests and real PTYs.** The UI tests spawn real shells. If that turns out flaky on a CI OS, those tests
  spawn a long-lived harmless command instead (`cat` on Unix, `cmd /K` on Windows).
- **Cell height from font metrics.** This may still leave box-drawing gaps with some fonts. Built-in box drawing is
  already listed as later work.
