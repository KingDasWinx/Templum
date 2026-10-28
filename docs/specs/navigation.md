# Moving the canvas over blocks

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [walking-skeleton.md](walking-skeleton.md) (panning and zooming) and [polish.md](polish.md)

## Goal

On a canvas full of blocks, the empty spaces that pan it run out: a drag over a terminal selects its text, and the
wheel scrolls its history. Templum gets two ways to move the canvas wherever the pointer is, and the wheel scrolls a
block only when the block is selected ([below](#the-wheel-over-blocks)).

## The hand: hold Alt

- While **Alt** is held (**Option** on macOS), a layer covers the canvas, blocks included, with the open-hand
  cursor (the closed hand while dragging), and the toolbar shows the Hand tool on; letting go shows the tool that
  was on before:
  - a drag pans the canvas;
  - the wheel, or two fingers on a touchpad, zooms around the pointer, as the Hand tool does;
  - Ctrl+wheel and pinching zoom, as anywhere.
- Blocks don't react meanwhile: no focus, no selection, no move, no hover.
- Letting go of Alt removes the layer. A drag already under way goes on until the button is released.
- The empty canvas's hint says so: "Hold Alt · Move the canvas over blocks".

Alt alone types nothing in a terminal, and Alt+key still reaches it (the layer only takes the pointer). On the dev
machine (Hyprland), window bindings use Super, so Alt with the mouse is free.

## Moving a selection

Blocks are selected with Shift+drag on the empty canvas or Shift+click on a title. With two or more selected, one
box is drawn around all of them, as Figma does: their union and a small margin (8 canvas units) outside, a
hairline in the theme's accent, at the canvas's zoom. It follows the blocks as they move or resize, and takes no
events itself (`Canvas::selection_box`, `templum-canvas`).

- **Press anywhere to move them.** A left press on any part of a selected block (its title or its body) or on the
  empty canvas inside the box starts moving every selected block, still selected. The body's content doesn't hear
  that press: no text selection in a terminal, no caret in a note, no kanban card drag. One selected block moves by
  its body too, though no box is drawn around it.
- **A click doesn't move them.** A press let go without moving past a click: on a selected block's *body* it picks
  that block (the keyboard goes to it, the selection ends), so the person can type right after; on the empty
  canvas inside the box it ends the selection; on a selected *title* nothing changes, the selection stays.
- **Shift+click** on a selected block, its title or its body, takes it out of the selection. Only a title adds a
  block that isn't selected: the press on an unselected body belongs to its content.
- Not affected: a right-click on a selected block opens its menu, with the selection's items; the middle button
  pans; the header's buttons, the resize strips along the sides, the corner squares and the grip keep their
  presses; a double-click on a title frames the block; grid mode's tiles (no free move, no box). The wheel over
  selected blocks scrolls their content, [below](#the-wheel-over-blocks).
- The body's content keeps the presses that are its own: a terminal's find field and the notice of a request that
  waits for the person (only the title moves the group over the find field) and a terminal's Ctrl+click (Cmd on
  macOS), which opens a link. Far away, the block's label is its title: a click on it keeps the selection, a
  double-click frames the block.
- A click is a press let go within a click's distance of where it started, without the wheel having carried the
  selection. On a body or in the box, blocks nudged on the way are put back, with no step for Ctrl+Z.
- Held, the selection pans the canvas at its edge like a title drag, [below](#holding-a-block).

## Holding a block

While a block (or a selection) is held by its title, body or box, the wheel, or two fingers on a touchpad, pans
the canvas, a mouse's wheel and Ctrl+wheel included, and the block stays under the pointer: it can be carried
farther than the window shows without letting go. That holds over any block, selected or not (`route_wheel` leaves
the content nothing to scroll meanwhile).

Bringing the pointer near the canvas's edge pans it too, the way the selection rectangle (Shift+drag) does: the
same rule (`templum_canvas::edge_scroll`: a 40 px zone, faster the nearer the edge, full speed at it or past it,
in screen pixels, so the zoom doesn't change it). The canvas keeps going while the pointer rests there, the block
or every selected block stays under the pointer, and it stops when the button is let go or the pointer leaves the
zone. A press on a title near the edge that doesn't move past a click pans nothing. In grid mode the tiles don't
move freely, so nothing pans.

## The wheel over blocks

With the Select tool, a block's content takes the wheel only when the block is **selected**: it has the keyboard (a
click, or the pointer moving onto it with *Focus follows the pointer*, below), or it is in the selection (Shift+drag,
Shift+click on a title). Over any other block the wheel does what it does over empty space: two fingers on a
touchpad pan the canvas, a mouse's wheel zooms it, Ctrl+wheel zooms. So a canvas panned with the wheel doesn't stop
dead when a block slides under the pointer, and a terminal's history, a note, a kanban's columns, an editor's file
or the Git block's lists scroll only once the block is picked.

- One place decides: a layer over the blocks (`route_wheel`, `workspace/input.rs`) hears the wheel before any block's
  own handler and, over a block that isn't selected, moves the canvas and stops the event there. The blocks keep
  their handlers as they were (stop the wheel unless Ctrl is held), so what a selected block scrolls is unchanged.
- **Focus follows the pointer** (the setting, on by default) makes the pointer *picking* a block: moving it onto a
  block gives it the keyboard, so the wheel scrolls it from then on. A block that comes under a *still* pointer, as the
  canvas pans, doesn't take the keyboard ([below](#the-keyboard-follows-the-pointer)) and so doesn't take the wheel.
  With the setting off, a click picks the block.
- Not affected: grid mode and fullscreen (no camera moves there, so every tile scrolls its own content, selected
  or not); the Hand tool, Alt and the drawing tools (their layer already takes the wheel); a block held by its title
  (the wheel moves the canvas, above); a block far away, drawn as bars (it scrolls nothing, as before).

## The middle button

A drag with the middle mouse button pans the canvas anywhere, over blocks too, with no key held. Terminals don't
use the middle button (no paste of the primary selection yet).

## How far the zoom goes

From 1% to 300%, on fixed steps: 1%, 1.5%, 2%, 3%, 4%, 6%, 8%, 12%, 16%, 25%, 33%, 50%, 67%, 75%, 90%, 100%, 110%, 125%,
150%, 175%, 200%, 250% and 300%. The wheel, a pinch, Ctrl+= and Ctrl+-, Fit all blocks, a framing and a restored
canvas all land on them; the title bar reads tenths under 10% ("1.5%"). Under 25% a block is its card with its title's
label, and too small for the label it is its shape with a terminal's status dot; the label reads past the card's edge
(over a shape's corner) and hides where it would pile on another's. A press on the card or its label moves the block
and a double-click frames it ([level-of-detail.md](level-of-detail.md)). The dots, wires, drawings, selection, the selection
rectangle and the scroll at the window's edges work the same at every step.

## Framing a block

A double-click on a block's title glides the view to show the block centered, at the largest zoom step that fits
above the toolbar. A double-click on the same title again, before the view moved, glides back to where it was.
Like any zoom, this magnifies the block's contents: a terminal keeps its columns and rows. The title, because a
double-click inside a terminal, an editor or a note selects a word.

## The keyboard follows the pointer

**Focus follows the pointer** (Settings ▸ Canvas, `[canvas] focus_follows_pointer`, on by default): once the pointer
moves, the block under it takes the keyboard, no click needed, as Hyprland's `follow_mouse` does and as grid mode's
tiles do ([grid.md](grid.md#moving-around)).

- **The keyboard only.** Blocks overlap on the canvas: the block stays where it is in the stack, so moving across
  the visible edge of a block behind another doesn't bring it to the front. A click still does.
- **Only when the pointer moves.** A block that comes under a still pointer (a wheel pan, a zoom, a glide, a block
  opened, moved or closed) doesn't take the keyboard until the pointer moves; nor does a move reported where the
  pointer already was.
- **Not while** a button is held or a drag goes on, a right-click menu is open, or the keyboard is outside the canvas
  and its blocks (a dialog, the settings panel, the Git popup, the theme picker): it stays there until a click.
- **A selection keeps it.** While blocks, a drawing or a wire are selected, the keyboard stays with them (Delete and
  Copy act on them) and the selection stays; a click ends it as before.
- **The block that had the keyboard lets go as on a click:** a checklist item being edited stops; a note's or an
  editor's text and cursor stay, and its editor takes the keyboard again when the pointer comes back.
- With the setting off, a click focuses, as before. Grid mode's tiles follow either way.

## Testing

**Headless UI:**

- With Alt held, the hand layer exists; a drag over a terminal pans the canvas and selects nothing; the wheel over
  a terminal zooms the canvas; the toolbar shows the Hand on until Alt is let go; letting go of Alt removes the
  layer.
- With the Hand tool, the wheel zooms over blocks and the empty canvas.
- A double-click on a title frames the block above the toolbar; another goes back.
- A middle-button drag over a terminal pans the canvas.
- With Select, the wheel over a terminal and a note that aren't selected pans the canvas (a mouse's wheel zooms) and
  scrolls neither; after a click on the terminal, the wheel is its own and the canvas stays; a note in the selection
  keeps the wheel too, and Ctrl+wheel zooms over it.
- The canvas panned under a still pointer doesn't give the wheel to the block that arrives under it; the pointer
  moving onto the block does.
- In grid mode, the wheel over a tile that isn't selected scrolls it.
- Moving the pointer over a terminal gives it the keyboard, leaves it under the others and takes it from the note's
  editor; a block coming under a still pointer, or a move to where the pointer is, doesn't take it; a real move
  does.
- The keyboard stays with the setting off, with blocks selected (they stay selected), with a menu open, with a
  button held and with the keyboard in the settings panel; back on the canvas, it follows.

**Manual (Hyprland):** hold Alt over a terminal and over a kanban block; drag and two-finger scroll. Move the pointer
between overlapping terminals, a note and an editor and type in each; scroll the canvas under a still pointer.
