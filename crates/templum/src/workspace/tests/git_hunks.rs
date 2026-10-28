//! The diff panel's hunk buttons: Stage, Unstage and Discard one hunk, and Open at its line.

use super::*;
use crate::git_block::diff_view::Row;

/// The row of hunk `hunk` of `path`, in Changes (`staged` false) or Staged.
fn hunk_at(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    staged: bool,
    path: &str,
    hunk: usize,
) -> usize {
    git_of(ws, cx, |g| {
        g.panel
            .rows
            .iter()
            .position(|r| {
                matches!(r, Row::Hunk { file, hunk: h }
                    if *h == hunk
                        && g.panel.files[*file].staged == staged
                        && g.panel.files[*file].diff.path() == path)
            })
            .unwrap_or_else(|| panic!("no hunk {hunk} of {path}"))
    })
}

fn hunk_button(what: &str, ix: usize) -> gpui_kit::ElementId {
    gpui_kit::ElementId::Name(format!("git-hunk-{what}-{ix}").into())
}

/// Scrolls the panel so row `ix` is drawn, and draws a frame.
fn scroll_to(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, ix: usize) {
    ws.update(cx, |ws, _| {
        ws.space
            .git()
            .unwrap()
            .panel
            .scroll
            .scroll_to_item(ix, gpui_kit::ScrollStrategy::Top)
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

/// Scrolls the panel to hunk row `ix`, hovers it and clicks its `what` button.
fn click_hunk(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    ix: usize,
    what: &str,
) {
    scroll_to(handle, ws, cx, ix);
    click_row_button(handle, cx, ("git-hunk", ix).into(), hunk_button(what, ix));
}

#[gpui_kit::test]
fn the_hunk_buttons_stage_unstage_and_discard_one_hunk(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let repo = three_hunk_repo();
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    open_git(handle, &ws, cx, repo.path());
    let ix = hunk_at(&ws, cx, false, "a.txt", 1);
    click_hunk(handle, &ws, cx, ix, "stage");
    assert_eq!(
        git_run(repo.path(), &["diff", "--cached", "--numstat"]),
        "1\t1\ta.txt\n",
        "the middle hunk alone"
    );
    let ix = hunk_at(&ws, cx, true, "a.txt", 0);
    click_hunk(handle, &ws, cx, ix, "unstage");
    assert_eq!(git_run(repo.path(), &["diff", "--cached", "--numstat"]), "");
    let ix = hunk_at(&ws, cx, false, "a.txt", 2);
    click_hunk(handle, &ws, cx, ix, "discard");
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "asks first");
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    let text = std::fs::read_to_string(repo.path().join("a.txt")).unwrap();
    assert!(
        text.contains("line 28\n") && text.contains("LINE 15\n"),
        "{text}"
    );
    cx.update_window(handle, |_, window, cx| window.click("git-undo", cx))
        .unwrap();
    settle(handle, cx);
    assert!(
        std::fs::read_to_string(repo.path().join("a.txt"))
            .unwrap()
            .contains("LINE 28\n"),
        "Undo"
    );
}

#[gpui_kit::test]
fn a_hunk_that_changed_since_it_was_read_is_refused(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = three_hunk_repo();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let ix = hunk_at(&ws, cx, false, "a.txt", 1);
    // A terminal stages the whole file; the panel still shows the read before it.
    git_run(repo.path(), &["add", "a.txt"]);
    click_hunk(handle, &ws, cx, ix, "stage");
    assert_eq!(notification_count(handle, cx), 1);
    assert_eq!(told_so_far(), ["The file changed; try again"]);
    assert_eq!(
        git_run(repo.path(), &["diff", "--cached", "--numstat"]),
        "3\t3\ta.txt\n",
        "nothing else changed"
    );
    git_of(&ws, cx, |g| {
        assert!(g.panel.files.iter().all(|s| s.staged), "read again")
    });
}

#[gpui_kit::test]
fn a_new_file_or_ignored_whitespace_has_no_hunk_buttons_and_open_goes_to_the_hunk(
    cx: &mut TestAppContext,
) {
    let repo = three_hunk_repo();
    std::fs::write(repo.path().join("n.txt"), "x\n").unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let a = hunk_at(&ws, cx, false, "a.txt", 0);
    let new = hunk_at(&ws, cx, false, "n.txt", 0);
    scroll_to(handle, &ws, cx, a);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(hunk_button("stage", a)).is_some());
    })
    .unwrap();
    scroll_to(handle, &ws, cx, new);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(hunk_button("open", new)).is_some(), "drawn");
        assert!(
            window.try_find(hunk_button("stage", new)).is_none(),
            "a new file's own buttons do it"
        );
    })
    .unwrap();
    scroll_to(handle, &ws, cx, a);
    cx.update_window(handle, |_, window, cx| window.click("git-ignore-ws", cx))
        .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(hunk_button("open", a)).is_some(), "drawn");
        assert!(
            window.try_find(hunk_button("stage", a)).is_none(),
            "the patch would miss the whitespace"
        );
    })
    .unwrap();
    // Open on the middle hunk: the editor at its first line, 12 (three of context before 15).
    let b = hunk_at(&ws, cx, false, "a.txt", 1);
    click_hunk(handle, &ws, cx, b, "open");
    ws.read_with(cx, |ws, cx| {
        let e = ws
            .space
            .blocks
            .values()
            .find_map(|b| b.editor())
            .expect("an editor opened");
        assert_eq!(
            e.active_tab()
                .unwrap()
                .editor
                .read(cx)
                .cursor_position()
                .line,
            11
        );
    });
}
