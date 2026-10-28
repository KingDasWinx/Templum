# Automations, Batch 1 (Time) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** a Sonnet 5.5 (high) implementer in a Templum block, then an Opus 5.5 (xhigh)
> reviewer, in a loop until the reviewer passes it. The implementer **commits nothing**: everything stays
> uncommitted for the orchestrator.

**Goal:** Kanban card 150, "a cron system (a button in the sidebar near Git)", batch 1: an **Automations** popup,
opened from a sidebar button above Git and from Ctrl+Shift+J, where the person writes rules that run at a time (once,
every N, every day, chosen weekdays or days of the month, or a cron expression) and do one thing (type a prompt into an
agent, run a command, start an agent, notify them), each row telling its next run and how its last run went; the
agents' own `schedule`s are listed under **Asked by agents** with Cancel.

**Architecture:** A pure layer, `crates/templum/src/automations/` (`mod.rs`: the saved rules and their file;
`clock.rs`: next times, the cron grammar, daylight saving, the words), unit-tested with fixed dates and a test time
zone. A runtime layer, `crates/templum/src/workspace/automations.rs`: a 1-second look (a `spawn_in(window)` loop, as
the snapshots' loop) that finds the rules due, holds a prompt or a command until its target is free, and runs each Do
through the operations that already exist (`hand_task` as `delegate` types a task, `type_text`, `add_block_in`,
`open_agent_in`, a notification). A view, `crates/templum/src/automations_view/`, draws the popup like
`agents_view.rs` (list at the left, the editor at the right) and the sidebar button with its badge.

**Tech Stack:** Rust, GPUI (gpui-fast 0.1.8) + gpui-kit 0.7.3 (pinned): `Input`/`InputState`, `Textarea`,
`Switch`, `Checkbox`, `Icon`/`IconName::{AlarmClock, Play, CopyPlus, Trash}`, `capture_key_down`; chrono 0.4 (its
`serde` feature turned on: no new crate).

**Spec:** [docs/specs/automations.md](../specs/automations.md) (read it first; this plan argues from it, and covers
only what it doesn't mark batch 2). Files to read before starting: `crates/templum/src/workspace/schedules.rs`,
`crates/templum/src/instructions.rs` and its load/save wiring (`workspace/spaces.rs`, `workspace/persist.rs`),
`crates/templum/src/agents_view.rs`, `crates/templum/src/workspace/agents_view.rs`,
`crates/templum/src/workspace/delegate.rs` (`hand_task`, `takes_notice`, `takes_typing`),
`crates/templum/src/workspace/ops.rs` (`add_block_in`, `type_text`), `crates/templum/src/workspace/spawn.rs`
(`open_agent_in`, `Opener`), `crates/templum/src/workspace/tests/schedules.rs` and `tests/agents_view.rs` (their
helpers).

## Global Constraints

- Only `crates/templum` changes code (plus `Cargo.toml`'s chrono feature); no new crate; no `unsafe`; no speculative
  structure: **no event trigger, cooldown or "agent that fired it" field in batch 1**.
- Every logic change ships a test that fails without it: pure unit tests in `automations/clock.rs` and
  `automations/mod.rs`; GPUI headless tests in `crates/templum/src/workspace/tests/automations.rs` (new, declared in
  `tests/mod.rs` beside `mod agents_view;`). UI-only looks: say how they were verified.
- CI (ubuntu, macOS, Windows) has no agent CLI, no person's HOME or config: tests use `tempfile` folders, `cat`/`jobs`
  stand-ins as `tests/spawn.rs` does, never `claude` on PATH. Tests needing a program in front of its shell are
  `#[cfg(unix)]`, and so are the helpers only they use (Windows clippy calls them dead code). Compare folders as
  canonical `Path`s (macOS's `/var` is `/private/var`; Windows' separators). A long temp HOME path widens text: never
  assert a sentence holding a temp path by its full width.
- Times are the computer's clock: runtime state uses `SystemTime` and `chrono::Local`; headless tests make a rule
  due by setting its `Run::next` in the past (as `tests/schedules.rs`'s `due_now`), then advance the test clock by
  `AUTOMATION_LOOK`.
- Words, verbatim from the spec: tooltip and header "Automations"; "New"; "Close (Esc)"; "Run now", "Duplicate",
  "Delete"; "Asked by agents"; "Cancel s3"; "No automations yet"; "Type a prompt, run a command, start an agent or
  notify you at a time you choose."; "Off"; "Never"; "Next: …"; "Save", "Revert"; "New automation"; the delete
  dialog "Delete "*name*"?", "It won't run again. This can't be undone.", "Delete"; the hold, run and problem
  sentences quoted in the tasks below; command `automations`, "Show automations", `ctrl-shift-j` (`cmd-shift-j` on
  macOS).
- The popup is window chrome (never scaled by the zoom), drawn like the Agents popup, at `git_popup_size` and its
  place; the list column is 340 px; only one of Git, Requests, Agents and Automations is open at a time.
- Theme tokens and the terminal palette only for colors; nothing animates.
- Minors deferred go to `docs/minors.md` under their area; docs lines ≤ 120 columns (tables may run longer, as the
  other specs' do).
- Before handing over, the full check:
  `cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`, also with
  no agent CLI on PATH:
  `env -i PATH=~/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=~/.cargo RUSTUP_HOME=~/.rustup cargo test --workspace --no-fail-fast`;
  and the benches `cargo run --release -p templum -- --bench` and `-- --bench-mixed` (`p95_ms` ≤ 16.7).
- **Commit nothing.** Every "leave uncommitted" step means: run the task's tests, then go on.

## Review Focus

1. A prompt falls due while the person is writing a message to that agent (a draft): it is held "you are typing in
   it" and never pasted onto their text. → Task 4, `a_prompt_waits_while_its_person_types`.
2. The automation's workspace is stopped when its time comes: Skipped "Target missing: … is stopped", nothing typed
   or opened; once the workspace starts again, the next time runs. → Task 3,
   `a_stopped_workspace_skips_its_automations_until_it_starts`.
3. Two automations fall due in the same look for one agent: the first is typed, the second held until the agent is
   free again; never two pastes into one input. → Task 4, `two_prompts_for_one_agent_take_turns`.
4. A hand-edited `automations.json` holds a cron that doesn't parse: the file loads, that rule never runs and its row
   says "Never", the others run. → Task 3, `an_unreadable_cron_never_runs_and_says_never`.
5. The terminal a command automation keeps (`opened`) now runs an agent the person started there: the command is not
   typed into the agent; another terminal opens. → Task 5, `a_kept_terminal_running_an_agent_gets_no_command`.

---

### Task 1: The clock: next times, cron, daylight saving, words

**Files:**
- Modify: `crates/templum/Cargo.toml` (chrono: `features = ["clock", "serde"]`)
- Create: `crates/templum/src/automations/mod.rs` (module doc pointing at docs/specs/automations.md; `pub(crate) mod
  clock;` and `When` only, the rest comes in Task 2); declare `mod automations;` in `crates/templum/src/main.rs`
  beside `mod agents_view;`
- Create: `crates/templum/src/automations/clock.rs` (with its `#[cfg(test)] mod tests`)
- Modify: `crates/templum/src/workspace/schedules.rs` (`next_at` resolves its moment through `clock::resolve`: one
  daylight-saving rule in one place; the schedule tests keep passing)

**Interfaces:**
- Produces:
  ```rust
  // automations/mod.rs
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(crate) enum When {
      Once { at: NaiveDateTime },               // local
      Every { minutes: u32 },                   // 1 ..= 10080, real time from `since`
      Daily { at: NaiveTime },
      Weekly { days: Vec<u8>, at: NaiveTime },  // 1 Monday ..= 7 Sunday
      Monthly { days: Vec<u8>, at: NaiveTime }, // 1 ..= 31
      Cron { expr: String },
  }
  // automations/clock.rs
  pub(crate) const EVERY_MOST: u32 = 7 * 24 * 60;   // minutes
  pub(crate) const HORIZON_DAYS: u64 = 5 * 366;     // a calendar time further off is never
  pub(crate) struct Cron { /* bit sets: minutes, hours, days, months, weekdays; day fields restricted */ }
  impl Cron {
      pub(crate) fn parse(expr: &str) -> Result<Cron, String>;
      pub(crate) fn matches(&self, at: NaiveDateTime) -> bool;
  }
  /// `at` on `tz`'s clock: a time a daylight-saving change skips is the hour after, one it repeats the first.
  pub(crate) fn resolve<Tz: TimeZone>(tz: &Tz, at: NaiveDateTime) -> Option<DateTime<Tz>>;
  /// The first time strictly after `after` that `when` falls due; `since` is where Every counts from. None: never.
  pub(crate) fn next_after<Tz: TimeZone>(when: &When, since: &DateTime<Tz>, after: &DateTime<Tz>)
      -> Option<DateTime<Tz>>;
  /// What stops `when` from being saved at `now` (local), in the editor's words; None: nothing.
  pub(crate) fn when_problem(when: &When, now: NaiveDateTime) -> Option<String>;
  pub(crate) fn when_words(when: &When, today: NaiveDate) -> String;
  pub(crate) fn time_words(t: NaiveDateTime, now: NaiveDateTime) -> String;
  ```

- [ ] **Step 1: Write the failing tests** in `clock.rs`'s tests, all on `Utc` but the daylight-saving one:

  ```rust
  #[test] fn cron_reads_every_field_form() {
      // "*/15 9-17 * * mon-fri": 09:00, 09:15 … 17:45 on weekdays; not 08:45, not Saturday.
      // "0 0 1,15 * *", "5 4 * * 0" == "5 4 * * 7" == "5 4 * * SUN", "0 9 * jan,JUL *",
      // "0 12 * * 1-5/2" (Mon, Wed, Fri),
      // "30 6/4 * * *" (06:30, 10:30 … 22:30)
  }
  #[test] fn cron_says_what_is_wrong() {
      assert_eq!(Cron::parse("* * *").err().unwrap(), "Use five fields: minute hour day month weekday.");
      assert_eq!(Cron::parse("@daily").err().unwrap(), "Use five fields: minute hour day month weekday.");
      assert_eq!(Cron::parse("0 25 * * *").err().unwrap(), "Hour: 25 is past 23.");
      assert_eq!(Cron::parse("0 0 0 * *").err().unwrap(), "Day: 0 is before 1.");
      assert_eq!(Cron::parse("x * * * *").err().unwrap(), "Minute: \"x\" isn't a number or a name.");
      assert_eq!(Cron::parse("*/0 * * * *").err().unwrap(), "Minute: a step of 0 never moves.");
      assert_eq!(Cron::parse("0 9 * * 5-1").err().unwrap(), "Weekday: 5-1 runs backwards.");
  }
  #[test] fn cron_day_fields_match_either_when_both_are_set() {
      // "0 9 13 * fri": Friday 2026-11-06 09:00, Friday 2026-11-13 and Sunday 2026-12-13 match;
      // Monday 2026-11-09 doesn't.
  }
  #[test] fn next_times_of_each_kind() {
      // after = 2026-10-24 (a Saturday) 12:00 UTC, since = 2026-10-24 10:13
      // Daily 17:00 -> 2026-10-24 17:00; Daily 09:00 -> 2026-10-25 09:00;
      // Daily 12:00 -> 2026-10-25 12:00 (strictly after)
      // Weekly [1,2,3,4,5] 09:00 -> Mon 2026-10-26 09:00
      // Monthly [31] 09:00 from 2027-02-01 -> 2027-02-28; from 2028-02-01 -> 2028-02-29; [30,31] from 2027-04-01:
      //   2027-04-30, then (after it) 2027-05-30, then 2027-05-31
      // Every 120 -> 2026-10-24 12:13; after = 12:13 -> 14:13; after = 09:00 (before since) -> 10:13
      // Once 2026-10-24 18:00 -> Some; Once 2026-10-24 11:00 -> None
      // Cron "0 9 * * 1-5" -> Mon 2026-10-26 09:00
  }
  #[test] fn a_time_that_never_comes_is_never() {
      // Cron "0 0 30 2 *" -> None; Cron "0 0 29 2 *" from 2026-10-24 -> 2028-02-29 00:00; an unparsable cron -> None
  }
  #[test] fn daylight_saving_is_counted() {
      // On `Dst` (below): Daily 02:30 after 2027-03-27 12:00 local -> 2027-03-28 03:30 local (the hour after the gap);
      // Daily 02:30 after 2027-10-30 12:00 local -> 2027-10-31 02:30 at UTC 00:30 (the first of the repeated two);
      // Every 60 since 2027-03-28 00:30 local: next after 01:45 local is 03:30 local
      // (real time: 60 min after 01:30 at UTC+1).
  }
  #[test] fn problems_in_the_editors_words() {
      // now = 2026-10-24 12:00: Weekly [] -> "Choose at least one weekday."; Monthly [] -> "Choose at least one day.";
      // Every 0 and Every 10081 -> "Every runs from 1 minute to 7 days.";
      // Once 2026-10-24 11:00 -> "That time has passed.";
      // Cron "0 25 * * *" -> "Hour: 25 is past 23."; Cron "0 0 30 2 *" -> "It would never run."; Daily 09:00 -> None
  }
  #[test] fn words_of_each_kind() {
      // today = 2026-10-24
      // Daily 17:00 "Every day at 17:00"; Weekly [1,2,3,4,5] "Every weekday at 09:00"; [1] "Every Monday at 09:00";
      // [1,3,5] "Every Mon, Wed and Fri at 09:00"; [1..=7] "Every day at 09:00";
      // Monthly [1,15] "On the 1st and 15th of each month at 09:00";
      // [2,3,22,31] "On the 2nd, 3rd, 22nd and 31st of each month at 09:00";
      // Every 1 "Every minute", 10 "Every 10 minutes", 60 "Every hour", 120 "Every 2 hours", 90 "Every 90 minutes";
      // Once 2026-10-24T17:00 "On 24 Oct at 17:00", 2027-01-03T09:00 "On 3 Jan 2027 at 09:00";
      // Cron "0 9 * * 1-5" "At cron \"0 9 * * 1-5\""
  }
  #[test] fn time_words_say_the_day() {
      // now = 2026-10-24 (Saturday) 12:00: 17:00 that day "17:00"; 2026-10-25 09:00 "Tomorrow 09:00";
      // 2026-10-23 16:00 "Yesterday 16:00"; 2026-10-26 09:00 "Mon 09:00"; 2026-10-21 16:00 "Wed 16:00";
      // 2026-11-03 09:00 "3 Nov 09:00"; 2027-01-03 09:00 "3 Jan 2027 09:00"
  }
  ```

  The test zone, a European-style daylight saving (UTC+1, UTC+2 from 2027-03-28 01:00 UTC to 2027-10-31 01:00 UTC):

  ```rust
  #[derive(Clone, Copy, Debug)]
  struct Dst;
  impl Dst {
      fn summer(utc: &NaiveDateTime) -> bool {
          let (from, to) = (dt("2027-03-28T01:00"), dt("2027-10-31T01:00"));
          *utc >= from && *utc < to
      }
  }
  impl TimeZone for Dst {
      type Offset = FixedOffset;
      fn from_offset(_: &FixedOffset) -> Self { Dst }
      fn offset_from_local_date(&self, _: &NaiveDate) -> MappedLocalTime<FixedOffset> { unimplemented!() }
      fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> MappedLocalTime<FixedOffset> {
          let (one, two) = (FixedOffset::east_opt(3600).unwrap(), FixedOffset::east_opt(7200).unwrap());
          let fits = |o: FixedOffset| self.offset_from_utc_datetime(&(*local - o)) == o;
          match (fits(one), fits(two)) {
              (true, true) => MappedLocalTime::Ambiguous(two, one), // summer's 02:30 came first
              (true, false) => MappedLocalTime::Single(one),
              (false, true) => MappedLocalTime::Single(two),
              (false, false) => MappedLocalTime::None,
          }
      }
      fn offset_from_utc_date(&self, _: &NaiveDate) -> FixedOffset { unimplemented!() }
      fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> FixedOffset {
          FixedOffset::east_opt(if Dst::summer(utc) { 7200 } else { 3600 }).unwrap()
      }
  }
  ```

- [ ] **Step 2: Run them to see them fail:** `cargo test -p templum automations::clock` — Expected: FAIL (nothing
  defined).

- [ ] **Step 3: Implement.** `resolve`: `tz.from_local_datetime(&at).earliest()`, else the same an hour later (the
  rule `schedules::next_at` has; `next_at` then calls it). `next_after`: Once is `resolve(at)` if after `after`;
  Every is `since + k·minutes` for the smallest k ≥ 0 that is after `after` (real time); the calendar kinds walk the
  local days from `after`'s date for at most `HORIZON_DAYS`, and on each day that matches (Weekly: its ISO weekday;
  Monthly: a chosen day, or the month's last day for a chosen one past it; Cron: its month, and its day fields with
  cron's either-rule) take that day's times in order (one `at`, or the cron's hours × minutes) and return the first
  whose `resolve` is after `after`. Cron's grammar is the spec's (`*`, `n`, `a-b`, `*/s`, `a-b/s`, `a/s` running to the
  field's end, comma lists, `jan`–`dec`, `sun`–`sat` in any case, weekday 7 is 0); field names in errors are Minute,
  Hour, Day, Month, Weekday. `when_problem` is the spec's order: empty days, Every out of range, a cron error, a Once
  passed, then "It would never run." when `next_after` finds none. Words: three-letter weekday and month names in
  English, `%H:%M`, ordinals 1st 2nd 3rd 4th … 11th 12th 13th … 21st 22nd 23rd … 31st.

- [ ] **Step 4: Run** `cargo test -p templum automations::clock` and `cargo test -p templum schedules` — Expected:
  PASS.

- [ ] **Step 5: Leave uncommitted.**

### Task 2: The saved list and its file

**Files:**
- Modify: `crates/templum/src/automations/mod.rs` (the model, load and save, with unit tests)
- Modify: `crates/templum/src/workspace/mod.rs` (fields `automations: Automations`, `automations_saved:
  Option<Automations>`, `automations_frozen: bool`, beside `standing*`)
- Modify: `crates/templum/src/workspace/spaces.rs` (load beside `instructions::load`, the same three arms and
  notifications with "automations")
- Modify: `crates/templum/src/workspace/persist.rs` (the snapshot writes it beside `standing`; `snapshot_done` takes a
  sixth file; the quit save) and the five `snapshot_done(` calls in tests
- Test: `crates/templum/src/workspace/tests/automations.rs` (new; `mod automations;` in `tests/mod.rs`)

**Interfaces:**
- Consumes: `When` (Task 1), `session::{read, write_newest, unique_ids, Loaded}` as `instructions.rs` uses them.
- Produces:
  ```rust
  pub(crate) const VERSION: u32 = 1;
  pub(crate) const NAME_MAX: usize = 60;
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub(crate) struct Automations { pub version: u32, #[serde(default)] pub next_id: u64,
                                  #[serde(default)] pub automations: Vec<Automation> }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub(crate) struct Automation {
      pub id: u64,
      #[serde(default)] pub name: String,
      #[serde(default = "on")] pub on: bool,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub workspace: Option<String>,
      pub when: When,
      #[serde(rename = "do")] pub act: Do,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub max_runs: Option<u32>,
      #[serde(default)] pub runs: u32,
      #[serde(default)] pub since: i64,                    // Unix seconds
      #[serde(default, skip_serializing_if = "Option::is_none")] pub last: Option<LastRun>,
      #[serde(default, skip_serializing_if = "Option::is_none")] pub opened: Option<String>, // a block uid
  }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(crate) enum Do {
      Prompt { block: String, text: String },
      Command { #[serde(default, skip_serializing_if = "Option::is_none")] terminal: Option<String>,
                #[serde(default, skip_serializing_if = "Option::is_none")] folder: Option<String>,
                text: String },
      Agent { agent: String, /* folder, model, effort, prompt: Option<String>, each skipped when None */ },
      Notify { text: String },
  }
  #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
  pub(crate) struct LastRun { pub at: i64, pub went: Went, pub about: String }
  #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
  #[serde(rename_all = "snake_case")]
  pub(crate) enum Went { Ran, Skipped, Failed, Missed }
  impl Automations {
      pub fn add(&mut self, a: Automation) -> u64;          // gives it the next id, appends it
      pub fn get(&self, id: u64) -> Option<&Automation>;
      pub fn get_mut(&mut self, id: u64) -> Option<&mut Automation>;
      pub fn remove(&mut self, id: u64);
      /// A copy after the last: "<name> (copy)" (or the empty name kept empty), off, no runs, last run nor opened.
      pub fn duplicate(&mut self, id: u64) -> Option<u64>;
  }
  pub(crate) fn path(dir: &Path) -> PathBuf;               // dir.join("automations.json")
  pub(crate) fn load(dir: &Path) -> Loaded<Automations>;
  pub(crate) fn save(dir: &Path, automations: &Automations, generation: u64) -> io::Result<()>;
  ```

- [ ] **Step 1: Write the failing tests.** Unit, in `automations/mod.rs`:
  - `the_specs_example_round_trips`: the JSON block of the spec's "Where they are saved", written to a temp folder,
    loads (`Loaded::Saved`) with `when == When::Weekly { days: vec![1,2,3,4,5], at: 09:00 }`, `act == Do::Prompt {
    block: "abc12", text: "Review the open pull requests." }`, `last.went == Went::Ran`, the second's `max_runs ==
    Some(6)` and `opened == Some("q3x9z")`; saved and loaded again it is equal.
  - `ids_are_never_reused_and_a_copy_is_off`: add, remove the last, add: a new id; `duplicate` of an on rule with runs
    3, a last run and `opened` gives `on == false`, `runs == 0`, `last == None`, `opened == None`, name "A (copy)",
    placed last.
  Headless, in `tests/automations.rs` (as `tests/instructions.rs`'s file tests):
  - `the_automations_are_saved_with_the_snapshot`: with a data folder, add one, run a snapshot: `automations.json`
    holds it; a restart (`open_in` on that folder) loads it.
  - `an_unreadable_file_is_set_aside_and_never_written_over`: a malformed `automations.json` is kept as
    `automations.json.bak` and the list is empty; an unreadable one (a folder in its place) freezes the list: the next
    snapshot writes nothing.

- [ ] **Step 2: Run** `cargo test -p templum automations` — Expected: FAIL.

- [ ] **Step 3: Implement** the model as `instructions.rs` is written (`load` runs `session::unique_ids`), and the
  wiring by copying `standing`'s (`standing_saved`, `standing_frozen`) in `spaces.rs` and `persist.rs`.

- [ ] **Step 4: Run** `cargo test -p templum automations` and `cargo test -p templum persist` — Expected: PASS.

- [ ] **Step 5: Leave uncommitted.**

### Task 3: Running on time: the look, missed, asleep, Stop after, Target missing, Notify

**Files:**
- Create: `crates/templum/src/workspace/automations.rs` (module doc pointing at the spec); `mod automations;` in
  `workspace/mod.rs` beside `mod schedules;`
- Modify: `crates/templum/src/workspace/mod.rs` (field `automation_runs: HashMap<u64, Run>`; the loop: a
  `cx.spawn_in(window, …)` every `AUTOMATION_LOOK` calling `update_in(… ws.look_automations(window, cx))`, kept in
  a `_automations: Task<()>` field beside `_snapshots`)
- Modify: `crates/templum/src/workspace/spaces.rs` (after the automations load:
  `self.start_automations(SystemTime::now())`)
- Test: `crates/templum/src/workspace/tests/automations.rs`

**Interfaces:**
- Consumes: Task 1's `next_after`, `when_words`; Task 2's model; `Workspace::find_block(uid) -> Option<(String,
  BlockId)>` (control.rs), `Workspace::spaces()`, `Space::{stopped, name, id}`, `Workspace::agents`,
  `show_notification`, `Workspace::delivery(window)` (usage.rs).
- Produces:
  ```rust
  pub(crate) const AUTOMATION_LOOK: Duration = Duration::from_secs(1);
  pub(crate) const HOLD_MOST: Duration = Duration::from_secs(60 * 60);
  /// The asker of an automation's tasks: a word no terminal has.
  pub(crate) fn asker(id: u64) -> String;                  // "automation-<id>"
  /// What an automation is doing, in memory only.
  #[derive(Default)]
  pub(crate) struct Run {
      pub next: Option<SystemTime>,  // None: off, or never again
      pub held: Option<Held>,        // due, waiting for its target
      pub typing: bool,              // a try's answer is on its way (`Tried::Later`): no new try meanwhile
      pub unseen: bool,              // its last run went other than Ran since the popup was opened
  }
  pub(crate) struct Held { pub since: SystemTime, pub why: String }
  /// One try of a Do.
  pub(super) enum Tried { Went(Went, String), Later(Task<Result<String, String>>), Hold(String) }
  impl Workspace {
      pub(super) fn start_automations(&mut self, now: SystemTime);
      pub(super) fn look_automations(&mut self, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn run_automation_now(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn set_automation_on(&mut self, id: u64, on: bool, cx: &mut Context<Self>);
      /// Err: "Target missing: abc12 isn't open" (…: "<name> is stopped", "a deleted workspace",
      /// "<id> isn't in Settings ▸ Agents").
      pub(crate) fn automation_target(&self, a: &Automation) -> Result<(), String>;
      /// The preview sentence: "Every weekday at 09:00, type "review open PRs" into Claude Code (abc12)."
      pub(crate) fn automation_sentence(&self, a: &Automation, cx: &App) -> String;
      pub(crate) fn automations_unseen(&self) -> usize;
      fn try_do(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) -> Tried; // Tasks 4–5 add arms
      /// Its run ended: `last`, then for a due run (`counts`) its count, Stop after, and a Once turned off.
      fn ended(&mut self, id: u64, went: Went, about: String, counts: bool, cx: &mut Context<Self>);
  }
  ```
- Behavior to pin (the spec's "Time" and "At the edges"):
  - `start_automations(now)`: for each rule that is on, `from = max(since, last.at)`; when `next_after(when, since,
    from) <= now`, `last = Missed` at that time with about "Templum was closed.", `unseen`, and a Once is turned off;
    then `next = next_after(when, since, now)`.
  - A rule that is on with no `Run` yet (added since the start) gets one, `next = next_after(when, since, now)`:
    tests add a rule, then set its run's `next` in the past.
  - Each look: a rule whose `next <= now` gets `next = next_after(…, now)` (a sleep runs it once), then, unless a
    run is held or typing (the time joins it), its target is checked (Err: `ended(Skipped, <the Err's words>)`) and
    its Do tried. Held runs are tried again each look; one held since `HOLD_MOST` is `ended(Skipped, "<uid> stayed
    busy for an hour.")`. `cx.notify()` only when something changed.
  - `ended` with `counts` and `Went::Ran` adds a run; at `max_runs` it turns the rule off and appends " Ran N of N
    times, now off." to about. `unseen` is set for every Went but Ran.
  - `run_automation_now` goes through the same target check and try with `counts: false`, leaving `next` alone, also
    for a rule that is off.
  - `set_automation_on(true)` sets `since = now`, `runs = 0` and computes `next`; off drops `next` and the held run.
  - Notify: a `Notification` titled the name (or the sentence), its text the message, `delivery(window)`;
    `Tried::Went(Ran, "Notified.")`. Task 6 gives it its click (`show_automation`).

- [ ] **Step 1: Write the failing tests** (Notify rules: they need no agent and run on every OS):
  - `a_due_automation_notifies_and_says_it_ran`: an on `Every { minutes: 10 }` Notify "Stand-up"; its `next` set in
    the past; one look: `notification_count` went up by one, `last.went == Ran`, `last.about == "Notified."`, `next`
    about 10 minutes ahead.
  - `a_time_missed_while_closed_is_missed_not_run`: a data folder whose `automations.json` holds a Daily 00:00 with
    `since` two days ago and a Once yesterday with `since` three days ago, both Notify; open it: no notification, both
    `last.went == Missed` with about "Templum was closed.", the Once is off, the Daily's `next` is after now.
  - `a_sleep_runs_a_missed_time_once`: `Every { minutes: 60 }` with `next` three hours ago: two looks give one
    notification; `next` is after now.
  - `stop_after_turns_it_off`: `max_runs: Some(2)`: two due looks, then `runs == 2`, `on == false`, `last.about ==
    "Notified. Ran 2 of 2 times, now off."`; a third look runs nothing.
  - `run_now_runs_an_off_automation_without_counting`: off rule, `run_automation_now`: one notification, `runs == 0`,
    `next == None`.
  - `a_missing_target_is_skipped_and_counted_unseen`: a Prompt to `"zzzzz"`: `automation_target` is
    `Err("Target missing: zzzzz isn't open")`; due: `last.went == Skipped` with that about, `automations_unseen() == 1`.
  - `a_stopped_workspace_skips_its_automations_until_it_starts` (Review Focus 2): a Notify for a second workspace;
    `stop_space` it; due: Skipped "Target missing: <its name> is stopped", no notification; `switch_to` it (it
    starts), due: a notification.
  - `an_unreadable_cron_never_runs_and_says_never` (Review Focus 4): a file with a `cron` of `"0 25 * * *"` and a
    Notify Every 10 beside it; both load; the cron's `next == None`; the other runs when due.

- [ ] **Step 2: Run** `cargo test -p templum workspace::tests::automations` — Expected: FAIL.

- [ ] **Step 3: Implement** `workspace/automations.rs` as pinned above. `Tried::Later`'s task is awaited in a
  `cx.spawn` that calls `ended` with `Ran` (Ok's words) or `Failed` (Err's words) and clears `typing`; Tasks 4–5 make
  it from an `Outcome::Later(Task<Reply>)` by a `cx.spawn` that maps `Reply::is_error` to Err.

- [ ] **Step 4: Run** `cargo test -p templum workspace::tests::automations` — Expected: PASS.

- [ ] **Step 5: Leave uncommitted.**

### Task 4: Send a prompt to an agent

**Files:**
- Modify: `crates/templum/src/workspace/automations.rs` (the Prompt arm of `try_do`; `prompt_busy`)
- Modify: `crates/templum/src/workspace/delegate.rs` (only visibility: `Pane::agent_starting`, `TYPING`,
  `NOTICE_GAP` to `pub(super)` where not already)
- Modify: `crates/templum/src/workspace/tests/agents_view.rs` (make `agent`, `begins` and `ends` `pub(super)`)
- Test: `crates/templum/src/workspace/tests/automations.rs`

**Interfaces:**
- Consumes: `Workspace::hand_task(asker, by, (space, block), uid, task, wait: false, tools::TASK_WAIT, None, cx) ->
  Outcome` (delegate.rs), `Workspace::doing(&Pane) -> Doing` and `Doing::works()` (agents_view.rs),
  `Pane::{task(now), takes_notice(), has_draft(), keyed, shell_in_front(), seen, handed}`.
- Produces:
  ```rust
  /// Why the agent in `pane` can't take a prompt now; None: it can.
  fn prompt_busy(&self, pane: &Pane) -> Option<&'static str>;
  // "it is starting" | "it needs you" | "it is working" | "it is on a task" | "you are typing in it"
  // | "Templum is typing into it"
  ```
- Behavior to pin (the spec's "Send a prompt to an agent"):
  - no agent seen there, or its shell in front: `Tried::Went(Failed, "<uid> runs no agent now: a prompt is never
    typed into a shell.")`, nothing typed;
  - `prompt_busy` Some: `Tried::Hold("Waiting for <uid>: <why>")`;
  - else `hand_task` with `asker(id)` and `by = automation "<name or when_words>"` (so its first line reads
    `[from automation "Morning review", task t1] …`): `Outcome::Later` → `Tried::Later` whose Ok is "Typed into
    <uid> as task t<n>." (n: the task `hand_task` set on the pane) and whose Err is the reply's text;
    `Outcome::Now` an error → `Failed`, else `Ran` with its words.

- [ ] **Step 1: Write the failing tests** (`#[cfg(unix)]`; agents as `tests/delegate.rs`'s `agents_running`, events as
  `tests/agents_view.rs`'s `begins`/`ends`; the typing confirmed as `hand_over` does: `screen_until` changed, then
  `entered` and the `submitted` event):
  - `a_prompt_is_handed_as_a_task_from_the_automation`: a Prompt "Morning review" to an idle agent; due: its screen
    shows `[from automation "Morning review", task t1] Your task is in the pasted text`; once confirmed, `last ==
    Ran "Typed into <uid> as task t1."` and the pane's `handed.by == "automation-1"`.
  - `a_prompt_waits_while_its_agent_works`: `begins` on the agent; due: nothing typed, `held.why == "Waiting for
    <uid>: it is working"`; `ends`; the next looks type it and clear `held`. Then working again with a held run whose
    `since` is 61 minutes ago: `Skipped "<uid> stayed busy for an hour."`.
  - `a_prompt_waits_while_its_person_types` (Review Focus 1): the person types into the agent (a key through the
    workspace, so a draft starts); due: held "…: you are typing in it", the screen shows only their text; their Enter
    ends the draft; the next looks (past `TYPING` and `NOTICE_GAP`) type the prompt.
  - `two_prompts_for_one_agent_take_turns` (Review Focus 3): two Prompt rules to one idle agent, due in the same look:
    the first is typed (task t1), the second held ("…: it is on a task" or "…: Templum is typing into it"); after the
    first task ends (`ends`), the second goes as t2.
  - `a_prompt_never_goes_into_a_shell`: a Prompt to a terminal running no agent: `Failed "<uid> runs no agent now: a
    prompt is never typed into a shell."`, its screen unchanged.

- [ ] **Step 2: Run** `cargo test -p templum workspace::tests::automations` — Expected: the new tests FAIL.

- [ ] **Step 3: Implement** the arm and `prompt_busy`, in that order of checks.

- [ ] **Step 4: Run** `cargo test -p templum workspace::tests::automations` and `cargo test -p templum delegate` —
  Expected: PASS.

- [ ] **Step 5: Leave uncommitted.**

### Task 5: Run a command, and start an agent

**Files:**
- Modify: `crates/templum/src/workspace/automations.rs` (the Command and Agent arms; `command_busy`; the folder)
- Modify: `crates/templum/src/workspace/spawn.rs` (`Opener::Automation { asker: String, by: String }`: its task's
  asker and signature, `opened_by` None; its answer's words as the Templum Agent's arm)
- Test: `crates/templum/src/workspace/tests/automations.rs`

**Interfaces:**
- Consumes: `Workspace::type_text(asker, (space, id), uid, text, enter: true, cx) -> Outcome`,
  `Workspace::add_block_in(space, NewBlock::Terminal { folder, command: Some(text) }, Place::Free, None, window, cx)
  -> Result<BlockId, String>`, `Workspace::open_agent_in(space, &entry, None, &folder, &spawn, Place::Free, &[],
  Opener::Automation { … }, window, cx) -> (Reply, Option<String>)`, `AgentEntry::launch_args_with`,
  `Workspace::spawn_problem`, `launcher::expand`, `Space::folder`.
- Produces:
  ```rust
  /// Why a command can't be typed into the terminal in `pane` now; None: its shell is at its prompt.
  fn command_busy(&self, pane: &Pane) -> Option<&'static str>;
  // "an agent runs in it" | "a command runs in it" (its status' `running`, or its shell not in front)
  // | "you are typing in it" (a key within TYPING)
  /// The folder a new block opens in: the rule's (`~` expanded), else its workspace's, else the home folder.
  fn automation_folder(&self, a: &Automation) -> PathBuf;
  ```
- Behavior to pin (the spec's "Run a command" and "Start an agent"):
  - In a terminal: `command_busy` Some → `Tried::Hold("Waiting for <uid>: <why>")`; else `type_text` → Later with
    Ok "Typed into <uid>.".
  - In a new terminal: a folder that isn't a folder → `Failed "No folder <as typed>."`; the kept `opened` terminal,
    when it is open in the rule's workspace, runs no agent and `command_busy` is None, gets the command ("Typed into
    <uid>."); otherwise `add_block_in` opens one, `opened` becomes its uid, `Ran "Opened <uid> in <folder>."`.
  - Start an agent: the entry by id (missing → Target missing, Task 3); `launch_args_with(model, effort, None)` Err
    or `spawn_problem` Some → `Failed` with its words; else `open_agent_in` with a `tools::Spawn { agent:
    Some(id), prompt, folder, model, effort, worktree: None, fork: false, role: None, args: None }` → `Ran "Opened
    <title> as <uid> in <folder>."`, or `Failed` with the reply's text.

- [ ] **Step 1: Write the failing tests** (`#[cfg(unix)]` where an agent is in front; folders from `tempfile`):
  - `a_command_opens_a_terminal_and_reuses_it`: a Command in a new terminal, folder a temp dir, text `echo automated`,
    for the shown workspace; due: one more terminal block, `opened == Some(its uid)`, `last.about` starts "Opened
    <uid> in "; its screen shows `echo automated`; due again: no new block, `last.about == "Typed into <uid>."`;
    the block closed, due: a new block and `opened` its uid.
  - `a_command_waits_for_its_terminal`: a Command into a terminal running an agent (`seen.agent` set, its program in
    front): held "Waiting for <uid>: an agent runs in it", nothing typed.
  - `a_missing_folder_fails`: folder `<tempdir>/gone`: `Failed`, about starting "No folder ", no block opened.
  - `a_kept_terminal_running_an_agent_gets_no_command` (Review Focus 5): the kept terminal now runs an agent: due
    opens another terminal; the agent's screen doesn't show the command.
  - `start_an_agent_opens_it_with_its_prompt`: as `tests/spawn.rs`'s `spawner` (its agents run `cat`), an Agent rule
    for `claude` in a temp folder with prompt "Triage the new issues": due: a new terminal block whose pane has a
    `prompt` waiting and `handed.by == "automation-1"`, `last.about` starts "Opened Claude Code as ". A rule with
    effort `"turbo"`: `Failed` with `launch_args_with`'s words.

- [ ] **Step 2: Run** `cargo test -p templum workspace::tests::automations` — Expected: the new tests FAIL.

- [ ] **Step 3: Implement** the arms, `command_busy`, `automation_folder` and the `Opener` variant.

- [ ] **Step 4: Run** `cargo test -p templum workspace::tests::automations` and `cargo test -p templum spawn` —
  Expected: PASS.

- [ ] **Step 5: Leave uncommitted.**

### Task 6: The popup, its list, Asked by agents, the button and the shortcut

**Files:**
- Create: `crates/templum/src/automations_view/mod.rs` (the popup's frame, the list, the rows, Asked by agents, the
  sidebar button); `mod automations_view;` in `main.rs`
- Modify: `crates/templum/src/keys.rs` (`Command::Automations`: every arm `AllAgents` has, with `automations`, "Show
  automations", `ctrl-shift-j` / `cmd-shift-j`, and a `Keys::automations` field)
- Modify: `crates/templum/src/workspace/input.rs` (`Command::Automations => self.toggle_automations(window, cx)`)
- Modify: `crates/templum/src/workspace/automations.rs` (the popup's state and actions below)
- Modify: `crates/templum/src/workspace/mod.rs` (fields `automations_open: bool`, `automations_at: Option<u64>`, the
  popup's focus handle)
- Modify: `crates/templum/src/workspace/git_ops.rs`, `workspace/inbox.rs`, `workspace/agents_view.rs` (their opens
  set `automations_open = false`)
- Modify: `crates/templum/src/sidebar.rs` (the button between Requests and Git),
  `crates/templum/src/workspace/render.rs` (the popup at the Agents popup's place)
- Test: `crates/templum/src/workspace/tests/automations.rs`; `keys.rs`'s tests

**Interfaces:**
- Consumes: Tasks 3–5; `Workspace::schedules`, `cancel_schedule_now(id, cx)`, `signature_of(terminal, cx)`;
  `inbox::{count_badge, dim, section}`; `crate::prompt::confirm`.
- Produces:
  ```rust
  impl Workspace {
      /// Closes the other three popups and clears every `unseen`.
      pub(crate) fn open_automations(&mut self, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn close_automations(&mut self, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn toggle_automations(&mut self, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn show_automation(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>);
      /// A row's right side, line 1: "17:00", "Tomorrow 09:00", "Mon 09:00", "3 Nov 09:00", "Off", "Never".
      pub(crate) fn next_words(&self, id: u64, now: SystemTime) -> String;
      /// Line 2's right side: "Ran 16:00", "Failed Yesterday 16:00", "Missed 24 Oct 17:00", "Waiting for abc12";
      /// None before the first run.
      pub(crate) fn last_words(&self, id: u64, now: SystemTime) -> Option<String>;
      /// Asked by agents, oldest first: (schedule id, "s3 from Claude Code · api (b7k2m) to k9x1d",
      /// "every 10 min · next 15:10 · <first line>").
      pub(crate) fn schedule_rows(&self, cx: &App) -> Vec<(u64, String, String)>;
      pub(crate) fn duplicate_automation(&mut self, id: u64, cx: &mut Context<Self>) -> Option<u64>; // selects it
      pub(crate) fn ask_delete_automation(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>);
      pub(crate) fn delete_automation(&mut self, id: u64, cx: &mut Context<Self>); // drops its Run and draft
      pub(crate) fn on_automations_key(&mut self, e: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>);
  }
  // automations_view/mod.rs
  pub(crate) fn popup(ws: &Workspace, w: f32, h: f32, window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement;
  /// "automations-button", its badge "automations-badge".
  pub(crate) fn button(ws: &Workspace, size: f32, cx: &mut Context<Workspace>) -> Div;
  ```
  Element ids for tests: `automations-button`, `automations-badge`, `automation-row-<id>`, `automation-run-<id>`,
  `automation-duplicate-<id>`, `automation-delete-<id>`, `automation-switch-<id>`, `schedule-cancel-<id>`.

- [ ] **Step 1: Write the failing tests:**
  - `keys.rs`: `automations_is_ctrl_shift_j` (defaults, and the typed keys find `Command::Automations`), as
    `all_agents_is_ctrl_shift_l`.
  - `one_popup_of_four_is_open`: Ctrl+Shift+J opens Automations; the Agents shortcut opens Agents and closes it; the
    button (`automations-button`) opens it again and closes Agents; Ctrl+Shift+J closes it.
  - `rows_say_the_next_and_the_last_run`: `next_words` of an off rule "Off", of a Once in the past "Never", of a rule
    due at today's 23:59 "23:59"; `last_words` of a held prompt "Waiting for <uid>", of a Failed run at now "Failed
    <HH:MM>".
  - `the_badge_counts_unseen_runs_until_the_popup_opens`: a Skipped run: `automations_unseen() == 1`; open the popup:
    0.
  - `asked_by_agents_lists_schedules_and_cancels_them` (`#[cfg(unix)]`, a schedule made as `tests/schedules.rs`
    does): one row, its first line starting "s1 from ", its second "every 1 min · next "; clicking
    `schedule-cancel-1` leaves no schedule.
  - `duplicate_is_off_and_delete_asks`: Duplicate gives an off copy, selected; Delete opens the dialog ("Delete
    "A"?"); confirming it removes the rule and its `Run`.
  - `arrows_select_and_esc_closes`: ↓ selects the next rule, ↑ the previous, none past the ends; Esc closes the popup.

- [ ] **Step 2: Run** `cargo test -p templum automations` — Expected: FAIL.

- [ ] **Step 3: Implement** the view after `agents_view.rs` (frame, `occlude`, `capture_key_down`, scroll wheel
  stopped) with a plain scrolling list (tens of rows: no virtual list), rows as the spec's "The list", the row's
  switch calling `set_automation_on`, the three icon buttons on hover and on the selected row, and Asked by agents
  below. The right column is a placeholder until Task 7. Task 3's Notify gets its click: `show_automation(id)`.

- [ ] **Step 4: Run** `cargo test -p templum automations` and `cargo test -p templum keys` — Expected: PASS.

- [ ] **Step 5: Leave uncommitted.**

### Task 7: The editor: draft, pickers, preview, Save and Revert, New

**Files:**
- Create: `crates/templum/src/automations_view/editor.rs` (the form; its field entities kept as the builder sees fit)
- Modify: `crates/templum/src/workspace/automations.rs` (drafts, validation, Save, Revert, New)
- Modify: `crates/templum/src/workspace/mod.rs` (field `automation_drafts: HashMap<u64, Automation>`)
- Modify: `crates/templum/src/settings/row.rs` only if its dropdown (`menu_button`) must become `pub(crate)`
- Test: `crates/templum/src/workspace/tests/automations.rs`

**Interfaces:**
- Consumes: Task 1's `when_problem`, `next_after`, `time_words`; Task 3's `automation_sentence`,
  `automation_target`.
- Produces:
  ```rust
  /// The draft key of a new automation, not saved yet.
  pub(crate) const NEW: u64 = 0;
  impl Workspace {
      /// "New automation": Every day at 09:00, Notify me, for the workspace shown; selected.
      pub(crate) fn new_automation(&mut self, cx: &mut Context<Self>);
      /// The draft of `id`, else the saved rule.
      pub(crate) fn automation_draft(&self, id: u64) -> Option<&Automation>;
      pub(crate) fn edit_automation(&mut self, id: u64, edit: impl FnOnce(&mut Automation), cx: &mut Context<Self>);
      /// What stops `a` from being saved, in the spec's words; None: nothing.
      pub(crate) fn automation_problem(&self, a: &Automation, now: NaiveDateTime) -> Option<String>;
      /// Saves the draft of `id`: a new one is added on; any is counted from now (`since`, `runs` 0, its held run and
      /// typing dropped, `next` computed). Err: its problem, nothing saved.
      pub(crate) fn save_automation(&mut self, id: u64, cx: &mut Context<Self>) -> Result<u64, String>;
      pub(crate) fn revert_automation(&mut self, id: u64, cx: &mut Context<Self>); // NEW's: removes it
  }
  ```
- `automation_problem`'s order and words: "A name is at most 60 characters." (over `NAME_MAX` characters);
  `when_problem`; then the Do: Prompt "Choose an agent." (empty block), "Write the prompt."; Command "Choose a
  terminal." (`terminal == Some("")`), "Write the command."; Agent "Choose an agent." (empty id), then
  `launch_args_with`'s error for its model and effort; Notify "Write the text."; then "Choose the workspace it opens
  in." for a Command in a new terminal or an Agent with no workspace. A text field the form can't read keeps the draft
  as it was and shows "Write the time as HH:MM." or "Write the date as YYYY-MM-DD." itself.

- [ ] **Step 1: Write the failing tests:**
  - `save_checks_the_draft`: each problem above from a draft that has only it, its exact words; nothing saved; a
    good draft saves.
  - `new_is_added_on_when_saved_and_revert_removes_it`: `new_automation` adds a draft row (`NEW`) and selects it;
    `revert_automation(NEW)` removes it; again, with a Notify text, `save_automation(NEW)` returns a new id, the rule
    is on, its `next` set.
  - `saving_counts_from_now_and_drops_a_held_run` (`#[cfg(unix)]`): a held prompt with `runs == 3`: save it with a
    new prompt: `held == None`, `runs == 0`, `since` now.
  - `a_draft_stays_while_another_row_is_selected`: edit A's name, select B, select A: the draft holds the new name,
    the saved rule the old one; Revert gives the old one back.
  - `the_preview_says_the_sentence`: a Weekly Mon–Fri 09:00 Prompt "review open PRs" to an agent block titled T with
    uid U: `automation_sentence == format!("Every weekday at 09:00, type \"review open PRs\" into {T} ({U}).")`;
    a Command in a new terminal folder "~/api" text "make test": "Every day at 17:00, run \"make test\" in a new
    terminal in ~/api."; a Notify "Stand-up": "On 24 Oct at 17:00, notify you: \"Stand-up\"." (dated in the current
    year so its words carry no year).

- [ ] **Step 2: Run** `cargo test -p templum automations` — Expected: FAIL.

- [ ] **Step 3: Implement** the drafts and Save in `workspace/automations.rs`, then the form in `editor.rs` as the
  spec's "The editor" orders it (Name with the sentence as placeholder; When and its fields, Advanced showing its next
  three times or its error under the field; Do and its fields, a saved block that isn't open listed as "<uid> (not
  open)"; Where, with only workspaces for a new terminal or an agent;
  Stop after; the preview and "Next: …"; Last run in full; Save and Revert with the problem in red above them; the
  row's dot while a draft exists). Dropdowns as Settings ▸ Instructions' Where; toggles for weekdays and days; plain
  `HH:MM` and `YYYY-MM-DD` fields (a gpui-kit date picker is the builder's call).

- [ ] **Step 4: Run** `cargo test -p templum automations` — Expected: PASS. Then look at it: `cargo run -p templum
  -- --dev`, make one of each When and each Do, check the preview, Save's problems, a Notify every minute, Run now,
  Duplicate, Delete, and the popup's place and size beside the Agents popup's. Write in As built what was seen, or
  that no display was at hand.

- [ ] **Step 5: Leave uncommitted.**

### Task 8: The docs, the minors, the full check and the benches

**Files:**
- Modify: `docs/specs/automations.md` (Status: batch 1 implemented; As built: what the builder chose where the spec
  left it open, and anything that differs)
- Modify: `docs/ARCHITECTURE.md` (`automations/`, `workspace/automations.rs`, `automations_view/`, and
  `automations.json` among the app-wide files), `docs/specs/workspaces.md` (the sidebar's foot: Agents, Requests,
  Automations, Git), `docs/specs/config.md` (`automations` in `[keys]`), `docs/specs/agent-tools.md` (`schedule`: the
  person also sees and cancels them under Automations ▸ Asked by agents), `docs/guide.md` (the Automations button)
- Modify: `docs/minors.md`, under a new "Automations" area: "A command typed into a terminal at its prompt joins
  what the person half-typed and left there: Templum can't read the input line. Fix: read the line between the
  prompt's mark (OSC 133 A) and the cursor, and hold the command while it isn't empty." — and every other minor the
  build deferred.

- [ ] **Step 1: Run the full check**, then the reduced-PATH run (Global Constraints). Expected: all green; report
  failures with their output if any remain.
- [ ] **Step 2: Run** `cargo run --release -p templum -- --bench` and `-- --bench-mixed`; keep their JSON lines for
  the pull request. Expected: `p95_ms` ≤ 16.7 in both.
- [ ] **Step 3: Leave everything uncommitted** and report: the tasks done, the test counts, the two JSON lines, how
  the editor was verified, the minors written.
