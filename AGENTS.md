# AGENTS.md

Instructions for AI coding agents working on Templum. Humans: see [CONTRIBUTING.md](CONTRIBUTING.md).

## What this is

A native Rust desktop app: an infinite pan/zoom canvas of terminal blocks, drawn on the GPU with no webview.
Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before changing structure. Decisions live in [docs/adr/](docs/adr/).

## Commands

- Run: `cargo run -p templum`
- Everything CI checks:
  `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
- Licenses and advisories: `cargo deny check` (install with `cargo install cargo-deny --locked`)

## Hard rules

- **Crate boundaries:** only `crates/templum` may depend on the UI framework. `templum-canvas` and
  `templum-terminal` stay UI-free and are tested headlessly.
- **No speculative structure:** don't create crates, modules, traits or config "for later". Add them with the first
  real code that needs them.
- **No copyleft code:** never copy from GPL/AGPL projects (Zed `terminal`/`terminal_view`, Warp client, Zorite
  outside `gpui-whiteboard`). The permissive references you may port from, with an attribution comment, are listed in
  [CONTRIBUTING.md](CONTRIBUTING.md#licensing).
- **Pinned UI framework:** `gpui-kit` is pinned with `=` and upgraded only in a dedicated PR. GPUI is pre-1.0 and
  barely documented: read its source in `~/.cargo/registry/src/*/gpui-pre-*` and the lessons in
  [ADR-0002](docs/adr/0002-ui-framework.md) instead of guessing APIs. The `spike/gpui` branch has a working
  canvas-of-terminals prototype to learn from.
- **Tests:** every logic change ships a test that fails without it. For UI-only changes, state how you verified them.
- **No `unsafe`:** the workspace denies it. If it is truly unavoidable, use a local `#[allow(unsafe_code)]` with a
  `// SAFETY:` comment.
- **Small PRs:** the PR title is an imperative sentence and becomes the squash commit.
- **Performance:** pull requests that touch painting or `templum-terminal` run
  `cargo run --release -p templum -- --bench` (Linux) and paste its JSON line in the PR description.
  The bar is `p95_ms` <= 16.7 (see [ADR-0002](docs/adr/0002-ui-framework.md)).

## Before saying "done"

Run the full check command above and report its result honestly, failures included.
