//! Git through the user's own `git` program, run as a command with stable output formats
//! (`status --porcelain=v2 -z`, a log record format, `for-each-ref`), parsed into typed values.
//! No UI, no library binding: credentials and SSH work as they do in the user's terminal.

pub mod blame;
pub mod conflict;
mod diff;
pub mod graph;
pub mod message;
pub mod ops;
mod parse;
mod repo;
pub mod trash;
mod words;

pub use diff::{
    FileDiff, Hunk, Kind, LARGE, Line, hunk_patch, new_binary_diff, new_file_diff, parse_diff,
};
pub use parse::{
    BRANCH_FORMAT, Branch, Change, Commit, CommitFile, Entry, LOG_DATE, LOG_FORMAT, Operation,
    STASH_FORMAT, Stash, Status, ago, needs_input, not_fast_forward, not_merged, parse_branches,
    parse_file_log, parse_log, parse_name_status, parse_stashes, parse_status,
};
pub use repo::{
    Back, DIFF_LIMIT, GitError, HistoryQuery, Pull, Repo, Search, Side, Synced, WorkingDiff,
    WorktreeState, clone_name, clone_target, left_on, root_of,
};
pub use words::changed;
