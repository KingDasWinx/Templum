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

use crate::agent::{Detect, agent_of};
use crate::input::{KeyInput, encode_key, paste_bytes};
use crate::palette::{self, Palette};
use crate::tap::{Mark, Status, Tap};

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
    /// What the shell and its programs told about their commands (see [`crate::tap`]).
    pub(crate) status: Mutex<Status>,
    /// The folder the shell last reported (OSC 7 or 9;9).
    pub(crate) folder: Mutex<Option<PathBuf>>,
    /// Each command's state, by the number of its prompt's mark.
    pub(crate) marks: Mutex<HashMap<u32, Mark>>,
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
            status: Mutex::default(),
            folder: Mutex::default(),
            marks: Mutex::default(),
        }
    }

    fn wake(&self) {
        if !self.pending.swap(true, Ordering::AcqRel) {
            (self.notify)();
        }
    }

    /// Wakes the UI even before the next re-arm: a hidden workspace's terminals are never re-armed,
    /// and a command's end or a call for attention must still reach the sidebar.
    pub(crate) fn wake_now(&self) {
        (self.notify)();
    }

    /// The terminal asks for attention: a bell (no message) or a notification.
    pub(crate) fn attention(&self, message: Option<String>) {
        let mut status = self.status.lock().unwrap();
        status.attention += 1;
        status.message = message;
        drop(status);
        self.wake_now();
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
            Event::Bell => s.attention(None),
            // Later: OSC 52 clipboard (needs a security review), cursor blinking.
            _ => {}
        }
    }
}

/// The terminal grid, shared with the PTY thread, and what the PTY thread heard. Cheap to clone.
#[derive(Clone)]
pub struct SharedTerm(
    pub(crate) Arc<FairMutex<Term<Listener>>>,
    pub(crate) Arc<Shared>,
);

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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Processes {
    /// The process Templum started.
    pub shell: Option<u32>,
    /// The PTY's foreground process (Unix): the shell, or the program it runs.
    pub foreground: Option<u32>,
    /// The folder the shell last reported, with shell integration.
    pub reported: Option<PathBuf>,
}

/// What a terminal's processes tell about it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Inspection {
    /// The current folder of the program in the foreground (see [`Terminal::cwd`]).
    pub folder: Option<PathBuf>,
    /// The id of the agent open in the terminal, among those [`inspect`] was given.
    pub agent: Option<String>,
}

/// Reads several terminals' processes in one pass over the OS's process table: slow enough to
/// keep off the UI thread when there are many terminals.
pub fn inspect(terminals: &[Processes], known: &[Detect]) -> Vec<Inspection> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let kind = ProcessRefreshKind::new()
        .with_cwd(UpdateKind::Always)
        .with_cmd(UpdateKind::Always);
    let mut system = System::new();
    if cfg!(windows) {
        // No foreground process: the agent is found among the shell's descendants.
        system.refresh_processes_specifics(ProcessesToUpdate::All, kind);
    } else {
        let pids: Vec<Pid> = terminals
            .iter()
            .flat_map(|p| [p.foreground, p.shell])
            .flatten()
            .map(Pid::from_u32)
            .collect();
        system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), kind);
    }
    terminals
        .iter()
        .map(|p| inspect_one(&system, p, known))
        .collect()
}

fn inspect_one(system: &sysinfo::System, p: &Processes, known: &[Detect]) -> Inspection {
    use sysinfo::Pid;
    let cwd = |pid: Pid| {
        system
            .process(pid)?
            .cwd()
            .filter(|cwd| !cwd.as_os_str().is_empty())
            .map(Path::to_path_buf)
    };
    let found = candidates(system, p)
        .into_iter()
        .find_map(|pid| Some((pid, agent_in(system.process(pid)?, known)?)));
    match found {
        // Its folder: under PowerShell, the shell's own doesn't follow `cd`, but programs it starts do.
        Some((pid, agent)) => Inspection {
            folder: cwd(pid),
            agent: Some(agent),
        },
        None => Inspection {
            folder: reported(p)
                .or_else(|| p.foreground.map(Pid::from_u32).and_then(cwd))
                .or_else(|| p.shell.map(Pid::from_u32).and_then(cwd)),
            agent: None,
        },
    }
}

/// The folder the shell reported, while it is in the foreground: its own word beats the OS's
/// (PowerShell's `cd` doesn't move its process), but not a program's it runs (a nested shell,
/// ssh). Without a foreground process (Windows), the shell is taken to be in front.
fn reported(p: &Processes) -> Option<PathBuf> {
    let in_front = p.foreground.is_none() || p.foreground == p.shell;
    p.reported
        .clone()
        .filter(|dir| in_front && dir.is_absolute() && dir.is_dir())
}

fn agent_in(process: &sysinfo::Process, known: &[Detect]) -> Option<String> {
    let args: Vec<String> = process
        .cmd()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    agent_of(&process.name().to_string_lossy(), &args, known)
}

/// Where an agent may be: the foreground process (Unix); without one (Windows), the shell and
/// every process below it, nearest first.
fn candidates(system: &sysinfo::System, p: &Processes) -> Vec<sysinfo::Pid> {
    use sysinfo::Pid;
    if let Some(foreground) = p.foreground {
        return vec![Pid::from_u32(foreground)];
    }
    let Some(shell) = p.shell else {
        return Vec::new();
    };
    let mut found = vec![Pid::from_u32(shell)];
    let mut i = 0;
    while i < found.len() {
        let parent = found[i];
        let children: Vec<Pid> = system
            .processes()
            .iter()
            .filter(|(pid, process)| process.parent() == Some(parent) && !found.contains(pid))
            .map(|(pid, _)| *pid)
            .collect();
        found.extend(children);
        i += 1;
    }
    found
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
    /// The folder [`crate::install`] wrote the shell integration to; None: no integration.
    pub integration: Option<PathBuf>,
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
    /// Starts `opts.command`, else the default shell (see [`crate::default_shell`]: the user's
    /// login shell, or Git Bash first on Windows), in a PTY. `notify` is called, coalesced, on
    /// new output, title changes and exit.
    pub fn spawn(opts: SpawnOptions, notify: Notify) -> io::Result<Self> {
        let size = window_size(opts.cols, opts.rows, opts.cell_px);
        let mut command = opts
            .command
            .clone()
            .or_else(|| crate::default_shell().map(|s| (s.program, s.args)));
        let shell_name = match &command {
            Some((program, _)) => file_stem(program),
            None => default_shell_name(),
        };
        let mut env = HashMap::from([
            ("TERM".into(), "xterm-256color".into()),
            ("COLORTERM".into(), "truecolor".into()),
            // Not the terminal Templum was started from.
            ("TERM_PROGRAM".into(), "templum".into()),
        ]);
        if let Some(dir) = &opts.integration {
            // Without a command, alacritty runs $SHELL: as a login shell on macOS.
            let (program, args) = command.clone().unwrap_or_else(|| {
                let login = cfg!(target_os = "macos").then(|| "-l".to_string());
                (
                    std::env::var("SHELL").unwrap_or_default(),
                    login.into_iter().collect(),
                )
            });
            if let Some(injection) =
                crate::integration::inject(&program, &args, dir, std::env::var_os("ZDOTDIR"))
            {
                env.extend(injection.env);
                if let Some(args) = injection.args {
                    command = Some((program, args));
                }
            }
        }
        let options = tty::Options {
            shell: command.map(|(program, args)| tty::Shell::new(program, args)),
            env,
            // Like other GUI terminals: a launch from Finder or the Start menu doesn't leave
            // shells in / or the install folder.
            working_directory: opts.working_directory.clone().or_else(std::env::home_dir),
            // Show everything the shell printed before reporting its exit.
            drain_on_exit: true,
            // Each argument stays one, spaces and all (a folder under C:\Users\Jane Doe).
            #[cfg(windows)]
            escape_args: true,
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
        let pty = Tap::new(pty, shared.clone());
        let event_loop = EventLoop::new(term.clone(), listener, pty, true, false)?;
        let pty = event_loop.channel();
        let _ = shared.pty.set(pty.clone());
        event_loop.spawn();
        Ok(Self {
            term: SharedTerm(term, shared.clone()),
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
    /// started; None when the OS won't say (another user, already gone). With Claude Code or Codex
    /// open, its folder.
    pub fn cwd(&self) -> Option<PathBuf> {
        let known: Vec<Detect> = crate::Agent::all().map(crate::Agent::detect).collect();
        inspect(&[self.processes()], &known)
            .pop()
            .and_then(|i| i.folder)
    }

    /// The shell's and the foreground's process ids: cheap, for [`inspect`] elsewhere.
    pub fn processes(&self) -> Processes {
        Processes {
            shell: self.pid,
            foreground: self.foreground(),
            reported: self.shared.folder.lock().unwrap().clone(),
        }
    }

    /// What the shell (with shell integration) and its programs told about their commands, and
    /// how often they asked for attention.
    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
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

    /// Scrolls to the previous (`up`) or the next prompt (shell integration marks them): the one
    /// above or below the top of the view goes to the top, or as near as the history allows.
    /// False when the view didn't move: no prompt there, or a full-screen program runs.
    pub fn jump_to_prompt(&self, up: bool) -> bool {
        Self::jump_in(&self.term, up)
    }

    fn jump_in(term: &SharedTerm, up: bool) -> bool {
        let mut term = term.0.lock();
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        let top = -(term.grid().display_offset() as i32);
        let starts = crate::tap::prompt_lines(term.grid());
        let target = if up {
            starts.into_iter().rfind(|&l| l < top)
        } else {
            starts.into_iter().find(|&l| l > top)
        };
        let Some(target) = target else {
            return false;
        };
        term.scroll_display(Scroll::Delta(top - target));
        -(term.grid().display_offset() as i32) != top
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
    (
        SharedTerm(Arc::new(FairMutex::new(term)), shared.clone()),
        shared,
    )
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
    fn a_bell_asks_for_attention_and_wakes_at_once() {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = count.clone();
        let shared = Arc::new(Shared::new(
            Arc::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
            }),
            window_size(10, 2, (8, 16)),
            Palette::default(),
        ));
        shared.wake();
        Listener(shared.clone()).send_event(Event::Bell);
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "not held back by the coalescing"
        );
        assert_eq!(shared.status.lock().unwrap().attention, 1);
        // As a program rings it.
        let (term, shared) = headless(10, 2);
        feed(&term, b"\x07");
        assert_eq!(shared.status.lock().unwrap().attention, 1);
        assert_eq!(shared.status.lock().unwrap().message, None);
    }

    #[test]
    fn the_reported_folder_counts_while_the_shell_is_in_front() {
        let reported = tempfile::tempdir().unwrap();
        let folder = |p: Processes| inspect(&[p], &[]).pop().unwrap().folder;
        let me = std::process::id();
        let mine = std::env::current_dir().unwrap();
        let p = Processes {
            shell: Some(me),
            foreground: None,
            reported: Some(reported.path().to_path_buf()),
        };
        assert_eq!(
            folder(p.clone()).as_deref(),
            Some(reported.path()),
            "no foreground (Windows)"
        );
        let front = Processes {
            foreground: Some(me),
            ..p.clone()
        };
        assert_eq!(
            folder(front).as_deref(),
            Some(reported.path()),
            "the shell in front"
        );
        // Another program in front (ssh, a nested shell): what the OS says of it.
        let other = Processes {
            shell: Some(u32::MAX),
            foreground: Some(me),
            ..p.clone()
        };
        assert_eq!(folder(other), Some(mine.clone()));
        let gone = Processes {
            reported: Some(reported.path().join("gone")),
            ..p
        };
        assert_eq!(folder(gone), Some(mine), "a folder that isn't there");
    }

    /// Feeds `bytes` as the PTY thread would: through the tap, which marks the prompts.
    fn feed_heard(term: &SharedTerm, bytes: &[u8]) {
        let mut scanner = crate::tap::Scanner::new();
        let inserts = scanner.scan(bytes, &term.1);
        feed(term, &crate::tap::rewrite(bytes, inserts));
    }

    fn rows(term: &SharedTerm) -> Vec<(String, Option<Mark>)> {
        let mut snap = crate::Snapshot::default();
        snap.refresh(term, &Palette::default());
        snap.rows.into_iter().map(|r| (r.text, r.mark)).collect()
    }

    #[test]
    fn a_prompt_shows_its_commands_state_on_its_first_line() {
        let (term, _) = headless(20, 5);
        feed_heard(
            &term,
            b"\x1b]133;A\x07$ false\r\n\x1b]133;C\x07\x1b]133;D;1\x07\
              \x1b]133;A\x07> two\r\nlines\r\n\x1b]133;C\x07\x1b]133;D;0\x07\
              \x1b]133;A\x07$ ",
        );
        let shown = rows(&term);
        let want = [
            ("$ false", Some(Mark::Ended(Some(1)))),
            ("> two", Some(Mark::Ended(Some(0)))),
            ("lines", None),
            ("$", None),
        ];
        for (row, (text, mark)) in shown.iter().zip(want) {
            assert_eq!((row.0.as_str(), row.1), (text, mark));
        }
        // The output goes on: the second prompt's first line goes into the history, and its other
        // line, now at the top, isn't a start.
        feed(&term, b"\r\n\r\n\r\n");
        assert_eq!(rows(&term)[0], ("lines".to_string(), None));
        term.0.lock().scroll_display(Scroll::Delta(1));
        assert_eq!(
            rows(&term)[0],
            ("> two".to_string(), Some(Mark::Ended(Some(0))))
        );
    }

    #[test]
    fn jumps_go_from_prompt_to_prompt() {
        let (term, _) = headless(20, 4);
        for i in 0..3 {
            let mut chunk = format!("\x1b]133;A\x07$ cmd{i}\r\n\x1b]133;C\x07");
            chunk.push_str(&"out\r\n".repeat(5));
            chunk.push_str("\x1b]133;D;0\x07");
            feed_heard(&term, chunk.as_bytes());
        }
        feed_heard(&term, b"\x1b]133;A\x07$ ");
        let jump = |up: bool| {
            let moved = Terminal::jump_in(&term, up);
            (moved, rows(&term)[0].0.clone())
        };
        assert_eq!(jump(true), (true, "$ cmd2".to_string()));
        assert_eq!(jump(true), (true, "$ cmd1".to_string()));
        assert_eq!(jump(true), (true, "$ cmd0".to_string()));
        assert_eq!(
            jump(true),
            (false, "$ cmd0".to_string()),
            "the first prompt"
        );
        assert_eq!(jump(false), (true, "$ cmd1".to_string()));
        assert_eq!(jump(false), (true, "$ cmd2".to_string()));
        assert_eq!(
            jump(false),
            (true, "out".to_string()),
            "the bottom: the last prompt's screen"
        );
        assert!(!jump(false).0, "nothing below");
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
