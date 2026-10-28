//! Pure parsers for `git`'s machine formats. Every format here is documented by Git and stable:
//! `status --porcelain=v2 -z`, `log --format` with `-z`, `for-each-ref --format`,
//! `stash list --format`, `diff-tree --name-status -z`.

use serde::{Deserialize, Serialize};

/// What happened to a file, as the status badges name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
    TypeChanged,
}

impl Change {
    /// The badge's letter (docs/specs/cockpit.md, section 8).
    pub fn letter(self) -> char {
        match self {
            Change::Modified => 'M',
            Change::Added => 'A',
            Change::Deleted => 'D',
            Change::Renamed => 'R',
            Change::Untracked => 'U',
            Change::Conflict => 'C',
            Change::TypeChanged => 'T',
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Change::Modified => "modified",
            Change::Added => "added",
            Change::Deleted => "deleted",
            Change::Renamed => "renamed",
            Change::Untracked => "untracked",
            Change::Conflict => "conflict",
            Change::TypeChanged => "type changed",
        }
    }

    /// A porcelain status letter; '.' and anything unknown is None. A copy counts as added.
    fn of(letter: u8) -> Option<Change> {
        Some(match letter {
            b'M' => Change::Modified,
            b'A' | b'C' => Change::Added,
            b'D' => Change::Deleted,
            b'R' => Change::Renamed,
            b'T' => Change::TypeChanged,
            b'U' => Change::Conflict,
            _ => return None,
        })
    }
}

/// One path in the status: what is staged for it and what isn't.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Relative to the root, with '/' on every OS, as git prints it.
    pub path: String,
    /// A rename's old path.
    pub from: Option<String>,
    /// Index against HEAD.
    pub staged: Option<Change>,
    /// Working tree against the index; an untracked file is `Untracked` here, a conflict is
    /// `Conflict` here.
    pub unstaged: Option<Change>,
}

impl Entry {
    pub fn conflict(&self) -> bool {
        self.unstaged == Some(Change::Conflict)
    }

    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    /// The folder part, "" at the root.
    pub fn folder(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(dir, _)| dir)
    }
}

/// `git status --porcelain=v2 --branch -z -uall`, parsed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    /// None: detached.
    pub branch: Option<String>,
    /// HEAD's commit; None before the first commit.
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// An upstream is set but no longer exists (deleted on the server, then pruned): the counts
    /// mean nothing.
    pub gone: bool,
    pub entries: Vec<Entry>,
}

impl Status {
    pub fn conflicts(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.conflict()).collect()
    }

    pub fn staged(&self) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.staged.is_some() && !e.conflict())
            .collect()
    }

    /// Every changed path but the conflicts, once each, staged or not: the Changes checklist.
    pub fn files(&self) -> Vec<&Entry> {
        self.entries.iter().filter(|e| !e.conflict()).collect()
    }

    /// Unstaged changes and untracked files, together.
    pub fn unstaged(&self) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|e| e.unstaged.is_some() && !e.conflict())
            .collect()
    }
}

/// NUL-separated records, as `-z` prints them, without the empty last one.
fn records(out: &[u8]) -> impl Iterator<Item = &str> {
    out.split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .map(|r| std::str::from_utf8(r).unwrap_or(""))
}

pub fn parse_status(out: &[u8]) -> Status {
    let mut status = Status::default();
    let mut records = records(out);
    let mut counted = false;
    while let Some(record) = records.next() {
        if let Some(header) = record.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" if value != "(initial)" => status.oid = Some(value.to_string()),
                "branch.head" if value != "(detached)" => status.branch = Some(value.to_string()),
                "branch.upstream" => status.upstream = Some(value.to_string()),
                "branch.ab" => {
                    counted = true;
                    let mut parts = value.split(' ');
                    status.ahead = parts.next().and_then(|a| a[1..].parse().ok()).unwrap_or(0);
                    status.behind = parts.next().and_then(|b| b[1..].parse().ok()).unwrap_or(0);
                }
                _ => {}
            }
            continue;
        }
        let mut fields = record.splitn(2, ' ');
        let (kind, rest) = (fields.next().unwrap_or(""), fields.next().unwrap_or(""));
        let entry = match kind {
            // ? <path>
            "?" => Entry {
                path: rest.to_string(),
                from: None,
                staged: None,
                unstaged: Some(Change::Untracked),
            },
            // 1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>
            // 2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>  then the old path's record
            // u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>
            "1" | "2" | "u" => {
                let xy = rest.as_bytes();
                let (x, y) = (
                    xy.first().copied().unwrap_or(b'.'),
                    xy.get(1).copied().unwrap_or(b'.'),
                );
                let skip = match kind {
                    "1" => 7,
                    "2" => 8,
                    _ => 9,
                };
                let path = rest
                    .splitn(skip + 1, ' ')
                    .nth(skip)
                    .unwrap_or("")
                    .to_string();
                let from = (kind == "2").then(|| records.next().unwrap_or("").to_string());
                if kind == "u" {
                    Entry {
                        path,
                        from: None,
                        staged: None,
                        unstaged: Some(Change::Conflict),
                    }
                } else {
                    Entry {
                        path,
                        from,
                        staged: Change::of(x),
                        unstaged: Change::of(y),
                    }
                }
            }
            // "!" (ignored) is never asked for; anything else is unknown.
            _ => continue,
        };
        status.entries.push(entry);
    }
    status.gone = status.upstream.is_some() && !counted;
    status
}

/// `git log -z --format=LOG_FORMAT --date=LOG_DATE`: fields split by 0x1f, records by NUL.
pub const LOG_FORMAT: &str = "%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%ad%x1f%P%x1f%D%x1f%s%x1f%b";
pub const LOG_DATE: &str = "format:%Y-%m-%d %H:%M";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub author: String,
    pub email: String,
    /// Unix seconds.
    pub time: i64,
    /// "2026-10-11 16:14", in the local time zone.
    pub date: String,
    pub parents: Vec<String>,
    /// As `%D` names them: "HEAD -> main", "tag: v1", "origin/main".
    pub refs: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// One `LOG_FORMAT` record (its ten fields); None for anything else.
pub(crate) fn commit_of(record: &str) -> Option<Commit> {
    let f: Vec<&str> = record.splitn(10, '\x1f').collect();
    if f.len() < 10 {
        return None;
    }
    let list = |s: &str| -> Vec<String> {
        s.split(", ")
            .filter(|r| !r.is_empty())
            .map(str::to_string)
            .collect()
    };
    Some(Commit {
        hash: f[0].to_string(),
        short: f[1].to_string(),
        author: f[2].to_string(),
        email: f[3].to_string(),
        time: f[4].parse().unwrap_or(0),
        date: f[5].to_string(),
        parents: f[6]
            .split(' ')
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect(),
        refs: list(f[7]),
        subject: f[8].to_string(),
        body: f[9].trim_end().to_string(),
    })
}

pub fn parse_log(out: &[u8]) -> Vec<Commit> {
    records(out).filter_map(commit_of).collect()
}

/// `git log -z --follow --name-status --format=LOG_FORMAT -- path`: each commit with the file
/// as it was in it (a rename's old path in `from`). After each record git prints "\n" and the
/// status, then one path (two for a rename or a copy); a merge prints none, and keeps the path
/// of the commit after it in the list.
pub fn parse_file_log(out: &[u8], path: &str) -> Vec<(Commit, CommitFile)> {
    let mut entries = Vec::new();
    let mut now = path.to_string();
    let mut fields = records(out).peekable();
    while let Some(record) = fields.next() {
        let Some(commit) = commit_of(record.trim_start_matches('\n')) else {
            continue;
        };
        let mut file = CommitFile {
            change: Change::Modified,
            path: now.clone(),
            from: None,
        };
        let status = fields
            .peek()
            .filter(|f| !f.contains('\x1f'))
            .map(|f| f.trim_start_matches('\n').to_string());
        if let Some(code) = status {
            fields.next();
            if let Some(change) = code.bytes().next().and_then(Change::of) {
                let first = fields.next().unwrap_or("").to_string();
                file = if change == Change::Renamed || code.starts_with('C') {
                    CommitFile {
                        change,
                        path: fields.next().unwrap_or("").to_string(),
                        from: Some(first),
                    }
                } else {
                    CommitFile {
                        change,
                        path: first,
                        from: None,
                    }
                };
            }
        }
        // Older commits knew it by its name before this one's rename.
        now = file.from.clone().unwrap_or_else(|| file.path.clone());
        entries.push((commit, file));
    }
    entries
}

/// `git for-each-ref --format=BRANCH_FORMAT refs/heads refs/remotes`: one line per ref, fields
/// split by NUL.
pub const BRANCH_FORMAT: &str = "%(refname)%00%(refname:lstrip=2)%00%(objectname:short)%00%(upstream:lstrip=2)%00%(upstream:track,nobracket)%00%(subject)%00%(HEAD)%00%(committerdate:unix)";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    /// "main", or "origin/main" for a remote one.
    pub name: String,
    pub remote: bool,
    pub current: bool,
    /// The short hash of its last commit.
    pub commit: String,
    pub upstream: Option<String>,
    /// Its upstream no longer exists.
    pub gone: bool,
    pub ahead: u32,
    pub behind: u32,
    /// Its last commit's subject.
    pub subject: String,
    /// Its last commit's date, Unix seconds (0 when git gave none).
    pub time: i64,
}

pub fn parse_branches(out: &[u8]) -> Vec<Branch> {
    let text = String::from_utf8_lossy(out);
    text.lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split('\0').collect();
            if f.len() < 8 || f[0].ends_with("/HEAD") {
                return None;
            }
            let track = f[4];
            let count = |word: &str| -> u32 {
                track
                    .split(", ")
                    .find_map(|part| part.strip_prefix(word))
                    .and_then(|n| n.trim().parse().ok())
                    .unwrap_or(0)
            };
            Some(Branch {
                name: f[1].to_string(),
                remote: f[0].starts_with("refs/remotes/"),
                current: f[6] == "*",
                commit: f[2].to_string(),
                upstream: (!f[3].is_empty()).then(|| f[3].to_string()),
                gone: track == "gone",
                ahead: count("ahead "),
                behind: count("behind "),
                subject: f[5].to_string(),
                time: f[7].trim().parse().unwrap_or(0),
            })
        })
        .collect()
}

/// `git stash list -z --format=STASH_FORMAT`.
pub const STASH_FORMAT: &str = "%gd%x1f%H%x1f%at%x1f%gs";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stash {
    /// N in `stash@{N}`: it shifts when a stash is pushed or dropped.
    pub index: usize,
    /// Its commit: who it is, whatever its index becomes.
    pub hash: String,
    pub time: i64,
    /// "On main: my work", "WIP on main: 3dba34c Add 2b".
    pub message: String,
}

pub fn parse_stashes(out: &[u8]) -> Vec<Stash> {
    records(out)
        .filter_map(|record| {
            let f: Vec<&str> = record.splitn(4, '\x1f').collect();
            if f.len() < 4 {
                return None;
            }
            let index = f[0]
                .strip_prefix("stash@{")?
                .strip_suffix('}')?
                .parse()
                .ok()?;
            Some(Stash {
                index,
                hash: f[1].to_string(),
                time: f[2].parse().unwrap_or(0),
                message: f[3].to_string(),
            })
        })
        .collect()
}

/// One file of a commit, from `git diff-tree --root --no-commit-id -r -z -M --name-status`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitFile {
    pub change: Change,
    pub path: String,
    pub from: Option<String>,
}

pub fn parse_name_status(out: &[u8]) -> Vec<CommitFile> {
    let mut files = Vec::new();
    let mut records = records(out);
    while let Some(code) = records.next() {
        let Some(change) = code.bytes().next().and_then(Change::of) else {
            continue;
        };
        let Some(first) = records.next() else {
            break;
        };
        // R<score> and C<score> take two paths: the old one first.
        if matches!(change, Change::Renamed) || code.starts_with('C') {
            let Some(path) = records.next() else {
                break;
            };
            files.push(CommitFile {
                change,
                path: path.to_string(),
                from: Some(first.to_string()),
            });
        } else {
            files.push(CommitFile {
                change,
                path: first.to_string(),
                from: None,
            });
        }
    }
    files
}

/// A merge, rebase, cherry-pick or revert in progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

impl Operation {
    pub fn name(self) -> &'static str {
        match self {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
        }
    }
}

/// `git` failed because it would have asked for a password, a passphrase or a host's key.
pub fn needs_input(stderr: &str) -> bool {
    [
        "terminal prompts disabled",
        "could not read Username",
        "could not read Password",
        "Permission denied (publickey",
        "Authentication failed",
        "Host key verification failed",
    ]
    .iter()
    .any(|s| stderr.contains(s))
}

pub fn not_fast_forward(stderr: &str) -> bool {
    stderr.contains("Not possible to fast-forward")
}

pub fn not_merged(stderr: &str) -> bool {
    stderr.contains("not fully merged")
}

/// "just now", "5 min ago", "3 h ago", "2 d ago", "1 mo ago", "2 y ago".
pub fn ago(now: i64, then: i64) -> String {
    let s = (now - then).max(0);
    match s {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86_400 => format!("{} h ago", s / 3600),
        86_400..2_592_000 => format!("{} d ago", s / 86_400),
        2_592_000..31_536_000 => format!("{} mo ago", s / 2_592_000),
        _ => format!("{} y ago", s / 31_536_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &[u8] = b"# branch.oid 0d817e9ea557196af5794a0915ba6b96e6745223\0\
# branch.head main\0\
# branch.upstream origin/main\0\
# branch.ab +1 -2\0\
1 A. N... 000000 100644 100644 0000000000000000000000000000000000000000 78981922613b2afb6025042ff6bd878ac1994e85 added.txt\0\
1 D. N... 100644 000000 000000 587be6b4c3f93f93c489c0111bba5596147a26cb 0000000000000000000000000000000000000000 del.txt\0\
1 .M N... 100644 100644 100644 814f4a422927b82f5f8a43f8fab6d3839e3983f2 814f4a422927b82f5f8a43f8fab6d3839e3983f2 keep.txt\0\
1 T. N... 100644 120000 120000 1f9d725a9de833a65966881dce2e907b86e72c5e 1764325aa997b79e6f74da850facef86261812e1 link.txt\0\
2 R. N... 100644 100644 100644 4286f428e3b19fe84de503916ce0e7dc8deefea1 4286f428e3b19fe84de503916ce0e7dc8deefea1 R100 new.txt\0old.txt\0\
1 MM N... 100644 100644 100644 b4785957bc986dc39c629de9fac9df46972c00fc b4785957bc986dc39c629de9fac9df46972c00fc sub/s.txt\0\
u UU N... 100644 100644 100644 100644 4cb29ea38f70d7c61b2a3a25b02e3bdf44905402 57fa2841853ee1fe297868902b06cd8c486741d6 879de50b9a967adae6ab5fe536d4ef6e0af21bb1 c.txt\0\
? untracked.txt\0\
? with space.txt\0";

    #[test]
    fn porcelain_v2_with_every_kind_of_entry() {
        let s = parse_status(STATUS);
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!(
            s.oid.as_deref(),
            Some("0d817e9ea557196af5794a0915ba6b96e6745223")
        );
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (1, 2));
        let kinds: Vec<_> = s
            .entries
            .iter()
            .map(|e| (e.path.as_str(), e.staged, e.unstaged))
            .collect();
        use Change::*;
        assert_eq!(
            kinds,
            [
                ("added.txt", Some(Added), None),
                ("del.txt", Some(Deleted), None),
                ("keep.txt", None, Some(Modified)),
                ("link.txt", Some(TypeChanged), None),
                ("new.txt", Some(Renamed), None),
                ("sub/s.txt", Some(Modified), Some(Modified)),
                ("c.txt", None, Some(Conflict)),
                ("untracked.txt", None, Some(Untracked)),
                ("with space.txt", None, Some(Untracked)),
            ]
        );
        assert_eq!(s.entries[4].from.as_deref(), Some("old.txt"));
        assert_eq!(s.conflicts().len(), 1);
        assert_eq!(s.staged().len(), 5, "added, del, link, new, sub/s");
        assert_eq!(s.unstaged().len(), 4, "keep, sub/s, untracked, with space");
        assert_eq!(
            s.files().len(),
            8,
            "each path once (sub/s too), the conflict apart"
        );
        assert_eq!(s.entries[5].name(), "s.txt");
        assert_eq!(s.entries[5].folder(), "sub");
        assert_eq!(s.entries[2].folder(), "");
    }

    #[test]
    fn a_fresh_detached_or_upstreamless_branch_header() {
        let s = parse_status(b"# branch.oid (initial)\0# branch.head master\0");
        assert_eq!(s.oid, None, "no commits yet");
        assert_eq!(s.branch.as_deref(), Some("master"));
        assert_eq!(s.upstream, None);
        let s = parse_status(b"# branch.oid 3dba34cf7d5d\0# branch.head (detached)\0? u\0");
        assert_eq!(s.branch, None, "detached");
        assert_eq!(s.entries.len(), 1);
        assert_eq!(parse_status(b""), Status::default());
    }

    #[test]
    fn a_gone_upstream_has_no_counts() {
        // Its branch was deleted on the server and pruned: git prints the upstream, no branch.ab.
        let s = parse_status(
            b"# branch.oid 247fff93\0# branch.head feat\0# branch.upstream origin/feat\0",
        );
        assert_eq!(s.upstream.as_deref(), Some("origin/feat"));
        assert!(s.gone);
        let s = parse_status(b"# branch.oid 247fff93\0# branch.head feat\0# branch.upstream origin/feat\0# branch.ab +0 -0\0");
        assert!(!s.gone, "there and up to date");
        assert!(
            !parse_status(b"# branch.oid 247fff93\0# branch.head feat\0").gone,
            "no upstream at all"
        );
    }

    const LOG: &[u8] = b"c3b2616d56fb1b9ab991507a8167c200c190b9f2\x1fc3b2616\x1fAda Lovelace\x1fada@example.com\x1f1791755697\x1f2026-10-11 16:14\x1fc10c14198b74a38923beeeface7f41e6774f4288\x1ftag: v1, origin/feature, feature\x1fUpper two\x1fA body line.\n\nAnother.\n\0\
0d817e9ea557196af5794a0915ba6b96e6745223\x1f0d817e9\x1fAda Lovelace\x1fada@example.com\x1f1791755685\x1f2026-10-11 16:14\x1f\x1fHEAD -> main\x1fInitial commit\x1f\0";

    #[test]
    fn a_log_record_with_a_body_refs_and_no_parents() {
        let log = parse_log(LOG);
        assert_eq!(log.len(), 2);
        let c = &log[0];
        assert_eq!(c.short, "c3b2616");
        assert_eq!(c.author, "Ada Lovelace");
        assert_eq!(c.email, "ada@example.com");
        assert_eq!(c.time, 1791755697);
        assert_eq!(c.date, "2026-10-11 16:14");
        assert_eq!(c.parents, ["c10c14198b74a38923beeeface7f41e6774f4288"]);
        assert_eq!(c.refs, ["tag: v1", "origin/feature", "feature"]);
        assert_eq!(c.subject, "Upper two");
        assert_eq!(c.body, "A body line.\n\nAnother.");
        let root = &log[1];
        assert!(root.parents.is_empty(), "a root commit");
        assert_eq!(root.refs, ["HEAD -> main"]);
        assert_eq!(root.body, "");
        assert!(parse_log(b"").is_empty());
    }

    const REFS: &[u8] = b"refs/heads/feature\0feature\0c3b2616\0\0\0Upper two\0 \x001791755697\n\
refs/heads/main\0main\0a95aff3\0origin/main\0ahead 1, behind 2\0Local only\0*\x001791755600\n\
refs/heads/old\0old\0a95aff3\0origin/old\0gone\0Was pushed\0 \x001791755600\n\
refs/remotes/origin/HEAD\0origin/HEAD\0a95aff3\0\0\0Local only\0 \x001791755600\n\
refs/remotes/origin/main\0origin/main\0a95aff3\0\0\0Local only\0 \0\n";

    #[test]
    fn branch_lists_with_upstreams() {
        let b = parse_branches(REFS);
        let names: Vec<_> = b.iter().map(|b| (b.name.as_str(), b.remote)).collect();
        assert_eq!(
            names,
            [
                ("feature", false),
                ("main", false),
                ("old", false),
                ("origin/main", true)
            ],
            "origin/HEAD is left out"
        );
        assert!(b[1].current);
        assert_eq!(b[1].upstream.as_deref(), Some("origin/main"));
        assert_eq!((b[1].ahead, b[1].behind), (1, 2));
        assert!(b[2].gone);
        assert_eq!(b[0].upstream, None);
        assert_eq!(b[0].subject, "Upper two");
        assert_eq!(b[3].commit, "a95aff3");
        assert_eq!(b[0].time, 1791755697, "its last commit's date");
        assert_eq!(b[3].time, 0, "a date git left empty");
    }

    #[test]
    fn a_stash_list() {
        let s = parse_stashes(b"stash@{0}\x1f8f4ad1c2\x1f1791755697\x1fOn main: my work\0stash@{1}\x1f3dba34cf\x1f1791755600\x1fWIP on main: 3dba34c Add 2b\0");
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].index, s[0].time), (0, 1791755697));
        assert_eq!(
            s[0].hash, "8f4ad1c2",
            "who it is, whatever its index becomes"
        );
        assert_eq!(s[0].message, "On main: my work");
        assert_eq!(s[1].index, 1);
        assert!(parse_stashes(b"").is_empty());
    }

    #[test]
    fn a_commits_files_with_a_rename() {
        let f = parse_name_status(
            b"A\0added.txt\0D\0del.txt\0M\0keep.txt\0T\0link.txt\0R100\0old.txt\0new.txt\0",
        );
        let got: Vec<_> = f
            .iter()
            .map(|f| (f.change, f.path.as_str(), f.from.as_deref()))
            .collect();
        use Change::*;
        assert_eq!(
            got,
            [
                (Added, "added.txt", None),
                (Deleted, "del.txt", None),
                (Modified, "keep.txt", None),
                (TypeChanged, "link.txt", None),
                (Renamed, "new.txt", Some("old.txt")),
            ]
        );
    }

    #[test]
    fn stderr_that_means_a_prompt_a_diverged_pull_or_an_unmerged_branch() {
        assert!(needs_input(
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled\n"
        ));
        assert!(needs_input(
            "git@github.com: Permission denied (publickey).\n"
        ));
        assert!(needs_input("Host key verification failed.\n"));
        assert!(!needs_input("fatal: repository 'x' not found\n"));
        assert!(not_fast_forward(
            "hint: Disable this message with \"git config set advice.diverging false\"\nfatal: Not possible to fast-forward, aborting.\n"
        ));
        assert!(!not_fast_forward("Already up to date.\n"));
        assert!(not_merged(
            "error: the branch 'feature' is not fully merged\n"
        ));
    }

    #[test]
    fn how_long_ago() {
        let now = 1_800_000_000;
        assert_eq!(ago(now, now - 5), "just now");
        assert_eq!(ago(now, now - 90), "1 min ago");
        assert_eq!(ago(now, now - 3 * 3600), "3 h ago");
        assert_eq!(ago(now, now - 2 * 86_400), "2 d ago");
        assert_eq!(ago(now, now - 40 * 86_400), "1 mo ago");
        assert_eq!(ago(now, now - 800 * 86_400), "2 y ago");
        assert_eq!(ago(now, now + 60), "just now", "a clock ahead of ours");
    }

    #[test]
    fn letters_and_words() {
        assert_eq!(Change::Modified.letter(), 'M');
        assert_eq!(Change::Untracked.letter(), 'U');
        assert_eq!(Change::Conflict.letter(), 'C');
        assert_eq!(Change::TypeChanged.letter(), 'T');
        assert_eq!(Change::Renamed.word(), "renamed");
    }
}
