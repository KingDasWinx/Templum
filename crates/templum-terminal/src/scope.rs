//! Each terminal's processes in a systemd user scope of their own (Linux), as GNOME Terminal's
//! are (VTE's `vte-spawn-<uuid>.scope`). Without one, every shell, agent and build lives in
//! Templum's own cgroup, and systemd-oomd, which ends a whole unit under memory pressure, ends the
//! window with every terminal in it (kanban card 173,
//! docs/specs/shells.md#a-scope-for-each-terminal).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const SYSTEMD: &str = "org.freedesktop.systemd1";
const MANAGER: &str = "org.freedesktop.systemd1.Manager";

/// How long after its scope's start a shell's end is checked for ([`start`]).
const SETTLING: Duration = Duration::from_secs(1);

/// Moves `pid`, the shell of the terminal `id` running `shell`, into a new scope,
/// `templum-term-<id>.scope`, on a thread of its own: opening a terminal never waits for systemd.
/// Without a user bus or systemd on it, or when systemd refuses, the shell stays in Templum's
/// cgroup, as before, with one line on stderr for the first failure of the run.
pub(crate) fn move_to_scope(pid: u32, id: String, shell: String) {
    let moved = std::thread::Builder::new()
        .name("templum-scope".into())
        .spawn(move || {
            let made = bus().and_then(|bus| {
                async_io::block_on(start(bus, pid, &id, &shell)).map_err(|e| e.to_string())
            });
            static TOLD: AtomicBool = AtomicBool::new(false);
            if let Err(err) = made
                && !TOLD.swap(true, Ordering::Relaxed)
            {
                eprintln!("templum: terminals stay in Templum's own cgroup: {err}");
            }
        });
    if let Err(err) = moved {
        eprintln!("templum: a terminal stays in Templum's own cgroup: {err}");
    }
}

/// The user's session bus, where `systemd --user` answers; connected once.
fn bus() -> Result<&'static zbus::Connection, String> {
    static BUS: OnceLock<Result<zbus::Connection, String>> = OnceLock::new();
    BUS.get_or_init(|| async_io::block_on(zbus::Connection::session()).map_err(|e| e.to_string()))
        .as_ref()
        .map_err(Clone::clone)
}

/// Asks the user manager to start the scope, as `systemd-run --user --scope --collect` does.
/// What `pid` forked before the scope exists stays where it was: its shell's first startup
/// commands, at most.
///
/// systemd (261) misses the emptying of a scope whose only process ends as it is moved: a shell
/// that ends at once would leave its scope running, empty, until the person logs out. One that
/// ends within [`SETTLING`] has its scope stopped, when nothing runs in it.
async fn start(bus: &zbus::Connection, pid: u32, id: &str, shell: &str) -> zbus::Result<()> {
    let name = format!("templum-term-{id}.scope");
    let properties: Vec<(&str, Value)> = vec![
        (
            "Description",
            Value::from(format!("Templum terminal {id} ({shell})")),
        ),
        ("PIDs", Value::from(vec![pid])),
        // Gone once its processes are, even when oomd ended them: no failed units pile up.
        ("CollectMode", Value::from("inactive-or-failed")),
    ];
    let aux: Vec<(&str, Vec<(&str, Value)>)> = Vec::new();
    let manager = "/org/freedesktop/systemd1";
    let start = (name.as_str(), "fail", properties, aux);
    bus.call_method(
        Some(SYSTEMD),
        manager,
        Some(MANAGER),
        "StartTransientUnit",
        &start,
    )
    .await?;
    let until = Instant::now() + SETTLING;
    let proc = format!("/proc/{pid}");
    while std::path::Path::new(&proc).exists() {
        if Instant::now() >= until {
            return Ok(());
        }
        async_io::Timer::after(Duration::from_millis(20)).await;
    }
    // Gone already (a scope that failed, or one systemd collected): nothing to stop.
    let Ok(unit) = bus
        .call_method(
            Some(SYSTEMD),
            manager,
            Some(MANAGER),
            "GetUnit",
            &(name.as_str(),),
        )
        .await
    else {
        return Ok(());
    };
    let unit: OwnedObjectPath = unit.body().deserialize()?;
    let tasks: OwnedValue = bus
        .call_method(
            Some(SYSTEMD),
            &unit,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.systemd1.Scope", "TasksCurrent"),
        )
        .await?
        .body()
        .deserialize()?;
    // Without task accounting it reads u64::MAX: left alone.
    if u64::try_from(tasks) == Ok(0) {
        bus.call_method(
            Some(SYSTEMD),
            manager,
            Some(MANAGER),
            "StopUnit",
            &(name.as_str(), "replace"),
        )
        .await?;
    }
    Ok(())
}
