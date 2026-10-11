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
| The empty canvas | **AI agent ▸** (the agents), **Add ▸** Terminal, Kanban board, Note. The block's top-left corner goes where you clicked. |
| A terminal block (header or body) | Copy (disabled without a selection), Paste, Close terminal |
| A kanban block (header, or its empty space) | Board list (or Back to the board, while the list shows), Close |
| A note block | Preview (or Edit), Copy all, Close |
| The toolbar's AI agent button | The agents; the chosen one opens in the middle of the canvas |
| A board in a block's board list | Open, Rename…, Delete… |
| A column's title | Rename…, Add a card, Delete |
| A card | Open, Delete card |
| A workspace tile in the sidebar | Rename…, Delete… (disabled for the last workspace) |

- Right-clicking a block focuses it first, so Copy and Paste act on that terminal.
- The menu closes on a choice, Esc, or a click anywhere else. The keyboard goes back where it was.
- Right-clicking again, anywhere, replaces the open menu.

## Architecture

| File | Change |
|---|---|
| `menu.rs` (new) | `Target` (what was right-clicked), `Workspace::open_menu`, each target's items, and drawing the open menu |
| `workspace.rs` | The open menu; the canvas's right-click; `add_terminal_at` and `add_kanban_at` |
| `block.rs`, `kanban.rs`, `sidebar.rs` | Each right-clickable element opens its target's menu and stops the event, so the innermost menu wins |

The menu itself is gpui-kit's `PopupMenu`, drawn the way its `context_menu` draws it.

**Adding to a menu later:** a new entry for something that already has a menu is one more item in its arm of
`Items::build`. A new block kind adds its item to the canvas's **Add ▸** submenu, and a new right-clickable thing
gets a `Target` variant, its `Items` arm, and a right-click handler that calls `open_menu` and stops the event.

## Testing

**Headless UI:**

- right-clicking the empty canvas, then **Add ▸ Terminal** or **Add ▸ Kanban board**, adds that block with its
  top-left corner at the click;
- right-clicking a card opens the card's menu, not its block's;
- a terminal's menu closes it;
- a second right-click replaces the first menu.
