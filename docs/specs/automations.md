# Automations: Templum's cron

- **Date:** 2026-10-24
- **Status:** Batch 1 (time) implemented and committed (kanban card 150, "a cron system (a button in the sidebar near
  Git)"); batch 2 (events) implemented, uncommitted until reviewed. Delivered in two batches: batch 1, time (plan:
  [2026-10-24-automations.md](../plans/2026-10-24-automations.md)); batch 2, agent and command events (plan:
  [2026-10-24-automations-events.md](../plans/2026-10-24-automations-events.md)).
- **Builds on:** [agent-tools.md](agent-tools.md#schedule-and-cancel_schedule) (an agent's `schedule`: deliveries
  later, the clock), [agents-view.md](agents-view.md) and [ask-inbox.md](ask-inbox.md) (the sibling popups this one is
  drawn like), [connections.md](connections.md) (Delegating: how a task is typed into an agent; Spawning; an agent's
  state), [shell-integration.md](shell-integration.md) (a command's end and its exit code),
  [instructions.md](instructions.md) (an app-wide list saved in `<data>/*.json`), [accounts.md](accounts.md),
  [workspaces.md](workspaces.md#the-sidebar)

The person said "pode decidir" (decide it): the product calls below are the orchestrator's, marked *the
orchestrator's call*; what the designer settled is marked *the designer's call*. The UI says **Automations**; this
spec is the card's cron.

## Intent

Card 150, in the person's words: "Create a cron system. Grow the idea and see how useful a cron is in the system:
delegate repetitive tasks, have a cron make an agent do something, run a command in a terminal, fetch something from
an API… I thought of things like 'when an agent is idle, do this'; make it very customizable, I want the user to have
lots of freedom. 'Run today at 17:00', 'every day at 17:00', specific days of the month or of the week… Choose
specific terminals or a kind of agent, like 'when any Claude agent goes idle' or 'when agent X and agent Y go idle'."

Today only agents can ask Templum for something later (`schedule`: a text to an agent, once or every few minutes, gone
at quit). The person has nothing: to ask an agent for a review every morning, to run a check every ten minutes, or to
hand the next step to an agent the moment another finishes, they must be there and type it.

An **automation** is a rule the person writes once: **When** something happens (a time comes; in batch 2, an agent
ends its turn or needs them, a command ends), **Do** something (type a prompt into an agent, run a command, start an
agent, notify them). Templum runs it while it runs, and shows each rule's next run and how its last one went.

## Goals

1. One place: a sidebar button beside Git, and a shortcut, open the Automations popup: the person's automations at the
   left, the selected one's editor at the right.
2. Times as people say them: once at a date and time, every N minutes or hours, every day at HH:MM, on chosen
   weekdays or days of the month at HH:MM, and a raw cron expression for power users. The local clock, daylight saving
   counted.
3. Events (batch 2): an agent ends its turn, or needs the person: a chosen block, any of several or all of them, or
   any agent of a kind; a command ends in a chosen terminal, any exit or failures only.
4. Four things to do: type a prompt into an agent (as `delegate` types a task), run a command in a terminal or a new
   one, start an agent with its folder, model, effort and first prompt, notify the person.
5. Safe by default: a prompt never interrupts an agent at work or a person typing, nothing is typed into a shell an
   agent left, an event automation can't run more often than its cooldown, a loop is pointed out, a rule whose target
   is gone says so and does nothing.
6. Every rule tells, in one line, when it runs next and how its last run went (ran, skipped and why, failed and why,
   missed).
7. The schedules agents asked for (`schedule`) show in the same popup, under **Asked by agents**, with Cancel.

## Non-goals

*The orchestrator's call* for each:

- MCP tools for agents to create automations (agents keep `schedule`).
- A built-in HTTP action: fetching from an API is a command (`curl …`).
- Running while Templum is closed: no system cron, no daemon.
- Chains or multi-step workflows: one When, one Do.
- A run history beyond the last run.
- Moving agents' own schedules into automations: they stay as they are, only listed here.

*The designer's call* for each:

- Keeping an automation's task's answer: the agent's answer stays in its own block (its `reply` goes to no one).
- A second Do, conditions ("only on weekdays" for an event), variables in the prompt (the time, the agent's name).
- Ordering the list: a new automation goes last, as instructions do.
- A worktree for **Start an agent**: `spawn_agent` has one; a rule that runs every morning would make a branch each
  time.

## Anti-goals (a failure even if it works)

- A prompt typed into an agent at work, into a dialog, or on top of what the person is typing.
- Anything typed into a shell where an agent was expected (the text would run as a command), or a command typed into a
  running agent or a running command.
- A time missed while Templum was closed running at start, or every missed time running at once after a sleep.
- An automation running silently against nothing: a target that is gone shows "Target missing" before its time, and
  its time says it was skipped.
- A closed popup costing frames: closed, Automations costs one look a second over the rules that are on.

## Words

- **Automation:** one rule: a name, a When, a Do, on or off, and where it applies (one workspace, or all).
- **When:** its trigger. **Time** (batch 1) or **event** (batch 2).
- **Do:** its action.
- **Run:** one time its Do went (or tried). **Due:** its time came, or its event happened. **Held:** due, and waiting
  for its target to be free.
- **Target:** what its Do acts on: a block (named by its uid, unique across workspaces), a workspace (where a new block
  opens), an agent of Settings ▸ Agents (what Start an agent opens).
- **The agent that fired it** (batch 2): the block whose event made the automation due.
- **Missed:** due while Templum was closed.

## An automation

*The orchestrator's call* for what it has; *the designer's call* for the details.

| Field | Values | Saved as |
|---|---|---|
| Name | one line, at most 60 characters; may be left empty | `name` |
| When | see [When](#when) | `when` |
| Do | see [Do](#do) | `do` |
| On | on when saved new; off keeps it and runs nothing | `on` (missing: on) |
| Where | All workspaces, or one workspace | no `workspace` key, or the workspace's id |
| Stop after | a number of runs; empty: no limit | `max_runs` |
| Cooldown (batch 2) | minutes between two runs of an event automation, 0 to 1440, default 5 | `cooldown` |

- **An empty name** (*the orchestrator's call*: a default built from it) shows as the preview sentence everywhere a
  name shows; a task's first line names it by its When's words.
- **Where** (*the designer's call*): an automation for one workspace runs only while that workspace runs (not stopped,
  not deleted); for **Run a command in a new terminal** and **Start an agent** it is where the new block opens, so those
  need one workspace (the editor offers no "All workspaces" for them). Batch 2: its events are heard only there.
- **Stop after N runs** (*the designer's call*: on every automation, not only events): it counts the runs that went
  (Ran), not those skipped, failed or missed; the Nth turns it off ("Ran 6 of 6 times, now off"). Saving it or turning
  it on starts the count again.
- **Cooldown** (batch 2, *the designer's call*): saved only when it isn't 5 (no `cooldown` key: 5); an empty box is
  5, its placeholder "5"; past 1440 Save says "The cooldown runs from 0 to 1440 minutes.", a box that isn't a whole
  number "Write the cooldown in minutes.". Shown only for an event; a time keeps the value, unused.

### Where they are saved (*the orchestrator's call*: the instructions' pattern)

`<data folder>/automations.json`, app-wide, written by the 5-second snapshot when it changed and at quit, through a
temporary file and a rename; a file Templum can't parse is kept as `automations.json.bak`, one it can't read means none
is written this session, each with the instructions' notification. No data folder (`--bench`): no automations. Ids are
numbers given once (`next_id`).

Besides the rule, each automation keeps (*the designer's call*) its count of runs (`runs`), when its When started
counting (`since`, Unix seconds: when it was saved or turned on), its last run (`last`: when, Ran, Skipped, Failed or
Missed, and the words about it), and the uid of the terminal its command last opened (`opened`). A restart keeps them.

```json
{ "version": 1, "next_id": 2, "automations": [
    { "id": 1, "name": "Morning review", "on": true, "workspace": "k2v8q",
      "when": { "weekly": { "days": [1, 2, 3, 4, 5], "at": "09:00:00" } },
      "do": { "prompt": { "block": "abc12", "text": "Review the open pull requests." } },
      "runs": 3, "since": 1761300000,
      "last": { "at": 1761555600, "went": "ran", "about": "Typed into abc12 as task t4." } },
    { "id": 2, "name": "", "on": true,
      "when": { "every": { "minutes": 10 } },
      "do": { "command": { "folder": "~/api", "text": "curl -fsS localhost:8080/health" } },
      "max_runs": 6, "runs": 0, "since": 1761558000, "opened": "q3x9z" } ] }
```

## When

### Time (batch 1)

*The orchestrator's call* for the kinds; *the designer's call* for the details.

| Kind | Fields | Saved as | In words |
|---|---|---|---|
| Once | a date and a time | `once: { at }` (local, `2026-10-24T17:00:00`) | "On 24 Oct at 17:00" ("On 3 Jan 2027 at 09:00" in another year) |
| Every | N, minutes or hours (1 minute to 7 days) | `every: { minutes }` | "Every 10 minutes", "Every hour", "Every 2 hours" |
| Every day | HH:MM | `daily: { at }` | "Every day at 17:00" |
| On weekdays | chosen weekdays, HH:MM | `weekly: { days, at }` (1 Monday … 7 Sunday) | "Every Monday at 09:00", "Every Mon, Wed and Fri at 09:00", "Every weekday at 09:00" (Monday to Friday), "Every day at 09:00" (all seven) |
| On days of the month | chosen days 1 to 31, HH:MM | `monthly: { days, at }` | "On the 1st and 15th of each month at 09:00" |
| Advanced | a 5-field cron expression | `cron: { expr }` | `At cron "0 9 * * 1-5"` |

- **The clock** (*the orchestrator's call*: as `schedules::next_at`): times are the computer's local clock, as a
  `chrono::Local` moment of that day. A time a daylight-saving change skips runs at the hour after (02:30 on the spring
  night runs at 03:30); a time it repeats runs once, the first.
- **Every N** (*the designer's call*) counts real time from `since` (the save or the switch on), not the clock: "every
  2 hours" saved at 10:13 runs at 12:13, 14:13…; daylight saving doesn't move it.
- **Days past a month's end** (*the orchestrator's call*: "31" means the last day in shorter months; *the designer's
  call*: the same for 29 and 30): a chosen day the month lacks runs on its last day, once even when several fall there
  (30 and 31 in April run once, on the 30th). The editor says it under the days: "29, 30 and 31 run on the last day of
  shorter months."
- **Weekdays** are listed Monday first (*the designer's call*).
- **Cron** (*the designer's call*: Vixie cron's five fields, parsed by Templum, no new crate: the grammar is a page
  of code and every crate on offer adds seconds, years or a scheduler Templum doesn't need): minute (0–59), hour
  (0–23), day of month (1–31), month (1–12 or `jan`–`dec`), day of week (0–7, 0 and 7 Sunday, or `sun`–`sat`); each
  field `*`, a number, a range `a-b`, a step `*/n`, `a-b/n` or `a/n`, or a list of those with commas; names in any
  case. When both day fields are restricted, either one matching is enough (cron's rule). No `@daily`, no seconds, no
  years: the editor says "Use five fields: minute hour day month weekday." A field out of range says which: "Hour: 25
  is past 23." Under the field the editor shows its next three times, or the error.
- **Never** (*the designer's call*): a Once whose time passed, or a cron that matches no day in the next five years
  (`0 0 30 2 *`), can't be saved: "That time has passed.", "It would never run."
- **A time comes** when a look finds the clock at or past it: within a second of its minute. Its next time is then the
  first one after now: a sleep or a clock moved forward runs it **once** (*the orchestrator's call*: as schedules do),
  never once per missed time.
- **Templum closed** (*the orchestrator's call*): at start, an automation whose first time after its last run (or
  after `since`) has passed is not run: its last run becomes **Missed** at that first missed time ("Missed 24 Oct
  17:00"), and its next time is the first one after now. *The designer's call:* a Once that was missed is turned off,
  as one that ran.

### Events (batch 2)

*The orchestrator's call* for the kinds; *the designer's call* for the details.

| Kind | Fields | Saved as | In words |
|---|---|---|---|
| An agent ends its turn | who | `agent: { event: "idle", who }` | "When Claude Code (abc12) finishes its turn" |
| An agent needs you | who | `agent: { event: "needs_you", who }` | "When any Codex in api needs you" |
| A command ends | a terminal; any exit, or failures only | `command: { terminal, failures }` | "When a command fails in zsh (q3x9z)" |

- **Who** is one of: a chosen block (`{ "blocks": ["abc12"] }`); several chosen blocks, with **Any of them** or **All
  of them** (`{ "blocks": [...], "all": true }`); or **any agent of a kind** (`{ "kind": "claude" }`), an id of
  Settings ▸ Agents, accounts included, matched as a card's "any X" is (`Workspace::runs`: an agent's accounts' blocks
  count for it; an account is its own only), in the automation's workspace or in all.
- **Ends its turn** is the moment the Agents view marks an agent Done: it goes from working (or waiting for its
  subagents) to idle, its subagents done (`agents_view::Watch::see`), whether or not the person looks at it. Heard in
  every workspace that runs, hidden ones included, wherever Templum hears an agent (a frame, a bus message, the
  once-a-second look at the screens).
- **Needs you** is the moment its state becomes Needs you (agents-view.md's first state: a request in the inbox, an
  attention alert, its own dialog), once per wait.
- **All of them** fires when one of the set ends its turn while every other one is already idle: the last to finish
  fires it, and the agent that fired it is that one. A block of the set that isn't open makes it **Target missing**.
- **A command ends:** the terminal's count of finished commands (shell integration's `OSC 133;D`) went up. **Failures
  only** fires on an exit code other than 0; a command without an exit code is no failure. Several commands that end
  between two looks fire once, for the last.
- **The cooldown** (*the orchestrator's call*: 5 minutes, settable): counted per automation and per agent that fired
  it (*the designer's call*: "when any Claude ends its turn" still answers each Claude), from that run's start. An
  event inside it does nothing and is not written as a run (it would bury the last run). **All of them** has one
  cooldown.
- **Events while Templum was closed** don't exist for it: nothing runs at start.

The rest of batch 2's details are *the designer's call*:

- **Heard at once, run at the next look:** an event is noted the moment Templum hears it (only while some event
  automation is on) and acted on at the automations' next look, within a second. The events of one look fire their
  automations in the list's order. An agent seen for the first time (Templum's start, a block restored) fires
  nothing, whatever its state; a Needs you whose question changes (another request) is the same wait.
- **Who and Where:** the blocks offered are the agents' blocks of the automation's workspace (all of them for All
  workspaces), as the Do's Agent dropdown lists them; a kind's events come only from the automation's workspace when it
  has one. Every chosen block must be open there, for one block, Any and All alike: one that isn't makes it **Target
  missing** ("Target missing: abc12 isn't open", "…: abc12 is in api" when another workspace has it); so does a kind
  no longer in Settings ▸ Agents ("…: claude2 isn't in Settings ▸ Agents") and a command's terminal that isn't open
  ("…: q3x9z isn't open"). An event that still comes meanwhile (another block of an Any, an agent the removed kind
  still names) is **Skipped** with those words, as a time is, and starts its cooldown.
- **All of them** is offered only for "finishes its turn"; several blocks that "need you" fire on any of them. A
  block of the set that runs no agent, or isn't open, doesn't hold it back. At the look, the last of the set heard
  ending is the agent that fired it, and it fires if every other block of the set is idle then.
- **The block that fired a command event** is its terminal: new blocks open beside it and the notification's click
  shows it, but it takes no prompt ("The agent that fired it" is offered only for an agent event).
- **One run per agent that fired it:** each agent's run is held or typed apart from the others' (two Claudes that
  finish together each get their prompt, and two prompts for one block take turns as the edge table says); an event
  from an agent whose run is still held or being typed joins that run, as a time joins a held run, and starts no
  cooldown.
- **The cooldown** starts when the event fires the automation, whatever the run then does (ran, held, skipped,
  failed). It lives in memory: saving the automation or turning it on starts its cooldowns over; Run now neither waits
  for one nor starts one.
- **A command's count:** the first look after Templum starts, after the automation is saved or turned on, or after its
  terminal is reopened notes the terminal's count of finished commands and fires nothing.
- **The words:** Any of them "When Claude Code (abc12) or Codex (k9x1d) finishes its turn" ("A, B or C"); All of them
  "When Claude Code (abc12) and Codex (k9x1d) have all finished their turns"; a kind names the automation's workspace
  when it has one ("When any Codex in api needs you", "When any Claude Code finishes its turn" for All workspaces);
  any exit "When a command ends in zsh (q3x9z)". The row's line 1 says the event it waits for: "on abc12's turn's
  end", "on a turn's end of 3 agents", "once all 3 agents finish", "on a Claude Code's turn's end"; "when abc12 needs
  you", "when one of 3 agents needs you", "when a Codex needs you"; "on a command's end in q3x9z", "on a failure in
  q3x9z". The editor's preview says "Runs " and the When's words, their first letter lowered: "Runs when Claude Code
  (abc12) finishes its turn".

## Do

*The orchestrator's call* for the four; *the designer's call* for the details.

| Do | Fields | Saved as | In words |
|---|---|---|---|
| Send a prompt to an agent | an agent's block (batch 2: or the agent that fired it); the prompt | `prompt: { block, text }` | `type "Review the open…" into Claude Code (abc12)` |
| Run a command | in a new terminal (a folder) or in a terminal (a block); the command, one line | `command: { folder }` or `command: { terminal }`, and `text` | `run "make test" in a new terminal in ~/api`, `run "make test" in zsh (q3x9z)` |
| Start an agent | an agent of Settings ▸ Agents; folder, model, effort, a first prompt (each optional but the agent) | `agent: { agent, folder, model, effort, prompt }` | `start Claude Code in ~/api with "Triage the new issues…"` |
| Notify me | a text | `notify: { text }` | `notify you: "Stand-up in 5 minutes"` |

The **preview sentence** is the When's words, a comma, the Do's words and a period: "Every weekday at 09:00, type
"Review the open…" into Claude Code (abc12)." A quoted text is its first line cut at 40 characters with "…"; a block is
named by its title and uid as the blocks list names it; a folder as typed.

### Send a prompt to an agent

- **Typed as `delegate` types a task** (*the orchestrator's call*): a first line `[from automation "Morning review",
  task t12] Your task is in the pasted text: do it, then answer it with reply.`, then the prompt, its Enter and
  confirmation as Delegating 2 says. The asker is the automation (`automation-<id>`, a word no terminal has), so the
  agent's answer and its task's end reach no block; another agent can't hand it a task until it ends, as for any task.
  A name left empty is the When's words: `[from automation "Every weekday at 09:00", task t12]`.
- **Free** (*the designer's call*): the target takes a prompt only when its agent is Idle (or Done) in the Agents
  view's words, is on no task, and Templum may type a notice into it (connections.md, Delegating 10: no draft of its
  person's, no key of theirs in the last 3 seconds, nothing Templum typed in the last 8.1 seconds).
- **Held** (*the orchestrator's call*: it waits until the agent is idle, as schedules' notices wait; *the designer's
  call*: how long and what shows): a prompt due while its agent isn't free waits, looked at every second, at most **60
  minutes**; meanwhile the row says why ("Waiting for abc12: it is working", "…: it needs you", "…: it is on a task",
  "…: you are typing in it", "…: it is starting", "…: Templum is typing into it"). Past the hour: **Skipped**, "abc12
  stayed busy for an hour." A time
  that comes while its run is held joins it: the prompt is typed once.
- **Ran** once the agent took the task ("Typed into abc12 as task t12."); **Failed** when it didn't (the reason
  `delegate` gives: "abc12 didn't take the task: typed but not submitted: …").
- **Never into a shell:** a target whose agent quit (no agent seen there, or its shell in front) fails at once: "abc12
  runs no agent now: a prompt is never typed into a shell." A subagent's read-only block can't be chosen.
- **The agent that fired it** (batch 2, *the designer's call*): saved as the block `"fired-it"` (`prompt: { block:
  "fired-it", text }`), a word no block id can be, as the asker's `automation-<id>`. Its runs wait and are written as
  any prompt's, with the id of the agent that fired it: "Waiting for abc12: it is working", "Typed into abc12 as task
  t12.". Only an agent event has one: switching the When to a time or a command ends empties the Agent dropdown
  ("Choose an agent." at Save), and a hand-edited file that pairs it with a time is Target missing: "Target missing:
  only an agent event has an agent that fired it".

### Run a command

- **In a new terminal** (*the designer's call*: the default, the safe one): opened in the automation's workspace at its
  view's middle, in the folder (`~` expands; empty: the workspace's folder, else the home folder), the command typed
  once its shell is ready, as `create_block`'s command. The keyboard, the selection and the camera stay. *The
  designer's call:* the automation keeps that terminal (`opened`) and runs its next command there while it is open, its
  shell at its prompt with no command running and no agent; otherwise it opens another. A rule every ten minutes keeps
  one terminal, its history readable, instead of filling the canvas. A folder that doesn't exist fails: "No folder
  ~/api."
- **In a terminal** (*the orchestrator's call*: only at its shell's prompt, never into a running agent): typed, then
  Enter, when its shell is in front with no command running (shell integration's command start; a shell without the
  integration counts as at its prompt), no agent runs there, and its person hasn't typed into it for 3 seconds;
  otherwise held as a prompt is ("Waiting for q3x9z: a command runs in it", "…: an agent runs in it", "…: you are
  typing in it"), at most 60 minutes, then Skipped. *The designer's call:* Templum can't read what the person
  half-typed at that prompt and left: the command would join it. Batch 1 records it in docs/minors.md (fix: read the
  line between the prompt's mark and the cursor, and hold the command while it isn't empty); a new terminal, the
  default, has no such risk.
- **Ran** once it was typed: "Typed into q3x9z." / "Opened q3x9z in ~/api."

### Start an agent

- As `spawn_agent` opens one (*the orchestrator's call*): the agent or account of Settings ▸ Agents, in the folder (as
  above), on the model and effort, which are checked as `spawn_agent` checks them when saved and again when it runs;
  its first prompt typed once it is ready, as a task from `automation "<name>"`. It opens in the automation's
  workspace at its view's middle (batch 2: beside the agent that fired it when that one is in the same workspace),
  wired to nothing; the person closes it as any block. *The designer's call:* a new terminal of **Run a command**
  opens beside it too (a kept terminal is reused wherever it is); Run now, or an agent that fired it and closed
  before its run, opens it at the view's middle.
- Each run opens a new agent (*the designer's call*); **Stop after N runs** is how to bound it.
- **Ran:** "Opened Claude Code as k9x1d in ~/api."; **Failed** with what `spawn_agent` would refuse ("Claude Code
  isn't installed: …").

### Notify me

- A notification at the bottom right, titled the automation's name (or its sentence), its text below; while Templum is
  in the background and `terminal.notifications` is on, a system notification too, as news is (*the designer's call*).
  A click opens the Automations popup on it (batch 2: shows the block that fired it). *The designer's call:* shown as
  Go to terminal shows a block (its workspace, the block focused, the camera on it); a block closed since, or a run
  from a time or Run now, opens the popup on the automation.
- It also sends a `notice` through the API (`{"text": "<name or sentence>: <text>", "workspace": …, "automation":
  <id>}`), to the tokens that see its workspace, so a computer that mirrors the workspace can say it too.
- **Ran:** "Notified."

## Safety

*The orchestrator's call* for each; *the designer's call* for the words.

- **Cooldown and Stop after:** above.
- **A loop** (batch 2): the editor warns, in the warning color under the Do, when an event's who could include the
  Do's target: "This can loop: its prompt ends a turn of abc12, which fires it again. The cooldown spaces the runs
  out." (the same block, the agent that fired it, or a kind that matches the target's agent); and for a command that
  ends in the terminal its command runs in: "This can loop: its command ends in q3x9z, which fires it again." It
  warns; it doesn't forbid (a review loop may be wanted). *The designer's call:* the prompt's warning is for "finishes
  its turn" only (a prompt to an agent that needs the person is held, it can't spin); for the agent that fired it:
  "This can loop: its prompt ends a turn of the agent that fired it, which fires it again. The cooldown spaces the runs
  out."; a kind matches the target as an event's who does (`Workspace::runs`), and a target that isn't open can't be
  told. And **Start an agent** whose agent a kind matches, on either event, the one loop no cooldown spaces out (each
  new agent has its own): "This can loop: the agent it starts can fire it again, and each new agent has its own
  cooldown. Stop after bounds it."
- **Target missing:** a target block that isn't open in a running workspace (closed, or its workspace stopped), the
  automation's workspace stopped or deleted, or an agent no longer in Settings ▸ Agents. The automation doesn't run;
  its row is drawn as an off one, its switch still as the person set it, and says "Target missing: abc12 isn't open"
  ("…: api is stopped", "…: a deleted workspace", "…: claude2 isn't in Settings ▸ Agents"). Its time still passes:
  **Skipped** with those words. It runs again once its target is back (a Ctrl+Z, the workspace started: uids are
  kept).
- **Missed while closed; once after a sleep:** above.

## At the edges

| | A busy target | Target gone | Its workspace stopped | Two at once | Templum restarts |
|---|---|---|---|---|---|
| **Time** | its Do decides (below) | Skipped, "Target missing" before | Skipped, "Target missing" before | each runs, in the list's order | missed times are Missed, not run; next times from now |
| **Agent event** (2) | — | Target missing: doesn't run | not heard there | each automation an event fires runs, in the list's order | nothing replays; cooldowns start over |
| **Command ends** (2) | — | Target missing: doesn't run | not heard there | one firing per look for the last command | nothing replays |
| **Prompt** | held up to 60 min, then Skipped | Skipped, "Target missing: abc12 isn't open" (shown so before) | as gone | two prompts for one agent: the first typed, the next held until it is free again | a held prompt is lost: its time shows as Missed |
| **Command in a terminal** | held up to 60 min, then Skipped | as a prompt | as a prompt | as a prompt | as a prompt |
| **Command in a new terminal** | its kept terminal busy: another opens | its kept terminal closed: another opens | Target missing | each opens or reuses its own | its kept terminal is reused (uids survive) |
| **Start an agent** | — | agent removed: Target missing | Target missing | each opens one | — |
| **Notify** | — | — | Target missing (a one-workspace rule) | one notification each | — |

- A held run is dropped when the automation is turned off, saved again, or deleted (*the designer's call*); batch 2:
  every agent's held run, also when Stop after turns it off.
- An automation is never run twice by one time or one event.

## What a person sees

### The sidebar button

At the sidebar's foot, a new **Automations** button between Requests and Git (*the orchestrator's call*: beside Git,
Requests and Agents; *the designer's call*: just above Git, the card's "near Git"): the same 40 px tile as
`agents-button`, gpui-kit's `IconName::AlarmClock` (*the designer's call*: a clock that goes off; `Clock` is already
the schedules' chip and Git's History tab), a tooltip "Automations". A click toggles the popup; the pressed fill while
it is open.

**The badge** (*the designer's call*) counts the automations whose last run failed, was skipped or was missed since the
person last opened the popup, in the terminal palette's red, as the Requests badge is drawn; opening the popup clears
it; "99+" past 99; it never pulses.

### The shortcut

A new configurable command, `automations`, "Show automations" in Settings ▸ Shortcuts: **Ctrl+Shift+J**
(**Cmd+Shift+J** on macOS) toggles the popup (*the designer's call*: free in Templum's defaults and in gpui-kit's
editor and fields; J for jobs). It works with the sidebar shown or hidden.

### The popup

Drawn as every popup is ([design-system.md](design-system.md#popups), card 217): the same place (the canvas's bottom
left, beside the sidebar), size (`git_popup_size`, resized by its right and top edges like Git's), frame, header,
full screen and closing; window chrome, never scaled by the zoom. A press outside it closes it, as Esc does. Only one
of Git, Requests, Agents and Automations is open at a time: opening one closes the others.

Under the header, two columns (*the orchestrator's call*): the list at the left (340 px, as the Agents list), the
selected automation's editor at the right.

### The list

- **The header**, across the popup (card 217): the AlarmClock tile, "Automations" over how many this computer has
  ("3 automations", "No automations yet"), a tinted **New** button with a plus, Full screen, and Close "Close (Esc)".
  The list starts right under it.
- **A row** (*the orchestrator's call* for what it says; *the designer's call* for where), two lines:
  1. its switch (on/off); its name (or its sentence); at the right its next run: "17:00" today, "Tomorrow 09:00",
     "Mon 09:00" within a week, "3 Nov 09:00", "3 Jan 2027 09:00" in another year; "Off" when off; batch 2, the event
     it waits for ("on abc12's turn's end", "on a failure in q3x9z").
  2. dim: its When in words; at the right its last run: "Ran 16:00", "Skipped 16:00", "Failed Yesterday 16:00",
     "Missed 24 Oct 17:00", "Waiting for abc12" while held, nothing before its first run. Failed in the palette's red,
     Skipped and Missed and Waiting in its yellow.
  - Target missing: the row drawn as an off one, line 2 "Target missing: abc12 isn't open" in yellow.
  - While the pointer is over a row, and on the selected row, three icon buttons at its right, each with its name as
    its tooltip: **Run now** (`IconName::Play`), **Duplicate** (`CopyPlus`), **Delete** (`Trash`) (*the designer's
    call*).
- **Run now** runs its Do now, also when it is off, through every check above (held, failed, skipped as any run);
  it doesn't move its next time and doesn't count toward Stop after (*the designer's call*). Batch 2: a Do on "the
  agent that fired it" has no one to act on: Run now is dimmed, its tooltip "It acts on the agent that fires it".
  *The designer's call:* a click on it does nothing; Run now of any other event automation runs its Do with no agent
  that fired it (a new block at the view's middle, a notification whose click opens the popup).
- **Duplicate** adds a copy after the list's last, named "*name* (copy)", **off**, with no runs and no last run, and
  selects it (*the designer's call*: off, so two copies never run together by surprise).
- **Delete** asks first (*the orchestrator's call*): "Delete "*name*"?", "It won't run again. This can't be undone.",
  **Delete** (*the designer's call* on the words).
- **The order** is the list's (new last). Clicking a row selects it and opens it in the editor.
- **Empty:** "No automations yet", and under it, dim: "Type a prompt, run a command, start an agent or notify you at a
  time you choose." (batch 2: "…, or when an agent finishes.") (*the designer's call*).

### Asked by agents

Below the person's automations, a section **Asked by agents** (*the orchestrator's call*), only while some agent's
schedule exists: one row per live `schedule` of any agent, oldest first, read-only, not selectable (*the designer's
call*):

1. "s3 from Claude Code · api (b7k2m) to k9x1d" (to "itself" for its own block; the Templum Agent as "Templum
   Agent");
2. dim: "every 10 min · next 15:10 · " and its text's first line, or "once · 15:10 · …";

and a **Cancel** button (tooltip "Cancel s3"), which ends it as its block's menu does: no notice to its agent.

### The editor

The selected automation, top to bottom (*the orchestrator's call*: a form with a When picker and a Do picker, its
sentence and its next run; *the designer's call* for the rest):

- **Name**, a field whose placeholder is the preview sentence.
- **When**, a dropdown: Once, Every, Every day, On weekdays, On days of the month, Advanced (cron); batch 2 adds When
  an agent finishes its turn, When an agent needs you, When a command ends. Under it its fields:
  - Once: a date (`YYYY-MM-DD`) and a time (`HH:MM`, 24-hour, as `schedule`'s `at`); new: today at the next full hour.
  - Every: a number and a unit dropdown (minutes, hours).
  - Every day: a time.
  - On weekdays: seven toggles, Mon to Sun, and a time; new: Monday to Friday.
  - On days of the month: 31 toggles in rows of seven, a time, and "29, 30 and 31 run on the last day of shorter
    months."
  - Advanced: a field "minute hour day month weekday", and under it its next three times or its error.
  - Batch 2: who (a block, several with Any of them / All of them, or any agent of a kind; the blocks listed by
    title and uid, the kinds by Settings ▸ Agents' titles); a terminal and "Failures only". *The designer's call:*
    **Who**, a dropdown: **Chosen agents**, a toggle per agent block ("*Title* (*uid*) · *workspace*", a chosen one
    that isn't open as "abc12 (not open)") and, once two are chosen for a turn's end, a dropdown **Any of them** /
    **All of them**; or **Any agent of a kind**, a dropdown of Settings ▸ Agents' titles. A new agent event starts
    with no block chosen; Save says "Choose at least one agent." (or "Choose a kind of agent." with none in
    Settings ▸ Agents). A command ends: **Terminal**, a dropdown of the workspace's terminals ("Choose a terminal."
    at Save), and a toggle **Failures only**, off when new. Switching between the two agent events keeps who (All of
    them becomes Any of them for Needs you).
- **Do**, a dropdown: Send a prompt to an agent, Run a command, Start an agent, Notify me. Under it its fields:
  - Send a prompt: **Agent**, a dropdown of the blocks running an agent in its workspace (in all, for All workspaces),
    "*Title* (*uid*) · *workspace*" (batch 2: first, "The agent that fired it", for an agent event); a saved block
    that isn't open stays chosen, "abc12 (not open)"; **Prompt**, a field of several lines.
  - Run a command: **In**, "A new terminal" (with **Folder**, its placeholder the workspace's folder) or "A terminal"
    (with a dropdown of its workspace's terminals); **Command**, one line.
  - Start an agent: **Agent**, a dropdown of Settings ▸ Agents by title, accounts included; **Folder**; **Model** and
    **Effort**, fields (empty: the agent's own); **Prompt**, several lines, optional.
  - Notify me: **Text**.
  - Batch 2: the loop warning, under the Do (*the designer's call*: wrapped on as many lines as it takes, never cut).
- **Where**, a dropdown: All workspaces, then the workspaces in the sidebar's order (only workspaces for a new
  terminal or Start an agent); new: the workspace shown.
- **Stop after** (a number field, empty for no limit, "runs"); batch 2, for an event, **Cooldown** ("minutes").
- **The preview:** its sentence, and "Next: Tomorrow 09:00" ("Next: never, it is off"; "Target missing: abc12 isn't
  open"; batch 2: "Runs when abc12 finishes its turn").
- **Last run:** "Failed Yesterday 16:00: abc12 runs no agent now: a prompt is never typed into a shell." in full, once
  there is one.
- **The foot:** **Save** and **Revert**. Edits are a draft until saved (*the designer's call*: a half-typed prompt
  must never run): the row shows a dot while it has one, and the draft stays while the person selects another row or
  closes the popup, until Save, Revert or a restart. Save checks the draft: a problem shows in red above the buttons
  ("Choose an agent.", "Choose a terminal.", "Write the prompt.", "Write the command.", "Write the text.", "Choose at
  least one weekday.", "Choose at least one day.", "Every runs from 1 minute to 7 days.", "Write the time as HH:MM.",
  "Write the date as YYYY-MM-DD.", "That time has passed.", "It would never run.", the cron's own error, "Choose the
  workspace it opens in.", what `spawn_agent` says of the model or the effort, "A name is at most 60 characters.") and
  nothing is saved. A saved automation starts counting from that moment (`since`), its
  runs at 0, its held run dropped. **New** adds an unsaved draft at the list's end ("New automation", selected, with
  Every day at 09:00 and Notify me); Save adds it, **on**; Revert removes it.

### Keys

Opening the popup gives it the keyboard (*the designer's call*): ↑ and ↓ select the previous and the next row of the
person's automations; Esc in a field gives the keyboard back to the list, Esc on the list closes the popup; a press
outside the popup closes it, and what is under the press doesn't get it (card 217). The workspace's shortcuts work while it has the
keyboard, but those that act on the terminal in front, as in the Agents popup; the Automations shortcut closes it.
Closing gives the keyboard back to the canvas only if the popup had it.

## On a computer that mirrors another

*(Templum to Templum, batch 2h; the person's hard rule.)* **Home's automations run only at home.** A mirror never
copies a home rule into its own `automations.json`, never schedules, runs or fires one; it shows and edits them only
through home's API ([templum-to-templum.md](templum-to-templum.md)). Its own rules never list or target a remote
workspace:

- **Where** lists this computer's workspaces only (never a mirror), and a new rule starts on the workspace shown when
  it is local, else on the first local one. A rule's Who and terminal lists skip a remote workspace's blocks.
- A rule saved before for a workspace that is now a mirror stays in the file and in the list, with its Where
  labelled "API (mirror of winx-desktop)". Every way it could run (its time, Run now, a time missed at start that then
  falls due) is **Skipped**: "Target missing: API is a mirror of winx-desktop: its automations run there.", and its Do
  never runs. Save refuses a rule whose Where is a mirror with "API is a mirror of winx-desktop: its automations run
  there." (in the editor and through this computer's own `save_automation`).
- Its event rules never hear a remote agent, nor a notice of home's. Home's schedules (agents' deliveries) are never
  delivered by the mirror.
- Notify me also sends a notice through the API (`automation` names the rule), so a mirror can say it.

## Constraints

- CLAUDE.md: only crates/templum depends on the UI framework; no speculative structure (batch 1 adds no event field);
  every logic change ships a test that fails without it: pure unit tests for the clock (next times, cron, the words,
  daylight saving through a test time zone) and GPUI headless tests in crates/templum/src/workspace/tests/
  automations.rs for the runs, the file and the popup; tests never depend on the person's HOME, config or installed
  agent CLIs (CI has none); the editor's look: say how verified.
- **No new crate** (*the orchestrator's call*; cron parsed by hand). chrono's `serde` feature (a feature of a crate
  already used, pulling nothing new) saves the dates and times.
- The schedules' daylight-saving rule (`schedules::next_at`) and the automations' are one function.
- **Performance:** closed, the popup costs nothing; the runtime is one look a second over the automations that are on
  (nothing with none), with no frame asked unless something changed. The pull request runs `cargo run --release -p
  templum -- --bench` and `--bench-mixed` and pastes their JSON lines; the bar is `p95_ms` ≤ 16.7.
- Theme tokens and the terminal palette only for colors; nothing animates.
- Minors deferred go to docs/minors.md under their area; docs lines ≤ 120 columns.
- Specs to update with each batch: workspaces.md (the sidebar's foot), config.md (`automations` in `[keys]`),
  agent-tools.md (`schedule`: the person sees and cancels them in Automations too), docs/guide.md (the Automations
  button), ARCHITECTURE.md (the new modules and `automations.json`), this file's Status and As built.

## Batches

- **Batch 1 (the plan):** everything above but what is marked batch 2: the file, the time triggers with cron, the
  four Do's, held runs, Stop after, missed and asleep, Target missing, the popup with its list, editor, Asked by
  agents, badge, button and shortcut.
- **Batch 2:** the event triggers (an agent ends its turn or needs you; who: a block, several any/all, a kind; a
  command ends, failures only), the cooldown, the loop warning, "the agent that fired it" (as a Do's target, new blocks
  beside it, the notification's click, Run now dimmed), and their words in the list and the editor. Its plan,
  [2026-10-24-automations-events.md](../plans/2026-10-24-automations-events.md), follows batch 1's As built; the
  details the sections above left open are marked *the designer's call* in place (Cooldown; Events; The agent that
  fired it; Start an agent; Notify me; Safety; the list's Run now; the editor's who; At the edges).

## Left to the builder

Sizes beyond the list's width, paddings, the rows' layout and truncation, the toggles' and dropdowns' gpui-kit
controls (a date picker may replace the date field), which key handler takes ↑ ↓ Esc, how the form's
fields are kept, the constants' and functions' names, the exact words of the empty states and tooltips not quoted here.

## As built

### Batch 1

What the builder chose where the spec left it open, and what differs:

- **Code:** `crates/templum/src/automations/` (`mod.rs` the rules and their file, `clock.rs` the times),
  `workspace/automations.rs` (the look, the Do's, the popup's state and the drafts),
  `automations_view/{mod.rs,editor.rs}` (the popup and the form). `schedules::next_at` resolves its moment through
  `clock::resolve`: one daylight-saving rule.
- **`Held` has a `counts` field** (a Run now that is held must not count when it finally runs; a due time that joins
  it makes it count), and the method that ends a run is `run_ended` (`ended` is taken by `delegate.rs`).
- **A rule saved with no `since`** (a hand-edited file) counts from the start of that session, and a run that
  is turned on again counts from then.
- **Drafts** are compared with the saved rule on the fields the editor changes (name, workspace, When, Do, Stop
  after): a draft typed back to what is saved is no draft, and loading the form makes none (`set_value` emits no
  change). The form keeps its text boxes in `Workspace::automation_form`; `prepare_automation_form` fills them before
  a frame, for the rule selected.
- **The editor's controls:** gpui-kit `Input`/`Textarea`, `settings::row::menu_button` for every dropdown (now
  `pub(crate)`), and plain toggles drawn as the Agents chips for the weekdays and the days of the month. The date of
  a Once is a `YYYY-MM-DD` field and the model and the effort are free text (see docs/minors.md). A box the form
  can't read (a time, a date, a number) keeps the draft as it was and says so above Save at once.
- **The popup** has the keyboard on its own focus handle: ↑ ↓ act while that handle (not a field) holds it; Esc in a
  field returns the keyboard to it, Esc on it closes the popup. The row's three buttons are drawn over its right side
  while the pointer is on it (`group_hover`) and always on the selected row.
- **Asked by agents** is `Workspace::schedule_rows()` (no `cx`): the asker's block as the Agents view names it, "to
  itself" for its own block.
- **A command in a terminal** is held while an agent runs there, a command runs there (shell integration's
  `running`, or a program in front of its shell) or the person pressed a key in the last 3 seconds; a draft of theirs
  that Templum can't see is the minor in docs/minors.md.
- **The look runs only while something is on** (a rule on, or a run held by hand), as the schedules' does; it is
  started by the first rule turned on, saved or run by hand, and by a start with rules on.
- **A run whose answer is still on its way** when its rule is saved or turned off writes only `last` (and the badge
  when it went wrong): the new rule counts from its save, and a time of it that falls due meanwhile runs at the next
  look. Saving another kind, terminal or folder forgets the terminal the old one kept (`opened`); another command in
  it keeps it.
- **Looked at:** the popup's look (spacing, hover buttons, the form) was checked by the headless tests that draw
  it (its elements, its widths); no display was at hand to see it by eye, so a person should open it once.

### Batch 2

What the builder chose where the plan left it open, and what differs:

- **Code:** no new module. `automations/mod.rs` has `Heard`, `Who`, `When::Agent`, `When::Command`,
  `Automation.cooldown`, `event_words` and `event_short`; `workspace/agents_view.rs` `Watch::see` returns what it
  heard and `watch_agents` queues it in `Workspace::automation_heard` (only while an event rule is on);
  `workspace/automations.rs` acts on the queue and on each command rule's terminal count at the look (`events_due`,
  `fire_event`, `look_slot`).
- **Slots:** `Run` keeps `slots: BTreeMap<Option<String>, Slot>` (None a time's or Run now's, Some(uid) the block that
  fired an event) with `held`, `typing` and `typing_counts` moved onto `Slot`, `cooled` (the cooldown's stamps by
  block, "" for All of them) and `finished` (a command rule's count). `Run::reset` is what turning a rule on and
  saving it do: held runs dropped, runs being typed no longer count, cooldowns and the count start over. A slot
  that isn't held or typing is not kept, so an idle rule has none.
- **The agent that fired it** is swapped into a copy of the rule (`for_firer`) before a run; `automation_target`
  checks the rule as saved (its blocks, the word `fired-it` only for an agent event) and then the copy (the firing
  block is open). Every check of batch 1 (busy, never into a shell, Target missing) applies unchanged.
- **Joining:** inside the cooldown an event does nothing, so a held run joins an event only when the cooldown has
  passed (a cooldown of 0, or a hold longer than it): as the spec says, it then starts no new cooldown.
- **All of them** counts a block that isn't open, runs no agent, or is Idle or Starting as resting; the last of the
  set heard ending in a look is the agent that fired it. A set of one block, whatever its flag, is that block.
- **Several events of one block in a look** (it ended twice between two looks) fire once.
- **Run now** of an event rule runs its Do with no agent that fired it (new blocks at the view's middle, a
  notification whose click opens the popup); for a prompt to the agent that fired it the function returns at once,
  and the list's button is dimmed with no click.
- **The editor:** the Who toggles are `automation-who-<uid>`, the set's dropdown `automation-who-all`, a kind's
  `automation-kind`, a command's terminal `automation-event-terminal` and `automation-failures`; the loop warning is
  `automation-loop`. A toggle's label is clipped (`block_label` already cuts the title and the workspace) and the
  weekday and month toggles use the same clipped helper. The empty list's words are the constant
  `automations_view::EMPTY_HINT`.
- **Looked at:** as in batch 1, the headless tests that draw the popup check the elements and widths (the Who toggles
  and the loop warning are drawn, a long kind title keeps the row and its next column inside their widths); no
  display was at hand to see the spacing, the toggles' wrap or the dimmed Run now by eye, so a person should open it
  once. The notification's click is one line (`open_fired`, tested) with no headless click on a notification.

### The API (card 171)

- `save_automation`, `set_automation`, `run_automation` and `delete_automation` are tools only the API serves
  ([api.md](api.md)); agents still have none (the non-goal stands). A save goes through `save_rule`, the editor's Save
  with its checks and words; a draft the person has open is left alone. A token limited to some workspaces saves rules
  for them only, and a rule that runs a command takes a Full token. The state shows each rule's `when` and `do` as the
  file keeps them, with its next run, its last run and its sentence.
