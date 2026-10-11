# Contributing to Templum

Thanks for helping! Templum is early, so expect things to move fast.

## Setup

1. Install [rustup](https://rustup.rs). The toolchain pinned in `rust-toolchain.toml` installs automatically.
2. Install the platform prerequisites:
   - **Linux (Debian/Ubuntu):**
     `sudo apt install pkg-config libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libfontconfig-dev libfreetype-dev libwayland-dev libvulkan1`
     plus a Vulkan driver (e.g. `mesa-vulkan-drivers`).
   - **Linux (Arch):** `sudo pacman -S --needed base-devel libxcb libxkbcommon libxkbcommon-x11 fontconfig freetype2 wayland vulkan-icd-loader`
     plus a Vulkan driver.
   - **macOS:** Xcode (the full app, not only the command-line tools: GPUI compiles Metal shaders).
   - **Windows:** Visual Studio Build Tools with the "Desktop development with C++" workload.
3. Clone and run:

   ```sh
   git clone https://github.com/KingDasWinx/Templum
   cd Templum
   cargo run -p templum
   ```

## Before you open a pull request

Run what CI runs:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Workflow

- Fork the repo (or create a branch if you have access) and open a pull request against `main`.
  Nobody pushes to `main` directly, maintainers included.
- Keep pull requests small: one change per PR.
- Pull requests are squash-merged and the title becomes the commit message. Write it as an imperative sentence:
  "Add block snapping", "Fix resize on Windows".
- CI (`ci-ok`) must pass and a maintainer must approve.
- Logic changes come with a test. UI changes come with a screenshot or short clip in the PR description.
- Before writing UI code, read the GPUI lessons in [ADR-0002](docs/adr/0002-ui-framework.md#lessons-from-the-spike-read-before-writing-gpui-code).

## Big decisions

Changes to the architecture, notable new dependencies, or anything hard to reverse need an Architecture Decision
Record in [`docs/adr/`](docs/adr/): copy the format of an existing one and use the next number.
Open an issue to discuss it first.

## Licensing

- Templum is MIT. By contributing, you agree that your contribution is licensed under MIT.
- Dependencies must be permissively licensed. `cargo deny check` enforces this (`deny.toml`).
- **Never copy code from GPL/AGPL projects.** Reading them for ideas is fine. Projects to watch out for:
  - Zed's `terminal` and `terminal_view` crates (GPL-3). GPUI itself is Apache-2.0.
  - Warp's client (AGPL-3). Its `warpui` crates are MIT.
  - Zorite (GPL-3). Its `gpui-whiteboard` crate is MIT.
- Code ported from permissive projects keeps an attribution comment. Useful references:
  - [Alacritty](https://github.com/alacritty/alacritty) (Apache-2.0)
  - [tty7](https://github.com/l0ng-ai/tty7) (Apache-2.0)
  - [Arbor](https://github.com/penso/arbor) (MIT)
  - [termy](https://github.com/lassejlv/termy) (MIT)
  - [gpui-whiteboard](https://github.com/packetThrower/zorite/tree/main/crates/gpui-whiteboard) (MIT)
  - [VS Code's shell integration scripts](https://github.com/microsoft/vscode/tree/main/src/vs/workbench/contrib/terminal/common/scripts)
    (MIT) and [WezTerm's](https://github.com/wezterm/wezterm/tree/main/assets/shell-integration) (MIT). Ghostty's
    and Kitty's are GPL-3: don't copy them.

## AI-assisted contributions

They are welcome, but you are responsible for every line: understand it, test it, and be ready to explain it in
review. Conventions for coding agents live in [AGENTS.md](AGENTS.md).

## Where to report things

- Bugs and ideas: [issues](https://github.com/KingDasWinx/Templum/issues).
- Security problems: follow [SECURITY.md](SECURITY.md), not public issues.
- Conduct: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).
