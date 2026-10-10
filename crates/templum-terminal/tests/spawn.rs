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
        },
        notify,
    )
    .expect("spawn");

    let deadline = Instant::now() + Duration::from_secs(15);
    while !term.exited() {
        assert!(Instant::now() < deadline, "the command never exited");
        let _ = rx.recv_timeout(Duration::from_millis(100));
        term.rearm();
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
        },
        notify,
    )
    .expect("spawn");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !term.exited() {
        assert!(Instant::now() < deadline, "the command never exited");
        std::thread::sleep(Duration::from_millis(50));
        term.rearm();
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
        let found: Vec<_> = inspect(&processes)
            .into_iter()
            .map(|i| i.folder.and_then(|f| f.canonicalize().ok()))
            .collect();
        if found == want.iter().cloned().map(Some).collect::<Vec<_>>() {
            assert!(
                inspect(&processes).iter().all(|i| i.agent.is_none()),
                "a shell isn't an agent"
            );
            break;
        }
        assert!(Instant::now() < deadline, "found {found:?}, want {want:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(inspect(&[]).is_empty());
}

#[test]
fn inspect_finds_an_agent_and_its_folder() {
    let bin = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    // A harmless long-running program, copied under an agent's name.
    #[cfg(unix)]
    let command = {
        let sleep = ["/bin/sleep", "/usr/bin/sleep"]
            .into_iter()
            .find(|p| std::path::Path::new(p).exists())
            .expect("sleep");
        let to = bin.path().join("claude");
        std::fs::copy(sleep, &to).unwrap();
        (to.display().to_string(), vec!["30".to_string()])
    };
    #[cfg(windows)]
    let command = {
        let to = bin.path().join("claude.exe");
        std::fs::copy(r"C:\Windows\System32\cmd.exe", &to).unwrap();
        (to.display().to_string(), vec!["/K".to_string()])
    };
    let term = Terminal::spawn(
        SpawnOptions {
            cols: 40,
            rows: 5,
            cell_px: (8, 16),
            command: Some(command),
            palette: Palette::default(),
            options: TermOptions::default(),
            working_directory: Some(work.path().to_path_buf()),
        },
        Arc::new(|| {}),
    )
    .expect("spawn");
    let want = work.path().canonicalize().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let found = inspect(&[term.processes()]).pop().unwrap();
        let folder = found.folder.as_ref().and_then(|f| f.canonicalize().ok());
        if found.agent == Some(Agent::Claude) && folder.as_ref() == Some(&want) {
            break;
        }
        assert!(Instant::now() < deadline, "found {found:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}
