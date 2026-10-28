//! A Git block's conflicts: a region's part chosen, a whole file's version chosen, Mark
//! resolved. Each choice keeps a copy of the file in the trash first. See docs/specs/git.md,
//! section 5, Conflicts.

use gpui_kit::component::notification::Notification;
use gpui_kit::{Context, Window};
use templum_git::conflict::Take;
use templum_git::ops::{self, Failure, Op};

use crate::prompt;

use super::git_discard::REFUSED;
use super::git_home::parsed;
use super::{Workspace, notify_git_error, show_notification};

/// A refusal said: "The file changed; try again", why the copy failed, or git's error.
fn tell(at: super::GitAt, refused: Failure, window: &mut Window, cx: &mut Context<Workspace>) {
    let message = match refused {
        Failure::Changed => REFUSED.to_string(),
        Failure::Keep(e) => {
            format!("Couldn't keep a copy of the file, so nothing was changed: {e}")
        }
        Failure::Io(e) => format!("Couldn't write the file: {e}"),
        Failure::Refused(why) => why,
        Failure::Agent(f) => f.to_string(),
        Failure::Git(e) => {
            let entity = cx.entity();
            notify_git_error(&entity, at, &e, Vec::new(), window, cx);
            return;
        }
    };
    eprintln!("templum: {message}");
    #[cfg(test)]
    super::told(&message);
    show_notification(window, cx, Notification::error(message));
}

impl Workspace {
    /// A region's part (or both) replaces the region in the file, after a copy in the trash;
    /// refused when the region isn't the one shown (`shown`: its two parts' lines as drawn).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn resolve_region(
        &mut self,
        path: String,
        region: usize,
        take: Take,
        shown: (Vec<String>, Vec<String>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep = self.trash.clone();
        let op = Op::ResolveRegion {
            path: path.clone(),
            region,
            take,
            shown: shown.clone(),
        };
        self.act_op(
            self.here(),
            op,
            move |repo| ops::resolve_region(repo, keep.as_ref(), &path, region, take, &shown),
            |heard| parsed::<()>(heard).map_err(Failure::Git),
            |ws, at, done, window, cx| {
                if let Err(refused) = done {
                    tell(at.clone(), refused, window, cx);
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// A whole file's version (`git checkout --ours` / `--theirs`), after a copy in the trash.
    /// A copy that fails stops it: the file may hold hand edits git keeps nowhere.
    pub(crate) fn resolve_file(
        &mut self,
        path: String,
        side: templum_git::Side,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let keep = self.trash.clone();
        let op = Op::Resolve {
            path: path.clone(),
            side,
        };
        self.act_op(
            self.here(),
            op,
            move |repo| ops::resolve_file(repo, keep.as_ref(), &path, side),
            |heard| parsed::<()>(heard).map_err(Failure::Git),
            |ws, at, done, window, cx| {
                if let Err(refused) = done {
                    tell(at.clone(), refused, window, cx);
                }
                ws.after_action(at, window, cx);
            },
            window,
            cx,
        );
    }

    /// Mark resolved: stages the file; with conflict markers still in it, asks first.
    pub(crate) fn mark_resolved(
        &mut self,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(git) = self.space.git() else {
            return;
        };
        if !git.markers_left(&path) {
            self.stage(vec![path], window, cx);
            return;
        }
        let (entity, at) = (cx.entity(), self.here());
        prompt::confirm(
            format!("Mark {path} resolved?"),
            "It still has conflict markers: Git will record them as they are.",
            "Mark resolved",
            move |window, cx| {
                let (path, at) = (path.clone(), at.clone());
                entity.update(cx, |ws, cx| {
                    if ws.show_git(&at, window, cx) {
                        ws.stage(vec![path], window, cx)
                    }
                })
            },
            window,
            cx,
        );
    }
}
