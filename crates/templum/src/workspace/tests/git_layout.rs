//! The Git block's layout: its size, the left column's fold, Filter files.

use super::*;
use gpui_kit::Pixels;

#[gpui_kit::test]
fn the_lists_fold_away_and_the_block_remembers(cx: &mut TestAppContext) {
    let repo = git_repo();
    std::fs::write(repo.path().join("a.txt"), "x\n").unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    // The lists' rows (a uniform list's own id isn't observable).
    let a = crate::git_block::row_id("files", 0);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(a.clone()).is_some(), "the lists are drawn");
        window.click("git-fold-lists", cx);
        window.render_frame(cx);
        assert!(
            window.try_find(a.clone()).is_none(),
            "the diff takes the width"
        );
    })
    .unwrap();
    let saved = ws.read_with(cx, |ws, cx| ws.space.git().unwrap().saved(cx));
    assert_eq!(saved.lists, "folded");
    cx.update_window(handle, |_, window, cx| {
        window.click("git-unfold-lists", cx);
        window.render_frame(cx);
        assert!(window.try_find(a).is_some());
    })
    .unwrap();
}

/// `count` untracked files a00.txt…, then b.txt.
fn many_files(count: usize) -> tempfile::TempDir {
    let repo = git_repo();
    for i in 0..count {
        std::fs::write(repo.path().join(format!("a{i:02}.txt")), "x\n").unwrap();
    }
    std::fs::write(repo.path().join("b.txt"), "x\n").unwrap();
    repo
}

#[gpui_kit::test]
fn past_15_files_a_filter_narrows_the_lists(cx: &mut TestAppContext) {
    use crate::git_block::List;
    let repo = many_files(15);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let filter = git_of(&ws, cx, |g| {
        assert!(g.filtering(), "16 files");
        g.files_filter.clone()
    });
    cx.update_window(handle, |_, window, cx| {
        filter.update(cx, |f, cx| f.set_value("B.T", window, cx))
    })
    .unwrap();
    let listed = ws.read_with(cx, |ws, cx| ws.space.git().unwrap().listed(cx));
    assert_eq!(
        listed,
        [(List::Files, None), (List::Files, Some(15)),],
        "b.txt, by its index among all 16, without case"
    );
    // At 15 files no field shows, and a filter left from before narrows nothing.
    std::fs::remove_file(repo.path().join("b.txt")).unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.refresh_git(window, cx))
    })
    .unwrap();
    settle(handle, cx);
    ws.read_with(cx, |ws, cx| {
        let g = ws.space.git().unwrap();
        assert!(!g.filtering());
        assert_eq!(g.listed(cx).len(), 16, "one header, fifteen rows");
    });
}

#[gpui_kit::test]
fn discard_all_with_a_filter_on_takes_only_the_rows_shown(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let repo = many_files(15);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let filter = git_of(&ws, cx, |g| g.files_filter.clone());
    cx.update_window(handle, |_, window, cx| {
        filter.update(cx, |f, cx| f.set_value("b", window, cx));
        window.render_frame(cx);
        window.click("git-discard-all", cx);
    })
    .unwrap();
    settle(handle, cx);
    let asked = told_so_far();
    assert!(
        asked.len() == 1 && asked[0].starts_with("Discard the changes to b.txt? "),
        "{asked:?}"
    );
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx));
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    assert!(!repo.path().join("b.txt").exists());
    assert!(
        repo.path().join("a00.txt").exists(),
        "a filtered-out file stays"
    );
}

/// The popup's box in the window, after a frame.
fn popup_box(handle: AnyWindowHandle, cx: &mut TestAppContext) -> Bounds<Pixels> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find("git-popup").bounds()
    })
    .unwrap()
}

/// Presses the middle of the resize strip `zone`, drags `by` and lets go.
fn drag_zone(handle: AnyWindowHandle, cx: &mut TestAppContext, zone: &'static str, by: (f32, f32)) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let from = window.find(zone).bounds().center();
        window.drag(from, point(from.x + px(by.0), from.y + px(by.1)), cx);
    })
    .unwrap();
}

fn close_to(a: Pixels, b: f32) -> bool {
    (f32::from(a) - b).abs() < 0.01
}

#[gpui_kit::test]
fn dragging_the_popups_free_edges_resizes_it_around_its_corner(cx: &mut TestAppContext) {
    let repo = git_repo();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let before = popup_box(handle, cx);
    assert_eq!(
        (f32::from(before.size.width), f32::from(before.size.height)),
        (crate::git_block::SIZE.w, crate::git_block::SIZE.h),
        "nothing set: the size it always had"
    );
    // The right edge, 100 px to the right: wider, the left edge and the height stay.
    drag_zone(handle, cx, "popup-resize-right", (100.0, 30.0));
    let wide = popup_box(handle, cx);
    assert!(close_to(
        wide.size.width,
        f32::from(before.size.width) + 100.0
    ));
    assert_eq!(wide.size.height, before.size.height);
    assert_eq!(wide.origin, before.origin);
    // The top edge, 60 px up: taller, the bottom edge and the width stay.
    drag_zone(handle, cx, "popup-resize-top", (20.0, -60.0));
    let tall = popup_box(handle, cx);
    assert!(close_to(
        tall.size.height,
        f32::from(before.size.height) + 60.0
    ));
    assert_eq!(tall.size.width, wide.size.width);
    assert_eq!(tall.bottom(), before.bottom());
    assert_eq!(tall.origin.x, before.origin.x);
    // The corner: both at once; dragging back down and left shrinks it.
    drag_zone(handle, cx, "popup-resize-top-right", (-50.0, 40.0));
    let small = popup_box(handle, cx);
    assert!(close_to(
        small.size.width,
        f32::from(tall.size.width) - 50.0
    ));
    assert!(close_to(
        small.size.height,
        f32::from(tall.size.height) - 40.0
    ));
    assert_eq!(
        (small.origin.x, small.bottom()),
        (before.origin.x, before.bottom())
    );
}

#[gpui_kit::test]
fn the_popup_stays_between_its_minimum_and_the_window(cx: &mut TestAppContext) {
    let repo = git_repo();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let room = cx
        .update_window(handle, |_, window, cx| {
            let vp = ws.read(cx).viewport(window);
            (vp.w - 16.0, vp.h - 16.0)
        })
        .unwrap();
    // Far past the window: the window's room, not the pointer's, and the pointer going back
    // shrinks it at once.
    drag_zone(handle, cx, "popup-resize-top-right", (5000.0, -5000.0));
    let big = popup_box(handle, cx);
    assert!(close_to(big.size.width, room.0) && close_to(big.size.height, room.1));
    let set = ws.read_with(cx, |ws, _| ws.git_size).expect("a size set");
    assert_eq!(
        (set.w, set.h),
        room,
        "what is kept never passes the room it was set in"
    );
    // Far the other way: the minimum.
    drag_zone(handle, cx, "popup-resize-top-right", (-5000.0, 5000.0));
    let small = popup_box(handle, cx);
    assert!(close_to(small.size.width, crate::git_block::MIN.w));
    assert!(close_to(small.size.height, crate::git_block::MIN.h));
    let set = ws.read_with(cx, |ws, _| ws.git_size).expect("a size set");
    assert_eq!(set, crate::git_block::MIN);
}

#[gpui_kit::test]
fn a_smaller_window_shrinks_the_popup_and_a_larger_one_gives_the_size_back(
    cx: &mut TestAppContext,
) {
    let repo = git_repo();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    drag_zone(handle, cx, "popup-resize-top-right", (300.0, -150.0));
    let set = popup_box(handle, cx);
    let kept = ws.read_with(cx, |ws, _| ws.git_size);
    cx.simulate_window_resize(handle, size(px(800.), px(560.)));
    let shrunk = popup_box(handle, cx);
    let (w, h) = cx
        .update_window(handle, |_, window, _| {
            let window_size = window.viewport_size();
            (window_size.width, window_size.height)
        })
        .unwrap();
    assert!(shrunk.size.width < set.size.width && shrunk.size.height < set.size.height);
    assert!(shrunk.right() <= w - px(8.0) && shrunk.origin.y >= px(0.0) && shrunk.bottom() <= h);
    assert_eq!(
        ws.read_with(cx, |ws, _| ws.git_size),
        kept,
        "what was set is kept"
    );
    cx.simulate_window_resize(handle, size(px(1280.), px(800.)));
    assert_eq!(popup_box(handle, cx), set, "back to the size set");
}

#[gpui_kit::test]
fn dragging_one_edge_in_a_smaller_window_keeps_the_other_side_set(cx: &mut TestAppContext) {
    let repo = git_repo();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    drag_zone(handle, cx, "popup-resize-top-right", (300.0, -150.0));
    let set = popup_box(handle, cx);
    cx.simulate_window_resize(handle, size(px(800.), px(560.)));
    assert!(popup_box(handle, cx).size.width < set.size.width);
    // Only the top edge, on the small window: the width set on the big one stays.
    drag_zone(handle, cx, "popup-resize-top", (0.0, 10.0));
    cx.simulate_window_resize(handle, size(px(1280.), px(800.)));
    assert_eq!(popup_box(handle, cx).size.width, set.size.width);
}

#[gpui_kit::test]
fn the_size_set_comes_back_when_the_popup_opens_again_and_after_a_restart(cx: &mut TestAppContext) {
    let repo = git_repo();
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    open_git(handle, &ws, cx, repo.path());
    drag_zone(handle, cx, "popup-resize-top-right", (140.0, -90.0));
    let set = popup_box(handle, cx);
    // Written at once, as dragged.
    let crate::session::Loaded::Saved(index) = crate::session::load_index(data.path()) else {
        panic!("no index");
    };
    let saved = index.git_size.expect("the size is in the index");
    assert!(close_to(set.size.width, saved.w) && close_to(set.size.height, saved.h));
    // Closed and opened again.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.close_git(window, cx);
            ws.open_git(window, cx);
        })
    })
    .unwrap();
    assert_eq!(popup_box(handle, cx), set);
    // A new Templum on the same folder.
    let (again_handle, again) = open_in(cx, None, Some(data.path().to_path_buf()));
    open_git(again_handle, &again, cx, repo.path());
    assert_eq!(popup_box(again_handle, cx).size, set.size);
}
