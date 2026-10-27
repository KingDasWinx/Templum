# Level of detail at far zoom

- **Date:** 2026-10-13
- **Status:** Implemented. Extended on 2026-10-26 (kanban card 227): zoom steps down to 1%, and the Shape level.
- **Builds on:** [ARCHITECTURE.md](../ARCHITECTURE.md) (Zoom), [ADR-0002](../adr/0002-ui-framework.md),
  [navigation.md](navigation.md), [walking-skeleton.md](walking-skeleton.md) (which lists this under "Later")

> **Since 2026-10-17 Git is not a block** but a popup drawn at a fixed scale ([git.md](git.md)): it has no level of
> detail, and this spec's rules for Git blocks are gone.

## Goal

Zoomed out, Templum stays smooth. At 25% the canvas shows 16 times the area it shows at 100%, and every block on
screen is drawn in full: terminals shape every row, editors and notes run gpui-kit's whole editor with syntax
highlighting, all at sizes nobody can read. Culling (`templum-canvas/src/canvas.rs:70-82`,
`workspace/render.rs:240-259`) doesn't help: those blocks are on screen.

Each block gets three levels of detail, picked by how big its text is **on screen** (font size × zoom), not by the
zoom itself, like InDesign's "Greek Type Below":

1. **Full**: as today, while the text is readable.
2. **Silhouette**: text too small to read becomes bars in the text's own colors, on exactly the rows and columns of
   the letters ("greeking", like VS Code's minimap with characters off). You still see an agent's output scroll, a
   red error, a green success, the shape of Claude Code's screen.
3. **Card**: further still, the block's card with its title and status, nothing inside.
4. **Shape**: a block too small on screen to hold its title's label (under 25%, down to 1%): its card alone, with a
   terminal's status dot in the middle.

Keeping an eye on agents from afar is the main use, so a block's title stays readable wherever its card holds it,
and a terminal's status dot at every zoom.

**Success criteria**

1. At 33% and 25% (default fonts), no unfocused block shapes a terminal row or builds a gpui-kit `Editor`, a
   Markdown view.
2. Zoom still scales like a photo: changing level moves nothing. Bars sit on the letters' rows and columns, within
   1 px, at every zoom step, and turn into letters in place when zooming in. A level changes only when the camera
   comes to rest, never during a glide.
3. At 33% and 25%, titles are 12 px and status dots 8 px on screen.
4. Under 25% (default fonts) no block lays out text but its fixed-size label: every block is its card or its shape,
   the focused one too, and labels never pile up: a card too small for its label has none.
5. The new `--bench-far` stays at p95 ≤ 16.7 ms. `--bench`, `--bench-mixed`, `--bench-zoom=<kind>` (every kind) and
   `--bench-git` are no worse than before (within run-to-run noise), and stay ≤ 16.7 ms where they were.
6. Nothing that works today breaks: every existing test passes, and so do the checks in
   [Focus and interaction](#4-focus-and-interaction) and the tests in [Testing](#testing), on the 3 OS.

## Out of scope

- The "cold preview" of unfocused editors at readable zooms (letters drawn without gpui-kit's editor).
- Blocks as cached views (`Entity::cached`, "batch C"), the gpui-kit 0.7.3 upgrade, and GPUI Fast: separate work.
- **A glow on a far terminal's border when new output arrives.** Decision: not now. The bars already move with the
  output, and the status dot turns yellow, red or green on news; a glow would be one more state and one more
  animation keeping frames coming. Revisit if people miss output on a terminal drawn as a Card.
- Syntax colors in the bars of editors and notes (see [The word map](#3-editors-and-notes-the-word-map)).
- Fewer frames from far terminals' output (each output still asks for a frame; the frame costs less).

## 1. The level

**Inputs:** the block's text size at zoom 1, its size, and the zoom the view last came to rest at.

| Text | Size at zoom 1 | Where |
|---|---|---|
| A terminal's grid | `font.size`: 14 by default, 6 to 72 | `config.rs:73-88`, `340-346`, `settings/fields.rs:155-163`, `ws.cell.font_size` (`grid.rs:29-36`) |
| An editor's file, a note | 13 | `editor.rs:38`, `note.rs:25` |
| A kanban block (its card and column titles) | 13 | `kanban.rs:310`, `kanban.rs:467` |
| A block's title | 12 | `block.rs:30` |

**Thresholds,** on the text's on-screen size (size × zoom):

| On screen | Level |
|---|---|
| 5.5 px or more | Full |
| 2.5 px to under 5.5 px | Silhouette |
| under 2.5 px | Card |

What the default sizes reach at each step (`ZOOM_STEPS`, `camera.rs`):

| Zoom | 0.01 to 0.12 | 0.16 | 0.25 | 0.33 | 0.5 | 0.67 | 0.75 and up |
|---|---|---|---|---|---|---|---|
| Terminal, 14 | ≤ 1.68 | 2.24 | 3.50 | 4.62 | 7.00 | 9.38 | ≥ 10.5 |
| Editor, note, kanban, 13 | ≤ 1.56 | 2.08 | 3.25 | 4.29 | 6.50 | 8.71 | ≥ 9.75 |
| Title, 12 | ≤ 1.44 | 1.92 | 3.00 | 3.96 | 6.00 | 8.04 | ≥ 9.0 |
| **Level** | Card (or Shape) | Card (or Shape) | Silhouette | Silhouette | Full | Full | Full |

**The steps under 25%** are 0.01, 0.015, 0.02, 0.03, 0.04, 0.06, 0.08, 0.12 and 0.16: each 1.5 or 1.33 times the one
below, as 25%, 33% and 50% are, so every second one doubles the zoom (16% to 25% is 1.56 times). The wheel, a pinch,
Ctrl+-, Fit, a framing, a restored canvas and a mirror's grid reach them as they reach any step; the title bar reads
"1.5%" under 10%. A step at 0.2 would put a 12 or 13 px text 0.1 px from 2.5 px (2.4 and 2.6): 0.16 keeps every
default size 0.26 px or more from it.

- **Why 5.5 px:** it sits between the largest default size at 33% (4.62 px) and the smallest at 50% (6.0 px), so
  every default block changes level at the same step and none sits on the edge. At 50% a terminal's glyphs are
  "a real 7 px: sharp but tiny" (ADR-0002's spike), still read; at 33% they are 4.6 px blobs. A terminal set to
  17 pt or more stays Full at 33% (17 × 0.33 = 5.61 px): bigger text keeps its letters further out, as it should.
- **Why 2.5 px:** under it a row is about 3 px tall and its bars 1 px lines, a gray texture that says nothing; the
  card says more and costs less. From 25% up with default fonts this never happens: it is a terminal font of
  9 pt or less at 25% (9 × 0.25 = 2.25), or 7 pt or less at 33%. Under 25% it is every default size: a kanban or a
  checklist block too, which are drawn in full only down to the Card level. Rendered Markdown is left empty at the
  Silhouette level (see [section 2](#2-what-each-block-draws)).
- **The block's size on screen: the Shape level.** A block whose card is under 40 × 24 px at the rest zoom
  (`LABEL_FITS`) can't hold its label, the 12 px title 4 px in from its corner: it is its shape, whatever its text.
  Every block of its kind's smallest size holds the label at 25% (the smallest, a 200 × 140 note, is 50 × 35 px;
  its title is cut by its card like any text in it; a smallest terminal about 45 × 30 px), so with the default font the
  Shape level is reached only under 25%. With the default sizes:

  | Block | 16% | 12% | 8% | 6% | 4% and under |
  |---|---|---|---|---|---|
  | Terminal, 80 × 24 (about 684 × 500) | Card | Card | Card | Card | Shape |
  | Editor 900 × 600, kanban 900 × 540 | Card | Card | Card | Card | Shape |
  | Note 360 × 280 | Card | Card | Shape | Shape | Shape |
  | Checklist 320 × 360 | Card | Shape | Shape | Shape | Shape |

  The block's size changes only by a resize and the camera rests on steps, so no hysteresis is needed here either.

**Where:** `templum-canvas/src/lod.rs`: `enum Level { Full, Silhouette, Card, Shape }` (ordered, most drawn first),
`GREEK_BELOW: f32 = 5.5`, `CARD_BELOW: f32 = 2.5`, `LABEL_FITS` (40 × 24 px), `fn level(text: f32, zoom: f32) ->
Level` and `fn block_level(text, size, zoom) -> Level` (Shape when the block can't hold its label, else `level`).
Pure, no UI types: the crate boundary holds and it is unit-tested headlessly. `block::render` asks `block_level` for
each block every frame (a few multiplications).

**When it changes: only when the camera rests.**

- `Space::rest` (new, `workspace/mod.rs` next to `view` and `flight`, `:323-327`) is the zoom the view last came to
  rest at. `render.rs` sets it where it decides the view (`render.rs:224-239`): whenever there is no flight, the view
  is the camera and `rest` is its zoom. During a flight it keeps its value, through a retarget (`view.rs:85-94`) and
  a pan mid-flight (`view.rs:106-111`): the glide is drawn at the level the camera rested at, and the level changes
  on the frame the glide ends. This is tldraw's idea of an "efficient zoom level" that holds still while the camera
  moves.
- The camera rests only on `ZOOM_STEPS` (`camera.rs:13-14`; only a flight's frames are between steps), so at rest
  the level is a function of the step: **no hysteresis is needed**. Nothing flickers between levels during a glide,
  and nothing can flip back and forth at rest.
- A Full body drawn mid-glide keeps today's settled zoom (`block.rs:53-66`). A Silhouette body of an editor or a note
  goes in the same `settled_box`, with the same pull toward the glide's pivot (`block.rs:74-90`, `598-607`), so its
  bars glide exactly as its letters would.
- Focus overrides the Silhouette level: the block you work in has its letters where it would have bars (see
  [section 4](#4-focus-and-interaction)). A Card or a Shape stays as it is.

## 2. What each block draws

### The title and the status dot

- **Full:** the header as today (`block.rs:114-150`, `173-188`).
- **When the title itself is below Full** (12 px × the rest zoom under 5.5 px: at 33% and 25%), the title becomes a
  **label**: a tag at the card's top-left corner, 4 px in from its border, with the status dot (8 px, terminals
  only) and the title in 12 px text, on the card's own background (a note's tint included) with a hairline border.
  It is as wide as the title, at most the card's width less 8 px, cut with an ellipsis, and never scaled. It covers
  the header and the top of the body: about three terminal rows at 25%, two at 33%. The card clips it
  (`block.rs:454`), so it never spills onto another block.
- The label **is** the title: the same `("title", n)` element with its handlers (press to move, double-click to
  frame, `block.rs:137-149`), and the dot keeps its id and tooltip (`("status", n)`, `block.rs:176-185`).
- The header's small buttons at the top left (a kanban block's boards, a note's notebook, an editor's tree toggle,
  `block.rs:190-290`) are hidden while the title is a label: the label sits there, and they are 5 px wide at 25%.
  The close button and the resize grip still show on hover (`block.rs:153-171`, `400-424`), and so do the resize
  strips (`block.rs:291-312`).
- The card itself (border, radius, shadow, background, focus ring) is the same at every level.

- **Shape:** no label and no header. The title is the whole card (the same `("title", n)` element, so a press
  anywhere on it moves the block and a double-click frames it), with a terminal's status dot in its middle: 8 px, or
  as big as the card's shorter side within its border; hovering it names the block in a tooltip. On hover no
  buttons, resize strips nor grip: a shape is only moved; zoom in to resize it.

### Every block at the Card and Shape levels

Nothing inside the card: no body is built, a terminal's grid isn't painted (below), an editor's tabs, a note's
editor, a kanban's board and a checklist's items aren't laid out. Their state is kept ([section 5](#5-state-across-levels)).

### Terminal

- **Silhouette:** each row of the snapshot (`templum-terminal/src/snapshot.rs:38-48`) is painted as today
  (`grid.rs:197-265`), but its text isn't shaped: its background spans, selection, prompt mark and cursor are drawn
  as today, and the text becomes **one bar per run of non-blank cells of one color**.
  - `Row::bars()` (new, `snapshot.rs`) walks `text` with its `runs` (one style each, `len` in bytes,
    `snapshot.rs:21-29`), counts columns in characters, and cuts at blanks. Bold and italic don't matter; dim keeps
    today's 0.66 opacity (`grid.rs:235`). It is pure and tested in `templum-terminal`.
  - A bar covers its cells, with the same cell math as the letters (`grid.rs:187-196`), so it sits exactly where
    they would. It is half the row high (at least 1 px; GPUI snaps a quad's edges to device pixels,
    `gpui-pre-0.3.8 src/window.rs:3171-3181`) and centered on the row, at 60% of the text's opacity. A bar covers its
    cells solidly where letters cover about half of them, so at full strength a screen of bars would read heavier
    than the text it stands for.
  - A wide character (CJK, emoji) is followed by a blank spacer (`snapshot.rs:277-282`): its bar covers its first
    column only.
  - The snapshot is still refreshed and the terminal re-armed (`grid.rs:180-186`): new output keeps moving the bars.
  - Not built: the find field (`block.rs:365-367`), and the body's wheel handler (`block.rs:350-352`). The press
    handler stays (`block.rs:338-349`): it reads the terminal's grid, not the painted text.
- **Card:** the card and the label, nothing inside. The grid isn't painted, so the terminal isn't re-armed: like a
  terminal off screen, its output doesn't ask for frames. A command's end and a call for attention still wake the
  UI (ARCHITECTURE, Runtime), so the dot stays current. When the block comes back, the snapshot catches up from
  alacritty's damage (`snapshot.rs:139-233`).

### Editor

- **Silhouette:** the body is built as in Full (`editor.rs:632-1008`), with three parts swapped:
  - the active file's gpui-kit `Editor` (`editor.rs:941-956`) becomes a canvas of the same size painting the file's
    **word map** ([section 3](#3-editors-and-notes-the-word-map)); `keep_rows` isn't called either;
  - a Markdown file shown rendered (`editor.rs:930-939`) becomes an empty area: gpui-kit's Markdown view has no
    public layout to draw bars from, and an empty area keeps everything in place;
  - the file tree (`editor.rs:643`, `706-757`) becomes its empty panel, with the same width and border.

  The tab row, the read-only and "Changed on disk" bars, and the "Click a file to open it" and binary-file messages
  stay as they are: a few short labels.
- **Card:** the card and the label.

### Note

- **Silhouette:** the note on screen's gpui-kit `Editor` (`note.rs`, `body`) becomes its word map's bars, in the
  same box below the tabs' place, which stays empty. The wheel stop isn't attached. A block with no note open is its
  tinted card, empty. Its title (the note's first line) is on the label.
- **Card:** the card and the label.

### Kanban block

At the Silhouette level a kanban block is **drawn in full** (`kanban.rs:104-124`):

- its columns, cards and label colors are what you recognize from afar;
- its cards' heights come from their wrapped titles, which only shaping knows, so bars couldn't keep them in place;
- a board is a few dozen short labels, with no editor and no highlighting.

At the Card level (under 25%) it is its card and label. If `--bench-far` shows kanban blocks cost too much, they
become Cards at the Silhouette level too ([Decisions](#decisions)).

A checklist block ([checklist.md](checklist.md)) is drawn in full the same way down to the Card level: a few short
rows, no editor; below Full its title is the fixed-size label, as every block's.

### Drawings and the dot grid

They need nothing, down to 1%. Drawings are already culled (`ink.rs:77-87`), drawn through points at least 2 px apart on screen
(`ink.rs:47-48`), and rebuilt at most 24 per frame when the zoom reaches another step (`ink.rs:58-60`, `148-175`);
they have no text; a line is never under 1 px. The dot grid thins its dots as the zoom goes down
(`dots.rs`): 3,072 units apart at 1%, 30.72 px. Wires stay 1 px wide with 2.5 px dots, and their labels go at the
Card level, as a block's text does.

## 3. Editors and notes: the word map

A note is soft-wrapped (`note.rs:51`), and an editor tab can be (`editor.rs:432`). Its rows depend on where gpui-kit
wrapped the text, which only gpui-kit knows. The bars must sit on those rows.

**Why not gpui-kit's own wrap rows.** gpui-base keeps them in a `DisplayMap` (`gpui-base-0.7.1
src/input/editor/display_map/mod.rs:19`), but `EditorState` holds it in a `pub(super)` field with no accessor
(`src/input/base/state.rs:356`), and its last layout too (`src/input/base/layout.rs:14-33`). Wrapping again in
Templum would mean porting gpui-base's private `measured_wrap_boundaries`
(`display_map/text_wrapper.rs:32-103`), which shapes the text to measure it (`text_wrapper.rs:407-429`): the very
cost we remove, plus a copy to keep in step with every gpui-kit upgrade.

**What the code does support.** `EditorState` makes public where its last frame put things: `visible_row_range()`,
`scroll_offset()` and `line_height()` (`state.rs:2992-3013`), `text_bounds()` (`state.rs:575-577`), and
`range_to_bounds(range)`, the on-screen bounds of any byte range of the laid-out lines (`state.rs:3635-3658`, through
`line_and_position_for_offset`, `state.rs:916-938`). gpui-kit itself uses it to place things over its text
(`src/input/base/element.rs:1511`, `1595`). The word map is built from it: **the row geometry of the last Full
frame**.

**Taking it.** On the first frame a body is drawn below Full, if its editor was laid out before (`line_height()` is
`Some`), Templum asks `range_to_bounds` for each run of non-blank characters of each laid-out line, from the first
line on screen until it returns `None`, and keeps:

- each word's **row**, counted from the first row on screen, and how much of that first row is above the top (the
  scroll's remainder, in rows, from `scroll_offset()` and `line_height()`);
- each word's **span across**, in world units from the text's left edge: its pixels divided by the zoom the body
  was last drawn at (kept on the body like the preview's last zoom, `note.rs:211-217`).

Because it is gpui-kit's own layout, it holds whatever gpui-kit did: soft wrap, a folded section in a note, tabs, a
horizontally scrolled long line. It costs one `range_to_bounds` per word on screen, once, not per frame.

**Drawing it,** at the body's settled zoom `s`:

- rows are `round(1.5 × 13 × s)` px apart: the editor's own rows (`block.rs:585-592`);
- a word sits at the text area's left edge plus its span × `s`;
- where the text area starts at `s` is computed the way Full lays it out. In a note, `TEXT_LEFT × s` and
  `FIXED_TOP × s` from its box: the note's compensation makes both scale (`note.rs:26-34`, `144-149`). In an editor
  tab, gpui-kit's fixed 6 px left and 8 px top padding (`gpui-component-0.7.1 src/input/input.rs:591-602`), then the
  line numbers: their digits at `s` plus a fixed 6 px margin (`gpui-base-0.7.1 src/input/base/element.rs:50-53`,
  `1090-1125`); editor tabs have no fold gutter (`editor.rs:434`);
- a bar is half a row high, centered, in the editor's text color at 60%. **One color:** the laid-out runs and their
  colors are private (`layout.rs:14-33`), and getting them again would run the tree-sitter queries we are removing.
  The line numbers aren't drawn.

**When it goes stale.** A word map remembers what it was taken of: the text's length, the body's size, the font,
the active tab and its wrap and preview flags. It is taken again when one of them differs, when a reload from disk
replaced the text (`Tab::reload`, `editor.rs:285-301`, which `check_files` runs for unfocused editors,
`workspace/files.rs:421-449`; a reload can keep the length), and when a body has none yet (a canvas restored at 25%).
Then the body is drawn Full once at opacity 0, which GPUI still lays out (`gpui-pre-0.3.8
src/window.rs:4078-4094`), with the old map's bars over it, if any, so nothing blinks. A frame is asked for, and the
next frame takes the map. With an agent writing a file shown far away, that is one Full frame per change on disk.

## 4. Focus and interaction

**Rules**

1. **The block you work in has its letters,** where it would have bars: the focused block (`space.focused`,
   `block.rs:91`), and any block where something holds the keyboard. That is checked with each focus handle's `is_focused` (the editors'
   handles, a terminal's find field), which compares with the window's focus
   directly (`gpui-pre-0.3.8 src/window.rs:507-509`), not through the last frame's element tree. Why: GPUI drops
   the focus of an element that isn't drawn (an empty focus path fires "focus lost", `window.rs:3393-3431`), and
   typing, the IME and the cursor need the Full body. Under 25% (the Card and Shape levels) it stays a card: its
   letters would be specks under 2.5 px, and laying them out would rasterize text at every step under 25%. A
   focused terminal still takes the keys there (the workspace holds the keyboard for it); an editor or a note
   drawn as a card gets its typing again once it is drawn (GPUI keeps its focus, only the field isn't there).
2. **A press anywhere on a block focuses it first** (`block.rs:465-471`), so any interaction turns it Full on the next
   frame: a drag by its label, a resize (`block.rs:415-423`, `573-580`), its menu (`block.rs:481-488`), a click into
   an editor.
3. **A body drawn as bars or as a card takes no wheel:** the wheel moves the canvas, as over the title bar today
   (`block.rs:455-458`, `workspace/input.rs:532-556`); Ctrl+wheel and pinching zoom, as everywhere. Focusing the block
   gives its content the wheel back. There is no gpui-kit `Editor` to scroll, and a terminal's history at a size
   nobody reads is better left still ([Decisions](#decisions)).

**What must keep working** (each has a test or a manual check):

- Typing into a focused terminal, editor or note at 25%, with dead keys and the IME.
- Hover: the lift, the close button, the resize strips and grip.
- Moving a block by its label; a double-click on the label frames the block ([navigation.md](navigation.md#framing-a-block)).
- Resizing from the edges and the grip.
- Every block's right-click menu ([context-menus.md](context-menus.md)).
- Ctrl+click on a path or a URL in an unfocused far terminal: the Silhouette keeps the press handler, which reads the
  grid (`block.rs:338-349`).
- Selecting text in a far terminal: the press focuses it (Full) and starts the selection.
- A click on a far editor or note focuses it; the cursor goes where the next click lands, since there was no
  `Editor` to place it. Today one click does both ([Decisions](#decisions)).
- Dragging kanban cards and columns at far zoom: the board is drawn in full there.
- "Go to the terminal waiting for you": `reveal` focuses the terminal and moves the camera (`view.rs:17-39`,
  `workspace/blocks.rs:145-173`), so it is Full.
- Find (Ctrl+F) acts on the focused terminal (`workspace/find.rs:30-37`), which is Full. A field left open on a
  terminal that lost the focus is hidden below Full and comes back with its text and match.
- No command moves the focus between blocks by keyboard today; the workspace's shortcuts don't depend on levels.
- The drawing tools and the hand sit above the blocks (`render.rs:337-389`): unchanged.
- Blocks popping in and closed blocks fading out (`render.rs:246-264`) are drawn at the same levels.

## 5. State across levels

Every block's state lives in its `Body`, not in what is drawn, so Full → Silhouette or Card → Full keeps it:

| State | Where it lives | Across levels |
|---|---|---|
| An editor's text, cursor, selection, undo history, open tabs, tree | the tabs' `EditorState` entities, `Editor`, `TreeState` (`editor.rs:245-259`, `312-329`) | Kept: only the `Editor` element isn't built. Its scroll is in pixels of the last Full rows; on the next Full frame `keep_rows` rescales it from `line_height()` (`editor.rs:545-558`), as after any zoom; the tree's, by `keep_list_rows` (`editor.rs:563-569`, `723`). |
| A note's text, cursor, undo, preview scroll | `Note` (`note.rs:37-43`, `211-217`) | Kept, the same way. |
| A terminal's scrollback position and selection | alacritty's `Term`, the snapshot | Kept: the bars are drawn at the same position. |
| A terminal's find field | `Pane::find` (`workspace/mod.rs:113-114`) | Kept, hidden below Full. |
| A kanban column's scroll | GPUI element state (`kanban.rs:202`, `277`, `403`) | Drawn in full at the Silhouette level, so kept; at the Card level, under 25%, the board isn't drawn and GPUI drops it: the columns are back at their tops when it is drawn again. |

## 6. Cost

What a frame builds for one block:

- **Terminal, Full (80 × 24):** 24 `shape_line` calls, each hashing its row's text and runs into GPUI's two-frame
  cache (`gpui-pre-0.3.8 src/text_system/line_layout.rs:459-460`; a changed row is shaped again), and one
  `paint_glyph` per non-blank cell, with raster bounds, an atlas lookup and a sprite (`window.rs:4687-4760`): up to
  1,920 a frame.
- **Terminal, Silhouette:** one `paint_quad` per word, which snaps the bounds and adds the quad to the scene
  (`window.rs:4550-4567`). That is about 10 per row of prose (the mixed bench's lines have 12, `bench.rs:23-25`):
  about 250 for a full screen, at most 960 (every other cell). Backgrounds, selection, marks and the cursor are
  quads already. No shaping, no glyph lookups, no `TextRun` vectors.
- **Editor or note, Full, even idle:** gpui-kit's input writes its state on every render (`gpui-component-0.7.1
  src/input/input.rs:540-556`); prepaint sets the font and prepares the text (`element.rs:2528-2531`), and a
  tree-sitter query highlights the lines on screen (`element.rs:2121`); the lines and their numbers are shaped
  (cached when unchanged). After painting it notifies when its bounds moved (`element.rs:3274-3295`): every pan
  moves every editor, and each one asks for one more frame of the whole workspace. A new font size re-wraps a note's
  whole text (`display_map/text_wrapper.rs:337-345`).
- **Editor or note, Silhouette:** one quad per word on screen: about 28 rows and 150 to 250 quads for a 900 × 600
  editor, about 80 for a 360 × 280 note. No `EditorState` update and no notify.
- **Any block at the Card level:** the card and one label, a single short line shaped.
- **Any block at the Shape level:** the card, and a terminal's dot: no text at all.

At 25% a 1280 × 800 window shows about 20 blocks of the default sizes: today 20 full bodies, after this a few
thousand quads and 20 labels. At 1% it shows 10,000 times the area of 100%: every block is a few pixels, its shape.

## 7. Architecture

| File | Change |
|---|---|
| `templum-canvas/src/lod.rs` (new), `lib.rs` | `Level`, `level`, `block_level`, `GREEK_BELOW`, `CARD_BELOW`, `LABEL_FITS`, with their tests. |
| `templum-canvas/src/camera.rs`, `flight.rs` | The steps under 25%; a glide's zooms on multiples of 0.02 from 16% up, as before, and under it of a step that halves as the zoom does (0.00125 under 2%): 0.02 would round 1% to nothing. |
| `templum-terminal/src/snapshot.rs` | `Row::bars()`, with its tests. |
| `templum/src/workspace/mod.rs`, `render.rs` | `Space::rest`, set where the view is decided. |
| `templum/src/block.rs` | The level of each block (and the focus rule); the label; the header's buttons hidden; no wheel handlers below Full; Card bodies; the level passed to the bodies. |
| `templum/src/grid.rs` | Bars painted in place of shaped rows. |
| `templum/src/silhouette.rs` (new) | The word map: taken from an `EditorState`, its staleness, its bars at a zoom (the math is pure and unit-tested here). |
| `templum/src/editor.rs`, `note.rs` | The word map and the last-drawn zoom on each tab and note; the bodies below Full; `Tab::reload` drops the map. |
| `templum/src/bench.rs`, `main.rs` | `--bench-far`. |

## 8. Bench

**`--bench-far`** (new, next to `--bench-mixed`, `bench.rs:27-124`; flag in `main.rs:39-42`) reproduces the
complaint: an agent cockpit seen from afar.

- **Canvas:** 21 blocks, 6 per row in the mixed scenario's 950 × 750 slots (`bench.rs:34-36`): 8 terminals (5 run the
  mixed scenario's `TUI_LOOP`, `bench.rs:23-25`; 3 `cat` a large file), 6 editors each with one of the folder's
  largest text files open (`sample_file`, `bench.rs:373-401`, extended to the n largest), 4 notes holding the
  folder's `README.md` (as `--bench-zoom=note` does, `bench.rs:159-166`), and 3 kanban
  blocks with `sample_board` (`bench.rs:403-420`). **No block focused**: the workspace keeps the keyboard, so every
  block is drawn at its level.
- **Drive:** the camera rests at 33% for 3 s, then glides to 25% and rests there for 2 s, then glides out a step every
  0.25 s, as the wheel does, down to 1% (at 7 s) and rests there, while panning in a circle of 1,500 world units
  around the canvas's center, so blocks keep entering and leaving the view. (Until card 227 it rested 5 s at 33%
  and 5 s at 25%.)
- **Run** from a real project folder, like `--bench-zoom`:
  `cd ~/Documents/github/DaemonQuest && ~/Documents/github/Templum/target/release/templum --bench-far`.
- **Output:** the same JSON line as every scenario (`measure`, `bench.rs:333-371`).

The plan adds `--bench-far` first and measures it on `development` before any level is drawn, then after. **Bar:**
p95 ≤ 16.7 ms on `--bench-far`, and the existing scenarios no worse than before (success criterion 4).
`--bench-zoom=editor` and `--bench-zoom=note` sweep 25% to 200% and show the far end improve too.

## Testing

Tests are written first, as always.

**Unit, `templum-canvas` (`lod.rs`):**

- each default size (12, 13, 14) is Silhouette at 25% and 33%, and Full from 50% up;
- no zoom step puts a default size within 0.25 px of a threshold, so float error can't flip a level;
- 9-pt text is Card at 25% and 10-pt is Silhouette; 17-pt is Full at 33%;
- every default size is Card at every step under 25%;
- a block under 40 × 24 px on screen is its Shape whatever its text; the smallest note holds its label at 25%.

**Unit, `templum-canvas` (`camera.rs`, `flight.rs`, `dots.rs`):** the steps reach 1%, 1.33 or 1.5 times apart under
25%, and Fit and a saved zoom reach them; a glide between 1% and 1.5% draws zooms between its ends, on multiples of 0.00125
(0.02 from 16% up); dots are at least 24 px apart at every step and under 3,800 on a Full HD screen at every zoom a glide draws.

**Unit, `templum-terminal` (`snapshot.rs`):** `Row::bars` for red text then plain text (two bars, their columns
and colors); a colored run with a space inside (two bars); leading blanks (none); a multi-byte character counts one
column; a dim run stays dim; a wide character's bar is one column; an empty row has none.

**Unit, `templum` (`silhouette.rs`):** a word map's bars at a zoom: the row pitch, the first row's share above the
top, the text area's origin for a note and for an editor tab with 3 and 4 line-number digits.

**Headless UI (`workspace/tests/lod.rs`, new):** the test platform lays text out (0.6 em per character,
`gpui-pre-0.3.8 src/platform.rs:1552-1590`), so positions are real numbers.

- An unfocused note and editor at 33% build no gpui-kit `Editor` (no `("note-text", n)` or `("code", n)` element)
  and paint bars (a `("silhouette", n)` element); at 50% they build the `Editor`.
- A focused editor at 25% is Full and typing reaches it; a focused terminal at 25% gets keys and draws its cursor;
  when the focus moves to another block, they are drawn as bars on the next frame.
- The level doesn't change mid-glide: from 50% to 33%, every frame before the glide ends has the `Editor` and the
  frame it ends has bars; from 33% to 50%, the other way around.
- **Bars line up with letters at every step of `ZOOM_STEPS`:** a scrolled soft-wrapped note with a folded section,
  and a scrolled editor tab with a horizontally scrolled long line. At each step, each word's bar against the
  word's `range_to_bounds` with the block focused (letters): within 1 px.
- A terminal's bars sit on its cells: colored output at 25%, each bar the cell rectangle of its run.
- At 25% the label's text is 12 px and the dot 8 px on screen; at 100% the header is as today.
- Under 25%: a terminal at 8% has its label; at 4% and 1% it is its shape (no label, the title covering the card,
  the dot in its middle, no buttons, strips nor grip on hover); a press on a shape at 1% moves the block and a
  double-click frames it; a focused terminal at 16% is a card and takes the keys; a kanban block and an editor at
  16% draw nothing inside; a canvas saved at 6% comes back at 6%.
- State survives Full → Silhouette → Full: an editor's rows shown, cursor, selection, undo and tabs; a note's rows;
  a terminal's scrollback position.
- The word map follows the text: a far editor whose file changes on disk shows the new text's bars a frame later;
  a canvas restored at 25% shows bars after its first frames.
- At 25%: the wheel over a far terminal and over a far editor pans the canvas, and Ctrl+wheel zooms; after a click
  the wheel scrolls the block again; Ctrl+click on a URL in an unfocused far terminal opens it (like
  `ctrl_click_on_a_url_opens_the_browser`, `workspace/tests/terminals.rs:909`); a drag on the label moves the block
  and a double-click frames it; a right-click opens its menu; "Go to the terminal waiting for you" focuses a far
  waiting terminal, drawn Full; a find field hides when its terminal loses the focus at 33% and comes back with its
  text.
- The existing zoom tests that look at a note's letters at 25% or 33% (`workspace/tests/navigation.rs:758-853`) use
  a note just added, which holds the keyboard, so they still measure letters; a test that doesn't gets the focus
  explicitly.

**Manual (Hyprland):** a real cockpit (Claude Code in three terminals, Codex in one, editors on files the agents
edit, notes, a kanban block) at 33% and 25%: the bars look like the text, the colors read, nothing
flickers during glides, and nothing moves when a glide ends or when zooming back in (a screen recording of 50% ↔ 33%,
watched frame by frame); typing into a far terminal with dead keys and fcitx5 at 25%; the status dots read at 25%;
light and dark themes; the benches.

## Risks

- **Mirrored gpui-kit sizes.** The word map's text origin repeats gpui-kit's fixed paddings and line-number margin,
  as `note.rs` already does. An upgrade that changes them misplaces the bars by a few pixels; the alignment test
  then fails, in the upgrade's own PR (ADR-0002).
- **Refresh frames.** A canvas restored at 25% with many editors pays one Full frame per editor at startup, as
  today; an agent rewriting a far file costs one Full frame per write.
- **Far terminals still ask for frames.** A Silhouette terminal is re-armed when painted (`grid.rs:182`), so its
  output still wakes the UI; each frame costs less, there are as many.
- **The label covers the top of a far body** (two or three terminal rows): the price of a title you can read.
- **Small inexactness accepted:** a wide character's bar is one column; editors' line numbers aren't drawn.

## Decisions

The code couldn't settle these; the maintainer chose on 2026-10-13:

1. **A far body is a picture.** Over bars or a card, the wheel moves the canvas and a click only focuses; today
   the wheel scrolls a far terminal's history or a far editor, and one click both focuses an editor and places its
   cursor. *Decision:* accepted. At 33% and 25% the canvas is mostly blocks, and a wheel that pans everywhere is
   what navigating there needs. The alternative, keeping the wheel on terminals only (cheap: their bars follow the
   scroll), makes the wheel act differently over a terminal and over an editor.
2. **The kinds without bars.** A Markdown note or file shown rendered is an empty (tinted) card; a kanban block is
   drawn in full. *Decision:* as written. Revisit kanban if `--bench-far` shows it costs.
3. **The threshold.** 5.5 px switches every default block between 50% and 33%. InDesign's default, 7 px, would turn
   editors and notes to bars at 50% while 14-pt terminals keep their letters there. *Decision:*
   5.5 px.

4. **Under 25% (card 227, 2026-10-26).** The person asked for zoom steps down to 1%. *Decided while implementing:*
   the focused block is lifted from bars to letters but not from a card (its letters would be specks, and every
   step under 25% would rasterize text); a block too small for its label is its shape, its title in a tooltip.

## Documentation

The plan that implements this updates:

- `docs/ARCHITECTURE.md:52`: "far-away blocks are drawn simplified (level of detail)" becomes the rule here (the
  three levels, picked by the text's size on screen when the camera rests, the focused block always Full), and the
  `templum-canvas` row of the crates table gains "level of detail";
- `docs/specs/walking-skeleton.md:35`: the "level-of-detail drawing at low zoom" item leaves "Later" and points here;
- `CLAUDE.md`'s performance rule (`:48`): `--bench-far` for changes to drawing at far zoom.

## Delivery

1. This spec.
2. A plan: `--bench-far` (measured before anything else), the level and `Space::rest`, the label, terminal bars,
   Cards for every kind, the wheel rule.
3. A plan: the word map for editors and notes, rendered Markdown, the Git block's scroll handles, the
   documentation.
