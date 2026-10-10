# Architecture

Templum is a single native Rust application. There is no webview: the UI is drawn directly on the GPU
([ADR-0001](adr/0001-pure-rust-gpu-no-webview.md)) with [GPUI](https://www.gpui.rs), through
[gpui-kit](https://github.com/longbridge/gpui-kit) ([ADR-0002](adr/0002-ui-framework.md)).

## Crates

| Crate | Kind | Responsibility | May depend on the UI framework? |
|---|---|---|---|
| `templum` | bin | Window, event loop, rendering, input, wiring | **Yes, the only one** (`gpui-kit`) |
| `templum-canvas` | lib | Pure model: camera (pan, zoom), blocks, hit-testing, snapping, layout (de)serialization | No |
| `templum-terminal` | lib | Wraps `alacritty_terminal`: shell in a PTY, input, resize, key encoding, events | No |

Only `templum` exists today. Library crates are created together with the first real code that needs them,
never as empty scaffolding.

**Boundary rule:** if the UI framework ever has to change, only `crates/templum` changes. Logic that can live
without the UI belongs in a library crate, where it is unit-tested headlessly.

## Runtime

```
              ┌─────────────── main thread ───────────────┐
  keyboard ─▶ │ event loop → focused block → key encoding │──bytes──▶ PTY ─▶ shell
  mouse    ─▶ │ camera, drag, resize                      │                 │
              │ draw visible blocks at display refresh    │◀── wakeup ──┐   │ output
              └───────────────────────────────────────────┘             │   ▼
                      │ try_lock → snapshot                    PTY reader thread
                      └──────────▶ Arc<FairMutex<Term>> ◀─ parse ─ (one per terminal)
```

- Each terminal owns a PTY (Unix PTY or Windows ConPTY) and a reader thread, both from `alacritty_terminal`.
  The thread parses output into a `Term` grid and wakes the UI, which coalesces redraws to the display refresh.
  Heavy output (`cat` of a huge file) is absorbed in Rust and only drawn at frame rate.
- **Painting never waits on a terminal.** Each frame tries the lock and copies the visible cells into a snapshot;
  if the PTY thread holds the lock, the previous snapshot is drawn. Blocking here doubled frame times in the spike.
- Only blocks inside the viewport are drawn.
- **Zoom:** positions, sizes and font size are multiplied by the zoom factor and laid out again, so text is
  rasterized at its real size and stays sharp. Zoom snaps to fixed steps to keep the glyph cache bounded;
  far-away blocks are drawn simplified (level of detail).
- **Persistence (planned):** the workspace (camera + blocks: rect, cwd, shell, title) is saved as JSON in the
  platform data directory. On restore, fresh shells start in the saved directories.

## Distribution

Installers are built by [`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) in
`.github/workflows/release.yml`: `.dmg` (macOS), NSIS `.exe` and `.msi` (Windows), `.deb`, AppImage and pacman
(Linux). Builds are unsigned for now; signing and auto-update come later.

## Licensing

Templum is MIT, so every dependency must be permissively licensed. `cargo-deny` enforces this in CI
(`deny.toml`). Copyleft code is never copied in; see [CONTRIBUTING.md](../CONTRIBUTING.md#licensing).
