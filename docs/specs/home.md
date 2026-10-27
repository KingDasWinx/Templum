# Home: the starting point

- **Date:** 2026-10-27
- **Status:** Implemented (kanban card 256, "a home screen to be the starting point and general control of all the
  workspaces")
- **Builds on:** [cockpit.md §6](cockpit.md#6-the-workspaces-screen) (the workspaces screen it replaces: its cards,
  search, keys, drag and drop and right-click menu), [welcome.md](welcome.md) (a first run's welcome comes first),
  [agents-view.md](agents-view.md), [ask-inbox.md](ask-inbox.md) and [automations.md](automations.md) (what waits, and
  the popups a press opens), [workspaces.md](workspaces.md#the-sidebar) (the sidebar), the Screens standard of
  [design-system.md](design-system.md#screens)

## Goal

Templum opened on the last workspace's canvas, and the only place that showed every workspace was a sheet over it.
The Home is the base: where Templum starts, where every workspace and what waits for the person in all of them are seen
at once, and where a workspace is started from. It stays simple: no new settings, no charts, no history beyond when
each workspace was last opened.

## When it shows

- **At launch:** every start of Templum with a data folder opens on the Home, with the workspace shown last loaded
  behind it (its terminals and agents running), so Esc or its card returns to it at once.
- **Not on a first run:** the welcome comes then ([welcome.md](welcome.md#when-it-shows)) and its closing opens the
  first terminal, not the Home: there is nothing yet to look at. Never over the welcome: opening the Home closes it.
- **Not without a data folder** (`--bench`): nothing to come back to.
- **Again:** the **Home** button atop the sidebar (a house; pressed while the Home is up), the workspace's name in the
  title bar, and the `workspaces` command (Ctrl+Shift+O, Cmd+Shift+O on macOS), each of which also closes it.
- **It goes** with Esc, a card, a sidebar tile, Ctrl+1–9 or Ctrl+Tab, or anything that shows the canvas: a popup
  (Agents, Requests, Automations, Git, from the sidebar or their shortcuts), Settings, a notification's click, the
  API's `show`, a new workspace.
- Nothing else is needed at launch and nothing is configured: the Home is where Templum starts, and one key or click
  leaves it.

## What it holds

Top to bottom, in a centered column ([Screens](design-system.md#screens)):

- **The header:** a greeting by the local hour ("Good morning" from 5:00, "Good afternoon" from 12:00, "Good evening"
  from 18:00) over what Templum holds: "4 workspaces · 3 agents at work" (the agents working or waiting for their
  subagents, in every workspace that runs). At its right: **New workspace** (the tinted primary: as the sidebar's +, it
  creates "Workspace *n*", shows it and opens its edit dialog), **Open folder…** (the system's folder picker: the
  workspace whose folder it is is shown; else a new workspace named after the folder, with that folder, is made and
  shown), **Connect to a Templum…** (plug) and **Settings** (gear).
- **The search:** "Find a workspace by name or folder"; it filters the workspace cards as you type (the name, or the
  folder as written or with `~`), whatever the case.
- **Waiting for you:** four tiles, what needs the person across every workspace, each a press away:

  | Tile | Counts | A press |
  |---|---|---|
  | Agents need you (yellow) | Agents whose state is Needs you ([agents-view.md](agents-view.md#an-agents-state)) | Shows the workspace of the first one (the one shown first, else the sidebar's order; the longest waiting), opens the Agents popup on the Needs you chip with it selected |
  | Agents done (accent) | Agents whose turn ended unseen (the Agents button's badge) | The same, on the Done chip, the newest |
  | Requests wait (yellow) | The inbox's requests of every workspace | Shows the workspace of the oldest and opens the Requests popup on it |
  | Automations went wrong (red) | Automations whose last run failed, was skipped or missed since the popup was opened (the Automations button's badge) | Opens the Automations popup on the first one, in its workspace when it names one |

  A tile at zero is calm (dim, no pointer, no press). The words follow the count: "1 agent needs you", "2 agents need
  you", "1 request waits", "3 automations went wrong".
- **Workspaces:** a card per workspace in the sidebar's order, in a grid of as many 220 px columns as fit (at most
  four). A card has the workspace's tile (icon, image or initials, its color), its name, its folder ("~/…"), then:
  - what it holds, as before: "3 terminals · 1 note · 2 boards"; "Stopped · 2 terminals"; "Empty"; a mirror's "Mirror
    of winx-desktop · live · 3 terminals" (the computer and the connection's state: `connecting`, `live`,
    `reconnecting`, `cut`, `refused`, `disconnected`);
  - its agents, when any: a green dot and "2 Claude Code running" ("working" on a mirror), a yellow dot and "1 needs
    you";
  - when it was last opened: "Open now" for the one behind the Home, else "Opened 3 h ago", "Opened yesterday"; nothing
    for one not opened since this version;
  - a ⋯ button at its top right with the right-click menu (Open, Edit…, Stop, Delete…; a mirror's Open, Edit…,
    Disconnect or Connect, Forget…): the sidebar tile's actions.

  The one behind the Home has the accent border and ring; a stopped one, or a mirror that isn't live, is dimmed as its
  tile is. A click opens it (a stopped one starts); dragging a card onto another reorders, as the sidebar does. With no
  match: "No workspace matches “api”".
- **Recent folders:** the folders the workspaces opened lately (Git, agents, editors, terminals; the one shown first,
  then the sidebar's order), six at most, leaving out every workspace's own folder and the mirrors'. Each is a chip:
  the folder's name, its path in the tooltip; a press makes a workspace on it, as Open folder… does. The section is
  left out when there are none.

## Keys and pointer

- The search has the keyboard: typing filters; Up and Down move the highlight a row of cards (Left and Right stay with
  the field); Enter opens the highlighted card, the first one shown until an arrow moves; Ctrl+1–9 open a workspace;
  Esc closes the Home.
- Templum's own shortcuts still run from it: the Home's (closes it), New workspace, Connect to a Templum…, next and
  previous workspace, the sidebar, Settings, the theme picker, Go to the terminal waiting, Agents, Automations, Quit.
- Should the keyboard be elsewhere while the Home is up (a dialog that closed), a key never reaches the canvas: the
  Home's keys run, and anything else gives the search the keyboard.
- The sidebar stays usable beside it. The Home occludes the canvas: no press, wheel or hover reaches a block under it.

## Look

The [Screens](design-system.md#screens) standard. The tiles of Waiting for you are 4 columns (2 when the column is
under 960 px, so their words are never cut); the cards' grid is 1 to 4 columns. A card the keyboard is on is never
dimmed, so its highlight shows.

## In the API

The Home is window chrome, like the popups: whether it is up is not in the API, and nothing opens it from there; the
API's `show` closes it, as a notification's click does. What it shows is the state's: `workspaces` (with `opened`,
when each was last opened, new with this change), the blocks' `agent.state` (`needs_you`, `done`), `requests`,
`automations[].last` (`went`); a workspace's recent folders are `recent_folders`. The "since the popup was opened" of
the Automations tile is the sidebar badge's: it belongs to this window, not to the state.

## Architecture

| File | Change |
|---|---|
| `home.rs` (was `spaces_screen.rs`) | `Screen` (the open Home: its search, highlight, scroll), the counts, opening and closing, the keys, the tiles' presses, Open folder…, and the drawing: header, tiles, cards, recent folders |
| `sidebar.rs` | The Home button in place of the grid button |
| `workspace/spaces.rs` | The Home opens at launch (not on a first run); showing a workspace, the one shown too, closes it; `opened` stamped when a workspace is shown, kept in the index |
| `workspace/render.rs` | The Home drawn under the theme picker, the usage popover and the menus; no key reaches the canvas while it is up |
| `session.rs` | The index's `opened` (Unix seconds; one that doesn't read is none) |
| `workspace/api_state.rs`, `templum-api` | `workspaces[].opened` |
| the popups' `open_*`, `workspace/view.rs` | Opening a popup or revealing a block closes the Home |

## Testing

Headless UI (`workspace/tests/home.rs`): the Home opens at launch over a saved workspace, not on a first run nor
without a data folder; Esc and a card return to the workspace behind; the sidebar's Home button, the title bar's name
and the command open and close it; the search by name and by folder; Up, Down and Enter; each tile's count and its
press (the right workspace shown, the right popup open on the right item); a card's words (blocks, agents running and
needing you, a stopped one, last opened); the ⋯ opens the workspace's menu; a recent folder and Open folder… make a
workspace on the folder, or show the one that has it; a popup opened from the sidebar closes it; keys never reach a
terminal behind it. Unit: the greeting by hour, the columns by width, the tiles' words, the card's lines.
