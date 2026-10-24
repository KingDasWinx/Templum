# The requests inbox: what agents ask, in one place

- **Date:** 2026-10-23
- **Status:** Implemented (kanban card 144; commits 0089726, 287c53d and e5f22cd); plan:
  [2026-10-23-ask-inbox.md](../plans/2026-10-23-ask-inbox.md)
- **Builds on:** [ask-user.md](ask-user.md) (the questions' dialog, its keys, its answers),
  [connections.md](connections.md#requests-from-agents) (the former balloon, `connect`),
  [agent-tools.md](agent-tools.md) (the worktree requests), [git.md](git.md) (the popup this one is drawn like),
  [workspaces.md](workspaces.md) (the sidebar)

## Intent

A person runs many agents at once, and several of them ask product decisions at the same time. Today each request
waits in a balloon on its own terminal: the person hunts terminal by terminal to find them, and a question often
doesn't say enough, so they go read the agent's terminal to learn which kanban card it is on and what it is doing
before they can answer. With tens or hundreds of agents this doesn't scale.

The inbox gathers every request of the workspace in one popup, opened from the sidebar, and puts next to each one
**why it is asked**: the card the agent works on, the task it was handed, and a line the agent writes itself. The
person answers there, one after the other, without going to any terminal.

## Goals

1. One place, per workspace: a sidebar button above Git opens the inbox; a badge counts what waits in the current
   workspace, and the button pulses for a few seconds when a request arrives.
2. Every request says where it comes from (the agent, its block, its folder) and why it is asked.
3. The person answers without leaving the inbox, one request after another.
4. A long preview reads well: it wraps, shows a part, and "Show more" shows the rest.

## Non-goals

- Requests of other workspaces in this inbox: each workspace has its own; the others keep telling as today (the
  workspace tile's yellow dot, the notification, the sound, Ctrl+Shift+A).
- The Templum Agent's own approvals (crates/templum/src/chat/approval.rs): they stay in its chat.
- The global view of every agent (kanban card 153): later; it may link to this inbox.
- New ways to ask: the tools and their answers stay as they are, but for `why` (below).

## Anti-goals (a failure even if it works)

- The inbox opening by itself, or taking the keyboard from what the person types: it never does.
- A request whose origin is unclear, or two requests that read the same without saying which agent asks.
- A request lost: whatever waits is counted on the button until it is answered, dismissed, withdrawn or timed out.
- Templum answering anything for the person.

## What moves where (decided with the person)

### The terminal no longer shows a balloon

Every request that waits for the person — `ask_user` (questions and `propose_plan`'s plan) and the yes/no requests
(`connect`'s Allow/Deny, `remove_worktree`'s Delete/Keep, `merge_worktree`'s Merge/Cancel: every `Waiting::Question`
and `Waiting::Allow` of crates/templum/src/workspace/waits.rs) — goes to the inbox **only**. The asking terminal shows
no balloon any more; instead a small notice on it, "Waiting for your answer" (the waiting yellow, with the number when
several of its requests wait), whose click opens the inbox at that terminal's oldest request. The block's waiting dot
stays as today.

### The sidebar button

Above Git, in the sidebar's corner (crates/templum/src/sidebar.rs, the same tile as `git-button`), an inbox button
with a tooltip "Requests". A badge at its top right counts the current workspace's waiting requests (none: no badge).
**When a request arrives** in the current workspace the button pulses for a few seconds, then stops and the badge
stays; each new request pulses it again (decided with the person: an animation in the sidebar redraws the whole window,
canvas included, so a pulse that never stops would keep the app redrawing while anything waits). With reduced motion:
no pulse, the badge only. A click toggles the inbox.

**With the sidebar hidden** (`toggle_sidebar`; *the builder's call, made by the designer*): the count shows as the same
badge on the title bar's sidebar toggle (no pulse there), and the inbox still opens from the asking terminal's notice
and Ctrl+Shift+A. It sits where the Git popup sits with no sidebar.

### The inbox popup

Drawn like the Git popup: same size rule (`git_popup_size`), place, layering, hairlines and colors; Esc closes it, as
does the button. Only one of the Git popup and the inbox is open at a time. Two columns:

- **The list** (left): one row per waiting request of the current workspace, **oldest first**, each showing the
  asking agent and its block's title, the card it works on ("#152 Attachments", when there is one), what it is (a
  question, "2 questions", a plan, a request to connect, to delete or merge a worktree), and how long it has waited
  ("2m"). The selected row is lit. New requests join at the end, never moving the selection.
- **The request** (right), from top to bottom:
  - **From:** the agent and block title, its folder, its workspace; a **Go to terminal** button that reveals the block
    (as a notification's click does) and closes the inbox.
  - **Why:** what Templum knows and what the agent said, each line only when known:
    - the kanban card the agent works on: a card of any board whose assignee is this block (`Assignee::Block(uid)`),
      not in the board's last column, the newest (highest id) if several (*the designer's call*: Templum records no
      move time, and one card per agent is the usual case); its number and title;
    - the task it was handed (`Handed`, crates/templum/src/workspace/mod.rs): its first lines; this needs `Handed` to
      keep the task's text, which it doesn't today;
    - the agent's own `why` (below), as the agent wrote it.
  - **The request itself:** today's balloon content, unchanged in behavior — the tabs of several questions, options,
    previews, Other fields, the note, Send and Dismiss (ask-user.md, The dialog and Keyboard); the plan with its
    buttons; the yes/no requests with their two buttons and what they act on. A cancelled question keeps its banner
    ("The agent stopped waiting: your answer will be typed into its terminal.").
- **After an answer** (Send, Dismiss, a button), the next request in the list is selected; with none left, the right
  column says nothing waits ("Nothing waits for you") and the inbox stays open.
- **Keys** (*the designer's call*; unlike the Git popup, whose backdrop closes it on a press outside, the inbox has no
  backdrop): opening the inbox (its button, the notice, Ctrl+Shift+A — all the person's own acts) gives it the
  keyboard, and so does a click in it; a click on the canvas or a block reaches what was clicked, gives it the
  keyboard, and the inbox **stays open** beside it, so the person
  can read or type into an agent's terminal and come back. While it has the keyboard, ask-user.md's keys work on the
  selected request, and ↑/↓ with Alt (or the builder's choice that doesn't clash with the dialog's own arrows) move in
  the list; Esc closes it. This replaces the balloon's `balloon_keys`/`away` routing (workspace/input.rs).
- **TOO_SOON** (waits.rs) still guards a request that just appeared from a press meant for something else.

### A long preview

An option's preview (and a plan's Markdown, if it overflows the same way) wraps its lines to the preview's width,
shows its first lines only (about 12), and a **Show more** link under it shows the whole text (then **Show less**).
This is the preview's own scroll problem of card 144's second paragraph; it applies in the inbox.

### `why`: the agent says why it asks

`ask_user` takes a new optional argument, `why`: one or two sentences, "what you are doing and what the answer
decides". Not blank when given; long text is shown whole (it wraps). The tool's description asks for it: "Give why:
what you are doing and what the answer decides, so the person can answer without reading your terminal." `templum
ask` gets no new flag (ask-user.md keeps it as is). The answer doesn't change. A call without `why` shows the rest of
**Why** only.

### Telling the person

As today (ask-user.md, Telling the person), with these changes:
- **Sound:** unchanged.
- **Notification and alert:** when the request's workspace isn't the one shown, or the window is in the background,
  as today; a click on the notification shows that workspace and opens its inbox at that request (instead of showing
  the block). The workspace tile's yellow dot stays.
- **Ctrl+Shift+A** (`go_to_waiting`): opens the inbox when the current workspace has a request waiting; otherwise
  does what it does today (the terminal that asked for attention).

### A request's block

A request belongs to its terminal (`Wait.from`): when its block moves to another workspace, the request goes with it
to that workspace's inbox; when its block closes, the request ends as today.

## Constraints

- CLAUDE.md: only crates/templum depends on the UI framework; tests for every logic change (GPUI test-support tests in
  crates/templum/src/workspace/tests/, the MCP argument's in templum-mcp); minors to docs/minors.md; no speculative
  structure.
- The inbox is window chrome (the sidebar's and the Git popup's layer): the canvas's zoom never scales it. The
  balloon's zoom-related parts (scaling like a photo, Level of detail) no longer apply to requests.
- The pulse runs a few seconds per new request only; nothing animates while requests merely wait. A change to painting
  runs `--bench` (CLAUDE.md, Performance).
- Specs to update with the change: ask-user.md (The dialog → the inbox; Telling the person), connections.md (Requests
  from agents), agent-tools.md (the worktree requests' place), shell-integration.md (`go_to_waiting`), this file's
  Status.

## Left to the builder

The icons, sizes, colors (theme tokens only), the list rows' layout and truncation, the empty states' words, the
pulse's timing, the keys that move in the list, how much of a handed task to keep and show, the preview's line limit
(about 12), and how the balloon's code is reused or moved (its question/plan/allow parts drawn at a fixed scale in the
inbox instead of at the canvas's zoom).

## As built

What the builder chose where this design left it open, and what changed on the way:

- **Keys:** Alt+↑ and Alt+↓ select the previous and the next request; Esc closes the inbox, also from a field; the
  theme picker's keys stay the picker's, and a terminal's copy, paste, close, find and prompt shortcuts are nothing
  while the inbox has the keyboard. A request that ends under a field the person types in gives the keyboard back to
  the inbox; one that ends elsewhere leaves it where it is.
- **The notice:** a chip with the waiting yellow border at the bottom center of the terminal, at the Full level; it
  takes its own presses, so a group move over the rest of the body works as for any block.
- **The pulse:** the button's background swells and fades once a second for 3 seconds from the arrival of a request
  in the workspace shown (`Workspace::inbox_pulse`); frames are asked for only while it lasts.
- **The list** scrolls to keep the selected row in view (Alt+↑/↓, a notification's click, the next request after an
  answer); a row's age is told as it was drawn.
- **A long preview** is cut after 12 lines of the wrapped text; Show more / Show less applies to all the previews of
  the request. The preview has no scroll of its own.
- **`go_to_waiting`** opens the inbox, selecting no request in particular, when the workspace shown has one waiting.
- **Notifications:** a request in the workspace shown with the window in front makes none, in view or not (the badge
  tells), but its block still gets the alert (answering or seeing the block clears it), so the workspace's tile has
  its dot and Ctrl+Shift+A finds the request from another workspace. A notification's click shows the workspace the
  asking block is in when it is clicked.
