# Templum to Templum, Batch 2c (Terminals: Scrolling Back and the Mouse) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer in a Templum agent block does the whole plan, then
> fresh Opus 5.5 reviewers review it in a loop until one passes it. The implementer **commits nothing**: everything
> stays uncommitted for the orchestrator.

**Goal:** Kanban card 171 part 2, batch 2, third of eight parts: in a remote terminal the person **scrolls back
through home's history** (the wheel, Shift+PageUp, Find) and **uses the mouse** in a program at home that asked for
it (a click, a drag, the wheel, moves while it asked for every motion). Home gains `terminal_input`'s `mouse` and a
frame's `modes.mouse_moves`.

**Architecture:** Home encodes the mouse as it encodes keys: `terminal_input`'s `mouse` events go through
`Terminal::mouse` in the program's mode at that moment. The mirror reads home's modes from its own viewer (the painter
writes them into it), sends a mouse event only when they say the program wants it, through the terminal's typing queue
(`Command::Mouse`, one call in flight, waiting moves merged). Scrolling back is a client command (`Command::History`):
the client calls `read_terminal` (2000 lines) on its calls connection and, on its own thread, puts the text into the
viewer's scrollback above the live screen (`Feeder::fill_history`, in `templum-terminal`), then tells the UI
(`Update::History`) so an open Find searches again. The UI thread never holds the history's text.

**Tech Stack:** Rust 2024; GPUI through gpui-kit `=0.7.3`; alacritty_terminal 0.26 (the viewer's grid: `Grid::
update_history`, `CSI 3J`/`CSI J`/`CSI n S`); 2a's client (`Call`, the calls lanes, the typing queue) and glue
(`Deed`, `Nay`, `why_not`, `mirror_send`, `RemoteSpace::told`).

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), "Batch 2: the whole workspace, live":
"Parts ▸ 2c", "Terminals" (Scrolling back, The mouse), "One rule for what the mirror can do", "What home's API gains"
(`modes.mouse_moves`, `terminal_input`'s `mouse`), "The mirror's side", "Constraints (batch 2)", "Left to the builder
(batch 2)" ("how the history reaches the viewer's scrollback"), the threat model rows marked *(batch 2)* ("one
terminal's history at a time"; "typing and the mouse" need Full), and "As built ▸ Batch 1b, 1c, 1d, 2a, 2b". The 2a and
2b plans' "Interfaces for later parts" (2c's `Deed::Mouse`, `Deed::ScrollBack`; `Deed::needs` for `["mouse"]`).

**Code it builds on:** `crates/templum-terminal/src/{terminal.rs,input.rs,screen.rs,lib.rs}`;
`crates/templum-api/src/{registry.rs,model.rs,terminals.rs,openapi.rs}`, `crates/templum-api/src/client/{mod.rs,
remote.rs,frames.rs}`, `crates/templum-api/tests/{mirror.rs,common/mod.rs}`; `crates/templum/src/workspace/{api.rs,
remote.rs,input.rs,find.rs}`, `crates/templum/src/block.rs`; `crates/templum/src/workspace/tests/{api.rs,remote.rs,
mirror_end_to_end.rs,mod.rs}`.

**Consumes (as built):** `Terminal::mouse(&MouseInput) -> bool` (home: clamps, encodes in the program's mode, false
when not asked); `MouseInput`, `Button`, `MouseKind` (`templum_terminal`); `SharedTerm::{modes, lines, find,
selection_text}`, `Terminal::{scroll, scrolled_back, feeder}`, `Feeder::feed`; the painter (`client/frames.rs`,
`Painter::modes_into`) that writes home's modes into a viewer; the typing queue (`client/remote.rs`: `Item`, `Queue`,
`pump`, `typing_task`, `WAITING_MOST` 1024, `PACE`); `Life::call(Job)`, `To`, `refuse`, `called`; `Workspace::
{mirror_send, warn, typing_at, remote_type, remote_paste, heard, open_find}`; `RemoteSpace::{why_not, told}`, `Deed`,
`Deed::{call, needs}`, `Nay`; the input path (`workspace/input.rs`: `report`, `press_program`, `release_program`,
`hover_program`, `drag_to`'s `Drag::Program`, `scroll_terminal`, `on_key_down`'s remote branch); the tests' `offline`,
`live`, `shown_mirror`, `shown_mirror_as`, `focus_viewer`, `pump_home`, `draw`, `said`, `wheel`, `click`, `barrier`,
`settled`, `calls_of`, `lend`, `viewer`, `live_on`, `until`, `mirrored`, `open_as`, `first_id`.

## Global Constraints

- **MEMORY LIMITS on this machine:** **every** cargo command takes `-j 4`; **never two cargo commands at once**; never
  `cargo clean`; while working run only the targeted tests each step names; the full suite **once**, in Task 8.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-terminal` and `templum-api` stay UI-free.
  The history fill and `Modes::wants` live in `templum-terminal`, tested headlessly. No `unsafe`. **No new dependency.**
- **The UI thread never waits on the network:** the mouse is `Command::Mouse` (the typing queue), scrolling back is
  `Command::History`; both are `mirror_send`, which only queues. The history's text is read, cut and filled on the
  client's thread and never reaches the UI thread (`Update::History` carries only the block).
- **Home is the truth:** a mouse event is encoded at home in the program's mode when it arrives; home drops an event
  the program didn't ask for and counts those taken. The mirror sends only what home's last frame's modes (as its
  viewer holds them) say the program wants.
- **What home sends is bounded and cleaned before it is drawn:** the history text is cut to its last
  `templum_mcp::tools::MAX_TEXT` (256 KiB) on the client, then `fill_history` feeds only its last `VIEWER_HISTORY +
  rows` lines, drops every control character but `\t` (a tab only moves the cursor) and keeps at most `VIEWER_HISTORY`
  rows; nothing in it runs (no title, no mode, no screen switch). Mouse events: at most `INPUT_MOUSE` (64) a call, and
  with the keys at most `WAITING_MOST` (1024) waiting per terminal; waiting moves merge into the newest. Home clamps
  `row` and `col` to its grid (`Terminal::mouse`).
- **Memory:** one terminal's history at a time per remote, at most 2000 rows, dropped when its stream ends (and on a
  reconnect); a viewer's scrollback is 0 again once dropped.
- **Never a relay:** the mirror's API, control server and block server gain nothing; a remote viewer stays out of
  `Space::terminals()` (unchanged), so Settings' `set_options` never gives it a scrollback.
- **The rule:** the mouse asks `why_not(Deed::Mouse)` (`terminal_input` with `mouse`: Full; a home whose
  `terminal_input` doesn't take `mouse` is "older"); scrolling back asks `why_not(Deed::ScrollBack)` (`read_terminal`:
  read, any share). A Control share's press says once per terminal why, and selects text as locally.
- **Values, verbatim:** `INPUT_MOUSE` = 64; `VIEWER_HISTORY` = 2000 (`templum_mcp::tools::MAX_LINES`); `read_terminal`
  with `"lines": 2000`; the history text at most 256 KiB; `WAITING_MOST` 1024 counts keys and mouse events alike.
- **Words, verbatim** (winx-desktop is `computer_of`; b7k2m home's block):
  - "Using the mouse in a terminal needs a Full share: share again on winx-desktop." (a Control share's first press
    in a terminal whose program asked for the mouse; once per terminal)
  - "winx-desktop's Templum is older: update it to do this from here." (`Nay::Older`, a home without `mouse`)
  - Home's answers: "Sent 1 mouse event to b7k2m."; "Sent 3 mouse events to b7k2m." (also "Sent 0 mouse events to
    b7k2m.": not an error)
  - Home's parser: "terminal_input takes one of text, keys or mouse." (replaces "terminal_input takes text or keys.");
    "\"mouse\" is a list of mouse events."; "\"mouse\" holds 1 to 64 events."; "mouse[3] isn't a mouse event: \"kind\"
    is press, release or move, \"button\" left, middle, right, wheel_up, wheel_down or null (a move with none held),
    \"row\" and \"col\" whole numbers."
  - Unchanged, reused: "Typing needs a Full share: share again on winx-desktop." (the wheel's arrow keys in a
    full-screen program); "Typing faster than winx-desktop answers: some keys were dropped." (also for mouse events
    past the queue).
  - Gone: "Only the live screen is mirrored for now."
- **Batch 1's, 2a's and 2b's words stay;** every earlier test keeps passing except those this plan rewrites (named in
  their task: `terminal_input_parses_text_or_keys`'s sentence, `the_wheel_over_a_remote_terminal_says_so_once_and_
  scrolls_nothing` deleted).
- **Every logic change has a test that fails without it;** UI-only changes say how they were verified (the pointer
  over a remote terminal at other zooms was not looked at on a display unless the implementer did: then a minors
  entry). Headless GPUI tests; keys in `keyring_core::mock::Store`; home on loopback in the same process; conditions
  with a 10 s deadline, never a fixed sleep; a test that needs a real shell program is `#[cfg(unix)]`. CI runs Linux,
  macOS and Windows with no agent CLI.
- **Every feature in the API:** `modes.mouse_moves` (Task 1) and `terminal_input`'s `mouse` (Task 2) ship their lines
  in `docs/api.md` in their task. The mirror's history and mouse add nothing to the mirror's own API.
- **Minors** go to `docs/minors.md` under `## Templum to Templum`; "No mouse input through the API." leaves it (Task
  2). Docs lines ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **Home's screen not full, or holding wrapped lines** (after `clear`, a fresh shell, a long command line):
   `read_terminal` answers history *and* screen, so the history must end exactly where the live screen begins, with no
   line lost and none doubled at the seam. → Task 3 `history_above_a_screen_that_isnt_full_loses_and_doubles_nothing`,
   `long_lines_wrap_again_at_home_s_width`.
2. **A hostile `read_terminal` answer** (8 MiB, escape sequences, a title, `?1049h`, 100 000 lines): nothing runs, the
   parse is bounded, at most 2000 rows stay. → Task 3 `history_from_home_is_drawn_never_run`,
   `history_keeps_at_most_two_thousand_rows`.
3. **The person is already scrolled back, or Find selected a match, when a new history lands:** the view keeps its
   place from the bottom, the selection is cleared, and an open Find searches again. → Task 3
   `a_fill_keeps_the_view_where_the_person_left_it`; Task 8's end-to-end Find.
4. **A drag or a hover storm while home asked for every motion:** waiting moves merge, so the queue never grows by
   hundreds, and clicks, moves and keys keep their order. → Task 4
   `mouse_events_ride_the_typing_queue_in_order_and_waiting_moves_merge`.
5. **Two terminals and a race:** scrolling back in B drops A's history; an answer for A that arrives after B was asked
   fills nothing; a terminal that stops streaming loses its history; the program at home going full screen between
   the ask and the answer fills nothing. → Task 5 `one_terminal_s_history_at_a_time_and_it_goes_with_its_stream`;
   Task 3 `a_viewer_in_the_alternate_screen_takes_no_history`.

---

### Task 1: `modes.mouse_moves`, home to the viewer

**Files:**
- Modify: `crates/templum-terminal/src/terminal.rs` (`Modes`: `mouse_moves`, `Modes::of`, `Modes::wants`)
- Modify: `crates/templum-api/src/model.rs` (`ModesView::mouse_moves`), `crates/templum-api/src/terminals.rs`
  (`frame` maps it), `crates/templum-api/src/client/frames.rs` (`Painter::modes_into`),
  `crates/templum-api/src/openapi.rs` (`Frame.modes` gains `mouse_moves`)
- Modify: `docs/api.md` (Terminal streams ▸ **Modes.**)
- Test: `crates/templum-terminal/src/terminal.rs` (tests), `crates/templum-api/src/terminals.rs` (tests),
  `crates/templum-api/src/client/frames.rs` (tests)

**Interfaces:**
- Consumes: `TermMode::{MOUSE_MODE, MOUSE_MOTION}`; `MouseInput`, `MouseKind`.
- Produces: `Modes { mouse, kitty_keys, full_screen, typing, mouse_moves: bool }` (`mouse_moves`: `?1003`, every
  motion); `Modes::wants(&self, e: &MouseInput) -> bool` (a move with no button: `mouse_moves`; anything else:
  `mouse`); `ModesView::mouse_moves` (`#[serde(default, skip_serializing_if = "is_false")]`); a viewer painted with
  `mouse_moves` has `?1003` set, so `viewer.shared().modes()` is home's modes (Tasks 6, 7 read them there).

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum-terminal/src/terminal.rs, tests
#[test]
fn modes_say_every_motion_apart_from_the_mouse() {
    let v = Terminal::viewer("v", (10, 2), Palette::default(), TermOptions::default(), Arc::new(|| {}));
    let modes = |bytes: &[u8]| {
        v.feed(bytes);
        let m = v.shared().modes();
        (m.mouse, m.mouse_moves)
    };
    assert_eq!(modes(b"\x1b[?1000h"), (true, false), "clicks");
    assert_eq!(modes(b"\x1b[?1002h"), (true, false), "drags");
    assert_eq!(modes(b"\x1b[?1003h"), (true, true), "every motion");
    assert_eq!(modes(b"\x1b[?1003l"), (false, false));
}

#[test]
fn a_mouse_event_is_wanted_as_the_modes_say() {
    let at = |kind, button| MouseInput { button, kind, row: 0, col: 0, ctrl: false, alt: false, shift: false };
    let clicks = Modes { mouse: true, ..Modes::default() };
    let every = Modes { mouse: true, mouse_moves: true, ..Modes::default() };
    for e in [
        at(MouseKind::Press, Some(Button::Left)),
        at(MouseKind::Release, Some(Button::Left)),
        at(MouseKind::Press, Some(Button::WheelUp)),
        at(MouseKind::Move, Some(Button::Left)),
    ] {
        assert!(clicks.wants(&e) && every.wants(&e) && !Modes::default().wants(&e), "{e:?}");
    }
    let hover = at(MouseKind::Move, None);
    assert!(!clicks.wants(&hover) && every.wants(&hover));
}

// crates/templum-api/src/terminals.rs, tests
#[test]
fn every_motion_rides_on_frames_as_mouse_moves() {
    let v = viewer(10, 3);
    let a = screen(&v);
    v.feed(b"\x1b[?1003h");
    let sent = frame(Some(&a), &screen(&v)).expect("a mode alone is a change");
    assert_eq!(serde_json::to_value(&sent).unwrap()["modes"], json!({"mouse": true, "mouse_moves": true}));
}

// crates/templum-api/src/client/frames.rs, tests
#[test]
fn the_mouse_modes_reach_the_viewer_as_home_has_them() {
    let mirror = viewer(10, 2);
    let mut painter = Painter::default();
    let mut frame = frame_of(10, 2, vec![run("x")]);
    let mut paint = |mouse, moves| {
        frame.modes.mouse = mouse;
        frame.modes.mouse_moves = moves;
        painter.paint(&frame, &mirror.feeder()).unwrap();
        let m = mirror.shared().modes();
        (m.mouse, m.mouse_moves)
    };
    assert_eq!(paint(true, true), (true, true));
    // alacritty's mouse modes replace each other: back to clicks only is clicks only.
    assert_eq!(paint(true, false), (true, false));
    assert_eq!(paint(false, false), (false, false));
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-terminal modes_say_every_motion` and
  `cargo test -j 4 -p templum-api --lib mouse_moves` / `the_mouse_modes_reach` → compile errors (no `mouse_moves`,
  no `wants`).
- [ ] **Step 3: Implement.** `Modes::of`: `mouse_moves: mode.contains(TermMode::MOUSE_MOTION)`. `Modes::wants` as
  the Interfaces say. `ModesView::mouse_moves` after `kitty_keys`; `terminals::frame` copies it. `Painter::
  modes_into`: when `mouse` or `mouse_moves` changed (or on the first frame) write `\x1b[?1000l\x1b[?1003l`, then
  `\x1b[?1003h` when `mouse_moves`, else `\x1b[?1000h` when `mouse` (one pair, because alacritty's `?1000h` and
  `?1003h` each remove the other). OpenAPI: `"properties": { "full_screen", "mouse", "kitty_keys", "mouse_moves" }`.
  Fix every `Modes { … }` literal the compiler names (`..Modes::default()`).
- [ ] **Step 4: Run them to see them pass,** and the modules they touch: `cargo test -j 4 -p templum-terminal`,
  `cargo test -j 4 -p templum-api --lib` → pass.
- [ ] **Step 5: `docs/api.md`,** Terminal streams ▸ **Modes.**: the example becomes
  `{"full_screen":true,"mouse":true,"mouse_moves":true,"kitty_keys":true}`; add "`mouse_moves`: the program asked for
  every motion (`?1003`), not only clicks and drags: send a move with no button held only then." and replace "a later
  one can send the mouse" with "a client that sends the mouse sends only what `mouse` and `mouse_moves` say".

### Task 2: Home takes `terminal_input`'s `mouse`

**Files:**
- Modify: `crates/templum-api/src/registry.rs` (`INPUT_MOUSE`, `Input::Mouse`, `mouse_value`, `mouse_of`, the
  tool's schema and description, `parse_own`, `NOT_TARGETS` gains `"mouse"`)
- Modify: `crates/templum/src/workspace/api.rs` (`api_input`'s `Input::Mouse` arm)
- Modify: `docs/api.md` (the tools table's `terminal_input` row; **Input.**), `docs/minors.md` (delete "No mouse
  input through the API.")
- Test: `crates/templum-api/src/registry.rs` (tests), `crates/templum/src/workspace/tests/api.rs`

**Interfaces:**
- Consumes: Task 1's nothing; `Terminal::mouse`.
- Produces: `pub const INPUT_MOUSE: usize = 64;` `Input::Mouse(Vec<MouseInput>)` (its `Debug` prints `Mouse(n)`);
  `pub fn mouse_value(e: &MouseInput) -> Value` (`{"kind","button","row","col","ctrl","alt","shift"}`, all seven,
  `button` null for none); `pub fn mouse_of(v: &Value) -> Option<MouseInput>` (`kind` `press`/`release`/`move`;
  `button` `left`/`middle`/`right`/`wheel_up`/`wheel_down`, or null/absent only for `move`; `row`, `col` whole
  numbers, past `usize` saturating; `ctrl`, `alt`, `shift` absent or booleans). Task 4 sends `mouse_value`'s JSON.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum-api/src/registry.rs, tests
#[test]
fn terminal_input_parses_mouse_events() {
    let press = json!({"kind": "press", "button": "left", "row": 3, "col": 10});
    let Own::TerminalInput { input: Input::Mouse(events), .. } =
        parse_own("terminal_input", &json!({"block": "b", "mouse": [press.clone()]})).unwrap()
    else {
        panic!("mouse")
    };
    assert_eq!(events, [MouseInput { button: Some(Button::Left), kind: MouseKind::Press, row: 3, col: 10,
        ctrl: false, alt: false, shift: false }]);
    let hover = json!({"kind": "move", "button": null, "row": 0, "col": 0});
    assert!(parse_own("terminal_input", &json!({"block": "b", "mouse": vec![hover.clone(); INPUT_MOUSE]})).is_ok());
    let err = |args: Value| parse_own("terminal_input", &args).unwrap_err();
    assert_eq!(err(json!({"block": "b", "mouse": vec![hover; INPUT_MOUSE + 1]})), "\"mouse\" holds 1 to 64 events.");
    assert_eq!(err(json!({"block": "b", "mouse": []})), "\"mouse\" holds 1 to 64 events.");
    assert_eq!(err(json!({"block": "b", "mouse": "click"})), "\"mouse\" is a list of mouse events.");
    let bad = err(json!({"block": "b", "mouse": [press.clone(), press.clone(), press.clone(),
        {"kind": "press", "button": null, "row": 0, "col": 0}]}));
    assert!(bad.starts_with("mouse[3] isn't a mouse event: \"kind\" is press, release or move"), "{bad}");
    assert_eq!(err(json!({"block": "b", "keys": ["a"], "mouse": [press.clone()]})),
        "terminal_input takes one of text, keys or mouse.");
    assert_eq!(scope("terminal_input", &json!({"block": "b", "mouse": [press]})), Ok(Scope::Full));
}

#[test]
fn a_mouse_event_reads_back_as_it_was_written() {
    for button in [None, Some(Button::Left), Some(Button::Middle), Some(Button::Right), Some(Button::WheelUp),
        Some(Button::WheelDown)] {
        let e = MouseInput { button, kind: if button.is_none() { MouseKind::Move } else { MouseKind::Release },
            row: 7, col: 2000, ctrl: true, alt: false, shift: true };
        assert_eq!(mouse_of(&mouse_value(&e)), Some(e));
    }
}
```

In `terminal_input_parses_text_or_keys`, `one` becomes `"terminal_input takes one of text, keys or mouse."` (the only
change to that test).

```rust
// crates/templum/src/workspace/tests/api.rs
#[gpui_kit::test]
fn terminal_input_sends_the_mouse_the_program_asked_for(cx: &mut TestAppContext) {
    // open; three_blocks; uid of the terminal; token = with_token(Full); inputs() as in
    // terminal_input_pastes_and_presses_as_the_keyboard.
    // 1. {"block": uid, "mouse": [press left at (1, 1), release left at (1, 1)]} before the program asked:
    //    said(&line) == (false, "Sent 0 mouse events to {uid}."); inputs unchanged.
    // 2. The terminal's grid fed "\x1b[?1000h\x1b[?1006h" (`term.feed`, as a program's output): the same call →
    //    (false, "Sent 2 mouse events to {uid}."); inputs == n + 2; keys_from's name == "API · phone".
    // 3. One move with no button: (false, "Sent 0 mouse events to {uid}.") (clicks only); after feeding
    //    "\x1b[?1003h": (false, "Sent 1 mouse event to {uid}.").
    // 4. A Control token: refused(&line).0 is the scope refusal, as for text.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-api --lib registry::tests::terminal_input`
  and `registry::tests::a_mouse_event_reads_back` → compile errors; `cargo test -j 4 -p templum
  workspace::tests::api::terminal_input_sends_the_mouse` → compile error.
- [ ] **Step 3: Implement.** `parse_own`'s `terminal_input`: exactly one of `text`, `keys`, `mouse` (else the new
  sentence); `mouse` an array (else "is a list"), 1 to `INPUT_MOUSE` (else "holds"), each through `mouse_of` (else the
  first failing position's sentence). The schema's `mouse`: `{"type":"array","minItems":1,"maxItems":INPUT_MOUSE,
  "items":{"type":"object","properties":{"kind":{"enum":["press","release","move"]},"button":{"enum":["left",
  "middle","right","wheel_up","wheel_down",null]},"row":{"type":"integer","minimum":0},"col":{…},"ctrl":
  {"type":"boolean"},"alt":{…},"shift":{…}},"required":["kind","row","col"]},"description":"Mouse events at cells of
  the grid (clamped), encoded at home in the program's mode; one it didn't ask for is dropped. 1 to 64."}`; the tool's
  description adds "or clicks and scrolls as their mouse does (mouse: only what the program asked for)". `api_input`'s
  arm: on the pane (`pane_mut(&worker)`) count the events `term.mouse` takes; when any, `scrolled.set(true)` (as the
  local `report` does) and `keys_from = Some((by, now))`; `repaint_after_answered`, `cx.notify()`; answer now,
  "Sent 1 mouse event to {block}." / "Sent {n} mouse events to {block}.".
- [ ] **Step 4: Run them to see them pass,** and `cargo test -j 4 -p templum-api --lib registry` (the test that
  classifies every argument passes with `"mouse"` in `NOT_TARGETS`) → pass.
- [ ] **Step 5: Docs.** `docs/api.md` table row: `` `terminal_input` | `block`, then `text` (a paste), `keys` (1 to
  256, [a key's name](#a-keys-name)) or `mouse` (1 to 64 events) | full ``. **Input.**: "No resize, and no mouse."
  becomes "No resize."; add after it: "`{"mouse":[{"kind":"press","button":"left","row":3,"col":10},{"kind":"release",
  "button":"left","row":3,"col":10}]}` clicks as your mouse does: `kind` `press`, `release` or `move`; `button`
  `left`, `middle`, `right`, `wheel_up`, `wheel_down`, or null for a move with none held; `row` and `col` a cell of the
  grid (clamped); `ctrl`, `alt`, `shift` optional. Home encodes each in the program's mode at that moment and drops
  the ones it didn't ask for (`modes`); the answer counts those taken (`Sent 2 mouse events to b7k2m.`). One of
  `text`, `keys`, `mouse` a call, 1 to 64 events." `docs/minors.md`: delete the entry "No mouse input through the
  API.".

### Task 3: A viewer's history, filled above its live screen

**Files:**
- Modify: `crates/templum-terminal/src/terminal.rs` (`Shared.fed`, `VIEWER_HISTORY`, `Feeder::{fill_history,
  drop_history, fed}`), `crates/templum-terminal/src/lib.rs` (export `VIEWER_HISTORY`)
- Modify: `crates/templum-api/src/client/frames.rs` (`Painter::bytes`: a whole frame clears with `\x1b[H\x1b[J`)
- Test: `crates/templum-terminal/src/terminal.rs` (tests), `crates/templum-api/src/client/frames.rs` (tests)

**Interfaces:**
- Consumes: alacritty's `Grid::{update_history, display_offset, scroll_display, cursor, IndexMut<Line>}`,
  `Term::grid_mut`, the vte `Processor` as `Feeder::feed` uses it.
- Produces: `pub const VIEWER_HISTORY: usize = 2000;` `Feeder::fill_history(&self, text: &str) -> bool` (false, and
  nothing changed, while the viewer is in the alternate screen); `Feeder::drop_history(&self)`; `Feeder::fed(&self) ->
  u64` (how many times `feed` ran: frames, never a fill). Task 5 calls all three.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum-terminal/src/terminal.rs, tests
/// A viewer as a mirror makes one: no scrollback of its own, home's rows written without autowrap.
fn mirrored(cols: u16, rows: u16, screen: &[u8]) -> Terminal {
    let options = TermOptions { scrollback: 0, ..TermOptions::default() };
    let v = Terminal::viewer("v", (cols, rows), Palette::default(), options, Arc::new(|| {}));
    v.feed(b"\x1b[?7l");
    v.feed(screen);
    v
}

#[test]
fn history_goes_above_the_live_screen_and_the_screen_stays() {
    let v = mirrored(10, 4, b"s1\r\ns2\r\ns3\r\ns4");
    let before = v.shared().screen().unwrap();
    assert!(v.feeder().fill_history("h1\nh2\nh3\ns1\ns2\ns3\ns4"));
    assert_eq!(v.shared().lines(100), ["h1", "h2", "h3", "s1", "s2", "s3", "s4"]);
    assert_eq!(v.shared().screen().unwrap(), before, "the live screen and its cursor stay");
}

#[test]
fn history_above_a_screen_that_isnt_full_loses_and_doubles_nothing() {
    let v = mirrored(10, 4, b"s1\r\ns2\r\n");
    let before = v.shared().screen().unwrap();
    // Home's text ends with its screen down to the last row written; a trailing blank line is tolerated.
    assert!(v.feeder().fill_history("h1\nh2\nh3\ns1\ns2\n"));
    assert_eq!(v.shared().lines(100), ["h1", "h2", "h3", "s1", "s2"]);
    assert_eq!(v.shared().screen().unwrap(), before);
    let v = mirrored(10, 4, b"s1\r\ns2");
    assert!(v.feeder().fill_history("h1\ns1\ns2"));
    assert_eq!(v.shared().lines(100), ["h1", "s1", "s2"]);
}

#[test]
fn long_lines_wrap_again_at_home_s_width() {
    let v = mirrored(10, 4, b"s1");
    assert!(v.feeder().fill_history("abcdefghijklmno\ns1"));
    assert_eq!(v.shared().lines(100), ["abcdefghijklmno", "s1"]);
}

#[test]
fn a_viewer_in_the_alternate_screen_takes_no_history() {
    let v = mirrored(10, 4, b"\x1b[?1049hvim");
    assert!(!v.feeder().fill_history("h1\nvim"));
    v.feed(b"\x1b[?1049l");
    assert!(!v.shared().lines(100).iter().any(|l| l == "h1"));
}

#[test]
fn history_from_home_is_drawn_never_run() {
    let v = mirrored(40, 4, b"s1");
    assert!(v.feeder().fill_history("\x1b]2;evil\x07a\x1b[?1049hb\x07\nh2\ns1"));
    let shared = v.shared();
    assert_eq!(shared.lines(100), ["]2;evila[?1049hb", "h2", "s1"], "shown as text, not run");
    assert_eq!(shared.screen().unwrap().title, None);
    assert!(!shared.modes().full_screen);
}

#[test]
fn history_keeps_at_most_two_thousand_rows() {
    let v = mirrored(10, 4, b"");
    let text: Vec<String> = (0..3000).map(|n| n.to_string()).collect();
    assert!(v.feeder().fill_history(&text.join("\n")));
    let lines = v.shared().lines(10_000);
    assert!(lines.len() <= VIEWER_HISTORY, "{}", lines.len());
    assert_eq!(lines.last().map(String::as_str), Some("2999"));
}

#[test]
fn a_fill_keeps_the_view_where_the_person_left_it() {
    let v = mirrored(10, 4, b"s1");
    let text: Vec<String> = (1..=30).map(|n| format!("h{n}")).chain(["s1".into()]).collect();
    assert!(v.feeder().fill_history(&text.join("\n")));
    v.scroll(5);
    v.shared().select_start(0, 0, Side::Left, SelectionKind::Line);
    assert!(v.feeder().fill_history(&text.join("\n")));
    assert!(v.scrolled_back(), "still scrolled back");
    assert_eq!(v.shared().selection_text(), None, "a selection in the old history goes");
    v.feeder().drop_history();
    assert!(!v.scrolled_back());
    assert_eq!(v.shared().lines(100), ["s1"]);
}

#[test]
fn fed_counts_what_the_painter_feeds_never_a_fill() {
    let v = mirrored(10, 4, b"");
    let f = v.feeder();
    let n = f.fed();
    f.feed(b"x");
    assert_eq!(f.fed(), n + 1);
    assert!(f.fill_history("h1\nx"));
    assert_eq!(f.fed(), n + 1, "a history isn't a frame");
}

// crates/templum-api/src/client/frames.rs, tests
#[test]
fn a_whole_frame_keeps_the_viewer_s_history() {
    let mirror = viewer(10, 2);
    let mut painter = Painter::default();
    painter.paint(&frame_of(10, 2, vec![run("old")]), &mirror.feeder()).unwrap();
    assert!(mirror.feeder().fill_history("h1\nh2\nold"));
    painter.paint(&frame_of(10, 2, vec![run("new")]), &mirror.feeder()).unwrap();
    assert_eq!(mirror.shared().lines(100), ["h1", "h2", "new"], "the old screen isn't pushed into the history");
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-terminal history` and `fed_counts` →
  compile errors; `cargo test -j 4 -p templum-api --lib a_whole_frame_keeps` → compile error.
- [ ] **Step 3: Implement `Feeder::fill_history`** (the algorithm is the plan's: the signature and tests leave the
  row math open). Under the grid's lock (`lock()`, as `feed`):
  1. In the alternate screen: return false.
  2. Keep the live screen: `used`, the rows from the top down to the last one holding a character other than a space
     (0 for a blank screen), each screen row (`grid[Line(y)].clone()`), `grid.cursor`, `display_offset()`.
  3. `grid_mut().update_history(VIEWER_HISTORY)`; parse `\x1b[3J\x1b[H\x1b[J\x1b[0m\x1b[?7h` and the text: its lines
     (`str::lines`), trailing blank lines dropped, the last `VIEWER_HISTORY + rows` of them, each without control
     characters but `\t`, joined with `\r\n` (autowrap on, so a long line wraps at home's width again).
  4. When the text wasn't empty: the cursor is on the text's last row `r`; parse `\x1b[{k}S` with `k = r + 1 − used`
     when `k > 0` (the text ends with home's screen down to its last written row, which the live screen draws; `CSI S`
     on the whole screen puts the rows it scrolls into the history).
  5. Parse `\x1b[?7l` (the painter's), put the kept rows and cursor back, `scroll_display(Scroll::Delta(offset))`,
     `term.selection = None`; then `touched()` and a wake (as `feed`). Return true.

  `drop_history`: parse `\x1b[3J`, `grid_mut().update_history(0)`, `touched()`. `Shared` gains `fed: AtomicU64`,
  `fetch_add`ed in `Feeder::feed` only; `Feeder::fed` loads it. `Painter::bytes`' whole frame: `\x1b[H\x1b[J`
  (alacritty's `CSI 2J` on the main screen scrolls the old screen into the history, `clear_viewport`; `CSI J` from the
  top left clears it in place).
- [ ] **Step 4: Run them to see them pass,** and the modules: `cargo test -j 4 -p templum-terminal`, `cargo test -j 4
  -p templum-api --lib client::frames` → pass.

### Task 4: The mouse rides the typing queue

**Files:**
- Modify: `crates/templum-api/src/client/mod.rs` (`Command::Mouse`), `crates/templum-api/src/client/remote.rs`
  (`Item::Mouse`, `Life::type_mouse`, `pump`)
- Test: `crates/templum-api/tests/mirror.rs`

**Interfaces:**
- Consumes: Task 2's `registry::{INPUT_MOUSE, mouse_value}`.
- Produces: `Command::Mouse { block: String, event: MouseInput }` (Task 6 sends it). A mouse event while not live is
  refused as typing is (`not_live`); past `WAITING_MOST` it is `Update::Dropped(block)` as keys are.

- [ ] **Step 1: Write the failing test**

```rust
// crates/templum-api/tests/mirror.rs
fn mouse(kind: MouseKind, button: Option<Button>, row: usize, col: usize) -> MouseInput {
    MouseInput { button, kind, row, col, ctrl: false, alt: false, shift: false }
}

#[test]
fn mouse_events_ride_the_typing_queue_in_order_and_waiting_moves_merge() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let mirror = live_on(&fake, &text);
    let send = |command| mirror.send("r1", command);
    let keys = |k: &str| Command::Keys { block: "b7k2m".into(), keys: vec![k.into()] };
    let event = |event| Command::Mouse { block: "b7k2m".into(), event };
    let (left, inputs) = (Some(Button::Left), || calls_of(&fake, "terminal_input"));
    fake.hold_calls.store(true, Ordering::SeqCst);
    send(keys("a"));
    wait_for("the first call", || inputs().len() == 1);
    let (down, up) = (mouse(MouseKind::Press, left, 1, 1), mouse(MouseKind::Release, left, 1, 4));
    send(event(down));
    for col in 2..=4 {
        send(event(mouse(MouseKind::Move, left, 1, col)));
    }
    send(event(up));
    send(keys("b"));
    barrier(&mirror);
    fake.release_calls();
    wait_for("the rest", || inputs().len() == 3);
    let calls = inputs();
    let moved = mouse(MouseKind::Move, left, 1, 4);
    assert_eq!(calls[1]["mouse"], json!([mouse_value(&down), mouse_value(&moved), mouse_value(&up)]),
        "in order, the waiting moves merged into the newest");
    assert_eq!(calls[2]["keys"], json!(["b"]), "the key typed after them goes after them");
    // Seventy steps of the wheel: 64 a call at most.
    fake.hold_calls.store(true, Ordering::SeqCst);
    send(keys("c"));
    wait_for("the held call", || inputs().len() == 4);
    for _ in 0..70 {
        send(event(mouse(MouseKind::Press, Some(Button::WheelUp), 0, 0)));
    }
    barrier(&mirror);
    fake.release_calls();
    wait_for("the wheel", || inputs().len() == 6);
    let sizes: Vec<usize> = inputs()[4..].iter().map(|c| c["mouse"].as_array().unwrap().len()).collect();
    assert_eq!(sizes, [64, 6]);
}
```

- [ ] **Step 2: Run it to see it fail:** `cargo test -j 4 -p templum-api --test mirror mouse_events_ride` → compile
  error (no `Command::Mouse`).
- [ ] **Step 3: Implement.** `Item::Mouse(MouseInput)`. `Life::type_mouse(block, event)`: not live → `not_live`; a
  `Move` whose queue's last item is an `Item::Mouse` `Move` replaces it; else pushed while `queue.keys <
  WAITING_MOST` (the count now holds keys and mouse events: say so in its doc comment), else `said_dropped`. `pump`:
  an `Item::Mouse` at the front takes the mouse events that follow it, up to `INPUT_MOUSE`, as
  `{"block": …, "mouse": [mouse_value(e), …]}`. `Command::Mouse` → `type_mouse`.
- [ ] **Step 4: Run it to see it pass,** and the typing tests beside it: `cargo test -j 4 -p templum-api --test
  mirror` → pass.

### Task 5: Home's history, read and filled by the client

**Files:**
- Modify: `crates/templum-api/src/client/mod.rs` (`Command::History`, `Update::History`),
  `crates/templum-api/src/client/remote.rs` (`To::History`, `Life::{history, fill}`, the history's end)
- Modify: `crates/templum/src/workspace/remote.rs` (`heard`: `Update::History(_)` joins the "said above" arm, so the
  glue compiles; Task 7 hears it)
- Modify: `crates/templum-api/tests/common/mod.rs` (the fake answers `read_terminal` from a lent terminal)
- Test: `crates/templum-api/tests/mirror.rs`

**Interfaces:**
- Consumes: Task 3's `Feeder::{fill_history, drop_history, fed}`; `templum_mcp::tools::{tail, MAX_TEXT, MAX_LINES}`;
  `Life::call(Job)` and its quick lane (the 8 MiB cap for `read_terminal` is 2a's).
- Produces: `Command::History { block: String }`: read home's history of that streamed terminal into its viewer,
  unless it holds it already and no frame was fed since; `Update::History(String)`: the viewer of that block holds
  home's history now (Task 7 searches again). `Life` keeps `history: Option<(String, Feeder, u64)>` (block, viewer,
  `fed()` at the fill) and `history_asked: Option<String>`.

- [ ] **Step 1: Write the failing tests**

In the fake (`common/mod.rs`), after the `held_tool` arm: `"tools/call" if name == "read_terminal"` answers the lent
terminal whose block is `arguments.block` with `Reply::ok(term.lines(arguments.lines or 200).join("\n"))`, else
`not_found` as `TERMINAL` does.

```rust
// crates/templum-api/tests/mirror.rs
/// A home terminal lent as `block`, its output `lines` lines `{prefix} 1.` …, and the viewer it streams to.
fn streamed_with_lines(fake: &Fake, block: &str, prefix: &str, lines: usize) -> (Terminal, Terminal) {
    let home = viewer(20, 4);
    lend(fake, block, "w3", &home);
    let out: Vec<String> = (1..=lines).map(|n| format!("{prefix} {n}.")).collect();
    home.feed(out.join("\r\n").as_bytes());
    (home, viewer(20, 4))
}

fn history_of(mirror: &Mirror, block: &str) {
    until(mirror, "r1", "the history", |u| matches!(u, Update::History(b) if b == block));
}

#[test]
fn home_s_history_fills_the_viewer_once_until_a_frame_comes() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (home, shown) = streamed_with_lines(&fake, "b7k2m", "row", 30);
    let mirror = live_on(&fake, &text);
    mirror.send("r1", Command::Terminals(vec![("b7k2m".into(), shown.feeder())]));
    wait_for("the live screen", || shown.shared().lines(1) == ["row 30."]);
    let back = || mirror.send("r1", Command::History { block: "b7k2m".into() });
    back();
    history_of(&mirror, "b7k2m");
    let all = shown.shared().lines(100);
    assert_eq!(all.len(), 30, "{all:?}");
    assert_eq!((all[0].as_str(), all[29].as_str()), ("row 1.", "row 30."));
    assert_eq!(calls_of(&fake, "read_terminal"), [json!({"block": "b7k2m", "lines": 2000})]);
    back();
    settled(&mirror);
    assert_eq!(calls_of(&fake, "read_terminal").len(), 1, "nothing changed: not read again");
    home.feed(b"\r\nrow 31.");
    wait_for("the new line", || shown.shared().lines(1) == ["row 31."]);
    back();
    wait_for("read again after a frame", || calls_of(&fake, "read_terminal").len() == 2);
}

#[test]
fn one_terminal_s_history_at_a_time_and_it_goes_with_its_stream() {
    let (text, token) = share_token(Scope::Full, "w3");
    let fake = fake(vec![token], |_| {});
    let (_a_home, a) = streamed_with_lines(&fake, "aaaaa", "a", 10);
    let (_b_home, b) = streamed_with_lines(&fake, "bbbbb", "b", 10);
    let mirror = live_on(&fake, &text);
    mirror.send("r1", Command::Terminals(vec![("aaaaa".into(), a.feeder()), ("bbbbb".into(), b.feeder())]));
    wait_for("both live", || a.shared().lines(1) == ["a 10."] && b.shared().lines(1) == ["b 10."]);
    let back = |block: &str| mirror.send("r1", Command::History { block: block.into() });
    // An answer for a terminal no longer asked for fills nothing (the held one answers "did read_terminal").
    fake.hold_tool("read_terminal");
    back("aaaaa");
    wait_for("a's read held", || fake.held() == 1);
    back("bbbbb");
    fake.release_tool();
    history_of(&mirror, "bbbbb");
    assert_eq!(a.shared().lines(100).len(), 4, "{:?}", a.shared().lines(100));
    assert_eq!(b.shared().lines(100).len(), 10);
    // Scrolling back in a drops b's.
    back("aaaaa");
    history_of(&mirror, "aaaaa");
    assert_eq!((a.shared().lines(100).len(), b.shared().lines(100).len()), (10, 4));
    // a's stream ends: its history goes.
    mirror.send("r1", Command::Terminals(vec![("bbbbb".into(), b.feeder())]));
    wait_for("a's history dropped", || a.shared().lines(100).len() == 4);
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-api --test mirror history` → compile errors.
- [ ] **Step 3: Implement.** `Life::history(block)`: no stream for the block → nothing; the history held is this
  block's, the same viewer (`same_viewer`) and its `fed()` unchanged → nothing; else `history_asked = Some(block)` and
  `call(Job { call: read_terminal {"block", "lines": MAX_LINES}, to: To::History(block) })`. `called`:
  `(To::History(block), Ok(json))` → `fill(block, json)`; an error → nothing. `refuse(To::History(_), …)` → nothing
  (a scroll back says nothing when it can't read). `fill`: only when `history_asked` is this block and its stream is
  open; the text is `json["text"]` cut by `tools::tail(text, MAX_TEXT)`; a history held for another viewer is dropped
  (`drop_history`) first; when `feeder.fill_history(text)`, keep `(block, feeder, feeder.fed())` and say
  `Update::History(block)`, else keep none. The end: after `sync_terminals`' `retain` and in `drop_attempt` (which
  clears `streams`), a history whose block has no stream for its viewer is dropped (`drop_history`) and forgotten.
  `Command::History` → `history`. In the glue's `heard`, `Update::History(_)` joins the second match's "said above"
  arm (Task 7 replaces that with its handling).
- [ ] **Step 4: Run them to see them pass,** and the file: `cargo test -j 4 -p templum-api --test mirror` → pass;
  `cargo build -j 4 -p templum` → builds.

### Task 6: The mouse in a remote terminal

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Deed::Mouse`: its call and `needs`; `MOUSE_NEEDS_FULL`;
  `Workspace::{remote_mouse, tell_mouse}`)
- Modify: `crates/templum/src/workspace/input.rs` (`report`'s remote branch; `press_program` takes `window`, `cx` and
  tells), `crates/templum/src/block.rs` (`press_program`'s call)
- Test: `crates/templum/src/workspace/remote.rs` (tests), `crates/templum/src/workspace/tests/remote.rs`

**Interfaces:**
- Consumes: Task 1's `Modes::wants` and the viewer's modes; Task 4's `Command::Mouse`; `RemoteSpace::{why_not,
  told}`; `mirror_send`.
- Produces: `Deed::Mouse` → `("terminal_input", {"block":"b","mouse":[{"kind":"press","button":"left","row":0,
  "col":0}]})`, `needs` `["mouse"]`; `Workspace::remote_mouse(&self, id: BlockId, input: MouseInput) -> bool` (sent);
  `Workspace::tell_mouse(&mut self, id: BlockId, window, cx)`. `report` now reaches home for a remote pane, so
  `press_program`, `release_program`, `drag_to`'s program drag, `hover_program` and the wheel (Task 7) need nothing
  else.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum/src/workspace/remote.rs, tests: in every_deed_home_runs_has_a_scope_in_the_registry
assert_eq!(scope(Deed::Mouse), Scope::Full);
// and a new one
#[test]
fn the_mouse_needs_a_home_that_takes_it() {
    let mut remote = remote_at("https://winx-desktop.example.com", Status::Live);
    remote.me = Some(MeView { scope: Scope::Full, ..MeView::default() });
    let input = |props: serde_json::Value| Tools::read(&serde_json::json!([
        { "name": "terminal_input", "inputSchema": { "properties": props } }
    ]));
    remote.tools = Some(input(serde_json::json!({ "block": {}, "text": {}, "keys": {} })));
    assert_eq!(remote.why_not(Deed::Mouse), Some(Nay::Older));
    assert_eq!(remote.why_not(Deed::Type), None, "typing still works");
    remote.tools = Some(input(serde_json::json!({ "block": {}, "mouse": {} })));
    assert_eq!(remote.why_not(Deed::Mouse), None);
}
```

```rust
// crates/templum/src/workspace/tests/remote.rs
#[gpui_kit::test]
fn a_click_reaches_a_program_at_home_that_asked_for_the_mouse(cx: &mut TestAppContext) {
    // shown_mirror; viewer = focus_viewer; home's terminal grid fed "\x1b[?1000h\x1b[?1006h" (`term.feed`: a program's
    // output); pump_home (draw each round) until the viewer's `term.shared().modes().mouse`. before = home's inputs().
    // The body's bounds (`window.find(("body", viewer.0 as usize))`); `click(window, cx, (x + 20, y + 20), 1)`.
    // pump_home until inputs == before + 2 (the press and the release). said(&ws) is empty. (The wheel's half is Task
    // 7's: the wheel over a remote terminal is routed there.)
}

#[gpui_kit::test]
fn a_control_share_s_press_says_once_and_selects_text(cx: &mut TestAppContext) {
    // shown_mirror_as(Scope::Control); the same mouse mode at home, pumped until the viewer has it. A left MouseDown
    // in the body → `matches!(ws.drag, Some(Drag::Select(_)))` (the press selects, as with Shift locally); MouseUp;
    // a second press and release. said(&ws) == ["Using the mouse in a terminal needs a Full share: share again on
    // {computer}."] (computer_of the remote), once.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum workspace::remote::tests::the_mouse_needs`,
  `workspace::remote::tests::every_deed`, `workspace::tests::remote::a_click_reaches`,
  `workspace::tests::remote::a_control_share_s_press` → compile errors, then failures (today a press on a remote
  terminal whose program asked for the mouse is swallowed: the viewer "takes" it and sends nothing).
- [ ] **Step 3: Implement.** `report`: for a pane with `remote`, return `self.remote_mouse(id, input)` instead of
  `term.mouse` (keep `scrolled.set(true)` when sent). `remote_mouse`: the pane's `RemotePane` not `read_only`, the
  viewer's `modes().wants(&input)`, the shown remote's `why_not(Deed::Mouse)` None → `mirror_send(space id,
  Command::Mouse { block: home's id, event: input })`, true; else false. `press_program(id, e, window, cx)`: before
  `report`, for a remote pane whose viewer's modes have `mouse`, no Shift held, call `tell_mouse(id, window, cx)`;
  `tell_mouse` says, once per terminal through `RemoteSpace::told` (`(home's id, tag)`), `NeedsFull` as "Using the
  mouse in a terminal needs a Full share: share again on {computer}." (tag `MOUSE_NEEDS_FULL = "mouse needs full"`)
  and `Older` as `Nay::Older`'s words (tag `OLDER`, shared with typing); any other answer says nothing (not live: the
  banner says it). `block.rs` passes `window, cx`.
- [ ] **Step 4: Run them to see them pass,** and `cargo test -j 4 -p templum workspace::tests::remote` → pass.

### Task 7: Scrolling back in a remote terminal

**Files:**
- Modify: `crates/templum/src/workspace/remote.rs` (`Deed::ScrollBack`; `Workspace::{remote_history, remote_scroll,
  remote_page, history_came, typing_into}`; `typing_at` calls `typing_into`; `remote_type`/`remote_paste` bring the
  viewer to the bottom; `heard` hears `Update::History`; delete `remote_wheel`, `RemoteSpace::wheel_told`;
  `NOT_MIRRORED`)
- Modify: `crates/templum/src/workspace/input.rs` (`scroll_terminal` takes `window` and falls back to `remote_scroll`
  for a remote pane; `on_key_down`'s remote branch asks `remote_page` first), `crates/templum/src/workspace/find.rs`
  (`open_find` asks `remote_history`; `search_again`), `crates/templum/src/block.rs` (the wheel calls
  `scroll_terminal` for every terminal)
- Test: `crates/templum/src/workspace/remote.rs` (tests), `crates/templum/src/workspace/tests/remote.rs` (delete
  `the_wheel_over_a_remote_terminal_says_so_once_and_scrolls_nothing`)

**Interfaces:**
- Consumes: Task 5's `Command::History`, `Update::History`; Task 6's `report`; `VIEWER_HISTORY`.
- Produces: `Deed::ScrollBack` → `("read_terminal", {"block":"b"})` (read); `Workspace::remote_history(&mut self, id:
  BlockId)` (sends `Command::History` when the viewer's modes have neither `full_screen` nor `mouse` and the rule
  says yes; the client decides whether it needs reading); `Workspace::typing_into(&mut self, id: BlockId, window, cx)
  -> Option<(String, String)>` (what `typing_at` did, for any remote terminal, not only the focused one);
  `Workspace::search_again(&mut self, id: BlockId, cx)` (find.rs: an open Find with text searches `Toward::Here`).

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum/src/workspace/remote.rs, tests: in every_deed_home_runs_has_a_scope_in_the_registry
assert_eq!(scope(Deed::ScrollBack), Scope::Read);
```

```rust
// crates/templum/src/workspace/tests/remote.rs
/// Home's terminal shows `n` lines `row 1.` … as a program's output (fed into its grid).
fn home_printed(home: &Entity<Workspace>, cx: &mut TestAppContext, terminal: BlockId, n: usize) { /* term.feed */ }

#[gpui_kit::test]
fn the_wheel_over_a_remote_shell_reads_home_s_history_and_scrolls_in_it(cx: &mut TestAppContext) {
    // shown_mirror; viewer = focus_viewer; home_printed(60); pump_home (draw) until shown(viewer) has "row 60.".
    // The wheel up one step over the body (tests' `wheel`, Lines(point(0., 1.))): pump_home until the viewer's
    // `term.shared().lines(200)` contains "row 1."; then the wheel up again → the viewer `scrolled_back()`.
    // said(&ws) is empty (no "Only the live screen is mirrored for now.").
    // Typing brings it back: `window.press("enter")` → the viewer is no longer scrolled_back().
}

#[gpui_kit::test]
fn the_wheel_over_a_program_at_home_is_its_mouse_or_arrow_keys(cx: &mut TestAppContext) {
    // shown_mirror; focus_viewer. 1. Home's grid fed "\x1b[?1049h" (full screen, no mouse); pump until the viewer's
    // modes().full_screen; before = home's inputs(); the wheel up two steps (Lines(point(0., 2.))) → pump_home until
    // inputs == before + 2 (two arrow keys). 2. Home's grid fed "\x1b[?1049l\x1b[?1000h\x1b[?1006h" (the main screen,
    // the mouse); pump until the viewer's modes() say mouse and not full_screen; the wheel up one step → pump_home
    // until inputs == before + 3 (a wheel press; a viewer scrolled locally would send nothing).
}

#[gpui_kit::test]
fn shift_page_up_pages_back_through_home_s_history_and_never_reaches_home(cx: &mut TestAppContext) {
    // shown_mirror; focus_viewer; home_printed(60); pump until the viewer shows "row 60."; before = home's inputs().
    // window.press("shift-pageup") → pump until the viewer's lines(200) contain "row 1."; press "shift-pageup" again
    // → scrolled_back(); "shift-pagedown" twice → not scrolled_back(). Then `cx.simulate_input(handle, "x")` → pump
    // until inputs == before + 1, and it stays + 1 (the keys typed go in order: a Shift+PageUp sent home would have
    // come before the x).
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum workspace::remote::tests::every_deed` and
  `cargo test -j 4 -p templum workspace::tests::remote::the_wheel_` / `shift_page_up` → compile error, then the wheel
  test fails on "row 1." (the wheel says "Only the live screen…").
- [ ] **Step 3: Implement.**
  - `remote_history(id)`: the pane's viewer modes have neither `full_screen` nor `mouse`, the shown remote's
    `why_not(Deed::ScrollBack)` is None → `mirror_send(space, Command::History { block })`.
  - `scroll_terminal(id, e, window, cx)`: unchanged for a local pane. When `report` didn't take the wheel and the pane
    is remote, `remote_scroll(id, lines, window, cx)`: with `full_screen`, `typing_into(id)` then
    `Command::Keys` of `"up"`/`"down"` × `|lines|`; else, scrolling up while not `scrolled_back()` →
    `remote_history(id)`; then `scrolled.set(true)` and `term.scroll(lines)` (what the viewer has, as it comes).
  - `remote_page(&mut self, up: bool) -> bool`, which `on_key_down`'s remote branch calls for Shift alone (no Ctrl, no
    Alt) with `pageup` (`up`) or `pagedown`, before naming the key: when the focused viewer's modes have neither
    `full_screen` nor `mouse`, `pageup` while not `scrolled_back()` asks `remote_history`, then `term.scroll(±rows)`
    (the viewer's rows) and true (the key stops there, `cx.stop_propagation()`); otherwise false, and the key goes home
    as before.
  - `open_find`: after the field opens on a remote pane, `remote_history(id)`.
  - `heard`: `Update::History(block)` (first match, as `Notice`) → `history_came(id, &block, cx)`: when remote `id` is
    the shown space and `homes` has the block, `search_again(viewer, cx)`; `cx.notify()`.
  - `typing_into(id)`: `typing_at`'s body for the block `id`; `typing_at` = `typing_into(self.space.focused?)`.
    `remote_type` and `remote_paste`, once allowed: when the focused viewer `scrolled_back()`,
    `term.scroll(-(VIEWER_HISTORY as i32))` (to the bottom, as typing locally does).
  - `block.rs`'s wheel: `ws.scroll_terminal(id, e, window, cx)` for every terminal (the remote check moves inside).
    Delete `remote_wheel`, `wheel_told` and the test that asserted them.
  - `NOT_MIRRORED`: "Not mirrored yet: notes', checklists', editors' and boards' content (a card shows each), ink and
    shapes. Not on a mirrored workspace yet: editors, adding notes, drawing, grid mode, Move to workspace, Copy and
    Paste of blocks, Stop, the Git popup." (a test that reads the ⓘ follows it).
- [ ] **Step 4: Run them to see them pass,** and `cargo test -j 4 -p templum workspace::tests::remote` and
  `cargo test -j 4 -p templum workspace::tests::terminals` (the local wheel and Find unchanged) → pass.

### Task 8: End to end, the docs, the full check and the benches

**Files:**
- Test: `crates/templum/src/workspace/tests/mirror_end_to_end.rs` (two cases, both `#[cfg(unix)]`)
- Modify: `docs/api.md` (Templum to Templum ▸ 7. What a mirror can't do yet), `docs/specs/api.md` (`POST
  /v1/terminals/{block}/input`: "or `{"mouse":[…]}`, encoded at home"), `docs/specs/templum-to-templum.md` (Status:
  "part 2c built (plan 2026-10-25-t2t-batch-2c-terminals)"; "As built ▸ Batch 2c": what the builder chose or changed,
  and this plan's "Open for the orchestrator" as settled), `docs/ARCHITECTURE.md` (a mirrored terminal's mouse goes
  home as `terminal_input`'s `mouse`; its history is filled on the client's thread, `Feeder::fill_history`),
  `docs/minors.md`

- [ ] **Step 1: Write the end-to-end cases**

```rust
#[cfg(unix)]
#[gpui_kit::test]
fn three_hundred_lines_printed_at_home_are_found_from_the_mirror(cx: &mut TestAppContext) {
    // As what_home_s_shell_prints_shows_on_the_mirror_and_keys_reach_it: open_as("sh"), mirrored, the viewer focused
    // at 100%. home_shell(cx, "i=1; while [ $i -le 300 ]; do echo \"row $i.\"; i=$((i+1)); done\r"); pump_home until
    // printed(cx) has "row 300." and shown(viewer) has "row 300.". On r1: ws.open_find(window, cx);
    // cx.simulate_input(handle, "row 1."); pump_home (draw) until the viewer's
    // `term.shared().selection_text().as_deref() == Some("row 1.")` (only in home's history: the search runs again
    // when it lands).
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_click_on_the_mirror_reaches_a_program_at_home_that_asked_for_the_mouse(cx: &mut TestAppContext) {
    // The same setup. home_shell(cx, "stty -icanon -echo min 1; printf '\\033[?1000h\\033[?1006h'; \
    //   head -c 5 | cat -v; echo; printf '\\033[?1000l\\033[?1006l'; stty sane\r");
    // pump_home (draw) until the viewer's modes().mouse. Click its body (tests' `click`, 20 px in).
    // pump_home until printed(cx) has a line containing "^[[<0;" (a left press, SGR, as the program read it).
}
```

- [ ] **Step 2: Run them:** `cargo test -j 4 -p templum workspace::tests::mirror_end_to_end` → all pass (fix what
  fails in the task that owns it).
- [ ] **Step 3: Write the docs** above. `docs/api.md` ▸ 7: drop "scrolling back in a terminal, the mouse in a
  terminal" from "Not mirrored"; add "Scrolling back in a mirrored shell (the wheel, Shift+PageUp, Find) reads home's
  last 2000 lines, without colors, and again after it changed; the mouse reaches a program that asked for it, with a
  Full share." `docs/minors.md`, add: "Shift+PageUp and Shift+PageDown scroll a mirrored shell but not a local one"
  (`workspace/input.rs`, `remote_page`; locally they reach the program as keys; fix: page a local terminal's history
  the same way when no program took the screen or the mouse); "A mirrored terminal's history is read whole after any
  change" (`client/remote.rs`, `Life::history`; up to 256 KiB per scroll back of a busy terminal; fix: a
  `read_terminal` from a line on); "Drags are sent to a program that asked only for clicks"
  (`workspace/remote.rs`, `remote_mouse`; the frame doesn't say `?1002`, home drops them, merged moves keep it to one
  call per pace; fix: a `mouse_drags` mode); and the mouse and the wheel over a remote terminal checked headlessly
  only (unless looked at on a display).
- [ ] **Step 4: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env
  run `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 5: The benches** (`templum-terminal` and the mirror's painter changed): `cargo run -j 4 --release -p
  templum -- --bench` then `cargo run -j 4 --release -p templum -- --bench-mirror` → `p95_ms` ≤ 16.7 each; report
  both JSON lines.
- [ ] **Step 6: Leave uncommitted;** report the files changed, the checks' and the benches' results, what went to and
  left `docs/minors.md`, and how each UI-only change was verified.

---

## Interfaces for later parts

What 2c leaves for 2d–2h, by name:

- **The rule:** `Deed::Mouse` (Full, `needs ["mouse"]`) and `Deed::ScrollBack` (read). `RemoteSpace::told`'s new tag
  `MOUSE_NEEDS_FULL`. Nothing later needs another terminal deed.
- **Typing into any remote terminal:** `Workspace::typing_into(id, window, cx)` (not only the focused one): 2g's
  "Run in a terminal" stays left out (Left out 11), but anything that types into a named remote terminal uses it.
- **The client:** `Command::Mouse`, `Command::History`, `Update::History`; `To::History` shows how a call whose
  answer the UI must never hold is handled on the client's thread (2g's diffs go to the UI as answers, since the panel
  draws them: not this way).
- **Home:** `registry::{INPUT_MOUSE, mouse_value, mouse_of}`, `Input::Mouse`; `ModesView::mouse_moves`.
- **templum-terminal:** `Modes::{mouse_moves, wants}`; `Feeder::{fill_history, drop_history, fed}`, `VIEWER_HISTORY`.
- **Gone:** `Workspace::remote_wheel`, `RemoteSpace::wheel_told`, "Only the live screen is mirrored for now.".
- **Benches:** 2d's `--bench-mirror` compares with Task 8's line.

## Open for the orchestrator

Each decided with the recommended option; the plan is written that way.

1. **The history keeps the live screen in place** instead of "then reopens the stream for a whole frame": the fill
   keeps the viewer's screen rows and cursor and puts them back after the history is written, so there is no frame of
   uncoloured text, no stream torn down, and no race with the old stream's frames. The spec leaves "how the history
   reaches the viewer's scrollback" to the builder. *Recommended.*
2. **Scrolling back is a client command, not `remote_call`:** the brief says "every remote act through why_not and
   remote_call", but the spec says the UI thread never holds the history's text, and `remote_call`'s `Then` runs on the
   UI thread with home's JSON. It asks `why_not(Deed::ScrollBack)`; typing and the mouse ride the typing queue, as 1d
   built typing. *Recommended.*
3. **"Changed since the last read" is a frame fed to the viewer since the fill** (`Feeder::fed`), checked in the
   client, which holds both the fill and the viewer. `SharedTerm::changes` also counts the person's own scrolls and
   selections, so it would re-read at every look. *Recommended.*
4. **Shift+PageUp/PageDown page a mirrored shell** as the spec says, though a local terminal sends them to its program
   (no local binding exists); a minors entry records the difference. The other ways: bind them locally too (a change a
   person sees on local terminals, out of 2c), or leave them going home (the spec's trigger dropped). *Recommended.*
5. **The history is read only when the modes have neither `full_screen` nor `mouse`** (the spec, literally): Shift and
   the wheel over a program that took the mouse scroll only what the viewer already has. *Recommended.*
6. **A refused press tells only for `NeedsFull` and `Older`,** once per terminal, and selects text; not live says
   nothing (the banner and veil say it). *Recommended.*
7. **The wheel in a full-screen program is arrow keys through typing,** so it needs Full; a Control share is told
   "Typing needs a Full share…" once per terminal, as typing tells it. *Recommended.*
8. **Zero events taken is not a refusal:** "Sent 0 mouse events to b7k2m." (the spec: "the answer counts those
   taken"); the "takes text or keys" sentence becomes "takes one of text, keys or mouse". *Recommended.*
9. **Drag moves are sent whenever `mouse`:** the viewer can't tell `?1002` from `?1000` (the frame says neither);
   home drops what the program didn't ask for, and merged moves cost at most one call per pace. A `mouse_drags` mode
   is a minors entry. *Recommended.*
10. **Typing into a remote terminal scrolled back brings its viewer to the bottom,** as typing locally does
    (`Terminal::write`); otherwise the person types into a screen they can't see. *Recommended.*
11. **A history line keeps its tabs** (`\t` only moves the cursor to home's tab stop) and loses every other control
    character, so columns line up as at home. *Recommended.*
12. **A history read that fails says nothing:** the person keeps the live screen and the next scroll back tries again.
    *Recommended.*

## Found against the code as built

- **`read_terminal` answers history and screen together** (`SharedTerm::lines`: the history, then the screen down to
  its last written row, wrapped lines joined), not history alone: put "above the live screen" as it comes, the screen
  would show twice at the seam, or (when home's screen isn't full) history lines would be lost. Task 3's row math
  places the text so its screen part falls where the live screen draws it.
- **A whole frame's `CSI 2J` would push the screen into a viewer's history** once it has one (alacritty's
  `clear_viewport` on the main screen): Task 3 clears with `CSI H CSI J`.
- **Today a press on a remote terminal whose program asked for the mouse is swallowed:** the painter sets `?1000h` on
  the viewer, so `Terminal::mouse` on the viewer "takes" the press, sends nothing (no PTY) and no selection starts.
  Task 6's remote branch in `report` fixes it.
- **alacritty's mouse modes replace each other** (`?1000h` removes `?1003`, and the other way), so the painter writes
  `mouse` and `mouse_moves` as one pair.
- **Shift+PageUp doesn't scroll a local terminal:** the spec's "as locally" can't hold for it (Open 4).
- **`read_terminal` cuts its answer to 256 KiB at home** (`tools::tail(MAX_TEXT)`), so "2000 lines" is fewer when
  lines are long; the viewer keeps at most 2000 rows either way.
- **The brief's "every remote act through remote_call"** doesn't fit typing, the mouse or the history (Open 2).
