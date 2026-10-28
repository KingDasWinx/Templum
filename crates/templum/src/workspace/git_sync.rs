//! A Git block's Sync and Publish (docs/specs/git.md, section 4, Sync).

use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};
use templum_git::Synced;
use templum_git::ops::{self, Op};

use crate::git_block::{self, words};
use crate::menu::Target;
use crate::prompt::{self, Choice};

use super::git_home::parsed;
use super::remote::Deed;
use super::{Extra, GitAt, Workspace, notify_git_error, notify_with, show_notification};

impl Workspace {
    /// Sync: fetch, fast-forward, push, then "Got 3 commits, sent 2". Both sides new: asks how
    /// to join them.
    pub(crate) fn sync_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.run_sync(None, window, cx);
    }

    /// A diverged Sync, joined by a rebase (`rebase`) or a merge, then pushed.
    pub(crate) fn sync_join(&mut self, rebase: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.run_sync(Some(rebase), window, cx);
    }

    fn run_sync(&mut self, join: Option<bool>, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        // A refused one must not leave the Sync button spinning.
        if let Some(why) = self.why_not(Deed::Git("sync")) {
            return self.warn(why, window, cx);
        }
        let at = self.here();
        if self.git_runs(&at).is_none() {
            return;
        }
        let Some(git) = self.space.git_mut() else {
            return;
        };
        // At home it runs on and can't be stopped from here: no flag.
        let cancel = Arc::new(AtomicBool::new(false));
        git.sync = Some(git_block::Sync::Both);
        git.cancel = git.home.is_none().then(|| cancel.clone());
        self.act_op(
            at,
            Op::Sync { join },
            move |repo| ops::sync(&repo.clone().with_cancel(cancel), join),
            parsed::<Synced>,
            move |ws, at, result, window, cx| {
                if let Some(git) = ws.git_at(&at) {
                    git.sync = None;
                    git.cancel = None;
                }
                match result {
                    Ok(Synced::Done { got, sent }) => show_notification(
                        window,
                        cx,
                        Notification::success(words::synced(got, sent)),
                    ),
                    Ok(done @ (Synced::Gone | Synced::Unpublished)) => {
                        let (id, sentence) = if done == Synced::Gone {
                            (
                                "git-publish-gone",
                                "This branch was deleted on the server: nothing was sent. Publish branch sends it again.",
                            )
                        } else {
                            (
                                "git-publish-new",
                                "This branch isn't on the server yet: nothing was sent. Publish branch sends it.",
                            )
                        };
                        let publish: Extra = (
                            id,
                            "Publish branch".into(),
                            Rc::new(move |ws, window, cx| {
                                let branch = ws.space.git().and_then(|g| g.status.branch.clone());
                                if let Some(branch) = branch {
                                    ws.publish(branch, window, cx)
                                }
                            }),
                        );
                        notify_with(
                            &entity,
                            at.clone(),
                            Notification::warning(sentence),
                            vec![publish],
                            window,
                            cx,
                        );
                    }
                    Ok(Synced::Diverged { .. }) => {
                        // Answered later: on its own block, in its own workspace.
                        let join = |rebase: bool| -> prompt::Act {
                            let (entity, at) = (entity.clone(), at.clone());
                            Rc::new(move |window, cx| {
                                entity.update(cx, |ws, cx| {
                                    if ws.show_git(&at, window, cx) {
                                        ws.sync_join(rebase, window, cx)
                                    }
                                })
                            })
                        };
                        prompt::choose(
                            "Your branch and the server's both have new commits.",
                            "",
                            vec![
                                Choice {
                                    id: "git-rebase",
                                    label: "Put my commits on top (rebase)".into(),
                                    primary: true,
                                    act: join(true),
                                },
                                Choice {
                                    id: "git-merge",
                                    label: "Merge".into(),
                                    primary: false,
                                    act: join(false),
                                },
                                Choice {
                                    id: "cancel",
                                    label: "Cancel".into(),
                                    primary: false,
                                    act: Rc::new(|_, _| {}),
                                },
                            ],
                            window,
                            cx,
                        );
                    }
                    // Cancelling isn't a failure: nothing to tell.
                    Err(e) if e.cancelled() => {}
                    Err(e) => notify_git_error(&entity, at.clone(), &e, Vec::new(), window, cx),
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// Publish branch: to the repository's only remote at once; with several, a menu of them at
    /// the mouse; with none, "No remote" with Run in a terminal (`git remote add origin `
    /// typed, for the user to finish).
    pub(crate) fn publish(&mut self, branch: String, window: &mut Window, cx: &mut Context<Self>) {
        let at_mouse = window.mouse_position();
        self.git_op(
            self.here(),
            Op::Remotes,
            |repo| repo.remotes(),
            parsed::<Vec<String>>,
            move |ws, at, remotes, window, cx| {
                let entity = cx.entity();
                // The workspace changed meanwhile: its block with this id isn't the one asked.
                if ws.space.id != at.space || ws.git_at(&at).is_none() {
                    return;
                }
                match remotes {
                    Ok(r) if r.len() == 1 => ws.publish_to(r[0].clone(), branch, window, cx),
                    Ok(r) if r.is_empty() && at.home.is_some() => {
                        let words = format!(
                            "No remote: add one on {} to publish this branch.",
                            at.home.as_deref().unwrap_or_default()
                        );
                        show_notification(window, cx, Notification::warning(words));
                    }
                    Ok(r) if r.is_empty() => {
                        let run: Extra = (
                            "git-add-remote",
                            "Run in a terminal".into(),
                            Rc::new(move |ws, window, cx| {
                                let root = ws
                                    .space
                                    .git()
                                    .and_then(|g| g.repo().map(|r| r.root.clone()));
                                if let Some(root) = root {
                                    ws.terminal_beside(
                                        None,
                                        &root,
                                        Some("git remote add origin "),
                                        window,
                                        cx,
                                    );
                                }
                            }),
                        );
                        let words = "No remote: add one to publish this branch.";
                        #[cfg(test)]
                        super::told(words);
                        notify_with(
                            &entity,
                            at,
                            Notification::warning(words),
                            vec![run],
                            window,
                            cx,
                        );
                    }
                    Ok(remotes) => ws.open_menu(
                        Target::GitRemotes {
                            at,
                            branch,
                            remotes,
                        },
                        at_mouse,
                        window,
                        cx,
                    ),
                    Err(e) => notify_git_error(&entity, at, &e, Vec::new(), window, cx),
                }
            },
            window,
            cx,
        );
    }

    /// Clone a repository…: a URL and where to put it: the block's folder at first, or beside
    /// the block's repository (from its ⋯), never inside it.
    pub(crate) fn ask_clone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Clone stays on home for now.
        if let Some(why) = self.why_not(Deed::GitLeftOut) {
            return self.warn(why, window, cx);
        }
        let Some(folder) = self.space.git().map(|g| {
            let beside = g
                .repo()
                .and_then(|r| r.root.parent().map(|p| p.to_path_buf()));
            beside
                .unwrap_or_else(|| g.folder.clone())
                .display()
                .to_string()
        }) else {
            return;
        };
        let (ws, at) = (cx.entity(), self.here());
        prompt::ask_two(
            "Clone a repository",
            "Clone",
            ("https://github.com/you/app.git", String::new()),
            ("Destination folder", folder),
            move |url, dest, window, cx| {
                let url = url.trim().to_string();
                if url.is_empty() || dest.trim().is_empty() {
                    return false;
                }
                let at = at.clone();
                ws.update(cx, |ws, cx| {
                    if ws.show_git(&at, window, cx) {
                        ws.clone_repo(url, dest, window, cx)
                    }
                });
                true
            },
            window,
            cx,
        );
    }

    /// Clones `url` into `dest` (or a folder named after it there, `clone_target`) in the
    /// background, the Clone (or Sync) button spinning and cancelling; then the block opens on
    /// the clone.
    pub(crate) fn clone_repo(
        &mut self,
        url: String,
        dest: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let program = self.config.git.program().to_string();
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let into = templum_git::clone_target(&crate::launcher::expand(dest.trim()), &url);
        let cancel = Arc::new(AtomicBool::new(false));
        git.sync = Some(git_block::Sync::Cloning);
        git.cancel = Some(cancel.clone());
        self.act_job(
            self.here(),
            move || templum_git::Repo::clone_into(&program, &url, &into, Some(cancel)),
            move |ws, at, cloned, window, cx| {
                if let Some(git) = ws.git_at(&at) {
                    git.sync = None;
                    git.cancel = None;
                }
                match cloned {
                    Ok(repo) => {
                        // The block opens on the clone, in its own workspace even when another
                        // one is on screen (its block with the same id is another one); a block
                        // gone or pointed elsewhere since is left alone.
                        ws.set_git_folder_at(&at, &repo.root, window, cx);
                        let message = format!("Cloned into {}", repo.root.display());
                        show_notification(window, cx, Notification::success(message));
                    }
                    Err(e) if e.cancelled() => {}
                    Err(e) => {
                        let entity = cx.entity();
                        notify_git_error(&entity, at, &e, Vec::new(), window, cx);
                    }
                }
            },
            window,
            cx,
        );
    }

    /// Reads the default branch in the background (after an action, and when the block opens).
    pub(crate) fn load_default(&mut self, at: GitAt, window: &mut Window, cx: &mut Context<Self>) {
        self.git_op(
            at,
            Op::DefaultBranch,
            |repo| repo.default_branch(),
            |heard| parsed::<Option<String>>(heard).ok().flatten(),
            |ws, at, default, _, _| {
                if let Some(git) = ws.git_at(&at) {
                    git.default_branch = default;
                }
            },
            window,
            cx,
        );
    }

    /// `git push -u <remote> <branch>`, its button spinning.
    pub(crate) fn publish_to(
        &mut self,
        remote: String,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git_sync(
            git_block::Sync::Push,
            Op::Publish {
                remote: remote.clone(),
                branch: branch.clone(),
            },
            move |repo| repo.publish(&remote, &branch),
            |_, at, e, window, cx| {
                let ws = cx.entity();
                notify_git_error(&ws, at, e, Vec::new(), window, cx)
            },
            window,
            cx,
        );
    }
}
