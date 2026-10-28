//! The Git popup's jobs and the API's operations, on a real repository in a temporary folder.
//! CI's runners have `git`.

use std::path::Path;

use serde_json::{Value, json};
use templum_git::conflict::{Region, Take, regions};
use templum_git::ops::{self, Discarding, Done, Failure, Home, Op, Undone};
use templum_git::trash::Trash;
use templum_git::{HistoryQuery, Repo, Side, Synced, hunk_patch};

/// A new repository on `main` with an author set, so commits work on a bare runner.
fn fresh() -> (tempfile::TempDir, Repo) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repo::init("git", dir.path()).expect("git init");
    for args in [
        ["symbolic-ref", "HEAD", "refs/heads/main"].as_slice(),
        &["config", "user.name", "Ada Lovelace"],
        &["config", "user.email", "ada@example.com"],
        &["config", "commit.gpgsign", "false"],
        // Files as written: a runner's core.autocrlf would turn restored files into CRLF.
        &["config", "core.autocrlf", "false"],
    ] {
        repo.run(args).unwrap();
    }
    (dir, repo)
}

fn write(root: &Path, name: &str, text: &str) {
    if let Some(parent) = Path::new(name).parent() {
        std::fs::create_dir_all(root.join(parent)).unwrap();
    }
    std::fs::write(root.join(name), text).unwrap();
}

fn git(repo: &Repo, args: &[&str]) -> String {
    String::from_utf8(repo.run(args).unwrap()).unwrap()
}

#[test]
fn look_reads_the_status_the_operation_and_the_author() {
    let (dir, repo) = fresh();
    write(dir.path(), "a.txt", "one\n");
    let look = ops::look(&repo).unwrap();
    assert_eq!(look.status.entries[0].path, "a.txt");
    assert_eq!(look.operation, None);
    assert_eq!(
        look.author,
        Some(("Ada Lovelace".to_string(), "ada@example.com".to_string()))
    );
}

#[test]
fn commit_with_no_summary_names_the_one_staged_file() {
    let (dir, repo) = fresh();
    write(dir.path(), "a.txt", "one\n");
    repo.stage(&["a.txt"]).unwrap();
    let c = ops::commit(&repo, "", "", false, false).unwrap().unwrap();
    assert_eq!(c.replaced, None);
    assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "Add a.txt\n");
    assert_eq!(
        c.short,
        git(&repo, &["rev-parse", "--short", "HEAD"]).trim()
    );
}

#[test]
fn commit_with_no_summary_and_two_staged_files_waits_for_a_message() {
    let (dir, repo) = fresh();
    write(dir.path(), "a.txt", "one\n");
    write(dir.path(), "b.txt", "two\n");
    repo.stage(&["a.txt", "b.txt"]).unwrap();
    assert_eq!(ops::commit(&repo, "", "", false, false).unwrap(), None);
    // Nothing was committed: no commit is reachable.
    assert!(git(&repo, &["rev-list", "--all"]).is_empty());
}

#[test]
fn commit_all_stages_every_change_first_and_joins_the_description() {
    let (dir, repo) = fresh();
    write(dir.path(), "a.txt", "one\n");
    repo.stage(&["a.txt"]).unwrap();
    repo.commit("First", false).unwrap();
    write(dir.path(), "a.txt", "two\n");
    write(dir.path(), "b.txt", "new\n");
    ops::commit(&repo, "Both", "Why", false, true)
        .unwrap()
        .unwrap();
    assert_eq!(git(&repo, &["log", "-1", "--format=%B"]), "Both\n\nWhy\n\n");
    assert!(repo.status().unwrap().entries.is_empty());
}

#[test]
fn an_amend_remembers_the_commit_it_replaced_and_undo_goes_back_to_it() {
    let (dir, repo) = fresh();
    write(dir.path(), "a.txt", "one\n");
    repo.stage(&["a.txt"]).unwrap();
    repo.commit("One", false).unwrap();
    let old = repo.head().unwrap();
    write(dir.path(), "a.txt", "two\n");
    repo.stage(&["a.txt"]).unwrap();
    let c = ops::commit(&repo, "One, amended", "", true, false)
        .unwrap()
        .unwrap();
    assert_eq!(c.replaced.as_deref(), Some(old.as_str()));
    let undone = ops::undo_commit(&repo, &c.short, c.replaced.as_deref()).unwrap();
    assert_eq!(undone, Undone::Done("One, amended".to_string()));
    assert_eq!(repo.head().unwrap(), old);
}

#[test]
fn undo_commit_of_a_commit_no_longer_last_is_moved() {
    let (dir, repo) = fresh();
    write(dir.path(), "a.txt", "one\n");
    repo.stage(&["a.txt"]).unwrap();
    let first = repo.commit("One", false).unwrap();
    write(dir.path(), "a.txt", "two\n");
    repo.stage(&["a.txt"]).unwrap();
    repo.commit("Two", false).unwrap();
    let head = repo.head().unwrap();
    assert_eq!(
        ops::undo_commit(&repo, &first, None).unwrap(),
        Undone::Moved
    );
    assert_eq!(repo.head().unwrap(), head);
}

#[test]
fn sync_without_an_upstream_is_unpublished() {
    let (dir, repo) = fresh();
    write(dir.path(), "a.txt", "one\n");
    repo.stage(&["a.txt"]).unwrap();
    repo.commit("One", false).unwrap();
    assert_eq!(ops::sync(&repo, None).unwrap(), Synced::Unpublished);
}

#[cfg(unix)]
#[test]
fn generate_runs_the_writer_and_says_whether_a_commit_came_meanwhile() {
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;
    use templum_git::message::{self, Writer};
    let (dir, repo) = fresh();
    write(dir.path(), "a.txt", "one\n");
    repo.stage(&["a.txt"]).unwrap();
    let writer = Writer {
        command: "printf 'Add a\\n\\nBecause'".into(),
        model: String::new(),
        env: Vec::new(),
        instructions: String::new(),
        template: message::PROMPT.into(),
        max_bytes: 65536,
        timeout: Duration::from_secs(10),
    };
    let (answer, moved) = message::generate(&repo, &writer, false, &AtomicBool::new(false));
    assert!(!moved);
    let answer = answer.unwrap();
    assert_eq!(
        message::split(&answer),
        Some(("Add a".to_string(), "Because".to_string()))
    );
}

fn commit(repo: &Repo, root: &Path, name: &str, text: &str, message: &str) {
    write(root, name, text);
    repo.stage(&[name]).unwrap();
    repo.commit(message, false).unwrap();
}

/// `main` has a.txt "ours\n", `feature` "theirs\n", both from "one\n", and the merge stopped.
fn conflicted() -> (tempfile::TempDir, Repo) {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    repo.run(&["checkout", "-q", "-b", "feature"]).unwrap();
    commit(&repo, dir.path(), "a.txt", "theirs\n", "Theirs");
    repo.run(&["checkout", "-q", "main"]).unwrap();
    commit(&repo, dir.path(), "a.txt", "ours\n", "Ours");
    assert!(repo.run(&["merge", "feature"]).is_err());
    (dir, repo)
}

fn trash() -> (tempfile::TempDir, Trash) {
    let dir = tempfile::tempdir().unwrap();
    let trash = Trash::new(dir.path().join("trash"));
    (dir, trash)
}

fn read(root: &Path, name: &str) -> String {
    std::fs::read_to_string(root.join(name)).unwrap()
}

#[test]
fn discard_keeps_a_copy_then_throws_the_changes_away() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    write(dir.path(), "a.txt", "two\n");
    write(dir.path(), "b.txt", "new\n");
    let (_keep, trash) = trash();
    let status = repo.status().unwrap();
    let entries: Vec<_> = status.unstaged().into_iter().cloned().collect();
    let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    let kept = ops::discard(
        &repo,
        Some(&trash),
        &paths,
        "Discarded 2 files",
        &Discarding::Files(entries),
    )
    .unwrap()
    .unwrap();
    assert_eq!(kept.what, "Discarded 2 files");
    assert_eq!(read(dir.path(), "a.txt"), "one\n");
    assert!(!dir.path().join("b.txt").exists());
    assert_eq!(ops::discarded(&trash, &repo.root), [kept]);
}

#[test]
fn a_discard_whose_copy_fails_discards_nothing() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    write(dir.path(), "a.txt", "two\n");
    let keep = tempfile::tempdir().unwrap();
    // The trash's folder is a file: nothing can be put in it.
    std::fs::write(keep.path().join("trash"), "x").unwrap();
    let trash = Trash::new(keep.path().join("trash"));
    let status = repo.status().unwrap();
    let entries: Vec<_> = status.unstaged().into_iter().cloned().collect();
    let result = ops::discard(
        &repo,
        Some(&trash),
        &["a.txt".to_string()],
        "Discarded a.txt",
        &Discarding::Files(entries),
    );
    assert!(matches!(result, Err(Failure::Keep(_))));
    assert_eq!(read(dir.path(), "a.txt"), "two\n");
}

#[test]
fn discard_hunk_reverses_one_hunk_on_the_working_tree() {
    let (dir, repo) = fresh();
    let lines = |two: &str, last: &str| {
        (1..=30)
            .map(|i| match i {
                2 => format!("{two}\n"),
                28 => format!("{last}\n"),
                i => format!("line {i}\n"),
            })
            .collect::<String>()
    };
    commit(
        &repo,
        dir.path(),
        "a.txt",
        &lines("line 2", "line 28"),
        "One",
    );
    write(dir.path(), "a.txt", &lines("changed 2", "changed 28"));
    let status = repo.status().unwrap();
    let files = repo.working_diff(&status, false, &[]).unwrap().unstaged;
    assert_eq!(files[0].hunks.len(), 2);
    let patch = hunk_patch(&files[0], 0).unwrap();
    let kept = ops::discard(
        &repo,
        None,
        &["a.txt".to_string()],
        "Discarded a change to a.txt",
        &Discarding::Hunk(patch),
    )
    .unwrap();
    assert_eq!(kept, None);
    assert_eq!(read(dir.path(), "a.txt"), lines("line 2", "changed 28"));
}

#[test]
fn discarded_lists_only_this_repository_s_discards() {
    let (_keep, trash) = trash();
    let mut roots = Vec::new();
    for _ in 0..2 {
        let (dir, repo) = fresh();
        commit(&repo, dir.path(), "a.txt", "one\n", "One");
        write(dir.path(), "a.txt", "two\n");
        let entries: Vec<_> = repo
            .status()
            .unwrap()
            .unstaged()
            .into_iter()
            .cloned()
            .collect();
        ops::discard(
            &repo,
            Some(&trash),
            &["a.txt".to_string()],
            "Discarded a.txt",
            &Discarding::Files(entries),
        )
        .unwrap();
        roots.push((dir, repo));
    }
    let mine = ops::discarded(&trash, &roots[0].1.root);
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0].root, roots[0].1.root);
}

#[test]
fn restore_version_writes_the_file_as_it_was_and_keeps_a_copy() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "A");
    let a = repo.head().unwrap();
    commit(&repo, dir.path(), "a.txt", "two\n", "B");
    let (_keep, trash) = trash();
    let kept = ops::restore_version(
        &repo,
        Some(&trash),
        &a,
        "a.txt",
        "a.txt",
        "Restored a.txt from a1b2c3d",
    )
    .unwrap()
    .unwrap();
    assert_eq!(read(dir.path(), "a.txt"), "one\n");
    trash.restore(&kept.id, false).unwrap();
    assert_eq!(read(dir.path(), "a.txt"), "two\n");
}

#[test]
fn resolve_file_takes_one_side_after_a_copy() {
    let (dir, repo) = conflicted();
    let (_keep, trash) = trash();
    ops::resolve_file(&repo, Some(&trash), "a.txt", Side::Theirs).unwrap();
    assert_eq!(read(dir.path(), "a.txt"), "theirs\n");
    assert_eq!(ops::discarded(&trash, &repo.root).len(), 1);
}

#[test]
fn resolve_region_keeps_the_part_chosen_and_refuses_a_changed_region() {
    let (dir, repo) = conflicted();
    let text = read(dir.path(), "a.txt");
    let r = &regions(&text)[0];
    let shown = (
        Region::lines(&text, &r.first),
        Region::lines(&text, &r.second),
    );
    let stale = (vec!["x".to_string()], Vec::new());
    let result = ops::resolve_region(&repo, None, "a.txt", 0, Take::First, &stale);
    assert!(matches!(result, Err(Failure::Changed)));
    assert_eq!(read(dir.path(), "a.txt"), text);
    ops::resolve_region(&repo, None, "a.txt", 0, Take::First, &shown).unwrap();
    assert_eq!(read(dir.path(), "a.txt"), "ours\n");
}

#[test]
fn compare_lists_the_branch_s_commits_and_its_whole_or_narrowed_diff() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    repo.run(&["checkout", "-q", "-b", "feature"]).unwrap();
    commit(&repo, dir.path(), "b.txt", "b\n", "Add b");
    commit(&repo, dir.path(), "c.txt", "c\n", "Add c");
    let newest = repo.head().unwrap();
    let all = ops::compare(&repo, "main", "feature", None, false).unwrap();
    assert_eq!(all.commits.len(), 2);
    let paths: Vec<_> = all.files.iter().map(|f| f.path().to_string()).collect();
    assert_eq!(paths, ["b.txt", "c.txt"]);
    let one = ops::compare(&repo, "main", "feature", Some(&newest), false).unwrap();
    let paths: Vec<_> = one.files.iter().map(|f| f.path().to_string()).collect();
    assert_eq!(paths, ["c.txt"]);
}

#[test]
fn history_pages_and_reads_unpushed_on_the_first_page_only() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    commit(&repo, dir.path(), "a.txt", "two\n", "Two");
    repo.run(&["mv", "a.txt", "b.txt"]).unwrap();
    repo.commit("Rename", false).unwrap();
    let q = HistoryQuery::default();
    let first = ops::history(&repo, &q, None, 0).unwrap();
    assert_eq!(first.commits.len(), 3);
    assert_eq!(first.unpushed, Some(Default::default()));
    let later = ops::history(&repo, &q, None, 1).unwrap();
    assert_eq!(later.commits.len(), 2);
    assert_eq!(later.unpushed, None);
    // b.txt's history follows it back to a.txt.
    let file = ops::history(&repo, &q, Some("b.txt"), 0).unwrap();
    assert_eq!(file.commits.len(), 3);
    assert_eq!(file.paths[1].path, "a.txt");
}

#[test]
fn show_reads_a_commit_its_files_and_its_diff() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    let head = repo.head().unwrap();
    let shown = ops::show(&repo, &head, None, false).unwrap();
    assert_eq!(shown.commit.hash, head);
    assert_eq!(shown.files[0].path, "a.txt");
    assert_eq!(shown.diffs.len(), 1);
}

// The API's door: `Op`, its checks, `run` and `fit`.

fn home() -> Home<'static> {
    Home {
        trash: None,
        writer: None,
        global_author: false,
    }
}

fn op(value: Value) -> Op {
    serde_json::from_value(value).unwrap()
}

fn go(repo: &Repo, value: Value) -> Result<ops::Answer, Failure> {
    ops::run(repo, op(value), &home())
}

/// What `run` refused with, which must be the words of a refusal (never git's).
fn words(result: Result<ops::Answer, Failure>) -> String {
    match result {
        Err(Failure::Refused(why)) => why,
        Err(other) => panic!("expected a refusal, got {other:?}"),
        Ok(a) => panic!("expected a refusal, got {:?}", a.text),
    }
}

fn json_of(done: &Done) -> Value {
    serde_json::to_value(done).unwrap()
}

fn minimal(name: &str) -> Value {
    let hash = "abcd1234";
    let stash = json!({"index": 0, "hash": hash});
    let extra = match name {
        "show" | "checkout" | "revert" => json!({"hash": hash}),
        "blame" | "ignore" => json!({"path": "a.txt"}),
        "unmerged" | "push_branch" | "merge" | "delete_branch" | "switch" => {
            json!({"branch": "main"})
        }
        "compare" => json!({"base": "main", "branch": "feature"}),
        "stash_diff" | "stash_apply" | "stash_pop" | "stash_drop" => json!({"stash": stash}),
        "measure" | "stage" | "unstage" | "discard" => json!({"paths": ["a.txt"]}),
        "set_author" => json!({"name": "Ada", "email": "ada@example.com"}),
        "stage_hunk" | "unstage_hunk" => json!({"patch": "x"}),
        "undo_commit" => json!({"hash": hash}),
        "publish" => json!({"remote": "origin", "branch": "main"}),
        "new_branch" => json!({"name": "x"}),
        "rename_branch" => json!({"branch": "x", "name": "y"}),
        "tag" => json!({"name": "v1", "hash": hash}),
        "stash" => json!({"message": "wip"}),
        "resolve" => json!({"path": "a.txt", "side": "ours"}),
        "resolve_region" => {
            json!({"path": "a.txt", "region": 0, "take": "first", "shown": [[], []]})
        }
        "restore" => json!({"entry": "17-3"}),
        "discard_hunk" => json!({"path": "a.txt", "patch": "x"}),
        "restore_version" => json!({"hash": hash, "then": "a.txt", "path": "a.txt"}),
        _ => json!({}),
    };
    let mut value = extra;
    value["op"] = json!(name);
    value
}

#[test]
fn every_operation_has_a_name_and_parses_from_its_json() {
    let mut seen = std::collections::HashSet::new();
    for name in ops::OPS {
        assert!(seen.insert(*name), "{name} twice");
        let parsed: Op = serde_json::from_value(minimal(name))
            .unwrap_or_else(|e| panic!("{name} doesn't parse: {e}"));
        assert_eq!(parsed.name(), *name);
        // Unknown keys beside it (the API's workspace and folder) are ignored.
        let mut more = minimal(name);
        more["workspace"] = json!("w1");
        more["folder"] = json!("src");
        assert_eq!(serde_json::from_value::<Op>(more).unwrap(), parsed);
    }
    assert!(serde_json::from_value::<Op>(json!({"op": "rm"})).is_err());
}

fn repo_with_work() -> (tempfile::TempDir, Repo) {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    repo.run(&["branch", "feature"]).unwrap();
    write(dir.path(), "a.txt", "two\n");
    write(dir.path(), "b.txt", "new\n");
    repo.stash_push("wip", true).unwrap();
    write(dir.path(), "a.txt", "three\n");
    (dir, repo)
}

#[test]
fn the_reads_answer_through_run() {
    let (_dir, repo) = repo_with_work();
    let a = go(&repo, json!({"op": "look"})).unwrap();
    assert_eq!(a.text, "On main: 1 changed file.");
    let Done::Look(look) = &a.git else { panic!() };
    assert_eq!(look.status.entries[0].path, "a.txt");
    let a = go(&repo, json!({"op": "diff"})).unwrap();
    assert_eq!(a.text, "1 file changed.");
    let Done::Diff(diff) = &a.git else { panic!() };
    assert_eq!(diff.unstaged[0].path(), "a.txt");
    let a = go(&repo, json!({"op": "branches"})).unwrap();
    assert_eq!(a.text, "2 branches.");
    let a = go(&repo, json!({"op": "stashes"})).unwrap();
    assert_eq!(a.text, "1 stash.");
    let Done::Stashes(stashes) = &a.git else {
        panic!()
    };
    let stash = json!({"index": stashes[0].index, "hash": stashes[0].hash});
    assert_eq!(
        go(&repo, json!({"op": "remotes"})).unwrap().text,
        "0 remotes."
    );
    let a = go(&repo, json!({"op": "default_branch"})).unwrap();
    assert_eq!(json_of(&a.git), json!("main"));
    assert_eq!(a.text, "The default branch is main.");
    assert_eq!(
        go(&repo, json!({"op": "last_message"})).unwrap().text,
        "The last commit's message."
    );
    assert_eq!(
        go(&repo, json!({"op": "history"})).unwrap().text,
        "1 commit."
    );
    let head = repo.head().unwrap();
    let a = go(&repo, json!({"op": "show", "hash": head})).unwrap();
    assert!(
        a.text.starts_with("Commit ") && a.text.ends_with(": 1 file."),
        "{}",
        a.text
    );
    assert_eq!(
        go(&repo, json!({"op": "blame", "path": "a.txt"}))
            .unwrap()
            .text,
        "1 line."
    );
    let a = go(&repo, json!({"op": "unmerged", "branch": "feature"})).unwrap();
    assert_eq!(json_of(&a.git), json!(0));
    let a = go(
        &repo,
        json!({"op": "compare", "base": "main", "branch": "feature"}),
    )
    .unwrap();
    assert_eq!(a.text, "0 commits.");
    let a = go(&repo, json!({"op": "stash_diff", "stash": stash})).unwrap();
    assert_eq!(a.text, "2 files.");
    let a = go(&repo, json!({"op": "measure", "paths": ["a.txt"]})).unwrap();
    assert_eq!(
        json_of(&a.git),
        json!({"files": 1, "bytes": 6, "keep": false})
    );
    assert_eq!(
        json_of(&go(&repo, json!({"op": "trash"})).unwrap().git),
        json!([])
    );
    let a = go(&repo, json!({"op": "find"})).unwrap();
    let Done::Found(found) = &a.git else { panic!() };
    assert_eq!(
        std::fs::canonicalize(found.root.as_ref().unwrap()).unwrap(),
        std::fs::canonicalize(&repo.root).unwrap()
    );
}

#[test]
fn the_acts_run_through_run() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    let hash = repo.head().unwrap();
    write(dir.path(), "a.txt", "two\n");
    assert_eq!(
        go(&repo, json!({"op": "stage", "paths": ["a.txt"]}))
            .unwrap()
            .text,
        "Staged 1 file."
    );
    assert!(repo.status().unwrap().entries[0].staged.is_some());
    assert_eq!(
        go(&repo, json!({"op": "unstage", "paths": ["a.txt"]}))
            .unwrap()
            .text,
        "Unstaged 1 file."
    );
    let a = go(
        &repo,
        json!({"op": "commit", "summary": "Two", "all": true}),
    )
    .unwrap();
    assert!(
        a.text.starts_with("Committed ") && a.text.ends_with('.'),
        "{}",
        a.text
    );
    assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "Two\n");
    go(&repo, json!({"op": "new_branch", "name": "x"})).unwrap();
    assert_eq!(repo.status().unwrap().branch.as_deref(), Some("x"));
    go(&repo, json!({"op": "switch", "branch": "main"})).unwrap();
    go(
        &repo,
        json!({"op": "rename_branch", "branch": "x", "name": "y"}),
    )
    .unwrap();
    go(&repo, json!({"op": "tag", "name": "v1", "hash": hash})).unwrap();
    assert_eq!(git(&repo, &["tag", "--list"]), "v1\n");
    go(&repo, json!({"op": "checkout", "hash": hash})).unwrap();
    assert_eq!(repo.status().unwrap().branch, None);
    go(&repo, json!({"op": "switch", "branch": "main"})).unwrap();
    go(&repo, json!({"op": "revert", "hash": repo.head().unwrap()})).unwrap();
    write(dir.path(), "b.txt", "b\n");
    go(
        &repo,
        json!({"op": "stash", "message": "wip", "untracked": true}),
    )
    .unwrap();
    let stashes = repo.stashes().unwrap();
    let stash = json!({"index": stashes[0].index, "hash": stashes[0].hash});
    go(&repo, json!({"op": "stash_pop", "stash": stash})).unwrap();
    assert!(dir.path().join("b.txt").exists());
    go(&repo, json!({"op": "ignore", "path": "b.txt"})).unwrap();
    assert_eq!(read(dir.path(), ".gitignore"), "/b.txt\n");
    go(
        &repo,
        json!({"op": "set_author", "name": "Grace", "email": "g@example.com"}),
    )
    .unwrap();
    assert_eq!(
        git(&repo, &["config", "--local", "--get", "user.name"]),
        "Grace\n"
    );
    let a = go(
        &repo,
        json!({"op": "undo_commit", "hash": repo.head().unwrap()[..7].to_string()}),
    )
    .unwrap();
    assert!(a.text.starts_with("Took back "), "{}", a.text);
    go(
        &repo,
        json!({"op": "delete_branch", "branch": "y", "force": true}),
    )
    .unwrap();
    write(dir.path(), "c.txt", "c\n");
    go(
        &repo,
        json!({"op": "stash", "message": "again", "untracked": true}),
    )
    .unwrap();
    let stashes = repo.stashes().unwrap();
    let stash = json!({"index": stashes[0].index, "hash": stashes[0].hash});
    let a = go(&repo, json!({"op": "stash_drop", "stash": stash})).unwrap();
    assert_eq!(a.text, "Dropped stash@{0}.");
    assert!(repo.stashes().unwrap().is_empty());
}

#[test]
fn arguments_that_could_be_options_or_leave_the_repository_are_refused() {
    let (dir, repo) = repo_with_work();
    let head = repo.head().unwrap();
    let status = repo.status().unwrap();
    let cases = [
        json!({"op": "stage", "paths": ["../x"]}),
        json!({"op": "stage", "paths": ["/etc/passwd"]}),
        json!({"op": "stage", "paths": ["C:\\x"]}),
        json!({"op": "stage", "paths": [".git/config"]}),
        json!({"op": "stage", "paths": [".GIT/hooks/x"]}),
        json!({"op": "stage", "paths": ["sub\\.git\\hooks"]}),
        json!({"op": "stage", "paths": [":(top)x"]}),
        json!({"op": "stage", "paths": ["a//b"]}),
        json!({"op": "stage", "paths": ["a\nb"]}),
        json!({"op": "stage", "paths": ["a\u{85}b"]}),
        json!({"op": "stage", "paths": []}),
        // What Windows and HFS+ read as `.git`, or as its short name.
        json!({"op": "stage", "paths": [".git."]}),
        json!({"op": "stage", "paths": [".git "]}),
        json!({"op": "stage", "paths": ["git~1/hooks/x"]}),
        json!({"op": "stage", "paths": [".git::$INDEX_ALLOCATION/hooks/pre-commit"]}),
        json!({"op": "stage", "paths": ["x/.git:stream"]}),
        json!({"op": "stage", "paths": [".g\u{200c}it/hooks/pre-commit"]}),
        json!({"op": "stage", "paths": ["\u{feff}.git/config"]}),
        json!({"op": "new_branch", "name": "-f"}),
        json!({"op": "new_branch", "name": "a b"}),
        // A name git would take as a forced refspec, a revision or a hash.
        json!({"op": "new_branch", "name": "+main"}),
        json!({"op": "new_branch", "name": "a:b"}),
        json!({"op": "new_branch", "name": "a^"}),
        json!({"op": "new_branch", "name": "a~1"}),
        json!({"op": "new_branch", "name": "a..b"}),
        json!({"op": "new_branch", "name": "a@{1}"}),
        json!({"op": "new_branch", "name": "feed1234"}),
        json!({"op": "rename_branch", "branch": "main", "name": "+x"}),
        json!({"op": "tag", "name": "+v1", "hash": head}),
        json!({"op": "tag", "name": "--delete", "hash": head}),
        json!({"op": "checkout", "hash": "--orphan"}),
        json!({"op": "blame", "path": "a.txt", "hash": "--contents=/etc/passwd"}),
        json!({"op": "show", "hash": "HEAD"}),
        json!({"op": "merge", "branch": "-X"}),
        json!({"op": "publish", "remote": "--receive-pack=touch x", "branch": "main"}),
        json!({"op": "restore", "entry": "../x"}),
        json!({"op": "commit", "summary": "a\nb"}),
        json!({"op": "set_author", "name": "--global", "email": "x"}),
        json!({"op": "set_author", "name": "", "email": "ada@example.com"}),
        json!({"op": "set_author", "name": "Ada", "email": "-x"}),
        json!({"op": "stash_pop", "stash": {"index": 0, "hash": "--all"}}),
        json!({"op": "ignore", "path": "../x"}),
    ];
    for case in cases {
        let parsed = op(case.clone());
        assert!(parsed.check().is_err(), "{case} passed its check");
        let why = words(ops::run(&repo, parsed, &home()));
        assert!(!why.is_empty(), "{case}");
    }
    let many: Vec<String> = (0..10_001).map(|i| format!("f{i}")).collect();
    let why = words(go(&repo, json!({"op": "stage", "paths": many})));
    assert_eq!(why, "paths takes 1 to 10,000 paths.");
    // The words are the rules'.
    assert_eq!(
        words(go(&repo, json!({"op": "stage", "paths": ["../x"]}))),
        "\"../x\" isn't a path inside the repository: give it relative to its root, without \"..\" or .git."
    );
    assert_eq!(
        words(go(&repo, json!({"op": "show", "hash": "zz"}))),
        "\"zz\" isn't a commit's hash."
    );
    assert_eq!(
        words(go(&repo, json!({"op": "new_branch", "name": "-f"}))),
        "\"-f\" can't name a branch."
    );
    assert_eq!(
        words(go(&repo, json!({"op": "restore", "entry": "../x"}))),
        "\"../x\" isn't a discard of the trash."
    );
    // Git never ran: nothing moved.
    assert_eq!(repo.head().unwrap(), head);
    assert_eq!(repo.status().unwrap(), status);
    assert!(!dir.path().join("x").exists());
}

#[test]
fn names_that_aren_t_there_are_refused_before_git_runs() {
    let (dir, repo) = repo_with_work();
    commit(&repo, dir.path(), "clean.txt", "c\n", "Clean");
    let head = repo.head().unwrap();
    assert_eq!(
        words(go(&repo, json!({"op": "merge", "branch": "nope"}))),
        "There's no branch nope: branches lists them."
    );
    assert_eq!(
        words(go(
            &repo,
            json!({"op": "publish", "remote": "https://evil.example/x.git", "branch": "main"})
        )),
        "There's no remote https://evil.example/x.git: remotes lists them."
    );
    // A remote-tracking branch is no local one.
    repo.run(&["update-ref", "refs/remotes/origin/main", &head])
        .unwrap();
    for case in [
        json!({"op": "push_branch", "branch": "origin/main"}),
        json!({"op": "delete_branch", "branch": "origin/main"}),
        json!({"op": "switch", "branch": "origin/main"}),
        json!({"op": "unmerged", "branch": "origin/main"}),
    ] {
        assert_eq!(
            words(go(&repo, case)),
            "There's no branch origin/main: branches lists them."
        );
    }
    assert_eq!(
        words(go(
            &repo,
            json!({"op": "switch", "branch": "main", "remote": true})
        )),
        "There's no branch main: branches lists them."
    );
    assert_eq!(
        words(go(&repo, json!({"op": "discard", "paths": ["clean.txt"]}))),
        "clean.txt has no changes to discard."
    );
    assert_eq!(
        words(go(&repo, json!({"op": "stage", "paths": ["clean.txt"]}))),
        "clean.txt has no changes to stage."
    );
    assert_eq!(
        words(go(&repo, json!({"op": "unstage", "paths": ["a.txt"]}))),
        "a.txt has nothing staged."
    );
    assert_eq!(
        words(go(
            &repo,
            json!({"op": "resolve", "path": "a.txt", "side": "ours"})
        )),
        "a.txt isn't in conflict."
    );
    assert_eq!(
        words(go(&repo, json!({"op": "continue"}))),
        "No merge, rebase, cherry-pick or revert is in progress."
    );
    assert_eq!(
        words(go(&repo, json!({"op": "abort"}))),
        "No merge, rebase, cherry-pick or revert is in progress."
    );
    assert!(
        words(go(&repo, json!({"op": "init"}))).starts_with("There's already a repository at ")
    );
    assert_eq!(
        words(go(&repo, json!({"op": "message"}))),
        "No commit-message agent is ready."
    );
    assert_eq!(repo.head().unwrap(), head);
}

#[cfg(unix)]
#[test]
fn a_write_through_a_link_leaving_the_repository_is_refused() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    let hash = repo.head().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "keep\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
    assert_eq!(
        words(go(
            &repo,
            json!({"op": "restore_version", "hash": hash, "then": "a.txt", "path": "out/x"})
        )),
        "out/x lies outside the repository."
    );
    // A file that is itself a link: committed, then asked to be overwritten.
    std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("l.txt")).unwrap();
    repo.stage(&["l.txt"]).unwrap();
    repo.commit("Link", false).unwrap();
    assert_eq!(
        words(go(
            &repo,
            json!({"op": "restore_version", "hash": hash, "then": "a.txt", "path": "l.txt"})
        )),
        "l.txt is a link, which Templum doesn't write through."
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("secret")).unwrap(),
        "keep\n"
    );
    assert_eq!(
        std::fs::read_dir(outside.path()).unwrap().count(),
        1,
        "nothing written beside it"
    );
    // Reads that stay: a link inside to a place outside is no file of the diff.
    std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("peek")).unwrap();
    let a = go(&repo, json!({"op": "diff"})).unwrap();
    let Done::Diff(diff) = &a.git else { panic!() };
    assert!(
        diff.unstaged.iter().all(|f| f.path() != "peek"),
        "an untracked link isn't read"
    );
    // The .gitignore a link leads out of is not written.
    std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join(".gitignore"))
        .unwrap();
    assert_eq!(
        words(go(&repo, json!({"op": "ignore", "path": "x"}))),
        ".gitignore is a link, which Templum doesn't write through."
    );
    assert_eq!(
        std::fs::read_to_string(outside.path().join("secret")).unwrap(),
        "keep\n"
    );
}

#[test]
fn restore_takes_only_this_repository_s_discards() {
    let (_keep, trash) = trash();
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    let (other_dir, other) = fresh();
    commit(&other, other_dir.path(), "z.txt", "z\n", "Z");
    write(other_dir.path(), "z.txt", "changed\n");
    let entries: Vec<_> = other
        .status()
        .unwrap()
        .unstaged()
        .into_iter()
        .cloned()
        .collect();
    let theirs = ops::discard(
        &other,
        Some(&trash),
        &["z.txt".to_string()],
        "Discarded z.txt",
        &Discarding::Files(entries),
    )
    .unwrap()
    .unwrap();
    write(dir.path(), "a.txt", "two\n");
    let entries: Vec<_> = repo
        .status()
        .unwrap()
        .unstaged()
        .into_iter()
        .cloned()
        .collect();
    let mine = ops::discard(
        &repo,
        Some(&trash),
        &["a.txt".to_string()],
        "Discarded a.txt",
        &Discarding::Files(entries),
    )
    .unwrap()
    .unwrap();
    let with_trash = Home {
        trash: Some(&trash),
        writer: None,
        global_author: false,
    };
    let run = |id: &str| {
        ops::run(
            &repo,
            op(json!({"op": "restore", "entry": id})),
            &with_trash,
        )
    };
    assert_eq!(
        words(run(&theirs.id)),
        format!(
            "There's no discard {} of this repository: trash lists them.",
            theirs.id
        )
    );
    assert_eq!(
        read(other_dir.path(), "z.txt"),
        "z\n",
        "the other repository's file is as discarded"
    );
    assert_eq!(
        words(go(&repo, json!({"op": "restore", "entry": mine.id}))),
        "This Templum keeps no trash."
    );
    let a = run(&mine.id).unwrap();
    assert_eq!(a.text, "Restored.");
    assert_eq!(read(dir.path(), "a.txt"), "two\n");
}

#[test]
fn a_patch_must_be_one_hunk_of_a_listed_file_as_the_diff_shows_it() {
    let (dir, repo) = fresh();
    let lines = |two: &str, last: &str| {
        (1..=30)
            .map(|i| match i {
                2 => format!("{two}\n"),
                28 => format!("{last}\n"),
                i => format!("line {i}\n"),
            })
            .collect::<String>()
    };
    commit(
        &repo,
        dir.path(),
        "a.txt",
        &lines("line 2", "line 28"),
        "One",
    );
    commit(&repo, dir.path(), "b.txt", "b\n", "Two");
    write(dir.path(), "a.txt", &lines("changed 2", "changed 28"));
    let status = repo.status().unwrap();
    let files = repo.working_diff(&status, false, &[]).unwrap().unstaged;
    let patch = hunk_patch(&files[0], 0).unwrap();
    let not_one = "The patch isn't one hunk of one file, as the diff shows it.";
    let with_mode = patch.replacen('\n', "\nold mode 100644\nnew mode 100755\n", 1);
    let two_hunks = format!(
        "{}{}",
        patch,
        patch
            .split_once("\n@@")
            .map(|(_, h)| format!("@@{h}"))
            .unwrap()
    );
    let link = patch.replacen("--- a/a.txt", "new file mode 120000\n--- a/a.txt", 1);
    for bad in [&with_mode, &two_hunks, &link] {
        let parsed = op(json!({"op": "stage_hunk", "patch": bad}));
        assert_eq!(parsed.check().unwrap_err(), not_one, "{bad}");
        assert_eq!(words(ops::run(&repo, parsed, &home())), not_one);
    }
    assert_eq!(
        words(go(&repo, json!({"op": "stage_hunk", "patch": "garbage"}))),
        not_one
    );
    // A file the status doesn't list.
    let other = patch.replace("a.txt", "other.txt");
    assert_eq!(
        words(go(&repo, json!({"op": "stage_hunk", "patch": other}))),
        "other.txt has no changes to stage."
    );
    // Unstaging a file with nothing staged.
    assert_eq!(
        words(go(&repo, json!({"op": "unstage_hunk", "patch": patch}))),
        "a.txt has nothing staged."
    );
    // A discard of another file than the patch's.
    assert_eq!(
        words(go(
            &repo,
            json!({"op": "discard_hunk", "path": "b.txt", "patch": patch})
        )),
        "The patch isn't a change to b.txt."
    );
    assert_eq!(repo.status().unwrap(), status);
    // The hunk as the panel draws it applies.
    assert_eq!(
        go(&repo, json!({"op": "stage_hunk", "patch": patch}))
            .unwrap()
            .text,
        "Staged a change to a.txt."
    );
    assert!(git(&repo, &["diff", "--cached", "--stat"]).contains("a.txt"));
    assert_eq!(
        go(&repo, json!({"op": "unstage_hunk", "patch": patch}))
            .unwrap()
            .text,
        "Unstaged a change to a.txt."
    );
    let a = go(
        &repo,
        json!({"op": "discard_hunk", "path": "a.txt", "patch": patch}),
    )
    .unwrap();
    assert_eq!(a.text, "Discarded a change to a.txt.");
    assert_eq!(read(dir.path(), "a.txt"), lines("line 2", "changed 28"));
}

fn big_file(path: &str, kb: usize) -> templum_git::FileDiff {
    let mut file = templum_git::FileDiff::empty();
    file.old_path = Some(path.into());
    file.new_path = Some(path.into());
    file.hunks = vec![templum_git::Hunk {
        old_start: 1,
        old_len: 1,
        new_start: 1,
        new_len: 1,
        context: String::new(),
        lines: vec![templum_git::Line {
            kind: templum_git::Kind::Added,
            old_no: None,
            new_no: Some(1),
            text: "x".repeat(kb * 1024),
            no_newline: false,
        }],
    }];
    file
}

#[test]
fn a_big_diff_is_cut_largest_files_first() {
    let mut done = Done::Diff(templum_git::WorkingDiff {
        unstaged: vec![
            big_file("small", 10),
            big_file("big", 60),
            big_file("middle", 30),
        ],
        staged: Vec::new(),
    });
    let size = serde_json::to_vec(&done).unwrap().len();
    ops::fit(&mut done, size - 40 * 1024).unwrap();
    let Done::Diff(diff) = &done else { panic!() };
    assert_eq!(diff.unstaged.len(), 3, "every row stays");
    let by = |name: &str| diff.unstaged.iter().find(|f| f.path() == name).unwrap();
    assert!(by("big").hunks.is_empty() && by("big").cut);
    assert!(!by("middle").hunks.is_empty() && !by("middle").cut);
    assert!(!by("small").hunks.is_empty() && !by("small").cut);
    assert!(serde_json::to_vec(&done).unwrap().len() <= size - 40 * 1024);
}

#[test]
fn an_answer_too_large_to_cut_is_refused() {
    let branches: Vec<templum_git::Branch> = (0..5000)
        .map(|i| templum_git::Branch {
            name: format!("branch-{i}"),
            remote: false,
            current: false,
            commit: "abc".into(),
            upstream: None,
            gone: false,
            ahead: 0,
            behind: 0,
            subject: "s".into(),
            time: 0,
        })
        .collect();
    let mut done = Done::Branches(branches);
    match ops::fit(&mut done, 1000) {
        Err(Failure::Refused(s)) => {
            assert!(s.starts_with("The answer is too large to send:"), "{s}")
        }
        other => panic!("{other:?}"),
    }
}

/// Windows has no `:` in a name.
#[cfg(windows)]
#[test]
fn a_colon_in_a_part_is_refused_on_windows() {
    assert!(
        op(json!({"op": "stage", "paths": ["x/C:/y"]}))
            .check()
            .is_err()
    );
}

fn bare_git(server: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(server)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// A bare "server" holding `main` at three commits, and a clone-like repository following it.
fn served() -> (tempfile::TempDir, tempfile::TempDir, Repo, Vec<String>) {
    let server = tempfile::tempdir().unwrap();
    bare_git(server.path(), &["init", "-q", "--bare"]);
    let bare = server.path().to_string_lossy().into_owned();
    let (dir, repo) = fresh();
    let mut hashes = Vec::new();
    for (i, text) in ["one\n", "two\n", "three\n"].iter().enumerate() {
        commit(&repo, dir.path(), "a.txt", text, &format!("C{}", i + 1));
        hashes.push(repo.head().unwrap());
    }
    git(&repo, &["remote", "add", "origin", &bare]);
    git(&repo, &["push", "origin", "main"]);
    (server, dir, repo, hashes)
}

#[test]
fn publish_sends_the_branch_and_never_reads_its_name_as_a_forced_refspec() {
    let (server, _dir, repo, hashes) = served();
    // Local main falls behind the server's, and a branch is named `+main` beside it.
    git(&repo, &["reset", "-q", "--hard", &hashes[0]]);
    git(&repo, &["branch", "+main"]);
    let a = go(
        &repo,
        json!({"op": "publish", "remote": "origin", "branch": "+main"}),
    );
    assert!(a.is_ok(), "{:?}", a.err());
    assert_eq!(
        bare_git(server.path(), &["rev-parse", "refs/heads/main"]).trim(),
        hashes[2],
        "the server's main keeps its commits"
    );
    assert_eq!(
        bare_git(server.path(), &["rev-parse", "refs/heads/+main"]).trim(),
        hashes[0],
        "the branch went up under its own name"
    );
}

#[test]
fn undo_commit_goes_back_only_to_the_commit_an_amend_replaced() {
    let (dir, repo) = fresh();
    for (i, text) in ["one\n", "two\n", "three\n"].iter().enumerate() {
        commit(&repo, dir.path(), "a.txt", text, &format!("C{}", i + 1));
    }
    let head = repo.head().unwrap();
    let root = git(&repo, &["rev-list", "--max-parents=0", "HEAD"]);
    let parent = git(&repo, &["rev-parse", "HEAD~1"]);
    for replaced in [root.trim(), parent.trim(), head.as_str()] {
        let why = go(
            &repo,
            json!({"op": "undo_commit", "hash": &head[..8], "replaced": replaced}),
        );
        assert!(why.is_err(), "{replaced} was taken for an amended commit");
        assert_eq!(repo.head().unwrap(), head, "the branch didn't move");
    }
    // An amend's own replaced commit still goes (an_amend_remembers_the_commit_it_replaced...).
    write(dir.path(), "a.txt", "four\n");
    repo.stage(&["a.txt"]).unwrap();
    let c = ops::commit(&repo, "C3, amended", "", true, false)
        .unwrap()
        .unwrap();
    let a = go(
        &repo,
        json!({"op": "undo_commit", "hash": c.short, "replaced": c.replaced}),
    )
    .unwrap();
    assert!(a.text.starts_with("Took back"), "{}", a.text);
    assert_eq!(repo.head().unwrap(), head);
}

#[test]
fn a_cut_of_thousands_of_files_takes_no_time() {
    let mut done = Done::Diff(templum_git::WorkingDiff {
        unstaged: (0..4000).map(|i| big_file(&format!("f{i}"), 1)).collect(),
        staged: Vec::new(),
    });
    let size = serde_json::to_vec(&done).unwrap().len();
    let started = std::time::Instant::now();
    ops::fit(&mut done, size / 2).unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(1),
        "took {:?}",
        started.elapsed()
    );
    assert!(serde_json::to_vec(&done).unwrap().len() <= size / 2);
}

#[cfg(unix)]
#[test]
fn restore_refuses_a_file_whose_folder_became_a_link_leading_out() {
    let (_keep, trash) = trash();
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "d/a.txt", "one\n", "One");
    write(dir.path(), "d/a.txt", "two\n");
    let entries: Vec<_> = repo
        .status()
        .unwrap()
        .unstaged()
        .into_iter()
        .cloned()
        .collect();
    let kept = ops::discard(
        &repo,
        Some(&trash),
        &["d/a.txt".to_string()],
        "Discarded d/a.txt",
        &Discarding::Files(entries),
    )
    .unwrap()
    .unwrap();
    // The folder is replaced by a link to somewhere else.
    let outside = tempfile::tempdir().unwrap();
    std::fs::remove_dir_all(dir.path().join("d")).unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("d")).unwrap();
    let with_trash = Home {
        trash: Some(&trash),
        writer: None,
        global_author: false,
    };
    let why = words(ops::run(
        &repo,
        op(json!({"op": "restore", "entry": kept.id})),
        &with_trash,
    ));
    assert_eq!(why, "d/a.txt lies outside the repository.");
    assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn a_conflicted_link_leading_out_is_no_file_of_the_diff() {
    let (dir, repo) = conflicted();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret"), "do not read\n").unwrap();
    std::fs::remove_file(dir.path().join("a.txt")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("a.txt")).unwrap();
    let a = go(&repo, json!({"op": "diff"})).unwrap();
    let Done::Diff(diff) = &a.git else { panic!() };
    assert!(
        diff.unstaged
            .iter()
            .chain(&diff.staged)
            .all(|f| f.path() != "a.txt"),
        "a conflicted link isn't read"
    );
    assert!(!json_of(&a.git).to_string().contains("do not read"));
}

#[test]
fn unstage_takes_a_rename_by_its_old_name_and_stage_takes_a_conflicted_file() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    git(&repo, &["mv", "a.txt", "b.txt"]);
    assert!(
        repo.status()
            .unwrap()
            .entries
            .iter()
            .any(|e| e.staged.is_some())
    );
    go(&repo, json!({"op": "unstage", "paths": ["a.txt"]})).unwrap();
    // The rename comes apart: the old name's deletion is no longer staged, the new file still is.
    let entries = repo.status().unwrap().entries;
    let by = |name: &str| entries.iter().find(|e| e.path == name).unwrap();
    assert_eq!(by("a.txt").staged, None);
    assert_eq!(by("b.txt").staged, Some(templum_git::Change::Added));
    let (_dir, repo) = conflicted();
    let a = go(&repo, json!({"op": "stage", "paths": ["a.txt"]})).unwrap();
    assert_eq!(a.text, "Staged 1 file.");
    assert!(repo.status().unwrap().entries.iter().all(|e| !e.conflict()));
}

#[test]
fn a_path_is_a_file_never_a_pattern() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "A");
    commit(&repo, dir.path(), "[ab].txt", "one\n", "B");
    repo.run(&["checkout", "-q", "-b", "feature"]).unwrap();
    for name in ["a.txt", "[ab].txt"] {
        commit(&repo, dir.path(), name, "theirs\n", "Theirs");
    }
    repo.run(&["checkout", "-q", "main"]).unwrap();
    for name in ["a.txt", "[ab].txt"] {
        commit(&repo, dir.path(), name, "ours\n", "Ours");
    }
    assert!(repo.run(&["merge", "feature"]).is_err());
    write(dir.path(), "a.txt", "my hand edit\n");
    go(
        &repo,
        json!({"op": "resolve", "path": "[ab].txt", "side": "theirs"}),
    )
    .unwrap();
    assert_eq!(read(dir.path(), "[ab].txt"), "theirs\n");
    assert_eq!(read(dir.path(), "a.txt"), "my hand edit\n", "not a match");
}

#[test]
fn a_commit_past_what_a_call_may_read_is_refused() {
    use std::io::Write as _;
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    // 70 MiB of lines, written as they go: the test itself never holds them.
    let mut big =
        std::io::BufWriter::new(std::fs::File::create(dir.path().join("big.txt")).unwrap());
    let line = [b"x".repeat(63), b"\n".to_vec()].concat();
    for _ in 0..70 * 1024 * 1024 / line.len() {
        big.write_all(&line).unwrap();
    }
    big.flush().unwrap();
    drop(big);
    repo.stage(&["big.txt"]).unwrap();
    repo.commit("Big", false).unwrap();
    let hash = repo.head().unwrap();
    let at = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let Err(Failure::Git(e)) = ops::run(
        &repo.with_deadline(at),
        op(json!({"op": "show", "hash": hash})),
        &home(),
    ) else {
        panic!("the commit was read");
    };
    assert!(e.too_large(), "{e}");
    // The popup has no deadline and reads it, as before.
    assert!(ops::show(&repo, &hash, None, false).is_ok());
}

#[test]
fn a_tag_and_a_branch_never_share_a_name() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    let head = repo.head().unwrap();
    let why = words_of(go(
        &repo,
        json!({"op": "tag", "name": "main", "hash": head}),
    ));
    assert!(why.contains("is a branch"), "{why}");
    git(&repo, &["tag", "v1"]);
    let why = words_of(go(&repo, json!({"op": "new_branch", "name": "v1"})));
    assert!(why.contains("is a tag"), "{why}");
    // A tag that came with a fetch, named like a branch: the branch is still `main`.
    git(&repo, &["tag", "main"]);
    git(&repo, &["branch", "other", "HEAD"]);
    let names: Vec<_> = repo
        .branches()
        .unwrap()
        .into_iter()
        .map(|b| b.name)
        .collect();
    assert_eq!(names, ["main", "other"]);
    go(&repo, json!({"op": "switch", "branch": "other"})).unwrap();
    go(&repo, json!({"op": "switch", "branch": "main"})).unwrap();
    assert_eq!(git(&repo, &["symbolic-ref", "HEAD"]), "refs/heads/main\n");
}

/// What a call failed with, git's words included.
fn words_of(result: Result<ops::Answer, Failure>) -> String {
    match result {
        Err(failure) => failure.sentence(),
        Ok(a) => panic!("expected a failure, got {:?}", a.text),
    }
}

#[test]
fn a_ref_named_like_the_short_hash_never_stands_in_for_the_commit_pushed() {
    let (_server, dir, repo, _hashes) = served();
    git(&repo, &["branch", "--set-upstream-to=origin/main", "main"]);
    let head = repo.head().unwrap();
    // A commit not pushed, tagged with the head's prefix (a tag can arrive by fetch).
    git(&repo, &["checkout", "-q", "-b", "side"]);
    commit(&repo, dir.path(), "b.txt", "b\n", "Side");
    git(&repo, &["tag", &head[..8]]);
    git(&repo, &["checkout", "-q", "main"]);
    assert!(matches!(
        ops::undo_commit(&repo, &head[..8], None).unwrap(),
        Undone::Pushed
    ));
    assert_eq!(repo.head().unwrap(), head);
}

#[test]
fn undoing_an_amend_of_a_root_goes_only_to_the_root_it_replaced() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "Root");
    let first = repo.head().unwrap();
    // Another root, on a branch of its own.
    git(&repo, &["checkout", "-q", "--orphan", "other"]);
    commit(&repo, dir.path(), "b.txt", "b\n", "Other root");
    let other = repo.head().unwrap();
    git(&repo, &["checkout", "-q", "-f", "main"]);
    let head = repo.head().unwrap();
    assert_eq!(head, first);
    let why = go(
        &repo,
        json!({"op": "undo_commit", "hash": &head[..8], "replaced": other}),
    );
    assert!(why.is_err(), "an unrelated root was taken for the amended");
    assert_eq!(repo.head().unwrap(), head, "the branch didn't move");
    // The root an amend really replaced still goes back.
    write(dir.path(), "a.txt", "two\n");
    repo.stage(&["a.txt"]).unwrap();
    let c = ops::commit(&repo, "Root, amended", "", true, false)
        .unwrap()
        .unwrap();
    let a = go(
        &repo,
        json!({"op": "undo_commit", "hash": c.short, "replaced": c.replaced}),
    )
    .unwrap();
    assert!(a.text.starts_with("Took back"), "{}", a.text);
    assert_eq!(repo.head().unwrap(), head);
}

#[test]
fn calls_with_control_never_push_out_the_copy_of_a_hand_merged_file() {
    let (dir, repo) = conflicted();
    let (_keep, trash) = trash();
    let api = trash.for_api();
    let home = Home {
        trash: Some(&api),
        writer: None,
        global_author: false,
    };
    let resolve = || {
        ops::run(
            &repo,
            op(json!({"op": "resolve", "path": "a.txt", "side": "ours"})),
            &home,
        )
    };
    // The person's discard of b.txt, which they edited since.
    write(dir.path(), "b.txt", "discarded\n");
    let discard = trash
        .put(
            &repo.root,
            &["b.txt".to_string()],
            "Discarded b.txt",
            std::time::SystemTime::now(),
        )
        .unwrap();
    trash.seal(&discard).unwrap();
    write(dir.path(), "b.txt", "HAND EDITS\n");
    // The person merges a.txt by hand; the file stays conflicted for git, so a call can repeat.
    write(dir.path(), "a.txt", "HAND-MERGED WORK\n");
    resolve().unwrap();
    let hand = ops::discarded(&trash, &repo.root)
        .into_iter()
        .find(|k| k.what.starts_with("Chose"))
        .unwrap();
    for _ in 1..templum_git::trash::KEEP {
        resolve().unwrap();
    }
    let refused = match resolve() {
        Err(failure @ Failure::Keep(_)) => failure.sentence(),
        other => panic!(
            "expected the copy to be refused, got {:?}",
            other.map(|a| a.text)
        ),
    };
    assert!(refused.contains("nothing was changed"), "{refused}");
    assert!(
        refused.contains("already holds 50 copies made through the API"),
        "{refused}"
    );
    let listed = ops::discarded(&trash, &repo.root);
    assert!(
        listed.iter().any(|k| k.id == hand.id),
        "the hand-merged copy is still there"
    );
    assert_eq!(
        listed.len(),
        templum_git::trash::KEEP + 1,
        "plus the person's discard"
    );
    // A forced restore needs a copy of the edits it writes over: refused, nothing written.
    let forced = ops::run(
        &repo,
        op(json!({"op": "restore", "entry": discard.id, "force": true})),
        &home,
    );
    assert!(
        matches!(forced, Err(Failure::Keep(_))),
        "{:?}",
        forced.map(|a| a.text)
    );
    assert_eq!(read(dir.path(), "b.txt"), "HAND EDITS\n");
    // The popup's own trash isn't held to it.
    trash.restore(&discard.id, true).unwrap();
    assert_eq!(read(dir.path(), "b.txt"), "discarded\n");
}

#[test]
fn a_conflicted_file_past_the_diff_limit_is_not_read_to_choose_a_region() {
    let (dir, repo) = conflicted();
    let big = format!(
        "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> feature\n{}",
        "x".repeat(templum_git::DIFF_LIMIT)
    );
    write(dir.path(), "a.txt", &big);
    let shown = (vec!["ours".to_string()], vec!["theirs".to_string()]);
    let result = ops::resolve_region(&repo, None, "a.txt", 0, Take::First, &shown);
    let Err(Failure::Refused(why)) = result else {
        panic!("expected a refusal, got {result:?}");
    };
    assert_eq!(
        why,
        "a.txt is too large to resolve by region: take one side of it instead."
    );
    assert_eq!(read(dir.path(), "a.txt"), big, "nothing was written");
}

fn restore_through(repo: &Repo, trash: &Trash, entry: &str) -> Result<String, Failure> {
    let home = Home {
        trash: Some(trash),
        writer: None,
        global_author: false,
    };
    ops::run(
        repo,
        op(json!({"op": "restore", "entry": entry, "force": true})),
        &home,
    )
    .map(|a| a.text)
}

// "Couldn't keep a copy" is for the copy that failed, whatever the error's kind; a write that
// fails after it is a write error, even for a full disk.
#[test]
fn a_forced_restore_says_keep_for_its_copy_failing_and_io_for_its_writes_failing() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "a.txt", "one\n", "One");
    commit(&repo, dir.path(), "sub/x.txt", "one\n", "Two");
    let (_keep, trash) = trash();
    // The copy fails (a folder can't be copied): not StorageFull, still "keep". The folder comes
    // after the seal: a folder's size, all its stamp holds, doesn't change with what it holds on
    // every file system (ext4, Windows).
    write(dir.path(), "a.txt", "mine\n");
    let discard = trash
        .put(
            &repo.root,
            &["a.txt".to_string()],
            "Discarded a.txt",
            std::time::SystemTime::now(),
        )
        .unwrap();
    trash.seal(&discard).unwrap();
    std::fs::remove_file(dir.path().join("a.txt")).unwrap();
    std::fs::create_dir(dir.path().join("a.txt")).unwrap();
    let result = restore_through(&repo, &trash, &discard.id);
    assert!(matches!(result, Err(Failure::Keep(_))), "{result:?}");
    // The copy is made, a write after it fails (the folder is a file): not "keep".
    write(dir.path(), "sub/x.txt", "mine\n");
    let discard = trash
        .put(
            &repo.root,
            &["sub/x.txt".to_string()],
            "Discarded sub/x.txt",
            std::time::SystemTime::now(),
        )
        .unwrap();
    trash.seal(&discard).unwrap();
    std::fs::remove_dir_all(dir.path().join("sub")).unwrap();
    write(dir.path(), "sub", "now a file\n");
    let result = restore_through(&repo, &trash, &discard.id);
    assert!(matches!(result, Err(Failure::Io(_))), "{result:?}");
}

// Every other copy is held to 100 MiB; a forced restore's copy of what it writes over was not.
#[test]
fn a_forced_restore_through_the_api_wont_copy_what_is_too_large_to_keep() {
    let (dir, repo) = fresh();
    commit(&repo, dir.path(), "b.txt", "one\n", "One");
    let (_keep, trash) = trash();
    let api = trash.for_api();
    write(dir.path(), "b.txt", "discarded\n");
    let discard = trash
        .put(
            &repo.root,
            &["b.txt".to_string()],
            "Discarded b.txt",
            std::time::SystemTime::now(),
        )
        .unwrap();
    trash.seal(&discard).unwrap();
    // A sparse file past the cap.
    let big = std::fs::File::create(dir.path().join("b.txt")).unwrap();
    big.set_len(templum_git::trash::MAX_BYTES + 1).unwrap();
    let result = restore_through(&repo, &api, &discard.id);
    let Err(failure @ Failure::Keep(_)) = result else {
        panic!("expected the copy to be refused, got {result:?}");
    };
    assert!(
        failure.sentence().contains("too large to keep a copy"),
        "{}",
        failure.sentence()
    );
    assert_eq!(
        std::fs::metadata(dir.path().join("b.txt")).unwrap().len(),
        templum_git::trash::MAX_BYTES + 1,
        "nothing was written"
    );
    assert_eq!(
        ops::discarded(&api, &repo.root).len(),
        1,
        "no copy was made"
    );
}

// A conflicted link to /dev/zero is size 0 through the link, so it passed the size check and
// the read never ended.
#[cfg(unix)]
#[test]
fn a_conflicted_link_is_refused_not_read_to_choose_a_region() {
    let (dir, repo) = conflicted();
    std::fs::remove_file(dir.path().join("a.txt")).unwrap();
    std::os::unix::fs::symlink("/dev/zero", dir.path().join("a.txt")).unwrap();
    let shown = (vec!["ours".to_string()], vec!["theirs".to_string()]);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let r = ops::resolve_region(&repo, None, "a.txt", 0, Take::First, &shown);
        let _ = tx.send(format!("{r:?}"));
    });
    let result = rx
        .recv_timeout(std::time::Duration::from_secs(20))
        .expect("resolve_region read through a link to a device and never ended");
    assert!(result.contains("isn't a regular file"), "{result}");
}
