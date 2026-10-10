//! A shell in a PTY, using alacritty_terminal's own tty and event loop.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode, viewport_to_point};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::CursorStyle;

use crate::input::{KeyInput, encode_key, paste_bytes};
use crate::palette::{self, Palette};

/// Called (coalesced) when a terminal has something new to show.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// State shared between the PTY thread (through [`Listener`]) and the UI thread.
pub(crate) struct Shared {
    notify: Notify,
    /// Set by the first notification; cleared by [`Terminal::rearm`].
    pending: AtomicBool,
    title: Mutex<Option<String>>,
    exited: AtomicBool,
    /// Reported to CSI 14 t (text area size) queries.
    size: Mutex<WindowSize>,
    /// PTY input: set right after the event loop exists, since the Term needs the listener first.
    pty: OnceLock<EventLoopSender>,
    /// Answers color queries (OSC 4/10/11); the app updates it on theme changes.
    palette: Mutex<Palette>,
}

impl Shared {
    fn new(notify: Notify, size: WindowSize, palette: Palette) -> Self {
        Self {
            notify,
            pending: AtomicBool::new(false),
            title: Mutex::new(None),
            exited: AtomicBool::new(false),
            size: Mutex::new(size),
            pty: OnceLock::new(),
            palette: Mutex::new(palette),
        }
    }

    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            (self.notify)();
        }
    }

    fn reply(&self, text: String) {
        if let Some(pty) = self.pty.get() {
            let _ = pty.send(Msg::Input(Cow::Owned(text.into_bytes())));
        }
    }

    /// The last OSC 0/2 title, unless blank.
    fn title(&self) -> Option<String> {
        self.title
            .lock()
            .unwrap()
            .clone()
            .filter(|t| !t.trim().is_empty())
    }
}

#[derive(Clone)]
pub(crate) struct Listener(Arc<Shared>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let s = &self.0;
        match event {
            // Replies to queries go straight back to the shell, off the UI thread.
            Event::PtyWrite(text) => s.reply(text),
            // ponytail: answers the theme's colors, not OSC-overridden ones (the Term is locked here).
            Event::ColorRequest(index, format) => {
                let color = palette::query(index, &s.palette.lock().unwrap());
                s.reply(format(color))
            }
            Event::TextAreaSizeRequest(format) => s.reply(format(*s.size.lock().unwrap())),
            Event::Title(title) => {
                *s.title.lock().unwrap() = Some(title);
                s.wake();
            }
            Event::ResetTitle => {
                *s.title.lock().unwrap() = None;
                s.wake();
            }
            // Exit comes after the child's last output has been read (ChildExit comes before).
            Event::Exit => {
                s.exited.store(true, Ordering::Release);
                s.wake();
            }
            Event::Wakeup => s.wake(),
            // Later: bell, OSC 52 clipboard (needs a security review), cursor blinking.
            _ => {}
        }
    }
}

/// The terminal grid, shared with the PTY thread. Cheap to clone.
#[derive(Clone)]
pub struct SharedTerm(pub(crate) Arc<FairMutex<Term<Listener>>>);

/// Settings a running terminal can change: the history length and the default cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermOptions {
    /// Lines of history.
    pub scrollback: usize,
    /// The cursor when the program doesn't ask for another one.
    pub cursor: CursorStyle,
}

impl Default for TermOptions {
    fn default() -> Self {
        Self {
            scrollback: 10_000,
            cursor: CursorStyle::default(),
        }
    }
}

impl TermOptions {
    fn config(self) -> Config {
        Config {
            scrolling_history: self.scrollback,
            default_cursor_style: self.cursor,
            ..Config::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    /// Drag.
    Simple,
    /// Double-click: the word under the pointer.
    Word,
    /// Triple-click: whole lines.
    Line,
}

impl SharedTerm {
    /// Applies new options to the running terminal (alacritty trims the history if needed).
    pub fn set_options(&self, options: TermOptions) {
        self.0.lock().set_options(options.config());
    }

    /// Starts a selection at a viewport cell (row 0 = top visible line).
    pub fn select_start(&self, row: usize, col: usize, side: Side, kind: SelectionKind) {
        let mut term = self.0.lock();
        let point = grid_point(&term, row, col);
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
        };
        term.selection = Some(Selection::new(ty, point, side));
    }

    pub fn select_update(&self, row: usize, col: usize, side: Side) {
        let mut term = self.0.lock();
        let point = grid_point(&term, row, col);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    pub fn select_clear(&self) {
        self.0.lock().selection = None;
    }

    /// The selected text; None when nothing (or only an empty range) is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.0
            .lock()
            .selection_to_string()
            .filter(|t| !t.is_empty())
    }
}

/// A viewport cell, clamped to the grid, as a grid point (accounting for scrollback).
fn grid_point(term: &Term<Listener>, row: usize, col: usize) -> GridPoint {
    let row = row.min(term.screen_lines() - 1);
    let col = col.min(term.columns() - 1);
    viewport_to_point(
        term.grid().display_offset(),
        GridPoint::new(row, Column(col)),
    )
}

/// A terminal's processes, to read from any thread with [`inspect`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Processes {
    /// The process Templum started.
    pub shell: Option<u32>,
    /// The PTY's foreground process (Unix): the shell, or the program it runs.
    pub foreground: Option<u32>,
}

/// What a terminal's processes tell about it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inspection {
    /// The current folder of the program in the foreground (see [`Terminal::cwd`]).
    pub folder: Option<PathBuf>,
}

/// Reads several terminals' processes in one pass over the OS's process table: slow enough to
/// keep off the UI thread when there are many terminals.
pub fn inspect(terminals: &[Processes]) -> Vec<Inspection> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let pids: Vec<Pid> = terminals
        .iter()
        .flat_map(|p| [p.foreground, p.shell])
        .flatten()
        .map(Pid::from_u32)
        .collect();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        ProcessRefreshKind::new().with_cwd(UpdateKind::Always),
    );
    let cwd = |pid: u32| {
        system
            .process(Pid::from_u32(pid))?
            .cwd()
            .filter(|cwd| !cwd.as_os_str().is_empty())
            .map(Path::to_path_buf)
    };
    terminals
        .iter()
        .map(|p| Inspection {
            folder: p.foreground.and_then(cwd).or_else(|| p.shell.and_then(cwd)),
        })
        .collect()
}

pub struct SpawnOptions {
    pub cols: u16,
    pub rows: u16,
    /// Cell size in pixels, reported to the shell.
    pub cell_px: (u16, u16),
    /// Program and arguments to run instead of the user's login shell (tests).
    pub command: Option<(String, Vec<String>)>,
    /// Colors reported to programs that ask (vim, neovim). Change them later with
    /// [`Terminal::set_palette`].
    pub palette: Palette,
    /// Scrollback and default cursor; change them later with [`SharedTerm::set_options`].
    pub options: TermOptions,
    /// Where the shell starts; None: the home folder.
    pub working_directory: Option<PathBuf>,
}

pub struct Terminal {
    term: SharedTerm,
    /// The shell's process ID, when the OS gave one.
    pid: Option<u32>,
    /// The PTY's master side, to ask which process is in the foreground.
    #[cfg(unix)]
    master: Option<std::fs::File>,
    shared: Arc<Shared>,
    pty: EventLoopSender,
    cols: u16,
    rows: u16,
    shell_name: String,
}

impl Terminal {
    /// Starts the user's login shell (PowerShell on Windows), or `opts.command`, in a PTY.
    /// `notify` is called, coalesced, on new output, title changes and exit.
    pub fn spawn(opts: SpawnOptions, notify: Notify) -> io::Result<Self> {
        let size = window_size(opts.cols, opts.rows, opts.cell_px);
        let shell_name = match &opts.command {
            Some((program, _)) => file_stem(program),
            None => default_shell_name(),
        };
        // Windows has one more field (escape_args): the update is only needless elsewhere.
        #[allow(clippy::needless_update)]
        let options = tty::Options {
            shell: opts
                .command
                .map(|(program, args)| tty::Shell::new(program, args)),
            env: HashMap::from([
                ("TERM".into(), "xterm-256color".into()),
                ("COLORTERM".into(), "truecolor".into()),
            ]),
            // Like other GUI terminals: a launch from Finder or the Start menu doesn't leave
            // shells in / or the install folder.
            working_directory: opts.working_directory.clone().or_else(std::env::home_dir),
            // Show everything the shell printed before reporting its exit.
            drain_on_exit: true,
            ..Default::default()
        };
        let shared = Arc::new(Shared::new(notify, size, opts.palette));
        let listener = Listener(shared.clone());
        let pty = tty::new(&options, size, 0)?;
        // The shell's process, to ask the OS for its current folder later.
        #[cfg(unix)]
        let pid = Some(pty.child().id());
        #[cfg(unix)]
        let master = pty.file().try_clone().ok();
        #[cfg(windows)]
        let pid = pty.child_watcher().pid().map(|pid| pid.get());
        let term = Arc::new(FairMutex::new(Term::new(
            opts.options.config(),
            &TermSize::new(opts.cols as usize, opts.rows as usize),
            listener.clone(),
        )));
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let pty = event_loop.channel();
        let _ = shared.pty.set(pty.clone());
        event_loop.spawn();
        Ok(Self {
            term: SharedTerm(term),
            pid,
            #[cfg(unix)]
            master,
            shared,
            pty,
            cols: opts.cols,
            rows: opts.rows,
            shell_name,
        })
    }

    pub fn shared(&self) -> &SharedTerm {
        &self.term
    }

    pub fn grid_size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// Re-arms the coalesced notification. Call once per frame.
    pub fn rearm(&self) {
        self.shared.pending.store(false, Ordering::Release);
    }

    /// The shell's title (OSC 0/2), or the shell's name.
    pub fn title(&self) -> String {
        self.shared
            .title()
            .unwrap_or_else(|| self.shell_name.clone())
    }

    pub fn exited(&self) -> bool {
        self.shared.exited.load(Ordering::Acquire)
    }

    /// The current folder of the program in the terminal's foreground: the shell the user types
    /// into, even under macOS's `login` or in a nested shell. Falls back to the process Templum
    /// started; None when the OS won't say (another user, already gone).
    pub fn cwd(&self) -> Option<PathBuf> {
        inspect(&[self.processes()]).pop().and_then(|i| i.folder)
    }

    /// The shell's and the foreground's process ids: cheap, for [`inspect`] elsewhere.
    pub fn processes(&self) -> Processes {
        Processes {
            shell: self.pid,
            foreground: self.foreground(),
        }
    }

    /// The PTY's foreground process group: its leader is the shell (or the program it runs).
    #[cfg(unix)]
    fn foreground(&self) -> Option<u32> {
        let group = rustix::termios::tcgetpgrp(self.master.as_ref()?).ok()?;
        u32::try_from(group.as_raw_nonzero().get()).ok()
    }

    /// ConPTY has no foreground process group: the shell Templum started is it.
    #[cfg(windows)]
    fn foreground(&self) -> Option<u32> {
        None
    }

    /// The colors this terminal reports to programs that ask for them.
    pub fn palette(&self) -> Palette {
        *self.shared.palette.lock().unwrap()
    }

    pub fn set_palette(&self, palette: Palette) {
        *self.shared.palette.lock().unwrap() = palette;
    }

    /// Sends raw input (scrolling the view back to the bottom, like any typing).
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        self.term.0.lock().scroll_display(Scroll::Bottom);
        let _ = self.pty.send(Msg::Input(bytes.into()));
    }

    /// Sends a key press. Returns false for printable text, which must come through the
    /// text-input path instead (IME, dead keys).
    pub fn key(&self, input: &KeyInput) -> bool {
        let app_cursor = self.term.0.lock().mode().contains(TermMode::APP_CURSOR);
        match encode_key(input, app_cursor) {
            Some(bytes) => {
                self.write(bytes);
                true
            }
            None => false,
        }
    }

    pub fn paste(&self, text: &str) {
        let bracketed = self
            .term
            .0
            .lock()
            .mode()
            .contains(TermMode::BRACKETED_PASTE);
        self.write(paste_bytes(text, bracketed));
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell_px: (u16, u16)) {
        if (cols, rows) == (self.cols, self.rows) {
            return;
        }
        (self.cols, self.rows) = (cols, rows);
        let size = window_size(cols, rows, cell_px);
        *self.shared.size.lock().unwrap() = size;
        self.term
            .0
            .lock()
            .resize(TermSize::new(cols as usize, rows as usize));
        let _ = self.pty.send(Msg::Resize(size));
    }

    /// Scrolls the history (positive = up). In full-screen apps that asked for it (less,
    /// vim: alternate screen + alternate scroll), sends arrow keys instead.
    pub fn scroll(&self, lines: i32) {
        if lines == 0 {
            return;
        }
        let mut term = self.term.0.lock();
        let mode = *term.mode();
        if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            drop(term);
            let key = KeyInput {
                key: if lines > 0 { "up" } else { "down" },
                ..KeyInput::default()
            };
            let arrow = encode_key(&key, mode.contains(TermMode::APP_CURSOR))
                .expect("arrow keys always encode");
            let _ = self.pty.send(Msg::Input(
                arrow.repeat(lines.unsigned_abs() as usize).into(),
            ));
        } else {
            term.scroll_display(Scroll::Delta(lines));
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Ends the event loop, which drops the PTY and kills the shell.
        let _ = self.pty.send(Msg::Shutdown);
    }
}

fn window_size(cols: u16, rows: u16, (cell_width, cell_height): (u16, u16)) -> WindowSize {
    WindowSize {
        num_lines: rows,
        num_cols: cols,
        cell_width,
        cell_height,
    }
}

fn file_stem(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

#[cfg(windows)]
fn default_shell_name() -> String {
    "powershell".to_string()
}

#[cfg(not(windows))]
fn default_shell_name() -> String {
    std::env::var("SHELL")
        .map(|s| file_stem(&s))
        .unwrap_or_else(|_| "sh".to_string())
}

/// A terminal grid with no PTY, for tests: feed it bytes with [`feed`].
#[cfg(test)]
pub(crate) fn headless(cols: u16, rows: u16) -> (SharedTerm, Arc<Shared>) {
    let shared = Arc::new(Shared::new(
        Arc::new(|| {}),
        window_size(cols, rows, (8, 16)),
        Palette::default(),
    ));
    let term = Term::new(
        Config::default(),
        &TermSize::new(cols as usize, rows as usize),
        Listener(shared.clone()),
    );
    (SharedTerm(Arc::new(FairMutex::new(term))), shared)
}

#[cfg(test)]
pub(crate) fn feed(term: &SharedTerm, bytes: &[u8]) {
    let mut parser: alacritty_terminal::vte::ansi::Processor =
        alacritty_terminal::vte::ansi::Processor::new();
    parser.advance(&mut *term.0.lock(), bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_are_coalesced_until_rearmed() {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = count.clone();
        let shared = Shared::new(
            Arc::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
            }),
            window_size(10, 2, (8, 16)),
            Palette::default(),
        );
        shared.wake();
        shared.wake();
        assert_eq!(count.load(Ordering::SeqCst), 1);
        shared.pending.store(false, Ordering::Release); // what Terminal::rearm does
        shared.wake();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn exited_waits_for_the_last_output() {
        // alacritty sends ChildExit before it reads the child's last output, and Exit after.
        let (_, shared) = headless(10, 2);
        let listener = Listener(shared.clone());
        listener.send_event(Event::ChildExit(std::process::ExitStatus::default()));
        assert!(
            !shared.exited.load(Ordering::Acquire),
            "output may still be unread"
        );
        listener.send_event(Event::Exit);
        assert!(shared.exited.load(Ordering::Acquire));
    }

    #[test]
    fn scrollback_option_limits_history() {
        let (term, _) = headless(10, 2);
        term.set_options(TermOptions {
            scrollback: 3,
            ..TermOptions::default()
        });
        feed(&term, b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n");
        assert_eq!(term.0.lock().grid().history_size(), 3);
    }

    #[test]
    fn osc_titles_are_stored_and_blank_ones_fall_back() {
        let (term, shared) = headless(20, 4);
        feed(&term, b"\x1b]2;vim main.rs\x07");
        assert_eq!(shared.title(), Some("vim main.rs".to_string()));
        feed(&term, b"\x1b]2;   \x07");
        assert_eq!(
            shared.title(),
            None,
            "blank titles fall back to the shell name"
        );
    }

    #[test]
    fn selection_text_by_kind() {
        let (term, _) = headless(20, 4);
        feed(&term, b"hello world\r\nsecond line");
        term.select_start(0, 0, Side::Left, SelectionKind::Simple);
        term.select_update(0, 4, Side::Right);
        assert_eq!(term.selection_text().as_deref(), Some("hello"));
        term.select_start(0, 7, Side::Left, SelectionKind::Word);
        assert_eq!(term.selection_text().as_deref(), Some("world"));
        term.select_start(1, 3, Side::Left, SelectionKind::Line);
        assert_eq!(term.selection_text().as_deref(), Some("second line\n"));
        term.select_clear();
        assert_eq!(term.selection_text(), None);
    }

    #[test]
    fn selection_outside_the_grid_clamps() {
        let (term, _) = headless(20, 4);
        feed(&term, b"abc");
        term.select_start(0, 0, Side::Left, SelectionKind::Simple);
        term.select_update(500, 500, Side::Right);
        assert!(term.selection_text().is_some_and(|t| t.starts_with("abc")));
        term.select_start(1000, 1000, Side::Left, SelectionKind::Simple);
        assert_eq!(
            term.selection_text(),
            None,
            "an empty selection is no selection"
        );
    }
}
