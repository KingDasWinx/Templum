//! Conflicts, region by region: each part named by its marker's label.

use gpui_kit::component::WindowExt as _;

use super::*;
use crate::git_block::diff_view::Row;

/// main and topic both changed a.txt's second line: `git merge topic` conflicts.
fn merge_conflict() -> tempfile::TempDir {
    let repo = git_repo();
    std::fs::write(repo.path().join("a.txt"), "one\nbase\nthree\n").unwrap();
    git_run(repo.path(), &["add", "a.txt"]);
    git_run(repo.path(), &["commit", "-q", "-m", "base"]);
    git_run(repo.path(), &["checkout", "-q", "-b", "topic"]);
    std::fs::write(repo.path().join("a.txt"), "one\ntopic\nthree\n").unwrap();
    git_run(repo.path(), &["commit", "-q", "-am", "topic"]);
    git_run(repo.path(), &["checkout", "-q", "main"]);
    std::fs::write(repo.path().join("a.txt"), "one\nmain\nthree\n").unwrap();
    git_run(repo.path(), &["commit", "-q", "-am", "main"]);
    let merge = std::process::Command::new("git")
        .args(["merge", "topic"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(!merge.status.success(), "conflicts");
    repo
}

/// The panel's row index of the first row matching `want`.
fn row_of(ws: &Entity<Workspace>, cx: &mut TestAppContext, want: impl Fn(&Row) -> bool) -> usize {
    git_of(ws, cx, |g| {
        g.panel.rows.iter().position(want).expect("the row")
    })
}

fn click_in_row(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    row: impl Into<gpui_kit::ElementId>,
    button: gpui_kit::ElementId,
) {
    cx.update_window(handle, |_, window, cx| {
        window.hover(row, cx);
        window.render_frame(cx);
        window.click(button, cx);
    })
    .unwrap();
    settle(handle, cx);
}

#[gpui_kit::test]
fn use_this_keeps_one_part_and_the_merge_continues(cx: &mut TestAppContext) {
    let repo = merge_conflict();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    git_of(&ws, cx, |g| {
        assert_eq!(
            g.conflict_labels("a.txt"),
            ("HEAD".to_string(), "topic".to_string())
        );
        assert!(g.markers_left("a.txt"));
    });
    // The second part (topic's).
    let ix = row_of(&ws, cx, |r| matches!(r, Row::Hunk { hunk: 1, .. }));
    click_in_row(
        handle,
        cx,
        ("git-hunk", ix),
        gpui_kit::ElementId::Name(format!("git-hunk-use-{ix}").into()),
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "one\ntopic\nthree\n"
    );
    git_of(&ws, cx, |g| assert!(!g.markers_left("a.txt")));
    let ix = row_of(&ws, cx, |r| {
        matches!(
            r,
            Row::Note {
                note: crate::git_block::diff_view::Note::Resolved,
                ..
            }
        )
    });
    cx.update_window(handle, |_, window, cx| {
        window.click(("git-mark-resolved", ix), cx)
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| assert!(g.status.conflicts().is_empty()));
    cx.update_window(handle, |_, window, cx| window.click("git-continue", cx))
        .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| assert_eq!(g.operation, None));
}

#[gpui_kit::test]
fn use_both_keeps_the_two_parts_in_order(cx: &mut TestAppContext) {
    let repo = merge_conflict();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let ix = row_of(&ws, cx, |r| matches!(r, Row::Region { region: 0, .. }));
    click_in_row(
        handle,
        cx,
        ("git-region", ix),
        gpui_kit::ElementId::Name(format!("git-region-both-{ix}").into()),
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "one\nmain\ntopic\nthree\n"
    );
}

#[gpui_kit::test]
fn a_rebase_names_the_parts_after_its_markers(cx: &mut TestAppContext) {
    let repo = merge_conflict();
    git_run(repo.path(), &["merge", "--abort"]);
    git_run(repo.path(), &["checkout", "-q", "topic"]);
    let rebase = std::process::Command::new("git")
        .args(["rebase", "main"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(!rebase.status.success());
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    git_of(&ws, cx, |g| {
        let (first, second) = g.conflict_labels("a.txt");
        assert_eq!(first, "HEAD", "main's commit, the one rebased onto");
        assert!(
            second.ends_with("(topic)"),
            "the user's own commit: {second}"
        );
    });
}

#[gpui_kit::test]
fn a_region_changed_since_it_was_read_is_refused(cx: &mut TestAppContext) {
    let repo = merge_conflict();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    // Someone fixes it in an editor before the click.
    let edited = "one\n<<<<<<< HEAD\nedited\n=======\ntopic\n>>>>>>> topic\nthree\n";
    std::fs::write(repo.path().join("a.txt"), edited).unwrap();
    let ix = row_of(&ws, cx, |r| matches!(r, Row::Hunk { hunk: 0, .. }));
    click_in_row(
        handle,
        cx,
        ("git-hunk", ix),
        gpui_kit::ElementId::Name(format!("git-hunk-use-{ix}").into()),
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        edited,
        "left as it is"
    );
    assert_eq!(notification_count(handle, cx), 1);
    assert_eq!(told_so_far(), ["The file changed; try again"]);
}

#[gpui_kit::test]
fn a_choice_keeps_a_copy_in_the_trash(cx: &mut TestAppContext) {
    let repo = merge_conflict();
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    open_git(handle, &ws, cx, repo.path());
    let ix = row_of(&ws, cx, |r| matches!(r, Row::Hunk { hunk: 0, .. }));
    click_in_row(
        handle,
        cx,
        ("git-hunk", ix),
        gpui_kit::ElementId::Name(format!("git-hunk-use-{ix}").into()),
    );
    let kept = ws.read_with(cx, |ws, _| ws.trash.as_ref().unwrap().list());
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].files[0].0, "a.txt");
}

#[gpui_kit::test]
fn s_on_a_conflict_with_markers_asks_first(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = merge_conflict();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.select_git_row(
                crate::git_block::List::Conflicts,
                "a.txt".to_string(),
                window,
                cx,
            );
            let focus = ws.space.git().unwrap().focus.clone();
            window.focus(&focus, cx);
        });
        window.render_frame(cx);
        window.press("s", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "the markers are still in it");
        window.within("dialog").click("cancel", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.conflicts().len(), 1, "still conflicted")
    });
}

fn take_whole_file(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    side: templum_git::Side,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.resolve_file("a.txt".to_string(), side, window, cx)
        })
    })
    .unwrap();
    settle(handle, cx);
}

#[gpui_kit::test]
fn a_whole_file_choice_keeps_the_file_as_it_was_in_the_trash(cx: &mut TestAppContext) {
    let repo = merge_conflict();
    let before = std::fs::read(repo.path().join("a.txt")).unwrap();
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    open_git(handle, &ws, cx, repo.path());
    take_whole_file(handle, &ws, cx, templum_git::Side::Theirs);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "one\ntopic\nthree\n"
    );
    let kept = ws.read_with(cx, |ws, _| ws.trash.as_ref().unwrap().list());
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].files, [("a.txt".to_string(), true)]);
    let copy = data
        .path()
        .join("trash")
        .join(&kept[0].id)
        .join("files")
        .join("0");
    assert_eq!(
        std::fs::read(copy).unwrap(),
        before,
        "the markers and all, as they were"
    );
}

// Review M3: a hand-edited, partly resolved file is never overwritten without its copy.
#[gpui_kit::test]
fn a_whole_file_choice_whose_copy_fails_changes_nothing(cx: &mut TestAppContext) {
    let repo = merge_conflict();
    let edited = "one\nfixed by hand\n<<<<<<< HEAD\nmain\n=======\ntopic\n>>>>>>> topic\nthree\n";
    std::fs::write(repo.path().join("a.txt"), edited).unwrap();
    let data = tempfile::tempdir().unwrap();
    // The trash can't be made: a file stands where its folder goes.
    std::fs::write(data.path().join("trash"), "").unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    open_git(handle, &ws, cx, repo.path());
    take_whole_file(handle, &ws, cx, templum_git::Side::Ours);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        edited,
        "left as it is"
    );
    assert_eq!(notification_count(handle, cx), 1);
    let told = told_so_far();
    assert!(
        told.len() == 1
            && told[0].starts_with("Couldn't keep a copy of the file, so nothing was changed: "),
        "{told:?}"
    );
}
