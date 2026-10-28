//! The audit log (docs/specs/api.md, Visibility): one JSON line per call in `<data>/api/audit.log`,
//! and, in `<data>/api/noise.log`, one per minute and address for the noise anyone can make without
//! a token (connections, refused or failed authentications). Never a token, a ticket, a body or
//! typed text: the token is named, not shown.
//!
//! Lines go through a channel to a thread of their own, so a stalled disk never stalls a
//! connection: nothing here waits for it. Noise is counted before the channel, by address, and may
//! fill only half of it, so a flood of strangers neither fills the log nor the channel, and cannot
//! push the calls out of either. The noise has a file pair of its own, so strangers can never
//! rotate the calls away.

use crate::store::private_dir;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The size at which the log is set aside as `audit.log.1`: two files of this size at most.
pub(crate) const AUDIT_CAP: u64 = 1024 * 1024;
/// The same for `noise.log`.
pub(crate) const NOISE_CAP: u64 = 1024 * 1024;
/// How long one address's noise is counted into a single line.
const WINDOW: Duration = Duration::from_secs(60);
/// The lines waiting for the writer. Noise fills half of it at most; a call past it is given up
/// and counted as lost, never waited for.
const QUEUE: usize = 4096;
/// How long a dropped `Audit` waits for its writer to finish: a stuck disk must not hold the UI.
const DROP_WAIT: Duration = Duration::from_secs(1);
/// The most characters of what a line says a call was.
const WHAT_MAX: usize = 200;
/// The (address, what, token) triples counted at once; past it the rest share one.
const COUNTED: usize = 4096;
/// The address the pairs past [`COUNTED`] are counted under.
const OTHER: &str = "other";
/// What anyone can make without a token: written once, then counted.
const NOISE: [&str; 5] = [
    "connect",
    "auth_failed",
    "refused",
    "tls_failed",
    "slow_down",
];

/// What a mirror does all day (an open Git popup reads the status and the diff every 2 s): one
/// line per token, call and window like the noise, with `more` counting the rest, written to the
/// calls' log. A morning of typing must not rotate the other
/// calls out of it; every other call keeps its own line.
const CHATTER: [&str; 5] = [
    "terminal_input",
    "/v1/me",
    "git:look",
    "git:diff",
    "read_terminal",
];

#[derive(Clone)]
struct Line {
    at: u64,
    addr: String,
    token: Option<String>,
    what: String,
    status: u16,
    /// Further times the same happened within the window.
    more: u64,
    /// One of [`NOISE`] or [`CHATTER`]: it is counted, and counts against the room the channel
    /// keeps for it.
    noise: bool,
}

/// (address's limiter key, what, token's name) → when it was first written in this window, and the
/// line with its count of further times. The token is in the key: a `slow_down` is noise that
/// carries one, and another token's must not be added to the first one's line.
type Counting = HashMap<(String, String, Option<String>, u16), (Instant, Line)>;

pub(crate) struct Audit {
    /// None once dropped: the writer then drains the channel and ends.
    tx: Option<SyncSender<Line>>,
    counting: Arc<Mutex<Counting>>,
    /// The noise lines in the channel, not yet taken by the writer.
    queued_noise: Arc<AtomicUsize>,
    /// The calls given up because the channel was full; the writer logs them as one line.
    lost: Arc<AtomicU64>,
    /// Told when the writer ended.
    done: Mutex<Receiver<()>>,
    wait: Duration,
}

/// What the writer is given.
struct Writer {
    rx: Receiver<Line>,
    counting: Arc<Mutex<Counting>>,
    queued_noise: Arc<AtomicUsize>,
    lost: Arc<AtomicU64>,
    window: Duration,
}

impl Audit {
    /// The logs in the folder `dir` (`<data>/api`), made `0700` if it is missing.
    pub(crate) fn new(dir: &Path, cap: u64) -> Audit {
        Audit::with_caps(dir, cap, NOISE_CAP, WINDOW)
    }

    fn with_caps(dir: &Path, cap: u64, noise_cap: u64, window: Duration) -> Audit {
        let _ = private_dir(dir);
        let mut calls = File::new(dir.join("audit.log"), cap);
        let mut noise = File::new(dir.join("noise.log"), noise_cap);
        Audit::with_writer(DROP_WAIT, move |mut writer| {
            writer.window = window;
            run(writer, &mut calls, &mut noise);
        })
    }

    /// An `Audit` whose writer thread runs `writer`, which ends when the channel is closed and
    /// drained. Dropping it waits `wait` for that, no longer.
    fn with_writer(wait: Duration, writer: impl FnOnce(Writer) + Send + 'static) -> Audit {
        let (tx, rx) = sync_channel(QUEUE);
        let (counting, queued_noise, lost) = (Arc::default(), Arc::default(), Arc::default());
        let (done_tx, done) = std::sync::mpsc::channel();
        let given = Writer {
            rx,
            counting: Arc::clone(&counting),
            queued_noise: Arc::clone(&queued_noise),
            lost: Arc::clone(&lost),
            window: WINDOW,
        };
        let spawned = std::thread::Builder::new()
            .name("templum-api-audit".into())
            .spawn(move || {
                writer(given);
                let _ = done_tx.send(());
            });
        // No thread, no log (every send then fails, and a call is counted as lost): never a
        // reason to refuse a call.
        let _ = spawned;
        Audit {
            tx: Some(tx),
            counting,
            queued_noise,
            lost,
            done: Mutex::new(done),
            wait,
        }
    }
}

impl Audit {
    /// Notes a call or an event. A log that can't be written is no reason to refuse a call, and
    /// nothing here waits for it. Noise is counted here, by address (an IPv6 /64 is one): its
    /// first line goes on while the channel has room for noise, the rest only add to the count the
    /// writer puts in the log when the window ends. A call goes on if the channel has room, and is
    /// counted as lost if not. A path segment that looks like a token or a ticket (a client that
    /// put one in the path) is not written.
    pub(crate) fn write(&self, addr: &str, token: Option<&str>, what: &str, status: u16) {
        let Some(tx) = &self.tx else {
            return;
        };
        let what: String = what
            .split('/')
            .map(|part| {
                if part.starts_with(crate::store::TOKEN_PREFIX) || part.starts_with("tkt_") {
                    "[secret]"
                } else {
                    part
                }
            })
            .collect::<Vec<_>>()
            .join("/");
        // A name in `/v1/tools/{name}` is the caller's, up to 16 KiB: at 20 calls a second it
        // would rotate the log away in seconds.
        let what: String = what.chars().take(WHAT_MAX).collect();
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let line = Line {
            at,
            addr: addr.to_string(),
            token: token.map(String::from),
            noise: NOISE.contains(&what.as_str()) || CHATTER.contains(&what.as_str()),
            what,
            status,
            more: 0,
        };
        if !line.noise {
            if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = tx.try_send(line) {
                self.lost.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
        let mut counting = self.counting.lock().unwrap_or_else(|e| e.into_inner());
        let mut key = (
            crate::server::limiter_key(&line.addr),
            line.what.clone(),
            line.token.clone(),
            line.status,
        );
        let mut first = line;
        if !counting.contains_key(&key) && counting.len() >= COUNTED {
            // Too many to tell apart: the rest are counted together.
            key.0 = OTHER.into();
            first.addr = OTHER.into();
        }
        match counting.get_mut(&key) {
            Some((_, counted)) => counted.more += 1,
            // The other half of the channel is the calls'. A line given up is a call lost when
            // it has a token (a mirror's chatter), and nobody's when it has none (noise).
            None if self.queued_noise.load(Ordering::Relaxed) >= QUEUE / 2 => {
                if first.token.is_some() {
                    self.lost.fetch_add(1, Ordering::Relaxed);
                }
            }
            None => {
                let counted = Line {
                    more: 0,
                    ..first.clone()
                };
                let called = first.token.is_some();
                self.queued_noise.fetch_add(1, Ordering::Relaxed);
                // Counted only once its first line is on its way.
                if tx.try_send(first).is_ok() {
                    counting.insert(key, (Instant::now(), counted));
                } else {
                    self.queued_noise.fetch_sub(1, Ordering::Relaxed);
                    if called {
                        self.lost.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    }
}

impl Drop for Audit {
    /// Whatever is waiting, and whatever is being counted, is written before it ends, if the
    /// writer is quick: the channel is closed, which the writer sees once it has read what is in
    /// it, and a writer stuck on a disk is left to finish alone after [`DROP_WAIT`].
    fn drop(&mut self) {
        self.tx = None;
        let done = self.done.get_mut().unwrap_or_else(|e| e.into_inner());
        let _ = done.recv_timeout(self.wait);
    }
}

/// One log file, kept open between lines and set aside as `<name>.1` at its cap.
struct File {
    path: PathBuf,
    cap: u64,
    /// The size of the file now.
    size: u64,
    open: Option<std::fs::File>,
}

impl File {
    fn new(path: PathBuf, cap: u64) -> File {
        let size = std::fs::metadata(&path).map_or(0, |m| m.len());
        File {
            path,
            cap,
            size,
            open: None,
        }
    }

    fn append(&mut self, line: &Line) {
        let mut json = json!({
            "at": line.at, "addr": line.addr, "token": line.token,
            "what": line.what, "status": line.status,
        });
        if line.more > 0 {
            json["more"] = Value::from(line.more);
        }
        let mut text = json.to_string();
        text.push('\n');
        if self.size > 0 && self.size + text.len() as u64 > self.cap {
            // Closed first: a file can't be renamed while it is open on every system.
            self.open = None;
            let mut old = self.path.clone().into_os_string();
            old.push(".1");
            if std::fs::rename(&self.path, old).is_ok() {
                self.size = 0;
            }
        }
        if self.open.is_none() {
            let mut options = std::fs::OpenOptions::new();
            options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            self.open = options.open(&self.path).ok();
        }
        match self
            .open
            .as_mut()
            .map(|file| file.write_all(text.as_bytes()))
        {
            Some(Ok(())) => self.size += text.len() as u64,
            // A file that can't be written is opened again at the next line.
            _ => self.open = None,
        }
    }
}

/// A line with a token is a call's: it goes where the person looks for them. The rest is noise.
fn put(line: &Line, calls: &mut File, noise: &mut File) {
    if line.token.is_some() {
        calls.append(line);
    } else {
        noise.append(line);
    }
}

/// The writer: lines as they come (a line with a token into `calls`, the rest into `noise`); the
/// counted noise is put into the log as a line with its count when its window ends, looked at once
/// a second at most; calls given up because the channel was full are one line of `calls`. It ends
/// when every sender is gone and the channel is empty.
fn run(writer: Writer, calls: &mut File, noise: &mut File) {
    let Writer {
        rx,
        counting,
        queued_noise,
        lost,
        window,
    } = writer;
    let every = window.min(Duration::from_secs(1));
    let mut looked = Instant::now();
    let flush = |calls: &mut File, noise: &mut File, all: bool| {
        let now = Instant::now();
        // Taken under the lock, written after it: a connection never waits for the disk.
        let mut due = Vec::new();
        counting
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, (since, line)| {
                let ends = all || now.duration_since(*since) >= window;
                if ends && line.more > 0 {
                    due.push(line.clone());
                }
                !ends
            });
        for line in &due {
            put(line, calls, noise);
        }
    };
    let say_lost = |calls: &mut File| {
        let n = lost.swap(0, Ordering::Relaxed);
        if n > 0 {
            calls.append(&Line {
                at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |since| since.as_secs()),
                addr: String::new(),
                token: None,
                what: "lost".into(),
                status: 0,
                more: n,
                noise: false,
            });
        }
    };
    loop {
        match rx.recv_timeout(every) {
            Ok(line) => {
                if line.noise {
                    queued_noise.fetch_sub(1, Ordering::Relaxed);
                }
                put(&line, calls, noise);
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush(calls, noise, true);
                say_lost(calls);
                return;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        if looked.elapsed() >= every {
            looked = Instant::now();
            flush(calls, noise, false);
            say_lost(calls);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(dir: &Path, name: &str) -> Vec<Value> {
        std::fs::read_to_string(dir.join(name))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn the_audit_log_turns_over_at_its_cap() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::new(dir.path(), 200);
        for _ in 0..4 {
            audit.write("127.0.0.1", Some("phone"), "overview", 200);
        }
        drop(audit);
        let (log, old) = (dir.path().join("audit.log"), dir.path().join("audit.log.1"));
        assert!(old.exists(), "set aside");
        assert!(std::fs::metadata(&log).unwrap().len() < 200);
        assert!(std::fs::metadata(&old).unwrap().len() <= 200);
        let first = &lines(dir.path(), "audit.log")[0];
        assert_eq!(
            (
                first["what"].as_str(),
                first["status"].as_u64(),
                first["token"].as_str()
            ),
            (Some("overview"), Some(200), Some("phone"))
        );
        // A new Audit on the same folder goes on counting from the file.
        let again = Audit::new(dir.path(), 200);
        for _ in 0..4 {
            again.write("::1", None, "create_card", 200);
        }
        drop(again);
        assert!(std::fs::metadata(&log).unwrap().len() < 200);
    }

    /// Every line of both logs (and their set-aside files), the calls' first.
    fn both(dir: &Path) -> Vec<Value> {
        ["audit.log", "audit.log.1", "noise.log", "noise.log.1"]
            .iter()
            .flat_map(|name| lines(dir, name))
            .collect()
    }

    #[test]
    fn a_flood_of_strangers_is_a_line_and_its_count_and_never_erases_the_calls() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::new(dir.path(), AUDIT_CAP);
        audit.write("1.2.3.4", Some("phone"), "create_card", 200);
        for _ in 0..1000 {
            audit.write("203.0.113.7", None, "connect", 0);
            audit.write("203.0.113.7", None, "auth_failed", 401);
            audit.write("203.0.113.7", None, "refused", 429);
        }
        audit.write("203.0.113.8", None, "auth_failed", 401);
        audit.write("1.2.3.4", Some("phone"), "create_card", 200);
        drop(audit);
        let all = both(dir.path());
        assert!(all.len() <= 3 * 2 + 3, "{} lines", all.len());
        assert_eq!(
            all.iter().filter(|l| l["what"] == "create_card").count(),
            2,
            "calls are all kept"
        );
        let counted: u64 = all
            .iter()
            .filter(|l| l["addr"] == "203.0.113.7" && l["what"] == "auth_failed")
            .map(|l| l["more"].as_u64().unwrap_or(1))
            .sum();
        assert_eq!(counted, 1000, "first line plus the count of the rest");
        assert!(!dir.path().join("audit.log.1").exists());
        assert!(
            lines(dir.path(), "audit.log")
                .iter()
                .all(|l| l["token"].is_string()),
            "the calls' log holds lines with a token only"
        );
    }

    #[test]
    fn a_flood_of_noise_past_the_queue_loses_no_call_and_no_count() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::new(dir.path(), 16 * AUDIT_CAP);
        let addrs: Vec<String> = (0..5).map(|n| format!("203.0.113.{n}")).collect();
        let (noise, calls) = (10_000, 200);
        for n in 0..noise {
            for addr in &addrs {
                audit.write(addr, None, "auth_failed", 401);
            }
            if n % (noise / calls) == 0 {
                audit.write("1.2.3.4", Some("phone"), "create_card", 200);
            }
        }
        drop(audit);
        let all = both(dir.path());
        assert_eq!(
            all.iter().filter(|l| l["what"] == "create_card").count(),
            calls
        );
        for addr in &addrs {
            let counted: u64 = all
                .iter()
                .filter(|l| l["addr"] == *addr.as_str())
                .map(|l| l["more"].as_u64().unwrap_or(1))
                .sum();
            assert_eq!(counted, noise as u64, "{addr}");
        }
    }

    #[test]
    fn noise_is_counted_by_ipv6_prefix_and_in_a_bounded_table() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::new(dir.path(), AUDIT_CAP);
        for n in 0..50u32 {
            audit.write(&format!("2001:db8:1:2::{n:x}"), None, "connect", 0);
        }
        assert_eq!(
            audit.counting.lock().unwrap().len(),
            1,
            "one /64, one count"
        );
        for n in 0..COUNTED as u32 + 100 {
            audit.write(
                &format!("10.{}.{}.{}", n / 65_536, (n / 256) % 256, n % 256),
                None,
                "connect",
                0,
            );
        }
        assert!(audit.counting.lock().unwrap().len() <= COUNTED + 1);
    }

    #[test]
    fn noise_is_written_again_when_its_minute_is_over() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::with_caps(dir.path(), AUDIT_CAP, NOISE_CAP, Duration::from_millis(100));
        audit.write("9.9.9.9", None, "auth_failed", 401);
        audit.write("9.9.9.9", None, "auth_failed", 401);
        let deadline = Instant::now() + Duration::from_secs(10);
        // Its window ends: the count is written without anything more being sent.
        while lines(dir.path(), "noise.log").len() < 2 {
            assert!(Instant::now() < deadline, "the count never came");
            std::thread::sleep(Duration::from_millis(20));
        }
        audit.write("9.9.9.9", None, "auth_failed", 401);
        drop(audit);
        let all = lines(dir.path(), "noise.log");
        assert_eq!(all[1]["more"], 1);
        assert_eq!(all.len(), 3, "and a new window writes a first line again");
    }

    #[test]
    fn typing_is_one_audit_line_a_minute() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::with_caps(dir.path(), AUDIT_CAP, NOISE_CAP, Duration::from_millis(100));
        for _ in 0..50 {
            audit.write("1.2.3.4", Some("laptop"), "terminal_input", 200);
        }
        for _ in 0..5 {
            audit.write("1.2.3.4", Some("laptop"), "/v1/me", 200);
        }
        audit.write("1.2.3.4", Some("laptop"), "move_block", 200);
        let of = |what: &str| -> Vec<Value> {
            lines(dir.path(), "audit.log")
                .into_iter()
                .filter(|l| l["what"] == what)
                .collect()
        };
        // The window ends by itself: the counts come without anything more being sent.
        let deadline = Instant::now() + Duration::from_secs(10);
        while of("terminal_input").len() < 2 || of("/v1/me").len() < 2 {
            assert!(Instant::now() < deadline, "the counts never came");
            std::thread::sleep(Duration::from_millis(20));
        }
        let (typing, me) = (of("terminal_input"), of("/v1/me"));
        assert_eq!((typing.len(), typing[1]["more"].as_u64()), (2, Some(49)));
        assert_eq!((me.len(), me[1]["more"].as_u64()), (2, Some(4)));
        assert!(typing[0].get("more").is_none());
        assert_eq!(
            of("move_block").len(),
            1,
            "every other call has its own line"
        );
        assert!(lines(dir.path(), "noise.log").is_empty());
    }

    #[test]
    fn read_terminal_calls_are_grouped_per_token_and_minute() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::with_caps(dir.path(), AUDIT_CAP, NOISE_CAP, Duration::from_millis(100));
        for _ in 0..3 {
            audit.write("1.2.3.4", Some("laptop"), "read_terminal", 200);
        }
        let of = || -> Vec<Value> {
            lines(dir.path(), "audit.log")
                .into_iter()
                .filter(|l| l["what"] == "read_terminal")
                .collect()
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        while of().len() < 2 {
            assert!(Instant::now() < deadline, "the count never came");
            std::thread::sleep(Duration::from_millis(20));
        }
        let reads = of();
        assert_eq!((reads.len(), reads[1]["more"].as_u64()), (2, Some(2)));
    }

    #[test]
    fn an_open_popup_s_git_reads_are_grouped_and_its_acts_are_not() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::with_caps(dir.path(), AUDIT_CAP, NOISE_CAP, Duration::from_millis(100));
        for _ in 0..3 {
            audit.write("1.2.3.4", Some("laptop"), "git:look", 200);
        }
        for _ in 0..2 {
            audit.write("1.2.3.4", Some("laptop"), "git:diff", 200);
        }
        for _ in 0..2 {
            audit.write("1.2.3.4", Some("laptop"), "git:push", 200);
        }
        let of = |what: &str| -> Vec<Value> {
            lines(dir.path(), "audit.log")
                .into_iter()
                .filter(|l| l["what"] == what)
                .collect()
        };
        // The window ends by itself: the counts come without anything more being sent.
        let deadline = Instant::now() + Duration::from_secs(10);
        while of("git:look").len() < 2 || of("git:diff").len() < 2 {
            assert!(Instant::now() < deadline, "the counts never came");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(of("git:look")[1]["more"].as_u64(), Some(2));
        assert_eq!(of("git:diff")[1]["more"].as_u64(), Some(1));
        let pushes = of("git:push");
        assert_eq!(pushes.len(), 2, "every act has its own line");
        assert!(pushes.iter().all(|l| l.get("more").is_none()));
    }

    #[test]
    fn a_refused_call_is_not_counted_into_the_ones_that_passed() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::with_caps(dir.path(), AUDIT_CAP, NOISE_CAP, Duration::from_millis(100));
        audit.write("1.2.3.4", Some("laptop"), "terminal_input", 200);
        for _ in 0..3 {
            audit.write("1.2.3.4", Some("laptop"), "terminal_input", 403);
        }
        drop(audit);
        let typing: Vec<Value> = lines(dir.path(), "audit.log")
            .into_iter()
            .filter(|l| l["what"] == "terminal_input")
            .collect();
        let statuses: Vec<u64> = typing
            .iter()
            .map(|l| l["status"].as_u64().unwrap())
            .collect();
        // Each status has its own first line; only the 403s are counted, on the second 403 line.
        assert_eq!(statuses, [200, 403, 403], "{typing:?}");
        assert!(typing[0].get("more").is_none() && typing[1].get("more").is_none());
        assert_eq!(typing[2]["more"].as_u64(), Some(2));
    }

    #[test]
    fn a_long_name_is_cut_in_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::new(dir.path(), AUDIT_CAP);
        audit.write("1.2.3.4", Some("phone"), &"x".repeat(16 * 1024), 404);
        drop(audit);
        let line = &lines(dir.path(), "audit.log")[0];
        assert_eq!(
            line["what"].as_str().map(|w| w.chars().count()),
            Some(WHAT_MAX)
        );
    }

    #[test]
    fn another_tokens_slow_down_is_not_added_to_the_first_ones() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::new(dir.path(), AUDIT_CAP);
        for _ in 0..3 {
            audit.write("1.2.3.4", Some("phone"), "slow_down", 429);
        }
        for _ in 0..5 {
            audit.write("1.2.3.4", Some("laptop"), "slow_down", 429);
        }
        drop(audit);
        let count = |token: &str| -> u64 {
            lines(dir.path(), "audit.log")
                .iter()
                .filter(|l| l["token"] == token)
                .map(|l| l["more"].as_u64().unwrap_or(1))
                .sum()
        };
        assert_eq!((count("phone"), count("laptop")), (3, 5));
    }

    #[test]
    fn a_secret_in_a_path_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::new(dir.path(), AUDIT_CAP);
        audit.write("a", Some("phone"), "/v1/blocks/tpl_abcdefghijklmnop/x", 404);
        audit.write("a", Some("phone"), "/v1/events/tkt_abcdefghijklmnop", 404);
        drop(audit);
        let log = std::fs::read_to_string(dir.path().join("audit.log")).unwrap();
        assert!(
            !log.contains("abcdefghijklmnop") && log.contains("/v1/blocks/[secret]/x"),
            "{log}"
        );
    }

    #[test]
    fn a_noise_flood_never_rotates_the_calls() {
        let dir = tempfile::tempdir().unwrap();
        // Small caps: the noise file turns over, the calls' file is the one that must not.
        let audit = Audit::with_caps(dir.path(), 4096, 600, WINDOW);
        audit.write("1.2.3.4", Some("phone"), "create_card", 200);
        for n in 0..1000u32 {
            let addr = format!("10.{}.{}.{}", n / 65_536, (n / 256) % 256, n % 256);
            for what in ["connect", "auth_failed", "refused", "tls_failed"] {
                audit.write(&addr, None, what, 0);
            }
        }
        audit.write("1.2.3.4", Some("phone"), "create_card", 200);
        drop(audit);
        assert!(!dir.path().join("audit.log.1").exists(), "never rotated");
        assert_eq!(
            lines(dir.path(), "audit.log").len(),
            2,
            "the calls are all there"
        );
        assert!(
            dir.path().join("noise.log.1").exists(),
            "the noise turned over"
        );
        let size = |name: &str| std::fs::metadata(dir.path().join(name)).unwrap().len();
        assert!(size("noise.log") <= 600 && size("noise.log.1") <= 600);
    }

    /// A writer that reads nothing until `gate` is opened (a stuck disk).
    fn stuck(gate: std::sync::mpsc::Receiver<()>) -> impl FnOnce(Writer) + Send + 'static {
        move |writer| {
            let _ = gate.recv();
            drop(writer);
        }
    }

    #[test]
    fn no_call_waits_for_a_stuck_writer_and_the_lost_ones_are_counted() {
        let (open, gate) = std::sync::mpsc::channel();
        let audit = Audit::with_writer(Duration::from_millis(50), stuck(gate));
        let started = Instant::now();
        let calls = QUEUE + 500;
        for _ in 0..calls {
            audit.write("1.2.3.4", Some("phone"), "create_card", 200);
        }
        // Noise gets no room past its half either, and doesn't wait.
        for n in 0..QUEUE as u32 {
            audit.write(&format!("10.0.{}.{}", n / 256, n % 256), None, "connect", 0);
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(
            audit.lost.load(Ordering::Relaxed),
            500,
            "past the queue, counted"
        );
        assert!(audit.queued_noise.load(Ordering::Relaxed) <= QUEUE / 2);
        let _ = open.send(());
    }

    #[test]
    fn a_mirrors_chatter_given_up_is_counted_lost_and_noise_is_not() {
        let (open, gate) = std::sync::mpsc::channel();
        let audit = Audit::with_writer(Duration::from_millis(50), stuck(gate));
        // The noise half is full.
        for n in 0..QUEUE as u32 {
            audit.write(&format!("10.0.{}.{}", n / 256, n % 256), None, "connect", 0);
        }
        assert_eq!(audit.lost.load(Ordering::Relaxed), 0, "noise is nobody's");
        audit.write("1.2.3.4", Some("phone"), "terminal_input", 200);
        audit.write("1.2.3.4", Some("laptop"), "/v1/me", 200);
        assert_eq!(audit.lost.load(Ordering::Relaxed), 2);
        let _ = open.send(());
    }

    #[test]
    fn dropping_with_a_stuck_writer_returns_within_its_wait() {
        let (open, gate) = std::sync::mpsc::channel();
        let audit = Audit::with_writer(Duration::from_millis(100), stuck(gate));
        audit.write("1.2.3.4", Some("phone"), "create_card", 200);
        let started = Instant::now();
        drop(audit);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        let _ = open.send(());
    }

    #[test]
    fn calls_lost_are_one_line_of_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let audit = Audit::new(dir.path(), AUDIT_CAP);
        audit.lost.fetch_add(7, Ordering::Relaxed);
        audit.write("1.2.3.4", Some("phone"), "create_card", 200);
        drop(audit);
        let all = lines(dir.path(), "audit.log");
        let lost: Vec<_> = all.iter().filter(|l| l["what"] == "lost").collect();
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0]["more"], 7);
    }

    #[cfg(unix)]
    #[test]
    fn the_log_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("api");
        let audit = Audit::new(&folder, AUDIT_CAP);
        audit.write("a", None, "connect", 200);
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        audit.write("a", Some("phone"), "overview", 200);
        drop(audit);
        assert_eq!(
            (
                mode(&folder),
                mode(&folder.join("audit.log")),
                mode(&folder.join("noise.log"))
            ),
            (0o700, 0o600, 0o600)
        );
    }
}
