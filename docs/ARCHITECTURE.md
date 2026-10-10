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

All three crates exist. New library crates are created together with the first real code that needs
them, never as empty scaffolding. The behavior of the current version is specified in
[specs/walking-skeleton.md](specs/walking-skeleton.md), [specs/design-system.md](specs/design-system.md) and
[specs/config.md](specs/config.md).

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

## Look and themes

- The window is translucent where the system blurs what is behind it (macOS, Windows, and KDE and Hyprland on
  Wayland) and opaque elsewhere. Blocks are always opaque.
- A theme is a file in Alacritty's color format. `crates/templum/src/theme.rs` derives every interface color
  from the terminal colors, so any Alacritty theme works. The built-ins live in `crates/templum/themes/`, with
  their credits.
- `templum-terminal` only knows a `Palette` (foreground, background, 16 ANSI colors). It never sees a theme.
- `config.toml` holds the light and dark theme names; the picker edits it with `toml_edit`, keeping comments.

## Configuration

- `config.toml` is read into a typed `Config` (`crates/templum/src/config.rs`) and polled every 0.5 s. A reload
  applies only what changed; an invalid file keeps the last valid config and shows a notification.
- Shortcuts are Templum's own keymap (`crates/templum/src/keys.rs`), matched in the workspace's key handler, so a
  reload can replace them. GPUI key bindings are only used to keep Tab for the shell.
- `templum-terminal` gets settings as plain values (`TermOptions`, a start folder) and reports a shell's folder
  through `sysinfo`. It never reads the file.
- The settings panel (`crates/templum/src/settings.rs`) builds gpui-kit `Settings` pages from the active `Config`
  every frame. A change becomes a `config::Edit` (set or remove one `table.key`). The workspace applies it on the next
  frame (fonts are measured with the window) and writes it with `toml_edit` 0.3 s after the last one, when the panel
  closes, and when the window closes or the app quits (`config::save_edits` keeps comments, CRLF and a BOM). It writes
  a temporary file and renames it over `config.toml` (the linked file, for a symlink), so a crash never leaves half a
  file. A write that fails keeps its edits for the next try; the poll saves them before reloading a file changed by
  hand.

## Distribution

Installers are built by [`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) in
`.github/workflows/release.yml`: `.dmg` (macOS), NSIS `.exe` and `.msi` (Windows), `.deb`, AppImage and pacman
(Linux). Builds are unsigned for now; signing and auto-update come later.

## Licensing

Templum is MIT, so every dependency must be permissively licensed. `cargo-deny` enforces this in CI
(`deny.toml`). Copyleft code is never copied in; see [CONTRIBUTING.md](../CONTRIBUTING.md#licensing).
