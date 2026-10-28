# Connections: blocks that talk to each other

- **Date:** 2026-10-15
- **Status:** Batches 1 to 8, [agents typed in a terminal](#agents-typed-in-a-terminal),
  [every agent that can take it](#every-agent-that-can-take-it) (kanban card 44) and
  [cards for an agent](#cards-for-an-agent) (kanban card 40) implemented; [agent tools](#agent-tools-card-35) (kanban
  card 35) planned
- **Builds on:** [agents.md](agents.md) (opening agents), [usage.md](usage.md) (the `--settings` Templum types for
  Claude Code), [shell-integration.md](shell-integration.md) (command marks, notifications),
  [persistence.md](persistence.md), [kanban.md](kanban.md) and [cockpit.md](cockpit.md) (notes, the editor, drawing)
- **A second server:** the Templum Agent's own, `templum control`, which no agent block gets
  ([templum-agent.md](templum-agent.md#the-control-server), kanban card 43)

## Goal

Templum is a cockpit for CLI agents, but each block is an island: an agent doesn't know it is on a canvas, can't
read the dev server's log in the block beside it, can't move its card, leaves nothing behind for the next agent,
and handing work to another agent means pasting a transcript. With connections, a person **wires** blocks together
on the canvas, and an agent can use every block wired to it: read a terminal, type into it, hand a task to another
agent and get its answer back, start a new agent beside itself, move kanban cards, write to a shared note, open a
file in the editor, and ask the person a question in the requests inbox.

Templum serves this as an **MCP server** the agents it opens are given, and as a **`templum` command line** for
agents without MCP, scripts and hooks.

**Success criteria**

1. The Claude Code and Codex Templum opens list Templum's MCP server as connected, without any setup and without
   Templum changing their settings files.
2. An agent knows which block it is, which blocks are wired to it, and the others it could ask to be wired to.
3. A person wires two blocks with the Wire tool or a block's menu; wires follow their blocks, can be selected,
   deleted and taken back, and come back after a restart.
4. An agent reads and writes only the blocks wired to it; asking for another waits in the inbox for the person.
5. Claude Code wired to Codex hands it a task and gets Codex's final answer back as the tool's result, and the task
   is visible in Codex's terminal.
6. A block's status dot shows whether its agent is working or waiting for the person, within a second.
7. `templum blocks`, `templum read <block>` and the other verbs work in any terminal Templum opened.
8. All tests in [Testing](#testing) pass in CI on the 3 OS; `--bench` and `--bench-mixed` keep **p95 ≤ 16.7 ms**.
9. Claude Code runs another agent as its own subagents run (batches 6 to 8): it picks the agent's model and effort,
   hands it a task and keeps working, hears its end typed back with its time and tokens, changes the task while it
   runs, answers its dialogs with real keys, stops it, and closes the blocks it opened.
10. A `claude` or `codex` a person types in a Templum terminal (bash, zsh, fish, PowerShell) gets what the ones
    Templum opens get, without Templum writing anything outside its own folder
    ([Agents typed in a terminal](#agents-typed-in-a-terminal)).
11. GitHub Copilot CLI, OpenCode, Kilo Code, Qwen Code and Pi, opened by Templum or typed in its terminals, get its
    server (Pi where it has MCP) and its hooks for that session, and Gemini CLI is asked for its extension once
    ([Every agent that can take it](#every-agent-that-can-take-it), card 44).

## Out of scope

- Editing an agent's own settings files (`~/.claude/settings.json`, `~/.codex/config.toml`, Gemini's, and from
  card 44 `~/.copilot`, OpenCode's, Kilo Code's, Qwen Code's and Pi's): Templum
  passes everything on the command line it types, for that session only. Gemini CLI gets an extension instead, below,
  only after the person agrees, and never through its settings file.
- Wires between workspaces, and wires to drawings.
- A general message bus between programs: the bus serves Templum's own clients (the MCP bridge, hooks, the command
  line).
- Agents talking to blocks they aren't wired to, other than asking to be wired.
- Remote agents, other machines, other users.

## Words

- **Block id:** a short code that names a block for its whole life, through restarts, undo and moves between
  workspaces: five lowercase letters and digits (`b7k2m`). Agents and the command line name blocks by it.
- **Wire:** a connection between two blocks of one workspace. It has no direction: both ends may use each other.
- **The bus:** the local socket Templum listens on while it runs; every MCP server, hook and `templum` command
  started in one of its terminals talks to it.
- **Caller:** the block the MCP server or command runs in, found from its terminal.

## What a person sees

### Wires

- **The Wire tool** (a cable icon, in the toolbar's Pointer group, after Hand): press on a block, drag, and a curve
  follows the pointer; release on another block of the workspace to wire them. Released anywhere else, nothing
  happens. The tool stays on until another tool is picked. A press on the empty canvas does nothing. While the pointer
  is over another block the curve already runs to its edge, in the accent color. Wiring leaves no block with the
  keyboard, so Ctrl+Z takes the wire back.
- **A block's menu:** **Connect to ▸** lists the workspace's blocks not wired to it, by title; **Disconnect from ▸**
  the ones that are. They follow Fullscreen. A tile's menu has neither: grid mode hides wires.
- **A wire** is a curve from the edge of one block that faces the other to the edge of the other, drawn on the
  canvas below the blocks in the title's dim color, 2 units wide and never under a pixel. It follows its blocks as
  they move or are resized. When another block is in its way, it takes another path: between the other pair of
  edges (down the gap beside a column of blocks), else an elbow from a side of one to the top or bottom of the
  other; only the blocks between its two are looked at, so a wire with nothing in its way costs one look.
- **Selecting:** a click on a wire where no block or drawing covers it selects it (accent color); a drag from it
  moves the canvas as from anywhere, leaving the keyboard where it was and selecting nothing. Delete or Backspace
  removes it while no block has the keyboard (focusing, adding or selecting a block unselects it); Esc unselects;
  right-click on it offers **Disconnect**.
- **Undo:** connecting and disconnecting are steps of Ctrl+Z and Ctrl+Shift+Z. Closing a block removes its wires,
  and undoing the close brings them back.
- **A wire named through `connect`:** the control server's and the API's `connect` take a `label` (one line, at most
  60 characters; blank takes it off). On two blocks already wired it renames their wire, one step of Ctrl+Z; else
  it wires them, one step, and the name is kept by the wire's key, as every label is.
- **Hand a task… on a mirror:** a remote terminal whose state shows an agent has **Hand a task…** in its menu: a field
  of several lines (Ctrl+Enter, or the **Hand over** button) and home's `delegate` types it into the agent as the
  API's tasks are (`[from API · <share's name>, task t7]`). The mirror says nothing on success; the task's end comes
  as a notice, and a refusal is said. A task is at most 256 KiB.
- **A block wired twice** to the same block, or to itself, is refused silently.
- **While a task is handed over** along a wire ([Delegating](#delegating)), a dot travels along it from the agent
  that handed it to the one working, until the answer comes back. It takes 1.2 s from one end to the other, then starts
  again; it stays at the wire's middle when the system asks for less motion. With no wire between the two, there is
  no dot (the answer still comes back).
- **Far away** a wire is 1 pixel wide: from 50% down, where the Card level of
  [level-of-detail.md](level-of-detail.md) comes. **Grid mode** hides wires, as it hides drawings.
- **Moving a block to another workspace** drops its wires, those between blocks moved together too.
- **Labels:** **Rename wire…** in a wire's menu names it (blank removes the name; it is put on one line and cut at
  60 characters), a step of Ctrl+Z. The label sits in a pill at the curve's middle, in the wire's color, scaled with
  the canvas at the Full level and at its own size at the Silhouette level, as a block's title is; grid mode hides it.
  It comes back with its wire: Ctrl+Z, a closed block brought back, a restart.

### An agent's state

A terminal running an agent Templum hears from ([Hooks](#hooks-and-agent-state)) shows it in its status dot:

| State | Dot | Tooltip |
|---|---|---|
| Working on a prompt | accent | "Working" |
| Waiting for the person (a permission dialog, a question) | yellow | "Waiting for you: *the agent's message*" ("Allow Bash?" as a permission dialog opens) |
| Done with its turn | none (its running command would otherwise read "Running") | — |

**From its screen.** Where its hooks say nothing, its screen tells: once a second Templum looks at the last 100 lines of
every agent's terminal, and each event of its hooks starts the looks over (the spinner that ran until its turn ended or
a dialog opened says nothing after). One that changed at 3 of the last 4 looks (a spinner, Codex's "Working (5s)") is an
agent at work. An agent whose hooks spoke and whose screen stayed still for 30 looks while it worked **seems stuck**
(batch 7): yellow, "Waiting for you: it seems stuck, its screen still for 30 seconds" (a dialog no hook tells of, as a
trust question or Codex's choice of model, looks like this; Claude Code's spinner and Codex's "Working (5s)" move every
second while they work); its task stays handed over, `wait_for(needs_you)` answers, the agent that handed it hears so
([Delegating](#delegating) 10), and its screen moving again turns it back to Working. An agent without hooks, or one the
person typed into since its last hook (an Esc while Claude Code writes, which no hook tells of), whose screen stays
still for 5 looks while it works is at rest instead, and the task it was on ends with its last 40 screen lines (from
card 69 not an agent with Templum's tools and no hooks, which can answer with `reply`: its task stays open and its
master is told once, [Delegating](#delegating) 6). A wait
for the person ends from the screen only once the person typed into the terminal since it began (they answered the
dialog): a subagent drawing under an open dialog doesn't end it, nor does a still screen. A look right after the person
typed, pasted, clicked or scrolled in it, or its block was resized (its agent redraws at the new size), tells nothing,
and the looks before it count no more. A spawned agent whose prompt still waits isn't looked at: its screen is its start
and its first questions, which [Spawning](#spawning) watches, and the minute there ends its task.

An alert the terminal already shows (attention, a finished command) comes first. An agent the last snapshot doesn't
see (it crashed: no `SessionEnd`) shows no state, and its state is forgotten, so a `claude` typed there by hand
later starts with none.

### Requests from agents

- **Notify:** a notification titled with the block's title, the agent's text as its message; clicking it reveals the
  block. In the background it is a system notification, as a terminal's are.
- **A question** (`ask_user`) and **a request to connect** (`connect`) wait in the **requests inbox** of the
  workspace ([ask-inbox.md](ask-inbox.md)): a popup opened from the sidebar's button, with a row per request and,
  for the selected one, where it comes from, why it is asked and the request itself, its body scrolling over a foot
  of buttons that stays ([ask-user.md](ask-user.md#the-dialog)). The asking terminal shows no balloon, only a small
  notice, "Waiting for your answer" (with how many when several), at its bottom, whose click opens the inbox at its
  oldest request:
  - a question ([ask-user.md](ask-user.md#the-dialog)) shows its questions, each with its header, its options (a
    radio or a checkbox, the label, the description), an Other field, then a note, **Dismiss** and **Send**; today's
    shape (`question`, `options` as strings) shows its text whole and a button per option, or a text field and
    **Send** when it has none, and **Dismiss**;
  - a request to connect says "*Claude Code · api* wants to connect to *Dev server*" with **Allow** and **Deny**;
    Allow wires them;
  - a request for a block already wired is answered at once; Allow is a step of Ctrl+Z, as wiring by hand is;
  - several requests from one block wait in the list, oldest first, and its notice counts them;
  - the inbox has the keyboard when the person opens it or clicks in it, until Esc closes it
    ([ask-user.md](ask-user.md#keyboard)); a request that arrives never takes it, no key reaches a terminal while
    the inbox has it, and presses within 0.5 s of the inbox drawing a request are ignored;
  - when the person doesn't see the request (it is in another workspace than the one shown, or the window is in the
    background), a notification says what the agent asks ("Asks you: …", with "(and 2 more)" for a call's other
    questions), also the system's in the background; clicking it shows that workspace with its inbox open at the
    request. The block gets an alert too, as a bell gives one: its workspace's tile shows the yellow dot, and
    Ctrl+Shift+A opens the inbox of the workspace shown when a request waits there, else goes to the terminal that
    asked for attention; answering clears it. A request in the workspace shown, the window in front, is told by the
    badge on the sidebar's inbox button (on the sidebar's toggle when the sidebar is hidden) and a short pulse;
  - a question or a request to connect makes a short system sound, seen or not, at most one every 2 seconds, unless
    `[connections] sound = false` ([ask-user.md](ask-user.md#telling-the-person));
  - the block's dot shows it waiting for the person (yellow, "Waiting for you: *the question*"), at every level;
  - a request no one answers is withdrawn after its timeout, and the agent is told so: a request to connect's is 30
    minutes unless the agent asked for another; a question waits until it is answered unless the agent gave a
    `timeout` or `[connections] question_timeout` sets one, the shorter winning
    ([ask-user.md](ask-user.md#how-long-it-waits)); closing the block withdraws its requests, and so does the
    agent leaving (its connection to Templum ends);
  - a cancelled call (`notifications/cancelled`: the agent's client gave up, or the person pressed Esc in it) drops a
    request to connect; a question stays, saying "The agent stopped waiting: your answer will be typed into its
    terminal.", and its answer is typed into the agent's terminal as a notice,
    `[answer to your question "…"] The answer is in the pasted text.` and the answer on the next line
    ([ask-user.md](ask-user.md#when-the-client-stops-waiting)); Dismiss or its deadline drop it silently.

### Delegating

1. *Claude Code · api* is wired to *Codex · api*. It calls `delegate` with Codex's block id and a task.
2. Templum types into Codex's terminal a first line, `[from Claude Code · api (b7k2m)] Your task is in the pasted text:
   do it.` (from batch 8 it names the task: `[from Claude Code · api (b7k2m), task t7]`; from card 69 it asks for the
   answer: `… do it, then answer it with reply.`), as a paste of its own, then the task as a second paste, which ends
   with how to answer it ([agent-protocol.md](agent-protocol.md#1-every-task-says-how-to-answer): call Templum's `reply`
   tool once with the whole answer, or run `templum reply -` in its shell without Templum's tools) (batch 7; before, both as one: Claude Code takes a paste of 4 lines or more than 800
   characters for data it was shown, not for what it is asked, unless typed text comes with it), then Enter, a moment
   (150 ms) after the paste: an agent may take keys that come in one burst for a paste, where Enter is a new line. The
   Enter goes only once the agent's screen shows the paste (Templum looks every 150 ms, for about 2 seconds) and, from
   card 118, stood still a look: Codex and Copilot CLI on Windows, whose console passes no bracketed paste on, read a
   paste as keys and take an Enter that comes while they still arrive, or just after, for a new line in it (checked
   with Codex 0.154 and Copilot CLI 1.0.100 in a pseudo-terminal, the task typed as keys: an Enter right after it was a
   new line, the task left in their input; one 110 ms or more after it sent it; a bracketed paste took either). A
   screen that keeps moving (an agent at work) gets it after the 2 seconds. It goes only if Templum
   still may type into it (5): a dialog that ignores pastes, such as Codex's approval,
   which Templum hears nothing of, or a permission prompt that opened meanwhile, gets no Enter that would answer it. The
   task then ends at once: the delegating agent is told the agent didn't take it, and to look at its block. The person
   sees it arrive. A resize of the agent's block meanwhile redraws it at its new size, which shows nothing of the paste:
   its Enter waits until its screen stays the same for two looks. An agent whose hooks say when a turn starts (Claude
   Code, Gemini CLI, Codex once its hooks spoke) then gets it, as one whose dialog would be heard; for another that
   settled screen is the new start, and only a change after it is the paste.
   **Confirmed** (batch 7): an agent whose hooks say when it takes a prompt (Claude Code's `UserPromptSubmit`, Codex's,
   Gemini CLI's `BeforeAgent`) must take the task within 3 seconds of its Enter, or open a dialog; one at work when the
   task was typed queued it (Codex says it took it only once a turn takes it in, maybe minutes later). Nothing else
   confirms it: not a turn's end, a notification (Claude Code's `idle_prompt`, a minute after a turn), nor a moving
   screen. With no word, it gets its Enter once more, only where that Enter can't answer a dialog: the first changed
   nothing on its screen (the task still sits in its input), or the paste went as keys (Windows' console, a program
   that asked for no bracketed paste), which an agent may take an Enter among for a new line in it. And only while
   Templum still may type into it, its screen hasn't changed since 150 ms after the first and the person hasn't typed
   into it for 3 seconds; otherwise, or with still no word 3 seconds later, the task ends untaken, with the last 20
   lines of its screen. A turn that ends before the agent took the task (the turn it was on) doesn't answer it. The
   person typing into the agent after the paste and before its Enter gets no Enter (it would send their text too):
   the task ends untaken, "its person is typing in it". Once the task ended, or another came after it, its looks stop
   and press nothing more.
   **Confirmed by its screen** (card 118): the agents without hooks, and those whose hooks haven't spoken yet (Codex's
   `SessionStart`, Copilot CLI's, OpenCode's and Kilo Code's first event come with their first prompt), took it once
   their screen moves (two looks in a row, 150 ms apart, see it change: a spinner, a timer) within 3 seconds of the
   Enter. Before card 118 they got no confirmation, and a task whose Enter became a new line sat in their input until
   its timeout. A screen that didn't change since 150 ms after the Enter gets the Enter once more, as above: the Enter
   changed nothing, or the paste went as keys; with still nothing moving 3 seconds later, the task ends untaken if
   neither Enter changed the screen: "typed but not submitted", whose answer asks to clear the agent's input before
   trying again (the task may still sit there). One an Enter changed, then nothing moved, took it and works drawing
   nothing. A screen that changed and stopped without a second Enter (a dialog no hook tells of, drawn at once, as
   Aider's "Add file to the chat? [Yes]" for a task naming a file, or work that draws nothing) took it too: Templum
   can't tell, and presses nothing more into it.
3. The wire's dot travels from Claude Code to Codex while Codex works.
4. When Codex answers the task with `reply` (it is asked to: 2), that text, whole, ends the task and becomes the
   result of Claude Code's `delegate` (when it asked to wait), or of a later `wait_for_idle`. Without a `reply`, its
   turn's end ends it with its last answer, as before card 69, unless Codex waits for a task it handed out itself
   (a `delegate` or a spawn's prompt of its own): then the turn's end ends nothing, and its `reply` or the end of a
   turn with nothing handed out any more does ([agent-protocol.md](agent-protocol.md#4-a-turn-that-ends-while-the-subagent-waits-on-its-own-task)). Each task
   has an id (`t7`, batch 8), which `delegate` and `spawn_agent` say; `delegate` without `wait` answers once the
   agent took the task (2; card 118, before at once, which let a master take a task left in an input for one under
   way): "Handed to k9x1d as task t7.", or, when it didn't, why (`k9x1d didn't take the task: typed but not
   submitted: …` and the last 20 lines of its screen), the task then ended without a notice besides (10). One still
   starting (5) answers at once: "Handed to k9x1d as task t7. It is starting: the task is typed once it is ready.",
   and its end comes back as any end does. For an agent without Templum's tools (no `initialize` from its server) it adds
   why and what else ends the task (` k9x1d has no Templum tools (<why>): it can still answer with `templum reply -`
   in its shell; …`; `list_blocks` says `, no Templum tools` on its line:
   [agent-protocol.md](agent-protocol.md#8-templums-tools-checked)). Ids count from `t1` each time Templum starts: tasks
   aren't saved ([Files](#files)).
5. Templum refuses to type into an agent that is waiting for its person (a permission prompt would take the text
   as its answer; the refusal says what it asks and gives the last 20 lines of its screen, the dialog as drawn, so
   the caller can answer it with `send_keys`), into a terminal running no agent (`send_to_terminal` is for those),
   into an agent no snapshot has found yet (one Templum just typed may not have started at all; the caller is told
   to try again in a few seconds), and into an agent already on another agent's task (`wait_for_idle` waits for its
   end; the agent whose task it is changes it instead, 11). From card 118 a task handed to an agent Templum started
   that may not take typing yet, its screen hasn't shown it (its cover is still on) or no snapshot found it within a
   minute of its command (after that it never started, and the task is refused as for a terminal running no agent),
   isn't refused: it waits as a spawn's prompt does, typed once the agent is ready and not waiting for its person
   ([Spawning](#spawning)), its task ended with its screen ("never got its task: a minute after it was handed") when it
   isn't within a minute. A `delegate` right after `spawn_agent` was refused there (Codex on
   Windows, which a snapshot didn't find yet), or typed into an input not up yet and lost (OpenCode, Kilo Code; Copilot
   CLI showed it later). A reply is still refused until a snapshot found the agent.
6. An agent Templum gets no events from (see the table under [Hooks](#hooks-and-agent-state)) answers through
   `reply`. Without Templum's tools, or with neither tools nor hooks, its task ends once its screen has been still for 5
   seconds after working, with its last screen lines. An agent **with** Templum's tools and no hooks keeps its task
   open then: its master is told once (**quiet**) that it hasn't answered, with its screen: a call of its that waits
   for the task gets that as an error, otherwise a notice `[task t7 from Codex · api (k9x1d) hasn't answered: its
   screen is still] Its screen is in the pasted text.` and the last 20 lines is typed as a dialog's is (10); `reply`
   still ends the task, and a change from the master (11) lets it tell once more. Not while the agent waits for a task
   of its own. Else at its timeout.
7. A task stays handed over until its answer comes, the agent's session ends (or a snapshot no longer finds the agent
   it found before: Codex says nothing when it quits), its block closes, or its timeout passes (`delegate`'s, 600 s
   unless asked, also without `wait`). Past its timeout the agent that handed it over still waits for its answer
   with `wait_for_idle`, and its end still comes back (10); the wire's dot stops, and another agent may hand it a
   task.
8. `reply` with a `block` that didn't hand the caller its task, with no call of it waiting, types `[reply from <title>
   (<id>)]` and the text, then Enter, into the agent named; never into a terminal running no agent, or an agent no
   snapshot has found yet. From batch 7 the first line goes as a task's does (2): `[reply from <title> (<id>)] Its
   reply is in the pasted text.`, then the text. It ends no task. From card 69 `reply` needs no `block`: without it, or
   with the block of the agent that handed the caller its task, it is the caller's answer and **ends that task** with
   its whole text: the calls of the asker that wait for it get the text, otherwise the asker hears the end as any end
   is told (10), and the Templum Agent, which no block names, in its chat. It answers "Answered task t7."; with no task,
   "You are on no task: reply answers the task an agent handed you; name a block to write to one."; a blank text is
   refused. A second `reply` finds no task. `templum reply -` is the same, its text read from standard input.
9. A task typed into an agent at work on a prompt of its own is answered by the end of the turn that took it
   (batch 7, once its hooks say when it takes a prompt). Codex takes it into the turn under way once a tool ends or
   its answer is written, and says so then, in that turn. Claude Code says so at its Enter and queues it: a tool's
   end takes it into the turn under way, whose end answers it; a turn that ends first, or an Esc during a tool,
   leaves it to the next turn, which no hook says starts, and that turn's end answers it. An Esc while it writes
   ends its turn with no hook, and the task's answer is lost (deferred: [minors](../minors.md#connections)).
10. **Its end comes back** (batch 8). When a task ends and no call of the agent that handed it over waits for it
    (`delegate` with `wait`, `wait_for_idle`; a call its client cancelled no longer waits: Esc in the agent, or a
    call silent past Claude Code's idle timeout, 1800 s unless its server's `timeout` is longer), Templum types into
    that agent, as a reply is typed, a first line
    `[task t7 from Codex · api (k9x1d): done in 31 s, 60231 tokens, transcript /…/rollout.jsonl] Its answer is in the
    pasted text.` and its answer on the lines after; `failed` instead of `done` and `What happened is in the pasted
    text.`, then why: it didn't take the task, its turn ended without an answer (an error, an Esc), its agent left,
    or its block closed. What it
    spent is what its usage chip shows ([usage.md](usage.md)), since the task was handed over: Claude Code's cost
    (`$0.42`, with its status line, whose token totals are its last call's context, not what it used: batch 8's
    spikes), Codex's tokens; a session's first task (a spawned agent's, or one handed before Templum read any figure of
    it) counts it whole; nothing when unknown or when nothing new was read, never `0 tokens`. Templum reads it again a
    second after the end (Claude Code's status line gives its last cost about 0.3 s after its `Stop`), and the notice
    goes then. The transcript is the one its agent's last turn's end
    named (`transcript_path`). Besides its end, only a task that gets stuck comes back, and a quiet one (6): its agent starts waiting for its
    person in its terminal (a hook says so, a permission or another dialog, while it wasn't waiting already), or it
    seems stuck. It sends `[task t7 from Codex · api (k9x1d) waits for its person: Allow
    Bash?] Its screen is in the pasted text.` and the last 20 lines of its screen, as drawn (300 ms after its hook),
    once per wait; not while a call of the asker waits for the task, which hears its end, nor to an asker whose
    `wait_for(needs_you)` hears it. Nothing else comes back, as noise the asker can't act on: not a question
    its request waits in the inbox (`ask_user`, `connect`: the person sees it there, and the asker can't answer
    it), nor its timeout, which only the asker's call that waits for it tells (the agent may still work, and its end
    comes then). While the asker waits for its person, its agent is starting
    (no snapshot found it yet), its person typed into it in the last 3 seconds, its input holds text its person typed
    and didn't send (a draft, however long ago they typed it), or Templum typed into it (a task, a reply, a notice) in
    the last 8.1 seconds (that text's Enter, up to 2 s after it, a second one 3 seconds later, and the looks after it 3
    seconds more, may still be due), a notice waits, oldest first. Into a terminal whose agent is gone (its shell is
    in front, as when `templum delegate` ran from a script), never: the notice is dropped. A draft starts with the
    person's text or paste (not while the agent waits for them: that answers its dialog), or a Shift+Enter, Alt+Enter,
    Tab, Up, Down or Ctrl+Y; it ends with their Enter (Codex's and the others'; not Claude Code's, whose `\` and Enter
    is a new line: its `Submitted` at the Enter ends it; and not while the agent waits for them), Ctrl+C, an agent's
    start or quit, or a look at the screen, once their last key is 3 seconds old, that finds the input empty (in the
    last six rows, a prompt marker, `›`, `❯` or `>`, with nothing after it). A `Submitted` that comes once a turn takes
    a prompt in (Codex, Qwen Code, OpenCode) ends nothing: the person may be writing the next.
    Where the signals disagree the notice waits. The terminal says so while notices wait behind a draft: "3 answers
    waiting" (one: "1 answer waiting"), a chip whose click shows a tooltip, "They go in once your message is sent or
    cleared" ([notice-batching.md](notice-batching.md)).
    The notices about a task (its end, stuck, quiet, waits for its person) come in batches: one waits 15 seconds for
    others to the same agent, each new one restarting the 15; it goes at once when no other task of that agent still
    runs (an end on its way, in its SPENT_AFTER second, counts as running; one waiting for its person or quiet does
    not), and no notice waits more than 60 seconds. A batch of one is typed as above; several are one paste with one
    Enter, a first line, `[3 tasks ended] They are in the pasted text.` (`[3 notices: 2 tasks ended, 1 needs a look]`
    when they differ), then each notice as above, a blank line between, oldest first. A draft or the gaps
    above hold a batch due, and what comes meanwhile joins it. The other notices (a `wait_for` with `wait: false`, a
    schedule's reminder or failure, a question's answer after its client stopped waiting) go as soon as the draft and
    the gaps allow.
11. **A change** (batch 8). `delegate` from the agent whose task it is changes that task: typed at once under
    `[from Claude Code · api (b7k2m), a change to task t7] The change is in the pasted text: do it, then answer the task with
    reply.` (and how, as 2), even while it
    works, as Claude Code's queued messages are; the task's answer is the end of the turn that took the change, and
    its timeout starts again from the change's. Answered "Typed into k9x1d as a change to task t7." (or, with `wait`,
    the task's end); refused while the agent waits for its person (5), and while a spawned agent hasn't got its
    prompt yet (it would come first). Any other agent is still refused.
12. `interrupt` by the agent that handed the task over ends it without a notice ([Keys](#keys-stopping-and-closing)),
    and so does its `close_block`.

### Spawning

`spawn_agent` opens a new agent block beside the caller, wired to it, in the caller's folder or the one it names.
The block goes on a side of the caller, 48 units (two dots of the grid) from it: right or left of it with their tops
level, below or above it with their left edges level, and 48 or more from every other block and drawing. When blocks
wired to the caller already line up on one of those sides (its earlier subagents, its own note), it takes the first
free place down that column or along that row: an agent's subagents form one column on its right, evenly spaced, and
the next one takes a closed one's place. Else it goes the way of the caller's connections: of the spots right of,
below, left of or above the caller or a block wired to it (at their edge, or past what is in the way along that side),
the one nearest the caller and its connections all together, so they draw it their way. The block that opened the
caller doesn't draw it: a subagent's own agents go on its right, not back over its opener's column. Any block Templum
opens beside another (a terminal from an editor's tree, `open_file`'s editor) keeps off everything the same way.
A block opened at the view's middle (a new terminal, note, kanban, editor or checklist, a paste, a block the Templum
Agent opens near no block) goes there when nothing is within 48 of it; else in the nearest place 48 from a block there
and level with it, so blocks opened in one view grow a tidy grid around its middle instead of piling up. When the
person opens it and it lands out of view, the view glides just enough to show it whole.
With a worktree name, Templum first runs `git worktree add -b <name> <repository>/.worktrees/<name>` from that
folder's repository (and adds `.worktrees/` to the repository's `.git/info/exclude` once), and the agent starts
there, so several agents can change one project at once without stepping on each other. A prompt is typed once the
agent is ready: its process is seen and its screen has been still for a second; from batch 7, Claude Code and Codex
also take typing then: they bracket pastes and show their cursor, which both hide in the questions they ask first
(trusting the folder; Codex's "Hooks need review"), where the prompt's Enter would answer for the person (on
Windows, whose console may not pass those on, the still screen alone). Neither one's session start tells: Codex's
`SessionStart` comes only with its first prompt, and Claude Code's a moment before its input box is ready, its
bracketed paste off and on again. A prompt not typed within a minute of the spawn (its person hasn't answered) is
dropped, and its task ends with the last 20 lines of its screen. From batch 7 it is typed under a first line
`[from <caller>] Your task is in the pasted text: do it.`, as a task is ([Delegating](#delegating) 2); from batch 8
`[from <caller>, task t8]`. The result names the new block and its folder, and from batch 8 ends "Its prompt is
typed once it is ready, as task t8."

`spawn_agent` also opens an account of an agent by its id (`claude2`, [accounts.md](accounts.md#opening-it)). It
takes a `model` and an `effort` (batch 6), typed after the agent's command and before what Templum
adds (Claude Code's `--mcp-config` takes every word after it):

| Agent | `model` | `effort` |
|---|---|---|
| Claude Code | `--model <m>` | `--effort <e>`: `low`, `medium`, `high`, `xhigh`, `max` |
| Codex | `-m <m>` | `-c model_reasoning_effort=<e>`: `low`, `medium`, `high`, `xhigh`, `max`, `ultra` (Codex 0.154's models; `ultra` only some of them), unquoted: Codex reads a word that isn't TOML as a string |
| Gemini CLI | `-m <m>` | — |

They are typed into a shell: a model is 1 to 64 letters, digits, `.`, `-`, `_`, `:` and `/`, beginning with a letter
or a digit (no option can hide in it); an effort is one of its agent's. Anything else, an effort for Gemini CLI, or
either for another agent is refused before anything opens, a worktree included. The block keeps them: a restart, or
Ctrl+Z after a close, resumes its agent with them while that agent runs there. A snapshot that finds that agent gone,
or another agent there, drops them (also from the file it writes then): they remember the agent they were typed for, so
one typed by hand where a spawned agent never started doesn't get them. Templum's agents get them also when the person
changed their command. The result names them ("…, connected to you, on gpt-5.6-luna (high).").

The keyboard, the selection and the camera stay where they were; one Ctrl+Z closes the new block and its wire. The
worktree comes from the repository of the folder the agent starts in, always under its main working tree's
`.worktrees/`, on a new branch from that folder's HEAD; a name other than letters, digits, `-`, `_`, `.` and `/`, or one
already a branch, is refused. The prompt is a task the caller handed over: the dot travels, `wait_for_idle` waits for
its answer, and a prompt the agent doesn't take ends at once, as a delegated task does. An agent no snapshot has found a
minute after the spawn never started (its folder's trust declined, a question of its shell took the command): its prompt
is dropped and its task ends, as when an agent leaves.

### Keys, stopping and closing

Batch 6.

- **`send_keys`** presses keys in a terminal wired to the caller, one at a time and 50 ms apart (an Esc and the key
  after it in one burst read as Alt+key): `enter`, `escape`, `tab`, `shift+tab`, `backspace`, `delete`, `up`,
  `down`, `left`, `right`, `home`, `end`, `page_up`, `page_down`, `space`, `ctrl+<letter>`, or one printable
  character (`2`, `y`). At most 16 a call; a name it doesn't know refuses the call before any key goes. A key goes as
  the program asked for keys (application cursor keys, the kitty keyboard), a character as typed, never as a paste:
  a dialog takes `2` as its option, where a pasted `2` lands in the agent's input. The result ends with the last 20
  lines of the screen, 300 ms after the last key.
- **Dialogs.** Keys go into an agent waiting for its person too: the wire is the person's consent that the agents
  on it answer each other's dialogs. Keys from another block count as the person's keys for its state ([An agent's
  state](#an-agents-state)), and the block shows **answered by** *Claude Code (b7k2m)* (the caller as its `[from …]`
  line names it) after its dot for 10 seconds, in place of its usage chip, and in its dot's tooltip, whatever it was
  waiting for: a trust question no hook tells of is no wait Templum knows. Refused: the caller's own block, and an
  agent on another agent's task unless it waits for its person.
- **`interrupt`** presses Esc once in an agent wired to the caller while it works or waits for its person (a second
  Esc at Claude Code's empty prompt opens its rewind), and ends the task the caller handed it (a spawned agent's
  prompt still waiting is dropped with it): its waits get "Interrupted by you." Refused while it is idle, and while
  it is on another agent's task. Its result ends with the screen, as `send_keys`'s.
- **`close_block`** closes a block the caller opened with `spawn_agent` and is still wired to (unwiring it takes
  the consent back), without asking the person, as one step of Ctrl+Z; its agent ends at once, with no fade (one in a
  workspace not shown would wait for a frame). Ctrl+Z brings it back as any closed block, its agent resumed on the
  session the last snapshot found it on (not the folder's latest, the caller's when they share it), with its model.
  Any other: "Only the person closes k9x1d: you didn't open it." One it opened but is no longer wired to is refused
  as any block not connected: connect asks the person again.
- `send_to_terminal`'s Enter goes 150 ms after its text, as a task's does, and its answer after its Enter: a caller
  that waits for each answer (a script's `templum send … --enter` twice) gets its texts and Enters in order. A block
  closed meanwhile gets no Enter, and the answer says so.

## Identity

- Every block gets a **block id** when it is added. It is saved with the block and kept when the canvas comes back,
  when a closed block is brought back with Ctrl+Z and when the block moves to another workspace. A pasted block,
  or one whose id is already taken, gets a new one. Ids are unique among all workspaces.
- Every terminal already gets `TEMPLUM_TERMINAL` ([usage.md](usage.md)); every program in it inherits it, the MCP
  server and hooks an agent starts included. The bus finds the caller by it, at each call, so a block moved to
  another workspace stays the same caller.
- Every terminal also gets `TEMPLUM_BUS`, the bus's address, when there is a bus, and a `PATH` that starts with
  `<data>/bin`, so `templum` in a Templum terminal is the Templum that opened it. There Templum keeps, on Unix, a
  link named `templum` to itself; on Windows `templum.exe`, a hard link to its program or, where none can be made
  (another drive), a copy, made again at start when its length or modified time aren't the program's. (The
  program's own folder may be Downloads, with no `templum.exe` and any other program in it.) Empty entries of
  Templum's own `PATH` (the current folder) are left out.

## The bus

- **Unix:** a socket at `<data folder>/bus/<Templum's pid>-<n>.sock`, in a folder only the user can open (0700).
  `TEMPLUM_BUS=unix:<path>`. When the path would pass 100 bytes (macOS's limit is 104), the socket goes to
  `bus.sock` in a folder of its own in the temporary folder instead, `templum-<12 random hex digits>`, made 0700 and
  only if it isn't there yet (`/tmp` is shared: no one can guess its name or make it first); it goes with the bus.
  A socket file left by a Templum that is gone is removed from `<data>/bus` when the bus starts.
- **Windows:** a TCP socket on `127.0.0.1` at a port the system picks, and a random token.
  `TEMPLUM_BUS=tcp:127.0.0.1:<port>:<token>`.
- **A hello whose client is `api` is refused** (the connection is closed with nothing written): the API's calls are
  made inside Templum, as bus messages from its own server, never on the socket, so an agent in a terminal can't borrow
  the API's reach (card 171, [api.md](api.md)).
- **No data folder** (`--bench`, a second Templum), no bus: terminals get an empty `TEMPLUM_BUS`, so one started
  from another Templum's terminal doesn't pass that one's on, and agents get no MCP server or hooks.
- **Trust:** the bus trusts every process of the user that can reach it, as the user's shells can already type into
  every terminal. The folder's permissions (Unix) and the token (Windows) keep other users out. The terminal a
  client names is taken as is.
- **Several Templums** each have their own bus; a terminal's `TEMPLUM_BUS` names its own Templum's.
- **The protocol** is JSON-RPC 2.0, one message per line, for every client. A client's first line says who it is:

  ```json
  {"jsonrpc":"2.0","method":"templum/hello","params":{"terminal":"12345-3","client":"mcp","token":null}}
  ```

  `client` is `mcp` (the bridge), `cli` (a verb) or `hook`. A first line that isn't a hello, a wrong token, a
  first line over 4096 bytes, or one not whole 5 seconds after connecting (however its bytes trickle in) closes the
  connection; nothing is written to a connection before its hello is accepted, and once it is, the connection has
  no time limit. Hooks send `templum/event` notifications, and a hook at a session's start also one request,
  `templum/instructions` with `{"agent": <the hook's agent>}`, answered with `{"text": <string>}`: the person's
  instructions for that agent and terminal, framed, empty when none apply
  ([instructions.md](instructions.md)); the command line's verbs send `tools/call`. When a connection ends the bus
  tells Templum (`templum/closed`, with the connection's number), and what that client waited for is dropped.
- Requests are answered on Templum's main thread, in the order they arrive per connection; a request that waits (a
  question, a delegation) never holds back the others.

## The MCP server

`templum mcp` is a bridge: it connects to the bus, sends its hello, then copies lines from its stdin to the bus and
back to its stdout. (The Templum Agent's process gets another server, `templum control`, a `control` client of the same
bus: [templum-agent.md](templum-agent.md#the-control-server).) Templum answers MCP's `initialize`, `ping`, `tools/list` and `tools/call` ([MCP 2025-11-25](
https://modelcontextprotocol.io/specification/2025-11-25)); tools only, no resources or prompts.

- **`initialize`** answers the client's protocol version when Templum knows it (`2025-11-25`, `2025-06-18`,
  `2025-03-26`, `2024-11-05`: none of them changed what tools exchange), else `2025-11-25`; server `templum`,
  Templum's version, and `instructions` written for the caller:

  > In Templum, an agent the person asks for is a block on this canvas: start one with spawn_agent, or hand work to
  > an agent connected to you with delegate. Your built-in subagents run hidden inside you, where the person can't
  > see them: use them only when the person asks for a subagent. You are block b7k2m, "Claude Code · api", in
  > Templum workspace "Main". Connected to you: t4q8p (terminal "dev server"), k9x1d (kanban "Sprint"). In this
  > workspace, not connected (ask with connect): 4qm2a (note "Ideas"). Name blocks by these ids. You may only read or write your own block and those connected to you; connect asks the person
  > for another. list_blocks gives the current list. When a connected kanban has your task, claim its card
  > (claim_card: a card for another agent isn't yours) and move it to Doing when you start; check its checklist's
  > items as you finish them (edit_card); when you are done, comment on it what you did and what the next agent
  > needs to know (comment_card), then move it to Review (Done when the board has no Review); write what the next
  > agent needs to know in the connected note too, and check the items of a connected checklist as you finish them.
  > For work of several steps, open your own checklist with create_block and check its items as you go.

  The first sentence comes first because Claude Code shows its own Agent tool whole and Templum's tools only by name
  until it searches them, so "start an agent" would otherwise open a subagent the person never sees; Codex weighs
  the first 512 characters of a server's instructions most, and Claude Code cuts them at 4096, which the block lists
  may outgrow. The kanban and note sentence comes with their tools (batch 4). From batch 8 they add: "A task you
  hand over without wait comes back typed into your terminal as [task …]: it isn't the person writing."

  Titles come from what runs in a block (a terminal's is whatever its programs set), so the instructions, `whoami`
  and `list_blocks` give them cleaned: no control characters, no invisible ones (the soft hyphen `U+00AD`, zero
  widths `U+200B`–`U+200F`, direction overrides `U+202A`–`U+202E` and `U+2066`–`U+2069`, the word joiner and
  invisible operators `U+2060`–`U+2064`, `U+FEFF`, and the tags `U+E0000`–`U+E007F`, which can spell hidden text),
  line breaks and runs of blanks as one space, at most 60 characters (cut with "…"), and the instructions put them in
  double quotes, a `"` inside escaped.
- **`tools/list`** is the same for every caller; which blocks a tool may touch is checked at each call.
  `spawn_agent` and `delegate` carry `"_meta": {"anthropic/alwaysLoad": true}`: Claude Code lists them whole from
  the session's start, beside its own Agent tool, where its other MCP tools wait behind its tool search.
- **Results** are text (`content: [{type: "text"}]`). A refusal or a failure is a result with `isError: true` and a
  sentence that says what to do next: `k9x1d isn't connected to you. Ask the person with connect("k9x1d").`
- **Outside Templum** (no `TEMPLUM_BUS` or `TEMPLUM_TERMINAL`): `templum: not inside a Templum terminal` on stderr,
  exit code 2, nothing on stdout. **Templum not running:** `templum: Templum isn't running`, exit code 2. **Templum
  quits** during a session: the bridge closes its stdout and exits 0; the agent reports the server as disconnected.
  **A terminal this Templum doesn't have:** `initialize` succeeds with instructions saying so, and every tool call
  fails.

### Tools

"Wired" means a wire between the caller and the block named; a caller may always use its own block.

| Tool | Arguments | Does | Needs | Batch |
|---|---|---|---|---|
| `whoami` | — | The caller's id, title, kind, folder and workspace | — | 1 |
| `list_blocks` | — | Every block of the caller's workspace: id, kind, title, folder, agent and its state (from batch 7 with what it asks), connected or not; `, no Templum tools` after an agent's state when its terminal has none (card 69); `, a Claude Code subagent's transcript, read-only` after a subagent's block, which the tools that type or press refuse ([claude-subagents.md](claude-subagents.md#its-block)) | — | 1, card 69, card 127 |
| `read_terminal` | `block`, `lines` (200, at most 2000), `last_command` (false) | The last lines of the terminal's history and screen, or its last finished command and its output (needs [shell integration](shell-integration.md)) | wired | 1 |
| `send_to_terminal` | `block`, `text`, `enter` (false) | Types the text, as a paste; Enter after it when asked | wired | 1 |
| `notify` | `text` | A notification from the caller ([Requests](#requests-from-agents)) | — | 1 |
| `connect` | `block`, `timeout` (1800 s) | Asks the person to wire the caller to the block; waits for the answer | — | 3 |
| `spawn_agent` | `agent` (an id of [agents.md](agents.md) or of `[agents]`), `prompt`, `folder`, `worktree`, `model`, `effort`, `fork`, `role` | [Spawning](#spawning); the prompt's task id; with `fork`, a copy of the caller's own Claude Code or Codex on its conversation ([agent-tools.md](agent-tools.md#spawn_agent-with-fork)); with `role`, a role of `[roles]` ([agent-tools.md](agent-tools.md#roles)) | — | 3, 6, 8, card 35 |
| `delegate` | `block`, `task`, `wait` (false), `timeout` (600 s) | [Delegating](#delegating); without `wait`, says the task's id once its agent took it, or why it didn't (card 118; one still starting: at once), and why the agent has no Templum tools, if so; its end is typed into the caller's terminal; to the agent on the caller's task, a change to it | wired, an agent | 3, 8, card 69, 118 |
| `reply` | `text`, `block` (optional) | Answers the task the caller is on, whole, and ends it ([Delegating](#delegating) 8); with a `block` that didn't hand it the task, types `[reply from …]` and the text into it | wired when it names a block | 3, 8, card 69 |
| `wait_for_idle` | `block`, `timeout` (300 s) | Waits until the agent finishes its turn, and for the answer to a task the caller handed it, past `delegate`'s timeout too; its last answer | wired | 3 |
| `ask_user` | `questions` (each `question`, `header`, `options` with `label`, `description`, `preview`, `multiSelect`), or `question` and `options` (strings); `timeout` (none) | Questions in the inbox ([Requests](#requests-from-agents), [ask-user.md](ask-user.md)); the answers as JSON, or with `question` the answer as text | — | 3 |
| `propose_plan` | `plan` (Markdown, at most 256 KB), `title`, `timeout` (none) | A plan in the inbox, Approve or Reject with a comment ([agent-tools.md](agent-tools.md#propose_plan)); `Approved.`, or the error `Rejected.`, with the comment | — | card 35 |
| `list_cards` | `board`, `card` | The wired kanban block's board: columns, and each card's id, title, labels, priority, assignee (`(you)` on the caller's), checklist progress, comment and attachment counts; with `card`, that card whole: its description, checklist items, attachments (each one's name, size and absolute path, [card 152](kanban.md#attachments-kanban-card-152)) and comments, each with its number (and "edited" once its author changed it) ([Cards for an agent](#cards-for-an-agent)) | wired kanban | 4, card 40, card 77 |
| `create_card` | `board`, `column`, `title`, `description`, `priority`, `for`, `checklist`, `labels` | A new card at the column's end (the board's first column when left out); its id. Its description tells an agent a review's findings are a card each (card 35) | wired kanban | 4, card 40 |
| `edit_card` | `card`, `title`, `description`, `labels`, `priority`, `for`, `add_items`, `remove_items`, `check`, `uncheck` | Sets the card's title, description, labels, priority and who it is for, adds, removes, checks and unchecks checklist items: one Ctrl+Z step ([Cards for an agent](#cards-for-an-agent)) | wired kanban | card 40 |
| `move_card` | `card`, `column`, `before` | Moves a card to a column, before another card of it (`before`) or at its end; into the board's last column or one titled Review (either case), unless the card's newest comment is the caller's, its answer asks for one: "Say on it what you did and what the next one needs to know: comment_card." | wired kanban | 4 |
| `comment_card` | `card`, `text` | Adds a comment signed with the caller's id: what it did on the card, and what the next one needs to know, when it finishes it; its answer says the comment's number: `Commented on card 40 (comment 51).` | wired kanban | 4, card 77 |
| `edit_comment` | `card`, `comment`, `text` | The text of a comment the caller wrote becomes `text`; the comment keeps its author, its place and its time, and reads "edited". Refused for the person's comment and another agent's ([Cards for an agent](#cards-for-an-agent)); one Ctrl+Z step | wired kanban | card 77 |
| `delete_comment` | `card`, `comment` | Deletes a comment the caller wrote; refused as `edit_comment` is; one Ctrl+Z step | wired kanban | card 77 |
| `claim_card` | `card` | Makes the caller the card's assignee; refused when another block holds it, or (card 40) when the card is for another agent ([Cards for an agent](#cards-for-an-agent)) | wired kanban | 4, card 40 |
| `read_note` | `block` | The text of the note the note block shows | wired note | 4 |
| `append_note` | `block`, `text` | Adds the text at the note's end, after a blank line; the note shows it at once and Ctrl+Z takes it back | wired note | 4 |
| `replace_note` | `block`, `text`, `old` | The note's whole text, or its one passage `old`, becomes `text`; one Ctrl+Z step ([agent-tools.md](agent-tools.md#fixing-a-note-or-a-checklist)) | wired note | card 35 |
| `read_checklist` | `block` | The checklist the block shows: its name and progress, then one line per item: its id, `[x]` or `[ ]`, its text ([checklist.md](checklist.md#agents)) | wired checklist | — |
| `check_item` | `block`, `item`, `done` (true) | Checks the item (unchecks it with `done: false`); one Ctrl+Z step | wired checklist | — |
| `add_items` | `block`, `text` | Each line with text becomes an item at the end; a leading `- ` or `* `, then `[ ]` or `[x]`, is dropped (`[x]` adds it checked); the new items' ids; one Ctrl+Z step | wired checklist | — |
| `create_block` | `kind` (`note`, `checklist`, `terminal`), `text`, `name`, `items`, `folder`, `command` | A note, a checklist or a terminal running `command` beside the caller, wired to it; one Ctrl+Z closes it ([agent-tools.md](agent-tools.md#create_block)) | — | card 35 |
| `edit_item` | `block`, `item`, `text` | The item's text becomes `text`; one Ctrl+Z step | wired checklist (the control server's too, on any checklist block: 171 2e) | card 35 |
| `delete_item` | `block`, `item` | Deletes the item, as its row's ×; one Ctrl+Z step | wired checklist (the control server's too, on any checklist block: 171 2e) | card 35 |
| `open_file` | `path`, `line` | Opens the file at the line in the wired editor block whose folder holds it; with none, in a new editor block beside the caller, wired to it. Neither the keyboard nor the camera moves | — | 4 |
| `wait_for` | `block`, `event` (`idle`, `needs_you`, `command_finished`, `output`, `attention`), `pattern`, `lines` (0), `wait` (true; false with a pattern: a watch whose match is typed back), `timeout` | Waits for the event in the block: `idle` as `wait_for_idle`; `needs_you`, its agent or its balloon asking the person (at once when it already does), with what it asks and, from batch 7, the last 20 lines of its screen; `command_finished`, the next command to finish there, with its output (needs [shell integration](shell-integration.md)); `output` (which `pattern` implies), the next line printed there that matches `pattern`, with the line and its last 20 lines, from its last `lines` too ([agent-tools.md](agent-tools.md#wait_for-with-a-pattern)); `attention`, its next bell or notification | wired | 5, card 35 |
| `send_keys` | `block`, `keys` (1 to 16) | Presses the keys ([Keys](#keys-stopping-and-closing)); the screen after them | wired | 6 |
| `interrupt` | `block` | Esc in its agent; ends the task the caller handed it | wired, an agent | 6 |
| `schedule` | `block`, `text`, `at` (`HH:MM`), `after`, `every` (minutes, 1 to 10080) | Delivers `text` later, from the caller: a task to an agent (`[from …, task t9, scheduled s3]`), never typed into a terminal running none, or a notice to the caller's own block; again every `every` minutes for 7 days at most ([agent-tools.md](agent-tools.md#schedule-and-cancel_schedule)); its id | wired, or the caller's own | card 35 |
| `cancel_schedule` | `schedule` (`s3`) | Cancels one of the caller's schedules; `whoami` lists them | — | card 35 |
| `remove_worktree` | `worktree`, `folder`, `discard` (false) | Removes a worktree `spawn_agent` made and its branch: at once when clean and merged; with `discard`, once the person says Delete ([agent-tools.md](agent-tools.md#remove_worktree-and-merge_worktree)) | none works in it | card 35 |
| `merge_worktree` | `worktree`, `folder` | Merges its branch into the branch of `folder`'s working tree once the person says Merge; a conflict is aborted | none works in it | card 35 |
| `stop_agent` | `block` | Ends the program in the terminal (its agent, or the command `create_block` started), everything below it and what it left running; its shell stays ([agent-tools.md](agent-tools.md#stop_agent)); ends the task the caller handed it | wired, opened by the caller | card 35 |
| `close_block` | `block` | Closes a block the caller opened | wired, opened by the caller | 6 |

- A block is named by its id. A **board** is named by its name or id, and may be left out when one wired kanban
  block shows one; a **column** by its title (either case) or id; a **card** by its id, as `list_cards` shows it.
  `board`, `column` and `card` may be numbers or strings; a blank title, text or comment is refused.
- `read_terminal` reads the terminal's own history, never what is painted, so a terminal off screen reads as
  current. The result is at most 256 KB. A suggestion an input draws where its shown cursor waits, a faint run from
  the cursor on (Claude Code's `Try "fix lint errors"`, Codex's `Ask Codex to do anything`, a completion), isn't
  typed text: it is left out, there and in every screen Templum answers with (`send_keys`, `interrupt`, `wait_for`,
  a task not taken or its dialog), so an agent doesn't take it for its person's.
- Agents may stop waiting on long tool calls. Claude Code gives up on a call that sends nothing for 1800 s, its idle
  timeout for stdio servers, unless the server's own `timeout` is longer (seen 2026-10-17: `MCP server "templum"
  tool "wait_for_idle" sent no response or progress for 1800s; aborting`; that `timeout` lifts it from Claude Code
  v2.1.203, the version that brought the idle timeout to stdio servers); the `mcp.json` Templum gives it sets
  seven days on Templum's server, for every one of its tools (`wait_for_idle`, `wait_for`, `delegate` with `wait`,
  `ask_user`). It also moves a call past 2 minutes to the background, where it keeps waiting by its documentation
  (not checked by hand). The descriptions of the tools that wait say they may take minutes.
- A `timeout` is whole seconds from 1 to 3600; a longer one is cut to 3600. The agents Templum opens wait seven days
  for a tool call (Codex's `tool_timeout_sec=604800`, the `timeout` of Claude Code's `mcp.json` and of Gemini CLI's
  extension), so a question asked without a `timeout` can wait for its person.

### Cards for an agent

*Implemented (kanban card 40, [kanban.md](kanban.md#version-2-kanban-card-40)):
[docs/plans/2026-10-18-kanban-v2.md](../plans/2026-10-18-kanban-v2.md).*

A card's assignee is none (anyone), **any session of an agent** (by the agent's id: `claude`, `codex`, an `[agents]`
key) or **one block** (one agent's session, kept across restarts: [kanban.md](kanban.md#who-a-card-is-for)). A block
runs the agent Templum sees in it (`Pane::seen.agent`), else the one Templum started there (`Pane::flags_for`); it
runs "any X" when its agent is X or runs X's program (the first word of their commands, without its folder or
`.exe`).

- **`for`** (in `create_card` and `edit_card`) names who: `anyone`; an agent's id or title in either case (`codex`,
  `Claude Code`); or a terminal block's id, in any workspace. Agents are matched first. Anything else is refused:
  `There's no agent or terminal block {for}. Agents: claude, codex, …; or a terminal's block id, or anyone.`
- **`priority`** is `low`, `medium`, `high` or `none`: the schema lists them so (an enum), and the server takes them
  in either case too (the command line's `--priority High`); another word is refused by the server:
  `"priority" is low, medium, high or none`.
- **`list_cards`** lines, in this order after the id and title: the labels, `priority high`, the assignee
  (`assignee b7k2m "Claude Code · api"` for a block, `for any codex` for an agent; ` (you)` after it when it is the
  caller's block, or the caller's agent), `checklist 2/5`, the comments, `2 attachments`. With `card`, after the
  description:
  `checklist:` then one line per item, `  41 [ ] Build` (its id, `[x]` or `[ ]`, its text), as `read_checklist` does;
  then, with a data folder, `attachments:` and a line each, `  shot.png  47 KB  <data folder>/attachments/41/shot.png`.
- **`create_card`** also takes `priority`, `for`, `checklist` (text, an item per line, read as `add_items` reads
  it) and `labels` (a list of red, yellow, green, cyan, blue, magenta). Its answer is unchanged: `Created card 40.`
- **`edit_card`** takes `card` and at least one of `title`, `description` (blank clears it), `labels` (`{"red": true,
  "blue": false}`: on, off; the rest keep), `priority`, `for`, `add_items` (text, as `add_items`), `remove_items`,
  `check` and `uncheck` (a list of item ids; the schema declares only the list, with no `anyOf`, but one id, a number
  or its text, is taken too). The server refuses a call with none: `edit_card needs something to change: title,
  description, priority, for, labels, add_items, remove_items, check or uncheck`. An item id the card doesn't have
  refuses the whole call before
  anything changes: `Card 40 has no item 99: list_cards with card shows its items.` What it does is one Ctrl+Z step,
  and its answer says it: `Card 40: priority high; for any codex; added items 43, 44; checked 41; unchecked 42.`
  Its `for` follows `claim_card`, and a refusal changes nothing: on a card another block holds it is refused with
  `claim_card`'s "taken by" sentence; when it names the caller (its block id, or an agent the caller runs) on a card
  for an agent the caller doesn't run, with `claim_card`'s "is for any …" sentences. Its `priority`, `add_items`,
  `check` and `uncheck` work on any card of a wired kanban.
- **`claim_card`:**

  | The card's assignee | The caller | Answer |
  |---|---|---|
  | none | any block | `Card 40 is yours.` (the assignee becomes its block) |
  | its block | — | `Card 40 is already yours.` |
  | another block | — | refused, as before: `Card 40 is taken by b7k2m "Codex · api". Ask the person to free it (the ✕ by its assignee in the open card), or take another card.` |
  | any `codex` | runs Codex, or an agent with Codex's program | `Card 40 is yours.` (the assignee becomes its block) |
  | any `codex` | runs another agent | refused: `Card 40 is for any Codex, and you are Claude Code: take a card for you or for anyone.` |
  | any `codex` | runs none Templum can name yet | refused: `Card 40 is for any Codex, and Templum can't tell yet which agent you are: try again in a few seconds.` |

- **`move_card` and `comment_card`** stay open to every wired agent. `move_card`'s `before` is a card of the target
  column (`Card 41 isn't in Doing.`, `A card can't go before itself.`): the card goes just before it. `move_card`'s
  answer ends with ` It is for any codex, not you.` (or `b7k2m "Codex · api"`) when the card's assignee isn't the
  caller's.
- **`edit_comment` and `delete_comment`** (card 77) change only the caller's own comments, those whose author
  (`by`) is the caller's block id; the rest, the person's and other agents', are refused with
  `Comment 52 is by the person, not you: you can only change your own.` (`b7k2m "Codex · api"` for another agent).
  A comment is named by its **number**, a never-reused id like a checklist item's: `list_cards` with `card` shows it
  (`comment 51 by b7k2m "Claude Code", 3 min ago, edited:`) and `comment_card` says it. A number that isn't on the
  card is refused (`Card 40 has no comment 99: list_cards with card shows its comments.`); an `edit_comment` whose
  text is blank is refused too (`delete_comment` removes one). An `edit_comment` with the text the comment already has
  changes nothing, takes no Ctrl+Z step and doesn't mark it "edited": it answers `Comment 52 already says that.`
  (not an error). They are not in the Templum Agent's tools, as
  `edit_item` and `delete_item` aren't.
- **The instructions** ask an agent to claim its card when it starts and to check its checklist's items as it
  finishes them ([The MCP server](#the-mcp-server)).
- **The command line:** `templum kanban add` takes `--priority`, `--for` and `--checklist`; `templum kanban edit <card>`
  takes `--priority`, `--for`, `--add <text>`, and `--check <item>` and `--uncheck <item>`, each as many times as
  wanted; `templum kanban edit-comment <card> <comment> <text>` and `templum kanban delete-comment <card> <comment>`
  (card 77).

### Agent tools (card 35)

*Planned (kanban card 35): [agent-tools.md](agent-tools.md), [its plan](../plans/2026-10-18-agent-tools.md).*

`stop_agent` (an agent and what it left running), `wait_for` with a `pattern` (a printed line, as "Ready in"),
`create_block` (an agent's own note, checklist, or a terminal running a command), `replace_note`, `edit_item` and
`delete_item`, `spawn_agent` with `fork` and `role`, `propose_plan`, `schedule` and `cancel_schedule`, and
`remove_worktree` and `merge_worktree`. Each batch adds its rows to the [Tools](#tools) table above and its verbs to
[the command line](#the-templum-command-line) as it lands.

## Hooks and agent state

Templum hears an agent's turns through its hooks. Each hook runs `templum hook <agent>`, which reads what the agent
hands it, turns it into an event, sends it to the bus with a one-second limit, and exits 0 at once, even when
Templum is gone.

**A session's start asks and prints.** For Claude Code, Codex, Gemini CLI, Copilot CLI and Qwen Code, the hook of a
session's start (`SessionStart`, `sessionStart`; any `source`: a new session, a resume, a `/clear`, a compaction, for
which no event is sent, as above) also sends `templum/instructions` on the same connection, after the event, and waits
for the answer 2 seconds at most. Templum answers the instructions the person wrote that apply to that terminal's
workspace and agent ([instructions.md](instructions.md)); the hook prints them in the agent's shape (Claude Code, Codex,
Gemini CLI and Qwen Code: `{"hookSpecificOutput":{"hookEventName":"SessionStart","additionalContext":…}}`; Copilot CLI:
`{"additionalContext":…}`), and the agent takes them as context. No answer, an error, an empty text or no Templum: it
prints nothing, and exits 0. Every other hook prints nothing, so a permission dialog still decides.

**Which instructions apply** (the `templum` crate's, `Workspace::standing_for`; [instructions.md](instructions.md)): the
terminal's block names the workspace; the agent is the one Templum sees in it when it runs the hook's agent, else the
block's account of the hook's agent, else the hook's agent itself, so a stale look at the screen never wins over the
hook. The answer is the frame (whose they are, that they hold for the session with the person's later words winning,
that they replace an earlier set) and, under `##` and each one's name, the text of every instruction that applies:
those for every workspace first, then the workspace's own, each in the list's order. Empty for a terminal that isn't a
block of this Templum and while connections are off.

| Agent | How Templum hears it | Events |
|---|---|---|
| Claude Code | hooks in the `--settings` Templum types (JSON on stdin) | started, working, done, waiting for the person, ended: the table below |
| Codex | hooks in the `-c` flags Templum types (JSON on stdin), once its person trusted them (batch 7; before, `notify`: done with its last answer) | the table below |
| Gemini CLI | the hooks of Templum's extension (below), JSON on stdin | `SessionStart` started, `BeforeAgent` took a prompt: working, `AfterTool` working again after a permission, `AfterAgent` done with `prompt_response`, `Notification` (`ToolPermission`) waiting for the person, `SessionEnd` ended |
| Copilot CLI, OpenCode, Kilo Code, Qwen Code, Pi | from card 44: a plugin's hooks, a JS plugin, a system settings file, an extension ([Every agent that can take it](#every-agent-that-can-take-it)) | each one's table there |
| Others | — | none: their screen ([An agent's state](#an-agents-state)), and [Delegating](#delegating) point 6 |

Claude Code's hooks:

| Hook | Event |
|---|---|
| `SessionStart` | started; nothing when its `source` is `compact`: compacting, automatically in the middle of a turn, leaves the agent working |
| `UserPromptSubmit` | took a prompt: working (what confirms a task's Enter, [Delegating](#delegating) 2). It comes at the Enter, also for a prompt Claude Code queues behind the turn under way, with that turn's `prompt_id` (batch 7's spikes) |
| `PermissionRequest` | waiting for the person, "Allow *tool*?" ("Claude asks you a question" for `AskUserQuestion`, "Claude has a plan for you" for `ExitPlanMode`), as soon as the permission dialog opens. The hook prints nothing and exits 0, so the dialog still decides |
| `PostToolUse` | working (again, after a permission); from batch 7 never a new turn: only a prompt taken starts one |
| `PostToolUseFailure` | working (again, after a permission: an allowed tool may fail), or done without an answer when `is_interrupt` is true (Esc while the tool ran) |
| `Stop` | done, with its last answer (`last_assistant_message`); still working while its `background_tasks` lists a subagent, or a subagent's report is still to come ([claude-subagents.md](claude-subagents.md)) |
| `StopFailure` | done, without an answer: an API error ended the turn, and `Stop` doesn't come |
| `SubagentStart`, `SubagentStop`, and any hook with an `agent_id` | one of its subagents: never its own turn ([claude-subagents.md](claude-subagents.md#hooks)) |
| `Notification` | waiting for the person on `permission_prompt`, `elicitation_dialog` and `agent_needs_input` (they come only after about six seconds without typing, hence `PermissionRequest`); `permission_prompt` says the dialog `PermissionRequest` already did, so its "Allow *tool*?" stays; not working on `idle_prompt` (the prompt has sat idle): never a wait for the person, since the agent asks nothing; the others nothing |
| `SessionEnd` | ended |

Esc while a tool runs ends Working at once (`PostToolUseFailure` with `is_interrupt`). Esc while the model writes
fires no hook of its own: the dot stays on Working until its screen has been still for 5 seconds
([An agent's state](#an-agents-state)).
A Templum tool's call works again at once after its own "Allow mcp__templum__*tool*?": the call arriving says the
person allowed it, and `delegate` or `wait_for_idle` may take minutes before their `PostToolUse`.

Codex's hooks (batch 7; checked with Codex 0.154: JSON on stdin with `hook_event_name`, `session_id`, `turn_id`
but for `SessionStart`, `transcript_path`, `cwd`, `model` and `permission_mode`):

| Hook | Event |
|---|---|
| `SessionStart` | started; nothing when its `source` is `compact`. It comes with the first prompt, just before its `UserPromptSubmit`, not when Codex opens |
| `UserPromptSubmit` | took a prompt: working. One typed while Codex works (Enter) comes once the turn under way takes it in, after a tool or once its answer is written, with that turn's `turn_id`; one queued with Tab, as the next turn |
| `PermissionRequest` | waiting for the person, "Allow *tool*?" (`tool_name`: `Bash` for a command), as its dialog opens; the hook prints nothing, so the dialog still decides |
| `PostToolUse` | working (again, after a permission); never a new turn: a command an interrupt left running ends later with its own |
| `Stop` | done, with its last answer (`last_assistant_message`) |
| `Interrupt` | done, without an answer: Esc or Ctrl+C stopped the turn, and no `Stop` comes. Codex gives it 3 s at most |

Codex's `SessionEnd` isn't followed (Codex also sends it after 30 idle minutes): a snapshot tells when Codex quits.

From batch 8 a turn's end (done, with or without an answer) also hands Templum the `transcript_path` its hook got:
Claude Code's and Codex's payloads carry it in every hook (batch 7's spike logs); Gemini CLI's is assumed (not checked:
no login). Templum keeps the last one a terminal's agent named, until its next session starts or the agent leaves, for
the notice of a task's end ([Delegating](#delegating) 10).

Claude Code merges hooks from every settings source, so the person's own hooks still run; Codex merges those of its
`-c` flags with the person's `hooks.json` too (its `/hooks` lists both, each active).

## What Templum types into each agent

Only for an agent Templum opens (the canvas menu, `spawn_agent`, and resuming on restore), when `[connections]` is
on, there is a bus, the shell isn't WSL, and every path is typeable (`typed_path` of [usage.md](usage.md); when
one isn't, the agent starts without connections, and a notification says so once per session; the notification is
deferred: [minors](../minors.md#connections)). A `claude` or `codex` typed by hand gets the same through
[Agents typed in a terminal](#agents-typed-in-a-terminal).

**Claude Code:** `claude --settings <data>/claude-statusline.json --mcp-config <data>/mcp.json`. The settings file
keeps the status line of [usage.md](usage.md) when `usage.claude_limits` is on, and gains the hooks. The server
comes only with the hooks: when the settings file can't be given (Templum's program path can't be typed), neither is
`--mcp-config`, so no agent is connected with a dot that never moves.

```json
{
  "statusLine": { "type": "command", "command": "/opt/templum --claude-statusline /home/u/.local/share/templum", "padding": 0 },
  "hooks": {
    "SessionStart":     [{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }],
    "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }],
    "PermissionRequest":[{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }],
    "PostToolUse":      [{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }],
    "PostToolUseFailure":[{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }],
    "Stop":             [{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }],
    "StopFailure":      [{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }],
    "Notification":     [{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }],
    "SessionEnd":       [{ "hooks": [{ "type": "command", "command": "/opt/templum hook claude", "timeout": 5 }] }]
  }
}
```

The server's `timeout` (seven days, ms) replaces Claude Code's `MCP_TOOL_TIMEOUT`, so a question can wait for its
person ([ask-user.md](ask-user.md#when-the-client-stops-waiting)).

```json
{ "mcpServers": { "templum": { "type": "stdio", "command": "/opt/templum", "args": ["mcp"], "timeout": 604800000 } } }
```

**Codex:** five flags until batch 7, in single quotes (so not under `cmd.exe`; not under Windows PowerShell 5.1
either, whose legacy argument passing strips the `"` inside them, `args=["mcp"]` reaching Codex as `args=[mcp]`, while
pwsh 7 is fine; and not when a path has a `'`); `env_vars` because Codex hands a server only the variables it is told
to:

```
codex -c 'mcp_servers.templum.command="/opt/templum"' -c 'mcp_servers.templum.args=["mcp"]' -c 'mcp_servers.templum.env_vars=["TEMPLUM_BUS","TEMPLUM_TERMINAL"]' -c 'mcp_servers.templum.tool_timeout_sec=604800' -c 'notify=["/opt/templum","hook","codex"]'
```

From batch 7 the four `mcp_servers` flags stay, and `notify` gives way to one flag per hook of the table
[above](#hooks-and-agent-state), when `<data>/bin` is on the terminal's `PATH` ([Identity](#identity)); `Interrupt`'s
timeout is 3 s, the most Codex gives it (it says so at every start otherwise):

```
-c 'hooks.SessionStart=[{hooks=[{type="command",command="templum hook codex",timeout=5}]}]' … -c 'hooks.Stop=[…]' -c 'hooks.Interrupt=[{hooks=[{type="command",command="templum hook codex",timeout=3}]}]'
```

The hooks run `templum` from the terminal's `PATH` (`<data>/bin` comes first, [Identity](#identity)), not Templum's
path: Codex asks its person to trust a hook it hasn't seen, and again when its command changes, so the same words mean
one question, whatever Templum's version or where it runs from. Codex asks once, at its start and before its composer
("Hooks need review": Review hooks, **Trust all and continue**, Continue without trusting), and keeps that trust in its
own `config.toml` (`[hooks.state."/<session-flags>/config.toml:stop:0:0"]`, its `trusted_hash`), so the same flags are
trusted at every later start; Templum writes nothing there and never passes `--dangerously-bypass-hook-trust`, which
would run the person's own untrusted hooks too. Until the person trusts them, and in a Codex with its hooks feature off,
Templum hears nothing from it and reads its screen. With no `notify` flag, the person's own `notify` from
`~/.codex/config.toml` runs again.

**Gemini CLI:** it has no flag for MCP servers or hooks, and ignores a system settings file root doesn't own.
**Settings ▸ Connections ▸ Gemini CLI ▸ Install**, after a question, writes `<data>/gemini/gemini-extension.json` (the
server, `<data>/bin/templum mcp`, with a seven days' timeout, 604800000 ms) and `<data>/gemini/hooks/hooks.json` (the hooks,
`'<data>/bin/templum' hook gemini`; `& '…'` on Windows, where Gemini CLI runs hooks through PowerShell), and runs
`gemini extensions link <data>/gemini --consent`; **Remove** runs `gemini extensions uninstall templum`. Each start,
while `~/.gemini/extensions/templum` is there, Templum writes the two files again (an unchanged file is left alone):
one installed by an older Templum gets this one's.
`~/.gemini/settings.json` is never touched. Every Gemini CLI the person opens then has the server and the hooks, typed
by hand or not, whatever `[connections]` says; outside Templum the server shows as disconnected and the hooks do
nothing. Gemini CLI starts no stdio server in a folder it doesn't trust; it asks when it first opens one.

**Others** (Grok Build, Cursor CLI, Kimi Code, the person's own; from card 44 Copilot CLI, OpenCode, Kilo Code, Qwen
Code and Pi get theirs as [Every agent that can take it](#every-agent-that-can-take-it) says): **Settings ▸ Connections**
shows `<data>/bin/templum mcp`
with a Copy button. They know their block anyway: the server inherits `TEMPLUM_TERMINAL`.

**Templum's program** is the AppImage when `$APPIMAGE` is set (its mounted executable goes away when it quits),
otherwise the running executable, without the ` (deleted)` an upgrade leaves on Linux. The status line uses it too.

## Agents typed in a terminal

*Implemented (kanban card 42): [docs/plans/2026-10-18-agents-in-any-terminal.md](../plans/2026-10-18-agents-in-any-terminal.md).*

A person opens a terminal block and types `claude` or `codex`: that agent gets what one Templum opens gets, its MCP
server, its hooks (the status dot, delegating to it, `wait_for_idle`) and, for Claude Code, the status line of
[usage.md](usage.md). Nothing is written outside Templum's data folder, and an agent run anywhere else is untouched.

**How.** Each new terminal gets the arguments Templum would type after each agent's command, one per line, in a
variable, and the [shell integration](shell-integration.md) defines a function of the agent's name that puts them in
front of the person's:

| Variable | Holds | Set when |
|---|---|---|
| `TEMPLUM_CLAUDE_ARGS` | `--settings`, `<data>/claude-statusline.json`, then `--mcp-config=<data>/mcp.json` when the settings carry the hooks | `typed` would give Claude Code `--settings` |
| `TEMPLUM_CODEX_ARGS` | `-c` and each value of the [flags above](#what-templum-types-into-each-agent) (`mcp_servers.templum.command="<exe>"`, …, the hooks with `templum` on the terminal's PATH), without the single quotes | `typed` would give Codex its flags |

- Same rules as what is typed: a variable is empty without a data folder or a bus (connections), under WSL, when a
  path can't be typed safely, and Codex's when the shell is cmd or Windows PowerShell 5.1. Both are always set, empty
  when there is nothing to give: a Templum started from another's terminal must not pass that one's on (as
  `TEMPLUM_BUS`). The values are the raw words, not quoted for a shell; paths keep their spaces (macOS's
  `Application Support`), Windows' `\` written `/`. The two files are written when a terminal opens, as when an agent
  is opened (only when they changed).
- **The function** (bash, zsh, fish, PowerShell), `claude` while `TEMPLUM_CLAUDE_ARGS` is set when the shell starts,
  `codex` while `TEMPLUM_CODEX_ARGS` is: it runs the program the shell would run without it (`command claude`; in
  PowerShell the first command of that name that is an application or an external script, `claude.ps1` of npm
  included) with the variable's words first, then the person's. It reads the variable at each call: emptied
  (`unset TEMPLUM_CLAUDE_ARGS`), it adds nothing.
- **Nothing twice.** It adds nothing when the person's words already hold `--settings` or `--mcp-config` (alone or
  with `=…`) for Claude Code, or a word starting with `mcp_servers.templum.` for Codex: Templum types those itself into
  the agents it opens (the canvas menu, `spawn_agent`, a restore), which keep working as today, and a person's own
  `--settings` would replace Templum's anyway (Claude Code keeps the last one).
- **Why first, and `=`.** `--mcp-config` takes every word after it; `--mcp-config=<path>` takes one (checked
  2026-10-18 with Claude Code 2.1.286: `claude --mcp-config=a.json b.json -p` took `b.json` for its prompt). In front,
  subcommands keep working: `claude --settings … --mcp-config=… mcp list` (and `doctor`, `update`, `auth`), `codex -c
  … resume` (and `mcp list`, `login status`) ran as without them (Claude Code 2.1.286, Codex 0.154, by hand).

**What the person keeps:**

- Their MCP servers and hooks: `--mcp-config` adds Templum's server to theirs (no `--strict-mcp-config`), Claude Code
  merges hooks from every settings source and Codex its `-c` flags with its `config.toml` and `hooks.json`, as for the
  agents Templum opens. Codex asks once to trust Templum's hooks, the same question as for the ones Templum opens (same
  words, same trust).
- Their own function named `claude` or `codex` wins and runs as it is: zsh reads the `.zshrc` after Templum's
  `.zshenv`, so theirs replaces ours; bash, fish and PowerShell read the person's files first, and define ours only
  when there is no function of that name (fish counts an autoloaded `functions/claude.fish`; in PowerShell an alias
  of that name, which comes before any function, also wins). In bash and zsh their alias composes: `alias
  claude='claude --model opus'` runs our function with `--model opus` after Templum's words; fish's `alias` makes a
  function, which wins. Ours is defined with the `function` keyword, so
  an alias of the same name doesn't break its definition.
- `command claude`, a path (`~/.local/bin/claude`), `npx`, `sudo` or a script: the program alone, as outside Templum.
- The exit status: the agent's, in every shell (PowerShell's prompt is told it: a function's caller would see `$?`
  true), except in zsh after Ctrl+Z and `fg`: zsh moves the rest of a suspended function into a subshell, and `$?`
  is then 0 ([minors](../minors.md#connections)). In PowerShell the function passes `--` and `-p:x` on as
  PowerShell parses them (`--` dropped, `-p:x` reaching the agent as `x`; [minors](../minors.md#connections)).

**Where it doesn't reach** (the agent runs as outside Templum; the canvas menu still opens a connected one):

- cmd, Nushell, WSL, `shell.integration` off, and a shell started from the first one (`bash` typed at a prompt): no
  function.
- tmux and screen run their own shells, without Templum's integration; their `TEMPLUM_TERMINAL` would name the
  terminal that started their server, not the block the person sees. ssh: another computer.
- An agent's own shell tool runs a non-interactive shell (Claude Code's snapshot of the shell reads the person's own
  startup files, not Templum's), so a `claude -p` an agent runs is never taken for the block's agent.
- Gemini CLI: nothing new. Once its [extension](#what-templum-types-into-each-agent) is installed, every Gemini CLI
  has the server and the hooks, typed by hand or not. Copilot CLI, OpenCode, Kilo Code, Qwen Code and Pi: [Every
  agent that can take it](#every-agent-that-can-take-it) (card 44). Grok Build and the others: unchanged.

**The block's identity.** Nothing new: the 5-second snapshot finds a typed agent (`agent_of`), which gives its name in
`list_blocks`, Shift+Enter, the question before closing, delegating to it, and its resume on restore, typed with
Templum's flags ([persistence.md](persistence.md#agents)); its hooks move the dot from its first event. Until that
snapshot it isn't an agent to `delegate` ([minors](../minors.md#connections)). A Gemini CLI typed without
`--session-id` resumes its folder's latest session ([agents.md](agents.md#the-agents)), as today.

**The switch.** `[connections] enabled` covers typed agents too; Settings ▸ Connections names it **Agents in
Templum** ("Agents opened from the canvas or typed in a terminal get Templum's server and hooks: they read and type
into the blocks wired to them"), and `config.toml`'s comment says "The agents Templum opens, or that are typed in its
terminals, get its MCP server and hooks". Off, a new terminal's `TEMPLUM_CODEX_ARGS`
is empty, and its `TEMPLUM_CLAUDE_ARGS` holds only `--settings` and its file when the status line asks for it
(`usage.claude_limits`), as what is typed. A terminal keeps the variables it started with: the switch reaches new terminals, as
`shell.integration` does. No switch of its own: nothing outside Templum changes, and `command claude` runs the bare
agent. Card 44 made these words name every agent and the switch reach agents already running: [The switch reaches every
agent](#the-switch-reaches-every-agent).

**Calls made for card 42** (the person was away; each can be changed): on by default under the existing switch, no
switch of its own; functions in the shell integration, nothing on PATH and nothing in the agents' files; Templum's
words before the person's; the person's own function wins, an alias composes; nothing twice when `--settings`,
`--mcp-config` or `mcp_servers.templum.` is already there; cmd, Nushell, tmux and ssh not covered; Gemini CLI left to
its extension; a typed agent's identity left to the snapshot.

**Considered and left out:**

- **Registering Templum in the agents' own settings** (`claude mcp add -s user`, hooks in `~/.claude/settings.json`,
  `[mcp_servers.templum]` in `~/.codex/config.toml`), a no-op outside Templum: it edits the person's files (out of
  scope above); every Claude Code and Codex outside Templum shows a failed `templum` server (Claude Code's `${VAR}`
  expansion gives an empty string, it can't leave a server out) and runs `templum hook` at each event; and the entries
  break when Templum moves or is uninstalled.
- **A `claude` program first on PATH** (`<data>/bin/claude`): every process in the terminal finds it, an agent's own
  `claude -p` included, whose hooks would speak for the block's agent; a `.bashrc`'s usual `export
  PATH="$HOME/.local/bin:$PATH"` puts the real one first again (on the maintainer's machine, `~/.local/bin` and mise's
  folders, where `claude` and `codex` are, come before `<data>/bin` in a Templum terminal); on Windows `templum.exe` has no console, and a `.cmd`
  or `.ps1` stand-in loses quotes through cmd or meets PowerShell's execution policy.
- **Variables the agents read:** Claude Code has none for a settings file, an MCP file or a plugin (only `--settings`,
  `--mcp-config`, `--plugin-dir`); Codex reads none but `CODEX_HOME`, which, like `CLAUDE_CONFIG_DIR`, moves the
  person's login and history too.
- **Claude Code's `--plugin-dir`** with a plugin of the server and the hooks: its tools would be named
  `mcp__plugin_…`, not `mcp__templum__…` as the instructions and the permission checks name them, and a plugin can't
  carry the status line.
- **Seeing the agent once it runs:** Templum already does (identity, state from the screen); a running agent can't be
  given an MCP server or hooks, and restarting it with them would cut its turn.

## Every agent that can take it

*Implemented (kanban card 44): [docs/plans/2026-10-18-agent-parity.md](../plans/2026-10-18-agent-parity.md). The
audit it closes: [agent-parity.md](agent-parity.md).*

Card 42 gave a `claude` or `codex` typed by hand what the ones Templum opens get. Card 44 gives the same to every
other agent whose own command line, environment or plugins take it **for one session**, opened from the menu,
`spawn_agent`, a restore, or typed by hand, with the mechanisms card 42 built (`typed`, `Workspace::agent_env`, the
shells' functions). Nothing is written outside Templum's data folder, as for Claude Code and Codex.

| Agent | Templum's server | Hooks (the status dot, a task's end) | Typed by hand | Checked |
|---|---|---|---|---|
| GitHub Copilot CLI | `--additional-mcp-config=@<data>/copilot/mcp.json` | a plugin, `--plugin-dir=<data>/copilot/plugin` | `copilot` | 1.0.95's `--help`, its SDK's types, a run by hand with a sign-in (its hooks) |
| OpenCode | `OPENCODE_CONFIG_CONTENT`, merged with the person's config | a JS plugin named in the same variable | `opencode` | 1.19.0, `opencode serve` with a probe plugin, `opencode run` with Templum's plugin by hand; not its TUI |
| Kilo Code | `KILO_CONFIG_CONTENT` (an OpenCode fork) | the same plugin | `kilo` | from its source (7.8.x); not installed here |
| Qwen Code | `--mcp-config=<data>/qwen/mcp.json` | its system settings file moved to `<data>/qwen/settings.json` (`QWEN_CODE_SYSTEM_SETTINGS_PATH`) | `qwen` | from its docs and source (0.25.2); not installed here |
| Pi | an extension, `-e <data>/pi/templum.ts`, that registers the server when Pi has MCP (0.99 and later) | the same extension's events | `pi` | 0.85.1 (no MCP: its events only; a probe extension by hand, without a model); the server from 1.3.0's docs |

Gemini CLI keeps its extension; it is asked for once ([below](#gemini-cli-without-its-extension)). The others stay as
they are: [agent-parity.md](agent-parity.md#after-card-44) says why for each.

**Common rules.** The same as for Claude Code and Codex: only with `[connections]` on, a bus, a data folder, not
under WSL, and every path typeable (`typed_path`); otherwise the agent starts without them. Copilot's, Qwen's and Pi's
words are paths and plain flags, so they go into every shell, cmd and Windows PowerShell 5.1 included. A file is
written only when it changed, when an agent is opened and when a terminal opens. The program these files run is
`<data>/bin/templum` (`templum.exe` on Windows), the link Templum points at itself at each start, as Gemini CLI's
extension names it: the same words whatever Templum's version or where it runs from (an agent that asks to trust a
hook asks once), so they need it there (`<data>/bin` on the terminal's `PATH`, as Codex's hooks do); a path holding
a `'` gives no connections.

### GitHub Copilot CLI

It takes everything for one session (checked with `copilot --help` 1.0.95 and its bundled SDK's `types.d.ts`). The
Copilot CLI Templum opens (the menu, `spawn_agent`, a restore) gets the words below (`templum_usage::copilot::args`,
`Workspace::typed`), only with `<data>/bin` on the terminal's `PATH`.

**Words** after its command, opened or typed (`=` keeps each one word; the last names Templum's folder, which the shell
function looks for):

```
copilot --allow-all-mcp-server-instructions --additional-mcp-config=@<data>/copilot/mcp.json --plugin-dir=<data>/copilot/plugin
```

- `--allow-all-mcp-server-instructions`: Copilot reads a server's instructions up front only for servers on its own
  allowlist (others "on demand"); this flag lets Templum's in, and also the person's own servers', for that session.
  It has no allowlist setting a session can add to.
- `<data>/copilot/mcp.json`: Copilot hands a local server only `PATH` and the variables its `env` names, expanding
  `${VAR}` from its own environment. The tool timeout is `timeout` in ms (30 s unless set):

  ```json
  { "mcpServers": { "templum": { "type": "local", "command": "<data>/bin/templum", "args": ["mcp"],
      "env": { "TEMPLUM_BUS": "${TEMPLUM_BUS}", "TEMPLUM_TERMINAL": "${TEMPLUM_TERMINAL}" },
      "tools": ["*"], "timeout": 604800000 } } }
  ```

  Its tools are named `templum-<tool>` in Copilot. Copilot asks the person before every MCP tool call; Templum passes
  no `--allow-tool`, as it pre-allows nothing for Claude Code.
- `<data>/copilot/plugin`: a plugin of hooks only, `plugin.json` (`{ "name": "templum" }`) and `hooks.json`. A hook's
  payload doesn't name its event, so each hook's command does; `bash` on Unix and macOS, `powershell` on Windows, as
  Gemini CLI's hooks are written (`templum_usage::hook_command`). A reporting hook must print nothing: Copilot reads a JSON
  object from a hook's stdout, and a failing `preToolUse` hook denies the tool, so Templum hooks no `preToolUse`.

  ```json
  { "version": 1, "hooks": {
      "agentStop": [{ "type": "command", "bash": "'<data>/bin/templum' hook copilot agentStop",
                      "powershell": "& '<data>/bin/templum' hook copilot agentStop", "timeoutSec": 5 }],
      "…": "the same for each hook of the table below" } }
  ```

| Hook | Event |
|---|---|
| `sessionStart` | started; nothing when it has an `initialPrompt`: Copilot starts its session with the first prompt (typed, or `-p`), after that prompt's `userPromptSubmitted`, and the turn is under way |
| `userPromptSubmitted` | took a prompt: working; once a turn takes it (`at_enter` false: a prompt typed during a turn not checked) |
| `postToolUse`, `postToolUseFailure` | working (again, after a permission) |
| `notification`, `notification_type` `permission_prompt` | waiting for the person, with its `message` ("Run command: touch x.txt"). Not `permissionRequest`: it comes before Copilot's own rules, for calls they allow too (an `echo`) |
| `notification`, `elicitation_dialog` | waiting for the person (its own `ask_user`), with its `message` |
| `agentStop` | done, with its last answer: the payload has none, only `transcriptPath` (`events.jsonl`), whose last `assistant.message` after the last `user.message` `templum hook copilot` reads (its last 256 KB; Copilot writes the file in batches, so the hook waits up to 3 seconds for it; a turn that ends on an empty message has none) |
| `errorOccurred` with `recoverable` false | done, without an answer |
| `sessionEnd` | ended |

Esc, and a permission the person refuses, fire no `agentStop`: the person's keys and a still screen end Working after
5 seconds, as an Esc while Claude Code writes does ([An agent's state](#an-agents-state)).

Checked by hand on 2026-10-18 with Copilot CLI 1.0.95, signed in, in a terminal of Templum (a probe plugin, `-p`
and the TUI driven through a pseudo-terminal): its hooks see `TEMPLUM_BUS` and `TEMPLUM_TERMINAL`, so `hooks.json`
needs no `env`; the payloads are camelCase but `notification`'s (`notification_type`, `message`, `title`); a refused
permission fires no hook after `notification`; the last `assistant.message` of `events.jsonl` is the turn's answer;
`--plugin-dir` asks nothing at start (Copilot's own question about trusting a folder is unrelated). Not seen:
`elicitation_dialog`, `errorOccurred`, a prompt typed during a turn.

**Its session.** `--session-id <uuid>` starts a session with that id, or resumes it when there is one: the Copilot
Templum opens gets one (`new_id`), restored with `copilot --session-id <id>` (`resume_id`), as Pi's
([agents.md](agents.md#the-agents)). `--continue` isn't used: it falls back to the latest session of any folder. A
Copilot typed without `--session-id` comes back as a new session.

**Not given:** a status line (Copilot takes one only from its `settings.json`; [usage.md](usage.md#later-batches));
`--allow-tool`.

### OpenCode and Kilo Code

Neither has a flag for a server or a plugin, but each merges a variable of JSON into its config, last and deeply
(`plugin` lists add up), and asks nothing about a server or a plugin it finds there (checked with OpenCode 1.19.0
under `opencode serve`; Kilo Code from its source):

```json
{ "mcp": { "templum": { "type": "local", "command": ["<data>/bin/templum", "mcp"], "timeout": 604800000 } },
  "plugin": ["file:///home/u/.local/share/templum/opencode/templum.js"] }
```

- **The variable:** `OPENCODE_CONFIG_CONTENT` for OpenCode, `KILO_CONFIG_CONTENT` for Kilo Code, the same JSON. Their
  local servers inherit the environment (`TEMPLUM_BUS` and `TEMPLUM_TERMINAL` reach `templum mcp`). The server's
  `timeout` covers its start, its tool list and every call (60 s without it, the MCP SDK's): seven days.
- **The blocks Templum opens** for one of them (the menu, `spawn_agent`, a restore) get the variable in their shell's
  environment; Templum types the command alone. A person's own `OPENCODE_CONFIG_CONTENT` (in Templum's environment)
  wins: Templum then adds nothing.
- **The plugin**, `<data>/opencode/templum.js` (`templum_usage::opencode`, written when an agent or a terminal opens),
  plain JavaScript (no `package.json`, no install), hands each event of the table below as one JSON line to
  `<data>/bin/templum hook opencode` and waits for it (`spawnSync` with a 3-second limit, so events stay in order; the
  hook exits within a second when Templum is gone, and a missing program throws nothing). Only those events: OpenCode
  streams many more (`message.part.updated` at each piece of text), each of which would start a process. It reports
  only the main session's turns: a subagent's session (one with a `parentID`) is never this block's turn. Its dialogs
  (`permission.*`, `question.*`) are told all the same: OpenCode holds the whole turn until the person answers them. It sets `TEMPLUM_HOOKED` to its process id when it is empty, and says
  nothing when it finds another process's: an `opencode run` an OpenCode in Templum starts from its shell tool
  inherits the variable, and must not speak for the block. Every Templum terminal gets an empty `TEMPLUM_HOOKED`.

| What the plugin hears | Event |
|---|---|
| its `chat.message` hook (a prompt taken) | took a prompt: working (`at_enter` false) |
| `session.status` `busy` | working (it comes again at each step of a turn) |
| `session.idle` | done, with the session's last assistant text, which the plugin reads with `client.session.messages` (its answer's `data`) |
| `session.error` | done, without an answer |
| `permission.asked` | waiting for the person: "Allow *permission*?" |
| `question.asked` | waiting for the person, with its question |
| `permission.replied`, `question.replied`, `question.rejected` | working |

Checked by hand on 2026-10-18 with OpenCode 1.19.0 (`opencode run` on a free model, the plugin with a fake
`templum`): `chat.message` once per prompt, then `session.status` busy at each step, idle, and `session.idle` with
the answer; a subagent's session (the task tool) said nothing; the agent's shell tool saw `TEMPLUM_HOOKED` set to
OpenCode's process id, so an `opencode run` it starts stays quiet; a plugin path with a space, `%20` in its URL,
loads; `opencode mcp list` showed `templum` connected beside the person's own servers. Not checked by hand: its
permission and question dialogs, Esc, and the TUI ([minors](../minors.md#connections)); Kilo Code (not installed).

Ended: the snapshot (it has no event for quitting), as Codex's. **Its session:** `-s` takes only an existing id, and a
new one's can't be chosen, so OpenCode and Kilo Code keep coming back on the folder's latest (`--continue`).

### Qwen Code

From Qwen Code's docs and source at 0.25.2, and its current docs for the hooks' `timeout` (seconds). Not checked by
hand: not installed here ([minors](../minors.md#connections)). `templum_usage::qwen` writes both files; the hooks
reach `hooks::qwen`, Claude Code's parser but for the prompt, taken once a turn takes it.

- **The server:** `--mcp-config=<data>/qwen/mcp.json`, servers that are never asked about and load in its safe mode.
  Its stdio servers inherit the environment. A tool call's `timeout` is in ms (10 minutes unless set):

  ```json
  { "mcpServers": { "templum": { "command": "<data>/bin/templum", "args": ["mcp"], "timeout": 604800000 } } }
  ```

- **The hooks:** Qwen Code takes Claude Code's hooks (same names, `last_assistant_message`, `transcript_path`), but
  only from settings files; it has no `--settings`. Its system settings file can be moved with
  `QWEN_CODE_SYSTEM_SETTINGS_PATH` (no check of its owner, unlike Gemini CLI's), and system hooks run with the
  person's, after them. So the Qwen Code Templum opens gets that variable naming `<data>/qwen/settings.json`, which
  holds only `hooks`. **Not when Qwen's own system file exists** (`/etc/qwen-code/settings.json`,
  `/Library/Application Support/QwenCode/settings.json`, `C:\ProgramData\qwen-code\settings.json`): Templum's would
  hide it, so Templum gives the server only. Each hook's command is `'<data>/bin/templum' hook qwen` (`& '…'` on
  Windows, as Gemini CLI, which it forks).

| Hook | Event |
|---|---|
| `SessionStart` | started; nothing when its `source` is `compact` |
| `UserPromptSubmit` | took a prompt: working (`at_enter` false: not checked) |
| `PermissionRequest` | waiting for the person, "Allow *tool*?" |
| `PostToolUse` | working |
| `Stop` | done, with `last_assistant_message` and `transcript_path` |
| `StopFailure` | done, without an answer |
| `Notification` `permission_prompt` | waiting for the person (the dialog `PermissionRequest` said); `idle_prompt` done without an answer, as Claude Code's |
| `SessionEnd` | ended |

- **The blocks Templum opens** get the words (`Workspace::typed`) and the variable (`Workspace::agent_vars`), as
  OpenCode's do. Programs Qwen Code starts inherit
  the variable (it passes it on by design), so a `qwen -p` its shell tool runs would speak for the block
  ([minors](../minors.md#connections)).
- **Its session:** unchanged, by its files (`qwen --resume <id>`).

### Pi

Pi has no flag for a server, but takes an extension for one session (`-e <path>`, TypeScript loaded without a build).
Templum's, `<data>/pi/templum.ts`:

- registers Templum's server with `pi.registerMcpServer("templum", { command, args: ["mcp"], timeout: 604800 })`
  (seconds) when Pi has it: Pi 0.99 and later have MCP; 0.85.1, installed here, has none, so it gets the events only;
- hands Pi's events to `<data>/bin/templum hook pi`, one JSON line each, as OpenCode's plugin does, with the same
  `TEMPLUM_HOOKED` rule (a `pi -p` its shell tool runs says nothing):

| Pi's event | Event |
|---|---|
| `session_start` | started |
| `agent_start` | took a prompt: working (`at_enter` false) |
| `tool_result` | working |
| `agent_settled` | done, with the last assistant text of the run's `agent_end` (from its messages). Not `agent_end` itself: Pi may still retry, compact and retry, or take a queued message after it; `agent_settled` (Pi 0.80.4 and later) says it won't go on by itself |
| `ui_prompt_start` | waiting for the person, with its title (an extension's dialog: Pi has no permission prompts; Pi 0.84.4 and later) |
| `ui_prompt_end` | working |
| `session_shutdown` | ended, when its `reason` is `quit` (`/new`, `/resume`, a fork start another session at once) |

The words after its command are `-e` and the file (`templum_usage::pi::args`), opened or typed, but never before one
of Pi's commands (`install`, `remove`, `uninstall`, `update`, `list`, `config`, `auth`): Pi reads one only as its
first word, and `pi -e <file> list` takes `list` for a prompt (checked with 0.85.1). Its session is unchanged
(`--session-id`).

Checked by hand on 2026-10-18 with Pi 0.85.1 (a probe extension, `pi -ne -e <probe> -p "say hi"`; Pi had no model it
could use here, so each run ended in an error): the events come in the order `session_start`, `input`,
`before_agent_start`, `agent_start`, `turn_start`, `turn_end`, `agent_end`, `agent_settled`, `session_shutdown`;
`pi.registerMcpServer` is undefined; `-e` asked nothing. Not checked by hand: an answer's text, a dialog, a nested
`pi -p`, and Pi with MCP ([minors](../minors.md#connections)).

### Typed by hand: the shells

Each new terminal also gets, always set and empty when there is nothing to give (the same rules as `typed`):

| Variable | Holds |
|---|---|
| `TEMPLUM_COPILOT_ARGS` | Copilot's three words above, one per line |
| `TEMPLUM_OPENCODE_CONFIG`, `TEMPLUM_KILO_CONFIG` | the JSON for `OPENCODE_CONFIG_CONTENT`, `KILO_CONFIG_CONTENT` |
| `TEMPLUM_QWEN_ARGS`, `TEMPLUM_QWEN_SETTINGS` | `--mcp-config=<data>/qwen/mcp.json`; the settings file's path, empty when Qwen's own system file exists |
| `TEMPLUM_PI_ARGS` | `-e` and `<data>/pi/templum.ts` |
| `TEMPLUM_HOOKED` | always empty ([above](#opencode-and-kilo-code)) |

The integration defines `copilot`, `opencode`, `kilo`, `qwen` and `pi` as it defines `claude` and `codex` (only while
its variable is set when the shell starts, never over the person's own function, the program the shell would run,
the exit status kept, what is piped in passed on), with one helper per shell for all of them:

- **Words first**, then the person's, as for `claude`.
- **Nothing twice:** no words when one of the person's words is the variable's last word, a path in Templum's folder,
  which only Templum types (into the ones it opens). A person's own `--plugin-dir`, `--additional-mcp-config` or `-e`
  adds to Templum's: Copilot CLI and Pi take several. Qwen Code takes one `--mcp-config` (a second is a fatal error),
  so a person's own `--mcp-config`, alone or with `=…`, means no words for it, as Claude Code's rule is.
- **Pi never before one of its commands** (`pi list`, `pi install …`): [above](#pi).
- **Qwen Code never before one of its commands:** no words when the person's first word is one (`agents`, `auth`,
  `batch`, `board`, `channel`, `extensions`, `hooks`, `mcp`, `review`, `sandbox`, `serve`, `sessions`, `update`,
  from `qwen --help` 0.25.2), which refuse `--mcp-config` (`qwen mcp list` printed `Unknown arguments: mcp-config,
  mcpConfig` and exited 1). Any other plain word is a prompt (`qwen [query..]`), so `qwen hi` gets it.
- **Copilot CLI only when it starts a session:** its words go only when the person's first word is none or an
  option (`copilot`, `copilot -p …`, `copilot --model …`), never before a subcommand (`copilot plugin list`, `copilot
  login`), nor when one of the person's words is `-h`, `--help`, `-v` or `--version`, which need none. AWS's Copilot
  CLI is also `copilot`, and refuses Templum's words (`✘ unknown flag: --allow-all-mcp-server-instructions`, exit 1,
  checked with v1.34.1): its subcommands, help and version run as they are (`copilot app deploy`, `copilot --help`),
  but a bare `copilot` still gets them ([minors](../minors.md#connections)).
- **A variable for that one command:** `OPENCODE_CONFIG_CONTENT`, `KILO_CONFIG_CONTENT` or
  `QWEN_CODE_SYSTEM_SETTINGS_PATH` is set for the agent only (bash and zsh `local -x`, fish `set -lx` outside any
  block, PowerShell set and removed after), unless the shell's environment already has a non-empty one (the block
  Templum opened, or the person's own). The shell itself never gets it.

### The switch reaches every agent

`[connections] enabled` off reaches every agent at once, Gemini CLI's extension included: the bus answers a
`templum mcp` and a `templum hook` from any terminal as from a terminal it doesn't have. `initialize` says "Connections
are off in Templum's settings: Templum's tools won't work here."; every tool call fails with "Connections are off:
turn on Agents in Templum (Settings ▸ Connections) to use Templum's tools."; events are dropped, and every agent's
state is read from its screen again (`speaks` false). The command line's verbs still work. Calls already waiting
stay as they are, and end as they do today (the agent's screen going still, the person's answer, their timeout).
Turned on again, the next call works; an agent started while it was off has no flags until its next start.

Settings ▸ Connections names it **Agents in Templum**, "Agents opened from the canvas or typed in a terminal get
Templum's server and hooks: they read and type into the blocks wired to them"; `config.toml`'s comment: "The agents
Templum opens, or that are typed in its terminals, get its MCP server and hooks: they can read and type into the blocks
wired to them (docs/specs/connections.md)." **Other agents** names the ones that take a server only from their own
files: "Grok Build, Cursor CLI, Kimi Code or your own: add this command to their MCP servers…".

### Gemini CLI without its extension

A Gemini CLI opened from the menu or by `spawn_agent` while Templum's extension isn't installed (and connections are
on, with a data folder) shows a notification, once per run of Templum: "Gemini CLI has no Templum tools or status dot"
/ "Click to install Templum's extension: every Gemini CLI you open after gets them." Clicking it asks Install's
question (Settings ▸ Connections' `INSTALL`), then installs. The Gemini CLI already open keeps running without it. A
restore never shows it. Not remembered across runs: once per run is the whole cost of ignoring it
([minors](../minors.md#connections)).

### A dialog Templum doesn't hear

The audit's gap 3, "an agent without hooks looks ready during a permission prompt", closes for each agent above
through its own hook (Copilot's `permission_prompt`, OpenCode's and Kilo's `permission.asked` and `question.asked`,
Qwen Code's `PermissionRequest`; Pi has no permission prompts, and its extensions' dialogs are `ui_prompt_start`).
For the agents that stay without hooks it stays a documented gap ([agent-parity.md](agent-parity.md#after-card-44)):
reading a dialog off each agent's screen is guesswork, and a wrong yellow is worse than none.

**Calls made for card 44** (the person was away; each can be changed): Copilot gets the instructions flag but no
`--allow-tool`; its last answer from the transcript, not its screen; OpenCode, Kilo Code and Qwen Code get their
variable in the blocks Templum opens for them, and for that one command when typed; `TEMPLUM_HOOKED` keeps a nested
OpenCode, Kilo Code or Pi quiet, while a nested Qwen Code stays a minor; Qwen Code gets no hooks when its own system file
exists, and none when the person gives their own `--mcp-config`; Copilot CLI's words never before a subcommand nor with
`--help` or `--version` (AWS's `copilot`); the files run `<data>/bin/templum`, not Templum's program path; Pi's
extension gives the server only where Pi has MCP; the switch off cuts agents already running and leaves the calls that
wait; Gemini CLI's notice once per run, never on restore; a generic dialog reader left out. Made while implementing it: Pi's turn ends at
`agent_settled`, not `agent_end`; Pi's words never before one of its commands; Copilot's answer waited for up to 3
seconds after its `agentStop` (it writes its transcript in batches); a Copilot `sessionStart` with an `initialPrompt`
is no event; OpenCode's question shows its text.

**Considered and left out:**

- **Templum's own environment variable for every terminal** (`OPENCODE_CONFIG_CONTENT` in each terminal): every
  `opencode` any program runs there would get the plugin, card 42's reason for functions.
- **Copilot's plugin carrying the server too** (its `.mcp.json`): one flag fewer, but whether a plugin's server
  expands `${TEMPLUM_BUS}` isn't documented, and `--additional-mcp-config` comes first among Copilot's sources.
- **Qwen Code's extension** (`qwen extensions link`, as Gemini CLI's Install): it serves every Qwen Code once linked,
  where a variable serves one session.
- **`OPENCODE_CONFIG_DIR`:** OpenCode installs packages into that folder.
- **A Pi extension that declares each of Templum's 26 tools with `registerTool`:** a second copy of the tools'
  schemas, for a Pi older than its own MCP.
- **Progress notifications** from Templum's server, which reset OpenCode's call timeout: a bus-wide change for one
  client, where its `timeout` does it.
- **Telling an agent from another program of its name** (by its package's path, or its `--version`): only Copilot's
  name is known to be shared (AWS's Copilot CLI), and its rules for subcommands, help and version cover all of it but
  a bare `copilot`; another `pi` would get `-e <file>` (`command pi` runs it bare).
- **Answering the calls that wait** when the switch goes off: they already end through the screen, the person or
  their timeout.

## The `templum` command line

The same program, without a window, when its first argument is one of these verbs. Each verb is one tool call: it
prints the result's text, or with `--json` the whole result; a refusal goes to stderr with exit code 1; outside
Templum or with Templum gone, exit code 2 as [above](#the-mcp-server).

| Command | Tool | Batch |
|---|---|---|
| `templum whoami` | `whoami` | 1 |
| `templum blocks` | `list_blocks` | 1 |
| `templum read <block> [-n <lines>] [--last]` | `read_terminal` | 1 |
| `templum send <block> <text> [--enter]` | `send_to_terminal` | 1 |
| `templum notify <text>` | `notify` | 1 |
| `templum mcp` | the MCP bridge | 1 |
| `templum hook claude` / `templum hook codex <json>` | an agent's hook (`templum hook codex` reads its JSON on stdin from batch 7; an argument, an older Templum's `notify`, is ignored, and stdin, then Codex's terminal, isn't read); at a session's start it prints the person's instructions ([Hooks and agent state](#hooks-and-agent-state)) | 1, 7, card 148 |
| `templum hook gemini` | Gemini CLI's hooks (JSON on stdin) | 5 |
| `templum hook copilot <event>` | Copilot CLI's hooks (JSON on stdin, the event named by the hook's command, as Copilot names its hook: `agentStop`); `agentStop`'s answer read from the end of its transcript | card 44 |
| `templum hook opencode` / `templum hook qwen` / `templum hook pi` | the OpenCode and Kilo Code plugin's events, Qwen Code's hooks, Pi's extension's events (JSON on stdin) | card 44 |
| `templum connect <block>` | `connect` | 3 |
| `templum spawn [<agent>] [--fork] [--role <role>] [--folder <f>] [--worktree <name>] [--prompt <text>] [--model <m>] [--effort <e>]` | `spawn_agent` | 3, 6, card 35 |
| `templum delegate <block> <task> [--wait]` | `delegate` (from batch 8 it prints the task's id; a script's terminal gets no notice: `templum wait` waits for its end) | 3, 8 |
| `templum reply <block> <text>\|-`, or `templum reply -` (`-` reads stdin; with no block it answers the caller's task) | `reply` | 3, card 69 |
| `templum wait <block> [--for <event>] [--match <pattern>] [--lines <n>] [--timeout <s>]` | `wait_for_idle`, `wait_for` | 3, 5, card 35 |
| `templum ask <question> [--option <o>]…` | `ask_user` | 3 |
| `templum plan <file> [--title <t>]` (`-` reads stdin; a rejection exits 1) | `propose_plan` | card 35 |
| `templum kanban list [<board>] [--card <card>] \| add <title> [--column <c>] [--description <d>] [--board <b>] [--priority <p>] [--for <who>] [--checklist <text>] \| edit <card> [--priority <p>] [--for <who>] [--add <text>] [--check <item>]… [--uncheck <item>]… \| move <card> <column> \| comment <card> <text> \| edit-comment <card> <comment> <text> \| delete-comment <card> <comment> \| claim <card>` | the kanban tools (`edit` and the new options: card 40; `edit-comment` and `delete-comment`: card 77) | 4, card 40, card 77 |
| `templum note read\|append <block> [text]` | `read_note`, `append_note` | 4 |
| `templum note replace <block> <text> [--old <text>]` | `replace_note` | card 35 |
| `templum checklist read <block> \| add <block> <text> \| check\|uncheck <block> <item>` | `read_checklist`, `add_items`, `check_item` | — |
| `templum checklist edit <block> <item> <text> \| delete <block> <item>` | `edit_item`, `delete_item` | card 35 |
| `templum new note [<text>] \| new checklist <name> [--items <text>] \| new terminal [--folder <f>] [--command <c>]` | `create_block` | card 35 |
| `templum open <path>[:<line>]` | `open_file` | 4 |
| `templum keys <block> <key>…` | `send_keys` | 6 |
| `templum interrupt <block>` | `interrupt` | 6 |
| `templum close <block>` | `close_block` | 6 |
| `templum stop <block>` | `stop_agent` | card 35 |
| `templum worktree remove <name> [--folder <f>] [--discard] \| merge <name> [--folder <f>]` | `remove_worktree`, `merge_worktree` | card 35 |
| `templum schedule <block> <text> [--at HH:MM] [--after <min>] [--every <min>]`, `templum unschedule <id>` | `schedule`, `cancel_schedule` | card 35 |
| `templum wire <a> <b>` | wires two blocks of the workspace of the terminal it runs in, a step of Ctrl+Z; not a tool (an agent's MCP server never lists it); it asks no one, so an agent's shell tool can wire its own block (the person's choice) | 5 |

A verb stays connected until its answer: stopped with Ctrl+C, what it waited for (a request in the inbox) is withdrawn.
`templum open` and `templum spawn --folder` read a relative path from the folder they run in (an agent's shell tool may
not be in the agent's); `templum open` takes `<path>:<line>:<column>` as compilers and grep print it, the column
ignored.

Any other first argument opens Templum, as today. On Windows the release program has no console, so the verbs'
output reaches a pipe (`templum blocks | more`) but not a console directly.

## Config

```toml
[connections]
## The agents Templum opens, or that are typed in its terminals, get its MCP server and
## hooks: they can read and type into the blocks wired to them (docs/specs/connections.md).
# enabled = true
## How long an agent's question waits for your answer, in minutes; 0 waits until you answer.
# question_timeout = 0
## A short sound when an agent asks you something.
# sound = true
```

Off, nothing is typed into agents, and the bus answers no agent's server or hooks ([The switch reaches every
agent](#the-switch-reaches-every-agent)); it still serves the command line. **Settings ▸ Connections** (batch 5) has
the switch, Questions wait and Sound ([ask-user.md](ask-user.md#settings)), the server's command for other agents with
a Copy button, and Gemini CLI's Install and Remove.

## Files

- **Workspace files** ([persistence.md](persistence.md#the-file)): every item gains its block id, and the canvas its
  wires, by block id. A wire whose block didn't come back, one of a block to itself, and a second one between the
  same two blocks are dropped. An older Templum ignores both. A label is trimmed and cut at 60 characters; a blank
  one is none.

  ```json
  { "version": 1,
    "items": [{ "kind": "terminal", "uid": "b7k2m", "rect": {…}, … }, { "kind": "note", "uid": "4qm2a", … }],
    "wires": [{ "a": "b7k2m", "b": "4qm2a" }, { "a": "b7k2m", "b": "t4q8p", "label": "dev server" }] }
  ```

- **`boards.json`** (batch 4, [kanban.md](kanban.md#files)): a card gains `assignee` (a block id) and `comments`, each
  `{ "by": "<block id or 'you'>", "text": "…", "at": <unix seconds> }`. An older file loads with none; an older
  Templum ignores them. The card shows the assignee's block title and the comment count; its dialog lists the
  comments, adds the person's, and frees the assignee (✕), each a step of the kanban's Ctrl+Z; `claim_card` refuses a
  card another block holds until the person frees it. Card 40 adds `priority`, `checklist` and an agent assignee
  (`"agent:codex"`) to a card ([kanban.md](kanban.md#files-1)).
- **Terminal items** (batch 6) also keep `opened_by` (the block id of the agent that spawned it), `model` and
  `effort` (what `spawn_agent` was given). A pasted block has none; an older file loads with none; an older Templum
  ignores them.
- **Tasks** (batch 8) aren't saved: a restart drops the tasks handed over, with no notice, and their ids count from
  `t1` again.
- **`<data>/mcp.json`** and the hooks in `<data>/claude-statusline.json` are written when an agent is opened, only
  when they changed, atomically.
- **From card 44:** `<data>/copilot/mcp.json` and `<data>/copilot/plugin/` (`plugin.json`, `hooks.json`),
  `<data>/opencode/templum.js` (OpenCode's and Kilo Code's plugin), `<data>/qwen/mcp.json` and
  `<data>/qwen/settings.json`, `<data>/pi/templum.ts`: written the same way, when an agent is opened and when a
  terminal opens.

## Architecture

| File | Change | Batch |
|---|---|---|
| `templum-mcp` (new crate, no UI) | `rpc.rs` (the line protocol), `tools.rs` (the tools' schemas, the typed calls, replies), `hooks.rs` (each agent's events), `bus.rs` (the socket: listener, a reader and a writer thread per connection, the client), `cli.rs` (the verbs, the MCP bridge), `lib.rs` (the `-c` flags Codex is given) | 1 |
| `templum-terminal/src/terminal.rs` | `SpawnOptions.env`; `Terminal::lines` and `last_command`, read from the grid | 1 |
| `templum-usage/src/claude.rs` | the settings file Claude Code is given holds the status line, the hooks, or both; `mcp.json`, the MCP config it is given | 1 |
| `templum/src/main.rs` | the verbs run before any window | 1 |
| `templum/src/workspace/bus.rs` (new) | the bus's messages answered on the main thread: the caller, the tools, the events, the requests that wait | 1, 3–5 |
| `templum/src/workspace/usage.rs` | what `typed()` adds for Claude Code and Codex | 1 |
| `templum/src/session.rs`, `workspace/persist.rs`, `workspace/blocks.rs`, `workspace/spaces.rs` | block ids: given, saved, restored, moved | 1 |
| `templum/src/block.rs` | the agent's state in the status dot; the notice of a waiting request | 1, 3 |
| `templum/src/config.rs` | `[connections]` | 1 |
| `templum-canvas/src/wire.rs` (new) | `Wire`; the curve between two rects, a point on it, hitting it | 2 |
| `templum/src/workspace/wires.rs` (new), `templum/src/wires.rs` (new) | wiring, unwiring, selecting, undo, labels (`Change::Label`); painting wires, the dot and the labels | 2, 5 |
| `templum/src/workspace/waits.rs`, `delegate.rs`, `spawn.rs` (new) | the requests that wait and the inbox's answers; tasks handed over, `reply`, `wait_for_idle`; `spawn_agent` | 3 |
| `templum/src/workspace/content.rs` (new) | the kanban, note and file tools | 4 |
| `templum/src/balloon.rs`, `inbox.rs` (new) | the request's dialog; the inbox and the notice ([ask-inbox.md](ask-inbox.md)) | 3 |
| `templum-git/src/repo.rs` | `Repo::add_worktree` | 3 |
| `templum/src/toolbar.rs`, `menu/`, `workspace/history.rs`, `workspace/input.rs`, `workspace/render.rs` | the Wire tool, the menus, the undo steps, the pointer | 2 |
| `templum/src/board.rs`, `kanban.rs` | a card's assignee and comments, saved in `boards.json`; the card's face and the open card show them | 4 |
| `templum-usage/src/gemini.rs` (new) | the Gemini CLI extension | 5 |
| `templum/src/workspace/activity.rs` (new) | an agent's state from its screen | 5 |
| `templum/src/settings/connections.rs` (new), `workspace/usage.rs` | Settings ▸ Connections; running Gemini CLI | 5 |
| `templum-terminal/src/input.rs`, `terminal.rs`, `agent.rs`; `templum/src/agents.rs` | named keys and `Terminal::press`; each agent's model and effort flags (`launch_args`) | 6 |
| `templum/src/workspace/delegate.rs`, `spawn.rs`, `bus.rs`, `mod.rs`, `blocks.rs`, `persist.rs`, `selection.rs`; `session.rs`, `block.rs` | `send_keys`, `interrupt`, `close_block`; `opened_by`, `model` and `effort` saved and resumed; "answered by" | 6 |
| `templum-mcp/src/hooks.rs`, `lib.rs`, `cli.rs`; `templum-terminal/src/terminal.rs`; `templum/src/workspace/bus.rs`, `delegate.rs`, `activity.rs`, `spawn.rs`, `waits.rs`, `input.rs`, `mod.rs` | Codex's hooks; a prompt taken (`Submitted`) confirms a task and decides which turn answers it; the first line as a paste of its own; a prompt waits for its agent's input (`Modes::typing`); seems stuck; dialogs as drawn | 7 |
| `templum-mcp/src/hooks.rs`, `tools.rs`; `templum/src/workspace/delegate.rs`, `waits.rs`, `bus.rs`, `activity.rs`, `spawn.rs`, `blocks.rs`, `usage.rs`, `mod.rs` | task ids; the notices typed back (`Workspace::notices`, delivered as each look and each notice comes), with what the task spent read again a second after its end; changes to a task; a task's timeout; the transcript a turn's end names | 8 |
| `templum-mcp/src/lib.rs`, `templum-usage/src/claude.rs`, `templum/src/workspace/usage.rs`, `blocks.rs`; `templum-terminal/src/integration/*` | the words typed as raw arguments (`codex_args`, `settings_file`, `mcp_file`, `claude_args`); `Workspace::agent_env` on each new terminal; the `claude` and `codex` functions ([Agents typed in a terminal](#agents-typed-in-a-terminal)) | card 42 |
| `templum-terminal/src/agent.rs`; `templum-usage/src/copilot.rs`, `opencode.rs`, `qwen.rs`, `pi.rs` (new); `templum-mcp/src/hooks.rs`, `cli.rs`; `templum/src/workspace/usage.rs`, `blocks.rs`, `persist.rs`, `spawn.rs`, `bus.rs`; `templum/src/settings/connections.rs`; `templum-terminal/src/integration/*` | each agent's model and effort and resume; the files and words each agent gets; `templum hook copilot`, `opencode`, `qwen`, `pi`; the variable a block Templum opens gets; the switch reaching running agents; Gemini CLI's notice; the five new functions ([Every agent that can take it](#every-agent-that-can-take-it)) | card 44 |

Every request reaches the main thread through a channel the bus's threads write to, drained by a task as the
terminals' wake-ups are; the main thread never touches a socket. A request that waits keeps its reply channel in
`Workspace::waits` until an event, a button, its block's closing (checked each frame), its connection ending or its
cancel (`notifications/cancelled`; a question stays, its answer typed back), or its deadline (a timer: with nothing on screen moving, no frame comes) answers or
drops it.
No new dependency: `futures`, `serde`, `serde_json` and `uuid` are in the tree.

## Testing

**Unit (`templum-mcp`):** a request and a notification parse; bad JSON answers `-32700` with a null id; every line
written ends in one newline; every tool parses its own example arguments and names a missing one; an unknown tool fails;
`initialize`'s result; each Claude Code hook payload and Codex's `notify` become their event (`PermissionRequest` waits
for the person, `PostToolUseFailure` works on unless it is an interrupt, which is done, `StopFailure` and `idle_prompt`
are done, a `compact` start is nothing), unknown ones none; the bus: bind, hello, a request answered; a wrong first line
or token closes the connection, as does a hello too long or too slow, a byte at a time included; a client quiet
for longer than the hello's time after it is still answered; two requests on one connection both answered; the
socket file goes away with the bus and a stale one is replaced; a data folder too long for a socket puts it in a private
(0700) temporary folder that goes with the bus; the bridge sends its hello first and copies both ways; `hook claude`
sends one event and prints nothing; `read <block> -n 10` sends its call and prints the text; no bus is exit code 2 with
nothing on stdout. Every read and wait in these tests has a 5-second limit. (batch 5) `wait_for` takes its four events
and refuses another; `wire` is parsed but never listed; Gemini CLI's hooks become their events; `wait --for` and `wire`
send their calls, and every verb's usage offers `--json`.

**Unit (`templum-usage`, batch 5):** the extension's two files exactly, not rewritten unchanged, Windows' hook
command, a `'` in the path gives none.

**Unit (`templum`, batch 5):** the screen's looks (3 of 4 changed: at work; 5 still: at rest; a wait for the person
stays); a label's scale per level.

**Unit (`templum-terminal`):** `lines` reads history past the screen; `last_command` is the last finished command and
its output; `SpawnOptions.env` reaches the shell.

**Unit (`templum-canvas`, batch 2):** a wire's ends sit on the facing edges; a point on it hits, one 50 units off
doesn't; wires are symmetric.

**Headless UI:** a terminal's `TEMPLUM_BUS`; `whoami` names the caller's id and workspace, also over the real socket;
`read_terminal` of the caller sees what it printed; `send_to_terminal` with Enter runs; another block is refused until
wired; `list_blocks` says connected or not; titles reach agents cleaned and quoted; a `Stopped` event leaves the agent
idle and a `NeedsYou` one shows the yellow dot; a crashed agent's state is forgotten; `typed()` adds the flags for
Claude Code and Codex, none for Codex under Windows PowerShell 5.1, none when `[connections]` is off or there is no data
folder, and Claude Code's server only with its hooks; a block id survives saving, restoring, closing and undoing, and
moving to another workspace, and a pasted block gets a new one; (batch 2) the Wire tool and the menu wire two blocks,
undo takes it back, a selected wire is deleted with Delete, closing a block and undoing brings its wires back, wires
come back after a restart and a wire to a missing block is dropped; (batch 3) `delegate` types the header and the task
and Enter, `wait` ends with the `Stopped` message or its timeout, `reply` answers it, the inbox answers `ask_user`,
Allow wires and Deny refuses, `spawn_agent` opens a wired block beside the caller and types its prompt once it started,
closing a block answers what waited on it, the dot travels only while a task is handed over; (batch 4) comments and
assignee survive a round trip and an older file loads, `claim_card` refuses a card another block holds, `append_note`
shows in the open note, `open_file` lands at the line in the wired editor without taking the keyboard, and every
tool is refused without a wire; (batch 5) `wait_for` answers `needs_you` at once or
when the agent or its request asks, a command's end and code, a bell and a notification, its timeout with the last
lines, a closed block, and shows no request; `templum wire` wires a step Ctrl+Z takes back and an MCP client can't
call it; a changing screen says Working and a still one ends the task, the person's typing not counting; Rename
wire… labels, Ctrl+Z, a disconnect, a close and a restart keep it, a hand-edited file's label is trimmed or dropped;
the pill shows at 100%, 50% and 25% and not in grid mode; Settings ▸ Connections' switch, its Copy button and
Gemini's question; Gemini CLI's run, its failure and its absence.

**Batch 6:** each named key's bytes, under the kitty keyboard and application cursor keys, a character as text and an
unknown name refused; each agent's model and effort flags and every unsafe model refused; `send_keys` parses 1 to 16
keys; keys reach a terminal in bracketed-paste mode as keys, never `ESC[200~`; keys into a waiting agent are taken and
say who answered; the caller's own block and another agent's task are refused; `interrupt` presses Esc and ends the
caller's task; `send_to_terminal`'s Enter comes after its text, and two in a row stay two lines; `spawn_agent` types
its model and effort first and a bad one opens nothing; `close_block` closes only what the caller opened and Ctrl+Z
brings it back; `opened_by`, `model` and `effort` survive a round trip, and an older file loads; a snapshot that finds
another agent drops the model and the effort; a model Templum can't type in a hand-edited file isn't typed.

**Batch 7:** Codex's hook payloads become their events (`compact` and `SessionEnd` none, an older `notify`
argument none, stdin unread); Codex's hook flags, none without `<data>/bin` on PATH; a turn that ends before its task
was taken doesn't answer it, nor the turn Claude Code queued it behind unless a tool's end took it in; a tool's end
starts no turn; only a prompt taken confirms a task (not a `Stopped` event, not a moving screen); no second Enter into
a changed screen, nor while the person types; the first line goes as a paste of its own; a prompt waits for its
agent's input to take pastes and show its cursor, Claude Code's without any hook, and one not typed within a minute
ends its task with its screen; a hooked agent gone still keeps its task and seems stuck, unless the person typed into
it; `wait_for(needs_you)` and `delegate`'s refusal show the dialog as drawn, `list_blocks` what it asks.

**Card 118:** a task's Enter waits for its paste to stand still a look, and a screen that keeps moving gets it after
the looks; an agent no hook speaks for took it when its screen moves; an Enter taken for a new line (a fake agent that
reads its first line and starts on the second Enter) gets a second one, and `delegate` answers once it went; two
Enters that change nothing (a fake whose input never sends) answer `delegate` with "typed but not submitted" and to
clear its input, end the task, and type no notice besides; a bracketed paste whose Enter changed the screen, then still
(a dialog drawn at once), gets no second Enter; the person typing before the Enter gets none; an earlier task's looks
press nothing into a later one; a task handed to an agent spawned a moment ago, which no snapshot found yet, waits and
is typed once it is ready and not asking its person, and ends a minute after it was handed when it isn't; one no
snapshot found within a minute of its start never started, and a task for it is refused.

**Batch 8:** a task handed without `wait` comes back typed, with its id and time; what it spent is its cost, else its
tokens, read again after its end, and its transcript is the one its hooks named, on one line; a waiting call gets the
answer and no notice, one its client cancelled gets the notice; a notice waits while its asker asks its person, is
starting or its person types, goes at least 8.1 s after anything Templum typed into the same agent, oldest first, and
never goes into a shell (also when its agent quit while it waited); a task whose agent left or whose block closed comes
back as failed, one its asker interrupts or closes doesn't; a dialog in a task is told once per wait, seeming stuck too,
and not while a call waits for the task, and a request in the inbox isn't told; a change from the asker goes into
its task and restarts its timeout, none goes before a spawn's prompt, and another agent is still refused; a task
past its timeout comes back only at its end; a task handed with `wait` gets no notice of its timeout.

**Agents typed in a terminal:** the raw words are what is typed, unquoted (a path with a space is one
word), with `--mcp-config=`; a new terminal's variables follow the switch, the status line, WSL, cmd and Windows
PowerShell 5.1 as `typed` does; in bash, zsh, fish and PowerShell started as Templum starts them, with a fake `claude`
and `codex` on PATH that write down their arguments and exit 3: `claude hi` runs with Templum's words then `hi` (its
exit code 3 reaches the prompt's mark), an alias adds
its words after Templum's, the person's own function runs instead, `--settings x` (and Codex's
`mcp_servers.templum.…`) passes as it is, an emptied variable adds nothing, and without the variable there is no
function; a shell that isn't installed is skipped.

**Card 44:** each agent's model and effort words, and an effort it doesn't take refused; Copilot's, Cursor's and Crush's
resume; each agent's files exactly (not rewritten unchanged, Windows' forms, a `'` in the path gives none) and its
words, typed and raw, none when `[connections]` is off, under WSL or without a data folder; each hook payload or
plugin event to its event, unknown ones none, Copilot's last answer from a transcript; a block Templum opens for
OpenCode, Kilo Code or Qwen Code gets its variable, a person's own wins; the switch off fails a tool call, drops an
event and reads the screen again, the verbs still answer; Gemini CLI's notice once, not on restore, not with the
extension; in bash, zsh, fish and PowerShell with fakes of the five agents that write their arguments and their
variable: Templum's words first, none when the last one is the person's, the variable for that command only and not
over one already set. A test that needs an agent installed (its `--help` naming the flags Templum gives) skips without
it.

**Manual (Hyprland):** Claude Code from the menu: `/mcp` lists `templum` connected, "list the blocks" and "read the
last 30 lines of the dev server" work, the dot follows its turns and a permission prompt; Codex: `/mcp` lists it;
`templum blocks` in a plain terminal; `templum mcp` in another terminal emulator exits 2; wiring by tool and menu,
restart; Claude Code delegating to Codex and getting its answer; a question and a connect request in the inbox;
`spawn_agent` with a worktree; (batch 5) Gemini CLI's Install, `/mcp`, dot and Remove; Claude Code delegating to
Gemini CLI; Codex's dot turning accent within about 3 s of a prompt; a permission dialog left 15 s still yellow, an
allowed `sleep 20` turning accent within about 3 s, Esc while Claude Code writes ending Working within about 5 s, a
long prompt typed into an idle agent showing no accent; `templum wait --for command_finished` and `--for attention`;
`templum wire` and Ctrl+Z; a label at 50% and 25%, hidden in grid mode, kept by a restart, and Ctrl+Z after a rename
taking the name, not the wire; Settings ▸ Connections' switch (no `--mcp-config` once off) and Copy; on Windows,
Gemini CLI's dot following its turns. (batches 6 to 8) The user's own test again: Claude Code spawns a Codex
with a model and an effort, hands it a task without waiting, changes it while it works, and hears its end typed back
with its time and tokens; Codex's trust question for Templum's hooks asked once and answered with `send_keys ["2"]`,
nothing left in Codex's input, "answered by" shown; a still Codex dialog turning yellow as stuck; `interrupt`, then
`close_block`, then Ctrl+Z.

**Manual, card 44** (2026-10-18, a `--dev` Templum driven with the `templum` command line against its bus): Copilot
CLI from `spawn_agent` gets its words and `--session-id`, `/mcp` lists `templum`; a task delegated with `--wait`
comes back with its answer ("ping", read from its transcript); a `touch` it asks to run turns the block "waiting for
its person: Run command: touch …", allowed it ends "Done."; its call of `templum/whoami` asks first, then answers
the block's id; a `copilot` typed in the person's own bash gets the three words; a restart brings it back with
`copilot --session-id <its id>`. OpenCode from `spawn_agent` (a free model) shows two MCP servers, a delegated task
comes back with its answer, a restart brings it back with `OPENCODE_CONFIG_CONTENT` in its environment. Connections
turned off in `config.toml`: `templum mcp`'s `initialize` and a tool call get the off sentences, `templum whoami`
answers; on again, the tool answers. Not checked by hand: Gemini CLI's notice clicked, Qwen Code and Kilo Code (not
installed), Pi answering (no model), OpenCode's dialogs ([minors](../minors.md#connections)).

## Delivery

One pull request per batch, each with its own plan in `docs/plans/`:

1. **The bus:** `templum-mcp`, the block id, `TEMPLUM_BUS`, `whoami`, `list_blocks`, `read_terminal` and
   `send_to_terminal` (the caller's own block until batch 2), `notify`, the hooks and the agent's state, what is
   typed into Claude Code and Codex, `[connections]`, the verbs of batch 1.
2. **Wires:** the model, painting, the Wire tool, the menus, selecting and deleting, undo, saving; tools check wires.
3. **Agents together:** `connect`, `spawn_agent` with worktrees, `delegate`, `reply`, `wait_for_idle`, `ask_user`,
   the inbox, the travelling dot.
4. **Content:** the kanban tools with comments, assignee and claiming; notes; `open_file`; `git_status` (removed
   since: agents run `git status` in their own terminal).
5. **The rest:** `wait_for`, wire labels, Settings ▸ Connections, Gemini CLI, `templum wire`, `--json` everywhere.
6. **Control:** `send_keys`, `interrupt`, `close_block` and `opened_by`, `spawn_agent`'s `model` and `effort`,
   "answered by", `send_to_terminal`'s later Enter.
7. **State:** Codex's hooks, `Submitted` confirming a task, a prompt waiting for its agent's input, seems stuck,
   dialogs as drawn.
8. **Async:** task ids, the notices typed back with time and tokens, changes to a task, a task's timeout told.
9. **Agents typed in a terminal** (kanban card 42, [its plan](../plans/2026-10-18-agents-in-any-terminal.md)): the
   variables, the shells' functions, the switch's words. It types nothing into agents and paints nothing: no bench.
10. **Every agent that can take it** (kanban card 44, [its plan](../plans/2026-10-18-agent-parity.md)): the small gaps
    of [agent-parity.md](agent-parity.md), then Copilot CLI, OpenCode and Kilo Code, Qwen Code and Pi. It paints
    nothing: no bench. Copilot, OpenCode and Pi start with a check by hand of their hooks and events.

Batches 2, 3 and 6 to 8 paint or type into agents, so their pull requests carry `--bench` and `--bench-mixed`. Batches 7
and 8 start with checks by hand of the agents themselves (what Codex takes on its command line and the payloads of its
hooks; when Claude Code and Codex fire `UserPromptSubmit` for a prompt typed while they work; the key that stops a turn;
when each agent turns bracketed paste on), and their plans correct this spec with what they find (batch 7's: [Spike
results](../plans/2026-10-16-connections-batch-7.md#spike-results); batch 8's: [Spike
results](../plans/2026-10-16-connections-batch-8.md#spike-results)).

## Risks

| Risk | What we do |
|---|---|
| Claude Code gives up on a tool call that sends nothing for 1800 s (its idle timeout for stdio servers) | The `mcp.json` Templum gives it sets Templum's server's `timeout` to seven days, which lifts it for every tool; a Claude Code opened by hand keeps 1800 s. A cancelled call (`notifications/cancelled`) drops its request, a cancelled question stays and its answer is typed back (tested); a call backgrounded past 2 minutes isn't checked by hand |
| Typing into an agent that is busy | Refused while it waits for its person; while it works, Claude Code and Codex queue typed input (checked by hand in batch 3); the `[from …]` line keeps it readable; Claude Code fires `UserPromptSubmit` for it at the Enter and queues it behind the turn under way unless a tool's end takes it in, Codex once a turn takes it in (batch 7's spikes), which a change to a task (batch 8) relies on |
| An agent answers another's trust question (a folder's, or Codex's for hooks, which may list a repository's own hooks) | The wire is the person's consent; "answered by" names the agent; the result shows the screen after the keys |
| Keys into a shell | The same trust as `send_to_terminal`; at most 16 keys a call |
| A model or effort typed into a shell | A strict set of characters, no leading `-`, an effort from its agent's list; refused before anything opens |
| A dialog no hook tells of (a trust question, Codex's choice of model, Aider's "Add file to the chat?") | No second Enter into a screen the first changed, unless the paste went as keys (Windows' console; an Aider there may still get one: [minors](../minors.md)); a hooked agent gone still for 30 seconds seems stuck, and its asker hears so |
| An agent reads a paste as keys and takes an Enter just after them for a new line (Codex and Copilot CLI on Windows) | From card 118 the Enter waits for the screen to stand still, and a screen that didn't move after it gets a second one |
| An agent no hook speaks for takes a task and draws nothing while it works | Its first Enter changed its screen: it took the task (a second Enter, only after a paste that went as keys, goes into an empty input); only an agent no Enter changed is told "typed but not submitted" |
| Codex's hooks need its person's trust | Asked once, about one fixed command (`templum hook codex`); never `--dangerously-bypass-hook-trust` |
| A notice typed into an agent the person is typing into | It waits until the person stopped typing for 3 seconds and sent or cleared what they typed; never into a shell |
| What a task spent, read before its agent wrote its last figure | Read again a second after its end: Claude Code's status line gives its last cost 0.3 s after its `Stop`, Codex writes its tokens just before its own (batch 8's spikes) |
| Claude Code's status-line token totals are its last call's context, not what it used (batch 8's spikes) | The notice gives its cost, as its usage chip does |
| Two notices into one agent in a moment would share an Enter | The next waits 5.1 seconds after the last text Templum typed into it, its Enters past |
| A spawned agent's prompt typed while it asks a first-run question (trusting a new folder, Codex's hooks) | From batch 7 Claude Code's and Codex's wait until they bracket pastes and show their cursor, which both hide in those questions (checked by hand); on Windows the still screen alone |
| Claude Code takes a long paste for data it was shown, not for what it is asked (batch 7's spikes) | The first line, `[from …]` and a sentence, goes as a paste of its own, which Claude Code reads as typed |
| A command an interrupt left running ends later, with Codex's `PostToolUse` | A tool's end never starts a turn (batch 7) |
| A task typed into a Codex at work is answered by the turn it was on | One task at a time per agent; from batch 7, a turn answers a task only once the agent took it (`UserPromptSubmit`) |
| An agent without events never says it is done | `reply`; its screen going still ends its task (`activity.rs`); else the wait's timeout with the last screen lines |
| Gemini CLI ignores a system settings file root doesn't own | An extension the person installs once from Settings ▸ Connections |
| A spinner drawn under a permission dialog would turn its yellow dot to Working | The screen ends a wait for the person only once the person typed into the terminal after it began; checked by hand with Claude Code and Gemini CLI (manual check 4) |
| Windows: PowerShell may not wait for, or hand stdin to, `templum.exe` (a windowed program) in a Gemini hook | Checked by hand on Windows; else `$input \| & '<bin>' hook gemini` |
| Codex's `-c` under `cmd.exe` and Windows PowerShell 5.1 | Not typed there (Git Bash is the default shell on Windows; pwsh 7 takes them) |
| A path Templum can't type safely (`"`, `$`, `'`, a backtick…) | No connections for that agent, and a notification once per session (deferred) |
| macOS's 104-byte socket path | A private folder in the temporary folder past 100 bytes |
| The AppImage's executable goes away when it quits | `$APPIMAGE` is the program typed |
| Reading history while the terminal's thread parses | The unfair lock waits only for the parse under way, as painting does; at most 2000 lines and 256 KB |
| A hook slows the agent when Templum is gone | One-second connect limit, exit 0; each hook has a 5-second timeout |
| Painting wires costs frames | One curve per wire per frame, below the blocks; cached per zoom step as drawings are if the benches ask for it |
| A process of the same user names another terminal | The documented trust: it could already type into it |
| A `claude` typed by hand gets Templum's words twice (the function, then what Templum types) | The function adds nothing when `--settings`, `--mcp-config` or `mcp_servers.templum.` is already there (tested) |
| A person's own `claude` function or alias | Theirs wins (a function) or composes (an alias), tested in each shell; `command claude` is always the bare agent |
| A newer Claude Code makes `--mcp-config=` take more than one word, or refuses root flags before a subcommand | Checked by hand on 2.1.286; the shells' tests run a fake `claude`, so a by-hand check at each card on connections (`/mcp` in a typed Claude Code) catches it |
| A tmux pane's agent speaking for the block that started tmux's server | No function there: tmux's shells don't load Templum's integration |
| A failing hook denies Copilot a tool (`preToolUse` fails closed) | Templum hooks no `preToolUse`, and `templum hook` always exits 0 and prints nothing |
| A nested agent (an `opencode run` or a `pi -p` its own shell tool starts) speaks for the block | `TEMPLUM_HOOKED`: the first plugin of a process tree speaks, the others say nothing; Qwen Code's hooks are commands and can't tell: a minor |
| Templum's Qwen settings file hides Qwen's own system file | Not given when that file exists: the server only |
| A seven-day `timeout` makes OpenCode wait seven days for a server that hangs at its start | `templum mcp` answers at once or exits 2 (no bus, Templum gone) |
| Copilot's flags or its hooks' payloads change (it ships weekly) | The test that reads an installed agent's `--help` for Templum's flags; the hook parser's payloads recorded by the plan's spike |
