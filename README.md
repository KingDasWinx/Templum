# Templum

An infinite canvas of terminals. Spawn as many terminals as you like, place them anywhere,
resize them like blocks, and pan and zoom around your workspace.

> **Status: pre-alpha.** Usable as a canvas of terminals; no session saving or settings yet.

## Why

- **Native and light.** Pure Rust, drawn directly on the GPU. No webview, no Electron.
- **Cross-platform.** macOS, Linux (Wayland and X11) and Windows.
- **Built on proven parts.** Terminal emulation by [`alacritty_terminal`](https://crates.io/crates/alacritty_terminal),
  UI by [GPUI](https://www.gpui.rs) (Zed's GPU framework) through [gpui-kit](https://github.com/longbridge/gpui-kit).
  See [the architecture](docs/ARCHITECTURE.md) and the [decision records](docs/adr/).

## Build from source

You need [rustup](https://rustup.rs); the pinned toolchain installs itself.
Platform prerequisites are listed in [CONTRIBUTING.md](CONTRIBUTING.md).

```sh
git clone https://github.com/KingDasWinx/Templum
cd Templum
cargo run -p templum
```

## Using it

| Mouse | Result |
|---|---|
| Double-click empty canvas | New terminal there |
| Drag a title bar / the bottom-right handle | Move / resize the block |
| Drag empty canvas, or scroll over it | Pan |
| Scroll over a terminal | Scroll its history |
| Ctrl+scroll, or pinch | Zoom |
| Drag in a terminal (double-click: word, triple-click: line) | Select text |

| Linux / Windows | macOS | Action |
|---|---|---|
| Ctrl+Shift+T | Cmd+T | New terminal |
| Ctrl+Shift+W | Cmd+W | Close the focused terminal |
| Ctrl+Shift+C / V | Cmd+C / V | Copy / paste |
| Ctrl+= / Ctrl+- | Cmd+= / Cmd+- | Zoom in / out |
| Ctrl+0 | Cmd+0 | Zoom 100% |
| Ctrl+Shift+1 | Cmd+Shift+1 | Fit all blocks |

## Themes

Templum follows your system's light or dark mode. Click the **palette** button in the title bar to pick a theme: hovering or the
arrow keys preview it, and a click or Enter keeps it for the current mode. The default is Catppuccin: Latte in
light mode, Mocha in dark mode.

Any theme in [Alacritty's format](https://github.com/alacritty/alacritty-theme) works. Put its `.toml` file in the
`themes` folder, and it shows up under "My themes" the next time you open the picker:

| OS | Folder |
|---|---|
| Linux, macOS | `~/.config/templum/themes/` (or `$XDG_CONFIG_HOME/templum/themes/`) |
| Windows | `%APPDATA%\templum\themes\` |

Your choice is saved in `config.toml`, one folder up:

```toml
[theme]
light = "Catppuccin Latte"
dark = "Catppuccin Mocha"
```

## Configuration

Everything is in `config.toml`, in the folder from [Themes](#themes) (`~/.config/templum/` on Linux and macOS,
`%APPDATA%\templum\` on Windows). Press **Ctrl+,** (**Cmd+,** on macOS) to open it in your editor: if it doesn't
exist yet, Templum writes it first with every option commented out. Uncomment a line and save, and it applies at
once.

```toml
[font]
family = "JetBrains Mono"
size = 14

[window]
blur = "auto"     # "auto" | true | false
opacity = 0.72    # the canvas, 0 to 1

[theme]
mode = "system"   # "system" | "light" | "dark"

[shell]
program = "fish"
args = ["--login"]
working_directory = "inherit"   # "inherit" (the focused terminal's folder) | "~" | a path

[cursor]
shape = "block"   # "block" | "bar" | "underline"
blink = false

[terminal]
scrollback = 10000

[keys]
new_terminal = "ctrl-shift-t"   # one shortcut, a list, or [] to disable
```

A mistake in the file (a typo, a value out of range) shows a notification with the line, and Templum keeps the
last working settings. The full list is in [docs/specs/config.md](docs/specs/config.md).

## Pre-built binaries

There are no releases yet. When they arrive on the [releases page](https://github.com/KingDasWinx/Templum/releases),
they will be **unsigned** at first:

- **macOS:** right-click the app → *Open* → *Open* (only needed the first time).
- **Windows:** on the SmartScreen prompt, click *More info* → *Run anyway*.

## Contributing

Contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) and the [Code of Conduct](CODE_OF_CONDUCT.md).
Security issues: see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE). You may use, modify and sell Templum, as long as the copyright notice is kept.
