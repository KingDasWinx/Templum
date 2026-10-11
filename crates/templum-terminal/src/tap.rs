//! Hears what shells and agents tell the terminal in sequences vte drops: OSC 133 (a prompt
//! starts, a command starts and ends with its exit code), OSC 7 and 9;9 (the current folder),
//! OSC 9 and 777 (notifications). The bytes reach vte unchanged, with one addition: from a prompt's
//! start to its command's, a hyperlink of ours (`templum:cmd/<n>`), which marks the prompt's
//! cells in the grid. The mark then moves with them: scrolling, the history limit, resizing.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{OnResize, WindowSize};
use alacritty_terminal::grid::{Dimensions, Grid};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::tty::{ChildEvent, EventedPty, EventedReadWrite};
use polling::{Event, PollMode, Poller};

use crate::terminal::Shared;

/// What the terminal heard about its commands and its need for attention.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    /// Since when the current command runs (OSC 133;C), while one does.
    pub running: Option<Instant>,
    /// How many commands finished (OSC 133;D after a C).
    pub finished: u64,
    /// The last one to finish.
    pub last: Option<Outcome>,
    /// How many times the terminal asked for attention: a bell, or an OSC 9 or 777 notification.
    pub attention: u64,
    /// The last notification's text; None after a bell.
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Outcome {
    /// None: the shell didn't say.
    pub code: Option<i32>,
    pub took: Duration,
}

/// A command's state, by the mark on its prompt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Running,
    /// With its exit code; None: the shell didn't say.
    Ended(Option<i32>),
}

/// Our hyperlinks' prefix; the prompt's number follows.
const URI: &str = "templum:cmd/";
/// Ends a hyperlink.
const CLOSE: &[u8] = b"\x1b]8;;\x1b\\";

/// The number of the prompt `cell` belongs to.
pub(crate) fn mark_of(cell: &Cell) -> Option<u32> {
    cell.extra.as_ref()?;
    cell.hyperlink()?.uri().strip_prefix(URI)?.parse().ok()
}

/// The number of the prompt with a cell on `line`.
pub(crate) fn line_mark(grid: &Grid<Cell>, line: Line) -> Option<u32> {
    let row = &grid[line];
    (0..grid.columns()).find_map(|c| mark_of(&row[Column(c)]))
}

/// The lines where prompts start, top to bottom (history included): a line marked with another
/// prompt than the line above.
pub(crate) fn prompt_lines(grid: &Grid<Cell>) -> Vec<i32> {
    let mut above = None;
    let mut starts = Vec::new();
    for line in -(grid.history_size() as i32)..grid.screen_lines() as i32 {
        let mark = line_mark(grid, Line(line));
        if mark.is_some() && mark != above {
            starts.push(line);
        }
        above = mark;
    }
    starts
}

/// The longest sequence of ours kept; longer ones are someone else's, or garbage.
const MAX_BODY: usize = 4096;
/// Notification texts are cut there.
const MAX_MESSAGE: usize = 200;

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Ground,
    /// After ESC.
    Esc,
    /// In an OSC, collecting its body.
    Osc,
    /// In an OSC that isn't ours: waiting for its end.
    Skip,
}

/// Finds our OSC sequences in a byte stream that may split them anywhere. It ends an OSC where
/// vte does: BEL, CAN, SUB or ESC (ST is ESC \).
pub(crate) struct Scanner {
    state: State,
    body: Vec<u8>,
    /// The next prompt's number.
    next: u32,
    /// The prompt being typed at (its hyperlink is open), and the command it started.
    prompt: Option<u32>,
    command: Option<u32>,
    /// No line feed since the open prompt started: another A now is the same prompt's, from a
    /// second integration (fish 4 marks its prompts itself, and so do some prompt themes).
    same_line: bool,
    /// Bytes to add after the ST that ends a sequence of ours, once its `\` is seen.
    after_st: Option<Vec<u8>>,
}

/// Bytes to add to the stream, each at an offset of the scanned bytes.
pub(crate) type Inserts = Vec<(usize, Vec<u8>)>;

impl Scanner {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            body: Vec::new(),
            next: 0,
            prompt: None,
            command: None,
            same_line: false,
            after_st: None,
        }
    }

    /// Hears `bytes`; returns what to add to them (see [`rewrite`]).
    pub fn scan(&mut self, bytes: &[u8], shared: &Shared) -> Inserts {
        let mut inserts = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            match self.state {
                // Most output: a single pass to the next ESC.
                State::Ground => {
                    let at = bytes[i..].iter().position(|&b| b == 0x1b);
                    let end = at.map_or(bytes.len(), |at| i + at);
                    if self.same_line && bytes[i..end].contains(&b'\n') {
                        self.same_line = false;
                    }
                    if at.is_none() {
                        return inserts;
                    }
                    self.state = State::Esc;
                    i = end;
                }
                State::Esc => {
                    // After a sequence of ours ended by ESC: only a whole ST gets its addition.
                    if let Some(insert) = self.after_st.take()
                        && bytes[i] == b'\\'
                    {
                        inserts.push((i + 1, insert));
                    }
                    if bytes[i] == b']' {
                        self.body.clear();
                        self.state = State::Osc;
                    } else if bytes[i] != 0x1b {
                        self.state = State::Ground;
                    }
                }
                State::Osc | State::Skip => match bytes[i] {
                    0x07 | 0x18 | 0x1a | 0x1b => {
                        if self.state == State::Osc {
                            let body = std::mem::take(&mut self.body);
                            if let Some(insert) = self.dispatch(&body, shared) {
                                if bytes[i] == 0x1b {
                                    self.after_st = Some(insert);
                                } else {
                                    inserts.push((i + 1, insert));
                                }
                            }
                            self.body = body;
                        }
                        self.state = if bytes[i] == 0x1b {
                            State::Esc
                        } else {
                            State::Ground
                        };
                    }
                    // vte ignores the other C0 controls inside an OSC.
                    0x00..=0x1f => {}
                    b if self.state == State::Osc => {
                        self.body.push(b);
                        if !could_be_ours(&self.body) {
                            self.state = State::Skip;
                        }
                    }
                    _ => {}
                },
            }
            i += 1;
        }
        inserts
    }

    /// Acts on one sequence; returns the bytes to add after it.
    fn dispatch(&mut self, body: &[u8], shared: &Shared) -> Option<Vec<u8>> {
        let text = String::from_utf8_lossy(body);
        let (number, rest) = text.split_once(';')?;
        match number {
            "133" => match rest.as_bytes().first() {
                // A prompt starts: its cells, and the command typed after it, get its mark.
                Some(b'A') if self.prompt.is_some() && self.same_line => {}
                Some(b'A') => {
                    self.same_line = true;
                    let id = self.next;
                    self.next = self.next.wrapping_add(1);
                    self.prompt = Some(id);
                    return Some(format!("\x1b]8;;{URI}{id}\x1b\\").into_bytes());
                }
                Some(b'C') => {
                    let mut status = shared.status.lock().unwrap();
                    // Another integration's C for the same command.
                    if status.running.is_some() {
                        return None;
                    }
                    status.running = Some(Instant::now());
                    drop(status);
                    self.command = self.prompt.take();
                    if let Some(id) = self.command {
                        // ponytail: kept for the session, a few bytes per command.
                        shared.marks.lock().unwrap().insert(id, Mark::Running);
                        return Some(CLOSE.to_vec());
                    }
                }
                Some(b'D') => {
                    let code = rest[1..]
                        .strip_prefix(';')
                        .and_then(|r| r.split(';').next())
                        .and_then(|c| c.parse().ok());
                    let mut status = shared.status.lock().unwrap();
                    // A D without a C: a prompt with nothing run (the first one, Ctrl+C, Enter).
                    if let Some(since) = status.running.take() {
                        status.finished += 1;
                        status.last = Some(Outcome {
                            code,
                            took: since.elapsed(),
                        });
                        drop(status);
                        if let Some(id) = self.command.take() {
                            shared.marks.lock().unwrap().insert(id, Mark::Ended(code));
                        }
                        shared.wake_now();
                    }
                }
                _ => {}
            },
            "7" => set_folder(shared, folder_of_url(rest, cfg!(windows))),
            "9" => match rest.strip_prefix("9;") {
                Some(path) => set_folder(shared, folder_of_path(path, cfg!(windows))),
                // Other ConEmu commands (9;4 is a progress bar): a number, then ";" or nothing.
                None if conemu(rest) => {}
                None => shared.attention(message(rest)),
            },
            "777" => {
                if let Some(note) = rest.strip_prefix("notify;") {
                    let (title, body) = note.split_once(';').unwrap_or((note, ""));
                    shared.attention(message(body).or_else(|| message(title)));
                }
            }
            _ => {}
        }
        None
    }
}

/// `bytes` with `inserts` added at their offsets (in order).
pub(crate) fn rewrite(bytes: &[u8], inserts: Inserts) -> Vec<u8> {
    let extra: usize = inserts.iter().map(|(_, b)| b.len()).sum();
    let mut out = Vec::with_capacity(bytes.len() + extra);
    let mut at = 0;
    for (offset, insert) in inserts {
        out.extend_from_slice(&bytes[at..offset]);
        out.extend_from_slice(&insert);
        at = offset;
    }
    out.extend_from_slice(&bytes[at..]);
    out
}

/// While the number is read, whether it can still be one of ours; then, whether it is.
fn could_be_ours(body: &[u8]) -> bool {
    if body.len() > MAX_BODY {
        return false;
    }
    match body.iter().position(|&b| b == b';') {
        Some(end) => matches!(&body[..end], b"7" | b"9" | b"133" | b"777"),
        None => body.len() <= 3 && body.iter().all(u8::is_ascii_digit),
    }
}

fn set_folder(shared: &Shared, folder: Option<PathBuf>) {
    if let Some(folder) = folder {
        *shared.folder.lock().unwrap() = Some(folder);
    }
}

fn conemu(rest: &str) -> bool {
    let number = rest.split(';').next().unwrap_or("");
    !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())
}

/// A notification's text, on one line and not too long; None when blank.
fn message(text: &str) -> Option<String> {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_MESSAGE)
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

/// OSC 7's `file://host/path` (percent-encoded) or `kitty-shell-cwd://host/path` (as is). The
/// host isn't checked: the folder is only trusted while the shell is in the foreground.
fn folder_of_url(url: &str, windows: bool) -> Option<PathBuf> {
    let (scheme, rest) = url.split_once("://")?;
    let path = &rest[rest.find('/')?..];
    let path = if scheme == "file" {
        percent_decode(path)
    } else {
        path.to_string()
    };
    folder_of_path(&path, windows)
}

/// A path as a shell reports it. On Windows, `/C:/x` (a file URL's) and `/c/x` (Git Bash's)
/// are `C:/x`.
fn folder_of_path(path: &str, windows: bool) -> Option<PathBuf> {
    let path = path.trim().trim_matches('"');
    if path.is_empty() {
        return None;
    }
    if windows && let Some(rest) = path.strip_prefix('/') {
        let b = rest.as_bytes();
        let drive = b.first().filter(|d| d.is_ascii_alphabetic());
        if let Some(&d) = drive {
            let tail = &rest[1..];
            let tail = tail.strip_prefix(':').unwrap_or(tail);
            if tail.is_empty() || tail.starts_with('/') {
                let tail = if tail.is_empty() { "/" } else { tail };
                return Some(PathBuf::from(format!(
                    "{}:{tail}",
                    d.to_ascii_uppercase() as char
                )));
            }
        }
    }
    Some(PathBuf::from(path))
}

/// Decodes `%XX`; anything else (a lone `%`) stays as it is.
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (
            b[i],
            b.get(i + 1).copied().and_then(hex),
            b.get(i + 2).copied().and_then(hex),
        ) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The PTY, with a [`Scanner`] over everything read from it. alacritty's event loop reads
/// through it as through the PTY.
pub(crate) struct Tap<P> {
    pty: P,
    scanner: Scanner,
    shared: Arc<Shared>,
    /// Rewritten bytes the last read had no room for, and how many of them were given.
    pending: Vec<u8>,
    given: usize,
}

impl<P> Tap<P> {
    pub fn new(pty: P, shared: Arc<Shared>) -> Self {
        Self {
            pty,
            scanner: Scanner::new(),
            shared,
            pending: Vec::new(),
            given: 0,
        }
    }
}

impl<P: EventedReadWrite> io::Read for Tap<P> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            // A quarter of the room: what we add (a hyperlink per prompt) then fits. Bytes left
            // over would wait for the PTY's next output, since the event loop polls the PTY.
            let room = (buf.len() / 4).max(1).min(buf.len());
            let n = self.pty.reader().read(&mut buf[..room])?;
            let inserts = self.scanner.scan(&buf[..n], &self.shared);
            if inserts.is_empty() {
                return Ok(n);
            }
            self.pending = rewrite(&buf[..n], inserts);
            self.given = 0;
        }
        let left = &self.pending[self.given..];
        let k = left.len().min(buf.len());
        buf[..k].copy_from_slice(&left[..k]);
        self.given += k;
        if self.given == self.pending.len() {
            self.pending.clear();
        }
        Ok(k)
    }
}

impl<P: EventedReadWrite> EventedReadWrite for Tap<P> {
    type Reader = Self;
    type Writer = P::Writer;

    // The trait's own `unsafe fn`; this only passes the call on.
    #[allow(unsafe_code)]
    unsafe fn register(
        &mut self,
        poller: &Arc<Poller>,
        event: Event,
        mode: PollMode,
    ) -> io::Result<()> {
        // SAFETY: the sources registered are the PTY's own, and the caller's promise that they
        // outlive their registration holds for them unchanged.
        unsafe { self.pty.register(poller, event, mode) }
    }

    fn reregister(&mut self, poller: &Arc<Poller>, event: Event, mode: PollMode) -> io::Result<()> {
        self.pty.reregister(poller, event, mode)
    }

    fn deregister(&mut self, poller: &Arc<Poller>) -> io::Result<()> {
        self.pty.deregister(poller)
    }

    fn reader(&mut self) -> &mut Self {
        self
    }

    fn writer(&mut self) -> &mut P::Writer {
        self.pty.writer()
    }
}

impl<P: EventedPty> EventedPty for Tap<P> {
    fn next_child_event(&mut self) -> Option<ChildEvent> {
        self.pty.next_child_event()
    }
}

impl<P: OnResize> OnResize for Tap<P> {
    fn on_resize(&mut self, size: WindowSize) {
        self.pty.on_resize(size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::headless;

    fn shared() -> Arc<Shared> {
        headless(10, 2).1
    }

    fn scan(bytes: &[u8]) -> Arc<Shared> {
        let shared = shared();
        Scanner::new().scan(bytes, &shared);
        shared
    }

    fn status(shared: &Shared) -> Status {
        shared.status.lock().unwrap().clone()
    }

    fn folder(shared: &Shared) -> Option<PathBuf> {
        shared.folder.lock().unwrap().clone()
    }

    #[test]
    fn a_command_runs_then_ends_with_its_code() {
        let shared = scan(b"\x1b]133;C\x07output\r\n");
        assert!(status(&shared).running.is_some());
        let mut scanner = Scanner::new();
        scanner.scan(b"\x1b]133;D;2\x1b\\", &shared);
        let s = status(&shared);
        assert_eq!((s.running, s.finished), (None, 1));
        assert_eq!(s.last.map(|o| o.code), Some(Some(2)));
    }

    #[test]
    fn options_after_the_code_and_a_missing_code() {
        let shared = scan(b"\x1b]133;C;\x07\x1b]133;D;0;aid=42\x07");
        assert_eq!(status(&shared).last.map(|o| o.code), Some(Some(0)));
        let shared = scan(b"\x1b]133;C\x07\x1b]133;D\x07");
        assert_eq!(status(&shared).last.map(|o| o.code), Some(None));
    }

    #[test]
    fn an_end_without_a_start_is_no_command() {
        let s = status(&scan(b"\x1b]133;D;0\x07\x1b]133;A\x07$ "));
        assert_eq!((s.finished, s.last), (0, None));
    }

    #[test]
    fn sequences_split_anywhere_are_heard_the_same() {
        let all: &[u8] = b"ab\x1b[1m\x1b]2;title\x07\x1b]133;C\x07out\x1b]133;D;1\x1b\\\x1b]7;file://h/tmp/a%20b\x07";
        for at in 0..=all.len() {
            let shared = shared();
            let mut scanner = Scanner::new();
            scanner.scan(&all[..at], &shared);
            scanner.scan(&all[at..], &shared);
            let s = status(&shared);
            assert_eq!(s.finished, 1, "split at {at}");
            assert_eq!(s.last.map(|o| o.code), Some(Some(1)), "split at {at}");
            assert_eq!(
                folder(&shared),
                Some(PathBuf::from("/tmp/a b")),
                "split at {at}"
            );
        }
    }

    #[test]
    fn other_sequences_are_not_kept() {
        // A long clipboard write (OSC 52) is skipped as it comes, never collected.
        let mut long = b"\x1b]52;c;".to_vec();
        long.extend(std::iter::repeat_n(b'A', 100_000));
        long.extend(b"\x07");
        let shared = shared();
        let mut scanner = Scanner::new();
        scanner.scan(&long, &shared);
        assert!(scanner.body.len() < 8, "kept {} bytes", scanner.body.len());
        // Numbers that start like ours aren't ours.
        scanner.scan(
            b"\x1b]1337;CurrentDir=/x\x07\x1b]77;file:///y\x07\x1b]13;C\x07",
            &shared,
        );
        assert_eq!(
            (folder(&shared), status(&shared)),
            (None, Status::default())
        );
    }

    #[test]
    fn an_endless_sequence_of_ours_is_dropped() {
        let mut body = b"\x1b]7;file://h/".to_vec();
        body.extend(std::iter::repeat_n(b'a', MAX_BODY + 10));
        body.extend(b"\x07");
        assert_eq!(folder(&scan(&body)), None);
    }

    #[test]
    fn an_escape_ends_an_osc_as_in_vte() {
        // ESC [ starts a CSI: the OSC before it ends there, and still counts.
        let shared = scan(b"\x1b]7;file:///srv\x1b[0m");
        assert_eq!(folder(&shared), Some(PathBuf::from("/srv")));
    }

    #[test]
    fn folders_from_urls_and_paths() {
        assert_eq!(
            folder_of_url("kitty-shell-cwd://h/a%20b", false),
            Some(PathBuf::from("/a%20b")),
            "kitty's scheme isn't encoded"
        );
        assert_eq!(folder_of_url("file://h/100%", false), Some("/100%".into()));
        assert_eq!(folder_of_url("file:/x", false), None);
        assert_eq!(
            folder_of_url("file:///C:/Users/u", true),
            Some("C:/Users/u".into())
        );
        assert_eq!(
            folder_of_url("file://h/c/Users/u", true),
            Some("C:/Users/u".into())
        );
        assert_eq!(folder_of_url("file://h/d", true), Some("D:/".into()));
        assert_eq!(
            folder_of_url("file://h/usr/bin", true),
            Some("/usr/bin".into())
        );
        assert_eq!(folder_of_url("file://h/c/x", false), Some("/c/x".into()));
        assert_eq!(
            folder_of_path("\"C:\\Users\\u\"", true),
            Some("C:\\Users\\u".into())
        );
    }

    #[test]
    fn conemu_folders_and_commands() {
        let shared = scan(b"\x1b]9;9;\"C:\\x\"\x07\x1b]9;4;1;50\x07");
        assert_eq!(folder(&shared), Some(PathBuf::from("C:\\x")));
        assert_eq!(status(&shared).attention, 0, "9;4 is a progress bar");
    }

    #[test]
    fn notifications_ask_for_attention() {
        let shared = scan(b"\x1b]9;Claude is waiting for you\x07");
        let s = status(&shared);
        assert_eq!(s.attention, 1);
        assert_eq!(s.message.as_deref(), Some("Claude is waiting for you"));
        Scanner::new().scan(b"\x1b]777;notify;Codex;Done\x07", &shared);
        Scanner::new().scan(b"\x1b]777;notify;Only a title\x07", &shared);
        let s = status(&shared);
        assert_eq!(s.attention, 3);
        assert_eq!(s.message.as_deref(), Some("Only a title"));
    }

    /// What vte gets from `chunks` read one after the other, and what was heard.
    fn through(chunks: &[&[u8]]) -> (Vec<u8>, Arc<Shared>) {
        let shared = shared();
        let mut scanner = Scanner::new();
        let mut out = Vec::new();
        for chunk in chunks {
            let inserts = scanner.scan(chunk, &shared);
            out.extend(rewrite(chunk, inserts));
        }
        (out, shared)
    }

    fn marks(shared: &Shared) -> Vec<(u32, Mark)> {
        let mut marks: Vec<_> = shared.marks.lock().unwrap().clone().into_iter().collect();
        marks.sort_by_key(|(id, _)| *id);
        marks
    }

    #[test]
    fn a_prompt_is_marked_until_its_command_starts() {
        let (out, shared) = through(&[b"\x1b]133;A\x07$ ls\r\n\x1b]133;C\x07a b\r\n"]);
        assert_eq!(
            out,
            b"\x1b]133;A\x07\x1b]8;;templum:cmd/0\x1b\\$ ls\r\n\x1b]133;C\x07\x1b]8;;\x1b\\a b\r\n"
        );
        assert_eq!(marks(&shared), [(0, Mark::Running)]);
        let (_, shared) =
            through(&[b"\x1b]133;A\x07$ \x1b]133;C\x07\x1b]133;D;3\x07\x1b]133;A\x07$ "]);
        assert_eq!(
            marks(&shared),
            [(0, Mark::Ended(Some(3)))],
            "prompt 1 runs nothing yet"
        );
    }

    #[test]
    fn a_command_without_a_prompt_mark_adds_nothing() {
        let (out, shared) = through(&[b"\x1b]133;C\x07x\x1b]133;D;0\x07"]);
        assert_eq!(out, b"\x1b]133;C\x07x\x1b]133;D;0\x07");
        assert_eq!((marks(&shared), status(&shared).finished), (vec![], 1));
    }

    #[test]
    fn after_an_st_the_mark_comes_after_its_backslash() {
        let want = b"\x1b]133;A\x1b\\\x1b]8;;templum:cmd/0\x1b\\$ ".to_vec();
        assert_eq!(through(&[b"\x1b]133;A\x1b\\$ "]).0, want);
        assert_eq!(
            through(&[b"\x1b]133;A\x1b", b"\\$ "]).0,
            want,
            "split in the ST"
        );
        // ESC starting another sequence: no whole ST, no mark (rather than splitting that one).
        assert_eq!(through(&[b"\x1b]133;A\x1b[0m$ "]).0, b"\x1b]133;A\x1b[0m$ ");
    }

    #[test]
    fn a_second_integrations_marks_are_the_same_prompts_and_command() {
        // fish 4 with Templum's script: our A, fish's A on the same line, then both C and D.
        let (out, shared) = through(&[
            b"\x1b]133;A\x07\x1b]0;~\x1b\\\x1b[?2004h\x1b]133;A;click_events=1\x1b\\$ false\r\n\
              \x1b]133;C;cmdline_url=false\x1b\\\x1b]133;C\x07\x1b]133;D;1\x1b\\\x1b]133;D;1\x07",
        ]);
        assert_eq!(
            out.windows(7).filter(|w| w == b"\x1b]8;;te").count(),
            1,
            "one mark"
        );
        assert_eq!(marks(&shared), [(0, Mark::Ended(Some(1)))]);
        assert_eq!(status(&shared).finished, 1);
        // Enter on an empty line (or Ctrl+C): the next prompt is a new one.
        let (_, shared) = through(&[b"\x1b]133;A\x07$ \r\n\x1b]133;A\x07$ ls\r\n\x1b]133;C\x07"]);
        assert_eq!(marks(&shared), [(1, Mark::Running)]);
    }

    #[test]
    fn marks_are_the_same_however_the_reads_split() {
        let all: &[u8] =
            b"\x1b]133;A\x07$ false\r\n\x1b]133;C\x07\x1b]133;D;1\x1b\\\x1b]133;A\x1b\\$ ";
        let (whole, _) = through(&[all]);
        for at in 0..=all.len() {
            let (split, shared) = through(&[&all[..at], &all[at..]]);
            assert_eq!(split, whole, "split at {at}");
            assert_eq!(marks(&shared), [(0, Mark::Ended(Some(1)))], "split at {at}");
        }
    }

    #[test]
    fn long_messages_are_cut() {
        let mut body = b"\x1b]9;".to_vec();
        body.extend(std::iter::repeat_n(b'x', 1000));
        body.extend(b"\x07");
        let s = status(&scan(&body));
        assert_eq!(s.message.map(|m| m.len()), Some(MAX_MESSAGE));
    }
}
