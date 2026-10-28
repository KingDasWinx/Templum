//! Runs a real command in a real PTY.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use templum_terminal::{
    Agent, Notify, Palette, Processes, Snapshot, SpawnOptions, TermOptions, Terminal, inspect,
};

#[test]
fn runs_a_command_and_reports_its_exit() {
    let (tx, rx) = mpsc::channel();
    let tx = std::sync::Mutex::new(tx);
    let notify: Notify = Arc::new(move || {
        let _ = tx.lock().unwrap().send(());
    });
    #[cfg(unix)]
    let command = (
        "sh".to_string(),
        vec!["-c".to_string(), "printf hello".to_string()],
    );
    #[cfg(windows)]
    let command = (
        "cmd".to_string(),
        vec!["/C".to_string(), "echo hello".to_string()],
    );
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 40,
            rows: 5,
            cell_px: (8, 16),
            command: Some(command),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: None,
            integration: None,
            env: Vec::new(),
        },
        notify,
    )
    .expect("spawn");

    let deadline = Instant::now() + Duration::from_secs(15);
    while !term.exited() {
        assert!(Instant::now() < deadline, "the command never exited");
        let _ = rx.recv_timeout(Duration::from_millis(100));
        term.shared().rearm();
    }
    let mut snapshot = Snapshot::default();
    snapshot.refresh(term.shared(), &Palette::default());
    assert!(
        snapshot.text().contains("hello"),
        "screen was: {:?}",
        snapshot.text()
    );
    assert!(!term.title().trim().is_empty());
}

#[test]
fn shells_start_in_the_home_directory() {
    let home = std::env::home_dir().expect("a home directory");
    let notify: Notify = Arc::new(|| {});
    #[cfg(unix)]
    let command = ("sh".to_string(), vec!["-c".to_string(), "pwd".to_string()]);
    #[cfg(windows)]
    let command = ("cmd".to_string(), vec!["/C".to_string(), "cd".to_string()]);
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 250,
            rows: 5,
            cell_px: (8, 16),
            command: Some(command),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: None,
            integration: None,
            env: Vec::new(),
        },
        notify,
    )
    .expect("spawn");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !term.exited() {
        assert!(Instant::now() < deadline, "the command never exited");
        std::thread::sleep(Duration::from_millis(50));
        term.shared().rearm();
    }
    let mut snapshot = Snapshot::default();
    snapshot.refresh(term.shared(), &Palette::default());
    let home = home.to_string_lossy();
    assert!(
        snapshot.text().lines().any(|l| l.trim_end() == home),
        "expected {home:?}, screen was: {:?}",
        snapshot.text()
    );
}

#[test]
fn starts_in_the_given_folder_and_reports_it() {
    // Started in the plain path (cmd.exe can't start in a canonical \\?\ path), compared
    // canonicalized: macOS's temp folder is behind a symlink, Windows's may be a short name.
    let dir = std::env::temp_dir();
    // Long-lived, so the process is still there to ask.
    #[cfg(unix)]
    let command = (
        "sh".to_string(),
        vec!["-c".to_string(), "sleep 20".to_string()],
    );
    #[cfg(windows)]
    let command = ("cmd".to_string(), vec!["/K".to_string()]);
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 40,
            rows: 5,
            cell_px: (8, 16),
            command: Some(command),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: Some(dir.clone()),
            integration: None,
            env: Vec::new(),
        },
        Arc::new(|| {}),
    )
    .expect("spawn");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(cwd) = term.cwd() {
            assert_eq!(cwd.canonicalize().unwrap(), dir.canonicalize().unwrap());
            break;
        }
        assert!(Instant::now() < deadline, "cwd() never answered");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// macOS's login shell runs under `login`, which stays alive as the shell's parent; nested
/// shells (`nix develop`, `poetry shell`) look the same. The folder must follow the shell the
/// user types into, not the process Templum started.
#[cfg(unix)]
#[test]
fn cwd_follows_the_shell_in_the_foreground() {
    let start = tempfile::tempdir().unwrap();
    let moved = tempfile::tempdir().unwrap();
    // `; true` keeps the outer sh alive (no exec), like login.
    let command = (
        "sh".to_string(),
        vec!["-c".to_string(), "sh -i; true".to_string()],
    );
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 80,
            rows: 5,
            cell_px: (8, 16),
            command: Some(command),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: Some(start.path().to_path_buf()),
            integration: None,
            env: Vec::new(),
        },
        Arc::new(|| {}),
    )
    .expect("spawn");
    term.write(format!("cd '{}'\r", moved.path().display()).into_bytes());
    let want = moved.path().canonicalize().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let cwd = term.cwd().and_then(|c| c.canonicalize().ok());
        if cwd.as_ref() == Some(&want) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "cwd stayed {cwd:?}, want {want:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A long-lived process in `dir`, so it's still there to ask.
fn long_lived(dir: &std::path::Path) -> Terminal {
    #[cfg(unix)]
    let command = (
        "sh".to_string(),
        vec!["-c".to_string(), "sleep 20".to_string()],
    );
    #[cfg(windows)]
    let command = ("cmd".to_string(), vec!["/K".to_string()]);
    Terminal::spawn(
        SpawnOptions {
            cols: 40,
            rows: 5,
            cell_px: (8, 16),
            command: Some(command),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: Some(dir.to_path_buf()),
            integration: None,
            env: Vec::new(),
        },
        Arc::new(|| {}),
    )
    .expect("spawn")
}

#[test]
fn inspect_reads_several_terminals_at_once() {
    // Plain paths to start in (cmd.exe can't start in a canonical \\?\ path), compared canonicalized.
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let terms = [long_lived(a.path()), long_lived(b.path())];
    let want = [a.path(), b.path()].map(|p| p.canonicalize().unwrap());
    let processes: Vec<Processes> = terms.iter().map(Terminal::processes).collect();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found: Vec<_> = inspect(&processes, &known())
            .into_iter()
            .map(|i| i.folder.and_then(|f| f.canonicalize().ok()))
            .collect();
        if found == want.iter().cloned().map(Some).collect::<Vec<_>>() {
            assert!(
                inspect(&processes, &known())
                    .iter()
                    .all(|i| i.agent.is_none()),
                "a shell isn't an agent"
            );
            break;
        }
        assert!(Instant::now() < deadline, "found {found:?}, want {want:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(inspect(&[], &known()).is_empty());
}

#[test]
fn inspect_finds_an_agent_and_its_folder() {
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let claude = fake_claude(bin.path()).display().to_string();
    #[cfg(unix)]
    let command = (claude, vec!["30".to_string()]);
    #[cfg(windows)]
    let command = (claude, vec!["/K".to_string()]);
    let options = || SpawnOptions {
        cols: 40,
        rows: 5,
        cell_px: (8, 16),
        command: Some(command.clone()),
        palette: Palette::default(),
        options: TermOptions::default(),
        working_directory: Some(work.path().to_path_buf()),
        integration: None,
        env: Vec::new(),
    };
    // Another test may fork while `fake_claude` still has its copy open for writing; that
    // child keeps the handle until it execs, and running `claude` meanwhile fails with
    // "Text file busy" on Linux. The window is short, so try again.
    let term = loop {
        match Terminal::spawn(options(), Arc::new(|| {})) {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(50));
            }
            spawned => break spawned.expect("spawn"),
        }
    };
    let want = work.path().canonicalize().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found = inspect(&[term.processes()], &known()).pop().unwrap();
        let folder = found.folder.as_ref().and_then(|f| f.canonicalize().ok());
        if found.agent.as_deref() == Some("claude") && folder.as_ref() == Some(&want) {
            // The agent's process, for reading its usage: its pid and when it started.
            assert!(found.pid.is_some(), "found {found:?}");
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let started = found.started.expect("a start time");
            assert!(now.abs_diff(started) < 60, "started {started}, now {now}");
            break;
        }
        assert!(Instant::now() < deadline, "found {found:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// What the terminal's programs read in `TEMPLUM_TERMINAL`: Claude Code's status line, run by
/// Templum's relay, names its file after it.
#[test]
fn each_terminal_gets_its_own_id_in_its_environment() {
    let printed = |command: (String, Vec<String>)| {
        let term = Terminal::spawn(
            SpawnOptions {
                cols: 60,
                rows: 5,
                cell_px: (8, 16),
                command: Some(command),
                palette: Palette::default(),
                options: TermOptions::default(),
                working_directory: None,
                integration: None,
                env: Vec::new(),
            },
            Arc::new(|| {}),
        )
        .expect("spawn");
        let deadline = Instant::now() + Duration::from_secs(15);
        while !term.exited() {
            assert!(Instant::now() < deadline, "the command never exited");
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut snapshot = Snapshot::default();
        snapshot.refresh(term.shared(), &Palette::default());
        (term.id().to_string(), snapshot.text())
    };
    #[cfg(unix)]
    let command = || {
        (
            "sh".to_string(),
            vec![
                "-c".to_string(),
                "printf 'id=%s;' \"$TEMPLUM_TERMINAL\"".to_string(),
            ],
        )
    };
    #[cfg(windows)]
    let command = || {
        (
            "cmd".to_string(),
            vec!["/C".to_string(), "echo id=%TEMPLUM_TERMINAL%;".to_string()],
        )
    };
    let (a, a_text) = printed(command());
    let (b, b_text) = printed(command());
    assert!(!a.is_empty());
    assert_ne!(a, b);
    assert!(
        a_text.contains(&format!("id={a};")),
        "screen was: {a_text:?}"
    );
    assert!(
        b_text.contains(&format!("id={b};")),
        "screen was: {b_text:?}"
    );
}

/// What Templum adds to a terminal's environment reaches its programs (the bus's address).
#[test]
fn extra_variables_reach_the_shell() {
    #[cfg(unix)]
    let command = (
        "sh".to_string(),
        vec![
            "-c".to_string(),
            "printf 'bus=%s;' \"$TEMPLUM_BUS\"".to_string(),
        ],
    );
    #[cfg(windows)]
    let command = (
        "cmd".to_string(),
        vec!["/C".to_string(), "echo bus=%TEMPLUM_BUS%;".to_string()],
    );
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 60,
            rows: 5,
            cell_px: (8, 16),
            command: Some(command),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: None,
            integration: None,
            env: vec![("TEMPLUM_BUS".into(), "unix:/x.sock".into())],
        },
        Arc::new(|| {}),
    )
    .expect("spawn");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !term.exited() {
        assert!(Instant::now() < deadline, "the command never exited");
        std::thread::sleep(Duration::from_millis(20));
    }
    let mut snapshot = Snapshot::default();
    snapshot.refresh(term.shared(), &Palette::default());
    assert!(
        snapshot.text().contains("bus=unix:/x.sock;"),
        "{:?}",
        snapshot.text()
    );
}

/// A harmless long-running program, copied under an agent's name into `bin`.
fn fake_claude(bin: &std::path::Path) -> std::path::PathBuf {
    #[cfg(unix)]
    let (from, to) = (
        ["/bin/sleep", "/usr/bin/sleep"]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .expect("sleep"),
        bin.join("claude"),
    );
    #[cfg(windows)]
    let (from, to) = (r"C:\Windows\System32\cmd.exe", bin.join("claude.exe"));
    std::fs::copy(from, &to).unwrap();
    to
}

/// The session an agent was started on, from its arguments: a shell named `pi` that waits.
#[cfg(unix)]
#[test]
fn inspect_reads_the_session_an_agent_was_started_on() {
    let bin = tempfile::tempdir().unwrap();
    let pi = bin.path().join("pi");
    std::fs::copy("/bin/sh", &pi).unwrap();
    let id = "019a1f2e-7b3c-7d4e-8f5a-6b7c8d9e0f1a";
    // `; :` keeps the shell from becoming `sleep`.
    let args = ["-c", "sleep 30; :", "pi", "--session-id", id];
    let options = || SpawnOptions {
        cols: 40,
        rows: 5,
        cell_px: (8, 16),
        command: Some((pi.display().to_string(), args.map(String::from).to_vec())),
        palette: Palette::default(),
        options: TermOptions::default(),
        working_directory: None,
        integration: None,
        env: Vec::new(),
    };
    // As in `inspect_finds_an_agent_and_its_folder`: a fork may hold the copy open a moment.
    let term = loop {
        match Terminal::spawn(options(), Arc::new(|| {})) {
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(50));
            }
            spawned => break spawned.expect("spawn"),
        }
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found = inspect(&[term.processes()], &known()).pop().unwrap();
        // macOS can name the process before its arguments can be read, just after its exec: a
        // later pass reads them, as Templum's own inspections, which run again, do.
        if found.agent.as_deref() == Some("pi") && found.session.is_some() {
            assert_eq!(found.session.as_deref(), Some(id));
            break;
        }
        assert!(Instant::now() < deadline, "found {found:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The usual case: the user starts the agent from the shell, in another folder than the shell's.
#[test]
fn inspect_finds_an_agent_the_shell_started() {
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let sub = work.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let claude = fake_claude(bin.path());
    #[cfg(unix)]
    let command = ("sh".to_string(), vec!["-i".to_string()]);
    // The outer cmd stays in `work`; the agent (a cmd copy) moves to `sub` and waits.
    #[cfg(windows)]
    let command = (
        "cmd".to_string(),
        [
            "/K",
            &claude.display().to_string(),
            "/K",
            "cd",
            "/d",
            &sub.display().to_string(),
        ]
        .map(String::from)
        .to_vec(),
    );
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 200,
            rows: 5,
            cell_px: (8, 16),
            command: Some(command),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: Some(work.path().to_path_buf()),
            integration: None,
            env: Vec::new(),
        },
        Arc::new(|| {}),
    )
    .expect("spawn");
    // A job of the interactive shell, in its own folder; the shell stays in `work`.
    #[cfg(unix)]
    term.write(
        format!(
            "sh -c 'cd \"{}\" && exec \"{}\" 30'\r",
            sub.display(),
            claude.display()
        )
        .into_bytes(),
    );
    let want = sub.canonicalize().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found = inspect(&[term.processes()], &known()).pop().unwrap();
        let folder = found.folder.as_ref().and_then(|f| f.canonicalize().ok());
        if found.agent.as_deref() == Some("claude") && folder.as_ref() == Some(&want) {
            break;
        }
        assert!(Instant::now() < deadline, "found {found:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Templum's own agents, as the app gives them to `inspect`.
fn known() -> Vec<templum_terminal::Detect> {
    Agent::all().map(Agent::detect).collect()
}

/// Processes that ignore the hangup a closing terminal sends still end: those read with `tree`
/// before are killed by `kill_left`.
#[cfg(unix)]
#[test]
fn a_tree_is_ended_even_when_it_ignores_hangup() {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 40,
            rows: 5,
            cell_px: (8, 16),
            command: Some((
                "sh".to_string(),
                vec![
                    "-c".to_string(),
                    "trap '' HUP; sleep 1000 & sleep 1000".to_string(),
                ],
            )),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: None,
            integration: None,
            env: Vec::new(),
        },
        Arc::new(|| {}),
    )
    .expect("spawn");
    let shell = term.processes().shell.expect("the shell's pid");
    let deadline = Instant::now() + Duration::from_secs(10);
    let tree = loop {
        let tree = templum_terminal::tree(&[shell]);
        if tree.len() >= 2 {
            break tree;
        }
        assert!(
            Instant::now() < deadline,
            "the sleeps never started: {tree:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    assert!(tree.iter().any(|r| r.pid == shell), "the shell is in it");
    let alive = || {
        let pids: Vec<Pid> = tree.iter().map(|r| Pid::from_u32(r.pid)).collect();
        let mut system = System::new();
        system
            .refresh_processes_specifics(ProcessesToUpdate::Some(&pids), ProcessRefreshKind::new());
        tree.iter()
            .filter(|r| {
                system.process(Pid::from_u32(r.pid)).is_some_and(|p| {
                    p.start_time() == r.started && p.status() != ProcessStatus::Zombie
                })
            })
            .count()
    };
    drop(term);
    std::thread::sleep(Duration::from_millis(300));
    assert!(alive() > 0, "the hangup was ignored");
    templum_terminal::kill_left(&tree);
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive() > 0 {
        assert!(Instant::now() < deadline, "{} still running", alive());
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The process among `parent`'s children whose arguments end with `last`, by sysinfo.
#[cfg(unix)]
fn child_running(parent: u32, last: &str) -> u32 {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        ProcessRefreshKind::new().with_cmd(UpdateKind::Always),
    );
    system
        .processes()
        .iter()
        .find(|(_, p)| {
            p.parent() == Some(Pid::from_u32(parent))
                && p.cmd().last().is_some_and(|a| a.to_string_lossy() == last)
        })
        .map(|(pid, _)| pid.as_u32())
        .unwrap_or_else(|| panic!("no child of {parent} ends with {last}"))
}

/// `pid` runs, and isn't a zombie.
#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid]), ProcessRefreshKind::new());
    system
        .process(pid)
        .is_some_and(|p| p.status() != ProcessStatus::Zombie)
}

/// The processes carrying `TEMPLUM_TERMINAL=id` whose arguments end with `last`.
#[cfg(unix)]
fn carriers(id: &str, last: &str) -> Vec<templum_terminal::Running> {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        ProcessRefreshKind::new()
            .with_cmd(UpdateKind::Always)
            .with_environ(UpdateKind::Always),
    );
    let variable = format!("TEMPLUM_TERMINAL={id}");
    system
        .processes()
        .values()
        .filter(|p| {
            p.environ().iter().any(|v| *v == *variable)
                && p.cmd().last().is_some_and(|a| a.to_string_lossy() == last)
        })
        .map(|p| templum_terminal::Running {
            pid: p.pid().as_u32(),
            started: p.start_time(),
        })
        .collect()
}

/// What `stop_agent` ends: the program in front, what runs below it, and what a tool's shell left
/// in a session of its own (as Claude Code's Bash tool does) carrying the terminal's variable,
/// started after the program; never the shell, a job the person started from it, nor a daemon
/// started before the program (an ssh-agent from the shell's startup files).
#[cfg(unix)]
#[test]
fn strays_are_the_program_what_it_left_in_a_session_of_its_own_and_never_the_shell() {
    let dir = tempfile::tempdir().unwrap();
    let flag = dir.path().join("f");
    // Unique: only what this test started carries it.
    let id = format!("t-strays-{}", std::process::id());
    // `setsid` through Perl: macOS has no `setsid` command.
    const SETSID: &str = "perl -MPOSIX -e \"setsid; exec @ARGV\"";
    // Seconds apart: start times are whole seconds.
    let script = format!(
        "({SETSID} sleep 37 &); sleep 31 & sleep 1.2; \
         sh -c 'sleep 1.2; ({SETSID} sh -c \"sleep 2 && touch {}\" &); exec sleep 33' & wait",
        flag.display()
    );
    let mut shell = std::process::Command::new("sh")
        .args(["-c", &script])
        .env("TEMPLUM_TERMINAL", &id)
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(2800));
    let program = child_running(shell.id(), "33");
    let person = child_running(shell.id(), "31");
    let daemon = carriers(&id, "37");
    assert_eq!(daemon.len(), 1, "the daemon started");
    let found = templum_terminal::strays(&id, &[shell.id()], Some(program), None);
    let pids: Vec<u32> = found.iter().map(|r| r.pid).collect();
    assert!(pids.contains(&program), "{pids:?}");
    assert!(
        !pids.contains(&shell.id()) && !pids.contains(&person),
        "{pids:?}"
    );
    assert!(
        !pids.contains(&daemon[0].pid),
        "started before the program: {pids:?}"
    );
    assert!(pids.len() >= 3, "the orphan sh and its sleep: {pids:?}");
    templum_terminal::terminate(&found);
    std::thread::sleep(Duration::from_millis(2500));
    assert!(!flag.exists(), "the orphan wrote its file");
    assert!(process_alive(person), "the person's job stays");
    assert!(process_alive(daemon[0].pid), "the daemon stays");
    // Without a program, only what started after `since`: here nothing.
    let since = daemon[0].started;
    assert!(templum_terminal::strays(&id, &[shell.id()], None, Some(since)).is_empty());
    templum_terminal::kill_left(&daemon);
    templum_terminal::kill_left(&templum_terminal::tree(&[shell.id()]));
    let _ = shell.wait();
}

#[test]
fn the_shell_is_logins_child_when_the_terminal_runs_login() {
    let children = [(11, "/bin/zsh".to_string()), (12, "/bin/sleep".to_string())];
    assert_eq!(
        templum_terminal::shells_of(10, "login", "/usr/bin/login", &children),
        [10, 11]
    );
    assert_eq!(
        templum_terminal::shells_of(10, "zsh", "/bin/zsh", &children[1..]),
        [10]
    );
}

/// Git for Windows' `bin\bash.exe` only starts the real shell, `usr\bin\bash.exe`: both are the
/// terminal's shell, never what `stop_agent` ends. A bash run directly (MSYS2's) starts none.
#[test]
fn the_shell_is_the_real_bash_git_bash_starts() {
    let real = (11, "C:\\Program Files\\Git\\usr\\bin\\bash.exe".to_string());
    let wrapper = "C:\\Program Files\\Git\\bin\\bash.exe";
    assert_eq!(
        templum_terminal::shells_of(10, "bash.exe", wrapper, std::slice::from_ref(&real)),
        [10, 11]
    );
    let sh = "C:\\Program Files\\Git\\bin\\SH.EXE";
    assert_eq!(
        templum_terminal::shells_of(10, "sh.exe", sh, std::slice::from_ref(&real)),
        [10, 11]
    );
    let agent = (12, "C:\\Users\\u\\.local\\bin\\claude.exe".to_string());
    assert_eq!(
        templum_terminal::shells_of(11, "bash.exe", &real.1, &[agent]),
        [11]
    );
    let msys = "C:\\msys64\\usr\\bin\\bash.exe";
    let nested = (12, msys.to_string());
    assert_eq!(
        templum_terminal::shells_of(10, "bash.exe", msys, &[nested]),
        [10]
    );
}

/// An IME that clears its composition replaces the marked text with nothing (a dead key, then
/// Backspace): the empty text reaches no program, and what is typed after it still does.
#[cfg(unix)]
#[test]
fn empty_text_does_not_block_what_is_typed_after_it() {
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 40,
            rows: 5,
            cell_px: (8, 16),
            command: Some((
                "sh".to_string(),
                vec![
                    "-c".to_string(),
                    "read l; printf 'got:%s' \"$l\"".to_string(),
                ],
            )),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: None,
            integration: None,
            env: Vec::new(),
        },
        Arc::new(|| {}),
    )
    .expect("spawn");
    term.text("");
    term.text("hi\r");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !term.exited() {
        assert!(Instant::now() < deadline, "what was typed never arrived");
        std::thread::sleep(Duration::from_millis(50));
        term.shared().rearm();
    }
    let mut snapshot = Snapshot::default();
    snapshot.refresh(term.shared(), &Palette::default());
    assert!(
        snapshot.text().contains("got:hi"),
        "screen was: {:?}",
        snapshot.text()
    );
}

/// The cgroup `/proc/<pid>/cgroup` names (cgroup v2: one line, `0::<path>`).
#[cfg(target_os = "linux")]
fn cgroup(pid: &str) -> String {
    std::fs::read_to_string(format!("/proc/{pid}/cgroup"))
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// A systemd user manager answers, and runs this process, so it can move its children: not in a
/// CI container, nor under an SSH login, whose session the system's manager owns.
#[cfg(target_os = "linux")]
fn user_manager() -> bool {
    let answers = std::process::Command::new("systemctl")
        .args(["--user", "is-system-running"])
        .output()
        .is_ok_and(|o| {
            let state = String::from_utf8_lossy(&o.stdout);
            matches!(
                state.trim(),
                "running" | "degraded" | "starting" | "initializing"
            )
        });
    answers && cgroup("self").contains(".service/")
}

/// Each terminal's shell, and what it starts, runs in a systemd user scope of its own, not in
/// Templum's cgroup: systemd-oomd, which kills a whole scope, ends that terminal alone (card 173).
#[cfg(target_os = "linux")]
#[test]
fn each_terminal_runs_in_a_systemd_scope_of_its_own() {
    if !user_manager() {
        eprintln!("skipped: no systemd user manager runs this test");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let terms = [long_lived(dir.path()), long_lived(dir.path())];
    let deadline = Instant::now() + Duration::from_secs(10);
    for term in &terms {
        let shell = term.processes().shell.expect("a pid").to_string();
        let scope = format!("/templum-term-{}.scope", term.id());
        while !cgroup(&shell).ends_with(&scope) {
            assert!(
                Instant::now() < deadline,
                "the shell is in {:?}, not in {scope}",
                cgroup(&shell)
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_ne!(cgroup(&shell), cgroup("self"));
    }
    // What oomd does to the first one's scope.
    let killed = std::process::Command::new("systemctl")
        .args(["--user", "kill", "--signal=SIGKILL"])
        .arg(format!("templum-term-{}.scope", terms[0].id()))
        .status()
        .unwrap();
    assert!(killed.success());
    while !terms[0].exited() {
        assert!(Instant::now() < deadline, "the killed scope's shell runs");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(terms[0].killed());
    assert!(!terms[1].exited(), "the other terminal lives");
}

/// A shell that ends at once leaves no scope behind: systemd misses the emptying of some of them,
/// which would stay running, empty, until the person logs out.
#[cfg(target_os = "linux")]
#[test]
fn a_shell_that_ends_at_once_leaves_no_scope() {
    if !user_manager() {
        eprintln!("skipped: no systemd user manager runs this test");
        return;
    }
    let terms: Vec<Terminal> = (0..20)
        .map(|_| {
            Terminal::spawn(
                SpawnOptions {
                    cols: 40,
                    rows: 5,
                    cell_px: (8, 16),
                    command: Some(("sh".into(), vec!["-c".into(), "exit 0".into()])),
                    palette: Palette::default(),
                    options: TermOptions::default(),
                    working_directory: None,
                    integration: None,
                    env: Vec::new(),
                },
                Arc::new(|| {}),
            )
            .expect("spawn")
        })
        .collect();
    let units = || {
        let listed = std::process::Command::new("systemctl")
            .args(["--user", "list-units", "--all", "--plain", "--no-legend"])
            .arg("templum-term-*")
            .output()
            .unwrap();
        let listed = String::from_utf8_lossy(&listed.stdout).into_owned();
        terms
            .iter()
            .map(|t| format!("templum-term-{}.scope", t.id()))
            .filter(|unit| listed.contains(unit.as_str()))
            .collect::<Vec<_>>()
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while !units().is_empty() {
        assert!(Instant::now() < deadline, "left: {:?}", units());
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Without a user bus (no systemd, a container), a terminal opens as before: its shell runs in
/// the cgroup of the process that opened it, and stderr says why, once.
#[cfg(target_os = "linux")]
#[test]
fn without_a_user_bus_a_terminal_runs_where_templum_does() {
    const NAME: &str = "without_a_user_bus_a_terminal_runs_where_templum_does";
    if std::env::var_os("TEMPLUM_TEST_NO_BUS").is_none() {
        // Again in a process whose bus isn't there: without the variable, `/run/user/<uid>/bus`.
        let none = tempfile::tempdir().unwrap();
        let run = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env("TEMPLUM_TEST_NO_BUS", "1")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                format!("unix:path={}", none.path().join("bus").display()),
            )
            .output()
            .unwrap();
        let (out, err) = (
            String::from_utf8_lossy(&run.stdout),
            String::from_utf8_lossy(&run.stderr),
        );
        assert!(
            run.status.success() && out.contains("1 passed"),
            "{out}{err}"
        );
        assert_eq!(
            err.matches("terminals stay in Templum's own cgroup")
                .count(),
            1,
            "{err}"
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let terms = [long_lived(dir.path()), long_lived(dir.path())];
    let ours = cgroup("self");
    // The scope's thread gives up at once; a second to see that nothing moved them.
    let until = Instant::now() + Duration::from_secs(1);
    while Instant::now() < until {
        for term in &terms {
            let shell = term.processes().shell.expect("a pid").to_string();
            assert!(!term.exited(), "the shell runs");
            assert_eq!(cgroup(&shell), ours);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A shell killed (SIGKILL), as systemd-oomd ends a scope, says so; one that exits doesn't.
#[cfg(unix)]
#[test]
fn a_killed_shell_says_so() {
    for (script, killed) in [("kill -9 $$", true), ("exit 3", false)] {
        let term = Terminal::spawn(
            SpawnOptions {
                cols: 40,
                rows: 5,
                cell_px: (8, 16),
                command: Some(("sh".into(), vec!["-c".into(), script.into()])),
                palette: Palette::default(),
                options: TermOptions::default(),
                working_directory: None,
                integration: None,
                env: Vec::new(),
            },
            Arc::new(|| {}),
        )
        .expect("spawn");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !term.exited() {
            assert!(Instant::now() < deadline, "{script}: never ended");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(term.killed(), killed, "{script}");
    }
}

/// A program's output is read with nothing painting: the PTY thread never waits on the UI, so an
/// agent can't stall on a full PTY while the UI thread is stuck (kanban card 291).
#[cfg(unix)]
#[test]
fn output_is_read_while_nothing_paints() {
    // Megabytes: far past what a PTY holds unread.
    let script = "head -c 4000000 /dev/zero | tr '\\0' a";
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 80,
            rows: 24,
            cell_px: (8, 16),
            command: Some(("sh".into(), vec!["-c".into(), script.into()])),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: None,
            integration: None,
            env: Vec::new(),
        },
        // No UI: the wake-up goes nowhere, and nothing is ever re-armed or copied.
        Arc::new(|| {}),
    )
    .expect("spawn");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !term.exited() {
        assert!(
            Instant::now() < deadline,
            "the program blocked on its output"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
