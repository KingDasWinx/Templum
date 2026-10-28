//! The Git popup on a mirrored workspace (docs/specs/templum-to-templum.md, The Git popup on a
//! remote workspace): every read and act is an operation of home's `git`, answered here by hand
//! (`ws.calls`); nothing local runs or is read.

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use templum_api::model::WorkspaceView;
use templum_api::store::Scope;
use templum_git::ops;
use templum_git::{Change, Entry, FileDiff, Status, WorkingDiff};

use super::remote::{
    answer, apply, live, live_terminals, refusal, sent, terminals_of_home, viewer,
};
use super::*;
use crate::git_block::Found;
use crate::workspace::remote::NOT_YET;

const ROOT: &str = "/home/ada/app";

/// A shown mirror of home's workspace `w1`, whose folder is `folder`, live on a share of `scope`
/// with a terminal `b1`.
fn on_mirror(
    cx: &mut TestAppContext,
    scope: Scope,
    folder: &str,
) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    on_mirror_with(cx, scope, folder, &[("b1", None)])
}

/// `on_mirror` with home's terminals `terminals`, each with its folder at home.
fn on_mirror_with(
    cx: &mut TestAppContext,
    scope: Scope,
    folder: &str,
    terminals: &[(&str, Option<&str>)],
) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let ids: Vec<&str> = terminals.iter().map(|(id, _)| *id).collect();
    let (handle, ws, dir) = live_terminals(cx, &ids);
    let mut state = terminals_of_home(&ids);
    state.workspaces = vec![Arc::new(WorkspaceView {
        id: "w1".into(),
        name: "API".into(),
        shown: true,
        folder: Some(folder.into()),
        ..WorkspaceView::default()
    })];
    for (block, (_, folder)) in state.blocks.iter_mut().zip(terminals) {
        Arc::make_mut(block).folder = folder.map(Into::into);
    }
    ws.update(cx, |ws, _| live(ws, scope));
    apply(handle, &ws, cx, state);
    // Its real connection to nowhere fails now, not in the middle of a test that reads answers.
    cx.run_until_parked();
    go_live(&ws, cx, scope);
    (handle, ws, dir)
}

/// The mirror is live again on a share of `scope`: drawing frames lets its real connection to
/// nowhere fail and change the status.
fn go_live(ws: &Entity<Workspace>, cx: &mut TestAppContext, scope: Scope) {
    ws.update(cx, |ws, _| live(ws, scope));
}

/// The only waiting call: its `op`, its arguments and whether it is on the slow lane.
fn call_op(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> (String, Value, bool) {
    ws.read_with(cx, |ws, _| {
        let all: Vec<_> = ws.calls.iter().map(|(_, c, _)| c.args.clone()).collect();
        assert_eq!(all.len(), 1, "{all:?}");
        let call = &ws.calls[0].1;
        assert_eq!(call.tool, "git");
        (
            call.args["op"].as_str().unwrap_or_default().to_string(),
            call.args.clone(),
            call.slow,
        )
    })
}

/// The ops of the waiting calls, in order.
fn waiting(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<String> {
    ws.read_with(cx, |ws, _| {
        ws.calls
            .iter()
            .map(|(_, c, _)| c.args["op"].as_str().unwrap_or(&c.tool).to_string())
            .collect()
    })
}

/// Home answers the first waiting call with `git`, and the answer is read (in the background).
fn reply(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, git: Value) {
    answer_with(
        handle,
        ws,
        cx,
        Ok(json!({"text": "ok", "git": git, "workspace": "w1"})),
    );
}

/// Home gives the first waiting call `answered`, and it is read (in the background).
fn answer_with(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    answered: templum_api::client::Answered,
) {
    answer(handle, ws, cx, 0, answered);
    cx.run_until_parked();
}

fn entry(path: &str, staged: Option<Change>, unstaged: Option<Change>) -> Entry {
    Entry {
        path: path.into(),
        from: None,
        staged,
        unstaged,
    }
}

fn look(entries: Vec<Entry>) -> Value {
    serde_json::to_value(ops::Look {
        status: Status {
            branch: Some("main".into()),
            oid: Some("a1b2c3d4".into()),
            entries,
            ..Default::default()
        },
        operation: None,
        author: Some(("Ada".into(), "ada@example.com".into())),
    })
    .unwrap()
}

fn modified(path: &str) -> Entry {
    entry(path, None, Some(Change::Modified))
}

fn diff_of(paths: &[&str]) -> Value {
    let file = |path: &str| FileDiff {
        old_path: Some(path.into()),
        new_path: Some(path.into()),
        ..FileDiff::empty()
    };
    serde_json::to_value(WorkingDiff {
        unstaged: paths.iter().map(|p| file(p)).collect(),
        staged: Vec::new(),
    })
    .unwrap()
}

fn toggle(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.toggle_git(window, cx))
    })
    .unwrap();
}

fn with_ws<R>(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    f: impl FnOnce(&mut Workspace, &mut gpui_kit::Window, &mut gpui_kit::Context<Workspace>) -> R,
) -> R {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| f(ws, window, cx))
    })
    .unwrap()
}

/// The popup opened on `ROOT` and read: find, look (a.txt modified), diff and default branch
/// answered in turn.
fn opened(
    cx: &mut TestAppContext,
    scope: Scope,
) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let (handle, ws, dir) = on_mirror(cx, scope, ROOT);
    read_in(handle, &ws, cx);
    (handle, ws, dir)
}

/// The popup opened on a mirror and read: find, look, diff and default branch.
fn read_in(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    let ws = ws.clone();
    toggle(handle, &ws, cx);
    reply(handle, &ws, cx, json!({"root": ROOT, "main_tree": ROOT}));
    reply(handle, &ws, cx, look(vec![modified("a.txt")]));
    reply(handle, &ws, cx, diff_of(&["a.txt"]));
    reply(handle, &ws, cx, json!("main"));
    assert!(waiting(&ws, cx).is_empty());
}

fn git_state<T>(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    read: impl FnOnce(&crate::git_block::Git) -> T,
) -> T {
    ws.read_with(cx, |ws, _| read(ws.space.git().expect("a Git")))
}

#[gpui_kit::test]
fn the_popup_on_a_remote_finds_home_s_repository_and_shows_its_status(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = on_mirror(cx, Scope::Control, ROOT);
    toggle(handle, &ws, cx);
    ws.read_with(cx, |ws, _| assert!(ws.git_open));
    git_state(&ws, cx, |g| {
        assert_eq!(g.home.as_deref(), Some(ROOT));
        assert!(matches!(g.found, Found::Looking));
    });
    assert_eq!(
        call_op(&ws, cx),
        (
            "find".into(),
            json!({"op": "find", "workspace": "w1", "folder": ROOT}),
            false
        )
    );
    reply(handle, &ws, cx, json!({"root": ROOT, "main_tree": ROOT}));
    git_state(&ws, cx, |g| {
        assert!(matches!(&g.found, Found::Home(r) if r == ROOT));
        assert!(g.repo().is_none());
    });
    let (op, args, _) = call_op(&ws, cx);
    assert_eq!((op.as_str(), &args["folder"]), ("look", &json!(ROOT)));
    reply(handle, &ws, cx, look(vec![modified("a.txt")]));
    git_state(&ws, cx, |g| {
        assert_eq!(g.status.entries[0].path, "a.txt");
        assert!(g.author.is_some());
        assert_eq!(g.reads, 1);
    });
    assert_eq!(
        call_op(&ws, cx),
        (
            "diff".into(),
            json!({"op": "diff", "ignore_whitespace": false, "whole": [], "workspace": "w1",
                "folder": ROOT}),
            false
        )
    );
    reply(handle, &ws, cx, diff_of(&["a.txt"]));
    git_state(&ws, cx, |g| assert_eq!(g.panel.files.len(), 1));
    assert_eq!(call_op(&ws, cx).0, "default_branch");
    reply(handle, &ws, cx, json!("main"));
    git_state(&ws, cx, |g| {
        assert_eq!(g.default_branch.as_deref(), Some("main"))
    });
    assert!(waiting(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn the_popup_on_a_remote_opens_on_the_focused_terminal_s_folder_then_another_then_the_workspace_s(
    cx: &mut TestAppContext,
) {
    let (handle, ws, _dir) = live_terminals(cx, &["b1", "b2"]);
    let with_folders = |folders: [Option<&str>; 2], workspace: Option<&str>| {
        let mut state = terminals_of_home(&["b1", "b2"]);
        state.workspaces = vec![Arc::new(WorkspaceView {
            id: "w1".into(),
            name: "API".into(),
            shown: true,
            folder: workspace.map(Into::into),
            ..WorkspaceView::default()
        })];
        for (block, folder) in state.blocks.iter_mut().zip(folders) {
            Arc::make_mut(block).folder = folder.map(Into::into);
        }
        state
    };
    apply(
        handle,
        &ws,
        cx,
        with_folders([Some("/a"), Some("/b")], Some("/w")),
    );
    let b2 = viewer(&ws, cx, "b2");
    ws.update(cx, |ws, _| ws.space.focused = Some(b2));
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.home_git_folder()).as_deref(),
        Some("/b")
    );
    ws.update(cx, |ws, _| ws.space.focused = None);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.home_git_folder()).as_deref(),
        Some("/a")
    );
    apply(handle, &ws, cx, with_folders([None, None], Some("/w")));
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.home_git_folder()).as_deref(),
        Some("/w")
    );
    apply(handle, &ws, cx, with_folders([None, None], None));
    assert_eq!(ws.read_with(cx, |ws, _| ws.home_git_folder()), None);
    toggle(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.git.is_none() && !ws.git_open);
        assert!(
            ws.said
                .last()
                .unwrap()
                .starts_with("No folder to open Git in on ")
        );
        assert!(ws.calls.is_empty());
    });
}

#[gpui_kit::test]
fn a_remote_git_never_looks_at_this_computer_s_folder_of_the_same_path(cx: &mut TestAppContext) {
    let repo = committed_repo();
    let path = repo.path().display().to_string();
    let (handle, ws, _dir) = on_mirror(cx, Scope::Control, &path);
    toggle(handle, &ws, cx);
    settle(handle, cx);
    settle(handle, cx);
    git_state(&ws, cx, |g| {
        assert!(
            matches!(g.found, Found::Looking),
            "a local discover would have found it"
        );
        assert_eq!(g.folder, std::path::PathBuf::new());
        assert!(g.repo().is_none());
    });
    with_ws(handle, &ws, cx, |ws, window, cx| ws.poll_git(window, cx));
    assert_eq!(waiting(&ws, cx), ["find"]);
    reply(handle, &ws, cx, json!({"root": path, "main_tree": path}));
    settle(handle, &mut *cx);
    git_state(&ws, cx, |g| {
        assert!(matches!(&g.found, Found::Home(r) if *r == path));
        assert!(g.repo().is_none());
    });
    ws.read_with(cx, |ws, _| {
        assert!(ws.calls.iter().all(|(_, c, _)| c.tool == "git"))
    });
}

#[gpui_kit::test]
fn the_panel_on_a_mirror_reads_the_diff_when_the_status_changes_or_ten_seconds_pass(
    cx: &mut TestAppContext,
) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    let poll = |cx: &mut TestAppContext| {
        with_ws(handle, &ws, cx, |ws, window, cx| ws.poll_git(window, cx));
        assert_eq!(call_op(&ws, cx).0, "look");
    };
    poll(cx);
    reply(handle, &ws, cx, look(vec![modified("a.txt")]));
    assert!(waiting(&ws, cx).is_empty(), "the same status asks no diff");
    ws.update(cx, |ws, _| {
        let git = ws.space.git_mut().unwrap();
        let (status, _) = git.home_diff.clone().unwrap();
        git.home_diff = Some((
            status,
            Instant::now().checked_sub(Duration::from_secs(11)).unwrap(),
        ));
    });
    poll(cx);
    reply(handle, &ws, cx, look(vec![modified("a.txt")]));
    assert_eq!(call_op(&ws, cx).0, "diff", "ten seconds passed");
    reply(handle, &ws, cx, diff_of(&["a.txt"]));
    poll(cx);
    reply(
        handle,
        &ws,
        cx,
        look(vec![modified("a.txt"), modified("b.txt")]),
    );
    assert_eq!(call_op(&ws, cx).0, "diff", "the status changed");
    reply(handle, &ws, cx, diff_of(&["a.txt", "b.txt"]));
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.refresh_git_now(window, cx)
    });
    assert_eq!(call_op(&ws, cx).0, "look");
    reply(
        handle,
        &ws,
        cx,
        look(vec![modified("a.txt"), modified("b.txt")]),
    );
    assert_eq!(call_op(&ws, cx).0, "diff", "Refresh reads the diff too");
}

#[gpui_kit::test]
fn an_answer_for_a_folder_left_since_changes_nothing(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.poll_git(window, cx));
    assert_eq!(waiting(&ws, cx), ["look"]);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.set_home_git_folder("/home/ada/other".into(), window, cx)
    });
    assert_eq!(waiting(&ws, cx), ["look", "find"]);
    // The look of the folder left: dropped.
    reply(
        handle,
        &ws,
        cx,
        look(vec![entry("a.txt", None, Some(Change::Deleted))]),
    );
    git_state(&ws, cx, |g| {
        assert!(g.status.entries.is_empty());
        assert_eq!(g.reads, 0);
        assert_eq!(g.home.as_deref(), Some("/home/ada/other"));
    });
    assert_eq!(waiting(&ws, cx), ["find"]);
    reply(
        handle,
        &ws,
        cx,
        json!({"root": "/home/ada/other", "main_tree": null}),
    );
    git_state(&ws, cx, |g| {
        assert!(matches!(&g.found, Found::Home(r) if r == "/home/ada/other"))
    });
    // Another workspace shown while a call waits: its answer lands on the Git it was for.
    assert_eq!(waiting(&ws, cx), ["look"]);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx)
    });
    reply(handle, &ws, cx, look(vec![modified("c.txt")]));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.id, ws.spaces().next().unwrap().id)
    });
}

#[gpui_kit::test]
fn a_popup_on_a_remote_that_isnt_live_polls_nothing(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = templum_api::client::Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        };
    });
    for _ in 0..2 {
        with_ws(handle, &ws, cx, |ws, window, cx| ws.poll_git(window, cx));
    }
    assert!(waiting(&ws, cx).is_empty());
    git_state(&ws, cx, |g| assert_eq!(g.error, None));
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.set_git_tab(crate::git_block::Tab::History, window, cx)
    });
    assert!(waiting(&ws, cx).is_empty());
    git_state(&ws, cx, |g| {
        assert_eq!(g.error.as_deref(), Some("Not connected."))
    });
}

#[gpui_kit::test]
fn a_malformed_answer_from_home_is_told_and_frees_the_popup(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.poll_git(window, cx));
    reply(handle, &ws, cx, json!({"status": 7}));
    git_state(&ws, cx, |g| {
        assert!(
            g.error
                .as_deref()
                .unwrap_or_default()
                .starts_with("git: The answer couldn't be read"),
            "{:?}",
            g.error
        );
        assert!(!g.busy);
    });
    with_ws(handle, &ws, cx, |ws, window, cx| ws.poll_git(window, cx));
    assert_eq!(call_op(&ws, cx).0, "look");
}

#[gpui_kit::test]
fn a_mirror_s_git_is_kept_in_memory_only(cx: &mut TestAppContext) {
    let (handle, ws, dir) = opened(cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        let summary = ws.space.git().unwrap().summary.clone();
        summary.update(cx, |s, cx| {
            s.set_value("Fix the mirror's login", window, cx)
        });
        ws.snapshot(window, cx);
    });
    cx.run_until_parked();
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    assert!(!dir.path().join("workspaces").join("r1.json").exists());
    let mut held = Vec::new();
    let mut folders = vec![dir.path().to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(folder).unwrap().flatten() {
            let path = entry.path();
            // `bin` holds a copy of this very test program, which holds the text.
            if path.is_dir() {
                if path.file_name().is_none_or(|name| name != "bin") {
                    folders.push(path);
                }
            } else if path.is_file()
                && String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default())
                    .contains("Fix the mirror's login")
            {
                held.push(path);
            }
        }
    }
    assert!(held.is_empty(), "{held:?}");
}

/// Answers the waiting reads (status, diff, default branch) with `status`'s entries, until the
/// first waiting call that is not one, or none waits.
fn settle_reads(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    entries: &[Entry],
) {
    loop {
        let ops = waiting(ws, cx);
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        match ops.first().map(String::as_str) {
            Some("look") => reply(handle, ws, cx, look(entries.to_vec())),
            Some("diff") => reply(handle, ws, cx, diff_of(&paths)),
            Some("default_branch") => reply(handle, ws, cx, json!("main")),
            _ => return,
        }
    }
}

fn staged(path: &str) -> Entry {
    entry(path, Some(Change::Modified), None)
}

/// Home's status shows `entries`, read and answered.
fn status_is(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    entries: Vec<Entry>,
) {
    with_ws(handle, ws, cx, |ws, window, cx| ws.poll_git(window, cx));
    settle_reads(handle, ws, cx, &entries);
}

fn summary_of(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> String {
    ws.read_with(cx, |ws, cx| {
        ws.space.git().unwrap().summary.read(cx).value().to_string()
    })
}

fn type_summary(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    text: &str,
) {
    with_ws(handle, ws, cx, |ws, window, cx| {
        let summary = ws.space.git().unwrap().summary.clone();
        summary.update(cx, |s, cx| s.set_value(text.to_string(), window, cx));
    });
}

#[gpui_kit::test]
fn staging_and_committing_on_a_mirror_run_at_home_one_at_a_time(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.stage(vec!["a.txt".into()], window, cx)
    });
    assert_eq!(
        call_op(&ws, cx),
        (
            "stage".into(),
            json!({"op": "stage", "paths": ["a.txt"], "workspace": "w1", "folder": ROOT}),
            false
        )
    );
    git_state(&ws, cx, |g| assert_eq!(g.acting, 1));
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.unstage(vec!["a.txt".into()], window, cx)
    });
    assert_eq!(waiting(&ws, cx), ["stage"], "one at a time");
    git_state(&ws, cx, |g| assert_eq!(g.acting, 2));
    reply(handle, &ws, cx, Value::Null);
    // The read the first one asks for, then the second act, in order.
    let mut seen = Vec::new();
    while waiting(&ws, cx).first().is_some_and(|op| op != "unstage") {
        seen.push(waiting(&ws, cx).remove(0));
        settle_reads_once(handle, &ws, cx, vec![staged("a.txt")]);
    }
    assert_eq!(seen, ["look", "diff", "default_branch"]);
    reply(handle, &ws, cx, Value::Null);
    git_state(&ws, cx, |g| assert_eq!(g.acting, 0));
    settle_reads(handle, &ws, cx, &[staged("a.txt")]);
    // A commit of the staged file.
    status_is(handle, &ws, cx, vec![staged("a.txt")]);
    type_summary(handle, &ws, cx, "Fix login");
    let before = notification_count(handle, cx);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.git_commit(false, window, cx)
    });
    assert_eq!(
        call_op(&ws, cx),
        (
            "commit".into(),
            json!({"op": "commit", "summary": "Fix login", "description": "", "amend": false,
                "all": false, "workspace": "w1", "folder": ROOT}),
            true // pre-commit and commit-msg hooks may take minutes
        )
    );
    reply(
        handle,
        &ws,
        cx,
        json!({"short": "a1b2c3d", "replaced": null}),
    );
    assert_eq!(summary_of(&ws, cx), "");
    assert_eq!(notification_count(handle, cx), before + 1);
}

/// Answers the first waiting call, a read, with `entries`.
fn settle_reads_once(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    entries: Vec<Entry>,
) {
    let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
    match waiting(ws, cx).first().map(String::as_str) {
        Some("look") => reply(handle, ws, cx, look(entries.clone())),
        Some("diff") => reply(handle, ws, cx, diff_of(&paths)),
        Some("default_branch") => reply(handle, ws, cx, json!("main")),
        _ => {}
    }
}

#[gpui_kit::test]
fn a_commit_home_says_changed_meanwhile_keeps_the_message(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    status_is(handle, &ws, cx, vec![staged("a.txt")]);
    type_summary(handle, &ws, cx, "Fix login");
    let before = notification_count(handle, cx);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.git_commit(false, window, cx)
    });
    assert_eq!(call_op(&ws, cx).0, "commit");
    answer_with(handle, &ws, cx, refusal(ops::CHANGED_MEANWHILE));
    assert_eq!(summary_of(&ws, cx), "Fix login");
    assert_eq!(notification_count(handle, cx), before + 1);
    assert_eq!(waiting(&ws, cx), ["look"]);
}

#[gpui_kit::test]
fn git_s_network_operations_go_on_the_slow_lane(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    let entries = [modified("a.txt")];
    with_ws(handle, &ws, cx, |ws, window, cx| ws.fetch(window, cx));
    assert_eq!(
        call_op(&ws, cx),
        (
            "fetch".into(),
            json!({"op": "fetch", "workspace": "w1", "folder": ROOT}),
            true
        )
    );
    reply(handle, &ws, cx, Value::Null);
    settle_reads(handle, &ws, cx, &entries);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.pull(templum_git::Pull::FastForward, window, cx)
    });
    let (op, args, slow) = call_op(&ws, cx);
    assert_eq!(
        (op.as_str(), &args["how"], slow),
        ("pull", &json!("fast_forward"), true)
    );
    reply(handle, &ws, cx, Value::Null);
    settle_reads(handle, &ws, cx, &entries);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.push_now(window, cx));
    let (op, _, slow) = call_op(&ws, cx);
    assert_eq!((op.as_str(), slow), ("push", true));
    reply(handle, &ws, cx, Value::Null);
    settle_reads(handle, &ws, cx, &entries);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.sync_git(window, cx));
    let (op, _, slow) = call_op(&ws, cx);
    assert_eq!((op.as_str(), slow), ("sync", true));
    reply(handle, &ws, cx, json!({"done": {"got": 1, "sent": 0}}));
    settle_reads(handle, &ws, cx, &entries);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.stage(vec!["a.txt".into()], window, cx)
    });
    assert!(!call_op(&ws, cx).2, "stage is quick");
}

#[gpui_kit::test]
fn a_push_cut_on_the_way_says_it_may_still_be_running_and_reads_again(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.push_now(window, cx));
    let cut = "127.0.0.1:9 didn't answer in time: it may still be running there.";
    answer_with(handle, &ws, cx, refusal(cut));
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.said.last().map(String::as_str), Some(cut))
    });
    git_state(&ws, cx, |g| assert!(g.sync.is_none()));
    assert_eq!(waiting(&ws, cx), ["look"]);
}

#[gpui_kit::test]
fn an_act_on_a_remote_that_isnt_live_says_not_connected_and_sends_nothing(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = templum_api::client::Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        };
    });
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.stage(vec!["a.txt".into()], window, cx);
        ws.create_branch("x".into(), None, window, cx);
        ws.push_now(window, cx);
    });
    assert!(waiting(&ws, cx).is_empty());
    ws.read_with(cx, |ws, _| {
        let said: Vec<_> = ws.said.iter().rev().take(3).collect();
        assert!(said.iter().all(|s| *s == "Not connected."), "{said:?}");
    });
    git_state(&ws, cx, |g| {
        assert_eq!(g.acting, 0);
        assert!(!g.busy);
        assert!(g.sync.is_none());
    });
}

#[gpui_kit::test]
fn the_spinning_sync_on_a_mirror_cannot_stop_home_s_push(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.push_now(window, cx));
    git_state(&ws, cx, |g| {
        assert_eq!(g.sync, Some(crate::git_block::Sync::Push))
    });
    ws.update(cx, |ws, cx| ws.cancel_git(cx));
    git_state(&ws, cx, |g| {
        assert!(
            g.cancel
                .as_ref()
                .is_none_or(|flag| !flag.load(std::sync::atomic::Ordering::Relaxed))
        )
    });
    reply(handle, &ws, cx, Value::Null);
    git_state(&ws, cx, |g| assert!(g.sync.is_none()));
}

fn commit(n: u32) -> templum_git::Commit {
    templum_git::Commit {
        hash: format!("{n}{}", "a".repeat(39)),
        short: format!("{n}aaaaaa"),
        author: "Ada".into(),
        email: "ada@example.com".into(),
        time: 1_700_000_000 - i64::from(n),
        date: "2026-10-11 16:14".into(),
        parents: Vec::new(),
        refs: Vec::new(),
        subject: format!("Commit {n}"),
        body: String::new(),
    }
}

fn set_tab(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    tab: crate::git_block::Tab,
) {
    with_ws(handle, ws, cx, |ws, window, cx| {
        ws.set_git_tab(tab, window, cx)
    });
}

#[gpui_kit::test]
fn history_branches_and_stashes_on_a_mirror_read_home_s_lists(cx: &mut TestAppContext) {
    use crate::git_block::Tab;
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    set_tab(handle, &ws, cx, Tab::History);
    assert_eq!(call_op(&ws, cx).0, "default_branch");
    reply(handle, &ws, cx, json!("main"));
    let (op, args, slow) = call_op(&ws, cx);
    assert_eq!(op, "history");
    assert_eq!(
        args,
        json!({"op": "history", "all": false, "search": null, "path": null, "skip": 0,
            "workspace": "w1", "folder": ROOT})
    );
    assert!(!slow);
    let page = ops::Page {
        commits: vec![commit(1), commit(2)],
        paths: Vec::new(),
        unpushed: Some(Default::default()),
    };
    reply(handle, &ws, cx, serde_json::to_value(page).unwrap());
    git_state(&ws, cx, |g| assert_eq!(g.commits.len(), 2));
    let hash = commit(1).hash;
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.show_commit(hash.clone(), window, cx)
    });
    let (op, args, _) = call_op(&ws, cx);
    assert_eq!(
        (op.as_str(), &args["hash"], &args["ignore_whitespace"]),
        ("show", &json!(hash), &json!(false))
    );
    let shown = ops::CommitShown {
        commit: commit(1),
        files: vec![templum_git::CommitFile {
            change: Change::Modified,
            path: "a.txt".into(),
            from: None,
        }],
        diffs: vec![FileDiff {
            old_path: Some("a.txt".into()),
            new_path: Some("a.txt".into()),
            ..FileDiff::empty()
        }],
    };
    reply(handle, &ws, cx, serde_json::to_value(shown).unwrap());
    git_state(&ws, cx, |g| {
        assert!(g.shown.is_some());
        assert_eq!(g.history_panel.files.len(), 1);
    });
    set_tab(handle, &ws, cx, Tab::Branches);
    assert_eq!(call_op(&ws, cx).0, "default_branch");
    reply(handle, &ws, cx, json!("main"));
    assert_eq!(call_op(&ws, cx).0, "branches");
    let branch = templum_git::Branch {
        name: "main".into(),
        remote: false,
        current: true,
        commit: "a1b2c3d".into(),
        upstream: None,
        gone: false,
        ahead: 0,
        behind: 0,
        subject: "Commit 1".into(),
        time: 0,
    };
    reply(handle, &ws, cx, json!([branch]));
    git_state(&ws, cx, |g| assert_eq!(g.branches.len(), 1));
    set_tab(handle, &ws, cx, Tab::Stashes);
    assert_eq!(call_op(&ws, cx).0, "default_branch");
    reply(handle, &ws, cx, json!("main"));
    assert_eq!(call_op(&ws, cx).0, "stashes");
    reply(handle, &ws, cx, json!([]));
    git_state(&ws, cx, |g| assert!(g.stashes.is_empty()));
    assert!(waiting(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn a_default_branch_home_refuses_is_none(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    git_state(&ws, cx, |g| {
        assert_eq!(g.default_branch.as_deref(), Some("main"))
    });
    let before = notification_count(handle, cx);
    set_tab(handle, &ws, cx, crate::git_block::Tab::Changes);
    assert_eq!(call_op(&ws, cx).0, "default_branch");
    answer_with(handle, &ws, cx, refusal("x"));
    git_state(&ws, cx, |g| assert_eq!(g.default_branch, None));
    assert_eq!(notification_count(handle, cx), before);
}

fn kept() -> Value {
    json!({"id": "17-3", "root": ROOT, "time": 0, "what": "Discarded a.txt",
        "files": [["a.txt", true]]})
}

#[gpui_kit::test]
fn discard_on_a_mirror_asks_with_home_s_measure_then_discards_there(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = opened(cx, Scope::Full);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.discard(vec!["a.txt".into()], window, cx)
    });
    assert_eq!(
        call_op(&ws, cx),
        (
            "measure".into(),
            json!({"op": "measure", "paths": ["a.txt"], "workspace": "w1", "folder": ROOT}),
            false
        )
    );
    reply(
        handle,
        &ws,
        cx,
        json!({"files": 1, "bytes": 4, "keep": true}),
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "asks first");
    })
    .unwrap();
    go_live(&ws, cx, Scope::Full);
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    assert_eq!(
        call_op(&ws, cx),
        (
            "discard".into(),
            json!({"op": "discard", "paths": ["a.txt"], "workspace": "w1", "folder": ROOT}),
            false
        )
    );
    reply(handle, &ws, cx, kept());
    settle_reads(handle, &ws, cx, &[]);
    settle(handle, cx);
    go_live(&ws, cx, Scope::Full);
    cx.update_window(handle, |_, window, cx| window.click("git-undo", cx))
        .unwrap();
    assert_eq!(
        call_op(&ws, cx),
        (
            "restore".into(),
            json!({"op": "restore", "entry": "17-3", "force": false, "workspace": "w1",
                "folder": ROOT}),
            false
        )
    );
}

#[gpui_kit::test]
fn recently_discarded_on_a_mirror_lists_home_s_trash(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (handle, ws, _dir) = opened(cx, Scope::Full);
    ws.update(cx, |ws, _| ws.trash = None);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.show_discarded(window, cx)
    });
    assert_eq!(call_op(&ws, cx).0, "trash");
    reply(handle, &ws, cx, json!([kept()]));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_control_share_reaches_no_full_operation(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    ws.update(cx, |ws, _| {
        let git = ws.space.git_mut().unwrap();
        git.operation = Some(templum_git::Operation::Merge);
        git.stashes = vec![templum_git::Stash {
            index: 0,
            hash: "b".repeat(40),
            time: 0,
            message: "On main: x".into(),
        }];
    });
    let said_before = ws.read_with(cx, |ws, _| ws.said.len());
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.discard(vec!["a.txt".into()], window, cx);
        ws.discard_hunk("a.txt".into(), "@@ patch".into(), window, cx);
        ws.restore_version("c".repeat(40), "a.txt".into(), window, cx);
        ws.delete_branch("x".into(), false, window, cx);
        ws.stash_drop(0, window, cx);
        ws.abort_operation(window, cx);
    });
    assert!(waiting(&ws, cx).is_empty());
    ws.read_with(cx, |ws, _| {
        let said = &ws.said[said_before..];
        assert_eq!(said.len(), 6, "{said:?}");
        assert!(
            said.iter()
                .all(|s| s.starts_with("Needs a Full share: share again on ")),
            "{said:?}"
        );
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
    })
    .unwrap();
    // The row's hover Discard, and the `d` key.
    go_live(&ws, cx, Scope::Control);
    super::click_row_button(
        handle,
        cx,
        crate::git_block::row_id("files", 0),
        super::git_button("discard", "files", 0),
    );
    assert!(waiting(&ws, cx).is_empty(), "{:?}", waiting(&ws, cx));
    go_live(&ws, cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.open_git(window, cx));
    ws.update(cx, |ws, _| ws.calls.clear());
    cx.update_window(handle, |_, window, cx| window.press("d", cx))
        .unwrap();
    assert!(waiting(&ws, cx).is_empty());
    go_live(&ws, cx, Scope::Control);
    ws.read_with(cx, |ws, _| {
        let why = ws.why_not(super::super::remote::Deed::Git("stage"));
        assert!(why.is_none(), "{why:?} {}", ws.space.id);
        assert!(
            ws.why_not(super::super::remote::Deed::Git("discard"))
                .is_some()
        );
    });
}

#[gpui_kit::test]
fn the_folder_menu_on_a_mirror_lists_home_s_folders_and_opens_a_terminal_there(
    cx: &mut TestAppContext,
) {
    let (handle, ws, _dir) = on_mirror_with(
        cx,
        Scope::Control,
        ROOT,
        &[
            ("b1", Some("/home/ada/app/src")),
            ("b2", Some("/home/ada/lib")),
        ],
    );
    read_in(handle, &ws, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.home_git_folders(), ["/home/ada/lib"]);
        let b1 = ws.space.remote.as_ref().unwrap().homes["b1"];
        assert_eq!(ws.home_terminals_in(ROOT), [(b1, "b1".to_string())]);
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("git-folder", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.menu.is_some());
        assert!(ws.said.is_empty(), "{:?}", ws.said);
    });
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.open_terminal_at_home(ROOT.into(), window, cx)
    });
    let calls = sent(&ws, cx);
    assert_eq!(calls.len(), 1, "{calls:?}");
    let (tool, args) = &calls[0];
    assert_eq!(tool, "add_block");
    assert_eq!(
        (&args["workspace"], &args["kind"], &args["folder"]),
        (&json!("w1"), &json!("terminal"), &json!(ROOT))
    );
    assert!(args["x"].is_number() && args["y"].is_number());
    ws.read_with(cx, |ws, _| assert!(!ws.git_open));
}

/// Kanban card 203: the folder menu on a mirror lists home's recent folders of the workspace
/// (`recent_folders`, read when the popup and the menu open) after its folder and its terminals',
/// each once, less the one shown and those inside its root; one chosen points the Git there, at
/// home. An older home, without the tool, isn't asked.
#[gpui_kit::test]
fn the_folder_menu_on_a_mirror_lists_home_s_recent_folders(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = on_mirror_with(
        cx,
        Scope::Control,
        ROOT,
        &[
            ("b1", Some("/home/ada/app/src")),
            ("b2", Some("/home/ada/lib")),
        ],
    );
    let tools_of_home = |ws: &Entity<Workspace>, cx: &mut TestAppContext, tools: Value| {
        ws.update(cx, |ws, _| {
            ws.space.remote.as_mut().unwrap().tools =
                Some(templum_api::client::Tools::read(&tools));
        })
    };
    tools_of_home(&ws, cx, json!(templum_api::registry::listed()));
    let asked = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        sent(ws, cx)
            .into_iter()
            .position(|(tool, _)| tool == "recent_folders")
    };
    toggle(handle, &ws, cx);
    let n = asked(&ws, cx).expect("read when the popup opens");
    assert_eq!(sent(&ws, cx)[n].1, json!({"workspace": "w1"}));
    let recent = "/home/ada/web\n/home/ada/lib\n/home/ada/app/src\n/home/ada/app\n/home/ada/docs";
    answer(handle, &ws, cx, n, Ok(json!({ "text": recent })));
    reply(handle, &ws, cx, json!({"root": ROOT, "main_tree": ROOT}));
    reply(handle, &ws, cx, look(vec![modified("a.txt")]));
    reply(handle, &ws, cx, diff_of(&["a.txt"]));
    reply(handle, &ws, cx, json!("main"));
    assert!(waiting(&ws, cx).is_empty(), "{:?}", waiting(&ws, cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.home_git_folders(),
            ["/home/ada/lib", "/home/ada/web", "/home/ada/docs"]
        );
    });
    // The menu reads them again, for its next opening.
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("git-folder", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.menu.is_some());
        assert!(ws.said.is_empty(), "{:?}", ws.said);
    });
    let n = asked(&ws, cx).expect("read when the menu opens");
    answer(handle, &ws, cx, n, Ok(json!({ "text": "/home/ada/notes" })));
    cx.run_until_parked();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.home_git_folders(), ["/home/ada/lib", "/home/ada/notes"]);
    });
    // Its item points the Git at that folder at home, as the others do.
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.set_home_git_folder("/home/ada/notes".into(), window, cx)
    });
    let (op, args, _) = call_op(&ws, cx);
    assert_eq!(
        (op.as_str(), &args["folder"], &args["workspace"]),
        ("find", &json!("/home/ada/notes"), &json!("w1"))
    );
    ws.update(cx, |ws, _| ws.calls.clear());
    // A home whose Templum has no `recent_folders` isn't asked.
    tools_of_home(&ws, cx, json!([{"name": "git"}]));
    with_ws(handle, &ws, cx, |ws, window, cx| ws.open_git(window, cx));
    assert_eq!(asked(&ws, cx), None, "{:?}", sent(&ws, cx));
}

#[test]
fn home_s_recent_folders_are_cut_and_nothing_is_cleaned_into_another_folder() {
    use crate::workspace::remote::{MOST_FOLDERS, home_recent};
    let many: Vec<String> = (0..30).map(|n| format!("/f{n}")).collect();
    let text = format!(
        "/a\n/a\n\n /b\n/c\u{7}d\n/e\u{202e}\n/two  spaces\n/{}\n{}",
        "x".repeat(400),
        many.join("\n")
    );
    let recent = home_recent(&text);
    assert_eq!(recent.len(), MOST_FOLDERS);
    assert_eq!(
        recent[..3],
        ["/a", "/f0", "/f1"],
        "once, and nothing cleaned"
    );
}

#[gpui_kit::test]
fn generate_on_a_mirror_asks_home_s_agent_on_the_slow_lane(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    ws.update(cx, |ws, _| ws.commit_agent = None);
    status_is(handle, &ws, cx, vec![staged("a.txt")]);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.generate_message(window, cx)
    });
    assert_eq!(
        call_op(&ws, cx),
        (
            "message".into(),
            json!({"op": "message", "all": false, "workspace": "w1", "folder": ROOT}),
            true
        )
    );
    git_state(&ws, cx, |g| assert!(g.generating.is_some()));
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.generate_message(window, cx)
    });
    assert_eq!(
        waiting(&ws, cx),
        ["message"],
        "a click while it runs does nothing"
    );
    reply(
        handle,
        &ws,
        cx,
        json!({"summary": "Fix login", "description": "Because.", "committed": false}),
    );
    assert_eq!(summary_of(&ws, cx), "Fix login");
    git_state(&ws, cx, |g| assert!(g.generating.is_none()));
    let before = notification_count(handle, cx);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.generate_message(window, cx)
    });
    answer_with(
        handle,
        &ws,
        cx,
        refusal(
            "Generate a message on 127.0.0.1:9 once first: it asks there whether the diff may go to the agent's provider.",
        ),
    );
    assert_eq!(notification_count(handle, cx), before + 1);
    assert_eq!(summary_of(&ws, cx), "Fix login");
}

#[gpui_kit::test]
fn the_popup_on_a_mirror_offers_none_of_what_stays_at_home(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let (handle, ws, _dir) = opened(cx, Scope::Full);
    status_is(handle, &ws, cx, vec![staged("a.txt")]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("git-generate-menu", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert!(ws.menu.is_none()));
    let blocks = ws.read_with(cx, |ws, _| ws.space.canvas.blocks().len());
    let said = ws.read_with(cx, |ws, _| ws.said.len());
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.ask_clone(window, cx);
        ws.choose_git_folder(window, cx);
        ws.open_from_git_at("a.txt", None, window, cx);
    });
    ws.read_with(cx, |ws, _| {
        let said = &ws.said[said..];
        assert_eq!(said, [NOT_YET, NOT_YET, NOT_YET]);
        assert_eq!(ws.space.canvas.blocks().len(), blocks);
        assert_eq!(ws.send_to_targets(), ws.remote_terminals());
    });
    assert!(waiting(&ws, cx).is_empty());
    cx.update_window(handle, |_, window, cx| {
        assert!(!window.has_active_dialog(cx))
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_question_answered_after_another_workspace_is_shown_does_nothing(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = opened(cx, Scope::Full);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.delete_branch("x".into(), false, window, cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "asks first");
    })
    .unwrap();
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.said.last().map(String::as_str),
            Some("Another workspace is shown now: nothing was done.")
        );
        assert!(ws.calls.is_empty(), "nothing was sent");
    });
}

/// Home's trash lists a discard with id `id`.
fn home_kept(id: &str) -> Value {
    json!({"id": id, "root": ROOT, "time": 0, "what": "Discarded a.txt", "files": [["a.txt", true]]})
}

/// This computer's own repository, in the first workspace, whose a.txt was discarded and kept in
/// the trash (the file is "one\n" now, "changed\n" in the trash).
struct Local {
    repo: tempfile::TempDir,
    _data: tempfile::TempDir,
    trash: templum_git::trash::Trash,
    kept: templum_git::trash::Kept,
}

/// `Local`, found and read by the first workspace's Git; the mirror stays shown.
fn local_discard(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> Local {
    let repo = committed_repo();
    std::fs::write(repo.path().join("a.txt"), "changed\n").unwrap();
    let data = tempfile::tempdir().unwrap();
    let trash = templum_git::trash::Trash::new(data.path().join("trash"));
    let kept = trash
        .put(
            repo.path(),
            &["a.txt".to_string()],
            "Discarded a.txt",
            std::time::SystemTime::now(),
        )
        .unwrap();
    std::fs::write(repo.path().join("a.txt"), "one\n").unwrap();
    trash.seal(&kept).unwrap();
    ws.update(cx, |ws, _| ws.trash = Some(trash.clone()));
    with_ws(handle, ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx);
        assert!(ws.set_git_folder(repo.path(), window, cx));
        ws.switch_to(1, window, cx);
    });
    settle(handle, cx);
    go_live(ws, cx, Scope::Full);
    Local {
        repo,
        _data: data,
        trash,
        kept,
    }
}

/// The waiting calls to home that restore: their arguments.
fn restores(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<Value> {
    ws.read_with(cx, |ws, _| {
        ws.calls
            .iter()
            .map(|(_, c, _)| c.args.clone())
            .filter(|a| a["op"] == "restore")
            .collect()
    })
}

fn local_file(local: &Local) -> String {
    std::fs::read_to_string(local.repo.path().join("a.txt")).unwrap()
}

#[gpui_kit::test]
fn a_restore_clicked_after_another_workspace_is_shown_goes_to_the_computer_that_kept_it(
    cx: &mut TestAppContext,
) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = on_mirror(cx, Scope::Full, ROOT);
    let local = local_discard(handle, &ws, cx);
    read_in(handle, &ws, cx);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.show_discarded(window, cx)
    });
    // Home's list names an id that is also one of this computer's trash.
    reply(handle, &ws, cx, json!([home_kept(&local.kept.id)]));
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "the list stays open");
        window.click(("git-restore", 0usize), cx);
    })
    .unwrap();
    // The click shows the mirror again, and the restore is asked of home, not of this trash.
    assert!(ws.read_with(cx, |ws, _| ws.on_remote()));
    assert_eq!(
        restores(&ws, cx),
        [
            json!({"op": "restore", "entry": local.kept.id, "force": false, "workspace": "w1",
            "folder": ROOT})
        ]
    );
    assert_eq!(
        local.trash.list().len(),
        1,
        "this computer's trash is untouched"
    );
    assert_eq!(local_file(&local), "one\n");
}

#[gpui_kit::test]
fn the_question_over_a_changed_file_answered_after_another_workspace_is_shown_goes_to_its_own(
    cx: &mut TestAppContext,
) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = on_mirror(cx, Scope::Full, ROOT);
    let local = local_discard(handle, &ws, cx);
    read_in(handle, &ws, cx);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.show_discarded(window, cx)
    });
    reply(handle, &ws, cx, json!([home_kept(&local.kept.id)]));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(("git-restore", 0usize), cx);
    })
    .unwrap();
    assert_eq!(waiting(&ws, cx), ["restore"]);
    reply(handle, &ws, cx, json!({"changed": ["a.txt"]}));
    settle_reads(handle, &ws, cx, &[modified("a.txt")]);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "asks before writing over");
    })
    .unwrap();
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    // Forced, at home: this computer's trash is not asked.
    assert_eq!(
        restores(&ws, cx),
        [
            json!({"op": "restore", "entry": local.kept.id, "force": true, "workspace": "w1",
            "folder": ROOT})
        ]
    );
    assert_eq!(local.trash.list().len(), 1);
    assert_eq!(local_file(&local), "one\n");
}

#[gpui_kit::test]
fn the_question_over_a_changed_local_file_answered_on_a_mirror_restores_the_local_one(
    cx: &mut TestAppContext,
) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = on_mirror(cx, Scope::Full, ROOT);
    let local = local_discard(handle, &ws, cx);
    read_in(handle, &ws, cx);
    std::fs::write(local.repo.path().join("a.txt"), "edited since then\n").unwrap();
    let id = local.kept.id.clone();
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx);
        let at = ws.here();
        ws.restore_discarded(at, id, false, window, cx);
    });
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "asks before writing over");
    })
    .unwrap();
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(1, window, cx)
    });
    go_live(&ws, cx, Scope::Full);
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    settle(handle, cx);
    assert!(
        restores(&ws, cx).is_empty(),
        "this computer's id never reaches home"
    );
    assert_eq!(local_file(&local), "changed\n", "restored here");
    assert!(
        local.trash.list().iter().all(|k| k.id != local.kept.id),
        "it left the trash"
    );
}

#[gpui_kit::test]
fn a_discard_confirmed_after_another_workspace_is_shown_does_nothing(cx: &mut TestAppContext) {
    no_motion(cx);
    // On a mirror, confirmed with this computer's workspace shown.
    let (handle, ws, _dir) = opened(cx, Scope::Full);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.discard(vec!["a.txt".into()], window, cx)
    });
    reply(
        handle,
        &ws,
        cx,
        json!({"files": 1, "bytes": 4, "keep": true}),
    );
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.said.last().map(String::as_str),
            Some("Another workspace is shown now: nothing was done.")
        );
        assert!(ws.calls.is_empty(), "nothing was sent");
    });
}

#[gpui_kit::test]
fn a_local_discard_confirmed_with_a_mirror_shown_does_nothing(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = on_mirror(cx, Scope::Full, ROOT);
    let local = local_discard(handle, &ws, cx);
    std::fs::write(local.repo.path().join("a.txt"), "changed again\n").unwrap();
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx);
        ws.refresh_git(window, cx);
    });
    settle(handle, cx);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.discard(vec!["a.txt".into()], window, cx)
    });
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "asks first");
    })
    .unwrap();
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(1, window, cx)
    });
    go_live(&ws, cx, Scope::Full);
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
    })
    .unwrap();
    settle(handle, cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.said.last().map(String::as_str),
            Some("Another workspace is shown now: nothing was done.")
        );
        assert!(ws.calls.iter().all(|(_, c, _)| c.args["op"] != "discard"));
    });
    assert_eq!(local_file(&local), "changed again\n");
}

#[gpui_kit::test]
fn a_malformed_answer_that_quotes_a_huge_string_is_told_in_a_short_line(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.poll_git(window, cx));
    reply(handle, &ws, cx, json!({"status": "x".repeat(1_000_000)}));
    git_state(&ws, cx, |g| {
        let error = g.error.as_deref().unwrap_or_default();
        assert!(
            error.starts_with("git: The answer couldn't be read"),
            "{error}"
        );
        assert!(error.chars().count() <= 300, "{} characters", error.len());
    });
}

#[gpui_kit::test]
fn a_root_with_control_characters_is_refused_not_altered(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = on_mirror(cx, Scope::Control, ROOT);
    toggle(handle, &ws, cx);
    reply(
        handle,
        &ws,
        cx,
        json!({"root": "/home/ada/\u{1b}[31mapp", "main_tree": null}),
    );
    git_state(&ws, cx, |g| {
        assert!(matches!(&g.found, Found::Refused(why) if why.contains("can't show")));
    });
    assert!(
        waiting(&ws, cx).is_empty(),
        "nothing is asked of that folder"
    );
}

#[gpui_kit::test]
fn what_home_generates_is_one_line_and_a_bounded_text(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    ws.update(cx, |ws, _| ws.commit_agent = None);
    status_is(handle, &ws, cx, vec![staged("a.txt")]);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.generate_message(window, cx)
    });
    reply(
        handle,
        &ws,
        cx,
        json!({"summary": format!("Fix\nlogin\u{1b}[0m{}", "x".repeat(1_000_000)),
            "description": "y".repeat(100_000), "committed": false}),
    );
    let summary = summary_of(&ws, cx);
    assert!(summary.starts_with("Fix login[0mxxx"), "{summary}");
    assert!(summary.chars().count() <= crate::workspace::remote::LINE_CHARS);
    let description = ws.read_with(cx, |ws, cx| {
        ws.space
            .git()
            .unwrap()
            .description
            .read(cx)
            .value()
            .to_string()
    });
    assert!(description.chars().count() <= crate::workspace::remote::TEXT_CHARS);
}

#[gpui_kit::test]
fn a_measure_home_refuses_asks_nothing_and_says_why(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws, _dir) = opened(cx, Scope::Full);
    let before = notification_count(handle, cx);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.discard(vec!["a.txt".into()], window, cx)
    });
    assert_eq!(call_op(&ws, cx).0, "measure");
    answer_with(handle, &ws, cx, refusal("fatal: not allowed"));
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx), "it asks nothing");
    })
    .unwrap();
    assert_eq!(notification_count(handle, cx), before + 1);
    assert!(waiting(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn a_find_the_rule_refuses_says_so_and_asks_again_when_told(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    ws.update(cx, |ws, _| {
        ws.space.remote.as_mut().unwrap().status = templum_api::client::Status::Reconnecting {
            attempt: 1,
            next: Duration::from_secs(1),
            live_at: None,
        };
    });
    // A folder of the menu picked while the mirror reconnects.
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.set_home_git_folder("/home/ada/other".into(), window, cx)
    });
    git_state(&ws, cx, |g| {
        assert!(matches!(&g.found, Found::Refused(why) if why == "Not connected."));
    });
    assert!(waiting(&ws, cx).is_empty());
    go_live(&ws, &mut *cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| ws.look_up_git(window, cx));
    assert_eq!(waiting(&ws, cx), ["find"], "Try again asks");
}

#[gpui_kit::test]
fn a_folder_of_home_chosen_with_another_workspace_shown_changes_nothing(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(0, window, cx);
        ws.set_home_git_folder("/home/ada/other".into(), window, cx);
        assert!(
            ws.space.git.is_none(),
            "this computer's workspace has no Git of home's"
        );
        ws.switch_to(1, window, cx);
    });
    git_state(&ws, cx, |g| assert_eq!(g.home.as_deref(), Some(ROOT)));
    assert!(waiting(&ws, cx).is_empty());
}

#[gpui_kit::test]
fn this_computer_s_folder_is_never_pointed_at_on_a_mirror(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = opened(cx, Scope::Control);
    let before = notification_count(handle, cx);
    let folder = std::env::temp_dir();
    assert!(!with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.set_git_folder(&folder, window, cx)
    }));
    git_state(&ws, cx, |g| {
        assert_eq!(g.home.as_deref(), Some(ROOT));
        assert!(matches!(&g.found, Found::Home(r) if r == ROOT));
    });
    assert_eq!(notification_count(handle, cx), before);
}

#[gpui_kit::test]
fn home_s_answer_is_read_in_the_background(cx: &mut TestAppContext) {
    let (handle, ws, _dir) = on_mirror(cx, Scope::Control, ROOT);
    toggle(handle, &ws, cx);
    answer(
        handle,
        &ws,
        cx,
        0,
        Ok(json!({"text": "ok", "git": {"root": ROOT, "main_tree": ROOT}, "workspace": "w1"})),
    );
    git_state(&ws, cx, |g| {
        assert!(
            g.busy && matches!(g.found, Found::Looking),
            "not on the frame's thread"
        )
    });
    cx.run_until_parked();
    git_state(&ws, cx, |g| {
        assert!(matches!(&g.found, Found::Home(r) if r == ROOT))
    });
}

/// Home's `git diff` of `a.txt`: one hunk that adds an escape sequence and a line over the
/// 10,000 characters a string used to be cut at.
fn odd_diff() -> (String, Value) {
    let long = "x".repeat(10_001);
    let text = format!(
        "diff --git a/a.txt b/a.txt\nindex 1111111..2222222 100644\n--- a/a.txt\n+++ b/a.txt\n\
         @@ -1,2 +1,4 @@\n one\n+\u{1b}[31mred\n+{long}\n two\n"
    );
    let files = templum_git::parse_diff(&text, usize::MAX);
    let patch = templum_git::hunk_patch(&files[0], 0).unwrap();
    let diff = serde_json::to_value(WorkingDiff {
        unstaged: files,
        staged: Vec::new(),
    })
    .unwrap();
    (patch, diff)
}

#[gpui_kit::test]
fn a_hunk_staged_from_a_mirror_is_sent_home_exactly_as_home_gave_it(cx: &mut TestAppContext) {
    no_motion(cx);
    let (patch, diff) = odd_diff();
    let (handle, ws, _dir) = on_mirror(cx, Scope::Control, ROOT);
    toggle(handle, &ws, cx);
    reply(handle, &ws, cx, json!({"root": ROOT, "main_tree": ROOT}));
    reply(handle, &ws, cx, look(vec![modified("a.txt")]));
    reply(handle, &ws, cx, diff);
    reply(handle, &ws, cx, json!("main"));
    let ix = git_state(&ws, cx, |g| {
        g.panel
            .rows
            .iter()
            .position(|r| matches!(r, crate::git_block::diff_view::Row::Hunk { .. }))
            .expect("the hunk is drawn")
    });
    ws.update(cx, |ws, _| {
        ws.space
            .git()
            .unwrap()
            .panel
            .scroll
            .scroll_to_item(ix, gpui_kit::ScrollStrategy::Top)
    });
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.hover(("git-hunk", ix), cx);
        window.render_frame(cx);
        window.click(
            gpui_kit::ElementId::Name(format!("git-hunk-stage-{ix}").into()),
            cx,
        );
    })
    .unwrap();
    cx.run_until_parked();
    let (op, args, _) = call_op(&ws, cx);
    assert_eq!(op, "stage_hunk");
    assert_eq!(args["patch"], patch, "home's lines, byte for byte");
}

#[gpui_kit::test]
fn a_path_with_a_control_character_is_sent_home_as_home_gave_it(cx: &mut TestAppContext) {
    let odd = "dir/a\u{1b}[31m.txt";
    let (handle, ws, _dir) = on_mirror(cx, Scope::Control, ROOT);
    toggle(handle, &ws, cx);
    reply(handle, &ws, cx, json!({"root": ROOT, "main_tree": ROOT}));
    reply(handle, &ws, cx, look(vec![modified(odd)]));
    settle_reads(handle, &ws, cx, &[modified(odd)]);
    let path = git_state(&ws, cx, |g| g.status.files()[0].path.clone());
    assert_eq!(path, odd);
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.stage(vec![path.clone()], window, cx)
    });
    let (op, args, _) = call_op(&ws, cx);
    assert_eq!(op, "stage");
    assert_eq!(args["paths"], json!([odd]));
}

#[gpui_kit::test]
fn a_folder_picked_in_a_menu_left_open_on_another_workspace_changes_nothing(
    cx: &mut TestAppContext,
) {
    no_motion(cx);
    let (handle, ws, _dir) = on_mirror_with(
        cx,
        Scope::Control,
        ROOT,
        &[
            ("b1", Some("/home/ada/app/src")),
            ("b2", Some("/home/ada/lib")),
        ],
    );
    read_in(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click("git-folder", cx);
        window.render_frame(cx);
    })
    .unwrap();
    let opened_at = git_state(&ws, cx, |g| g.home.clone());
    // The menu stays up while this computer's workspace is shown (switching clears it, so it is
    // put back: the item is the one of the mirror's folders).
    with_ws(handle, &ws, cx, |ws, window, cx| {
        let menu = ws.menu.take();
        ws.switch_to(0, window, cx);
        ws.menu = menu;
    });
    let before = notification_count(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.within("popup-menu").click(0usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(notification_count(handle, cx), before + 1, "it says so");
    with_ws(handle, &ws, cx, |ws, window, cx| {
        ws.switch_to(1, window, cx)
    });
    git_state(&ws, cx, |g| assert_eq!(g.home, opened_at));
    assert!(waiting(&ws, cx).is_empty());
}
