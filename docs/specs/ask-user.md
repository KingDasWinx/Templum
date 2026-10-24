# Questions from agents: `ask_user` v2

- **Date:** 2026-10-17
- **Status:** Implemented (kanban card 34, "Correções ask_user"; card 70 made a click only pick, Send answers; card 81
  redrew the dialog in the language of the kanban card's dialog and the Git popup; card 115 shows several questions
  one at a time under tabs; card 144 moved the dialog from a balloon on the asking terminal into the requests inbox,
  [ask-inbox.md](ask-inbox.md)); plan: [2026-10-17-ask-user-v2.md](../plans/2026-10-17-ask-user-v2.md)
- **Builds on:** [connections.md](connections.md#requests-from-agents) (the inbox, the tools, the notices typed
  back), [settings.md](settings.md), [shell-integration.md](shell-integration.md#news-and-notifications) (alerts and
  notifications)

## Goal

An agent that asks the person something through `ask_user` gets less than Claude Code's own `AskUserQuestion`: one
question, buttons or a text field but never both, a bare string back. The person, meanwhile, misses questions: they
expire after 30 minutes and make no sound. This brings `ask_user` close to `AskUserQuestion`, whose shape agents
already know (several questions in one dialog, a header, options with a description and a preview, checkboxes, an
Other field, a note, an answer per question), keeps what Templum's has and the native one lacks (a timeout, the
dialog on the asking block, parallel calls, any number of options), lets a question wait until it is answered, and
tells the person with a sound and, when they are away, a notification and an alert.

**Success criteria**

1. Today's calls (`question`, `options` as strings, `timeout`; `templum ask`) show today's dialog, with Dismiss added,
   and get the answer as plain text: one of their options, or what was typed when they have none.
2. A call with `questions` shows them in one dialog, one at a time under a tab each: each with its header, its options
   (label, description; a checkbox when `multiSelect`), a preview beside the options, and an Other field; one note for
   the whole call. The answer comes back as JSON, an answer per question.
3. By default a question waits until the person answers or dismisses it; Settings ▸ Connections can set a limit.
4. An agent whose client stops waiting doesn't lose the person's answer: it is typed into its terminal.
5. A request makes a short sound; one the person doesn't see is a notification and an alert (the workspace's dot,
   Ctrl+Shift+A).
6. The dialog works with the keyboard, and scales like a photo at every zoom.

## Out of scope

- `connect`'s dialog and timeout (it only shares the sound).
- New flags for `templum ask`: it keeps today's shape (one question, `--option`).
- MCP's `structuredContent` and `outputSchema`: the JSON is the result's text. Add them when a client uses them.
- Progress notifications that keep a client's idle timer alive. Add them if a client gives up on a call despite the
  timeout Templum gives it (see [When the client stops waiting](#when-the-client-stops-waiting)).
- A note per question (one per call), a sound of Templum's own, choosing the sound.

## The tool

### Arguments

```json
{
  "questions": [
    {
      "question": "Which database should the service use?",
      "header": "Database",
      "options": [
        { "label": "Postgres", "description": "What production runs", "preview": "DATABASE_URL=postgres://…\npool = 10" },
        { "label": "SQLite", "description": "One file, no server" }
      ],
      "multiSelect": false
    },
    {
      "question": "Which features go in the first release?",
      "header": "Features",
      "options": [{ "label": "Auth" }, { "label": "Search" }, { "label": "Export" }],
      "multiSelect": true
    }
  ],
  "timeout": 600
}
```

- **`questions`**: one or more, no maximum (the dialog scrolls). Each has:
  - `question`, required and not blank; no two questions of a call share their text (the answer is keyed by it);
  - `header`, optional: a short label, shown up to 12 characters, a longer one cut with "…";
  - `options`, optional, any number; none leaves only the Other field. An option is `{ "label", "description",
    "preview" }` (only `label` required, not blank, unique within its question), or a plain string, its label;
  - `multiSelect`, false unless given.
- **`question`** with **`options`** (strings), today's shape: one single-select question, answered as plain text, in
  today's dialog: a button per option and no Other, or a field when it has none; no note. Scripts that check for one
  of their options keep working. A call with both `question` and `questions` is refused: "Give question or questions,
  not both."
- **`timeout`**: whole seconds from 1 to 3600, as every tool's; optional ([How long it waits](#how-long-it-waits)).
- A refusal names the place: "questions[1].options[0] needs a label", "questions[0] and questions[2] have the same
  text: the answers are keyed by it."
- The schema lists both shapes and requires neither, so a client that checks arguments against it before calling
  still sends today's calls. Option items are declared as objects only (no `anyOf`, which not every client's model
  API takes); strings are accepted all the same.
- The description, which agents read: "Asks the person in a dialog on your block, as AskUserQuestion does: questions,
  each with a header, options (label, description, preview) and multiSelect; every question also gets an Other field,
  and the person may add a note. Returns {"answers": {question: answer, or a list for multiSelect}, "note": …}. Waits
  until the person answers: no limit unless you give timeout (seconds) or the person set one; if your client stops
  waiting, the answer is typed into your terminal later. The older question with options (strings) still works and
  returns the answer as text."

### The answer

- **A `questions` call:** the result's text is one JSON object:
  `{"answers": {"Which database should the service use?": "SQLite", "Which features go in the first release?":
  ["Auth", "Export"]}, "note": "keep it small"}`. A single-select question's answer is a string; a multiSelect's is a
  list, in the options' order with Other's text last. Other's text is the answer as typed (trimmed). `note` is there
  only when the person wrote one.
- **A `question` call:** the option's label or the text typed, as today.
- **Dismissed:** an error result, "The person dismissed the question without answering."
- **Past its deadline:** as today, "No one answered within 600 seconds; the request was withdrawn."

### How long it waits

- **The setting** `[connections] question_timeout`: minutes, 0 to 1440, 0 by default, which waits until the person
  answers.
- **Which wins:** the shorter of the call's `timeout` and the setting (when not 0); with neither, there is no
  deadline. The call's timeout is the agent's own need ("answer within two minutes or I go on with the default"), the
  setting is the person's cap on how long questions hang around; taking the shorter honors both, and neither can make
  the other wait longer than it said. A change to the setting applies to questions asked after it.
- `connect` keeps its 1800-second default.

### When the client stops waiting

What each client does with a long tool call (read in their documentation, 2026-10-17):

| Client | A long call | What Templum does |
|---|---|---|
| Claude Code | Gives up on a call that sends nothing for 1800 s, its idle timeout for stdio servers (`CLAUDE_CODE_MCP_TOOL_IDLE_TIMEOUT`; seen 2026-10-17 with a Templum `mcp.json` without `timeout`: `MCP server "templum" tool "wait_for_idle" sent no response or progress for 1800s; aborting. If this server is configured in your MCP settings, set a per-server "timeout" (ms)…`). A server's own `timeout` lifts it (Claude Code v2.1.203 or later, the version that brought the idle timeout to stdio servers: before it they had none), and replaces `MCP_TOOL_TIMEOUT` (100000000 ms, about 28 hours, by default). Moves a call past 2 minutes to the background, where its documentation says it keeps waiting; not checked by hand yet ([minors](../minors.md#connections)). Esc in the agent cancels the call (`notifications/cancelled`) | The `mcp.json` it is given sets `"timeout": 604800000` on Templum's server |
| Codex | Gives up at the server's `tool_timeout_sec` | `tool_timeout_sec=604800` (it was 3600) |
| Gemini CLI | Gives up at the server's `timeout` (ms; 10 minutes unless set); the MCP TypeScript SDK sends `notifications/cancelled` when it does | The extension's `timeout` is 604800000 (it was 3600000); one linked before gets it at Templum's next start |
| Others (the command in Settings ▸ Connections) | Their own limits | The fallback below |

- Seven days is longer than any question is left in practice, and stays under the largest JavaScript timer (2³¹ − 1
  ms, 24.8 days; a longer one fires at once), which Claude Code and Gemini CLI run on.
- **A cancelled question** (the client gave up, or the person pressed Esc in the agent) stays in the dialog, which
  says under its first line: "The agent stopped waiting: your answer will be typed into its terminal." Answered, the
  answer is typed into the agent's terminal as a notice (`queue_notice`, the path of
  [connections.md's Delegating 10](connections.md#delegating): typed once the agent takes typing):
  `[answer to your question "Which database should the service use?"] The answer is in the pasted text.`, then on
  the next line `{"answers": …}`, the text the tool would have returned; the question is the first question's first
  line (at most 60 characters, cut with "…"). A long answer is a paste in Claude Code ("[Pasted text]"), which it
  takes for data unless typed text comes with it: the first line is that text, as delegate's notices. Dismiss drops
  it. Its deadline, if it has one, drops it silently: no one waits for the refusal.
- The connection ending (the agent quit, `templum ask` stopped with Ctrl+C) withdraws its questions, as today, and
  so does closing the block.
- Either way the person's answer reaches the agent: as the call's result while the client waits, typed in once it
  cancelled. Claude Code's 1800-second idle timeout is lifted by the server `timeout` Templum gives it, so a question
  waits there as long as the person takes; a Claude Code given no such `timeout` (opened by hand with its own MCP
  settings) gives up at 1800 s, and whether that abort sends a cancel isn't known.
- A client that drops a call without a cancel loses an answer that comes after it gave up; that happens only past its
  own timeout, which Templum sets to seven days where it can.

## The dialog

The request's part of the inbox of [ask-inbox.md](ask-inbox.md) holds the call's questions: the right column of its
popup, under where the request comes from and why it is asked. Since card 81 it is drawn in the language of the kanban
card's dialog ([kanban.md](kanban.md#the-open-card)) and the Git popup ([git.md](git.md)): the popup's color
(`ui.surface`) under hairlines, the theme's accent for what is lit, every color a theme token. Since card 144 it is
window chrome: sizes are pixels, never scaled by the canvas's zoom, and it was a balloon centered on the asking
terminal (80% of its width) before. It is **a body that scrolls and a foot that stays**, so a long question never
pushes Send out of reach:

```
┌──────────────────────────────────────────────────────────────────────────┐
│ FROM  Claude Code · api    WHY  Releasing 1.2: the answer picks the store │  inbox's
├──────────────────────────────────────────────────────────────────────────┤
│  (✓) Database    ( ) Features                                            │  tabs
│  ━━━━━━━━━━━━━                                                           │
│  ────────────────────────────────────────────────────────────────────    │
│  Which database should the service use?                                  │
│  ┌──────────────────────────────┐  Preview                               │
│  │ ◉ Postgres                   │  ┌─────────────────────────────────┐   │
│  │   What production runs       │  │ DATABASE_URL=postgres://…       │   │   body,
│  │ ○ SQLite                     │  │ pool = 10                       │   │   scrolls
│  │   One file, no server        │  └─────────────────────────────────┘   │
│  │ ○ Other [ Something else   ] │                                        │
│  └──────────────────────────────┘                                        │
│  ────────────────────────────────────────────────────────────────────    │
│  ▤ Note                                                         optional │
│  [ Add a note for the agent                                            ] │
├──────────────────────────────────────────────────────────────────────────┤
│ Dismiss                                        1 of 2 answered   [↑ Send] │  foot
└──────────────────────────────────────────────────────────────────────────┘
```

- **The inbox's own parts** (the list of requests, "From" and "Why", the notice on the terminal) are
  [ask-inbox.md](ask-inbox.md)'s. A request that waits has no header of its own any more: the list's row says who
  asks, what it is and for how long.
- **A cancelled question** says "The agent stopped waiting: your answer will be typed into its terminal." in a banner
  at the top of the scrolling body (a rounded 8 box of the waiting yellow at 12%, lucide `circle-alert`).
- **Body** (18 at the sides): what the request says scrolls under "From" and "Why", with gpui-component's scrollbar
  over it (the kanban column's, `kanban::scrollbar_over`: shown on hover and while scrolling, only once the body
  overflows) and the wheel over it. The foot stays under it, 51 high.
- **A small popup** keeps the dialog usable: the popup is at least the Git popup's smallest size (600 × 420 pixels)
  whatever the window, and the body scrolls, so every part is reachable by the wheel or the keys. The foot's buttons
  wrap on a second line in a column narrower than 260, Send on the right, so none is cut at the sides.
- **Tabs** (card 115): a call of several questions shows **one at a time**, under a row of tabs at the top of the
  body, one per question in its order, in the Git popup's look: each its header as its chip shows it (12 characters
  at most), or "Question 2" without one, in `title_dim` (the terminal's text color under the pointer), medium, the
  Git tabs' size; the shown one in the terminal's text color, underlined by 2 of the accent; a hairline under the row.
  Before each label a 14 ring (`title_dim`) **fills with the accent and a check** once its question has an answer
  (an option picked, or Other's text), so the person sees what is left. A click on a tab shows its question. The row
  wraps on more lines in a dialog too narrow for one, so no tab is ever cut; it lies in the body, so the small popup's
  rules hold with it unchanged. Another question
  shows from the body's top, where the tabs are. A call of one question (and today's shape, and a plan) shows no
  tab. The note, the foot and its count are the call's, under every tab; what is typed in a tab (an Other text, a
  free answer, the note) stays when another shows.
- **The question shown** starts with its header as a chip (the kanban card number's look: `ui.accent` on `ui.accent`
  at 16%, rounded 6, 11.5 semibold; not under tabs, which already name it), then its text, whole (14.5, medium, line
  height 1.4); a multiSelect's says "Pick any" under it, dim, with lucide `list-checks`.
- **Options** sit in a faint panel (`ui.card`, rounded 10, 4 inside), one row each: a radio (a 16 circle, a dot of the
  accent inside when picked) or a checkbox (a 16 rounded square, filled with the accent and a check when picked), the
  label (13.5) and the description under it (12, `title_dim`, line height 1.45). A row is padded 10 and 8, rounded
  8, `ui.hover` under the pointer or the keys' highlight; a picked one is tinted with the accent (more under the
  pointer or the highlight).
- **Other** is every question's last row, in the same panel: its radio or checkbox, "Other" and a field (placeholder
  "Something else"). Typing in the field picks it (in a single-select question, the option picked before is unpicked);
  emptying it unpicks it; picking an option of a single-select question unpicks Other, its text kept. A question
  without options shows the field alone, full width (placeholder "Your answer"), with no radio and no panel.
- **Text never gets cut.** Every text wraps in the dialog's width, a word longer than it (a path, a hash) by letters,
  and its container may shrink to it (`min_w_0`): long questions, long labels and long descriptions grow their rows,
  and the body scrolls. Many questions stay usable: one shows at a time under its tab, and the foot says "1 of 3
  answered".
- **Preview:** a question with an option that has a preview lays out in two columns when the request's column is at
  least 520 pixels wide (a wider Git-sized popup; the default popup's column is a little narrower), its panel on the
  left (45%), the preview on the right: the highlighted option's ([Keyboard](#keyboard); hovering an option
  highlights it), else the
  picked one's, else the first one's with a preview; an option without one shows "No preview" dim. In a narrower
  column the preview goes **under** the options, full width. Either way it has a caption ("Preview", lucide `eye`)
  and a frame (hairline, rounded 8, the canvas color) in the terminal's font at 12 pixels. **A long preview wraps** to
  the frame's width (a word longer than it by letters: no sideways scroll), **shows its first 12 lines** and, when
  it has more, a **Show more** link under the frame that shows the whole text (then **Show less**; `part(n,
  "q<q>-preview-more")`), for the request's previews together (`Wait::more`). The frame has no scroll of its own: the
  wheel over it scrolls the body.
- **Note:** a section header (lucide `sticky-note`, "Note", "optional" at the right, as the kanban card's sections)
  over one field (placeholder "Add a note for the agent"), after the questions and a hairline.
- **The foot** (a hairline over it, 18 at the sides, 10 up and down): **Dismiss** on the left, a ghost button (30
  high, rounded 8, `ui.hover` under the pointer); at the right **Send**, a solid accent button (30 high, rounded 8,
  `ui.surface` text, lucide `arrow-up`) once every question has an answer (an option picked, or Other's text), a faint
  `ui.hover` fill, dim and inert before, as the kanban's Comment; and left of it, when the call has several
  questions, how many have an answer ("1 of 3 answered").
- **A click only picks.** An option, the only one of a single question too, is picked or checked and nothing more:
  the person sends with Send, so a click by mistake answers nothing. Under tabs, a click that picks an option of a
  single-select question then shows the next question without an answer (wrapping), as Enter does, so a call is
  answered with one click per question and Send; a checkbox stays (several may be checked), and so does a call whose
  questions all have an answer. A wrong pick is still only a pick: its tab shows it again. (A plan's Approve and Reject,
  [agent-tools.md](agent-tools.md), are the decision itself and answer with their click.)
- **Today's shape** draws today's dialog in the same frame: its options as full-width buttons (no radio: a picked one is
  tinted and its label in the accent), a click picking one; or, with none, the field. No Other row, no note; Dismiss
  and Send in the foot as for any question (Send used to sit beside a lone field).
- **The other requests share the frame:** a plan ([agent-tools.md](agent-tools.md#propose_plan)) holds its chip, its
  frame and the Comment field in the body and **Reject** (ghost) and **Approve** (solid, lucide `check`) in the foot;
  a request to connect or another with two buttons holds its words in the body and **Deny** (ghost) and **Allow**
  (solid) at the right of the foot.

### Calls made for card 81

The card, as the person wrote it: *"Refatoração modal de ask_user: atualmente está péssimo, não está responsivo para
textos gigantes, o texto fica cortado, layout com aspecto antigo; quero um layout mais moderno igual refatoramos o
modal de create card do kanban e o modal de git."* The redesign changes no behavior: the same ids (`q<q>-option-<i>`,
`q<q>-other`, `note`, `send`, `dismiss`, `allow`, `deny`, `stopped`, `plan-*`; `count` and `waiting`, the balloon's
header, went with it, and `("balloon", n)` became `inbox`), the same rows and keys, the same answers (card 115
changed the rows: the shown question's).

1. **A foot outside the scroll,** not one dialog that scrolls whole: the old balloon scrolled its first
   line and its buttons away with the text (two minors, deleted). The request is a column; its body is the part that
   shrinks (`min_h_0`) and scrolls, with the scrollbar the kanban's columns have.
2. **The same frame for a question, a plan and a request with two buttons,** so the three look like one dialog; each
   kind only fills the body and the foot (`Parts`).
3. **The preview goes under the options in a narrow dialog.** Two columns in a 320-wide dialog left each about 130
   wide; the layout depends on the column's width, so it never changes with the zoom.
4. **The wheel over a preview goes on to the body.** The preview always took the wheel, so over a short one
   (most of them) the dialog stopped scrolling under the pointer; since card 144 a preview has no scroll at all.
5. **"1 of 3 answered" in the foot** with several questions, so a person who scrolled past a question can tell why
   Send is faint.
6. ~~A block that isn't the one worked in~~ pans no more: the dialog isn't on a block any more.

### Calls made for card 115

The card, as the person wrote it: *"ask_user em sistema de tabs: atualmente está em scroll, mas quero uma abinha que
pode mudar entre as perguntas."* The answers, Other, the note, previews, multiSelect, plans and requests with two
buttons don't change; the ids tests use stay, with `tab-<q>` and `tab-<q>-answered` added.

1. **The tabs are in the body, not fixed over it.** Fixed, their height (more when they wrap) would eat the
   body's room in a small popup; in the body they scroll with it. Another question shows from the top, where they
   are.
2. **They wrap, rather than scroll sideways:** every tab stays in sight and clickable, and a header is 12 characters
   at most, so a row wraps only with many questions or a narrow block.
3. **← and → turn them; Ctrl+Tab doesn't:** it is the next workspace's shortcut, which the dialog leaves running.
   In a field the arrows stay the field's, also at the start or the end of its text, where they do nothing.
4. **A click on a single-select option moves on** to the next question without an answer, as Enter already did:
   without it, tabs would cost a click on the next tab per question. A checkbox doesn't (the person checks several),
   nor Space (it only picks).
5. **The keys' rows are the shown question's**, so ↓ and Tab never land on a hidden row; the next question is reached
   with →, or by Enter and a pick moving on.

### Keyboard

- The dialog has the keyboard while the **inbox** does: opening the inbox (its sidebar button, the notice on the asking
  terminal, Ctrl+Shift+A: all the person's own acts) gives it the keyboard, and so does a click in it; a click on the
  canvas or a block gives the keyboard to what was clicked, and the inbox stays open beside it. A request that
  arrives never takes the keyboard. While the inbox has it no key reaches a terminal; shortcuts still run (but the
  ones that act on the terminal in front: copy, paste, close, find, the prompts), and the theme picker's keys are the
  picker's while it is open.
- **The highlight** is on one row (an option, an Other row, the note or Send) or on none, as the dialog opens. Its
  rows are the shown question's, the note and Send. ↓ and Tab put it on the first row, ↑ and Shift+Tab on Send, then
  move it; hovering a row puts it there. With no row
  highlighted, Space and Enter do nothing: a key meant for the terminal while the pointer drifts over the block
  can't answer. A move by the keys (not the pointer, nor the wheel) scrolls the body just enough to show the row,
  once, when it is drawn: a wheel that scrolls it away after isn't fought.
- **← and →** show the previous and the next question under tabs, wrapping, while no field has the keyboard (in a
  field they move in its text, and do nothing at its start or its end). A highlight on the question left goes to the
  first row of the one shown; on the note or Send it stays. Not Ctrl+Tab: it switches workspaces, and shortcuts still
  run while the dialog has the keys.
- **Space** picks the highlighted option (single-select) or checks and unchecks it (multiSelect).
- **Enter** on a single-select question's option that isn't picked picks it, and sends nothing: the highlight goes to
  the next question without an answer (wrapping to the first; under tabs, its tab shows) or, when that was the
  last, to Send. Anywhere else (an option already picked, an Other field, the note, Send) it sends once every
  question has an answer, and moves the highlight to the next question without one before (the shown one first,
  from the note or Send). So a question without options sends what was typed on
  Enter, as today; a multiSelect's options are checked with Space, never Enter.
- On an Other row or the note, the field has the keyboard (GPUI's focus) and types; Tab and Shift+Tab, and ↑ and ↓
  where the field lets them through, move on.
- **Alt+↑ and Alt+↓** select the previous and the next request of the inbox's list.
- **Esc** closes the inbox, also from a field, and gives the keyboard back to the canvas.
- **Presses too soon:** keys and clicks on its options, Send and Dismiss are ignored for 0.5 s after the inbox first
  draws a request (it is selected, or the one before it is answered): someone typing or clicking as it appears
  doesn't answer it.
- A request to connect keeps its buttons and takes no keys.

## Telling the person

- **Notification:** when the person doesn't see the request (it is in another workspace than the one shown, or
  the window is in the background), a notification says "Asks you: *the first question*", with "(and 2 more)" when
  the call has more; the system's too in the background, when `terminal.notifications` is on. A click shows that
  workspace with its inbox open at the request. A request in the workspace shown, the window in front, makes none:
  the sidebar button's badge and pulse tell it ([ask-inbox.md](ask-inbox.md)).
- **Alert:** in that same case the asking block gets an alert, "Asks you: …", as a bell gives one: its workspace's
  tile shows the yellow dot. Ctrl+Shift+A (`go_to_waiting`) opens the inbox when the workspace shown has a request
  waiting; else it goes to the terminal that asked for attention, so a person coming back finds the question.
  Seeing the block clears the alert, as any alert's. No second notification comes from it.
- **Sound:** a short system sound when a question or a request to connect arrives, seen or not, unless
  `[connections] sound = false`; at most one every 2 seconds, so parallel calls don't stack. The platform's own
  player plays it in a background thread; one missing or failing is silence, never an error:
  - Linux and the BSDs: `canberra-gtk-play -i message-new-instant -d Templum` (the desktop's sound theme, which
    honors its switch for event sounds); when it is missing or fails, `pw-play`, then `paplay`, with
    `/usr/share/sounds/freedesktop/stereo/message-new-instant.oga` when that file exists.
  - macOS: `afplay /System/Library/Sounds/Glass.aiff`.
  - Windows: `powershell -NoProfile -NonInteractive -Command "[System.Media.SystemSounds]::Asterisk.Play();
    Start-Sleep -Milliseconds 1500"` with `CREATE_NO_WINDOW`, as Templum's other commands (the user's sound scheme;
    the sleep lets the sound finish before PowerShell exits).
  - **No audio crate:** GPUI's system notifications are silent everywhere (gpui-fast-linux's notify-rust call sets
    no sound hint, its Windows toast is `silent`, its macOS content sets no sound), and an audio crate (rodio, through
    cpal) would add ALSA's headers to every Linux build and libasound to the packages, for one chime.

## Settings

Settings ▸ Connections, two rows after the switch for the agents Templum opens:

| Field | Control | Option |
|---|---|---|
| Questions wait | Number 0–1440 minutes, step 5; its description: "How long an agent's question waits for your answer, in minutes. 0 waits until you answer" | `connections.question_timeout` |
| Sound | Switch, on: "A short sound when an agent asks you something" | `connections.sound` |

`config.toml`'s template:

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

A `question_timeout` outside 0–1440 is an invalid file, as any bad value is.

## Architecture

| File | Change |
|---|---|
| `templum-mcp/src/ask.rs` (new) | `Ask` (the questions, and whether the call used `questions`; whether it has tabs, and each tab's name), `Question`, `Choice`; parsing both shapes; the answer's text (JSON or plain), the notice's; `Form`: what is picked, the question shown (`tab`: `show`, `turn`), the rows, the highlight, answered or not, how many answered, the next question without an answer. Headless, unit-tested |
| `templum-mcp/src/tools.rs` | the schema and description; `Call::AskUser { ask, timeout: Option<u64> }` |
| `templum-mcp/src/lib.rs` | Codex's `tool_timeout_sec=604800` |
| `templum-usage/src/claude.rs`, `gemini.rs` | the server's `timeout`, 604800000 ms |
| `templum/src/config.rs`, `settings/connections.rs`, `settings/fields.rs` | `question_timeout`, `sound`; their rows |
| `templum/src/workspace/waits.rs` | the deadline from the setting; a cancel keeps a question; the front-of-queue time |
| `templum/src/workspace/questions.rs` (new) | a question's fields (one per question's Other, the note), picks, Send, Dismiss, keys, the answer typed back once cancelled, the alert |
| `templum/src/workspace/inbox.rs`, `templum/src/inbox.rs` | the inbox's keys before the workspace's (`on_inbox_key`), its popup and the notice on a terminal ([ask-inbox.md](ask-inbox.md)) |
| `templum/src/balloon.rs` | the request: the banner, the scrolling body and the foot, and what each kind puts in it (`question`, its `tabs`, `plan`, `allow`) |
| `templum/src/workspace/waits.rs`, `markdown.rs`, `kanban.rs` | `Wait::body`, the body's scroll; `PreviewScroll::follow`; `scrollbar_over`, the kanban column's scrollbar shared |
| `templum/src/chime.rs` (new) | the platform's player |

No new dependency.

## Testing

- **Unit (`templum-mcp`):** both shapes parse; strings and objects as options; each refusal (blank, duplicate texts or
  labels, both shapes, a bad `multiSelect` or `timeout`); a long header cut; the JSON answer (string, list in option
  order with Other last, note only when written) and the plain one; the notice's line; `Form`: picking in single and
  multi, Other typed and emptied, the rows of each shape, the highlight from none and its moves and wrap, the next
  question without an answer, answered or not; Codex's flag, the `mcp.json` and Gemini's extension
  carry seven days.
- **App (`workspace/tests/requests.rs`, `config.rs`, `settings/tests.rs`):** today's call still draws today's dialog
  and answers its label and its typed text; a `questions` call answers its JSON through clicks, checkboxes, Other and the note; Send stays off
  until every question has an answer; a click on an option, even of the only single-select question, picks and
  Send answers; Dismiss refuses; no deadline by
  default, the setting's or the call's, the shorter; a cancelled question stays, says so, and its answer is typed as a
  notice; the connection's end still withdraws it; the dialog's keys (moves, Space, Enter, Tab into a field, Esc and
  the inbox closing and the keyboard going back to the canvas) and none reaching the terminal; Enter with no row
  highlighted; presses within 0.5 s ignored; the preview of the hovered option; the
  notification's "(and N more)"; the alert when unseen and none when seen; a sound is due once per 2 seconds, none
  with the setting off; the two settings' defaults, limits and rows.
- **The redesign (card 81, `workspace/tests/requests.rs`):** a question of thousands of words and thirty options keeps
  the popup within its size with the foot where it was while the body scrolls,
  and an option far down is reached, picked and sent; a word longer than the dialog wraps inside it (the content
  never grows past the body); the preview goes beside the options in a wide dialog and under them in a narrow one; the
  foot counts the answers of several questions and says nothing for one; the
  wheel over a preview, short or tall, scrolls the body. `templum-mcp`: `Form::answered`.
- **Tabs (card 115, `templum-mcp` and `workspace/tests/requests.rs`):** the rows are the shown question's; showing a
  question moves a highlight off the hidden one, none and the note's staying; turning wraps both ways; only several
  questions have tabs, each named by its header or "Question N"; the next question without an answer starts after
  the shown one, or from it. In the app: one question shows at a time and a click on a tab shows another, one
  question shows no tab; a tab's mark follows its answer; Other texts, a free answer and the note stay across tabs and
  Send answers the JSON; ← → turn the tabs, wrapping, and stay the field's in a field, at its start and its end too;
  a click on a single-select option shows the next unanswered question, a checkbox stays; another question shows from
  the body's top, and ← → on one question leave the scroll alone; Enter moving on shows its tab; tabs wrap inside the
  body of a narrow dialog and every tab, option and Send is reached in the smallest popup.
- **The inbox (card 144, `workspace/tests/inbox.rs`):** the requests of [ask-inbox.md](ask-inbox.md), answered by
  clicks and by keys from the popup; a long preview wraps, is cut at 12 lines and shows the rest on Show more.
- **By hand:** Claude Code, Codex and Gemini CLI each asking with `questions` and getting the JSON; a question left 3
  minutes in Claude Code (is the call cancelled once backgrounded? the answer reaches it either way); Esc in Claude Code while it asks, then an answer typed into
  it; the sound on Linux (with and without `canberra-gtk-play`), macOS and Windows; the inbox with a dozen requests
  open.
- **Bench:** the dialog paints, so the pull request carries `--bench` and `--bench-mixed`.

## Risks

| Risk | What we do |
|---|---|
| A key or click meant for the terminal answers a question that just appeared | Presses within 0.5 s of it showing are ignored; no row is highlighted until an arrow, Tab or the pointer puts one there; Enter sends only once every question has an answer |
| A question with no deadline covers its terminal for good | Dismiss; Esc gives the keyboard back; the agent's own Esc keeps it but says the agent stopped waiting |
| An answer typed into an agent that moved on confuses it | The notice names the question it answers; it is typed only once the agent takes typing (batch 8's rule) |
| A client gives up without a cancel | Seven-day timeouts where Templum sets them; others are documented as the fallback's limit |
| No player on the system | Silence, as a muted desktop; the notification and alert still tell |
| A JavaScript timer past 2³¹ ms fires at once | Seven days (604800000 ms) |
