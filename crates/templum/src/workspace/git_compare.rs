//! A Git block's comparison: a branch against the default one (or a base chosen), its commits
//! on the left, its diff in the panel. See docs/specs/git.md, section 5, Comparison.

use std::rc::Rc;

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};
use templum_git::ops::{self, Op};

use crate::git_block::diff_view::{DiffPanel, Source};
use crate::git_block::{self, Compare, Tab, words};

use super::git_home::parsed;
use super::{GitAt, Workspace, show_notification};

impl Workspace {
    /// Compare `branch` with the default branch: the Branches tab shows it.
    pub(crate) fn compare_branch(
        &mut self,
        branch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let Some(base) = git.default_branch.clone() else {
            show_notification(
                window,
                cx,
                Notification::warning("No main branch to compare with."),
            );
            return;
        };
        let mut panel = DiffPanel::range(String::new());
        panel.split = git.split;
        git.compare = Some(Compare {
            branch,
            base,
            commits: Rc::new(Vec::new()),
            narrowed: None,
            panel,
        });
        git.tab = Tab::Branches;
        cx.notify();
        let at = self.here();
        self.refresh_lists_at(at, window, cx);
    }

    /// Reads the comparison: its commits, and the whole diff or the narrowed commit's. A result
    /// for a comparison changed meanwhile (another branch, base or commit) is dropped.
    pub(crate) fn load_compare(&mut self, at: GitAt, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        let (true, Some(c)) = (git.has_repo(), git.compare.as_ref()) else {
            return;
        };
        let asked = (c.branch.clone(), c.base.clone(), c.narrowed.clone());
        let ignore_ws = c.panel.ignore_ws;
        let (branch, base, narrowed) = asked.clone();
        let op = Op::Compare {
            base: base.clone(),
            branch: branch.clone(),
            hash: narrowed.clone(),
            ignore_whitespace: ignore_ws,
        };
        self.git_op(
            at,
            op,
            move |repo| ops::compare(repo, &base, &branch, narrowed.as_deref(), ignore_ws),
            parsed::<ops::Compared>,
            move |ws, at, read, _, cx| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                let Some(c) = git.compare.as_mut().filter(|c| {
                    (&c.branch, &c.base, &c.narrowed) == (&asked.0, &asked.1, &asked.2)
                }) else {
                    return;
                };
                match read {
                    Ok(ops::Compared { commits, files }) => {
                        let title = match &c.narrowed {
                            Some(hash) => format!(
                                "{} vs {} · commit {}",
                                c.branch,
                                c.base,
                                &hash[..hash.len().min(7)]
                            ),
                            None => format!(
                                "{} vs {} · {}",
                                c.branch,
                                c.base,
                                words::commits(commits.len() as u32)
                            ),
                        };
                        c.panel.source = Source::Range { title };
                        c.commits = Rc::new(commits);
                        c.panel.set_files(files);
                        git.error = None;
                    }
                    Err(e) => git.error = Some(e.to_string()),
                }
                cx.notify();
            },
            window,
            cx,
        );
    }

    /// A commit of the comparison (Some) narrows the panel to it; the heading (None) shows them
    /// all again.
    pub(crate) fn narrow_compare(
        &mut self,
        hash: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(c) = self.space.git_mut().and_then(|g| g.compare.as_mut()) else {
            return;
        };
        c.narrowed = hash;
        let at = self.here();
        self.load_compare(at, window, cx);
    }

    /// The column's ←: back to the branches.
    pub(crate) fn close_compare(&mut self, cx: &mut Context<Self>) {
        if let Some(git) = self.space.git_mut() {
            git.compare = None;
            cx.notify();
        }
    }

    /// The base button: any other branch, local or remote, from a searchable list.
    pub(crate) fn choose_compare_base(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git() else {
            return;
        };
        let Some(branch) = git.compare.as_ref().map(|c| c.branch.clone()) else {
            return;
        };
        let names: Vec<String> = git
            .branches
            .iter()
            .map(|b| b.name.clone())
            .filter(|b| *b != branch)
            .collect();
        let at = self.here();
        git_block::switcher::pick(
            cx.entity(),
            "Compare with",
            names,
            move |ws, base, window, cx| {
                // Answered later: on its own block, in its own workspace.
                if !ws.show_git(&at, window, cx) {
                    return;
                }
                if let Some(c) = ws.space.git_mut().and_then(|g| g.compare.as_mut()) {
                    c.base = base;
                    c.narrowed = None;
                }
                ws.load_compare(at.clone(), window, cx);
            },
            window,
            cx,
        );
    }
}
