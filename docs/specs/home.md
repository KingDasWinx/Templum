# Home: the starting point

- **Date:** 2026-10-27
- **Status:** Implemented (kanban card 256, "a home screen to be the starting point and general control of all the
  workspaces"; card 265, "the Home screen must be a real screen, not a modal; it must be Templum's initial screen when
  it opens"; card 264, stopping the last workspace running lands on it)
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

The Home is a place of its own, a peer of the workspaces: Templum shows either the Home or one workspace's canvas,
never one over the other.

- **At launch:** every start of Templum with a data folder opens on the Home. The workspace shown last is loaded under
  it, unseen, as a hidden workspace is: its terminals and agents run, at their sizes, but none of its blocks is laid out
  or drawn.
- **Not on a first run:** the welcome comes then ([welcome.md](welcome.md#when-it-shows)) and its closing opens the
  first terminal, not the Home: there is nothing yet to look at. Never over the welcome: showing the Home closes it.
- **Not without a data folder** (`--bench`): nothing to come back to.
- **Again:** the **Home** item atop the sidebar (a house), the workspace's name in the title bar, and the `workspaces`
  command (Ctrl+Shift+O, Cmd+Shift+O on macOS). On the Home they do nothing more: it is shown.
- **When no workspace runs:** stopping the last workspace running, the only one included, shows the Home
  ([workspaces.md](workspaces.md#workspaces)).
- **Leaving it is choosing a workspace:** its card, its sidebar tile, Ctrl+1–9, Ctrl+Tab and Ctrl+Shift+Tab (the
  first and the last workspace: the Home sits above them in the sidebar), New workspace, Open folder…, a recent
  folder, or anything that goes to a block (a notification's click, Go to the terminal waiting, the API's `show`). A
  stopped workspace chosen starts. The switch is the workspaces' own: a plain swap, no motion.
- **Esc doesn't leave it:** the Home is a place, not a popup. Esc empties the search.

### What stays and what goes

| On the Home | Why |
|---|---|
| The sidebar, with **Home** marked as the workspace shown is (the hover fill, a hairline, the accent bar at its left); no tile marked | The Home is the place shown |
| The title bar: the house and "Home" in place of the workspace's name; no zoom level | The Home has no canvas to zoom |
| No toolbar, no Templum Agent pill or chat, no mirror banner | They work on a canvas: an approval the Templum Agent asks comes as a notification, whose click opens the chat in the workspace held |
| **Agents** and **Automations** (sidebar buttons, shortcuts) open over the Home | They list every workspace; going to an agent shows its workspace |
| **Requests** shows the workspace of the oldest request waiting (none: the one held) with its popup; **Git** shows the workspace held with its Git | Requests and Git are a workspace's |
| The Git and Requests popups close when the Home is shown; Agents and Automations stay open, as across a workspace switch | |
| Settings, the theme picker, the usage popover and the menus open over it | Window chrome |

## What it holds

Top to bottom, in a centered column ([Screens](design-system.md#screens)):

- **The header:** a greeting by the local hour ("Good morning" from 5:00, "Good afternoon" from 12:00, "Good evening"
  from 18:00) over what Templum holds: "4 workspaces · 3 agents at work" (the agents working or waiting for their
  subagents, in every workspace that runs). At its right: **New workspace** (the tinted primary: as the sidebar's +, it
  opens the New workspace dialog, whose Create makes the workspace and shows it, see
  [cockpit.md](cockpit.md#6-the-workspaces-screen)), **Open folder…** (the system's folder picker: the
  workspace whose folder it is is shown; else a new workspace named after the folder, with that folder, is made and
  shown), **Connect to a Templum…** (plug) and **Settings** (gear).
- **Waiting for you:** four tiles, what needs the person across every workspace, each a press away:

  | Tile | Counts | A press |
  |---|---|---|
  | Agents need you (yellow) | Agents whose state is Needs you ([agents-view.md](agents-view.md#an-agents-state)) | Shows the workspace of the first one (the one shown first, else the sidebar's order; the longest waiting), opens the Agents popup on the Needs you chip with it selected |
  | Agents done (accent) | Agents whose turn ended unseen (the Agents button's badge) | The same, on the Done chip, the newest |
  | Requests wait (yellow) | The inbox's requests of every workspace | Shows the workspace of the oldest and opens the Requests popup on it |
  | Automations went wrong (red) | Automations whose last run failed, was skipped or missed since the popup was opened (the Automations button's badge) | Opens the Automations popup on the first one, in its workspace when it names one |

  A tile at zero is calm (dim, no pointer, no press). The words follow the count: "1 agent needs you", "2 agents need
  you", "1 request waits", "3 automations went wrong".
- **Workspaces:** at the right of the section's name, on its line, the search ("Find by name or folder", the
  [search field](design-system.md#search-fields)): what it finds is workspaces, so it sits with them. It filters the
  cards as you type (the name, or the folder as written or with `~`), whatever the case. Then a card per workspace in
  the sidebar's order, in a grid of as many 220 px columns as fit (at most
  four). A card has the workspace's tile (icon, image or initials, its color), its name, its folder ("~/…"), then:
  - what it holds, as before: "3 terminals · 1 note · 2 boards"; "Stopped · 2 terminals"; "Empty"; a mirror's "Mirror
    of winx-desktop · live · 3 terminals" (the computer and the connection's state: `connecting`, `live`,
    `reconnecting`, `cut`, `refused`, `disconnected`);
  - its agents, when any: a green dot and "2 Claude Code running" ("working" on a mirror), a yellow dot and "1 needs
    you";
  - when it was last opened: "Opened 3 h ago", "Opened yesterday"; nothing for one not opened since this version;
  - a ⋯ button at its top right with the right-click menu (Open, Edit…, Stop, Delete…; a mirror's Open, Edit…,
    Disconnect or Connect, Forget…): the sidebar tile's actions.

  No card is marked current: on the Home no workspace is shown. A stopped one, or a mirror that isn't live, is dimmed as
  its tile is. A click opens it (a stopped one starts); dragging a card onto another reorders, as the sidebar does.
  With no match: "No workspace matches “api”".
- **Recent folders:** the folders the workspaces opened lately (Git, agents, editors, terminals; the one shown first,
  then the sidebar's order), six at most, leaving out every workspace's own folder and the mirrors'. Each is a chip:
  the folder's name, its path in the tooltip; a press makes a workspace on it, as Open folder… does. The section is
  left out when there are none.

## Keys and pointer

- The search has the keyboard: typing filters; Up and Down move the highlight a row of cards (Left and Right stay with
  the field); Enter opens the highlighted card, the first one shown until an arrow moves; Ctrl+1–9 open a workspace;
  Esc empties the search.
- Templum's own shortcuts still run from it: the Home's (nothing more), New workspace, Connect to a Templum…, next and
  previous workspace, the sidebar, Settings, the theme picker, Go to the terminal waiting, Agents, Automations, Quit.
- Should the keyboard be elsewhere on the Home (a popup or a dialog that closed gives it to the canvas, which the Home
  stands for), a key never reaches a block: the Home's keys run, and anything else gives the search the keyboard.
- The sidebar stays usable beside it. No canvas is under it: nothing takes a press, a wheel or a hover but the Home.

## Look

The [Screens](design-system.md#screens) standard. The tiles of Waiting for you are 4 columns (2 when the column is
under 960 px, so their words are never cut); the cards' grid is 1 to 4 columns. A card the keyboard is on is never
dimmed, so its highlight shows.

## In the API

The state says the Home is shown: `app.shown` is null and no workspace has `shown: true`; `view` is null (no canvas,
no camera), so `event: view` sends `null`. `overview` names no workspace shown. Nothing opens the Home from the API;
`show` leaves it for the workspace it names (or the block's), the one held under it included, as a notification's
click does. What it shows is the state's: `workspaces` (with `opened`, when each was last opened, and `stopped`), the
blocks' `agent.state` (`needs_you`, `done`), `requests`, `automations[].last` (`went`); a workspace's recent folders
are `recent_folders`. The "since the popup was opened" of the Automations tile is the sidebar badge's: it belongs to
this window, not to the state.

## Architecture

| File | Change |
|---|---|
| `home.rs` (was `spaces_screen.rs`) | `Screen` (the Home shown: its search, highlight, scroll), the counts, `show_home`, the keys, the tiles' presses, Open folder…, and the drawing: header, tiles, cards, recent folders |
| `sidebar.rs` | The Home item in place of the grid button, marked as the workspace shown is while the Home is |
| `title_bar.rs` | "Home" in place of the workspace's name, no zoom level |
| `workspace/spaces.rs` | The Home at launch (not on a first run); `switch_to` leaves it, the workspace held under it included (`hold_space` keeps one there, `leave_space` ends what only makes sense on screen); `shown_space` is none on it; stopping the last workspace running shows it; `opened` stamped when a workspace is shown, kept in the index |
| `workspace/render.rs` | The Home in the canvas's place beside the sidebar (`canvas_area` isn't built), under the popups, the theme picker, the usage popover and the menus; the Templum Agent hidden; the Home's root holds the canvas's focus, so no key reaches a block |
| `workspace/api_state.rs`, `control.rs` | `app.shown` null, no workspace `shown`, `view` null and no ", shown" in `overview` on the Home |
| `session.rs` | The index's `opened` (Unix seconds; one that doesn't read is none) |
| `workspace/api_state.rs`, `templum-api` | `workspaces[].opened` |
| the popups' `open_*`, `git_ops.rs`, `chat/mod.rs`, `workspace/view.rs` | Agents and Automations open over the Home; Requests, Git, the Templum Agent's review and revealing a block leave it for a workspace (`switch_to`) |

## Testing

Headless UI (`workspace/tests/home.rs`): the Home shows at launch with a saved workspace held under it, its terminal
running at its size, and no canvas, block, toolbar, pill or zoom level drawn; the title bar says Home and the sidebar
marks the Home, no tile; Esc stays on it; a card shows its workspace with its canvas and the keyboard; not on a first
run nor without a data folder; the sidebar's Home item, the title bar's name and the command show it (again: nothing
more); Ctrl+1 and a tile leave it; next and previous go to the first and the last; the API's state (`app.shown` null,
no workspace shown, no `view`) and `show` leaving it; the search by name and by folder, Esc emptying it; Up, Down and
Enter; Settings, Agents and Automations over it, Requests and Git in a workspace; each tile's count and its press; a
card's words; the ⋯ opens the workspace's menu; a recent folder and Open folder…; keys never reach the canvas.
Stopping (`workspace/tests/workspaces.rs`): the last workspace running and the only one land on the Home. Unit: the
greeting by hour, the columns by width, the tiles' words, the card's lines.
