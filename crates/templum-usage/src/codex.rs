//! Codex: its rollout files, `sessions/YYYY/MM/DD/rollout-*.jsonl` under `$CODEX_HOME` or
//! `~/.codex`. The first line is the session's `session_meta` (its folder and start); after
//! each turn a `token_count` event tells the tokens used and the plan's `rate_limits`.

use std::io::BufRead as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike as _};
use serde::Deserialize;

use crate::{Limits, Proc, Session, TAIL, Window, modified, tail};

const DAY: u64 = 24 * 3600;

/// The sessions of the Codex processes `procs`, in order: each process gets its own file.
pub fn sessions(procs: &[&Proc], codex_dir: &Path, now: u64) -> Vec<Option<Session>> {
    files(procs, codex_dir, now)
        .iter()
        .map(|f| usage(f.as_ref()?))
        .collect()
}

/// The ids of the sessions of the Codex processes `procs`, in order, for `codex resume`.
pub fn session_ids(procs: &[&Proc], codex_dir: &Path, now: u64) -> Vec<Option<String>> {
    files(procs, codex_dir, now)
        .iter()
        .map(|f| meta(f.as_ref()?)?.id)
        .collect()
}

/// The rollout of each of the Codex processes `procs`, in order: each process gets its own.
pub fn rollouts_of(procs: &[&Proc], codex_dir: &Path, now: u64) -> Vec<Option<PathBuf>> {
    files(procs, codex_dir, now)
}

/// The plan limits in the newest of the rollouts `files` (by modified time) that has them: the
/// account's when several Codex folders share one `sessions/` and these are its own processes'.
pub fn limits_of(files: &[PathBuf]) -> Option<Limits> {
    let mut files: Vec<(u64, &PathBuf)> = files
        .iter()
        .filter_map(|f| Some((modified(f)?, f)))
        .collect();
    files.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
    files.into_iter().find_map(|(at, f)| limits_in(f, at))
}

fn files(procs: &[&Proc], codex_dir: &Path, now: u64) -> Vec<Option<PathBuf>> {
    let earliest = procs.iter().map(|p| p.started).min().unwrap_or(now);
    let files = rollouts(codex_dir, earliest.saturating_sub(DAY), now + DAY);
    let metas: Vec<(PathBuf, Meta, u64)> = files
        .into_iter()
        .filter_map(|(path, mtime)| Some((meta(&path)?, mtime, path)))
        // A subagent's thread is no process's session.
        .filter(|(m, ..)| !m.subagent)
        .map(|(m, mtime, path)| (path, m, mtime))
        .collect();
    let mut taken = vec![false; metas.len()];
    let mut order: Vec<usize> = (0..procs.len()).collect();
    // The newest process first: an older one in the same folder can't take its file.
    order.sort_by(|&a, &b| procs[b].started.cmp(&procs[a].started));
    let mut found = vec![None; procs.len()];
    // The file a process has open is its own, even one from long ago it resumed: those first.
    #[cfg(target_os = "linux")]
    let open: Vec<Option<PathBuf>> = procs.iter().map(|p| open_rollout(p.pid)).collect();
    #[cfg(not(target_os = "linux"))]
    let open: Vec<Option<PathBuf>> = vec![None; procs.len()];
    for (i, path) in open.iter().enumerate() {
        if let Some(path) = path {
            if let Some(j) = metas.iter().position(|(p, ..)| p == path) {
                taken[j] = true;
            }
            found[i] = Some(path.clone());
        }
    }
    for i in order.into_iter().filter(|&i| open[i].is_none()) {
        let p = procs[i];
        let Some(cwd) = &p.cwd else { continue };
        let ours = |j: &usize| !taken[*j] && metas[*j].1.cwd == *cwd;
        // Started by this process; else one it resumed and writes to.
        let pick = (0..metas.len())
            .filter(|j| ours(j) && metas[*j].1.start + 5 >= p.started)
            .max_by_key(|&j| metas[j].1.start)
            .or_else(|| {
                (0..metas.len())
                    .filter(|j| ours(j) && metas[*j].2 + 2 >= p.started)
                    .max_by_key(|&j| metas[j].2)
            });
        if let Some(j) = pick {
            taken[j] = true;
            found[i] = Some(metas[j].0.clone());
        }
    }
    found
}

/// The plan limits in the newest rollout of the last week that has them: they are the
/// account's. A session just started, or one on an API key, has none yet.
pub fn limits(codex_dir: &Path, now: u64) -> Option<Limits> {
    let mut files = rollouts(codex_dir, now.saturating_sub(7 * DAY), now + DAY);
    files.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
    // ponytail: the 10 newest only, so a week of keyless sessions isn't read every 5 s
    files
        .into_iter()
        .take(10)
        .find_map(|(path, at)| limits_in(&path, at))
}

fn limits_in(path: &Path, at: u64) -> Option<Limits> {
    let text = tail(path, TAIL)?;
    text.lines().rev().find_map(|line| {
        let event = token_count(line)?;
        let rate = event.rate_limits?;
        if rate.limit_id.as_deref().is_some_and(|id| id != "codex") {
            return None;
        }
        let windows: Vec<Window> = [rate.primary, rate.secondary]
            .into_iter()
            .flatten()
            .filter_map(|w| {
                Some(Window {
                    minutes: w.window_minutes?,
                    used_percent: w.used_percent?,
                    resets_at: w.resets_at?,
                    scope: None,
                })
            })
            .collect();
        (!windows.is_empty()).then_some(Limits { windows, at })
    })
}

/// The rollout files in the dated folders from `from` to `to` (seconds since the epoch), with
/// their modification times. Codex names the folders by local date: the days around are read.
fn rollouts(codex_dir: &Path, from: u64, to: u64) -> Vec<(PathBuf, u64)> {
    let sessions = codex_dir.join("sessions");
    let mut files = Vec::new();
    let mut day = from - from % DAY;
    while day <= to {
        if let Some(date) = DateTime::from_timestamp(day as i64, 0) {
            let folder = sessions
                .join(format!("{:04}", date.year()))
                .join(format!("{:02}", date.month()))
                .join(format!("{:02}", date.day()));
            if let Ok(entries) = std::fs::read_dir(folder) {
                files.extend(
                    entries
                        .flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
                        .filter_map(|p| Some((modified(&p)?, p)))
                        .map(|(m, p)| (p, m)),
                );
            }
        }
        day += DAY;
    }
    files
}

/// The rollout a Codex process has open, or one of its children has (npm's `codex` is a Node
/// script that starts the native one): exact, even for a session it resumed. After `/new` the
/// old one stays open, and a subagent opens its own: the newest of the session's own.
#[cfg(target_os = "linux")]
fn open_rollout(pid: u32) -> Option<PathBuf> {
    let open = |pid: u32| -> Vec<PathBuf> {
        let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
            return Vec::new();
        };
        fds.flatten()
            .filter_map(|e| std::fs::read_link(e.path()).ok())
            .filter(|p| {
                p.extension().is_some_and(|e| e == "jsonl")
                    && p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("rollout-"))
            })
            .collect()
    };
    let children = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|c| c.parse().ok())
        .flat_map(open)
        .collect::<Vec<_>>();
    // Named `rollout-<start>-<id>`: by name is by start.
    open(pid).into_iter().chain(children).max_by_key(|p| {
        (
            meta(p).map(|m| !m.subagent),
            p.file_name().map(|n| n.to_owned()),
        )
    })
}

/// What a rollout's first line says about its session.
struct Meta {
    cwd: PathBuf,
    /// When it started, in seconds since the epoch.
    start: u64,
    /// What `codex resume` takes, the session's (a subagent's names its parent); None unless it
    /// names a file.
    id: Option<String>,
    /// A subagent's thread, not a session of its own.
    subagent: bool,
}

#[derive(Deserialize)]
struct Line<T> {
    #[serde(rename = "type")]
    kind: String,
    payload: T,
}

#[derive(Deserialize)]
struct SessionMeta {
    cwd: PathBuf,
    timestamp: String,
    #[serde(default)]
    id: String,
    #[serde(default)]
    session_id: String,
}

fn meta(path: &Path) -> Option<Meta> {
    let mut first = String::new();
    std::io::BufReader::new(std::fs::File::open(path).ok()?)
        .read_line(&mut first)
        .ok()?;
    let line: Line<SessionMeta> = serde_json::from_str(&first).ok()?;
    if line.kind != "session_meta" {
        return None;
    }
    let SessionMeta {
        cwd,
        timestamp,
        id,
        session_id,
    } = line.payload;
    let start = DateTime::parse_from_rfc3339(&timestamp).ok()?;
    let subagent = !session_id.is_empty() && session_id != id;
    let id = if session_id.is_empty() {
        id
    } else {
        session_id
    };
    Some(Meta {
        cwd,
        start: u64::try_from(start.timestamp()).ok()?,
        id: crate::valid_session_id(&id).then_some(id),
        subagent,
    })
}

#[derive(Deserialize)]
struct TokenCount {
    #[serde(rename = "type")]
    kind: String,
    info: Option<Info>,
    rate_limits: Option<RateLimits>,
}

#[derive(Deserialize)]
struct Info {
    total_token_usage: TokenUsage,
    last_token_usage: TokenUsage,
    model_context_window: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct TokenUsage {
    input_tokens: u64,
    total_tokens: u64,
}

#[derive(Deserialize)]
struct RateLimits {
    limit_id: Option<String>,
    primary: Option<RateWindow>,
    secondary: Option<RateWindow>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RateWindow {
    used_percent: Option<f32>,
    window_minutes: Option<u32>,
    resets_at: Option<u64>,
}

fn token_count(line: &str) -> Option<TokenCount> {
    let line: Line<TokenCount> = serde_json::from_str(line).ok()?;
    (line.kind == "event_msg" && line.payload.kind == "token_count").then_some(line.payload)
}

/// The session's usage, from its last `token_count` that has one.
fn usage(path: &Path) -> Option<Session> {
    let text = tail(path, TAIL)?;
    text.lines().rev().find_map(|line| {
        let info = token_count(line)?.info?;
        let context_percent = info
            .model_context_window
            .filter(|&w| w > 0)
            .map(|w| (info.last_token_usage.input_tokens as f64 / w as f64 * 100.0) as f32);
        Some(Session {
            context_percent,
            tokens: Some(info.total_token_usage.total_tokens),
            ..Session::default()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-08T22:15:12Z
    const START: u64 = 1_791_497_712;

    fn rollout(dir: &Path, name: &str, cwd: &str, start: &str, lines: &[String]) -> PathBuf {
        let folder = dir.join("sessions/2026/10/08");
        std::fs::create_dir_all(&folder).unwrap();
        let path = folder.join(format!("rollout-{name}.jsonl"));
        let meta = format!(
            r#"{{"timestamp":"{start}","type":"session_meta","payload":{{"id":"{name}","timestamp":"{start}","cwd":"{cwd}","cli_version":"0.154.0"}}}}"#
        );
        let text = std::iter::once(meta)
            .chain(lines.iter().cloned())
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&path, text).unwrap();
        path
    }

    fn count(input: u64, total: u64, limits: &str) -> String {
        format!(
            r#"{{"type":"event_msg","payload":{{"type":"token_count","info":{{"total_token_usage":{{"input_tokens":1,"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":{total}}},"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":1,"reasoning_output_tokens":0,"total_tokens":2}},"model_context_window":200000}},"rate_limits":{limits}}}}}"#
        )
    }

    const LIMITS: &str = r#"{"limit_id":"codex","primary":{"used_percent":2.0,"window_minutes":300,"resets_at":1791500000},"secondary":{"used_percent":19.5,"window_minutes":10080,"resets_at":1791900000},"rate_limit_reached_type":null}"#;

    fn proc(cwd: &str, started: u64) -> Proc {
        Proc {
            agent: "codex".into(),
            // No such process: the folder and start decide.
            pid: u32::MAX,
            started,
            cwd: Some(cwd.into()),
            terminal: "1-0".into(),
            home: None,
            account: None,
        }
    }

    #[test]
    fn a_process_reads_the_session_it_started() {
        let dir = tempfile::tempdir().unwrap();
        rollout(
            dir.path(),
            "a",
            "/srv/app",
            "2026-10-08T22:15:12.504Z",
            &[count(50_000, 1234, LIMITS), count(100_000, 5678, "null")],
        );
        let p = proc("/srv/app", START - 1);
        let found = sessions(&[&p], dir.path(), START + 60);
        let s = found[0].clone().unwrap();
        assert_eq!(s.tokens, Some(5678));
        assert_eq!(s.context_percent, Some(50.0));
    }

    /// Two Codex in one folder each get their own; one started after the file isn't its.
    #[test]
    fn two_processes_in_one_folder_each_get_their_own_file() {
        let dir = tempfile::tempdir().unwrap();
        rollout(
            dir.path(),
            "a",
            "/srv/app",
            "2026-10-08T22:15:12Z",
            &[count(1, 111, "null")],
        );
        rollout(
            dir.path(),
            "b",
            "/srv/app",
            "2026-10-08T22:25:12Z",
            &[count(1, 222, "null")],
        );
        let (a, b) = (proc("/srv/app", START), proc("/srv/app", START + 600));
        let found = sessions(&[&a, &b], dir.path(), START + 900);
        assert_eq!(found[0].as_ref().unwrap().tokens, Some(111));
        assert_eq!(found[1].as_ref().unwrap().tokens, Some(222));
        // Each resumes its own on restore.
        let ids = session_ids(&[&a, &b], dir.path(), START + 900);
        assert_eq!(ids, [Some("a".to_string()), Some("b".to_string())]);
        // A subagent of `b`'s, started later in the same folder, is no one's session.
        let sub = dir.path().join("sessions/2026/10/08/rollout-c.jsonl");
        std::fs::write(
            &sub,
            format!(
                "{}\n{}",
                r#"{"type":"session_meta","payload":{"id":"c","session_id":"b","cwd":"/srv/app","timestamp":"2026-10-08T22:26:12Z"}}"#,
                count(1, 333, "null")
            ),
        )
        .unwrap();
        let found = sessions(&[&b], dir.path(), START + 900);
        assert_eq!(found[0].as_ref().unwrap().tokens, Some(222));
        assert_eq!(meta(&sub).unwrap().id.as_deref(), Some("b"), "its parent's");
        // Started after both files were last written, elsewhere: none is its.
        let late = proc("/srv/other", START + 3600);
        assert_eq!(sessions(&[&late], dir.path(), START + 3700), vec![None]);
    }

    /// On Linux the file a Codex has open is its own, even a session from long ago it resumed.
    #[cfg(target_os = "linux")]
    #[test]
    fn the_rollout_a_process_has_open_is_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("sessions/2020/01/01");
        std::fs::create_dir_all(&folder).unwrap();
        let file = |name: &str, ids: &str, tokens: u64| {
            let path = folder.join(format!("rollout-2020-01-01T{name}.jsonl"));
            let meta = format!(
                r#"{{"type":"session_meta","payload":{{{ids}"cwd":"/srv/app","timestamp":"2020-01-01T00:00:00Z"}}}}"#
            );
            std::fs::write(&path, format!("{meta}\n{}", count(1, tokens, "null"))).unwrap();
            path
        };
        // A session, the one `/new` started after it (the old stays open), and its subagent's.
        let old = file("00-00-00-old", r#""id":"old","session_id":"old","#, 444);
        let new = file("01-00-00-new", r#""id":"new","session_id":"new","#, 555);
        let sub = file("02-00-00-sub", r#""id":"sub","session_id":"new","#, 666);
        let mut child = std::process::Command::new("sh")
            .arg("-c")
            .arg("exec 3<\"$0\" 4<\"$1\" 5<\"$2\"; sleep 10")
            .args([&old, &new, &sub])
            .spawn()
            .unwrap();
        let p = Proc {
            pid: child.id(),
            ..proc("/srv/app", START)
        };
        // Until `sh` has the three files open: read before, the two reads below saw it open
        // different ones.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let open = || {
            std::fs::read_dir(format!("/proc/{}/fd", child.id()))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| std::fs::read_link(e.path()).ok())
                .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
                .count()
        };
        while open() < 3 && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let found = sessions(&[&p], dir.path(), START + 60);
        let ids = session_ids(&[&p], dir.path(), START + 60);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(ids[0].as_deref(), Some("new"));
        assert_eq!(found[0].as_ref().map(|s| s.tokens), Some(Some(555)));
    }

    #[test]
    fn a_turn_without_info_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let null_info = r#"{"type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":null}}"#;
        rollout(
            dir.path(),
            "a",
            "/srv/app",
            "2026-10-08T22:15:12Z",
            &[count(1, 333, "null"), null_info.to_string()],
        );
        let p = proc("/srv/app", START);
        let found = sessions(&[&p], dir.path(), START + 60);
        assert_eq!(found[0].as_ref().unwrap().tokens, Some(333));
    }

    #[test]
    fn shared_codex_sessions_give_each_its_processes_limits() {
        let dir = tempfile::tempdir().unwrap();
        let mine = rollout(
            dir.path(),
            "mine",
            "/srv/app",
            "2026-10-08T22:15:12Z",
            &[count(1, 1, LIMITS)],
        );
        let theirs = LIMITS.replace("2.0", "80.0");
        let other = rollout(
            dir.path(),
            "other",
            "/srv/web",
            "2026-10-08T22:25:12Z",
            &[count(1, 1, &theirs)],
        );
        let file = std::fs::File::options().write(true).open(&mine).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(START))
            .unwrap();
        // codex2's, the newer, is what the folder's newest says.
        assert_eq!(
            limits(dir.path(), START + 60).unwrap().windows[0].used_percent,
            80.0
        );
        let p = proc("/srv/app", START - 1);
        let files = rollouts_of(&[&p], dir.path(), START + 60);
        assert_eq!(files, [Some(mine.clone())]);
        let l = limits_of(&files.into_iter().flatten().collect::<Vec<_>>()).unwrap();
        assert_eq!(l.windows[0].used_percent, 2.0, "its own process's file");
        assert_eq!(
            limits_of(&[other, mine]).unwrap().windows[0].used_percent,
            80.0,
            "the newest"
        );
        assert_eq!(limits_of(&[]), None);
    }

    #[test]
    fn a_codex_whose_sessions_are_shared_reads_its_own_processes_limits() {
        let dir = tempfile::tempdir().unwrap();
        let mine = rollout(
            dir.path(),
            "mine",
            "/srv/app",
            "2026-10-08T22:15:12Z",
            &[count(1, 1, LIMITS)],
        );
        let theirs = LIMITS.replace("2.0", "80.0");
        rollout(
            dir.path(),
            "other",
            "/srv/web",
            "2026-10-08T22:25:12Z",
            &[count(1, 1, &theirs)],
        );
        let file = std::fs::File::options().write(true).open(&mine).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(START))
            .unwrap();
        let dirs = crate::Dirs {
            relay: dir.path().join("relay"),
            claude: None,
            codex: Some(dir.path().to_path_buf()),
            qwen: None,
            gemini: None,
        };
        let home = |sessions_shared| crate::AgentHome {
            id: "codex".into(),
            agent: "codex".into(),
            dir: dir.path().to_path_buf(),
            sessions_shared,
        };
        let p = proc("/srv/app", START - 1);
        let used = |shared| {
            let (_, limits) =
                crate::read(std::slice::from_ref(&p), &dirs, &[home(shared)], START + 60);
            limits[0].1.windows[0].used_percent
        };
        assert_eq!(used(true), 2.0, "its own process's");
        assert_eq!(used(false), 80.0, "the folder's newest");
    }

    #[test]
    fn limits_come_from_codexs_own_rate_limits() {
        let dir = tempfile::tempdir().unwrap();
        let premium = r#"{"limit_id":"premium","primary":null,"secondary":null}"#;
        let one_window = r#"{"limit_id":"codex","primary":{"used_percent":100.0,"window_minutes":300,"resets_at":1791500000},"secondary":null,"rate_limit_reached_type":"primary"}"#;
        let a = rollout(
            dir.path(),
            "a",
            "/srv/app",
            "2026-10-08T22:15:12Z",
            &[count(1, 1, LIMITS), count(1, 1, premium)],
        );
        let file = std::fs::File::options().write(true).open(&a).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(START))
            .unwrap();
        let l = limits(dir.path(), START + 60).unwrap();
        assert_eq!(
            l.windows,
            vec![
                Window {
                    minutes: 300,
                    used_percent: 2.0,
                    resets_at: 1791500000,
                    scope: None,
                },
                Window {
                    minutes: 10080,
                    used_percent: 19.5,
                    resets_at: 1791900000,
                    scope: None,
                },
            ]
        );
        rollout(
            dir.path(),
            "b",
            "/srv/app",
            "2026-10-08T22:25:12Z",
            &[count(1, 1, one_window)],
        );
        let l = limits(dir.path(), START + 60).unwrap();
        assert_eq!(l.windows.len(), 1);
        assert_eq!(l.windows[0].used_percent, 100.0);
        // A Codex just started has no limits yet: the older file's stay.
        let c = rollout(dir.path(), "c", "/srv/app", "2026-10-08T22:35:12Z", &[]);
        let file = std::fs::File::options().write(true).open(&c).unwrap();
        file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
            .unwrap();
        let l = limits(dir.path(), START + 60).unwrap();
        assert_eq!(l.windows[0].used_percent, 100.0);
    }
}
