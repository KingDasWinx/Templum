//! `git blame --porcelain`, parsed, and as the diff panel shows it: one group of rows per run
//! of lines from one commit, under a heading naming it. See docs/specs/git.md, section 6, Blame.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::diff::{FileDiff, Hunk, Kind, Line};
use crate::parse::ago;

/// One line of the file and the commit that last changed it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    /// Forty (or sixty-four) zeros for a line not committed yet.
    pub hash: String,
    pub author: String,
    /// Unix seconds.
    pub time: i64,
    pub summary: String,
    /// As the file has it, without its '\n' (a CRLF line keeps its '\r').
    pub text: String,
}

/// The porcelain format: a header `<hash> <orig> <final> [<count>]`, the commit's details the
/// first time it shows (`author`, `author-time`, `summary`, …), then the line after a TAB.
pub fn parse_blame(out: &[u8]) -> Vec<BlameLine> {
    let text = String::from_utf8_lossy(out);
    let mut known: HashMap<String, (String, i64, String)> = HashMap::new();
    let mut hash = String::new();
    let mut lines = Vec::new();
    for l in text.split('\n') {
        if let Some(content) = l.strip_prefix('\t') {
            let (author, time, summary) = known.get(&hash).cloned().unwrap_or_default();
            lines.push(BlameLine {
                hash: hash.clone(),
                author,
                time,
                summary,
                text: content.to_string(),
            });
            continue;
        }
        let Some((key, value)) = l.split_once(' ') else {
            continue;
        };
        match key {
            "author" => known.entry(hash.clone()).or_default().0 = value.to_string(),
            "author-time" => known.entry(hash.clone()).or_default().1 = value.parse().unwrap_or(0),
            "summary" => known.entry(hash.clone()).or_default().2 = value.to_string(),
            k if k.len() >= 40 && k.bytes().all(|b| b.is_ascii_hexdigit()) => hash = k.to_string(),
            _ => {}
        }
    }
    lines
}

/// The panel's view of a blame: a hunk per run of lines from one commit, its heading
/// "a1b2c3d · Ada Lovelace · 3 d ago · Subject", its lines numbered as in the file; and each
/// hunk's commit, for a click on its heading.
pub fn blame_diff(path: &str, lines: &[BlameLine], now: i64) -> (FileDiff, Vec<String>) {
    let mut file = FileDiff::empty();
    file.old_path = Some(path.to_string());
    file.new_path = Some(path.to_string());
    let mut hashes: Vec<String> = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let n = i as u32 + 1;
        if hashes.last() != Some(&l.hash) {
            let heading = if l.hash.bytes().all(|b| b == b'0') {
                "Not committed yet".to_string()
            } else {
                format!(
                    "{} · {} · {} · {}",
                    &l.hash[..7],
                    l.author,
                    ago(now, l.time),
                    l.summary
                )
            };
            file.hunks.push(Hunk {
                old_start: 0,
                old_len: 0,
                new_start: n,
                new_len: 0,
                context: heading,
                lines: Vec::new(),
            });
            hashes.push(l.hash.clone());
        }
        let h = file.hunks.last_mut().expect("a hunk was just made");
        h.lines.push(Line {
            kind: Kind::Context,
            old_no: None,
            new_no: Some(n),
            text: l.text.clone(),
            no_newline: false,
        });
        h.new_len += 1;
    }
    (file, hashes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZEROS: &str = "0000000000000000000000000000000000000000";

    fn porcelain() -> Vec<u8> {
        format!(
            "8ce28ba23e0873643eff7811d0d0a5bee855ad97 1 1 2\n\
author Ada Lovelace\nauthor-mail <ada@example.com>\nauthor-time 1791894925\nauthor-tz -0300\n\
committer Ada Lovelace\ncommitter-mail <ada@example.com>\ncommitter-time 1791894925\ncommitter-tz -0300\n\
summary add a\nboundary\nfilename a.txt\n\tone\n\
8ce28ba23e0873643eff7811d0d0a5bee855ad97 2 2\n\ttwo\twith a tab\r\n\
{ZEROS} 3 3 1\nauthor Not Committed Yet\nauthor-mail <not.committed.yet>\nauthor-time 1791895000\n\
author-tz -0300\nsummary Version of a.txt from a.txt\nfilename a.txt\n\tthree\n\
8ce28ba23e0873643eff7811d0d0a5bee855ad97 3 4 1\n\tfour\n"
        )
        .into_bytes()
    }

    #[test]
    fn porcelain_with_an_uncommitted_line_and_a_crlf_line() {
        let lines = parse_blame(&porcelain());
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[0].author, "Ada Lovelace");
        assert_eq!(lines[0].time, 1791894925);
        assert_eq!(lines[0].summary, "add a");
        assert_eq!(
            lines[1].text, "two\twith a tab\r",
            "the line as the file has it"
        );
        assert_eq!(
            lines[1].author, "Ada Lovelace",
            "a commit's details are given once"
        );
        assert_eq!(lines[2].hash, ZEROS);
        assert_eq!(lines[3].hash, lines[0].hash);
    }

    #[test]
    fn groups_of_lines_become_hunks() {
        let (d, hashes) = blame_diff("a.txt", &parse_blame(&porcelain()), 1791894925 + 3 * 86_400);
        assert_eq!(d.hunks.len(), 3, "one, two | three | four");
        assert_eq!(hashes.len(), 3);
        assert_eq!(
            d.hunks[0].context,
            "8ce28ba · Ada Lovelace · 3 d ago · add a"
        );
        assert_eq!(d.hunks[0].lines.len(), 2);
        assert_eq!(d.hunks[1].context, "Not committed yet");
        assert_eq!(d.hunks[2].lines[0].new_no, Some(4));
        assert!(
            d.hunks
                .iter()
                .flat_map(|h| &h.lines)
                .all(|l| l.kind == crate::Kind::Context)
        );
        assert_eq!(d.path(), "a.txt");
        assert!(!d.by_hunk(), "never staged from a blame");
    }
}
