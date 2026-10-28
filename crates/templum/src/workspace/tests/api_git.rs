//! The API's `git` tool at home: a repository of a workspace, scoped by operation, confined for
//! a limited token, answered in turn.

use std::sync::mpsc::Receiver;

use serde_json::{Value, json};
use templum_api::store::Scope;
use templum_api::{Token, codes};

use super::api::{api_later, refused, said, with_token};
use super::*;

/// The answer line to a `git` call, once the repository was found and git ran.
fn answer_of(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    answers: &Receiver<String>,
) -> Value {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        settle(handle, cx);
        if let Ok(line) = answers.try_recv() {
            return serde_json::from_str(&line).unwrap();
        }
        assert!(std::time::Instant::now() < deadline, "no answer in 10 s");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn git_later(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
    conn: u64,
    args: Value,
) -> Receiver<String> {
    api_later(
        handle,
        ws,
        cx,
        token,
        conn,
        "tools/call",
        json!({ "name": "git", "arguments": args }),
    )
}

fn git_call(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    token: &Token,
    args: Value,
) -> Value {
    let answers = git_later(handle, ws, cx, token, 900, args);
    answer_of(handle, cx, &answers)
}

/// The shown workspace works in `folder`; its id, and its name as the API says it.
fn folder_of(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    folder: &std::path::Path,
) -> (String, String) {
    ws.update(cx, |ws, _| {
        ws.space.folder = Some(folder.to_path_buf());
        (
            ws.space.id.clone(),
            super::super::bus::clean_title(&ws.space.name),
        )
    })
}

fn changed_repo() -> tempfile::TempDir {
    let repo = committed_repo();
    std::fs::write(repo.path().join("a.txt"), "two\n").unwrap();
    repo
}

#[gpui_kit::test]
fn look_on_the_workspace_s_repository_answers_its_status(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let repo = changed_repo();
    let (id, _) = folder_of(&ws, cx, repo.path());
    let token = with_token(&ws, cx, Scope::Read, None);
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "look"}),
    );
    assert_eq!(
        said(&line),
        (false, "On main: 1 changed file.".to_string()),
        "{line}"
    );
    assert_eq!(
        line["result"]["git"]["status"]["entries"][0]["path"],
        "a.txt"
    );
    assert_eq!(line["result"]["target"]["workspace"], id.as_str());
}

#[gpui_kit::test]
fn stage_and_commit_through_the_api(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let repo = changed_repo();
    let real = repo.path().canonicalize().unwrap();
    let (id, _) = folder_of(&ws, cx, &real);
    let token = with_token(&ws, cx, Scope::Control, Some(vec![id.as_str()]));
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "stage", "paths": ["a.txt"]}),
    );
    assert_eq!(said(&line), (false, "Staged 1 file.".to_string()), "{line}");
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "commit", "summary": "Two"}),
    );
    let (failed, text) = said(&line);
    assert!(!failed && text.starts_with("Committed "), "{line}");
    assert_eq!(git_run(repo.path(), &["log", "-1", "--format=%s"]), "Two\n");
}

#[gpui_kit::test]
fn a_limited_token_s_repository_outside_its_folder_is_refused(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let repo = changed_repo();
    let sub = repo.path().canonicalize().unwrap().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let (id, name) = folder_of(&ws, cx, &sub);
    let limited = with_token(&ws, cx, Scope::Full, Some(vec![id.as_str()]));
    let line = git_call(
        handle,
        &ws,
        cx,
        &limited,
        json!({"workspace": id, "op": "look"}),
    );
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::SCOPE)),
            format!("That repository lies outside {name}'s folder.")
        ),
        "{line}"
    );
    // A token for all workspaces is not confined.
    let all = with_token(&ws, cx, Scope::Full, None);
    let line = git_call(
        handle,
        &ws,
        cx,
        &all,
        json!({"workspace": id, "op": "look"}),
    );
    assert_eq!(
        said(&line),
        (false, "On main: 1 changed file.".to_string()),
        "{line}"
    );
}

#[gpui_kit::test]
fn a_worktree_whose_main_tree_is_outside_is_refused(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let main = committed_repo();
    let folder = tempfile::tempdir().unwrap();
    let real = dunce::canonicalize(folder.path()).unwrap();
    git_run(
        main.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            real.join("wt").to_str().unwrap(),
        ],
    );
    let (id, name) = folder_of(&ws, cx, &real);
    let limited = with_token(&ws, cx, Scope::Full, Some(vec![id.as_str()]));
    let line = git_call(
        handle,
        &ws,
        cx,
        &limited,
        json!({"workspace": id, "folder": "wt", "op": "look"}),
    );
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::SCOPE)),
            format!("That repository lies outside {name}'s folder.")
        ),
        "{line}"
    );
}

#[gpui_kit::test]
fn a_control_token_can_t_discard_and_a_bad_path_is_invalid(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let repo = changed_repo();
    let real = repo.path().canonicalize().unwrap();
    let (id, _) = folder_of(&ws, cx, &real);
    let token = with_token(&ws, cx, Scope::Control, Some(vec![id.as_str()]));
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "discard", "paths": ["a.txt"]}),
    );
    assert_eq!(
        refused(&line),
        (
            Some(i64::from(codes::SCOPE)),
            "git's discard needs a Full token.".to_string()
        )
    );
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "stage", "paths": ["../x"]}),
    );
    assert_eq!(
        refused(&line).0,
        Some(i64::from(templum_mcp::rpc::PARAMS)),
        "{line}"
    );
    assert_eq!(
        std::fs::read_to_string(real.join("a.txt")).unwrap(),
        "two\n"
    );
}

#[gpui_kit::test]
fn a_limited_token_s_folder_outside_its_workspace_is_refused_before_git_runs(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    let repo = changed_repo();
    let elsewhere = committed_repo();
    let real = repo.path().canonicalize().unwrap();
    let (id, _) = folder_of(&ws, cx, &real);
    let limited = with_token(&ws, cx, Scope::Full, Some(vec![id.as_str()]));
    let outside = elsewhere.path().canonicalize().unwrap();
    for folder in [outside.to_str().unwrap(), "..", "/"] {
        let line = git_call(
            handle,
            &ws,
            cx,
            &limited,
            json!({"workspace": id, "folder": folder, "op": "stage", "paths": ["a.txt"]}),
        );
        assert_eq!(
            refused(&line).0,
            Some(i64::from(codes::SCOPE)),
            "{folder}: {line}"
        );
    }
    // Nothing was staged anywhere.
    assert_eq!(git_run(&real, &["diff", "--cached", "--name-only"]), "");
}

#[gpui_kit::test]
fn a_failed_git_answers_its_first_line_and_its_stderr(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let repo = changed_repo();
    let (id, _) = folder_of(&ws, cx, repo.path());
    let token = with_token(&ws, cx, Scope::Control, None);
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "checkout", "hash": "deadbeef"}),
    );
    let (failed, text) = said(&line);
    assert!(
        failed && (text.starts_with("fatal:") || text.starts_with("error:")),
        "{line}"
    );
    assert!(
        line["result"]["details"]
            .as_str()
            .unwrap()
            .contains("deadbeef"),
        "{line}"
    );
}

#[gpui_kit::test]
fn message_is_refused_until_home_asked_its_question(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let repo = changed_repo();
    git_run(repo.path(), &["add", "a.txt"]);
    let (id, _) = folder_of(&ws, cx, repo.path());
    let token = with_token(&ws, cx, Scope::Control, None);
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "message"}),
    );
    let computer = templum_api::computer();
    assert_eq!(
        said(&line),
        (
            true,
            format!(
                "Generate a message on {computer} once first: it asks there whether the diff may go to the agent's provider."
            )
        ),
        "{line}"
    );
}

#[gpui_kit::test]
fn git_on_a_mirrored_workspace_is_refused(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = super::remote::offline(cx);
    let words = ws.read_with(cx, |ws, _| {
        ws.space_named("r1").and_then(|s| s.mirror_words()).unwrap()
    });
    let token = with_token(&ws, cx, Scope::Full, None);
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": "r1", "op": "look"}),
    );
    assert_eq!(said(&line), (true, words), "{line}");
}

#[gpui_kit::test]
fn a_folder_without_a_repository_finds_none_and_init_makes_one(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let empty = tempfile::tempdir().unwrap();
    let real = empty.path().canonicalize().unwrap();
    let (id, _) = folder_of(&ws, cx, &real);
    let token = with_token(&ws, cx, Scope::Control, None);
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "find"}),
    );
    assert_eq!(line["result"]["git"]["root"], Value::Null, "{line}");
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "look"}),
    );
    assert_eq!(
        said(&line),
        (
            true,
            format!(
                "{} isn't in a Git repository: init makes one.",
                real.display()
            )
        )
    );
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "init"}),
    );
    let root = line["result"]["git"]["root"]
        .as_str()
        .unwrap_or_else(|| panic!("{line}"));
    assert_eq!(std::path::Path::new(root).canonicalize().unwrap(), real);
    let line = git_call(
        handle,
        &ws,
        cx,
        &token,
        json!({"workspace": id, "op": "find"}),
    );
    assert!(line["result"]["git"]["root"].is_string(), "{line}");
}

#[gpui_kit::test]
fn two_calls_on_one_repository_answer_in_turn(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let repo = changed_repo();
    let (id, _) = folder_of(&ws, cx, repo.path());
    let token = with_token(&ws, cx, Scope::Control, None);
    let first = git_later(
        handle,
        &ws,
        cx,
        &token,
        901,
        json!({"workspace": id, "op": "stage", "paths": ["a.txt"]}),
    );
    let second = git_later(
        handle,
        &ws,
        cx,
        &token,
        902,
        json!({"workspace": id, "op": "commit", "summary": "Two"}),
    );
    let (first, second) = (
        answer_of(handle, cx, &first),
        answer_of(handle, cx, &second),
    );
    assert_eq!(
        said(&first),
        (false, "Staged 1 file.".to_string()),
        "{first}"
    );
    let (failed, text) = said(&second);
    assert!(!failed && text.starts_with("Committed "), "{second}");
    assert_eq!(
        git_run(repo.path(), &["show", "--stat", "--format=%s", "HEAD"])
            .lines()
            .next(),
        Some("Two")
    );
    assert!(
        ws.read_with(cx, |ws, _| ws.api.git.is_empty()),
        "the queue is gone"
    );
}
