# Cockpit: notes, drawing, snapping, a toolbar, a workspaces screen, an editor, Git and Docker

- **Date:** 2026-10-11
- **Status:** Draft
- **Builds on:** [kanban.md](kanban.md) (a block that isn't a terminal), [context-menus.md](context-menus.md),
  [workspaces.md](workspaces.md), [persistence.md](persistence.md), [polish.md](polish.md),
  [shell-integration.md](shell-integration.md), [agents.md](agents.md)

## Goal

Templum is a whiteboard that developers work in, and a cockpit for their CLI agents. Today it has terminals and
kanban boards. This spec adds what keeps a developer from leaving it:

1. **Notes:** a block to write in Markdown.
2. **Drawing:** pens, a highlighter, shapes and an eraser, on the canvas itself.
3. **Snapping:** blocks line up with their neighbours while they are moved or resized, as in Figma, with no key held.
4. **A toolbar:** a floating bar at the bottom of the canvas holds every tool; the title bar loses its add buttons.
5. **A workspaces screen:** a full-window view of every workspace, each with a name, a color, an icon or an image,
   and a drag to reorder them.
6. **An editor:** a code editor block with a file tree, tabs, syntax highlighting, search and save.
7. **Git:** a block that does what GitHub Desktop and the editors' Git panels do: changes, diffs, staging, commits,
   branches, history, stashes, push and pull, with files opened in the editor block.
8. **Docker:** a block that lists containers, images, volumes, networks and Compose projects, acts on them, runs new
   containers, and opens logs and shells in terminal blocks.

Every one of these is a block or a tool on the existing canvas, saved with it, drawn at its zoom, with the menus,
motion and theme of the blocks that exist. Nothing leaves the window.

**Success criteria**

1. Every behavior below works on Linux (Wayland and X11), macOS and Windows. CI checks this on every OS, and Linux
   (Hyprland) is also checked by hand.
2. `cargo run --release -p templum -- --bench` still reports **p95 ≤ 16.7 ms**, with 200 strokes on the bench's
   canvas.
3. Every block kind, every drawing and every workspace's look come back after a restart.
4. All tests in each sub-project's Testing section pass in CI on the 3 OS.
5. `templum-canvas`, `templum-terminal`, `templum-git` and `templum-docker` stay free of GPUI.
6. No new dependency is added to the workspace. Everything is built with what the tree already has: GPUI, gpui-kit
   (its editor, Markdown view, tree, table, tabs, color picker, dialogs and the Lucide icons), `serde`,
   `serde_json`, `std::process::Command` for `git` and `docker`.

## Out of scope

- A language server in the editor (completion, hover, go to definition). gpui-kit's editor has the hooks
  (`CompletionProvider`, `HoverProvider`, `DefinitionProvider`); a later spec wires rust-analyzer and friends.
- Hunk and line staging in Git, a commit graph, rebase and cherry-pick, submodules, GitHub pull requests.
- Building images from a Dockerfile in the Docker block, remote Docker contexts, Kubernetes.
- Drawing text labels, images and sticky colors; grouping blocks; connectors that follow blocks.
- Workspace templates, archiving, sharing or exporting a workspace.
- Collaborative editing of notes; notes outside a canvas.

## What the dependency tree already gives us

Read before changing the plan: all of this is in `gpui-kit 0.7.1`, which is already pinned. Only Cargo
**features** of gpui-kit are turned on, no crate is added.

| Need | What we use | Where |
|---|---|---|
| Code editor: rope buffer, undo, line numbers, soft wrap, indentation, search and replace, tree-sitter highlighting, diagnostics, decorations | `gpui_kit::component::input::{Editor, EditorState}` (`.code_editor(true)`, `.language(..)`, `.line_number(..)`, `.soft_wrap(..)`, `open_search`, `set_value`, `value`, `text`) | `gpui-base/src/input/editor/`, `gpui-base/src/input/base/state.rs` |
| Syntax highlighting, 35 languages including Markdown, JSON, diff, Rust, TypeScript, Python, Go, YAML, TOML, Bash | gpui-kit feature `tree-sitter-languages` (turns on gpui-component's grammars) | `gpui-kit/Cargo.toml` features |
| Markdown preview | `TextView::markdown(id, text)` with `.selectable(true)`, `.scrollable(true)`, `.code_block_highlighter(..)` | `gpui-base/src/text/text_view.rs` |
| File tree | `Tree`, `TreeState`, `TreeItem` (lazy children, selection, context menu, reveal) | `gpui-base/src/tree.rs` |
| Tables (containers, images, log) | `Table`, `TableState` | `gpui-component/src/table/` |
| Tabs (editor files, Git and Docker sections) | `TabBar`, `Tab` | `gpui-component/src/tab/` |
| Color choice | `ColorPicker`, `ColorPickerState` (`featured_colors`) | `gpui-component/src/color_picker.rs` |
| Icons: all 1,818 Lucide icons (ISC) by name, `IconName::ALL` | `gpui_kit::assets::AllAssets` instead of `Assets` (binary grows by about 1 MiB) | `gpui-kit-assets` |
| Vector paths for strokes and shapes | `PathBuilder::stroke(width)`, `move_to`, `line_to`, `curve_to`, `build()`, `window.paint_path(path, color)` | `gpui-pre/src/path_builder.rs`, `window.rs` |
| Images from a file | `img(PathBuf)` | `gpui-pre/src/elements/img.rs` |
| Folder and file pickers, opening a URL, revealing a path | `cx.prompt_for_paths`, `cx.open_url`, `cx.reveal_path` | `gpui-pre/src/app.rs` |

**Git and Docker through their command lines, not libraries.** `git` and `docker` are run with
`std::process::Command`, the way VS Code's Git extension and Docker's own extension do it:

- Credentials and SSH keys work as they do in the user's terminal (helpers, agents, `gh auth`), which is where
  `libgit2` and `gix` based clients spend most of their effort. `GIT_TERMINAL_PROMPT=0` keeps `git` from hanging on a
  prompt; when it would have asked, Templum says so and offers a terminal block with the command typed in.
- Podman, Docker contexts, Compose v2 and rootless setups come for free: `[docker] program = "podman"` is the whole
  change.
- No `tokio`, `hyper`, `bollard` (Apache-2.0, but 60 crates) or `libgit2` (GPL-2.0 with a linking exception) enters
  the tree. The output formats are stable, documented interfaces: `git status --porcelain=v2 -z`,
  `git log --format=…`, `docker … --format '{{json .}}'`, `docker events --format '{{json .}}'`.
- Both run off the UI thread, one command at a time per block, through `cx.background_executor().spawn`.

## 1. Blocks of any kind

The canvas holds a `Space::terminals` map and a `Space::kanbans` map, and five enums repeat the choice
(`block::Content`, `workspace::GhostBody`, `session::Item`, `session::LayoutBlock`, `menu::Target`). Four new
kinds would make that twenty arms spread over five files. Before anything else, the two maps become one:

```rust
// workspace.rs
pub(crate) enum Body {
    Terminal(Pane),
    Kanban(Kanban),
    Note(Note),       // note.rs
    Editor(Editor),   // editor.rs  (the block's state, not gpui-kit's element)
    Git(Git),         // git_block.rs
    Docker(Docker),   // docker_block.rs
}
pub(crate) struct Space { pub blocks: HashMap<BlockId, Body>, /* as today, minus terminals and kanbans */ }
impl Space {
    pub fn terminals(&self) -> impl Iterator<Item = (BlockId, &Pane)>;
    pub fn terminal(&self, id: BlockId) -> Option<&Pane>;
}
```

- `block::Content<'a>` becomes `&'a Body`. `GhostBody` becomes `Body`. `session::Item` gets one variant per kind,
  `LayoutBlock` becomes `Terminal(Rect, Processes)` or `Other(Item)` (the other kinds are built on the UI thread,
  they have no processes to read). `menu::Target::Terminal` and `Kanban` become `Target::Block(BlockId)`, and
  the menu's items come from the body's kind.
- Each kind has its default size, its smallest size (`restorable` clamps with it) and its title; `block.rs` asks
  the body for them.
- **A block kind's body is only drawn while the block is live.** A ghost of a note, editor, Git or Docker block is
  drawn empty, as kanban ghosts are today.
- This step changes no behavior: the existing tests pass unchanged except for renamed fields.

## 2. Snapping

While a block is moved or resized, its edges and center snap to the other blocks' edges and centers when they
come within **8 screen pixels**, with no key held. The block can still be put anywhere: snapping only pulls
when it is that close, and lets go as soon as the pointer moves away.

- **What snaps, moving:** the block's left, right and horizontal center to the others' left, right and horizontal
  center; its top, bottom and vertical center to their top, bottom and vertical center. Each axis takes the
  candidate with the smallest distance within the threshold, independently.
- **What snaps, resizing:** the right and bottom edges to the others' left, right (and top, bottom) edges, and
  the width or height to another block's width or height, so two blocks can be made the same size.
- **Guides:** while a snap is in effect, a 1 px line in the `accent` color is drawn along the aligned edge (or
  center), from the nearest end of the two blocks to the farthest. It disappears when the drag ends or the snap
  lets go.
- **Threshold in world units** is `8 / zoom`, so it feels the same at every zoom.
- Every block of the space counts, visible or not. There are at most a few hundred.
- Panning never snaps. Blocks don't snap to the dot grid or to drawings.

Architecture: `templum-canvas/src/snap.rs`, pure and unit-tested:

```rust
pub const SNAP_PX: f32 = 8.0;
pub enum Guide { Vertical { x: f32, y0: f32, y1: f32 }, Horizontal { y: f32, x0: f32, x1: f32 } }
pub struct Snapped { pub rect: Rect, pub guides: Vec<Guide> }
pub fn snap_move(rect: Rect, others: &[Rect], threshold: f32) -> Snapped;
pub fn snap_resize(rect: Rect, others: &[Rect], threshold: f32) -> Snapped;
```

`Gesture::update` computes the unsnapped rect from the grab offset as today, then snaps it (never accumulating:
the rect is recomputed from the pointer every move), and returns the guides; the workspace keeps them in
`Space::guides` and paints them in the canvas layer, after the dots and before the blocks.

## 3. The toolbar

A floating pill at the bottom center of the canvas, 12 px above the edge, over the blocks, with the `surface`
fill, a hairline border, the card radius and the unfocused block's shadow. Buttons are 32 px, in three groups
separated by hairlines:

| Group | Buttons | What a click does |
|---|---|---|
| Pointer | **Select** (`mouse-pointer-2`), **Hand** (`hand`) | Select is the normal state. Hand pans with any drag or wheel, over blocks too, like holding Alt ([navigation.md](navigation.md)); it stays on until Select is clicked or Esc is pressed. |
| Blocks | **Terminal** (`square-terminal`), **Note** (`sticky-note`), **Kanban** (`layout-dashboard`), **Editor** (`file-code-2`), **Git** (`git-branch`), **Docker** (`container`), **AI agent** (`bot`) | Adds that block at the viewport's center (cascading like a new terminal does) and focuses it. Editor and Git first ask for a folder, as agents do. AI agent opens the agents menu above the button, the same list as the canvas menu. The tool goes back to Select at once. |
| Drawing | **Pen** (`pencil`), **Highlighter** (`highlighter`), **Shapes** (`shapes`), **Eraser** (`eraser`) | Drawing tools stay on until Select is clicked or Esc is pressed. Shapes opens a small popover above the button: Line, Arrow, Rectangle, Ellipse; the chosen one becomes the tool and the button shows its icon. |

- **Stroke options** appear as a second, smaller pill right above the toolbar while a drawing tool is on: the
  seven ink colors as swatches (the active one ringed) and three widths (thin, medium, thick). The highlighter
  has one width. The eraser shows nothing.
- Hovering a button shows its tooltip, with its shortcut when it has one.
- The active tool's button has the `card` fill and full-strength icon color, like the settings tabs.
- **The title bar** loses the **+** and board buttons. The canvas's right-click menu **Add ▸** gains Note, Editor,
  Git and Docker.
- **No single-letter shortcuts** (V, H, P…): a letter is typing for the focused terminal. Esc returns to Select
  from any tool and deselects a selected drawing. The new blocks get commands like the existing ones:

  | Command | Linux, Windows | macOS | Opens |
  |---|---|---|---|
  | `new_note` | Ctrl+Shift+M | Cmd+Shift+M | A note at the viewport's center |
  | `new_editor` | Ctrl+Shift+E | Cmd+Shift+E | The editor's folder dialog |
  | `new_git` | Ctrl+Shift+G | Cmd+Shift+G | The Git block's folder dialog |
  | `new_docker` | Ctrl+Shift+D | Cmd+Shift+D | A Docker block |
  | `workspaces` | Ctrl+Shift+O | Cmd+Shift+O | The workspaces screen |

- The toolbar is hidden while the settings panel is open on a window narrower than 900 px, so they don't overlap.
- With a drawing tool or the hand on, a layer over the blocks takes the pointer, as the Alt layer does; Ctrl+wheel
  and pinch still zoom, the wheel still pans.

Architecture: `toolbar.rs` (new) draws it; `Workspace::tool: Tool` (`Select`, `Hand`, `Pen`, `Highlighter`,
`Shape(ShapeKind)`, `Eraser`) and `Workspace::ink_style: (Color, Width)`. The agents submenu builder moves from
`menu.rs`'s canvas arm into a function both call.

## 4. Drawing

Strokes and shapes live on the canvas, in world units, drawn **above the blocks** (so a terminal can be
annotated) and under the toolbar and dialogs. They never take the pointer unless a drawing tool or a selection is
involved, so terminals under them keep working.

### Tools

| Tool | Drag | Result |
|---|---|---|
| Pen | Freehand | A stroke: the points the pointer passed, in world units, keeping only points at least 1.5 world units from the previous one. Drawn as a smooth curve: quadratic segments through the midpoints of consecutive points (`curve_to`), with round joins given by overlapping round caps at each point. |
| Highlighter | Freehand | A stroke drawn 16 world units wide at 35% opacity, under the pen strokes (highlighters are painted first). |
| Line, Arrow | From press to release | A straight segment; an arrow adds a head at the release end: two lines at 30°, `max(10, 4 × width)` long. Holding Shift snaps the angle to multiples of 45°. |
| Rectangle, Ellipse | From press to release | The shape inside that rect. Holding Shift makes it a square or a circle. |
| Eraser | Freehand | Removes every stroke or shape the pointer passes within `6 / zoom` world units of. |

- **Colors:** seven, by name, resolved from the active theme so drawings follow it: `foreground`, `red`,
  `yellow`, `green`, `cyan`, `blue`, `magenta` (the six kanban label colors plus the foreground).
- **Widths:** thin 2, medium 4, thick 8 world units; the highlighter 16. Drawn at `width × zoom` on screen, never
  under 1 px.
- A stroke under 2 points, or a shape smaller than 2 world units, is dropped on release.
- **While drawing,** the stroke so far is painted every frame.

### Selecting, moving, deleting

With the Select tool, a click on a drawing (within `6 / zoom` of it) and not on a block selects it: its bounding
box gets the `accent` outline, 1 px, with 4 px of padding. A drag moves it. Delete or Backspace removes it, Esc
deselects, a click elsewhere deselects. Right-click a drawing: **Delete**. Only one drawing is selected at a time.
Clicking a block always goes to the block: drawings are hit only where no block is.

### Undo

Ctrl+Z (Cmd+Z on macOS) undoes the last drawing change (a stroke, a shape, an erasure, a move, a delete) while a
drawing tool is on or a drawing is selected; otherwise the key goes to the terminal as today. Up to 50 steps per
workspace, kept in memory for the session. Ctrl+Shift+Z redoes.

### Files

A canvas file gets an `"ink"` list, missing when empty, so a Templum from before this spec reads the file and
ignores it (`version` stays 1):

```json
"ink": [
  { "kind": "stroke", "color": "red", "width": 4, "points": [[10.0, 20.5], [12.0, 21.0]] },
  { "kind": "stroke", "color": "yellow", "width": 16, "highlight": true, "points": [[0, 0], [100, 0]] },
  { "kind": "arrow", "color": "foreground", "width": 2, "from": [0, 0], "to": [120, 40] },
  { "kind": "rect", "color": "blue", "width": 2, "from": [0, 0], "to": [200, 100] },
  { "kind": "ellipse", "color": "blue", "width": 2, "from": [0, 0], "to": [200, 100] },
  { "kind": "line", "color": "green", "width": 2, "from": [0, 0], "to": [50, 50] }
]
```

Unknown colors become `foreground`; a point that isn't a finite number drops its drawing; a width is clamped
to 1 … 64. Drawings are saved by the 5-second snapshot with everything else.

### Performance

- A stroke's `Path` is built once per frame per visible stroke. Strokes whose bounding box is outside the viewport
  are skipped. The bench puts 200 strokes of 50 points on its canvas and must keep p95 ≤ 16.7 ms; if it doesn't,
  built paths are cached per (stroke, zoom) first, and the curve becomes a polyline at zoom ≤ 0.33 second.

Architecture: `templum-canvas/src/ink.rs` (pure: `Drawing`, `Stroke`, `Shape`, `Color`, `hit(drawing, point,
tolerance)`, `bounds(drawing)`, `simplify(points, min_distance)`, `move_by`) and `templum/src/ink.rs` (the
paint, the tool's pointer handling, the undo stack, serde in `session.rs`).

## 5. Notes

A **note block** holds Markdown text. It is a block like the others: title bar, close, resize, move, focus, hover
lift, pop-in and fade-out, menus, zoom.

- **Opening one:** the toolbar's Note, **Add ▸ Note** in the canvas menu, or `new_note`. It opens at the viewport's
  center, 360 × 280, at least 200 × 140, empty and focused, in edit mode.
- **Edit mode:** gpui-kit's `Editor` with `language("markdown")`, soft wrap on, no line numbers, no border, the
  block's `surface` behind it, in the terminal font at 13 world units × zoom. Tree-sitter highlights headings,
  lists, code and emphasis; Tab and Shift+Tab indent and outdent list items (gpui-kit's indentation).
- **Preview mode:** `TextView::markdown` of the text, selectable and scrollable, in the UI font, with code blocks
  highlighted and links opening in the browser. The toggle is an eye button at the left of the header (like the
  kanban's ⇅) and in the block's menu.
- **Title:** the first line that starts with `#`, without the hashes, trimmed; "Note" when there is none.
- **Keyboard:** focusing the block focuses its editor, so typing goes there. Templum's shortcuts (Ctrl+Shift+…)
  keep working from inside it. Copy, paste, undo and search are the editor's own.
- **Right-click a note:** Preview (or Edit), Copy all, Close.
- **File:** `{ "kind": "note", "rect": {…}, "text": "# Plan\n- …", "preview": false }`. The text is read from the
  editor at each snapshot. Any size of text is accepted; a text that isn't valid UTF-8 can't be in JSON, so none is.

Architecture: `note.rs` (new): `Note { editor: Entity<EditorState>, preview: bool }`, the body, the title. The
gpui-kit feature `tree-sitter-markdown` comes on with this sub-project (the rest of `tree-sitter-languages` with
the editor).

## 6. The workspaces screen

The sidebar stays as the quick switcher. The screen is where workspaces are looked at and customized.

### Opening it

The sidebar gets a **grid button** (`layout-grid`) at its top, above the tiles; the workspace's name in the title
bar is a button too; and the `workspaces` command. The screen covers the whole window below the title bar, over
the canvas, with a scrim of `canvas @ 0.6` behind a centered sheet: 920 px wide at most, the `surface` fill, the
card radius, the focused block's shadow. Esc, the ✕ in its header, or a click on the scrim closes it. Opening it
slides it up over 200 ms like the settings panel; closing slides it down.

### The screen

- **Header:** "Workspaces", a search field that filters the cards by name as you type, and **New workspace**.
- **Cards,** in the sidebar's order, in a grid of 4 columns (3 under 760 px of width), 200 × 140 each: the icon
  (or image) 40 px, tinted with the workspace's color; the name; a line with what is in it, live:
  "3 terminals · 1 note · 1 Claude Code running" (counts of blocks by kind, and the agents found by the last
  snapshot); the active card has the `accent` border and ring, the others a hairline that goes `hover_border`
  on hover.
- **Click a card:** switches to it and closes the screen. **Right-click:** Open, Edit…, Delete… (disabled for the
  last one, with the same confirmation as the sidebar).
- **Drag a card onto another:** the dragged one takes that card's place (the kanban columns' drag and drop). The
  sidebar follows at once, and `index.json` is written.
- **New workspace** creates "Workspace *n*" as the sidebar's **+** does, switches to it, and opens its edit dialog
  with the name selected. The **+** does the same from now on.

### The edit dialog

Replaces the sidebar's **Rename…**, which becomes **Edit…** everywhere. A dialog 480 px wide:

- A **preview tile** at the top: the sidebar tile as it will look.
- **Name:** a text field; empty is refused, as today.
- **Color:** eight swatches: none, red, yellow, green, cyan, blue, magenta, and the accent; drawn from the theme.
- **Icon:** a grid of Lucide icons, 36 px each, with a search field above it that filters by name
  (`IconName::ALL`, 1,818 icons; "git" shows `git-branch`, `git-commit`…). The chosen icon is ringed. A first row
  of 24 suggested icons shows before any search: rocket, folder, code, terminal, server, database, globe, bug,
  flask, book, briefcase, home, heart, star, zap, cloud, cpu, package, wrench, test-tube, gamepad-2, music,
  camera, coffee.
- **Image:** **Choose an image…** opens the system's file picker (PNG, JPG, JPEG, WebP, SVG); the file is copied
  to `<data folder>/workspaces/icons/<id>.<ext>` (replacing an older one) and shown instead of the icon, cropped to
  the tile's rounded square. **Remove image** goes back to the icon. A file over 4 MB is refused with a message.
- **Save** and Cancel. Enter saves. Every change previews in the tile at once; Cancel puts everything back.

### Where the look shows

- **Sidebar tiles:** the image, else the icon in the color (or `title` when there is no color), else the initials
  as today. A color also tints the tile's fill at 18% (hover and active add their fills over it), and the active
  bar takes the color instead of the accent.
- **Title bar:** the icon or image, 14 px, before the name.
- **Notifications** about a terminal in another workspace show the workspace's icon when it has one.

### Files

`index.json` entries get three optional keys; missing means none, so a file from today loads unchanged:

```json
{ "id": "w2", "name": "Side project", "icon": "rocket", "color": "blue", "image": "w2.png" }
```

- `icon` is a Lucide name; an unknown one is treated as none. `color` is one of the eight names; unknown is none.
  `image` is a file name inside `workspaces/icons/` (letters, digits, `-`, `_`, `.`, no path separators, no
  `..`); a file that doesn't exist or can't be decoded shows the icon instead and says nothing.
- Deleting a workspace removes its image file along with setting its canvas aside.

Architecture: `spaces_screen.rs` (new): the sheet, the cards, the drag and drop, the search; `space_dialog.rs`
(new): the edit dialog; `session.rs`: the three fields, `icons_dir`, `copy_icon`, `remove_icon`; `sidebar.rs`
and `title_bar.rs`: the look; `main.rs`: `AllAssets`.

## 7. The editor

An **editor block** is a small editor on one folder: a file tree, tabs of open files, and gpui-kit's code editor.
It is where Git sends files, and where a developer reads or fixes code without leaving the canvas.

- **Opening one:** the toolbar's Editor, **Add ▸ Editor**, or `new_editor`. A dialog asks for the folder, like
  the agents' ([agents.md](agents.md)): this workspace's recent folders, **Choose a folder…**, or a typed path.
  The block opens 900 × 600, at least 480 × 320. Its title is the folder's name, and `file name — folder` with a
  file open (`●` before a changed file's name).

### The tree

- 220 world units wide on the left, hidden and shown by a panel button at the left of the header; the block
  remembers it. The root is the folder; it is read when the block opens, and a folder's children when it is
  expanded (so `node_modules` costs nothing until it is opened). Folders first, then files, each sorted
  case-insensitively. Hidden files are shown. Symlinks show as what they point to; a cycle is not followed.
- Click a file: opens it in a tab (or shows its tab). Click a folder: expands or collapses it.
- **Right-click a file:** Open, Reveal in file manager, Rename…, Delete…. **Right-click a folder:** New file…,
  New folder…, Open a terminal here (a terminal block in that folder, snapped to the editor's right), Rename…,
  Delete…, Reveal in file manager. Deleting asks first and names the file or how many entries the folder has.
  **Right-click the tree's empty space:** New file…, New folder…, Refresh.
- The tree is refreshed when the block is focused and 2 seconds have passed since the last read, and after any
  of its own actions.

### Tabs and the editor

- One tab per open file, in opening order, with its name and a ✕; a changed file shows ● instead until it is
  saved. Ctrl+W closes the active tab (`close_terminal`'s shortcut is not used: that command closes blocks). A
  tab whose file has unsaved changes asks: Save, Don't save, Cancel.
- The editor is gpui-kit's, `code_editor(true)`, line numbers on, soft wrap off (a toggle in the block's menu),
  the language picked from the file's extension through gpui-component's language registry; unknown extensions
  are plain text. Tab size and indentation come with the language. The terminal font, 13 world units × zoom.
- **Search:** Ctrl+F opens the editor's own search bar (Ctrl+H with replace), Enter and Shift+Enter move, Esc
  closes it. These keys reach the editor only while it has the focus; a terminal keeps them otherwise.
- **Save:** Ctrl+S (Cmd+S) writes the file through a temporary file and a rename (`config::write_whole`), keeping
  the file's line endings (CRLF if the first line ended that way) and its final newline as it was. A write that
  fails shows a notification and keeps the tab changed. The `save` command has that shortcut only while an
  editor or note block has the focus; otherwise Ctrl+S goes to the terminal.
- **Files that aren't text:** a file with a NUL byte in its first 8 KB shows "Binary file" and its size, with
  **Reveal in file manager**. A file over 4 MB opens read-only with a note at its top; over 32 MB it isn't opened
  (a notification says so).
- **Changes on disk:** every 2 seconds, while the block is visible, each open file's modification time is checked.
  Changed and the tab is clean: it is reloaded in place, keeping the cursor line. Changed and the tab has unsaved
  edits: the tab shows ⚠ and the editor's top says "Changed on disk" with **Reload** and **Keep mine**. A file
  that disappeared keeps its tab, marked ●, so Save brings it back.
- **Markdown files** get the note's preview toggle.
- **Go to line:** Ctrl+G opens a small prompt; Enter goes there.

### Receiving a file

`Workspace::open_in_editor(folder, path, line)`:

1. an editor block of this workspace whose folder contains the path, else any editor block of this workspace, else a
   new one on `folder`, placed snapped to the right of the block that asked (or at the viewport's center);
2. opens the file in a tab, scrolls to `line` and focuses the editor;
3. flies the camera there if the block is off screen, as `reveal` does for notifications.

### Files

```json
{ "kind": "editor", "rect": {…}, "folder": "/home/u/proj", "open": ["src/main.rs", "README.md"], "active": 0, "tree": true }
```

- Paths in `open` are relative to the folder; one that no longer exists is skipped on restore. Unsaved edits are
  not saved across restarts: closing a block or quitting with unsaved edits asks first (Save all, Don't save,
  Cancel; on quit, cancel stops the quit).
- A folder that is gone restores as a block that says so, with **Choose another folder…**.

Architecture: `editor.rs` (new): the block state (`folder`, `tree: Entity<TreeState>`, `tabs: Vec<Tab>`,
`active`), the body, the actions, the file reads and writes (in the background), `lang_of(path)`,
`is_binary(bytes)`, `line_ending(text)`. The gpui-kit feature `tree-sitter-languages` comes on here.
`keys.rs`: `save`, `find`, `go_to_line` with their shortcuts, effective only with an editor focused.

## 8. Git

A **Git block** shows one repository and does what the Git panels of VS Code, Zed and GitHub Desktop do, with the
same words. It never runs a command the user didn't ask for, except reads.

- **Opening one:** the toolbar's Git, **Add ▸ Git**, or `new_git`, then the folder dialog (recent folders first:
  a developer's recent folders are usually repositories). The block opens 800 × 560, at least 480 × 320, on the
  repository that contains the folder (`git rev-parse --show-toplevel`). A folder in no repository shows
  "Not a Git repository" with **Initialize** (`git init`) and **Choose another folder…**. No `git` on the computer:
  the block says so, with how to install it per OS.
- **Header:** the repository's name; the current branch as a pill (click: the Branches tab), with ↑*n* ↓*m* after
  it when it is ahead or behind its upstream, or "no upstream"; then **Fetch**, **Pull**, **Push** and ↻.
  During merge, rebase, cherry-pick or revert a banner under the header names it, with **Abort** and, once nothing
  conflicts, **Continue**.
- **Tabs:** Changes, History, Branches, Stashes.

### Changes

Left, 300 world units: the lists. Right: the diff.

- **Conflicts** (only during a merge or rebase with conflicts), **Staged**, **Changes** (unstaged and untracked
  together). Each list has a header with its count and **Stage all** / **Unstage all** / **Discard all…**.
- A row: a status letter in a colored badge (M modified, A added, D deleted, R renamed, U untracked, C conflict,
  T type changed), the file name, its folder dimmed, and on hover: **+** (stage) or **−** (unstage), ↺ (discard,
  asks first), and **Open** (the editor block). Right-click: the same, plus **Copy path** and **Ignore** (appends
  the path to `.gitignore`).
- **Click a row:** its diff on the right, read-only, in gpui-kit's editor with the `diff` language, so additions
  and deletions are colored by tree-sitter; the file's own language is not applied inside hunks (a later step). An
  untracked file shows its whole content as added; a binary file says "Binary file"; a renamed file shows the
  rename and its diff. **Open in editor** above the diff opens the file at the hunk's first line.
- **A conflict row:** Open in editor (the markers are there), **Use ours**, **Use theirs**, **Mark resolved**
  (stages it).
- **Commit:** below the lists, a one-line **Summary** field and a **Description** text area, a checkbox **Amend
  the last commit** (which fills the fields with its message), and **Commit** (disabled with nothing staged or an
  empty summary; Ctrl+Enter from either field commits). After it: a notification "Committed *abc1234*", the lists
  refresh, the fields clear.
- The author comes from Git's config; with none set, Commit is disabled and a line says to set `user.name` and
  `user.email`, with **Set…** (two fields, written with `git config --global`).

### History

- The last 200 commits of the current branch (`git log` with a record format), **Load more** at the end; each row:
  the short hash in the terminal font, the subject, the author, how long ago, and badges for the refs that point
  at it (branch names, tags, HEAD). The list is virtual: only visible rows are built.
- **Click a commit:** the right side shows its full message, author, date, and its files with their status; click
  a file: its diff in that commit. **Right-click:** Checkout… (warns about the detached state), New branch from
  here…, Create tag…, Revert…, Copy hash.
- A search field filters by subject and author as you type (`git log --grep` / `--author`, both, case-insensitive).

### Branches

- **Local** and **Remote** lists, the current branch marked, each with its last commit's subject dimmed and, for
  local ones, its upstream and ↑↓ counts. A filter field.
- **Click a branch:** checkout (a remote one creates the local tracking branch). A checkout that Git refuses
  (local changes in the way) shows Git's message, with **Stash and switch**.
- **New branch…** (from the current one; the name is checked with `git check-ref-format`), and on a branch:
  **Rename…**, **Merge into *current*…**, **Delete…** (asks; an unmerged branch asks again to force), **Publish**
  (a local branch with no upstream: `git push -u origin name`), **Copy name**.

### Stashes

- `git stash list`: each with its message and date. **Stash changes…** asks for a message and whether to include
  untracked files. On a stash: **Apply**, **Pop**, **Drop…**, **Show** (its diff).

### Fetch, pull, push

- **Fetch:** `git fetch --prune`. **Pull:** `git pull --ff-only`; when a fast-forward isn't possible it says so and
  offers **Merge** or **Rebase**. **Push:** `git push`; with no upstream, `git push -u origin <branch>` after
  asking. While one runs, its button shows a spinner and the others are disabled.
- **Credentials:** `git` runs with `GIT_TERMINAL_PROMPT=0` and no terminal, so it never waits for a password. When
  it fails because it would have asked, the notification says so and offers **Run in a terminal**: a terminal block
  in the repository, snapped to the Git block's right, with the command typed in; the user's helper or SSH agent
  then does what it does in any terminal. The same button appears on any failed command's notification, with the
  command's stderr in a dialog on **Details**.

### Refreshing

- `git status --porcelain=v2 --branch -z` (and ahead/behind from it) every 2 seconds while the block is visible,
  at once after any action, and at once when a terminal of this workspace in this repository finishes a command
  (shell integration's `finished` count), so `git add` in a terminal shows in the block within a frame.
- Reads run in the background, one at a time per block; a refresh asked while one runs waits for it and runs once.
- The History and Branches tabs refresh when shown and after actions, not on the 2-second timer.

### Files

```json
{ "kind": "git", "rect": {…}, "folder": "/home/u/proj", "tab": "changes" }
```

Architecture: a new library crate **`templum-git`** (no UI): `Repo` (its root, the `git` program), every
command as a method returning typed values (`Status`, `Entry`, `Branch`, `Commit`, `Stash`, `Diff`) or
`GitError { command, code, stderr }`, and pure parsers for porcelain v2, the log record format, branch and stash
lists, unit-tested on recorded output. `templum/src/git_block.rs` (new): the block, the tabs, the lists, the
diff view, the actions, the refresh timer and the terminal hand-off. `config.rs`: `[git] program = "git"`, in
the settings panel's new **Tools** section.

## 9. Docker

A **Docker block** is Docker Desktop's dashboard on the canvas, for developers who run their stack in
containers. Long-running things (logs, shells, pulls, `compose up`) open in **terminal blocks** next to it, so
the output is a terminal's, searchable and scrollable, and Templum has nothing to invent.

- **Opening one:** the toolbar's Docker, **Add ▸ Docker**, or `new_docker`. 820 × 560, at least 480 × 320. Title:
  "Docker" (or the program's name, "Podman", when configured).
- **Daemon:** `docker version --format '{{json .}}'` when the block opens and whenever a command fails with a
  connection error. Not installed: the block says so, with the install page per OS (**Open**). Installed but not
  running: "Docker isn't running", with **Retry** and, on Linux, **Start** (a terminal block with
  `sudo systemctl start docker` typed in).
- **Tabs:** Containers, Images, Volumes, Networks, Compose. Each has a filter field and ↻.

### Containers

- A table: a state dot (green running, gray exited, yellow paused, red unhealthy), name, image, status ("Up 3
  hours"), ports (`8080→80`, each a link that opens `http://localhost:8080`), CPU % and memory (from
  `docker stats --no-stream --format '{{json .}}'` every 5 seconds for the running ones, while the tab shows).
  A **Show stopped** toggle, on by default. Rows sort by name; click a header to sort by that column.
- **Hover and right-click a row:** Start, Stop, Restart, Pause/Unpause, **Logs** (a terminal block with
  `docker logs -f --tail 200 <name>`), **Shell** (a terminal block with `docker exec -it <name> sh`; `bash` when
  `docker exec <name> bash -c true` succeeds), **Inspect** (a dialog with `docker inspect` as read-only JSON in
  gpui-kit's editor), Copy ID, **Remove…** (asks; a running one asks to force).
- **Run…** opens a dialog: image (a combobox of the local images, or a name to pull), container name, ports (rows
  of host → container), environment (rows of KEY = VALUE), volumes (rows of host path → container path, with a
  folder picker), command, **Remove when it exits**, **Interactive** (off: `-d`). The dialog shows the `docker run`
  line it builds, and **Run** types it into a terminal block next to the Docker block: a pull, an error or an
  interactive process is then visible, and the user sees the command they would have typed.
- Terminal blocks opened here are placed snapped to the Docker block's right, cascading.

### Images

- A table: repository, tag, image ID, size, created. Actions: **Run…** (the dialog with the image set),
  **Pull…** (a tag to pull, typed into a terminal block), Copy tag, **Remove…**; at the top, **Prune unused…**
  (`docker image prune -f`, asking first with what it will do).

### Volumes and networks

- Volumes: name, driver, mount point; **Remove…**, **Prune unused…**. Networks: name, driver, scope; **Create…**
  (name and driver), **Remove…** (the built-in bridge, host and none can't be removed).

### Compose

- `docker compose ls --all --format json`: a row per project with its name, status ("running(3)"), and its config
  files. Expanding a row lists its services (`docker compose -p <name> ps --format json`): service, state, ports,
  with the container actions on each. On a project: **Up** (`docker compose -f <files> up -d` in a terminal
  block), **Down**, **Restart**, **Logs** (`logs -f`), **Open compose file** (the editor block).
- A folder with a `compose.yaml` or `docker-compose.yml` but no project yet shows up when it is one of the
  workspace's recent folders, with **Up** only.

### Live updates

- `docker events --format '{{json .}}'` runs as a child process for as long as the block exists, read on a
  thread like a PTY; each event on a container, image, volume or network marks that tab stale, and a stale tab is
  re-read after 300 ms (events come in bursts). A 15-second poll covers what events don't say. The process is
  killed when the block closes or Templum quits; if it exits by itself (the daemon stopped), the block re-checks
  the daemon.
- Reads run in the background, one at a time per block, like Git's.

### Files

```json
{ "kind": "docker", "rect": {…}, "tab": "containers", "stopped": true }
```

Architecture: a new library crate **`templum-docker`** (no UI): `Docker` (the program), every read as a method
returning typed values parsed from `--format '{{json .}}'` lines with serde (`Container`, `Image`, `Volume`,
`Network`, `Project`, `Service`, `Stats`, `Event`), actions returning `DockerError { command, code, stderr }`,
the `events()` reader, and the command builders (`run_line(RunSpec)`, `logs_line`, `shell_line`,
`compose_up_line`) that give the terminal blocks their text, all unit-tested on recorded output.
`templum/src/docker_block.rs` (new): the block, the tabs, the tables, the dialogs, the terminal hand-off.
`config.rs`: `[docker] program = "docker"`, in **Tools**.

## Shared pieces

- **Placing a block next to another** (`Workspace::add_beside(anchor: BlockId, size)`): to the right of the anchor
  with a 24-unit gap, top-aligned, cascading down by 32 while that spot's top-left is taken, like `place_new`.
  Git, Docker and the editor use it for terminals and editors they open.
- **Typing a command into a new terminal block:** `Workspace::open_terminal_with(folder, command, at)`, what
  `launch_agent` does today, made reusable.
- **Settings ▸ Tools** (new section): `git.program`, `docker.program`, each a text field with a reset button, and
  a line showing the version found (or "not found").
- **Keymap:** the new commands in `keys.rs`, the settings panel's Shortcuts section and the README's table.
  Commands effective only with an editor or note focused (`save`, `find`, `go_to_line`) say so in the panel.
- **Menus:** `menu::Target::Block(id)`, `Target::Ink(index)`, `Target::SpaceCard(id)`, `Target::TreeEntry {block,
  path}`, `Target::GitRow`, `Target::DockerRow` and their items, as [context-menus.md](context-menus.md) describes
  adding one.
- **Docs:** README sections for each feature; ARCHITECTURE.md's crate table gets `templum-git` and
  `templum-docker` and a paragraph on blocks of any kind; this spec is linked from there.

## Error handling

| Situation | Behavior |
|---|---|
| A saved note, editor, Git or Docker block has a rect that isn't finite or is too small | As terminals: dropped, or clamped to its kind's minimum |
| A saved editor or Git folder is gone | The block opens and says so, with **Choose another folder…**; nothing else fails |
| A saved `ink` entry has an unknown kind, a bad color or a non-finite point | That entry is dropped; the rest loads |
| A workspace's image file is missing or can't be decoded | The icon, else the initials; no message |
| `git` or `docker` isn't installed, or the daemon isn't running | The block says so with what to do; it retries when asked |
| A `git` or `docker` command fails | A notification with the first line of stderr, **Details** for all of it, and **Run in a terminal** when it may need input |
| A command is still running when another is asked for on the same block | The second waits, or is refused for the write actions (buttons disabled) |
| Saving a file in the editor fails | A notification; the tab stays changed |
| An editor file is changed by Templum's own save | Its modification time is recorded, so the 2-second check doesn't reload it |
| The eraser or a click hits nothing | Nothing happens |

## Testing

Tests are written first, as always. Every item runs in CI on Linux, macOS and Windows unless marked.

**`templum-canvas`** (unit):

- `snap_move`: an edge within the threshold snaps and gives a guide; one just outside doesn't; centers snap; each
  axis is independent; the nearest candidate wins; `snap_resize` matches another block's width.
- `ink`: `simplify` drops points closer than the minimum; `hit` for a stroke, a line, a rectangle's edge, an
  ellipse's rim and a miss; `bounds`; `move_by`.

**`templum-git`** (unit, on recorded output): porcelain v2 with modified, added, deleted, renamed, untracked,
conflicted and type-changed entries, and the branch headers with ahead/behind; a log record with a multi-line
subject-free message, refs and no parents; branch lists with upstreams; a stash list; a diff split into files
and hunks. **Integration** (CI has `git`): `git init` in a temporary folder, write a file, `status` shows it
untracked, `stage`, `commit`, `log` has one commit, a branch is created and checked out.

**`templum-docker`** (unit, on recorded output): a container line with ports and a stopped one; an image line;
volume, network, compose project and service lines; a stats line; an event line; `run_line` with ports,
environment, volumes, a command and `--rm`; `shell_line` and `logs_line` quote names. Nothing in CI talks to a
daemon.

**`templum`** (unit): `Body` kinds' default and minimum sizes; a note's title from its first heading; `lang_of`
for `.rs`, `.ts`, `.md`, `Dockerfile` and `.unknown`; `is_binary`; `line_ending`; `session` round trips of every
new item kind and of `ink`, with bad entries dropped; `Index` entries with and without icon, color and image, and
an image name with a path separator dropped.

**`templum`** (headless UI):

- **Snapping:** dragging a block's title near another's left edge lands it on that edge and a guide exists; a drag
  10 px away doesn't snap; resizing to another block's width snaps.
- **Toolbar:** it exists with its buttons; the title bar has no `new-terminal` or `new-kanban`; clicking Terminal
  adds a terminal; clicking Pen then dragging on the canvas adds a stroke; Esc returns to Select; clicking a block
  with the pen on doesn't focus it.
- **Drawing:** a stroke is saved and comes back; the eraser removes it; Ctrl+Z after a stroke removes it and
  Ctrl+Z with no drawing tool reaches the terminal (the test shell echoes it); clicking a stroke selects it and
  Delete removes it.
- **Notes:** `new_note` adds a focused note; typing "# Hi" sets the block's title to "Hi"; the preview toggle shows
  the rendered text; a note comes back with its text after a restart.
- **Workspaces screen:** the grid button opens it with a card per workspace; clicking a card switches; dragging a
  card reorders the sidebar and the index; the edit dialog saves a name, a color and an icon, and the tile shows
  them; an image file is copied into the icons folder and removed with the workspace.
- **Editor:** a block on a temporary folder lists its files; clicking one opens a tab with its text; typing marks
  the tab changed and Ctrl+S writes the file; a file changed on disk while clean is reloaded; `open_in_editor`
  reuses the block and opens the file at a line; closing with unsaved changes asks.
- **Git** (CI has `git`): a block on a fresh repository shows an untracked file; **+** stages it; Commit with a
  summary creates a commit that History lists; New branch… creates and checks out; a terminal in the repository
  that runs `git add` makes the block refresh (Unix).
- **Docker:** with `docker.program` set to a script in the test's temporary folder that prints recorded output
  for each subcommand (a shell script on Unix, a `.cmd` on Windows), the Containers tab lists them with their
  state; Logs opens a terminal block whose typed command is the logs line; Run… types the built command into a new
  terminal block.
- **Persistence:** a canvas with one block of every kind, strokes and a customized workspace comes back after a
  restart.

**Performance:** `--bench` with 200 strokes, p95 ≤ 16.7 ms, in the PR that adds drawing and in every PR that
touches painting.

**Manual (Hyprland):** draw on a terminal at zoom 0.5, 1 and 2; snap two blocks and resize to the same width;
a note with a list, indent with Tab, preview; the workspaces screen with an image icon; edit, search and save a
Rust file; stage, commit and push a real repository (SSH); a push that needs a password falls back to a terminal;
Docker: start, stop, logs, shell, run `nginx` with a port and open it in the browser, `compose up` a project.

## Delivery

Six plans, in this order. Each is a batch of small commits on `development`, each commit green in CI and the
app usable after it; each plan ends with its docs.

| Plan | Sub-projects | Depends on | New feature flags |
|---|---|---|---|
| **1. Canvas foundations** | §1 blocks of any kind, §2 snapping, §3 toolbar, §5 notes | — | `tree-sitter-markdown` |
| **2. Drawing** | §4 | 1 (toolbar) | — |
| **3. Workspaces screen** | §6 | — | `AllAssets` |
| **4. Editor** | §7, Settings ▸ Tools | 1 (blocks, toolbar) | `tree-sitter-languages` |
| **5. Git** | §8, `templum-git` | 4 (opens files) | — |
| **6. Docker** | §9, `templum-docker` | 1 (terminal hand-off); 4 for "Open compose file" | — |

Plans 2 and 3 can be done in either order after 1; 5 and 6 are independent of each other.

## Risks

- **gpui-kit's editor at the canvas's zoom.** Kanban text scales by setting `text_size(px(13 × z))` on plain
  elements. The editor computes its line height from the text style it inherits, so the same should hold; the
  first commit of the notes sub-project verifies it at zoom 0.5 and 2. If it doesn't scale, notes and editors are
  drawn at zoom 1 inside a frame that scales (text stays readable at every zoom, which is what one wants from an
  editor anyway), and this spec is amended.
- **Grammar crates.** `tree-sitter-languages` adds about 35 C grammars: a longer clean build (minutes, cached
  afterwards), a larger binary (about 10–15 MB), and each grammar's license must pass `cargo deny` (they are MIT
  or Apache-2.0 as far as checked). If one fails, the feature list becomes the curated set this spec uses:
  markdown, json, toml, yaml, diff, bash, rust, javascript, typescript, tsx, python, go, html, css, c, cpp, java,
  sql, lua, make.
- **Shortcuts inside the editor.** gpui-kit's editor binds its own keys (Ctrl+A, Ctrl+Z, Ctrl+F…) in its focus
  context; Templum's Ctrl+Shift shortcuts bubble past it. A conflict found by hand is settled in favour of the
  editor while it has the focus.
- **Docker on Windows** needs the daemon's named pipe, which the CLI finds by itself; the tests use a stub program,
  so CI never needs Docker.
- **`docker events` and `git` processes on quit.** Children are killed in `Drop`; a `kill -9` of Templum leaves a
  `docker events` process that exits on its next write, which is harmless.
- **Big diffs and logs.** A 10,000-line diff in the read-only editor is fine (gpui-kit handles 200K lines); the
  log is loaded 200 commits at a time.
