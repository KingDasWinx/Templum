//! [`run`]: an operation of the API's `git` tool, its arguments checked, then the same function
//! the popup calls; and [`fit`], which keeps an answer within what the client takes.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use crate::repo::Repo;
use crate::trash::{self, Restored};
use crate::{HistoryQuery, Side, Status, Synced, message};

use super::check::{self, Which, staged_change, stash_of, unstaged_change};
use super::{
    Answer, CHANGED_MEANWHILE, Discarding, Done, Failure, Found, Home, Measured, Message, Op,
    Undone, thousands,
};

/// The most a `git` answer's JSON weighs: 8 MiB less 64 KiB for the text, the envelope and
/// `details` around it (the client takes 8 MiB).
pub const ANSWER_MOST: usize = 8_323_072;

const NO_OPERATION: &str = "No merge, rebase, cherry-pick or revert is in progress.";

fn count(n: usize, noun: &str) -> String {
    let es = ["s", "sh", "ch"].iter().any(|end| noun.ends_with(end));
    match (n, es) {
        (1, _) => format!("1 {noun}"),
        (_, true) => format!("{n} {noun}es"),
        _ => format!("{n} {noun}s"),
    }
}

fn path_of(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

fn short(hash: &str) -> &str {
    &hash[..hash.len().min(7)]
}

fn say(text: impl Into<String>, git: Done) -> Result<Answer, Failure> {
    Ok(Answer {
        text: text.into(),
        git,
    })
}

fn refused<T>(why: impl Into<String>) -> Result<T, Failure> {
    Err(Failure::Refused(why.into()))
}

/// The operation in progress, or the refusal that none is.
fn under_way(repo: &Repo) -> Result<crate::Operation, Failure> {
    repo.operation()
        .ok_or_else(|| Failure::Refused(NO_OPERATION.to_string()))
}

/// Every path named is a change the status lists on the side `wanted` picks.
fn each_listed(
    status: &Status,
    paths: &[String],
    from: bool,
    wanted: fn(&crate::Entry) -> bool,
    refusal: &str,
) -> Result<(), Failure> {
    paths
        .iter()
        .try_for_each(|p| check::listed(status, p, from, wanted, refusal).map(drop))
}

fn each_inside(repo: &Repo, paths: &[String]) -> Result<(), Failure> {
    paths.iter().try_for_each(|p| check::inside(&repo.root, p))
}

/// Runs `op` in `repo`: its shape checked, then what the repository says (names listed, paths
/// inside, files the status has), then the function the Git popup calls for the same job. Every
/// string a caller wrote passed a check before git or the filesystem saw it.
pub fn run(repo: &Repo, op: Op, home: &Home) -> Result<Answer, Failure> {
    op.check().map_err(Failure::Refused)?;
    let mut answer = act(repo, op, home)?;
    fit(&mut answer.git, ANSWER_MOST)?;
    Ok(answer)
}

fn act(repo: &Repo, op: Op, home: &Home) -> Result<Answer, Failure> {
    match op {
        Op::Find => {
            let root = path_of(&repo.root);
            let main_tree = repo.main_tree()?.map(|p| path_of(&p));
            say(
                format!("The repository is at {root}."),
                Done::Found(Found {
                    root: Some(root),
                    main_tree,
                }),
            )
        }
        Op::Look => {
            let look = super::look(repo)?;
            let n = count(look.status.entries.len(), "changed file");
            let text = match &look.status.branch {
                Some(branch) => format!("On {branch}: {n}."),
                None => format!("Detached: {n}."),
            };
            say(text, Done::Look(look))
        }
        Op::Diff {
            ignore_whitespace,
            whole,
        } => {
            let mut status = repo.status()?;
            // Templum reads an untracked or conflicted file itself, and a link would lead it out
            // of the repository: such a file isn't in the diff.
            status.entries.retain(|e| {
                let readable = check::inside(&repo.root, &e.path).is_ok()
                    && check::not_link(&repo.root, &e.path).is_ok();
                readable || !(e.conflict() || e.unstaged == Some(crate::Change::Untracked))
            });
            let whole: Vec<(bool, String)> =
                whole.into_iter().map(|w| (w.staged, w.path)).collect();
            let diff = repo.working_diff(&status, ignore_whitespace, &whole)?;
            let n = count(diff.unstaged.len() + diff.staged.len(), "file");
            say(format!("{n} changed."), Done::Diff(diff))
        }
        Op::History {
            all,
            search,
            path,
            skip,
        } => {
            if let Some(p) = &path {
                check::inside(&repo.root, p)?;
            }
            let q = HistoryQuery {
                all,
                search: search.map(|s| (s.by, s.text)),
            };
            let page = super::history(repo, &q, path.as_deref(), skip)?;
            say(
                count(page.commits.len(), "commit") + ".",
                Done::History(page),
            )
        }
        Op::Show {
            hash,
            ignore_whitespace,
        } => {
            let shown = super::show(repo, &hash, None, ignore_whitespace)?;
            let text = format!(
                "Commit {}: {}.",
                shown.commit.short,
                count(shown.files.len(), "file")
            );
            say(text, Done::Show(shown))
        }
        Op::Blame { path, hash } => {
            check::inside(&repo.root, &path)?;
            let lines = repo.blame(&path, hash.as_deref())?;
            say(count(lines.len(), "line") + ".", Done::Blame(lines))
        }
        Op::Branches => {
            let branches = repo.branches()?;
            say(
                count(branches.len(), "branch") + ".",
                Done::Branches(branches),
            )
        }
        Op::Unmerged { branch } => {
            check::branch(repo, &branch, Which::Local)?;
            let n = repo.unmerged_count(&branch)?;
            say(
                format!("{} only on {branch}.", count(n as usize, "commit")),
                Done::Count(n),
            )
        }
        Op::DefaultBranch => {
            let name = repo.default_branch();
            let text = match &name {
                Some(b) => format!("The default branch is {b}."),
                None => "No default branch.".to_string(),
            };
            say(text, Done::Name(name))
        }
        Op::Compare {
            base,
            branch,
            hash,
            ignore_whitespace,
        } => {
            check::branch(repo, &base, Which::Any)?;
            check::branch(repo, &branch, Which::Any)?;
            let compared =
                super::compare(repo, &base, &branch, hash.as_deref(), ignore_whitespace)?;
            say(
                count(compared.commits.len(), "commit") + ".",
                Done::Compare(compared),
            )
        }
        Op::Stashes => {
            let stashes = repo.stashes()?;
            say(count(stashes.len(), "stash") + ".", Done::Stashes(stashes))
        }
        Op::StashDiff {
            stash,
            ignore_whitespace,
        } => {
            let files = repo.stash_diff(&stash_of(&stash), ignore_whitespace)?;
            say(count(files.len(), "file") + ".", Done::Files(files))
        }
        Op::Remotes => {
            let remotes = repo.remotes()?;
            say(count(remotes.len(), "remote") + ".", Done::Names(remotes))
        }
        Op::LastMessage => say(
            "The last commit's message.",
            Done::Text(repo.last_message()?),
        ),
        Op::Measure { paths } => {
            each_inside(repo, &paths)?;
            let (files, bytes) = trash::measure(&repo.root, &paths);
            let keep = home.trash.is_some() && trash::fits(files, bytes);
            say(
                format!("{}, {bytes} bytes.", count(files, "file")),
                Done::Measured(Measured { files, bytes, keep }),
            )
        }
        Op::Trash => {
            let kept = home
                .trash
                .map(|t| super::discarded(t, &repo.root))
                .unwrap_or_default();
            say(count(kept.len(), "discard") + ".", Done::Trash(kept))
        }
        Op::Init => refused(format!(
            "There's already a repository at {}.",
            repo.root.display()
        )),
        Op::SetAuthor { name, email } => {
            repo.set_author(&name, &email, home.global_author)?;
            say(
                format!("Set the author to {name} <{email}>."),
                Done::Nothing,
            )
        }
        Op::Stage { paths } => {
            each_inside(repo, &paths)?;
            each_listed(
                &repo.status()?,
                &paths,
                false,
                unstaged_change,
                "has no changes to stage.",
            )?;
            let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
            repo.stage(&refs)?;
            say(
                format!("Staged {}.", count(paths.len(), "file")),
                Done::Nothing,
            )
        }
        Op::Unstage { paths } => {
            each_inside(repo, &paths)?;
            each_listed(
                &repo.status()?,
                &paths,
                true,
                staged_change,
                "has nothing staged.",
            )?;
            let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
            repo.unstage(&refs)?;
            say(
                format!("Unstaged {}.", count(paths.len(), "file")),
                Done::Nothing,
            )
        }
        Op::StageHunk { patch } => {
            let path = hunk_path(repo, &patch)?;
            check::listed(
                &repo.status()?,
                &path,
                false,
                unstaged_change,
                "has no changes to stage.",
            )?;
            repo.apply_patch(&patch, true, false)?;
            say(format!("Staged a change to {path}."), Done::Nothing)
        }
        Op::UnstageHunk { patch } => {
            let path = hunk_path(repo, &patch)?;
            check::listed(
                &repo.status()?,
                &path,
                false,
                staged_change,
                "has nothing staged.",
            )?;
            repo.apply_patch(&patch, true, true)?;
            say(format!("Unstaged a change to {path}."), Done::Nothing)
        }
        Op::Commit {
            summary,
            description,
            amend,
            all,
        } => {
            let committed = super::commit(repo, summary.trim(), description.trim(), amend, all)?;
            let Some(committed) = committed else {
                return refused(CHANGED_MEANWHILE);
            };
            say(
                format!("Committed {}.", committed.short),
                Done::Committed(committed),
            )
        }
        Op::UndoCommit { hash, replaced } => {
            let undone = super::undo_commit(repo, &hash, replaced.as_deref())?;
            let text = match &undone {
                Undone::Done(_) => format!("Took back {}.", short(&hash)),
                Undone::Pushed => format!("{} is pushed: revert it instead.", short(&hash)),
                Undone::Moved => format!(
                    "A commit was made since {}: only the last commit is taken back.",
                    short(&hash)
                ),
            };
            say(text, Done::Undone(undone))
        }
        Op::Fetch => {
            repo.fetch()?;
            say("Fetched.", Done::Nothing)
        }
        Op::Pull { how } => {
            repo.pull(how)?;
            say("Pulled.", Done::Nothing)
        }
        Op::Push => {
            repo.push()?;
            say("Pushed.", Done::Nothing)
        }
        Op::PushBranch { branch } => {
            check::branch(repo, &branch, Which::Local)?;
            repo.push_branch(&branch)?;
            say(format!("Pushed {branch}."), Done::Nothing)
        }
        Op::Publish { remote, branch } => {
            check::remote(repo, &remote)?;
            check::branch(repo, &branch, Which::Local)?;
            repo.publish(&remote, &branch)?;
            say(format!("Published {branch} to {remote}."), Done::Nothing)
        }
        Op::Sync { join } => {
            let synced = super::sync(repo, join)?;
            let text = match synced {
                Synced::Done { got, sent } => format!("Synced: got {got}, sent {sent}."),
                Synced::Diverged { ahead, behind } => {
                    format!("Both sides have new commits: {ahead} here, {behind} there.")
                }
                Synced::Gone | Synced::Unpublished => {
                    let branch = repo
                        .status()?
                        .branch
                        .unwrap_or_else(|| "This branch".into());
                    format!("{branch} has no upstream: publish it.")
                }
            };
            say(text, Done::Synced(synced))
        }
        Op::Switch {
            branch,
            remote,
            leave,
        } => {
            check::branch(
                repo,
                &branch,
                if remote { Which::Remote } else { Which::Local },
            )?;
            let back = repo.switch(&branch, remote, leave.as_deref())?;
            say(format!("Switched to {branch}."), Done::Switched(back))
        }
        Op::NewBranch { name, hash } => {
            repo.new_branch(&name, hash.as_deref())?;
            say(format!("Made {name} and switched to it."), Done::Nothing)
        }
        Op::RenameBranch { branch, name } => {
            check::branch(repo, &branch, Which::Local)?;
            repo.rename_branch(&branch, &name)?;
            say(format!("Renamed {branch} to {name}."), Done::Nothing)
        }
        Op::Merge { branch } => {
            check::branch(repo, &branch, Which::Any)?;
            repo.merge(&branch)?;
            say(format!("Merged {branch}."), Done::Nothing)
        }
        Op::Checkout { hash } => {
            repo.checkout_commit(&hash)?;
            say(format!("Detached at {}.", short(&hash)), Done::Nothing)
        }
        Op::Tag { name, hash } => {
            repo.tag(&name, &hash)?;
            say(format!("Tagged {} {name}.", short(&hash)), Done::Nothing)
        }
        Op::Revert { hash } => {
            repo.revert(&hash)?;
            say(format!("Reverted {}.", short(&hash)), Done::Nothing)
        }
        Op::Stash { message, untracked } => {
            repo.stash_push(&message, untracked)?;
            say("Stashed.", Done::Nothing)
        }
        Op::StashApply { stash } => {
            repo.stash_apply(&stash_of(&stash))?;
            say(format!("Applied stash@{{{}}}.", stash.index), Done::Nothing)
        }
        Op::StashPop { stash } => {
            repo.stash_pop(&stash_of(&stash))?;
            say(format!("Popped stash@{{{}}}.", stash.index), Done::Nothing)
        }
        Op::StashDrop { stash } => {
            repo.stash_drop(&stash_of(&stash))?;
            say(format!("Dropped stash@{{{}}}.", stash.index), Done::Nothing)
        }
        Op::Continue => {
            let operation = under_way(repo)?;
            repo.continue_(operation)?;
            say(
                format!("Continued the {}.", operation.name()),
                Done::Nothing,
            )
        }
        Op::Abort => {
            let operation = under_way(repo)?;
            repo.abort(operation)?;
            say(format!("Aborted the {}.", operation.name()), Done::Nothing)
        }
        Op::Resolve { path, side } => {
            check::inside(&repo.root, &path)?;
            check::conflicted(&repo.status()?, &path)?;
            super::resolve_file(repo, home.trash, &path, side)?;
            let side = if side == Side::Ours { "ours" } else { "theirs" };
            say(format!("Took {side} for {path}."), Done::Nothing)
        }
        Op::ResolveRegion {
            path,
            region,
            take,
            shown,
        } => {
            check::inside(&repo.root, &path)?;
            check::conflicted(&repo.status()?, &path)?;
            check::not_link(&repo.root, &path)?;
            super::resolve_region(repo, home.trash, &path, region, take, &shown)?;
            say(format!("Chose a part of {path}."), Done::Nothing)
        }
        Op::Restore { entry, force } => {
            let Some(trash) = home.trash else {
                return refused("This Templum keeps no trash.");
            };
            let Some(kept) = super::discarded(trash, &repo.root)
                .into_iter()
                .find(|k| k.id == entry)
            else {
                return refused(format!(
                    "There's no discard {entry} of this repository: trash lists them."
                ));
            };
            // Restoring writes each file where the discard found it: not through a link out.
            for (file, _) in &kept.files {
                check::inside(&repo.root, file)?;
            }
            // A forced restore's copy of what it writes over: refused before any write.
            let restored = trash.restore(&entry, force).map_err(|e| {
                if trash::kept_nothing(&e) {
                    Failure::Keep(e)
                } else {
                    Failure::Io(e)
                }
            })?;
            let text = match &restored {
                Restored::Done => "Restored.".to_string(),
                Restored::Changed(files) => format!(
                    "{} changed since the discard: restore with force to write over them.",
                    count(files.len(), "file")
                ),
            };
            say(text, Done::Restored(restored))
        }
        Op::Ignore { path } => {
            check::inside(&repo.root, &path)?;
            // The pattern goes into the root's .gitignore, written as a file of the repository.
            check::not_link(&repo.root, ".gitignore")?;
            repo.ignore(&path).map_err(Failure::Io)?;
            say(format!("Ignored {path}."), Done::Nothing)
        }
        Op::Message { all } => {
            let Some(writer) = home.writer else {
                return refused("No commit-message agent is ready.");
            };
            let (answer, committed) = message::generate(repo, writer, all, &AtomicBool::new(false));
            let answer = answer.map_err(Failure::Agent)?;
            let Some((summary, description)) = message::split(&answer) else {
                return Err(Failure::Agent(message::Failure::Empty {
                    stderr: String::new(),
                }));
            };
            say(
                "Wrote a message.",
                Done::Message(Message {
                    summary,
                    description,
                    committed,
                }),
            )
        }
        Op::Discard { paths } => {
            each_inside(repo, &paths)?;
            let status = repo.status()?;
            each_listed(
                &status,
                &paths,
                false,
                discardable,
                "has no changes to discard.",
            )?;
            let entries: Vec<crate::Entry> = status
                .unstaged()
                .into_iter()
                .filter(|e| paths.contains(&e.path))
                .cloned()
                .collect();
            let paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
            let what = match entries.as_slice() {
                [one] => format!("Discarded {}", one.path),
                many => format!("Discarded {} files", thousands(many.len())),
            };
            let kept = discard_kept(repo, home, &paths, &what, &Discarding::Files(entries))?;
            say(what + ".", Done::Kept(kept))
        }
        Op::DiscardHunk { path, patch } => {
            check::inside(&repo.root, &path)?;
            check::not_link(&repo.root, &path)?;
            check::listed(
                &repo.status()?,
                &path,
                false,
                discardable,
                "has no changes to discard.",
            )?;
            let what = format!("Discarded a change to {path}");
            let kept = discard_kept(repo, home, &[path], &what, &Discarding::Hunk(patch))?;
            say(what + ".", Done::Kept(kept))
        }
        Op::RestoreVersion { hash, then, path } => {
            check::inside(&repo.root, &path)?;
            check::not_link(&repo.root, &path)?;
            let what = format!("Restored {path} from {}", short(&hash));
            let kept = super::restore_version(repo, home.trash, &hash, &then, &path, &what)?;
            say(what + ".", Done::Kept(kept))
        }
        Op::DeleteBranch { branch, force } => {
            check::branch(repo, &branch, Which::Local)?;
            repo.delete_branch(&branch, force)?;
            say(format!("Deleted {branch}."), Done::Nothing)
        }
    }
}

fn discardable(e: &crate::Entry) -> bool {
    e.unstaged.is_some() && !e.conflict()
}

/// The file a hunk's patch changes, which passed its shape check, and is inside the repository.
fn hunk_path(repo: &Repo, patch: &str) -> Result<String, Failure> {
    let file = check::patch_file(patch).map_err(Failure::Refused)?;
    let path = file.path().to_string();
    check::inside(&repo.root, &path)?;
    Ok(path)
}

/// A discard that keeps a copy in the trash when there is one and the paths fit in it.
fn discard_kept(
    repo: &Repo,
    home: &Home,
    paths: &[String],
    what: &str,
    discarding: &Discarding,
) -> Result<Option<crate::trash::Kept>, Failure> {
    let (files, bytes) = trash::measure(&repo.root, paths);
    let trash = home.trash.filter(|_| trash::fits(files, bytes));
    super::discard(repo, trash, paths, what, discarding)
}

/// Cuts `done` to within `most` bytes of JSON: the file with the heaviest hunks loses them
/// first (the file's row stays, marked `cut`), then the next, until it fits. An answer with no
/// hunks left to drop, or none to begin with, is refused.
pub fn fit(done: &mut Done, most: usize) -> Result<(), Failure> {
    let mut size = serde_json::to_vec(&*done).map_or(usize::MAX, |v| v.len());
    // Each cut takes the file's hunks out of the JSON (down to `[]`) and flips `cut` to `true`
    // (one byte shorter): the size is known without writing the answer again.
    if size <= most {
        return Ok(());
    }
    // Weighed once each, then cut heaviest first: linear in the answer, not quadratic.
    let mut files: Vec<(usize, &mut crate::FileDiff)> = files_of(done)
        .into_iter()
        .filter(|f| !f.hunks.is_empty())
        .map(|f| (serde_json::to_vec(&f.hunks).map_or(0, |v| v.len()), f))
        .collect();
    files.sort_by_key(|(weight, _)| std::cmp::Reverse(*weight));
    for (weight, file) in files {
        file.hunks.clear();
        file.cut = true;
        size -= weight - 2 + 1;
        if size <= most {
            return Ok(());
        }
    }
    refused(format!(
        "The answer is too large to send: {:.1} MB, past 8 MB.",
        size as f64 / 1_000_000.0
    ))
}

/// The file diffs an answer holds, which `fit` may cut.
fn files_of(done: &mut Done) -> Vec<&mut crate::FileDiff> {
    match done {
        Done::Diff(d) => d.unstaged.iter_mut().chain(d.staged.iter_mut()).collect(),
        Done::Show(s) => s.diffs.iter_mut().collect(),
        Done::Compare(c) => c.files.iter_mut().collect(),
        Done::Files(f) => f.iter_mut().collect(),
        _ => Vec::new(),
    }
}
