//! A Git block's History and Stashes tabs.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};
use templum_git::blame::BlameLine;
use templum_git::graph::graph;
use templum_git::ops::{self, Failure, Op};
use templum_git::trash::Kept;
use templum_git::{FileDiff, HistoryQuery, Search, Stash};

use crate::git_block::{self, Git, Tab, diff_view::Source};
use crate::prompt;

use super::git_home::parsed;
use super::remote::Deed;
use super::{Extra, GitAt, Workspace, notify_with, show_notification};

/// What History lists now: its branches and its search.
fn history_query(git: &Git) -> HistoryQuery {
    HistoryQuery {
        all: git.all_branches,
        search: (!git.searched.is_empty()).then(|| (git.search_mode, git.searched.clone())),
    }
}

impl Workspace {
    pub(crate) fn load_stashes(&mut self, at: GitAt, window: &mut Window, cx: &mut Context<Self>) {
        self.git_op(
            at,
            Op::Stashes,
            |repo| repo.stashes(),
            parsed::<Vec<Stash>>,
            |ws, at, stashes, _, _| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                match stashes {
                    Ok(s) => {
                        if git
                            .stash_shown
                            .is_some_and(|i| !s.iter().any(|st| st.index == i))
                        {
                            git.stash_shown = None;
                            git.stash_panel.clear();
                        }
                        git.stashes = s;
                    }
                    Err(e) => git.error = Some(e.to_string()),
                }
            },
            window,
            cx,
        );
    }

    /// Stash changes…: a message and whether untracked files go too.
    pub(crate) fn stash_changes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let ws = cx.entity();
        let asked = self.asked_on();
        prompt::ask_text_and_flag(
            "Stash changes",
            "Stash",
            "Message",
            "Include untracked files",
            move |message, untracked, window, cx| {
                let message = message.trim().to_string();
                if message.is_empty() {
                    return false;
                }
                ws.update(cx, |ws, cx| {
                    if !ws.answered_on(&asked, window, cx) {
                        return;
                    }
                    let op = Op::Stash {
                        message: message.clone(),
                        untracked,
                    };
                    ws.git_action(
                        op,
                        move |repo| repo.stash_push(&message, untracked),
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

    /// The stash at `index` as the Stashes tab shows it (with its hash, so a list changed
    /// meanwhile by a terminal is caught rather than acted on).
    pub(super) fn shown_stash(&self, index: usize) -> Option<templum_git::Stash> {
        self.space
            .git()?
            .stashes
            .iter()
            .find(|s| s.index == index)
            .cloned()
    }

    /// Apply (or Pop: apply and drop) a stash.
    pub(crate) fn stash_apply(
        &mut self,
        index: usize,
        pop: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(stash) = self.shown_stash(index) else {
            return;
        };
        let by = ops::StashRef {
            index: stash.index,
            hash: stash.hash.clone(),
        };
        let op = if pop {
            Op::StashPop { stash: by }
        } else {
            Op::StashApply { stash: by }
        };
        self.git_action(
            op,
            move |repo| {
                if pop {
                    repo.stash_pop(&stash)
                } else {
                    repo.stash_apply(&stash)
                }
            },
            window,
            cx,
        );
    }

    pub(crate) fn stash_drop(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        // Full on a mirror: refused here, before any question.
        if let Some(why) = self.why_not(Deed::Git("stash_drop")) {
            return self.warn(why, window, cx);
        }
        let Some(stash) = self.shown_stash(index) else {
            return;
        };
        let ws = cx.entity();
        let asked = self.asked_on();
        prompt::confirm(
            format!("Drop “{}”?", stash.message),
            "Its changes are lost. This can't be undone.",
            "Drop",
            move |window, cx| {
                let stash = stash.clone();
                ws.update(cx, |ws, cx| {
                    if !ws.answered_on(&asked, window, cx) {
                        return;
                    }
                    let op = Op::StashDrop {
                        stash: ops::StashRef {
                            index: stash.index,
                            hash: stash.hash.clone(),
                        },
                    };
                    ws.git_action(op, move |repo| repo.stash_drop(&stash), window, cx)
                })
            },
            window,
            cx,
        );
    }

    /// Shows a stash's changes, its new files too, in the Stashes tab's panel.
    pub(crate) fn show_stash(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(stash) = self.shown_stash(index) else {
            return;
        };
        let Some(git) = self.space.git_mut() else {
            return;
        };
        git.stash_shown = Some(index);
        let ignore_ws = git.stash_panel.ignore_ws;
        let title = format!("stash@{{{}}} · {}", stash.index, stash.message);
        let op = Op::StashDiff {
            stash: ops::StashRef {
                index: stash.index,
                hash: stash.hash.clone(),
            },
            ignore_whitespace: ignore_ws,
        };
        self.git_op(
            self.here(),
            op,
            move |repo| repo.stash_diff(&stash, ignore_ws),
            parsed::<Vec<FileDiff>>,
            move |ws, at, diff, _, cx| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                // Another stash clicked meanwhile shows instead.
                if git.stash_shown != Some(index) {
                    return;
                }
                match diff {
                    Ok(files) => {
                        git.stash_panel.source = Source::Range { title };
                        git.stash_panel.set_files(files);
                    }
                    Err(e) => git.error = Some(e.to_string()),
                }
                cx.notify();
            },
            window,
            cx,
        );
    }

    /// The diff on screen, read again (Ignore whitespace changed): the working changes, the
    /// comparison, the commit shown, or the stash shown.
    pub(crate) fn reload_shown_diff(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git() else {
            return;
        };
        match git.tab {
            Tab::History => {
                if let Some(shown) = &git.shown {
                    let hash = shown.commit.hash.clone();
                    self.show_commit(hash, window, cx)
                }
            }
            Tab::Stashes => {
                if let Some(index) = git.stash_shown {
                    self.show_stash(index, window, cx)
                }
            }
            Tab::Branches if git.compare.is_some() => {
                let at = self.here();
                self.load_compare(at, window, cx)
            }
            _ => self.reload_git_panel(window, cx),
        }
    }

    /// Loads a page of History: the first replaces the list (and reads which commits aren't
    /// pushed), a later one extends it; the graph is laid out again over the whole list, so its
    /// lanes go on. A result for a query changed meanwhile is dropped.
    pub(crate) fn load_history(
        &mut self,
        at: GitAt,
        skip: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        let q = history_query(git);
        let file = git.file.clone();
        let asked = (q.clone(), file.clone());
        let op = Op::History {
            all: q.all,
            search: q
                .search
                .clone()
                .map(|(by, text)| ops::SearchArg { by, text }),
            path: file.clone(),
            skip,
        };
        self.git_op(
            at,
            op,
            move |repo| ops::history(repo, &q, file.as_deref(), skip),
            parsed::<ops::Page>,
            move |ws, at, read, _, _| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                // Another query asked since, or this page already added (Load more twice).
                if (history_query(git), git.file.clone()) != asked
                    || (skip > 0 && git.commits.len() != skip)
                {
                    return;
                }
                match read {
                    Ok(ops::Page {
                        commits: page,
                        paths,
                        unpushed,
                    }) => {
                        git.more = page.len() == git_block::PAGE;
                        let (mut all, mut at_each) = if skip == 0 {
                            (Vec::new(), Vec::new())
                        } else {
                            ((*git.commits).clone(), (*git.file_paths).clone())
                        };
                        all.extend(page);
                        at_each.extend(paths);
                        // A search's or a file's list lacks the parents the lanes need.
                        let (q, file) = &asked;
                        git.graph = Rc::new(if q.search.is_none() && file.is_none() {
                            graph(&all)
                        } else {
                            Vec::new()
                        });
                        git.commits = Rc::new(all);
                        git.file_paths = Rc::new(at_each);
                        if let Some(u) = unpushed {
                            git.unpushed = Rc::new(u);
                        }
                        git.error = None;
                    }
                    Err(e) => git.error = Some(e.to_string()),
                }
            },
            window,
            cx,
        );
    }

    /// File history: History narrowed to `path`, through its renames.
    pub(crate) fn file_history(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        git.tab = Tab::History;
        git.file = Some(path);
        git.blame = false;
        git.blame_at = None;
        git.hide_shown();
        // The list read for another file (or the whole history) goes at once: a menu on one of
        // its rows would pair that commit with this file.
        git.commits = Rc::new(Vec::new());
        git.file_paths = Rc::new(Vec::new());
        git.graph = Rc::new(Vec::new());
        git.more = false;
        cx.notify();
        let at = self.here();
        // A block still looking its repository up reads the list once it has found it.
        self.refresh_lists_at(at, window, cx);
    }

    /// The file's chip ×: the whole history again.
    pub(crate) fn clear_file_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        git.file = None;
        git.blame = false;
        git.blame_at = None;
        git.hide_shown();
        git.commits = Rc::new(Vec::new());
        git.file_paths = Rc::new(Vec::new());
        git.graph = Rc::new(Vec::new());
        git.more = false;
        cx.notify();
        let at = self.here();
        self.load_history(at, 0, window, cx);
    }

    /// Restore this version…: asks, then writes the file as it was in `hash` (from `then`, its
    /// path at that commit) to its path now, after a copy of it in the trash.
    pub(crate) fn restore_version(
        &mut self,
        hash: String,
        then: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Full on a mirror: refused here, before any question.
        if let Some(why) = self.why_not(Deed::Git("restore_version")) {
            return self.warn(why, window, cx);
        }
        let Some(git) = self.space.git() else {
            return;
        };
        let (Some(now), Some(short)) = (
            git.file.clone(),
            git.commits
                .iter()
                .find(|c| c.hash == hash)
                .map(|c| c.short.clone()),
        ) else {
            return;
        };
        // A mirror's copy is kept by home, which has a trash unless it is told otherwise.
        let why = if self.trash.is_some() || self.on_remote() {
            "The file as it is now goes to Recently discarded."
        } else {
            "The file as it is now is replaced. This can't be undone."
        };
        let (entity, at) = (cx.entity(), self.here());
        prompt::confirm(
            format!("Restore {now} as it was in {short}?"),
            why,
            "Restore",
            move |window, cx| {
                let (hash, then, now, short, at) = (
                    hash.clone(),
                    then.clone(),
                    now.clone(),
                    short.clone(),
                    at.clone(),
                );
                entity.update(cx, |ws, cx| {
                    if ws.show_git(&at, window, cx) {
                        ws.restore_now(at, hash, then, now, short, window, cx)
                    }
                })
            },
            window,
            cx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn restore_now(
        &mut self,
        at: GitAt,
        hash: String,
        then: String,
        now: String,
        short: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trash = self.trash.clone();
        let done = format!("Restored {now} from {short}");
        let what = done.clone();
        let job_now = now.clone();
        let op = Op::RestoreVersion {
            hash: hash.clone(),
            then: then.clone(),
            path: now.clone(),
        };
        self.act_op(
            at,
            op,
            move |repo| ops::restore_version(repo, trash.as_ref(), &hash, &then, &job_now, &what),
            |heard| parsed::<Option<Kept>>(heard).map_err(Failure::Git),
            move |ws, at, result, window, cx| {
                let entity = cx.entity();
                match result {
                    Ok(Some(kept)) => {
                        let (entry, undo_at) = (kept.id, at.clone());
                        let undo: Extra = (
                            "git-undo",
                            "Undo".into(),
                            Rc::new(
                                move |ws: &mut Workspace,
                                      window: &mut Window,
                                      cx: &mut Context<Workspace>| {
                                    ws.restore_discarded(
                                        undo_at.clone(),
                                        entry.clone(),
                                        false,
                                        window,
                                        cx,
                                    )
                                },
                            ),
                        );
                        notify_with(
                            &entity,
                            at.clone(),
                            Notification::success(done),
                            vec![undo],
                            window,
                            cx,
                        );
                    }
                    Ok(None) => show_notification(window, cx, Notification::success(done)),
                    Err(failure) => {
                        let message = match failure {
                            Failure::Keep(e) => {
                                format!("Couldn't keep a copy, so nothing was restored: {e}")
                            }
                            Failure::Io(e) => format!("Couldn't write {now}: {e}"),
                            Failure::Git(e) => e.to_string(),
                            other => other.sentence(),
                        };
                        eprintln!("templum: {message}");
                        show_notification(window, cx, Notification::error(message));
                    }
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// File history (or Blame) from an editor's tree, in the Git popup: the workspace's Git
    /// moves to the file's repository first when it shows another.
    pub(crate) fn history_from_tree(
        &mut self,
        path: PathBuf,
        blame: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = templum_git::root_of(&path).map(Path::to_path_buf) else {
            let message = format!("{} isn't in a Git repository", path.display());
            show_notification(window, cx, Notification::warning(message));
            return;
        };
        let Ok(inside) = path.strip_prefix(&root) else {
            return;
        };
        let rel = inside
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let root_real = root.canonicalize().ok();
        let same = |a: &Path| a.canonicalize().ok() == root_real;
        let open = self
            .space
            .git()
            .is_some_and(|g| g.repo().map_or(same(&g.folder), |r| same(&r.root)));
        if !open && !self.set_git_folder(&root, window, cx) {
            return;
        }
        if blame {
            self.blame_file(rel, None, window, cx);
        } else {
            self.file_history(rel, window, cx);
        }
        self.open_git(window, cx);
    }

    /// All branches on or off: the list read again.
    pub(crate) fn toggle_all_branches(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        git.all_branches = !git.all_branches;
        git.hide_shown();
        cx.notify();
        let at = self.here();
        self.load_history(at, 0, window, cx);
    }

    /// Enter in a History search field (or the field cleared): that block's list read again.
    pub(crate) fn search_from(
        &mut self,
        field: gpui_kit::EntityId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .space
            .git()
            .is_some_and(|g| g.search.entity_id() == field)
        {
            self.run_search(window, cx);
        }
    }

    pub(crate) fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let text = git.search.read(cx).value().trim().to_string();
        git.searched = text;
        git.hide_shown();
        cx.notify();
        let at = self.here();
        self.load_history(at, 0, window, cx);
    }

    /// A mode clicked: with a search on, it runs again in that mode.
    pub(crate) fn set_search_mode(
        &mut self,
        mode: Search,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        git.search_mode = mode;
        let again = !git.searched.is_empty();
        cx.notify();
        if again {
            let at = self.here();
            self.load_history(at, 0, window, cx);
        }
    }

    /// A commit clicked (in the list, or a blame's group): the left column shows its message
    /// and files, the panel its whole diff (against its first parent).
    pub(crate) fn show_commit(
        &mut self,
        hash: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git() else {
            return;
        };
        if !git.has_repo() {
            return;
        }
        let loaded = git.commits.iter().find(|c| c.hash == hash).cloned();
        let ignore_ws = git.history_panel.ignore_ws;
        let asked = hash.clone();
        if let Some(git) = self.space.git_mut() {
            git.showing = Some(hash.clone());
        }
        // At home the commit is read there: `loaded` is for this computer's read only.
        let op = Op::Show {
            hash: hash.clone(),
            ignore_whitespace: ignore_ws,
        };
        self.git_op(
            self.here(),
            op,
            move |repo| ops::show(repo, &hash, loaded, ignore_ws),
            parsed::<ops::CommitShown>,
            move |ws, at, read, _, cx| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                // Left meanwhile (←, a search, another file…), or another commit clicked.
                if git.showing.as_deref() != Some(asked.as_str()) {
                    return;
                }
                match read {
                    Ok(ops::CommitShown {
                        commit,
                        files,
                        diffs,
                    }) => {
                        // In a file's history, that file's change alone, by its name then.
                        let then = git.file.as_ref().and_then(|_| {
                            git.commits
                                .iter()
                                .position(|c| c.hash == commit.hash)
                                .and_then(|i| git.file_paths.get(i))
                                .map(|f| f.path.clone())
                        });
                        let diffs = match then {
                            Some(then) => diffs.into_iter().filter(|d| d.path() == then).collect(),
                            None => diffs,
                        };
                        git.history_panel.source = Source::Range {
                            title: format!("Commit {}", commit.short),
                        };
                        git.history_panel.set_files(diffs);
                        // Another commit opens at the top of its details (the same one read
                        // again keeps its place).
                        if git
                            .shown
                            .as_ref()
                            .is_none_or(|s| s.commit.hash != commit.hash)
                        {
                            git.details_scroll.set_offset(Default::default());
                        }
                        git.shown = Some(git_block::Shown {
                            commit,
                            files,
                            file: None,
                        });
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

    /// The commit column's ←: back to the list, the panel empty (or the file's blame again).
    pub(crate) fn hide_commit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        git.hide_shown();
        let blame = git.blame;
        cx.notify();
        if blame {
            let at = self.here();
            self.load_blame(at, window, cx);
        }
    }

    /// Blame: History narrowed to the file, its panel showing who last changed each line (at
    /// `rev`, or in the working tree). A block still looking its repository up reads it once
    /// found (`refresh_lists_at` loads the blame while `blame` is on).
    pub(crate) fn blame_file(
        &mut self,
        path: String,
        rev: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.file_history(path, window, cx);
        if let Some(git) = self.space.git_mut() {
            git.blame = true;
            git.blame_at = rev;
        }
        let at = self.here();
        self.load_blame(at, window, cx);
    }

    /// The file's chip's Blame: off, the panel empties.
    pub(crate) fn toggle_blame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        if git.file.is_none() {
            return;
        }
        if git.blame {
            git.blame = false;
            git.blame_at = None;
            if git.shown.is_none() {
                git.history_panel.clear();
            }
            cx.notify();
        } else {
            git.blame = true;
            git.blame_at = None;
            git.hide_shown();
            cx.notify();
            let at = self.here();
            self.load_blame(at, window, cx);
        }
    }

    /// Reads the blame of History's file into its panel.
    pub(crate) fn load_blame(&mut self, at: GitAt, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        let (true, Some(path), true) = (git.has_repo(), git.file.clone(), git.blame) else {
            return;
        };
        let rev = git.blame_at.clone();
        let asked = (path.clone(), rev.clone());
        let op = Op::Blame {
            path: path.clone(),
            hash: rev.clone(),
        };
        self.git_op(
            at,
            op,
            move |repo| repo.blame(&path, rev.as_deref()),
            parsed::<Vec<BlameLine>>,
            move |ws, at, lines, _, cx| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                // Turned off, another file, or a commit opened meanwhile: not shown.
                if !git.blame
                    || git.shown.is_some()
                    || (git.file.as_ref(), git.blame_at.as_ref())
                        != (Some(&asked.0), asked.1.as_ref())
                {
                    return;
                }
                match lines {
                    Ok(lines) => {
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_or(0, |d| d.as_secs() as i64);
                        let (file, hashes) = templum_git::blame::blame_diff(&asked.0, &lines, now);
                        let title = match &asked.1 {
                            Some(rev) => {
                                format!("Blame · {} at {}", asked.0, &rev[..rev.len().min(7)])
                            }
                            None => format!("Blame · {}", asked.0),
                        };
                        git.history_panel.source = Source::Blame { title, hashes };
                        git.history_panel.set_files(vec![file]);
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

    /// A file of the commit shown: the panel scrolls to it.
    pub(crate) fn show_commit_file(&mut self, path: String, cx: &mut Context<Self>) {
        let Some(git) = self.space.git_mut() else {
            return;
        };
        if let Some(ix) = git.history_panel.row_of(false, &path) {
            git.history_panel
                .scroll
                .scroll_to_item(ix, gpui_kit::ScrollStrategy::Top);
        }
        if let Some(shown) = git.shown.as_mut() {
            shown.file = Some(path);
        }
        cx.notify();
    }

    /// Checkout… on a commit: warns about the detached state first.
    pub(crate) fn checkout_commit(
        &mut self,
        hash: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = cx.entity();
        let asked = self.asked_on();
        prompt::confirm(
            format!("Check out {}?", &hash[..hash.len().min(7)]),
            "HEAD will be detached: commits made here belong to no branch until one is made from them.",
            "Check out",
            move |window, cx| {
                let hash = hash.clone();
                ws.update(cx, |ws, cx| {
                    if !ws.answered_on(&asked, window, cx) {
                        return;
                    }
                    let op = Op::Checkout { hash: hash.clone() };
                    ws.git_action(op, move |repo| repo.checkout_commit(&hash), window, cx)
                })
            },
            window,
            cx,
        );
    }

    pub(crate) fn tag_commit(&mut self, hash: String, window: &mut Window, cx: &mut Context<Self>) {
        let ws = cx.entity();
        let asked = self.asked_on();
        prompt::ask_text(
            "New tag",
            "Create",
            "",
            move |name, window, cx| {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return false;
                }
                let hash = hash.clone();
                ws.update(cx, |ws, cx| {
                    if !ws.answered_on(&asked, window, cx) {
                        return;
                    }
                    let op = Op::Tag {
                        name: name.clone(),
                        hash: hash.clone(),
                    };
                    ws.git_action(op, move |repo| repo.tag(&name, &hash), window, cx)
                });
                true
            },
            window,
            cx,
        );
    }

    pub(crate) fn revert_commit(
        &mut self,
        hash: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ws = cx.entity();
        let asked = self.asked_on();
        prompt::confirm(
            format!("Revert {}?", &hash[..hash.len().min(7)]),
            "A new commit undoes its changes.",
            "Revert",
            move |window, cx| {
                let hash = hash.clone();
                ws.update(cx, |ws, cx| {
                    if !ws.answered_on(&asked, window, cx) {
                        return;
                    }
                    let op = Op::Revert { hash: hash.clone() };
                    ws.git_action(op, move |repo| repo.revert(&hash), window, cx)
                })
            },
            window,
            cx,
        );
    }
}
