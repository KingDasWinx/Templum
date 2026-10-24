# Kanban boards

- **Date:** 2026-10-10
- **Status:** Implemented; [version 2](#version-2-kanban-card-40) (who a card is for, priority, a checklist, the
  card's id, scrolling while dragging, the open card redone) implemented; agents
  [edit and delete their own comments](#editing-and-deleting-a-comment-kanban-card-77) (card 77) and a long column
  has [a scrollbar](#a-scrollbar-in-a-column-kanban-card-78) (card 78); a card holds
  [attachments](#attachments-kanban-card-152) (card 152, implemented)
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

- Due dates, members, search, and archiving. (A card's checklist came with
  [version 2](#version-2-kanban-card-40), and its files with [card 152](#attachments-kanban-card-152).)
- Label names: a label is one of six colors.

## A kanban block

- **Opening one:** the title bar's board button (gpui-kit's dashboard icon), or the `new_kanban` command
  (Ctrl+Shift+K, Cmd+Shift+K on macOS). It opens at the viewport's center, 900 × 540, and pops in like a terminal.
- **The card around it** is a terminal's: the header with the title, ✕ and the resize grip, moving, resizing,
  focus, hover lift and the pop-in and fade-out. It can't be smaller than 420 × 280.
- **Its title** is the board's name ("Boards" while it shows none). The title stays the handle for moving the
  block; a button at the left of the header (gpui-kit's dashboard icon, tooltip "All boards") opens the
  [Boards dialog](#the-boards-dialog) for the block.
- **Everything inside scales with the zoom**, like a terminal's text.
- **Undo:** with a kanban block focused, Ctrl+Z (Cmd+Z on macOS) undoes the last change to the boards (a card or a
  column added, moved, renamed or deleted, a label, a board added or deleted), up to 50. A card's dialog changes
  the board only at Done, which is one step; typing in it is undone in its fields. Delete card in a card's menu asks first. A comment added, an assignee freed, a comment the
  person edits or deletes in the card's dialog ([below](#the-person-edits-and-deletes-a-comment-kanban-card-83), whose Undo row
  is one more step), and an agent's change ([connections.md](connections.md#tools); a comment edited or deleted by its
  author among them) are steps of this Ctrl+Z too.

### No board

A new block, and a block whose board was deleted, say "No board open here" with **New board** (it creates "Board
*n*" with three columns, To do, Doing and Done, and shows it) and **Open a board…** (the dialog, for the block).

### The Boards dialog

The Notes and Checklists dialog ([checklist.md](checklist.md)), for boards: a field that finds a board by its name
or a card's title, in any case; one row per board, in their order, with how many cards it has and where it is shown
("12 cards · Open", "Open in Workspace 2"); **New board**.

- A click on a row, or New board, shows that board in the block the dialog was opened for. Any number of blocks
  can show one board.
- **Its trash** deletes a board on a second click ("Click again to delete"); the blocks that showed it show none.
- **The block's menu** (right-click its header) has All boards…, and for the board it shows Rename board… and
  Delete board…, which asks first, saying how many cards go with it.

### The board

- **Columns** sit side by side, 260 wide, and scroll sideways when they don't fit. Each has its title and its
  number of cards. **+ Add a column** after the last one adds "New column".
  - **Right-click a column's title:** Rename…, Add a card and Delete. Deleting a column with cards asks first.
    Every menu is listed in [context-menus.md](context-menus.md).
  - **Dragging a column's title** onto another column moves it to that column's place.
- **Cards** stack in their column, which scrolls when they don't fit, and shows a
  [scrollbar](#a-scrollbar-in-a-column-kanban-card-78) then.
  - A card shows its labels as small colored bars, its title, a line icon when it has a description, how many
    comments it has ("2 comments"), and its assignee's block title at the right. (Version 2 adds its id, priority
    and checklist: [its face](#a-cards-face).)
  - **+ Add a card** at the bottom of a column opens the [card dialog](#the-open-card) for a new card, titled "New card"
    and selected. Nothing is created until **Done**. While the column holds a [draft](#drafts) the button reads
    "Continue draft" and reopens it.
  - **Dragging a card** onto another card puts it in that card's place; onto a column's empty space, at the end
    of that column. While dragged, the card follows the pointer and the drop target is highlighted.
  - **Clicking a card** opens it.

### An open card

[Version 2](#the-open-card) redoes this dialog; this is the first one.

A dialog in the middle of the window, at the window's scale:

- the title, in a text field;
- the description, in a multi-line text field;
- the six label colors (red, yellow, green, cyan, blue, magenta, from the theme), each a toggle;
- **Assignee**, when an agent claimed the card (`claim_card`, [connections.md](connections.md#tools)): its block's
  title, and a ✕ that frees the card;
- **Comments**, oldest first, each with its author (an agent's block title, or "You") and how long ago it was written;
  a text field under them and **Add comment**, which adds the person's. An agent that finishes its card says there
  what it did and what the next one needs to know (`comment_card`, [connections.md](connections.md#tools));
- **Delete card**, and **Done** to close.

The dialog edits a working copy: nothing it changes reaches the board until Done, except comments (below). See
[the open card](#the-open-card) and [drafts](#drafts).

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
            { "id": 5, "title": "Kanban", "description": "Trello-like", "labels": ["green"],
              "assignee": "b7k2m",
              "comments": [{ "by": "b7k2m", "text": "On it.", "at": 1760000000 }] }
          ] }
        ]
      }
    ]
  }
  ```

  - Ids are unique across boards, columns, cards, checklist items and comments, and never reused (`next_id`). A file
    edited by hand keeps them so: an id given twice gets a new one after its first, and `next_id` passes them all.
  - Missing `description`, `labels` and `comments` mean empty, a missing `assignee` none; a card with no assignee
    or comments writes neither key. An unknown label color is dropped.
  - `assignee` is the block id of the agent that claimed the card. A comment's `by` is an agent's block id, or
    `"you"` for the person; `at` is in Unix seconds.
- **A canvas file** gets a second kind of item: `{ "kind": "kanban", "rect": {…}, "board": 1 }`. `board` is
  missing for a block showing none. A Templum from before this spec sets such a file aside
  ([persistence.md](persistence.md#the-file)), as designed.
- **Saving:** `boards.json` is written by the 5-second snapshot when the boards changed, and on quit, through the
  same temporary file and rename. A file Templum can't read is kept as `boards.json.bak` and the boards start empty.

## Architecture

| File | Change |
|---|---|
| `board.rs` (new) | The model (`Boards`, `Board`, `Column`, `Card`, `Label`) and every edit: add, rename, remove, move. Serde, as `session.rs` does. Pure and unit-tested. |
| `kanban.rs` (new) | The kanban block's body (columns, cards, drag and drop, or no board), the Boards dialog and the card dialog. |
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

- the board button opens a kanban block showing no board, and its **New board** creates a board and shows it;
- the Boards dialog finds a board by a card's title, shows it in the block, and deletes it on a second click;
- the block's menu deletes the board it shows;
- two blocks showing the same board both show a card added through one of them;
- deleting a board leaves its blocks with none;
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
- **Wheel events** over a selected kanban block stay in it, as over a terminal (over any other block the wheel moves
  the canvas: [navigation.md](navigation.md#the-wheel-over-blocks)): a vertical wheel scrolls a column's cards,
  a sideways one the row of columns, and Ctrl+wheel zooms the canvas. GPUI measures a scroll container's range
  from its own children, so the columns are the row's children, with no wrapper in between.

## Version 2 (kanban card 40)

*Implemented (kanban card 40): [docs/plans/2026-10-18-kanban-v2.md](../plans/2026-10-18-kanban-v2.md).* The card, as
the person wrote it:

> aprimorar sistema kanban, tipo quero poder ter tarefas para o claude e tarefas para o codex, delegar tarefas para
> agentes específicos, ou caso tarefa não tiver nenhum agente, qualquer um pode pegar ela, ou sistema de sessão
> conectada, tipo eu posso ter 2 agentes claude mas tipo eu poderia querer que uma task fosse feita por uma sessão
> específica do claude. Também ter sistema de prioridade (low, medium e high), poder ter checklist dentro da tarefa.
> Corrija também quando estou com uma task no drag and drop e arrasto ela deve ir movimentando o kanban, tanto dentro
> da coluna ou entre colunas, para tipo movimentar a tela para scroll automático. Mostrar o ID da task do kanban no
> card e nos detalhes do card. Melhorar visualmente o detalhes da task do kanban, está bem feio e botão espaço errado.

### Goal

The board becomes the person's way to hand work to the agents on the canvas. A card can be for anyone, for any session
of one agent (Claude Code, Codex, any agent Templum knows), or for one session (one of two Claude Code blocks); agents
see which cards are theirs and `claim_card` refuses one meant for another. A card gets a priority and a checklist, and
shows its id. Dragging a card near the edge of a column or of the board scrolls it. The open card is redone, clean and
in Templum's look, with its buttons where they belong.

**Success criteria**

1. Everything below works on Linux, macOS and Windows at zoom 0.25, 0.5, 1 and 2 (the dialog at the window's scale).
2. A `boards.json` from before loads unchanged, and a card that uses none of the new things writes none of their keys.
3. All tests in [Testing](#testing-1) pass in CI on the 3 OS; `--bench` and `--bench-mixed` stay at p95 ≤ 16.7 ms.

### Who a card is for

A card's **assignee** says who does it. It is one field, as before, with one more kind of value:

| Assignee | Means | Shown on the card | Saved as |
|---|---|---|---|
| none | anyone may take it | nothing | no `assignee` key |
| an agent | any session of that agent may take it | "Any Codex" | `"assignee": "agent:codex"` (the agent's id) |
| a block | that block's agent does it: one session | its block's title ("Claude Code · api"), else its id | `"assignee": "b7k2m"` (its block id, as before) |

- **An agent** is one of `Workspace::agents`: Templum's own and the person's `[agents]`, named by id (`claude`,
  `codex`, an `[agents]` key) and shown by title. An account of an agent (`claude2`) is one too; a card for it is
  its own ([accounts.md](accounts.md#cards-for-it)).
- **A block is one session.** A block keeps its id across restarts, Ctrl+Z and a move to another workspace, and its
  agent resumes its own session in it ([agents.md](agents.md#the-agents), "One session per block"), so a card for a
  block stays for that session. A card for a closed block keeps the id; the person changes it.
- **Which agent a block runs:** the one Templum sees running in it (`Pane::seen.agent`, read every 5 s), else the one
  Templum started there (`Pane::flags_for`, set by `spawn_agent` before the first read). A block with neither runs no
  agent Templum can name yet.
- **A block runs "any X"** when its agent is X, or runs the same program as X (the first word of their commands,
  without its folder or `.exe`, in any case): a person's `[agents]` entry `claude-work = "~/bin/claude --settings …"`
  runs `claude`, which Templum sees as Claude Code, so a
  card for any claude-work is taken by a Claude Code block and the other way round. Otherwise a card for such an entry
  could never be taken.
- **Claiming** turns the assignee into the claiming block (as before): a card for any Codex, claimed by a Codex block,
  becomes that block's. Choosing another assignee in the open card replaces it, whoever held it.

#### In the open card

The **Assignee** row ([layout](#the-open-card)) holds a dropdown button showing the current value ("Anyone", "Any
Codex", a block's title), and, when there is one, a ✕ (`unassign`) that sets Anyone. Its menu:

1. **Anyone**.
2. A separator, the label **Sessions**, then one item per terminal block running an agent (as above) and wired to
   a kanban block that shows the board, in every workspace: the active one first, then the others in the sidebar's
   order, each back to front. An item reads the block's title as agents see it (`Workspace::uid_title`), its
   folder's name and its id ("Claude Code · app · k3x9a"), followed by " — {workspace name}" when the block is in
   another workspace. The current block, when it is in none of them (closed, or its agent quit), comes first, by its
   title or its id. The section is left out when it would be empty.
3. A separator and a submenu **Any session of**, listing every agent of `Workspace::agents` in their order, by title.

The current value is checked. Each choice, and the ✕, is one step of the kanban's Ctrl+Z.

#### For agents

Tools ([connections.md](connections.md#cards-for-an-agent)): `list_cards` says each card's assignee and marks the
caller's own with `(you)`; `create_card` and the new `edit_card` take `for`; `claim_card` refuses a card meant for
another agent or another block, saying why, and so does `edit_card`'s `for` (an agent hands out cards nobody holds,
and never gives itself one meant for another); `move_card`, `comment_card`, and `edit_card`'s priority and checklist
stay open to every wired agent (an agent that hands work out, or one that reviews, moves, ticks and comments on others'
cards), and `move_card` says when the card is for someone else.

**Assigning a card tells nobody.** It types nothing into the agent and wires nothing: an agent learns of its cards in
`list_cards` on a kanban wired to it, as before, and the person hands a card over as before (Send to ▸, a delegation,
the prompt).

### Priority

- **None, Low, Medium or High.** Saved as `"priority": "low" | "medium" | "high"`; missing is none; a value Templum
  doesn't know reads as none (a typo mustn't lose the file).
- **On the card:** a signal icon in its meta row ([the face](#a-cards-face)): lucide `signal-low` in `title_dim`,
  `signal-medium` in the theme's yellow (ANSI 3), `signal-high` in its red (ANSI 1).
- **In the open card:** the Priority row's four chips, None, Low, Medium, High; a click sets it, one Ctrl+Z step.
- **Nothing is sorted by it.** A column's order is the person's plan, set by dragging; reordering on its own would
  fight drag and drop and move cards while someone looks at them. Priority is a cue, for the person and for agents.
- **Agents** read it in `list_cards` and set it with `create_card` and `edit_card`.

### A checklist in a card

- **Items** have an id, a text and whether they are done: the same item as a checklist block's
  (`checklists::Item`), read from an agent's text the same way (`checklists::parse_items`: one item per line, a
  leading `- ` or `* ` dropped, `[x]` checks it). A card's items live in the card, in `boards.json`, with ids from the
  boards' `next_id` (unique across boards, columns, cards and items, never reused). A checklist block can't show a
  card's checklist, nor a card a checklist block's: two places for one list would have to be kept in step.
- **Saved** as `"checklist": [{ "id": 41, "text": "Build" }, { "id": 42, "text": "Test", "done": true }]`; missing is
  none; an unchecked item writes no `done`. A hand edit mustn't lose the file: a `checklist` that isn't a list is none,
  an item that doesn't read (`"done": "yes"`) is dropped, and one with no id gets a new one. `labels` that aren't a
  list are none too.
- **On the card:** its progress, "2/5", in the meta row, in the accent color once every item is done; nothing without
  items.
- **In the open card:** its section ([layout](#the-open-card)): a progress bar, one row per item (a check box, its
  text, a ✕ under the pointer that deletes it), and a field that adds what is typed when Enter is pressed (several
  lines pasted make several items) and keeps the dialog open. Checking, adding and deleting are each one Ctrl+Z step.
- **Not in version 2:** editing an item's text in place, or dragging items. Delete it and add it again.
- **Agents** read the items in `list_cards` with `card`, add them with `create_card`'s `checklist` and `edit_card`'s
  `add_items`, and check or uncheck them with `edit_card`.

### The card's id

- **On the card:** "#40", first in its meta row, which every card now has.
- **In the open card:** its title, "Card #40".
- Agents already name cards by it.

### Scrolling while dragging

While a card is dragged, a container under it scrolls when the pointer is near its edge:

- **A column's cards** scroll up or down when the pointer is near the top or bottom of the column's card list, and
  over the column sideways (another column's pointer never scrolls this one).
- **The board's columns** scroll sideways when the pointer is near the left or right of the block's body, and within
  its height. A column dragged by its title scrolls them too.
- **How fast:** the canvas's own edge rule for a marquee (`templum_canvas::edge_scroll`: a 40-unit zone, faster the
  nearer the edge, 900 a second at the edge or past it), measured in **world units**: zone and speed scale with the
  zoom like everything in the block, so the board moves the same at zoom 0.25 and 2 (only resizing changes a board's
  layout, never the zoom). The pointer may leave the container on that side, by up to
  one zone: full speed; further off it stops, and over another block at once, since every container hears every
  drag.
- It runs each frame while the pointer holds still (a frame asked for while it moves), from the time since the last
  frame, at most 0.05 s, so a stalled frame doesn't jump; it stops at the ends and when the drag ends or the pointer
  leaves the zone. Drop targets follow what scrolled under the pointer.

### A card's face

World units, each × the zoom `z` (as now: padding 10 × 8, gap 6, radius 8, the hairline border, the light shadow):

1. **Labels** (when it has any): unchanged, bars 32 × 6, gap 4.
2. **Title:** unchanged, 13.
3. **Meta row**, always: items centered, gap 8, text 11 in `title_dim`, in this order:
   - `#40` (element `card-id-{block}-{card}`);
   - the priority's signal icon, 12 (`priority-{block}-{card}`), when it has one;
   - the checklist: lucide `list-checks` 12 and "2/5", gap 3 (`checklist-{block}-{card}`), accent once all are done,
     when it has items;
   - the description's `file-text` icon, 12, as now, when it has one;
   - the comments: lucide `message-square` 12 and their number, gap 3 (`comments-{block}-{card}`, as now), when it has
     any. It replaces "2 comments", which no longer fits beside the rest;
   - at the right (`ml_auto`, truncated): the assignee as the table above shows it (`assignee-{block}-{card}`).

### The open card

A dialog at the window's scale (z = 1), **600 wide**, in the Git popup's language: a block's own color (`ui.surface`) under
hairlines, rounded 8, the theme's accent for what is lit. Its body scrolls past the window's height (gpui-kit does: the
header and the footer stay). Sizes in pixels; every color comes from the theme.

- **Header** (20 at the sides, a hairline under it): the card's number as a faint accent badge (`card-number`: the
  kanban icon and "Card #40", 12 semibold, `ui.accent` on `ui.accent` at 16%, rounded 6), then "In {column} on {board}"
  (12, `ui.title`, truncated), read each frame, so an agent's move shows.
- **Body**, a column with a gap of 22 between these blocks:
  1. **Title:** the single-line field (`card-title`), 15 high text in a 38 high field. Enter closes the dialog.
  2. **Properties** (`card-properties`): a faint panel (`ui.card`, rounded 8, 12 sideways, 6 up and down); each row at
     least 32 high, an icon (14, `title_dim`) and a label (12, `ui.title`) in 92, then its control:
     - **Assignee:** the dropdown button (gpui-kit `Button`, small, outline, with a caret; `card-assignee`) and, when
       set, the ✕ (the Git popup's 24 × 24 icon button, tooltip "Give it to anyone", `unassign`).
     - **Priority:** one strip (hairline border, rounded 8, 2 inside) of four segments (`priority-none`, `-low`,
       `-medium`, `-high`): 24 high, padding 10, rounded 6, text 12, a gap of 5 between the icon (12, in its color;
       None has none) and the word. The current one: `ui.accent` at 16% fill, `ui.accent` text, medium weight; the
       others: `ui.title` text, `ui.hover` fill under the pointer.
     - **Labels:** the six as round swatches (22, gap 8, `label-{name}`): a 2 ring of the label's ANSI color; the
       card's own are filled with it and checked (the terminal's background color for the check). A tooltip says
       "Toggle the red label".
  3. **Description:** a section header (below), 8, the multi-line field (4 rows, placeholder "Add a description").
  4. **Attachments** (card 152, [below](#attachments-kanban-card-152)): the section header (`paperclip`) with their
     number and **Attach** at the right, then thumbnails and rows; with no data folder it is not drawn.
  5. **Checklist:** the section header with "2/5" at the right when it has items; 8; a progress bar 4 high, rounded,
     `ui.hover` behind and `ui.accent` for the done part; the item rows (padding 6 sideways and 5 up and down, rounded
     6, `ui.hover` under the pointer, gap 10, items at the top): the Git popup's round box (16, filled with `ui.accent`
     and a check in `ui.surface` when done; `card-item-box-{item}`), the text at 13 (done: `title_dim`, struck
     through), and at the right an 18 × 18 ✕ (tooltip "Remove the item", `card-item-delete-{item}`) shown while the
     pointer is over its row; last, the Git popup's one-line field with a `plus` before the placeholder "Add an item"
     (`card-item-field`).
  6. **Comments:** the section header with their number at the right; a gap of 10; the thread; the comment field.
     - **The thread**, oldest first, a gap of 14: each comment is its author's round mark (26: the initial on the
       theme's accent at 16% for the person's own, on ANSI magenta at 16% for an agent's or a block's), then the author
       (12.5 semibold) and its age (11) over the text in a bubble (13, line height 1.5, rounded 8, 12 and 8 inside;
       `ui.card`, and the accent at 10% for the person's own). With none: a dashed hairline box with "No comments
       yet". While the pointer is over a comment, two small icon buttons show at the right end of its first line:
       edit (`pencil`) on the person's own and delete (`trash`) on any ([below](#the-person-edits-and-deletes-a-comment-kanban-card-83)).
     - **The comment field** (`comment-field` is its text): a box (hairline border, `ui.card`, rounded 8) holding the
       2-row field (placeholder "Write a comment") and, at the bottom right, **Comment** (`add-comment`, `arrow-up`
       icon, 26 high): the solid accent once something is typed, a faint `ui.hover` while the field is empty.
  The **section header** is an icon (13, `title_dim`), the name (12, medium, `ui.title`) and what the section holds.
- **Footer** (a hairline over it, 20 at the sides, 12 up and down), one row spread across: at the left **Delete card**
  (`delete-card`), a ghost button: no fill, gpui-kit's `trash` icon at 14 and the words at 12.5, both in the theme's
  red (ANSI 1), 30 high, rounded 8, red at 12% under the pointer; it deletes the card and closes the dialog, as before
  (Ctrl+Z brings it back). On a card that doesn't exist yet it reads **Discard** (`discard-card`) and throws the work
  away, the column's draft with it. At the right **Done** (`card-done`), the Git popup's solid accent button (30 high,
  rounded 8, `ui.surface` text), which closes it and applies the work. Enter in the title is Done. Esc and a press
  outside the dialog (card 116) close it when nothing changed since it opened; else they open a question, **Save
  your changes?**, with **Save** (Done), **Save draft** and **Discard** (`leave-save`, `leave-save-draft`,
  `leave-discard`); Esc in the question goes back to the card, with a comment edit in the middle still open. Save and
Save draft take that edit in; Discard drops it. On a new card an edit with text counts as a change.

**Done is one Ctrl+Z step**, and applies only what the person changed: the dialog keeps the card as it was opened
and as the person made it, and Done sets the fields that differ, turns on or off the labels they toggled, and adds,
removes or ticks the items they did. What an agent changed meanwhile (a priority the person didn't touch, an item
it ticked or added, a move, a comment) stays, and the dialog shows it at once. On a new card Done adds it at the end
of its column with everything typed. A click in the dialog acts on what it shows: before it changes the work, the
work is rebased on the card as it is now (the person's changes stay over it), so unticking an item an agent just
ticked, freeing a card it just claimed, or turning off a label or priority it just set does what the click says.
A window closed with the dialog open keeps what was typed as a draft. A card deleted meanwhile is not brought back: Done closes the dialog and
changes nothing.

**Comments are sent at once**, like chat: Comment on a card that exists posts it, and editing or deleting one (with
its Undo row) works as before, each its own step. On a new card the comments typed stay in the dialog, with the
card, and are posted when Done creates it.

### Drafts

A draft is a card dialog's work, kept aside and not published. **Save draft**, in the question above, keeps it.

- **A new card's** draft does not appear on the board. There is one per column; its "Add a card" reads "Continue
  draft" (the column's menu says so too) and reopens the dialog with what was typed (comments too).
- **An existing card's** draft keeps its pending edits. Its face shows a small pencil (`draft-{block}-{card}`,
  `title_dim`, tooltip "Unsaved draft") in the meta row. Agents (the tools, `templum kanban`) and other blocks keep
  seeing the published card until Done. Reopening the card shows the draft's changes over the card as it is now.
- **Discard** (the question, or a new card's footer button) throws away what was typed and the kept draft with it.
  Leaving a reopened draft untouched keeps it. Done drops it. A draft whose card or column is gone is dropped.
- Saving or dropping a draft is not a board edit: no Ctrl+Z step (Ctrl+Z keeps the drafts kept since), but it is
  saved like any board change.

Version 2 first drew the dialog in gpui-kit's own colors (the canvas's color, a white primary button, a row of dim
bare swatches, comments as bare lines under a bare field). The redesign changes no behavior: the same ids, the same
undo steps.

### Agents' tools

[connections.md](connections.md#cards-for-an-agent) has the tools' arguments and answers. In short: `list_cards` shows
each card's priority, assignee (`(you)` on the caller's) and checklist progress, and with `card` its items;
`create_card` takes `priority`, `for` and `checklist`; `edit_card` (new) sets `priority` and `for`, adds items and
checks or unchecks them; `claim_card` follows [who a card is for](#who-a-card-is-for). The command line's
`templum kanban add` and `templum kanban edit` do the same.

### Files

`boards.json` keeps `"version": 1`: older Templums read the new files (serde skips keys it doesn't know). A card may
now hold:

```json
{ "id": 40, "title": "Kanban v2", "priority": "high", "assignee": "agent:codex",
  "checklist": [{ "id": 41, "text": "Model" }, { "id": 42, "text": "Tools", "done": true }],
  "comments": [] }
```

A draft is kept in the file's `drafts` (only written while there are any, so older files and older Templums read
it): each is `{ "at": 40, "base": {card}, "work": {card} }`, `at` the card it edits, or, with `"new": true`, the
column a new card is for (ids are unique, so one key does for both). `base` is the card as the dialog opened it and
`work` as the person left it; Done sets what differs. An existing card's draft carries no comments (they are
on the board); a new card's carries the ones typed. An older Templum drops `drafts` on its next save.

An older Templum shows `agent:codex` as an assignee it can't find, and refuses claims on it. It reads the file, but
its next save drops every priority and checklist (it keeps only the keys it knows): going back to it, then forward
again, loses them.

### Calls made for card 40

The person was away; each can be changed.

1. **One field for who does a card.** "For any Codex" and "claimed by this block" are the same `assignee`, so claiming
   keeps working as before and there is no second rule to explain; a claimed card forgets that it was for any Codex
   (the person sees who holds it, and can set it back).
2. **A block is a session.** No session ids on cards: the block id already outlives restarts, and its agent resumes
   its session there.
3. **`move_card`, `comment_card` and `edit_card`'s priority and items aren't gated, its `for` is:** agents that hand out or review work need them on others' cards,
   and the person's Ctrl+Z undoes a wrong move. `move_card`'s answer says when the card is someone else's. Without a
   gate on `for`, an agent refused by `claim_card` could set `for` to itself, or free a card another block holds.
10. **Assigning tells nobody** (above): delivering a card to its agent (typing it, wiring the block) is a flow of its
   own, and the existing ones still work.
11. **Same program, same agent** (above), so no entry of the menu is a card nobody can take.
4. **Priority sorts nothing** (above).
5. **A card's checklist is its own**, sharing only the item type and the parsing with checklist blocks.
6. **One new tool, `edit_card`,** rather than three (priority, who, items): each tool costs every agent context.
7. **Scrolling speed in world units**, from the canvas's edge rule, rather than a second set of constants.
8. **The meta row on every card**, for the id: a bare card grows by one line; ids in the same place on every card
   read better than an id at the start of each title.
9. **The assignee menu's agents in a submenu:** Templum knows 20 and more agents; the sessions running now are what
   the person picks most.
12. **`templum kanban edit`'s `--add` may come more than once**, an item each, like `--check` and `--uncheck`.
13. **A dialog's own Ctrl+Z is the board's:** what the dialog changes (a chip, a box, an item, the menu) is one step
    of the kanban's Ctrl+Z, which keeps `next_id` (an undone item's id is never given out again).
14. **Several lines pasted into "Add an item" are added at once,** an item each: the field is one line, so they
    can't wait there for Enter.
15. **Past an edge, one zone at most, and never over another block:** a drag further off, or over a block beside
    (tiles 8 apart), scrolls nothing, so a card dragged in one block never scrolls another block's board.

### Considered and left out

- **A separate "for" beside the assignee:** two fields to keep consistent (a card for Codex held by a Claude block?).
- **Gating every tool on the assignee:** an agent handing out work couldn't move the cards it handed out.
- **Showing a checklist block's list in a card** (a card naming a `checklists.json` id): deleting it in the
  Checklists dialog would empty a card, and Ctrl+Z would have two stores to step back.
- **Columns sorted by priority, a filter "cards for me" in the block:** nobody asked; the badge and `(you)` cover it.
- **Send to ▸ assigning the card** to the terminal it types into: the person also sends a card to an agent just to
  read it.

### Testing

Tests are written first, as always.

**Unit (`board`):** a card's priority, checklist and an agent assignee read back equal and write the keys above; a
card from before reads with none (`"assignee": "b7k2m"` is a block) and writes none; an unknown priority reads as
none; items get ids never given before, check, uncheck and go; a file whose item ids pass `next_id` loads with
`next_id` after them.

**Unit (`kanban`):** the scroll speed is zero in the middle and across another column, grows toward an edge, is full
past it, and its zone doubles in pixels at zoom 2.

**Unit (`templum-mcp`):** `create_card` and `edit_card` parse their new arguments (a priority in either case, `check`
as one id or a list), refuse an unknown priority and an `edit_card` with nothing to change; the command line's verbs
build them.

**Headless UI:**

- a card shows its id always, and its priority and checklist progress when it has them;
- the open card sets a priority (one Ctrl+Z step), adds an item with Enter and stays open, checks and deletes it,
  frees its assignee, deletes the card from the footer, and closes with Done;
- the assignee menu offers Anyone, the agent sessions wired to the board in every workspace, and every agent;
- dragging a card near a long column's bottom scrolls its cards; near the board's right edge, its columns;
- the tools: `list_cards` shows priority, assignee, `(you)` and progress, and the items with `card`; `create_card` and
  `edit_card` set them; `claim_card` takes a card for anyone or for its agent (or an agent running the same program),
  refuses one for another agent or block, and says when it can't tell which agent it is; `edit_card`'s `for` refuses
  the same; `move_card` says when the card is someone else's.

**Manual (Hyprland):** drag cards to every edge at zoom 0.5, 1 and 2, in a long column and a wide board; the open
card in light and dark themes, a long checklist and many comments (the body scrolls, the footer stays); the assignee
menu with two Claude Code blocks and a Codex one, one of them in another workspace.

### Risks

- **A menu inside a dialog:** gpui-kit's dropdown opens over the dialog; if a click on its item reaches the dialog's
  overlay, the menu is the first thing to check by hand. The ✕ and the tools still set the assignee.
- **Enter in the "Add an item" field:** gpui-kit's single-line input propagates Enter, which confirms (closes) the
  dialog. The field's wrapper takes the dialog's `Confirm` action so Enter only adds; a test holds it.
- **Scrolling and drop targets:** GPUI highlights drop targets from the last frame's hit boxes; scrolling moves them
  every frame, so the highlight may trail by a frame. Harmless.

## Editing and deleting a comment (kanban card 77)

The card, as the person wrote it: *"Permita MCP editar/excluir comentário nos cards do kanban."* An agent could add a
comment (`comment_card`) but not fix a wrong one.

- **Tools:** `edit_comment` (`card`, `comment`, `text`) and `delete_comment` (`card`, `comment`)
  ([connections.md](connections.md#tools)); the command line has `templum kanban edit-comment` and `delete-comment`.
- **A comment is named by its number**, an id from the boards' `next_id` (unique across boards, columns, cards,
  checklist items and comments, never reused), like a checklist item's. A number rather than a place in the thread
  because the thread changes: the person or another agent deleting the third comment would make "comment 4" mean
  another one. `list_cards` with `card` shows it (`comment 51 by …`) and `comment_card` says the number of the one it
  just added.
- **Saved** as `"comments": [{ "id": 51, "by": "b7k2m", "text": "On it.", "at": 1760000000, "edited": true }]`. A
  comment with no `id` (every file from before) gets a new one when the file loads, in the file's order; `edited`
  is missing for a comment its author never changed, and nothing else in the file changes, so an older Templum reads it
  and a file from before loads unchanged.
- **Whose to change:** an agent changes only a comment whose `by` is its own block id. The person's (`"you"`) and
  other agents' are refused, saying whose they are. The person's own changes, in the card's dialog, are
  [card 83's](#the-person-edits-and-deletes-a-comment-kanban-card-83).
- **A step of Ctrl+Z each:** the kanban's, like a comment added.
- **The open card** shows the change at once: it reads the card each frame (with the person's unsaved changes over it). An edited comment keeps its author,
  its place in the thread and its time, and its age reads "3 min ago · edited" so the person can tell that what they
  read is not what was first said.

### Calls made for card 77

1. **Two tools, not one:** `edit_comment` and `delete_comment` follow `edit_item` and `delete_item`; one tool with an
   optional `text` would delete a comment on an empty text.
2. **An id, not a number in the thread:** see above; the saved file gains one key per comment.
3. **Only the author:** a wrong comment of the person's or of another agent's is for the person to fix; the answer
   says whose it is.
4. **"edited" is shown,** because it is one word in the age and an agent rewriting what was said should not pass
   unseen.
5. **Not in the Templum Agent's tools,** which have no `edit_item` and `delete_item` either.

### Testing

**Unit (`board`):** comments get ids; editing changes the text in place, marks it edited, and leaves author and time;
the same text is no edit; deleting; a file from before gives its comments ids that clash with nothing and writes no
`edited`. **Unit (`templum-mcp`):** the two tools parse `comment` as a number or a string and name what is missing; the
command line's verbs build them. **Headless UI:** an agent edits and deletes its own comment, is refused the person's
and another agent's (nothing changes, no Ctrl+Z step), the open card shows each change at once, and Ctrl+Z undoes the
delete, then the edit.

## A scrollbar in a column (kanban card 78)

The card, as the person wrote it: *"Kanban scroll: nas colunas do kanban, quando os itens passarem da tela, crie uma
barra de scroll para facilitar a movimentação."* A column's cards already scrolled (wheel, and a card dragged near
the edge), but nothing showed that there were more, or let the pointer take hold of them.

- **The scrollbar** is gpui-component's, the one the notes' editors and the menus draw, over the right edge of a
  column's list of cards (the header and "+ Add a card" stay out of it). It appears as the theme says (on hover over
  the track, or while scrolling, following the system) and only once the cards overflow. Dragging its thumb scrolls the
  column; a press on the track jumps there.
- **It scales with the zoom,** like everything in the block: its track (16 units at zoom 1) and thumb (6, 8 under the
  pointer) are multiplied by the zoom, so at 0.25 it is as thin, against the column, as at 2. Unscaled, its track would
  take a quarter of a 260-wide column at 0.25. Zoom changes no layout.
- **It sits over the list, not in it:** the list scrolls inside a box of its own and the scrollbar is drawn over that
  box, so it doesn't scroll away with the cards and takes no room from them. While it shows, a press in its track (the
  right 16 units of the list) belongs to it and not to the card under it. Only the left button, though: a right-click
  on a card there still opens the card's menu and a middle drag still pans the canvas, and any press in the list picks
  its block (with "Focus follows the pointer" off, a scrolled kanban would otherwise stay unpicked and the wheel would
  pan the canvas). The scrollbar takes every press in the bubble phase and stops it, so the list picks the block, and
  pans for the middle button, in the capture phase, and a card opens its menu there too.
- **The wheel is unchanged:** over a column of a picked block a vertical wheel scrolls its cards, over any other block
  it moves the canvas like over every block ([navigation.md](navigation.md#the-wheel-over-blocks), `route_wheel`).
  The card asked for the scrollbar; a kanban that took the wheel without being picked would be the one block that
  does.
- **Dragging a card** is as before: the scrollbar draws no hit box that blocks the cards under it, and a drag near a
  column's top or bottom scrolls it, scrolled or not, with the pointer over the strip or not.

### Testing

**Headless UI:** a column with 30 cards has a scrollbar over exactly its list, and a press on its track near the
bottom scrolls the cards; at zoom 0.5 the track is 8 pixels, so a press 3 pixels inside the edge scrolls and one 12 pixels
inside (a card) doesn't; a card held at the strip of a column scrolled to its end scrolls it back up; a right-click on
a card at the strip opens its menu, a middle drag from there pans the canvas, a press there picks an unpicked block
(focus following the pointer off), and the wheel scrolls the cards over the list and over the strip. **Not testable
headlessly:** dropping a card (no `on_drop` fires in the harness, [minors.md](../minors.md)), so a drop on the strip is
checked by hand. **Bench:** `--bench-mixed` still draws its kanban blocks with the scrollbar, and costs no more than without it
(release, on the dev machine with the person's Templum running: p95 31.4 ms with the scrollbar, 32.5 ms without, p50 19.1
and 18.3, so both over the 16.7 bar there: the machine's, not the scrollbar's).

**Manual (Hyprland):** a long column at zoom 0.5, 1 and 2: the scrollbar's thumb follows the wheel and is dragged; a
card dropped on a long column's right edge lands in it.

## The person edits and deletes a comment (kanban card 83)

The card, as the person wrote it: *"Permitir usuário excluir/editar um comentário no card do kanban."* Agents already
fix and remove their own comments ([card 77](#editing-and-deleting-a-comment-kanban-card-77)); the person could only
add one.

- **Which comments:** the person **deletes any** comment (the board is theirs, as the cards are, and Undo brings it
  back) and **edits only their own** ("You"): an agent's words changed by someone else would still wear its name; to
  correct one, delete it and write another.
- **The buttons** show at the right end of a comment's first line while the pointer is over the comment: `pencil`
  (`comment-edit-{id}`, tooltip "Edit the comment") on the person's own and `trash` (`comment-delete-{id}`, "Delete
  the comment") on every one: 22 × 22, rounded 5, the icon 12 in `title_dim`, `ui.hover` under the pointer, as the
  checklist item's ✕. They are laid over the line's end, so showing them moves nothing.
- **Delete** is at once, with no question, as an item's ✕ and **Delete card** are in this dialog; it is a step of the
  kanban's Ctrl+Z (taken once the dialog is closed: with it open, Ctrl+Z goes to the field that has the keyboard, and
  afterwards it would undo every later change of the boards with it). So a row stands **in the comment's place** until
  the dialog closes: a dashed hairline box, `trash`, "Comment deleted", "·" and **Undo** (`comment-deleted-{id}`,
  `comment-undo-{id}`; the accent, semibold). Undo puts the same comment back (its number, author, text, time and
  edited flag, `Boards::restore_comment`) at its place in the thread, or at the end when the thread is shorter now, as
  one more step of Ctrl+Z (which takes it away again). Several deletes leave several rows, each at its own place; a
  comment that is in the thread again (Ctrl+Z, an agent) drops its row, and closing the dialog drops them all.
- **Edit** is in place: the comment's bubble becomes a box (a hairline in the accent at 60%, `ui.card`, rounded 8) with
  its text in a field (`comment-edit-field-{id}`, 2 rows growing to 8, the caret after the last word) and, at the
  bottom right, **Cancel** (a ghost button, `comment-cancel-{id}`) and **Save** (`comment-save-{id}`, the accent with
  a `check`; a faint `ui.hover` fill while the text is empty, as Comment is). **Enter** saves (Shift+Enter adds a line)
  and **Escape** or Cancel ends the edit and changes nothing; Escape ends only the edit, not the dialog, and Enter only
  saves, not confirms it (the field's wrapper takes the dialog's cancel and confirm). One comment is edited at a
  time: starting another, or closing the dialog (Done, Enter in the title, Esc or a press outside it), **saves the draft** of
  the first if it holds text, and drops an empty one; a comment gone meanwhile (an agent, Ctrl+Z) ends its edit. When
  it ends, the keyboard goes to the comment field.
- **A comment needs text:** Save does nothing while the field is empty (the trash is how to remove one). The text is
  trimmed at both ends, as a new comment's is. Saving the same words changes nothing: no "edited", no step of Ctrl+Z.
- **The model is card 77's:** `Boards::edit_comment` and `remove_comment`; an edited comment keeps its author, its
  place and its time and reads "3 min ago · edited" ("edited" for the person's too: what was first said is gone),
  and each save and each delete is one `edit_boards` step.

### Calls made for card 83

1. **Delete any, edit only your own,** the card's own suggestion: a board's person may remove what an agent said
   there, but changing its words would misattribute them.
2. **No question before a delete, but an Undo row:** the dialog's other deletes (an item, the card) have no question
   and a question in a dialog is a dialog in a dialog; yet the trash is 22 px, hover-only and 2 px from the pencil, and
   Ctrl+Z with the dialog open goes to the field that has the keyboard (and, once closed, undoes everything after the
   delete too: an agent's moves and comments, the person's typing). The row is the way back that touches nothing else.
3. **The same field as the new comment's,** a `Textarea`, not a single line: a comment may hold several lines, which
   Shift+Enter writes.
4. **Escape through the dialog's cancel:** GPUI runs key bindings before key-down listeners, so a listener on the
   field can't stop the Escape that closes the dialog; the field hands it on as the dialog's `Cancel` action, which
   its wrapper takes.

### Testing

**Headless UI:** the person's comment has Edit and Delete and an agent's only Delete; deleting the agent's takes it
out of the open card at once and one Ctrl+Z brings it back; a deleted comment leaves a row with Undo where it was
(above the next comment), Undo restores it whole and once, one Ctrl+Z takes the restore away, and the row is gone
when the dialog is closed and opened again; editing the person's makes the text a field with Save and
Cancel, typing and Save change the text, mark it edited and keep its author and time, and one Ctrl+Z undoes it;
Escape and Cancel change nothing and Escape leaves the dialog open; Enter saves and leaves it open; saving the same
words takes no step (one Ctrl+Z undoes the edit before); an empty field can't be saved; the text is trimmed at both
ends and Shift+Enter adds a line; another comment's pencil, and Done, save a draft with text and drop an empty one;
a dialog closed in the middle of an edit lets go of the edit's field; a comment removed meanwhile
ends its edit. **Not seen in a window:** the buttons' look on hover and the editor's box ([minors.md](../minors.md)).

## A press outside closes the open card (kanban card 116)

A press outside the open card closes it when nothing changed since it opened, and else asks what to do with the
changes ([the open card](#the-open-card)); an edit's draft is saved either way (the dialog's `on_cancel`; Done, Enter
in the title and Esc save it too). A press inside it or on its own menus
(the assignee's dropdown and submenu) keeps it open, and so does a drag that starts inside (selecting a field's text)
and ends outside: only a press closes it, not a release.

The card is gpui-kit's `Dialog`, no longer its `AlertDialog`: an alert never closes from a press on its overlay (its
`overlay_closable` does nothing), a dialog does by default. Its ✕ is turned off: Done is the way out it shows. A press
outside while the assignee's menu is open closes both.

### Testing

**Headless UI:** with a typed title and a comment's draft, a press on the card's number and a drag from it to the
window's edge keep the dialog open; a press at the window's edge asks, Save keeps what was typed in the title, and the
draft is saved; a session chosen in the assignee menu keeps the dialog open (card 40's test).

### Testing the card dialog's work and drafts

`crates/templum/src/workspace/tests/card_draft.rs` (headless UI) and `board.rs`: Add a card and Esc with nothing typed
creates nothing and keeps no draft; a title, an item and a comment typed, then Done, make one card with all of them,
and one Ctrl+Z removes it; Esc with changes asks, Esc in the question goes back, Discard leaves the card, Save draft
leaves the board as agents read it and puts the mark on the face, reopening and Done applies the draft, Save is Done;
a new card's Save draft is not a card and "Continue draft" reopens it; an agent's change to what the person left
alone survives Done; Done on a deleted card closes without bringing it back; the draft round-trips `boards.json` and
an older file without `drafts` loads. **Not seen in a window:** the question's look and the pencil mark.

## Attachments (kanban card 152)

*Implemented ([As built](#as-built-card-152)); plan:
[docs/plans/2026-10-24-card-attachments.md](../plans/2026-10-24-card-attachments.md).* The card, as
the person wrote it: *"Sistema de anexar arquivos, imagens etc no card do kanban."* A card held only words: the
screenshot of a bug, a spec's PDF or a log lived somewhere else, and an agent handed the card never saw them.

### Intent

A card carries the files its work needs. The person attaches them in the open card (a button, a drop, a pasted
screenshot); Templum keeps its own copy, so the card still has the file when the original moves or is deleted; the
card's face says how many it has; and an agent that reads the card gets each file's path and reads it there (Claude
Code reads images).

### Goals

1. Any file can be attached, up to 50 MB each: images, PDFs, text, anything.
2. Three ways in, all in the open card: **Attach** (the system's file picker, several files at once), files dropped
   from the system onto the open card, and a pasted image (a screenshot) while the clipboard holds an image and no
   text.
3. The open card shows them: images as small thumbnails, other files as rows (icon, name, size); a click opens one
   in the system's default app; each has a remove button. The card's face shows a paperclip and their count.
4. They are part of the dialog's work: they reach the board at Done like the other fields, and Save draft keeps them.
5. Nothing is left behind for good: what a discarded draft, a removed attachment or a deleted card leaves on disk is
   deleted at the next start.
6. `list_cards` with `card`, and `templum kanban list --card`, list each one's name, size and absolute path.

### Non-goals

- A tool for agents to add or remove attachments.
- Attaching without opening the card (a drop on a card's face), or pasting files copied in a file manager (the
  clipboard's list of files): a drop on the open card covers both.
- Previews past a thumbnail: the system's app opens the file. Renaming or reordering attachments.
- A folder attached whole: it is refused (zip it first).
- One file shared by two cards: each card keeps its own copy.

### Anti-goals

- A file outside the card's folder opened, listed or deleted, whatever `boards.json` says.
- A file deleted while something can still bring it back to a card: Ctrl+Z of a delete or of a Done, a kept draft.
- Painting that waits on a copy: files are copied off the UI thread.
- A new dependency: GPUI has the picker, the file drop, the clipboard's image, the image element and the system's
  opener.

### What a person sees

- **The open card's Attachments section**, between Description and Checklist: the section header (lucide `paperclip`,
  "Attachments", their number at the right) and, at its right end, **Attach** (`attach-files`, gpui-kit's small ghost
  button with `plus`). Under it, its box (`card-attachments`):
  - **Images** first, as thumbnails in a wrapping row (gap 8): 72 × 72, the image cropped to fill
    (`ObjectFit::Cover`), rounded 6, a hairline border, a tooltip with the name and size. An image is a file whose
    name ends in `png`, `jpg`, `jpeg`, `gif`, `webp` or `bmp`, in any case, of 10 MB or less (designer's call: GPUI
    decodes it whole; a bigger one is a row). One that doesn't decode shows the `image` icon on `ui.card`.
  - **Other files** under them, a row each (a checklist item's padding, rounding and hover): lucide `file` 14 in
    `title_dim`, the name (13, truncated at its end), its size at the right (11, `title_dim`: "12 B", "47 KB",
    "1.2 MB", 1 KB being 1024 bytes).
  - **Remove:** an 18 × 18 ✕ (`attachment-remove-{name}`, tooltip "Remove the attachment"), shown while the pointer is
    over the row, or at a thumbnail's top right on a `ui.surface` disc. It takes the attachment out of the work.
  - **A click** on a row or a thumbnail (`attachment-{name}`) opens the file with the system's default app
    (`cx.open_with_system`).
  - **Copying:** a file whose copy hasn't ended shows as a row with its name and "Copying…" for its size
    (`attachment-copying-{name}`), with no ✕ and no click.
  - **None yet:** a dashed hairline box with `paperclip` and "Drop files here or paste a screenshot", as "No comments
    yet" is drawn.
  - **Files dragged over the open card:** its body shows a 2-wide accent ring until they leave or drop.
- **The face:** lucide `paperclip` 12 and the count, gap 3 (`attachments-{block}-{card}`), in the meta row after the
  description's icon and before the draft's pencil, when the card has any.
- **A file refused** (over 50 MB, a folder, unreadable): a warning notification, one for the files of one action:
  "Not attached: video.mov (over 50 MB), build (a folder)."
- **No data folder** (a benchmark, a second Templum): no Attachments section; a drop or a paste attaches nothing.

### Ways to attach

- **Attach:** `cx.prompt_for_paths` for files, not folders, several at once (as `space_dialog.rs` asks for one).
  Cancel attaches nothing; a picker that fails to open says so, as there. Files picked after the card closed (a
  picker that isn't modal) attach nothing.
- **A drop:** GPUI turns the platform's file drop into a drag of `ExternalPaths` (gpui-fast's `window.rs`), on Linux
  (X11 and Wayland), macOS and Windows alike; the dialog's body takes it with `drag_over::<ExternalPaths>` and
  `on_drop::<ExternalPaths>`.
- **A paste:** Ctrl+V (Cmd+V) while a field of the open card has the keyboard (the title has it when the card opens).
  When the clipboard holds an image (`ClipboardEntry::Image`) and no text that isn't blank, the image is attached as
  `Pasted image.<its format's extension>` and the field gets nothing. With text on it, even beside an image (some
  apps put both), the field pastes the text as it does today and nothing is attached. The dialog's body takes
  gpui-kit's input `Paste` action in the capture phase, ahead of any field's own paste handler (the item field's).
- All three end in one function: the copies run in the background, then each lands in the work.

### Files

- **The folder:** `<data folder>/attachments/<card id>/<name>`, one folder per card and the file under its own name,
  so the system's app knows it by its extension and an agent reads a name that means something.
- **A name taken** in the card's folder gets " (2)", " (3)"… before its extension (`report (2).pdf`,
  `Makefile (2)`). It is taken on disk (`create_new`), so two copies at once never share one, and a removed file
  still on disk keeps its name until a start deletes it (undoing its removal finds it).
- **A new card** has no id until Done: its first attachment takes one from the boards' `next_id`, set in its draft's
  `base` and `work` alike (so the id alone is no change), and Done gives the card that id instead of a new one. A new
  card with no attachment is created as today.
- **The copy** runs on the background executor: the size read first (over 50 MB is refused before a byte is copied),
  the name taken, the bytes copied (`io::copy`), the file removed if the copy fails. A pasted image's bytes are
  written the same way. Only then does the attachment join the work, with the size copied.
- **`boards.json`:** a card may hold `"attachments": [{ "name": "screenshot.png", "size": 48213 }]`, in the order
  attached. Missing is none and none writes no key; `version` stays 1 (an older Templum skips the key). An entry that
  doesn't read is dropped, as a checklist item's is, and so is one whose `name` isn't a plain file name (empty, `.`,
  `..`, or holding `/`, `\` or NUL): a hand edit can't make a click open, or a start delete, a file outside the
  card's folder. A draft's `base` and `work` carry them like any field.
- **What is deleted, and when** (designer's call): nothing when it leaves a card. At start, right after `boards.json`
  is read cleanly (`Loaded::Saved`), on the UI thread before any dialog can open, Templum deletes in
  `<data folder>/attachments/` every entry no card and no kept draft names: the folders of cards that are gone, the
  files a card no longer lists, a copy a crash cut short. A link there is removed as a link, never followed. Ctrl+Z
  lives only in a session, so at start nothing can bring them back; until then, a Ctrl+Z of a Delete card, of a
  column's or a board's delete, or of a Done that removed an attachment finds the file where it was. When
  `boards.json` was set aside, unreadable or missing, nothing is deleted.
- **Going back to an older Templum** drops the attachments of the cards it saves (it keeps only the keys it knows, as
  it does priorities and checklists), and the next start of this one deletes their files.

### The dialog's work

- Attaching and removing change the work (`Draft::work.attachments`), never the board: Done applies them with the
  rest as one step of Ctrl+Z, Discard drops them, Save draft keeps them. A copy still running counts as a change, so
  Esc asks as it does for any.
- **Done applies what the person changed** (`Draft::merge_into`): attachments in `work` and not in `base` are added
  to the card unless it has them, ones in `base` and not in `work` are removed, by name (unique in a card's folder);
  the others stay as the card has them.
- **A copy that ends after the dialog closed** (designer's call) goes where the dialog's work went: into the dialog
  while it is still open on that work; else into the kept draft of that card (Save draft, or the window closing);
  else, after Done, onto the card, as a step of Ctrl+Z of its own; after Discard or Delete card, nowhere (a start
  deletes its file). Done never waits for a copy.

### For agents

- `list_cards`: a card's line adds "1 attachment" or "3 attachments" after its comments. With `card`, an
  `attachments:` part after the checklist, a line each, the path whole (`<data folder>` written out), so the agent
  reads the file there:

  ```
  attachments:
    screenshot.png  47 KB  <data folder>/attachments/152/screenshot.png
  ```

- `templum kanban list --card <card>` prints the same answer: it is the same call. The Templum Agent's control server
  shares the operation (`cards_of`, workspace/ops.rs), so it sees them too.
- The tool's description says so: "…, checklist progress, comment and attachment counts. With card, that card whole:
  its description, checklist items, attachments (each one's name, size and path) and comments too."
- **Send to ▸** in a card's menu types its text and, after it, `Attachments:` and each path on its own line
  (designer's call): a card handed to an agent that way brings its screenshot.

### Calls made for card 152

The person was asleep. The calls given with the card (any file, a copy per card in the data folder, a size limit,
Attach with the system's picker, a drop, a pasted image with no text, thumbnails and rows, a click opens, a remove
button, the face's paperclip, the work reaching the board at Done, nothing left behind, `list_cards` and the CLI with
paths, no tool to add) are followed; these were the designer's, and each can be changed:

1. **50 MB a file** (designer's call), 1 MB being 1024 × 1024 bytes as in the sizes shown: a screen recording fits,
   and the data folder doesn't fill by accident.
2. **The file's own name in its card's folder** (designer's call), " (2)" when taken, rather than an id: the system's
   app and an agent both read it.
3. **A new card takes its id at its first attachment** (designer's call), so its files are in their card's folder
   from the start and nothing moves at Done.
4. **Deleted at start, never before** (designer's call): one rule covers a discarded draft, a removed attachment, a
   deleted card, column or board, and a crash, and it can't race Ctrl+Z, which lives only in a session. The price: a
   discarded file takes disk space until the next start.
5. **Copied in the background; Done never waits** (designer's call), and a copy that ends late follows the work.
6. **Thumbnails up to 10 MB** (designer's call): bigger images are rows.
7. **A pasted image is `Pasted image.png`** (designer's call), numbered when taken.
8. **Send to ▸ carries the paths** (designer's call).
9. **A folder is refused** (designer's call), named in the notification: copying a tree is another feature.
10. **The section sits between Description and Checklist** (designer's call): it belongs with what the card says,
    before what is done about it.

### Constraints

- CLAUDE.md: `board.rs` and the new `attachments.rs` stay pure and unit-tested; the dialog's behavior has headless
  tests (crates/templum/src/workspace/tests/attachments.rs); its look is checked by hand and said so; minors go to
  docs/minors.md; no new crate, trait or setting; docs ≤ 120 columns.
- `boards.json` stays readable by older Templums (serde `default` and `skip_serializing_if`).
- No new dependency (above).
- The face is painting: `--bench-mixed` stays at p95 ≤ 16.7 ms.
- Docs to update: this file's Status and Out of scope, connections.md (`list_cards`), ARCHITECTURE.md (the
  attachments folder, `attachments.rs`).

### Testing

**Unit (`board`):** attachments read back equal, and a card with none writes no key; an entry named `../x`, `a/b` or
`""` is dropped and the rest of the card loads; Done adds and removes what the person did and keeps the rest; a new
card's reserved id is its id at Done; a card's text ends with its attachments' paths.
**Unit (`attachments`):** a copy keeps the name, and takes a free one when it is taken, with or without an extension;
a file over 50 MB and a folder are refused with why and leave nothing behind; a pasted image is written; sizes read
"12 B", "47 KB", "1.2 MB"; the sweep removes what no card or draft names and keeps the rest, a new card's draft's
folder among them.
**Headless UI:** Attach and the picker's two files show a thumbnail and a row, Done puts both on the card and its face
counts 2, one Ctrl+Z takes them off; ✕ changes nothing on the board before Done; a refused file notifies and attaches
nothing; a copy ending after Done reaches the card, after Save draft the draft, after Discard nothing; a pasted image
is attached and the title is unchanged, a paste with text pastes the text; a drop attaches; no data folder, no
section; `list_cards` lists the attachments with paths that exist; a start deletes a deleted card's folder and keeps
everything when `boards.json` was set aside or is missing; Ctrl+Z of a Delete card brings the card back with its file.
**Manual (Hyprland, and once on macOS and Windows):** a drop from the file manager; a screenshot pasted (`grim -g
"$(slurp)" - | wl-copy` on Hyprland); a click opening an image, a PDF and a text file; thumbnails in light and dark
themes; a 9 MB image.

### Left to the builder

The section's sizes past those above; how a copy's result finds the dialog's work (`Rc::ptr_eq` on `card_work`, or a
weak handle); where the refusals' notification is raised; the path of gpui-kit's input `Paste` action; whether a
file drop reaches the dialog in the headless harness (a card's drop doesn't: [minors.md](../minors.md), Kanban). If it
doesn't, the test calls the drop's function, the drop is checked by hand, and a minor says so.

### As built (card 152)

The plan's four tasks, as one change, left in the working tree for review. The person was asleep, so where the plan
left a choice the one best for the system was taken:

- **Where a late copy lands.** The dialog that started the copy keeps its outcome (`Work::outcome`: open, Done, Kept
  by Save draft, or Dropped by Discard and Delete card) and `land` (`kanban.rs`) routes by it first. Done: onto the
  card, as its own step of Ctrl+Z (a dialog reopened since sees the file there, through `merge_into` of the live card,
  and has nothing to ask about). Dropped: nowhere, even when the card is reopened before the copy ends. Open or Kept:
  into the dialog open on that card, found by the id in its work (`ws.card_work` with `work.id == card`), not by
  `Rc::ptr_eq`, so a dialog reopened from the kept draft shows it too; else into the kept draft. `attach` itself
  still does nothing unless the dialog it is given is the open one.
- **Boards set aside take their files.** When `boards.json` is read as unreadable-but-movable (`Loaded::Moved`),
  `attachments::set_aside` renames `<data>/attachments` to `attachments.bak` beside `boards.json.bak`, replacing an
  older one the way the older `boards.json.bak` is replaced: the pair restores together, and the session starts with
  no files, so the next start's sweep has nothing of the old cards to delete and new cards don't reuse old folders.
  The files go to a temporary name first (`attachments.moving`), the older `.bak` is removed only after that worked,
  and the temporary becomes `attachments.bak`; if a step fails (a file held open on Windows) the files are put back
  where they were, and the set-aside notification adds "and their files couldn't be set aside: <error>" (on success
  it adds ", their files as attachments.bak").
- **Refusals** are one warning per `attach` call, raised from its task once every copy has ended; the copies run side
  by side on the background executor and are taken in the order attached. A file that grew past 50 MB after its size
  was read is refused too (the copy reads one byte past the limit).
- **Names:** a pasted image takes a free name like any file (`Pasted image (2).png`); a path with no name, or one
  that isn't a plain file name, is refused.
- **The sweep** also removes the empty folder of a card with no attachments left (not counted), and keeps a file a
  kept draft's `base` still names (Ctrl+Z of a removal), as well as its `work`. It runs in `load_spaces` only on
  `Loaded::Saved`; an unreadable folder or an entry that won't go is kept as the first error (printed), and the sweep
  goes on with the rest. `plain_name` also needs exactly one normal path component, so on Windows `C:x.png` is no
  name.
- **The paste** is gpui-kit's `component::input::Paste`, captured on the dialog's body (`card-body`) ahead of any
  field. A clipboard with a non-blank text, an image beside it or not, is the field's; with an image and no text
  (blank counts as none) the image is attached and the field gets nothing. With no data folder a pasted image does
  nothing.
- **The drop** reaches the dialog in the headless harness (unlike a card's drop), so `dropping_files_on_the_open_card…`
  dispatches a real `FileDropEvent` and no minor is needed for it. The ring is the body's border, always 2 wide and
  clear, lit with the accent while `ExternalPaths` are over it, so nothing moves when it lights (the body's sides are
  2 narrower to make up).
- **`list_cards`:** the count ("1 attachment", "3 attachments") is in a card's line with or without a data folder;
  the `attachments:` part, with paths, only with one. `Card::text(data)` takes the data folder (None: no paths).
- **Verified by hand:** nothing in a window, as the person was asleep (see the minors, Kanban). Headless: every
  behavior above has a test; the face's count is checked by the data (the harness can't read a div's text, so the
  test asserts the face's element appears and goes with Ctrl+Z). `--bench-mixed` was left to the person.

