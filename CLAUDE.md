# AGENTS.md

Instructions for AI coding agents working on Templum. Humans: see [CONTRIBUTING.md](CONTRIBUTING.md).

## What this is

A native Rust desktop app: an infinite pan/zoom canvas of terminal blocks, drawn on the GPU with no webview.
Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) before changing structure. Decisions live in [docs/adr/](docs/adr/).

## Commands

- Run: `cargo run -p templum` (`-- --dev` beside an installed Templum: own workspaces and settings in `templum-dev`)
- Dev builds compile the dependencies optimized (`[profile.dev.package."*"]` in `Cargo.toml`), so `cargo run` is
  usable; performance is judged on `--release`.
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
  barely documented: read its source in `~/.cargo/registry/src/*/gpui-fast-*` (the engine, [ADR-0003](docs/adr/0003-gpui-fast-engine.md))
  and the lessons in [ADR-0002](docs/adr/0002-ui-framework.md) instead of guessing APIs. The `spike/gpui` branch has a working
  canvas-of-terminals prototype to learn from.
- **Tests:** every logic change ships a test that fails without it. For UI-only changes, state how you verified them.
- **One layout and design standard** ([docs/specs/design-system.md](docs/specs/design-system.md), its Popups section;
  the Git popup is the reference): every popup or modal is built from `crates/templum/src/popup.rs` (`frame`,
  `header`, `slot`, `content`, `section`, `row`, `line`, `foot`) and never draws its own frame, so each one has the
  full-width header (tile, title, subtitle, actions, Full screen, Close), the resize edges, full screen, Esc and a
  press outside to close, and forms as sections of labelled rows, not a column of stacked fields. Colors come only
  from the theme's tokens, sizes and spacing from the spec's values. A new kind of UI writes its standard in that
  spec in the same change, and its look is checked by running the app beside the Git popup.
- **Every feature in the API** (kanban card 171, [docs/specs/api.md](docs/specs/api.md)): a new feature ships its
  Templum API surface (a tool in the registry, or its data in the API's state) and its lines in `docs/api.md`, in the
  same change. A control tool is in the API by itself; give it its scope (`control::reads`, `control::rule`).
- **Minor issues left for later:** every minor deferred by a review, a plan or a fix goes into
  [docs/minors.md](docs/minors.md), under its area, with where it is, what a person sees and a way to fix it. When one
  is fixed, delete its entry in the same commit as the fix.
- **No `unsafe`:** the workspace denies it. If it is truly unavoidable, use a local `#[allow(unsafe_code)]` with a
  `// SAFETY:` comment.
- **Branches:** `main` and `development`. Commit on `development`, never on `main`, in small commits titled
  with [Conventional Commits](https://www.conventionalcommits.org) and an imperative sentence:
  `feat(canvas): zoom with the wheel while the hand is on`, `fix(terminal): …`, `perf: …`, `docs: …`, `test: …`,
  `refactor: …`, `ci: …`, `chore: …`. The CHANGELOG is generated from `feat`, `fix` and `perf`, so write those for
  someone who uses Templum. Contributors' pull requests target `development` and are squash-merged. The
  maintainer opens the pull request from `development` to `main` and merges it with a **merge commit**, never a
  squash, so both branches keep the same history.
- **Releases:** only the maintainer cuts them, from an up-to-date `main`, with `./publish.sh patch|minor|major`
  (`--dry-run` first). It bumps the version, writes the CHANGELOG section, tags and pushes; the tag runs
  `.github/workflows/release.yml`, which builds every platform and publishes the GitHub Release.
- **Performance:** pull requests that touch painting or `templum-terminal` run
  `cargo run --release -p templum -- --bench` (Linux) and paste its JSON line in the PR description (and
  `--bench-mixed`, an agent cockpit: TUIs, editors, a note, kanban; and `--bench-zoom=<kind>` for a change to
  zooming, run from a real project folder: `cd ~/Documents/github/DaemonQuest &&
  ~/Documents/github/Templum/target/release/templum --bench-zoom=editor`; `--bench-far`, run the same way, for a
  change to drawing at far zoom; `--bench-git` for a change to the Git popup's diff panel; and `--bench-mirror`,
  the cockpit mirrored by the same Templum over loopback, for a change to the mirror's painting or its client).
  The bar is `p95_ms` <= 16.7 (see [ADR-0002](docs/adr/0002-ui-framework.md)).

## Before saying "done"

Run the full check command above and report its result honestly, failures included.
