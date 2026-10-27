# Templum Agent: a modern look

- **Date:** 2026-10-24
- **Status:** Implemented (kanban card 143, "a visual refactor of the Templum Agent"), uncommitted until reviewed;
  plan: [docs/plans/2026-10-24-templum-agent-look.md](../plans/2026-10-24-templum-agent-look.md)
- **Builds on:** [templum-agent.md](templum-agent.md) (what the pill, the chat, its rows and the approval do: all of it
  stays), [design-system.md](design-system.md), [polish.md](polish.md) and [themes.md](themes.md) (the tokens and the
  motion rules)
- **Comes before:** kanban card 46 (Templum Agent batches 3 and 4), whose new UI is built from the pieces below
  ([For card 46](#for-card-46))

The person asked, in their words: "Do a general visual refactor to make the Templum Agent look better. I want a
modern design. Take care of UX and UI. See when it's best to do this task: before or after the other Templum Agent
tasks." Then: "pode decidir" (decide it). The product calls are the orchestrator's, marked *the orchestrator's call*;
the details are the designer's, marked *the designer's call*. Each can be changed.

## Intent

The Templum Agent works, but its chat looks like a debug log: every row is the same small dim text, the agent's
answers read at 60% of the text color, the header holds seven controls, the model shows twice, and a code block or a
long turn has no shape. This card gives it the look of a current AI chat (Claude.ai, ChatGPT, Raycast AI, Linear's
assistant), drawn with Templum's own tokens so every theme, light or dark, gets it: the person's messages as quiet
bubbles, the agent's answers as well-set Markdown, every tool call as one compact line that opens to show its call,
a composer that grows with its text, and an empty chat that says what the agent can do. Nothing it does changes.

## Goals

1. **It reads well.** Answers in the theme's own text color at 14 px, a readable line length in full screen, and
   Markdown with real headings, lists, inline code and code blocks that copy.
2. **It is quiet.** One accent, few borders, a header with only what a person needs, tool calls that take one line.
3. **Nothing is lost.** Every button, key, row, test id and behavior of [templum-agent.md](templum-agent.md) is still
   there and still does what it did; where one moved, this spec says where.
4. **Card 46 builds on it** (*the orchestrator's call*: this card goes first): the message row, the tool row, the
   composer, the chips and the empty state are pieces 46 extends, not redraws.

**Success criteria**

1. Every look below shows in Catppuccin Latte, Mocha, One Light and any theme, with the tokens named here.
2. The headless tests of the plan pass, each failing without its change (layout math, the row gaps, scrolling to the
   end, the growing composer, Send becoming Stop, suggestions typing a prompt, the greeting after New chat, a code
   block's copy, a tool row opening, a row fading in only when it arrives).
3. `--bench` and `--bench-mixed` keep **p95 ≤ 16.7 ms** (the mixed bench streams into the open chat).
4. Screenshots of the empty state, a conversation with Markdown, code and tool rows, an approval and full screen, in a
   light and a dark theme, were looked at and fixed (the plan's last task).

## Non-goals

*The orchestrator's call* for each:

- **No behavior or feature change** beyond the looks and the UX below. Nothing from card 46: no workspace, block or
  board tools, files, slash commands, history or voice.
- **No change to the engine:** `crates/templum-agent` (the conversation, its rows, the stream) is untouched, and so is
  the control server.
- **No new dependency.** gpui-kit 0.7.3 stays pinned; everything here is in it.
- **No API surface.** Templum's rule "every feature in the API" (card 171) is for features; this card is visual only
  and adds nothing a program could drive.
- **↑ doesn't edit the last message:** the orchestrator kept that only if it existed, and it doesn't.

*The designer's call* for each:

- **No new theme token.** The chat uses the [derived tokens](design-system.md#derived-colors) and
  [polish.md's](polish.md#new-theme-tokens), the theme's foreground (`fg`), its red (`normal.red`) and yellow
  (`normal.yellow`), and `Theme::on_accent`. One color is mixed at its use: the code fill (below).
- **No Tab navigation.** Templum has none anywhere; the chat doesn't start it.
- **No scrollbar on the chat's body.** The scroll-to-the-end button is the way back; a bar would be a second moving
  part on a quiet surface. The approval's box gets one (its end must be reached).

## Anti-goals (a failure even if it works)

- **Esc stopping a run.** A day-long orchestration must never end on a reflex meant to put the chat away (*the
  designer's call*: Esc closes, Stop is a button).
- **A bright Allow.** An approval whose Allow is the loudest button gets clicked unread; its three buttons weigh the
  same.
- **Motion that moves what the person reads:** no row slides, no height animates, nothing reflows each frame.
- **Anything drawn per frame while idle.** Spinners, the shimmer and the pill's sweep run only while the agent works;
  every fade runs once.
- **Text past its box.** A long path, URL, command or table wraps, is cut with "…" or scrolls; it never widens a row
  or runs under a button.
- **The chat scaling with the zoom.** It is window chrome: every size here is in window pixels.

## Words

- **The card:** the chat's floating card ([templum-agent.md](templum-agent.md#the-chat)), in its normal size or full
  screen.
- **The column:** the width the rows, the composer and a pinned approval share: the card less its border and 16 px
  on each side, at most 720 px, centered.
- **The composer:** the pill while it is wide (it has the keyboard or text), floating or as the card's footer.
- **A tool row:** an action's row ([Rows](templum-agent.md#rows)).
- **`fg`:** the theme's foreground, at full strength: the reading color.
- **The code fill:** `surface` mixed 6% toward `fg`, opaque.

## The scale

*The designer's call.* Every size of the chat comes from this table (the plan puts it in `chat/look.rs`).

| What | Value |
|---|---|
| Reading text (answers, bubbles, the composer's field) | 14 px, lines 22 px, `fg` |
| Interface text (tool rows, titles, chips) | 13 px |
| Small text (meta, hints, the usage chip, keys) | 12 px |
| The greeting | 16 px semibold |
| Answer headings, semibold | H1 18 px, H2 16, H3 15, H4–H6 14 |
| Code | in answers, gpui-kit's monospace (13 px in blocks); a call or an approval's box, the terminal's font, 12 px |
| Spacing steps | 2, 4, 8, 12, 16, 20, 24 px |
| Radii | card 16, bubble 16, composer and pill 18, code block 10, chip 12, buttons and rows 8 |
| Icons | 15 px in the header and the composer, 14 px in rows |

Colors: primary text `fg`, secondary `title`, meta `title_dim`; fills `hover` (the bubble, inline code) and `card`
(the usage chip, key caps); `hairline` borders; `accent` for the one live thing (send, focus, spinners); `normal.red`
for failures and `normal.yellow` for approvals.

## What a person sees

### The card

- **Size and place** unchanged: 640 px wide (at most the canvas's width − 32), as tall as its rows up to 70% of the
  canvas's height and at least 300 px; full screen 16 px in from every side. **Radius 16** (was 14); `surface` fill,
  `hairline` border, the focused block's shadow (y 10, blur 30); it rises 12 px and fades in over 0.22 s.
- **Full screen** (*the orchestrator's call*: a readable line length): the card fills the canvas and the column stays
  at most 720 px, centered, for the rows, the composer and a pinned approval. Switching is instant (*the designer's
  call*: animating the card's size would lay every row out again each frame).

### The header

*The orchestrator's call*: title, the usage chip, full screen, close, nothing else. *The designer's call*: New chat
stays in the header too, since removing it would remove a feature and every reference chat keeps it there; the model
picker lives once, in the composer (two chips naming the same model is noise).

- **44 px tall**, 12 px side padding, 8 px between items, **no bottom border** (the rows start below it).
- **Left:** `sparkles` (15 px, `accent`), a spinning `orbit` instead while the agent works; "Templum Agent" (13 px,
  semibold, `fg`).
- **Right:** the usage chip, then three icon buttons: **New chat** (`message-square-plus`), **Full screen**
  (`maximize-2`; `minimize-2`, "Exit full screen", while full) and **Close** (`chevron-down`, "Close (Esc)").
  Each is 28 × 28, radius 8, its icon 15 px `title_dim`; `hover` behind it on hover, `pressed` while pressed; its
  tooltip says its name.
- **The usage chip** (the run meter, "2 working · $0.35"): shown once the conversation ran or spent something, as
  today. 24 px tall, radius 12, 10 px side padding, `card` fill, `hairline` border, 12 px `title_dim`. While tasks
  run it has a `chevron-down` (12 px) and opens the tasks drawer; otherwise it is a label (*the designer's call*: today
  it opens an empty list).
- **Moved out:** Stop goes to the composer (*the orchestrator's call*: Send becomes Stop), **Catch me up** to the
  tasks drawer, the model chip to the composer.

### The tasks drawer

Under the header while the usage chip is open, with a `hairline` under it; 6 px above and below its rows, which take
the column.

- A row per task under way: `bot` (14 px, `title_dim`) and its block's title (13 px, `fg`, one line, cut with "…");
  28 px tall, radius 8, 8 px side padding, `hover` on hover; a click shows its block, as today.
- Last, **Catch me up**: `sparkles` (14 px) and "Catch me up" (13 px), both `accent`; a click sends it, as today.

### The rows

The body scrolls; only the rows in view are drawn, as today. It has 8 px above its first row and 16 px under its
last. Each row takes the column, and its distance from the row above depends on both (*the designer's call*):

| This row | Above it | Gap |
|---|---|---|
| Any | nothing (the first) | 0 |
| The person's message | anything | 20 px |
| A tool row, Reads, an approval's record or Thinking | one of those | 2 px |
| A notice | a notice | 2 px |
| A turn's end or a note | anything | 6 px |
| Any other | anything | 12 px |

So a turn's tool rows stack as one log, a turn's time and cost hug its answer, and each message of the person starts
a new block of the page.

### The person's message

- Right-aligned, at most 80% of the column: a bubble with `hover` fill, radius 16, 14 px side and 9 px vertical
  padding, its text in the reading size and `fg`. A long word or path breaks inside it.
- **Queued:** under the bubble, right-aligned, 4 px below it: `clock` (12 px) and "Queued" (12 px, `title_dim`).

### The agent's answer

- Full column, no bubble: Markdown through gpui-kit's `TextView`, selectable as today, in the reading size and `fg`,
  paragraphs 12 px apart (0.75 rem), headings at the scale's sizes.
- **Inline code:** gpui-kit's monospace, on `hover`, in `fg`.
- **A code block:** the code fill, a `hairline` border, radius 10, 13 px code at 1.6 line height, 16 px side and 14 px
  bottom padding, and 34 px on top: a band whose right end holds the block's language (12 px, `title_dim`) and a
  **copy** button (gpui-kit's `Clipboard`, a ghost icon button: `copy`, then `check` once copied; tooltip "Copy").
  Long lines wrap; the block shows whole (*the designer's call*: no inner scroll, so the wheel always scrolls the chat).
- **Tables** take gpui-kit's look and are cut at the column's edge; **links** keep gpui-kit's link color (no theme
  change in this card).
- **While it streams** the text is plain, at the same size, line height and color, and becomes Markdown once whole,
  as today (*the designer's call*: Markdown parsed at each delta grew the chat's memory, perf commit 69921dc).

### Tool rows

*The orchestrator's call*: one compact line with a status icon and the tool's short name and target, which opens to
show details. *The designer's call*: the "short name and target" is the sentence Templum already writes ("Created
workspace API (w3).", "Opening an agent…"), never the model's words.

- **The line:** 28 px tall, radius 8, 8 px side padding, 8 px between its parts:
  - **status** (14 px): running, a spinner in `accent`; done, `check` in `title_dim`; failed, `circle-x` in red;
    denied, `ban` in `title_dim`. The tool-kind icons go (*the designer's call*: one icon per line, and it says the
    state);
  - **the sentence** (13 px): `title`, or `title_dim` with "You denied this."; one line, cut with "…";
  - **a chevron** at the right end (`chevron-right`, 14 px, `title_dim`; `chevron-down` while open), shown while the
    pointer is over the row and while it is open.
- `hover` behind the line on hover. **A click** shows what it touched, as today; on a row that touched nothing, it
  opens the row. **The chevron** always opens or closes it.
- **A failed row** keeps its refusal under the line (12 px, red at 80%, wrapping), lined up with the sentence (30 px
  in).
- **Open** (not saved; New chat closes them all): under the line, lined up with the sentence, a box with the code
  fill, radius 8, 10 px padding: the whole sentence when it was cut (13 px, `title`, wrapping), then the tool's name
  (`create_workspace`, 12 px code, `title_dim`), then its call, a line per argument: `agent: codex`, a string as it
  is, any other value as compact JSON, a string's further lines indented two spaces; at most 12 lines, the 12th
  "⋯ N more lines".
- **Reads:** the same line with `eye` and "Looked at the canvas (4)", both `title_dim`; no chevron, no click.
- **An approval's record:** the same line with `shield-check` (allowed) or `shield-x` (denied), `title_dim`; its
  sentence (`title`, cut with "…"), then its answer ("Allowed once", 12 px, `title_dim`).
- **A notice:** the same line with its dot (8 px: `accent` done, red failed, yellow waiting) in the status slot and
  its words (13 px, `title`, cut with "…"); a click shows its block, as today.

### The other rows

- **A turn's end:** "14 s · $0.04", 12 px, `title_dim`, left-aligned under the answer (*the designer's call*: it ends
  the agent's turn, so it sits on the agent's side; it was right-aligned).
- **A failure:** `circle-alert` (14 px) and its text (13 px), both red, wrapping.
- **A note:** centered, 12 px, `title_dim`.
- **Thinking:** a spinning `orbit` (14 px, `accent`) and "Thinking" (13 px, `title_dim`) with gpui-kit's shimmer
  sweeping across it in `fg`.

### Working

*The orchestrator's call*: a working indicator, and smooth streaming. While the agent works: the header's icon spins,
Thinking shows until its first text or tool, each running tool row spins, and the composer's Send is Stop. The pill at
rest keeps its working look. The streamed text grows in place and the body follows its end while it is there.

### Scrolling to the end

*The orchestrator's call*: a button once the person has scrolled up. It replaces "↓ New".

- **Shown** while the body's content is taller than the body, it doesn't follow its end, and it isn't at its end.
- A 32 px circle, `surface` fill, `hairline` border, shadow y 2, blur 8 (`shadow`), `arrow-down` (16 px, `fg`),
  centered 12 px above the body's bottom; tooltip "Scroll to the end". It fades in over 0.12 s; it goes at once.
- **New below:** while rows came below since the person scrolled up, an 8 px `accent` dot on its top right.
- **A click** goes to the end, and the body follows it again.

### The empty state

*The orchestrator's call*: a short greeting and 3–4 suggestion chips, which type a prompt.

- **Shown** when the conversation has no rows or only notes (*the designer's call*: so it comes back after New chat,
  which leaves a "New conversation." note; today it doesn't).
- **The greeting,** centered in the body (which scrolls if the card is too short for it), 24 px from its top:
  - a 40 px circle of `accent` at 12% with `sparkles` (20 px, `accent`);
  - 12 px below, "What should Templum do?" (16 px semibold, `fg`);
  - 6 px below, at most 420 px wide, centered: "It builds and runs your canvas: workspaces, agents, boards and notes.
    Anything it can't take back waits for your click." (13 px, `title_dim`).
- **Four chips,** 20 px below, stacked 8 px apart, each the column's width up to 520 px: at least 40 px tall,
  radius 12, 14 px side and 10 px vertical padding, a `hairline` border and no fill; `hover` fill and `hover_border`
  on hover, `pressed` while pressed; an icon (15 px, `title_dim`) and the prompt (13 px, `fg`, wrapping):

  | Icon | Prompt |
  |---|---|
  | `layout-grid` | Set up a workspace for a project, with a kanban and an agent per card |
  | `eye` | Catch me up: what's going on? |
  | `grid-2x2` | Tidy this workspace into a grid |
  | `bot` | Open an agent for each card on my board |

- **A click** types the prompt into the composer, which takes the keyboard, so it can be finished before Enter.
  Catch me up is whole and is sent at once, as today (*the designer's call*: the "no behavior change" rule wins).

### The composer

*The orchestrator's call*: a rounded field that grows with its text up to a limit, the model picker as a chip, Send
becoming Stop while it works; no line telling the keys (card 226 took out card 143's hint: it said nothing the person
needed). *The designer's call* for the rest:

```
╭────────────────────────────────────────────────────────────────╮
│ Ask Templum to build, run or tidy your canvas…                 │
│                                                                │
│ [Claude Code · opus ▾]                                   (↑)   │
╰────────────────────────────────────────────────────────────────╯
```

- **Its shape:** radius 18, `surface` fill, a `hairline` border; with the keyboard, an `accent` border and a 2 px
  `ring` around it (a spread with no blur, as the focused block's ring). Floating (the chat closed), it keeps the
  pill's shadow (y 4, blur 12); in the card it has none and takes the column, 8 px under the body and 16 px above the
  card's bottom.
- **Padding:** 12 px on top, 8 px under, 14 px left, 8 px right; 4 px between its two rows.
- **The field:** 1 to 8 lines as today, then it scrolls; the reading size, `fg`; placeholder "Ask Templum to build,
  run or tidy your canvas…" (gpui-kit's muted color, `title_dim`).
- **The bar** (28 px tall), under the field:
  - **the model chip** on the left: "Claude Code · opus" with a caret, 12 px `title_dim`, 24 px tall, radius 12, 8 px
    side padding, `hover` on hover; its menu is the header chip's of today (Default, the models with a check,
    Another model…), opening upward, and a choice gives the keyboard back to the field. The pill's old engine label
    (a click opened Settings) goes: Another model… opens Settings;
  - **the buttons,** right-aligned, 28 px circles:
    - **Send** (`arrow-up`, 15 px): with text, an `accent` fill and the arrow in `on_accent`; without, a `hover` fill,
      the arrow in `title_dim`, and no click. It shows whenever the agent doesn't work, so the bar doesn't jump at the
      first letter;
    - **Stop** while the agent works: an `accent` fill and a 10 px square (radius 2) in `on_accent`; tooltip "Stop";
    - while the agent works **and** the field has text, Send shows left of Stop (the text is queued, as Enter does).
- **Growing:** the width springs from the pill's 300 px to 640 as today; the bar fades in over 0.12 s as it appears.

### The pill at rest

*The orchestrator's call*: it keeps its role and its expand animation, with the new look. Its states, sizes and
words stay those of [The pill](templum-agent.md#the-pill) (300 × 36, radius 18, `surface`, `hairline`, shadow y 4 /
blur 12; working, asks, something new, not installed), with:

- the icon at 15 px and the words at 13 px `title_dim`;
- on hover, its border `hover_border` (*the designer's call*: it says it can be clicked);
- **Review** (asks) a 22 px pill: 10 px side padding, a yellow border, 12 px `fg`, `hover` on hover.

### The approval

*The orchestrator's call*: a clear card with its buttons, keeping its behavior. Every rule of
[Approvals](templum-agent.md#approvals) stays (what it shows, what it hides nothing of, its keys, the 0.5 s, "1 of 3",
asked again). *The designer's call* for the look:

- **The card:** radius 12, `surface` fill, a border of yellow at 55%, 14 px padding, 10 px between its parts. In the
  chat it is pinned 8 px above the composer, at the column's width; floating, 640 px wide (at most the canvas less
  32) above the pill, with the card's shadow (y 10, blur 30).
- **Its keyboard shows** (focus visible): while it has the keyboard its border is full yellow with a 2 px ring of
  yellow at 25%.
- **First line:** `shield-alert` (15 px, yellow), "Templum Agent wants to" (12 px, `title_dim`) and, when several
  wait, "1 of 3" on the right (12 px, `title_dim`).
- **The action:** 14 px semibold, `fg`. "It changed since you looked.": 12 px yellow, with `triangle-alert` (12 px).
- **The box** of what will happen: the code fill, radius 8, 10 px padding, 12 px code, `fg`, at most 160 px, scrolling,
  now with gpui-kit's vertical scrollbar. "Scroll to its end to allow it.": 12 px yellow. "Its reason:" and the
  reason: 12 px `title_dim`.
- **The buttons:** Allow once, Allow for this conversation, Deny: the same weight (*the designer's call*: no primary
  Allow): 30 px tall, radius 8, 12 px side padding, a `hairline` border, 13 px `fg`, then the key in a small key cap
  (18 px square, radius 4, `card` fill, 12 px `title_dim`); `hover` on hover, `pressed` while pressed. Ask each
  time's Allow once: a red border and red at 12% behind it. Until the box was seen to its end, both Allows are at 40%
  and answer nothing, as today.
- **The words field** ("Or tell it what to do instead"): gpui-kit's small `Input`, the card's width.

### Keys and focus

*The orchestrator's call*: keyboard first, focus visible. *The designer's call*: Esc keeps its role.

- **Enter** sends (queues while the agent works); **Shift+Enter** is a new line; **Esc** closes the chat and gives the
  keyboard back, never stops the agent (see [Anti-goals](#anti-goals-a-failure-even-if-it-works)); the approval's
  `1`, `2`, `3` and Esc as today; the shortcut as today.
- **Focus shows** on what takes the keyboard: the composer's ring, the approval's ring, the words field's own
  gpui-kit ring. The chips and buttons take clicks, not the keyboard.

### Motion

*The orchestrator's call*: subtle, following the motion rules ([polish.md](polish.md#motion)); with reduced motion,
none. *The designer's call* for the values:

| What | Duration | Easing | Notes |
|---|---|---|---|
| The card rises and fades in | 220 ms | ease-out | Unchanged |
| The pill grows into the composer | ≈ 180 ms | spring, no overshoot | Unchanged; the bar fades in over 120 ms |
| A row arriving while the chat is open | 160 ms | ease-out | Opacity only; see below |
| A tool row's details opening | 160 ms | ease-out | Opacity only; the rows under it move at once |
| The scroll-to-the-end button | 120 ms | linear | In only |
| Spinners, the shimmer, the pill's sweep | as gpui-kit's | | Only while the agent works |

A row fades only when it arrives while the chat is open: rows already there when the chat opens, and rows scrolled
back into view, show at once. Every one goes through GPUI's animations, which show the end (a one-shot) or the start
(a loop) and schedule no frame when the system asks for reduced motion (`App::reduce_motion`).

### Long content

| Content | What happens |
|---|---|
| A long word, path or URL in a bubble or an answer | Breaks inside its line |
| A long tool sentence, notice, task title or approval record | One line, cut with "…"; an open row shows it all |
| A long call | Wraps, at most 12 lines then "⋯ N more lines" |
| A long code block | Wraps; shows whole; the chat scrolls |
| A wide table | Cut at the column's edge |
| The empty state in a short card | The body scrolls |
| What an approval will do | Its box scrolls past 160 px, with a scrollbar |

## For card 46

*The orchestrator's call*: 46's UI is built from these pieces. Where each of its features plugs in (*the designer's
call*, a guide, not code written now):

- **Workspace, block and board tools:** tool rows. Their sentences are Templum's, their call opens the same way.
- **File input:** chips in the composer's bar, right of the model chip, each with a ✕.
- **Slash commands:** a list rising from the composer, the column's width, rows as the tasks drawer's.
- **History:** the header's title becomes a menu (its conversations, then New chat).
- **Voice:** a microphone button left of Send, the same 28 px circle.

## Constraints

- **Crates:** only `crates/templum` changes; `templum-agent`, `templum-mcp`, `templum-canvas` and `templum-terminal`
  don't.
- **Window chrome:** sizes in window pixels; the camera's zoom never scales the chat
  ([zoom rule](templum-agent.md#the-chat)).
- **Performance:** nothing new runs per frame while the chat is idle; the open chat redraws only when its state
  changes, a row arrives, the person scrolls or a one-shot fade runs. `--bench` and `--bench-mixed` keep p95 ≤
  16.7 ms.
- **Element ids:** every id the tests use stays, on the element that does the same thing (`agent-stop`, `agent-model`
  and `agent-catch-up` moved with their elements); `agent-new-below` becomes `agent-to-end`, `agent-engine` goes.
- **Tests:** headless, independent of HOME, config and installed CLIs, stable under CPU load.

## Left to the builder

- The code fill's share (6%) if a theme makes it too faint or too strong: 4 to 8%, the same for every theme.
- How the chevron shows on hover (a group hover is enough), and the shimmer's highlight if `fg` is too strong.
- The exact offsets of the code block's language and copy button inside its 34 px band.
- Anything the screenshots show off by a few pixels, within the scale.

## Considered and left out

*The designer's call* for each:

- **gpui-kit's `MessageScroller`** (a list with its own jump button): it would replace the chat's proven list state
  and its tests for a button this spec draws in a few lines.
- **gpui-kit's `Bubble` and `Message`:** they carry gpui-kit's own theme semantics; the bubble here is one `div` with
  Templum's tokens.
- **Markdown live while it streams** (a `TextViewState` fed with `push_str`, with gpui-kit's stream fade): the risk of
  69921dc's memory growth on day-long runs; worth its own card with a bench.
- **Opening the card on the first shortcut of an empty conversation:** it would end the pill's quick, Spotlight-like
  role; the greeting shows after New chat and whenever the card is open with no rows.
- **A model chip in the header too,** a primary Allow, Esc to stop, a scrollbar on the body, animated heights and an
  animated full screen: see above.

## Calls made for card 143

**The orchestrator's:** this card before card 46, its pieces for 46 to extend; the look (bubbles on the right,
answers full width with Markdown and copyable code, one-line tool rows that open, approvals as cards, a working
indicator, scroll to the end, a growing composer with the model chip, Send becoming Stop and a hint, the greeting
with 3–4 chips, a header of title, usage, full screen and close, a type and spacing scale with a readable full-screen
width, subtle motion and none when reduced); keyboard first and visible focus; the pill and its animation keep their
roles; no behavior change, nothing from 46, no new dependency, no engine change, the zoom rule, the performance bar,
headless tests and screenshots.

**The designer's:** the scale and every size above; the 720 px column; the row gaps; `fg` for reading text; the code
fill; no new token; New chat kept in the header and the model chip only in the composer; Catch me up in the tasks
drawer; the usage chip a label without tasks; status icons instead of kind icons; a click on a row with no target
opens it; the call's 12 lines; the turn's end on the left; the shimmer on Thinking; the scroll button's look and when
it shows; the greeting's words and the fourth chip; the greeting after New chat; Catch me up still sends; Send always
shown, dim without text, beside Stop when text waits; the hint's words; Esc never stops; no primary Allow; the
approval's scrollbar and focus ring; opacity-only fades, an instant full screen; no body scrollbar; what was left out.
