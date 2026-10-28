# Templum Agent Look (Kanban Card 143) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** a Sonnet 5.5 (high) implementer in a Templum block, then an Opus 5.5 (xhigh)
> reviewer, in a loop until the reviewer passes it. The implementer **commits nothing**: everything stays
> uncommitted for the orchestrator.

**Goal:** Kanban card 143: the Templum Agent's pill, chat, rows and approval get a modern, quiet look (bubbles for
the person, Markdown answers with copyable code, one-line tool rows that open, a two-row composer whose Send becomes
Stop, a greeting with four chips, a scroll-to-the-end button, a 720 px column in full screen) with no behavior lost.

**Architecture:** Only `crates/templum/src/chat/` (and `markdown.rs`, one line of `workspace/agent.rs`) changes. A new
`chat/look.rs` holds the spec's scale and three pure functions (`column`, `gap_above`, `fades`, plus `code_fill`), each
unit-tested. `panel.rs` lays every row, the composer and a pinned approval on the column and draws the header, the
tasks drawer, the greeting and the scroll button; `pill.rs`'s wide branch becomes the composer (the model chip moves
there); `rows.rs` redraws each row and gains the tool row's details (`call_lines`); `approval.rs` gets its card. The
engine, the conversation's rows, the control server and every behavior stay as they are.

**Tech Stack:** Rust, GPUI (gpui-fast 0.1.8) + gpui-kit 0.7.3 (pinned): `TextView` (`code_block_actions`), `Clipboard`,
`ShimmerText`, `Spinner`, `Button` (ghost, dropdown), `Textarea::auto_grow`, `Kbd`, `Scrollbar::vertical`,
`ListState` (`is_following_tail`, `is_scrolled_to_end`, `max_offset_for_scrollbar`, `remeasure_items`),
`with_animation` and `with_spring` (both respect `App::reduce_motion`). Every Lucide icon is embedded (`AllAssets`).

**Spec:** [docs/specs/templum-agent-look.md](../specs/templum-agent-look.md): read all of it; what each piece *does*
is [docs/specs/templum-agent.md](../specs/templum-agent.md) (The pill, The chat, Rows, Approvals), which stays true.
Code to read first: `crates/templum/src/chat/{mod,panel,pill,rows,approval}.rs`, `crates/templum/src/markdown.rs`
(`style`), `crates/templum/src/theme.rs` (`Ui`, `on_accent`), `crates/templum/src/motion.rs`, the render site in
`crates/templum/src/workspace/render.rs` (search `crate::chat::`), `workspace/agent.rs` (`new_chat`), and the chat's
tests in `crates/templum/src/workspace/tests/templum_agent.rs` (helpers `open`, `open_chat`, `pill_text`,
`with_fake_engine`, `handed_by_the_templum_agent`; tests from `a_turn_shows_its_rows` on). gpui-kit sources:
`~/.cargo/registry/src/*/gpui-component-0.7.3/src/{clipboard,shimmer}.rs`, `.../src/text/compat.rs`
(`code_block_actions`), `~/.cargo/registry/src/*/gpui-base-0.7.3/src/text/node.rs` (around line 2190: how a code
block draws its language and its actions), `~/.cargo/registry/src/*/gpui-fast-0.1.8/src/elements/list.rs`.

## Global Constraints

- Only `crates/templum` changes code. One new file, `crates/templum/src/chat/look.rs`; tests go in its own `mod
  tests`, in `rows.rs`'s, and in `workspace/tests/templum_agent.rs`. No new dependency, no new theme token, no
  `unsafe`, no change to `crates/templum-agent`.
- Keep to the spec's sizes, tokens and words. Verbatim: "Templum Agent", "New chat", "Full screen", "Exit full
  screen", "Close (Esc)", "Catch me up", "Stop", "Copy", "Queued", "Thinking", "Scroll to the end", "What should
  Templum do?", "It builds and runs your canvas: workspaces, agents, boards and notes. Anything it can't take back
  waits for your click.", the four prompts of the empty state, "Enter to send · Shift+Enter for a new line · Esc to
  close", "Enter to queue it · Shift+Enter for a new line · Esc to close", "⋯ N more lines", "Templum Agent wants
  to", and every string of templum-agent.md that stays.
- `look.rs` holds the spec's scale; add each constant with its first use (clippy denies dead code). Colors are
  `ws.ui` tokens, `fg = crate::grid::hsla(ws.theme.palette.foreground, 1.0)`, red `palette.ansi[1]`, yellow
  `palette.ansi[3]`, and `ws.theme.on_accent()` for anything on the accent (the send arrow was `ui.canvas`).
- **Keep every element id the tests use** on the element that does the same thing; `agent-new-below` becomes
  `agent-to-end`, `agent-engine` goes. When an element moves (Stop, the model chip, Catch me up), the tests that
  click it change only as the task says.
- **Every logic change has a headless test that fails without it.** UI-only looks: verified in Task 9's screenshots.
- **Tests don't depend on HOME, config or installed agent CLIs:** `open(cx)` and the fake engine, as the existing
  chat tests; folders are `tempfile`.
- **Stable under CPU load:** assert on state after `window.render_frame(cx)` (render again where a list or a
  growing field needs a second layout), never after a sleep.
- **Fixed-width text is clipped:** one-line text gets `min_w(px(0.))`, `overflow_hidden`, `whitespace_nowrap` and
  `text_ellipsis`; nothing widens its row.
- **Nothing runs per frame while idle:** spinners, the shimmer and the pill's sweep exist only while the agent works;
  fades are one-shot `with_animation`s; no `request_animation_frame`, no timer.
- Window chrome: sizes in window pixels; the camera's zoom never enters the chat's code.
- Minors deferred go to `docs/minors.md` under "Templum Agent", with where, what a person sees and a fix; a minor
  this work fixes is deleted in the same task. Docs lines ≤ 120 columns.
- Before handing over (Task 8): `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo
  test --workspace`; the reduced-env run; `--bench` and `--bench-mixed`.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **A long unbroken path or URL** in a bubble, an answer or a tool sentence: it breaks inside the bubble, and a tool
   row stays one line cut with "…" (Task 3's `a_long_word_breaks_inside_its_bubble`, Task 4's
   `a_long_sentence_stays_on_one_line`).
2. **Opening a tool row** in a virtual list: the row is measured again, so the rows under it move and nothing overlaps
   (Task 4's `a_tool_row_opens_to_show_its_call`).
3. **A narrow canvas** (about 420 px): the composer's chip and buttons stay inside it, in order; the hint is cut
   (Task 2's `a_narrow_canvas_keeps_the_composer_inside_itself`).
4. **A short canvas with the greeting:** the greeting scrolls in the body; the composer keeps its place at the card's
   bottom (Task 5's `a_short_card_keeps_its_composer_under_the_greeting`).
5. **New chat while rows are open:** row numbers start again, so nothing of the old conversation opens a new row
   (Task 4's `new_chat_closes_every_open_row`).

---

### Task 1: The scale, the column and the row gaps

**Files:**
- Create: `crates/templum/src/chat/look.rs`
- Modify: `crates/templum/src/chat/mod.rs` (`mod look;`), `crates/templum/src/chat/panel.rs` (`approval_width`,
  `render`: card radius, the body's rows, the footer, the pinned approval)
- Test: `crates/templum/src/chat/look.rs` (`mod tests`), `crates/templum/src/workspace/tests/templum_agent.rs`

**Interfaces:**
- Produces: in `look.rs`, `pub(crate) const BORDER: f32 = 1.0`, `GUTTER: f32 = 16.0`, `COLUMN_MAX: f32 = 720.0`,
  `CARD_RADIUS: f32 = 16.0`; `pub(crate) fn column(card_w: f32) -> f32`; `pub(crate) fn gap_above(prev:
  Option<&templum_agent::Row>, row: &templum_agent::Row) -> f32`. `panel::approval_width(canvas_w, full)` keeps its
  signature and returns `look::column(width(canvas_w, full))`.

- [ ] **Step 1: Write the failing tests**

In `look.rs`:

```rust
#[test]
fn the_column_is_the_card_less_its_border_and_gutters_up_to_720() {
    assert_eq!(column(640.0), 606.0);
    assert_eq!(column(1568.0), 720.0);
    assert_eq!(column(300.0), 266.0);
    assert_eq!(column(20.0), 0.0);
}

#[test]
fn rows_sit_by_kind() {
    use templum_agent::{ActionState, NoticeLevel, Row};
    let person = Row::Person { text: "hi".into(), queued: false };
    let text = Row::Text { text: "a".into() };
    let action = Row::Action {
        id: "1".into(), tool: "show".into(), input: serde_json::Value::Null,
        state: ActionState::Done, said: String::new(), target: None,
    };
    let reads = Row::Reads { count: 2 };
    let notice = Row::Notice { text: "n".into(), level: NoticeLevel::Done, target: None };
    let end = Row::End { ms: None, cost_usd: None };
    let note = Row::Note { text: "x".into() };
    assert_eq!(gap_above(None, &person), 0.0);
    assert_eq!(gap_above(Some(&text), &person), 20.0);
    assert_eq!(gap_above(Some(&person), &text), 12.0);
    assert_eq!(gap_above(Some(&action), &reads), 2.0);
    assert_eq!(gap_above(Some(&reads), &Row::Thinking), 2.0);
    assert_eq!(gap_above(Some(&notice), &notice), 2.0);
    assert_eq!(gap_above(Some(&text), &action), 12.0);
    assert_eq!(gap_above(Some(&action), &notice), 12.0);
    assert_eq!(gap_above(Some(&text), &end), 6.0);
    assert_eq!(gap_above(Some(&end), &note), 6.0);
}
```

In `templum_agent.rs`, replace `an_approval_in_the_open_card_fits_inside_its_border_and_padding` with
`an_approval_in_the_open_card_takes_the_column` (same setup): for `full` in `[false, true]`, after a render,
`approval.size.width == look::column(card.size.width)` and the gaps on its left and right are equal (within 0.5 px);
for `full == false` the left gap is exactly 17 px (1 of border, 16 of gutter). Add:

```rust
#[gpui_kit::test]
fn full_screen_keeps_the_column_readable(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("hi");
        c.apply(::templum_agent::ChatEvent::Text("an answer".into()));
        ws.templum_agent.full = true;
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let card = window.find("agent-chat").bounds();
        let row = window.find("agent-row-1").bounds();
        let want = crate::chat::look::column(f32::from(card.size.width));
        assert_eq!(f32::from(row.size.width), want);
        assert!(want <= 720.0);
        let (left, right) = (row.left() - card.left(), card.right() - row.right());
        assert!((f32::from(left) - f32::from(right)).abs() <= 0.5, "centered: {left:?} {right:?}");
    })
    .unwrap();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p templum -- the_column_is rows_sit_by_kind takes_the_column keeps_the_column_readable`
Expected: compile error (`look` missing), then FAIL on the widths.

- [ ] **Step 3: Implement**

- `column` = `(card_w - 2.0 * (BORDER + GUTTER)).min(COLUMN_MAX).max(0.0)`.
- `gap_above`: group the rows (Person; Text; Log = Action, Reads, Approval, Thinking; Notice; Meta = End, Note;
  Failure); `None` → 0; Person → 20; Log after Log and Notice after Notice → 2; Meta → 6; else 12.
- `panel::render`: the card's radius `CARD_RADIUS`; the list's padding 8 on top and 16 under (was 8 and 8); each row
  wrapped as `div().w_full().flex().justify_center().child(div().w(px(col)).pt(px(gap)).child(row))`, with `col =
  look::column(card_w)` computed before the row processor and moved into it, and `gap` from the previous row of
  `ws.templum_agent.conversation.rows`; the footer centered at `col`, 8 px above it and 16 px under it; a pinned
  approval centered at `col`, 8 px above the composer. The row elements keep their ids (`agent-row-{n}`) and stay
  `w_full()` of the column.

- [ ] **Step 4: Run them to see them pass, and the chat's tests**

Run: `cargo test -p templum -- chat:: templum_agent`
Expected: PASS (the old 9 px test is gone; `an_approval_of_the_closed_chat_keeps_its_width_in_full_screen` still
asserts 640).

- [ ] **Step 5: Leave it uncommitted**

---

### Task 2: The header, the tasks drawer and the composer

**Files:**
- Modify: `crates/templum/src/chat/panel.rs` (header, drawer; `model_chip` moves out), `crates/templum/src/chat/pill.rs`
  (the wide branch becomes the composer; `model_chip` moves in), `crates/templum/src/chat/look.rs` (the sizes used)
- Test: `crates/templum/src/workspace/tests/templum_agent.rs`

**Interfaces:**
- Consumes: `look::column` (Task 1).
- Produces: `pill::model_chip(ws: &Workspace, cx: &mut Context<Workspace>) -> impl IntoElement` (moved from
  `panel.rs`, id `agent-model`, its menu anchored `Anchor::BottomLeft` so it opens upward); `pill::HINT: &str =
  "Enter to send · Shift+Enter for a new line · Esc to close"`, `pill::HINT_WORKING: &str = "Enter to queue it ·
  Shift+Enter for a new line · Esc to close"`; ids `agent-hint`, `agent-send`, `agent-stop` (in the composer).
  A test helper in `templum_agent.rs`: `fn set_pill(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut
  TestAppContext, text: &str)` (sets the pill field's value through `templum_agent.input`, as `pill_text` reads it)
  and `fn resize(handle: AnyWindowHandle, cx: &mut TestAppContext, w: f32, h: f32)` (through
  `VisualTestContext::from_window(..).simulate_resize`, gpui-fast-0.1.8/src/app/test_context.rs:1003), unless the
  test module already has one.

- [ ] **Step 1: Write the failing tests**

```rust
#[gpui_kit::test]
fn send_becomes_stop_while_it_works(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| ws.update(cx, |ws, cx| ws.toggle_agent(window, cx)))
        .unwrap();
    let has = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    };
    assert!(has(cx, "agent-send") && !has(cx, "agent-stop"), "idle: Send, dim while empty");
    assert!(has(cx, "agent-hint"), "the keys while it has the keyboard");
    ws.update(cx, |ws, _| ws.templum_agent.conversation.person("go"));
    assert!(has(cx, "agent-stop") && !has(cx, "agent-send"), "working, nothing typed: Stop");
    set_pill(handle, &ws, cx, "and then");
    assert!(has(cx, "agent-stop") && has(cx, "agent-send"), "working with text: Send beside Stop");
}

#[gpui_kit::test]
fn the_model_chip_sits_in_the_composer(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let pill = window.find("agent-pill").bounds();
        assert!(pill.contains(&window.find("agent-model").bounds().center()));
        assert!(window.try_find("agent-engine").is_none(), "the old engine label is gone");
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_composer_grows_to_eight_lines_then_scrolls(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| ws.update(cx, |ws, cx| ws.toggle_agent(window, cx)))
        .unwrap();
    let height = |cx: &mut TestAppContext, lines: usize| {
        let text: Vec<String> = (0..lines).map(|i| format!("line {i}")).collect();
        set_pill(handle, &ws, cx, &text.join("\n"));
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            f32::from(window.find("agent-pill").bounds().size.height)
        })
        .unwrap()
    };
    let (one, three, eight, twenty) = (height(cx, 1), height(cx, 3), height(cx, 8), height(cx, 20));
    assert!(one > 36.0 + 28.0, "the field and its bar: {one}");
    assert!(three > one && eight > three, "{one} {three} {eight}");
    assert_eq!(twenty, eight, "at most 8 lines, then it scrolls");
}

#[gpui_kit::test]
fn a_narrow_canvas_keeps_the_composer_inside_itself(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    resize(handle, cx, 420.0, 700.0);
    cx.update_window(handle, |_, window, cx| ws.update(cx, |ws, cx| ws.toggle_agent(window, cx)))
        .unwrap();
    set_pill(handle, &ws, cx, "hello");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let pill = window.find("agent-pill").bounds();
        let (chip, send) = (window.find("agent-model").bounds(), window.find("agent-send").bounds());
        assert!(chip.right() <= send.left(), "in order");
        assert!(send.right() <= pill.right() && chip.left() >= pill.left());
    })
    .unwrap();
}

#[gpui_kit::test]
fn the_usage_chip_is_a_label_without_tasks(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("go");
        c.apply(::templum_agent::ChatEvent::TurnEnded { ms: None, cost_usd: Some(0.04), error: None });
        c.working = false;
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("agent-meter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    assert!(!ws.read_with(cx, |ws, _| ws.templum_agent.tasks_open));
}
```

Change `catch_me_up_sends_its_sentence`: before `window.click("agent-catch-up", cx)`, click `agent-meter` and render
(Catch me up now lives in the drawer).

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p templum -- send_becomes_stop model_chip_sits composer_grows narrow_canvas usage_chip catch_me_up`
Expected: FAIL (`agent-stop` is in the header and absent while idle there, `agent-model` in the header,
`agent-catch-up` already shown, the meter toggles with no task).

- [ ] **Step 3: Implement**

- **Header** (spec, The header): 44 px; `sparkles`, or `Spinner::new().icon(IconName::Orbit)` while
  `conversation.working`; "Templum Agent" 13 px semibold `fg`; flex; the usage chip (`agent-meter`, a click toggles
  `tasks_open` only while `ws.agent_tasks() > 0`, with a `chevron-down` then); `agent-new`, `agent-full`,
  `agent-close` as 28 px icon buttons with `hover` and `.active(|s| s.bg(ui.pressed))`, tooltip "Close (Esc)" on
  close. No bottom border. No Stop, no model chip, no Catch me up.
- **Drawer:** while `tasks_open`, the task rows (`agent-task-{n}`, `bot` icon, title cut with "…") and
  `agent-catch-up` last (`sparkles` and "Catch me up" in `accent`), on the column, a `hairline` under it.
- **Composer** (`pill.rs`, the `(Some(input), true)` branch; spec, The composer): two rows; the field
  (`Textarea::new(input).appearance(false)`, 14 px, `auto_grow(1, 8)` unchanged); the bar: `model_chip`, the hint
  (`agent-hint`, only when `ws.agent_has_keys(window, cx)`, `HINT_WORKING` while working, right-aligned, cut with
  "…"), then Send and Stop per the spec's three cases. Send's click calls `submit_to_agent` only with text; Stop's
  calls `stop_engine`. Border `accent` and a `BoxShadow` spread 2, blur 0, `ui.ring` while it has the keyboard; the
  pill's shadow only when not `footer`. The bar fades in once with `with_animation` over `motion::HOVER`.
- Remove the `agent-engine` label. `model_chip`'s menu choice still gives the keyboard back to the field.

- [ ] **Step 4: Run them, and the chat's tests**

Run: `cargo test -p templum -- templum_agent`
Expected: PASS, `new_chat_and_stop_buttons`, `the_engine_chip_picks_the_model`, `the_meter_counts_its_tasks_and_cost`
and `the_pill_shows_at_its_width_and_grows_when_it_has_the_keyboard` included.

- [ ] **Step 5: Leave it uncommitted**

---

### Task 3: Messages and Markdown

**Files:**
- Modify: `crates/templum/src/chat/rows.rs` (Person, Text, End, Failure, Note, Thinking),
  `crates/templum/src/markdown.rs` (`chat_style`), `crates/templum/src/chat/look.rs` (`code_fill`, sizes),
  `docs/minors.md` (delete "The Thinking row has no sweeping band.")
- Test: `crates/templum/src/chat/look.rs`, `crates/templum/src/workspace/tests/templum_agent.rs`

**Interfaces:**
- Consumes: the column and gaps (Task 1).
- Produces: `look::CODE_SHARE: f32 = 0.06`, `pub(crate) fn code_fill(surface: Hsla, fg: Hsla) -> Hsla` (opaque:
  `surface.blend(fg.opacity(CODE_SHARE))`); `markdown::chat_style(ui: &crate::theme::Ui, fg: Hsla) ->
  TextViewStyle`; ids `agent-bubble-{n}` (the person's bubble), `agent-copy` (a `div` with `test_support()` around
  each code block's `Clipboard`). Task 4 and Task 6 use `code_fill`.

- [ ] **Step 1: Write the failing tests**

```rust
// look.rs
#[test]
fn the_code_fill_is_opaque_and_near_the_surface() {
    let fill = code_fill(gpui_kit::hsla(0.0, 0.0, 0.0, 1.0), gpui_kit::hsla(0.0, 0.0, 1.0, 1.0));
    assert_eq!(fill.a, 1.0);
    assert!((fill.l - 0.06).abs() < 0.01, "{fill:?}");
}
```

```rust
// templum_agent.rs
#[gpui_kit::test]
fn a_code_blocks_copy_button_copies_its_code(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.person("hi");
        c.apply(::templum_agent::ChatEvent::Text("Run:\n\n```bash\ncargo test\n```".into()));
    });
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("agent-copy", cx);
    })
    .unwrap();
    let copied = cx.read_from_clipboard().and_then(|c| c.text());
    assert_eq!(copied.as_deref().map(str::trim_end), Some("cargo test"));
}

#[gpui_kit::test]
fn a_long_word_breaks_inside_its_bubble(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.person(&"x".repeat(400)));
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let row = window.find("agent-row-0").bounds();
        let bubble = window.find("agent-bubble-0").bounds();
        assert!(bubble.size.width <= row.size.width * 0.8 + gpui_kit::px(0.5));
        assert!(f32::from(bubble.size.height) > 2.0 * 22.0, "it wrapped: {:?}", bubble.size);
    })
    .unwrap();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p templum -- code_fill copy_button long_word`
Expected: compile error, then FAIL (no `agent-copy`, no `agent-bubble-0`).

- [ ] **Step 3: Implement** (spec: The person's message, The agent's answer, The other rows)

- `chat_style`: `paragraph_gap: rems(0.75)`; `heading_base_font_size: px(14.)` with a resolver giving 18, 16, 15,
  then 14 px; `code_block` refined with `text_size(px(13.))`, `bg(code_fill(ui.surface, fg))`,
  `border_color(ui.hairline)`, radius 10, `pt(px(34.))` (room for the language and the copy button; gpui-base gives
  14 px when actions replace the language), `px(px(16.))`, `pb(px(14.))`; `inline_code: HighlightStyle {
  background_color: Some(ui.hover), color: Some(fg), ..Default::default() }`; `table` refined with
  `overflow_hidden()` (cut at the column). `markdown::style` stays for its other callers.
- Text rows: `TextView::markdown(..).style(chat_style(..)).selectable(true).code_block_actions(..)`, the actions an
  `h_flex` of the language (`block.lang()`, 12 px `title_dim`) and `div().id("agent-copy").child(Clipboard::new("copy")
  .value(block.code()).tooltip("Copy")).test_support()`, the whole painted with the code fill so gpui-base's own
  background under it never shows. The container: 14 px text, 22 px line height, `fg`. Streaming stays plain
  (`agent-streaming`) at the same size, line height and color.
- Person: the bubble (`agent-bubble-{n}`), `max_w(relative(0.8))`, `hover` fill, radius 16, 14/9 px padding, 14 px
  `fg`; "Queued" moves under it (`clock` 12 px, 12 px `title_dim`, right-aligned, 4 px below).
- End left-aligned 12 px `title_dim`; Failure `circle-alert` and red text, wrapping; Note centered 12 px `title_dim`;
  Thinking `Spinner` with `IconName::Orbit` (14 px, `accent`) and gpui-kit's `ShimmerText::new("Thinking")` (13 px,
  `title_dim`, highlight `fg`).

- [ ] **Step 4: Run them, and the chat's tests**

Run: `cargo test -p templum -- templum_agent chat::`
Expected: PASS, `the_text_it_writes_is_markdown_once_whole` and `a_turn_shows_its_rows` included.

- [ ] **Step 5: Leave it uncommitted**

---

### Task 4: Tool rows that open

**Files:**
- Modify: `crates/templum/src/chat/rows.rs` (Action, Reads, Approval, Notice; `call_lines`; delete `fn icon`),
  `crates/templum/src/chat/mod.rs` (`TemplumAgent::expanded`, `Workspace::toggle_row`),
  `crates/templum/src/workspace/agent.rs` (`new_chat` clears `expanded`)
- Test: `crates/templum/src/chat/rows.rs` (`mod tests`), `crates/templum/src/workspace/tests/templum_agent.rs`

**Interfaces:**
- Consumes: `look::code_fill` (Task 3).
- Produces: `pub(crate) const CALL_LINES: usize = 12` and `pub(crate) fn call_lines(input: &serde_json::Value) ->
  Vec<String>` in `rows.rs`; `pub expanded: std::collections::BTreeSet<usize>` on `TemplumAgent` (rows open, by
  number; empty by default); `pub(crate) fn toggle_row(&mut self, n: usize, cx: &mut Context<Self>)` on
  `Workspace`; ids `agent-row-{n}-more` (the chevron), `agent-row-{n}-call` (the details).

- [ ] **Step 1: Write the failing tests**

```rust
// rows.rs
#[test]
fn a_call_shows_a_line_per_argument() {
    use serde_json::json;
    assert_eq!(super::call_lines(&json!({"agent": "codex", "workspace": "w6"})), ["agent: codex", "workspace: w6"]);
    assert_eq!(super::call_lines(&json!({"keys": ["enter", "y"], "n": 3})), ["keys: [\"enter\",\"y\"]", "n: 3"]);
    assert_eq!(super::call_lines(&json!({"text": "ls\ncd /"})), ["text: ls", "  cd /"]);
    assert!(super::call_lines(&json!({})).is_empty());
    assert!(super::call_lines(&serde_json::Value::Null).is_empty());
    assert_eq!(super::call_lines(&json!("x")), ["\"x\""]);
    let long: Vec<String> = (0..20).map(|i| i.to_string()).collect();
    let lines = super::call_lines(&json!({"text": long.join("\n")}));
    assert_eq!(lines.len(), super::CALL_LINES);
    assert_eq!(lines[11], "⋯ 9 more lines");
}
```

```rust
// templum_agent.rs
fn action(said: &str) -> ::templum_agent::Row {
    ::templum_agent::Row::Action {
        id: "t1".into(),
        tool: "open_agent".into(),
        input: json!({"agent": "codex"}),
        state: ::templum_agent::ActionState::Done,
        said: said.into(),
        target: None,
    }
}

#[gpui_kit::test]
fn a_tool_row_opens_to_show_its_call(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let rows = &mut ws.templum_agent.conversation.rows;
        rows.push(action("Opened Codex as b1 in ~/proj."));
        rows.push(::templum_agent::Row::Note { text: "after".into() });
    });
    open_chat(handle, &ws, cx);
    let click = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, cx| {
            window.click(id, cx);
            window.render_frame(cx);
            window.render_frame(cx);
        })
        .unwrap()
    };
    let open_row = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, _| window.try_find("agent-row-0-call").is_some()).unwrap()
    };
    assert!(!open_row(cx));
    click(cx, "agent-row-0-more");
    assert!(open_row(cx));
    cx.update_window(handle, |_, window, _| {
        let (row, below) = (window.find("agent-row-0").bounds(), window.find("agent-row-1").bounds());
        assert!(below.top() >= row.bottom(), "the row under it moved: {row:?} {below:?}");
    })
    .unwrap();
    click(cx, "agent-row-0-more");
    assert!(!open_row(cx));
    click(cx, "agent-row-0");
    assert!(open_row(cx), "a row that touched nothing opens on a click");
}

#[gpui_kit::test]
fn new_chat_closes_every_open_row(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    ws.update(cx, |ws, cx| {
        ws.templum_agent.conversation.rows.push(action("Opened Codex."));
        ws.toggle_row(0, cx);
        ws.new_chat(cx);
    });
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.expanded.is_empty()));
}

#[gpui_kit::test]
fn a_long_sentence_stays_on_one_line(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.rows.push(action(&"x".repeat(300))));
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(f32::from(window.find("agent-row-0").bounds().size.height) <= 28.5);
    })
    .unwrap();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p templum -- a_call_shows tool_row_opens new_chat_closes long_sentence`
Expected: compile errors (`call_lines`, `expanded`, `toggle_row`), then FAIL.

- [ ] **Step 3: Implement** (spec: Tool rows)

- `call_lines`: an object, a line per key (serde_json's order): a string as `key: first line`, its further lines
  `"  " + line`; any other value `key: ` + compact JSON; `Null` or `{}` → none; any other value, its compact JSON;
  past `CALL_LINES` lines, the first 11 then `format!("⋯ {} more lines", total - 11)`.
- The line: status per the spec (`Spinner` `accent`, `check`, `circle-x` red, `ban`), the sentence (as today's
  `sentence`) cut with "…", the chevron (`agent-row-{n}-more`, `cx.stop_propagation()` then `toggle_row`), shown on
  the row's group hover and while open; 28 px tall, radius 8, `hover`. A click: `show_row` when the row has a target,
  else `toggle_row`. The failed refusal under the line, 30 px in. Open: `agent-row-{n}-call` under it, 30 px in,
  `code_fill`, radius 8, 10 px padding: the whole `said` (13 px `title`, wrapping) when Done or Failed, the tool's
  name (12 px code, `title_dim`), then `call_lines(input)` (12 px code, `fg`), in the terminal's font
  (`ws.cell.font.family`); fading in once over `look::FADE` (add `pub(crate) const FADE: Duration =
  Duration::from_millis(160)` here).
- `toggle_row`: insert or remove `n` in `expanded`, `self.templum_agent.list.remeasure_items(n..n + 1)`, `cx.notify()`.
- Reads, the approval's record (`shield-check` when its answer starts with "Allowed", else `shield-x`) and the notice
  per the spec, on the same 28 px line.
- `new_chat`: `agent.expanded.clear()` with the grants.

- [ ] **Step 4: Run them, and the chat's tests**

Run: `cargo test -p templum -- templum_agent chat::`
Expected: PASS, `a_rows_click_reveals_its_block_in_another_workspace` and the notice tests included.

- [ ] **Step 5: Leave it uncommitted**

---

### Task 5: The empty state, scrolling to the end, rows fading in

**Files:**
- Modify: `crates/templum/src/chat/panel.rs` (`SUGGESTIONS`, `greets`, the greeting, `scrolled_up`, the scroll button,
  the fade), `crates/templum/src/chat/mod.rs` (`arrival`, `drawn`), `crates/templum/src/chat/look.rs` (`fades`),
  `docs/minors.md` (delete "The suggestions don't come back after New chat.")
- Test: `crates/templum/src/chat/look.rs`, `crates/templum/src/workspace/tests/templum_agent.rs`

**Interfaces:**
- Consumes: `look::FADE` (Task 4).
- Produces: `pub(crate) const SUGGESTIONS: [(IconName, &str); 4]` (index 1 is `CATCH_UP`; `suggest_to_agent` keeps
  its signature); `pub(crate) fn greets(rows: &[templum_agent::Row]) -> bool` (no rows, or only notes);
  `pub(crate) fn scrolled_up(list: &gpui_kit::ListState) -> bool` (`max_offset_for_scrollbar().y > 0`, not following
  the tail, `is_scrolled_to_end()` not `Some(true)`, as gpui-kit's `MessageScrollerState::is_scrolled_up`);
  `pub(crate) fn fades(n: usize, arrival: Option<(usize, Instant)>, now: Instant) -> bool` in `look.rs`; on
  `TemplumAgent`, `pub arrival: Cell<Option<(usize, Instant)>>` (the first row of the last arrival and when) and
  `pub drawn: Cell<bool>` (the chat was drawn open last frame); ids `agent-greeting`, `agent-to-end`,
  `agent-to-end-new`.

- [ ] **Step 1: Write the failing tests**

```rust
// look.rs
#[test]
fn only_rows_that_arrived_fade_and_only_for_a_moment() {
    let t0 = std::time::Instant::now();
    assert!(!fades(3, None, t0));
    assert!(!fades(2, Some((3, t0)), t0));
    assert!(fades(3, Some((3, t0)), t0));
    assert!(fades(5, Some((3, t0)), t0 + FADE / 2));
    assert!(!fades(3, Some((3, t0)), t0 + FADE));
}
```

In `templum_agent.rs`, change `suggestions_fill_the_field` to look for `agent-suggestion-0` to `-3` and to check, after
clicking `agent-suggestion-3`, that `pill_text` is "Open an agent for each card on my board" (keep its check of
suggestion 0). Add:

```rust
#[gpui_kit::test]
fn the_greeting_comes_back_after_new_chat(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.person("go"));
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find("agent-greeting").is_none());
        window.click("agent-new", cx);
        window.render_frame(cx);
        assert!(window.try_find("agent-greeting").is_some());
        assert!(window.try_find("agent-suggestion-3").is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn scrolling_up_shows_the_way_back_to_the_end(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        for i in 0..200 {
            c.person(&format!("message {i}"));
            c.apply(::templum_agent::ChatEvent::Text(format!("answer {i}")));
        }
    });
    open_chat(handle, &ws, cx);
    let shown = |cx: &mut TestAppContext, id: &'static str| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
        .unwrap()
    };
    assert!(!shown(cx, "agent-to-end"), "at its end");
    ws.read_with(cx, |ws, _| ws.templum_agent.list.scroll_by(gpui_kit::px(-2000.0)));
    assert!(shown(cx, "agent-to-end"), "scrolled up");
    assert!(!shown(cx, "agent-to-end-new"));
    ws.update(cx, |ws, _| ws.templum_agent.conversation.note("new"));
    assert!(shown(cx, "agent-to-end-new"), "a row came below");
    cx.update_window(handle, |_, window, cx| window.click("agent-to-end", cx)).unwrap();
    assert!(!shown(cx, "agent-to-end"), "back at its end");
    assert!(ws.read_with(cx, |ws, _| ws.templum_agent.list.is_following_tail()));
}

#[gpui_kit::test]
fn rows_fade_in_only_when_they_arrive_while_the_chat_is_open(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.note("one"));
    open_chat(handle, &ws, cx);
    let render = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| window.render_frame(cx)).unwrap()
    };
    let arrival = |cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, _| ws.templum_agent.arrival.get().map(|(from, _)| from))
    };
    render(cx);
    assert_eq!(arrival(cx), None, "rows there when it opens show at once");
    let n = ws.update(cx, |ws, _| {
        let c = &mut ws.templum_agent.conversation;
        c.note("two");
        c.rows.len() - 1
    });
    render(cx);
    assert_eq!(arrival(cx), Some(n));
    ws.update(cx, |ws, _| ws.templum_agent.open = false);
    render(cx);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.note("three"));
    open_chat(handle, &ws, cx);
    render(cx);
    assert_eq!(arrival(cx), Some(n), "rows that came while it was closed don't fade");
}

#[gpui_kit::test]
fn a_short_card_keeps_its_composer_under_the_greeting(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    resize(handle, cx, 900.0, 420.0);
    ws.update(cx, |ws, _| ws.templum_agent.conversation.note("New conversation."));
    open_chat(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let (chat, pill) = (window.find("agent-chat").bounds(), window.find("agent-pill").bounds());
        let body = window.find("agent-body").bounds();
        assert!(window.try_find("agent-greeting").is_some());
        assert_eq!(f32::from(chat.bottom() - pill.bottom()), 17.0, "16 px and the border");
        assert!(pill.top() >= body.bottom() - gpui_kit::px(0.5), "the greeting stays in the body");
    })
    .unwrap();
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p templum -- only_rows_that_arrived suggestions_fill greeting_comes_back scrolling_up rows_fade_in
short_card`
Expected: compile errors, then FAIL.

- [ ] **Step 3: Implement** (spec: The empty state, Scrolling to the end, Motion)

- The body shows the greeting (`agent-greeting`, in an `overflow_y_scroll` with an id) while `greets(rows)`; the
  chips are `agent-suggestion-{n}` with their icons; a click calls `suggest_to_agent` as today.
- The scroll button replaces `new_below`: shown while `scrolled_up(&agent.list)`, the dot (`agent-to-end-new`) while
  `agent.unseen`; a click `list.set_follow_mode(FollowMode::Tail)` and `cx.notify()`; it fades in once over
  `motion::HOVER`. `unseen` keeps today's rule.
- The fade, in `render` before `seen` is set (`now` from `cx.background_executor().now()`):

  ```rust
  if agent.drawn.get() && rows.len() > agent.seen.get() {
      agent.arrival.set(Some((agent.seen.get(), now)));
  }
  agent.drawn.set(true); // and `drawn.set(false)` where render returns None because the chat is closed
  ```

  and in the row processor, a row for which `look::fades(n, arrival, now)` is wrapped in `with_animation(
  ElementId::Name(format!("agent-fade-{n}").into()), Animation::new(look::FADE).with_easing(motion::slide), |el, t|
  el.opacity(t))`. `fades` = `arrival.is_some_and(|(from, at)| n >= from && now.saturating_duration_since(at) <
  FADE)`.

- [ ] **Step 4: Run them, and the chat's tests**

Run: `cargo test -p templum -- templum_agent chat::`
Expected: PASS, `a_long_chat_draws_only_the_rows_in_view` and `catch_me_up_sends_its_sentence` included.

- [ ] **Step 5: Leave it uncommitted**

---

### Task 6: The approval card and the pill at rest

**Files:**
- Modify: `crates/templum/src/chat/approval.rs`, `crates/templum/src/chat/pill.rs` (the rest, working and asks
  branches), `docs/minors.md` (delete "The approval's box has no visible scrollbar.")

**Interfaces:**
- Consumes: `look::code_fill` (Task 3), the column (Task 1).
- Produces: no new name. Every `approval-*` id stays.

- [ ] **Step 1: Restyle the approval** (spec, The approval): radius 12, yellow at 55% border, 14 px padding, 10 px
  gaps; while `approval_focus` is focused, full yellow with a 2 px ring of yellow at 25%; the first line with
  `shield-alert`; the action 14 px semibold `fg`; `approval-changed` with `triangle-alert`; the box with the code fill
  and `Scrollbar::vertical(&approval.scroll)` as a sibling over it (as gpui-base's code blocks do); three equal
  buttons (30 px, radius 8, `hairline`, 13 px `fg`, the key in an 18 px cap), Ask each time's Allow once red; floating
  (chat closed), the card's shadow. Behavior, keys, the 0.5 s and `seen()` untouched.
- [ ] **Step 2: Restyle the pill at rest** (spec, The pill at rest): icon 15 px, words 13 px; `hover_border` on hover;
  Review a 22 px pill with a yellow border and 12 px `fg`.
- [ ] **Step 3: Run the approval and pill tests**

Run: `cargo test -p templum -- templum_agent`
Expected: PASS, every approval test (`the_popup_answers_with_its_buttons_and_keys`,
`what_it_would_type_is_allowed_only_once_seen_to_its_end`, `a_digit_for_the_pill_never_answers_an_approval`, …) and
`the_pill_says_what_it_does_what_it_asks_and_that_something_is_new` unchanged.

- [ ] **Step 4: Leave it uncommitted** (the look is checked in Task 9)

---

### Task 7: The docs

**Files:**
- Modify: `docs/specs/templum-agent.md`, `docs/specs/templum-agent-look.md` (status), `docs/ARCHITECTURE.md`,
  `docs/minors.md`

- [ ] **Step 1: templum-agent.md.** Status line: add "its look: [templum-agent-look.md](templum-agent-look.md)
  (kanban card 143)". Under "What a person sees", one line: the look (sizes, colors, motion) is in
  templum-agent-look.md; this section says what each piece does. Then make what moved true: the pill's Typing row
  (the composer: the model chip, Send or Stop); the chat's header bullet (title, usage chip, New chat, Full screen,
  Close; Stop and the model chip in the composer; Catch me up in the tasks drawer); "↓ New" becomes the
  scroll-to-the-end button; four suggestions, shown also after New chat; Rows: an action opens to show its call (a
  click on one that touched nothing opens it), Thinking shimmers, a turn's end is left-aligned.
- [ ] **Step 2: ARCHITECTURE.md**, the `chat/` line: add `look.rs` (the scale, the column, the row gaps, the fade)
  and a link to templum-agent-look.md.
- [ ] **Step 3: templum-agent-look.md** status: "Implemented (kanban card 143), uncommitted until reviewed".
- [ ] **Step 4: minors.md:** every minor deferred during Tasks 1–6 under "Templum Agent" (where, what a person sees,
  a fix); check the three deleted entries are gone.
- [ ] **Step 5: Check the lines:** `awk 'length > 120 {print FILENAME": "FNR}' docs/specs/templum-agent*.md
  docs/plans/2026-10-24-templum-agent-look.md docs/ARCHITECTURE.md docs/minors.md` prints nothing.

---

### Task 8: The full check and the benches

- [ ] **Step 1: The full check**

Run: `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: no diff from fmt, no clippy warning, every test passes. Report failures as they are.

- [ ] **Step 2: The reduced environment**

Run: `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test --workspace --no-fail-fast`
Expected: every test passes.

- [ ] **Step 3: The benches**

Run: `cargo run --release -p templum -- --bench` then `cargo run --release -p templum -- --bench-mixed`
Expected: each prints a JSON line with `p95_ms` ≤ 16.7. Keep both lines for the report. Over the bar: find the
cause (a style built per row, a fade that never ends, a spinner while idle) and fix it before Task 9.

---

### Task 9: Screenshots, fixes, and the dev window closed

The look is checked by eye in a real window, light and dark. `SHOTS=/tmp/templum-card143`. The person's own Templum
runs too: touch only the dev window, found by its process id.

- [ ] **Step 1: Back up the dev data and write the fixtures**

```bash
SHOTS=/tmp/templum-card143; mkdir -p $SHOTS
cp ~/.local/share/templum-dev/agent/conversation.json $SHOTS/conversation.json.bak
cp ~/.config/templum-dev/config.toml $SHOTS/config.toml.bak
```

Write `$SHOTS/empty.json`: `{"version":1,"engine":"claude","session":null,"rows":[{"kind":"note","text":"New
conversation."}]}`. Write `$SHOTS/talk.json` with the same header and these rows: a person's message ("Set up a
workspace API for ~/proj with a kanban Sprint of 2 cards and one Claude Code per card."); `reads` 3; a text with an
`## Workspace API` heading, a numbered list with **bold**, *italic* and `inline code`, and a ```` ```bash ```` block
(`git worktree add ../proj-card-1 -b card-1`); actions: `create_workspace` done ("Created workspace API (w3).",
target `{"workspace":"w3"}`), `add_block` done, `open_agent` done with a two-line `prompt` in its input and a long
`said`, `open_agent` failed ("Codex isn't installed: install it or pick another agent."), `close_block` denied; an
`approval` record (`"Close \"zsh · proj\""`, "Denied"); a `done` and a `waiting` notice; a text holding a 150-character
path with no space; an `end` (14000 ms, $0.04); a queued person's message. Check each row's fields against
`crates/templum-agent/src/conversation.rs`. Copy `empty.json` over `~/.local/share/templum-dev/agent/conversation.json`.

- [ ] **Step 2: A click tool for an X11 window** (Hyprland has no click; the dev window runs under Xwayland, where
  XTest reaches it). Write `$SHOTS/click.py`:

```python
#!/usr/bin/env python3
"""click.py X Y [BUTTON]: one click at layout point (X, Y) through XTest (an X11 window only). 4/5 = wheel."""
import ctypes, sys, time
x11, xtst = ctypes.CDLL("libX11.so.6"), ctypes.CDLL("libXtst.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
d = ctypes.c_void_p(x11.XOpenDisplay(None))
x, y, button = int(sys.argv[1]), int(sys.argv[2]), int(sys.argv[3]) if len(sys.argv) > 3 else 1
xtst.XTestFakeMotionEvent(d, -1, x, y, 0); x11.XFlush(d); time.sleep(0.1)
xtst.XTestFakeButtonEvent(d, button, 1, 0); xtst.XTestFakeButtonEvent(d, button, 0, 0)
x11.XFlush(d); x11.XCloseDisplay(d)
```

- [ ] **Step 3: Launch the dev window under X11 and find it**

```bash
cargo build -p templum
(env -u WAYLAND_DISPLAY target/debug/templum --dev > $SHOTS/dev.log 2>&1 &)
# then, once its window shows:
PID=$(pgrep -f 'target/debug/templum --dev' | head -1)
hyprctl clients -j | jq --argjson p "$PID" '.[] | select(.pid == $p) | {xwayland, at, size}'
shot() { grim -g "$(hyprctl clients -j | jq -r --argjson p "$PID" \
  '.[] | select(.pid == $p) | "\(.at[0]),\(.at[1]) \(.size[0])x\(.size[1])"')" "$SHOTS/$1.png"; }
hyprctl dispatch focuswindow pid:$PID
```

Expected: `xwayland: true`. If it is `false`, the clicks of the steps below can't be made: take the shots that need
none (empty state, conversation, approval, pill) and report the others as not taken. A point in a shot at pixel
(px, py) is at layout point (`at[0]` + px / scale, `at[1]` + py / scale), scale from `hyprctl monitors -j`. If the
welcome or the workspaces screen shows, `wtype -k Escape`.

- [ ] **Step 4: Take the shots, light then dark.** The theme: add `mode = "light"` under `[theme]` in
  `~/.config/templum-dev/config.toml` (One Light), later `mode = "dark"` (Catppuccin Mocha); Templum reads it again
  within a second. Open the chat with `wtype -M ctrl -M shift -k space -m shift -m ctrl`. For each theme:
  1. `NN-empty-<theme>`: with `empty.json` (copy it in and relaunch, or press New chat in the header);
  2. `NN-composer-<theme>`: Esc, then the shortcut and `wtype "Open an agent"` with the chat closed (the floating
     composer, its hint);
  3. `NN-talk-<theme>`: quit the dev window (Step 6's way), copy `talk.json` in, relaunch, open the chat;
  4. `NN-row-open-<theme>`: click a tool row's chevron (read the last shot to find it);
  5. `NN-scrolled-<theme>`: wheel up over the body (`click.py X Y 4`, a few times), the scroll button showing;
  6. `NN-full-<theme>`: click Full screen;
  7. `NN-approval-<theme>`: type into the composer "Type echo hello into the terminal in this workspace." and Enter;
     take a shot every 10 s for up to 2 minutes until the approval shows (this needs Claude Code logged in; if it
     never shows, say why from the chat's rows);
  8. `NN-pill-<theme>`: Esc, the pill at rest.
- [ ] **Step 5: Look and fix.** Read every PNG. Check against the spec: sizes, the column, contrast of every text and
  of the send arrow, the code block's band and copy button, nothing cut that shouldn't be, nothing overflowing, both
  themes. Fix in the code, run `cargo test -p templum -- templum_agent chat::`, and take the affected shots again.
  If code changed, run Task 8 again (all three steps).
- [ ] **Step 6: Close the dev window and restore the data**

```bash
hyprctl dispatch closewindow pid:$PID
for i in $(seq 20); do kill -0 $PID 2>/dev/null || break; sleep 0.5; done
kill -0 $PID 2>/dev/null && kill $PID
cp $SHOTS/conversation.json.bak ~/.local/share/templum-dev/agent/conversation.json
cp $SHOTS/config.toml.bak ~/.config/templum-dev/config.toml
rm -f ~/.local/share/templum-dev/agent/conversation.json.bak
```

Expected: `pgrep -f 'target/debug/templum --dev'` prints nothing; the dev data is as it was (`rm` takes only a
`.bak` a fixture that didn't read would have left: check there was none before Step 1 with `ls`, and keep it if
there was).

- [ ] **Step 7: Report.** The screenshot paths (`ls $SHOTS/*.png`), what was fixed after looking, the shots not taken
  and why, Task 8's results (the full check, the reduced run, both bench JSON lines), and the minors added. Leave
  everything uncommitted.
