//! A Git block's branches, and its fetch, pull and push.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::{Context, Window};
use templum_git::ops::Op;
use templum_git::{Branch, GitError, Repo};

use crate::git_block::{self, words};
use crate::prompt;

use super::git_home::parsed;
use super::remote::Deed;
use super::{Extra, GitAt, Workspace, notify_git_error};

impl Workspace {
    pub(crate) fn load_branches(&mut self, at: GitAt, window: &mut Window, cx: &mut Context<Self>) {
        self.git_op(
            at,
            Op::Branches,
            |repo| repo.branches(),
            parsed::<Vec<Branch>>,
            |ws, at, branches, _, _| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                match branches {
                    Ok(b) => git.branches = b,
                    Err(e) => git.error = Some(e.to_string()),
                }
            },
            window,
            cx,
        );
    }

    pub(crate) fn rename_branch(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = cx.entity();
        let asked = self.asked_on();
        let from = name.clone();
        prompt::ask_text(
            "Rename branch",
            "Rename",
            name,
            move |to, window, cx| {
                let to = to.trim().to_string();
                if to.is_empty() || to == from {
                    return false;
                }
                let from = from.clone();
                ws.update(cx, |ws, cx| {
                    if !ws.answered_on(&asked, window, cx) {
                        return;
                    }
                    let op = Op::RenameBranch {
                        branch: from.clone(),
                        name: to.clone(),
                    };
                    ws.git_action(op, move |repo| repo.rename_branch(&from, &to), window, cx)
                });
                true
            },
            window,
            cx,
        );
    }

    /// Merges `name` into the current branch, after asking.
    pub(crate) fn merge_branch(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .space
            .git()
            .and_then(|g| g.status.branch.clone())
            .unwrap_or_else(|| "HEAD".to_string());
        let ws = cx.entity();
        let asked = self.asked_on();
        prompt::confirm(
            format!("Merge {name} into {current}?"),
            "Conflicts, if any, show in the Changes tab.",
            "Merge",
            move |window, cx| {
                let name = name.clone();
                ws.update(cx, |ws, cx| {
                    if !ws.answered_on(&asked, window, cx) {
                        return;
                    }
                    let op = Op::Merge {
                        branch: name.clone(),
                    };
                    ws.git_action(op, move |repo| repo.merge(&name), window, cx)
                })
            },
            window,
            cx,
        );
    }

    /// Deletes a local branch after asking; an unmerged one asks again, to force.
    pub(crate) fn delete_branch(
        &mut self,
        name: String,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Full on a mirror: refused here, before any question.
        if let Some(why) = self.why_not(Deed::Git("delete_branch")) {
            return self.warn(why, window, cx);
        }
        if force {
            self.delete_now(name, true, window, cx);
            return;
        }
        let ws = cx.entity();
        let asked = self.asked_on();
        prompt::confirm(
            format!("Delete {name}?"),
            "Its commits stay in the history while something points at them.",
            "Delete",
            move |window, cx| {
                let name = name.clone();
                ws.update(cx, |ws, cx| {
                    if ws.answered_on(&asked, window, cx) {
                        ws.delete_now(name, false, window, cx)
                    }
                })
            },
            window,
            cx,
        );
    }

    pub(super) fn delete_now(
        &mut self,
        name: String,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let job_name = name.clone();
        let op = Op::DeleteBranch {
            branch: name.clone(),
            force,
        };
        self.act_op(
            self.here(),
            op,
            move |repo| repo.delete_branch(&job_name, force),
            parsed::<()>,
            move |ws, at, result, window, cx| {
                match result {
                    Err(e) if !force && templum_git::not_merged(&e.stderr) => {
                        ws.ask_delete_anyway(at.clone(), name.clone(), window, cx)
                    }
                    Err(e) => {
                        let entity = cx.entity();
                        notify_git_error(&entity, at.clone(), &e, Vec::new(), window, cx);
                    }
                    Ok(()) => {}
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// `git branch -d` refused: counts what only that branch has, then asks to force it. The
    /// answer acts on the block it is about, in its own workspace (`show_git`).
    pub(super) fn ask_delete_anyway(
        &mut self,
        at: GitAt,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let counted = name.clone();
        let op = Op::Unmerged {
            branch: name.clone(),
        };
        self.git_op(
            at,
            op,
            move |repo| repo.unmerged_count(&counted),
            parsed::<u32>,
            move |_, at, count, window, cx| {
                let entity = cx.entity();
                prompt::confirm(
                    format!("Delete {name} anyway?"),
                    words::unmerged(count.ok()),
                    "Delete",
                    move |window, cx| {
                        let (name, at) = (name.clone(), at.clone());
                        entity.update(cx, |ws, cx| {
                            if ws.show_git(&at, window, cx) {
                                ws.delete_now(name, true, window, cx)
                            }
                        })
                    },
                    window,
                    cx,
                );
            },
            window,
            cx,
        );
    }

    /// The Branches tab's Remote heading: its rows show or hide.
    pub(crate) fn toggle_remote_branches(&mut self, cx: &mut Context<Self>) {
        if let Some(git) = self.space.git_mut() {
            git.remote_folded = !git.remote_folded;
            cx.notify();
        }
    }

    /// The Branches tab's Publish: `git push -u <remote> <name>`, the remote by Publish's rule.
    pub(crate) fn publish_branch(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.publish(name, window, cx);
    }

    /// Runs one of fetch, pull and push, its button spinning meanwhile.
    /// The spinning button: kills the running fetch, pull or push.
    pub(crate) fn cancel_git(&mut self, cx: &mut Context<Self>) {
        // Home's command runs on: a mirror can't stop it.
        if self.space.git().is_some_and(|g| g.home.is_some()) {
            return;
        }
        if let Some(cancel) = self.space.git().and_then(|g| g.cancel.as_ref()) {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            cx.notify();
        }
    }

    pub(super) fn git_sync(
        &mut self,
        which: git_block::Sync,
        op: Op,
        job: impl FnOnce(&Repo) -> Result<(), GitError> + Send + 'static,
        on_err: impl FnOnce(&mut Workspace, GitAt, &GitError, &mut Window, &mut Context<Workspace>)
        + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A refused one must not leave the Sync button spinning.
        if let Some(why) = self.why_not(Deed::Git(op.name())) {
            return self.warn(why, window, cx);
        }
        let at = self.here();
        if self.git_runs(&at).is_none() {
            return;
        }
        let Some(git) = self.space.git_mut() else {
            return;
        };
        // Asked while something runs, it waits its turn (a Publish or a Merge from a
        // notification); the buttons themselves are disabled meanwhile. At home it runs on and
        // can't be stopped from here: no flag.
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        git.sync = Some(which);
        git.cancel = git.home.is_none().then(|| cancel.clone());
        self.act_op(
            at,
            op,
            move |repo| job(&repo.clone().with_cancel(cancel)),
            parsed::<()>,
            move |ws, at, result, window, cx| {
                if let Some(git) = ws.git_at(&at) {
                    git.sync = None;
                    git.cancel = None;
                }
                match result {
                    // Cancelling isn't a failure: nothing to tell.
                    Err(e) if !e.cancelled() => on_err(ws, at.clone(), &e, window, cx),
                    _ => {}
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    pub(crate) fn fetch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_sync(
            git_block::Sync::Fetch,
            Op::Fetch,
            |repo| repo.fetch(),
            |_, at, e, window, cx| {
                let ws = cx.entity();
                notify_git_error(&ws, at, e, Vec::new(), window, cx)
            },
            window,
            cx,
        );
    }

    /// `--ff-only`; when that isn't possible, the notification offers Merge and Rebase.
    pub(crate) fn pull(
        &mut self,
        how: templum_git::Pull,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use templum_git::Pull;
        self.git_sync(
            git_block::Sync::Pull,
            Op::Pull { how },
            move |repo| repo.pull(how),
            move |_, at, e, window, cx| {
                let ws = cx.entity();
                let mut extra: Vec<Extra> = Vec::new();
                if how == Pull::FastForward && templum_git::not_fast_forward(&e.stderr) {
                    extra.push((
                        "git-pull-merge",
                        "Merge".into(),
                        Rc::new(move |ws, window, cx| ws.pull(Pull::Merge, window, cx)),
                    ));
                    extra.push((
                        "git-pull-rebase",
                        "Rebase".into(),
                        Rc::new(move |ws, window, cx| ws.pull(Pull::Rebase, window, cx)),
                    ));
                }
                notify_git_error(&ws, at, e, extra, window, cx);
            },
            window,
            cx,
        );
    }

    /// `git push`; a branch with no upstream (or one gone from the server) is published instead
    /// (the button says Publish branch: no question).
    pub(crate) fn push(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git() else {
            return;
        };
        let s = &git.status;
        match &s.branch {
            Some(branch) if s.upstream.is_none() || s.gone => {
                let branch = branch.clone();
                self.publish(branch, window, cx)
            }
            _ => self.push_now(window, cx),
        }
    }

    pub(super) fn push_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_sync(
            git_block::Sync::Push,
            Op::Push,
            |repo| repo.push(),
            |_, at, e, window, cx| {
                let ws = cx.entity();
                notify_git_error(&ws, at, e, Vec::new(), window, cx)
            },
            window,
            cx,
        );
    }
}
