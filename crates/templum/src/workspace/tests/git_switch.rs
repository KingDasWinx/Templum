//! Switching branches: the switcher, the question about changes, the unpushed notice.

use gpui_kit::Focusable as _;
use gpui_kit::component::WindowExt as _;

use super::*;
use crate::git_block::switcher::Opened;

/// main with a.txt, and feat (b.txt), on main.
fn two_branches() -> tempfile::TempDir {
    let repo = committed_repo();
    git_run(repo.path(), &["checkout", "-q", "-b", "feat"]);
    std::fs::write(repo.path().join("b.txt"), "b\n").unwrap();
    git_run(repo.path(), &["add", "b.txt"]);
    git_run(repo.path(), &["commit", "-q", "-m", "b"]);
    git_run(repo.path(), &["checkout", "-q", "main"]);
    repo
}

fn switch(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext, name: &str) {
    let name = name.to_string();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_branch(name, false, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
}

fn answer(handle: AnyWindowHandle, cx: &mut TestAppContext, button: &'static str) {
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "the question");
        window.within("dialog").click(button, cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
}

#[gpui_kit::test]
fn the_switcher_finds_a_branch_and_enter_switches(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("branch", cx))
        .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "the switcher");
    })
    .unwrap();
    cx.simulate_input(handle, "fea");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("feat"))
    });
}

#[gpui_kit::test]
fn the_switcher_creates_a_branch_from_a_name_no_branch_has(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("branch", cx))
        .unwrap();
    settle(handle, cx);
    // "fea" is in "feat": Enter takes that match and creates nothing.
    cx.simulate_input(handle, "fea");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("feat"));
    });
    assert_eq!(git_run(repo.path(), &["branch"]).lines().count(), 2);
    // A name nobody has is the only row: Enter creates it and switches to it.
    cx.update_window(handle, |_, window, cx| window.click("branch", cx))
        .unwrap();
    settle(handle, cx);
    cx.simulate_input(handle, "brand-new");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("brand-new"));
    });
    assert_eq!(git_run(repo.path(), &["branch"]).lines().count(), 3);
    assert_eq!(
        git_run(repo.path(), &["rev-parse", "brand-new"]),
        git_run(repo.path(), &["rev-parse", "feat"]),
        "made where HEAD was"
    );
}

#[gpui_kit::test]
fn a_new_branch_from_a_commit_lists_no_branches_and_starts_there(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    let first = git_run(repo.path(), &["rev-parse", "main~0"])
        .trim()
        .to_string();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_branch_dialog(Opened::Commit(first.clone()), window, cx)
        });
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "opens at once");
    })
    .unwrap();
    // "ma" would match main; from a commit there is nothing to switch to, only the creation.
    cx.simulate_input(handle, "ma");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
    })
    .unwrap();
    cx.run_until_parked();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("ma"))
    });
}

#[gpui_kit::test]
fn changes_left_on_a_branch_come_back_with_it(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    std::fs::write(repo.path().join("a.txt"), "mine\n").unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    switch(handle, &ws, cx, "feat");
    answer(handle, cx, "git-leave");
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("feat"));
        assert!(g.status.entries.is_empty(), "left on main");
    });
    assert!(git_run(repo.path(), &["stash", "list"]).contains("templum: left on main"));
    // Back to main: no question (nothing changed on feat), and the changes are back.
    switch(handle, &ws, cx, "main");
    cx.update_window(handle, |_, window, cx| {
        assert!(!window.has_active_dialog(cx))
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(repo.path().join("a.txt")).unwrap(),
        "mine\n"
    );
    assert_eq!(git_run(repo.path(), &["stash", "list"]), "", "popped");
}

#[gpui_kit::test]
fn bringing_changes_along_and_a_refusal_offers_leaving_them(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    std::fs::write(repo.path().join("new.txt"), "n\n").unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    switch(handle, &ws, cx, "feat");
    answer(handle, cx, "git-bring");
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("feat"));
        assert_eq!(g.status.entries.len(), 1, "carried");
    });
    // feat changes a.txt; a change to it on main can't come along.
    std::fs::remove_file(repo.path().join("new.txt")).unwrap();
    git_run(repo.path(), &["checkout", "-q", "main"]);
    git_run(repo.path(), &["checkout", "-q", "feat"]);
    std::fs::write(repo.path().join("a.txt"), "feat\n").unwrap();
    git_run(repo.path(), &["commit", "-q", "-am", "a on feat"]);
    git_run(repo.path(), &["checkout", "-q", "main"]);
    std::fs::write(repo.path().join("a.txt"), "dirty\n").unwrap();
    settle_read(handle, &ws, cx);
    switch(handle, &ws, cx, "feat");
    answer(handle, cx, "git-bring");
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("main"), "refused")
    });
    assert_eq!(notification_count(handle, cx), 1);
    cx.update_window(handle, |_, window, cx| window.click("git-leave-them", cx))
        .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("feat"))
    });
    assert!(git_run(repo.path(), &["stash", "list"]).contains("templum: left on main"));
}

#[gpui_kit::test]
fn commit_first_stays_and_gives_the_summary_the_keyboard(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    std::fs::write(repo.path().join("a.txt"), "mine\n").unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    switch(handle, &ws, cx, "feat");
    answer(handle, cx, "git-commit-first");
    let summary = git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("main"));
        g.summary.clone()
    });
    cx.update_window(handle, |_, window, cx| {
        assert!(summary.read(cx).focus_handle(cx).is_focused(window));
    })
    .unwrap();
}

#[gpui_kit::test]
fn leaving_a_branch_with_commits_not_pushed_offers_to_push_it(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    let remote = add_remote(repo.path());
    std::fs::write(repo.path().join("a.txt"), "two\n").unwrap();
    git_run(repo.path(), &["commit", "-q", "-am", "two"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    switch(handle, &ws, cx, "feat");
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("feat"), "no question")
    });
    assert_eq!(notification_count(handle, cx), 1);
    assert_eq!(told_so_far(), ["main has 1 commit not pushed yet"]);
    cx.update_window(handle, |_, window, cx| window.click("git-push-left", cx))
        .unwrap();
    settle(handle, cx);
    assert_eq!(
        git_run(remote.path(), &["rev-parse", "main"]),
        git_run(repo.path(), &["rev-parse", "main"])
    );
}

#[gpui_kit::test]
fn a_detached_head_says_so_and_goes_back_to_main(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    git_run(repo.path(), &["checkout", "-q", "--detach", "feat"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    git_of(&ws, cx, |g| {
        assert_eq!(g.default_branch.as_deref(), Some("main"))
    });
    cx.update_window(handle, |_, window, cx| {
        assert!(window.try_find("git-banner").is_some(), "the banner");
        window.click("git-back", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("main"))
    });
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find("git-banner").is_none(),
            "gone with the detached HEAD"
        )
    })
    .unwrap();
}

#[gpui_kit::test]
fn create_a_branch_here_keeps_the_commits_on_a_branch(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    git_run(repo.path(), &["checkout", "-q", "--detach", "feat"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-branch-here", cx))
        .unwrap();
    // The branches are read first, then the dialog opens.
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx));
        window.input("rescue", cx);
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("rescue"))
    });
}

/// "fea" is in the existing "feat": from a create entry, Enter right after typing makes "fea".
#[gpui_kit::test]
fn a_create_entry_makes_the_name_typed_even_when_another_branch_holds_it(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    git_run(repo.path(), &["checkout", "-q", "--detach", "feat"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-branch-here", cx))
        .unwrap();
    settle(handle, cx);
    cx.simulate_input(handle, "fea");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch.as_deref(), Some("fea"));
    });
    assert_eq!(
        git_run(repo.path(), &["rev-parse", "fea"]),
        git_run(repo.path(), &["rev-parse", "feat"]),
        "made where HEAD was, not a switch to feat"
    );
    assert_eq!(git_run(repo.path(), &["branch"]).lines().count(), 3);
}

/// "main" exists: from a detached HEAD the dialog must not switch to it (the commits would be left
/// behind); the creation is the first row, and git refuses it in its own words.
#[gpui_kit::test]
fn a_create_entry_never_switches_away_from_a_detached_head(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    git_run(repo.path(), &["checkout", "-q", "--detach", "feat"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-branch-here", cx))
        .unwrap();
    settle(handle, cx);
    cx.simulate_input(handle, "main");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch, None, "still detached");
    });
    assert_eq!(
        git_run(repo.path(), &["rev-parse", "HEAD"]),
        git_run(repo.path(), &["rev-parse", "feat"]),
        "HEAD stayed on its commit"
    );
    assert_eq!(notification_count(handle, cx), 1, "git's own refusal");
}

/// Enter acts on the text typed, not on the rows the last frame drew for the text before it. Here
/// the frame drew every branch (nothing typed) and "fea" is typed and confirmed with no frame
/// between: the helpers (`input`, `press`, `simulate_input`) and the end of every window update
/// redraw first, which hides it, so the keys go straight to the window and the kit's confirm action
/// to the focused field. Row 0 of the stale frame is a branch to switch to; the text names none, so
/// Enter makes "fea".
#[gpui_kit::test]
fn enter_acts_on_the_text_typed_since_the_last_frame(cx: &mut TestAppContext) {
    use gpui_kit::Keystroke;
    use gpui_kit::base::actions::Confirm;
    no_motion(cx);
    let repo = two_branches();
    git_run(repo.path(), &["checkout", "-q", "--detach", "feat"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-branch-here", cx))
        .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx));
        window.render_frame(cx);
        for c in "fea".chars() {
            let mut key = Keystroke::parse(&c.to_string()).unwrap();
            key.key_char = Some(c.to_string());
            window.dispatch_keystroke(key, cx);
        }
        let field = window
            .focused(cx)
            .expect("the dialog's field has the keyboard");
        field.dispatch_action(&Confirm { secondary: false }, window, cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(
            g.status.branch.as_deref(),
            Some("fea"),
            "made, not switched"
        )
    });
}

/// `git checkout -b origin/main` is accepted, and then `origin/main` is ambiguous: rebase, merge
/// and Compare against it would use the stale local branch. The creation (the first row, as for
/// any name) is refused, and HEAD stays.
#[gpui_kit::test]
fn a_create_entry_refuses_a_remote_branchs_full_name(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    let _remote = add_remote(repo.path());
    git_run(repo.path(), &["checkout", "-q", "--detach", "feat"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-branch-here", cx))
        .unwrap();
    settle(handle, cx);
    cx.simulate_input(handle, "origin/main");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.branch, None, "still detached");
    });
    assert_eq!(
        git_run(repo.path(), &["rev-parse", "HEAD"]),
        git_run(repo.path(), &["rev-parse", "feat"]),
        "HEAD stayed on its commit"
    );
    assert_eq!(
        git_run(repo.path(), &["branch", "--list", "origin/main"]),
        "",
        "no local branch of a remote's name"
    );
    assert_eq!(notification_count(handle, cx), 1, "the reason");
}

#[gpui_kit::test]
fn branches_that_cannot_be_read_say_so_and_the_dialog_stays_closed(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = two_branches();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    std::fs::remove_dir_all(repo.path().join(".git")).unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_branch_dialog(Opened::Header, window, cx)
        });
    })
    .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(!window.has_active_dialog(cx), "nothing to list");
    })
    .unwrap();
    assert_eq!(notification_count(handle, cx), 1, "git's own message");
}
