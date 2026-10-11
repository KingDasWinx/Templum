# Kanban boards

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [workspaces.md](workspaces.md) and [persistence.md](persistence.md)

## Goal

The canvas becomes a whiteboard for work, not only terminals. A **kanban block** sits on the canvas like a terminal
and shows a **board**, Trello-style: columns of cards that you drag around. Boards are the user's projects. They
live outside any canvas, so any number of blocks, in any workspace, can show any board, and two blocks showing the
same board stay in sync.

**Success criteria**

1. Every behavior below works on Linux, macOS and Windows. CI checks this on every OS, and Linux (Hyprland) is also
   checked by hand.
2. Boards and kanban blocks come back after a restart.
3. All tests in [Testing](#testing) pass in CI on the 3 OS, and `--bench` stays at p95 ≤ 16.7 ms.

## Out of scope

- Checklists, due dates, members, attachments, comments, search, and archiving.
- Label names: a label is one of six colors.
- Undo.

## A kanban block

- **Opening one:** the title bar's board button (gpui-kit's dashboard icon), or the `new_kanban` command
  (Ctrl+Shift+K, Cmd+Shift+K on macOS). It opens at the viewport's center, 900 × 540, and pops in like a terminal.
- **The card around it** is a terminal's: the header with the title, ✕ and the resize grip, moving, resizing,
  focus, hover lift and the pop-in and fade-out. It can't be smaller than 420 × 280.
- **Its title** is the board's name. The title stays the handle for moving the block; a ⇅ button at the left of
  the header shows the board list in the block (and goes back to the board).
- **Everything inside scales with the zoom**, like a terminal's text.

### The board list

Shown by a new block, by the ⇅ button, and when its board was deleted.

- One row per board: the name, and how many cards it has. Clicking a row shows that board in the block.
- **+ New board** creates "Board *n*" with three columns, To do, Doing and Done, and shows it.
- **Right-click a row:** Rename… and Delete…. Deleting asks first, saying how many cards go with it. Blocks showing
  a deleted board show the list.

### The board

- **Columns** sit side by side, 260 wide, and scroll sideways when they don't fit. Each has its title and its
  number of cards. **+ Add a column** after the last one adds "New column".
  - **Right-click a column's title:** Rename… and Delete…. Deleting a column with cards asks first.
  - **Dragging a column's title** onto another column moves it to that column's place.
- **Cards** stack in their column, which scrolls when they don't fit.
  - A card shows its labels as small colored bars, its title, and a line icon when it has a description.
  - **+ Add a card** at the bottom of a column adds "New card" there and opens it.
  - **Dragging a card** onto another card puts it in that card's place; onto a column's empty space, at the end
    of that column. While dragged, the card follows the pointer and the drop target is highlighted.
  - **Clicking a card** opens it.

### An open card

A dialog in the middle of the window, at the window's scale:

- the title, in a text field;
- the description, in a multi-line text field;
- the six label colors (red, yellow, green, cyan, blue, magenta, from the theme), each a toggle;
- **Delete card**, and **Done** (or Esc) to close.

Every change applies as it is typed or clicked, so every block showing the board updates at once.

## Files

- **`<data folder>/boards.json`** holds every board:

  ```json
  {
    "version": 1,
    "next_id": 12,
    "boards": [
      {
        "id": 1,
        "name": "Templum",
        "columns": [
          { "id": 2, "title": "To do", "cards": [
            { "id": 5, "title": "Kanban", "description": "Trello-like", "labels": ["green"] }
          ] }
        ]
      }
    ]
  }
  ```

  - Ids are unique across boards, columns and cards, and never reused (`next_id`).
  - Missing `description` and `labels` mean empty. An unknown label color is dropped.
- **A canvas file** gets a second kind of item: `{ "kind": "kanban", "rect": {…}, "board": 1 }`. `board` is
  missing for a block showing the board list. A Templum from before this spec sets such a file aside
  ([persistence.md](persistence.md#the-file)), as designed.
- **Saving:** `boards.json` is written by the 5-second snapshot when the boards changed, and on quit, through the
  same temporary file and rename. A file Templum can't read is kept as `boards.json.bak` and the boards start empty.

## Architecture

| File | Change |
|---|---|
| `board.rs` (new) | The model (`Boards`, `Board`, `Column`, `Card`, `Label`) and every edit: add, rename, remove, move. Serde, as `session.rs` does. Pure and unit-tested. |
| `kanban.rs` (new) | The kanban block's body (board list, columns, cards, drag and drop) and the card dialog. |
| `prompt.rs` (new) | The text-field and confirmation dialogs, shared with the sidebar. |
| `block.rs` | The card around any block; the terminal's body moves to its own function. |
| `session.rs` | `Item::Kanban`; the layout and snapshot of kanban blocks; reading and writing `boards.json`. |
| `workspace.rs` | A space's kanban blocks; the boards, saved with the snapshot; the `new_kanban` command. |
| `title_bar.rs` | The board button. |

The board model lives in the app crate with its serde types, as `session.rs` and `config.rs` do: it is pure and
tested headlessly, and a copy without serde in `templum-canvas` would have to be kept in step by hand.

## Testing

Tests are written first, as always.

**Unit (`board`):**

- a new board has the three columns;
- ids are never reused, across boards and after removals;
- moving a card within a column and to another column, to the end and before another card;
- moving a column;
- removing a column removes its cards;
- toggling a label, and editing a card;
- a file written and read back is equal; an unknown label is dropped.

**Unit (`session`):** a canvas with a kanban item reads back equal.

**Headless UI:**

- the board button opens a kanban block showing the board list;
- **+ New board** creates a board and shows it in the block;
- two blocks showing the same board both show a card added through one of them;
- deleting a board sends its blocks back to the list;
- kanban blocks and boards come back after a restart.

**Manual (Hyprland):** drag cards and columns at zoom 0.5, 1 and 2; open, edit and delete a card; scroll a long
column.

## Delivery

1. This spec.
2. The board model and `boards.json`.
3. The kanban block: list, board, drag and drop, the card dialog, the docs.

## Risks

- **Drag and drop on a zoomed canvas:** the dragged card is drawn at the canvas's zoom, and drop targets come from
  the element tree, so they follow the zoom by themselves.
- **Wheel events** over a kanban block stay in it, as over a terminal: a vertical wheel scrolls a column's cards,
  a sideways one the row of columns, and Ctrl+wheel zooms the canvas. GPUI measures a scroll container's range
  from its own children, so the columns are the row's children, with no wrapper in between.
