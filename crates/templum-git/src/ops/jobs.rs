//! The job bodies: what the popup runs in the background, moved here so the API runs the same.

use std::path::Path;
use std::time::SystemTime;

use crate::conflict::{Region, Take, regions, resolve};
use crate::parse::Change;
use crate::repo::Repo;
use crate::trash::{self, Kept, Trash};
use crate::{Commit, Entry, GitError, HistoryQuery, Side, Status, Synced};

use super::{CommitShown, Committed, Compared, Discarding, Failure, Look, PAGE, Page, Undone};

/// The status, the operation under way and the author, read after every action and every poll.
pub fn look(repo: &Repo) -> Result<Look, GitError> {
    Ok(Look {
        status: repo.status()?,
        operation: repo.operation(),
        author: repo.author()?,
    })
}

/// The message a commit of the one file `entry` gets when none is typed: "Add a.rs", "Delete
/// a.rs", "Rename a.rs to b.rs" ("Move a.rs to dir/" when only its folder changed), else
/// "Update a.rs", by the change in the index (`staged`) or in the working tree. None when the entry
/// has no such change.
pub fn generic_summary(entry: &Entry, staged: bool) -> Option<String> {
    let change = if staged { entry.staged } else { entry.unstaged }?;
    let name = entry.name();
    Some(match (change, &entry.from) {
        (Change::Added | Change::Untracked, _) => format!("Add {name}"),
        (Change::Deleted, _) => format!("Delete {name}"),
        (Change::Renamed, Some(from)) => {
            let old = from.rsplit('/').next().unwrap_or(from);
            match (old == name, entry.folder()) {
                (false, _) => format!("Rename {old} to {name}"),
                (true, "") => format!("Move {name} to the root"),
                (true, dir) => format!("Move {name} to {dir}/"),
            }
        }
        _ => format!("Update {name}"),
    })
}

/// `generic_summary` of the one file the index holds in `status`: None with no file or several.
pub fn staged_summary(status: &Status) -> Option<String> {
    match status.staged().as_slice() {
        [entry] => generic_summary(entry, true),
        _ => None,
    }
}

/// The summary, then a blank line and the description when there is one.
pub fn join_message(summary: &str, description: &str) -> String {
    if description.is_empty() {
        summary.to_string()
    } else {
        format!("{summary}\n\n{description}")
    }
}

/// Commits what is staged, or with `all` every change. An empty `summary` is the sentence for
/// the one file staged now (by what is staged now, not by a status read earlier); None when
/// there isn't one, and the commit waits for a message.
pub fn commit(
    repo: &Repo,
    summary: &str,
    description: &str,
    amend: bool,
    all: bool,
) -> Result<Option<Committed>, GitError> {
    if all {
        repo.stage_all()?;
    }
    let summary = if summary.is_empty() {
        match staged_summary(&repo.status()?) {
            Some(summary) => summary,
            None => return Ok(None),
        }
    } else {
        summary.to_string()
    };
    // An Amend's Undo goes back to the commit it replaces: read it before.
    let replaced = if amend { Some(repo.head()?) } else { None };
    let message = join_message(&summary, description);
    Ok(Some(Committed {
        short: repo.commit(&message, amend)?,
        replaced,
    }))
}

/// Takes back the commit `short`, if it is still the last and not pushed.
pub fn undo_commit(repo: &Repo, short: &str, replaced: Option<&str>) -> Result<Undone, GitError> {
    // A commit made since: it is never the one taken back, pushed or not.
    let Some(head) = repo.head().ok().filter(|h| h.starts_with(short)) else {
        return Ok(Undone::Moved);
    };
    // The full hash: a short one can be a tag's or a branch's name, which git reads first.
    if repo.on_upstream(&head)? {
        return Ok(Undone::Pushed);
    }
    Ok(repo
        .undo_commit(short, replaced)?
        .map_or(Undone::Moved, Undone::Done))
}

/// Sync: with `join` None the plain one, else the join of diverged branches (`Some(rebase)`).
pub fn sync(repo: &Repo, join: Option<bool>) -> Result<Synced, GitError> {
    match join {
        None => repo.sync(),
        Some(rebase) => repo.sync_join(rebase),
    }
}

/// Throws away the unstaged changes of `paths` (or one hunk): the copy first, when `trash` is
/// given, then git, then the copy's seal. A copy that can't be made stops it before anything
/// is thrown away, and a git that fails takes the copy back out.
pub fn discard(
    repo: &Repo,
    trash: Option<&Trash>,
    paths: &[String],
    what: &str,
    discarding: &Discarding,
) -> Result<Option<Kept>, Failure> {
    let kept = match trash {
        Some(t) => Some(
            t.put(&repo.root, paths, what, SystemTime::now())
                .map_err(Failure::Keep)?,
        ),
        None => None,
    };
    let discarded = match discarding {
        Discarding::Files(entries) => repo.discard(&entries.iter().collect::<Vec<_>>()),
        Discarding::Hunk(patch) => repo.apply_patch(patch, false, true),
    };
    if let Err(e) = discarded {
        if let (Some(t), Some(k)) = (trash, &kept) {
            let _ = t.remove(&k.id);
        }
        return Err(Failure::Git(e));
    }
    seal(trash, kept.as_ref());
    Ok(kept)
}

/// This repository's discards still in the trash.
pub fn discarded(trash: &Trash, root: &Path) -> Vec<Kept> {
    trash
        .list()
        .into_iter()
        .filter(|k| k.root == root)
        .collect()
}

/// Writes `path` as it was in commit `hash` (named `then` there), after a copy of the file as
/// it is now in the trash, when `trash` is given and the file fits.
pub fn restore_version(
    repo: &Repo,
    trash: Option<&Trash>,
    hash: &str,
    then: &str,
    now: &str,
    what: &str,
) -> Result<Option<Kept>, Failure> {
    let bytes = repo.file_at(hash, then)?;
    let kept = keep_copy(repo, trash, now, what)?;
    let full = repo.root.join(now);
    let written = match full.parent() {
        Some(dir) => std::fs::create_dir_all(dir),
        None => Ok(()),
    }
    .and_then(|_| std::fs::write(&full, bytes));
    if let Err(e) = written {
        // Nothing was restored: an entry left would list a change that never was.
        unkeep(trash, kept.as_ref());
        return Err(Failure::Io(e));
    }
    seal(trash, kept.as_ref());
    Ok(kept)
}

/// A whole file's version (`git checkout --ours` / `--theirs`), after a copy in the trash. A
/// copy that fails stops it: the file may hold hand edits git keeps nowhere.
pub fn resolve_file(
    repo: &Repo,
    trash: Option<&Trash>,
    path: &str,
    side: Side,
) -> Result<(), Failure> {
    let kept = keep_copy(repo, trash, path, &format!("Chose a version of {path}"))?;
    if let Err(e) = repo.resolve(path, side) {
        unkeep(trash, kept.as_ref());
        return Err(Failure::Git(e));
    }
    seal(trash, kept.as_ref());
    Ok(())
}

/// A region's part (or both) replaces the region in the file, after a copy in the trash;
/// refused when the region isn't the one shown (`shown`: its two parts' lines as drawn).
pub fn resolve_region(
    repo: &Repo,
    trash: Option<&Trash>,
    path: &str,
    region: usize,
    take: Take,
    shown: &(Vec<String>, Vec<String>),
) -> Result<(), Failure> {
    let full = repo.root.join(path);
    // The diff shows no conflicted file past DIFF_LIMIT: no region of one is chosen.
    let meta = std::fs::symlink_metadata(&full).map_err(Failure::Io)?;
    if !meta.is_file() {
        return Err(Failure::Refused(format!(
            "{path} isn't a regular file: take one side of it instead."
        )));
    }
    if meta.len() > crate::DIFF_LIMIT as u64 {
        return Err(Failure::Refused(format!(
            "{path} is too large to resolve by region: take one side of it instead."
        )));
    }
    let bytes = std::fs::read(&full).map_err(Failure::Io)?;
    let text = String::from_utf8(bytes).map_err(|_| Failure::Changed)?;
    let r = regions(&text)
        .into_iter()
        .nth(region)
        .ok_or(Failure::Changed)?;
    if (
        Region::lines(&text, &r.first),
        Region::lines(&text, &r.second),
    ) != *shown
    {
        return Err(Failure::Changed);
    }
    let resolved = resolve(&text, region, take).ok_or(Failure::Changed)?;
    let kept = keep_copy(repo, trash, path, &format!("Chose a part of {path}"))?;
    if let Err(e) = std::fs::write(&full, resolved) {
        unkeep(trash, kept.as_ref());
        return Err(Failure::Io(e));
    }
    seal(trash, kept.as_ref());
    Ok(())
}

/// The copy of `path` a change keeps in the trash: None without a trash, or when the file is
/// too much to keep.
fn keep_copy(
    repo: &Repo,
    trash: Option<&Trash>,
    path: &str,
    what: &str,
) -> Result<Option<Kept>, Failure> {
    let paths = [path.to_string()];
    let (files, size) = trash::measure(&repo.root, &paths);
    match trash.filter(|_| trash::fits(files, size)) {
        Some(t) => Ok(Some(
            t.put(&repo.root, &paths, what, SystemTime::now())
                .map_err(Failure::Keep)?,
        )),
        None => Ok(None),
    }
}

/// A change failed: its copy goes back out of the trash.
fn unkeep(trash: Option<&Trash>, kept: Option<&Kept>) {
    if let (Some(t), Some(k)) = (trash, kept) {
        let _ = t.remove(&k.id);
    }
}

/// A change is done: its copy is sealed, and old ones go.
fn seal(trash: Option<&Trash>, kept: Option<&Kept>) {
    if let (Some(t), Some(k)) = (trash, kept) {
        let _ = t.seal(k);
        let _ = t.prune(SystemTime::now());
    }
}

/// The commits `branch` has that `base` lacks, and its diff against `base` (or `narrowed`
/// commit's alone).
pub fn compare(
    repo: &Repo,
    base: &str,
    branch: &str,
    narrowed: Option<&str>,
    ignore_ws: bool,
) -> Result<Compared, GitError> {
    let commits = repo.compare_log(base, branch)?;
    let files = match narrowed {
        Some(hash) => repo.commit_changes(hash, ignore_ws)?,
        None => repo.range_diff(base, branch, ignore_ws)?,
    };
    Ok(Compared { commits, files })
}

/// A page of History after `skip` commits (a file's, through its renames, when `file` is
/// given); the first also reads which commits aren't pushed.
pub fn history(
    repo: &Repo,
    q: &HistoryQuery,
    file: Option<&str>,
    skip: usize,
) -> Result<Page, GitError> {
    let (commits, paths) = match file {
        Some(path) => repo.file_log(path, skip, PAGE)?.into_iter().unzip(),
        None => (repo.history(q, skip, PAGE)?, Vec::new()),
    };
    let unpushed = if skip == 0 {
        Some(repo.unpushed()?)
    } else {
        None
    };
    Ok(Page {
        commits,
        paths,
        unpushed,
    })
}

/// A commit, its files and its whole diff; `loaded` saves reading the commit again.
pub fn show(
    repo: &Repo,
    hash: &str,
    loaded: Option<Commit>,
    ignore_ws: bool,
) -> Result<CommitShown, GitError> {
    let commit = match loaded {
        Some(c) => c,
        None => repo.commit_info(hash)?,
    };
    Ok(CommitShown {
        commit,
        files: repo.commit_files(hash)?,
        diffs: repo.commit_changes(hash, ignore_ws)?,
    })
}
