# Templum to Templum, Batch 2f (Git at Home) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or
> superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Execution, already chosen:** one Sonnet 5.5 (high) implementer in a Templum agent block does the whole plan, then
> fresh Opus 5.5 reviewers review it in a loop until one passes it. The implementer **commits nothing**: everything
> stays uncommitted for the orchestrator.

**Goal:** Kanban card 171 part 2, batch 2, sixth of eight parts, home only: the Git popup's background jobs become
named operations in `templum_git::ops` (one function per job, which the popup now calls, with no change a person sees),
and home's API gains its own `git` tool that runs those operations in a repository of a workspace, scoped by operation
(reads: any token; acts: Control; what can lose work: Full), confined to a limited token's workspace folders, never with
raw git arguments, answering at most 8 MiB with git's whole stderr in a refusal's `details`.

**Architecture:** Bottom up, each step buildable. templum-git first: its result types gain serde (Task 1), then the
popup's multi-step job bodies move into `ops` as public functions the popup's closures call (Tasks 2–3; the popup's
`done` closures, words and queue stay as they are, proven by the existing tests passing unedited). Then `ops::Op` (one
serde enum, one variant per job), its argument checks, `ops::run` (state checks, then the same functions) and
`ops::fit` (the 8 MiB cut) (Task 4). Then templum-api: the `git` tool in the registry with its per-operation scope, the
answer's `git` and `details`, the audit's `git:<op>` (Task 5). Last the glue, `workspace/api_git.rs`: the folder, the
repository found off the UI thread, the confinement, a queue per repository root on the UI thread, a 10-minute
deadline, Generate's writer shared with the popup (Task 6). Docs and the full check close (Task 7).

**Tech Stack:** Rust 2024; serde/serde_json (already in the tree); the user's `git` via `std::process::Command`
(`templum_git::Repo`); GPUI through gpui-kit `=0.7.3` (only in `crates/templum`); the API's `tokio`/`hyper` server.

**Spec:** [docs/specs/templum-to-templum.md](../specs/templum-to-templum.md), "Batch 2: the whole workspace, live":
"Parts ▸ 2f", "What works on a remote workspace" (the three Git rows), "The Git popup on a remote workspace" (what 2g
will need), "What home's API gains ▸ The git tool", "Constraints (batch 2)", "Left to the builder (batch 2)", the
threat model's rows *(batch 2)* "A Control share now reaching further" and "Git through a share", "As built ▸ 1a, 2a"
(answers of `git` weigh up to 8 MiB on the client; the audit's grouping); [docs/specs/git.md](../specs/git.md) for the
popup's behaviour that must not change.

**Code it builds on:** `crates/templum-git/src/{lib.rs,repo.rs,parse.rs,diff.rs,blame.rs,trash.rs,conflict.rs,
message.rs}`, `crates/templum-git/tests/repo.rs`; the popup's glue `crates/templum/src/workspace/{git_ops.rs,
git_changes.rs,git_branches.rs,git_switch.rs,git_sync.rs,git_history.rs,git_compare.rs,git_conflicts.rs,
git_discard.rs,git_message.rs}`, `crates/templum/src/git_block/{mod.rs,words.rs}`; the API
`crates/templum-api/src/{registry.rs,server.rs,audit.rs,openapi.rs,lib.rs}`, `crates/templum/src/workspace/api.rs`
(`api_call`, `api_allows`, `confined`, `confined_roots`, `confined_default`, `refuses_share`, `prefix_of`,
`main_tree_inside`, `answer_line`); tests `crates/templum/src/workspace/tests/{mod.rs,api.rs,remote.rs,git_*.rs}`.

**Consumes (as built):** `Repo::{discover, init, run, status, operation, author, stage, unstage, discard, ignore,
main_tree, commit, last_message, stage_all, on_upstream, head, undo_commit, apply_patch, working_diff, compare_log,
range_diff, commit_changes, resolve, abort, continue_, fetch, pull, push, publish, push_branch, remotes,
default_branch, sync, sync_join, history, commit_info, unpushed, file_log, file_at, blame, commit_files,
checkout_commit, new_branch, tag, revert, branches, rename_branch, merge, delete_branch, unmerged_count, stash_push,
stashes, stash_apply, stash_pop, stash_drop, stash_diff, switch, with_cancel}`, `GitError::{first_line, cancelled,
needs_input}`, `hunk_patch`, `parse_diff`, `trash::{Trash, Kept, Restored, measure, fits}`, `conflict::{regions,
resolve, Region, Take}`, `message::{changes, request, run, split, Request, Failure, PROMPT}`; in the glue
`Workspace::{git_job, act_job, git_at, here, after_action, space_of, space_named, api_scope, confined, confined_roots,
confined_default}`, `Space::mirror_words`, `bus::clean_title`, `Outcome`, `Replier`, `rpc::{result, error}`,
`codes::{SCOPE, NOT_FOUND}`, `rpc::PARAMS`; the tests' `open`, `open_with`, `write_config`, `git_repo`,
`committed_repo`, `git_run`, `settle`, `with_token`, `api_later`, `refused`, `said`, and `tests/remote.rs`'s helpers
that make a remote workspace (`offline` / `live`).

## Global Constraints

- **MEMORY LIMITS on this machine:** **every** cargo command takes `-j 4`; **never two cargo commands at once**; never
  `cargo clean`; never `git stash`; while working run only the targeted tests each step names; the full suite
  **once**, in Task 7.
- **Crate boundaries:** only `crates/templum` depends on gpui-kit; `templum-git` and `templum-api` stay UI-free and are
  tested headlessly. `templum-api` gains a path dependency on `templum-git` (Templum's own, UI-free). **No new external
  dependency:** `templum-git` gains `serde` (derive) and `serde_json`, both already in `Cargo.lock`; the lock's diff
  is the two crates' dependency lines only. No `unsafe`.
- **The local Git popup behaves exactly as before.** Only job bodies move; every `done` closure keeps its words,
  notifications, buttons and order. Proof: every test in `crates/templum/src/workspace/tests/git_*.rs`,
  `crates/templum/src/git_block/**` and `crates/templum-git/tests/repo.rs` passes **without an edit**, and
  `--bench-git` stays at `p95_ms` ≤ 16.7 (Task 7). The popup calls the job's function in `ops`, never `ops::run`
  (which checks untrusted arguments the popup never has).
- **Never raw git arguments.** The API reaches git only through `ops::Op`; no field of it is passed to git unless the
  checks of Task 4 passed first. Rules, verbatim:
  - a **path** argument: relative to the repository's root; 1–4,096 bytes; no control character (below U+0020, or
    U+007F); not starting with `/`, `\` or `:` and no drive (`C:`); split on `/` and `\`, no part empty, `.` or `..`,
    and none equal to `.git` in any case. Before use (Task 4's `inside`): the deepest existing ancestor of
    `root.join(path)`, canonicalized, lies under the canonical root; for a path the operation **writes**
    (`restore_version`'s `path`, `resolve_region`'s `path`) the file itself, when it exists, is not a symbolic link.
  - a **hash**: 4–64 ASCII hex digits.
  - a **new name** (`new_branch`'s and `rename_branch`'s `name`, `tag`'s `name`): 1–255 bytes, not starting with `-`,
    no control character, no whitespace (git's own `check-ref-format` follows for branches, as today).
  - an **existing branch** (`branch`, `base`): not starting with `-`, no control character, 1–255 bytes, and then
    listed by `Repo::branches()` (a local one where the operation needs a local one).
  - a **remote**: listed by `Repo::remotes()`: never a URL or a path.
  - a **stash**: `{ "index": n ≤ 10,000, "hash": <hash> }`, then `Repo`'s own check that `stash@{n}` is still that hash.
  - a **trash entry**: 1–100 bytes of `[A-Za-z0-9._-]`, not `.` or `..`, then listed by `Trash::list()` **with this
    repository's root**.
  - a **patch** (`stage_hunk`, `unstage_hunk`, `discard_hunk`): parsed by `parse_diff` it is exactly one file with
    exactly one hunk, and `hunk_patch(&file, 0)` equals it byte for byte (so no mode, rename, binary or symlink
    header passes); its path passes the path rule and the status lists that file on the side the operation acts on.
  - **texts:** `summary`, `name` and `email` (set_author), `leave`, `message` (stash): one line, at most 4,096
    characters; search `text`: one line, at most 1,000; `description`: at most 65,536 characters; `paths`: 1–10,000;
    `whole`: at most 1,000; `skip` ≤ 1,000,000; `region` ≤ 10,000.
- **Values, verbatim:** `ops::ANSWER_MOST` = 8,323,072 bytes (8 MiB − 64 KiB: the `git` value of an answer as JSON;
  the text, envelope and `details` fit in the rest of the client's 8 MiB); `DETAILS_MOST` = 65,536 bytes (stderr cut
  at a character's boundary); `GIT_DEADLINE` = 600 s; `ops::PAGE` = 200 (History's page, moved from
  `git_block::PAGE`).
- **Scopes, verbatim** (the registry's `git_scope`):
  - **read:** `find`, `look`, `diff`, `history`, `show`, `blame`, `branches`, `unmerged`, `default_branch`,
    `compare`, `stashes`, `stash_diff`, `remotes`, `last_message`, `measure`, `trash`;
  - **full:** `discard`, `discard_hunk`, `restore_version`, `delete_branch`, `stash_drop`, `abort`;
  - **control:** every other operation of `ops::OPS`: `init`, `set_author`, `stage`, `unstage`, `stage_hunk`,
    `unstage_hunk`, `commit`, `undo_commit`, `fetch`, `pull`, `push`, `push_branch`, `publish`, `sync`, `switch`,
    `new_branch`, `rename_branch`, `merge`, `checkout`, `tag`, `revert`, `stash`, `stash_apply`, `stash_pop`,
    `continue`, `resolve`, `resolve_region`, `restore`, `ignore`, `message`.
- **Status codes:** a malformed argument or an unknown operation is `400` `invalid` (the registry, before the UI
  thread); too low a scope or a repository outside a limited token's folders is `403` `scope`; a state refusal or a
  git failure is `422` `refused` with `message` and, for a git failure, `details` (its stderr).
- **Words, verbatim** (`{computer}` is `templum_api::computer()`, this computer's name; `{workspace}` the workspace's
  name through `clean_title`):
  - 403: "That repository lies outside {workspace}'s folder."; "git's discard needs a Full token." (the operation's
    name in it; any Full or Control operation the same way).
  - 400: "There's no git operation rm: GET /v1/tools lists them."; "git needs \"op\": GET /v1/tools lists the
    operations."; "\"../x\" isn't a path inside the repository: give it relative to its root, without \"..\" or
    .git."; "\"zz\" isn't a commit's hash."; "\"-f\" can't name a branch."; "\"../x\" isn't a discard of the trash.";
    "The patch isn't one hunk of one file, as the diff shows it."; "{field} is one line of at most {n} characters.";
    "paths takes 1 to 10,000 paths.".
  - 422 (state): "There's no branch nope: branches lists them."; "There's no remote https://evil.example/x.git: remotes
    lists them."; "a.txt has no changes to discard."; "a.txt has no changes to stage."; "a.txt has nothing staged.";
    "a.txt isn't in conflict."; "out/x lies outside the repository."; "a.txt is a link, which Templum doesn't write
    through."; "There's no discard 17-3 of this repository: trash lists them."; "This Templum keeps no trash.";
    "No merge, rebase, cherry-pick or revert is in progress."; "There's already a repository at /home/x/app.";
    "/home/x/notes isn't in a Git repository: init makes one."; "/home/x/gone isn't a folder on this computer.";
    "The files to commit changed since you last looked: write a message for them." (the popup's);
    "The file changed; try again" (`ops::CHANGED`, the popup's `REFUSED`); "Couldn't keep a copy, so nothing was
    changed: {e}"; "Couldn't write the file: {e}"; "The answer is too large to send: 9.4 MB, past 8 MB.".
  - 422 (glue): "git on {computer} asks for a login: run it in a terminal there." (`GitError::needs_input`); "git on
    {computer} ran for 10 minutes and was stopped." (the deadline); "Generate a message on {computer} once first: it
    asks there whether the diff may go to the agent's provider."; "No commit-message agent is ready on {computer}:
    choose one in Settings ▸ Git there."; a mirrored workspace answers `Space::mirror_words` as every tool does.
  - Every other git failure: `message` is `GitError::first_line()`, `details` its whole stderr.
- **Every feature in the API** (CLAUDE.md): the `git` tool ships its registry entry, scope, OpenAPI schema and its
  lines in `docs/api.md` and `docs/specs/api.md` in Task 5–7, as named there.
- **Tests:** Linux, macOS and Windows, no agent CLI, no network beyond loopback, no fixed sleeps (conditions with a 10 s
  deadline); `git` is on CI (templum-git's tests run it); symlinks, shell commands and `sleep` are `#[cfg(unix)]`;
  paths compared through `dunce::canonicalize` in the glue (macOS's `/var` → `/private/var`, Windows' `\\?\`). Every
  logic change has a test that fails without it.
- **Minors** go to `docs/minors.md` (`## Templum to Templum`), with where, what a person sees and a fix. Docs lines
  ≤ 120 columns.
- **Commit nothing.** Every task ends with its tests passing, left uncommitted.

## Review Focus

1. **An argument that is an option or leaves the repository** (a branch `-f`, a tag `--delete`, a hash `--output=x`,
   a blame hash `--contents=/etc/passwd`, a publish remote `--receive-pack=touch x` or `https://evil.example/x.git`,
   a path `../x`, `/etc/passwd`, `C:\x`, `.git/hooks/pre-commit`, `:(top)x`, a write through a symlink inside the
   repository that points outside): refused before git or the filesystem is touched, with the rule's words.
   → Task 4 `arguments_that_could_be_options_or_leave_the_repository_are_refused`,
   `names_that_aren_t_there_are_refused_before_git_runs`, `a_write_through_a_link_leaving_the_repository_is_refused`.
2. **A trash entry from elsewhere** (`../../x`, an entry of another repository's discard, one of a token for another
   workspace): `Trash::restore(id)` joins `id` into a path and writes where its manifest says, so only an entry
   `Trash::list()` gives for this repository's root may reach it. → Task 4
   `restore_takes_only_this_repository_s_discards`.
3. **A crafted hunk** (an `old mode`/`new mode` header, `new file mode 120000`, two hunks, a binary patch, a rename, a
   file the status doesn't list, a `discard_hunk` whose `path` differs from its patch's): refused; only a hunk as the
   panel draws it applies. → Task 4 `a_patch_must_be_one_hunk_of_a_listed_file_as_the_diff_shows_it`.
4. **A limited token's repository that only starts inside its folder** (its folder is a subfolder of a repository
   whose root is above; a linked worktree whose main tree is elsewhere; a mirrored workspace on this computer): `403`
   or the mirror's words, and no git operation runs; a token for all workspaces is not confined (it opens agents
   anywhere). → Task 6 `a_limited_token_s_repository_outside_its_folder_is_refused`,
   `a_worktree_whose_main_tree_is_outside_is_refused`, `git_on_a_mirrored_workspace_is_refused`.
5. **A huge or hung answer:** a diff past 8 MiB keeps every file's row and drops the largest files' hunks first, each
   `cut`; any other answer past it is refused, never sent; a git that never ends is killed at the deadline and its
   repository's queue moves on. → Task 4 `a_big_diff_is_cut_largest_files_first`,
   `an_answer_too_large_to_cut_is_refused`; Task 1 `a_deadline_kills_a_command_that_runs_past_it`; Task 6
   `two_calls_on_one_repository_answer_in_turn`.

---

### Task 1: templum-git's types travel as JSON; a deadline; the author in a repository

**Files:**
- Modify: `crates/templum-git/Cargo.toml` (`[dependencies]` `serde = { version = "1", features = ["derive"] }`,
  `serde_json = "1"`, each with an "Already in the tree" comment as `templum-api`'s are)
- Modify: `crates/templum-git/src/parse.rs` (`Change`, `Entry`, `Status`, `Commit`, `Branch`, `Stash`, `CommitFile`,
  `Operation`), `src/diff.rs` (`FileDiff`, `Hunk`, `Line`, `Kind`; `FileDiff::cut`), `src/blame.rs` (`BlameLine`),
  `src/repo.rs` (`WorkingDiff`, `Side`, `Pull`, `Synced`, `Back`, `Search`, `HistoryQuery`; `Repo::deadline`,
  `with_deadline`, `run_in`, `set_author`), `src/trash.rs` (`Kept`, `Restored`), `src/conflict.rs` (`Take`)
- Modify: `crates/templum/src/workspace/git_changes.rs:310` (Set…'s job: `repo.set_author(&name, &email, true)`)
- Test: `crates/templum-git/tests/repo.rs`, `crates/templum-git/src/repo.rs` (its `#[cfg(test)]` module; add one if
  missing)

**Interfaces:**
- Produces: `#[derive(Serialize, Deserialize)]` on every type above; every enum `#[serde(rename_all = "snake_case")]`
  in serde's default (external) representation (`"cherry_pick"`, `{"done":{"got":1,"sent":2}}`, `{"kept":"why"}`).
  `FileDiff { …, #[serde(default)] pub cut: bool }` (doc: "Its hunks were dropped to fit an answer of the API's `git`
  tool (`ops::fit`)."; `FileDiff::empty()` sets false). `Repo::with_deadline(&self, at: Instant) -> Repo`;
  `Repo::set_author(&self, name: &str, email: &str, global: bool) -> Result<(), GitError>` (`git config [--global]
  user.name <name>`, then `user.email`, run in the root; replaces the old associated function, whose only caller is
  Set…).
- `run_in(program, dir, args, cancel: Option<&AtomicBool>, deadline: Option<Instant>)`: with either set it goes through
  `until_cancelled(c, cancel.unwrap_or(&NEVER), deadline)` (`static NEVER: AtomicBool`); a kill is today's
  `CANCELLED` error, so `GitError::cancelled()` is true.

- [ ] **Step 1: Write the failing tests**

```rust
// crates/templum-git/tests/repo.rs
#[test]
fn the_git_types_go_to_json_and_back() {
    // fresh(); a.txt committed, then a.txt changed and b.txt untracked, a stash pushed, a branch "feature".
    // For each of status(), branches(), history(&HistoryQuery::default(), 0, 10), stashes(),
    // working_diff(&status, false, &[]): serde_json::from_value(serde_json::to_value(&x)) == x.
}

#[test]
fn enums_are_snake_case_and_a_diff_without_cut_reads_as_not_cut() {
    assert_eq!(serde_json::to_value(Operation::CherryPick).unwrap(), "cherry_pick");
    assert_eq!(serde_json::to_value(Change::TypeChanged).unwrap(), "type_changed");
    assert_eq!(serde_json::to_value(Synced::Done { got: 1, sent: 2 }).unwrap(), json!({"done": {"got": 1, "sent": 2}}));
    assert_eq!(serde_json::to_value(Back::Kept("x".into())).unwrap(), json!({"kept": "x"}));
    // A FileDiff serialized, its "cut" key removed, reads back with cut == false.
}

#[test]
fn set_author_writes_the_repository_s_own_config_when_not_global() {
    // fresh(); repo.set_author("Grace", "grace@example.com", false);
    // repo.run(&["config", "--local", "--get", "user.name"]) == "Grace\n", and user.email likewise.
}
```

```rust
// crates/templum-git/src/repo.rs, #[cfg(test)]
#[cfg(unix)]
#[test]
fn a_deadline_kills_a_command_that_runs_past_it() {
    let started = Instant::now();
    let err = run_in("sleep", Path::new("."), &["30"], None, Some(started + Duration::from_millis(100))).unwrap_err();
    assert!(err.cancelled(), "{err}");
    assert!(started.elapsed() < Duration::from_secs(10));
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-git -- the_git_types enums_are
  set_author_writes a_deadline_kills` → compile errors (no serde, no `set_author(&self…)`, `run_in` takes four
  arguments).
- [ ] **Step 3: Implement** the derives, `cut`, `deadline`/`with_deadline`/`run_in`, `set_author`; update Set…'s job in
  `git_changes.rs` to `move |repo| repo.set_author(&name, &email, true)` (`program` no longer captured there).
- [ ] **Step 4: Run** the four tests → pass; then `cargo test -j 4 -p templum-git` → all pass; then
  `cargo test -j 4 -p templum git_commit` → `without_an_author_commit_is_disabled_and_set_asks_for_one` passes
  unedited.

---

### Task 2: `ops`, part 1: the status, commits, sync and Generate move to templum-git

**Files:**
- Create: `crates/templum-git/src/ops/mod.rs` (module doc; `pub mod jobs` re-exported with `pub use jobs::*`; the
  types of this task), `crates/templum-git/src/ops/jobs.rs` (the job bodies)
- Modify: `crates/templum-git/src/lib.rs` (`pub mod ops;`), `src/message.rs` (`Writer`, `generate`)
- Modify (popup, bodies only): `crates/templum/src/git_block/mod.rs` (`Read` removed; `join_message` becomes
  `pub(crate) use templum_git::ops::join_message;`), `crates/templum/src/git_block/words.rs` (`generic_summary` and
  `staged_summary` become `pub(crate) use templum_git::ops::{generic_summary, staged_summary};`, so its tests and
  callers compile unchanged), `crates/templum/src/workspace/git_ops.rs` (`refresh_at`: `move || ops::look(&repo)`),
  `git_changes.rs` (`git_commit`: `move || ops::commit(&repo, &typed, &description, amend, all)`; `undo_commit`:
  `move || ops::undo_commit(&repo, &job_short, replaced.as_deref())`, the private `Undone` replaced by `ops::Undone`),
  `git_sync.rs` (`run_sync`: `move || ops::sync(&repo, join)`), `git_message.rs` (`commit_writer`, `generate_now`)
- Test: `crates/templum-git/tests/ops.rs` (new; its own `fresh()` and `write()` as `tests/repo.rs` has them)

**Interfaces:**
- Produces, in `templum_git::ops` (all `pub`, serde both ways unless said):
  - `struct Look { status: Status, operation: Option<Operation>, author: Option<(String, String)> }`;
    `fn look(repo: &Repo) -> Result<Look, GitError>` (status, `operation()`, `author()`, as `Read::of` did).
  - `fn generic_summary(entry: &Entry, staged: bool) -> Option<String>`, `fn staged_summary(status: &Status) ->
    Option<String>`, `fn join_message(summary: &str, description: &str) -> String`: moved verbatim.
  - `struct Committed { short: String, replaced: Option<String> }`; `fn commit(repo: &Repo, summary: &str,
    description: &str, amend: bool, all: bool) -> Result<Option<Committed>, GitError>`: the closure's body verbatim
    (`all` stages all first; a blank summary is `staged_summary` of the status now, None when there is none; `replaced`
    is HEAD read before an amend).
  - `enum Undone { Done(String), Pushed, Moved }`; `fn undo_commit(repo: &Repo, short: &str, replaced:
    Option<&str>) -> Result<Undone, GitError>`: the closure's body verbatim.
  - `fn sync(repo: &Repo, join: Option<bool>) -> Result<Synced, GitError>` (None: `sync()`; Some(rebase):
    `sync_join(rebase)`).
- Produces, in `templum_git::message`: `struct Writer { command: String, model: String, env: Vec<(String, String)>,
  instructions: String, template: String, max_bytes: usize, timeout: Duration }` (no serde: it never crosses the
  network); `fn generate(repo: &Repo, w: &Writer, all: bool, cancel: &AtomicBool) -> (Result<String, Failure>,
  bool)`: `generate_now`'s background body verbatim (HEAD before; `changes`, `request`, `run` in the root; the bool is
  "HEAD moved meanwhile").
- Produces, in the glue: `Workspace::commit_writer(&self, root: &Path) -> Option<message::Writer>`: what
  `generate_now` builds today from `commit_agent` (when `ready`) and `config.git` (this repository's instructions over
  Settings ▸ Git's; the template, else `message::PROMPT`; `max_diff_kb` × 1024; `timeout` seconds). `generate_now`
  uses it; Task 6 uses it too.

- [ ] **Step 1: Write the failing tests** (`tests/ops.rs`):

```rust
#[test]
fn look_reads_the_status_the_operation_and_the_author() {
    // fresh(); a.txt untracked → look.status.entries[0].path == "a.txt", operation None,
    // author == Some(("Ada Lovelace", "ada@example.com")).
}

#[test]
fn commit_with_no_summary_names_the_one_staged_file() {
    // a.txt staged; ops::commit(&repo, "", "", false, false) → Ok(Some(c)); c.replaced == None;
    // `git log -1 --format=%s` == "Add a.txt\n"; c.short == `git rev-parse --short HEAD`.
}

#[test]
fn commit_with_no_summary_and_two_staged_files_waits_for_a_message() {
    // a.txt and b.txt staged → Ok(None), nothing committed (`git rev-list --all` empty).
}

#[test]
fn commit_all_stages_every_change_first_and_joins_the_description() {
    // a.txt committed, then changed, b.txt untracked, nothing staged; commit(&repo, "Both", "Why", false, true)
    // → log -1 --format=%B == "Both\n\nWhy\n"; status clean.
}

#[test]
fn an_amend_remembers_the_commit_it_replaced_and_undo_goes_back_to_it() {
    // commit "One", then change, stage, commit(&repo, "One, amended", "", true, false) → replaced == Some(old head);
    // undo_commit(&repo, &c.short, c.replaced.as_deref()) == Undone::Done("One, amended"); HEAD == old head.
}

#[test]
fn undo_commit_of_a_commit_no_longer_last_is_moved() {
    // two commits; undo_commit(&repo, <first's short>, None) == Undone::Moved; HEAD unchanged.
}

#[test]
fn sync_without_an_upstream_is_unpublished() {
    // one commit, no remote → ops::sync(&repo, None) == Ok(Synced::Unpublished).
}

#[cfg(unix)]
#[test]
fn generate_runs_the_writer_and_says_whether_a_commit_came_meanwhile() {
    // a.txt staged; Writer { command: "printf 'Add a\\n\\nBecause'".into(), model: "".into(), env: vec![],
    // instructions: "".into(), template: message::PROMPT.into(), max_bytes: 65536, timeout: 10 s }
    // → (Ok(answer), false) with message::split(&answer) == Some(("Add a", "Because")).
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-git --test ops` → compile errors (no `ops`).
- [ ] **Step 3: Implement** `ops/mod.rs`, `ops/jobs.rs` and `message::{Writer, generate}`, moving the bodies; then the
  popup: each closure named above calls the moved function, its `done` closure unchanged (it matches
  `ops::Undone::{Done, Pushed, Moved}` and reads `read.status`, `read.operation`, `read.author` as before);
  `generate_now` takes `self.commit_writer(&repo.root)` (return when None) and its background body is
  `message::generate(&repo, &writer, all, &cancel)`.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-git --test ops` → pass; then, one at a time,
  `cargo test -j 4 -p templum git_commit`, `… git_status`, `… git_sync`, `… git_message` → every existing test passes
  **unedited**.

---

### Task 3: `ops`, part 2: discards, the trash, conflicts, comparisons, History and a commit shown

**Files:**
- Modify: `crates/templum-git/src/ops/mod.rs` (`Failure`, `CHANGED`, `PAGE`, `thousands`, the types below),
  `src/ops/jobs.rs` (the bodies below)
- Modify (popup, bodies only): `crates/templum/src/workspace/git_discard.rs` (`Discarding` and `Failed` replaced by
  `ops::Discarding` and `ops::Failure`; `REFUSED` becomes `pub(crate) use templum_git::ops::CHANGED as REFUSED;`;
  `thousands` becomes `pub(crate) use templum_git::ops::thousands;` (its test stays); `discard_now`: `move ||
  ops::discard(&repo, trash.as_ref(), &paths, &kept_as, &what)`; `show_discarded`: `move || ops::discarded(&trash,
  &root)`), `git_conflicts.rs` (private `Refused` replaced by `ops::Failure`; `resolve_region`: `move ||
  ops::resolve_region(&repo, keep.as_ref(), &path, region, take, &shown)`, which now needs the repo, not only its root;
  `resolve_file`: `move || ops::resolve_file(&repo, keep.as_ref(), &path, side)`; `tell` matches `Failure::{Changed,
  Keep, Io, Git}` with today's words and says a `Refused` sentence as an error notification), `git_history.rs`
  (`load_history`: `move || ops::history(&repo, &q, file.as_deref(), skip)` returning `ops::Page`, the done closure
  reading `page.commits`, `page.paths`, `page.unpushed`; `restore_now`: `move || ops::restore_version(&repo,
  trash.as_ref(), &hash, &then, &now, &what)`, its done closure mapping `Failure::Keep(e)` to "Couldn't keep a copy, so
  nothing was restored: {e}", `Failure::Io(e)` to "Couldn't write {now}: {e}", `Failure::Git(e)` to `e.to_string()`
  as today; `show_commit`: `move || ops::show(&repo, &hash, loaded, ignore_ws)` returning `ops::CommitShown`),
  `git_compare.rs` (`load_compare`: `move || ops::compare(&repo, &base, &branch, narrowed.as_deref(), ignore_ws)`
  returning `ops::Compared`), `crates/templum/src/git_block/mod.rs` (`PAGE` becomes `pub(crate) use
  templum_git::ops::PAGE;`)
- Test: `crates/templum-git/tests/ops.rs`

**Interfaces:**
- Produces, in `templum_git::ops`:
  - `#[derive(Debug)] enum Failure { Refused(String), Changed, Keep(io::Error), Io(io::Error), Git(GitError),
    Agent(message::Failure) }` with `impl From<GitError>` and `fn sentence(&self) -> String` (Refused: itself; Changed:
    `CHANGED`; Keep: "Couldn't keep a copy, so nothing was changed: {e}"; Io: "Couldn't write the file: {e}"; Git:
    `first_line()`; Agent: its `Display`). `pub const CHANGED: &str = "The file changed; try again";`
  - `pub const PAGE: usize = 200;` `fn thousands(n: usize) -> String` (moved verbatim).
  - `#[derive(Clone)] enum Discarding { Files(Vec<Entry>), Hunk(String) }`; `fn discard(repo: &Repo, trash:
    Option<&Trash>, paths: &[String], what: &str, discarding: &Discarding) -> Result<Option<Kept>, Failure>` (the
    closure's body verbatim: `trash` is Some only when a copy is to be kept; the copy, then git, then the seal and
    prune; a failed git removes the copy).
  - `fn discarded(trash: &Trash, root: &Path) -> Vec<Kept>` (`list()` filtered by root).
  - `fn restore_version(repo: &Repo, trash: Option<&Trash>, hash: &str, then: &str, now: &str, what: &str) ->
    Result<Option<Kept>, Failure>` (the body verbatim; the copy kept when `trash` is Some and the file fits).
  - `fn resolve_file(repo: &Repo, trash: Option<&Trash>, path: &str, side: Side) -> Result<(), Failure>`;
    `fn resolve_region(repo: &Repo, trash: Option<&Trash>, path: &str, region: usize, take: Take, shown:
    &(Vec<String>, Vec<String>)) -> Result<(), Failure>` (bodies verbatim, `root` = `repo.root`).
  - `struct Compared { commits: Vec<Commit>, files: Vec<FileDiff> }`; `fn compare(repo: &Repo, base: &str, branch:
    &str, narrowed: Option<&str>, ignore_ws: bool) -> Result<Compared, GitError>`.
  - `struct Page { commits: Vec<Commit>, paths: Vec<CommitFile>, unpushed: Option<HashSet<String>> }`;
    `fn history(repo: &Repo, q: &HistoryQuery, file: Option<&str>, skip: usize) -> Result<Page, GitError>` (a file's
    `file_log` unzipped, else `history`; `PAGE` commits; `unpushed()` only when `skip == 0`).
  - `struct CommitShown { commit: Commit, files: Vec<CommitFile>, diffs: Vec<FileDiff> }`; `fn show(repo: &Repo, hash:
    &str, loaded: Option<Commit>, ignore_ws: bool) -> Result<CommitShown, GitError>` (`commit_info` only when
    `loaded` is None).

- [ ] **Step 1: Write the failing tests** (`tests/ops.rs`; a `conflicted()` helper: on `main` a.txt "one\n" committed,
  branch `feature` changes it to "theirs\n", `main` to "ours\n", `git merge feature` fails):

```rust
#[test]
fn discard_keeps_a_copy_then_throws_the_changes_away() {
    // a.txt committed "one\n" then changed to "two\n", b.txt untracked; trash = Trash::new(tempdir);
    // entries from status().unstaged(); discard(&repo, Some(&trash), &paths, "Discarded 2 files",
    // &Discarding::Files(entries)) → Ok(Some(kept)), kept.what == "Discarded 2 files"; a.txt reads "one\n";
    // b.txt is gone; discarded(&trash, &repo.root) == [kept].
}

#[test]
fn a_discard_whose_copy_fails_discards_nothing() {
    // the trash's folder is a file → Err(Failure::Keep(_)); a.txt still "two\n".
}

#[test]
fn discard_hunk_reverses_one_hunk_on_the_working_tree() {
    // a.txt of 30 lines committed, lines 2 and 28 changed; the unstaged diff's first hunk through hunk_patch;
    // discard(&repo, None, &["a.txt"], "…", &Discarding::Hunk(patch)) → Ok(None); line 2 back, line 28 still changed.
}

#[test]
fn discarded_lists_only_this_repository_s_discards() {
    // two repositories, one discard each into one trash → discarded(&trash, &first.root) has only the first's.
}

#[test]
fn restore_version_writes_the_file_as_it_was_and_keeps_a_copy() {
    // a.txt "one\n" in commit A, "two\n" in B; restore_version(&repo, Some(&trash), &a_hash, "a.txt", "a.txt",
    // "Restored a.txt from a1b2c3d") → Ok(Some(kept)); a.txt reads "one\n"; trash.restore(&kept.id, false) puts "two\n"
    // back.
}

#[test]
fn resolve_file_takes_one_side_after_a_copy() {
    // conflicted(); resolve_file(&repo, Some(&trash), "a.txt", Side::Theirs) → Ok; a.txt == "theirs\n";
    // discarded(&trash, &repo.root).len() == 1.
}

#[test]
fn resolve_region_keeps_the_part_chosen_and_refuses_a_changed_region() {
    // conflicted(); shown = the region's two parts' lines (`Region::lines`); resolve_region(&repo, None, "a.txt", 0,
    // Take::First, &(vec!["x".into()], vec![])) → Err(Failure::Changed); with the real `shown` → Ok, a.txt == "ours\n".
}

#[test]
fn compare_lists_the_branch_s_commits_and_its_whole_or_narrowed_diff() {
    // main: a.txt; feature: two commits adding b.txt then c.txt; compare(&repo, "main", "feature", None, false)
    // → 2 commits, files b.txt and c.txt; narrowed to the newest commit's hash → files == [c.txt].
}

#[test]
fn history_pages_and_reads_unpushed_on_the_first_page_only() {
    // 3 commits; history(&repo, &HistoryQuery::default(), None, 0) → 3 commits, unpushed == Some(empty);
    // skip 1 → 2 commits, unpushed == None; a.txt renamed to b.txt in a commit → history(…, Some("b.txt"), 0)
    // follows it (2 commits, paths[1].path == "a.txt").
}

#[test]
fn show_reads_a_commit_its_files_and_its_diff() {
    // show(&repo, &head, None, false) → commit.hash == head, files[0].path == "a.txt", diffs.len() == 1.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-git --test ops` → compile errors.
- [ ] **Step 3: Implement** the functions by moving the bodies, then the popup's closures as listed above.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-git --test ops` → pass; then, one at a time,
  `cargo test -j 4 -p templum git_trash`, `… git_conflicts`, `… git_history`, `… git_compare`, `… git_changes`,
  `… git_hunks`, `… git_keys` → every existing test passes **unedited**.

---

### Task 4: `Op`, its checks, `run` and the 8 MiB cut

**Files:**
- Modify: `crates/templum-git/src/ops/mod.rs` (`Op`, `OPS`, `Answer`, `Done`, the answer types below, `Home`, `run`,
  `fit`, `ANSWER_MOST`)
- Create: `crates/templum-git/src/ops/check.rs` (the shape checks, `Op::check`, and the state checks `run` makes)
- Test: `crates/templum-git/tests/ops.rs`

**Interfaces:**
- Consumes: Tasks 1–3.
- Produces, in `templum_git::ops`:
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] #[serde(tag = "op", rename_all = "snake_case")]
    enum Op` with one struct (or unit) variant per operation, fields as the table of "Interfaces for later parts"
    names them (optional ones `#[serde(default)]`: `bool` false, `Option` None, `Vec` empty, `skip` 0, `how`
    `Pull::FastForward`). Helper argument types: `struct SearchArg { by: Search, text: String }`, `struct StashRef {
    index: usize, hash: String }`, `struct WholeFile { staged: bool, path: String }`. Unknown keys beside them (the
    API's `workspace`, `folder`) are ignored by serde.
  - `pub const OPS: &[&str]`: every operation's name, in the table's order; `Op::name(&self) -> &'static str`.
  - `Op::check(&self) -> Result<(), String>`: the shape rules of the Global Constraints (no git, no filesystem); the
    Err is the 400 sentence.
  - `struct Home<'a> { trash: Option<&'a Trash>, writer: Option<&'a message::Writer>, global_author: bool }`.
  - `struct Answer { text: String, git: Done }`; `#[derive(Serialize)] #[serde(untagged)] enum Done { Found(Found),
    Look(Look), Diff(WorkingDiff), History(Page), Show(CommitShown), Blame(Vec<BlameLine>), Branches(Vec<Branch>),
    Count(u32), Name(Option<String>), Compare(Compared), Stashes(Vec<Stash>), Files(Vec<FileDiff>),
    Names(Vec<String>), Text(String), Measured(Measured), Trash(Vec<Kept>), Committed(Committed), Undone(Undone),
    Synced(Synced), Switched(Back), Restored(Restored), Kept(Option<Kept>), Message(Message), Nothing }` (`Nothing`
    is JSON `null`). `struct Found { root: Option<String>, main_tree: Option<String> }`, `struct Measured { files:
    usize, bytes: u64, keep: bool }`, `struct Message { summary: String, description: String, committed: bool }` (serde
    both ways).
  - `fn run(repo: &Repo, op: Op, home: &Home) -> Result<Answer, Failure>`: `op.check()` (Err → `Failure::Refused`),
    then the operation's state checks (`check.rs`: branches and remotes listed, paths `inside` and listed in the status
    where the rule says, the patch rule, the stash, the trash entry), then the same function the popup calls (Tasks
    1–3, or the one `Repo` method its job calls), then `fit(&mut done, ANSWER_MOST)`. Notes per operation:
    `find` answers the root and `main_tree()`; `init` refuses "There's already a repository at {root}." (the glue runs
    `Repo::init` itself when there is none); `diff` reads the status first; `history` builds the `HistoryQuery`;
    `show` passes no `loaded`; `measure`'s `keep` is `home.trash.is_some() && fits(files, bytes)`; `trash` is empty
    without a trash; `restore` needs a trash and an entry `discarded(trash, &repo.root)` lists; `set_author` writes
    globally only when `home.global_author`; `commit` maps `Ok(None)` to the popup's refusal; `continue` and `abort`
    read `repo.operation()`; `discard` keeps a copy when the trash fits (`measure`), its `what` the popup's ("Discarded
    a.txt" / "Discarded 1,240 files", `thousands`); `discard_hunk`'s `what` "Discarded a change to {path}";
    `restore_version`'s "Restored {path} from {hash[..7]}"; `resolve` and `resolve_region` keep a copy when they fit;
    `message` needs `home.writer` (else "No commit-message agent is ready."), runs `message::generate` with a cancel
    never set, splits the answer (`Failure::Agent(message::Failure::Empty { stderr: String::new() })` when it
    can't).
  - `fn fit(done: &mut Done, most: usize) -> Result<(), Failure>`: when `serde_json::to_vec(done)`'s length is above
    `most` and `done` holds file diffs (`Diff` (both lists), `Show`, `Compare`, `Files`), drop the hunks of the file
    whose hunks weigh most as JSON, set its `cut`, and measure again until within; otherwise (or nothing left to cut)
    `Failure::Refused("The answer is too large to send: {x.x} MB, past 8 MB.")` (MB = 1,000,000 bytes, one decimal).
  - `Answer.text`, verbatim (`{n}` counts, its noun singular for 1: "1 changed file", "2 changed files"; `{s}` a
    short hash): find "The repository is at {root}."; look "On {branch}: {n} changed files." ("Detached: …" with no
    branch); diff "{n} files changed."; history "{n} commits."; show
    "Commit {s}: {n} files."; blame "{n} lines."; branches "{n} branches."; unmerged "{n} commits only on {branch}.";
    default_branch "The default branch is {b}." / "No default branch."; compare "{n} commits."; stashes "{n}
    stashes."; stash_diff "{n} files."; remotes "{n} remotes."; last_message "The last commit's message."; measure "{n}
    files, {bytes} bytes."; trash "{n} discards."; set_author "Set the author to {name} <{email}>."; stage / unstage
    "Staged {n} files." / "Unstaged {n} files."; stage_hunk / unstage_hunk "Staged a change to {path}." / "Unstaged a
    change to {path}."; commit "Committed {short}."; undo_commit "Took back {s}." / "{s} is pushed: revert it
    instead." / "A commit was made since {s}: only the last commit is taken back."; fetch "Fetched."; pull "Pulled.";
    push "Pushed."; push_branch "Pushed {branch}."; publish "Published {branch} to {remote}."; sync "Synced: got {g},
    sent {s}." / "Both sides have new commits: {a} here, {b} there." / "{branch} has no upstream: publish it."; switch
    "Switched to {branch}."; new_branch "Made {name} and switched to it."; rename_branch "Renamed {branch} to {name}.";
    merge "Merged {branch}."; checkout "Detached at {s}."; tag "Tagged {s} {name}."; revert "Reverted {s}."; stash
    "Stashed."; stash_apply / stash_pop / stash_drop "Applied stash@{n}." / "Popped stash@{n}." / "Dropped
    stash@{n}."; continue / abort "Continued the {operation}." / "Aborted the {operation}."; resolve "Took {side} for
    {path}."; resolve_region "Chose a part of {path}."; restore "Restored." / "{n} files changed since the discard:
    restore with force to write over them."; ignore "Ignored {path}."; message "Wrote a message."; discard,
    discard_hunk, restore_version: their `what` with a period.

- [ ] **Step 1: Write the failing tests** (`tests/ops.rs`; `home()` is `Home { trash: None, writer: None,
  global_author: false }`; `op(json)` is `serde_json::from_value::<Op>(json).unwrap()`):

```rust
#[test]
fn every_operation_has_a_name_and_parses_from_its_json() {
    // For each name in OPS, a minimal JSON (a table in the test giving each its required fields) parses, and
    // op.name() == name; {"op":"rm"} doesn't parse; OPS has no duplicate.
}

#[test]
fn the_reads_answer_through_run() {
    // fresh(); a.txt committed, then changed; branch "feature"; a stash of b.txt.
    // run(look).git is Done::Look with entries[0].path "a.txt" and text "On main: 1 changed file." …
    // diff → Diff (unstaged[0] a.txt); branches → 2 local; stashes → 1; remotes → []; default_branch → Some("main");
    // last_message; history → History(1 commit); show {hash: head}; blame {path: "a.txt"} → 1 line; unmerged
    // {branch: "feature"} → Count(0); compare {base: "main", branch: "feature"}; stash_diff {stash: {index: 0, hash}};
    // measure {paths: ["a.txt"]} → keep false (no trash); trash → []; find → root == repo.root.
}

#[test]
fn the_acts_run_through_run() {
    // stage {paths:["a.txt"]} → staged; unstage; commit {summary:"Two", all:true} → "Committed …."; new_branch
    // {name:"x"} then switch {branch:"main"}; rename_branch {branch:"x", name:"y"}; tag {name:"v1", hash}; checkout
    // {hash} (detached); switch {branch:"main"}; revert {hash}; stash {message:"wip", untracked:true}; stash_pop
    // {stash}; ignore {path:"b.txt"} (.gitignore holds "/b.txt"); set_author {name, email} writes the local config;
    // undo_commit {hash: short}; delete_branch {branch:"y", force:true}; stash then stash_drop.
}

#[test]
fn arguments_that_could_be_options_or_leave_the_repository_are_refused() {
    // Each is Err(Failure::Refused(words)) from run and Err(words) from check, git never run (HEAD and status
    // unchanged): stage paths ["../x"], ["/etc/passwd"], ["C:\\x"], [".git/config"], [".GIT/hooks/x"], [":(top)x"],
    // ["a//b"], []; new_branch name "-f"; tag name "--delete"; checkout hash "--orphan"; blame hash
    // "--contents=/etc/passwd"; show hash "HEAD"; merge branch "-X"; publish remote "--receive-pack=touch x";
    // restore entry "../x"; a summary with "\n"; 10,001 paths.
}

#[test]
fn names_that_aren_t_there_are_refused_before_git_runs() {
    // merge {branch:"nope"} → "There's no branch nope: branches lists them."; publish {remote:
    // "https://evil.example/x.git", branch:"main"} → "There's no remote https://evil.example/x.git: remotes lists
    // them."; push_branch {branch:"origin/main"} and delete_branch {branch:"origin/main"} (remote, not local) refused;
    // discard {paths:["clean.txt"]} → "clean.txt has no changes to discard."; resolve {path:"a.txt", side:"ours"} with
    // no conflict → "a.txt isn't in conflict."; continue → "No merge, rebase, cherry-pick or revert is in progress.".
}

#[cfg(unix)]
#[test]
fn a_write_through_a_link_leaving_the_repository_is_refused() {
    // a symlink "out" → another tempdir; restore_version {hash, then:"a.txt", path:"out/x"} →
    // "out/x lies outside the repository."; a symlink "l.txt" → a file outside, committed, then
    // restore_version {…, path:"l.txt"} → "l.txt is a link, which Templum doesn't write through."; nothing written
    // in the other tempdir.
}

#[test]
fn restore_takes_only_this_repository_s_discards() {
    // one trash; a discard in another repository → run(restore {entry: its id}) in this one with home.trash →
    // "There's no discard {id} of this repository: trash lists them."; without a trash → "This Templum keeps no
    // trash."; this repository's own entry restores.
}

#[test]
fn a_patch_must_be_one_hunk_of_a_listed_file_as_the_diff_shows_it() {
    // three_hunk-style file; hunk_patch(&file, 0) → stage_hunk passes and stages it. Refused, each with "The patch
    // isn't one hunk of one file, as the diff shows it." (or the state words): the patch with "old mode 100644\nnew
    // mode 100755\n" added after its first line; two hunks; "new file mode 120000"; a patch for "other.txt" not in
    // the status → "other.txt has no changes to stage."; unstage_hunk of an unstaged-only file → "a.txt has nothing
    // staged."; discard_hunk {path:"b.txt", patch: a.txt's} → refused.
}

#[test]
fn a_big_diff_is_cut_largest_files_first() {
    // Done::Diff(WorkingDiff { unstaged: three files with hunks of about 60, 30 and 10 KB of text }) and
    // fit(&mut done, <its JSON length minus 40 KB>) → Ok; the 60 KB file has no hunks and cut == true; the other two
    // keep theirs; every file row is still there.
}

#[test]
fn an_answer_too_large_to_cut_is_refused() {
    // Done::Branches(5,000 branches) with fit(&mut done, 1000) → Err(Refused(s)), s starts "The answer is too large
    // to send:".
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-git --test ops` → compile errors.
- [ ] **Step 3: Implement** `Op`, `OPS`, `check.rs`, `Home`, `Answer`, `Done`, `run`, `fit`.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-git` → all pass; `cargo clippy -j 4 -p templum-git --all-targets
  -- -D warnings` → clean.

---

### Task 5: The `git` tool in the registry, its answer, its audit line

**Files:**
- Modify: `crates/templum-api/Cargo.toml` (`templum-git = { path = "../templum-git" }`, commented "Templum's own:
  the `git` tool's operations (UI-free)")
- Modify: `crates/templum-api/src/lib.rs` (`pub fn computer() -> String`: `sysinfo::System::host_name()
  .unwrap_or_default()`; `server.rs`'s options default uses it)
- Modify: `crates/templum-api/src/registry.rs` (`OWN` gains `"git"`, 9; the `git` tool's schema in `tools()`;
  `GIT_READS`, `GIT_FULL`, `git_scope`; `scope` and `nominal` for `git`; `needs_words`; `Own::Git`; `parse_own`;
  `NOT_TARGETS` gains `all`, `amend`, `base`, `branch`, `email`, `entry`, `force`, `hash`, `how`,
  `ignore_whitespace`, `join`, `leave`, `message`, `op`, `patch`, `path`, `paths`, `region`, `remote`, `replaced`,
  `search`, `shown`, `side`, `skip`, `stash`, `summary`, `take`, `then`, `untracked`, `whole`, sorted in)
- Modify: `crates/templum-api/src/server.rs` (`tool()`: the scope refusal says `needs_words`; a `git` answer's
  response carries `Audited(format!("git:{op}"))` in its extensions; `route` writes that `what`; `tool_answer` copies
  `result["git"]` into the answer and an error's `result["details"]` into `{"error":{…,"details"}}`)
- Modify: `crates/templum-api/src/audit.rs` (`CHATTER` gains `"git:look"` and `"git:diff"`, 4)
- Modify: `crates/templum-api/src/openapi.rs` (`Error`'s `error` gains an optional `details` string; `ToolResult`
  gains `git` (any JSON, "the git tool's result"))
- Test: the same files' `#[cfg(test)]` modules

**Interfaces:**
- Consumes: `templum_git::ops::{Op, OPS}` (Task 4).
- Produces: `registry::git_scope(op: &str) -> Option<Scope>` (the Global Constraints' table: `GIT_READS` Read,
  `GIT_FULL` Full, any other name of `OPS` Control, else None); `scope("git", args)` is `git_scope(args["op"])`, an Err
  with the 400 words when missing or unknown; `nominal("git")` = `"read, control or full, by op"`;
  `registry::needs_words(name: &str, args: &Value, needs: Scope) -> String` ("git's discard needs a Full token." for
  `git`, else today's "{name} needs a {word} token."), used by `server::tool` and the glue's `api_call`;
  `Own::Git { workspace: String, folder: Option<String>, op: templum_git::ops::Op }`, parsed by `parse_own` (serde,
  then `op.check()`, its Err the 400 sentence). The tool's schema: `workspace` (string, required), `folder` (string:
  "A folder at home, whole or relative to the workspace's; its repository is used. Left out: the workspace's
  folder."), `op` (string, `enum` = `OPS`, required), and each operation argument with its type; description: "Runs
  one of the Git popup's operations in a repository of a workspace, as the popup does: op names it, its arguments go
  beside it, paths are relative to the repository's root. Reads take any token; acts take Control; discards, Restore
  version, deleting a branch, dropping a stash and aborting take Full. Never raw git arguments."

- [ ] **Step 1: Write the failing tests**

```rust
// registry.rs
#[test]
fn git_takes_the_scope_of_its_operation() {
    assert_eq!(scope("git", &json!({"op": "look"})), Ok(Scope::Read));
    assert_eq!(scope("git", &json!({"op": "stage", "paths": ["a"]})), Ok(Scope::Control));
    for op in ["discard", "discard_hunk", "restore_version", "delete_branch", "stash_drop", "abort"] {
        assert_eq!(git_scope(op), Some(Scope::Full), "{op}");
    }
    // Every name of templum_git::ops::OPS has a scope; exactly those six are Full and the 16 reads Read.
    assert_eq!(git_scope("rm"), None);
}

#[test]
fn an_unknown_git_operation_is_invalid() {
    let words = "There's no git operation rm: GET /v1/tools lists them.";
    assert_eq!(scope("git", &json!({"op": "rm"})), Err(words.into()));
    assert!(scope("git", &json!({})).unwrap_err().starts_with("git needs \"op\""));
}

#[test]
fn git_parses_into_its_operation_and_refuses_bad_shapes() {
    // parse_own("git", {"workspace":"w3","folder":"src","op":"stage","paths":["a.rs"]}) ==
    // Ok(Own::Git { workspace: "w3", folder: Some("src"), op: Op::Stage { paths: vec!["a.rs"] } });
    // {"workspace":"w3","op":"stage","paths":["../x"]} → Err starting "\"../x\" isn't a path inside the repository".
}

#[test]
fn needs_words_name_the_git_operation() {
    assert_eq!(needs_words("git", &json!({"op": "discard"}), Scope::Full), "git's discard needs a Full token.");
    assert_eq!(needs_words("send_keys", &json!({}), Scope::Full), "send_keys needs a Full token.");
}

// server.rs
#[test]
fn a_git_answer_carries_its_result_and_a_refusal_its_details() {
    // tool_answer of {"result":{"content":[{"type":"text","text":"Committed a1b2c3d."}],"git":{"short":"a1b2c3d",
    // "replaced":null}}} → 200, body {"text":"Committed a1b2c3d.","git":{"short":"a1b2c3d","replaced":null}};
    // of {"result":{"isError":true,"content":[{"type":"text","text":"fatal: x"}],"details":"fatal: x\nhint: y"}} →
    // 422, body {"error":{"code":"refused","message":"fatal: x","details":"fatal: x\nhint: y"}}.
}

#[test]
fn a_git_call_is_audited_by_its_operation() {
    // A response carrying Audited("git:push".into()) → the `what` route writes is "git:push"; one without → "git".
    // (Factor the choice as `fn audited(response: &Response<Body>, what: String) -> String` and test it.)
}

// audit.rs
#[test]
fn an_open_popup_s_git_reads_are_grouped_and_its_acts_are_not() {
    // write "git:look" three times and "git:diff" twice within a minute → one line each, more 2 and 1;
    // "git:push" twice → two lines.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum-api -- git_ an_unknown_git needs_words
  a_git_ an_open_popup` → compile errors.
- [ ] **Step 3: Implement** the items listed under Files.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum-api` → all pass, `every_argument_is_a_target_or_not`,
  `every_tool_is_known_with_a_schema`, `the_listed_scopes_are_the_nominal_ones` and the OpenAPI tests included.

---

### Task 6: Home runs `git`: the folder, the confinement, a queue per repository

**Files:**
- Create: `crates/templum/src/workspace/api_git.rs` (module doc; `GIT_DEADLINE`, `DETAILS_MOST`; `Workspace::{api_git,
  git_turn, git_turn_done}`; `repo_inside`; `git_line`, `git_refusal`)
- Modify: `crates/templum/src/workspace/mod.rs` (`mod api_git;`), `crates/templum/src/workspace/api.rs` (`Api` gains
  `git: HashMap<PathBuf, VecDeque<GitTurn>>`; `api_call` routes `Own::Git` to `api_git` and answers later (`Ok(None)`),
  its scope refusal through `registry::needs_words`; `api_allows` confines `git`'s `folder` as it does
  `add_block`'s and `open_agent`'s (`matches!(name, "add_block" | "open_agent" | "git")`, `opens` true for `git`);
  `api_own`'s match gains an `Own::Git` arm that refuses "git must go through api_git." as `TerminalInput`'s does),
  `crates/templum/src/workspace/git_message.rs` (`commit_writer`, Task 2)
- Test: `crates/templum/src/workspace/tests/api_git.rs` (new; registered in `tests/mod.rs`)

**Interfaces:**
- Consumes: Task 4's `ops::{Op, Home, Answer, Done, Failure, run}`, Task 1's `Repo::with_deadline`, Task 2's
  `commit_writer`, Task 5's `Own::Git`, `needs_words`, `templum_api::computer()`.
- Produces:
  - `type GitTurn = Box<dyn FnOnce(&mut Workspace, &mut Context<Workspace>)>`.
  - `Workspace::api_git(&mut self, caller: &Caller, id: &Value, reply: &Replier, workspace: String, folder:
    Option<String>, op: Op, cx: &mut Context<Self>) -> Result<(), Refusal>`: on the UI thread, `space_of(&workspace,
    true)` (a mirrored workspace refuses with its words), the folder (a limited token's comes from `api_allows`, real
    and confined; otherwise `launcher::expand`, relative to the workspace's folder, which must exist: "{folder} isn't a
    folder on this computer.", or with none named the workspace's folder, else "This workspace has no folder: name one
    with \"folder\"."), the token's roots (`Some(confined_roots(&seen))` for a limited token, else None), the program
    (`config.git.program()`) and `trash`; then **hop 1** on the background executor: `Repo::discover`; with none,
    `find` answers `Found { root: None, main_tree: None }` ("No repository at {folder}."), `init` runs `Repo::init`
    and answers the new root, anything else refuses "{folder} isn't in a Git repository: init makes one."; with roots,
    `repo_inside(&repo, &roots)` or `403` "That repository lies outside {workspace}'s folder."; `find` answers there.
    Back on the UI thread, `message` takes its writer (refused first with the glue's words when
    `config.git.commit_message.confirmed` is false or `commit_writer` is None), then `git_turn(repo.root, …)`
    runs **hop 2** on the background executor: `ops::run(&repo.with_deadline(now + GIT_DEADLINE), op, &Home { trash,
    writer, global_author: caller.workspaces.is_none() })`, then `reply.send` of `git_line` or `git_refusal`, then
    `git_turn_done`.
  - `Workspace::git_turn(&mut self, root: PathBuf, turn: GitTurn, cx)`: runs `turn` now when `api.git` has no entry
    for `root` (and makes an empty one), else queues it; `git_turn_done(&mut self, root: &Path, cx)`: pops and runs
    the next, or removes the entry. (The popup's own queue is untouched: if both act at once git's lock refusal is
    told in git's words.)
  - `fn repo_inside(repo: &Repo, roots: &[PathBuf]) -> bool`: both `repo.root` and `repo.main_tree()` (None, a bare
    repository: false) pass `refuses_share` first, then `dunce::canonicalize` lies under one of `roots`.
  - `fn git_line(id: &Value, workspace: &str, answer: &Answer) -> String`: `rpc::result` of `{"content":[{"type":
    "text","text":…}],"git":…,"target":{"workspace":…,"block":null}}`.
  - `fn git_refusal(id: &Value, failure: &Failure, computer: &str) -> String`: `{"isError":true,"content":[…],
    "details":…}`: a `Git` failure that `needs_input()` says the login words, one that is `cancelled()` the deadline's,
    any other `first_line()`, with `details` its stderr cut to `DETAILS_MOST`; an `Agent` failure its sentence and
    `details()`; the rest `sentence()` and no `details`.

- [ ] **Step 1: Write the failing tests** (`tests/api_git.rs`; `git_call(handle, ws, cx, token, args)` sends
  `tools/call` of `git` through `api_later`, then `settle(handle, cx)` and `try_recv` (a loop over `settle` with a
  10 s deadline if one round isn't enough); `folder_of(ws, cx, path)` sets the shown workspace's `folder`):

```rust
#[gpui_kit::test]
fn look_on_the_workspace_s_repository_answers_its_status(cx: &mut TestAppContext) {
    // committed_repo(), a.txt changed; folder_of(repo); a Read token for all workspaces;
    // git {workspace: w1, op: look} → said == (false, "On main: 1 changed file."),
    // result.git.status.entries[0].path == "a.txt", result.target.workspace == "w1".
}

#[gpui_kit::test]
fn stage_and_commit_through_the_api(cx: &mut TestAppContext) {
    // a Control token limited to w1: stage {paths:["a.txt"]} then commit {summary:"Two"} → "Committed {short}.";
    // git_run(repo, log -1 --format=%s) == "Two\n".
}

#[gpui_kit::test]
fn a_limited_token_s_repository_outside_its_folder_is_refused(cx: &mut TestAppContext) {
    // committed_repo() with a subfolder "sub"; folder_of(repo/sub); a Full token limited to w1;
    // git {workspace:w1, op:look} → refused(&line) == (Some(codes::SCOPE), "That repository lies outside {name}'s
    // folder."); a token for all workspaces gets the status.
}

#[gpui_kit::test]
fn a_worktree_whose_main_tree_is_outside_is_refused(cx: &mut TestAppContext) {
    // committed_repo() R; workspace folder W (another tempdir); `git worktree add W/wt` in R; a limited Full token,
    // folder "wt" → 403 with the same words.
}

#[gpui_kit::test]
fn a_control_token_can_t_discard_and_a_bad_path_is_invalid(cx: &mut TestAppContext) {
    // limited Control token: git {op:"discard", paths:["a.txt"]} → (Some(codes::SCOPE), "git's discard needs a Full
    // token."); git {op:"stage", paths:["../x"]} → Some(rpc::PARAMS); a.txt still changed.
}

#[gpui_kit::test]
fn a_failed_git_answers_its_first_line_and_its_stderr(cx: &mut TestAppContext) {
    // git {op:"checkout", hash:"deadbeef"} → isError, text == git's fatal line, result.details contains "deadbeef".
}

#[gpui_kit::test]
fn message_is_refused_until_home_asked_its_question(cx: &mut TestAppContext) {
    // a.txt staged; git {op:"message"} → said == (true, "Generate a message on {computer} once first: it asks there
    // whether the diff may go to the agent's provider.") with computer == templum_api::computer().
}

#[gpui_kit::test]
fn git_on_a_mirrored_workspace_is_refused(cx: &mut TestAppContext) {
    // a remote workspace r1 (tests/remote.rs's helper) → git {workspace:"r1", op:"look"} answers r1's mirror words
    // and runs nothing.
}

#[gpui_kit::test]
fn a_folder_without_a_repository_finds_none_and_init_makes_one(cx: &mut TestAppContext) {
    // folder_of(an empty tempdir); find → result.git.root == null; look → "{folder} isn't in a Git repository: init
    // makes one."; init → result.git.root is the folder (canonical); find now answers it.
}

#[gpui_kit::test]
fn two_calls_on_one_repository_answer_in_turn(cx: &mut TestAppContext) {
    // stage {paths:["a.txt"]} and commit {summary:"Two"} sent back to back with api_later (conns 901, 902) before any
    // settle → both answered after settling, the commit "Committed …" and holding a.txt; ws.api.git is empty after.
}
```

- [ ] **Step 2: Run them to see them fail:** `cargo test -j 4 -p templum api_git` → compile errors.
- [ ] **Step 3: Implement** `api_git.rs` and the `api.rs` changes.
- [ ] **Step 4: Run** `cargo test -j 4 -p templum api_git` → pass; then
  `cargo test -j 4 -p templum workspace::tests::api`
  → every existing API test passes.

---

### Task 7: Docs, minors, the full check and the bench

**Files:**
- Docs: `docs/api.md` (Scopes: control gains "Git's acts (`git`: stage, commit, push, branches, stashes…)", full gains
  "Git's discards, Restore version, deleting a branch, dropping a stash and aborting (`git`)"; Tools and aliases: the
  `git` row "`workspace`, `folder`, `op` and its arguments | read, control or full, by op" and nine for "the eight only
  the API has"; a paragraph **Git** after **Placing and naming**: the operations table with their arguments and scopes
  (the "Interfaces for later parts" table, shortened), paths relative to the repository, the answer `{"text","git"}`,
  a refusal's `details`, the 8 MiB cut and `cut`, a limited token's confinement (root and main tree), one call at a
  time per repository, the 10-minute stop, `message`'s first-use rule, `set_author` writing the repository's config
  for a limited token; Errors: "a 422 of `git` adds `details`, git's stderr (at most 64 KiB)"; the audit log: "`git`
  is written `git:<op>`, and `git:look` and `git:diff` are grouped as `terminal_input` is"),
  `docs/specs/api.md` (the API's own tools table gains `git`; a bullet "**`git`** (Templum to Templum, part 2f)" with
  the scopes, the confinement and the answer), `docs/specs/templum-to-templum.md` (Status: parts 2a–2f built (plan
  …-2f-git-home); "As built ▸ Batch 2f": what the builder chose or changed, and this plan's "Open for the
  orchestrator" as settled), `docs/ARCHITECTURE.md` (`templum-git`: "the Git popup's operations as one enum the popup
  and the API's `git` tool share (`ops/`)"; `templum-api` depends on `templum-git`; the glue's
  `workspace/api_git.rs`), `docs/minors.md`
- [ ] **Step 1: Write the docs** above.
- [ ] **Step 2: The full check, once, nothing else running:** `cargo fmt --all && cargo clippy -j 4 --workspace
  --all-targets -- -D warnings && cargo test -j 4 --workspace` → all pass; `cargo deny check` → ok; the reduced-env run
  `env -i PATH=/home/winx/.cargo/bin:/usr/bin:/bin HOME=$(mktemp -d) CARGO_HOME=/home/winx/.cargo
  RUSTUP_HOME=/home/winx/.rustup TERM=dumb cargo test -j 4 --workspace --no-fail-fast` → all pass.
- [ ] **Step 3: The bench** (the popup's jobs moved): `cargo run -j 4 --release -p templum -- --bench-git` →
  `p95_ms` ≤ 16.7; report its JSON line.
- [ ] **Step 4: Leave uncommitted;** report the files changed, the checks' and the bench's results, what went to
  `docs/minors.md`, and confirm no existing git test was edited (`git diff --stat -- crates/templum/src/workspace/tests/
  crates/templum/src/git_block crates/templum-git/tests/repo.rs` shows only Task 1's new tests in `tests/repo.rs`).

---

## Interfaces for later parts

What 2g (the mirror's Git popup) calls, by name. All in `templum_git::ops` unless said; the mirror deserializes an
answer's `git` into the type the table names (the same crate as home's).

**The call:** `POST /v1/tools/git` (through the client's `Mirror::call`, `Call { tool: "git", args, slow }`), args
`{"workspace": <home's id>, "folder": <home's path, or left out>, "op": <name>, …the operation's own}`. The client
already caps `git` answers at 8 MiB (2a). Which operations go on the slow lane is 2g's choice; the long ones are
`fetch`, `pull`, `push`, `push_branch`, `publish`, `sync`, `message`. Scope per operation:
`templum_api::registry::git_scope(op)` (the mirror's rule dims with it; home's `403` wins if they differ).

| op | arguments (`?` optional) | `git` in the answer |
|---|---|---|
| `find` | – | `Found { root: Option<String>, main_tree: Option<String> }` (root null: no repository) |
| `look` | – | `Look { status: Status, operation: Option<Operation>, author: Option<(String, String)> }` |
| `diff` | `ignore_whitespace?`, `whole?: [{staged, path}]` | `WorkingDiff` (a conflicted file: `conflict_diff`'s) |
| `history` | `all?`, `search?: {by, text}` (`by`: `message`, `author`, `code`), `path?`, `skip?` | `Page` (200 each) |
| `show` | `hash`, `ignore_whitespace?` | `CommitShown { commit, files, diffs }` |
| `blame` | `path`, `hash?` | `Vec<BlameLine>` |
| `branches` | – | `Vec<Branch>` |
| `unmerged` | `branch` (local) | `u32` |
| `default_branch` | – | `Option<String>` |
| `compare` | `base`, `branch`, `hash?` (narrowed), `ignore_whitespace?` | `Compared { commits, files }` |
| `stashes` | – | `Vec<Stash>` |
| `stash_diff` | `stash: {index, hash}`, `ignore_whitespace?` | `Vec<FileDiff>` |
| `remotes` | – | `Vec<String>` |
| `last_message` | – | `String` |
| `measure` | `paths` | `Measured { files, bytes, keep }` (the discard question's words) |
| `trash` | – | `Vec<Kept>` (this repository's) |
| `init` | – | `Found` |
| `set_author` | `name`, `email` | `null` (a share writes the repository's config) |
| `stage`, `unstage` | `paths` | `null` |
| `stage_hunk`, `unstage_hunk` | `patch` (`hunk_patch` of the shown file) | `null` |
| `commit` | `summary` (blank: the staged file's sentence), `description?`, `amend?`, `all?` | `Committed` |
| `undo_commit` | `hash` (the short one committed), `replaced?` | `Undone` (`{"done": msg}`, `"pushed"`, `"moved"`) |
| `fetch`, `push` | – | `null` |
| `pull` | `how?: "fast_forward"\|"merge"\|"rebase"` | `null` |
| `push_branch` | `branch` (local) | `null` |
| `publish` | `remote` (listed), `branch` (local) | `null` |
| `sync` | `join?: true` (rebase) or `false` (merge) | `Synced` |
| `switch` | `branch`, `remote?` (an `origin/x`), `leave?` (the stash's name for changes left) | `Back` |
| `new_branch` | `name`, `hash?` | `null` |
| `rename_branch` | `branch`, `name` | `null` |
| `merge` | `branch` | `null` |
| `checkout` | `hash` | `null` |
| `tag` | `name`, `hash` | `null` |
| `revert` | `hash` | `null` |
| `stash` | `message`, `untracked?` | `null` |
| `stash_apply`, `stash_pop` | `stash` | `null` |
| `continue` | – | `null` |
| `resolve` | `path`, `side: "ours"\|"theirs"` | `null` |
| `resolve_region` | `path`, `region`, `take: "first"\|"second"\|"both"`, `shown: [[lines], [lines]]` | `null` |
| `restore` | `entry` (a `Kept::id` of `trash`), `force?` | `Restored` (`"done"` or `{"changed": [paths]}`) |
| `ignore` | `path` | `null` |
| `message` | `all?` | `Message { summary, description, committed }` (committed: a commit came meanwhile) |
| `discard` | `paths` | `Option<Kept>` (the copy for Undo) |
| `discard_hunk` | `path`, `patch` | `Option<Kept>` |
| `restore_version` | `hash`, `then` (its path in that commit), `path` | `Option<Kept>` |
| `delete_branch` | `branch` (local), `force?` | `null` |
| `stash_drop` | `stash` | `null` |
| `abort` | – | `null` |

**Answers and errors:** `200 {"text": "…", "git": …, "workspace": "w3"}`; `400 invalid` for an unknown operation or a
malformed argument (the shape words above); `403 scope` for the operation's scope or "That repository lies outside
{workspace}'s folder."; `422 refused` with `message` (git's first line, a state refusal, the login, deadline, first-use
and no-agent words) and `details` (git's stderr, at most 64 KiB) when git failed; `404` for a workspace the token
doesn't see. A `FileDiff` with `cut: true` had its hunks dropped to fit 8 MiB ("Too large to show from winx-desktop."
is 2g's to say). Generate runs at home with home's agent; the mirror's agent is never asked.

**Also left for 2g and later:** `ops::{look, commit, undo_commit, sync, compare, history, show, discard, discarded,
restore_version, resolve_file, resolve_region}` and `message::{Writer, generate}` are the popup's jobs: a popup that
runs on a remote replaces each job's closure with the matching `git` call and keeps its `done` closure;
`ops::Failure::sentence`; `Workspace::commit_writer`; the API's queue (`Api::git`, `git_turn`).

## Open for the orchestrator

Each decided with the recommended option; the plan is written that way.

1. **The popup calls each job's own function in `ops`, not `ops::run`.** `run` is the API's door: it checks
   untrusted arguments (extra git calls: branches, remotes, status) and answers JSON-shaped `Done`; the popup's
   arguments come from git itself and its closures want typed results. Both reach the same function, so they can't
   drift. The spec's `ops::run(repo, op, trash)` becomes `run(repo, op, &Home { trash, writer, global_author })`
   (Generate needs home's writer, Set… whether to write globally). *Recommended.*
2. **Operations follow the jobs, not the spec's list word for word:** `conflict` is folded into `diff` (a conflicted
   file's regions come in the working diff, as the popup reads them), `stage_all` into `commit`'s `all`,
   `mark_resolved` into `stage` (the popup stages), `file_history` into `history`'s `path`; added because the popup
   runs them as jobs: `unmerged`, `default_branch`, `last_message`, `measure`, `push_branch`, `resolve_region`,
   `stash_pop`. `clone` stays out (batch 2 leaves Clone out). *Recommended.*
3. **`set_author` through a limited token writes the repository's own config,** not the global one: home's global
   identity would change every repository at home, outside the share's folders. A token for all workspaces writes
   globally, as Set… does. *Recommended.*
4. **A hunk is accepted only as `hunk_patch` draws it** (one file, one hunk, byte for byte, a file the status lists):
   no mode, symlink, binary or rename header can be applied through a share. *Recommended.*
5. **Every API git call is killed after 10 minutes** (`Repo::with_deadline`), so a git hung on a network or a prompt
   can't hold its repository's API queue forever; the client gives up at 10 minutes too. *Recommended.*
6. **The queue per repository root lives on the UI thread** (as the popup's), jobs on the background executor: no
   thread blocks on a lock, and the tests settle it with `run_until_parked`. *Recommended.*
7. **`git:diff` is grouped in the audit log with `git:look`:** an open popup reads the working diff after every status
   read, so both come every 2 s. *Recommended.*
8. **`resolve` and `resolve_region` stay Control,** as the spec's table says, though a whole-file choice overwrites
   hand edits: like the popup they keep a copy in the trash when it fits (500 files, 100 MB). Making them Full would
   dim conflict resolution on every Control share. *Recommended.*
9. **A discard through the API keeps a copy whenever the trash fits, without asking:** the mirror's popup asks first,
   with `measure`'s numbers. *Recommended.*
10. **`message`'s answer carries `committed`** instead of refusing when a commit came meanwhile: 2g decides as the
    popup does ("Left out … a commit was made while it was writing"). *Recommended.*
11. **`FileDiff` gains `cut`** rather than reusing `too_large`: the local panel offers "show anyway" for a large file,
    which can't help a file cut for the network. *Recommended.*
12. **`templum-api` depends on `templum-git`** (Templum's own, UI-free): one parse of `Op` at the server (`400` before
    the UI thread, as every own tool) and one list of operation names for the scopes. *Recommended.*
13. **Shape refusals are `400`, state refusals `422`, confinement `403`:** what the registry can tell from the
    arguments alone is a malformed argument; what needs the repository is the tool refusing. *Recommended.*
14. **The computer's name** in the glue's words comes from a new `templum_api::computer()` (the name `GET /v1/me`
    gives), so a mirror shows home's sentences as they are. *Recommended.*

## Found against the code as built

- **The popup's jobs are closures, mostly one `Repo` call each** (`repo.branches()`, `repo.stage(..)`): only about a
  dozen have bodies of their own (the status read, commit, undo commit, sync, compare, a History page, a commit shown,
  discard, the trash list, Restore version, the two conflict choices, Generate). Those move; for the others `run`
  calls the same `Repo` method, which is already the shared function.
- **`Trash::restore(id)` joins `id` into the trash's folder and writes wherever the manifest there says** (and
  `remove_dir_all`s it): through an API an id is an arbitrary path, so `restore` takes only an id `Trash::list()`
  gives for this repository's root (Review Focus 2).
- **Several `Repo` methods put a caller's string where git reads options** (`branch -d <name>`, `tag <name> <hash>`,
  `checkout -q <name> --`, `merge --no-edit <name>`, `blame --porcelain <rev> --`, `diff-tree … <hash>`,
  `push -u <remote> <branch>`): safe for the popup, whose names come from git, not for a share; hence Task 4's rules
  before any of them runs. `publish`'s remote could even be a URL or a local path with `--receive-pack`.
- **`resolve_region` and `restore_version` write with `std::fs::write`,** which follows a symbolic link: a link inside
  the repository pointing outside would let a write leave it, hence `inside` and the link refusal for written paths.
- **`Repo::set_author` ran `git config --global` in Templum's own working folder (`.`),** outside any workspace; it now
  runs in the repository's root.
- **The status read the API serves is `Read` in `crates/templum`** (`git_block::Read::of`); it becomes `ops::Look`, so
  2g's popup reads the same struct from home.
- **`git_block::PAGE`, `words::{generic_summary, staged_summary}`, `join_message` and `thousands`** live in
  `crates/templum`; the commit and discard bodies need them, so they move to `ops` and the popup re-exports them,
  keeping its tests unedited.
- **`api_allows` returns early for a token for all workspaces,** so the glue resolves such a token's folder itself; a
  limited token's folder is confined by `api_allows`, as `add_block`'s is.
- **A planted `.git` file naming a network share is read by git before Templum can look at the repository's root**
  (Windows would offer the person's credentials to that share). The share-prefix check runs on git's answers, as part
  1's `main_tree_inside` does today; only something already able to write the workspace's folder (not a share's `git`
  calls, whose paths can't name `.git`) could plant one. Not changed here.
