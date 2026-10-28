//! A Git block's discards: the copy in the trash, Undo, Recently discarded…, and what can't be kept.

use super::*;
use crate::git_block::row_id;

/// A workspace with a data folder: the trash lives there.
fn open_kept(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Workspace>, tempfile::TempDir) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    (handle, ws, data)
}

/// Clicks row `i`'s Discard, waits for the question (it comes once what would be kept is
/// measured), and answers Discard.
fn discard_row(handle: AnyWindowHandle, cx: &mut TestAppContext, i: usize) {
    use gpui_kit::component::WindowExt as _;
    click_row_button(
        handle,
        cx,
        row_id("files", i),
        git_button("discard", "files", i),
    );
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "asks first");
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
}

fn kept(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> Vec<templum_git::trash::Kept> {
    ws.read_with(cx, |ws, _| {
        ws.trash.as_ref().map(|t| t.list()).unwrap_or_default()
    })
}

#[gpui_kit::test]
fn a_discard_keeps_a_copy_and_undo_puts_it_back(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = committed_repo();
    std::fs::write(repo.path().join("a.txt"), "changed\n").unwrap();
    std::fs::write(repo.path().join("new.txt"), "mine\n").unwrap();
    let (handle, ws, _data) = open_kept(cx);
    open_git(handle, &ws, cx, repo.path());
    // a.txt, then new.txt: git lists tracked files first.
    discard_row(handle, cx, 0);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "one\n"
    );
    let k = kept(&ws, cx);
    assert_eq!(k.len(), 1);
    assert_eq!(k[0].what, "Discarded a.txt");
    cx.update_window(handle, |_, window, cx| window.click("git-undo", cx))
        .unwrap();
    settle(handle, cx);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "changed\n"
    );
    assert!(
        kept(&ws, cx).is_empty(),
        "a restored discard leaves the trash"
    );
    // The dismissed notification's exit runs on the executor's clock: let it end, so the next
    // discard's Undo is the only one.
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    settle(handle, cx);
    // An untracked file is deleted by a discard, and comes back too.
    discard_row(handle, cx, 1);
    assert!(!repo.path().join("new.txt").exists());
    cx.update_window(handle, |_, window, cx| window.click("git-undo", cx))
        .unwrap();
    settle(handle, cx);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("new.txt")).unwrap(),
        "mine\n"
    );
}

#[gpui_kit::test]
fn recently_discarded_restores_and_asks_over_a_file_changed_since(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let repo = committed_repo();
    std::fs::write(repo.path().join("a.txt"), "changed\n").unwrap();
    let (handle, ws, _data) = open_kept(cx);
    open_git(handle, &ws, cx, repo.path());
    discard_row(handle, cx, 0);
    std::fs::write(repo.path().join("a.txt"), "edited since then\n").unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click("git-more", cx);
        window.render_frame(cx);
        // Fetch, Pull, Push, —, Recently discarded….
        window.within("popup-menu").click(4usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "the list");
        window.click(("git-restore", 0usize), cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.has_active_dialog(cx),
            "asks before writing over the edit"
        );
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "changed\n"
    );
}

#[gpui_kit::test]
fn without_a_data_folder_a_discard_keeps_nothing(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = committed_repo();
    std::fs::write(repo.path().join("a.txt"), "changed\n").unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    discard_row(handle, cx, 0);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "one\n"
    );
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find("git-undo").is_none(),
            "nothing to undo with"
        )
    })
    .unwrap();
}

#[gpui_kit::test]
fn past_the_cap_a_discard_keeps_nothing(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let repo = committed_repo();
    for i in 0..=templum_git::trash::MAX_FILES {
        std::fs::write(repo.path().join(format!("junk{i:03}.txt")), "x").unwrap();
    }
    let (handle, ws, _data) = open_kept(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-discard-all", cx))
        .unwrap();
    settle(handle, cx);
    let asked = told_so_far();
    assert!(
        asked.len() == 1
            && asked[0].contains(" Too much to keep a copy: ")
            && asked[0].ends_with(". This can't be undone."),
        "says no copy is kept: {asked:?}"
    );
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx));
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    assert!(!repo.path().join("junk000.txt").exists());
    assert!(kept(&ws, cx).is_empty(), "no copy past the cap");
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find("git-undo").is_none())
    })
    .unwrap();
}

#[gpui_kit::test]
fn old_discards_go_when_templum_starts(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a"), "x").unwrap();
    let trash = templum_git::trash::Trash::new(data.path().join("trash"));
    let old = std::time::SystemTime::now()
        - templum_git::trash::MAX_AGE
        - std::time::Duration::from_secs(60);
    trash
        .put(root.path(), &["a".to_string()], "old", old)
        .unwrap();
    let (_handle, _ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    cx.run_until_parked();
    assert!(trash.list().is_empty());
}
