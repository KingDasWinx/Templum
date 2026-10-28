//! The notes: plain text, app-wide like the kanban boards, kept in `notes.json`. A note block
//! shows some of them as tabs; closing a tab or the block keeps the note, and only the Notes
//! dialog deletes one (docs/specs/cockpit.md, section 5).

use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::board::Label;
use crate::session::{self, Loaded};

/// The file format's version: raised only when an older Templum couldn't read the new files.
pub(crate) const VERSION: u32 = 1;
/// A name's longest length, in characters.
const TITLE_MAX: usize = 60;
/// What a new note is called until the person names it.
pub(crate) const UNTITLED: &str = "Untitled";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Notes {
    pub version: u32,
    /// The last id given out: never reused.
    #[serde(default)]
    pub next_id: u64,
    #[serde(default)]
    pub notes: Vec<Entry>,
    /// The last `rev` given out (kept in memory only: `stamp` seeds it at launch).
    #[serde(skip)]
    pub rev: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Entry {
    pub id: u64,
    /// Its name, on its tab, its block's title and in the Notes dialog: UNTITLED for a new note,
    /// whatever the person renamed it to. A file from before names has none: `load` reads it as
    /// the note's first line, which is what the note was called until then.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub text: String,
    /// Its card's tint, a post-it's color; none: the blocks' own.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::board::known_label"
    )]
    pub color: Option<Label>,
    /// When its text last changed, in seconds since the Unix epoch.
    #[serde(default)]
    pub changed: u64,
    /// Its text's revision: grows each time the text changes (and at each API edit of it), from
    /// the Unix time in milliseconds when Templum started, so a restarted Templum never reuses
    /// one. In memory only.
    #[serde(skip)]
    pub rev: u64,
    /// The workspace whose block last showed it, while no block does: what lets a share list and
    /// reopen the notes it closed. Only meant while no block shows the note.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_in: Option<String>,
}

impl Default for Notes {
    fn default() -> Self {
        Notes {
            version: VERSION,
            next_id: 0,
            notes: Vec::new(),
            rev: 0,
        }
    }
}

impl Notes {
    /// A new note, named by its first line with text (UNTITLED without text); returns its id.
    pub fn add(&mut self, text: String, color: Option<Label>, now: u64) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.notes.push(Entry {
            id,
            name: title(&text),
            text,
            color,
            changed: now,
            rev: self.rev,
            closed_in: None,
        });
        id
    }

    /// Seeds the revisions at launch with `seed` (the Unix time in ms): every note's, and the
    /// last given out.
    pub fn stamp(&mut self, seed: u64) {
        self.rev = seed;
        for note in &mut self.notes {
            note.rev = seed;
        }
    }

    /// Note `id` has a new revision whatever its text: a caller waiting for a newer one gets one.
    pub fn touch(&mut self, id: u64) {
        let next = self.rev + 1;
        if let Some(note) = self.get_mut(id) {
            note.rev = next;
            self.rev = next;
        }
    }

    /// No block shows note `id` any more, and it was last open in `workspace`.
    pub fn close_in(&mut self, id: u64, workspace: &str) {
        if let Some(note) = self.get_mut(id) {
            note.closed_in = Some(workspace.to_string());
        }
    }

    /// A block shows note `id` again: it is closed in no workspace, whatever it was last.
    pub fn opened(&mut self, id: u64) {
        if let Some(note) = self.get_mut(id) {
            note.closed_in = None;
        }
    }

    /// Workspace `workspace` is gone: its id may be given out again, so no note is closed in it.
    pub fn forget_workspace(&mut self, workspace: &str) {
        for note in &mut self.notes {
            if note.closed_in.as_deref() == Some(workspace) {
                note.closed_in = None;
            }
        }
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.notes.iter().find(|n| n.id == id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut Entry> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    /// Its text is `text` now; its time of change moves only when the text did.
    pub fn set_text(&mut self, id: u64, text: &str, now: u64) {
        let next = self.rev + 1;
        if let Some(note) = self.get_mut(id)
            && note.text != text
        {
            note.text = text.to_string();
            note.changed = now;
            note.rev = next;
            self.rev = next;
        }
    }

    /// Its name is `name` now: its first line with text, trimmed and cut as a title is; UNTITLED
    /// when blank. Its time of change stays: the text didn't change.
    pub fn rename(&mut self, id: u64, name: &str) {
        if let Some(note) = self.get_mut(id) {
            note.name = title(name);
        }
    }

    /// Note `id`'s name; UNTITLED when it is gone.
    pub fn name(&self, id: u64) -> &str {
        self.get(id).map_or(UNTITLED, |n| n.name.as_str())
    }

    pub fn set_color(&mut self, id: u64, color: Option<Label>) {
        if let Some(note) = self.get_mut(id) {
            note.color = color;
        }
    }

    pub fn remove(&mut self, id: u64) {
        self.notes.retain(|n| n.id != id);
    }

    /// A copy of note `id`, named alike, as a new note (a pasted note block's); None when it is
    /// gone.
    pub fn duplicate(&mut self, id: u64, now: u64) -> Option<u64> {
        let note = self.get(id)?.clone();
        let copy = self.add(note.text, note.color, now);
        if let Some(entry) = self.get_mut(copy) {
            entry.name = note.name;
        }
        Some(copy)
    }

    /// The notes whose name or text holds `query` (any case; every note when it is blank), the
    /// most recently changed first.
    pub fn find(&self, query: &str) -> Vec<&Entry> {
        let query = query.trim().to_lowercase();
        let mut found: Vec<&Entry> = self
            .notes
            .iter()
            .filter(|n| {
                query.is_empty()
                    || n.name.to_lowercase().contains(&query)
                    || n.text.to_lowercase().contains(&query)
            })
            .collect();
        found.sort_by_key(|n| std::cmp::Reverse((n.changed, n.id)));
        found
    }
}

impl Entry {
    /// No text, and never named: closing its last tab drops it, nothing lost.
    pub fn is_blank(&self) -> bool {
        self.text.trim().is_empty() && self.name == UNTITLED
    }
}

/// A name from `text`: its first line with text, trimmed, cut at TITLE_MAX characters; UNTITLED
/// when it has none.
pub(crate) fn title(text: &str) -> String {
    let Some(line) = text.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return UNTITLED.to_string();
    };
    match line.char_indices().nth(TITLE_MAX) {
        Some((cut, _)) => format!("{}…", line[..cut].trim_end()),
        None => line.to_string(),
    }
}

/// One change from `old` to `new`: the bytes `range` of `old` become `text`, the longest common
/// start and end of the two left out, both on character boundaries.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Splice {
    pub range: Range<usize>,
    pub text: String,
}

/// The change from `old` to `new` as one stretch; None when they are equal.
pub(crate) fn splice(old: &str, new: &str) -> Option<Splice> {
    if old == new {
        return None;
    }
    let (a, b) = (old.as_bytes(), new.as_bytes());
    let mut start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    while !old.is_char_boundary(start) || !new.is_char_boundary(start) {
        start -= 1;
    }
    // The common end is looked for only over what the common start left, so they never overlap.
    let mut same = a[start..]
        .iter()
        .rev()
        .zip(b[start..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    while same > 0
        && !(old.is_char_boundary(a.len() - same) && new.is_char_boundary(b.len() - same))
    {
        same -= 1;
    }
    Some(Splice {
        range: start..a.len() - same,
        text: new[start..b.len() - same].to_string(),
    })
}

/// `mine` and `home`, both made from `base`, as one text: their splices from `base` go together
/// when at least one byte of `base` lies between them; two insertions at the same place keep
/// home's first. None: they overlap or touch, a conflict.
pub(crate) fn merge(base: &str, mine: &str, home: &str) -> Option<String> {
    if mine == base {
        return Some(home.to_string());
    }
    if home == base || home == mine {
        return Some(mine.to_string());
    }
    let (a, b) = (splice(base, mine)?, splice(base, home)?);
    let (first, second) = if a.range.end < b.range.start {
        (&a, &b)
    } else if b.range.end < a.range.start {
        (&b, &a)
    } else if a.range.is_empty() && b.range.is_empty() && a.range.start == b.range.start {
        let at = a.range.start;
        return Some(format!(
            "{}{}{}{}",
            &base[..at],
            b.text,
            a.text,
            &base[at..]
        ));
    } else {
        return None;
    };
    Some(format!(
        "{}{}{}{}{}",
        &base[..first.range.start],
        first.text,
        &base[first.range.end..second.range.start],
        second.text,
        &base[second.range.end..]
    ))
}

/// Where `at`, a byte offset of the text before `splice`, is after it: before its range the same,
/// after it moved by its growth, inside it at the end of the new text. A caret at the start of
/// the range stays before what is put there.
pub(crate) fn shift(at: usize, splice: &Splice) -> usize {
    if at <= splice.range.start {
        at
    } else if at >= splice.range.end {
        at + splice.text.len() - splice.range.len()
    } else {
        splice.range.start + splice.text.len()
    }
}

/// Milliseconds since the Unix epoch: what the revisions start from at launch.
pub(crate) fn rev_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Seconds since the Unix epoch.
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How long ago `then` was, at `now` (both from `now()`): "Just now", "5 min ago", "3 h ago",
/// "2 days ago".
pub(crate) fn ago(then: u64, now: u64) -> String {
    let s = now.saturating_sub(then);
    match s {
        0..60 => "Just now".to_string(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86_400 => format!("{} h ago", s / 3600),
        86_400..172_800 => "Yesterday".to_string(),
        _ => format!("{} days ago", s / 86_400),
    }
}

pub(crate) fn path(dir: &Path) -> PathBuf {
    dir.join("notes.json")
}

pub(crate) fn load(dir: &Path) -> Loaded<Notes> {
    match session::read::<Notes>(&path(dir)) {
        Loaded::Saved(mut notes) => {
            // An edited file's next_id may lag behind its ids: never give one out twice.
            notes.next_id = notes
                .notes
                .iter()
                .map(|n| n.id)
                .fold(notes.next_id, u64::max);
            // A file from before names: each note keeps the name it had, its first line.
            for note in notes.notes.iter_mut().filter(|n| n.name.trim().is_empty()) {
                note.name = title(&note.text);
            }
            Loaded::Saved(notes)
        }
        other => other,
    }
}

/// Writes the notes; like canvases, a write older than the last one done is dropped.
pub(crate) fn save(dir: &Path, notes: &Notes, generation: u64) -> io::Result<()> {
    let text = serde_json::to_string_pretty(notes).map_err(io::Error::other)?;
    session::write_newest(&path(dir), &text, generation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_never_reused_and_a_copy_is_a_new_note() {
        let mut notes = Notes::default();
        let a = notes.add("a".into(), Some(Label::Green), 1);
        notes.remove(a);
        let b = notes.add("b".into(), None, 2);
        assert_ne!(a, b);
        let c = notes.duplicate(b, 3).unwrap();
        assert_ne!(c, b);
        assert_eq!(notes.get(c).unwrap().text, "b");
        assert_eq!(notes.duplicate(a, 4), None, "a deleted note has no copy");
    }

    #[test]
    fn a_change_moves_its_time_and_the_same_text_does_not() {
        let mut notes = Notes::default();
        let id = notes.add("x".into(), None, 10);
        notes.set_text(id, "x", 20);
        assert_eq!(notes.get(id).unwrap().changed, 10);
        notes.set_text(id, "y", 30);
        assert_eq!(notes.get(id).unwrap().changed, 30);
        assert_eq!(notes.get(id).unwrap().text, "y");
    }

    #[test]
    fn find_takes_any_case_newest_first() {
        let mut notes = Notes::default();
        let old = notes.add("Buy MILK".into(), None, 1);
        let new = notes.add("milk tea".into(), None, 5);
        notes.add("bread".into(), None, 9);
        let ids = |found: Vec<&Entry>| found.iter().map(|n| n.id).collect::<Vec<_>>();
        assert_eq!(ids(notes.find(" Milk ")), [new, old]);
        assert_eq!(notes.find("").len(), 3);
    }

    #[test]
    fn a_note_is_named_by_its_text_at_first_and_renamed_by_the_person() {
        let mut notes = Notes::default();
        let blank = notes.add(String::new(), None, 1);
        let written = notes.add("Plan\nmore".into(), None, 2);
        assert_eq!(notes.name(blank), UNTITLED);
        assert_eq!(
            notes.name(written),
            "Plan",
            "an agent's note: its first line"
        );
        // Typing changes the text, never the name.
        notes.set_text(blank, "milk", 3);
        assert_eq!(notes.name(blank), UNTITLED);
        notes.rename(blank, "  Groceries ");
        assert_eq!(notes.name(blank), "Groceries");
        assert_eq!(notes.get(blank).unwrap().text, "milk");
        assert_eq!(
            notes.get(blank).unwrap().changed,
            3,
            "a name isn't a text change"
        );
        notes.rename(blank, "   ");
        assert_eq!(notes.name(blank), UNTITLED, "blank: back to the default");
        notes.rename(blank, "One\nTwo");
        assert_eq!(notes.name(blank), "One", "one line");
        assert_eq!(notes.name(99), UNTITLED, "a deleted note");
        let copy = notes.duplicate(blank, 4).unwrap();
        assert_eq!(notes.name(copy), "One", "a copy is named alike");
    }

    #[test]
    fn find_matches_the_name_too_and_a_named_blank_note_is_not_blank() {
        let mut notes = Notes::default();
        let id = notes.add("milk".into(), None, 1);
        notes.rename(id, "Groceries");
        assert_eq!(notes.find("grocer").len(), 1, "by name");
        assert_eq!(notes.find("milk").len(), 1, "by text");
        let empty = notes.add(String::new(), None, 2);
        assert!(notes.get(empty).unwrap().is_blank());
        notes.rename(empty, "Ideas");
        assert!(!notes.get(empty).unwrap().is_blank(), "named by the person");
    }

    #[test]
    fn the_title_is_the_first_line_with_text() {
        assert_eq!(title("\n  Plan for Monday  \nmore"), "Plan for Monday");
        assert_eq!(title("# Not a heading"), "# Not a heading", "plain text");
        assert_eq!(title(" \n\t\n"), "Untitled");
        let long = "é".repeat(TITLE_MAX + 5);
        assert_eq!(title(&long), format!("{}…", "é".repeat(TITLE_MAX)));
    }

    #[test]
    fn ago_says_how_long() {
        assert_eq!(ago(100, 130), "Just now");
        assert_eq!(ago(0, 300), "5 min ago");
        assert_eq!(ago(0, 3 * 3600), "3 h ago");
        assert_eq!(ago(0, 90_000), "Yesterday");
        assert_eq!(ago(0, 3 * 86_400), "3 days ago");
        assert_eq!(ago(500, 100), "Just now", "a clock set back");
    }

    #[test]
    fn notes_read_back_equal_and_next_id_follows_the_ids() {
        let d = tempfile::tempdir().unwrap();
        let mut notes = Notes::default();
        notes.add("one".into(), Some(Label::Cyan), 7);
        save(d.path(), &notes, 1).unwrap();
        assert_eq!(load(d.path()), Loaded::Saved(notes));
        std::fs::write(
            path(d.path()),
            r#"{"version":1,"notes":[{"id":9,"text":"x","color":"plaid"}]}"#,
        )
        .unwrap();
        let Loaded::Saved(read) = load(d.path()) else {
            panic!("not read");
        };
        assert_eq!(read.next_id, 9, "missing: the largest id");
        assert_eq!(read.notes[0].color, None, "an unknown color is none");
    }

    /// A file from before names: each note is called what it was, its first line; a name the file
    /// has is kept even when the text says otherwise, and read back as it was written.
    #[test]
    fn a_file_from_before_names_keeps_every_notes_name() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            path(d.path()),
            r#"{"version":1,"next_id":3,"notes":[
                {"id":1,"text":"\n Plan for Monday \nmore"},
                {"id":2,"text":""},
                {"id":3,"name":"Mine","text":"Something else"}]}"#,
        )
        .unwrap();
        let Loaded::Saved(read) = load(d.path()) else {
            panic!("not read");
        };
        let names: Vec<_> = read.notes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["Plan for Monday", UNTITLED, "Mine"]);
        save(d.path(), &read, 1).unwrap();
        assert_eq!(load(d.path()), Loaded::Saved(read));
    }

    #[test]
    fn rev_moves_only_when_the_text_does_and_a_restart_seeds_it() {
        let mut notes = Notes::default();
        let id = notes.add("x".into(), None, 1);
        assert_eq!(notes.get(id).unwrap().rev, 0, "a new note bumps nothing");
        notes.stamp(1_000);
        assert_eq!((notes.rev, notes.get(id).unwrap().rev), (1_000, 1_000));
        let other = notes.add("y".into(), None, 2);
        assert_eq!(notes.get(other).unwrap().rev, 1_000, "add doesn't bump");
        notes.set_text(id, "x", 3);
        assert_eq!(notes.get(id).unwrap().rev, 1_000, "the same text");
        notes.set_text(id, "z", 3);
        assert_eq!(notes.get(id).unwrap().rev, 1_001);
        notes.set_text(other, "w", 3);
        assert_eq!(notes.get(other).unwrap().rev, 1_002, "never reused");
        notes.rename(id, "Name");
        notes.set_color(id, Some(Label::Red));
        assert_eq!(notes.get(id).unwrap().rev, 1_001, "a name isn't a text");
        notes.touch(id);
        assert_eq!(notes.get(id).unwrap().rev, 1_003, "touch: newer, same text");
        assert_eq!(notes.get(id).unwrap().text, "z");
    }

    #[test]
    fn a_note_is_closed_in_a_workspace_until_that_workspace_is_forgotten() {
        let d = tempfile::tempdir().unwrap();
        let mut notes = Notes::default();
        let (a, b, c) = (
            notes.add("a".into(), None, 1),
            notes.add("b".into(), None, 1),
            notes.add("c".into(), None, 1),
        );
        notes.close_in(a, "w2");
        notes.close_in(b, "w2");
        notes.close_in(c, "w3");
        save(d.path(), &notes, 1).unwrap();
        let Loaded::Saved(read) = load(d.path()) else {
            panic!("not read");
        };
        assert_eq!(read.get(a).unwrap().closed_in.as_deref(), Some("w2"));
        notes.forget_workspace("w2");
        assert_eq!(notes.get(a).unwrap().closed_in, None);
        assert_eq!(notes.get(b).unwrap().closed_in, None);
        assert_eq!(notes.get(c).unwrap().closed_in.as_deref(), Some("w3"));
        std::fs::write(
            path(d.path()),
            r#"{"version":1,"notes":[{"id":1,"text":"x"}]}"#,
        )
        .unwrap();
        let Loaded::Saved(older) = load(d.path()) else {
            panic!("not read");
        };
        assert_eq!(older.notes[0].closed_in, None, "an older file has none");
        let mut one = Notes::default();
        one.add("x".into(), None, 1);
        let json = serde_json::to_string(&one).unwrap();
        assert!(!json.contains("closed_in") && !json.contains("rev"));
    }

    fn sp(old: &str, new: &str) -> (std::ops::Range<usize>, String) {
        let s = splice(old, new).unwrap();
        (s.range, s.text)
    }

    #[test]
    fn a_splice_is_the_one_stretch_that_changed() {
        assert_eq!(splice("same", "same"), None);
        assert_eq!(sp("abc", "abXc"), (2..2, "X".into()));
        assert_eq!(sp("abcd", "ad"), (1..3, String::new()));
        assert_eq!(sp("", "new"), (0..0, "new".into()));
        assert_eq!(sp("aXa", "aYYa"), (1..2, "YY".into()));
        // The end is looked for past the start: "aa" -> "aaa" never overlaps.
        assert_eq!(sp("aa", "aaa"), (2..2, "a".into()));
        // Never inside a character: é is C3 A9, ã is C3 A3.
        assert_eq!(sp("é", "ã"), (0..2, "ã".into()));
        assert_eq!(sp("ação", "ações"), (3..6, "ões".into()));
    }

    #[test]
    fn a_splice_never_cuts_a_character() {
        for (old, new) in [
            ("ação", "aÇão"),
            ("日本", "日木"),
            ("日本語", "日語"),
            ("ação", "ações"),
        ] {
            let s = splice(old, new).unwrap();
            assert!(old.is_char_boundary(s.range.start) && old.is_char_boundary(s.range.end));
            let mut rebuilt = old.to_string();
            rebuilt.replace_range(s.range, &s.text);
            assert_eq!(rebuilt, new);
        }
    }

    #[test]
    fn disjoint_changes_merge_both_ways() {
        let base = "first\nsecond\nthird";
        let top = "zero\nfirst\nsecond\nthird";
        let end = "first\nsecond\nthird\nfourth";
        let both = "zero\nfirst\nsecond\nthird\nfourth";
        assert_eq!(merge(base, top, end).as_deref(), Some(both));
        assert_eq!(merge(base, end, top).as_deref(), Some(both));
        // A deletion and an insertion apart.
        let deleted = "first\nthird";
        let inserted = "first\nsecond\nthird!";
        assert_eq!(
            merge(base, deleted, inserted).as_deref(),
            Some("first\nthird!")
        );
        assert_eq!(
            merge(base, inserted, deleted).as_deref(),
            Some("first\nthird!")
        );
        // One side didn't change, or both made the same change.
        assert_eq!(merge(base, base, end).as_deref(), Some(end));
        assert_eq!(merge(base, end, base).as_deref(), Some(end));
        assert_eq!(merge(base, end, end).as_deref(), Some(end));
    }

    #[test]
    fn overlapping_and_touching_changes_are_a_conflict() {
        let base = "one two three";
        assert_eq!(
            merge(base, "one 2 three", "one TWO three"),
            None,
            "same words"
        );
        assert_eq!(
            merge(base, "one two", "one two three!"),
            None,
            "touch at the end"
        );
        assert_eq!(
            merge(base, "ONE two three", "one two three"),
            Some("ONE two three".into())
        );
        assert_eq!(
            merge(base, "oneX two three", "one Y two three").as_deref(),
            Some("oneX Y two three"),
            "one byte between is enough"
        );
        // Adjacent replacements share no byte between them.
        assert_eq!(merge("ab", "Xb", "aY"), None);
        // One byte between: fine.
        assert_eq!(merge("abc", "Xbc", "abY").as_deref(), Some("XbY"));
    }

    #[test]
    fn two_insertions_at_one_place_keep_home_s_first() {
        assert_eq!(merge("ab", "aXb", "aYb").as_deref(), Some("aYXb"));
        assert_eq!(merge("", "mine", "home").as_deref(), Some("homemine"));
    }

    #[test]
    fn a_merge_of_multi_byte_text_is_never_cut_inside_a_character() {
        assert_eq!(
            merge("ação 日本", "Aação 日本", "ação 日本語").as_deref(),
            Some("Aação 日本語")
        );
        assert_eq!(merge("日本", "日本語", "日本").as_deref(), Some("日本語"));
        assert_eq!(
            merge("ação x", "ações x", "ação y").as_deref(),
            Some("ações y")
        );
    }

    #[test]
    fn a_caret_moves_with_a_splice() {
        let s = splice("one two", "one zero two").unwrap(); // 4..4 "zero "
        assert_eq!(shift(2, &s), 2, "before");
        assert_eq!(shift(4, &s), 4, "at the start stays");
        assert_eq!(shift(5, &s), 10, "after");
        let s = splice("abcdef", "aXf").unwrap(); // 1..5 "X"
        assert_eq!(shift(3, &s), 2, "inside: the end of the new text");
        assert_eq!(shift(5, &s), 2, "after: by the growth");
        assert_eq!(shift(6, &s), 3);
    }
}
