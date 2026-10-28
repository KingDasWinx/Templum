# Notices that wait their turn: the person's text first, and answers in batches

- **Date:** 2026-10-24
- **Status:** Implemented (kanban card 146)
- **Builds on:** [connections.md](connections.md) (Delegating: notices typed back),
  [agent-protocol.md](agent-protocol.md) (a task's end, stuck, quiet), [templum-agent.md](templum-agent.md) (its chat's
  notices)

## Intent

Templum types notices into an agent that handed tasks to others: a task ended, got stuck, went quiet, waits for its
person (`Workspace::notices`, `deliver_notices`, crates/templum/src/workspace/delegate.rs). Two things go wrong today:

1. **The person's half-written message is sent with a notice.** A notice waits only while the person typed in the
   last 3 seconds (`TYPING` in `Pane::takes_notice`). A person who stops mid-sentence to think, or is typing at that
   very moment as the 3 seconds run out, gets the notice pasted after their text and both sent by its Enter. The person
   saw both happen.
2. **Many answers at once are many interruptions.** With 20 agents finishing together, the orchestrator gets 20
   notices, each its own turn.

## Goals

1. Templum never types into an agent's input while it holds text the person typed and didn't send, however long ago
   they typed it. The notices wait, the terminal says how many wait, and they go once the input is empty again.
2. Answers to one agent arrive in batches: a notice waits up to 15 seconds for others to the same agent, unless no
   other task of that agent is still running.

## Non-goals

- Calls that wait for their answer (`delegate` with `wait`, `wait_for_idle`, `wait_for` with `wait`): their answer
  returns to the caller as today, at once.
- **The reverse race** (decided with the person): Templum pastes a notice and the person starts typing before its Enter
  (under a second). Today Templum then presses no Enter and the paste stays in the input with the person's keys. It
  stays so, recorded in docs/minors.md; the draft rule below already removes the common case.
- What a notice says (its words, its pasted screen or transcript): unchanged, but for the batch's title.
- Text another agent types with `send_to_terminal` or `delegate`: their own rules stay (`Typing` refuses a task while
  the person types).

## Anti-goals

- Sending, or mixing into, anything the person typed.
- A notice lost: a held notice is delivered once the input is empty, or dropped only when its agent is gone (as today).
- A notice held forever by a timer: the batch never holds a notice more than 60 seconds.

## The person's text first (decided with the person)

- **A draft:** text the person typed or pasted into an agent's input and hasn't sent. While one is there, `takes_notice`
  says not now, whatever the time since the last key.
- **When the draft is gone:** the person sent it (Enter that submits, or the agent's hook says a prompt was submitted),
  or emptied the input (Backspace to empty, Ctrl+C, Esc that clears, Ctrl+U…). Templum can't see every agent's input,
  so it combines what it knows (*the builder decides the exact signals per agent family*): the person's own keys
  (`person_typed`, the key that submits), the hooks (`Submitted`) where an agent has them, and a look at the input line
  of the screen (the faint suggestion left out, as `SharedTerm::lines` does since card 147). When the signals disagree,
  the safe side is to wait; the look at the screen ends a wait the keys can't explain (an input seen empty).
- **What the person sees:** while one or more notices wait for a terminal's draft, the terminal shows a small notice of
  its own, "3 answers waiting" (one: "1 answer waiting"), in the style of the inbox's "Waiting for your answer" chip
  (crates/templum/src/inbox.rs `notice`), where the two don't overlap. A click on it does nothing but show a tooltip
  ("They go in once your message is sent or cleared"). It goes away when they are typed.

## Answers in batches (decided with the person)

- **Which notices batch** (*the designer's call*): the notices about a task Templum types into the agent that handed
  it (`Notice.to`): a task's end, stuck, quiet, waits for its person. The other notices — a `wait_for` with `wait:
  false` that matched or ended, a schedule's reminder or its failure, the answer to a question whose client stopped
  waiting — don't wait for the window: they go as soon as the draft rule and the existing gaps allow. So each queued
  notice carries its kind.
- **The Templum Agent** (*the designer's call*): its chat shows each notice's row at once (and its run meter counts it
  at once); only what is told to its engine batches, by the same window, as one message. The draft rule doesn't apply
  there: the chat's field isn't the engine's input.
- **Still running:** a task whose end notice is on its way (the short delay before its spend is read) counts as still
  running, so 20 agents ending together make one batch.
- **The window:** when a notice for agent A arrives, it waits **15 seconds**; each new notice for A restarts the 15
  seconds. If, when a notice arrives, A has no other task still running (no terminal's `Handed` with `by == A` still
  awaited), the batch goes at once. A batch never holds its oldest notice more than **60 seconds** (*the designer's
  call*): past that it goes even while notices keep coming.
- **One message:** a batch of one is typed as today. A batch of several is typed as one paste with one Enter: a title
  line, "[3 tasks ended]" (the builder words it when the batch mixes kinds, e.g. "[3 notices: 2 tasks ended, 1 waits
  for its person]"), then each notice as today, oldest first, separated by a blank line (*the designer's call*).
- **With a draft:** a batch due while the person has a draft waits for the draft (above); the notices that arrive
  meanwhile join it.
- The existing gaps stay: a notice still waits for `NOTICE_GAP` after Templum typed into the agent, and while the
  agent waits for its person.

## Constraints

- CLAUDE.md: tests for every logic change (the batching and draft rules are logic: headless tests with the test clock;
  the chip's look: say how it was verified); minors to docs/minors.md; no speculative structure; docs ≤ 120 columns.
- No new setting: 15 s and 60 s are constants.
- Specs to update: connections.md (Delegating, notices), agent-protocol.md (notices), templum-agent.md (its chat's
  notices), this file's Status.

## As built

- **The draft's signals** (`Pane::person_wrote`, `person_keyed`, `settle_draft`, `input_empty`). Set by the person's
  text, a paste, text typed for them (`send_to`; not while the agent waits for them), Shift+Enter, Alt+Enter, Tab (not
  Shift+Tab; it takes the suggestion), Up, Down and Ctrl+Y. Cleared by Enter, except Claude Code's (`\` and Enter is a
  new line there; its `Submitted` at the Enter clears it), by Ctrl+C, by a `Submitted` that comes at the Enter (Claude
  Code's only: the others come once a turn takes a prompt in, while the person may be writing the next), by `Started`
  or `Ended`, and by a look at the screen once their last key is `TYPING` old that finds, in the last six rows, the
  last row opening with a prompt marker (`›` Codex, `❯` or `>` Claude Code, `>` a plain agent, inside the box some
  draw) with nothing after it. A screen with no such row (a dialog, Claude Code's shell mode) never ends a draft.
  Tested for Claude Code, Codex and a plain agent without hooks. Backspace, Esc and Ctrl+U (to the start of the
  line only) say nothing: the screen does.
- **The chip** is `answers-waiting-{block}` (`block.rs`), at the bottom center like the inbox's, lifted above it when
  that shows; it shows while a draft holds a notice and a click on it does nothing.
- **Titles:** `[3 tasks ended]`, `[3 tasks need a look]` (a dialog, or a screen gone still), and `[3 notices: 2 tasks
  ended, 1 needs a look]`, each followed by `They are in the pasted text.`: the first line is the one typed, the rest
  is the paste. A notice that a task waits for its person or went quiet is dropped if, before it goes, its worker
  doesn't any more (the dialog was answered, the task ended).
- **Still running** leaves out a task that waits for its person or went quiet: its own notice is on its way.
- **The Templum Agent's engine** hears its batches from the same queue (`Workspace::notices`, recipient
  `templum_agent::ASKER`): no second queue, and `deliver_notices` skips the draft and gap rules for it. Constants:
  `BATCH_WAIT`, `BATCH_MOST`.

## Left to the builder (as the design wrote it)

The draft's exact signals per agent family (with tests for Claude Code, Codex and a plain agent without hooks), where
the "answers waiting" chip sits, the batch title's words, how the Templum Agent's batch reaches its engine (one message
with the same shape), and the constants' names.
