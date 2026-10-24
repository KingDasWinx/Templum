# Architecture

Templum is a single native Rust application. There is no webview: the UI is drawn directly on the GPU
([ADR-0001](adr/0001-pure-rust-gpu-no-webview.md)) with [GPUI](https://www.gpui.rs), through
[gpui-kit](https://github.com/longbridge/gpui-kit) ([ADR-0002](adr/0002-ui-framework.md)).

## Crates

| Crate | Kind | Responsibility | May depend on the UI framework? |
|---|---|---|---|
| `templum` | bin | Window, event loop, rendering, input, wiring | **Yes, the only one** (`gpui-kit`) |
| `templum-canvas` | lib | Pure model: camera (pan, zoom), blocks, hit-testing, snapping, level of detail, wires' curves (`wire.rs`), layout (de)serialization | No |
| `templum-terminal` | lib | Wraps `alacritty_terminal`: shell in a PTY, input, resize, key encoding, events; a viewer, a terminal that runs nothing and shows what Templum feeds it (`Terminal::viewer`) | No |
| `templum-git` | lib | Runs the user's `git` as a command (`std::process::Command`) and parses its machine formats into typed values, finds conflict regions by their markers (`conflict.rs`), lays History's commits out in lanes from their parents (`graph.rs`), turns a blame into the diff panel's rows (`blame.rs`), makes a worktree for a new branch under `.worktrees/` (`repo.rs`), keeps the trash of discarded changes, and asks an AI agent for a commit message (`message.rs`) | No |
| `templum-usage` | lib | Reads what CLI agents write about their sessions and plan limits: Claude Code's status line through Templum's relay, and its transcripts (`claude.rs`, which also writes the settings file Claude Code is given, with Templum's hooks, and its `mcp.json`); Codex's rollout files (`codex.rs`); the Gemini CLI extension (`gemini.rs`); the files that give Copilot CLI Templum's server and a plugin of hooks, with the words that name them (`copilot.rs`); the JSON and the JavaScript plugin that give OpenCode and Kilo Code the same through their config variable (`opencode.rs`, `opencode.js`); Qwen Code's MCP file and the system settings file with Templum's hooks that its variable moves (`qwen.rs`); Pi's extension, which reports its events and registers the server where Pi has MCP (`pi.rs`, `pi.ts`); the program those files run and its hook command (`bin`, `hook_command`); when to warn about a limit or tell its reset (`watch.rs`); each agent's variable, items and login for its accounts (`accounts.rs`), and each account's command, the file in `~/.local/bin` that runs its agent on its folder (`accounts/command.rs`). Never an agent's credentials ([specs/usage.md](specs/usage.md)) | No |
| `templum-update` | lib | Finds the newest release on GitHub (`lib.rs`), how Templum was installed and the release file that fits it (`install.rs`). Downloads it and checks GitHub's SHA-256 (`fetch.rs`), then puts it in place: a rename over the AppImage or executable, a bundle swap on macOS, an installer's command line, a command for a terminal ([specs/updates.md](specs/updates.md)) | No |
| `templum-mcp` | lib | The bus Templum's blocks talk over: JSON-RPC lines on a local socket (`bus.rs`), the MCP tools (`tools.rs`), the Templum Agent's control tools and their danger (`control.rs`), the agents' hook events (`hooks.rs`) and the `templum` command line (`cli.rs`: the MCP bridges `mcp` and `control`, `hook`, the verbs) ([specs/connections.md](specs/connections.md)) | No |
| `templum-agent` | lib | The Templum Agent's engine: Claude Code's words and its stream-json lines read into chat events (`claude.rs`), the conversation's rows, its own things and its file (`conversation.rs`), the child process and its reader threads (`engine.rs`), and the fake engine the tests run (`fake.rs`) ([specs/templum-agent.md](specs/templum-agent.md)); a Claude Code subagent's transcript read, as it grows, into the text its block shows (`follow.rs`, [specs/claude-subagents.md](specs/claude-subagents.md)) | No |

All eight crates exist. New library crates are created together with the first real code that needs
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
  the UI even in a hidden workspace. See [specs/shell-integration.md](specs/shell-integration.md). The scripts also
  define a `claude` and a `codex` function that put the words of `TEMPLUM_CLAUDE_ARGS` and `TEMPLUM_CODEX_ARGS`,
  which every new terminal gets (`Workspace::agent_env`), before the person's
  ([specs/connections.md](specs/connections.md#agents-typed-in-a-terminal)).
- **Painting never waits on a terminal.** Each frame re-arms the terminals it paints and copies the rows
  alacritty reports as changed into a per-terminal snapshot (nothing when nothing changed); if the PTY
  thread holds the lock, the previous snapshot is drawn and a frame is asked for. After 100 ms it waits,
  with the unfair lock: for the parse under way, never for the PTY thread's whole read pass. Terminals
  off screen are never re-armed, so their output can't drive frames; a shell's exit wakes the UI anyway.
  Blocking here doubled frame times in the spike.
- Only blocks inside the viewport are drawn.
- **The bus** (`templum-mcp`'s `bus.rs`, [specs/connections.md](specs/connections.md)): a local socket (Unix) or
  loopback TCP with a token (Windows) in the data folder. A thread accepts connections; each has a reader thread and a
  writer thread, and its lines reach the UI thread through a channel that `workspace/bus.rs` drains: the UI thread
  never touches a socket. Every terminal gets `TEMPLUM_BUS` and `<data>/bin` first in `PATH`, where `templum` is
  this Templum: a link to it on Unix, a hard link or a copy named `templum.exe` on Windows. `templum mcp`,
  `templum hook …` and the verbs are the same program run without a window (`main.rs`). Each connection has a number;
  when one ends the UI hears it (`templum/closed`) and drops what it waited for.
  A fourth kind of client, `control`, is the MCP server only the Templum Agent's process gets: its hello carries a
  secret new at each start of the engine, compared at every message, and `workspace/control.rs` serves it on every
  workspace, through the operations both servers share (`workspace/ops.rs`) and the gate (`workspace/approvals.rs`)
  ([specs/templum-agent.md](specs/templum-agent.md)).
- **The Templum Agent** (`templum-agent`, `workspace/agent.rs`): a headless Claude Code started with the first
  message (`std::process::Command`, no PTY), its stdout and stderr read on threads of their own into a channel the UI
  thread drains; Stop sends its interrupt request; the conversation is saved in `<data>/agent/`.
- **Zoom:** positions, sizes and font size are multiplied by the zoom factor and laid out again, so text is
  rasterized at its real size and stays sharp. Zoom snaps to fixed steps to keep the glyph cache bounded.
  Far away, each block is drawn at a level of detail picked by its text's size on screen at the zoom the camera
  last came to rest at (`Space::rest`; `templum-canvas`'s `lod.rs`), so a level never changes mid-glide: Full
  while the text is 5.5 px or more; Silhouette under it, where a terminal paints bars of its text's colors on the
  letters' cells, an editor or a note bars of the words gpui-kit laid out in its last Full frame
  (`silhouette.rs`), and a kanban or checklist block is drawn in full; Card under 2.5 px. The block worked in (focused, or holding the
  keyboard) is always Full, and below Full a title is a fixed-size label (specs/level-of-detail.md).
  A zoom glides to its step over 150 ms through
  zooms snapped to 0.02 (`templum-canvas`'s `CameraFlight`); the camera holds the target, the workspace's `view`
  is what is drawn. Terminals and kanban blocks glide with the view. Editor and note bodies glide too, but
  only through the zooms where gpui-kit's editor rows (1.5 × the font, rounded to whole pixels by GPUI) are
  exact, in a box of that zoom's size that their frames clip: elsewhere the rows would grow a pixel at a time
  while the letters grow smoothly, and the text would jump. Their editors' scroll, in pixels, scales with their
  rows (`editor::keep_rows`). Each of those zooms costs a soft-wrapped editor (a note) a re-wrap of its whole
  text: gpui-kit re-measures it on every font size and wrap width.
  gpui-kit's editor keeps its padding, fold gutter and right margin at fixed pixel sizes: a note's editor box
  makes up for them (`note.rs`), so a note wraps at the same words at every zoom, only a resize re-wraps it.
- **Persistence (planned):** the workspace (camera + blocks: rect, cwd, shell, title) is saved as JSON in the
  platform data directory. On restore, fresh shells start in the saved directories.

## Look and themes

- The window is opaque: a solid canvas with a dot grid fixed to the world, and blocks drawn as cards on it
  ([specs/polish.md](specs/polish.md)).
- A theme is a file in Alacritty's color format. `crates/templum/src/theme.rs` derives every interface color
  from the terminal colors, so any Alacritty theme works; an optional `[templum]` table sets the accent, the canvas
  and the dots instead. The built-ins live in `crates/templum/themes/`, with their credits.
- The file of the theme in use is polled with `config.toml` (`poll_theme` in `workspace/preferences.rs`): saving
  it applies it, and a file that stops reading keeps the last good colors. New themes are written from
  `theme::template`, never over an existing file.
- `gallery.rs` lists alacritty-theme's files through GitHub's API (`templum_update::get`) and keeps them in
  `<data folder>/theme-gallery/` (`index.json` and one `<git hash>.toml` per file); only files not cached are
  downloaded. Installing copies one into the themes folder.
- `templum-terminal` only knows a `Palette` (foreground, background, 16 ANSI colors). It never sees a theme.
- `config.toml` holds the light and dark theme names; the picker edits it with `toml_edit`, keeping comments.

## Source layout

A file holds one responsibility; a module that grows past a few hundred lines becomes a folder of files. In
`crates/templum/src`:

- `workspace/`: `mod.rs` has the types (`Workspace`, `Space`, `Body`, `Tool`…) and the constructor; each other file
  is an `impl Workspace` block for one area: `spaces.rs` (the workspaces), `blocks.rs` (adding, focusing and
  closing blocks), `view.rs` (the camera), `input.rs` (keys and the pointer), `selection.rs` (selected blocks: Shift+drag,
  Shift+click on a title; moving them together from a block or from the box around them, Delete, Copy and
  Paste), `history.rs` (Ctrl+Z and Ctrl+Shift+Z on the
  blocks: added, closed, moved, resized; each step keeps the change that takes it back), `tiling.rs` (grid mode:
  blocks tiled full screen like Hyprland, [specs/grid.md](specs/grid.md)), `render.rs` (drawing a frame),
  `drawing.rs` (the drawing tools), `wires.rs` (wires between blocks: wiring and unwiring, their undo steps, the Wire
  tool, selecting a wire, the curves to paint), `note_tabs.rs` (note blocks' tabs and the notes they open),
  `checklist_blocks.rs` (the checklist each block shows, opened where it is, in an empty block or in a new one
  beside, the field that edits a name or an item in place, and `edit_checklist`: one checklist's content as it was
  before each change, for Ctrl+Z), `persist.rs`
  (restoring and saving the canvas), `preferences.rs` (config,
  themes, settings panel), `files.rs` and `file_tree.rs` (editor blocks), `git_ops.rs`, `git_changes.rs`,
  `git_branches.rs`, `git_history.rs`, `git_discard.rs` (discards through the trash), `git_keys.rs` (the
  Git's keys), `git_switch.rs` (switching branches without losing work), `git_sync.rs` (Sync, Publish, Clone),
  `git_compare.rs` (a branch against the default one), `git_conflicts.rs` (conflicts region by region) and
  `git_message.rs` (commit messages from an agent) (the workspace's Git popup), `usage.rs` (the agents' usage read after each
  snapshot, the limits' notifications, what the Claude Code and Codex Templum opens are given), `bus.rs` (the bus's
  messages: the caller found by its `TEMPLUM_TERMINAL`, the tools, the agents' hook events), `waits.rs` (tool calls
  that wait: a question, a request to connect, an agent's turn; their deadlines, cancels and the inbox's answers),
  `delegate.rs` (tasks handed between agents: typing them, their answers, `reply`, `wait_for_idle`, `send_keys` and
  `interrupt`), `spawn.rs` (`spawn_agent`: an agent beside the caller, its worktree, its model and effort, its prompt
  once it is ready, `close_block`), `subagents.rs` (a Claude Code subagent's block: a viewer fed its transcript off the UI
  thread, opened beside its parent as it starts, closed a minute after it finished unless read, never saved), `content.rs` (the tools on a
  wired block's content: a kanban's cards, a note, a checklist, a file opened in an editor, after their wire check),
  `ops.rs` (the workspace operations both MCP servers call, each taking its target, never a caller: `stop_in`,
  `add_block_in`, the worktree operations, a terminal's text, typing into it, closing a block, cards, notes and
  checklists…), `schedules.rs` (texts an agent asked to have delivered later, and the watches whose match is typed
  back), `worktrees.rs` (`remove_worktree`, `merge_worktree`), `control.rs` (the control server: its tools on every
  workspace, `overview`), `approvals.rs` (the calls that wait for the person, grants, their answers and
  notification), `agent.rs` (the Templum Agent's engine: started, resumed, stopped, its lines folded into the
  conversation, its file). Methods a sibling or a test calls are `pub(super)`.
  `workspace/tests/` holds its tests, one file per area, with the shared helpers in `tests/mod.rs`.
- `settings/`, `git_block/` and `menu/`: the panel's sections, the Git popup's view and tabs, and what each menu lists, each
  in its own file behind the folder's `mod.rs`, which keeps the paths the rest of the crate uses. `git_block/diff_view/`
  holds the diff panel: its rows model (`rows.rs`, pure; Split's pairs too), the highlight cache, the selection,
  and the rows painted on a canvas in the terminal's font, unified or side by side; it shows the working changes,
  a read-only list of files (a comparison, a commit, a stash) or a blame. Each tab has its own panel (Changes',
  History's, Stashes', a comparison's: `Git::diff()` picks the one on screen), so the 2-second read of the
  working changes never touches another tab's. `git_block/history.rs` draws History's left column: the commits
  with their graph, or the commit shown. `git_block/words.rs` holds the Git words' tooltips and the sentences the Git says;
  `git_block/switcher.rs` the branch dialog (switch or create).
- `chat/`: the Templum Agent's state on the workspace (`mod.rs`: `TemplumAgent`, the pill's keyboard, the
  approval's), the pill (`pill.rs`), the chat (`panel.rs`) and its rows (`rows.rs`), and the approval popup
  (`approval.rs`): window chrome over the canvas's bottom center, never scaled by the zoom
  ([specs/templum-agent.md](specs/templum-agent.md#what-a-person-sees)).
- `sidebar.rs`: the workspaces' tiles, a workspace's face (its image, icon or initials, in its color) and the Git
  and inbox buttons at its foot;
  `spaces_screen.rs`: the workspaces screen (cards, search, keys, dragging to reorder); `space_dialog.rs`: a
  workspace's look and its Edit… dialog ([specs/cockpit.md](specs/cockpit.md#6-the-workspaces-screen)).
- `welcome.rs`: the welcome a first run opens over a frosted canvas (each block a soft shadow of its shape: GPUI has
  no backdrop blur), five pages whose settings are the panel's rows (`settings/welcome.rs`); the first terminal
  opens when it closes ([specs/welcome.md](specs/welcome.md)).
- `autostart.rs`: the system's own entry that starts Templum when the person logs in (an XDG autostart file, a
  LaunchAgent, a `Run` registry value), the only record of it ([specs/settings.md](specs/settings.md#start-at-login)).
- `usage.rs`: the title bar's usage meter, its popover and a terminal's usage chip.
- `wires.rs`: painting the wires in the layer under the blocks, with the dot grid, and the dot that travels along one
  while a task is handed over ([specs/connections.md](specs/connections.md#wires)).
- `inbox.rs`: the requests inbox, one popup per workspace for what agents ask the person, and the notice on a
  terminal whose agent waits for an answer ([specs/ask-inbox.md](specs/ask-inbox.md));
  `workspace/inbox.rs` is its model.
- `balloon.rs`: the request itself as the inbox draws it: a question's dialog, a plan to approve, a yes/no request
  ([specs/connections.md](specs/connections.md#requests-from-agents)).
- `update.rs`: where an update stands (`Update`), its title-bar pill and its lines in Settings ▸ Updates;
  `workspace/update.rs` checks GitHub at start and every day, downloads through `templum-update`, tells the news,
  restarts (GPUI's `cx.restart()`) or types the package manager's command in a terminal, and swaps the files when
  Templum quits ([specs/updates.md](specs/updates.md)).

## Configuration

- `config.toml` is read into a typed `Config` (`crates/templum/src/config.rs`) and polled every 0.5 s. A reload
  applies only what changed; an invalid file keeps the last valid config and shows a notification.
  `[connections] enabled` decides whether the Claude Code and Codex Templum opens, or that are typed in its
  terminals, get its MCP server and hooks ([specs/connections.md](specs/connections.md)); the bus runs either way.
- Shortcuts are Templum's own keymap (`crates/templum/src/keys.rs`), matched in the workspace's key handler, so a
  reload can replace them. GPUI key bindings are only used to keep Tab for the shell. Commands that only make sense
  in an editor (`save`, `close_tab`, `go_to_line`) are dropped by `effective` when no editor block has the focus, so
  their keys reach the terminal; so are zoom, settings, the theme picker and the prompt jumps
  (`Command::yields_to_programs`) when a program took over the focused terminal (full screen, an agent, the mouse or
  the kitty keyboard asked for): its own hotkeys use them.
- `templum-terminal` encodes keys as xterm does, and in the kitty keyboard protocol for programs that turn it on
  (Claude Code and Codex hear Shift+Enter); mouse events go to programs that asked for them (xterm's 1000/1002/1003,
  SGR or UTF-8), Shift keeping a drag Templum's selection.
- `templum-terminal` gets settings as plain values (`TermOptions`, a start folder) and reports a shell's folder
  through `sysinfo`. It never reads the file.
- The settings panel (`crates/templum/src/settings/`) builds gpui-kit `Settings` pages from the active `Config`
  every frame; its Tools section names the programs the blocks run (`git.program`). A change becomes a `config::Edit` (set or remove one `table.key`). The workspace applies it on the next
  frame (fonts are measured with the window) and writes it with `toml_edit` 0.3 s after the last one, when the panel
  closes, and when the window closes or the app quits (`config::save_edits` keeps comments, CRLF and a BOM). It writes
  a temporary file and renames it over `config.toml` (the linked file, for a symlink), so a crash never leaves half a
  file. A write that fails keeps its edits for the next try; the poll saves them before reloading a file changed by
  hand.
- The workspace on screen is `Workspace::space`; the hidden ones (`others`) keep their terminals running, unarmed so
  their output never wakes the UI. The sidebar (`sidebar.rs`) switches them by swapping. `session.rs` keeps their
  order, names and looks in `workspaces/index.json` ([specs/workspaces.md](specs/workspaces.md)). A stopped one
  (`Space::stopped`) has no blocks: its canvas waits in `Space::saved`, which the snapshots never write over, and
  `switch_to` restores it as a start does. Its processes end through `templum_terminal::tree` (read before the
  shells hang up) and `kill_left` two seconds later. Moving blocks takes them off one space (`take_block`) and adds
  them to another swapped in as the one shown (`with_space`), still running.
- Every block's content is a `Body` in `Space::blocks`: a terminal, a kanban block, a note block (`note.rs`: tabs
  of plain-text notes in gpui-kit's editor, and the Notes dialog; the notes themselves are app-wide in `notes.rs`, saved
  in `notes.json` with the snapshot, every change written there as it is typed; `workspace/note_tabs.rs` opens, closes
  and deletes them), a checklist block (`checklist.rs`: one checklist, its items checked, edited in place and
  dragged, and the Checklists dialog; the checklists are app-wide in `checklists.rs`, saved in `checklists.json` with
  the snapshot, ids never reused across checklists and items, [specs/checklist.md](specs/checklist.md)), an editor (`editor.rs`: a lazy folder model behind gpui-base's `Tree`, tabs of
  gpui-kit `EditorState`s, files read and written on the UI thread). Git is not a block: each workspace has one
  (`Space::git`), shown in a popup over the canvas's bottom-left corner, at a fixed scale, opened from the sidebar's
  foot or Ctrl+Shift+G (`git_block/`, `workspace/git_ops.rs`: the repository's state read in the background by
  `templum-git` every 2 s while the popup is open, one command at a time with the others waiting in turn, the lists,
  the diff view and the commit form; a failed `git` is a notification that can hand the command to a terminal
  opened at the view's center). Adding a kind is one `Body` arm, one `session::Item` variant and one
  `menu::Items` arm ([specs/cockpit.md](specs/cockpit.md)). Blocks snap to each other's edges and centers while they
  are moved or resized (`templum-canvas`'s `snap.rs`). The toolbar (`toolbar.rs`) holds the tools;
  `workspace::Tool` is the active one. Kanban boards are app-wide (`Workspace::boards`,
  `board.rs`, saved in `boards.json` with the snapshot; a card's attached files are copied by `attachments.rs`
  into `<data>/attachments/<card id>/` on the background executor and swept at start when no card holds them);
  `kanban.rs` draws a block's board at the zoom, with GPUI's
  drag and drop, a card's priority, checklist and who it is for; a column or the board scrolls while a card is dragged
  near its edge ([specs/kanban.md](specs/kanban.md)). The person's instructions for their agents are app-wide too
  (`Workspace::standing`, `instructions.rs`, saved in `instructions.json` with the snapshot): the bus answers a hook's
  `templum/instructions` at a session's start (`standing_for` in `workspace/bus.rs`: which apply to the terminal's
  workspace and agent, framed), and Settings ▸ Instructions edits them
  ([specs/instructions.md](specs/instructions.md)). Note blocks draw their tabs with `tab_strip.rs`.
  Note and checklist blocks share their dialog's body (`list_dialog.rs`: the rows a search finds, open, delete on a
  second click, new).
- The canvas is saved by `crates/templum/src/session.rs`: one JSON file per workspace in the data folder. Every
  5 s the workspace takes a `Layout` (rects and process ids, cheap) and a background task reads the processes
  (`templum_terminal::inspect`: folders, and whether Claude Code or Codex runs, by `agent_of`), builds the `Saved`
  file content and writes it when it changed; on quit it runs at once. Writes take turns and an older one never
  replaces a newer one. At startup a saved canvas is restored (camera, blocks back to front, focus, folders, and an
  agent's resume command typed into its block); a file it can't read is set aside as `.bak`. Each item carries its
  block's `uid` (`Space::uids`: five letters and digits, what agents name blocks by), kept by a restart, an undo and
  a move to another workspace; a missing, taken or malformed one is replaced; the canvas's `wires` name their blocks
  by it. A terminal's item also keeps the block that opened it (`opened_by`), its agent's model and effort, and the
  account its shell was opened for (`Pane::account`, saved as `account`): the shell gets that account's variable
  back, and its agent found there is the account ([specs/accounts.md](specs/accounts.md)), and the shell chosen for it
  (`Pane::shell`, saved as its label in `shell`, [specs/shells.md](specs/shells.md#a-shell-for-each-terminal)); an
  agent's block runs its agent's shell (`[agents]`'s `shell`, `Workspace::agent_shell`, [specs/shells.md](specs/shells.md#a-shell-for-each-agent)),
  and a restored one its own saved shell, not the setting as it is then.
  The workspace's Git is the file's optional `git` object (`SavedGit`: folder, tab, the commit message draft, the
  left column folded, Split); an older file's Git block item is read into it, and no block.

## Distribution

`./publish.sh` (on `main`) bumps the workspace version, writes the `CHANGELOG.md` section from the Conventional
Commits since the last release, and pushes a `vX.Y.Z` tag. The tag runs `.github/workflows/release.yml`: installers
built by [`cargo-packager`](https://github.com/crabnebula-dev/cargo-packager) (`.dmg` for macOS on Apple Silicon and
Intel; NSIS `.exe` and `.msi` for Windows x64; `.deb` and AppImage for Linux x86_64 and ARM64, plus pacman on x86_64),
the bare executables (`.tar.gz` on Linux, `.exe` on Windows), a `.sha256` for each file, and a GitHub Release whose
notes are the CHANGELOG section (a `-rc.N` tag is a prerelease). The app's icon lives in `crates/templum/icons/`:
PNGs for Linux (16 to 48 px are the logo drawn for small sizes), `templum.icns` for macOS, and `templum.ico`, which
the `.msi` and the NSIS installer show and `build.rs` embeds in `templum.exe` (Explorer, the taskbar, its shortcuts and
its title bar take it from there). The macOS app carries an ad-hoc signature (no Apple account:
`signing-identity = "-"`), checked after packaging, so a downloaded one opens after *Open Anyway* in Privacy &
Security instead of being "damaged"; signing with Apple and Microsoft accounts, notarization and package managers
(Homebrew, Scoop, Winget) come later.

## Licensing

Templum is MIT, so every dependency must be permissively licensed. `cargo-deny` enforces this in CI
(`deny.toml`). Copyleft code is never copied in; see [CONTRIBUTING.md](../CONTRIBUTING.md#licensing).
