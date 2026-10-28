//! The UI thread's watchdog (Linux, kanban card 291): when the UI thread stops answering, Templum
//! restarts itself.
//!
//! GPUI draws on the UI thread, and wgpu waits there for the GPU without a timeout: reconfiguring
//! the window's surface (wgpu-core's `configure_surface`, `PollType::wait_indefinitely`), and
//! releasing its swapchain when the window resizes or closes (`vkDeviceWaitIdle`). After a sleep
//! the GPU driver may never finish the work it had (NVIDIA on Wayland, after resume), and that
//! wait never ends: no frame, no key, no close. The terminals' PTY threads go on, but nothing
//! reaches the person, and agents wait forever on the UI for Templum's tools. Another thread
//! notices it and starts a new Templum, which restores the canvas from its last snapshot (written
//! every 5 seconds) and resumes the agents, as any restart does. See docs/specs/persistence.md.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use gpui_kit::App;

/// How often the UI thread beats, and the watchdog looks.
const TICK: Duration = Duration::from_secs(1);
/// Looks without a beat before Templum restarts. A frame takes milliseconds: this is a UI thread
/// that won't come back. Looks, not time: the watchdog sleeps with the computer, so a sleep
/// counts nothing.
const STALL: u32 = 30;
/// Looks without a beat once Templum quits: what it saves on quit was written first.
const QUIT_STALL: u32 = 10;
/// The flag a Templum restarted by its watchdog starts with: it says so, and a second stall quits
/// instead of restarting again (a GPU no new process can use either).
pub(crate) const AFTER_STALL: &str = "--after-stall";

/// The UI thread's beats, counted by a timer on its own executor.
static BEAT: AtomicU64 = AtomicU64::new(0);
/// Templum began quitting.
static QUITTING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, PartialEq)]
enum Act {
    Restart,
    Quit,
}

/// What the watchdog last saw: the beat, and how many looks it stayed the same.
#[derive(Default)]
struct Watch {
    beat: u64,
    still: u32,
}

impl Watch {
    /// One look at the beat. Nothing before the first one: Templum is still starting.
    fn look(&mut self, beat: u64, quitting: bool, restarted: bool) -> Option<Act> {
        if beat != self.beat {
            self.beat = beat;
            self.still = 0;
            return None;
        }
        if beat == 0 {
            return None;
        }
        self.still += 1;
        let limit = if quitting { QUIT_STALL } else { STALL };
        (self.still >= limit).then_some(if quitting || restarted {
            Act::Quit
        } else {
            Act::Restart
        })
    }
}

/// Starts the beat on the UI thread and the watchdog on its own. `restarted`: this Templum was
/// started by a watchdog.
pub(crate) fn start(restarted: bool, cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(TICK).await;
            BEAT.fetch_add(1, Ordering::Relaxed);
        }
    })
    .detach();
    cx.on_app_quit(|_| {
        quitting();
        async {}
    })
    .detach();
    let watching = std::thread::Builder::new()
        .name("templum-watchdog".into())
        .spawn(move || {
            let mut watch = Watch::default();
            loop {
                std::thread::sleep(TICK);
                let quitting = QUITTING.load(Ordering::Relaxed);
                match watch.look(BEAT.load(Ordering::Relaxed), quitting, restarted) {
                    None => {}
                    Some(Act::Restart) => {
                        eprintln!(
                            "templum: the window stopped answering for {STALL} seconds (its GPU \
                             driver never finished a frame): restarting"
                        );
                        restart();
                        end(1);
                    }
                    Some(Act::Quit) => {
                        eprintln!("templum: the window stopped answering: quitting");
                        end(i32::from(!quitting));
                    }
                }
            }
        });
    if let Err(err) = watching {
        eprintln!("templum: couldn't start the watchdog: {err}");
    }
}

/// Templum quits: a stall from now on ends it instead of restarting it.
pub(crate) fn quitting() {
    QUITTING.store(true, Ordering::Relaxed);
}

/// Starts this Templum again, with its own arguments and AFTER_STALL. It takes the data folder
/// once this one has ended, waiting for it as for one quitting (session::QUITTING).
fn restart() {
    let started = std::env::current_exe().and_then(|exe| {
        std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .arg(AFTER_STALL)
            .spawn()
    });
    if let Err(err) = started {
        eprintln!("templum: couldn't restart: {err}");
    }
}

/// Ends Templum at once. Not `std::process::exit`: it runs the libraries' exit handlers, and the
/// GPU driver's could wait on the stuck UI thread too.
fn end(code: i32) -> ! {
    // SAFETY: `_exit` takes no pointer and only ends the process; nothing of Rust runs after it.
    #[allow(unsafe_code)]
    unsafe {
        libc::_exit(code)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` looks at the same beat; the last one's act.
    fn looks(watch: &mut Watch, n: u32, beat: u64, quitting: bool, restarted: bool) -> Option<Act> {
        (0..n)
            .map(|_| watch.look(beat, quitting, restarted))
            .last()
            .flatten()
    }

    #[test]
    fn a_beating_ui_thread_is_never_restarted() {
        let mut watch = Watch::default();
        for beat in 1..=10 * u64::from(STALL) {
            assert_eq!(watch.look(beat, false, false), None);
        }
    }

    #[test]
    fn a_ui_thread_still_for_the_stall_restarts_templum() {
        let mut watch = Watch::default();
        assert_eq!(watch.look(7, false, false), None, "a new beat");
        assert_eq!(looks(&mut watch, STALL - 1, 7, false, false), None);
        assert_eq!(watch.look(7, false, false), Some(Act::Restart));
    }

    #[test]
    fn a_beat_starts_the_count_again() {
        let mut watch = Watch::default();
        watch.look(1, false, false);
        looks(&mut watch, STALL - 1, 1, false, false);
        assert_eq!(watch.look(2, false, false), None);
        assert_eq!(looks(&mut watch, STALL - 1, 2, false, false), None);
    }

    #[test]
    fn nothing_happens_before_the_first_beat() {
        // Restoring a big canvas blocks the UI thread before its first beat.
        let mut watch = Watch::default();
        assert_eq!(looks(&mut watch, 10 * STALL, 0, false, false), None);
    }

    #[test]
    fn a_stall_while_quitting_quits_sooner_and_never_restarts() {
        let mut watch = Watch::default();
        watch.look(3, true, false);
        assert_eq!(looks(&mut watch, QUIT_STALL - 1, 3, true, false), None);
        assert_eq!(watch.look(3, true, false), Some(Act::Quit));
    }

    #[test]
    fn a_templum_restarted_after_a_stall_quits_at_the_next_one() {
        let mut watch = Watch::default();
        watch.look(5, false, true);
        assert_eq!(looks(&mut watch, STALL, 5, false, true), Some(Act::Quit));
    }
}
