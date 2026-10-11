# Canvas Foundations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Blocks of any kind on the canvas, snapping while they move and resize, a floating toolbar at the bottom instead of the title bar's add buttons, and the first new block kind: a Markdown note.

**Architecture:** The two per-kind maps in `Space` (`terminals`, `kanbans`) become one `HashMap<BlockId, Body>` so new kinds are one enum arm. Snapping is a pure function in `templum-canvas` that `Gesture` calls; the workspace only paints the guides it returns. The toolbar is a new view module; the note block is a new module around gpui-kit's `EditorState` (Markdown highlighting from tree-sitter) and `TextView::markdown` for the preview.

**Tech Stack:** Rust 1.98, GPUI through `gpui-kit =0.7.1` (features `tree-sitter-markdown`, `AllAssets`), `serde_json`. No new crate.

**Spec:** `docs/specs/cockpit.md`, sections 1 (blocks of any kind), 2 (snapping), 3 (toolbar) and 5 (notes). Read the spec's "What the dependency tree already gives us" table first.

## Global Constraints

- Only `crates/templum` depends on `gpui-kit`; `templum-canvas` stays free of GPUI and serde.
- `gpui-kit` stays pinned `=0.7.1`; this plan only adds Cargo **features** to it.
- No `unsafe`. No new dependency in any `Cargo.toml` `[dependencies]`.
- Every logic change ships a test that fails without it; UI changes get a headless `#[gpui_kit::test]`.
- Commits go on `development` directly, each titled with an imperative sentence (AGENTS.md).
- Before every commit: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`.
- Painting changes (Task 4's guides, Task 6's toolbar) need `cargo run --release -p templum -- --bench` with `p95_ms` ≤ 16.7, pasted in the commit message body.
- Snap threshold is **8 screen pixels** (`SNAP_PX`), converted to world units as `8 / zoom`.
- Note defaults: 360 × 280 world units, minimum 200 × 140, editor font 13 world units × zoom, title "Note" without a heading.
- New shortcut: `new_note` = `ctrl-shift-m` (Linux, Windows), `cmd-shift-m` (macOS).
- GPUI lessons in `docs/adr/0002-ui-framework.md` apply: never guess an API, read `~/.cargo/registry/src/*/gpui-pre-0.3.8` and `gpui-base-0.7.1` / `gpui-component-0.7.1`.

## Review Focus

1. **A note whose editor has the focus and a Templum shortcut.** Ctrl+Shift+T pressed inside a note must still open a terminal (the key bubbles from the editor to the workspace's handler). Pinned by `shortcuts_run_from_inside_a_note` in Task 5.
2. **Clicking inside a note's editor must not steal the focus back to the workspace.** The block root's mouse-down runs after the editor's; `focus_block` must focus the editor for notes. Pinned by `clicking_a_note_focuses_its_editor` in Task 5.
3. **Snapping must let go.** A block dragged 8 px past a neighbour's edge must follow the pointer, not stick. Pinned by `snapping_lets_go_past_the_threshold` in Task 3 (pure) and the headless drag test in Task 4.
4. **Dragging a block with the hand tool on.** With the hand tool, a drag over a block's title must pan, never move the block (the layer takes the pointer). Pinned by `the_hand_tool_pans_over_blocks` in Task 6.
5. **A saved note with text that isn't a string, or a missing `preview`.** The file must still load (missing `preview` is `false`; a non-string `text` makes the item fail to parse, which sets the file aside as designed). Pinned by `a_note_item_reads_back_and_preview_defaults_to_false` in Task 5.

## File Structure

| File | Responsibility after this plan |
|---|---|
| `crates/templum/Cargo.toml` | gpui-kit feature `tree-sitter-markdown` |
| `crates/templum/src/main.rs` | `AllAssets` (every Lucide icon, for the toolbar's icons) |
| `crates/templum-canvas/src/snap.rs` (new) | `SNAP_PX`, `Guide`, `Snapped`, `snap_move`, `snap_resize`: pure |
| `crates/templum-canvas/src/gesture.rs` | `Gesture::update` snaps moves and resizes and returns `Update { resized, guides }` |
| `crates/templum-canvas/src/lib.rs` | exports `snap::*` and `gesture::Update` |
| `crates/templum/src/workspace.rs` | `Body`, `Space::blocks`, accessors; `Space::guides`; `Tool`; `new_note`, `add_note_at`, `toggle_preview`; the hand tool's layer; the toolbar's child; Esc |
| `crates/templum/src/block.rs` | `render(ws, block, &Body, …)`, the note's preview button, `Target::Block` |
| `crates/templum/src/menu.rs` | `Target::Block`, `Target::Agents`; `Items::Note`; the agents list shared with the toolbar |
| `crates/templum/src/session.rs` | `LayoutBlock::Saved(Item)`, `Item::Note`, `NoteItem` |
| `crates/templum/src/note.rs` (new) | `Note`, `SIZE`, `MIN`, `title`, `body` |
| `crates/templum/src/toolbar.rs` (new) | the floating toolbar |
| `crates/templum/src/title_bar.rs` | loses the two add buttons |
| `crates/templum/src/keys.rs` | `Command::NewNote` |
| `crates/templum/src/sidebar.rs`, `bench.rs` | use `Space::terminals()` |
| `README.md`, `docs/ARCHITECTURE.md`, `docs/specs/context-menus.md` | docs |

---

### Task 1: Turn on Markdown highlighting and every Lucide icon

**Files:**
- Modify: `crates/templum/Cargo.toml:10` (the `gpui-kit` line)
- Modify: `crates/templum/src/main.rs:32`

**Interfaces:**
- Produces: the `tree-sitter-markdown` grammar for `EditorState::language("markdown")` (Task 5) and `IconName::StickyNote`, `IconName::Hand`, `IconName::MousePointer2`, `IconName::Bot`, `IconName::Eye`, `IconName::EyeOff` drawn with an SVG (Task 5 and 6). Without `AllAssets` those names compile but draw nothing: only 101 icons are embedded by default (`gpui-kit-assets/README.md`).

- [ ] **Step 1: Add the feature**

In `crates/templum/Cargo.toml`, replace the dependency line:

```toml
gpui-kit = { version = "=0.7.1", features = ["tree-sitter-markdown"] }
```

Keep the two comment lines above it. The `[dev-dependencies]` entry stays as it is (Cargo unifies features).

- [ ] **Step 2: Register every icon**

In `crates/templum/src/main.rs`, change the line

```rust
        .with_assets(gpui_kit::assets::Assets)
```

to

```rust
        // Every Lucide icon, by name: the toolbar and the notes use ones outside the default set.
        .with_assets(gpui_kit::assets::AllAssets)
```

- [ ] **Step 3: Build, check licenses, run the app**

Run: `cargo build -p templum && cargo deny check`
Expected: both succeed. The first build compiles `tree-sitter` and `tree-sitter-md` (C code, about a minute). If `cargo deny` rejects a license, stop and report it: the spec's risk section names the fallback.

Run: `cargo run -p templum` and close it. Expected: it opens as before.

- [ ] **Step 4: Run the checks and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.

```bash
git add crates/templum/Cargo.toml crates/templum/src/main.rs Cargo.lock
git commit -m "Turn on Markdown highlighting and the full Lucide icon set"
```

---

### Task 2: One map of blocks of any kind

Pure refactor: no behavior changes, every existing test keeps passing (with renamed accessors).

**Files:**
- Modify: `crates/templum/src/workspace.rs` (the `Space` struct, `Ghost`, and every `.terminals` / `.kanbans` use; lines 76-175, 711-760, 896-1034, 1200-1240, 1304-1340, 1403-1475, 1506-1526, 1848-1920, 1997-2010, 2072-2082, 2194-2232, 2320-2340, 2470-2570, 2749-2765, and the tests)
- Modify: `crates/templum/src/block.rs:98-102, 112-120, 131-136, 172-175, 206-248, 267-293`
- Modify: `crates/templum/src/menu.rs:851-975, 979-993, 1102-1134`
- Modify: `crates/templum/src/session.rs:1375-1424, 679-706`
- Modify: `crates/templum/src/sidebar.rs:122`
- Modify: `crates/templum/src/bench.rs:30`

**Interfaces:**
- Produces (workspace.rs):
  ```rust
  pub(crate) enum Body { Terminal(Pane), Kanban(Kanban) }
  impl Body {
      pub fn terminal(&self) -> Option<&Pane>;
      pub fn terminal_mut(&mut self) -> Option<&mut Pane>;
      pub fn kanban(&self) -> Option<&Kanban>;
      pub fn kanban_mut(&mut self) -> Option<&mut Kanban>;
      pub fn min_size(&self, cell: Size) -> Size;
  }
  pub(crate) struct Space { pub blocks: HashMap<BlockId, Body>, /* unchanged otherwise */ }
  impl Space {
      pub fn terminals(&self) -> impl Iterator<Item = (BlockId, &Pane)>;
      pub fn terminal(&self, id: BlockId) -> Option<&Pane>;
      pub fn terminal_mut(&mut self, id: BlockId) -> Option<&mut Pane>;
      pub fn kanban(&self, id: BlockId) -> Option<&Kanban>;
      pub fn kanban_mut(&mut self, id: BlockId) -> Option<&mut Kanban>;
      pub fn focused_terminal(&self) -> Option<&Pane>;
  }
  impl Workspace { fn add_block(&mut self, rect: Rect, body: Body) -> BlockId; }
  pub(crate) struct Ghost { pub block: Block, pub body: Body, since: Instant }
  ```
- Produces (block.rs): `pub(crate) fn render(ws: &Workspace, block: Block, body: &Body, look: Look, live: bool, window, cx) -> AnyElement`
- Produces (menu.rs): `Target::Block(BlockId)` replaces `Target::Terminal` and `Target::Kanban`; `Workspace::items` returns `Option<Items>`.
- Produces (session.rs): `enum LayoutBlock { Terminal(Rect, Processes), Saved(Item) }`.

- [ ] **Step 1: Write the failing test for the accessors**

In `crates/templum/src/workspace.rs`, inside `mod tests`, after `starts_with_one_focused_terminal`, add:

```rust
    #[gpui_kit::test]
    fn blocks_of_any_kind_share_one_map(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let terminal = first_id(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.new_kanban(window, cx));
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            let kanban = ws.space.canvas.top().unwrap();
            assert_eq!(ws.space.blocks.len(), 2);
            assert!(ws.space.terminal(terminal).is_some());
            assert!(ws.space.kanban(terminal).is_none());
            assert!(ws.space.kanban(kanban).is_some());
            assert!(ws.space.terminal(kanban).is_none());
            assert_eq!(ws.space.terminals().count(), 1);
            assert_eq!(ws.space.focused, Some(kanban));
            assert!(ws.space.focused_terminal().is_none());
        });
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p templum blocks_of_any_kind_share_one_map`
Expected: compile error, `no field blocks`.

- [ ] **Step 3: Replace the two maps in `Space`**

In `workspace.rs`, replace the `Ghost` / `GhostBody` definitions (lines 85-95) with:

```rust
/// A closed block, still drawn while it fades out. A terminal's shell lives until then.
pub(crate) struct Ghost {
    pub block: Block,
    pub body: Body,
    since: Instant,
}

/// What a block holds. Every block on a canvas is one of these.
pub(crate) enum Body {
    Terminal(Pane),
    Kanban(Kanban),
}

impl Body {
    pub fn terminal(&self) -> Option<&Pane> {
        match self {
            Body::Terminal(pane) => Some(pane),
            _ => None,
        }
    }

    pub fn terminal_mut(&mut self) -> Option<&mut Pane> {
        match self {
            Body::Terminal(pane) => Some(pane),
            _ => None,
        }
    }

    pub fn kanban(&self) -> Option<&Kanban> {
        match self {
            Body::Kanban(kanban) => Some(kanban),
            _ => None,
        }
    }

    pub fn kanban_mut(&mut self) -> Option<&mut Kanban> {
        match self {
            Body::Kanban(kanban) => Some(kanban),
            _ => None,
        }
    }

    /// The smallest block of this kind.
    pub fn min_size(&self, cell: Size) -> Size {
        match self {
            Body::Terminal(_) => chrome::min_block_size(cell),
            Body::Kanban(_) => kanban::MIN,
        }
    }
}
```

In the `Space` struct, replace

```rust
    pub terminals: HashMap<BlockId, Pane>,
    /// The kanban blocks: every block is a terminal or one of these.
    pub kanbans: HashMap<BlockId, Kanban>,
```

with

```rust
    /// What each block of the canvas holds. Inserted and removed together with `canvas`'s blocks.
    pub blocks: HashMap<BlockId, Body>,
```

In `impl Space`, replace `layout` with:

```rust
    /// The canvas as a snapshot needs it: rects and process ids, cheap on the UI thread.
    fn layout(&self, agents: Arc<Vec<templum_terminal::Detect>>) -> session::Layout {
        let blocks = self.canvas.blocks();
        session::Layout {
            camera: self.canvas.camera,
            blocks: blocks
                .iter()
                .map(|b| match &self.blocks[&b.id] {
                    Body::Terminal(pane) => {
                        session::LayoutBlock::Terminal(b.rect, pane.term.processes())
                    }
                    Body::Kanban(k) => session::LayoutBlock::Saved(session::Item::Kanban(
                        session::KanbanItem {
                            rect: b.rect.into(),
                            board: k.board,
                        },
                    )),
                })
                .collect(),
            focused: self
                .focused
                .and_then(|id| blocks.iter().position(|b| b.id == id)),
            recent: self.recent.clone(),
            agents,
        }
    }
```

and change `has_agent`'s `session::Item::Kanban(_) => false,` to `_ => false,`. Then add, still in `impl Space`:

```rust
    /// Every terminal block, in no order.
    pub fn terminals(&self) -> impl Iterator<Item = (BlockId, &Pane)> {
        self.blocks
            .iter()
            .filter_map(|(id, body)| Some((*id, body.terminal()?)))
    }

    pub fn terminal(&self, id: BlockId) -> Option<&Pane> {
        self.blocks.get(&id)?.terminal()
    }

    pub fn terminal_mut(&mut self, id: BlockId) -> Option<&mut Pane> {
        self.blocks.get_mut(&id)?.terminal_mut()
    }

    pub fn kanban(&self, id: BlockId) -> Option<&Kanban> {
        self.blocks.get(&id)?.kanban()
    }

    pub fn kanban_mut(&mut self, id: BlockId) -> Option<&mut Kanban> {
        self.blocks.get_mut(&id)?.kanban_mut()
    }

    /// The focused block's terminal, when the focused block is one.
    pub fn focused_terminal(&self) -> Option<&Pane> {
        self.focused.and_then(|id| self.terminal(id))
    }
```

- [ ] **Step 4: Replace every use in `workspace.rs` outside the tests**

Work through these in order (line numbers are from before the edit; use the code to find them):

1. `add_kanban` (line 711) becomes `add_block`:
   ```rust
       /// Adds a block on top, focused, popping in.
       fn add_block(&mut self, rect: Rect, body: Body) -> BlockId {
           let id = self.space.canvas.add(rect);
           self.space.blocks.insert(id, body);
           self.space.focused = Some(id);
           self.space.appearing.insert(id, Instant::now());
           id
       }
   ```
   `add_kanban_at`: `self.add_block(rect, Body::Kanban(Kanban::default()));`.
2. `launch_agent` (line 792) and `restore` (line 1008): `self.space.terminals[&id].term.write(...)` becomes
   `self.space.terminal(id).expect("just spawned").term.write(...)`.
3. `show_board` and `pick_board`: `self.space.kanbans.get_mut(&block)` becomes `self.space.kanban_mut(block)`.
4. `spawn_block` (line 934): `self.space.terminals.insert(id, Pane {...})` becomes
   `self.space.blocks.insert(id, Body::Terminal(Pane { ... }))`. Keep the rest.
5. `restore`, the kanban arm: `self.add_kanban(rect, Kanban {...})` becomes `self.add_block(rect, Body::Kanban(Kanban { board: k.board, picking: false }))`.
6. `close` (line 1206):
   ```rust
           let body = self.space.blocks.remove(&id);
           if let (Some(block), Some(body)) = (block, body) {
               self.space.ghosts.push(Ghost {
                   block,
                   body,
                   since: Instant::now(),
               });
           }
   ```
7. `apply` (line 1230) and `apply_config` (line 1321): `self.spaces().flat_map(|s| s.terminals.values())` becomes `self.spaces().flat_map(|s| s.terminals().map(|(_, p)| p))`.
8. `set_font` (line 1413): the inner loop becomes
   ```rust
           for space in std::iter::once(&mut self.space).chain(&mut self.others) {
               let Space { blocks, canvas, .. } = space;
               for (id, body) in blocks.iter_mut() {
                   if let (Some(pane), Some(block)) = (body.terminal_mut(), canvas.get(*id)) {
                       let content = chrome::content_rect(block.rect).size();
                       let (cols, rows) = chrome::grid_size(content, self.cell.size);
                       pane.term.resize(cols, rows, self.cell.px());
                   }
               }
           }
   ```
9. `hear` (line 1433): the loop becomes
   ```rust
           for (i, space) in spaces.enumerate() {
               let Space {
                   blocks,
                   focused,
                   name,
                   id: space_id,
                   ..
               } = space;
               let terminals = blocks
                   .iter_mut()
                   .filter_map(|(id, body)| Some((*id, body.terminal_mut()?)));
               for (id, pane) in terminals {
                   let status = pane.term.status();
                   let new = alert::news(&status, pane.heard);
                   pane.heard = (status.finished, status.attention);
                   if active && i == 0 && *focused == Some(id) {
                       pane.alert = None;
                   } else if let Some(alert) = new {
                       if pane.alert.is_none() {
                           let mut title = pane.term.title();
                           if several {
                               title = format!("{title} · {name}");
                           }
                           news.push((space_id.clone(), id, title, alert.clone()));
                       }
                       pane.alert = Some(alert);
                   }
               }
           }
   ```
10. `start_dir` (line 1512), `copy_selection` (1913), `paste_clipboard` (2004), `jump_to_prompt` (2077), `on_key_down` (2214), `replace_text_in_range` (2760): every
    `self.space.focused.and_then(|id| self.space.terminals.get(&id))` becomes `self.space.focused_terminal()`.
11. `begin_resize` (line 1850):
    ```rust
            let min = self
                .space
                .blocks
                .get(&id)
                .map_or(chrome::min_block_size(self.cell.size), |b| b.min_size(self.cell.size));
    ```
12. `drag_to` (1865): `self.space.terminals.get_mut(&id)` → `self.space.terminal_mut(id)`; (1873) `self.space.terminals.get(&id)` → `self.space.terminal(id)`.
13. `begin_select` (1897) and `scroll_terminal` (2335): `self.space.terminals.get(&id)` → `self.space.terminal(id)`.
14. `render` (2473-2486):
    ```rust
            let exited: Vec<BlockId> = self
                .space
                .terminals()
                .filter(|(_, pane)| pane.term.exited())
                .map(|(id, _)| id)
                .collect();
            for id in exited {
                self.close(id, cx);
            }
            // Re-arm once per frame: output is coalesced to frame rate.
            for (_, pane) in self.space.terminals() {
                pane.term.rearm();
            }
    ```
    and the blocks (2552-2569):
    ```rust
                    let body = &self.space.blocks[&b.id];
                    block::render(self, b, body, look, true, window, cx)
    ```
    ```rust
            blocks.extend(self.space.ghosts.iter().map(|g| {
                let look = motion::closing(motion::progress(g.since, now, motion::CLOSE));
                block::render(self, g.block, &g.body, look, false, window, cx)
            }));
    ```
15. Remove `use crate::block::{self, Content};` → `use crate::block;`. Remove `GhostBody` everywhere.

- [ ] **Step 5: `block.rs` takes a `&Body`**

Replace the `Content` enum (lines 98-102) by nothing, and import `use crate::workspace::{Body, Pane, Workspace};`. Change the signature:

```rust
pub(crate) fn render(
    ws: &Workspace,
    block: Block,
    body: &Body,
    look: Look,
    live: bool,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
```

Then:
- `let target = ... match &content {...}` becomes `let target = Target::Block(id);`.
- the title: `match body { Body::Terminal(pane) => SharedString::from(pane.term.title()), Body::Kanban(k) => k.title(ws) }`.
- the status dot: `let dot = match body { Body::Terminal(pane) if live => status_dot(ws, pane).map(...), _ => None };`.
- the boards button: `let boards = match body { Body::Kanban(_) if live => Some(...), _ => None };`.
- the body: `let body_el = match body { Body::Terminal(pane) => body_div.when(live, ...).child(grid::body(...)), Body::Kanban(k) => body_div.when(live, |el| el.child(kanban::body(ws, id, *k, z, cx))) }.test_support();` (rename the local `body` div to `body_div` first, so the parameter keeps its name).
- `let hovered`, `focused` are unchanged. Remove `use crate::kanban::{self, Kanban};` → `use crate::kanban;`.

- [ ] **Step 6: `menu.rs` targets a block, whatever it holds**

Replace the `Terminal(BlockId)` and `Kanban(BlockId)` variants of `Target` with one:

```rust
    /// A block, whatever it holds: the menu is built for its kind.
    Block(BlockId),
```

`open_menu`: replace `let items = self.items(&target);` with

```rust
        let Some(items) = self.items(&target) else {
            return;
        };
```

`items` returns `Option<Items>`: wrap every existing arm's value in `Some(...)`, and replace the two block arms with

```rust
            Target::Block(block) => match self.space.blocks.get(&block)? {
                Body::Terminal(pane) => Items::Terminal {
                    block,
                    selection: pane.term.shared().selection_text().is_some(),
                },
                Body::Kanban(k) => Items::Kanban {
                    block,
                    listing: k.shown(self).is_none() && k.board.is_some(),
                },
            },
```

Import `use crate::workspace::{Body, Workspace};`. The `Items` enum and `Items::build` are unchanged.

- [ ] **Step 7: `session.rs` carries whole items for the kinds without processes**

Replace `LayoutBlock`:

```rust
pub(crate) enum LayoutBlock {
    /// A terminal's rect and its processes, read in the background.
    Terminal(Rect, Processes),
    /// Any other kind: its item, complete, built on the UI thread.
    Saved(Item),
}
```

In `Layout::snapshot`, the `processes` filter: `LayoutBlock::Saved(_) => None,`; the items map:

```rust
                    LayoutBlock::Saved(item) => item.clone(),
```

In the test at line 679-681, replace `LayoutBlock::Kanban(Rect::new(5.0, 5.0, 10.0, 10.0), Some(3))` with

```rust
                LayoutBlock::Saved(Item::Kanban(KanbanItem {
                    rect: Rect::new(5.0, 5.0, 10.0, 10.0).into(),
                    board: Some(3),
                })),
```

- [ ] **Step 8: `sidebar.rs` and `bench.rs`**

`sidebar.rs:122`: `space.terminals.values().any(|p| p.alert.is_some())` → `space.terminals().any(|(_, p)| p.alert.is_some())`.
`bench.rs:30`: `for pane in ws.space.terminals.values().take(5)` → `for (_, pane) in ws.space.terminals().take(5)`.

- [ ] **Step 9: Update the tests mechanically**

From the repository root:

```bash
sed -i -E \
  -e 's/\.terminals\[&([a-z_]+)\]/.terminal(\1).unwrap()/g' \
  -e 's/\.terminals\.len\(\)/.terminals().count()/g' \
  -e 's/\.terminals\.is_empty\(\)/.terminals().next().is_none()/g' \
  -e 's/\.terminals\.values\(\)/.terminals().map(|(_, p)| p)/g' \
  -e 's/\.terminals\.contains_key\(&([a-z_]+)\)/.terminal(\1).is_some()/g' \
  -e 's/\.kanbans\[&([a-z_]+)\]/.kanban(\1).unwrap()/g' \
  -e 's/\.kanbans\.contains_key\(&([a-z_.]+)\)/.kanban(\1).is_some()/g' \
  -e 's/\.kanbans\.get\(&b\.id\)\.map\(\|k\| k\.board\)/.kanban(b.id).map(|k| k.board)/g' \
  crates/templum/src/workspace.rs
grep -n '\.terminals\b[^(]\|\.kanbans\b' crates/templum/src/workspace.rs
```

Expected: the `grep` prints nothing. If it prints a line, fix that line by hand with the accessors above.

- [ ] **Step 10: Run every test**

Run: `cargo test --workspace`
Expected: all pass, including `blocks_of_any_kind_share_one_map`.

- [ ] **Step 11: Checks and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add -A crates/templum/src
git commit -m "Hold every block's body in one map, whatever its kind"
```

---

### Task 3: Snapping, pure

**Files:**
- Create: `crates/templum-canvas/src/snap.rs`
- Modify: `crates/templum-canvas/src/gesture.rs`
- Modify: `crates/templum-canvas/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  pub const SNAP_PX: f32 = 8.0;
  pub enum Guide { Vertical { x: f32, y0: f32, y1: f32 }, Horizontal { y: f32, x0: f32, x1: f32 } }
  pub struct Snapped { pub rect: Rect, pub guides: Vec<Guide> }
  pub fn snap_move(rect: Rect, others: &[Rect], threshold: f32) -> Snapped;
  pub fn snap_resize(rect: Rect, others: &[Rect], threshold: f32) -> Snapped;
  #[derive(Default)] pub struct Update { pub resized: Option<(BlockId, Rect)>, pub guides: Vec<Guide> }
  impl Gesture { pub fn update(&mut self, canvas: &mut Canvas, pointer: Point) -> Update; }
  ```
  Guides are in world units.

- [ ] **Step 1: Write the failing tests**

Create `crates/templum-canvas/src/snap.rs` with only the tests:

```rust
//! Snapping: a moved or resized block lines up with its neighbours' edges and centers when it
//! comes within a few screen pixels, with no key held, and lets go as soon as it is farther.

use crate::Rect;

#[cfg(test)]
mod tests {
    use super::*;

    const T: f32 = 8.0;

    fn others() -> Vec<Rect> {
        vec![Rect::new(100.0, 100.0, 200.0, 100.0)]
    }

    #[test]
    fn a_left_edge_near_another_left_edge_snaps_and_gives_a_guide() {
        let s = snap_move(Rect::new(105.0, 300.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect, Rect::new(100.0, 300.0, 50.0, 50.0));
        assert_eq!(
            s.guides,
            [Guide::Vertical {
                x: 100.0,
                y0: 100.0,
                y1: 350.0
            }]
        );
    }

    #[test]
    fn snapping_lets_go_past_the_threshold() {
        let s = snap_move(Rect::new(109.0, 300.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect, Rect::new(109.0, 300.0, 50.0, 50.0));
        assert!(s.guides.is_empty());
    }

    #[test]
    fn right_edges_and_centers_snap_too() {
        // Right edge 303 near the other's right edge 300.
        let s = snap_move(Rect::new(253.0, 300.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect.right(), 300.0);
        // Vertical center 152 near the other's 150 (y 127 + 25).
        let s = snap_move(Rect::new(500.0, 127.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect.y, 125.0);
        assert_eq!(
            s.guides,
            [Guide::Horizontal {
                y: 150.0,
                x0: 100.0,
                x1: 550.0
            }]
        );
    }

    #[test]
    fn each_axis_snaps_on_its_own_and_the_nearest_candidate_wins() {
        // x: left 103 → 100 (3 away) beats center 128 → 200 (far). y: top 203 → 200 (bottom edge).
        let s = snap_move(Rect::new(103.0, 203.0, 50.0, 50.0), &others(), T);
        assert_eq!(s.rect, Rect::new(100.0, 200.0, 50.0, 50.0));
        assert_eq!(s.guides.len(), 2);
        // Two others: the nearer edge wins.
        let two = vec![others()[0], Rect::new(400.0, 400.0, 10.0, 10.0)];
        let s = snap_move(Rect::new(398.0, 600.0, 50.0, 50.0), &two, T);
        assert_eq!(s.rect.x, 400.0);
    }

    #[test]
    fn resizing_snaps_the_far_edges_and_matches_a_width() {
        // Right edge 297 → the other's right edge 300.
        let s = snap_resize(Rect::new(0.0, 300.0, 297.0, 50.0), &others(), T);
        assert_eq!(s.rect.w, 300.0);
        assert_eq!(s.rect.x, 0.0, "resizing never moves the block");
        // Width 203 → the other's width 200.
        let s = snap_resize(Rect::new(500.0, 300.0, 203.0, 50.0), &others(), T);
        assert_eq!(s.rect.w, 200.0);
        // Bottom edge 198 → the other's bottom 200.
        let s = snap_resize(Rect::new(500.0, 100.0, 50.0, 98.0), &others(), T);
        assert_eq!(s.rect.h, 100.0);
    }

    #[test]
    fn nothing_to_snap_to_changes_nothing() {
        let r = Rect::new(1.5, 2.5, 3.0, 4.0);
        assert_eq!(snap_move(r, &[], T).rect, r);
        assert_eq!(snap_resize(r, &[], T).rect, r);
    }
}
```

Add `mod snap;` to `lib.rs` and `pub use snap::{Guide, SNAP_PX, Snapped, snap_move, snap_resize};`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p templum-canvas snap`
Expected: compile errors (`snap_move` not found).

- [ ] **Step 3: Implement**

Above the tests in `snap.rs`:

```rust
/// How close, in screen pixels, an edge must come to snap. The caller divides by the zoom.
pub const SNAP_PX: f32 = 8.0;

/// A line drawn while a snap is in effect, world units, along the aligned edge or center.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Guide {
    Vertical { x: f32, y0: f32, y1: f32 },
    Horizontal { y: f32, x0: f32, x1: f32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapped {
    pub rect: Rect,
    pub guides: Vec<Guide>,
}

/// A block being moved: its left, center and right line up with the others' left, center and
/// right (and the same vertically). Each axis takes its nearest candidate within `threshold`.
pub fn snap_move(rect: Rect, others: &[Rect], threshold: f32) -> Snapped {
    let xs = [rect.x, rect.x + rect.w / 2.0, rect.right()];
    let ys = [rect.y, rect.y + rect.h / 2.0, rect.bottom()];
    let x = nearest(&xs, others, |o| [o.x, o.x + o.w / 2.0, o.right()], threshold);
    let y = nearest(&ys, others, |o| [o.y, o.y + o.h / 2.0, o.bottom()], threshold);
    let mut snapped = rect;
    if let Some(hit) = &x {
        snapped.x += hit.delta;
    }
    if let Some(hit) = &y {
        snapped.y += hit.delta;
    }
    Snapped {
        rect: snapped,
        guides: guides(snapped, x, y),
    }
}

/// A block being resized by its bottom-right corner: its right and bottom edges line up with the
/// others' edges, or its width or height matches another block's. It never moves.
pub fn snap_resize(rect: Rect, others: &[Rect], threshold: f32) -> Snapped {
    let x = nearest(
        &[rect.right()],
        others,
        |o| [o.x, o.right(), rect.x + o.w],
        threshold,
    );
    let y = nearest(
        &[rect.bottom()],
        others,
        |o| [o.y, o.bottom(), rect.y + o.h],
        threshold,
    );
    let mut snapped = rect;
    if let Some(hit) = &x {
        snapped.w += hit.delta;
    }
    if let Some(hit) = &y {
        snapped.h += hit.delta;
    }
    Snapped {
        rect: snapped,
        guides: guides(snapped, x, y),
    }
}

/// One axis's snap: how far to move, the line landed on, and whose it is.
struct Hit {
    delta: f32,
    at: f32,
    other: Rect,
}

/// The nearest (mine, theirs) pair within `threshold`, over every other block.
fn nearest(
    mine: &[f32],
    others: &[Rect],
    theirs: impl Fn(&Rect) -> [f32; 3],
    threshold: f32,
) -> Option<Hit> {
    let mut best: Option<Hit> = None;
    for other in others {
        for at in theirs(other) {
            for m in mine {
                let delta = at - m;
                if delta.abs() <= threshold
                    && best.as_ref().is_none_or(|b| delta.abs() < b.delta.abs())
                {
                    best = Some(Hit {
                        delta,
                        at,
                        other: *other,
                    });
                }
            }
        }
    }
    best
}

/// The guides for the snapped rect: along the line, from the nearest end of the two blocks to
/// the farthest.
fn guides(rect: Rect, x: Option<Hit>, y: Option<Hit>) -> Vec<Guide> {
    let mut guides = Vec::new();
    if let Some(hit) = x {
        guides.push(Guide::Vertical {
            x: hit.at,
            y0: rect.y.min(hit.other.y),
            y1: rect.bottom().max(hit.other.bottom()),
        });
    }
    if let Some(hit) = y {
        guides.push(Guide::Horizontal {
            y: hit.at,
            x0: rect.x.min(hit.other.x),
            x1: rect.right().max(hit.other.right()),
        });
    }
    guides
}
```

- [ ] **Step 4: Run the snap tests**

Run: `cargo test -p templum-canvas snap`
Expected: 6 passed. If `is_none_or` is reported missing, the toolchain isn't the pinned 1.98: run `rustup show`.

- [ ] **Step 5: Write the failing gesture tests**

In `gesture.rs`'s `mod tests`, add:

```rust
    #[test]
    fn a_moved_block_snaps_to_a_neighbour_and_reports_guides() {
        let mut c = Canvas::default();
        c.add(Rect::new(100.0, 100.0, 200.0, 100.0));
        let id = c.add(Rect::new(500.0, 500.0, 50.0, 50.0));
        let mut g = Gesture::move_block(&c, id, Point::new(500.0, 500.0)).unwrap();
        // Dragged so its left edge would be at 105: within 8 px of 100.
        let u = g.update(&mut c, Point::new(105.0, 500.0));
        assert_eq!(u.resized, None);
        assert_eq!(c.get(id).unwrap().rect.x, 100.0);
        assert_eq!(u.guides.len(), 1);
        // Farther: it follows the pointer again, and the guide is gone.
        let u = g.update(&mut c, Point::new(120.0, 500.0));
        assert_eq!(c.get(id).unwrap().rect.x, 120.0);
        assert!(u.guides.is_empty());
    }

    #[test]
    fn the_threshold_follows_the_zoom() {
        let mut c = Canvas::default();
        c.add(Rect::new(100.0, 100.0, 200.0, 100.0));
        let id = c.add(Rect::new(500.0, 500.0, 50.0, 50.0));
        c.camera.zoom_step(5, Point::new(0.0, 0.0)); // zoom 2: 8 px is 4 world units
        let grab = c.camera.to_screen(Point::new(500.0, 500.0));
        let mut g = Gesture::move_block(&c, id, grab).unwrap();
        // Left edge at 105 world units: 5 away, more than 4.
        g.update(&mut c, c.camera.to_screen(Point::new(105.0, 500.0)));
        assert_eq!(c.get(id).unwrap().rect.x, 105.0);
        // At 103: 3 away, snaps.
        g.update(&mut c, c.camera.to_screen(Point::new(103.0, 500.0)));
        assert_eq!(c.get(id).unwrap().rect.x, 100.0);
    }

    #[test]
    fn a_resized_block_snaps_its_width_and_reports_the_rect() {
        let mut c = Canvas::default();
        c.add(Rect::new(100.0, 100.0, 200.0, 100.0));
        let id = c.add(Rect::new(500.0, 500.0, 50.0, 50.0));
        let min = Size::new(10.0, 10.0);
        let mut g = Gesture::resize_block(&c, id, Point::new(550.0, 550.0), min).unwrap();
        // Width would be 203: snaps to 200.
        let u = g.update(&mut c, Point::new(703.0, 550.0));
        assert_eq!(u.resized, Some((id, Rect::new(500.0, 500.0, 200.0, 50.0))));
        assert_eq!(u.guides.len(), 1);
    }
```

Then update the existing tests to the new return type: in `pan_moves_the_camera_with_the_pointer`, `assert_eq!(g.update(...), None)` becomes `assert_eq!(g.update(...).resized, None)`; in `resize_reports_the_new_rect_and_clamps_to_minimum` both `assert_eq!(g.update(...), Some(...))` become `assert_eq!(g.update(...).resized, Some(...))`; in `gesture_on_a_removed_block_is_inert` both become `.resized`.

- [ ] **Step 6: Run them to see them fail**

Run: `cargo test -p templum-canvas gesture`
Expected: compile errors (`resized` not found on `Option`).

- [ ] **Step 7: Snap in `Gesture::update`**

In `gesture.rs`, add after the imports:

```rust
use crate::snap::{Guide, SNAP_PX, snap_move, snap_resize};

/// What a pointer move did: the new rect of a resized block (to resize its PTY), and the guides
/// of the snaps in effect.
#[derive(Debug, Default, PartialEq)]
pub struct Update {
    pub resized: Option<(BlockId, Rect)>,
    pub guides: Vec<Guide>,
}
```

Replace `update`:

```rust
    /// Applies a pointer move. A moved or resized block snaps to the others' edges and centers
    /// within [`SNAP_PX`] screen pixels; the rect is recomputed from the pointer every time, so
    /// a snap lets go as soon as the pointer is farther. A gesture whose block is gone does nothing.
    pub fn update(&mut self, canvas: &mut Canvas, pointer: Point) -> Update {
        match self {
            Gesture::Pan { last } => {
                canvas
                    .camera
                    .pan_by(Point::new(pointer.x - last.x, pointer.y - last.y));
                *last = pointer;
                Update::default()
            }
            Gesture::Move { id, grab } => {
                let w = canvas.camera.to_world(pointer);
                let others = others(canvas, *id);
                let threshold = SNAP_PX / canvas.camera.zoom();
                let Some(b) = canvas.get_mut(*id) else {
                    return Update::default();
                };
                let raw = Rect::new(w.x - grab.x, w.y - grab.y, b.rect.w, b.rect.h);
                let snapped = snap_move(raw, &others, threshold);
                b.rect = snapped.rect;
                Update {
                    resized: None,
                    guides: snapped.guides,
                }
            }
            Gesture::Resize { id, grab, min } => {
                let w = canvas.camera.to_world(pointer);
                let others = others(canvas, *id);
                let threshold = SNAP_PX / canvas.camera.zoom();
                let Some(b) = canvas.get_mut(*id) else {
                    return Update::default();
                };
                let raw = Rect::new(
                    b.rect.x,
                    b.rect.y,
                    (w.x + grab.x - b.rect.x).max(min.w),
                    (w.y + grab.y - b.rect.y).max(min.h),
                );
                let snapped = snap_resize(raw, &others, threshold);
                b.rect = Rect::new(
                    raw.x,
                    raw.y,
                    snapped.rect.w.max(min.w),
                    snapped.rect.h.max(min.h),
                );
                Update {
                    resized: Some((*id, b.rect)),
                    guides: snapped.guides,
                }
            }
        }
    }
```

and add, after the `impl`:

```rust
/// Every block's rect but `id`'s: what it can snap to.
fn others(canvas: &Canvas, id: BlockId) -> Vec<Rect> {
    canvas
        .blocks()
        .iter()
        .filter(|b| b.id != id)
        .map(|b| b.rect)
        .collect()
}
```

In `lib.rs`: `pub use gesture::{Gesture, Update};`.

- [ ] **Step 8: Run the crate's tests**

Run: `cargo test -p templum-canvas`
Expected: all pass.

- [ ] **Step 9: Fix the app's one caller and commit**

`workspace.rs` `drag_to`: `if let Some((id, rect)) = gesture.update(&mut self.space.canvas, p)` becomes

```rust
                let update = gesture.update(&mut self.space.canvas, p);
                if let Some((id, rect)) = update.resized {
```

(The guides are used in Task 4.) Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/templum-canvas crates/templum/src/workspace.rs
git commit -m "Snap a moved or resized block to its neighbours' edges and centers"
```

---

### Task 4: Paint the guides

**Files:**
- Modify: `crates/templum/src/workspace.rs` (`Space`, `drag_to`, `end_drag`, `release`, `switch_to`, `paint_dots`'s neighbourhood in `render`)

**Interfaces:**
- Produces: `Space::guides: Vec<Guide>`, set by `drag_to`, cleared by `end_drag`; painted by `paint_guides(camera, color, bounds, &guides, window)`.

- [ ] **Step 1: Write the failing headless test**

In `workspace.rs` `mod tests`, after `dragging_the_title_moves_the_block`:

```rust
    #[gpui_kit::test]
    fn a_dragged_block_snaps_to_a_neighbour_and_shows_a_guide_until_release(
        cx: &mut TestAppContext,
    ) {
        let (handle, ws) = open(cx);
        let first = first_id(&ws, cx);
        // A second block well away, same size.
        let second = cx
            .update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| {
                    let r = ws.space.canvas.get(first).unwrap().rect;
                    ws.open(
                        templum_canvas::Rect::new(r.x + 400.0, r.y + 300.0, r.w, r.h),
                        window,
                        cx,
                    );
                    ws.space.appearing.clear();
                    ws.space.canvas.top().unwrap()
                })
            })
            .unwrap();
        let (first_rect, second_rect) = ws.read_with(cx, |ws, _| {
            (
                ws.space.canvas.get(first).unwrap().rect,
                ws.space.canvas.get(second).unwrap().rect,
            )
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            let from = window.find(("title", second.0 as usize)).bounds().center();
            // Move left so the second's left edge lands 5 px right of the first's: snaps to it.
            let dx = first_rect.x - second_rect.x + 5.0;
            let to = point(from.x + px(dx), from.y);
            let modifiers = Modifiers::default();
            window.dispatch_event(
                PlatformInput::MouseDown(MouseDownEvent {
                    button: MouseButton::Left,
                    position: from,
                    modifiers,
                    click_count: 1,
                    first_mouse: false,
                }),
                cx,
            );
            window.dispatch_event(
                PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                    position: to,
                    pressed_button: Some(MouseButton::Left),
                    modifiers,
                }),
                cx,
            );
            let ws_now = ws.read(cx);
            assert_eq!(ws_now.space.canvas.get(second).unwrap().rect.x, first_rect.x);
            assert_eq!(ws_now.space.guides.len(), 1, "a vertical guide");
            window.dispatch_event(
                PlatformInput::MouseUp(MouseUpEvent {
                    button: MouseButton::Left,
                    position: to,
                    modifiers,
                    click_count: 1,
                }),
                cx,
            );
            assert!(ws.read(cx).space.guides.is_empty(), "gone on release");
        })
        .unwrap();
    }
```

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p templum a_dragged_block_snaps`
Expected: compile error, `no field guides`.

- [ ] **Step 3: Keep and paint the guides**

In `Space`, add after `hovered`:

```rust
    /// The snaps in effect during a drag, drawn as lines; empty otherwise.
    pub guides: Vec<Guide>,
```

Import `Guide` from `templum_canvas` (add it to the existing `use templum_canvas::{...}` list). In `drag_to`'s `Drag::Canvas` arm, after computing `update`:

```rust
                self.space.guides = update.guides;
```

In `end_drag`: add `self.space.guides.clear();`. In `switch_to`, next to `left.hovered = None;`: `left.guides.clear();`.

Add the painter next to `paint_dots`:

```rust
/// The snap guides: 1 px lines in the accent color, world units to screen.
fn paint_guides(
    camera: &Camera,
    color: Hsla,
    bounds: Bounds<Pixels>,
    guides: &[Guide],
    window: &mut Window,
) {
    for guide in guides {
        let (a, b) = match *guide {
            Guide::Vertical { x, y0, y1 } => (Point::new(x, y0), Point::new(x, y1)),
            Guide::Horizontal { y, x0, x1 } => (Point::new(x0, y), Point::new(x1, y)),
        };
        let (a, b) = (camera.to_screen(a), camera.to_screen(b));
        let rect = Bounds::new(
            point(bounds.origin.x + px(a.x), bounds.origin.y + px(a.y)),
            size(px((b.x - a.x).max(1.0)), px((b.y - a.y).max(1.0))),
        );
        window.paint_quad(fill(rect, color));
    }
}
```

In `render`, where the layer is built: before `let layer = canvas(...)`, add `let guides = self.space.guides.clone(); let accent = self.ui.accent;` and inside the paint closure, right after `paint_dots(&camera, dot, bounds, window);`:

```rust
                paint_guides(&camera, accent, bounds, &guides, window);
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p templum a_dragged_block_snaps`
Expected: PASS.

- [ ] **Step 5: Bench, checks, commit**

Run: `cargo run --release -p templum -- --bench` and keep the JSON line.
Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/templum/src/workspace.rs
git commit -m "Draw a guide along the edge a dragged block snapped to" -m "<bench JSON line>"
```

---

### Task 5: The note block

**Files:**
- Create: `crates/templum/src/note.rs`
- Modify: `crates/templum/src/main.rs` (`mod note;`)
- Modify: `crates/templum/src/workspace.rs` (`Body::Note`, `min_size`, `layout`, `restore`, `focus_block`, `new_note`, `add_note_at`, `toggle_preview`, `save_canvas_now`, `snapshot`, `on_release`/`on_app_quit`)
- Modify: `crates/templum/src/block.rs` (title, preview button, body)
- Modify: `crates/templum/src/menu.rs` (`Items::Note`, the canvas's **Add ▸ Note**)
- Modify: `crates/templum/src/session.rs` (`Item::Note`, `NoteItem`)
- Modify: `crates/templum/src/keys.rs` (`Command::NewNote`)

**Interfaces:**
- Consumes: `Body`, `add_block`, `LayoutBlock::Saved` (Task 2); `EditorState` with the `markdown` grammar (Task 1).
- Produces:
  ```rust
  // note.rs
  pub(crate) const SIZE: Size;   // 360 × 280
  pub(crate) const MIN: Size;    // 200 × 140
  pub(crate) struct Note { pub editor: Entity<EditorState>, pub preview: bool }
  impl Note {
      pub fn new(text: &str, preview: bool, window: &mut Window, cx: &mut App) -> Note;
      pub fn text(&self, cx: &App) -> String;
      pub fn title(&self, cx: &App) -> SharedString;
      pub fn focus(&self, window: &mut Window, cx: &mut App);
  }
  pub(crate) fn title(text: &str) -> String;
  pub(crate) fn body(ws: &Workspace, id: BlockId, note: &Note, z: f32, cx: &mut Context<Workspace>) -> AnyElement;
  // workspace.rs
  impl Workspace {
      pub(crate) fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn add_note_at(&mut self, at: Point, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn toggle_preview(&mut self, id: BlockId, cx: &mut Context<Self>);
  }
  // session.rs
  pub(crate) struct NoteItem { pub rect: SavedRect, pub text: String, pub preview: bool }  // Item::Note
  // keys.rs
  Command::NewNote  // "new_note", "New note", ctrl-shift-m / cmd-shift-m
  ```
- Note: `Space::layout` and `save_canvas_now` gain a `cx: &App` parameter (the note's text is read from its editor).

- [ ] **Step 1: Write the failing unit tests**

Create `crates/templum/src/note.rs`:

```rust
//! A note block: Markdown text, edited in gpui-kit's editor (tree-sitter highlights it) or shown
//! rendered. Its title is its first heading. See docs/specs/cockpit.md, section 5.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_is_the_first_heading_or_note() {
        assert_eq!(title("# Plan\n- x"), "Plan");
        assert_eq!(title("text\n\n## Second  \n"), "Second");
        assert_eq!(title("  ###   Spaced   "), "Spaced");
        assert_eq!(title("#NoSpace\n# Real"), "Real", "a heading needs a space");
        assert_eq!(title("#\n# "), "Note", "empty headings don't count");
        assert_eq!(title("no heading"), "Note");
        assert_eq!(title(""), "Note");
    }
}
```

In `main.rs`, add `mod note;` between `mod motion;` and `mod prompt;`.

- [ ] **Step 2: Run it to see it fail**

Run: `cargo test -p templum note::tests`
Expected: compile error, `title` not found.

- [ ] **Step 3: Write `note.rs`**

Above the tests:

```rust
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::text::TextView;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, ScrollWheelEvent, SharedString, TestSupportExt, Window, div,
    px, relative,
};
use templum_canvas::{BlockId, Size};

use crate::workspace::Workspace;

/// A new note's size, and the smallest one (world units).
pub(crate) const SIZE: Size = Size { w: 360.0, h: 280.0 };
pub(crate) const MIN: Size = Size { w: 200.0, h: 140.0 };
/// The text's size at zoom 1, world units.
const FONT: f32 = 13.0;

/// A note block: its text lives in the editor; `preview` shows it rendered instead.
pub(crate) struct Note {
    pub editor: Entity<EditorState>,
    pub preview: bool,
}

impl Note {
    pub fn new(text: &str, preview: bool, window: &mut Window, cx: &mut App) -> Note {
        let text = text.to_string();
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .language("markdown")
                .soft_wrap(true)
                .line_number(false)
                .default_value(text)
        });
        Note { editor, preview }
    }

    pub fn text(&self, cx: &App) -> String {
        self.editor.read(cx).value().to_string()
    }

    pub fn title(&self, cx: &App) -> SharedString {
        title(&self.text(cx)).into()
    }

    /// Typing goes to the editor.
    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        self.editor.update(cx, |editor, cx| editor.focus(window, cx));
    }
}

/// The first heading (`#` to `######`, a space, text), without its hashes; "Note" without one.
pub(crate) fn title(text: &str) -> String {
    text.lines()
        .find_map(|line| {
            let line = line.trim_start();
            let rest = line.trim_start_matches('#');
            let hashes = line.len() - rest.len();
            let heading = (1..=6).contains(&hashes) && rest.starts_with(' ');
            heading.then(|| rest.trim().to_string())
        })
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Note".to_string())
}

/// The body below the header: the editor, or the rendered text. Both at the block's zoom `z`.
pub(crate) fn body(
    ws: &Workspace,
    id: BlockId,
    note: &Note,
    z: f32,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let n = id.0 as usize;
    let inner = if note.preview {
        div()
            .id(("note-preview", n))
            .size_full()
            .overflow_y_scroll()
            .px(px(6.0 * z))
            .child(TextView::markdown(("note-markdown", n), SharedString::from(note.text(cx))))
            .test_support()
            .into_any_element()
    } else {
        div()
            .id(("note-editor", n))
            .size_full()
            .font_family(ws.cell.font.family.clone())
            .child(
                Editor::new(&note.editor)
                    .appearance(false)
                    .bordered(false)
                    .h(relative(1.0)),
            )
            .test_support()
            .into_any_element()
    };
    // The wheel scrolls the note and stops there; with Ctrl it zooms the canvas.
    div()
        .size_full()
        .text_size(px(FONT * z))
        .on_scroll_wheel(|e: &ScrollWheelEvent, _, cx| {
            if !e.modifiers.control {
                cx.stop_propagation();
            }
        })
        .child(inner)
        .into_any_element()
}
```

If `gpui_kit::component::text::TextView` doesn't resolve, use `gpui_kit::base::text::TextView` (same type; `gpui-component/src/text/mod.rs` re-exports from `gpui_base::text`).

- [ ] **Step 4: Run the unit test**

Run: `cargo test -p templum note::tests`
Expected: PASS.

- [ ] **Step 5: Write the failing session test**

In `session.rs` `mod tests`, add:

```rust
    #[test]
    fn a_note_item_reads_back_and_preview_defaults_to_false() {
        let d = tempfile::tempdir().unwrap();
        let mut saved = sample();
        saved.items.push(Item::Note(NoteItem {
            rect: SavedRect {
                x: 1.0,
                y: 2.0,
                w: 360.0,
                h: 280.0,
            },
            text: "# Hi\n- one".into(),
            preview: true,
        }));
        save(d.path(), "n", &saved, 1).unwrap();
        assert_eq!(load(d.path(), "n"), Loaded::Saved(saved));
        let text = r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[
            {"kind":"note","rect":{"x":0,"y":0,"w":300,"h":200},"text":"t"}]}"#;
        std::fs::write(path(d.path(), "m"), text).unwrap();
        let Loaded::Saved(read) = load(d.path(), "m") else {
            panic!("not read");
        };
        assert!(matches!(&read.items[0], Item::Note(n) if n.text == "t" && !n.preview));
    }
```

- [ ] **Step 6: Run it to see it fail**

Run: `cargo test -p templum a_note_item_reads_back`
Expected: compile error, no variant `Note`.

- [ ] **Step 7: Add `Item::Note`**

In `session.rs`, add the variant and its struct:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub(crate) enum Item {
    Terminal(TerminalItem),
    Kanban(KanbanItem),
    Note(NoteItem),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct NoteItem {
    pub rect: SavedRect,
    /// The Markdown text.
    #[serde(default)]
    pub text: String,
    /// Shown rendered instead of the editor.
    #[serde(default)]
    pub preview: bool,
}
```

Run: `cargo test -p templum a_note_item_reads_back` → PASS. (Unknown kinds are still set aside: the existing `version: 2 and kind: "note" do the same` test in `session.rs` must be changed to use `"kind": "sticker"` instead of `"note"`, since `note` is known now; find it with `grep -n '"note"' crates/templum/src/session.rs`.)

- [ ] **Step 8: Write the failing workspace tests**

In `workspace.rs` `mod tests`, add (with the `cfg` constants next to `NEW`):

```rust
    #[cfg(target_os = "macos")]
    const NEW_NOTE: &str = "cmd-shift-m";
    #[cfg(not(target_os = "macos"))]
    const NEW_NOTE: &str = "ctrl-shift-m";

    fn top_note(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> BlockId {
        ws.read_with(cx, |ws, _| {
            let id = ws.space.canvas.top().unwrap();
            assert!(
                matches!(ws.space.blocks.get(&id), Some(super::Body::Note(_))),
                "the top block is a note"
            );
            id
        })
    }

    #[gpui_kit::test]
    fn new_note_adds_a_focused_note_whose_heading_titles_it(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW_NOTE, cx);
            window.render_frame(cx);
        })
        .unwrap();
        let id = top_note(&ws, cx);
        cx.update_window(handle, |_, window, cx| {
            let ws_now = ws.read(cx);
            assert_eq!(ws_now.space.focused, Some(id));
            let rect = ws_now.space.canvas.get(id).unwrap().rect;
            assert_eq!((rect.w, rect.h), (crate::note::SIZE.w, crate::note::SIZE.h));
            let super::Body::Note(note) = &ws_now.space.blocks[&id] else {
                unreachable!()
            };
            assert!(
                note.editor.read(cx).focus_handle(cx).is_focused(window),
                "the editor has the keyboard"
            );
        })
        .unwrap();
        // Typed text reaches the editor.
        cx.simulate_input(handle, "hi");
        ws.read_with(cx, |ws, cx| {
            let super::Body::Note(note) = &ws.space.blocks[&id] else {
                unreachable!()
            };
            assert_eq!(note.text(cx), "hi");
            assert_eq!(note.title(cx).as_ref(), "Note");
        });
        // A heading becomes the title.
        cx.update_window(handle, |_, window, cx| {
            let super::Body::Note(note) = &ws.read(cx).space.blocks[&id] else {
                unreachable!()
            };
            let editor = note.editor.clone();
            editor.update(cx, |e, cx| e.set_value("# Hello\nhi", window, cx));
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, cx| {
            let super::Body::Note(note) = &ws.space.blocks[&id] else {
                unreachable!()
            };
            assert_eq!(note.title(cx).as_ref(), "Hello");
        });
    }
```

`focus_handle(cx)` is `gpui_kit::Focusable`'s method (implemented for every input state in `gpui-base/src/input/base/state.rs`): add `use gpui_kit::Focusable as _;` to the tests module's imports. `FocusHandle::is_focused(&self, window: &Window)` is in `gpui-pre-0.3.8/src/window.rs`. `ElementSnapshot` exposes no text, which is why the title is read from the note.

Then:

```rust
    #[gpui_kit::test]
    fn shortcuts_run_from_inside_a_note(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW_NOTE, cx);
            window.render_frame(cx);
            window.press(NEW, cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.blocks.len(), 3, "terminal, note, terminal");
        });
    }

    #[gpui_kit::test]
    fn clicking_a_note_focuses_its_editor(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW_NOTE, cx);
            window.render_frame(cx);
        })
        .unwrap();
        let id = top_note(&ws, cx);
        let terminal = ws.read_with(cx, |ws, _| ws.space.terminals().next().unwrap().0);
        cx.update_window(handle, |_, window, cx| {
            // Focus the terminal, then click inside the note's body.
            ws.update(cx, |ws, cx| ws.focus_block(terminal, window, cx));
            window.render_frame(cx);
            let body = window.find(("note-editor", id.0 as usize)).bounds();
            click(
                window,
                cx,
                (
                    f32::from(body.origin.x) + 20.0,
                    f32::from(body.origin.y) + 20.0,
                ),
                1,
            );
            window.render_frame(cx);
            let ws_now = ws.read(cx);
            assert_eq!(ws_now.space.focused, Some(id));
            let super::Body::Note(note) = &ws_now.space.blocks[&id] else {
                unreachable!()
            };
            assert!(
                note.editor.read(cx).focus_handle(cx).is_focused(window),
                "the editor keeps the keyboard"
            );
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn the_preview_button_shows_the_rendered_note(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW_NOTE, cx);
            window.render_frame(cx);
        })
        .unwrap();
        let id = top_note(&ws, cx);
        let n = id.0 as usize;
        cx.update_window(handle, |_, window, cx| {
            window.hover(("block", n), cx);
            window.render_frame(cx);
            window.click(("preview", n), cx);
            window.render_frame(cx);
            window.find(("note-preview", n));
            let super::Body::Note(note) = &ws.read(cx).space.blocks[&id] else {
                unreachable!()
            };
            assert!(note.preview);
        })
        .unwrap();
    }

    #[gpui_kit::test]
    fn a_note_comes_back_with_its_text_and_mode(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let (handle, ws) = open_in(cx, None, Some(dir.path().to_path_buf()));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.press(NEW_NOTE, cx);
            window.render_frame(cx);
            ws.update(cx, |ws, cx| {
                let id = ws.space.canvas.top().unwrap();
                let super::Body::Note(note) = &ws.space.blocks[&id] else {
                    unreachable!()
                };
                note.editor
                    .update(cx, |e, cx| e.set_value("# Keep me\n- x", window, cx));
                ws.toggle_preview(id, cx);
                ws.save_canvas_now(cx);
            });
        })
        .unwrap();
        // As the_canvas_comes_back_as_it_was_left does: a second workspace on the same folder.
        let (_, again) = open_in(cx, None, Some(dir.path().to_path_buf()));
        again.read_with(cx, |ws, cx| {
            let notes: Vec<_> = ws
                .space
                .blocks
                .values()
                .filter_map(|b| match b {
                    super::Body::Note(n) => Some((n.text(cx), n.preview)),
                    _ => None,
                })
                .collect();
            assert_eq!(notes, [("# Keep me\n- x".to_string(), true)]);
        });
    }
```

Note that `save_canvas_now` gains a `cx: &App` parameter in Step 11, so the existing tests that call `ws.save_canvas_now()` (search: `grep -n 'save_canvas_now()' crates/templum/src/workspace.rs`) become `ws.save_canvas_now(cx)`.

- [ ] **Step 9: Run them to see them fail**

Run: `cargo test -p templum note`
Expected: compile errors (`Body::Note`, `toggle_preview`, `NEW_NOTE` has no command…).

- [ ] **Step 10: The command**

In `keys.rs`: add `NewNote` to `Command` after `NewKanban`; `ALL` becomes `[Command; 20]` with `Command::NewNote` after `NewKanban`; `name` → `"new_note"`; `title` → `"New note"`; `defaults`: `(Command::NewNote, false) => &["ctrl-shift-m"]`, `(Command::NewNote, true) => &["cmd-shift-m"]`; `KeysConfig` gets `pub new_note: Option<Keys>,` with its `get` and `set` arms. In `workspace.rs` `run`: `Command::NewNote => self.new_note(window, cx),`.

- [ ] **Step 11: The body kind**

In `workspace.rs`:

- `use crate::note::{self, Note};`
- `Body` gets `Note(Note)`; `min_size`: `Body::Note(_) => note::MIN,`; add
  ```rust
      pub fn note(&self) -> Option<&Note> {
          match self {
              Body::Note(note) => Some(note),
              _ => None,
          }
      }
  ```
- `Space::layout(&self, agents, cx: &App)`: add the arm
  ```rust
                      Body::Note(n) => session::LayoutBlock::Saved(session::Item::Note(
                          session::NoteItem {
                              rect: b.rect.into(),
                              text: n.text(cx),
                              preview: n.preview,
                          },
                      )),
  ```
  and pass `cx` at both call sites: in `snapshot`, `s.layout(known.clone(), cx)`; `save_canvas_now(&mut self, cx: &App)` with `space.layout(known.clone(), cx)`. The two closures in `with` become `cx.on_release(|ws, cx| { ws.save(); ws.save_canvas_now(cx); })` and `cx.on_app_quit(|ws, cx| { ws.save(); ws.save_canvas_now(cx); async {} })`.
- `restore`: a new arm before the terminal fallthrough:
  ```rust
                  session::Item::Note(n) => {
                      let id = Self::note_rect(n.rect.into()).map(|rect| {
                          let note = Note::new(&n.text, n.preview, window, cx);
                          self.add_block(rect, Body::Note(note))
                      });
                      ids.push(id);
                      continue;
                  }
  ```
  with, next to `kanban_rect`:
  ```rust
      /// A saved note's rect made safe, like a kanban's.
      fn note_rect(r: Rect) -> Option<Rect> {
          [r.x, r.y, r.w, r.h].iter().all(|v| v.is_finite()).then(|| {
              Rect::new(
                  r.x,
                  r.y,
                  r.w.clamp(note::MIN.w, 20_000.0),
                  r.h.clamp(note::MIN.h, 20_000.0),
              )
          })
      }
  ```
- New methods, after `add_kanban_at`:
  ```rust
      /// A note at the viewport's center, focused, in edit mode.
      pub(crate) fn new_note(&mut self, window: &mut Window, cx: &mut Context<Self>) {
          let center = self.space.canvas.camera.to_world(self.center(window));
          let size = note::SIZE;
          let at = self
              .space
              .canvas
              .place_new(
                  size,
                  Point::new(center.x - size.w / 2.0, center.y - size.h / 2.0),
              )
              .origin();
          self.add_note_at(at, window, cx);
      }

      /// An empty note, its top-left corner at `at` (world units), with the keyboard.
      pub(crate) fn add_note_at(&mut self, at: Point, window: &mut Window, cx: &mut Context<Self>) {
          self.space.pristine = false;
          let rect = Rect::new(at.x, at.y, note::SIZE.w, note::SIZE.h);
          let note = Note::new("", false, window, cx);
          note.focus(window, cx);
          self.add_block(rect, Body::Note(note));
          cx.notify();
      }

      /// A note shows its rendered text, or its editor again.
      pub(crate) fn toggle_preview(&mut self, id: BlockId, cx: &mut Context<Self>) {
          if let Some(Body::Note(note)) = self.space.blocks.get_mut(&id) {
              note.preview = !note.preview;
              cx.notify();
          }
      }
  ```
- `focus_block`: the editor gets the keyboard for a note:
  ```rust
      pub(crate) fn focus_block(&mut self, id: BlockId, window: &mut Window, cx: &mut Context<Self>) {
          self.space.pristine = false;
          self.space.canvas.raise(id);
          self.space.focused = Some(id);
          match self.space.blocks.get(&id) {
              Some(Body::Note(note)) if !note.preview => note.focus(window, cx),
              _ => window.focus(&self.focus_handle, cx),
          }
          cx.notify();
      }
  ```
- `block.rs`: the title arm `Body::Note(n) => n.title(cx)`; the body arm
  `Body::Note(n) => body_div.when(live, |el| el.child(note::body(ws, id, n, z, cx)))`; and a preview button built like the kanban's `boards` button, with id `("preview", n)`, icon `IconName::Eye` (editing) or `IconName::EyeOff` (previewing), `on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.toggle_preview(id, cx)))`, added to `header` with `.children(preview)`. Import `use crate::note;`.
- `menu.rs`: `Items::Note { block: BlockId, preview: bool }`; in `items`: `Body::Note(n) => Items::Note { block, preview: n.preview }`; its `build` arm:
  ```rust
              Items::Note { block, preview } => menu
                  .item(item(
                      ws,
                      if preview { "Edit" } else { "Preview" },
                      Some(if preview { IconName::EyeOff } else { IconName::Eye }),
                      move |ws, _, cx| ws.toggle_preview(block, cx),
                  ))
                  .item(item(ws, "Copy all", Some(IconName::Copy), move |ws, _, cx| {
                      if let Some(note) = ws.space.blocks.get(&block).and_then(|b| b.note()) {
                          let text = note.text(cx);
                          cx.write_to_clipboard(gpui_kit::ClipboardItem::new_string(text));
                      }
                  }))
                  .separator()
                  .item(item(
                      ws,
                      "Close",
                      Some(IconName::Close),
                      move |ws, _, cx| ws.close(block, cx),
                  )),
  ```
  and in the canvas's **Add ▸** submenu, after "Kanban board":
  ```rust
                          .item(item(
                              ws,
                              "Note",
                              Some(IconName::StickyNote),
                              move |ws, window, cx| ws.add_note_at(at, window, cx),
                          ))
  ```

- [ ] **Step 12: Run the note tests**

Run: `cargo test -p templum note`
Expected: all pass. If `new_note_adds_a_focused_note…` fails on the zoomed text, don't change the test: check `text_size` is on the wrapper div (gpui-kit's editor takes its line height from the inherited text style, see its own `line_height` test in `gpui-component/src/input/editor.rs`).

- [ ] **Step 13: Check it by hand**

Run: `cargo run -p templum`. Ctrl+Shift+M; type a heading and a list; Tab indents a list item; zoom to 50% and 200% (text scales, stays sharp); the eye button renders it; Ctrl+Shift+T from inside the note opens a terminal; quit and reopen: the note is back. Note anything odd in the commit message.

- [ ] **Step 14: Checks and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/templum/src
git commit -m "Add note blocks: Markdown edited in place, with a rendered preview"
```

---

### Task 6: The toolbar

**Files:**
- Create: `crates/templum/src/toolbar.rs`
- Modify: `crates/templum/src/main.rs` (`mod toolbar;`)
- Modify: `crates/templum/src/workspace.rs` (`Tool`, the hand layer, Esc, the toolbar child)
- Modify: `crates/templum/src/title_bar.rs` (remove `new-kanban`, `new-terminal`)
- Modify: `crates/templum/src/menu.rs` (`Target::Agents`, the shared agents list)

**Interfaces:**
- Produces:
  ```rust
  // workspace.rs
  #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
  pub(crate) enum Tool { #[default] Select, Hand }
  impl Workspace { pub(crate) fn set_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>); pub(crate) tool: Tool }
  // toolbar.rs
  pub(crate) fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement;
  // menu.rs
  Target::Agents(Point)   // the agents list alone, for the toolbar; blocks go at that world point
  ```
  Element ids: `toolbar`, `tool-select`, `tool-hand`, `tool-terminal`, `tool-note`, `tool-kanban`, `tool-agent`.

- [ ] **Step 1: Write the failing tests**

In `workspace.rs` `mod tests`:

```rust
    #[gpui_kit::test]
    fn the_toolbar_adds_blocks_and_the_title_bar_lost_its_add_buttons(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.find("toolbar");
            assert!(window.try_find("new-terminal").is_none(), "no + in the title bar");
            assert!(window.try_find("new-kanban").is_none());
            window.click("tool-terminal", cx);
            window.render_frame(cx);
            window.click("tool-note", cx);
            window.render_frame(cx);
            window.click("tool-kanban", cx);
            window.render_frame(cx);
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.space.terminals().count(), 2);
            assert_eq!(
                ws.space
                    .blocks
                    .values()
                    .filter(|b| b.note().is_some())
                    .count(),
                1
            );
            assert_eq!(
                ws.space
                    .blocks
                    .values()
                    .filter(|b| b.kanban().is_some())
                    .count(),
                1
            );
            assert_eq!(ws.tool, super::Tool::Select, "block tools don't stay on");
        });
    }

    #[gpui_kit::test]
    fn the_hand_tool_pans_over_blocks_until_escape(cx: &mut TestAppContext) {
        let (handle, ws) = open(cx);
        let id = first_id(&ws, cx);
        let before = ws.read_with(cx, |ws, _| {
            (ws.space.canvas.camera.origin, ws.space.canvas.get(id).unwrap().rect)
        });
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.click("tool-hand", cx);
            window.render_frame(cx);
            window.find("hand");
            let from = window.find(("title", id.0 as usize)).bounds().center();
            press_with(
                window,
                cx,
                MouseButton::Left,
                Modifiers::default(),
                (f32::from(from.x), f32::from(from.y)),
                (f32::from(from.x) + 40.0, f32::from(from.y) + 10.0),
            );
        })
        .unwrap();
        ws.read_with(cx, |ws, _| {
            assert_eq!(ws.tool, super::Tool::Hand);
            assert_eq!(ws.space.canvas.get(id).unwrap().rect, before.1, "not moved");
            assert_ne!(ws.space.canvas.camera.origin, before.0, "panned");
        });
        cx.update_window(handle, |_, window, cx| {
            window.press("escape", cx);
            window.render_frame(cx);
            assert!(window.try_find("hand").is_none(), "the layer is gone");
        })
        .unwrap();
        ws.read_with(cx, |ws, _| assert_eq!(ws.tool, super::Tool::Select));
    }
```

`try_find` is `gpui_kit::test::TestWindowExt`'s; `find` panics on a missing element.

Then change the two existing tests that click the title bar: line 3768 `window.click("new-terminal", cx)` → `window.click("tool-terminal", cx)`; line 6111 `window.click("new-kanban", cx)` → `window.click("tool-kanban", cx)`; and `the_board_button_opens_a_block_with_the_board_list` keeps its name.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p templum toolbar`
Expected: compile error (`Tool` not found).

- [ ] **Step 3: The tool state and the hand**

In `workspace.rs`:

```rust
/// What the pointer does on the canvas. Block tools act at once and don't stay on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    #[default]
    Select,
    /// Pans with any drag or wheel, over blocks too, like holding Alt.
    Hand,
}
```

Add `pub(crate) tool: Tool,` to `Workspace` (after `hand: bool,`), initialized `tool: Tool::default(),`. Add:

```rust
    pub(crate) fn set_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        self.tool = tool;
        self.menu = None;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }
```

In `render`, the hand layer's condition: `let hand = window.modifiers().alt || self.tool == Tool::Hand;` (keep `self.hand = window.modifiers().alt;` for the modifier bookkeeping: change the line to `self.hand = window.modifiers().alt;` before computing `hand`). In `on_key_down`, before the keymap match:

```rust
        if e.keystroke.key == "escape"
            && !e.keystroke.modifiers.modified()
            && self.tool != Tool::Select
        {
            self.set_tool(Tool::Select, window, cx);
            cx.stop_propagation();
            return;
        }
```

Add the toolbar as the last child of `area` (after the hint `when`): `.child(toolbar::render(self, cx))`, and `use crate::toolbar;`.

- [ ] **Step 4: `toolbar.rs`**

```rust
//! The floating toolbar at the bottom of the canvas: pointer tools, block tools and the AI
//! agents. See docs/specs/cockpit.md, section 3.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, BoxShadow, ClickEvent, Context, MouseButton, MouseDownEvent, ScrollWheelEvent,
    SharedString, TestSupportExt, div, point, px,
};
use templum_canvas::Point;

use crate::keys::{self, Command};
use crate::menu::Target;
use crate::theme::Ui;
use crate::workspace::{Tool, Workspace};

const BUTTON: f32 = 32.0;
/// Clear of the canvas's bottom edge.
const MARGIN: f32 = 12.0;

pub(crate) fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement {
    let ui = ws.ui;
    let tool = ws.tool;
    let key = |command: Command| {
        ws.keymap
            .first(command)
            .map(|k| format!(" · {}", keys::display(k)))
            .unwrap_or_default()
    };
    let group = |children: Vec<AnyElement>| {
        div()
            .flex()
            .items_center()
            .gap(px(2.0))
            .px(px(4.0))
            .children(children)
    };
    let pointer = group(vec![
        button("tool-select", IconName::MousePointer2, "Select · Esc", tool == Tool::Select, ui)
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                ws.set_tool(Tool::Select, window, cx)
            }))
            .into_any_element(),
        button("tool-hand", IconName::Hand, "Hand: move the canvas over blocks", tool == Tool::Hand, ui)
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.set_tool(Tool::Hand, window, cx)))
            .into_any_element(),
    ]);
    let blocks = group(vec![
        button("tool-terminal", IconName::SquareTerminal, format!("Terminal{}", key(Command::NewTerminal)), false, ui)
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_terminal(window, cx)))
            .into_any_element(),
        button("tool-note", IconName::StickyNote, format!("Note{}", key(Command::NewNote)), false, ui)
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_note(window, cx)))
            .into_any_element(),
        button("tool-kanban", IconName::LayoutDashboard, format!("Kanban board{}", key(Command::NewKanban)), false, ui)
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| ws.new_kanban(window, cx)))
            .into_any_element(),
        button("tool-agent", IconName::Bot, "AI agent", false, ui)
            .on_click(cx.listener(|ws, e: &ClickEvent, window, cx| {
                let at = ws.space.canvas.camera.to_world(ws.center(window));
                ws.open_menu(Target::Agents(at), e.position(), window, cx);
            }))
            .into_any_element(),
    ]);
    div()
        .absolute()
        .bottom(px(MARGIN))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .id("toolbar")
                .flex()
                .items_center()
                .h(px(BUTTON + 8.0))
                .rounded(px(12.0))
                .bg(ui.surface)
                .border_1()
                .border_color(ui.hairline)
                .shadow(vec![BoxShadow {
                    color: ui.shadow,
                    offset: point(px(0.0), px(4.0)),
                    blur_radius: px(12.0),
                    spread_radius: px(0.0),
                    inset: false,
                }])
                // Nothing under the toolbar reacts to its clicks or wheel.
                .block_mouse_except_scroll()
                .on_scroll_wheel(|_: &ScrollWheelEvent, _, cx| cx.stop_propagation())
                .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, cx| cx.stop_propagation())
                .child(pointer)
                .child(div().w(px(1.0)).h(px(20.0)).bg(ui.hairline))
                .child(blocks)
                .test_support(),
        )
}

fn button(
    id: &'static str,
    icon: IconName,
    tip: impl Into<SharedString>,
    active: bool,
    ui: Ui,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let tip = tip.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .size(px(BUTTON))
        .rounded(px(8.0))
        .cursor_pointer()
        .when(active, |el| el.bg(ui.card))
        .hover(move |s| s.bg(ui.hover))
        .active(move |s| s.bg(ui.pressed))
        .child(
            Icon::new(icon)
                .size(px(16.0))
                .text_color(if active { ui.accent } else { ui.title }),
        )
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .test_support()
}
```

`ClickEvent::position()` is in `gpui-pre-0.3.8/src/interactive.rs`. `Workspace::center` is private: make it `pub(crate)`.

- [ ] **Step 5: `Target::Agents` and the shared list**

In `menu.rs`, add the variant:

```rust
    /// The AI agents alone (the toolbar): the chosen one opens at this world point.
    Agents(Point),
```

Move the agent-list building out of the `Items::Canvas` arm into a function both arms call:

```rust
/// The agents, installed first; the others say how to install them when chosen. `at`: where the
/// agent's block goes.
fn agent_items(
    ws: &Entity<Workspace>,
    menu: PopupMenu,
    agents: Vec<(AgentEntry, bool)>,
    at: Point,
) -> PopupMenu {
    let ws = ws.clone();
    let (found, missing): (Vec<_>, Vec<_>) = agents
        .into_iter()
        .partition(|(_, installed)| *installed);
    let entry = |menu: PopupMenu, (agent, installed): (AgentEntry, bool)| {
        let title = SharedString::from(agent.title.clone());
        menu.item(item(&ws, title, None, move |ws, window, cx| {
            launcher::ask_folder(ws, agent.clone(), at, installed, window, cx)
        }))
    };
    // Twenty agents: the list scrolls in a short window.
    let menu = found
        .into_iter()
        .fold(menu.scrollable(true).max_h(px(420.0)), entry);
    if missing.is_empty() {
        return menu;
    }
    missing
        .into_iter()
        .fold(menu.separator().label("Not installed"), entry)
}
```

The `Items::Canvas` arm's "AI agent" submenu closure becomes `move |menu, _, _| agent_items(&ws, menu, agents.clone(), at)` (clone `agents` before the closure since `Items::Canvas` owns it; `AgentEntry` is `Clone`). Add `Items::Agents { at: Point, agents: Vec<(AgentEntry, bool)> }` built in `items` the same way the canvas arm reads the agents (factor that read into `fn agents_for_menu(&self) -> Vec<(AgentEntry, bool)>`), and its `build` arm: `Items::Agents { at, agents } => agent_items(ws, menu, agents, at),`.

- [ ] **Step 6: Remove the title bar's buttons**

In `title_bar.rs`, delete the two `.child(button("new-kanban", ui)...)` and `.child(button("new-terminal", ui)...)` blocks (lines 541-566), and the module comment's "and a button for a new terminal". The `IconName` import stays (the other buttons use it).

- [ ] **Step 7: Run the toolbar tests, then all**

Run: `cargo test -p templum toolbar && cargo test --workspace`
Expected: all pass.

- [ ] **Step 8: Check by hand and bench**

`cargo run -p templum`: the pill at the bottom; each button; the hand over a terminal; the agent menu opens above its button and opening Claude Code places the block in the middle; Esc returns to Select. Ctrl+wheel still zooms over the toolbar? It must not (the toolbar stops the wheel); over the rest of the canvas it does.

Run: `cargo run --release -p templum -- --bench`, keep the JSON line.

- [ ] **Step 9: Checks and commit**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`

```bash
git add crates/templum/src
git commit -m "Put the tools in a floating toolbar at the bottom of the canvas" -m "<bench JSON line>"
```

---

### Task 7: Docs

**Files:**
- Modify: `README.md` (the mouse table, the shortcuts table, "Kanban boards", a new "Notes" section)
- Modify: `docs/ARCHITECTURE.md` (the "A block is a terminal or a kanban block" paragraph, line 81-83)
- Modify: `docs/specs/context-menus.md` (the menus table)
- Modify: `docs/specs/cockpit.md` (status of sections 1, 2, 3, 5)

- [ ] **Step 1: README**

- In the mouse table, change the "Right-click empty canvas" row's **Add ▸** list to "Terminal, Note or Kanban board".
- Add a row: `| Drag a block near another | It snaps to the other's edges or center within 8 px; a line shows which |`.
- In the shortcuts table, after the kanban row: `| Ctrl+Shift+M | Cmd+Shift+M | New note |`.
- "Kanban boards": replace "The board button in the title bar (or Ctrl+Shift+K)" with "The toolbar's board button (or Ctrl+Shift+K)".
- New section after "Kanban boards":

  ```markdown
  ## Notes

  The toolbar's note button (or Ctrl+Shift+M) puts a note on the canvas. Write Markdown in it: headings, lists
  (Tab indents an item), code. The eye button at the left of its header shows it rendered; the first heading is
  the note's title. Notes are saved with the canvas.

  ## The toolbar

  The bar at the bottom of the canvas holds the tools: **Select** and **Hand** (move the canvas by dragging
  anywhere, over blocks too; Esc goes back to Select), then a button for each kind of block (terminal, note,
  kanban board) and the AI agents.
  ```

- [ ] **Step 2: ARCHITECTURE and specs**

In `docs/ARCHITECTURE.md`, replace the "A block is a terminal or a kanban block (`Space::kanbans`)" sentence with: "Every block's content is a `Body` in `Space::blocks` (terminal, kanban, note); adding a kind is one enum arm, one `session::Item` variant and one `menu::Items` arm. Blocks snap to each other through `templum-canvas`'s `snap.rs` while they are moved or resized. The toolbar (`toolbar.rs`) holds the tools; `workspace::Tool` is the active one." Link `specs/cockpit.md`.

In `docs/specs/context-menus.md`'s table: the empty-canvas row lists "Terminal, Note, Kanban board"; add `| A note block | Preview (or Edit), Copy all, Close |`.

In `docs/specs/cockpit.md`, under **Delivery**, mark plan 1 done: add "Plan 1 shipped on 2026-MM-DD (fill in the date)" after the table, and move `AllAssets` from plan 3's row to plan 1's.

- [ ] **Step 3: Commit**

```bash
git add README.md docs
git commit -m "Document notes, snapping and the toolbar"
```

---

## Self-review

- **Spec coverage.** §1 → Task 2 (`Body`, `Space::blocks`, `LayoutBlock::Saved`, `Target::Block`, ghosts drawn empty for non-terminals: a note ghost renders `note::body` only when `live`, as the arm says). §2 → Tasks 3 and 4 (edges, centers, resize edges and size match, threshold `8/zoom`, guides, all blocks count, pan never snaps). §3 → Task 6 (Select, Hand, Terminal, Note, Kanban, AI agent, tooltips with shortcuts, active fill, title bar buttons removed, Add ▸ Note in Task 5, Esc, no single-letter keys, the layer; the drawing group is plan 2; `new_editor`/`new_git`/`new_docker`/`workspaces` are plans 4, 5, 6 and 3). The spec's "hidden while the settings panel is open on a narrow window" is left out on purpose: the panel is 480 px wide at most 60% of the window, and the toolbar is about 260 px centered, so they overlap only below about 740 px; add `.when(self.settings.is_none() || vp.w >= 900.0, ...)` in Task 6 Step 3 if the maintainer wants it. §5 → Task 5 (size, minimum, editor with markdown/soft wrap/no line numbers/terminal font at 13×z, preview with `TextView::markdown`, title rule, keyboard focus, menu, file format with `preview` defaulting to false, text read at snapshot time).
- **Placeholders.** None. Every GPUI and gpui-kit API named was read in the pinned sources: `EditorState::new(window, cx)`, `.language`, `.soft_wrap`, `.line_number`, `.default_value`, `.value`, `.set_value`, `.focus` (`gpui-base/src/input/base/state.rs`), `Editor::new/.appearance/.bordered/.h` (`gpui-component/src/input/editor.rs`), `TextView::markdown` (`gpui-base/src/text/text_view.rs`), `Focusable::focus_handle`, `FocusHandle::is_focused`, `ClickEvent::position`, `try_find`, `relative`, `AllAssets`, and the icon names `StickyNote`, `Hand`, `MousePointer2`, `Eye`, `EyeOff`, `Bot` (generated `icon_name.rs`).
- **Type consistency.** `Body::note()` (Task 5) is used by Task 6's test and menu; `Space::terminals()` yields `(BlockId, &Pane)` everywhere; `Gesture::update` returns `Update` in Task 3 and `drag_to` reads `.resized` / `.guides` in Tasks 3 and 4; `Workspace::center` is made `pub(crate)` in Task 6; `save_canvas_now(cx)` and `layout(agents, cx)` carry `cx` from Task 5 on.
- **Review Focus.** Each of the five has its test: 1 `shortcuts_run_from_inside_a_note`, 2 `clicking_a_note_focuses_its_editor`, 3 `snapping_lets_go_past_the_threshold` + the headless drag, 4 `the_hand_tool_pans_over_blocks_until_escape`, 5 `a_note_item_reads_back_and_preview_defaults_to_false`.
