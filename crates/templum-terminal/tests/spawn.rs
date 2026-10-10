//! Runs a real command in a real PTY.

use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use templum_terminal::{Notify, Palette, Snapshot, SpawnOptions, Terminal};

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
