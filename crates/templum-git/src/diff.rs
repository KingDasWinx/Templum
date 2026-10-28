//! `git diff` output, parsed: files, their hunks, their lines. Pure: tested on recorded output.
//! See docs/specs/git.md, section 2.

use serde::{Deserialize, Serialize};

use crate::parse::Change;

/// Past this many changed lines a file is folded by default (with Show diff).
pub const LARGE: u32 = 2000;

/// One file of a diff, as `git diff` prints it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDiff {
    /// The path before (a rename's old one); None for a new file.
    pub old_path: Option<String>,
    /// The path after; None for a deleted file.
    pub new_path: Option<String>,
    pub status: Change,
    pub binary: bool,
    /// `old mode 100644` → `new mode 100755`.
    pub mode: Option<(String, String)>,
    pub hunks: Vec<Hunk>,
    /// Its diff was longer than the limit: the hunks were dropped.
    pub too_large: bool,
    /// The working file's size and line count, counted by `Repo::working_diff`; None when the
    /// file is gone or wasn't counted.
    pub bytes: Option<u64>,
    pub lines: Option<u32>,
    /// An untracked file past what one read takes (`Repo::working_diff`): listed, not read,
    /// until asked for.
    pub unread: bool,
    /// Its hunks were dropped to fit an answer of the API's `git` tool (`ops::fit`).
    #[serde(default)]
    pub cut: bool,
}

impl FileDiff {
    pub fn empty() -> FileDiff {
        FileDiff {
            old_path: None,
            new_path: None,
            status: Change::Modified,
            binary: false,
            mode: None,
            hunks: Vec::new(),
            too_large: false,
            bytes: None,
            lines: None,
            unread: false,
            cut: false,
        }
    }

    /// The path now (the old one for a deleted file).
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or("")
    }

    fn count(&self, kind: Kind) -> u32 {
        self.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.kind == kind)
            .count() as u32
    }

    pub fn added(&self) -> u32 {
        self.count(Kind::Added)
    }

    pub fn removed(&self) -> u32 {
        self.count(Kind::Removed)
    }

    pub fn changed(&self) -> u32 {
        self.added() + self.removed()
    }

    /// A lock file, a minified file or a source map.
    pub fn generated(&self) -> bool {
        let name = self.path().rsplit('/').next().unwrap_or("");
        matches!(
            name,
            "Cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml"
        ) || name.ends_with(".min.js")
            || name.ends_with(".min.css")
            || name.ends_with(".map")
    }

    /// Folded by default, with Show diff: generated, or over `LARGE` changed lines.
    pub fn large(&self) -> bool {
        self.generated() || self.changed() > LARGE
    }

    /// A hunk of it can be staged, unstaged or discarded alone. Not a new or deleted file (its
    /// one hunk is the whole file: the file's buttons do it), nor a binary, a file too large or
    /// one not read; nor one whose text isn't UTF-8 (read with U+FFFD in place of its bytes, a
    /// patch would write those back: `git add` takes the whole file byte for byte instead);
    /// nor a conflicted file: its hunks are the parts of its regions; nor one with no added or
    /// removed line (a blame's view, all context, is never taken apart).
    pub fn by_hunk(&self) -> bool {
        self.old_path.is_some()
            && self.new_path.is_some()
            && !self.binary
            && !self.too_large
            && !self.unread
            && self.status != Change::Conflict
            && !self
                .hunks
                .iter()
                .flat_map(|h| &h.lines)
                .any(|l| l.text.contains('\u{FFFD}'))
            && self
                .hunks
                .iter()
                .flat_map(|h| &h.lines)
                .any(|l| l.kind != Kind::Context)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// What git names after `@@ … @@`: the function or heading, often empty.
    pub context: String,
    pub lines: Vec<Line>,
}

impl Hunk {
    /// Where it is: git's context, else "Lines 12–20" (the new side).
    pub fn title(&self) -> String {
        if !self.context.is_empty() {
            return self.context.clone();
        }
        // A deletion has no new lines: its old ones say where it was.
        let (start, len) = if self.new_len == 0 {
            (self.old_start, self.old_len)
        } else {
            (self.new_start, self.new_len)
        };
        let end = start + len.saturating_sub(1);
        if end > start {
            format!("Lines {start}–{end}")
        } else {
            format!("Line {start}")
        }
    }

    /// Removed lines paired with the added lines right after them, the i-th with the i-th:
    /// indices into `lines`. The pairs are what the changed words are computed on.
    pub fn pairs(&self) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        let mut i = 0;
        while i < self.lines.len() {
            if self.lines[i].kind != Kind::Removed {
                i += 1;
                continue;
            }
            let removed = i;
            while i < self.lines.len() && self.lines[i].kind == Kind::Removed {
                i += 1;
            }
            let added = i;
            while i < self.lines.len() && self.lines[i].kind == Kind::Added {
                i += 1;
            }
            for k in 0..(added - removed).min(i - added) {
                pairs.push((removed + k, added + k));
            }
        }
        pairs
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Context,
    Added,
    Removed,
}

impl Kind {
    /// The gutter's sign, and the line's first byte in a patch.
    pub fn sign(self) -> char {
        match self {
            Kind::Context => ' ',
            Kind::Added => '+',
            Kind::Removed => '-',
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub kind: Kind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    /// Without its leading sign and its '\n'; a CRLF file's lines keep their '\r'.
    pub text: String,
    /// "\ No newline at end of file" followed it.
    pub no_newline: bool,
}

/// `git diff --no-color` output into files. A file whose text is longer than `limit` bytes keeps
/// its headers and drops its hunks (`too_large`). Combined diffs (`diff --cc`, a conflicted
/// file) are skipped.
pub fn parse_diff(text: &str, limit: usize) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = Vec::new();
    // Where the open file's section started, for the limit; None while skipping.
    let mut start: Option<usize> = None;
    let mut offset = 0;
    // The next line numbers of the open hunk.
    let (mut old_next, mut new_next) = (0u32, 0u32);
    // What the open hunk's header says is left of it, old side and new side.
    let (mut old_left, mut new_left) = (0u32, 0u32);
    for raw in text.split_inclusive('\n') {
        let at = offset;
        offset += raw.len();
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix("diff --git ") {
            let (old, new) = git_header_paths(rest);
            let mut file = FileDiff::empty();
            file.old_path = Some(old);
            file.new_path = Some(new);
            files.push(file);
            start = Some(at);
            continue;
        }
        if line.starts_with("diff --cc ") || line.starts_with("diff --combined ") {
            start = None;
            continue;
        }
        let Some(from) = start else {
            continue;
        };
        let file = files.last_mut().expect("opened by diff --git");
        if offset - from > limit {
            file.hunks.clear();
            file.too_large = true;
            start = None;
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            if let Some(hunk) = parse_hunk_header(rest) {
                (old_next, new_next) = (hunk.old_start, hunk.new_start);
                (old_left, new_left) = (hunk.old_len, hunk.new_len);
                file.hunks.push(hunk);
            }
            continue;
        }
        if let Some(hunk) = file.hunks.last_mut() {
            // "\ No newline at end of file" follows the line it is about, the last one too.
            if line.starts_with('\\') {
                if let Some(last) = hunk.lines.last_mut() {
                    last.no_newline = true;
                }
                continue;
            }
            // The header's counts say where the hunk ends: what follows (a submodule's log with
            // diff.submodule=log) isn't in it.
            if old_left == 0 && new_left == 0 {
                continue;
            }
            let (kind, text) = match line.as_bytes().first() {
                Some(b' ') => (Kind::Context, &line[1..]),
                Some(b'+') => (Kind::Added, &line[1..]),
                Some(b'-') => (Kind::Removed, &line[1..]),
                // diff.suppressBlankEmpty prints an empty context line as nothing at all.
                None => (Kind::Context, ""),
                _ => continue,
            };
            let (old_no, new_no) = match kind {
                Kind::Context => (Some(old_next), Some(new_next)),
                Kind::Added => (None, Some(new_next)),
                Kind::Removed => (Some(old_next), None),
            };
            old_next += u32::from(old_no.is_some());
            new_next += u32::from(new_no.is_some());
            old_left = old_left.saturating_sub(u32::from(old_no.is_some()));
            new_left = new_left.saturating_sub(u32::from(new_no.is_some()));
            hunk.lines.push(Line {
                kind,
                old_no,
                new_no,
                text: text.to_string(),
                no_newline: false,
            });
            continue;
        }
        // The headers before the first hunk.
        if let Some(mode) = line.strip_prefix("old mode ") {
            file.mode = Some((mode.to_string(), String::new()));
        } else if let Some(mode) = line.strip_prefix("new mode ") {
            if let Some(m) = &mut file.mode {
                m.1 = mode.to_string();
            }
        } else if line.starts_with("new file mode ") {
            file.status = Change::Added;
            file.old_path = None;
        } else if line.starts_with("deleted file mode ") {
            file.status = Change::Deleted;
            file.new_path = None;
        } else if let Some(from) = line.strip_prefix("rename from ") {
            file.status = Change::Renamed;
            file.old_path = Some(unquoted(from));
        } else if let Some(to) = line.strip_prefix("rename to ") {
            file.new_path = Some(unquoted(to));
        } else if let Some(from) = line.strip_prefix("copy from ") {
            file.status = Change::Added;
            file.old_path = Some(unquoted(from));
        } else if let Some(to) = line.strip_prefix("copy to ") {
            file.new_path = Some(unquoted(to));
        } else if let Some(p) = line.strip_prefix("--- ") {
            if let Some(p) = named(p, "a/") {
                file.old_path = Some(p);
            }
        } else if let Some(p) = line.strip_prefix("+++ ") {
            if let Some(p) = named(p, "b/") {
                file.new_path = Some(p);
            }
        } else if line.starts_with("Binary files ") || line == "GIT binary patch" {
            file.binary = true;
        }
    }
    files
}

/// "a/x b/x" → ("x", "x"): the split where both halves agree (a name with spaces), else the
/// first " b/". A rename's halves are set again by its `rename from` and `rename to` lines.
fn git_header_paths(rest: &str) -> (String, String) {
    // A name with a quote, a backslash or a control character comes C-quoted, on either side.
    if let Some((a, b)) = quoted_halves(rest) {
        let strip = |s: String, p: &str| s.strip_prefix(p).map(str::to_string).unwrap_or(s);
        return (strip(a, "a/"), strip(b, "b/"));
    }
    let rest = rest.strip_prefix("a/").unwrap_or(rest);
    let mut first = None;
    for (i, _) in rest.match_indices(" b/") {
        let (left, right) = (&rest[..i], &rest[i + 3..]);
        if left == right {
            return (left.to_string(), right.to_string());
        }
        first.get_or_insert((left.to_string(), right.to_string()));
    }
    first.unwrap_or_else(|| (rest.to_string(), rest.to_string()))
}

/// A `---`/`+++` line's path: unquoted, without its prefix and the tab git adds after a name
/// with spaces; None for /dev/null.
fn named(p: &str, prefix: &str) -> Option<String> {
    unquoted(p.trim_end_matches('\t'))
        .strip_prefix(prefix)
        .map(str::to_string)
}

/// A name as git prints it, C-quoted (`"a/we\"ird.txt"`) when it holds a quote, a backslash
/// or a control character, even with core.quotePath off: as written. A name not in quotes
/// comes back as it is.
fn unquoted(s: &str) -> String {
    let Some(inner) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return s.to_string();
    };
    let mut bytes = Vec::with_capacity(inner.len());
    let mut it = inner.bytes().peekable();
    while let Some(b) = it.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match it.next() {
            Some(b'a') => bytes.push(7),
            Some(b'b') => bytes.push(8),
            Some(b't') => bytes.push(b'\t'),
            Some(b'n') => bytes.push(b'\n'),
            Some(b'v') => bytes.push(11),
            Some(b'f') => bytes.push(12),
            Some(b'r') => bytes.push(b'\r'),
            // \ooo: a byte in octal (a non-UTF-8 name, or quotePath on).
            Some(d @ b'0'..=b'7') => {
                let mut v = u32::from(d - b'0');
                for _ in 0..2 {
                    if let Some(&n @ b'0'..=b'7') = it.peek() {
                        v = v * 8 + u32::from(n - b'0');
                        it.next();
                    }
                }
                bytes.push(v as u8);
            }
            // \" and \\.
            Some(c) => bytes.push(c),
            None => {}
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The other way: a name as a patch must carry it for git to read it back.
fn quoted(s: &str) -> String {
    if !s.chars().any(|c| c == '"' || c == '\\' || c.is_control()) {
        return s.to_string();
    }
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if c.is_control() => {
                for b in c.to_string().bytes() {
                    out.push_str(&format!("\\{b:03o}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `"a/x" "b/x"`, `a/x "b/y"` or `"a/x" b/y`, unquoted; None when neither side is quoted.
fn quoted_halves(rest: &str) -> Option<(String, String)> {
    if rest.starts_with('"') {
        let end = closing_quote(rest)?;
        let (a, b) = rest.split_at(end + 1);
        return Some((unquoted(a), unquoted(b.trim_start())));
    }
    let i = rest.find(" \"b/")?;
    Some((rest[..i].to_string(), unquoted(&rest[i + 1..])))
}

/// The index of the quote that closes the one `s` starts with.
fn closing_quote(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut i = 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

/// "-10,3 +12,4 @@ fn x() {" (after "@@ "); a missing length is 1.
fn parse_hunk_header(rest: &str) -> Option<Hunk> {
    let rest = rest.strip_prefix('-')?;
    let (old, rest) = rest.split_once(" +")?;
    let (new, rest) = rest.split_once(" @@")?;
    let range = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let ((old_start, old_len), (new_start, new_len)) = (range(old)?, range(new)?);
    Some(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
        context: rest.trim().to_string(),
        lines: Vec::new(),
    })
}

/// Hunk `hunk` of `file` alone, as a patch `git apply` takes: on the index forward (stage), on
/// the index reversed (unstage), on the working tree reversed (discard). It names only the
/// file's current path, so a staged rename stays staged when one of its hunks is unstaged.
/// None when the file can't be taken apart by hunk (`FileDiff::by_hunk`) or has no such hunk.
pub fn hunk_patch(file: &FileDiff, hunk: usize) -> Option<String> {
    if !file.by_hunk() {
        return None;
    }
    let h = file.hunks.get(hunk)?;
    let path = file.new_path.as_deref()?;
    let (a, b) = (quoted(&format!("a/{path}")), quoted(&format!("b/{path}")));
    let mut out = format!(
        "diff --git {a} {b}\n--- {a}\n+++ {b}\n@@ -{},{} +{},{} @@\n",
        h.old_start, h.old_len, h.new_start, h.new_len
    );
    for l in &h.lines {
        out.push(l.kind.sign());
        out.push_str(&l.text);
        out.push('\n');
        if l.no_newline {
            out.push_str("\\ No newline at end of file\n");
        }
    }
    Some(out)
}

/// An untracked file as a new file's diff, so the panel shows it like any other (git has no
/// diff for it): every line added.
pub fn new_file_diff(path: &str, text: &str) -> String {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n",
        lines.len()
    );
    for line in lines {
        out.push('+');
        out.push_str(line);
        if !line.ends_with('\n') {
            out.push_str("\n\\ No newline at end of file\n");
        }
    }
    out
}

/// An untracked binary file as a new file's diff.
pub fn new_binary_diff(path: &str) -> String {
    format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\nBinary files /dev/null and b/{path} differ\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::Change;

    #[test]
    fn a_deletions_hunk_is_named_by_its_old_lines() {
        let text = "diff --git a/x b/x\ndeleted file mode 100644\n--- a/x\n+++ /dev/null\n@@ -12,3 +0,0 @@\n-a\n-b\n-c\n";
        assert_eq!(
            parse_diff(text, usize::MAX)[0].hunks[0].title(),
            "Lines 12–14"
        );
    }

    const MODIFIED: &str = "diff --git a/src/lib.rs b/src/lib.rs\n\
index 3b18e51..a1b2c3d 100644\n\
--- a/src/lib.rs\n\
+++ b/src/lib.rs\n\
@@ -1,4 +1,5 @@\n\
\x20fn one() {}\n\
-fn two() {}\n\
+fn two() { 2 }\n\
+fn three() {}\n\
\x20fn four() {}\n\
\x20fn five() {}\n\
@@ -20,3 +21,3 @@ fn twenty() {\n\
\x20a\n\
-b\n\
+c\n\
\x20d\n";

    #[test]
    fn a_modified_file_with_two_hunks() {
        let files = parse_diff(MODIFIED, usize::MAX);
        assert_eq!(files.len(), 1);
        let f = &files[0];
        assert_eq!(f.path(), "src/lib.rs");
        assert_eq!(f.old_path.as_deref(), Some("src/lib.rs"));
        assert_eq!(f.status, Change::Modified);
        assert!(!f.binary && !f.too_large);
        assert_eq!(f.hunks.len(), 2);
        let h = &f.hunks[0];
        assert_eq!(
            (h.old_start, h.old_len, h.new_start, h.new_len),
            (1, 4, 1, 5)
        );
        assert_eq!(h.context, "");
        assert_eq!(h.title(), "Lines 1–5");
        let got: Vec<_> = h
            .lines
            .iter()
            .map(|l| (l.kind, l.old_no, l.new_no, l.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (Kind::Context, Some(1), Some(1), "fn one() {}"),
                (Kind::Removed, Some(2), None, "fn two() {}"),
                (Kind::Added, None, Some(2), "fn two() { 2 }"),
                (Kind::Added, None, Some(3), "fn three() {}"),
                (Kind::Context, Some(3), Some(4), "fn four() {}"),
                (Kind::Context, Some(4), Some(5), "fn five() {}"),
            ]
        );
        assert_eq!(f.hunks[1].context, "fn twenty() {");
        assert_eq!(f.hunks[1].title(), "fn twenty() {");
        assert_eq!((f.added(), f.removed(), f.changed()), (3, 2, 5));
        assert_eq!(
            h.pairs(),
            [(1, 2)],
            "the first removed with the first added"
        );
    }

    #[test]
    fn pairs_match_removed_runs_with_the_added_run_after_them() {
        let text =
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,4 +1,3 @@\n-a\n-b\n+c\n d\n-e\n+f\n+g\n";
        let h = &parse_diff(text, usize::MAX)[0].hunks[0];
        assert_eq!(h.pairs(), [(0, 2), (4, 5)], "b has no partner; g has none");
        assert_eq!(h.title(), "Lines 1–3");
        let one = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n";
        let h = &parse_diff(one, usize::MAX)[0].hunks[0];
        assert_eq!(
            (h.old_start, h.old_len, h.new_start, h.new_len),
            (1, 1, 1, 1)
        );
        assert_eq!(h.title(), "Line 1");
    }

    #[test]
    fn a_rename_with_changes_and_a_pure_rename() {
        let text = "diff --git a/old.rs b/new.rs\n\
similarity index 90%\n\
rename from old.rs\n\
rename to new.rs\n\
index 1111111..2222222 100644\n\
--- a/old.rs\n\
+++ b/new.rs\n\
@@ -1,2 +1,2 @@\n\
\x20x\n\
-y\n\
+z\n\
diff --git a/a.txt b/b.txt\n\
similarity index 100%\n\
rename from a.txt\n\
rename to b.txt\n";
        let files = parse_diff(text, usize::MAX);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].status, Change::Renamed);
        assert_eq!(files[0].old_path.as_deref(), Some("old.rs"));
        assert_eq!(files[0].new_path.as_deref(), Some("new.rs"));
        assert_eq!(files[0].hunks.len(), 1);
        assert_eq!(files[1].status, Change::Renamed);
        assert_eq!(files[1].path(), "b.txt");
        assert!(files[1].hunks.is_empty(), "a pure rename");
    }

    #[test]
    fn binary_new_deleted_and_mode_change() {
        let text = "diff --git a/img.png b/img.png\n\
index 1111111..2222222 100644\n\
Binary files a/img.png and b/img.png differ\n\
diff --git a/new.txt b/new.txt\n\
new file mode 100644\n\
index 0000000..3bd1f0e\n\
--- /dev/null\n\
+++ b/new.txt\n\
@@ -0,0 +1,2 @@\n\
+foo\n\
+bar\n\
diff --git a/gone.txt b/gone.txt\n\
deleted file mode 100644\n\
index 3bd1f0e..0000000\n\
--- a/gone.txt\n\
+++ /dev/null\n\
@@ -1,2 +0,0 @@\n\
-foo\n\
-bar\n\
diff --git a/run.sh b/run.sh\n\
old mode 100644\n\
new mode 100755\n";
        let files = parse_diff(text, usize::MAX);
        assert_eq!(files.len(), 4);
        assert!(files[0].binary);
        assert_eq!(files[0].path(), "img.png");
        let new = &files[1];
        assert_eq!(new.status, Change::Added);
        assert_eq!(new.old_path, None);
        assert_eq!(new.path(), "new.txt");
        assert_eq!(new.hunks[0].lines[1].new_no, Some(2));
        let gone = &files[2];
        assert_eq!(gone.status, Change::Deleted);
        assert_eq!(gone.new_path, None);
        assert_eq!(gone.path(), "gone.txt");
        assert_eq!(gone.hunks[0].lines[1].old_no, Some(2));
        assert_eq!(
            files[3].mode,
            Some(("100644".to_string(), "100755".to_string()))
        );
        assert!(files[3].hunks.is_empty());
    }

    #[test]
    fn crlf_no_newline_and_a_name_with_spaces() {
        let text = "diff --git a/with space.txt b/with space.txt\n\
index 1111111..2222222 100644\n\
--- a/with space.txt\t\n\
+++ b/with space.txt\t\n\
@@ -1,2 +1,2 @@\n\
\x20a\r\n\
-b\r\n\
+c\r\n\
diff --git a/x b/x\n\
--- a/x\n\
+++ b/x\n\
@@ -1 +1 @@\n\
-x\n\
\\ No newline at end of file\n\
+y\n\
\\ No newline at end of file\n";
        let files = parse_diff(text, usize::MAX);
        assert_eq!(
            files[0].path(),
            "with space.txt",
            "the tab after the name is not part of it"
        );
        let lines = &files[0].hunks[0].lines;
        assert_eq!(lines[0].text, "a\r", "kept: batch 2's patches need it back");
        assert_eq!(lines[2].text, "c\r");
        let lines = &files[1].hunks[0].lines;
        assert!(lines[0].no_newline && lines[1].no_newline);
        assert_eq!(lines[1].text, "y");
    }

    #[test]
    fn a_combined_diff_is_skipped_and_a_huge_file_keeps_only_its_headers() {
        let text = "diff --cc c.txt\n\
index 1111111,2222222..0000000\n\
--- a/c.txt\n\
+++ b/c.txt\n\
@@@ -1,1 -1,1 +1,5 @@@\n\
++<<<<<<< HEAD\n\
\x20+ours\n\
++=======\n\
+ theirs\n\
++>>>>>>> topic\n\
diff --git a/ok.txt b/ok.txt\n\
--- a/ok.txt\n\
+++ b/ok.txt\n\
@@ -1 +1 @@\n\
-a\n\
+b\n";
        let files = parse_diff(text, usize::MAX);
        assert_eq!(files.len(), 1, "the conflicted file is left out");
        assert_eq!(files[0].path(), "ok.txt");
        let files = parse_diff(MODIFIED, 80);
        assert!(files[0].too_large, "longer than the limit");
        assert!(files[0].hunks.is_empty());
        assert_eq!(files[0].path(), "src/lib.rs");
    }

    #[test]
    fn an_untracked_file_as_a_new_files_diff() {
        let files = parse_diff(&new_file_diff("n.txt", "one\ntwo"), usize::MAX);
        let f = &files[0];
        assert_eq!(f.status, Change::Added);
        assert_eq!(f.path(), "n.txt");
        let h = &f.hunks[0];
        assert_eq!(
            (h.old_start, h.old_len, h.new_start, h.new_len),
            (0, 0, 1, 2)
        );
        assert_eq!(h.lines.len(), 2);
        assert_eq!(h.lines[1].text, "two");
        assert!(h.lines[1].no_newline && !h.lines[0].no_newline);
        let files = parse_diff(&new_binary_diff("b.bin"), usize::MAX);
        assert!(files[0].binary);
        assert_eq!(files[0].status, Change::Added);
        assert!(
            parse_diff(&new_file_diff("e.txt", ""), usize::MAX)[0].hunks[0]
                .lines
                .is_empty()
        );
    }

    #[test]
    fn large_and_generated_files() {
        let mut lock = FileDiff::empty();
        lock.new_path = Some("Cargo.lock".to_string());
        assert!(lock.generated() && lock.large());
        for name in [
            "app.min.js",
            "a/b/style.min.css",
            "x.map",
            "package-lock.json",
            "yarn.lock",
            "pnpm-lock.yaml",
        ] {
            lock.new_path = Some(name.to_string());
            assert!(lock.generated(), "{name}");
        }
        lock.new_path = Some("src/main.rs".to_string());
        assert!(!lock.generated() && !lock.large());
        let big = format!(
            "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,0 +1,{} @@\n{}",
            LARGE + 1,
            "+l\n".repeat(LARGE as usize + 1)
        );
        assert!(parse_diff(&big, usize::MAX)[0].large());
        assert_eq!(Kind::Added.sign(), '+');
        assert_eq!(Kind::Removed.sign(), '-');
        assert_eq!(Kind::Context.sign(), ' ');
    }

    #[test]
    fn a_hunk_ends_where_its_counts_say() {
        // diff.submodule=log prints a submodule's log after the file before it.
        let text = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n a\n-b\n+c\nSubmodule sub 1111111..2222222:\n  > two\n";
        let h = &parse_diff(text, usize::MAX)[0].hunks[0];
        assert_eq!(h.lines.len(), 3, "{:?}", h.lines);
        // diff.suppressBlankEmpty prints an empty context line as nothing at all.
        let text = "diff --git a/x b/x\n--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n\n-b\n+c\n";
        let h = &parse_diff(text, usize::MAX)[0].hunks[0];
        let got: Vec<_> = h
            .lines
            .iter()
            .map(|l| (l.kind, l.old_no, l.new_no, l.text.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                (Kind::Context, Some(1), Some(1), "a"),
                (Kind::Context, Some(2), Some(2), ""),
                (Kind::Removed, Some(3), None, "b"),
                (Kind::Added, None, Some(3), "c"),
            ]
        );
    }

    #[test]
    fn c_quoted_names_come_unquoted() {
        let text = "diff --git \"a/we\\\"ird.txt\" \"b/we\\\"ird.txt\"\nindex 1111111..2222222 100644\n--- \"a/we\\\"ird.txt\"\n+++ \"b/we\\\"ird.txt\"\n@@ -1 +1 @@\n-a\n+b\n\
diff --git \"a/tab\\there.txt\" \"b/tab\\there.txt\"\nnew file mode 100644\n--- /dev/null\n+++ \"b/tab\\there.txt\"\n@@ -0,0 +1 @@\n+x\n\
diff --git a/old.txt \"b/caf\\303\\251 \\\\.txt\"\nsimilarity index 100%\nrename from old.txt\nrename to \"caf\\303\\251 \\\\.txt\"\n";
        let files = parse_diff(text, usize::MAX);
        let paths: Vec<_> = files.iter().map(|f| f.path()).collect();
        assert_eq!(paths, ["we\"ird.txt", "tab\there.txt", "café \\.txt"]);
        assert_eq!(files[2].old_path.as_deref(), Some("old.txt"));
        assert_eq!(unquoted("\"a/x\\001y\""), "a/x\u{1}y", "octal");
        assert_eq!(unquoted("a/plain"), "a/plain");
    }

    #[test]
    fn a_hunks_patch_is_its_files_header_and_itself() {
        let files = parse_diff(MODIFIED, usize::MAX);
        assert!(files[0].by_hunk());
        assert_eq!(
            hunk_patch(&files[0], 1).unwrap(),
            "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -20,3 +21,3 @@\n a\n-b\n+c\n d\n"
        );
        assert_eq!(hunk_patch(&files[0], 2), None, "no third hunk");
    }

    #[test]
    fn a_renamed_file_patches_its_new_path_and_new_or_deleted_files_none() {
        let text = "diff --git a/old.rs b/new.rs\nsimilarity index 90%\nrename from old.rs\nrename to new.rs\n--- a/old.rs\n+++ b/new.rs\n@@ -1,2 +1,2 @@\n x\n-y\n+z\n";
        let renamed = &parse_diff(text, usize::MAX)[0];
        assert_eq!(
            hunk_patch(renamed, 0).unwrap(),
            "diff --git a/new.rs b/new.rs\n--- a/new.rs\n+++ b/new.rs\n@@ -1,2 +1,2 @@\n x\n-y\n+z\n"
        );
        let new = &parse_diff(&new_file_diff("n.txt", "a\n"), usize::MAX)[0];
        assert!(!new.by_hunk());
        assert_eq!(hunk_patch(new, 0), None, "its file's buttons do it");
        let gone = "diff --git a/g b/g\ndeleted file mode 100644\n--- a/g\n+++ /dev/null\n@@ -1 +0,0 @@\n-a\n";
        assert_eq!(hunk_patch(&parse_diff(gone, usize::MAX)[0], 0), None);
        let binary = "diff --git a/i.png b/i.png\nBinary files a/i.png and b/i.png differ\n";
        assert_eq!(hunk_patch(&parse_diff(binary, usize::MAX)[0], 0), None);
    }

    #[test]
    fn crlf_lines_a_missing_last_newline_and_a_quoted_name_go_back_as_they_came() {
        let text = "diff --git a/w.txt b/w.txt\n--- a/w.txt\n+++ b/w.txt\n@@ -1,2 +1,2 @@\n a\r\n-b\r\n+c\r\n\
diff --git \"a/we\\\"ird.txt\" \"b/we\\\"ird.txt\"\n--- \"a/we\\\"ird.txt\"\n+++ \"b/we\\\"ird.txt\"\n@@ -1 +1 @@\n-x\n\\ No newline at end of file\n+y\n\\ No newline at end of file\n";
        let files = parse_diff(text, usize::MAX);
        assert_eq!(
            hunk_patch(&files[0], 0).unwrap(),
            "diff --git a/w.txt b/w.txt\n--- a/w.txt\n+++ b/w.txt\n@@ -1,2 +1,2 @@\n a\r\n-b\r\n+c\r\n"
        );
        assert_eq!(
            hunk_patch(&files[1], 0).unwrap(),
            "diff --git \"a/we\\\"ird.txt\" \"b/we\\\"ird.txt\"\n--- \"a/we\\\"ird.txt\"\n+++ \"b/we\\\"ird.txt\"\n@@ -1,1 +1,1 @@\n-x\n\\ No newline at end of file\n+y\n\\ No newline at end of file\n"
        );
        assert_eq!(quoted("a/plain name.txt"), "a/plain name.txt");
        assert_eq!(quoted("a/tab\there.txt"), "\"a/tab\\there.txt\"");
        assert_eq!(unquoted(&quoted("a/x\u{1}y")), "a/x\u{1}y");
    }

    #[test]
    fn a_file_read_with_replaced_bytes_cant_go_by_hunk() {
        // A Latin-1 "café": the diff was read lossily, so a patch would write U+FFFD back.
        let text = "diff --git a/l.txt b/l.txt\n--- a/l.txt\n+++ b/l.txt\n@@ -1,2 +1,2 @@\n caf\u{FFFD}\n-b\n+B\n";
        let file = &parse_diff(text, usize::MAX)[0];
        assert!(!file.by_hunk());
        assert_eq!(
            hunk_patch(file, 0),
            None,
            "its file's buttons take it byte for byte"
        );
    }
}
