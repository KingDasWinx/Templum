//! The Git block's top bar: Sync, Publish, the ⋯ menu, and the server's state.

use super::*;

/// `repo` with an origin, and a second clone of it whose pushes `repo` can get.
fn other_clone(remote: &tempfile::TempDir) -> tempfile::TempDir {
    let other = tempfile::tempdir().unwrap();
    git_run(
        other.path(),
        &["clone", "-q", &remote.path().display().to_string(), "."],
    );
    git_run(other.path(), &["config", "user.name", "B"]);
    git_run(other.path(), &["config", "user.email", "b@example.com"]);
    other
}

fn commit_in(dir: &std::path::Path, name: &str) {
    std::fs::write(dir.join(name), name).unwrap();
    git_run(dir, &["add", name]);
    git_run(dir, &["commit", "-q", "-m", name]);
}

#[gpui_kit::test]
fn sync_gets_and_sends_in_one_click(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let repo = committed_repo();
    let remote = add_remote(repo.path());
    let other = other_clone(&remote);
    commit_in(other.path(), "b.txt");
    git_run(other.path(), &["push", "-q"]);
    commit_in(repo.path(), "c.txt");
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    // Diverged: the question first. Put my commits on top.
    no_motion(cx);
    cx.update_window(handle, |_, window, cx| window.click("git-sync", cx))
        .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "both sides have new commits");
        window.within("dialog").click("git-rebase", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!((g.status.ahead, g.status.behind), (0, 0), "joined and sent");
        assert_eq!(g.sync, None, "no spinner left");
    });
    assert!(repo.path().join("b.txt").is_file());
    assert_eq!(
        git_run(repo.path(), &["log", "-1", "--format=%s"]),
        "c.txt\n",
        "mine on top"
    );
    // Then a plain one: the server got one more (the other clone takes ours first: no divergence there).
    git_run(other.path(), &["pull", "-q", "--ff-only"]);
    commit_in(other.path(), "d.txt");
    git_run(other.path(), &["push", "-q"]);
    cx.update_window(handle, |_, window, cx| window.click("git-sync", cx))
        .unwrap();
    settle(handle, cx);
    assert!(repo.path().join("d.txt").is_file(), "fast-forwarded");
}

#[gpui_kit::test]
fn publish_goes_to_the_only_remote_and_asks_among_several(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = committed_repo();
    let _remote = add_remote(repo.path());
    git_run(repo.path(), &["checkout", "-q", "-b", "topic"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-sync", cx))
        .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(
            g.status.upstream.as_deref(),
            Some("origin/topic"),
            "no question with one remote"
        )
    });
    // A second remote: Publish lists them.
    let backup = tempfile::tempdir().unwrap();
    git_run(backup.path(), &["init", "-q", "--bare"]);
    git_run(
        repo.path(),
        &[
            "remote",
            "add",
            "backup",
            &backup.path().display().to_string(),
        ],
    );
    git_run(repo.path(), &["checkout", "-q", "-b", "next"]);
    settle_read(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| window.click("git-sync", cx))
        .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(ws.read(cx).menu.is_some(), "the remotes' menu");
        // backup, origin: as `git remote` lists them.
        window.within("popup-menu").click(0usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| {
        assert_eq!(g.status.upstream.as_deref(), Some("backup/next"))
    });
}

#[gpui_kit::test]
fn without_a_remote_publish_says_so(cx: &mut TestAppContext) {
    let repo = committed_repo();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-sync", cx))
        .unwrap();
    settle(handle, cx);
    assert_eq!(notification_count(handle, cx), 1);
    assert_eq!(
        told_so_far(),
        ["No remote: add one to publish this branch."]
    );
    git_of(&ws, cx, |g| assert_eq!(g.status.upstream, None));
}

#[gpui_kit::test]
fn a_gone_upstream_offers_publish_not_sync(cx: &mut TestAppContext) {
    let repo = committed_repo();
    let _remote = add_remote(repo.path());
    git_run(repo.path(), &["checkout", "-q", "-b", "topic"]);
    git_run(repo.path(), &["push", "-q", "-u", "origin", "topic"]);
    git_run(repo.path(), &["push", "-q", "origin", "--delete", "topic"]);
    git_run(repo.path(), &["fetch", "-q", "--prune"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    git_of(&ws, cx, |g| {
        assert!(g.status.gone);
        assert_eq!(
            crate::git_block::words::server_state(&g.status).0,
            "not published"
        );
    });
    cx.update_window(handle, |_, window, cx| window.click("git-sync", cx))
        .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| assert!(!g.status.gone, "published again"));
}

#[gpui_kit::test]
fn the_more_menu_fetches_and_lists_recently_discarded(cx: &mut TestAppContext) {
    let repo = committed_repo();
    let remote = add_remote(repo.path());
    let other = other_clone(&remote);
    commit_in(other.path(), "b.txt");
    git_run(other.path(), &["push", "-q"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| {
        window.click("git-more", cx);
        window.render_frame(cx);
        assert!(ws.read(cx).menu.is_some());
        // Fetch, Pull, Push, —, Clone a repository… (no data folder: no Recently discarded…).
        window.within("popup-menu").click(0usize, cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    git_of(&ws, cx, |g| assert_eq!(g.status.behind, 1, "fetched"));
}

#[gpui_kit::test]
fn clone_from_a_folder_that_isnt_a_repository_opens_the_block_on_it(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let repo = committed_repo();
    let remote = add_remote(repo.path());
    let empty = tempfile::tempdir().unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, empty.path());
    git_of(&ws, cx, |g| {
        assert!(matches!(g.found, crate::git_block::Found::NotARepo))
    });
    cx.update_window(handle, |_, window, cx| {
        window.click("git-clone", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "URL and destination");
    })
    .unwrap();
    // The URL field has the keyboard; the destination holds the chosen folder.
    cx.simulate_input(handle, &remote.path().display().to_string());
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    let root = git_of(&ws, cx, |g| g.repo().map(|r| r.root.clone()));
    assert_eq!(
        root.unwrap().canonicalize().unwrap(),
        empty.path().canonicalize().unwrap(),
        "an empty folder takes the clone"
    );
    assert!(empty.path().join("a.txt").is_file());
}

#[gpui_kit::test]
fn a_failed_clone_says_why_and_leaves_no_folder(cx: &mut TestAppContext) {
    let parent = tempfile::tempdir().unwrap();
    std::fs::write(parent.path().join("keep.txt"), "").unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, parent.path());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.clone_repo(
                "/no/such/app.git".to_string(),
                parent.path().display().to_string(),
                window,
                cx,
            )
        })
    })
    .unwrap();
    git_of(&ws, cx, |g| {
        assert_eq!(g.sync, Some(crate::git_block::Sync::Cloning), "spinning")
    });
    settle(handle, cx);
    assert_eq!(notification_count(handle, cx), 1);
    // Git's line that says why: the repository isn't there.
    let told = told_so_far();
    assert!(
        told.len() == 1 && told[0].starts_with("fatal: ") && told[0].contains("no/such/app.git"),
        "{told:?}"
    );
    assert!(!parent.path().join("app").exists(), "nothing half-made");
    git_of(&ws, cx, |g| {
        assert_eq!(g.sync, None);
        assert!(matches!(g.found, crate::git_block::Found::NotARepo));
    });
}

#[gpui_kit::test]
fn sync_after_the_servers_branch_was_deleted_offers_publish(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = committed_repo();
    let remote = add_remote(repo.path());
    git_run(repo.path(), &["checkout", "-q", "-b", "topic"]);
    git_run(repo.path(), &["push", "-q", "-u", "origin", "topic"]);
    // Merged and deleted on the server; this clone hasn't fetched since.
    git_run(remote.path(), &["branch", "-q", "-D", "topic"]);
    commit_in(repo.path(), "c.txt");
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| window.click("git-sync", cx))
        .unwrap();
    settle(handle, cx);
    assert_eq!(
        git_run(remote.path(), &["branch", "--list", "topic"]),
        "",
        "nothing sent"
    );
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find("git-publish-gone").is_some(),
            "says the server's branch was deleted, not Up to date"
        );
        window.click("git-publish-gone", cx);
    })
    .unwrap();
    settle(handle, cx);
    assert_eq!(
        git_run(remote.path(), &["rev-parse", "topic"]),
        git_run(repo.path(), &["rev-parse", "topic"]),
        "published again"
    );
}

// Review I1: a question's answer acts on its own block, in its own workspace.
#[gpui_kit::test]
fn the_sync_question_answered_after_a_switch_acts_on_its_own_repository(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    // X: diverged from its server. Y: another repository, same block id in another workspace.
    let x = committed_repo();
    let x_remote = add_remote(x.path());
    let other = other_clone(&x_remote);
    commit_in(other.path(), "b.txt");
    git_run(other.path(), &["push", "-q"]);
    commit_in(x.path(), "c.txt");
    let y = committed_repo();
    let y_head = git_run(y.path(), &["rev-parse", "HEAD"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, x.path());
    cx.update_window(handle, |_, window, cx| window.click("git-sync", cx))
        .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(window.has_active_dialog(cx), "both sides have new commits")
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            ws.add_note_at(Point::new(3000.0, 3000.0), window, cx);
        })
    })
    .unwrap();
    settle(handle, cx);
    open_git(handle, &ws, cx, y.path());
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("git-rebase", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    assert_eq!(
        git_run(x_remote.path(), &["log", "-1", "--format=%s", "main"]),
        "c.txt\n",
        "X joined and sent"
    );
    assert_eq!(
        git_run(y.path(), &["rev-parse", "HEAD"]),
        y_head,
        "Y untouched"
    );
}

// Review I2: a clone that ends after a switch opens in its own workspace's Git, never in the one
// shown, and with no click.
#[gpui_kit::test]
fn a_clone_finished_after_a_switch_opens_its_own_block_only(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = committed_repo();
    let remote = add_remote(repo.path());
    let x = tempfile::tempdir().unwrap();
    let y = tempfile::tempdir().unwrap();
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, x.path());
    let first = ws.read_with(cx, |ws, _| ws.space.id.clone());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.clone_repo(
                remote.path().display().to_string(),
                x.path().display().to_string(),
                window,
                cx,
            );
            // The user moves on before the clone ends.
            ws.new_space(window, cx);
            ws.add_note_at(Point::new(3000.0, 3000.0), window, cx);
        })
    })
    .unwrap();
    open_git(handle, &ws, cx, y.path());
    settle(handle, cx);
    assert!(x.path().join("a.txt").is_file(), "cloned");
    git_of(&ws, cx, |g| {
        assert_eq!(g.folder, y.path(), "the block shown keeps its folder");
        assert!(matches!(g.found, crate::git_block::Found::NotARepo));
    });
    // Back in the first workspace, without a click: its block is on the clone.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let to = ws.order().iter().position(|s| s.id == first).unwrap();
            ws.switch_to(to, window, cx)
        })
    })
    .unwrap();
    settle(handle, cx);
    let root = git_of(&ws, cx, |g| g.repo().map(|r| r.root.clone()));
    assert_eq!(
        root.unwrap().canonicalize().unwrap(),
        x.path().canonicalize().unwrap()
    );
}

// Review M8: Clone… from a repository's ⋯ goes beside it, not inside it.
#[gpui_kit::test]
fn clone_from_a_repository_goes_beside_it(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let around = tempfile::tempdir().unwrap();
    let proj = around.path().join("proj");
    std::fs::create_dir(&proj).unwrap();
    for args in [
        ["init", "-q"].as_slice(),
        &["symbolic-ref", "HEAD", "refs/heads/main"],
        &["config", "user.name", "Ada"],
        &["config", "user.email", "ada@example.com"],
        &["config", "commit.gpgsign", "false"],
    ] {
        git_run(&proj, args);
    }
    commit_in(&proj, "a.txt");
    let server = tempfile::tempdir().unwrap();
    let bare = server.path().join("app.git");
    std::fs::create_dir(&bare).unwrap();
    git_run(&bare, &["init", "-q", "--bare"]);
    // Clones check out its HEAD: main, not git's default branch.
    git_run(&bare, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git_run(&proj, &["push", "-q", &bare.display().to_string(), "main"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, &proj);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.ask_clone(window, cx));
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
    })
    .unwrap();
    cx.simulate_input(handle, &bare.display().to_string());
    cx.update_window(handle, |_, window, cx| {
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
    })
    .unwrap();
    settle(handle, cx);
    assert!(!proj.join("app").exists(), "not inside the repository");
    assert!(
        around.path().join("app").join("a.txt").is_file(),
        "beside it"
    );
}

// Re-review: a Sync on a branch never published (made in a terminal, the status not read yet)
// says it isn't on the server yet, not that it was deleted there.
#[gpui_kit::test]
fn sync_on_a_branch_never_published_offers_publish(cx: &mut TestAppContext) {
    no_motion(cx);
    let repo = committed_repo();
    let remote = add_remote(repo.path());
    git_run(repo.path(), &["checkout", "-q", "-b", "new"]);
    let (handle, ws) = open(cx);
    open_git(handle, &ws, cx, repo.path());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.sync_git(window, cx))
    })
    .unwrap();
    settle(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        assert!(
            window.try_find("git-publish-gone").is_none(),
            "never deleted"
        );
        assert!(
            window.try_find("git-publish-new").is_some(),
            "not published yet"
        );
        window.click("git-publish-new", cx);
    })
    .unwrap();
    settle(handle, cx);
    assert_eq!(
        git_run(remote.path(), &["rev-parse", "new"]),
        git_run(repo.path(), &["rev-parse", "new"]),
        "published"
    );
}
