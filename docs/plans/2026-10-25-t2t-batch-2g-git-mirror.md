# Templum to Templum, Batch 2g (the Git Popup on the Mirror) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer in a Templum agent block does the whole plan, then
> fresh Opus 5.5 reviewers review it in a loop until one passes it. The implementer **commits nothing**: everything
> stays uncommitted for the orchestrator.

**Goal:** Kanban card 171 part 2, batch 2, seventh of eight parts: the Git popup opens on a mirrored workspace and
looks and acts as it does locally, but every read and act is an operation of home's `git` tool (2f), never a git
command or a file read on this computer; home's status is read every 2 s while the popup is shown, the diff as the tab
asks; long operations go on the client's slow lane; what a Control share can't do is dimmed with the rule's words; the
folder, tab and message being written live in memory only.

**Architecture:** One seam, in the popup's job runner. Today every popup job is `git_job(at, closure, done)`: the
closure runs on the background executor against a `Repo`, `done` runs on the UI thread. 2g adds `git_op(at, op, here,
home, done)` (and `act_op`): on this computer it is exactly `git_job(at, move || here(&repo), done)`; on a mirrored
workspace it is `home_job`, which keeps the popup's queue and busy rules but sends `op` to home through
`remote_call` (`Call { tool: "git", args: op + workspace + folder, slow }`) and turns home's answer into the same `T`
with `home` (usually `parsed::<X>`, home's refusal becoming a `GitError` whose stderr is home's `details`), so every
`done` closure stays as it is (2f's "Interfaces for later parts": "a popup that runs on a remote replaces each job's
closure with the matching `git` call and keeps its `done` closure"). A mirrored workspace's `Git` never holds a `Repo`
(`Found::Home(root)`), its `folder` is empty and home's path lives in `Git::home`, so no local code path can stat or run
git on it. Task 1 carries home's `details` through the client; Task 2 builds the seam and the status; Tasks 3–5 move
every other job onto it (acts, the tabs' reads, discards and the Full gates); Task 6 the remote-only parts (folder
menu, Generate at home, the controls left out); Task 7 the end-to-end cases, docs and checks.

**Tech Stack:** Rust 2024; GPUI through gpui-kit `=0.7.3` (only in `crates/templum`); `templum_git::ops` (2f);
`templum_api::client` (2a's answered calls and slow lane); serde/serde_json (in the tree).

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), "Batch 2: the whole workspace, live":
"Parts ▸ 2g", "What works on a remote workspace" (the three Git rows), "The Git popup on a remote workspace", "One rule
for what the mirror can do", "The mirror's side", "Constraints (batch 2)", "Left out of batch 2" (item 11: Clone,
Choose another folder…, the ⚙ beside Generate, Open in editor, Run in a terminal), the threat model's *(batch 2)* rows,
"As built ▸ 2a–2f". [2f's plan](2026-10-25-t2t-batch-2f-git-home.md) "Interfaces for later parts" (operations,
arguments, answers, scopes, errors). [docs/specs/git.md](../specs/git.md) for the popup's behaviour, which must not
change on this computer.

**Code it builds on:** `crates/templum/src/workspace/{git_ops.rs, git_changes.rs, git_branches.rs, git_switch.rs,
git_sync.rs, git_history.rs, git_compare.rs, git_conflicts.rs, git_discard.rs, git_message.rs, git_keys.rs,
remote.rs, remote_boards.rs (why_not_in, remote_terminals), mod.rs (GitAt)}`, `crates/templum/src/git_block/{mod.rs,
view.rs, changes.rs, commit_form.rs, widgets.rs, diff_view/{rows.rs, paint.rs}}`, `crates/templum/src/menu/{mod.rs,
build.rs}`, `crates/templum/src/sidebar.rs`, `crates/templum-git/src/ops/{mod.rs, op.rs, run.rs}`,
`crates/templum-api/src/client/{mod.rs, http.rs, remote.rs}`; tests `crates/templum/src/workspace/tests/{mod.rs
(git_repo, committed_repo, git_run, settle, git_of, open_git, notification_count, frame), remote.rs (offline,
live_terminals, terminals_of_home, live, apply, show_remote, sent, answer, refusal, said, viewer, remote_of,
pump_home), mirror_end_to_end.rs (mirrored, three_blocks), git_trash.rs (click_row_button, git_button)}`.

**Consumes (as built):** `Op` and `Op::name`, `ops::{look, commit, undo_commit, sync, compare, history, show, discard,
discarded, restore_version, resolve_file, resolve_region, Look, Committed, Undone, Page, CommitShown, Compared,
Measured, Message, Found, Failure, StashRef, SearchArg, WholeFile, CHANGED}`, `trash::{Kept, Restored, measure, fits,
MAX_FILES}`, `registry::{scope, git_scope}`; the client's `Call { tool, args, slow }`, `Answered`, `Refusal`,
`Mirror::call`; the glue's `remote_call(space, call, then, cx)` (`Then`), `answered_later`, `Workspace::{why_not,
why_not_in, warn, said, calls, answer_call, home_folders, remote_terminals, remote_add, new_terminal_rect,
still_shown, space_named, on_remote}`, `RemoteSpace::{why_not, home_of, state, home_workspace}`, `Nay::words`,
`NOT_YET`, `Gates::of`/`why`, `gated_item`, `crate::mirror::computer_of`, `clean_text`, `bus::clean_to`.

## Global Constraints

- **MEMORY LIMITS on this machine:** every cargo command takes `-j 4`; never two cargo commands at once; never
  `cargo clean`; never `git stash`; while working run only the targeted tests each step names; the full suite **once**,
  in Task 7.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-api` and `templum-git` stay UI-free. **No
  new dependency.** No `unsafe`.
- **The local Git popup behaves exactly as before.** On this computer `git_op` is `git_job` with the same closure, and
  every `done` closure keeps its words, notifications, buttons and order. Proof: every test in
  `crates/templum/src/workspace/tests/git_*.rs`, `crates/templum/src/git_block/**` and
  `crates/templum-git/tests/repo.rs` passes **without an edit**, and `--bench-git` stays at `p95_ms` ≤ 16.7 (Task 7).
  The popup never calls `ops::run` (the API's door).
- **Never a local git command or file read on a mirrored workspace's folder.** Home's paths are home's: the same path
  may exist here with other content. A mirrored workspace's `Git` holds no `Repo` (`Found::Home(root)`), its `folder`
  is `PathBuf::new()`, home's path is `Git::home`; `look_up_at`, `poll_git`, `set_git_folder`, `git_folders`,
  `choose_git_folder`, the folder menu's `is_dir`, `open_from_git*`, Run in a terminal and Clone never reach it.
- **Never a relay:** nothing of home's repository reaches the mirror's API, control server, block server, agents or
  stores; Generate runs at home with home's agent (`message`), the mirror's commit agent is never asked or required.
- **Nothing of a remote Git is written:** its folder, tab, summary and description live in `Space::git` of the remote's
  space, which is never saved (persist skips remote spaces); a test proves no file holds them.
- **Polling:** the status (`look`) every `git_block::POLL` (2 s) only while the popup is open on the shown remote, it
  is live, and the rule allows `look`; a poll is skipped while a job runs (as locally). The panel's working diff
  (`diff`) after a status read only when the status differs from the one it was last read with, or `HOME_DIFF_AGAIN`
  = 10 s passed, and always on opening, Refresh, whitespace and whole-file toggles (spec: "the diff panel as the tab
  asks"). Every other read only as the tab asks, as locally.
- **The slow lane** (`Call.slow = true`, 10 minutes): `fetch`, `pull`, `push`, `push_branch`, `publish`, `sync`,
  `message`. Every other operation is quick.
- **The rule:** every act and read on a mirror asks `why_not_in(space, Deed::Git(op.name()))` before it is sent; the
  Full-only acts (`discard`, `discard_hunk`, `restore_version`, `delete_branch`, `stash_drop`, `abort`) ask it at their
  entry, before any question or measure; what batch 2 leaves out asks `Deed::GitLeftOut` (`Nay::NotYet`).
- **Words, verbatim** (`{computer}` = `crate::mirror::computer_of(remote)`):
  - the rule's: "Needs a Full share: share again on {computer}.", "Not connected.", "{computer}'s Templum is older:
    update it to do this from here.", "Not on a mirrored workspace yet" (all as built, `Nay::words`, `NOT_YET`);
  - the client's (as built): "{computer} didn't answer in time: it may still be running there."; home's (2f, passed
    through): "Generate a message on {computer} once first: it asks there whether the diff may go to the agent's
    provider.", "git on {computer} ran for 10 minutes and was stopped.";
  - new: "git on {computer} asks for a login: run it in a terminal there." (a failure whose stderr needs input, on a
    mirror; home's own words for it); "Too large to show from {computer}." (a `FileDiff` with `cut`); "No folder to
    open Git in on {computer}." (the popup opened with no folder known); "No remote: add one on {computer} to publish
    this branch."; "Running on {computer}: it can't be stopped from here." (the spinning Sync and Generate on a
    mirror); "Generate: {computer}'s agent writes the message" (Generate's tooltip on a mirror); "The answer couldn't
    be read: {serde's error}" (a `git` answer of the wrong shape); Generate's words with `{agent}` = `{computer}`
    ("Left out {computer}'s message: a commit was made while it was writing", "Kept your message: it changed while
    {computer} was writing one", the failure's title "{computer}: commit message").
  - `NOT_MIRRORED`'s tail becomes "…grid mode, Move to workspace, Copy and Paste of blocks, Stop, a card's
    attachments, and in the Git popup Clone, Choose another folder, Generate's ⚙, Open in editor and Run in a
    terminal." (the banner tip's test, which looks for "Git popup", passes unedited).
- **Values, verbatim:** `HOME_DIFF_AGAIN` = 10 s; `templum_api::client::DETAILS_MOST` = 65,536 bytes (home's own cap,
  again where it enters; cut at a character's boundary); details cleaned with `clean_text(_, DETAILS_MOST)` (newlines
  and tabs kept, every other control character dropped).
- **Every feature in the API** (CLAUDE.md): 2g adds no tool and no state at home: it is the client of 2f's `git`. Its
  lines are docs/api.md §7 ("What a mirror can't do yet") and the specs listed in Task 7.
- **Tests:** Linux, macOS and Windows, no agent CLI, no network beyond loopback, no fixed sleeps (conditions with a
  deadline; time moved by setting `home_diff`'s instant back); headless remote tests answer `ws.calls` by hand
  (`tests/remote.rs`'s `answer`); `git` runs only in the end-to-end tests, at home; paths compared through
  `dunce::canonicalize`. Every logic change has a test that fails without it; UI-only changes say how they were
  checked (headless render and clicks).
- **The one pre-existing test whose meaning changes:** `a_remote_workspace_refuses_what_it_cant_do_yet`
  (`tests/remote.rs`) calls `toggle_git`/`open_git` on a mirror and asserts `ws.space.git.is_none() && !ws.git_open`;
  Task 2 removes those two calls and that part of the assertion (the popup works there now). No other existing test is
  edited, except mechanical `Refusal` literals (Task 1) and `mirrored` gaining `mirrored_as` (Task 7).
- **Minors** go to `docs/minors.md` (`## Templum to Templum`) with where, what a person sees and a fix. Docs lines
  ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **Home's path exists on this computer too** (a repository at the same path, or the path of this computer's own
   project): nothing local runs or is read; the popup shows only what home answers. → Task 2
   `a_remote_git_never_looks_at_this_computer_s_folder_of_the_same_path`.
2. **An answer that lands after the Git moved on** (the folder changed, the remote was forgotten, the popup closed,
   another workspace shown): it changes nothing, frees what was busy, panics nothing. → Task 2
   `an_answer_for_a_folder_left_since_changes_nothing`.
3. **The remote stops being live with the popup open:** no poll is sent, no error banner every 2 s, an act says
   "Not connected." and sends nothing, `acting` stays 0. → Task 2 `a_popup_on_a_remote_that_isnt_live_polls_nothing`;
   Task 3 `an_act_on_a_remote_that_isnt_live_says_not_connected_and_sends_nothing`.
4. **A Control share reaching a Full operation another way** (the `d` key, a row's hover Discard, a menu built before
   the scope was known, a notification's button): refused before any measure, question or call, with the words.
   → Task 5 `a_control_share_reaches_no_full_operation`.
5. **A home that answers badly** (a `git` of the wrong shape, a refusal whose details hold ESC and a megabyte): the
   popup says it couldn't read it, frees the job, and Details shows cleaned, capped text. → Task 1
   `a_refusal_keeps_home_s_details_cut_to_64_kib`, `a_refusal_is_cleaned_where_it_enters`; Task 2
   `a_malformed_answer_from_home_is_told_and_frees_the_popup`.

---

### Task 1: Home's refusals keep git's stderr on their way to the mirror

**Files:**
- Modify: `crates/templum-api/src/client/mod.rs` (`Refusal` gains `details`, derives `Default`; `Refusal::of` becomes
  `pub`; `pub const DETAILS_MOST: usize = 65_536`; the test at :872 if it names `Failure::Refused`),
  `crates/templum-api/src/client/http.rs` (`templum_error` returns the details; `Failure::Refused(String,
  Option<String>)`; its tests' expected values), `crates/templum-api/src/client/remote.rs` (`called` hands the details
  on; every other `Failure::Refused(_)` pattern becomes `Failure::Refused(..)`), `crates/templum-api/tests/mirror.rs`,
  `crates/templum/src/workspace/remote.rs` (`remote_call`'s "Not connected." literal; `answered_later` cleans through
  `clean_refusal`), `crates/templum/src/workspace/tests/remote.rs` (`refusal()` and other `Refusal { sentence }`
  literals become `Refusal::of(…)`)
- Test: `crates/templum-api/src/client/http.rs`, `crates/templum-api/src/client/remote.rs`,
  `crates/templum/src/workspace/remote.rs` (its `tests` module)

**Interfaces:**
- Produces: `pub struct Refusal { pub sentence: String, pub details: Option<String> }` (`Clone, Debug, PartialEq,
  Default`), `pub fn Refusal::of(sentence: impl Into<String>) -> Refusal` (details None); `pub const DETAILS_MOST`;
  `pub(super) fn clean_refusal(refusal: Refusal) -> Refusal` in the glue (sentence `clean_to(_, 200)` as today,
  details `clean_text(_, DETAILS_MOST)`, a details that is blank after cleaning is None).

- [ ] **Step 1: Write the failing tests**

```rust
// http.rs
#[test]
fn a_refusal_keeps_home_s_details_cut_to_64_kib() {
    // body {"error":{"code":"refused","message":"fatal: x","details":"hint: y\nfatal: x"}}, 422, "/v1/tools/git"
    // → Failure::Refused("fatal: x".into(), Some("hint: y\nfatal: x".into()));
    // a 403 with no details → Failure::Refused("Needs Full.".into(), None) (the existing assertion, its value grown);
    // details of 70,000 "é" → Some(d) with d.len() <= DETAILS_MOST and d.is_char_boundary(d.len()).
}

// client/remote.rs
#[test]
fn a_refused_call_hands_its_details_to_the_ui() {
    // as a_call_in_flight_when_the_attempt_is_dropped_is_refused: life live, answered("git", false), life.call(job),
    // life.called(Lane::Quick, Err(Failure::Refused("fatal: x".into(), Some("d".into()))), ZERO, true)
    // → answer.try_recv() == Err(Refusal { sentence: "fatal: x".into(), details: Some("d".into()) }).
}

// crates/templum/src/workspace/remote.rs tests
#[test]
fn a_refusal_is_cleaned_where_it_enters() {
    // clean_refusal(Refusal { sentence: "a\u{1b}[31mb\nc".into(), details: Some("x\u{1b}y\n\tz".into()) })
    // → sentence without ESC on one line ("a[31mb c" as clean_to makes it), details == Some("xy\n\tz");
    // details Some("\u{1b}") → None.
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -j 4 -p templum-api a_refus` then `cargo test -j 4 -p templum a_refusal_is_cleaned`
Expected: FAIL to compile (no `details`, no `clean_refusal`).

- [ ] **Step 3: Implement** the fields and patterns above: `templum_error` reads `error.details` as a string, cut to
  `DETAILS_MOST` at a character's boundary; `failure` puts it in `Refused` (a `404` stays `Missing(message)`);
  `called` maps `Err(Failure::Refused(sentence, details))` to `Refusal { sentence, details }` and `Missing` to
  `Refusal::of`; every refusal the client makes itself is `Refusal::of`. `answered_later` applies `clean_refusal`
  (replacing its inline `clean_to`).

- [ ] **Step 4: Run the targeted tests, then the client's and the remote's**

Run: `cargo test -j 4 -p templum-api` then `cargo test -j 4 -p templum remote`
Expected: PASS.

---

### Task 2: The seam: a Git job runs here or at home; the mirror's popup shows home's status

**Files:**
- Create: `crates/templum/src/workspace/git_home.rs` (module doc; `HOME_DIFF_AGAIN`; `Runs`; `slow`, `heard`,
  `parsed`, `lies_in`, `failure_words`; `impl Workspace { runs, home_args, home_job, home_git_folder,
  set_home_git_folder, panel_due, home_terminals_in }`; unit tests)
- Modify: `crates/templum/src/workspace/mod.rs` (`mod git_home;`; `GitAt` gains `pub home: Option<String>`),
  `crates/templum/src/workspace/git_ops.rs` (`toggle_git`, `open_git`, `look_up_at`, `init_git`, `refresh_at`,
  `poll_git`, `here`, `git_job` through `job_done`, new `job_done`, `git_op`, `act_op`, `refresh_git_now`;
  `notify_git_error`/`notify_git_failure` words and buttons on a mirror), `crates/templum/src/workspace/git_changes.rs`
  (`reload_git_panel`), `crates/templum/src/workspace/remote.rs` (`Deed::Git(&'static str)`, `Deed::GitLeftOut`,
  `Deed::call`, `command_deed`, `RemoteSpace::home_workspace` → `pub(crate)`, `NOT_MIRRORED`),
  `crates/templum/src/git_block/mod.rs` (`Found::{Home, Refused}`, `Git::{home, home_diff}`, `Git::{at_home,
  has_repo, place}`, `name` on a mirror, `home_name`), `crates/templum/src/git_block/view.rs` (the body's arms, the
  header's place and terminal count, Refresh → `refresh_git_now`), `crates/templum/src/git_block/commit_form.rs`
  (`has_repo`), `crates/templum/src/sidebar.rs` (the Git button through `toolbar::gated(ws, Deed::Git("look"),
  "Git")`), `crates/templum-git/src/ops/{mod.rs, run.rs}` (`pub const CHANGED_MEANWHILE` moves to `mod.rs`, Task 3
  reads it), `crates/templum/src/workspace/tests/remote.rs` (`a_remote_workspace_refuses_what_it_cant_do_yet`, as
  Global Constraints say)
- Test: `crates/templum/src/workspace/tests/git_mirror.rs` (new; `mod git_mirror;` in `tests/mod.rs`), the unit tests in
  `git_home.rs` and `git_block/mod.rs`

**Interfaces:**
- Consumes: Task 1's `Refusal.details`.
- Produces:
  - `Deed::Git(&'static str)` (an operation of `git` by its `Op::name`; `call()` is `Some(("git", json!({"op": op})))`
    when `templum_git::ops::OPS` has it, else None) and `Deed::GitLeftOut` (`call()` None: `Nay::NotYet`);
    `command_deed(Command::NewGit) == Some(Deed::Git("look"))`; `toggle_git` and `open_git` ask `Deed::Git("look")`.
  - `GitAt { space, generation, home: Option<String> }`: `home` is `Some(computer_of(remote))` when `here()` is on a
    mirror, else None.
  - `git_block::Found::Home(String)` (home's repository, its root as home writes it) and `Found::Refused(String)`
    (home couldn't look: its words); `Git.home: Option<String>` ("A mirrored workspace's Git: the folder at home its
    repository is found from, as home writes it. Nothing on this computer reads it."); `Git.home_diff: Option<(Status,
    Instant)>` (the status the panel's diff was last asked with on a mirror, and when);
    `Git::at_home(folder: String, tab: Tab, window, cx) -> Git` (`Git::new(PathBuf::new(), …)`, then `gone = false`,
    `home = Some(folder)`); `Git::has_repo(&self) -> bool` (`Found::Repo(_) | Found::Home(_)`); `Git::place(&self) ->
    String` (the repository's root, else the folder: local `display()`, home's as written); `Git::name` on a mirror is
    `home_name(&place)`; `pub(crate) fn home_name(path: &str) -> String` (the last non-empty part split on `/` and `\`,
    else the whole).
  - `pub(super) enum Runs { Here(Repo), Home(serde_json::Value) }` (the second: the arguments every call carries).
  - `pub(super) fn slow(op: &Op) -> bool`; `pub(super) fn heard(op: &str, answered: Answered) -> Result<Value,
    GitError>` (Ok: the answer's `git`, `Value::Null` when absent; Err: `GitError { command: format!("git {op}"), code:
    None, stderr: details (when not blank) else sentence, missing: false }`); `pub(super) fn parsed<X:
    DeserializeOwned>(heard: Result<Value, GitError>) -> Result<X, GitError>` (a shape error is `GitError { command:
    "git".into(), code: None, stderr: format!("The answer couldn't be read: {e}"), missing: false }`);
    `pub(crate) fn lies_in(path: &str, root: &str) -> bool` (equal, or `root` then `/` or `\`, a trailing separator of
    `root` allowed); `pub(crate) fn failure_words(err: &GitError, computer: Option<&str>) -> String` (None:
    `git_error_message(err)`; Some: the login words when `err.needs_input()`, else `err.first_line()`).
  - `Workspace::runs(&mut self, at: &GitAt) -> Option<Runs>` (`Found::Repo` → `Here(repo.clone())`; `Found::Home(root)`
    → `Home(home_args(&at.space, root)?)`; anything else None); `Workspace::home_args(&self, space: &str, folder: &str)
    -> Option<Value>` (`{"workspace": remote.home_workspace()?, "folder": folder}`).
  - `Workspace::home_job<T: 'static>(&mut self, at: GitAt, args: Value, op: Op, act: bool, home: fn(Result<Value,
    GitError>) -> T, done: impl FnOnce(&mut Workspace, GitAt, T, &mut Window, &mut Context<Workspace>) + 'static,
    window, cx)`: the rule first (`why_not_in(&at.space, Deed::Git(op.name()))`: an act `warn`s the words, a read sets
    `git.error` to them; nothing is sent, `acting` untouched); then, for an act, `acting += 1` and `done` wrapped to
    take it back (as `act_job` does, before any queueing); then a private `home_send` does what `git_job` does: queued
    behind a busy job (the queued closure calls `home_send`, never counting `acting` twice), else `busy = true` and
    `remote_call(&at.space, Call { tool: "git", args: args merged with serde_json::to_value(&op), slow: slow(&op) },
    then)`; `then` maps `heard(op.name(), answered)` (under `cfg(test)` an Err's `first_line()` is pushed to `said`),
    runs `home` and calls `job_done`.
  - `Workspace::job_done<T>(&mut self, at: GitAt, result: T, done: impl FnOnce(...), window, cx)`: `git_job`'s
    completion moved out unchanged (`busy = false`, `done`, the pending refresh or lists, the next queued job,
    `notify`); `git_job` calls it.
  - `Workspace::git_op<T: Send + 'static>(&mut self, at: GitAt, op: Op, here: impl FnOnce(&Repo) -> T + Send +
    'static, home: fn(Result<Value, GitError>) -> T, done: impl FnOnce(&mut Workspace, GitAt, T, &mut Window, &mut
    Context<Workspace>) + 'static, window, cx)`: `runs(&at)` → `Here(repo)`: `git_job(at, move || here(&repo), done)`;
    `Home(args)`: `home_job(at, args, op, false, home, done)`; None: nothing. `act_op` (same signature): `act_job`
    here, `home_job(…, true, …)` at home.
  - `Workspace::home_git_folder(&self) -> Option<String>`: on the remote shown, the focused block's folder when it is a
    remote terminal (`home_of`, then home's state's `BlockView.folder`), else the first remote terminal by `z` with a
    folder, else home's workspace's folder; each through `clean_to(_, LINE_CHARS)`, blank skipped.
  - `Workspace::set_home_git_folder(&mut self, folder: String, window, cx)`: `Git::at_home` with the old one's tab and
    `generation + 1`, focused when the popup is open, then `look_up_git`; never touches `space.recent`.
  - `Workspace::panel_due(&self, at: &GitAt) -> bool`: true on this computer; on a mirror true when `home_diff` is None,
    its status differs from `git.status`, or `HOME_DIFF_AGAIN` passed.
  - `Workspace::home_terminals_in(&self, root: &str) -> Vec<(BlockId, String)>`: `remote_terminals()` whose block's
    folder in home's state `lies_in(root)`.
  - `Workspace::refresh_git_now(&mut self, window, cx)`: `home_diff = None`, then `refresh_git` (the header's ↻ and the
    folder menu's Refresh call it; on this computer it is `refresh_git`).
  - `ops::CHANGED_MEANWHILE: &str` = "The files to commit changed since you last looked: write a message for them."

- [ ] **Step 1: Write the failing tests** (`tests/git_mirror.rs`; helpers there: `fn on_mirror(cx, scope: Scope,
  folder: &str) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir)` = `live_terminals(cx, &["b1"])`, `live(ws,
  scope)`, home's state with workspace `w1`'s `folder = Some(folder)`, applied; `fn call_op(ws, cx) -> (String,
  Value, bool)` the only waiting call's `args["op"]`, its args and `slow` (asserting there is exactly one);
  `fn reply(handle, ws, cx, git: Value)` answers it `Ok(json!({"text": "…", "git": git, "workspace": "w1"}))`;
  `fn look(entries: Vec<Entry>) -> Value` = `serde_json::to_value(ops::Look { status: Status { branch:
  Some("main".into()), oid: Some("a1b2c3d4".into()), entries, ..Default::default() }, operation: None, author:
  Some(("Ada".into(), "ada@example.com".into())) })`; `fn opened(cx, scope) -> (handle, ws, dir)` = `on_mirror` at
  `"/home/ada/app"`, `toggle_git`, then find (root `/home/ada/app`), look (a.txt modified), diff (a.txt's
  `WorkingDiff`) and `default_branch` (`"main"`) answered in turn; `const ROOT: &str = "/home/ada/app"`):

```rust
#[gpui_kit::test]
fn the_popup_on_a_remote_finds_home_s_repository_and_shows_its_status(cx: &mut TestAppContext) {
    // on_mirror(Control, ROOT); toggle_git → ws.git_open; git.home == Some(ROOT); git.found is Looking;
    // call_op == ("find", {"op":"find","workspace":"w1","folder":ROOT}, false).
    // reply {"root":ROOT,"main_tree":ROOT} → found == Found::Home(ROOT); git.repo().is_none().
    // call_op is look with folder ROOT; reply look(a.txt modified) → git.status.entries[0].path == "a.txt",
    // git.author is Some, git.reads == 1; call_op is ("diff", {.., "ignore_whitespace": false, "whole": []}, false);
    // reply a WorkingDiff with a.txt → git.panel.files.len() == 1; call_op is default_branch; reply "main"
    // → git.default_branch == Some("main"); no call waits.
}

#[gpui_kit::test]
fn the_popup_on_a_remote_opens_on_the_focused_terminal_s_folder_then_another_then_the_workspace_s(
    cx: &mut TestAppContext,
) {
    // state: w1 folder "/w"; b1 (z 0) folder "/a"; b2 (z 1) folder "/b". Focus b2's viewer → home_git_folder() == "/b";
    // nothing focused → "/a"; terminals without folders → "/w"; none at all → None, and toggle_git then says
    // "No folder to open Git in on {computer}." (said), with ws.space.git None and no call.
}

#[gpui_kit::test]
fn a_remote_git_never_looks_at_this_computer_s_folder_of_the_same_path(cx: &mut TestAppContext) {
    // repo = committed_repo() on this computer; on_mirror(Control, repo's path as text); toggle_git; settle(handle, cx)
    // twice → found still Looking (a local discover would have made it Repo), git.folder == PathBuf::new(),
    // git.repo().is_none(); poll_git → still one call (find). Reply root = that path → Found::Home(path);
    // settle; git.repo().is_none(); every call in ws.calls is "git".
}

#[gpui_kit::test]
fn the_panel_on_a_mirror_reads_the_diff_when_the_status_changes_or_ten_seconds_pass(cx: &mut TestAppContext) {
    // opened(Control). poll_git → look; reply the same status → no call waits (no diff).
    // move home_diff's Instant back 11 s; poll_git → look; same status → a diff call follows; reply it.
    // poll_git → look with b.txt added → a diff follows; reply it.
    // refresh_git_now → look; same status → a diff follows.
}

#[gpui_kit::test]
fn an_answer_for_a_folder_left_since_changes_nothing(cx: &mut TestAppContext) {
    // opened(Control); poll_git → a look waits; set_home_git_folder("/home/ada/other") → a find is sent too (2 calls).
    // answer the look (calls[0]) with a.txt deleted → the new Git's status is empty and its reads == 0;
    // its find answers as usual. Forgetting the remote while a look waits, then answering it, panics nothing.
}

#[gpui_kit::test]
fn a_popup_on_a_remote_that_isnt_live_polls_nothing(cx: &mut TestAppContext) {
    // opened(Control); remote status = Status::Reconnecting {..}; poll_git twice → no call, git.error None.
    // set_git_tab(History) → no call; git.error == Some("Not connected.").
}

#[gpui_kit::test]
fn a_malformed_answer_from_home_is_told_and_frees_the_popup(cx: &mut TestAppContext) {
    // opened(Control); poll_git → look; reply {"status": 7} → git.error starts with
    // "git: The answer couldn't be read"; git.busy false; poll_git → a look is sent again.
}

#[gpui_kit::test]
fn a_mirror_s_git_is_kept_in_memory_only(cx: &mut TestAppContext) {
    // opened(Control); set the summary to "Fix the mirror's login"; snapshot + save_canvas_now;
    // no "r1.json" under workspaces/, and no file under the data folder (walked, read lossily) holds that summary.
}

// git_home.rs tests
#[test]
fn home_s_paths_lie_in_its_roots_by_either_separator() {
    // lies_in("/a/b", "/a"), lies_in("/a", "/a"), lies_in("/a/b", "/a/"), lies_in("C:\\x\\y", "C:\\x");
    // !lies_in("/ab", "/a"), !lies_in("/a", "/a/b").
}
#[test]
fn only_the_network_and_generate_are_slow() {
    // slow(&Op::Fetch), slow(&Op::Push), slow(&Op::Pull{how: Default::default()}), slow(&Op::Sync{join: None}),
    // slow(&Op::Message{all: false}), slow(&Op::PushBranch{..}), slow(&Op::Publish{..}); !slow(&Op::Look),
    // !slow(&Op::Stage{paths: vec![]}), !slow(&Op::Discard{..}).
}
#[test]
fn home_s_refusal_is_a_git_error_with_its_details() {
    // heard("push", Err(Refusal { sentence: "fatal: x".into(), details: Some("hint\nfatal: x".into()) }))
    // == Err(GitError { command: "git push", code: None, stderr: "hint\nfatal: x", missing: false });
    // details None → stderr == "fatal: x"; Ok({"git": {"a": 1}}) → Ok({"a": 1}); Ok({}) → Ok(Null).
}
#[test]
fn a_failure_on_a_mirror_is_said_in_home_s_words() {
    // a GitError whose stderr is "fatal: could not read Username for 'https://x': terminal prompts disabled":
    // failure_words(&e, Some("winx-desktop")) == "git on winx-desktop asks for a login: run it in a terminal there.";
    // failure_words(&e, None) == git_error_message(&e); a plain one → first_line() on a mirror.
}
// git_block/mod.rs tests
#[test]
fn home_s_folder_is_named_by_its_last_part() {
    // home_name("/home/ada/app") == "app"; home_name("C:\\Users\\ada\\app\\") == "app"; home_name("/") == "/".
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -j 4 -p templum git_mirror` then `cargo test -j 4 -p templum git_home`
Expected: FAIL to compile.

- [ ] **Step 3: Implement the seam** as the Interfaces say: `job_done` moved out of `git_job` unchanged; `git_op`,
  `act_op`, `runs`, `home_args`, `home_job` and the free functions; `GitAt.home` filled by `here()` only.

- [ ] **Step 4: The status on a mirror.**
  - `toggle_git`: on a mirror with no Git, `home_git_folder()` (None: `warn` the no-folder words and stop), then
    `set_home_git_folder`; a local workspace is as today.
  - `open_git`: on a mirror `home_diff = None` before `refresh_git`.
  - `look_up_at`: on a Git with `home`, `home_job(at, home_args(&at.space, folder)?, Op::Find, false,
    parsed::<ops::Found>, done)` where `done` sets `found` to `Home(root)`, `NotARepo` (root null) or
    `Refused(e.first_line())`, then `after_action`; the local branch keeps its `is_dir` check and `Repo::discover`.
  - `init_git`: on a mirror `home_job(…, Op::Init, true, parsed::<ops::Found>, …)` (an Err notifies through
    `notify_git_error`).
  - `refresh_at`: `git_op(at, Op::Look, |repo| ops::look(repo), parsed::<ops::Look>, done)`; `done` calls
    `reload_git_panel` only when `panel_due(&at)` (always true here).
  - `reload_git_panel`: `git_op(at, Op::Diff { ignore_whitespace, whole }, …, parsed::<WorkingDiff>, done)`
    (`whole` from `panel.whole` as `WholeFile { staged, path }`); on a mirror it sets `home_diff = Some((status,
    now))` when it sends.
  - `poll_git`: on a Git with `home`, no `gone` stat; skipped when `why_not(Deed::Git("look"))` is Some.
  - `notify_git_error` uses `failure_words(err, at.home.as_deref())`; `notify_git_failure` leaves out **Run in a
    terminal** when `at.home` is Some (Details stays).
  - The view: `Found::Repo(_) | Found::Home(_)` → `repo_body`; `Found::Refused(why)` → centered `why` and **Try
    again** (`look_up_git`); the header's tooltip and the not-a-repository line use `git.place()`; `repo` in `popup`
    is `git.has_repo() && !git.gone`; the header's terminal count on a mirror is `home_terminals_in(root).len()`;
    `commit_form`'s two `repo().is_some()`/`is_none()` read `has_repo()`; the ↻ calls `refresh_git_now`.
  - `NOT_MIRRORED` as Global Constraints say; the sidebar's Git button gated by `Deed::Git("look")` (dimmed with the
    words when older or not connected).
  - Edit `a_remote_workspace_refuses_what_it_cant_do_yet` as Global Constraints say.

- [ ] **Step 5: Run the new tests, then every Git test unedited**

Run: `cargo test -j 4 -p templum git_mirror`, then `cargo test -j 4 -p templum git_`, then `cargo test -j 4 -p templum
remote`
Expected: PASS; `git diff --stat -- crates/templum/src/workspace/tests/git_*.rs crates/templum/src/git_block` shows no
test file of the popup changed.

---

### Task 3: Acts at home: stage, commit, sync, branches and the rest

**Files:**
- Modify: `crates/templum/src/workspace/git_changes.rs` (`git_action` takes the `Op`; `git_commit`, `set_amend`,
  `undo_commit`, `apply_hunk`, `ignore_path`, `stage`, `unstage`, `abort_operation`, `continue_operation`, Set…),
  `crates/templum/src/workspace/git_branches.rs` (`git_sync` takes the `Op`; `fetch`, `pull`, `push_now`,
  `rename_branch`, `merge_branch`, `delete_now`, `cancel_git`), `crates/templum/src/workspace/git_switch.rs`
  (`create_branch`, `switch_now`, `push_branch`), `crates/templum/src/workspace/git_sync.rs` (`run_sync`,
  `publish_to`, the no-remote words), `crates/templum/src/workspace/git_history.rs` (`stash_changes`, `stash_apply`,
  `stash_drop`'s act, `checkout_commit`, `tag_commit`, `revert_commit`), `crates/templum/src/workspace/git_conflicts.rs`
  (`resolve_region`, `resolve_file`), `crates/templum/src/git_block/view.rs` (the spinning Sync on a mirror)
- Test: `crates/templum/src/workspace/tests/git_mirror.rs`

**Interfaces:**
- Consumes: Task 2's `act_op`, `git_op`, `parsed`, `heard`'s errors, `ops::CHANGED_MEANWHILE`.
- Produces:
  - `Workspace::git_action(&mut self, op: Op, job: impl FnOnce(&Repo) -> Result<(), GitError> + Send + 'static,
    window, cx)`: `act_op(self.here(), op, job, parsed::<()>, <today's done>)`. Its callers pass, in order:
    `RenameBranch { branch: from, name: to }`, `Merge { branch }`, `Revert { hash }` (both), `SetAuthor { name, email
    }`, `Stage { paths }`, `Unstage { paths }`, `Abort`, `Continue`, `NewBranch { name, hash: from }`, `Stash { message,
    untracked }`, `StashApply`/`StashPop { stash: StashRef { index, hash } }`, `StashDrop { stash }`,
    `Checkout { hash }`,
    `Tag { name, hash }`.
  - `Workspace::git_sync(&mut self, which: Sync, op: Op, job: impl FnOnce(&Repo) -> Result<(), GitError> + Send +
    'static, on_err, window, cx)`: the cancel flag wraps the local repo inside the closure (`repo.clone().with_cancel`);
    callers: `Fetch`, `Pull { how }`, `Push`, `Publish { remote, branch }`, `PushBranch { branch }`. `git_sync` and
    `run_sync` ask `why_not(Deed::Git(op.name()))` before they set `sync` and `cancel` (warned, nothing set): a refused
    push must not leave the Sync button spinning.
  - The jobs with their own results, each `act_op`/`git_op` with these `home` mappers (free `fn`s in the file that
    uses them):
    - `git_commit`: `Op::Commit { summary: typed, description, amend, all }`; home's `Committed` → `Ok(Some(c))`, a
      refusal whose stderr is `CHANGED_MEANWHILE` → `Ok(None)` (the existing warning and focus follow), else `Err`.
    - `undo_commit`: `UndoCommit { hash: short, replaced }` → `parsed::<Undone>`; `set_amend`: `LastMessage` →
      `parsed::<String>`; `run_sync`: `Sync { join }` → `parsed::<Synced>` (on the slow lane by `slow`).
    - `apply_hunk`: `StageHunk { patch }` / `UnstageHunk { patch }` → `parsed::<()>`; `ignore_path`: `Ignore { path }`
      → `io::Result<()>` (an Err is `io::Error::other(e.first_line())`); `switch_now`: `Switch { branch: to, remote,
      leave }` → `parsed::<Back>`; `delete_now`: `DeleteBranch { branch, force }` → `parsed::<()>`.
    - `resolve_region`: `ResolveRegion { path, region, take, shown }`; `resolve_file`: `Resolve { path, side }`; both
      → `Result<(), Failure>` (`Err(e)` → `Failure::Git(e)`).
  - On a mirror (`at.home` / the shown Git's `home` is Some): `cancel_git` does nothing and the spinning Sync has no
    click, its tooltip "Running on {computer}: it can't be stopped from here."; `publish` with no remote says "No
    remote: add one on {computer} to publish this branch." with no **Run in a terminal**.

- [ ] **Step 1: Write the failing tests** (Task 2's helpers):

```rust
#[gpui_kit::test]
fn staging_and_committing_on_a_mirror_run_at_home_one_at_a_time(cx: &mut TestAppContext) {
    // opened(Control). stage(["a.txt"]) → call_op == ("stage", {"op":"stage","paths":["a.txt"],"workspace":"w1",
    // "folder":ROOT}, false); git.acting == 1. unstage(["a.txt"]) now → still one waiting call.
    // reply null → acting back to 0 once the queue drains; the calls that follow, answered in turn, are look, diff
    // (the status changed), default_branch and then unstage: each act reaches home, in order, one at a time.
    // summary "Fix login" (a staged a.txt in the look) → git_commit(false) → ("commit", {"op":"commit",
    // "summary":"Fix login","description":"","amend":false,"all":false}, false); reply {"short":"a1b2c3d",
    // "replaced":null} → the summary field is empty and notification_count went up by one.
}

#[gpui_kit::test]
fn a_commit_home_says_changed_meanwhile_keeps_the_message(cx: &mut TestAppContext) {
    // as above to the commit; answer refusal(ops::CHANGED_MEANWHILE) → the summary is still "Fix login",
    // a notification was shown, and look follows.
}

#[gpui_kit::test]
fn git_s_network_operations_go_on_the_slow_lane(cx: &mut TestAppContext) {
    // opened(Control); fetch, pull(Pull::FastForward), push, sync_git, each answered null before the next (and
    // the look/diff/default_branch that follow answered): their calls' slow == true and ops "fetch", "pull"
    // (how "fast_forward"), "push", "sync"; stage's slow == false.
}

#[gpui_kit::test]
fn a_push_cut_on_the_way_says_it_may_still_be_running_and_reads_again(cx: &mut TestAppContext) {
    // opened(Control); push → answer refusal("127.0.0.1:9 didn't answer in time: it may still be running there.")
    // → said's last is that sentence; git.sync is None; the next call is look.
}

#[gpui_kit::test]
fn an_act_on_a_remote_that_isnt_live_says_not_connected_and_sends_nothing(cx: &mut TestAppContext) {
    // opened(Control); status Reconnecting; stage(["a.txt"]), create_branch("x", None), push → no call;
    // said ends with "Not connected." three times; git.acting == 0; git.busy false; git.sync is None.
}

#[gpui_kit::test]
fn the_spinning_sync_on_a_mirror_cannot_stop_home_s_push(cx: &mut TestAppContext) {
    // opened(Control); push waits; git.sync == Some(Sync::Push); cancel_git → git.cancel's flag (when Some) is false;
    // the answer null then ends it as usual (git.sync None).
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -j 4 -p templum git_mirror`
Expected: FAIL (no call is sent: the jobs still need a `Repo`).

- [ ] **Step 3: Implement** the signatures and mappers above; every caller of `git_action` and `git_sync` passes its
  `Op`; each of today's `act_job`/`git_job` sites in these files becomes `act_op`/`git_op` with its closure unchanged
  apart from taking `repo` as the argument.

- [ ] **Step 4: Run the new tests, then every Git test unedited**

Run: `cargo test -j 4 -p templum git_mirror`, then `cargo test -j 4 -p templum git_`
Expected: PASS.

---

### Task 4: History, Branches, Stashes and Compare read at home

**Files:**
- Modify: `crates/templum/src/workspace/git_history.rs` (`load_stashes`, `show_stash`, `load_history`, `show_commit`,
  `load_blame`), `crates/templum/src/workspace/git_branches.rs` (`load_branches`, `ask_delete_anyway`'s count),
  `crates/templum/src/workspace/git_switch.rs` (`open_branch_dialog`'s branches), `crates/templum/src/workspace/
  git_compare.rs` (`load_compare`), `crates/templum/src/workspace/git_sync.rs` (`load_default`, `publish`'s remotes),
  `crates/templum/src/git_block/diff_view/{rows.rs, paint.rs}` (`Note::Cut`)
- Test: `crates/templum/src/workspace/tests/git_mirror.rs`, `crates/templum/src/git_block/diff_view/rows.rs`

**Interfaces:**
- Consumes: Task 2's `git_op`, `parsed`.
- Produces: each read as `git_op` with: `Stashes` → `parsed::<Vec<Stash>>`; `StashDiff { stash: StashRef,
  ignore_whitespace }` → `parsed::<Vec<FileDiff>>`; `History { all, search: q.search.map(|(by, text)| SearchArg { by,
  text }), path: file, skip }` → `parsed::<ops::Page>`; `Show { hash, ignore_whitespace }` →
  `parsed::<ops::CommitShown>` (home reads the commit itself: `loaded` is local only); `Blame { path, hash: rev }` →
  `parsed::<Vec<BlameLine>>`; `Branches` → `parsed::<Vec<Branch>>`; `Unmerged { branch }` → `parsed::<u32>`; `Compare
  { base, branch, hash: narrowed, ignore_whitespace }` → `parsed::<ops::Compared>`; `DefaultBranch` → `Option<String>`
  (`|r| parsed::<Option<String>>(r).ok().flatten()`, as locally it never fails); `Remotes` → `parsed::<Vec<String>>`.
  `rows::Note::Cut`: a file whose `diff.cut` is true draws one note row, before the `too_large` check, and no line;
  `paint` says "Too large to show from {computer}." (`computer_of` of the shown remote; a local diff never has `cut`).

- [ ] **Step 1: Write the failing tests**

```rust
#[gpui_kit::test]
fn history_branches_and_stashes_on_a_mirror_read_home_s_lists(cx: &mut TestAppContext) {
    // opened(Control). set_git_tab(History) → default_branch, then ("history", {"op":"history","all":false,
    // "search":null,"path":null,"skip":0,..}); reply a Page of 2 commits (unpushed {}) → git.commits.len() == 2.
    // show_commit(first hash) → ("show", {"hash": h, "ignore_whitespace": false, ..}); reply a CommitShown with
    // one FileDiff → git.shown.is_some(), git.history_panel.files.len() == 1.
    // set_git_tab(Branches) → default_branch, branches; reply [main] → git.branches.len() == 1.
    // set_git_tab(Stashes) → default_branch, stashes; reply [] → git.stashes.is_empty(); no call waits.
}

#[gpui_kit::test]
fn a_default_branch_home_refuses_is_none(cx: &mut TestAppContext) {
    // opened(Control), default "main"; after_action's default_branch answered with refusal("x") → git.default_branch
    // is None and no notification was shown (notification_count unchanged).
}

// rows.rs
#[test]
fn a_file_cut_for_the_network_draws_one_note_and_no_lines() {
    // a FileDiff with cut: true and no hunks → rows hold Row::Note { note: Note::Cut, .. } once and no Row::Line.
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -j 4 -p templum git_mirror` then `cargo test -j 4 -p templum a_file_cut`
Expected: FAIL.

- [ ] **Step 3: Implement** the reads and the note above.

- [ ] **Step 4: Run the new tests, then every Git test unedited**

Run: `cargo test -j 4 -p templum git_mirror`, `cargo test -j 4 -p templum a_file_cut`, `cargo test -j 4 -p templum
git_`
Expected: PASS.

---

### Task 5: Discards, the trash, Restore version, and what needs a Full share

**Files:**
- Modify: `crates/templum/src/workspace/git_discard.rs` (`discard`, `discard_hunk`, `ask_discard`, `discard_now`,
  `restore_discarded`, `show_discarded`), `crates/templum/src/workspace/git_history.rs` (`restore_version`,
  `restore_now`, `stash_drop`), `crates/templum/src/workspace/git_branches.rs` (`delete_branch`),
  `crates/templum/src/workspace/git_changes.rs` (`abort_operation`), `crates/templum/src/workspace/remote.rs`
  (`GIT_GATES`), `crates/templum/src/git_block/{changes.rs, view.rs}`, `crates/templum/src/git_block/diff_view/
  paint.rs`, `crates/templum/src/menu/{mod.rs, build.rs}` (the Git items' `gates`; `GitMore`'s `trash` on a mirror)
- Test: `crates/templum/src/workspace/tests/git_mirror.rs`

**Interfaces:**
- Consumes: Task 2's `git_op`, `act_op`, `parsed`, `Deed::Git`, `Deed::GitLeftOut`.
- Produces:
  - Guards at the entry of `discard`, `discard_hunk`, `restore_version`, `delete_branch`, `stash_drop` and
    `abort_operation`: `if let Some(why) = self.why_not(Deed::Git("<its op>")) { self.warn(why, window, cx); return; }`
    (`discard`, `discard_hunk`, `restore_version`, `delete_branch`, `stash_drop`, `abort`), before any question,
    measure or call; on this computer `why_not` is None.
  - `ask_discard`: `git_op(at, Op::Measure { paths }, …, …, done)` with the job's result an `ops::Measured`: here
    `Measured { files, bytes, keep: trash_here && trash::fits(files, bytes) }` (`trash_here = self.trash.is_some()`
    read before the job), at home `parsed::<Measured>` with an Err as `Measured { files: 0, bytes: 0, keep: false }`;
    `done` words: `keep` → today's restore sentence; else when the trash exists (`self.trash.is_some()` here; always
    on a mirror) and `!fits` → today's "Too much to keep a copy…"; else "This can't be undone." The question then
    sends `discard_now`.
  - `discard_now`: `act_op(at, Op::Discard { paths }` or `Op::DiscardHunk { path, patch }`, …) → `Result<Option<Kept>,
    Failure>` (`parsed::<Option<Kept>>`, `Err(e)` → `Failure::Git(e)`); on a mirror home keeps the copy when it fits
    (`keep` is not sent).
  - `restore_discarded` and `show_discarded` need no `Repo` here (they read this computer's trash), so they branch on
    `runs(&at)`: `Home(args)` → `home_job(at, args, Op::Restore { entry, force }, true, …)` → `io::Result<Restored>`
    (Err → `io::Error::other(e.first_line())`), and `home_job(…, Op::Trash, false, …)` → `Vec<Kept>` (an Err → empty)
    then today's dialog; anything else exactly as today (the mirror's own `self.trash` is never asked on a mirror).
  - `restore_now`: `act_op(Op::RestoreVersion { hash, then, path: now })` → `Result<Option<Kept>, Failure>`.
  - `pub(crate) const GIT_GATES: [Deed; 7] = [Git("discard"), Git("discard_hunk"), Git("restore_version"),
    Git("delete_branch"), Git("stash_drop"), Git("abort"), GitLeftOut]`; `Items::{GitRow, GitCommit, GitBranch,
    GitStash, GitMore}` gain `gates: Gates` (`Gates::of(self, &GIT_GATES)`); their Discard changes…, Restore this
    version…, Delete…, Drop… and Clone a repository… items become `gated_item` with `gates.why(..)`; `GitMore.trash`
    is true on a mirror (home's trash).
  - Buttons: the row's hover Discard and the header's Discard all… (`changes.rs`), the file header's and the hunk's
    Discard (`paint.rs`) and the banner's Abort (`view.rs`) read `ws.why_not(Deed::Git(..))` once per list or panel;
    gated, the button is built with the words as its only tooltip (GPUI takes one tooltip per element), `opacity(0.5)`
    and no click.

- [ ] **Step 1: Write the failing tests**

```rust
#[gpui_kit::test]
fn discard_on_a_mirror_asks_with_home_s_measure_then_discards_there(cx: &mut TestAppContext) {
    // opened(Full). discard(["a.txt"]) → ("measure", {"op":"measure","paths":["a.txt"],..}, false);
    // reply {"files":1,"bytes":4,"keep":true} → window.has_active_dialog; within("dialog").click("ok") →
    // ("discard", {"op":"discard","paths":["a.txt"],..}, false); reply {"id":"17-3","root":ROOT,"time":0,
    // "what":"Discarded a.txt","files":[["a.txt",true]]} → click "git-undo" (after the look/diff/default answered) →
    // ("restore", {"op":"restore","entry":"17-3","force":false,..}, false).
}

#[gpui_kit::test]
fn recently_discarded_on_a_mirror_lists_home_s_trash(cx: &mut TestAppContext) {
    // opened(Full); this workspace has no data folder (ws.trash None); show_discarded → ("trash", ..);
    // reply [one Kept] → a dialog is open.
}

#[gpui_kit::test]
fn a_control_share_reaches_no_full_operation(cx: &mut TestAppContext) {
    // opened(Control) with a stash loaded and a look whose operation is Merge.
    // discard(["a.txt"]), discard_hunk("a.txt", patch), restore_version(hash, "a.txt", ..), delete_branch("x", ..),
    // stash_drop(0), abort_operation → no call sent and no dialog; said's last six each start with
    // "Needs a Full share: share again on ".
    // render; click git_button("discard", "files", 0) (click_row_button) → no call;
    // the popup focused, key "d" → no call.
    // why_not(Deed::Git("stage")) is None and why_not(Deed::Git("discard")) is Some (Control acts work).
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -j 4 -p templum git_mirror`
Expected: FAIL.

- [ ] **Step 3: Implement** the guards, jobs, gates and dimmed buttons above.

- [ ] **Step 4: Run the new tests, then the trash's and every Git test unedited**

Run: `cargo test -j 4 -p templum git_mirror`, then `cargo test -j 4 -p templum git_`
Expected: PASS (`git_trash.rs` unedited proves the local discard's words and Undo unchanged).

---

### Task 6: What only a mirror's popup does: its folders, Generate at home, the controls left out

**Files:**
- Modify: `crates/templum/src/workspace/git_home.rs` (`home_git_folders`, `open_terminal_at_home`),
  `crates/templum/src/workspace/remote.rs` (`remote_add_in`; `remote_add` calls it with None),
  `crates/templum/src/workspace/git_message.rs` (`generate_message` on a mirror, `generate_at_home`,
  `generated_parts`, `notify_agent_failure`), `crates/templum/src/workspace/git_sync.rs` (`ask_clone` guard),
  `crates/templum/src/workspace/git_ops.rs` (`choose_git_folder` guard), `crates/templum/src/workspace/git_changes.rs`
  (`open_from_git_at` guard), `crates/templum/src/menu/{mod.rs, build.rs}` (`Items::HomeGitFolders`; `GitTerminals` on
  a mirror; `GitRow`/`GitLine` gain `remote: bool` and leave Open and Open in editor at this line out on a mirror;
  `send_to_targets`), `crates/templum/src/git_block/{commit_form.rs, changes.rs}`,
  `crates/templum/src/git_block/diff_view/paint.rs` (Open buttons left out on a mirror)
- Test: `crates/templum/src/workspace/tests/git_mirror.rs`

**Interfaces:**
- Consumes: Tasks 2–5; 2b's `remote_add`, `new_terminal_rect`, `awaiting`.
- Produces:
  - `Workspace::home_git_folders(&self) -> Vec<String>`: `home_folders()`' paths less the shown Git's `home` and those
    that `lies_in` its root.
  - `Items::HomeGitFolders { folders: Vec<String>, root: Option<String>, gates: Gates }` (built for
    `Target::GitFolders` when `on_remote()`, `gates` of `[GitLeftOut, AddTerminal]`): each folder (label `home_name`,
    act `set_home_git_folder`), a separator when any, **Choose another folder…** gated `GitLeftOut`, **Refresh**
    (`refresh_git_now`), and with a root **Open a terminal here** gated `AddTerminal` (act `open_terminal_at_home`).
    `Target::GitTerminals` on a mirror lists `home_terminals_in(root)`.
  - `Workspace::remote_add_in(&mut self, kind: Kind, at: Point, folder: Option<String>, cx)`: `remote_add` with
    `args["folder"]` when given. `Workspace::open_terminal_at_home(&mut self, root: String, window, cx)`:
    `remote_add_in(Kind::Terminal, new_terminal_rect(window, cx).origin(), Some(root), cx)`, then `close_git`.
  - Generate on a mirror: shown when `has_repo()` and there is something to describe, whatever this computer's agent
    (tooltip "Generate: {computer}'s agent writes the message"); a click while it runs does nothing (tooltip "Running on
    {computer}: it can't be stopped from here."); the ⚙ dimmed with `NOT_YET` and no menu. `Workspace::generate_at_home
    (&mut self, window, cx)`: the rule (`Deed::Git("message")`, warned), `before` the fields, `generating = Some(flag)`,
    then straight `remote_call` (not the job queue: as locally it runs beside the Git's commands) of `Op::Message {
    all: commit_all() }` with `slow: true`; its answer: `generating = None`; Ok → `parsed::<ops::Message>` →
    `generated_parts(at, computer, before, Ok((summary, description)), committed)` (a shape error is told as a
    refusal is); Err(refusal) → `notify_agent_failure(computer, refusal.sentence, refusal.details or the sentence)`.
  - `fn generated_parts(&mut self, at: GitAt, agent: &str, before: (SharedString, SharedString), parts:
    Result<(String, String), message::Failure>, committed: bool, window, cx)`: `generated`'s body after the split (the
    local `generated` splits and calls it); `fn notify_agent_failure(&self, agent: &str, text: String, details: String,
    window, cx)`: `notify_generate_failure`'s body with the words given (the local one calls it with
    `failure.to_string()` and `failure.details()`). A refusal of `message` on a mirror is
    `notify_agent_failure(computer, refusal sentence, details or sentence)` (the `Answered` is read directly, not
    through `heard`, so home's sentence is kept even with details).
  - Guards: `ask_clone`, `choose_git_folder` and `open_from_git_at` start with `why_not(Deed::GitLeftOut)` (warned):
    the `Enter` key over a file and a stale menu say "Not on a mirrored workspace yet" and open nothing.
  - `Workspace::send_to_targets(&self) -> Vec<(BlockId, String)>` (`menu/mod.rs`): what Send to ▸ lists on the
    workspace shown, `remote_terminals()` on a mirror, else `terminal_list(None)`; `send_to_menu` builds its `to` from
    it.

- [ ] **Step 1: Write the failing tests**

```rust
#[gpui_kit::test]
fn the_folder_menu_on_a_mirror_lists_home_s_folders_and_opens_a_terminal_there(cx: &mut TestAppContext) {
    // state: w1 folder ROOT; b1 folder ROOT + "/src"; b2 folder "/home/ada/lib"; opened on ROOT.
    // home_git_folders() == ["/home/ada/lib"]; home_terminals_in(ROOT) == [(b1's viewer, "b1")].
    // open_menu(Target::GitFolders, ..) → ws.menu.is_some(), said empty.
    // open_terminal_at_home(ROOT) → an add_block call {"workspace":"w1","kind":"terminal","folder":ROOT,"x":..,"y":..}
    // and !ws.git_open.
}

#[gpui_kit::test]
fn generate_on_a_mirror_asks_home_s_agent_on_the_slow_lane(cx: &mut TestAppContext) {
    // opened(Control) with a.txt staged; ws.commit_agent is None.
    // generate_message → ("message", {"op":"message","all":false,..}, true); git.generating is Some;
    // generate_message again → still one call. reply {"summary":"Fix login","description":"Because.",
    // "committed":false} → summary "Fix login", description "Because.", generating None.
    // generate_message, answered refusal("Generate a message on 127.0.0.1:9 once first: it asks there whether the
    // diff may go to the agent's provider.") → notification_count + 1, the fields unchanged.
}

#[gpui_kit::test]
fn the_popup_on_a_mirror_offers_none_of_what_stays_at_home(cx: &mut TestAppContext) {
    // opened(Full); render: the popup draws (find "git-popup"), "git-generate-menu" is there and a click opens no
    // menu (ws.menu None). ask_clone, choose_git_folder, open_from_git_at("a.txt", None) → said's last three are
    // NOT_YET, no dialog, no call, no block added. send_to_targets() == remote_terminals().
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -j 4 -p templum git_mirror`
Expected: FAIL.

- [ ] **Step 3: Implement** the above.

- [ ] **Step 4: Run the new tests, then every Git test unedited and the menus'**

Run: `cargo test -j 4 -p templum git_mirror`, `cargo test -j 4 -p templum git_`, `cargo test -j 4 -p templum menus`
Expected: PASS.

---

### Task 7: The end-to-end cases, the docs, the full check and the bench

**Files:**
- Modify: `crates/templum/src/workspace/tests/mirror_end_to_end.rs` (`mirrored` becomes `mirrored_as(…, scope)`
  with `mirrored` calling it with `Scope::Full`; a "Batch 2g" section)
- Docs: `docs/api.md` (§7 "What a mirror can't do yet": "the Git popup" leaves the list; a sentence: "The Git popup
  works on a mirrored workspace: everything it reads and does is home's `git` (reads with any share, acts with Control,
  discards, Restore version, deleting a branch, dropping a stash and aborting with Full, dimmed on a Control share);
  Fetch, Pull, Push, Sync, Publish and Generate wait up to 10 minutes, and one a tunnel cuts says it may still be
  running on home; Generate uses home's agent; Clone, Choose another folder, Generate's ⚙, Open in editor and Run in a
  terminal stay on home."), `docs/specs/git.md` (a section "On a mirrored workspace": the same popup through home's
  `git`, the folder, the reads and the diff's pace, the slow operations, Generate at home, the trash, Send to, the
  dimming, memory only), `docs/specs/context-menus.md` (the folder ▾ on a mirror: home's folders, Choose another
  folder… dimmed, Refresh, Open a terminal here at home), `docs/specs/templum-to-templum.md` (Status: parts 2a–2g
  built; "As built ▸ Batch 2g": what the builder chose or changed, and this plan's "Open for the orchestrator" as
  settled), `docs/ARCHITECTURE.md` (`workspace/git_home.rs`: a mirrored workspace's Git through home's `git`),
  `docs/minors.md` (below)
- Test: `crates/templum/src/workspace/tests/mirror_end_to_end.rs`

**Interfaces:**
- Consumes: everything above; `mirrored`, `three_blocks`, `committed_repo`, `git_run`, `pump_home`, `remote_of`.
- Produces: `fn git_mirrored(handle, ws, cx, dir, scope) -> (tempfile::TempDir, String)`: `three_blocks`, then
  `committed_repo()` with a.txt changed to "two\n", the first workspace's `folder` set to it, `mirrored_as(…, scope)`,
  `set_home_git_folder(root)` + `open_git` on `r1` (root = `dunce::canonicalize` of the repository, as text), pumped
  until the mirror's status lists a.txt; returns the repository and root.

- [ ] **Step 1: Write the end-to-end tests** (in one process, over loopback, git running at home):

```rust
// Batch 2g: the Git popup on the mirror.

#[gpui_kit::test]
fn the_mirror_s_git_popup_shows_home_s_change_and_commits_it_at_home(cx: &mut TestAppContext) {
    // git_mirrored(Full): r1's Git is Found::Home(r) with dunce::canonicalize(r) == dunce::canonicalize(repo)
    // (git writes C:/… on Windows), repo() None, status lists a.txt modified.
    // stage(["a.txt"]) → pump until git_run(repo, ["diff", "--cached", "--name-only"]) == "a.txt\n" and r1's
    // status shows it staged. Summary "From the mirror", git_commit(false) → pump until
    // git_run(repo, ["log", "-1", "--format=%s"]).trim() == "From the mirror" and r1's summary field is empty.
}

#[gpui_kit::test]
fn a_branch_made_on_the_mirror_is_home_s(cx: &mut TestAppContext) {
    // git_mirrored(Full); create_branch("from-mirror", None) → pump until
    // git_run(repo, ["branch", "--show-current"]).trim() == "from-mirror" and r1's status.branch == "from-mirror".
}

#[gpui_kit::test]
fn on_a_control_share_discard_is_dimmed_and_send_to_lists_no_local_terminal(cx: &mut TestAppContext) {
    // git_mirrored(Control): why_not(Deed::Git("discard")) starts with "Needs a Full share: share again on ";
    // discard(["a.txt"]) → said's last is it, no dialog; after a few pump rounds home's a.txt is still "two\n".
    // stage still works (pump until home's index has a.txt).
    // send_to_targets() == remote_terminals(), not empty, and each id is a remote viewer of r1
    // (ws.space.terminal(id).unwrap().remote.is_some()).
}
```

- [ ] **Step 2: Run them**

Run: `cargo test -j 4 -p templum mirror_end_to_end`
Expected: PASS (a failure here is a bug in Tasks 1–6: fix it there, with a headless test in `git_mirror.rs` when the
headless ones missed it).

- [ ] **Step 3: Write the docs** above, and in `docs/minors.md` (`## Templum to Templum`):
  - **The folder menu on a mirror lists folders, not repositories** (`git_home.rs`, `home_git_folders`): two terminals
    in different folders of one repository outside the shown one show as two rows. Fix: a `find` per folder when the
    menu opens, deduplicated by root.
  - **A discard too large to keep says "Too much to keep a copy" even when home keeps no trash**
    (`git_discard.rs`, `ask_discard`): `measure` answers no "has a trash". Fix: `Measured.trash` at home.
  - **A Sync, Push or Generate running at home can't be stopped from the mirror** (`git_branches.rs`, `cancel_git`):
    the button says so and home's 10-minute deadline ends it. Fix: a `cancel` operation that kills the call's process
    group at home.
  - **The panel's diff on a mirror can be 10 s old** (`git_home.rs`, `panel_due`): a file edited again at home without
    its status changing shows its new lines within `HOME_DIFF_AGAIN`. Fix: a cheap `look` field that changes with the
    working files' stamps.
  - **The mirror's Git popup was checked headlessly only** (`git_block/view.rs`, `changes.rs`, `commit_form.rs`,
    `diff_view/paint.rs`, `menu/build.rs`): the dimmed Discard, Abort, ⚙ and menu items, and the spinning Sync's
    tooltip were not looked at on a display.

- [ ] **Step 4: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env run
  `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.

- [ ] **Step 5: The bench:** `cargo run -j 4 --release -p templum -- --bench-git` → `p95_ms` ≤ 16.7; report its JSON
  line.

- [ ] **Step 6: Leave uncommitted;** report the files changed, the checks' and the bench's results, what went to
  `docs/minors.md`, and confirm no existing Git test was edited (`git diff --stat --
  crates/templum/src/workspace/tests/git_*.rs crates/templum/src/git_block crates/templum-git/tests` shows only the new
  `Note::Cut` test in `rows.rs`, and `tests/remote.rs` only the planned edit and the `Refusal` literals).

---

## Interfaces for later parts

What a later card (Clone on a mirror, Choose another folder on a mirror, home's Settings ▸ Git from a mirror, Open in
editor through the API) or a later part builds on:

- **The seam:** `Workspace::git_op` / `act_op(at, op, here, home, done)` run any popup job here or at home;
  `home_job(at, args, op, act, home, done)` for a job with no local twin; `runs(&at)`, `home_args(space, folder)`.
  A new operation of home's `git` needs only its `Op` variant, a `home` mapper (`parsed::<X>` mostly) and its call site.
- **The rule:** `Deed::Git(op_name)` (scope by `registry::git_scope`, home's tools), `Deed::GitLeftOut` for what is not
  on a mirror yet (a card that brings Clone or the folder picker replaces its `GitLeftOut` guard with a `Deed::Git`),
  `GIT_GATES` for the menus.
- **A mirrored workspace's Git:** `Git::home`, `Found::Home(root)`, `Found::Refused`, `Git::at_home`,
  `set_home_git_folder`, `home_git_folder`, `home_git_folders`, `home_terminals_in`, `lies_in`, `home_name`.
- **Failures from home:** `client::Refusal.details`, `DETAILS_MOST`, `clean_refusal`, `heard`, `failure_words`,
  `GitAt.home` (a notification knows it is about a mirror's Git).
- **Generate:** `generate_at_home`, `generated_parts`, `notify_agent_failure`.
- **Adds at home:** `remote_add_in(kind, at, folder)`.
- 2h needs none of this; it shares only `remote_call` and the rule.

## Open for the orchestrator

Each decided with the recommended option; the plan is written that way.

1. **The seam passes both the `Op` and today's closure** (`git_op(at, op, here, home, done)`), not a local
   dispatcher over `Op`: 2f settled that the popup calls each job's own function and never `ops::run`, and a second
   dispatcher without `run`'s checks would be another place for the two to drift. *Recommended.*
2. **A mirrored workspace's Git holds no `Repo` and an empty `folder`:** home's path lives in `Git::home` and
   `Found::Home(root)`, so every local code path that stats or runs git (`poll_git`'s `is_dir`, `git_folders`'
   ancestors, `look_up_at`) finds nothing to touch. *Recommended* over a `Repo` with home's root (one forgotten branch
   would run git here on a path that may be someone else's folder).
3. **The panel's diff on a mirror is read when the status changed, after 10 s, or when the person asks** (open,
   Refresh, whitespace, whole file), not after every 2-second status read: the spec says "the diff panel as the tab
   asks", and a diff can weigh 8 MiB, which every 2 s through a tunnel is 4 MB/s. *Recommended.*
4. **A running Fetch, Pull, Push, Sync or Generate can't be stopped from the mirror;** the spinning button says so.
   Dropping the answer while home runs on would tell the person it stopped when it didn't; home's 10-minute deadline
   ends it. *Recommended.* (Minor logged with a fix.)
5. **Home's `details` travel in the client's `Refusal`** (new field, capped where it enters, cleaned in the glue), and a
   refusal becomes a `GitError` whose stderr is the details (else the sentence): every `done` closure, Details
   included, stays as it is. *Recommended.*
6. **A failed command's notification on a mirror keeps Details and drops Run in a terminal;** a login failure says
   home's words ("git on {computer} asks for a login: run it in a terminal there."), so `GitAt` carries the computer's
   name (`GitAt.home`): the notification is built inside the workspace's update and can't read it. *Recommended.*
7. **Full-only acts are refused at their entry** (`discard`, `discard_hunk`, `restore_version`, `delete_branch`,
   `stash_drop`, `abort_operation`), before any measure or question, besides their buttons and items being dimmed: a
   key, a stale menu or a notification's button can't reach home, and the person is never asked a question whose
   answer is refused. *Recommended.*
8. **The folder menu lists home's folders** (the workspace's and its terminals'), less those inside the shown
   repository, not repositories: listing repositories costs a `find` at home per folder each time it opens.
   *Recommended* (minor logged).
9. **`measure` has no "has a trash":** on a mirror a discard too large to keep says "Too much to keep a copy…" even
   when home keeps no trash, rather than growing home's answer in this part. *Recommended* (minor logged).
10. **Generate on a mirror never needs this computer's agent** and doesn't ask the first-use question here: home asks
    it, and refuses with its words until it was answered there. *Recommended.*
11. **Generate goes straight to `remote_call`, not the Git's queue,** as locally it runs beside the Git's commands
    (it only reads). *Recommended.*
12. **Recently discarded… is always offered on a mirror** (home's trash, an empty list when home keeps none).
    *Recommended.*
13. **The folder the popup opens on is home's path as home's state writes it, cleaned as `home_folders` cleans it**
    (one line, 300 characters); a path its cleaning changed fails at home with home's words. *Recommended.*
14. **Reads refused by the rule set the popup's error line; acts `warn`:** a read the person didn't click (a tab's list)
    shouldn't pop a notification, an act should say why. *Recommended.*

## Found against the code as built

- **The client's `Refusal` had only a sentence** (`templum-api/src/client/mod.rs`): home's `422 details` (git's whole
  stderr, 2f) were dropped at `http::templum_error`, so Details on a mirror needed Task 1.
- **`a_remote_workspace_refuses_what_it_cant_do_yet` asserts there is no Git on a mirror** (`tests/remote.rs`): it is
  the one existing test whose meaning 2g changes; the banner tip's test looks for "Git popup", which the new
  `NOT_MIRRORED` keeps.
- **A remote pane has no `seen.folder`** (remote panes are viewers): `terminal_list(Some(root))` and `Pane::works_in`
  count none on a mirror, so the header's count and the terminals' menu read home's state's `BlockView.folder`.
- **Several local paths would touch this computer's disk with home's path:** `Git::new` stats its folder, `poll_git`
  stats it every 2 s, `git_folders` walks ancestors for `.git`, the folder menu's Open a terminal here checks
  `is_dir`, `look_up_at` checks `is_dir`. Hence decision 2.
- **`CHANGED_MEANWHILE` was private to `ops/run.rs`:** a mirror's commit must tell it from other refusals to keep the
  popup's "write a message" path; it moves to `ops/mod.rs` as `pub`.
- **`ops::show` takes the loaded commit, `Op::Show` doesn't:** home reads the commit again; nothing to change.
- **The panel has no note for `FileDiff.cut`** (2f added the flag; no local diff sets it): `Note::Cut` is added.
- **A notification is built inside the workspace's update,** so it can't read whether its Git is a mirror's; `GitAt`
  carries it (decision 6).
- **Nothing in the design is unworkable against the code;** two parts are narrowed: the folder menu lists folders
  rather than repositories (decision 8), and the mirror can't stop a long operation (decision 4).
