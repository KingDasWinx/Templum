//! Hears what shells and agents tell the terminal in sequences vte drops: OSC 133 (a command
//! starts and ends, with its exit code), OSC 7 and 9;9 (the current folder), OSC 9 and 777
//! (notifications). The bytes reach vte unchanged: this only listens.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{OnResize, WindowSize};
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
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            state: State::Ground,
            body: Vec::new(),
        }
    }

    pub fn scan(&mut self, bytes: &[u8], shared: &Shared) {
        let mut i = 0;
        while i < bytes.len() {
            match self.state {
                // Most output: a single pass to the next ESC.
                State::Ground => match bytes[i..].iter().position(|&b| b == 0x1b) {
                    Some(at) => {
                        self.state = State::Esc;
                        i += at;
                    }
                    None => return,
                },
                State::Esc => {
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
                            dispatch(&self.body, shared);
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
    }
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

fn dispatch(body: &[u8], shared: &Shared) {
    let text = String::from_utf8_lossy(body);
    let Some((number, rest)) = text.split_once(';') else {
        return;
    };
    match number {
        "133" => match rest.as_bytes().first() {
            Some(b'C') => shared.status.lock().unwrap().running = Some(Instant::now()),
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
}

impl<P> Tap<P> {
    pub fn new(pty: P, shared: Arc<Shared>) -> Self {
        Self {
            pty,
            scanner: Scanner::new(),
            shared,
        }
    }
}

impl<P: EventedReadWrite> io::Read for Tap<P> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.pty.reader().read(buf)?;
        self.scanner.scan(&buf[..n], &self.shared);
        Ok(n)
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

    #[test]
    fn long_messages_are_cut() {
        let mut body = b"\x1b]9;".to_vec();
        body.extend(std::iter::repeat_n(b'x', 1000));
        body.extend(b"\x07");
        let s = status(&scan(&body));
        assert_eq!(s.message.map(|m| m.len()), Some(MAX_MESSAGE));
    }
}
