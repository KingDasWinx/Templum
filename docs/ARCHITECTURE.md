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
- **Shell integration:** the reader thread reads the PTY through a scanner (`templum-terminal`'s `tap.rs`) that
  hears the sequences vte drops: a command's start and end (OSC 133), the folder (OSC 7, 9;9), notifications (OSC 9,
  777). It passes every byte on. The UI reads what it heard each frame; a command's end or a call for attention wakes
  the UI even in a hidden workspace. See [specs/shell-integration.md](specs/shell-integration.md).
- **Painting never waits on a terminal.** Each frame tries the lock and copies the visible cells into a snapshot;
  if the PTY thread holds the lock, the previous snapshot is drawn. Blocking here doubled frame times in the spike.
- Only blocks inside the viewport are drawn.
- **Zoom:** positions, sizes and font size are multiplied by the zoom factor and laid out again, so text is
  rasterized at its real size and stays sharp. Zoom snaps to fixed steps to keep the glyph cache bounded;
  far-away blocks are drawn simplified (level of detail). A zoom glides to its step over 150 ms through
  zooms snapped to 0.02 (`templum-canvas`'s `CameraFlight`); the camera holds the target, the workspace's `view`
  is what is drawn.
- **Persistence (planned):** the workspace (camera + blocks: rect, cwd, shell, title) is saved as JSON in the
  platform data directory. On restore, fresh shells start in the saved directories.

## Look and themes

- The window is opaque: a solid canvas with a dot grid fixed to the world, and blocks drawn as cards on it
  ([specs/polish.md](specs/polish.md)).
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
- The workspace on screen is `Workspace::space`; the hidden ones (`others`) keep their terminals running, unarmed so
  their output never wakes the UI. The sidebar (`sidebar.rs`) switches them by swapping. `session.rs` keeps their
  order and names in `workspaces/index.json` ([specs/workspaces.md](specs/workspaces.md)).
- A block is a terminal or a kanban block (`Space::kanbans`). Kanban boards are app-wide (`Workspace::boards`,
  `board.rs`, saved in `boards.json` with the snapshot); `kanban.rs` draws a block's board at the zoom, with GPUI's
  drag and drop ([specs/kanban.md](specs/kanban.md)).
- The canvas is saved by `crates/templum/src/session.rs`: one JSON file per workspace in the data folder. Every
  5 s the workspace takes a `Layout` (rects and process ids, cheap) and a background task reads the processes
  (`templum_terminal::inspect`: folders, and whether Claude Code or Codex runs, by `agent_of`), builds the `Saved`
  file content and writes it when it changed; on quit it runs at once. Writes take turns and an older one never
  replaces a newer one. At startup a saved canvas is restored (camera, blocks back to front, focus, folders, and an
  agent's resume command typed into its block); a file it can't read is set aside as `.bak`.

## Distribution

Installers are built by [`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) in
`.github/workflows/release.yml`: `.dmg` (macOS), NSIS `.exe` and `.msi` (Windows), `.deb`, AppImage and pacman
(Linux). Builds are unsigned for now; signing and auto-update come later.

## Licensing

Templum is MIT, so every dependency must be permissively licensed. `cargo-deny` enforces this in CI
(`deny.toml`). Copyleft code is never copied in; see [CONTRIBUTING.md](../CONTRIBUTING.md#licensing).
