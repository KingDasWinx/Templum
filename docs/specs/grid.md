# Grid mode: blocks full screen, tiled like Hyprland

- **Date:** 2026-10-14
- **Status:** Implemented
- **Builds on:** [cockpit.md](cockpit.md) (blocks), [persistence.md](persistence.md) and the canvas selection
  (Shift+drag)

## Goal

A terminal at work is easier to read full screen, at its real size, with more columns and rows, and next to it
the editor it works with. A workspace gets a **grid mode**: some of its blocks fill the workspace area
tiled side by side, like Hyprland's dwindle layout, and go back to the canvas exactly as they were.

## What a person sees

- **In grid mode** the canvas is gone: no dots, drawings, toolbar or other blocks. The tiled blocks fill the
  workspace area (below the title bar, beside the sidebar), 8 px apart and 8 px from its edges, at zoom 1: text at
  its real size, a terminal resized to fill its tile. The camera doesn't move: the wheel scrolls inside blocks only,
  Ctrl+wheel, pinching, the zoom keys and dragging the background do nothing, the gaps are inert.
- **Each workspace** has its own grid: switching workspaces shows each as it was left, and a restart brings it back.
- **On a mirrored workspace** ([templum-to-templum.md](templum-to-templum.md)) grid mode is the mirror's own view of
  home's blocks: going in and out calls nothing at home, and home's moves and closes keep the tiles (a moved tile goes
  back to home's place). A restart doesn't bring it back: a mirror's canvas is never saved.
- **Tiles:** each pair of tiles shares its area in halves. A new tile takes half of the one it splits, along its
  longer side (a wide tile splits side by side, a tall one top and bottom). Halves stay halves: there are no
  dividers to drag.

### Going in and out

- **Fullscreen** (`toggle_grid`, Ctrl+Shift+Enter, Cmd+Shift+Enter on macOS; the button left of a block's close
  button; Fullscreen in a block's menu) fills the workspace with the focused block. With blocks selected on the
  canvas (Shift+drag, Shift+click on a title), they all go in, left to right as they are on the canvas, then top to
  bottom, each splitting the one before.
- In grid mode the same shortcut and the button (now "back") return to the canvas: every tiled block goes back to
  its rect on the canvas, the camera to where it was, and terminals to their sizes there.
- **Leave the grid** in a block's menu sends that block back to the canvas; the other half of its pair takes its
  place. Closing a tile does the same. When the last one goes, the canvas comes back.

### More blocks in the grid

- A block opened in grid mode (a shortcut, Send to, Open in editor, Open a terminal here, a note from the Notes
  dialog, paste, Ctrl+Z reopening a closed block) takes half of the focused tile. Its place on the canvas, for when
  the grid ends, is where it would have opened on the canvas as it was left, off the places of the others.
- **Right-click** a tile: **AI agent ▸** and **Add ▸** (Terminal, Kanban board, Note, Editor) open a new block
  that takes half of that tile, as the canvas menu does; **From the canvas ▸** lists the workspace's blocks left on
  the canvas, and the one chosen takes half of the tile. Right-clicking a gap between tiles opens the canvas menu
  (AI agent ▸, Add ▸, Notes…): the new block takes half of the focused tile.
- Going to a block that isn't tiled (a notification, Go to waiting, Send to, an open note) brings it into the grid.

### Moving around

- **The keyboard follows the pointer**, as Hyprland's `follow_mouse`: once the pointer moves, the tile under it takes
  the keyboard, no click needed. A tile that comes under a still pointer (a swap, a new tile, a resized window)
  doesn't take it until the pointer moves; nor while a button is held, a menu is open or the keyboard is outside
  the tiles (a dialog, the settings panel, the Git popup). Tiles always follow: the setting that turns it off,
  Settings ▸ Canvas ▸ Focus follows the pointer, is for the canvas, where blocks overlap
  ([navigation.md](navigation.md#the-keyboard-follows-the-pointer)).
- **Focus** (`focus_left`, `focus_right`, `focus_up`, `focus_down`; Ctrl+Alt+arrows): the keyboard goes to the
  nearest tile on that side.
- **Swap** (`swap_left`…; Ctrl+Alt+Shift+arrows): the focused tile trades places with that neighbour.
- **Drag a title onto another tile:** the two trade places. Once the pointer has moved past a click, the tile is
  picked up (96%, faded), its place is outlined with an accent dashed line, its title follows the pointer in a
  card, and the tile under the pointer is tinted with the accent color, saying "⇄ Swap with *its title*". Let go,
  the two slide to their new places (220 ms); let go over no tile, nothing changes. Ctrl+Alt+Shift+arrows slide
  them too. A tile's title shows an open hand, a closed one while it is held.
- **Flip the split** in a tile's menu turns its pair from side by side to top and bottom, and back.
- These keys reach the terminal outside grid mode. Ctrl+Alt+arrows switch workspaces on some desktops (GNOME) and
  are AltGr on some Windows layouts: `[keys]` changes them.

## How it works

While a workspace is in grid mode, each tiled block's rect **on the canvas** is its tile's, and the canvas camera is
at the origin at zoom 1; the blocks' rects on the canvas (their homes) and the canvas camera wait in `Space::grid`.
Drawing, the pointer, a terminal's cells and the bodies' sizes then work as on the canvas. Every frame, the tiles
are laid out again over the area as it is (the window, the sidebar) and a terminal is resized only when its tile
changed. What reads a block's place on the canvas (a snapshot, closing for Ctrl+Z, copying, Ctrl+Z moving a block,
placing a new block) uses the homes and the kept camera.

- `templum-canvas/src/tile.rs`: the tree of tiles (`Tile`: a block, or a `Split` pair), its rects, inserting,
  removing, neighbours, swapping, flipping. Pure, unit-tested.
- `workspace/tiling.rs`: going in and out, laying out, joining, leaving, focus and swap.
- **File:** the workspace's file keeps the canvas as it is (camera, rects) and adds `grid`, the tree by item index:
  `"grid": {"split": "beside", "a": 0, "b": {"split": "above", "a": 2, "b": 1}}`. Items that don't come back are
  left out of it; an older Templum ignores it and shows the canvas.

## Testing

**Headless:** the tree's layout, dwindle, removal, neighbours, swap and flip; going in and out gives every rect, the
camera and a terminal's size back; a window resize tiles again; the selection goes in left to right and hides the
rest; the camera stays put; a snapshot in grid mode keeps the canvas; closing tiles; Ctrl+Z moves a tile's home; new
blocks split the focused tile and land apart on the canvas; focus and swap keys; dragging a title swaps; the menu's
items; a restart brings the grid back.

**Manual (Hyprland):** a terminal full screen with an agent in it; three blocks from a selection; Ctrl+Alt+arrows;
resize the window and toggle the sidebar; switch workspaces; quit and start again.
