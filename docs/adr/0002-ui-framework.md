# 2. GPUI (through gpui-kit) as the UI framework

- **Date:** 2026-10-10
- **Status:** Accepted

## Context

[ADR-0001](0001-pure-rust-gpu-no-webview.md) settled on a pure-Rust, GPU-rendered UI. We needed a toolkit that can
show many terminals on a zoomable canvas with crisp text, handle IME and dead keys on every platform, and still give
us ready-made widgets for menus, settings and dialogs. The candidates:

- **GPUI.** Zed's framework, Apache-2.0. Shipping alacritty-based terminals exist (tty7, Arbor, termy), and so does
  an infinite pan/zoom canvas (gpui-whiteboard). It has IME on X11, Wayland, Windows and macOS.
  [gpui-kit](https://github.com/longbridge/gpui-kit) adds 60+ widgets.
- **egui.** Its `Scene` container blurs text when zoomed in (emilk/egui#4813).
- **iced.** Canvas text can only be drawn as an overlay.
- **Raw wgpu.** We would have to write the whole toolkit ourselves: widgets, focus, IME, accessibility.

GPUI was the front-runner, so we built a throwaway spike (branch
[`spike/gpui`](https://github.com/KingDasWinx/Templum/tree/spike/gpui)). It is a canvas of real terminals with
pan, zoom, move, resize and typing, plus automated `--bench`, `--shot` and `--selftest` modes. egui would only be
tried if GPUI failed a criterion.

## Spike results

Measured on Arch Linux + Hyprland (Wayland) with an i5-13420H and an RTX 3050 Laptop, at 1920×1080 165 Hz, in a
926×1034 window.

| Criterion | Bar | Result |
|---|---|---|
| Idle memory, 20 terminals | < 300 MB | **164 MB** (146 MB with one terminal) |
| Frame time while panning and zooming, 20 terminals, 5 running `yes` | p95 ≤ 16.7 ms | **p95 12.6 / 14.5 / 13.8 ms** (p50 ≈ 6.5 ms); 17–18 ms when the CPU is saturated by other processes |
| Crisp text at zoom 0.5 / 1 / 2 | Sharp | Sharp at 1 and 2. At 0.5 glyphs are a real 7 px: sharp but tiny |
| Input | Works | Accents and dead keys (´+a → á, ~+a → ã), Enter, Backspace, Tab, arrows and Ctrl+C work; `btop` and `nvim` render correctly. Tested with synthetic Wayland input; a physical ABNT2 keyboard and fcitx5/ibus IME are still to be checked by hand |
| Release binary / clean build | Noted | 26.8 MiB stripped / 3 min (12 threads) |

## Decision

Use GPUI through **`gpui-kit`**, pinned exactly (`gpui-kit = "=0.7.1"`, which pins the `gpui-pre` 0.3.8 snapshot).
`use gpui_kit::*` gives GPUI itself plus the widget library. Only `crates/templum` depends on it.

## Consequences

- **Upgrades are deliberate.** GPUI is pre-1.0, so every upgrade breaks things. Dependabot proposes `gpui-kit`
  bumps in their own PR, and each one is tested by hand.
- **The snapshot has one maintainer.** `gpui-pre` is published by gpui-kit's author, not by Zed. If it stops, we pin an
  exact Zed git revision instead.
- **Terminal rendering is ours to build.** Zed's terminal crates are GPL, so box drawing, ligatures, selection and
  IME preedit are all written in-house.

### Lessons from the spike (read before writing GPUI code)

1. **Painting must never wait on a terminal's lock.** Blocking on each terminal's `FairMutex` pushed p95 to 32–37 ms.
   Paint from a per-terminal snapshot instead (`try_lock`, and reuse the previous snapshot while the PTY thread is
   busy). Coalesce PTY wakeups to one redraw per frame (an atomic flag cleared in `render`).
2. **There is no subtree transform.** The canvas is drawn by hand in one `canvas()` paint closure, with positions and
   font size multiplied by zoom. Use `shape_line(…, force_width = cell width)` to keep glyphs on grid columns.
3. **Keyboard and IME.** Printable text only arrives through `EntityInputHandler::replace_text_in_range`, registered
   every frame with `window.handle_input` during paint. Key-down handlers must call `stop_propagation()`, or the text
   is inserted twice. On Wayland, text reaches the input handler only when the only modifier is Shift, so Alt+key
   must be encoded by hand.
4. **Waking the UI from other threads.** Send through a `futures` unbounded channel to a task spawned on the app that
   calls `notify()`. `window.refresh()` is ignored while drawing; `request_animation_frame()` works.
5. **Surprising defaults.**
   - No monospace font ships with GPUI: pick one from `all_font_names()`, because `resolve_font` panics if nothing
     matches.
   - Mouse-move listeners only fire while the pointer is over the element.
   - Unfocused windows are throttled to 30 fps (`inactive_frame_interval`).
