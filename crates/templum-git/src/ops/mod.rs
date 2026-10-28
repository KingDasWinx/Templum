//! The Git popup's background jobs, one function each, and the API's `git` tool that runs the
//! same ones: the popup calls the functions with arguments git gave it, the API goes through
//! [`run`] after checking every argument a caller wrote. No UI.

mod check;
mod jobs;
mod op;
mod run;

use std::collections::HashSet;
use std::io;

use serde::{Deserialize, Serialize};

use crate::message;
use crate::{Commit, CommitFile, Entry, FileDiff, GitError, Operation, Status};

pub use jobs::*;
pub use op::*;
pub use run::{ANSWER_MOST, fit, run};

/// What a hunk's patch git refuses says: the file changed since it was read.
pub const CHANGED: &str = "The file changed; try again";

/// What a commit refused because the files to commit changed since the message was written.
pub const CHANGED_MEANWHILE: &str =
    "The files to commit changed since you last looked: write a message for them.";

/// How many commits History loads at a time.
pub const PAGE: usize = 200;

/// Why a job didn't happen.
#[derive(Debug)]
pub enum Failure {
    /// A state refusal the API makes before git runs: its sentence.
    Refused(String),
    /// The region or the file isn't what was shown any more.
    Changed,
    /// The copy in the trash couldn't be made: nothing was changed.
    Keep(io::Error),
    /// A file couldn't be read or written.
    Io(io::Error),
    Git(GitError),
    /// The commit-message agent gave no message.
    Agent(message::Failure),
}

impl From<GitError> for Failure {
    fn from(e: GitError) -> Failure {
        Failure::Git(e)
    }
}

impl Failure {
    /// The refusal as a sentence, as the API says it.
    pub fn sentence(&self) -> String {
        match self {
            Failure::Refused(why) => why.clone(),
            Failure::Changed => CHANGED.to_string(),
            Failure::Keep(e) => format!("Couldn't keep a copy, so nothing was changed: {e}"),
            Failure::Io(e) => format!("Couldn't write the file: {e}"),
            Failure::Git(e) => e.first_line().to_string(),
            Failure::Agent(f) => f.to_string(),
        }
    }
}

/// "1,240".
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What a discard throws away: whole files' unstaged changes, or one hunk (its patch, reversed
/// on the working tree).
#[derive(Clone)]
pub enum Discarding {
    Files(Vec<Entry>),
    Hunk(String),
}

/// A branch against another: its commits and its diff.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compared {
    pub commits: Vec<Commit>,
    pub files: Vec<FileDiff>,
}

/// A page of History: its commits, the file at each (in a file's history), and the commits
/// not pushed (read with the first page).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub commits: Vec<Commit>,
    pub paths: Vec<CommitFile>,
    pub unpushed: Option<HashSet<String>>,
}

/// A commit with its files and its whole diff (against its first parent).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitShown {
    pub commit: Commit,
    pub files: Vec<CommitFile>,
    pub diffs: Vec<FileDiff>,
}

/// One refresh's result: what the popup shows of the repository, and the API's `look`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    pub status: Status,
    pub operation: Option<Operation>,
    /// `user.name` and `user.email`; None when either is unset.
    pub author: Option<(String, String)>,
}

/// A commit made.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Committed {
    pub short: String,
    /// An amend's old HEAD: Undo goes back to it.
    pub replaced: Option<String>,
}

/// What Undo commit found.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Undone {
    /// Taken back: its message, for the fields.
    Done(String),
    /// On the upstream: Revert… instead.
    Pushed,
    /// HEAD isn't that commit any more.
    Moved,
}
