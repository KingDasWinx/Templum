//! The operations of the API's `git` tool, one per Git popup job, as one enum that parses from
//! the call's JSON, and the answer each gives.

use serde::{Deserialize, Serialize};

use crate::blame::BlameLine;
use crate::conflict::Take;
use crate::trash::{Kept, Restored};
use crate::{Back, Branch, FileDiff, Pull, Search, Side, Stash, Synced, WorkingDiff, message};

use super::{CommitShown, Committed, Compared, Look, Page, Undone};

/// A search of History: where it looks and what for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchArg {
    pub by: Search,
    pub text: String,
}

/// A stash as the list shows it: its number, and the commit it is, so a list that changed
/// since is told rather than acted on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StashRef {
    pub index: usize,
    pub hash: String,
}

/// A file the diff shows whole (every line as context), on the staged side or not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WholeFile {
    pub staged: bool,
    pub path: String,
}

/// One operation of the `git` tool, named by `op`, its arguments beside it. Paths are relative
/// to the repository's root. Nothing here reaches git before `Op::check` and the state checks
/// of [`run`](super::run) passed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Find,
    Look,
    Diff {
        #[serde(default)]
        ignore_whitespace: bool,
        #[serde(default)]
        whole: Vec<WholeFile>,
    },
    History {
        #[serde(default)]
        all: bool,
        #[serde(default)]
        search: Option<SearchArg>,
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        skip: usize,
    },
    Show {
        hash: String,
        #[serde(default)]
        ignore_whitespace: bool,
    },
    Blame {
        path: String,
        #[serde(default)]
        hash: Option<String>,
    },
    Branches,
    Unmerged {
        branch: String,
    },
    DefaultBranch,
    Compare {
        base: String,
        branch: String,
        #[serde(default)]
        hash: Option<String>,
        #[serde(default)]
        ignore_whitespace: bool,
    },
    Stashes,
    StashDiff {
        stash: StashRef,
        #[serde(default)]
        ignore_whitespace: bool,
    },
    Remotes,
    LastMessage,
    Measure {
        paths: Vec<String>,
    },
    Trash,
    Init,
    SetAuthor {
        name: String,
        email: String,
    },
    Stage {
        paths: Vec<String>,
    },
    Unstage {
        paths: Vec<String>,
    },
    StageHunk {
        patch: String,
    },
    UnstageHunk {
        patch: String,
    },
    Commit {
        #[serde(default)]
        summary: String,
        #[serde(default)]
        description: String,
        #[serde(default)]
        amend: bool,
        #[serde(default)]
        all: bool,
    },
    UndoCommit {
        hash: String,
        #[serde(default)]
        replaced: Option<String>,
    },
    Fetch,
    Pull {
        #[serde(default)]
        how: Pull,
    },
    Push,
    PushBranch {
        branch: String,
    },
    Publish {
        remote: String,
        branch: String,
    },
    Sync {
        #[serde(default)]
        join: Option<bool>,
    },
    Switch {
        branch: String,
        #[serde(default)]
        remote: bool,
        #[serde(default)]
        leave: Option<String>,
    },
    NewBranch {
        name: String,
        #[serde(default)]
        hash: Option<String>,
    },
    RenameBranch {
        branch: String,
        name: String,
    },
    Merge {
        branch: String,
    },
    Checkout {
        hash: String,
    },
    Tag {
        name: String,
        hash: String,
    },
    Revert {
        hash: String,
    },
    Stash {
        message: String,
        #[serde(default)]
        untracked: bool,
    },
    StashApply {
        stash: StashRef,
    },
    StashPop {
        stash: StashRef,
    },
    Continue,
    Resolve {
        path: String,
        side: Side,
    },
    ResolveRegion {
        path: String,
        region: usize,
        take: Take,
        shown: (Vec<String>, Vec<String>),
    },
    Restore {
        entry: String,
        #[serde(default)]
        force: bool,
    },
    Ignore {
        path: String,
    },
    Message {
        #[serde(default)]
        all: bool,
    },
    Discard {
        paths: Vec<String>,
    },
    DiscardHunk {
        path: String,
        patch: String,
    },
    RestoreVersion {
        hash: String,
        then: String,
        path: String,
    },
    DeleteBranch {
        branch: String,
        #[serde(default)]
        force: bool,
    },
    StashDrop {
        stash: StashRef,
    },
    Abort,
}

/// Every operation's name, in the order the docs list them.
pub const OPS: &[&str] = &[
    "find",
    "look",
    "diff",
    "history",
    "show",
    "blame",
    "branches",
    "unmerged",
    "default_branch",
    "compare",
    "stashes",
    "stash_diff",
    "remotes",
    "last_message",
    "measure",
    "trash",
    "init",
    "set_author",
    "stage",
    "unstage",
    "stage_hunk",
    "unstage_hunk",
    "commit",
    "undo_commit",
    "fetch",
    "pull",
    "push",
    "push_branch",
    "publish",
    "sync",
    "switch",
    "new_branch",
    "rename_branch",
    "merge",
    "checkout",
    "tag",
    "revert",
    "stash",
    "stash_apply",
    "stash_pop",
    "continue",
    "resolve",
    "resolve_region",
    "restore",
    "ignore",
    "message",
    "discard",
    "discard_hunk",
    "restore_version",
    "delete_branch",
    "stash_drop",
    "abort",
];

impl Op {
    /// Its name, as `op` says it.
    pub fn name(&self) -> &'static str {
        match self {
            Op::Find => "find",
            Op::Look => "look",
            Op::Diff { .. } => "diff",
            Op::History { .. } => "history",
            Op::Show { .. } => "show",
            Op::Blame { .. } => "blame",
            Op::Branches => "branches",
            Op::Unmerged { .. } => "unmerged",
            Op::DefaultBranch => "default_branch",
            Op::Compare { .. } => "compare",
            Op::Stashes => "stashes",
            Op::StashDiff { .. } => "stash_diff",
            Op::Remotes => "remotes",
            Op::LastMessage => "last_message",
            Op::Measure { .. } => "measure",
            Op::Trash => "trash",
            Op::Init => "init",
            Op::SetAuthor { .. } => "set_author",
            Op::Stage { .. } => "stage",
            Op::Unstage { .. } => "unstage",
            Op::StageHunk { .. } => "stage_hunk",
            Op::UnstageHunk { .. } => "unstage_hunk",
            Op::Commit { .. } => "commit",
            Op::UndoCommit { .. } => "undo_commit",
            Op::Fetch => "fetch",
            Op::Pull { .. } => "pull",
            Op::Push => "push",
            Op::PushBranch { .. } => "push_branch",
            Op::Publish { .. } => "publish",
            Op::Sync { .. } => "sync",
            Op::Switch { .. } => "switch",
            Op::NewBranch { .. } => "new_branch",
            Op::RenameBranch { .. } => "rename_branch",
            Op::Merge { .. } => "merge",
            Op::Checkout { .. } => "checkout",
            Op::Tag { .. } => "tag",
            Op::Revert { .. } => "revert",
            Op::Stash { .. } => "stash",
            Op::StashApply { .. } => "stash_apply",
            Op::StashPop { .. } => "stash_pop",
            Op::Continue => "continue",
            Op::Resolve { .. } => "resolve",
            Op::ResolveRegion { .. } => "resolve_region",
            Op::Restore { .. } => "restore",
            Op::Ignore { .. } => "ignore",
            Op::Message { .. } => "message",
            Op::Discard { .. } => "discard",
            Op::DiscardHunk { .. } => "discard_hunk",
            Op::RestoreVersion { .. } => "restore_version",
            Op::DeleteBranch { .. } => "delete_branch",
            Op::StashDrop { .. } => "stash_drop",
            Op::Abort => "abort",
        }
    }
}

/// What home lends an operation beyond the repository.
pub struct Home<'a> {
    /// The trash discards go to; None: this Templum keeps none.
    pub trash: Option<&'a crate::trash::Trash>,
    /// The commit-message agent, when one is ready and the person allowed the diff to go to it.
    pub writer: Option<&'a message::Writer>,
    /// `set_author` writes the global config (a token for all workspaces) or only the
    /// repository's own (a share: home's identity is not its to change).
    pub global_author: bool,
}

/// An operation's answer: a sentence for people, and its result as JSON.
pub struct Answer {
    pub text: String,
    pub git: Done,
}

/// What an operation gives back, as the JSON `git` of the answer.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum Done {
    Found(Found),
    Look(Look),
    Diff(WorkingDiff),
    History(Page),
    Show(CommitShown),
    Blame(Vec<BlameLine>),
    Branches(Vec<Branch>),
    Count(u32),
    Name(Option<String>),
    Compare(Compared),
    Stashes(Vec<Stash>),
    Files(Vec<FileDiff>),
    Names(Vec<String>),
    Text(String),
    Measured(Measured),
    Trash(Vec<Kept>),
    Committed(Committed),
    Undone(Undone),
    Synced(Synced),
    Switched(Back),
    Restored(Restored),
    Kept(Option<Kept>),
    Message(Message),
    /// JSON `null`.
    Nothing,
}

/// Where the repository is: `find`, `init`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    /// Its working tree's root; None: there is no repository.
    pub root: Option<String>,
    /// The main working tree of a linked worktree.
    pub main_tree: Option<String>,
}

/// What a discard of some paths would keep.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measured {
    pub files: usize,
    pub bytes: u64,
    /// A copy would be kept: there is a trash and the paths fit in it.
    pub keep: bool,
}

/// A commit message written by the agent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub summary: String,
    pub description: String,
    /// A commit was made while it was writing: the message describes changes already committed.
    pub committed: bool,
}
