//! A Git block's discards: asked first, measured, a copy kept in the trash when it fits, Undo on
//! the notification, Recently discarded… to put one back later. See docs/specs/git.md,
//! section 4, The trash.

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};
use std::rc::Rc;
use templum_git::ops::{self, Discarding, Failure, Measured, Op};
use templum_git::trash::{self, Kept, Restored};

use crate::editor::human;
use crate::git_block;
use crate::prompt;

use super::git_home::{Runs, parsed};
use super::remote::Deed;
use super::{
    Extra, GitAt, Workspace, notify_git_error, notify_git_failure, notify_with, show_notification,
};

/// What a hunk's patch git refuses says: the file changed since it was read.
pub(crate) use templum_git::ops::CHANGED as REFUSED;
pub(crate) use templum_git::ops::thousands;

impl Workspace {
    /// Discard changes… and Discard all…: the unstaged changes of `paths`, after the question.
    pub(crate) fn discard(
        &mut self,
        paths: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Full on a mirror: refused here, before any question or measure.
        if let Some(why) = self.why_not(Deed::Git("discard")) {
            return self.warn(why, window, cx);
        }
        let Some(git) = self.space.git() else {
            return;
        };
        let entries: Vec<templum_git::Entry> = git
            .status
            .unstaged()
            .into_iter()
            .filter(|e| paths.contains(&e.path))
            .cloned()
            .collect();
        let (title, done) = match entries.as_slice() {
            [] => return,
            [one] => (
                format!("Discard the changes to {}?", one.path),
                format!("Discarded {}", one.path),
            ),
            many => (
                format!("Discard {} files?", thousands(many.len())),
                format!("Discarded {} files", thousands(many.len())),
            ),
        };
        let paths = entries.iter().map(|e| e.path.clone()).collect();
        self.ask_discard(paths, title, done, Discarding::Files(entries), window, cx);
    }

    /// A hunk's Discard: `patch` reversed on `path`, after the question.
    pub(crate) fn discard_hunk(
        &mut self,
        path: String,
        patch: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(why) = self.why_not(Deed::Git("discard_hunk")) {
            return self.warn(why, window, cx);
        }
        let title = format!("Discard this change to {path}?");
        let done = format!("Discarded a change to {path}");
        self.ask_discard(vec![path], title, done, Discarding::Hunk(patch), window, cx);
    }

    /// Measures what a copy would take (in the background: a build folder is thousands of
    /// files), then asks, saying whether the discard can be undone.
    #[allow(clippy::too_many_arguments)]
    fn ask_discard(
        &mut self,
        paths: Vec<String>,
        title: String,
        done: String,
        what: Discarding,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let measured = paths.clone();
        let asked = self.asked_on();
        // This computer's trash decides here; at home the copy is home's to keep.
        let trash_here = self.trash.is_some();
        self.git_op(
            self.here(),
            Op::Measure {
                paths: paths.clone(),
            },
            move |repo| {
                let (files, bytes) = trash::measure(&repo.root, &measured);
                Ok(Measured {
                    files,
                    bytes,
                    keep: trash_here && trash::fits(files, bytes),
                })
            },
            parsed::<Measured>,
            move |ws, at, measured, window, cx| {
                // A measure home refused asks nothing: its words are told, as any failure.
                let Measured { files, bytes, keep } = match measured {
                    Ok(measured) => measured,
                    Err(e) => {
                        let entity = cx.entity();
                        return notify_git_error(&entity, at, &e, Vec::new(), window, cx);
                    }
                };
                // Home keeps a trash of its own (minors.md: `measure` doesn't say).
                let has_trash = ws.trash.is_some() || at.home.is_some();
                let why = if keep {
                    let them = if paths.len() == 1 { "it" } else { "them" };
                    format!("You can restore {them} from Recently discarded.")
                } else if has_trash {
                    let size = if files > trash::MAX_FILES {
                        format!("{} files", thousands(files))
                    } else {
                        human(bytes)
                    };
                    format!("Too much to keep a copy: {size}. This can't be undone.")
                } else {
                    "This can't be undone.".to_string()
                };
                let entity = cx.entity();
                prompt::confirm(
                    title,
                    why,
                    "Discard",
                    move |window, cx| {
                        let (paths, done, what, at) =
                            (paths.clone(), done.clone(), what.clone(), at.clone());
                        entity.update(cx, |ws, cx| {
                            if ws.answered_on(&asked, window, cx) {
                                ws.discard_now(at, paths, done, what, keep, window, cx)
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

    /// The discard, in the background: the copy first (with `keep`), then git, then the copy's
    /// seal. A copy that can't be made stops it before anything is thrown away.
    #[allow(clippy::too_many_arguments)]
    fn discard_now(
        &mut self,
        at: GitAt,
        paths: Vec<String>,
        done: String,
        what: Discarding,
        keep: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let trash = self.trash.clone().filter(|_| keep);
        let hunk = matches!(what, Discarding::Hunk(_));
        let kept_as = done.clone();
        let op = match &what {
            Discarding::Files(_) => Op::Discard {
                paths: paths.clone(),
            },
            Discarding::Hunk(patch) => Op::DiscardHunk {
                path: paths.first().cloned().unwrap_or_default(),
                patch: patch.clone(),
            },
        };
        self.act_op(
            at,
            op,
            move |repo| ops::discard(repo, trash.as_ref(), &paths, &kept_as, &what),
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
                    Err(Failure::Keep(e)) => {
                        let message =
                            format!("Couldn't keep a copy, so nothing was discarded: {e}");
                        eprintln!("templum: {message}");
                        show_notification(window, cx, Notification::error(message));
                    }
                    Err(Failure::Git(e)) if hunk => notify_git_failure(
                        &entity,
                        at.clone(),
                        &e,
                        REFUSED.to_string(),
                        Vec::new(),
                        window,
                        cx,
                    ),
                    Err(Failure::Git(e)) => {
                        notify_git_error(&entity, at.clone(), &e, Vec::new(), window, cx)
                    }
                    // A discard has no other way to fail.
                    Err(other) => {
                        show_notification(window, cx, Notification::error(other.sentence()))
                    }
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// Puts discard `entry` back (Undo, or Restore in Recently discarded…) in the Git `at` names,
    /// which is shown first: an entry is of the trash of the computer that kept it, and only
    /// that Git can put it back. A file changed since the discard asks before it is written
    /// over (`force`).
    pub(crate) fn restore_discarded(
        &mut self,
        at: GitAt,
        entry: String,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.show_git(&at, window, cx) {
            return;
        }
        let job_entry = entry.clone();
        let done = move |ws: &mut Workspace,
                         at: GitAt,
                         result: std::io::Result<Restored>,
                         window: &mut Window,
                         cx: &mut Context<Workspace>| {
            match result {
                Ok(Restored::Done) => {}
                Ok(Restored::Changed(paths)) => {
                    let title = match paths.as_slice() {
                        [one] => format!("{one} changed since it was discarded"),
                        many => {
                            format!("{} files changed since they were discarded", many.len())
                        }
                    };
                    let (entity, asked_at) = (cx.entity(), at.clone());
                    prompt::confirm(
                        title,
                        "Restoring puts the discarded version back over the changes.",
                        "Restore",
                        move |window, cx| {
                            let (at, entry) = (asked_at.clone(), entry.clone());
                            entity.update(cx, |ws, cx| {
                                ws.restore_discarded(at, entry, true, window, cx)
                            })
                        },
                        window,
                        cx,
                    );
                }
                Err(e) => {
                    let message = format!("Couldn't restore: {e}");
                    eprintln!("templum: {message}");
                    show_notification(window, cx, Notification::error(message));
                }
            }
            ws.after_action(at, window, cx);
        };
        // At home the trash is home's: this computer's is never asked for a mirror, whatever
        // state its Git is in.
        if at.home.is_some() {
            let Some(Runs::Home(args)) = self.git_runs(&at) else {
                return;
            };
            let op = Op::Restore {
                entry: job_entry,
                force,
            };
            self.home_job(
                at,
                args,
                op,
                true,
                |heard| {
                    parsed::<Restored>(heard)
                        .map(shown)
                        .map_err(|e| std::io::Error::other(e.first_line().to_string()))
                },
                done,
                window,
                cx,
            );
            return;
        }
        let Some(trash) = self.trash.clone() else {
            return;
        };
        self.act_job(
            at,
            move || trash.restore(&job_entry, force),
            done,
            window,
            cx,
        );
    }

    /// Recently discarded…: this repository's discards still in the trash, read in the
    /// background, then listed.
    pub(crate) fn show_discarded(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let at = self.here();
        let show = |ws: &mut Workspace,
                    at: GitAt,
                    kept: Vec<Kept>,
                    window: &mut Window,
                    cx: &mut Context<Workspace>| {
            let (entity, dim) = (cx.entity(), ws.ui.title_dim);
            git_block::discarded::open(entity, at, kept, dim, window, cx);
        };
        // At home the trash is home's: its list, one line each, an empty one when it fails.
        if let Some(Runs::Home(args)) = self.git_runs(&at) {
            self.home_job(
                at,
                args,
                Op::Trash,
                false,
                |heard| {
                    let mut kept = parsed::<Vec<Kept>>(heard).unwrap_or_default();
                    kept.truncate(MOST_KEPT);
                    for k in &mut kept {
                        k.what = super::bus::clean_to(&k.what, 200);
                    }
                    kept
                },
                show,
                window,
                cx,
            );
            return;
        }
        let (Some(trash), Some(root)) = (
            self.trash.clone(),
            self.space
                .git()
                .and_then(|g| g.repo().map(|r| r.root.clone())),
        ) else {
            return;
        };
        self.git_job(at, move || ops::discarded(&trash, &root), show, window, cx);
    }
}

/// The most discards of home's trash that are listed.
const MOST_KEPT: usize = 100;

/// The most files a restore's question names (it counts the ones it names).
const MOST_CHANGED: usize = 1000;

/// What home's restore said, as the question draws it: its paths on a line each, and not
/// without end.
fn shown(restored: Restored) -> Restored {
    match restored {
        Restored::Changed(mut paths) => {
            paths.truncate(MOST_CHANGED);
            Restored::Changed(
                paths
                    .iter()
                    .map(|p| super::bus::clean_to(p, super::remote::LINE_CHARS))
                    .collect(),
            )
        }
        done => done,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restore_home_says_changed_names_its_files_on_a_line_each_and_not_endlessly() {
        let paths: Vec<String> = (0..MOST_CHANGED + 5)
            .map(|i| format!("a\u{1b}[31m\n{i}{}", "x".repeat(1000)))
            .collect();
        let Restored::Changed(shown) = shown(Restored::Changed(paths)) else {
            panic!("still changed");
        };
        assert_eq!(shown.len(), MOST_CHANGED);
        assert!(shown.iter().all(|p| {
            p.chars().count() <= super::super::remote::LINE_CHARS && !p.contains(['\n', '\u{1b}'])
        }));
        assert_eq!(super::shown(Restored::Done), Restored::Done);
    }

    #[test]
    fn thousands_are_grouped_by_three() {
        assert_eq!(super::thousands(500), "500");
        assert_eq!(super::thousands(1240), "1,240");
        assert_eq!(super::thousands(1_000_000), "1,000,000");
    }
}
