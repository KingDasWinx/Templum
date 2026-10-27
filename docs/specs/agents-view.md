# The Agents view: every agent, at a glance

- **Date:** 2026-10-24
- **Status:** Implemented (kanban card 153, "a global view of all agents"); plan:
  [2026-10-24-agents-view.md](../plans/2026-10-24-agents-view.md)
- **Builds on:** [ask-inbox.md](ask-inbox.md) (the Requests inbox: this popup is its sibling, drawn the same way),
  [usage.md](usage.md#what-a-person-sees) (the usage chip's data and words), [workspaces.md](workspaces.md#the-sidebar)
  (the sidebar; hidden workspaces keep running), [claude-subagents.md](claude-subagents.md) (an agent's subagents),
  [connections.md](connections.md) (an agent's state, a handed task), [shell-integration.md](shell-integration.md)
  (a terminal's news, alert.rs)

The person said "pode decidir" (decide it): the product calls below are the orchestrator's, marked *the
orchestrator's call*; what the designer settled is marked *the designer's call*.

## Intent

A person runs tens, soon hundreds, of agents across their workspaces. Today each one tells its state only on its own
block: a dot before its title, a notice when it asks something. To learn who is still working, who is idle waiting
for its subagents, who finished and hasn't been looked at, and who needs them, the person pans canvas after canvas
and workspace after workspace. Nothing says "this agent finished" once the person missed the moment.

The Agents view lists every running agent of every workspace in one popup, each with its state and how long it has
been in it, what it works on, and what it costs; it marks the agents that finished since the person last looked at
them (**Done**), counts those on a sidebar button, and takes the person to any agent's block in one press.

## Goals

1. One place for all agents: a sidebar button above Requests, and a shortcut, open a popup listing every agent of
   every running workspace, grouped by workspace.
2. A row tells at a glance what the agent's block dot tells, in words: needs you, working (and with how many
   subagents), waiting for its subagents, done, idle, starting; and how long it has been so.
3. "This agent finished": a turn that ended while the person wasn't looking marks its agent **Done** until they look
   at its block; the sidebar button counts them.
4. What the agent works on and what it costs, without opening its block: its task, its card, its context and usage.
   Not its screen: Go to terminal shows it (card 224).
5. One press to the agent ("Go to terminal"), or to its request in the inbox ("Open request").
6. Hundreds of agents cost only the rows on screen.

## Non-goals

*The orchestrator's call* for each:

- Controlling agents from the list: no stop, interrupt, close or send.
- History or charts: the view shows now, not a timeline.
- **Done** kept across restarts: it starts empty each time Templum starts.
- Rows for Claude Code subagents: their count shows on their agent's row, the details list them.
- A new MCP tool: `list_blocks` already tells agents about each other.
- Per-agent notification settings.
- The Templum Agent panel: it is no terminal and isn't listed.

## Anti-goals (a failure even if it works)

- A row whose state contradicts its block's dot: the view never infers a state of its own.
- The popup opening by itself, or taking the keyboard from what the person types.
- A row jumping from under the pointer while the person aims at it.
- An agent marked **Done** while the person is looking at its block, or left **Done** after they looked.
- A closed popup slowing the canvas: closed, it costs the watch of each agent's state and the badge's count, nothing
  more.

## What a person sees

### The sidebar button

At the sidebar's foot, a new **Agents** button above Requests (*the orchestrator's call*): the same 40 px tile as
`inbox-button` and `git-button`, gpui-kit's `IconName::Bot` (the toolbar's "AI agent" icon; *the designer's call*), a
tooltip "Agents". A click toggles the popup; it shows the pressed fill while the popup is open.

**The badge** counts the **Done** agents of every running workspace, at the button's top right, drawn like the
Requests badge but in the accent color (Requests' is yellow), "99+" past 99; none when nothing is Done. It never
pulses: an animation in the sidebar redraws the whole window (*the orchestrator's call*).

**With the sidebar hidden** the shortcut still opens the popup, where the Git popup sits with no sidebar; no badge
shows anywhere else (*the orchestrator's call*).

### The shortcut

A new configurable command, `all_agents`, "Show all agents" in Settings ▸ Shortcuts: **Ctrl+Shift+L** (**Cmd+Shift+L**
on macOS) toggles the popup (*the designer's call*: free in Templum's defaults and in gpui-kit's editor and fields; L
for list). It works with the sidebar shown or hidden, and closes the popup when it has the keyboard.

### The popup

Drawn as every popup is ([design-system.md](design-system.md#popups), card 217): the same place (the canvas's bottom
left, beside the sidebar), size (`git_popup_size`, resized by its right and top edges like Git's), frame, header
(the Bot tile, "Agents" over "8 running · 1 needs you"), full screen and closing; window chrome, so the canvas's zoom
never scales it. A press outside it closes it, and what is under the press doesn't get it (card 217: one rule for
every popup). Only one of Git, Requests, Agents and Automations is open at a time: opening one closes the others.

Two columns: **the list** at the left (340 px wide; *the designer's call*: a row says more than an inbox row), **the
details** of the selected agent at the right.

### Who is listed

One row per terminal running an agent (`Pane::agent()`), in every workspace that isn't stopped, the hidden ones
included (*the orchestrator's call*). Not listed: stopped workspaces, the Templum Agent panel, Claude Code subagents'
blocks (`Pane::follows`), terminals with no agent. An agent leaves the list when its block closes, when its agent
quits, or when its workspace stops; it joins when one starts.

### An agent's state

A row's state is what its block's status dot says (`block::status_dot`), in words, with nothing inferred anew (*the
orchestrator's call*); agents without hooks show whatever their dot shows (activity.rs reads their screen). The
precedence is the dot's, the first that holds:

1. **Needs you** ("Needs you"; the details add what it asks): a request of its waits in the inbox
   (`Workspace::asking`); else its block has an attention alert (a bell, a notification); else its agent asks its
   person (`AgentState::NeedsYou`: its own dialog or a subagent's).
2. **Starting** ("Starting"): Templum's "Starting…" cover is on its block (`Pane::starting`).
3. **Waiting for n subagents** ("Waiting for 2 subagents", "Waiting for 1 subagent"): its agent counts as at work
   only because its turn ended while its subagents work (`Pane::awaits_subagents`).
4. **Working** ("Working", or "Working · 2 subagents" while some of its subagents work): `AgentState::Working`.
5. **Done** ("Done"): its agent is idle, and its last turn ended while the person wasn't looking at its block
   (below).
6. **Idle** ("Idle"): its agent is idle (`AgentState::Idle`), or no hook or look has said anything yet.

- *The designer's call:* **Waiting for n subagents** counts every subagent it waits for: those at work and those
  whose report is still to come (`Pane::subagents`).
- *The designer's call:* **Starting** is exactly the block's cover. An agent no hook or look has spoken of yet (the
  first seconds of one the person typed themselves) is **Idle**: an agent whose session just ended would otherwise
  read "Starting" until the next snapshot misses it.
- *The designer's call:* a command's news on an agent's block (a long command ended, red or green) changes the row's
  dot, which is the block's, but not its state's words: the news isn't the agent's.
- An agent whose snapshot no longer sees it says nothing of its turn, as on its block: **Idle**.

**The age** of each state ("now", "2m", "1h", "3d", inbox.rs's `age`) counts from when its state last changed kind
(*the orchestrator's call*). *The designer's call:* a change inside a kind (from 1 subagent to 2, another question)
keeps the age; **Done** and **Idle** count from the turn's end, so looking at a Done agent leaves its age as it was. The
change is noted when Templum hears it (an agent's event, a look at its screen) as well as at each frame, so an agent
that finished while the window was hidden or minimized is aged from its turn's end, not from the next frame.

### Done: this agent finished

New per-terminal tracking (*the orchestrator's call*), kept in memory only, never saved:

- **Becomes Done:** its agent goes from working (Working, or Waiting for subagents) to idle.
- **Stops being Done:**
  - when the person looks at its block: the same rule as a terminal's news (alert.rs): its block is the focused block
    of the workspace shown while the window is active. Focusing it, revealing it (a notification's click, "Go to
    terminal", Ctrl+Shift+A) and typing into it all do that. Selecting its row in the list does not;
  - when its agent works again;
  - with **Mark all seen** (below);
  - when its agent quits or its block closes.
- *The designer's call:* a turn that ends while its block is already the one looked at is never Done.
- *The designer's call:* when the agent asks the person after its turn ended (a bell), the row reads **Needs you**,
  which comes first; the Done mark stays under it until the person looks.
- A block moved to another workspace keeps its Done mark; its row moves to that workspace's group.

### The list

- **The top:** under the popup's header (card 217: it runs across the popup, with Full screen and Close "Close
  (Esc)"), a search field ("Find an agent"); the filter chips; with the **Done** chip chosen and something Done, a
  **Mark all seen** button.
- **Filter chips** with counts (*the orchestrator's call*): **All**, **Needs you**, **Working** (Waiting for
  subagents included), **Done**, **Idle**. *The designer's call:* **Idle** includes **Starting**; the counts are of the
  agents the search matches, so each says what its chip would list; the chips wrap when the list is narrow; **All** is
  chosen when the popup first opens, and the choice stays while Templum runs.
- **The search** matches, case-insensitive, anywhere in (*the orchestrator's call*): the block title, the agent's or
  account's name, the folder, the workspace's name, the kanban card ("#152 Attachments": its number and title) and
  the handed task's whole text.
- **Groups** by workspace: the workspace shown first, then the sidebar's order. A group's header names the workspace
  and how many of its agents need you ("Templum · 2 need you"; the name alone when none do). A group with no row
  that passes the filter and the search is hidden. Groups don't collapse (*the orchestrator's call*).
- **The order** within a group (*the orchestrator's call*): Needs you, longest waiting first; Done, newest first;
  Waiting for subagents; Working; Starting and Idle; ties by block title (*the designer's call:* case-insensitive,
  then by the terminal's id, so two blocks titled alike never swap).
- **Holding still** (*the orchestrator's call*): while the pointer is over the list, the rows keep their order: a row
  whose state changes stays where it is, a row that goes away leaves its place, a new one joins the end of its group.
  The order catches up when the pointer leaves.
- **The selection** follows the agent, not the place (*the orchestrator's call*): the selected agent stays selected
  wherever it moves; when it leaves the list (filtered out, quit, closed), the first row is selected. The selected row
  is lit and kept in view when it moves by the keys.
- **Empty:** "No agent is running" when no workspace has one; "No agent matches" when the filter or search hides
  them all (*the designer's call*).

### A row

Two lines (*the orchestrator's call*):

1. The status dot, in its block's own colors (`block::status_dot`; *the designer's call:* a hollow ring in the dim
   title color where the block shows no dot, as when Idle or Done); the block's title; the agent's or account's name,
   dim; at the right, the context used ("42%", or "84k ctx" when the window's size is unknown, as the usage chip says
   it; nothing when unknown).
2. Dim: the task's first line (`Handed.text`), else its card ("#152 Attachments", by the inbox's card rule), else its
   folder ("~/api"); at the right, the state and its age ("Done · 3m", "Needs you · 12m").

*The designer's call:* the state's words take the color of their dot (Done the accent, as the badge).

*(Card 222.)* A row lays its parts out side by side, as Automations' rows do (design-system.md, Popups): on the first
line the block's title gives way first, cut with an ellipsis, then the agent's name (at most 110 px, cut too), the
context at the right; on the second what it works on is cut before the state and its age. Nothing is drawn over a
row: its actions are the details' foot.

### The details

*(Card 222: drawn as the Requests detail and the Automations editor are, design-system.md, Popups.)* The selected
agent, top to bottom, each part only when it has something to say (*the orchestrator's call* for what; *the
designer's call* for the words). Every line is a label beside its words; the content grows to `popup::CONTENT_MAX`
(680 px) and stays centered past it, so full screen never stretches it; it scrolls over the foot.

- **The summary card** (`agents-summary`, `ui.card`): **Agent**, the agent's or account's name, then dim its model
  and effort ("Opus · xhigh": `spawn_agent`'s, else the session's model); **Role**, its role; **State**, its state as
  the row says it, in its color, then dim since when ("for 12m", "just now"), and under them what it asks when it
  needs you; **Terminal**, the block's title; **Where**, dim, its folder and its workspace ("~/Templum · Main"), cut
  to one line.
- **Work:** **Task**, its first 3 lines, under them dim its id, who handed it over (the handing block's agent and
  title, as the inbox's "From" names a block; "the API" for a task handed through it) and when: "t7, from Codex ·
  Main, 2m ago"; **Card**, the card it works on, by the inbox's rule (a card of any board assigned to this block, not
  in its board's last column, the newest if several): "#152 Attachments"; **Subagents**, "2 working · 1 reported",
  then one line per subagent: its block's title when it has one, else its id, and "working" or "reported".
- **Cost:** the usage chip's tooltip lines, each beside its label (*the orchestrator's call*: the same data and words
  as the chip): **Context** with its bar ("42% used"), **Tokens** ("1.2M"), **Cost** ("$3.10 at API prices"),
  **Effort**. The model is the card's.
- **The foot**, which stays while the details scroll, its buttons at the right of the content's width:
  - **Open request** (ghost): only while a request of its waits in the inbox (*the designer's call:* an agent's own
    dialog has no request, and is answered in its terminal: Go to terminal). It opens the Requests inbox at its oldest
    request, switching to its workspace (`Workspace::show_request`), which closes the Agents popup.
  - **Go to terminal** (solid, what Enter does): reveals its block, switching workspace when it is in another, focuses
    it, and closes the popup: the card's "automatic focus" (*the orchestrator's call*). That is looking at it: it is
    no longer Done.

With no row selected (nothing listed), the right column says nothing.

### Mark all seen

With the **Done** chip chosen and at least one Done agent, a small button **Mark all seen** clears every Done mark in
every workspace (*the orchestrator's call*); the badge goes, the rows read Idle.

### Keys

Opening the popup (its button, its shortcut: the person's own acts) gives it the keyboard (*the orchestrator's
call*); the keyboard is the search field's:

- typing goes to the search field;
- ↑ and ↓ move the selection to the previous and the next row, skipping headers, none past the ends;
- Enter is **Go to terminal** on the selected row;
- Esc clears the search first; with the field empty, it closes the popup;
- a click on a row selects it and gives the popup the keyboard; a press outside the popup closes it, and what is
  under the press doesn't get it (card 217, as every popup);
- *the designer's call*, as the inbox: the workspace's shortcuts still work while the popup has the keyboard, but
  those that act on the terminal in front (copy, paste, close, find, previous and next prompt); the Agents shortcut
  closes the popup.

Closing the popup gives the keyboard back to the canvas only if the popup had it.

## Agents of another computer

*(Templum to Templum, part 2h.)* A mirrored workspace's agents are listed too, in a group of their own headed "API ·
winx-desktop" (with " · 1 needs you"), dim while the remote isn't live. A row says home's title for the block, its
agent as home's Settings names it, the task it works on (a task handed through the API says "from the API"), its card
and its context. Its state is home's, except that home's own Done reads as idle: Done on a mirror is this computer's
watch, an agent whose turn ended while nobody here looked at its block, and looking at it here (the workspace shown, its
viewer focused) clears it, as for a local one. The details give home's model and effort, role, task (its number, its
first three lines, who handed it and how long ago), the count of its subagents (no lines), its usage with an "Effort:
high" line; never its screen, so the open popup asks home for nothing. Go to terminal shows the mirrored workspace
with the viewer focused. A remote agent is never heard by this computer's event automations. The usage chip's tooltip
of a remote terminal ends with the same "Effort: high" line.

## Constraints

- CLAUDE.md: only crates/templum depends on the UI framework; every logic change ships a headless test (GPUI
  test-support tests in crates/templum/src/workspace/tests/agents_view.rs); minors go to docs/minors.md; no
  speculative structure.
- **Performance** (*the orchestrator's call*): the list is a virtual list (`uniform_list`, as gallery.rs and
  git_block/history.rs use), so hundreds of rows cost only those on screen. Closed, the popup costs the watch of each
  agent's state (a few comparisons per terminal per frame, the loop `hear` already runs) and the badge's count. The
  pull request runs `cargo run --release -p templum -- --bench` and `--bench-mixed` (CLAUDE.md, Performance) and
  pastes their JSON lines; the bar is `p95_ms` ≤ 16.7.
- Theme tokens only for colors; nothing animates.
- Specs to update with the change: workspaces.md (The sidebar's foot), config.md (`all_agents` in `[keys]`),
  ask-inbox.md (its Non-goal on card 153 links here), docs/guide.md (the Agents button), ARCHITECTURE.md (the new
  modules), this file's Status.

## Left to the builder

Sizes beyond the list's width, paddings, the rows' exact layout and truncation, the hollow ring's look, the chips'
look, the empty states' look, the details' section spacing, which key handler of the popup takes ↑ ↓ Enter Esc before
the field does, and where exactly Templum notes a state's change when it hears it (the plan names the places).

## As built

What the builder chose where the spec left it open:

- The model is `workspace/agents_view.rs` (`Doing`, `Show`, `Watch`, `AgentRow`, the keys); the view is
  `agents_view.rs`. The watch runs in a frame, after a bus message (`Workspace::serve`) and in the 1 s look loop.
- `Doing::AwaitsSubagents` needs a subagent: a stale `awaits_subagents` flag with none left reads Working.
- "Working · 1 subagent" is singular for one, as "Waiting for 1 subagent".
- The group header's count of agents that need the person ignores the filter and the search; it reads "1 needs
  you" for one and "2 need you" for more.
- A request waiting in the inbox makes the row Needs you whatever news the block has (the dot may show the news).
- The selection the list falls back on (the first row, when the chosen agent is hidden or left, or none was chosen)
  is pinned each frame: it stays selected when the order changes. The person's own choice is kept apart: it is
  selected again as soon as the filter or the search lists it, and goes only when its agent is gone.
- A folder, in the rows and in the details (and the inbox's), is the last one read after a snapshot
  (`Pane::seen.folder`): asking the process (`term.cwd()`) every frame is too dear on Windows.
- The empty list says "No agent is running" whenever no agent runs, whatever is typed in the search.
- ↑ and ↓, Enter and Esc are taken in the capture phase of the popup, so they reach the search field only as
  typing; the picker's keys win while the theme picker is open, as in the inbox.
- The empty words sit in the list column; the details column is blank when no row is selected.
- The ages are drawn when the window is drawn: an idle window shows "2m" until something redraws it.
- The details scroll with the wheel and show no scrollbar.
