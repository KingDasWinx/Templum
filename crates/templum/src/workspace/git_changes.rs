//! A Git block's changes: staging, discarding, diffs and commits.

use std::rc::Rc;

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};
use serde_json::Value;
use templum_git::ops::{self, Op, Undone};
use templum_git::{GitError, Repo};

use crate::git_block;
use crate::prompt;

use super::git_discard::REFUSED;
use super::git_home::parsed;
use super::remote::Deed;
use super::{
    Extra, GitAt, Workspace, notify_git_error, notify_git_failure, notify_with, show_notification,
};

impl Workspace {
    /// Commits with the form's message: what is staged, or every change (`Git::commit_all`).
    /// "Committed abc1234" with Undo commit, and the fields clear; with `push`, then pushes (a
    /// branch with no upstream asks to publish, as Push does).
    pub(crate) fn git_commit(&mut self, push: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git() else {
            return;
        };
        if !git.can_commit(cx) {
            return;
        }
        let (amend, all) = (git.amend, git.commit_all());
        let typed = git.summary.read(cx).value().trim().to_string();
        let description = git.description.read(cx).value().trim().to_string();
        let at = self.here();
        let op = Op::Commit {
            summary: typed.clone(),
            description: description.clone(),
            amend,
            all,
        };
        self.act_op(
            at,
            op,
            // No summary typed: the sentence for the one file (`can_commit` saw one), by what is
            // staged now; else the commit waits for a message.
            move |repo| ops::commit(repo, &typed, &description, amend, all),
            committed,
            move |ws, at, result, window, cx| {
                let entity = cx.entity();
                // Only a commit that happened is pushed.
                let mut pushed = false;
                match result {
                    Ok(None) => {
                        show_notification(
                            window,
                            cx,
                            Notification::warning(ops::CHANGED_MEANWHILE),
                        );
                        // Only where the person still is: `show_git` would take them back to a
                        // workspace or a popup they left while the commit ran.
                        if ws.space.id == at.space && ws.git_open && ws.git_at(&at).is_some() {
                            ws.focus_summary(window, cx);
                        }
                    }
                    Ok(Some(ops::Committed { short, replaced })) => {
                        if let Some(git) = ws.git_at(&at) {
                            git.amend = false;
                            let (summary, description) =
                                (git.summary.clone(), git.description.clone());
                            summary.update(cx, |s, cx| s.set_value("", window, cx));
                            description.update(cx, |d, cx| d.set_value("", window, cx));
                        }
                        let committed = Notification::success(format!("Committed {short}"));
                        if push {
                            // It goes to the server next: nothing to take back here.
                            show_notification(window, cx, committed);
                            pushed = true;
                        } else {
                            let undo: Extra = (
                                "git-undo-commit",
                                "Undo commit".into(),
                                Rc::new(
                                    move |ws: &mut Workspace,
                                          window: &mut Window,
                                          cx: &mut Context<Workspace>| {
                                        ws.undo_commit(
                                            short.clone(),
                                            replaced.clone(),
                                            window,
                                            cx,
                                        )
                                    },
                                ),
                            );
                            notify_with(&entity, at.clone(), committed, vec![undo], window, cx);
                        }
                    }
                    Err(e) => notify_git_error(&entity, at.clone(), &e, Vec::new(), window, cx),
                }
                ws.after_action(at, window, cx);
                if pushed {
                    ws.push(window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// The left column's fold button: the diff takes the whole width, or gives it back.
    pub(crate) fn toggle_lists(&mut self, cx: &mut Context<Self>) {
        if let Some(git) = self.space.git_mut() {
            git.lists_folded = !git.lists_folded;
            cx.notify();
        }
    }

    /// Ctrl+Enter in a block's summary or description.
    pub(crate) fn commit_from(
        &mut self,
        field: gpui_kit::EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.space.git().is_some_and(|g| g.owns_field(field)) {
            self.git_commit(false, window, cx);
        }
    }

    /// Amend on: the fields show the last commit's message; off: they clear.
    pub(crate) fn set_amend(&mut self, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        git.amend = on;
        git.point_placeholder(window, cx);
        cx.notify();
        let (summary, description) = (git.summary.clone(), git.description.clone());
        if !on {
            summary.update(cx, |s, cx| s.set_value("", window, cx));
            description.update(cx, |d, cx| d.set_value("", window, cx));
            return;
        }
        let at = self.here();
        self.git_op(
            at,
            Op::LastMessage,
            |repo| repo.last_message(),
            parsed::<String>,
            move |ws, at, message, window, cx| {
                let Ok(message) = message else {
                    return;
                };
                ws.fill_message(&at, &message, window, cx);
            },
            window,
            cx,
        );
    }

    /// The fields hold `message`: its first line the summary, the rest the description.
    fn fill_message(
        &mut self,
        at: &GitAt,
        message: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git_at(at) else {
            return;
        };
        let (summary, description) = (git.summary.clone(), git.description.clone());
        let (first, rest) = message.split_once('\n').unwrap_or((message, ""));
        summary.update(cx, |s, cx| s.set_value(first.to_string(), window, cx));
        description.update(cx, |d, cx| d.set_value(rest.trim().to_string(), window, cx));
    }

    /// Undo commit (the notification after a commit): takes it back, its changes staged and its
    /// message in the fields. Pushed meanwhile, it can't be taken back for others: Revert…
    /// instead, after saying why.
    pub(crate) fn undo_commit(
        &mut self,
        short: String,
        replaced: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let asked = self.asked_on();
        let job_short = short.clone();
        let op = Op::UndoCommit {
            hash: short.clone(),
            replaced: replaced.clone(),
        };
        self.act_op(
            self.here(),
            op,
            move |repo| ops::undo_commit(repo, &job_short, replaced.as_deref()),
            parsed::<Undone>,
            move |ws, at, result, window, cx| {
                match result {
                    Ok(Undone::Done(message)) => {
                        if let Some(git) = ws.git_at(&at) {
                            git.amend = false;
                        }
                        ws.fill_message(&at, &message, window, cx);
                    }
                    Ok(Undone::Moved) => {
                        let message = format!(
                            "A commit was made since {short}; Undo commit only takes back the last commit."
                        );
                        eprintln!("templum: {message}");
                        show_notification(window, cx, Notification::error(message));
                    }
                    Ok(Undone::Pushed) => {
                        let entity = cx.entity();
                        prompt::confirm(
                            format!("Revert {short}?"),
                            "Pushed commits can't be removed for others; this adds a new commit that undoes it.",
                            "Revert",
                            move |window, cx| {
                                let short = short.clone();
                                entity.update(cx, |ws, cx| {
                                    if !ws.answered_on(&asked, window, cx) {
                                        return;
                                    }
                                    let hash = short.clone();
                                    ws.git_action(
                                        Op::Revert { hash },
                                        move |repo| repo.revert(&short),
                                        window,
                                        cx,
                                    )
                                })
                            },
                            window,
                            cx,
                        );
                    }
                    Err(e) => {
                        let entity = cx.entity();
                        notify_git_error(&entity, at.clone(), &e, Vec::new(), window, cx)
                    }
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// Asks the name, then the email, and writes both to the global config.
    pub(crate) fn set_git_author(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ws = cx.entity();
        prompt::ask_text(
            "Your name for commits",
            "Next",
            "",
            move |name, window, cx| {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return false;
                }
                let ws = ws.clone();
                // The next dialog opens once this one has closed.
                window.defer(cx, move |window, cx| {
                    ws.update(cx, |ws, cx| ws.ask_git_email(name, window, cx))
                });
                true
            },
            window,
            cx,
        );
    }

    /// The second half of Set…: the email, then both are written.
    pub(super) fn ask_git_email(
        &mut self,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = cx.entity();
        let asked = self.asked_on();
        prompt::ask_text(
            "Your email for commits",
            "Set",
            "",
            move |email, window, cx| {
                let email = email.trim().to_string();
                if email.is_empty() {
                    return false;
                }
                let name = name.clone();
                ws.update(cx, |ws, cx| {
                    if !ws.answered_on(&asked, window, cx) {
                        return;
                    }
                    let op = Op::SetAuthor {
                        name: name.clone(),
                        email: email.clone(),
                    };
                    ws.git_action(
                        op,
                        move |repo| repo.set_author(&name, &email, true),
                        window,
                        cx,
                    )
                });
                true
            },
            window,
            cx,
        );
    }

    /// A click on the left column's row: it is marked, and the panel scrolls to that file.
    pub(crate) fn select_git_row(
        &mut self,
        list: git_block::List,
        path: String,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let staged = git.in_staged(&path);
        if let Some(ix) = git.panel.row_of(staged, &path) {
            git.panel
                .scroll
                .scroll_to_item(ix, gpui_kit::ScrollStrategy::Top);
        }
        git.selected = Some((list, path));
        cx.notify();
    }

    /// Reads the whole working diff again in the background (after every status read): a
    /// result equal to what the panel shows changes nothing.
    pub(crate) fn reload_git_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        if !git.has_repo() {
            return;
        }
        let (status, ignore_ws, whole) = (
            git.status.clone(),
            git.panel.ignore_ws,
            git.panel.whole.clone(),
        );
        // On a mirror: what the diff is read with, and when.
        if git.home.is_some() {
            git.home_diff = Some((status.clone(), std::time::Instant::now()));
        }
        let at = self.here();
        let op = Op::Diff {
            ignore_whitespace: ignore_ws,
            whole: whole
                .iter()
                .map(|(staged, path)| ops::WholeFile {
                    staged: *staged,
                    path: path.clone(),
                })
                .collect(),
        };
        self.git_op(
            at,
            op,
            move |repo| repo.working_diff(&status, ignore_ws, &whole),
            parsed::<templum_git::WorkingDiff>,
            |ws, at, diff, _, cx| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                git.panel.reads += 1;
                match diff {
                    Ok(diff) => {
                        if git.panel.set(diff) {
                            cx.notify();
                        }
                    }
                    Err(e) => git.error = Some(e.to_string()),
                }
            },
            window,
            cx,
        );
    }

    /// The file row's arrow, or the row itself: its rows show or hide.
    pub(crate) fn toggle_fold(&mut self, key: (bool, String), cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let p = git.diff_mut();
        if !p.folded.remove(&key) {
            p.folded.insert(key);
        }
        p.rebuild();
        cx.notify();
    }

    /// Expand all (false) or Collapse all (true).
    pub(crate) fn fold_all(&mut self, folded: bool, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let p = git.diff_mut();
        p.folded = if folded {
            p.files
                .iter()
                .map(git_block::diff_view::Shown::key)
                .collect()
        } else {
            Default::default()
        };
        p.rebuild();
        cx.notify();
    }

    /// A fold's click: the file is read again with every line as context.
    pub(crate) fn show_whole(
        &mut self,
        key: (bool, String),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        if !git.panel.whole.contains(&key) {
            git.panel.whole.push(key);
        }
        self.reload_git_panel(window, cx);
    }

    /// Show diff on a large or generated file.
    pub(crate) fn show_large(&mut self, key: (bool, String), cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let p = git.diff_mut();
        p.large_shown.insert(key);
        p.rebuild();
        cx.notify();
    }

    /// The mouse went down on a line row: one click starts a selection there, two select the
    /// word, three the line.
    pub(crate) fn diff_press(
        &mut self,
        row: usize,
        x: gpui_kit::Pixels,
        clicks: usize,
        cx: &mut Context<Self>,
    ) {
        use crate::git_block::diff_view::select::{Pos, Selection, word_at};
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let p = git.diff_mut();
        // Split: the press picks the side; a drag keeps it (a selection stays on one side).
        if p.split {
            p.split_right = p.side_at(row, x);
        }
        let Some(col) = p.col_at(row, x) else {
            return;
        };
        let text = p.display_of(row).unwrap_or_default();
        let (a, b) = match clicks {
            2 => {
                let r = word_at(&text, col);
                (r.start, r.end)
            }
            n if n >= 3 => (0, text.len()),
            _ => (col, col),
        };
        p.selection = Some(Selection {
            anchor: Pos { row, col: a },
            head: Pos { row, col: b },
        });
        p.dragging = clicks == 1;
        cx.notify();
    }

    /// The mouse moved over a line row with the button down: the selection's head follows.
    pub(crate) fn diff_drag(&mut self, row: usize, x: gpui_kit::Pixels, cx: &mut Context<Self>) {
        use crate::git_block::diff_view::select::Pos;
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let p = git.diff_mut();
        if !p.dragging {
            return;
        }
        let (Some(col), Some(sel)) = (p.col_at(row, x), p.selection.as_mut()) else {
            return;
        };
        sel.head = Pos { row, col };
        cx.notify();
    }

    /// The mouse came up: a selection that never moved is no selection.
    pub(crate) fn diff_release(&mut self, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let p = git.diff_mut();
        p.dragging = false;
        if p.selection.is_some_and(|s| s.is_empty()) {
            p.selection = None;
            cx.notify();
        }
    }

    /// Unified or Split, for every panel of the block.
    pub(crate) fn set_split(&mut self, on: bool, cx: &mut Context<Self>) {
        if let Some(git) = self.space.git_mut() {
            git.set_split(on);
            cx.notify();
        }
    }

    /// Ignore whitespace on or off: the diff is read again with or without `-w`.
    pub(crate) fn toggle_ignore_whitespace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let p = git.diff_mut();
        p.ignore_ws = !p.ignore_ws;
        cx.notify();
        self.reload_shown_diff(window, cx);
    }

    /// Runs `op` on the block's repository in the background (`job` here, home's `git` on a
    /// mirror); its failure is a notification, and the block is read again either way.
    pub(crate) fn git_action(
        &mut self,
        op: Op,
        job: impl FnOnce(&Repo) -> Result<(), GitError> + Send + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let at = self.here();
        self.act_op(
            at,
            op,
            job,
            parsed::<()>,
            |ws, at, result, window, cx| {
                if let Err(e) = result {
                    let entity = cx.entity();
                    notify_git_error(&entity, at.clone(), &e, Vec::new(), window, cx);
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    pub(crate) fn stage(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op = Op::Stage {
            paths: paths.clone(),
        };
        self.git_action(op, move |repo| repo.stage(&refs(&paths)), window, cx);
    }

    /// A file's box in the Changes checklist: checked, it is unstaged whole; unchecked or
    /// checked in part, it is staged whole.
    pub(crate) fn toggle_check(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(check) = self
            .space
            .git()
            .and_then(|g| g.status.entries.iter().find(|e| e.path == path))
            .map(git_block::Check::of)
        else {
            return;
        };
        if check == git_block::Check::Checked {
            self.unstage(vec![path], window, cx);
        } else {
            self.stage(vec![path], window, cx);
        }
    }

    /// The checklist's top box, over the files shown (Filter files narrows them): all checked,
    /// they are all unstaged; else they are all staged.
    pub(crate) fn check_all(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git() else {
            return;
        };
        let all = paths.iter().all(|p| {
            git.status
                .entries
                .iter()
                .find(|e| e.path == *p)
                .is_some_and(|e| git_block::Check::of(e) == git_block::Check::Checked)
        });
        if all {
            self.unstage(paths, window, cx);
        } else {
            self.stage(paths, window, cx);
        }
    }

    pub(crate) fn unstage(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op = Op::Unstage {
            paths: paths.clone(),
        };
        self.git_action(op, move |repo| repo.unstage(&refs(&paths)), window, cx);
    }

    /// A hunk's Stage (`unstage` false) or Unstage: `patch` applied on the index. One git
    /// refuses (the file changed since it was read) says so in the user's words, and the block
    /// reads again either way.
    pub(crate) fn apply_hunk(
        &mut self,
        patch: String,
        unstage: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let op = if unstage {
            Op::UnstageHunk {
                patch: patch.clone(),
            }
        } else {
            Op::StageHunk {
                patch: patch.clone(),
            }
        };
        self.act_op(
            self.here(),
            op,
            move |repo| repo.apply_patch(&patch, true, unstage),
            parsed::<()>,
            |ws, at, result, window, cx| {
                if let Err(e) = result {
                    let entity = cx.entity();
                    notify_git_failure(
                        &entity,
                        at.clone(),
                        &e,
                        REFUSED.to_string(),
                        Vec::new(),
                        window,
                        cx,
                    );
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// Appends the path to the repository's .gitignore.
    pub(crate) fn ignore_path(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Not a git command: a failure is said plainly, with nothing to run in a terminal.
        let op = Op::Ignore { path: path.clone() };
        self.act_op(
            self.here(),
            op,
            move |repo| repo.ignore(&path),
            |heard| {
                parsed::<()>(heard).map_err(|e| std::io::Error::other(e.first_line().to_string()))
            },
            |ws, at, written, window, cx| {
                if let Err(e) = written {
                    let message = format!("Couldn't write .gitignore: {e}");
                    eprintln!("templum: {message}");
                    show_notification(window, cx, Notification::error(message));
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    pub(crate) fn abort_operation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(op) = self.space.git().and_then(|g| g.operation) else {
            return;
        };
        // Full on a mirror: refused at the entry, before anything is sent.
        if let Some(why) = self.why_not(Deed::Git("abort")) {
            return self.warn(why, window, cx);
        }
        self.git_action(Op::Abort, move |repo| repo.abort(op), window, cx);
    }

    pub(crate) fn continue_operation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(op) = self.space.git().and_then(|g| g.operation) else {
            return;
        };
        self.git_action(Op::Continue, move |repo| repo.continue_(op), window, cx);
    }

    /// Opens a repository path in an editor beside the Git block, at the first hunk of the
    /// file as the panel shows it (Staged when nothing of it is left unstaged, else Changes).
    pub(crate) fn open_from_git(
        &mut self,
        path: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git() else {
            return;
        };
        let staged = git.in_staged(path);
        let line = git
            .panel
            .first_line(staged, path)
            .or_else(|| git.panel.first_line(!staged, path));
        self.open_from_git_at(path, line, window, cx);
    }

    /// Opens a repository path in an editor beside the Git block, at `line` (the camera goes
    /// there).
    pub(crate) fn open_from_git_at(
        &mut self,
        path: &str,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Open in editor stays on home for now.
        if let Some(why) = self.why_not(Deed::GitLeftOut) {
            return self.warn(why, window, cx);
        }
        let Some(root) = self
            .space
            .git()
            .and_then(|g| g.repo().map(|r| r.root.clone()))
        else {
            return;
        };
        self.open_in_editor(&root, &root.join(path), line, None, window, cx);
    }
}

/// Home's answer to a commit as `ops::commit` gives it: a refusal because the files changed
/// since is `None`, as locally.
fn committed(heard: Result<Value, GitError>) -> Result<Option<ops::Committed>, GitError> {
    match parsed::<ops::Committed>(heard) {
        Ok(committed) => Ok(Some(committed)),
        Err(e) if e.stderr == ops::CHANGED_MEANWHILE => Ok(None),
        Err(e) => Err(e),
    }
}

/// Borrows a list of paths as git's arguments.
fn refs(paths: &[String]) -> Vec<&str> {
    paths.iter().map(String::as_str).collect()
}
