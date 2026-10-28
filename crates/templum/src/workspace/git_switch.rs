//! Switching a Git block's branch without losing work: the question about uncommitted changes
//! (a stash per branch, popped on return), the unpushed notice. See docs/specs/git.md,
//! section 4, Switching branches.

use std::rc::Rc;

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};
use templum_git::ops::Op;
use templum_git::{Back, Branch, Status};

use crate::git_block::switcher::{self, Opened};
use crate::git_block::{self, words};
use crate::prompt::{self, Act, Choice};

use super::git_home::parsed;
use super::{Extra, Workspace, notify_git_error, notify_with, show_notification};

impl Workspace {
    /// The branch dialog, for switching and creating: the branches read afresh (git's own message
    /// when it can't), then the dialog. From a commit (`opened`) the dialog lists no branches, so it
    /// opens at once.
    pub(crate) fn open_branch_dialog(
        &mut self,
        opened: Opened,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.space.git().is_some_and(|g| g.has_repo()) {
            return;
        }
        let dim = self.ui.title_dim;
        if matches!(opened, Opened::Commit(_)) {
            switcher::open(
                cx.entity(),
                self.here(),
                Vec::new(),
                opened,
                dim,
                window,
                cx,
            );
            return;
        }
        self.git_op(
            self.here(),
            Op::Branches,
            |repo| repo.branches(),
            parsed::<Vec<Branch>>,
            move |ws, at, branches, window, cx| {
                let branches = match branches {
                    Ok(branches) => branches,
                    Err(e) => {
                        let entity = cx.entity();
                        notify_git_error(&entity, at, &e, Vec::new(), window, cx);
                        return;
                    }
                };
                if let Some(git) = ws.git_at(&at) {
                    git.branches = branches.clone();
                }
                switcher::open(cx.entity(), at, branches, opened, dim, window, cx);
            },
            window,
            cx,
        );
    }

    /// Creates the branch `name` at `from` (a commit; None: HEAD) and switches to it; git refuses
    /// a name it doesn't accept in its own words.
    pub(crate) fn create_branch(
        &mut self,
        name: String,
        from: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op = Op::NewBranch {
            name: name.clone(),
            hash: from.clone(),
        };
        self.git_action(
            op,
            move |repo| repo.new_branch(&name, from.as_deref()),
            window,
            cx,
        );
    }

    /// Switches to `name` (`remote`: "origin/x"). With uncommitted changes it asks what happens
    /// to them first; during a merge or a rebase it says to finish it.
    pub(crate) fn switch_branch(
        &mut self,
        name: String,
        remote: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git() else {
            return;
        };
        if let Some(op) = git.operation {
            let message = format!("Finish or abort the {} first.", op.name());
            show_notification(window, cx, Notification::warning(message));
            return;
        }
        let s = &git.status;
        if !remote && s.branch.as_deref() == Some(name.as_str()) {
            return;
        }
        // What the stash is named after: the branch, or the commit on a detached HEAD.
        let here = s
            .branch
            .clone()
            .or_else(|| s.oid.as_ref().map(|o| o[..o.len().min(7)].to_string()))
            .unwrap_or_default();
        if s.entries.is_empty() {
            self.switch_now(name, remote, None, window, cx);
            return;
        }
        let unpushed = unpushed(s)
            .map(|(branch, n)| format!("{branch} has {} not pushed yet.", words::commits(n)))
            .unwrap_or_default();
        let title = format!(
            "You have {}. What should happen to them?",
            words::changed_files(s.entries.len())
        );
        let (entity, block_at) = (cx.entity(), self.here());
        // Answered later: on its own block, in its own workspace.
        let act = |leave: Option<String>| -> Act {
            let (entity, name, at) = (entity.clone(), name.clone(), block_at.clone());
            Rc::new(move |window, cx| {
                let (name, leave) = (name.clone(), leave.clone());
                entity.update(cx, |ws, cx| {
                    if ws.show_git(&at, window, cx) {
                        ws.switch_now(name, remote, leave, window, cx)
                    }
                })
            })
        };
        let (commit, commit_at) = (entity.clone(), block_at.clone());
        prompt::choose(
            title,
            unpushed,
            vec![
                Choice {
                    id: "git-leave",
                    label: format!("Leave them on {here}").into(),
                    primary: true,
                    act: act(Some(here.clone())),
                },
                Choice {
                    id: "git-bring",
                    label: "Bring them along".into(),
                    primary: false,
                    act: act(None),
                },
                Choice {
                    id: "git-commit-first",
                    label: "Commit first".into(),
                    primary: false,
                    act: Rc::new(move |window, cx| {
                        commit.update(cx, |ws, cx| {
                            if ws.show_git(&commit_at, window, cx) {
                                ws.focus_summary(window, cx)
                            }
                        })
                    }),
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

    /// The switch itself (`leave`: the stash's name for the changes left behind), then what
    /// came back, and the branch left behind's commits not pushed.
    pub(crate) fn switch_now(
        &mut self,
        name: String,
        remote: bool,
        leave: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git() else {
            return;
        };
        let left = unpushed(&git.status);
        let here = git.status.branch.clone().unwrap_or_default();
        let (to, job_leave) = (name.clone(), leave.clone());
        let op = Op::Switch {
            branch: name.clone(),
            remote,
            leave: leave.clone(),
        };
        self.act_op(
            self.here(),
            op,
            move |repo| repo.switch(&to, remote, job_leave.as_deref()),
            parsed::<Back>,
            move |ws, at, result, window, cx| {
                let entity = cx.entity();
                let local = if remote {
                    name.split_once('/').map_or(name.as_str(), |(_, b)| b)
                } else {
                    name.as_str()
                };
                match result {
                    Ok(back) => {
                        match back {
                            Back::Nothing => {}
                            Back::Popped => show_notification(
                                window,
                                cx,
                                Notification::success(format!("Your changes on {local} are back")),
                            ),
                            Back::Kept(why) => show_notification(
                                window,
                                cx,
                                Notification::warning(format!(
                                    "Your changes left on {local} are still in Stashes: {why}"
                                )),
                            ),
                        }
                        if let Some((branch, n)) = left.clone() {
                            let push: Extra = (
                                "git-push-left",
                                format!("Push {branch}").into(),
                                Rc::new({
                                    let branch = branch.clone();
                                    move |ws, window, cx| ws.push_branch(branch.clone(), window, cx)
                                }),
                            );
                            let message =
                                format!("{branch} has {} not pushed yet", words::commits(n));
                            #[cfg(test)]
                            super::told(&message);
                            notify_with(
                                &entity,
                                at.clone(),
                                Notification::info(message),
                                vec![push],
                                window,
                                cx,
                            );
                        }
                    }
                    Err(e) => {
                        let mut extra: Vec<Extra> = Vec::new();
                        let refused = e.stderr.contains("would be overwritten")
                            || e.stderr.contains("local changes");
                        if leave.is_none() && refused && !here.is_empty() {
                            let (name, here) = (name.clone(), here.clone());
                            extra.push((
                                "git-leave-them",
                                format!("Leave them on {here}").into(),
                                Rc::new(move |ws, window, cx| {
                                    ws.switch_now(
                                        name.clone(),
                                        remote,
                                        Some(here.clone()),
                                        window,
                                        cx,
                                    )
                                }),
                            ));
                        }
                        notify_git_error(&entity, at.clone(), &e, extra, window, cx);
                    }
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// The Changes tab, its column shown, the summary with the keyboard (the `c` key, Commit
    /// first).
    pub(crate) fn focus_summary(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        git.tab = git_block::Tab::Changes;
        git.lists_folded = false;
        let summary = git.summary.clone();
        cx.notify();
        summary.update(cx, |s, cx| s.focus(window, cx));
    }

    /// Push `main` (from the unpushed notice): a branch not checked out, to its upstream.
    pub(crate) fn push_branch(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.git_sync(
            git_block::Sync::Push,
            Op::PushBranch {
                branch: branch.clone(),
            },
            move |repo| repo.push_branch(&branch),
            |_, at, e, window, cx| {
                let ws = cx.entity();
                notify_git_error(&ws, at, e, Vec::new(), window, cx)
            },
            window,
            cx,
        );
    }
}

/// The checked-out branch and its commits not pushed, when it has an upstream and some.
fn unpushed(s: &Status) -> Option<(String, u32)> {
    let branch = s.branch.clone()?;
    (s.upstream.is_some() && !s.gone && s.ahead > 0).then_some((branch, s.ahead))
}
