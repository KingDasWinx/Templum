//! Conflict regions in a file's text: what `<<<<<<<`, `|||||||`, `=======` and `>>>>>>>` mark,
//! each part named after its marker's label (never "ours" or "theirs": in a rebase those are
//! backwards), and the text with one region resolved. Pure: tested on text.
//! See docs/specs/git.md, section 5, Conflicts.

use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::diff::{FileDiff, Hunk, Kind, Line};
use crate::parse::Change;

/// One conflict region, as byte ranges into the file's text. A part's range holds its lines
/// with their endings, as the file has them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    /// From the `<<<<<<<` line's start to the end of the `>>>>>>>` line.
    pub whole: Range<usize>,
    /// The part after `<<<<<<<`: HEAD's in a merge and in a rebase alike (`checkout --ours`).
    pub first: Range<usize>,
    /// The common ancestor's part (`|||||||`, with diff3 or zdiff3): never shown.
    pub base: Option<Range<usize>>,
    /// The part after `=======` (`checkout --theirs`).
    pub second: Range<usize>,
    /// What follows `<<<<<<< ` ("HEAD") and `>>>>>>> ` ("feature", "a6caaa4 (feat)").
    pub first_label: String,
    pub second_label: String,
    /// The `<<<<<<<` line, from 1.
    pub line: u32,
}

impl Region {
    /// A part's lines as the panel shows them: without their '\n' (a CRLF line keeps its '\r',
    /// as the diff parser's lines do).
    pub fn lines(text: &str, part: &Range<usize>) -> Vec<String> {
        text[part.clone()]
            .split_inclusive('\n')
            .map(|l| l.strip_suffix('\n').unwrap_or(l).to_string())
            .collect()
    }
}

/// Which part of a region stays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Take {
    First,
    Second,
    /// The first part, then the second.
    Both,
}

/// A marker line's label: `c` seven times at the line's start, then nothing, or a space and
/// the label.
fn marker(line: &str, c: char) -> Option<String> {
    let rest = line.strip_prefix(&c.to_string().repeat(7))?;
    if rest.is_empty() {
        Some(String::new())
    } else {
        rest.strip_prefix(' ').map(str::to_string)
    }
}

/// The regions of `text`, in order. A region that never closes ends the list; a `<<<<<<<` inside
/// an open region starts over from it.
pub fn regions(text: &str) -> Vec<Region> {
    // Each line: its start, its end (after its ending), its text without the ending.
    let mut lines = Vec::new();
    let mut start = 0;
    for piece in text.split_inclusive('\n') {
        let end = start + piece.len();
        let bare = piece.strip_suffix('\n').unwrap_or(piece);
        lines.push((start, end, bare.strip_suffix('\r').unwrap_or(bare)));
        start = end;
    }
    let mut out = Vec::new();
    let mut i = 0;
    'regions: while i < lines.len() {
        let Some(first_label) = marker(lines[i].2, '<') else {
            i += 1;
            continue;
        };
        let open = i;
        let (mut base, mut mid, mut close) = (None, None, None);
        let mut j = open + 1;
        while j < lines.len() {
            let l = lines[j].2;
            if marker(l, '<').is_some() {
                i = j;
                continue 'regions;
            }
            if mid.is_none() && base.is_none() && marker(l, '|').is_some() {
                base = Some(j);
            } else if mid.is_none() && l == "=======" {
                mid = Some(j);
            } else if mid.is_some() && marker(l, '>').is_some() {
                close = Some(j);
                break;
            }
            j += 1;
        }
        let (Some(mid), Some(close)) = (mid, close) else {
            break;
        };
        let at = |k: usize| lines[k].0;
        let after = |k: usize| lines[k].1;
        out.push(Region {
            whole: at(open)..after(close),
            first: after(open)..at(base.unwrap_or(mid)),
            base: base.map(|b| after(b)..at(mid)),
            second: after(mid)..at(close),
            first_label,
            second_label: marker(lines[close].2, '>').unwrap_or_default(),
            line: open as u32 + 1,
        });
        i = close + 1;
    }
    out
}

/// `text` with region `index` replaced by what `take` keeps; None when there is no such region.
pub fn resolve(text: &str, index: usize, take: Take) -> Option<String> {
    let r = regions(text).into_iter().nth(index)?;
    let kept = match take {
        Take::First => &text[r.first.clone()],
        Take::Second => &text[r.second.clone()],
        Take::Both => {
            return Some(format!(
                "{}{}{}{}",
                &text[..r.whole.start],
                &text[r.first.clone()],
                &text[r.second.clone()],
                &text[r.whole.end..]
            ));
        }
    };
    Some(format!(
        "{}{kept}{}",
        &text[..r.whole.start],
        &text[r.whole.end..]
    ))
}

/// A conflicted file as the panel shows it: region k's parts are hunks 2k and 2k+1, their lines
/// context lines numbered as in the file, each hunk's context its part's label. With no region
/// left there is no hunk (the file can be marked resolved). None for a text that isn't UTF-8:
/// its row alone shows, with the whole-file choices.
pub fn conflict_diff(path: &str, bytes: &[u8]) -> Option<FileDiff> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut file = FileDiff::empty();
    file.old_path = Some(path.to_string());
    file.new_path = Some(path.to_string());
    file.status = Change::Conflict;
    for r in regions(text) {
        let count = |part: &Range<usize>| text[part.clone()].split_inclusive('\n').count() as u32;
        let first_at = r.line + 1;
        // The ||||||| line and the base part, then the ======= line.
        let base = r.base.as_ref().map_or(0, |b| count(b) + 1);
        let second_at = first_at + count(&r.first) + base + 1;
        for (part, label, at) in [
            (&r.first, &r.first_label, first_at),
            (&r.second, &r.second_label, second_at),
        ] {
            let lines: Vec<Line> = Region::lines(text, part)
                .into_iter()
                .enumerate()
                .map(|(k, text)| Line {
                    kind: Kind::Context,
                    old_no: None,
                    new_no: Some(at + k as u32),
                    text,
                    no_newline: false,
                })
                .collect();
            file.hunks.push(Hunk {
                old_start: 0,
                old_len: 0,
                new_start: at,
                new_len: lines.len() as u32,
                context: label.clone(),
                lines,
            });
        }
    }
    Some(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO_WAY: &str = "a\n<<<<<<< HEAD\nB-main\n=======\nB-feature\n>>>>>>> feature\nc\n";

    #[test]
    fn a_two_way_region_and_its_labels() {
        let r = regions(TWO_WAY);
        assert_eq!(r.len(), 1);
        let r = &r[0];
        assert_eq!(
            (r.first_label.as_str(), r.second_label.as_str()),
            ("HEAD", "feature")
        );
        assert_eq!(&TWO_WAY[r.first.clone()], "B-main\n");
        assert_eq!(&TWO_WAY[r.second.clone()], "B-feature\n");
        assert_eq!(r.base, None);
        assert_eq!(r.line, 2);
        assert_eq!(
            &TWO_WAY[r.whole.clone()],
            "<<<<<<< HEAD\nB-main\n=======\nB-feature\n>>>>>>> feature\n"
        );
    }

    #[test]
    fn each_resolution() {
        assert_eq!(resolve(TWO_WAY, 0, Take::First).unwrap(), "a\nB-main\nc\n");
        assert_eq!(
            resolve(TWO_WAY, 0, Take::Second).unwrap(),
            "a\nB-feature\nc\n"
        );
        assert_eq!(
            resolve(TWO_WAY, 0, Take::Both).unwrap(),
            "a\nB-main\nB-feature\nc\n"
        );
        assert_eq!(resolve(TWO_WAY, 1, Take::First), None, "no second region");
    }

    #[test]
    fn a_rebase_names_the_commit_being_applied() {
        let text = "<<<<<<< HEAD\nx\n=======\ny\n>>>>>>> a6caaa4 (feat)\n";
        assert_eq!(regions(text)[0].second_label, "a6caaa4 (feat)");
    }

    #[test]
    fn crlf_and_diff3_regions_resolve_byte_for_byte() {
        let text = "a\r\n<<<<<<< HEAD\r\nmain\r\n||||||| 63b4097\r\nbase\r\n=======\r\nfeature\r\n>>>>>>> feature\r\nc\r\n";
        let r = &regions(text)[0];
        assert_eq!(&text[r.first.clone()], "main\r\n");
        assert_eq!(&text[r.base.clone().unwrap()], "base\r\n");
        assert_eq!(&text[r.second.clone()], "feature\r\n");
        assert_eq!(
            resolve(text, 0, Take::Both).unwrap(),
            "a\r\nmain\r\nfeature\r\nc\r\n"
        );
        assert_eq!(
            Region::lines(text, &r.first),
            ["main\r"],
            "the panel's lines keep their \\r"
        );
        // The base part is never shown; the second part starts after it.
        let d = conflict_diff("f", text.as_bytes()).unwrap();
        assert_eq!(d.hunks.len(), 2);
        assert_eq!(d.hunks[0].lines[0].new_no, Some(3));
        assert_eq!(
            d.hunks[1].lines[0].new_no,
            Some(7),
            "after |||||||, base and ======="
        );
    }

    #[test]
    fn several_regions_and_an_unfinished_one() {
        let text = "<<<<<<< HEAD\n1\n=======\n2\n>>>>>>> b\nmid\n<<<<<<< HEAD\n3\n4\n=======\n>>>>>>> b\n<<<<<<< HEAD\nno end\n";
        let r = regions(text);
        assert_eq!(r.len(), 2, "the last one never closes");
        assert_eq!(&text[r[1].second.clone()], "", "an empty part");
        let once = resolve(text, 0, Take::Second).unwrap();
        assert_eq!(regions(&once).len(), 1, "the other region stays");
        assert!(once.starts_with("2\nmid\n"));
    }

    #[test]
    fn the_panel_view_of_a_conflicted_file() {
        let d = conflict_diff("src/x.rs", TWO_WAY.as_bytes()).unwrap();
        assert_eq!(d.status, crate::Change::Conflict);
        assert_eq!(d.path(), "src/x.rs");
        assert_eq!(d.hunks.len(), 2);
        assert_eq!(
            (d.hunks[0].context.as_str(), d.hunks[1].context.as_str()),
            ("HEAD", "feature")
        );
        assert_eq!(d.hunks[0].lines[0].text, "B-main");
        assert_eq!(d.hunks[0].lines[0].new_no, Some(3));
        assert_eq!(d.hunks[1].lines[0].new_no, Some(5));
        assert!(
            d.hunks
                .iter()
                .flat_map(|h| &h.lines)
                .all(|l| l.kind == crate::Kind::Context)
        );
        assert!(!d.by_hunk(), "never staged or discarded by hunk");
        let none = conflict_diff("f", b"resolved\n").unwrap();
        assert!(none.hunks.is_empty(), "no region left");
        assert!(conflict_diff("f", &[0xff, 0xfe]).is_none(), "not UTF-8");
    }
}
