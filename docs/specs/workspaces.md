# Workspaces

- **Date:** 2026-10-10
- **Status:** Implemented
- **Builds on:** [persistence.md](persistence.md) (one saved canvas, `workspaces/default.json`) and
  [polish.md](polish.md) (the look)

## Goal

Templum gets any number of workspaces. Each one is a whole canvas, with its own blocks, camera and focus, such as
"work" and "personal", or one per project. A thin sidebar on the left shows them all and switches between them.
Terminals in the workspaces you aren't looking at keep running, so an agent keeps working while you look elsewhere.

**Success criteria**

1. Creating, renaming, deleting and switching workspaces works on Linux, macOS and Windows. CI checks this on every
   OS, and Linux (Hyprland) is also checked by hand.
2. Every workspace comes back after a restart, with what [persistence.md](persistence.md#what-comes-back) restores
   for one canvas.
3. A canvas saved by the previous version opens as the first workspace, with nothing lost.
4. Terminals in a hidden workspace keep running and cost no frames: they don't wake the UI.
5. All tests in [Testing](#testing) pass in CI on the 3 OS, and `--bench` stays at p95 ≤ 16.7 ms.

## Out of scope

- Dragging tiles to reorder workspaces, icons or colors per workspace, and moving a block to another workspace: all
  three came later with the workspaces screen ([cockpit.md](cockpit.md#6-the-workspaces-screen)), where **Rename…**
  became **Edit…**.
- More than one window.
- Kanban boards: the next sub-project.

## The sidebar

- **Where:** a 56 px column on the left of the window, under the title bar, next to the canvas.
- **Home** (a house) at its top, above the tiles: shows the [Home](home.md), Templum's first screen, in the canvas's
  place. While the Home is shown this item is marked as the active tile is (below), and no tile is; a tile shows its
  workspace in the Home's place.
- **Tiles:** one per workspace, in order, 40 px square with a 10 px radius, showing the first letter of its name
  (two for a two-word name: "Side project" is "SP").
  - The active tile (the workspace shown; none on the Home) has the `hover` fill, a hairline border and a 3 px accent
    bar on its left.
  - Hovering any tile fills it with `hover`, and shows its full name in a tooltip.
  - A tile gets a small accent dot when the last snapshot of its workspace found Claude Code or Codex running.
  - A tile of a workspace another Templum mirrors now (a share's event stream is open) wears a small Lucide `cast`
    mark at its bottom-right, in the accent; its tooltip says "Mirrored now on Another computer (203.0.113.7)", a
    line per mirror ([templum-to-templum.md](templum-to-templum.md)).
- **A remote workspace's tile** (a mirror of another computer's workspace,
  [templum-to-templum.md](templum-to-templum.md)) wears home's color and icon, a small Lucide `globe` mark at its
  bottom-right and, while it isn't live, is dimmed. Its tooltip is "API, on winx-desktop · 84 ms". It is dragged,
  Ctrl+Tab'd and Ctrl+1–9'd like any tile; its right-click menu is Open, Edit…, Disconnect (or Connect) and Forget….
  **Edit…** keeps home's name ("named on winx-desktop") and the person's own icon and color (none: home's), and shows
  the connection with **Paste a new code…**; it has no Folder, Image or Sharing. It has no canvas file: only its index
  entry.
- **+** at the bottom creates a workspace and switches to it. Under it, a plug button, **Connect to a Templum…**,
  opens the dialog that turns a connection code into a remote workspace.
- **Right-click a tile:** a menu with **Rename…** and **Delete…**.
  - Rename opens a dialog with a text field. Enter or **Rename** applies it, Esc cancels.
  - Delete asks first: "Delete *name*? Its terminals will close." It is disabled while only one workspace exists.
- **At the foot, apart:** the Agents button ("Agents"; [agents-view.md](agents-view.md)) above the requests inbox
  button ("Requests"; [ask-inbox.md](ask-inbox.md)) above the Automations button ("Automations";
  [automations.md](automations.md); a red badge counts the automations whose last run failed, was skipped or was
  missed since the popup was last opened) above the Git button ([git.md](git.md)). A yellow badge at the
  Requests button's top right counts the requests waiting in the workspace shown, "99+" past 99, and the button
  pulses for 3 seconds when one arrives there (not with reduced motion). An accent badge at the Agents button's top
  right counts the agents of every running workspace that finished unseen, "99+" past 99; it never pulses.
- **Collapsing:** the title bar gets a button on its left (gpui-kit's panel icon) that hides or shows the
  sidebar. The canvas then takes the whole width. Whether it is shown is kept across restarts.

## Workspaces

- **A new workspace** is named "Workspace *n*", with the smallest *n* not in use, and starts as an empty canvas
  with its hint.
- **Names:**
  - surrounding spaces are trimmed;
  - an empty name is refused, and the dialog stays open;
  - two workspaces may share a name.
- **Switching:**
  - shows the other canvas at once, with its camera, blocks and focus;
  - the keyboard goes to its focused terminal;
  - a drag in progress ends, and a block still fading out of the old canvas is dropped;
  - from the Home it is the same plain swap, to the workspace held under it too ([home.md](home.md#when-it-shows)).
- **Hidden workspaces keep running.** Their terminals stay alive, their output is still read, and agents keep
  working. Their blocks aren't drawn, and their output doesn't wake the UI.
- **A theme, font or terminal setting change** applies to every workspace's terminals, hidden or not.
- **Deleting** closes the workspace's terminals and keeps its file as `<id>.json.bak`. If it was active, the one
  before it becomes active (on the Home, the one held under it: the Home stays).
- **The title bar** shows the active workspace's name where it said "Templum"; on the Home, a house and "Home".

## Shortcuts

| Command | Linux, Windows | macOS | What it does |
|---|---|---|---|
| `next_workspace` | Ctrl+Tab | Ctrl+Tab | The next workspace, wrapping around; from the Home, the first |
| `previous_workspace` | Ctrl+Shift+Tab | Ctrl+Shift+Tab | The previous one, wrapping around; from the Home, the last |
| `new_workspace` | Ctrl+Shift+N | Cmd+Shift+N | Creates a workspace and switches to it |
| `toggle_sidebar` | none | none | Hides or shows the sidebar |

- They are commands like the others, so `[keys]` in `config.toml` and the settings panel can change them.
- **Ctrl+1 to Ctrl+9** (Cmd on macOS) jump to the workspace at that position. They are fixed, and they don't
  apply when a command uses the same keys.
- Terminals rarely see these keys: most terminals send plain Tab for Ctrl+Tab, and plain digits for Ctrl+digit.

## Files

- Each workspace keeps the file format of [persistence.md](persistence.md#the-file), unchanged, in
  `<data folder>/workspaces/<id>.json`.
  - An id is chosen once and never changes, so a rename doesn't move a file.
  - The first workspace's id is `default`, so the file the previous version wrote is already in place.
  - New ids are `w2`, `w3`, … (the smallest unused).
- **`workspaces/index.json`** lists them, in order:

  ```json
  {
    "version": 1,
    "workspaces": [
      { "id": "default", "name": "Main" },
      { "id": "w2", "name": "Side project" }
    ],
    "active": 1,
    "sidebar": true,
    "git_size": { "w": 820, "h": 600 }
  }
  ```

  `git_size` is the [Git popup's](git.md#1-layout-and-navigation) size the person dragged it to, in pixels; it is
  left out until they do, and a value that isn't two positive numbers is as if left out (a number too large for JSON, only a hand edit
  writes one, sets the whole index aside as any other unreadable index).

- **Without an index,** as on a first run or after the previous version, there is one workspace, `default`,
  named "Main". It opens the canvas in `default.json` if there is one.
- **When files are written:**
  - the index is written at once whenever it changes: create, rename, delete, switch, collapsing the sidebar, or
    letting go of the Git popup's edge after resizing it;
  - the canvases are written by the snapshot every 5 seconds and on quit, as before, each only when it changed.
- **Errors:**
  - an index that doesn't parse, or has a newer version, is renamed to `index.json.bak`, and Templum starts as
    without an index;
  - an index entry whose file can't be read is handled as [persistence.md](persistence.md#errors) does for its
    canvas, except that only that file stops being written: the other workspaces, the index and the boards keep
    saving (the same holds when a canvas's terminals can't all start, and for an unreadable index or boards file);
  - an entry with an id that isn't made of letters, digits, `-` and `_` is dropped, so an edited index can't name a
    file outside the folder.

## Architecture

| File | Change |
|---|---|
| `workspace.rs` | The canvas state moves into `Space` (name, id, canvas, terminals, focus, hover, animations, view, flight, last saved snapshot). `Workspace` holds the active `space` and the hidden `others`; switching swaps them. Theme, font and terminal setting changes reach every space. The snapshot covers every space. |
| `session.rs` | `Index` (the index file, read and written), `path(dir, id)`, `load(path)`. |
| `sidebar.rs` (new) | The sidebar: tiles, +, the context menu, the rename and delete dialogs. |
| `title_bar.rs` | The sidebar button on the left; the workspace's name in the middle. |
| `keys.rs` | The four commands. |

No dependency is added: the dialog, menu and tooltip are gpui-kit's.

## Testing

Tests are written first, as always.

**Unit (`session`):**

- an index written and read back is equal;
- no index gives one `default` workspace named "Main";
- a broken index becomes `.bak`;
- an id with a `/` or `..` is dropped;
- the next id is the smallest unused `w<n>`.

**Headless UI:**

- **Create and switch:**
  - a new workspace is empty and active;
  - switching back shows the first canvas and its terminal, still running;
  - Ctrl+Tab, Ctrl+Shift+Tab and Ctrl+2 switch.
- A hidden workspace's terminals get a theme change.
- **Rename and delete:**
  - renaming changes the tile and the title;
  - an empty name is refused;
  - deleting closes its terminals and keeps its file as `.bak`;
  - the last workspace can't be deleted.
- **Persistence:**
  - two workspaces with blocks, saved and reopened, come back in order, with the active one and the sidebar
    state;
  - a `default.json` from the previous version, with no index, opens as "Main".

**Manual (Hyprland):** three workspaces, `claude` running in a hidden one (its dot shows), quit and reopen.

## Delivery

1. This spec.
2. Move the canvas state into `Space`, with no change in behavior.
3. Workspaces: files, switching, commands, sidebar, dialogs, docs.

## Risks

- **Startup time grows** with every workspace's terminals, since all of them start at launch so their agents
  resume. A few dozen shells start in well under a second.
- **Memory** grows with hidden terminals (about 5 MB each with the default scrollback). This is the price of
  keeping agents running.
