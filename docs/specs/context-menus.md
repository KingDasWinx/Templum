# Right-click menus

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [kanban.md](kanban.md) and [workspaces.md](workspaces.md)

## Goal

A right-click anywhere in Templum opens a menu for what is under the pointer, and the empty canvas gets one to add
blocks where you clicked. One system serves every menu, so a new block kind or action is one more entry.

**Success criteria**

1. Every menu below opens on Linux, macOS and Windows, at the pointer, and only one menu is ever open.
2. All tests in [Testing](#testing) pass in CI on the 3 OS.

## Before this spec

gpui-kit's `context_menu` was used in three places: the sidebar's tiles, the kanban board list's rows and the kanban
columns' titles. It opens a menu for every wrapped element under the pointer, so a menu on a card inside a block
would have opened together with the block's. The canvas and the blocks had no menu.

## The menus

| Right-click on | Items |
|---|---|
| The empty canvas | **AI agent ▸** (the agents), **Add ▸** Terminal ▸ (Default, then the shells found, [shells.md](shells.md#the-menus)), Kanban board, Note, Checklist, Editor, **Notes…** (the Notes dialog), **Checklists…** (the Checklists dialog). The block's top-left corner goes where you clicked. |
| A terminal block (header or body) | Copy (disabled without a selection), Paste, *the grid's items*, Close terminal |
| A kanban block (header, or its empty space) | All boards… (the Boards dialog), Rename board… and Delete board… (asks first; for the board it shows), *the grid's items*, Close |
| A note block | All notes… (the Notes dialog), Rename… (the note on screen's name), Copy all (the note on screen), Send to ▸ (the workspace's terminals), Color ▸ (None and the six label colors, for the note on screen), *the grid's items*, Close |
| A checklist block | All checklists… (the Checklists dialog), *the grid's items*, Close |
| An editor block | Change folder…, Hide (or Show) the file tree, Turn soft wrap on (or off), Send to ▸, *the grid's items*, Close |
| A file in an editor's tree | Open, Reveal in file manager, File history, Blame (in the Git popup, moved to the file's repository when it shows another), Rename…, Delete… |
| A folder in an editor's tree | New file…, New folder…, Open a terminal here, Rename…, Delete…, Reveal in file manager |
| An editor tree's empty space | New file…, New folder…, Refresh |
| The folder's ▾ in the Git popup's header (a click) | The repositories this workspace's terminals work in, then the workspace's recent folders (each once, the one shown left out), Choose another folder…, Refresh, Open a terminal here (at the view's center; the popup closes) |
| The folder's ▾ in the Git popup's header on a mirrored workspace (a click) | Home's folders (the workspace's and its terminals', then home's recent folders of the workspace; each once, less the one shown and those inside its repository), Choose another folder… (dimmed: stays on home), Refresh, Open a terminal here (at home; the popup closes) |
| The ⋯ of the Git popup's top bar (a click) | Fetch, Pull, Push, Recently discarded… (with a data folder), Clone a repository… |
| The Git popup's Publish branch, with several remotes (a click) | Publish to *remote*, one per remote, as `git remote` lists them |
| The ▾ beside the Git popup's Commit | Commit (or Commit all changes), and the same & Push |
| The ⚙ beside the Git popup's Generate ✨ | The agent's models (the one in use checked), Instructions for this repository… |
| The terminal count in the Git popup's top bar | The terminals working in its repository, agents first; the one chosen is shown |
| A row of the Git popup's Changes | Stage (or Unstage), Discard changes… (unstaged rows), Open, File history, Blame (not on a new file: it has no history), Copy path, Ignore; a conflict: Open in editor, Use the version from *HEAD*, Use the version from *the other side's label* (the markers' labels, never "ours" or "theirs"), Mark resolved (asks while markers are left), Copy path, Ignore |
| A line of the Git popup's diff panel | Copy (disabled without a selection), Copy line, Copy hunk, Send to ▸ (the selection, else the hunk), Open in editor at this line (the working changes only, not a comparison) |
| A commit in the Git popup's History | Checkout…, New branch here…, Create tag…, Revert…, Undo this commit (the newest, not pushed), Copy hash, Copy message, Restore this version… (in a file's history, unless the commit deleted the file) |
| A file of the commit shown in the Git popup's History | File history, Blame (as the file was in that commit), Copy path |
| A branch in the Git popup | Compare with *main* (the default branch; not on itself), Merge into *current*…, Rename…, Delete… (an unmerged one asks again with its count), Publish (a local branch with no upstream, or one deleted on the server), Copy name |
| A stash in the Git popup | Apply, Pop, Drop…, Show |
| The toolbar's AI agent button | The agents; the chosen one opens in the middle of the canvas |
| The toolbar's Terminal button (a click) | Default (*its name*), then the shells found; the chosen one opens where a new terminal goes |
| A column's title | Rename…, Add a card, Delete |
| A card | Open, Send to ▸, Delete card (asks first) |
| A workspace tile in the sidebar, or its card on the Home (also its ⋯) | Open (disabled for the one shown), Edit…, Stop (disabled while it is the only workspace, or stopped), Delete… (disabled for the last workspace) |
| A drawing (with the Select tool) | Delete |

On a **mirrored workspace** the same menus open, and what home can't do there is dimmed with the rule's words as its
tooltip ([Templum to Templum](templum-to-templum.md#batch-2b-blocks-wires-and-agents-on-the-canvas)): the canvas menu's
**AI agent ▸** lists home's agents (before home answered: "Asking *computer*…") and a choice asks which folder of
home's;
**Add ▸** has Terminal ▸ (home's shells: Default, then the others home has; "Asking *computer*…" before home answered;
an older home's Templum has Default only, its other shells dimmed with the rule's words), Kanban board, Checklist, and
Note and Editor dimmed;
**Notes…** and **Checklists…** are dimmed. A remote terminal's menu is Copy, Paste (dimmed without a Full share),
**Hand a task…** (when an agent runs in it) and the block menu's end, with Fullscreen, Connect to ▸, Disconnect from ▸,
Move to workspace and Close each dimmed when home can't. A card of home's has the end alone. A wire's menu has Rename
wire… and Disconnect. The toolbar's shells' menu lists home's shells, as Add ▸ Terminal ▸; a drawing's menu doesn't
open.

Every block's header has a **⋯** (More), shown on hover beside Fullscreen and Close, which opens the block's menu at the
click, as a right-click does.

Every block's menu ends with **Move to workspace ▸** (the other workspaces, a stopped one disabled, and New
workspace; the selection the block is in moves with it), then Close.

*The grid's items*, before Close in every block's menu ([grid.md](grid.md)): on the canvas, **Fullscreen** (with the
blocks selected along with it); in grid mode, **AI agent ▸** and **Add ▸** (a new block taking half of the tile), **From the canvas ▸** (the
workspace's blocks left on the canvas), then **Flip the split** (a tile with a neighbour), **Leave the grid** and
**Back to the canvas**. In grid mode a gap between tiles opens the empty canvas's menu.

- Right-clicking a block focuses it first, so Copy and Paste act on that terminal.
- The menu closes on a choice, Esc, or a click anywhere else. The keyboard goes back where it was.
- Right-clicking again, anywhere, replaces the open menu.
- **Send to ▸** types a note (or its selection), a card (title, then description) or an editor's selection (or
  its file and the cursor's line, `/path/file.rs:12`) into the chosen terminal, never pressing Enter: as a
  bracketed paste when the program asked for one, else on one line. The terminal is then shown with the keyboard.

## Architecture

| File | Change |
|---|---|
| `menu.rs` (new) | `Target` (what was right-clicked), `Workspace::open_menu`, each target's items, and drawing the open menu |
| `workspace.rs` | The open menu; the canvas's right-click; `open_shell` and `add_kanban_at` |
| `block.rs`, `kanban.rs`, `sidebar.rs` | Each right-clickable element opens its target's menu and stops the event, so the innermost menu wins |

The menu itself is gpui-kit's `PopupMenu`, drawn the way its `context_menu` draws it.

**Adding to a menu later:** a new entry for something that already has a menu is one more item in its arm of
`Items::build`. A new block kind adds its item to the canvas's **Add ▸** submenu, and a new right-clickable thing
gets a `Target` variant, its `Items` arm, and a right-click handler that calls `open_menu` and stops the event.

## Testing

**Headless UI:**

- right-clicking the empty canvas, then **Add ▸ Terminal ▸** a shell or **Add ▸ Kanban board**, adds that block with its
  top-left corner at the click;
- the toolbar's Terminal opens the shells' menu; Esc closes it and opens nothing;
- right-clicking a card opens the card's menu, not its block's;
- a terminal's menu closes it;
- a second right-click replaces the first menu.
