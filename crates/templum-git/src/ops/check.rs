//! What a caller's strings must be before git or the filesystem sees them. Shapes first
//! ([`Op::check`], no git, no filesystem): the registry runs them, so a malformed argument is
//! refused before the UI thread. Then the state ([`run`](super::run) asks the repository):
//! names that are listed, paths that stay inside, files the status has.

use std::path::Path;

use crate::repo::Repo;
use crate::{DIFF_LIMIT, Entry, FileDiff, Stash, Status, hunk_patch, parse_diff};

use super::{Failure, Op, SearchArg, StashRef, thousands};

const LINE_MOST: usize = 4096;
const SEARCH_MOST: usize = 1000;
const DESCRIPTION_MOST: usize = 65_536;
const PATH_MOST: usize = 4096;
const PATHS_MOST: usize = 10_000;
const WHOLE_MOST: usize = 1000;
const SKIP_MOST: usize = 1_000_000;
const REGION_MOST: usize = 10_000;
const STASH_MOST: usize = 10_000;
const NAME_MOST: usize = 255;
const ENTRY_MOST: usize = 100;

fn invalid(what: &str) -> String {
    format!(
        "\"{what}\" isn't a path inside the repository: give it relative to its root, without \"..\" or .git."
    )
}

/// A path relative to the root, as git would take it as a pathspec and Templum as a file:
/// no way out of the repository, no `.git`, no pathspec magic, no drive.
pub(super) fn path(p: &str) -> Result<(), String> {
    let bad = || Err(invalid(p));
    if p.is_empty() || p.len() > PATH_MOST || p.chars().any(|c| c.is_control()) {
        return bad();
    }
    let mut chars = p.chars();
    let first = chars.next().unwrap_or(' ');
    if matches!(first, '/' | '\\' | ':')
        || (first.is_ascii_alphabetic() && chars.next() == Some(':'))
    {
        return bad();
    }
    for part in p.split(['/', '\\']) {
        // Windows has no `:` in a name, and reads one as a stream of the file before it.
        if part.is_empty()
            || part == "."
            || part == ".."
            || is_dotgit(part)
            || (cfg!(windows) && part.contains(':'))
        {
            return bad();
        }
    }
    Ok(())
}

/// What HFS+ skips in a name (git's `is_hfs_dotgit` does too): `.g\u{200c}it` is `.git` there.
fn hfs_ignored(c: char) -> bool {
    matches!(c, '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}')
}

/// A part some file system reads as `.git` (git's `is_hfs_dotgit` and `is_ntfs_dotgit`): in any
/// case, with HFS+'s ignored characters, with trailing dots and spaces or a stream after it
/// (`.git::$INDEX_ALLOCATION`), or as NTFS's short name `git~1`.
fn is_dotgit(part: &str) -> bool {
    let plain: String = part.chars().filter(|c| !hfs_ignored(*c)).collect();
    let name = plain.split(':').next().unwrap_or_default();
    let name = name.trim_end_matches(['.', ' ']).to_ascii_lowercase();
    name == ".git" || name == "git~1"
}

fn paths(list: &[String]) -> Result<(), String> {
    if list.is_empty() || list.len() > PATHS_MOST {
        return Err("paths takes 1 to 10,000 paths.".to_string());
    }
    list.iter().try_for_each(|p| path(p))
}

/// A commit's hash, whole or short.
pub(super) fn hash(h: &str) -> Result<(), String> {
    if (4..=64).contains(&h.len()) && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("\"{h}\" isn't a commit's hash."))
    }
}

fn flat(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MOST
        && !name.starts_with('-')
        && !name.chars().any(|c| c.is_control())
}

/// The name of a branch, tag or remote that exists (listed later).
fn existing(noun: &str, name: &str) -> Result<(), String> {
    if flat(name) {
        Ok(())
    } else {
        Err(format!("\"{name}\" can't name a {noun}."))
    }
}

/// A name to make: a branch or a tag. Git would take one starting with `+` as a forced refspec
/// and one with `:`, `^`, `~`, `..` or `@{` as a revision, and a hash's digits name a ref ahead
/// of the commit.
fn new_name(noun: &str, name: &str) -> Result<(), String> {
    let odd = name.starts_with('+')
        || name.contains(['^', '~', ':', '?', '*', '[', '\\'])
        || name.contains("..")
        || name.contains("@{")
        || name.chars().any(char::is_whitespace);
    if flat(name) && !odd && hash(name).is_err() {
        Ok(())
    } else {
        Err(format!("\"{name}\" can't name a {noun}."))
    }
}

fn entry(id: &str) -> Result<(), String> {
    let ok = (1..=ENTRY_MOST).contains(&id.len())
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(format!("\"{id}\" isn't a discard of the trash."))
    }
}

/// One line of at most `most` characters.
fn line(field: &str, text: &str, most: usize) -> Result<(), String> {
    if text.chars().count() <= most && !text.chars().any(|c| c.is_control()) {
        Ok(())
    } else {
        Err(format!(
            "{field} is one line of at most {} characters.",
            thousands(most)
        ))
    }
}

fn name_line(field: &str, text: &str) -> Result<(), String> {
    if text.is_empty() || text.starts_with('-') {
        return Err(format!(
            "{field} is one line of at most {} characters.",
            thousands(LINE_MOST)
        ));
    }
    line(field, text, LINE_MOST)
}

fn most(field: &str, n: usize, most: usize) -> Result<(), String> {
    if n <= most {
        Ok(())
    } else {
        Err(format!("{field} is at most {}.", thousands(most)))
    }
}

fn stash(s: &StashRef) -> Result<(), String> {
    most("stash index", s.index, STASH_MOST)?;
    hash(&s.hash)
}

const NOT_A_HUNK: &str = "The patch isn't one hunk of one file, as the diff shows it.";

/// The file a patch changes, when it is exactly one hunk of one file as the panel draws it
/// (`hunk_patch`, byte for byte): no mode, rename, binary or symlink header gets through.
pub(super) fn patch_file(patch: &str) -> Result<FileDiff, String> {
    if patch.len() > DIFF_LIMIT {
        return Err(NOT_A_HUNK.to_string());
    }
    let mut files = parse_diff(patch, DIFF_LIMIT);
    let (Some(file), true) = (files.pop(), files.is_empty()) else {
        return Err(NOT_A_HUNK.to_string());
    };
    if file.hunks.len() != 1 || hunk_patch(&file, 0).as_deref() != Some(patch) {
        return Err(NOT_A_HUNK.to_string());
    }
    path(file.path())?;
    Ok(file)
}

impl Op {
    /// The shape rules: what an argument may look like. No git, no filesystem; the Err is the
    /// sentence of a malformed call.
    pub fn check(&self) -> Result<(), String> {
        match self {
            Op::Find
            | Op::Look
            | Op::Branches
            | Op::DefaultBranch
            | Op::Stashes
            | Op::Remotes
            | Op::LastMessage
            | Op::Trash
            | Op::Init
            | Op::Fetch
            | Op::Push
            | Op::Continue
            | Op::Abort
            | Op::Pull { .. }
            | Op::Sync { .. }
            | Op::Message { .. } => Ok(()),
            Op::Diff { whole, .. } => {
                most("whole", whole.len(), WHOLE_MOST)?;
                whole.iter().try_for_each(|w| path(&w.path))
            }
            Op::History {
                search,
                path: file,
                skip,
                ..
            } => {
                most("skip", *skip, SKIP_MOST)?;
                if let Some(SearchArg { text, .. }) = search {
                    line("text", text, SEARCH_MOST)?;
                }
                file.as_deref().map_or(Ok(()), path)
            }
            Op::Show { hash: h, .. } | Op::Checkout { hash: h } | Op::Revert { hash: h } => hash(h),
            Op::Blame { path: p, hash: h } => {
                path(p)?;
                h.as_deref().map_or(Ok(()), hash)
            }
            Op::Unmerged { branch }
            | Op::PushBranch { branch }
            | Op::Merge { branch }
            | Op::DeleteBranch { branch, .. } => existing("branch", branch),
            Op::Compare {
                base,
                branch,
                hash: h,
                ..
            } => {
                existing("branch", base)?;
                existing("branch", branch)?;
                h.as_deref().map_or(Ok(()), hash)
            }
            Op::StashDiff { stash: s, .. }
            | Op::StashApply { stash: s }
            | Op::StashPop { stash: s }
            | Op::StashDrop { stash: s } => stash(s),
            Op::Measure { paths: list }
            | Op::Stage { paths: list }
            | Op::Unstage { paths: list }
            | Op::Discard { paths: list } => paths(list),
            Op::SetAuthor { name, email } => {
                name_line("name", name)?;
                name_line("email", email)
            }
            Op::StageHunk { patch } | Op::UnstageHunk { patch } => patch_file(patch).map(drop),
            Op::Commit {
                summary,
                description,
                ..
            } => {
                line("summary", summary, LINE_MOST)?;
                if description.chars().count() > DESCRIPTION_MOST || description.contains('\0') {
                    return Err(format!(
                        "description is at most {} characters.",
                        thousands(DESCRIPTION_MOST)
                    ));
                }
                Ok(())
            }
            Op::UndoCommit { hash: h, replaced } => {
                hash(h)?;
                replaced.as_deref().map_or(Ok(()), hash)
            }
            Op::Publish { remote, branch } => {
                existing("remote", remote)?;
                existing("branch", branch)
            }
            Op::Switch { branch, leave, .. } => {
                existing("branch", branch)?;
                leave
                    .as_deref()
                    .map_or(Ok(()), |l| line("leave", l, LINE_MOST))
            }
            Op::NewBranch { name, hash: h } => {
                new_name("branch", name)?;
                h.as_deref().map_or(Ok(()), hash)
            }
            Op::RenameBranch { branch, name } => {
                existing("branch", branch)?;
                new_name("branch", name)
            }
            Op::Tag { name, hash: h } => {
                new_name("tag", name)?;
                hash(h)
            }
            Op::Stash { message, .. } => line("message", message, LINE_MOST),
            Op::Resolve { path: p, .. } | Op::Ignore { path: p } => path(p),
            Op::ResolveRegion {
                path: p, region, ..
            } => {
                path(p)?;
                most("region", *region, REGION_MOST)
            }
            Op::Restore { entry: id, .. } => entry(id),
            Op::DiscardHunk { path: p, patch } => {
                path(p)?;
                let file = patch_file(patch)?;
                if file.path() == p {
                    Ok(())
                } else {
                    Err(format!("The patch isn't a change to {p}."))
                }
            }
            Op::RestoreVersion {
                hash: h,
                then,
                path: p,
            } => {
                hash(h)?;
                path(then)?;
                path(p)
            }
        }
    }
}

// What the repository says.

fn outside(path: &str) -> Failure {
    Failure::Refused(format!("{path} lies outside the repository."))
}

/// `path`'s folder resolves inside the repository: its deepest ancestor that exists, with every
/// link followed, lies under the root. The file itself may be a link: that is `not_link`'s to
/// refuse, where Templum writes, and git doesn't follow it.
pub(super) fn inside(root: &Path, path: &str) -> Result<(), Failure> {
    let root = std::fs::canonicalize(root).map_err(|_| outside(path))?;
    let mut at = root.join(path);
    while at.pop() {
        match std::fs::symlink_metadata(&at) {
            // A link that leads nowhere is no folder to write in.
            Ok(_) => {
                let real = std::fs::canonicalize(&at).map_err(|_| outside(path))?;
                return if real.starts_with(&root) {
                    Ok(())
                } else {
                    Err(outside(path))
                };
            }
            Err(_) => continue,
        }
    }
    Err(outside(path))
}

/// The file isn't a symbolic link: Templum doesn't write through one.
pub(super) fn not_link(root: &Path, path: &str) -> Result<(), Failure> {
    match std::fs::symlink_metadata(root.join(path)) {
        Ok(m) if m.file_type().is_symlink() => Err(Failure::Refused(format!(
            "{path} is a link, which Templum doesn't write through."
        ))),
        _ => Ok(()),
    }
}

/// The branch `name` is listed; a local one when `local`, else a remote one.
pub(super) fn branch(repo: &Repo, name: &str, side: Which) -> Result<(), Failure> {
    let listed = repo.branches()?.into_iter().any(|b| {
        b.name == name
            && match side {
                Which::Local => !b.remote,
                Which::Remote => b.remote,
                Which::Any => true,
            }
    });
    if listed {
        Ok(())
    } else {
        Err(Failure::Refused(format!(
            "There's no branch {name}: branches lists them."
        )))
    }
}

/// Which branches a name may be.
#[derive(Clone, Copy)]
pub(super) enum Which {
    Local,
    Remote,
    Any,
}

pub(super) fn remote(repo: &Repo, name: &str) -> Result<(), Failure> {
    if repo.remotes()?.iter().any(|r| r == name) {
        Ok(())
    } else {
        Err(Failure::Refused(format!(
            "There's no remote {name}: remotes lists them."
        )))
    }
}

/// The status's entry for `path` (the old name of a rename too, with `from`) that has the
/// change `wanted` picks; or `refusal` says what it lacks.
pub(super) fn listed<'a>(
    status: &'a Status,
    path: &str,
    from: bool,
    wanted: impl Fn(&Entry) -> bool,
    refusal: &str,
) -> Result<&'a Entry, Failure> {
    status
        .entries
        .iter()
        .find(|e| (e.path == path || (from && e.from.as_deref() == Some(path))) && wanted(e))
        .ok_or_else(|| Failure::Refused(format!("{path} {refusal}")))
}

pub(super) fn conflicted<'a>(status: &'a Status, path: &str) -> Result<&'a Entry, Failure> {
    listed(status, path, false, Entry::conflict, "isn't in conflict.")
}

pub(super) fn unstaged_change(e: &Entry) -> bool {
    e.unstaged.is_some()
}

pub(super) fn staged_change(e: &Entry) -> bool {
    e.staged.is_some()
}

/// A stash as the list showed it.
pub(super) fn stash_of(s: &StashRef) -> Stash {
    Stash {
        index: s.index,
        hash: s.hash.clone(),
        time: 0,
        message: String::new(),
    }
}
