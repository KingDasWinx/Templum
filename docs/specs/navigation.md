# Moving the canvas over blocks

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [walking-skeleton.md](walking-skeleton.md) (panning and zooming) and [polish.md](polish.md)

## Goal

On a canvas full of blocks, the empty spaces that pan it run out: a drag over a terminal selects its text, and the
wheel scrolls its history. Templum gets two ways to move the canvas wherever the pointer is.

## The hand: hold Alt

- While **Alt** is held (**Option** on macOS), a layer covers the canvas, blocks included, with the open-hand
  cursor (the closed hand while dragging):
  - a drag pans the canvas;
  - the wheel, or two fingers on a touchpad, pans it;
  - Ctrl+wheel and pinching zoom, as anywhere.
- Blocks don't react meanwhile: no focus, no selection, no move, no hover.
- Letting go of Alt removes the layer. A drag already under way goes on until the button is released.
- The empty canvas's hint says so: "Hold Alt · Move the canvas over blocks".

Alt alone types nothing in a terminal, and Alt+key still reaches it (the layer only takes the pointer). On the dev
machine (Hyprland), window bindings use Super, so Alt with the mouse is free.

## The middle button

A drag with the middle mouse button pans the canvas anywhere, over blocks too, with no key held. Terminals don't
use the middle button (no paste of the primary selection yet).

## Testing

**Headless UI:**

- With Alt held, the hand layer exists; a drag over a terminal pans the canvas and selects nothing; the wheel over
  a terminal pans the canvas; letting go of Alt removes the layer.
- A middle-button drag over a terminal pans the canvas.

**Manual (Hyprland):** hold Alt over a terminal and over a kanban block; drag and two-finger scroll.
