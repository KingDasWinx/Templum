//! Claude Code: its status line, through Templum's relay, and its transcripts.
//!
//! Claude Code hands its status line command a JSON document on stdin
//! (https://code.claude.com/docs/en/statusline), with the session's cost and context and, for
//! Pro/Max subscribers, the plan's `rate_limits`. The Claude Code Templum opens gets
//! `--settings <data>/claude-statusline.json`, which makes Templum itself (`templum
//! --claude-statusline`) that command: the relay writes the document where Templum reads it,
//! then runs the user's own status line.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::{Limits, Proc, Session, TAIL, Window, modified, tail, valid_session_id};

/// The argument that makes Templum the relay.
pub const RELAY_FLAG: &str = "--claude-statusline";

/// The settings file the Claude Code Templum opens is given, in Templum's data folder.
const SETTINGS: &str = "claude-statusline.json";
/// The MCP config the Claude Code Templum opens is given (`--mcp-config`).
const MCP: &str = "mcp.json";
/// What each hook runs after Templum's program (docs/specs/connections.md, Hooks).
const HOOK: &str = "hook claude";
/// The Claude Code hooks Templum follows a session's turns with.
const HOOKED: [&str; 11] = [
    "SessionStart",
    "UserPromptSubmit",
    "PermissionRequest",
    "PostToolUse",
    "PostToolUseFailure",
    "Stop",
    "StopFailure",
    "Notification",
    "SessionEnd",
    // Its subagents (docs/specs/claude-subagents.md).
    "SubagentStart",
    "SubagentStop",
];

/// A relay file older than this when Templum starts is deleted (a week's window, and a day).
pub const KEEP_SECS: u64 = 8 * 24 * 3600;

/// What Templum reads of the status line's input.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Status {
    model: Option<Model>,
    cost: Option<Cost>,
    context_window: Option<ContextWindow>,
    rate_limits: Option<RateLimits>,
    workspace: Option<Workspace>,
    cwd: Option<PathBuf>,
    /// The folder the Claude Code it came from was on, when its `CLAUDE_CONFIG_DIR` named one:
    /// an account's (docs/specs/accounts.md, Usage and limits).
    templum_config_dir: Option<PathBuf>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Model {
    display_name: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Cost {
    total_cost_usd: Option<f64>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ContextWindow {
    total_input_tokens: Option<u64>,
    total_output_tokens: Option<u64>,
    used_percentage: Option<f32>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RateLimits {
    five_hour: Option<RateWindow>,
    seven_day: Option<RateWindow>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RateWindow {
    used_percentage: Option<f32>,
    resets_at: Option<u64>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Workspace {
    project_dir: Option<PathBuf>,
}

/// The relay, given Claude Code's status line `input` in the terminal `terminal` (its
/// `TEMPLUM_TERMINAL`): writes it to `relay_dir`, with `"templum_config_dir"` added when its
/// `CLAUDE_CONFIG_DIR` named `config_dir`, then runs the user's own status line, found in the
/// project's settings or `claude_dir`'s, and returns what it printed. Never fails: Claude Code
/// would show the error in its status line.
pub fn relay(
    input: &[u8],
    relay_dir: &Path,
    terminal: Option<&str>,
    claude_dir: Option<&Path>,
    config_dir: Option<&Path>,
) -> Vec<u8> {
    if let Some(terminal) = terminal.filter(|t| valid_id(t)) {
        let named = config_dir.and_then(|dir| {
            let mut value: serde_json::Map<String, serde_json::Value> =
                serde_json::from_slice(input).ok()?;
            value.insert(
                "templum_config_dir".into(),
                dir.to_string_lossy().into_owned().into(),
            );
            serde_json::to_vec(&value).ok()
        });
        let bytes = named.as_deref().unwrap_or(input);
        let _ = write_atomic(&relay_dir.join(format!("{terminal}.json")), bytes);
    }
    let status: Status = serde_json::from_slice(input).unwrap_or_default();
    let project = status
        .workspace
        .and_then(|w| w.project_dir)
        .or(status.cwd)
        .or_else(|| std::env::current_dir().ok());
    match users_status_line(project.as_deref(), claude_dir) {
        Some(command) => run(&command, input),
        None => Vec::new(),
    }
}

/// A terminal id as Templum makes them: `<pid>-<n>`. Anything else could name a file elsewhere.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().all(|c| c.is_ascii_digit() || c == '-')
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().expect("a file in a folder");
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().expect("a file name").to_string_lossy();
    let temp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temp);
    })
}

/// The status line command the user set, as Claude Code would pick it without `--settings`:
/// the project's local settings, its shared ones, then the user's. None when there is none, or
/// when it is the relay itself.
fn users_status_line(project: Option<&Path>, claude_dir: Option<&Path>) -> Option<String> {
    let project = project.map(|p| p.join(".claude"));
    let files = [
        project.as_ref().map(|p| p.join("settings.local.json")),
        project.as_ref().map(|p| p.join("settings.json")),
        claude_dir.map(|c| c.join("settings.json")),
    ];
    let line = files.into_iter().flatten().find_map(|file| {
        let text = std::fs::read(file).ok()?;
        let settings: serde_json::Value = serde_json::from_slice(&text).ok()?;
        settings.get("statusLine").cloned()
    })?;
    let command = line.get("command")?.as_str()?;
    (!command.trim().is_empty() && !command.contains(RELAY_FLAG)).then(|| command.to_string())
}

/// Runs `command` in the shell with `input` on its stdin; its output, or nothing.
fn run(command: &str, input: &[u8]) -> Vec<u8> {
    #[cfg(unix)]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    // ponytail: unverified that Claude Code runs status lines through cmd on Windows (it may
    // use Git Bash); check by hand, then pick the same shell.
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt as _;
        let mut c = Command::new("cmd");
        // CREATE_NO_WINDOW: the relay has no console, and cmd would open one after each answer.
        c.arg("/C").arg(command).creation_flags(0x0800_0000);
        c
    };
    let child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let Ok(mut child) = child else {
        return Vec::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A status line that doesn't read its input still runs.
        let _ = stdin.write_all(input);
    }
    child
        .wait_with_output()
        .map(|o| o.stdout)
        .unwrap_or_default()
}

/// `settings_file`'s path as typed after `--settings` in a shell.
pub fn settings_arg(
    data: &Path,
    exe: &Path,
    windows: bool,
    status_line: bool,
    hooks: bool,
) -> Option<String> {
    let path = settings_file(data, exe, windows, status_line, hooks)?;
    typed_path(&path.to_string_lossy(), windows)
}

/// Writes the settings file the Claude Code Templum opens is given, with Templum's data folder
/// `data` and its program `exe`: Templum's status line when `status_line`, and the hooks that
/// tell Templum its turns when `hooks`. Returns its path; None when there is nothing to give, or
/// a path can't be typed safely in every shell.
pub fn settings_file(
    data: &Path,
    exe: &Path,
    windows: bool,
    status_line: bool,
    hooks: bool,
) -> Option<PathBuf> {
    let path = data.join(SETTINGS);
    typed_path(&path.to_string_lossy(), windows)?;
    // Linux names a program replaced while it runs (an upgrade) "<path> (deleted)": the new one
    // is at the path.
    let exe = exe.to_string_lossy();
    let exe = typed_path(exe.strip_suffix(" (deleted)").unwrap_or(&exe), windows)?;
    let mut settings = serde_json::Map::new();
    if status_line {
        // The relay is told the folder: Claude Code's environment may name another one.
        let folder = typed_path(&data.to_string_lossy(), windows)?;
        settings.insert(
            "statusLine".into(),
            serde_json::json!({
                "type": "command",
                "command": format!("{exe} {RELAY_FLAG} {folder}"),
                "padding": 0,
            }),
        );
    }
    if hooks {
        let hook = serde_json::json!([{
            "hooks": [{ "type": "command", "command": format!("{exe} {HOOK}"), "timeout": 5 }]
        }]);
        let all: serde_json::Map<String, serde_json::Value> = HOOKED
            .iter()
            .map(|event| (event.to_string(), hook.clone()))
            .collect();
        settings.insert("hooks".into(), all.into());
        // The server comes with the hooks (`mcp_file`): `reply` runs without asking, and only
        // `reply` (docs/specs/agent-protocol.md, 9).
        settings.insert(
            "permissions".into(),
            serde_json::json!({ "allow": ["mcp__templum__reply"] }),
        );
    }
    if settings.is_empty() {
        return None;
    }
    write_changed(&path, &serde_json::Value::Object(settings))?;
    Some(path)
}

/// `mcp_file`'s path as typed after `--mcp-config` in a shell.
pub fn mcp_arg(data: &Path, exe: &Path, windows: bool) -> Option<String> {
    let path = mcp_file(data, exe, windows)?;
    typed_path(&path.to_string_lossy(), windows)
}

/// Writes the MCP config the Claude Code Templum opens is given: Templum's own server, `exe
/// mcp`. Returns its path; None when it can't be typed safely in every shell.
pub fn mcp_file(data: &Path, exe: &Path, windows: bool) -> Option<PathBuf> {
    let path = data.join(MCP);
    typed_path(&path.to_string_lossy(), windows)?;
    let exe = exe.to_string_lossy();
    let exe = exe.strip_suffix(" (deleted)").unwrap_or(&exe);
    let config = serde_json::json!({
        "mcpServers": { "templum": {
            "type": "stdio", "command": exe, "args": ["mcp"], "timeout": crate::MCP_TIMEOUT_MS
        } }
    });
    write_changed(&path, &config)?;
    Some(path)
}

/// Writes `value` to `path` unless the file already holds it.
pub(crate) fn write_changed(path: &Path, value: &serde_json::Value) -> Option<()> {
    let text = serde_json::to_vec_pretty(value).expect("JSON");
    if std::fs::read(path).ok().as_deref() != Some(&text[..]) {
        write_atomic(path, &text).ok()?;
    }
    Some(())
}

/// `path` as one word in bash, zsh, fish, PowerShell and cmd: as is when it is plain, in double
/// quotes otherwise; Windows' `\` as `/`, which they all take. None when it holds a character
/// one of them would expand inside double quotes.
pub fn typed_path(path: &str, windows: bool) -> Option<String> {
    let path = if windows {
        path.replace('\\', "/")
    } else {
        path.to_string()
    };
    if path.is_empty() || path.contains(['"', '$', '%', '!', '`', '\\', '\n']) {
        return None;
    }
    let plain = path
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "/._-:+@=,".contains(c));
    Some(if plain { path } else { format!("\"{path}\"") })
}

/// The session of the Claude Code `proc`: from the relay's file when it wrote one for this
/// process, else from Claude Code's transcript.
pub fn session(proc: &Proc, relay_dir: &Path, claude_dir: Option<&Path>) -> Option<Session> {
    relayed(proc, relay_dir).or_else(|| transcript(proc, claude_dir?))
}

fn relayed(proc: &Proc, relay_dir: &Path) -> Option<Session> {
    let path = relay_dir.join(format!("{}.json", proc.terminal));
    // Written for an earlier agent in the same terminal: not this one's. Start times are about
    // a second off on Linux.
    if modified(&path)? + 2 < proc.started {
        return None;
    }
    let status: Status = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    let context = status.context_window.unwrap_or_default();
    let tokens = match (context.total_input_tokens, context.total_output_tokens) {
        (None, None) => None,
        (i, o) => Some(i.unwrap_or(0) + o.unwrap_or(0)),
    };
    Some(Session {
        model: status.model.and_then(|m| m.display_name),
        context_percent: context.used_percentage,
        context_tokens: None,
        tokens,
        cost_usd: status.cost.and_then(|c| c.total_cost_usd),
    })
}

/// Claude Code's own record of a process (`sessions/<pid>.json`; undocumented).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Running {
    session_id: String,
    cwd: PathBuf,
}

fn running(pid: u32, claude_dir: &Path) -> Option<Running> {
    let path = claude_dir.join("sessions").join(format!("{pid}.json"));
    let running: Running = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    valid_session_id(&running.session_id).then_some(running)
}

/// The id of the session the Claude Code process `pid` has open, for `claude --resume`; Qwen
/// Code's, given its folder (`~/.qwen`), for `qwen --resume`.
pub fn session_id(pid: u32, claude_dir: &Path) -> Option<String> {
    running(pid, claude_dir).map(|r| r.session_id)
}

/// Whether Claude Code still has its session `id` in `claude_dir` to resume
/// (`projects/<project>/<id>.jsonl`): it writes the file with the conversation's first message,
/// deletes old ones, and exits with "No conversation found" on one it doesn't find. A folder it
/// can't read can't tell: kept.
pub(crate) fn session_kept(claude_dir: &Path, id: &str) -> bool {
    let Ok(projects) = std::fs::read_dir(claude_dir.join("projects")) else {
        return true;
    };
    let name = format!("{id}.jsonl");
    projects.flatten().any(|p| p.path().join(&name).is_file())
}

/// The context in tokens of the session's last answer, from its transcript.
fn transcript(proc: &Proc, claude_dir: &Path) -> Option<Session> {
    let running = running(proc.pid, claude_dir)?;
    let file = claude_dir
        .join("projects")
        .join(project_folder(&running.cwd))
        .join(format!("{}.jsonl", running.session_id));
    let text = tail(&file, TAIL)?;
    text.lines().rev().find_map(|line| {
        let entry: serde_json::Value = serde_json::from_str(line).ok()?;
        if entry.get("type")?.as_str()? != "assistant"
            || entry.get("isSidechain").and_then(|s| s.as_bool()) == Some(true)
        {
            return None;
        }
        let message = entry.get("message")?;
        // Claude Code's own notes ("You've hit your limit"), with no usage of their own.
        if message.get("model").and_then(|m| m.as_str()) == Some("<synthetic>") {
            return None;
        }
        let usage = message.get("usage")?;
        let count = |key: &str| usage.get(key).and_then(|v| v.as_u64()).unwrap_or(0);
        Some(Session {
            model: message
                .get("model")
                .and_then(|m| m.as_str())
                .map(str::to_string),
            context_tokens: Some(
                count("input_tokens")
                    + count("cache_creation_input_tokens")
                    + count("cache_read_input_tokens"),
            ),
            ..Session::default()
        })
    })
}

/// The folder Claude Code keeps a project's transcripts in: its path with every character
/// that isn't a letter or a digit as `-`.
fn project_folder(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The plan limits in the newest relay file that has them, of the Claude Code on `config_dir`
/// (None: one whose `CLAUDE_CONFIG_DIR` named none): they are its account's.
pub fn limits(relay_dir: &Path, config_dir: Option<&Path>) -> Option<Limits> {
    limits_where(relay_dir, |dir| dir == config_dir)
}

/// The plan limits of Claude Code's own folder: the newest relay file's of a Claude Code on no
/// folder of `accounts` (the person's shell may name one of its own with `CLAUDE_CONFIG_DIR`).
pub fn own_limits(relay_dir: &Path, accounts: &[&Path]) -> Option<Limits> {
    limits_where(relay_dir, |dir| dir.is_none_or(|d| !accounts.contains(&d)))
}

/// The plan limits in the newest relay file that has them, of a Claude Code whose folder (None:
/// none named) `wanted` takes.
fn limits_where(relay_dir: &Path, wanted: impl Fn(Option<&Path>) -> bool) -> Option<Limits> {
    let mut files: Vec<(u64, PathBuf)> = std::fs::read_dir(relay_dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter_map(|p| Some((modified(&p)?, p)))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    files.into_iter().find_map(|(at, path)| {
        let status: Status = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
        if !wanted(status.templum_config_dir.as_deref()) {
            return None;
        }
        let rate = status.rate_limits?;
        let windows: Vec<Window> = [(300, rate.five_hour), (10080, rate.seven_day)]
            .into_iter()
            .filter_map(|(minutes, w)| {
                let w = w?;
                Some(Window {
                    minutes,
                    used_percent: w.used_percentage?,
                    resets_at: w.resets_at?,
                    scope: None,
                })
            })
            .collect();
        (!windows.is_empty()).then_some(Limits { windows, at })
    })
}

/// How long Claude Code may take to say its plan's usage.
const POLL_TIMEOUT: Duration = Duration::from_secs(30);

/// How long after Claude Code ended, or was killed, its output may still be read: a process it
/// started (`claude.cmd`'s node on Windows) may keep the pipe open and never let it end.
const READ_GRACE: Duration = Duration::from_secs(5);

/// The plan's limits as Claude Code's `/usage` tells them, asked of `program` (Claude Code) at
/// `now`: it reads its own login, asks nothing of the model and keeps no session. It runs in
/// `dir`, an empty folder, with that folder's settings only, so the user's hooks and plugins
/// don't run; on an account's folder `config_dir` when given. None for an API key, which has no
/// plan. Slow: off the UI thread.
pub fn poll(program: &Path, dir: &Path, config_dir: Option<&Path>, now: u64) -> Option<Limits> {
    poll_within(program, dir, config_dir, now, (POLL_TIMEOUT, READ_GRACE))
}

/// `poll`, with `(timeout, grace)`: how long Claude Code may take, and how long its output may
/// take to end after.
fn poll_within(
    program: &Path,
    dir: &Path,
    config_dir: Option<&Path>,
    now: u64,
    (timeout, grace): (Duration, Duration),
) -> Option<Limits> {
    std::fs::create_dir_all(dir).ok()?;
    let mut command = Command::new(program);
    if let Some(config_dir) = config_dir {
        command.env("CLAUDE_CONFIG_DIR", config_dir);
    }
    command
        .args(["--setting-sources", "local", "-p", "/usage"])
        .args(["--output-format", "stream-json", "--verbose"])
        .arg("--no-session-persistence")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        // CREATE_NO_WINDOW: no console flashing every few minutes.
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let (sent, read) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        let _ = sent.send(text);
    });
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            Ok(Some(_)) => break,
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    // The reader is left to finish on its own when the pipe stays open.
    usage_report(&read.recv_timeout(grace).ok()?, now)
}

/// What `/usage` says, in its stream-json output.
#[derive(Deserialize)]
struct Reported {
    usage_report: Option<UsageReport>,
}

#[derive(Deserialize)]
struct UsageReport {
    rate_limits: Option<ReportedLimits>,
}

#[derive(Deserialize)]
struct ReportedLimits {
    #[serde(default)]
    limits: Vec<ReportedLimit>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ReportedLimit {
    /// `session`, `weekly_all`, `weekly_scoped`.
    kind: String,
    /// `session` or `weekly`.
    group: String,
    percent: Option<f32>,
    /// ISO 8601.
    resets_at: Option<String>,
    scope: Option<serde_json::Value>,
}

/// The limits in `/usage`'s output, read at `at`.
fn usage_report(output: &str, at: u64) -> Option<Limits> {
    let limits = output.lines().find_map(|line| {
        serde_json::from_str::<Reported>(line)
            .ok()?
            .usage_report?
            .rate_limits
    })?;
    let windows: Vec<Window> = limits
        .limits
        .into_iter()
        .filter_map(|l| {
            let minutes = match (l.kind.as_str(), l.group.as_str()) {
                ("session", _) => 300,
                (_, "weekly") => 10080,
                _ => return None,
            };
            let resets_at = chrono::DateTime::parse_from_rfc3339(l.resets_at.as_deref()?).ok()?;
            let scope = l
                .scope
                .as_ref()
                .and_then(|s| Some(s.get("model")?.get("display_name")?.as_str()?.to_string()));
            Some(Window {
                minutes,
                used_percent: l.percent?,
                resets_at: u64::try_from(resets_at.timestamp()).ok()?,
                scope,
            })
        })
        .collect();
    (!windows.is_empty()).then_some(Limits { windows, at })
}

/// Deletes the relay files older than [`KEEP_SECS`] at `now`.
pub fn prune(relay_dir: &Path, now: u64) {
    let Ok(entries) = std::fs::read_dir(relay_dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if modified(&path).is_some_and(|m| m + KEEP_SECS < now) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn set_modified(path: &Path, secs: u64) {
        let file = std::fs::File::options().write(true).open(path).unwrap();
        file.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    fn proc(terminal: &str, started: u64) -> Proc {
        Proc {
            agent: "claude".into(),
            pid: 4242,
            started,
            cwd: None,
            terminal: terminal.into(),
            home: None,
            account: None,
        }
    }

    const STATUS: &str = r#"{
        "session_id": "s1",
        "model": {"id": "claude-opus-5-5", "display_name": "Opus"},
        "cost": {"total_cost_usd": 3.1},
        "context_window": {"total_input_tokens": 15500, "total_output_tokens": 1200,
                           "context_window_size": 200000, "used_percentage": 42},
        "rate_limits": {"five_hour": {"used_percentage": 23.5, "resets_at": 1738425600},
                        "seven_day": {"used_percentage": 41.2, "resets_at": 1738857600}}
    }"#;

    #[cfg(unix)]
    #[test]
    fn the_relay_keeps_its_input_and_prints_the_users_status_line() {
        let dir = tempfile::tempdir().unwrap();
        let relay_dir = dir.path().join("relay");
        let claude = dir.path().join("claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("settings.json"),
            r#"{"statusLine": {"type": "command", "command": "printf mine; wc -c >/dev/null"}}"#,
        )
        .unwrap();
        let out = relay(
            STATUS.as_bytes(),
            &relay_dir,
            Some("12-3"),
            Some(&claude),
            None,
        );
        assert_eq!(out, b"mine");
        assert_eq!(
            std::fs::read_to_string(relay_dir.join("12-3.json")).unwrap(),
            STATUS
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_projects_local_status_line_wins() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let claude = dir.path().join("claude");
        std::fs::create_dir_all(project.join(".claude")).unwrap();
        std::fs::create_dir_all(&claude).unwrap();
        let line = |text: &str| format!(r#"{{"statusLine": {{"command": "printf {text}"}}}}"#);
        std::fs::write(claude.join("settings.json"), line("user")).unwrap();
        std::fs::write(project.join(".claude/settings.json"), line("shared")).unwrap();
        let input = format!(
            r#"{{"workspace": {{"project_dir": "{}"}}}}"#,
            project.display()
        );
        let out = relay(input.as_bytes(), dir.path(), None, Some(&claude), None);
        assert_eq!(out, b"shared");
        std::fs::write(project.join(".claude/settings.local.json"), line("local")).unwrap();
        let out = relay(input.as_bytes(), dir.path(), None, Some(&claude), None);
        assert_eq!(out, b"local");
    }

    #[test]
    fn the_relay_names_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let relay_dir = dir.path().join("relay");
        relay(
            STATUS.as_bytes(),
            &relay_dir,
            Some("1-0"),
            None,
            Some(Path::new("/a")),
        );
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(relay_dir.join("1-0.json")).unwrap()).unwrap();
        assert_eq!(written["templum_config_dir"], "/a");
        let input: serde_json::Value = serde_json::from_str(STATUS).unwrap();
        for (key, value) in input.as_object().unwrap() {
            assert_eq!(&written[key], value, "{key}");
        }
        relay(STATUS.as_bytes(), &relay_dir, Some("1-1"), None, None);
        assert_eq!(
            std::fs::read_to_string(relay_dir.join("1-1.json")).unwrap(),
            STATUS
        );
        relay(
            b"not json",
            &relay_dir,
            Some("1-2"),
            None,
            Some(Path::new("/a")),
        );
        assert_eq!(
            std::fs::read(relay_dir.join("1-2.json")).unwrap(),
            b"not json"
        );
    }

    #[test]
    fn limits_are_the_folders_own() {
        let dir = tempfile::tempdir().unwrap();
        for (terminal, folder, used) in [
            ("1-0", "/a", "11"),
            ("1-1", "/b", "22"),
            ("1-2", "/c", "33"),
        ] {
            let status = STATUS.replace("23.5", used);
            relay(
                status.as_bytes(),
                dir.path(),
                Some(terminal),
                None,
                Some(Path::new(folder)),
            );
        }
        let used = |folder: Option<&str>| {
            limits(dir.path(), folder.map(Path::new)).map(|l| l.windows[0].used_percent)
        };
        assert_eq!(used(Some("/a")), Some(11.0));
        assert_eq!(used(Some("/b")), Some(22.0));
        assert_eq!(used(None), None, "the agent's own folder has none of these");
        relay(STATUS.as_bytes(), dir.path(), Some("1-3"), None, None);
        assert_eq!(used(None), Some(23.5));
        assert_eq!(used(Some("/a")), Some(11.0));
    }

    /// A Claude Code on a folder no account has (the person's shell set `CLAUDE_CONFIG_DIR`)
    /// is the agent's own: its limits stay the meter's.
    #[test]
    fn the_agents_own_limits_are_every_folder_but_its_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let account = Path::new("/a");
        let status = STATUS.replace("23.5", "11");
        relay(
            status.as_bytes(),
            dir.path(),
            Some("1-0"),
            None,
            Some(account),
        );
        let used = |dir: &Path| own_limits(dir, &[account]).map(|l| l.windows[0].used_percent);
        assert_eq!(used(dir.path()), None, "the account's");
        let path = dir.path().join("1-0.json");
        set_modified(&path, now() - 100);
        let shells = STATUS.replace("23.5", "44");
        relay(
            shells.as_bytes(),
            dir.path(),
            Some("1-1"),
            None,
            Some(Path::new("/home/u/.config/claude")),
        );
        assert_eq!(used(dir.path()), Some(44.0));
    }

    #[test]
    fn the_relay_never_runs_itself_nor_writes_a_strange_name() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        std::fs::create_dir_all(&claude).unwrap();
        std::fs::write(
            claude.join("settings.json"),
            r#"{"statusLine": {"command": "/opt/templum --claude-statusline"}}"#,
        )
        .unwrap();
        let relay_dir = dir.path().join("relay");
        assert!(relay(b"{}", &relay_dir, Some("../x"), Some(&claude), None).is_empty());
        assert!(relay(b"{}", &relay_dir, Some(""), Some(&claude), None).is_empty());
        assert!(!relay_dir.exists());
    }

    #[test]
    fn a_relay_file_tells_the_session_and_the_limits() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("1-0.json"), STATUS).unwrap();
        let s = session(&proc("1-0", now() - 60), dir.path(), None).unwrap();
        assert_eq!(
            s,
            Session {
                model: Some("Opus".into()),
                context_percent: Some(42.0),
                context_tokens: None,
                tokens: Some(16700),
                cost_usd: Some(3.1),
            }
        );
        let l = limits(dir.path(), None).unwrap();
        assert_eq!(l.windows.len(), 2);
        assert_eq!(
            l.windows[0],
            Window {
                minutes: 300,
                used_percent: 23.5,
                resets_at: 1738425600,
                scope: None,
            }
        );
        assert_eq!(l.windows[1].minutes, 10080);
    }

    /// A file written for an agent the terminal ran before isn't this one's.
    #[test]
    fn a_relay_file_older_than_the_agent_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("1-0.json");
        std::fs::write(&path, STATUS).unwrap();
        set_modified(&path, now() - 100);
        assert_eq!(session(&proc("1-0", now() - 10), dir.path(), None), None);
        assert!(session(&proc("1-0", now() - 101), dir.path(), None).is_some());
    }

    /// A window missing a field is left out; the rest of the read stays.
    #[test]
    fn a_broken_window_drops_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let broken = STATUS.replace(r#""resets_at": 1738857600"#, r#""resets_at": null"#);
        std::fs::write(dir.path().join("1-0.json"), broken).unwrap();
        let l = limits(dir.path(), None).unwrap();
        assert_eq!(l.windows.len(), 1);
        assert_eq!(l.windows[0].minutes, 300);
        let s = session(&proc("1-0", now() - 60), dir.path(), None).unwrap();
        assert_eq!(s.cost_usd, Some(3.1));
    }

    #[test]
    fn the_newest_relay_file_with_limits_wins() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("1-0.json");
        std::fs::write(&old, STATUS).unwrap();
        set_modified(&old, now() - 100);
        let newer = STATUS.replace("23.5", "60");
        std::fs::write(dir.path().join("1-1.json"), newer).unwrap();
        // The newest has none: an API key user, or before the first answer.
        let newest = dir.path().join("1-2.json");
        std::fs::write(&newest, r#"{"session_id": "s3"}"#).unwrap();
        set_modified(&newest, now() + 5);
        let l = limits(dir.path(), None).unwrap();
        assert_eq!(l.windows[0].used_percent, 60.0);
    }

    #[test]
    fn without_the_relay_the_transcript_tells_the_context() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path();
        std::fs::create_dir_all(claude.join("sessions")).unwrap();
        std::fs::write(
            claude.join("sessions/4242.json"),
            r#"{"pid": 4242, "sessionId": "abc-1", "cwd": "/home/u/my app", "kind": "interactive"}"#,
        )
        .unwrap();
        let project = claude.join("projects/-home-u-my-app");
        std::fs::create_dir_all(&project).unwrap();
        let line = |sidechain: bool, input: u64| {
            format!(
                r#"{{"type":"assistant","isSidechain":{sidechain},"message":{{"model":"claude-opus-5-5","usage":{{"input_tokens":{input},"cache_creation_input_tokens":100,"cache_read_input_tokens":1000,"output_tokens":5}}}}}}"#
            )
        };
        let text = [
            line(false, 1),
            line(false, 2),
            line(true, 9),
            r#"{"type":"user"}"#.to_string(),
            r#"{"type":"assistant","isSidechain":false,"message":{"model":"<synthetic>","usage":{"input_tokens":0,"output_tokens":0}}}"#.to_string(),
        ]
        .join("\n");
        std::fs::write(project.join("abc-1.jsonl"), text).unwrap();
        let s = session(&proc("1-0", 0), &dir.path().join("relay"), Some(claude)).unwrap();
        assert_eq!(session_id(4242, claude).as_deref(), Some("abc-1"));
        assert_eq!(session_id(4243, claude), None);
        assert_eq!(s.context_tokens, Some(1102));
        assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(s.context_percent, None);
    }

    #[test]
    fn a_path_is_typed_as_one_word_in_every_shell() {
        assert_eq!(
            typed_path("/home/u/.local/share/templum/claude-statusline.json", false).as_deref(),
            Some("/home/u/.local/share/templum/claude-statusline.json")
        );
        assert_eq!(
            typed_path("/Users/u/Library/Application Support/Templum/c.json", false).as_deref(),
            Some("\"/Users/u/Library/Application Support/Templum/c.json\"")
        );
        assert_eq!(
            typed_path(r"C:\Users\Jane Doe\AppData\Roaming\templum\c.json", true).as_deref(),
            Some("\"C:/Users/Jane Doe/AppData/Roaming/templum/c.json\"")
        );
        assert_eq!(typed_path("/home/$USER/c.json", false), None);
        assert_eq!(typed_path(r"C:\100%\c.json", true), None);
    }

    // Typed as for Unix shells: a Windows temp folder's `\` is never typed that way.
    #[cfg(unix)]
    #[test]
    fn the_settings_file_names_templum_as_the_status_line() {
        let dir = tempfile::tempdir().unwrap();
        let typed = settings_arg(
            dir.path(),
            Path::new("/opt/Templum/templum"),
            false,
            true,
            false,
        )
        .unwrap();
        assert_eq!(typed, dir.path().join(SETTINGS).display().to_string());
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(SETTINGS)).unwrap()).unwrap();
        assert_eq!(
            written["statusLine"]["command"],
            format!(
                "/opt/Templum/templum --claude-statusline {}",
                dir.path().display()
            )
        );
        assert_eq!(
            settings_arg(dir.path(), Path::new("/opt/$x/templum"), false, true, false),
            None
        );
        // Upgraded while it runs: the new program is at the old path.
        settings_arg(
            dir.path(),
            Path::new("/usr/bin/templum (deleted)"),
            false,
            true,
            false,
        )
        .unwrap();
        let written = std::fs::read_to_string(dir.path().join(SETTINGS)).unwrap();
        assert!(
            written.contains("\"/usr/bin/templum --claude-statusline"),
            "{written}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_settings_file_holds_the_status_line_the_hooks_or_both() {
        let dir = tempfile::tempdir().unwrap();
        let exe = Path::new("/opt/templum");
        let read = || {
            serde_json::from_slice::<serde_json::Value>(
                &std::fs::read(dir.path().join(SETTINGS)).unwrap(),
            )
            .unwrap()
        };
        settings_arg(dir.path(), exe, false, false, true).unwrap();
        let written = read();
        assert!(written.get("statusLine").is_none());
        let hooks = written["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), HOOKED.len());
        assert_eq!(
            hooks["Stop"][0]["hooks"][0]["command"],
            "/opt/templum hook claude"
        );
        assert_eq!(hooks["PostToolUse"][0]["hooks"][0]["timeout"], 5);
        // The permission dialog opening: the wait for the person, at once.
        assert_eq!(
            hooks["PermissionRequest"][0]["hooks"][0]["command"],
            "/opt/templum hook claude"
        );
        // A turn an API error ends fires this instead of Stop.
        assert_eq!(
            hooks["StopFailure"][0]["hooks"][0]["command"],
            "/opt/templum hook claude"
        );
        // A tool that fails (or Esc during it) fires this instead of PostToolUse.
        assert_eq!(
            hooks["PostToolUseFailure"][0]["hooks"][0]["command"],
            "/opt/templum hook claude"
        );
        // Its subagents open blocks of their own (docs/specs/claude-subagents.md).
        for hook in ["SubagentStart", "SubagentStop"] {
            assert_eq!(
                hooks[hook][0]["hooks"][0]["command"],
                "/opt/templum hook claude"
            );
        }
        // `reply` asks no permission where the server comes with the hooks, and only `reply`
        // (docs/specs/agent-protocol.md, 9).
        assert_eq!(
            written["permissions"],
            serde_json::json!({ "allow": ["mcp__templum__reply"] })
        );
        settings_arg(dir.path(), exe, false, true, true).unwrap();
        assert!(read()["statusLine"].is_object() && read()["hooks"].is_object());
        // The status line alone has no server beside it: nothing to allow.
        settings_arg(dir.path(), exe, false, true, false).unwrap();
        assert!(read().get("permissions").is_none() && read().get("hooks").is_none());
        assert_eq!(settings_arg(dir.path(), exe, false, false, false), None);
    }

    #[cfg(unix)]
    #[test]
    fn the_mcp_config_names_templums_server() {
        let dir = tempfile::tempdir().unwrap();
        let typed = mcp_arg(dir.path(), Path::new("/opt/Templum App/templum"), false).unwrap();
        assert_eq!(typed, dir.path().join(MCP).display().to_string());
        let written: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join(MCP)).unwrap()).unwrap();
        // Not typed in a shell: the path as it is.
        assert_eq!(
            written["mcpServers"]["templum"],
            serde_json::json!({ "type": "stdio", "command": "/opt/Templum App/templum", "args": ["mcp"], "timeout": 604_800_000 })
        );
        assert_eq!(
            mcp_arg(Path::new("/home/$x"), Path::new("/opt/templum"), false),
            None
        );
    }

    /// What a new terminal hands the `claude` a person types: the path itself, a space and all.
    #[cfg(unix)]
    #[test]
    fn settings_file_is_the_path_unquoted() {
        let parent = tempfile::tempdir().unwrap();
        let dir = parent.path().join("Templum Data");
        std::fs::create_dir(&dir).unwrap();
        let exe = Path::new("/opt/templum");
        let file = settings_file(&dir, exe, false, true, true).unwrap();
        assert_eq!(file, dir.join(SETTINGS));
        let written = std::fs::read(&file).unwrap();
        assert_eq!(
            settings_arg(&dir, exe, false, true, true),
            Some(format!("\"{}\"", file.display()))
        );
        assert_eq!(std::fs::read(&file).unwrap(), written);
        let mcp = mcp_file(&dir, exe, false).unwrap();
        assert_eq!(mcp, dir.join(MCP));
        assert_eq!(
            mcp_arg(&dir, exe, false),
            Some(format!("\"{}\"", mcp.display()))
        );
        assert_eq!(settings_file(&dir, exe, false, false, false), None);
        assert_eq!(mcp_file(Path::new("/home/$x"), exe, false), None);
    }

    /// As Claude Code 2.1.286 printed it for a Max plan.
    const USAGE: &str = r#"{"type":"assistant","message":{"model":"<synthetic>","content":[{"type":"text","text":"Current session: 27% used"}]},"usage_report":{"session":{"total_cost_usd":0},"rate_limits":{"limits":[{"kind":"session","group":"session","percent":27,"resets_at":"2026-10-14T09:40:00.011014+00:00","scope":null,"severity":"normal","is_active":true},{"kind":"weekly_all","group":"weekly","percent":18,"resets_at":"2026-10-16T14:00:00.011037+00:00","scope":null,"severity":"normal","is_active":false},{"kind":"weekly_scoped","group":"weekly","percent":0,"resets_at":"2026-10-16T14:00:00+00:00","scope":{"model":{"display_name":"Fable"},"surface":null},"severity":"normal","is_active":false}],"extra_usage":{"is_enabled":false}}},"local_command_run":{"command":"usage","args":""}}"#;

    #[test]
    fn usage_reads_every_window_with_its_model() {
        let output = format!("{{\"type\":\"system\",\"subtype\":\"init\"}}\n{USAGE}\n");
        let l = usage_report(&output, 77).unwrap();
        assert_eq!(l.at, 77);
        assert_eq!(
            l.windows,
            vec![
                Window {
                    minutes: 300,
                    used_percent: 27.0,
                    resets_at: 1791970800,
                    scope: None
                },
                Window {
                    minutes: 10080,
                    used_percent: 18.0,
                    resets_at: 1792159200,
                    scope: None
                },
                Window {
                    minutes: 10080,
                    used_percent: 0.0,
                    resets_at: 1792159200,
                    scope: Some("Fable".into())
                },
            ]
        );
        // An API key: no plan, no limits.
        assert_eq!(
            usage_report(r#"{"type":"result","total_cost_usd":0}"#, 77),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn poll_asks_claude_code_in_an_empty_folder() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("claude");
        let printed = dir.path().join("usage.jsonl");
        std::fs::write(&printed, USAGE).unwrap();
        // Records its arguments, then prints what Claude Code would.
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\necho \"$@\" > \"{0}/args\"\necho \"$CLAUDE_CONFIG_DIR\" > \"{0}/config\"\ncat \"{1}\"\n",
                dir.path().display(),
                printed.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let probe = dir.path().join("probe");
        let l = poll(&program, &probe, None, 5).unwrap();
        assert_eq!(l.windows.len(), 3);
        assert!(probe.is_dir());
        let args = std::fs::read_to_string(dir.path().join("args")).unwrap();
        assert!(
            args.starts_with("--setting-sources local -p /usage"),
            "{args}"
        );
        assert!(args.contains("--no-session-persistence"), "{args}");
        // An account's: on its folder.
        let account = dir.path().join("accounts").join("claude2");
        poll(&program, &probe, Some(&account), 5).unwrap();
        let config = std::fs::read_to_string(dir.path().join("config")).unwrap();
        assert_eq!(config.trim_end(), account.to_string_lossy());
    }

    /// A process Claude Code started keeps its output open: the ask ends anyway, with nothing.
    #[cfg(unix)]
    #[test]
    fn poll_gives_up_on_an_output_a_child_keeps_open() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("claude");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\ncat \"{0}/usage.jsonl\"\nsleep 3 &\n",
                dir.path().display()
            ),
        )
        .unwrap();
        std::fs::write(dir.path().join("usage.jsonl"), USAGE).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let start = Instant::now();
        let within = (Duration::from_secs(30), Duration::from_millis(300));
        let probe = dir.path().join("probe");
        assert_eq!(poll_within(&program, &probe, None, 5, within), None);
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn old_relay_files_are_pruned() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir.path().join("1-0.json");
        let recent = dir.path().join("1-1.json");
        std::fs::write(&old, "{}").unwrap();
        std::fs::write(&recent, "{}").unwrap();
        set_modified(&old, now() - KEEP_SECS - 10);
        set_modified(&recent, now() - KEEP_SECS + 3600);
        prune(dir.path(), now());
        assert!(!old.exists());
        assert!(recent.exists());
    }
}
