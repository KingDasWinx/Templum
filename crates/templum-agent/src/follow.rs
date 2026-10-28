//! A Claude Code subagent's transcript read as it grows, into the text its block on the canvas
//! shows (docs/specs/claude-subagents.md). It reads, never writes.

use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::ChatEvent;

/// The most characters of a tool call's summary, a result's line, or a title's description.
const WIDE: usize = 100;
const DIM: &str = "\x1b[2m";
const BOLD: &str = "\x1b[1m";
const CYAN: &str = "\x1b[36m";
const RED: &str = "\x1b[31m";
const RESET: &str = "\x1b[0m";

/// A subagent's transcript being followed: how far it was read, and what its block's title says.
pub struct Follower {
    path: PathBuf,
    /// Its type (`Explore`), from the hook that started it.
    kind: String,
    /// Bytes read so far; the end of a line not yet whole.
    at: u64,
    partial: Vec<u8>,
    /// What it was asked to do in a few words: its `meta.json`'s, else its task's first line.
    meta: Option<String>,
    task: Option<String>,
    /// Its task was printed: a later prompt is a message that resumed it.
    tasked: bool,
    /// Its last turn ended (`end_turn`), and nothing came since.
    done: bool,
    /// The title last written.
    title: String,
}

impl Follower {
    pub fn new(path: PathBuf, kind: &str) -> Self {
        Self {
            path,
            kind: clean(kind),
            at: 0,
            partial: Vec::new(),
            meta: None,
            task: None,
            tasked: false,
            done: false,
            title: String::new(),
        }
    }

    /// What is new since the last read, as the terminal shows it: each whole line written since,
    /// then the title (OSC 2) when it changed. Nothing while the file isn't there (it comes a
    /// moment after the subagent starts) or didn't grow.
    pub fn read(&mut self) -> String {
        let mut out = String::new();
        let mut bytes = Vec::new();
        let read = std::fs::File::open(&self.path).and_then(|mut file| {
            file.seek(SeekFrom::Start(self.at))?;
            file.read_to_end(&mut bytes)
        });
        if let Ok(n) = read {
            self.at += n as u64;
            self.partial.extend_from_slice(&bytes);
            // The last piece has no new line yet: it waits for the rest of its line.
            if let Some(end) = self.partial.iter().rposition(|b| *b == b'\n') {
                let whole: Vec<u8> = self.partial.drain(..=end).collect();
                for line in String::from_utf8_lossy(&whole).lines() {
                    out.push_str(&self.line(line));
                }
            }
        }
        if self.meta.is_none() {
            self.meta = meta_description(&self.path);
        }
        let title = self.title();
        if title != self.title {
            out.push_str(&format!("\x1b]2;{title}\x07"));
            self.title = title;
        }
        out
    }

    /// The block's title: its type and description, and "done" once its last turn ended.
    fn title(&self) -> String {
        let mut title = match &self.kind {
            kind if kind.is_empty() => "Subagent".to_string(),
            kind => kind.clone(),
        };
        if let Some(description) = self.meta.as_ref().or(self.task.as_ref()) {
            title.push_str(" · ");
            title.push_str(description);
        }
        if self.done {
            title.push_str(" · done");
        }
        title
    }

    /// One line of the transcript, as text: its task, a thought, its words, a tool's call and
    /// its result's first line, and a rule once its last turn ended. Nothing for the rest (the
    /// attachments Claude Code adds) or a line that isn't JSON.
    fn line(&mut self, line: &str) -> String {
        let Ok(json) = serde_json::from_str::<Value>(line) else {
            return String::new();
        };
        let message = &json["message"];
        let mut out = String::new();
        match json["type"].as_str() {
            Some("user") => {
                self.done = false;
                if let Some(prompt) = message["content"].as_str() {
                    let prompt = clean(prompt);
                    if self.tasked {
                        out.push_str(&format!("\n{BOLD}› {}{RESET}\n", prompt.trim()));
                    } else {
                        self.tasked = true;
                        let first = prompt.lines().find(|l| !l.trim().is_empty());
                        self.task = first.map(|l| cut(l.trim()));
                        out.push_str(&format!("{BOLD}Task{RESET}\n{}\n", prompt.trim()));
                    }
                }
            }
            Some("assistant") => {
                self.done = false;
                let mut blocks = message["content"].as_array().into_iter().flatten();
                if blocks.any(|b| b["type"] == "thinking") {
                    out.push_str(&format!("{DIM}thinking…{RESET}\n"));
                }
            }
            _ => return String::new(),
        }
        for event in crate::claude::parse(line) {
            match event {
                ChatEvent::Text(text) => out.push_str(&format!("\n{}\n", clean(text.trim()))),
                ChatEvent::ToolCall { name, input, .. } => out.push_str(&format!(
                    "{CYAN}●{RESET} {BOLD}{}{RESET}  {}\n",
                    clean(&name),
                    summary(&input)
                )),
                ChatEvent::ToolResult { text, is_error, .. } => {
                    let text = clean(&text);
                    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
                    let first = lines.next().map_or("(no output)".to_string(), cut);
                    let more = match lines.count() {
                        0 => String::new(),
                        n => format!(" {DIM}+{n} lines{RESET}"),
                    };
                    match is_error {
                        true => out.push_str(&format!("  {RED}✗ {first}{RESET}{more}\n")),
                        false => out.push_str(&format!("  {DIM}⎿ {first}{RESET}{more}\n")),
                    }
                }
                _ => {}
            }
        }
        if json["type"] == "assistant" && message["stop_reason"] == "end_turn" {
            self.done = true;
            out.push_str(&format!("\n{DIM}─ Done{RESET}\n"));
        }
        out
    }
}

/// The description in the `meta.json` Claude Code writes beside a subagent's transcript
/// (observed, not documented: none when it isn't there).
fn meta_description(transcript: &Path) -> Option<String> {
    let meta = std::fs::read(transcript.with_extension("meta.json")).ok()?;
    let meta: Value = serde_json::from_slice(&meta).ok()?;
    let description = clean(meta["description"].as_str()?);
    let description = description.lines().next()?.trim();
    (!description.is_empty()).then(|| cut(description))
}

/// A tool call in a few words: its command, its file, its pattern… else its input, compact.
fn summary(input: &Value) -> String {
    let named = [
        "command",
        "file_path",
        "notebook_path",
        "pattern",
        "path",
        "url",
        "query",
        "description",
        "prompt",
    ]
    .iter()
    .find_map(|key| input[*key].as_str());
    let text = match named {
        Some(text) => text.to_string(),
        None if input.as_object().is_some_and(|o| o.is_empty()) => String::new(),
        None => input.to_string(),
    };
    let text = clean(&text);
    cut(text.lines().next().unwrap_or_default().trim())
}

/// `text` without control characters but new lines and tabs: what the transcript holds came from
/// tools, files and pages, and an escape in it would drive the terminal (its title, its
/// clipboard).
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect()
}

/// `text`'s first WIDE characters, with "…" when it is longer.
fn cut(text: &str) -> String {
    match text.char_indices().nth(WIDE) {
        Some((at, _)) => format!("{}…", &text[..at]),
        None => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write as _;

    /// A transcript line of the shape Claude Code 2.1.286 wrote (docs/plans/
    /// 2026-10-22-claude-subagents.md, The probe), its content `content`.
    fn line(kind: &str, message: Value) -> String {
        let line = json!({ "type": kind, "isSidechain": true, "agentId": "a1",
            "sessionId": "s1", "message": message });
        format!("{line}\n")
    }

    fn append(path: &Path, text: &str) {
        let mut file = std::fs::File::options()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    /// `text` without its escape sequences: what a person reads.
    fn plain(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            match c {
                '\x1b' => match chars.next() {
                    // OSC: up to its BEL.
                    Some(']') => for _ in chars.by_ref().take_while(|c| *c != '\x07') {},
                    _ => for _ in chars.by_ref().take_while(|c| !c.is_ascii_alphabetic()) {},
                },
                c => out.push(c),
            }
        }
        out
    }

    #[test]
    fn a_transcript_reads_as_its_task_its_tools_and_its_report() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-a1.jsonl");
        let mut follower = Follower::new(path.clone(), "Explore");
        assert_eq!(
            follower.read(),
            "\x1b]2;Explore\x07",
            "no file yet: its type"
        );
        assert_eq!(follower.read(), "", "nothing new");
        append(
            &path,
            &line(
                "user",
                json!({ "role": "user", "content": "List the files.\nThen stop." }),
            ),
        );
        append(
            &path,
            "{\"type\":\"attachment\",\"attachment\":{\"type\":\"date\"}}\n",
        );
        append(
            &path,
            &line(
                "assistant",
                json!({ "role": "assistant", "content": [{ "type": "thinking", "thinking": "hm" }] }),
            ),
        );
        let call = json!({ "role": "assistant", "stop_reason": "tool_use", "content": [{
            "type": "tool_use", "id": "t1", "name": "Bash", "input": { "command": "ls -la" } }] });
        append(&path, &line("assistant", call));
        // Half a line: it waits for the rest.
        let result = line(
            "user",
            json!({ "role": "user", "content": [{ "type": "tool_result", "tool_use_id": "t1",
                "content": "a.txt\nb.txt\n" }] }),
        );
        let (head, tail) = result.split_at(20);
        append(&path, head);
        let shown = plain(&follower.read());
        assert!(
            shown.starts_with("Task\nList the files.\nThen stop.\n"),
            "{shown}"
        );
        assert!(shown.contains("thinking…\n● Bash  ls -la\n"), "{shown}");
        assert!(!shown.contains("a.txt"), "half a line isn't read: {shown}");
        assert!(!shown.contains("date"), "attachments say nothing: {shown}");
        assert!(
            follower.title.ends_with("Explore · List the files."),
            "its task's first line, as its description: {}",
            follower.title
        );
        append(&path, tail);
        let denied = json!({ "role": "user", "content": [{ "type": "tool_result",
            "tool_use_id": "t2", "is_error": true, "content": "Permission denied" }] });
        append(&path, &line("user", denied));
        let report = json!({ "role": "assistant", "stop_reason": "end_turn",
            "content": [{ "type": "text", "text": "Two files: a.txt, b.txt." }] });
        append(&path, &line("assistant", report));
        let read = follower.read();
        let shown = plain(&read);
        assert_eq!(
            shown,
            "  ⎿ a.txt +1 lines\n  ✗ Permission denied\n\nTwo files: a.txt, b.txt.\n\n─ Done\n",
        );
        assert!(
            read.ends_with("\x1b]2;Explore · List the files. · done\x07"),
            "{read:?}"
        );
        // Resumed: a message, and it works again.
        let again = line(
            "user",
            json!({ "role": "user", "content": "And the hidden ones?" }),
        );
        append(&path, &again);
        let read = follower.read();
        assert!(plain(&read).contains("› And the hidden ones?"), "{read:?}");
        assert!(
            read.ends_with("\x1b]2;Explore · List the files.\x07"),
            "{read:?}"
        );
    }

    #[test]
    fn its_meta_names_it_and_escapes_never_reach_the_terminal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-a2.jsonl");
        std::fs::write(
            dir.path().join("agent-a2.meta.json"),
            r#"{"agentType":"general-purpose","description":"Review the parser"}"#,
        )
        .unwrap();
        let output = "\x1b]52;c;cGF3bmVk\x07\x1b[2Jdone";
        let result = json!({ "role": "user", "content": [{ "type": "tool_result",
            "tool_use_id": "t1", "content": output }] });
        append(&path, &line("user", result));
        let mut follower = Follower::new(path, "general-purpose");
        let read = follower.read();
        assert!(read.contains("]52;c;cGF3bmVk"), "its text stays: {read:?}");
        assert!(
            !read.contains("\x1b]52") && !read.contains("\x1b[2J"),
            "but not as escapes: {read:?}"
        );
        assert!(
            read.ends_with("\x1b]2;general-purpose · Review the parser\x07"),
            "{read:?}"
        );
    }

    #[test]
    fn a_tool_call_is_said_in_a_few_words() {
        assert_eq!(
            summary(&json!({ "file_path": "/src/a.rs", "limit": 1 })),
            "/src/a.rs"
        );
        assert_eq!(
            summary(&json!({ "command": "cargo test\nmore" })),
            "cargo test"
        );
        assert_eq!(summary(&json!({})), "");
        assert_eq!(summary(&json!({ "n": 1 })), r#"{"n":1}"#);
        assert_eq!(cut(&"x".repeat(WIDE + 5)).chars().count(), WIDE + 1);
    }
}
