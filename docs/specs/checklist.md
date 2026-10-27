# Checklists

- **Date:** 2026-10-17
- **Status:** Implemented
- **Builds on:** [cockpit.md](cockpit.md#5-notes) (note blocks), [kanban.md](kanban.md) and
  [connections.md](connections.md)

## Goal

A **checklist block** on the canvas: a list of items you tick off. It serves two uses, in the user's words: the
person's own tracking, or a list handed to an agent, which reads it, ticks items as it finishes them and adds new
ones (it can build the list for the person).

Checklists are kept app-wide, like notes: closing a block never loses one, and a dialog finds every checklist
again. A block shows **one** checklist, with no tabs: to see another at the same time, open it in another block.

**Success criteria**

1. Every behavior below works on Linux, macOS and Windows; Linux (Hyprland) is also checked by hand.
2. Checklists and checklist blocks come back after a restart.
3. A wired agent reads a checklist, ticks an item and adds items, and the block shows it at once.
4. All tests in [Testing](#testing) pass, and `--bench` and `--bench-mixed` stay at p95 ≤ 16.7 ms.

## Out of scope

Colors, due dates, sub-items, assignees, "Send to ▸", moving checked items to the end, a keyboard shortcut to open
one (the toolbar and the canvas menu do), arrow keys between items, redo.

## A checklist block

It is a block like the others: title bar, close, resize, move, focus, hover lift, pop-in and fade-out, menus, zoom,
wires, copy and paste, moving to another workspace.

- **Opening one:** the toolbar's **Checklist** button (gpui-kit's `ListChecks` icon, after Note) or **Add ▸
  Checklist** in the canvas menu opens a block with a new checklist at the viewport's center (the menu: at the point
  clicked), 320 × 360, at least 220 × 160. Its title is being edited, with the keyboard.
- **One checklist per block:** it fills the body under the header; there is no tab strip. A checklist is open in
  **one block at most**: opening it again shows that block, in its workspace (the camera goes there).
- **The block's title:** the checklist's name and its progress, "Deploy 2/5"; the name alone while it has no items.
  "Checklists" with none.
- **No checklist:** a block whose checklist was deleted in the dialog (or a saved block whose checklist is gone) says
  "No checklist open here" with **New checklist** (a new one, in this block) and **Open a checklist…** (the dialog,
  for it).
- **Where a checklist opens** (the dialog's click and its **New checklist**): where it is open already, that block is
  shown; else in the block the dialog was opened from when it shows none; else in a new block beside that block, the
  camera going there, as an editor opens beside a terminal (from the canvas menu's Checklists…: at the viewport's
  center). No agent tool opens one: an agent works on the checklist a wired block shows.
- **The header's button** at its top left (`ListChecks`, tooltip "All checklists") opens the dialog for the block.

### The checklist on screen

Under the header, scrolling when it doesn't fit (the wheel scrolls it and stops there; Ctrl+wheel zooms the canvas),
with [a scrollbar](#a-scrollbar-kanban-card-157) then:

- **The name** on top, in 15 world units, semi-bold. A new checklist is named "Checklist". A click edits it: Enter or
  a click elsewhere keeps it, Esc too. A blank name reads "Checklist".
- **The items**, one row each: a check box, then the text, which wraps at the block's width. A checked item's text is
  struck through and dimmed, in place.
  - A click on the box checks or unchecks it.
  - A click on the text edits it, in place.
  - Hovering a row shows a × at its right, which deletes the item.
  - **Dragging a row** onto another puts it in that row's place; onto the list's empty space or **+ Add an item**, at
    the end. Rows move within their checklist only. While dragged, the row follows the pointer and the drop target is highlighted (as a kanban card).
- **+ Add an item** after the last row adds an empty item at the end, being edited.

**Editing an item** (one field at a time in a block, gpui-kit's single-line `Input` in the row, in the item's font
and size at the zoom, its padding and height set to scale with the zoom). While an item is edited its row is one
line: a long item's text scrolls sideways in the field, and the rows below move up until editing stops (the user's
choice over a wrapping field):

- every key is written to the checklist at once, so agents and other blocks see it;
- **Enter** adds a new item right after it, being edited; **Enter** in an empty item stops editing (the item
  goes);
- **Backspace** in an empty item deletes it and edits the one before (the first item's: the name), its caret at
  the end; a held Backspace so stops in the name and never closes the block;
- **Esc** or a click elsewhere stops editing;
- an item left empty when its editing stops is deleted.

**Undo:** with a checklist block focused and no field editing, Ctrl+Z (Cmd+Z on macOS) undoes the last change to a
checklist's content (an item added, edited, checked, moved or deleted, its name, an agent's change), up to 50: that
checklist goes back as it was, in every block showing it. A field's typing is one step, taken at its first key;
inside the field, Ctrl+Z is the field's own. Adding a checklist, removing a blank one and deleting one in the dialog
(already a second click) are not steps, so a block never shows a checklist undo took away; a step whose checklist was
deleted since is skipped.

**Blank checklists go:** a checklist named "Checklist" (or blank) whose items have no text, whose block closes, is
removed; nothing is lost.

**Right-click a checklist block:** All checklists…, *the grid's items*, Close.

**Copy and paste** a checklist block (canvas selection): the pasted block shows a copy of its checklist, with the
same name.

**Far away** (docs/specs/level-of-detail.md): a checklist block is drawn in full down to the Card level, as a kanban
block, which its 13 world units reach under 25%: then it is its card, or its shape; below Full its title is the
fixed-size label, as every block's.

### The Checklists dialog

Opened by the header's button, **All checklists…** in the block's menu, **Checklists…** in the canvas menu, and
**Open a checklist…**. As the Notes dialog:

- every checklist, the most recently changed first; each row its name, its progress ("2/5"), when it last changed
  ("5 min ago") and where it is open ("Open", "Open in Workspace 2");
- the field on top finds checklists by their name and their items' text, in any case;
- a click opens one, and **New checklist** adds one, where [a checklist opens](#a-checklist-block);
- the trash deletes a checklist on its **second** click ("Click again to delete" shows after the first); the block that
  showed it shows none.

## On a mirrored workspace (Templum to Templum, part 2e; kanban card 205)

A checklist block of a [mirrored workspace](templum-to-templum.md) is drawn from home's state by the same view, and
what is done to it is done at home (`check_item`, `add_items`, `edit_item`, `delete_item`, `edit_checklist` and
`new_checklist`, each the rule's deed, asked of `why_not` first):

- the check box and the × send their tool, and show at once: a check, an edit and a delete stand over home's state
  until it shows them, a refusal, or two seconds;
- an item's text is a field that starts with home's text and writes nothing as it is typed: **Enter** sends the whole
  text once (nothing when it is blank or unchanged: the item stays, ✕ deletes), **Esc** drops it, and the keyboard
  leaving sends what Enter would, to the remote the field was opened on, whichever workspace is shown by then;
- **+ Add an item** opens a field for a new item: Enter sends `add_items` and opens the next, a blank Enter closes it,
  and the keyboard leaving sends it too (the item appears with home's state);
- the checklist's name is a field as an item's is: Enter or the keyboard leaving sends `edit_checklist` with the
  whole name once (nothing when unchanged; a blank one is "Checklist", as home shows it), Esc drops it; the new name
  shows at once and stands over home's state as an edit does;
- a row dragged onto another row, or onto the list's empty space or + Add an item, sends `edit_checklist` with the
  item and `before` (the row it took the place of, or `"end"`), and shows there at once, held the same way; a drop that
  changes nothing sends nothing, and an item of another block is not taken;
- **All checklists…** (the header's button, the block's menu, the canvas menu's Checklists…, the empty block's Open a
  checklist…) lists home's checklists that the blocks of the mirrored workspace show (what the share sees), with
  their progress and "Open"; a click shows the block that has it, as locally. **New checklist** in a block that shows
  none is home's `new_checklist` there; elsewhere home adds a checklist block beside the block (or in the view), as
  Add ▸ Checklist does. The dialog's trash says "Not on a mirrored workspace yet" and deletes nothing. A dialog acts
  only while its workspace is shown: one of this computer's, once a mirror is shown, does nothing and says so;
- a block that shows none offers New checklist and Open a checklist…, as locally; the controls the share or home
  can't do are dimmed with the rule's words as their tooltip (a home without `edit_checklist` or `new_checklist`:
  "winx-desktop's Templum is older: update it to do this from here."); Ctrl+Z there stops at the mark and says to undo
  on home.

Nothing local is written: the mirror's `checklists.json` stays as it was.

## Agents

A checklist block wired to an agent ([connections.md](connections.md#tools)) gets five tools and their verbs. The
checklist is the block's; an item is named by its id, as `read_checklist` shows it.

| Tool | Arguments | Does | Needs |
|---|---|---|---|
| `read_checklist` | `block` | The checklist's name and progress, then one line per item: its id, `[x]` or `[ ]`, its text | wired checklist |
| `check_item` | `block`, `item`, `done` (true) | Checks the item (unchecks it with `done: false`) | wired checklist |
| `add_items` | `block`, `text` | Each line with text becomes an item at the end; a leading `- ` or `* `, then `[ ]` or `[x]`, is dropped (`[x]` adds it checked); the new items' ids | wired checklist |
| `edit_item` | `block`, `item`, `text` | The item's text becomes `text` (blank is refused: "delete_item removes an item"); `Item 41 is now "Build".` (card 35, [agent-tools.md](agent-tools.md#fixing-a-note-or-a-checklist)) | wired checklist |
| `delete_item` | `block`, `item` | Deletes the item, as its row's ×; `Deleted item 41.` (card 35) | wired checklist |

- Results, as the other tools': `Deploy (2/5)\n12 [x] Build\n13 [ ] Run the tests`; `Item 13 is checked.`;
  `Added items 14, 15.`. Refusals say what to do next: `4qm2a isn't a checklist.`, `4qm2a shows no checklist.`,
  `There's no item 99 in "Deploy". read_checklist shows its items.`, `There's nothing to add.`
- Each call is one Ctrl+Z step, and every block showing the checklist updates at once.
- `list_blocks` and `whoami` give the kind `checklist`.
- The MCP instructions' kanban and note sentence gains: "check the items of a connected checklist as you finish
  them."
- **Command line:** `templum checklist read <block>`, `templum checklist add <block> <text>` (its words joined with
  spaces, as `templum note append`), `templum checklist check|uncheck <block> <item>`, `templum checklist edit <block>
  <item> <text>`, `templum checklist delete <block> <item>`.

## Files

- **`<data folder>/checklists.json`** holds every checklist:

  ```json
  {
    "version": 1,
    "next_id": 15,
    "checklists": [
      { "id": 11, "name": "Deploy", "changed": 1760000000, "items": [
        { "id": 12, "text": "Build", "done": true },
        { "id": 13, "text": "Run the tests" }
      ] }
    ]
  }
  ```

  - Ids are unique across checklists and items, and never reused (`next_id`; a file whose `next_id` lags behind its
    ids is read with the largest, and an id a hand-edited file gives twice gets a new one after its first, as does an
    item it gives none).
  - A missing `items` means none, a missing `done` false; an unchecked item writes no `done`. `changed` is in Unix
    seconds, moved by any change to the checklist.
- **A canvas file** gets a new item: `{ "kind": "checklist", "rect": {…}, "checklist": 11 }` (no `checklist`: it
  shows none). A Templum from before this spec sets such a file aside ([persistence.md](persistence.md#the-file)),
  as for the kanban.
  - Before one checklist per block, a block named its tabs: `"checklists": [11, 16], "active": 1`. Such a block shows
    the tab that was on screen (16), else the first of its tabs still there that no other block shows; its other
    checklists stay in the dialog, but for blank ones, which go. These two fields are read, never written: the canvas
    is saved back with `"checklist"`.
- **Saving:** as `boards.json`: written by the 5-second snapshot when the checklists changed, and on quit, through the
  temporary file and rename. A `checklists.json` Templum can't parse is kept as `checklists.json.bak` and the
  checklists start empty; one it can't open isn't written that session. The canvases are written either way: a block
  naming a checklist that isn't there shows none (nothing is lost, the dialog keeps every checklist).
- The workspaces screen's card counts them: "2 checklists".

## Architecture

| File | Change |
|---|---|
| `checklists.rs` (new) | The model (`Checklists`, `Checklist`, `Item`) and every edit: add, rename, add an item after another, set its text, check, move, delete, duplicate, find, progress, the blank test, `add_items`'s line parsing. Serde; `checklists.json`'s load and save. Pure and unit-tested, as `notes.rs` and `board.rs`. |
| `checklist.rs` (new) | The block (`Checklist { list, editing }`): its body (name, rows, drag and drop, the field), the empty block, the header button and the dialog. |
| `workspace/checklist_blocks.rs` (new) | Opening checklists in blocks (where open, an empty block, a new one beside), deleting them, the field's start and end, `edit_checklist` (one checklist's content as it was, for Ctrl+Z), as `edit_boards`. |
| `workspace/mod.rs`, `block.rs`, `session.rs`, `workspace/persist.rs`, `workspace/selection.rs`, `spaces_screen.rs` | `Body::Checklist`, `Item::Checklist`, `checklists.json` with the snapshot, paste, counts. |
| `menu/`, `toolbar.rs` | Add ▸ Checklist, Checklists…, the block's menu, the toolbar button. |
| `templum-mcp`'s `tools.rs`, `cli.rs` | The three tools and their verbs. |
| `workspace/content.rs`, `workspace/bus.rs` | The tools on a wired checklist; the kind `checklist`. |

The model lives in the app crate with its serde types, as `notes.rs` and `board.rs` do.

## Testing

Tests are written first.

**Unit (`checklists`):** ids never reused across checklists and items, after removals and in a copy; an item added
after another, and at the end; moving an item before another and to the end; checking moves `changed`; find by name
and by item text, any case, newest first; progress and the title; the blank test; `add_items`'s lines (markers
dropped, `[x]` checked, blank lines skipped); a file written and read back is equal, `next_id` follows the ids, a
missing `done` is false.

**Unit (`session`):** a canvas with a checklist item reads back equal; a block's tabs from before are read and never
written.

**Unit (`templum-mcp`):** the three tools parse their arguments and refuse a missing one; the verbs build their
calls.

**Headless UI (`workspace/tests/`):**

- the toolbar button opens a block with a new checklist, its name editing;
- Enter in an item adds the next one; Backspace in an empty one deletes it; an item left empty is deleted;
- the box checks an item, and the block's title shows the progress;
- the block has no tabs; closing a blank checklist's block removes it, closing a named one's keeps it;
- the dialog opens a checklist where it is open, in an empty block, and beside a block that shows one (so does its
  New checklist); it deletes one, and its block shows none;
- Ctrl+Z undoes a check, and skips a step whose checklist was deleted;
- a wired agent's `read_checklist`, `check_item` and `add_items`, and an unwired one refused;
- checklists and blocks come back after a restart; a block of tabs from before shows its tab on screen and keeps
  the others in the dialog; a pasted block shows a copy.
- a long checklist's scrollbar is over its list and its track scrolls it; its scroll follows the zoom; a press on
  the scrollbar picks an unpicked block and a middle drag from there pans; a press in a field keeps it editing.

**Manual (Hyprland):** edit, check and drag items at zoom 0.5, 1 and 2, a one-line item's text staying where the
row's was while it is edited; a long list scrolls; the dialog finds and deletes.

## Delivery

1. This spec.
2. The model and `checklists.json`.
3. The block: rows, editing, the dialog, menus, the toolbar, persistence.
4. Drag and drop.
5. The agents' tools and verbs.
6. The docs (ARCHITECTURE.md, connections.md, context-menus.md, level-of-detail.md).
7. One checklist per block: the tab strip removed, a block of tabs from before loaded with its tab on screen.
8. [A scrollbar](#a-scrollbar-kanban-card-157).

## A scrollbar (kanban card 157)

The card, as the person wrote it: *"Bloco de checklist falta scroll na tela."* A long checklist already scrolled with
the wheel, but nothing showed that there was more, or let the pointer take hold of it.

- **The scrollbar** is the kanban columns' (`kanban::scrollbar_over`, gpui-component's), over the right edge of the
  list (name, items and + Add an item, which all scroll): on hover over its track or while scrolling, as the theme
  says, and only once the list overflows. Its track (16 units) and thumb scale with the zoom.
- **The scroll follows the zoom:** the list's offset scales with it, so a scrolled checklist shows the same items at
  every zoom (zoom changes no layout).
- **Presses:** as in a kanban column, a press on the shown scrollbar's strip is the scrollbar's; the list picks an
  unpicked block and pans for the middle button in the capture phase, before it. A picked block isn't picked again
  there: that would stop its field. A right-click on the strip opens no menu ([minors.md](../minors.md)).
- **The wheel is unchanged:** over a picked block it scrolls the list, over any other it moves the canvas.

**Not testable headlessly:** the thumb's look on hover. **Manual (Hyprland):** a long checklist at zoom 0.5, 1 and 2:
the thumb follows the wheel and is dragged.
