//! A workspace's Git: opening a repository, reading it, and running its commands in the
//! background.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::Sizable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, SharedString, Window, div, px};
use templum_git::{GitError, Repo};

use crate::git_block::{self, Found, Git, Read};
use crate::launcher;
use crate::prompt;

use super::{Body, GitAt, Workspace, git_error_message, remember, show_notification};

impl Workspace {
    /// Ctrl+Shift+G and the sidebar's Git button: the Git popup opens on the workspace's Git,
    /// or closes. A workspace without one yet gets one on the folder it works in; with no folder
    /// known, the folder dialog asks.
    pub(crate) fn toggle_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Open in a workspace that has no Git yet (another one opened it): it opens here.
        if self.git_open && self.space.git.is_some() {
            self.close_git(window, cx);
            return;
        }
        if self.space.git.is_none() {
            let Some(folder) = self.git_folder_guess() else {
                self.choose_git_folder(window, cx);
                return;
            };
            if !self.set_git_folder(&folder, window, cx) {
                return;
            }
        }
        self.open_git(window, cx);
    }

    /// Shows the popup on the workspace's Git, with the keyboard, read afresh.
    pub(crate) fn open_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git() else {
            return;
        };
        self.git_open = true;
        self.inbox_open = false;
        // A field (the summary, a filter) keeps the keyboard it just took on a click.
        if !git.focus.contains_focused(window, cx) {
            git.focus.focus(window, cx);
        }
        self.refresh_git(window, cx);
        cx.notify();
    }

    pub(crate) fn close_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.git_open = false;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    /// Where a workspace's first Git looks: the focused terminal's folder, another terminal's,
    /// the workspace's folder, the newest recent one.
    fn git_folder_guess(&self) -> Option<PathBuf> {
        let focused = self
            .space
            .focused
            .and_then(|id| self.space.blocks.get(&id))
            .and_then(Body::folder);
        focused
            .or_else(|| self.space.terminals().find_map(|(_, p)| p.term.cwd()))
            .or_else(|| self.space.folder.as_ref().map(PathBuf::from))
            .or_else(|| self.space.recent.first().cloned())
            .filter(|f| f.is_dir())
    }

    /// The folders the popup's folder menu offers: the repositories this workspace's terminals
    /// work in, then the recent folders; each once, the one shown left out, by where they really
    /// are (Git names a repository by its real path, macOS's temporary folders through a link).
    pub(crate) fn git_folders(&self) -> Vec<PathBuf> {
        let real = |f: &Path| f.canonicalize().unwrap_or_else(|_| f.to_path_buf());
        let mut seen: Vec<PathBuf> = self
            .space
            .git()
            .map(|g| real(g.repo().map_or(&g.folder, |r| &r.root)))
            .into_iter()
            .collect();
        let mut folders: Vec<PathBuf> = Vec::new();
        let terminals = self.space.terminals().filter_map(|(_, p)| {
            let folder = p.seen.folder.as_deref()?;
            folder
                .ancestors()
                .find(|d| d.join(".git").exists())
                .map(Path::to_path_buf)
        });
        for f in terminals.chain(self.space.recent.iter().cloned()) {
            let at = real(&f);
            if !seen.contains(&at) {
                seen.push(at);
                folders.push(f);
            }
        }
        folders
    }

    /// The folder dialog, to point the workspace's Git at another folder: from its folder menu,
    /// or when its folder is gone or isn't a repository.
    pub(crate) fn choose_git_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let entity = cx.entity();
        launcher::ask_folder_for(
            self,
            "Open a Git repository".to_string(),
            Rc::new(move |folder, window, cx| {
                entity.update(cx, |ws, cx| {
                    if ws.set_git_folder(folder, window, cx) {
                        ws.open_git(window, cx)
                    }
                });
                None
            }),
            window,
            cx,
        );
    }

    /// Points the workspace's Git at `folder`, looked up afresh: nothing of the repository it
    /// showed stays (lists, diffs, the commit's fields), and what its commands still running
    /// return is dropped. The folder becomes a recent one. A path that isn't a folder changes
    /// nothing (false).
    pub(crate) fn set_git_folder(
        &mut self,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !folder.is_dir() {
            let message = format!("{} isn't a folder", folder.display());
            eprintln!("templum: {message}");
            show_notification(window, cx, Notification::warning(message));
            return false;
        }
        let (tab, generation) = self
            .space
            .git()
            .map_or((git_block::Tab::Changes, 0), |g| (g.tab, g.generation + 1));
        let mut git = Git::new(folder.to_path_buf(), tab, window, cx);
        git.generation = generation;
        if self.git_open {
            git.focus.focus(window, cx);
        }
        self.space.git = Some(Box::new(git));
        remember(&mut self.space.recent, folder.to_path_buf());
        self.look_up_git(window, cx);
        cx.notify();
        true
    }

    /// Points the Git `at` names at `folder`, in its own workspace even when another one is on
    /// screen (a clone that ended there), and looks it up, as `set_git_folder` does. False when
    /// it changed folder since.
    pub(super) fn set_git_folder_at(
        &mut self,
        at: &GitAt,
        folder: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tab) = self.git_at(at).map(|g| g.tab) else {
            return false;
        };
        let mut git = Git::new(folder.to_path_buf(), tab, window, cx);
        git.generation = at.generation + 1;
        let Some(space) = self.space_by_id_mut(&at.space) else {
            return false;
        };
        remember(&mut space.recent, folder.to_path_buf());
        space.git = Some(Box::new(git));
        let now = GitAt {
            generation: at.generation + 1,
            ..at.clone()
        };
        self.look_up_at(now, window, cx);
        cx.notify();
        true
    }

    /// Finds the repository holding the block's folder, in the background, then reads it.
    pub(crate) fn look_up_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.here();
        self.look_up_at(at, window, cx);
    }

    /// `look_up_git` for the block `at` names, in whichever workspace.
    pub(super) fn look_up_at(&mut self, at: GitAt, window: &mut Window, cx: &mut Context<Self>) {
        let program = self.config.git.program().to_string();
        let Some(folder) = self.git_at(&at).map(|g| g.folder.clone()) else {
            return;
        };
        if !folder.is_dir() {
            return;
        }
        self.git_job(
            at,
            move || Repo::discover(&program, &folder),
            |ws, at, found, window, cx| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                git.found = match found {
                    Ok(Some(repo)) => Found::Repo(repo),
                    Ok(None) => Found::NotARepo,
                    Err(e) => Found::NoGit(e),
                };
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// `git init` in the block's folder, then reads it.
    pub(crate) fn init_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.space.git() else {
            return;
        };
        let (program, folder) = (self.config.git.program().to_string(), git.folder.clone());
        let at = self.here();
        self.act_job(
            at,
            move || Repo::init(&program, &folder),
            |ws, at, made, window, cx| match made {
                Ok(repo) => {
                    if let Some(git) = ws.git_at(&at) {
                        git.found = Found::Repo(repo);
                    }
                    ws.after_action(at, window, cx);
                }
                Err(e) => {
                    eprintln!("templum: {e}");
                    show_notification(window, cx, Notification::error(e.to_string()));
                }
            },
            window,
            cx,
        );
    }

    /// Shows a tab of a Git block, and reads its list.
    pub(crate) fn set_git_tab(
        &mut self,
        tab: git_block::Tab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(git) = self.space.git_mut() {
            git.tab = tab;
            cx.notify();
        }
        let at = self.here();
        self.refresh_lists_at(at, window, cx);
    }

    /// The Git of the workspace on screen (the History list's closure builds it too).
    pub(crate) fn here(&self) -> GitAt {
        GitAt {
            space: self.space.id.clone(),
            generation: self.space.git().map_or(0, |g| g.generation),
        }
    }

    /// Shows the Git `at` names in the popup, switching to its workspace; false when it changed
    /// folder since.
    pub(crate) fn show_git(
        &mut self,
        at: &GitAt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.space.id != at.space {
            let Some(to) = self.order().iter().position(|s| s.id == at.space) else {
                return false;
            };
            self.switch_to(to, window, cx);
        }
        if self.space.id == at.space && !self.git_open {
            self.open_git(window, cx);
        }
        // A Git that changed folder since isn't the one the notification is about.
        self.space.id == at.space
            && self
                .space
                .git()
                .is_some_and(|g| g.generation == at.generation)
    }

    /// After a command: the status now, and the shown tab's list once the status is in.
    pub(super) fn after_action(&mut self, at: GitAt, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(git) = self.git_at(&at) {
            git.pending_lists = true;
        }
        self.refresh_at(at, window, cx);
    }

    /// Reads the list behind the shown tab (History, Branches or Stashes) in the background.
    pub(super) fn refresh_lists_at(
        &mut self,
        at: GitAt,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        if git.busy {
            git.pending_lists = true;
            return;
        }
        let tab = git.tab;
        // The tab's list job queues behind it.
        self.load_default(at.clone(), window, cx);
        match tab {
            git_block::Tab::Changes => {}
            git_block::Tab::History => {
                let blame = self.git_at(&at).is_some_and(|g| g.blame);
                self.load_history(at.clone(), 0, window, cx);
                if blame {
                    self.load_blame(at, window, cx);
                }
            }
            git_block::Tab::Branches => {
                let comparing = self.git_at(&at).is_some_and(|g| g.compare.is_some());
                self.load_branches(at.clone(), window, cx);
                if comparing {
                    self.load_compare(at, window, cx);
                }
            }
            git_block::Tab::Stashes => self.load_stashes(at, window, cx),
        }
    }

    /// Reads the block's status in the background: at once, or when the running command ends.
    pub(crate) fn refresh_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.here();
        self.refresh_at(at, window, cx);
    }

    pub(super) fn refresh_at(&mut self, at: GitAt, window: &mut Window, cx: &mut Context<Self>) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        let Some(repo) = git.repo().cloned() else {
            return;
        };
        if git.busy {
            git.pending = true;
            return;
        }
        self.git_job(
            at,
            move || Read::of(&repo),
            |ws, at, read, window, cx| {
                let Some(git) = ws.git_at(&at) else {
                    return;
                };
                git.reads += 1;
                match read {
                    Ok(read) => {
                        git.status = read.status;
                        git.operation = read.operation;
                        git.author = read.author;
                        git.error = None;
                        git.point_placeholder(window, cx);
                    }
                    Err(e) => git.error = Some(e.to_string()),
                }
                // The panel follows the status (a file staged in a terminal, a file gone).
                if at.space == ws.space.id {
                    ws.reload_git_panel(window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Every git_block::POLL: reads the workspace's Git again while the popup shows it.
    pub(super) fn poll_git(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.git_open {
            return;
        }
        let Some(git) = self.space.git_mut() else {
            return;
        };
        let gone = !git.folder.is_dir();
        if gone != git.gone {
            git.gone = gone;
            cx.notify();
        }
        // Still reading: skipped, so a slow repository never piles reads up.
        if git.repo().is_some() && !git.busy {
            self.refresh_git(window, cx);
        }
    }

    /// The Git `at` names, in whichever workspace it is, unless it changed folder since.
    pub(super) fn git_at(&mut self, at: &GitAt) -> Option<&mut Git> {
        self.space_by_id_mut(&at.space)?
            .git_mut()
            .filter(|g| g.generation == at.generation)
    }

    /// `git_job` for an action (not a read): while any action is asked and not done, the
    /// block's action buttons are disabled.
    pub(super) fn act_job<T: Send + 'static>(
        &mut self,
        at: GitAt,
        job: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut Workspace, GitAt, T, &mut Window, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        git.acting += 1;
        self.git_job(
            at,
            job,
            move |ws, at, result, window, cx| {
                if let Some(git) = ws.git_at(&at) {
                    git.acting = git.acting.saturating_sub(1);
                }
                done(ws, at, result, window, cx)
            },
            window,
            cx,
        );
    }

    /// Runs `job` on the background executor, the block marked busy meanwhile, then `done` on
    /// the UI thread with its result. A refresh asked while it ran follows, once. A block
    /// closed in the meantime gets nothing.
    pub(super) fn git_job<T: Send + 'static>(
        &mut self,
        at: GitAt,
        job: impl FnOnce() -> T + Send + 'static,
        done: impl FnOnce(&mut Workspace, GitAt, T, &mut Window, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.git_at(&at) else {
            return;
        };
        // One command at a time per block: this one waits for the running one.
        if git.busy {
            let at = at.clone();
            git.queued.push(Box::new(move |ws, window, cx| {
                ws.git_job(at, job, done, window, cx)
            }));
            return;
        }
        git.busy = true;
        cx.notify();
        cx.spawn_in(window, async move |ws, cx| {
            let result = cx.background_executor().spawn(async move { job() }).await;
            ws.update_in(cx, |ws, window, cx| {
                if let Some(git) = ws.git_at(&at) {
                    git.busy = false;
                }
                done(ws, at.clone(), result, window, cx);
                let (again, lists) = ws.git_at(&at).map_or((false, false), |g| {
                    (
                        std::mem::take(&mut g.pending),
                        std::mem::take(&mut g.pending_lists),
                    )
                });
                if again {
                    if lists {
                        // Taken back: the status read's completion runs the lists.
                        if let Some(g) = ws.git_at(&at) {
                            g.pending_lists = true;
                        }
                    }
                    ws.refresh_at(at.clone(), window, cx);
                } else if lists {
                    ws.refresh_lists_at(at.clone(), window, cx);
                }
                // Then what waited, unless the refresh above took the turn.
                let next = ws
                    .git_at(&at)
                    .filter(|g| !g.busy && !g.queued.is_empty())
                    .map(|g| g.queued.remove(0));
                if let Some(next) = next {
                    next(ws, window, cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }
}

/// One more button on a failed command's notification: its element id, its label, what it does.
pub(crate) type Extra = (
    &'static str,
    SharedString,
    Rc<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>,
);

/// A failed git command: stderr's first line, Details for all of it, Run in a terminal (a
/// terminal beside the block with the command typed in), and `extra`'s buttons.
pub(crate) fn notify_git_error(
    ws: &Entity<Workspace>,
    at: GitAt,
    err: &GitError,
    extra: Vec<Extra>,
    window: &mut Window,
    cx: &mut App,
) {
    notify_git_failure(ws, at, err, git_error_message(err), extra, window, cx);
}

/// A failed git command said in `message`'s words (git's own stay under Details).
pub(crate) fn notify_git_failure(
    ws: &Entity<Workspace>,
    at: GitAt,
    err: &GitError,
    message: String,
    extra: Vec<Extra>,
    window: &mut Window,
    cx: &mut App,
) {
    eprintln!("templum: {err}");
    let (err, ws) = (err.clone(), ws.clone());
    let notification = Notification::error(message)
        .title(err.command.clone())
        .autohide(false)
        .content(move |_, _, cx| {
            let note = cx.entity();
            let (details, run) = (err.clone(), err.clone());
            let (ws_run, ws_details) = (ws.clone(), ws.clone());
            let (run_at, run_note) = (at.clone(), note.clone());
            let extras: Vec<AnyElement> = extra
                .iter()
                .map(|(id, label, act)| {
                    let (ws, act, note, at) = (ws.clone(), act.clone(), note.clone(), at.clone());
                    Button::new(*id)
                        .label(label.clone())
                        .small()
                        .primary()
                        .on_click(move |_, window, cx| {
                            // The block's own workspace first: block ids repeat across them.
                            ws.update(cx, |ws, cx| {
                                if ws.show_git(&at, window, cx) {
                                    act(ws, window, cx)
                                }
                            });
                            note.update(cx, |n, cx| n.dismiss(window, cx));
                        })
                        .into_any_element()
                })
                .collect();
            // Wraps: three buttons are wider than a notification, which clips what overflows.
            div()
                .flex()
                .flex_wrap()
                .gap(px(6.0))
                .mt(px(6.0))
                .child(
                    Button::new("git-details")
                        .label("Details")
                        .small()
                        .on_click(move |_, window, cx| {
                            // Read now: the caller may be updating the workspace.
                            let mono = ws_details.read(cx).cell.font.family.clone();
                            prompt::details(
                                details.command.clone(),
                                details.stderr.clone(),
                                mono,
                                window,
                                cx,
                            )
                        }),
                )
                .child(
                    Button::new("git-run")
                        .label("Run in a terminal")
                        .small()
                        .on_click(move |_, window, cx| {
                            let command = run.command.clone();
                            ws_run.update(cx, |ws, cx| {
                                if !ws.show_git(&run_at, window, cx) {
                                    return;
                                }
                                // The repository's root, else the block's folder (a failed
                                // clone or init has no repository).
                                let folder = ws.space.git().map(|g| {
                                    g.repo()
                                        .map_or_else(|| g.folder.clone(), |r| r.root.clone())
                                });
                                if let Some(folder) = folder {
                                    ws.terminal_beside(None, &folder, Some(&command), window, cx);
                                }
                            });
                            run_note.update(cx, |n, cx| n.dismiss(window, cx));
                        }),
                )
                .children(extras)
                .into_any_element()
        });
    show_notification(window, cx, notification);
}

/// A notification about something done ("Discarded src/x.rs"), with buttons that act on the
/// block it is about (Undo): each shows the block's workspace first, and does nothing when the
/// block changed folder since.
pub(crate) fn notify_with(
    ws: &Entity<Workspace>,
    at: GitAt,
    notification: Notification,
    buttons: Vec<Extra>,
    window: &mut Window,
    cx: &mut App,
) {
    let ws = ws.clone();
    let notification = notification.content(move |_, _, cx| {
        let note = cx.entity();
        div()
            .flex()
            .gap(px(6.0))
            .mt(px(6.0))
            .children(buttons.iter().map(|(id, label, act)| {
                let (ws, act, note, at) = (ws.clone(), act.clone(), note.clone(), at.clone());
                Button::new(*id)
                    .label(label.clone())
                    .small()
                    .primary()
                    .on_click(move |_, window, cx| {
                        ws.update(cx, |ws, cx| {
                            if ws.show_git(&at, window, cx) {
                                act(ws, window, cx)
                            }
                        });
                        note.update(cx, |n, cx| n.dismiss(window, cx));
                    })
                    .into_any_element()
            }))
            .into_any_element()
    });
    show_notification(window, cx, notification);
}
