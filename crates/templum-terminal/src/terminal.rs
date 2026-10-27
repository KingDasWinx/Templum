//! A shell in a PTY, using alacritty_terminal's own tty and event loop.

use std::borrow::Cow;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, EventLoopSender, Msg};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point as GridPoint, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode, viewport_to_point};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::CursorStyle;

use crate::agent::{Detect, agent_of};
use crate::input::{
    KeyInput, MouseInput, MouseKind, encode_key, encode_mouse, paste_bytes, typed_bytes,
};
use crate::palette::{self, Palette};
use crate::scrollback::Scrollback;
use crate::tap::{Mark, Pattern, Status, Tap};

/// Called (coalesced) when a terminal has something new to show.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// State shared between the PTY thread (through [`Listener`]) and the UI thread.
pub(crate) struct Shared {
    notify: Notify,
    /// Set by the first notification; cleared by [`SharedTerm::rearm`].
    pending: AtomicBool,
    /// The grid may differ from what was last copied out of it: set by the PTY thread's wake-ups
    /// and by the UI calls that change the grid; cleared by [`crate::Snapshot::refresh`] under
    /// the lock.
    pub(crate) dirty: AtomicBool,
    /// Counts every time `dirty` was set: a reader that keeps the count it last read at knows
    /// whether the grid may differ (see [`SharedTerm::changes`]).
    changes: AtomicU64,
    title: Mutex<Option<String>>,
    /// When the terminal was made, and when its shell ended: how long the shell lived.
    born: Instant,
    exited: OnceLock<Instant>,
    /// How its shell ended, as the OS reported it.
    ended: OnceLock<std::process::ExitStatus>,
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
    /// The watches on its printed lines: each one's id, its pattern and the first line that
    /// matched it. `watching` spares the reader thread the lock while there are none.
    watches: Mutex<Vec<(u64, Pattern, Option<String>)>>,
    watching: AtomicBool,
    /// How many times a viewer was fed (`Feeder::feed`; never a history).
    fed: AtomicU64,
}

impl Shared {
    fn new(notify: Notify, size: WindowSize, palette: Palette) -> Self {
        Self {
            notify,
            pending: AtomicBool::new(false),
            dirty: AtomicBool::new(true),
            changes: AtomicU64::new(0),
            title: Mutex::new(None),
            born: Instant::now(),
            exited: OnceLock::new(),
            ended: OnceLock::new(),
            size: Mutex::new(size),
            pty: OnceLock::new(),
            palette: Mutex::new(palette),
            status: Mutex::default(),
            folder: Mutex::default(),
            marks: Mutex::default(),
            watches: Mutex::default(),
            watching: AtomicBool::new(false),
            fed: AtomicU64::new(0),
        }
    }

    /// Adds a watch for `pattern` on the lines printed from now on; its id.
    pub(crate) fn watch(&self, pattern: Pattern) -> u64 {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let mut watches = self.watches.lock().unwrap();
        watches.push((id, pattern, None));
        self.watching.store(true, Ordering::Release);
        id
    }

    /// The first line that matched the watch `id`, once one did.
    pub(crate) fn hit(&self, id: u64) -> Option<String> {
        let watches = self.watches.lock().unwrap();
        watches.iter().find(|w| w.0 == id)?.2.clone()
    }

    pub(crate) fn unwatch(&self, id: u64) {
        let mut watches = self.watches.lock().unwrap();
        watches.retain(|w| w.0 != id);
        self.watching.store(!watches.is_empty(), Ordering::Release);
    }

    /// A watch is set: the reader thread keeps the line being printed.
    pub(crate) fn watched(&self) -> bool {
        self.watching.load(Ordering::Acquire)
    }

    /// A line was printed: each watch with no match yet whose pattern it matches keeps it.
    pub(crate) fn heard(&self, line: &[u8]) {
        let line = String::from_utf8_lossy(line);
        let mut watches = self.watches.lock().unwrap();
        for (_, pattern, hit) in watches.iter_mut().filter(|w| w.2.is_none()) {
            if pattern.is_match(&line) {
                *hit = Some(line.clone().into_owned());
            }
        }
    }

    /// The grid may differ from what was last copied out of it.
    fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Release);
        self.changes.fetch_add(1, Ordering::Release);
    }

    fn wake(&self) {
        self.mark_dirty();
        if !self.pending.swap(true, Ordering::AcqRel) {
            (self.notify)();
        }
    }

    /// Wakes the UI even before the next re-arm: a hidden workspace's terminals are never re-armed,
    /// and a command's end or a call for attention must still reach the sidebar.
    pub(crate) fn wake_now(&self) {
        self.mark_dirty();
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
    pub(crate) fn title(&self) -> Option<String> {
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
            Event::ChildExit(status) => {
                let _ = s.ended.set(status);
            }
            // Exit comes after the child's last output has been read (ChildExit comes before).
            // Uncoalesced: a block off screen is never re-armed, and must still close.
            Event::Exit => {
                let _ = s.exited.set(Instant::now());
                s.wake_now();
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

/// A viewer's grid, fed from another thread: what a PTY reader does for a shell. Cheap to clone.
#[derive(Clone)]
pub struct Feeder(SharedTerm);

impl Feeder {
    /// Shows `bytes` as a program's output: the tap's scanner, vte's parser under the grid's lock,
    /// then a wake-up for the UI.
    pub fn feed(&self, bytes: &[u8]) {
        crate::tap::Scanner::new().scan(bytes, &self.0.1);
        let mut parser: alacritty_terminal::vte::ansi::Processor =
            alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut *self.0.0.lock(), bytes);
        self.0.1.fed.fetch_add(1, Ordering::Relaxed);
        self.0.1.wake();
    }

    /// How many times `feed` ran: the frames of a mirror, never a history.
    pub fn fed(&self) -> u64 {
        self.0.1.fed.load(Ordering::Relaxed)
    }

    /// Puts home's history above the live screen: `text` is what home's `read_terminal` answers,
    /// its history and then its screen down to the last row written. The screen the viewer shows
    /// (its rows, its cursor, how far it is scrolled back) stays; a selection goes, and so does
    /// the history held before. The text is drawn, never run: every control character but a tab
    /// is dropped (a tab only moves the cursor), and a line longer than the grid wraps as it did
    /// at home. At most [`VIEWER_HISTORY`] rows stay. False, and nothing changed, in the alternate
    /// screen: a full-screen program has no history.
    pub fn fill_history(&self, text: &str) -> bool {
        let mut term = self.0.0.lock();
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        let (rows, cols) = (term.screen_lines(), term.columns());
        let wrap = term.mode().contains(TermMode::LINE_WRAP);
        let (screen, cursor, offset) = {
            let grid = term.grid();
            let screen: Vec<_> = (0..rows).map(|y| grid[Line(y as i32)].clone()).collect();
            (screen, grid.cursor.clone(), grid.display_offset())
        };
        // Home's text ends with its screen down to the last row written, which the live screen
        // draws: the rows of the text under the history are that many.
        let used = screen
            .iter()
            .rposition(|row| (0..cols).any(|x| row[Column(x)].c != ' '))
            .map_or(0, |y| y + 1);
        // From the end, so only the lines kept are cleaned and allocated.
        let mut lines: Vec<String> = text
            .lines()
            .rev()
            .map(|line| {
                line.chars()
                    .filter(|c| !c.is_control() || *c == '\t')
                    .collect()
            })
            .skip_while(String::is_empty)
            .take(VIEWER_HISTORY + rows)
            .collect();
        lines.reverse();
        let mut parser: alacritty_terminal::vte::ansi::Processor =
            alacritty_terminal::vte::ansi::Processor::new();
        term.grid_mut().update_history(VIEWER_HISTORY);
        parser.advance(&mut *term, b"\x1b[3J\x1b[H\x1b[J\x1b[0m\x1b[?7h");
        parser.advance(&mut *term, lines.join("\r\n").as_bytes());
        if !lines.is_empty() {
            // The cursor is on the text's last row: `CSI S` on the whole screen pushes the rows
            // above the live screen's into the history.
            let above = (term.grid().cursor.point.line.0.max(0) as usize + 1).saturating_sub(used);
            if above > 0 {
                parser.advance(&mut *term, format!("\x1b[{above}S").as_bytes());
            }
        }
        parser.advance(&mut *term, if wrap { b"\x1b[?7h" } else { b"\x1b[?7l" });
        let grid = term.grid_mut();
        for (y, row) in screen.into_iter().enumerate() {
            grid[Line(y as i32)] = row;
        }
        grid.cursor = cursor;
        term.scroll_display(Scroll::Delta(offset as i32));
        term.selection = None;
        drop(term);
        self.0.touched();
        self.0.1.wake();
        true
    }

    /// Forgets the history `fill_history` put above the screen.
    pub fn drop_history(&self) {
        let mut term = self.0.0.lock();
        let mut parser: alacritty_terminal::vte::ansi::Processor =
            alacritty_terminal::vte::ansi::Processor::new();
        parser.advance(&mut *term, b"\x1b[3J");
        term.grid_mut().update_history(0);
        term.selection = None;
        drop(term);
        self.0.touched();
        self.0.1.wake();
    }

    /// Whether both feed the same viewer.
    pub fn same_viewer(&self, other: &Feeder) -> bool {
        Arc::ptr_eq(&self.0.0, &other.0.0)
    }

    /// Gives the grid `cols` × `rows` when it has another size; the size the window reports
    /// follows.
    pub fn resize(&self, cols: u16, rows: u16) {
        if self.0.grid_size() == (cols, rows) {
            return;
        }
        {
            let mut size = self.0.1.size.lock().unwrap();
            *size = window_size(cols, rows, (size.cell_width, size.cell_height));
        }
        self.0
            .0
            .lock_unfair()
            .resize(TermSize::new(cols as usize, rows as usize));
        self.0.touched();
    }
}

/// The rows of home's history a mirror's viewer keeps above its screen.
pub const VIEWER_HISTORY: usize = 2000;

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
            // Programs that ask for it (Claude Code, Codex) hear Shift+Enter, Ctrl+Shift+letters.
            kitty_keyboard: true,
            ..Config::default()
        }
    }
}

/// What the program in a terminal asked of it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modes {
    /// Mouse events (clicks, the wheel; maybe moves).
    pub mouse: bool,
    /// Every motion, not only clicks and drags (`?1003`).
    pub mouse_moves: bool,
    /// Keys in the kitty keyboard protocol.
    pub kitty_keys: bool,
    /// The alternate screen: it runs full screen.
    pub full_screen: bool,
    /// It reads typed input: it brackets pastes and shows its cursor. Claude Code and Codex hide
    /// it in their dialogs.
    pub typing: bool,
}

impl Modes {
    pub(crate) fn of(mode: TermMode) -> Modes {
        Modes {
            mouse: mode.intersects(TermMode::MOUSE_MODE),
            mouse_moves: mode.contains(TermMode::MOUSE_MOTION),
            kitty_keys: mode.intersects(TermMode::KITTY_KEYBOARD_PROTOCOL),
            full_screen: mode.contains(TermMode::ALT_SCREEN),
            typing: mode.contains(TermMode::BRACKETED_PASTE | TermMode::SHOW_CURSOR),
        }
    }

    /// Whether the program asked for this event: a move with no button needs every motion,
    /// anything else any mouse mode.
    pub fn wants(&self, e: &MouseInput) -> bool {
        match (e.kind, e.button) {
            (MouseKind::Move, None) => self.mouse_moves,
            _ => self.mouse,
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

// The UI thread locks the grid unfairly: it never waits for the PTY thread's lease (held for a
// whole read pass), only for the parse under way. See alacritty's sync.rs.
impl SharedTerm {
    /// Applies new options to the running terminal (alacritty trims the history if needed).
    pub fn set_options(&self, options: TermOptions) {
        self.0.lock_unfair().set_options(options.config());
        self.touched();
    }

    pub fn modes(&self) -> Modes {
        Modes::of(*self.0.lock_unfair().mode())
    }

    /// Where its view is in its history. None while the PTY thread holds the grid: painting never
    /// waits for it, and [`crate::Snapshot::scrollback`] has the last one read.
    pub fn scrollback(&self) -> Option<Scrollback> {
        self.0.try_lock_unfair().map(|term| Scrollback::of(&term))
    }

    /// Scrolls the view to `below` lines back from the live screen (0: the live screen), as far
    /// as the history goes.
    pub fn scroll_to(&self, below: usize) {
        let mut term = self.0.lock_unfair();
        let delta = below as i32 - term.grid().display_offset() as i32;
        term.scroll_display(Scroll::Delta(delta));
        drop(term);
        self.touched();
    }

    /// The grid's columns and screen lines now.
    pub fn grid_size(&self) -> (u16, u16) {
        let term = self.0.lock_unfair();
        (term.columns() as u16, term.screen_lines() as u16)
    }

    /// Starts a selection at a viewport cell (row 0 = top visible line).
    pub fn select_start(&self, row: usize, col: usize, side: Side, kind: SelectionKind) {
        let mut term = self.0.lock_unfair();
        let point = grid_point(&term, row, col);
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Word => SelectionType::Semantic,
            SelectionKind::Line => SelectionType::Lines,
        };
        term.selection = Some(Selection::new(ty, point, side));
        self.touched();
    }

    pub fn select_update(&self, row: usize, col: usize, side: Side) {
        let mut term = self.0.lock_unfair();
        let point = grid_point(&term, row, col);
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
        self.touched();
    }

    pub fn select_clear(&self) {
        self.0.lock_unfair().selection = None;
        self.touched();
    }

    /// The selected text; None when nothing (or only an empty range) is selected.
    pub fn selection_text(&self) -> Option<String> {
        self.0
            .lock_unfair()
            .selection_to_string()
            .filter(|t| !t.is_empty())
    }

    /// Re-arms the coalesced notification: the next output wakes the UI again. Call it right
    /// before reading the grid, so output parsed during the read isn't missed. Only painted
    /// terminals are re-armed: output off screen wakes the UI once, then waits to be painted.
    pub fn rearm(&self) {
        // A swap, not a store: against the PTY thread's swap in `wake`, one of the two sees the
        // other, so either `refresh` sees `dirty` or the PTY thread wakes the UI again. A plain
        // store followed by the load of `dirty` can miss both (store-buffer reordering).
        self.1.pending.swap(false, Ordering::AcqRel);
    }

    /// The grid may have changed since it was last copied out (see [`crate::Snapshot::refresh`]).
    pub(crate) fn dirty(&self) -> bool {
        self.1.dirty.load(Ordering::Acquire)
    }

    /// The UI changed the grid (a scroll, the selection, a resize): the next refresh reads it.
    pub(crate) fn touched(&self) {
        self.1.mark_dirty();
    }

    /// How many times the grid may have changed (output, a scroll, the selection, a resize, a
    /// title): equal counts, equal screens. Read it before the grid, so a change during the read
    /// is counted again.
    pub fn changes(&self) -> u64 {
        self.1.changes.load(Ordering::Acquire)
    }

    /// Whether both are the same terminal.
    pub fn same(&self, other: &SharedTerm) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// False while a wake-up waits to be re-armed (tests).
    pub fn armed(&self) -> bool {
        !self.1.pending.load(Ordering::Acquire)
    }

    /// The last `count` lines of the history and the screen, as text, without the blank rows
    /// under the last one written. A wrapped line is one line. A suggestion where the cursor
    /// waits for typing isn't typed, and is left out (an agent reading it took it for its
    /// person's text): its row is then a line of its own.
    pub fn lines(&self, count: usize) -> Vec<String> {
        let term = self.0.lock_unfair();
        let (top, bottom) = (term.topmost_line().0, term.bottommost_line().0);
        // The blank rows under the cursor are at most a screen: read that much more.
        let from = (bottom + 1 - (count + term.screen_lines()) as i32).max(top);
        let mut lines = match suggestion(&term).filter(|(line, _)| line.0 >= from) {
            Some((line, cols)) => {
                let part = |a: usize, b: usize| {
                    term.bounds_to_string(
                        GridPoint::new(line, Column(a)),
                        GridPoint::new(line, Column(b)),
                    )
                };
                let head = (cols.start > 0).then(|| part(0, cols.start - 1));
                let tail = (cols.end < term.columns()).then(|| part(cols.end, term.columns() - 1));
                let mut lines = text_between(&term, from, line.0 - 1);
                let row = head.unwrap_or_default() + &tail.unwrap_or_default();
                lines.push(row.trim_end().to_string());
                lines.extend(text_between(&term, line.0 + 1, bottom));
                lines
            }
            None => text_between(&term, from, bottom),
        };
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        let skip = lines.len().saturating_sub(count);
        lines.drain(..skip);
        lines
    }

    /// The last command that finished (shell integration marks it), with its output, up to the
    /// next prompt. None without one.
    pub fn last_command(&self) -> Option<String> {
        let term = self.0.lock_unfair();
        let grid = term.grid();
        let starts = crate::tap::prompt_lines(grid);
        let marks = self.1.marks.lock().unwrap();
        let ended = |line: i32| {
            crate::tap::line_mark(grid, Line(line))
                .and_then(|prompt| marks.get(&prompt))
                .is_some_and(|mark| matches!(mark, Mark::Ended(_)))
        };
        let i = starts.iter().rposition(|&line| ended(line))?;
        let end = starts
            .get(i + 1)
            .map_or(term.bottommost_line().0, |next| next - 1);
        let mut lines = text_between(&term, starts[i], end);
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        Some(lines.join("\n"))
    }
}

/// A viewport cell, clamped to the grid, as a grid point (accounting for scrollback).
pub(crate) fn grid_point(term: &Term<Listener>, row: usize, col: usize) -> GridPoint {
    let row = row.min(term.screen_lines() - 1);
    let col = col.min(term.columns() - 1);
    viewport_to_point(
        term.grid().display_offset(),
        GridPoint::new(row, Column(col)),
    )
}

/// The text of the grid's lines `top` to `bottom` (history lines are negative), one string per
/// line, trailing spaces trimmed.
fn text_between<T>(term: &Term<T>, top: i32, bottom: i32) -> Vec<String> {
    if top > bottom {
        return Vec::new();
    }
    let text = term.bounds_to_string(
        GridPoint::new(Line(top), Column(0)),
        GridPoint::new(Line(bottom), term.last_column()),
    );
    text.lines()
        .map(|line| line.trim_end().to_string())
        .collect()
}

/// The suggestion an input draws where its shown cursor waits for typing, not typed yet: the
/// faint run of cells from the cursor on, its line and columns. Claude Code's placeholder
/// (`Try "fix lint errors"`), Codex's (`Ask Codex to do anything`), a completion after what was
/// typed.
fn suggestion<T>(term: &Term<T>) -> Option<(Line, std::ops::Range<usize>)> {
    if !term.mode().contains(TermMode::SHOW_CURSOR) {
        return None;
    }
    let at = term.grid().cursor.point;
    let row = &term.grid()[at.line];
    let end = (at.column.0..term.columns())
        .find(|&col| !row[Column(col)].flags.contains(Flags::DIM))
        .unwrap_or(term.columns());
    (end > at.column.0).then_some((at.line, at.column.0..end))
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
    /// The agent's process, to find its session's files.
    pub pid: Option<u32>,
    /// When the agent's process started, in seconds since the Unix epoch (about a second off
    /// on Linux).
    pub started: Option<u64>,
    /// The id of the session the agent was started on, from its arguments
    /// ([`Agent::session_in`](crate::Agent::session_in)); one of Templum's agents only.
    pub session: Option<String>,
    /// What the agent's process has for its folder's variable ([`Detect::var`]): `None` when it
    /// wasn't read (the agent has none, or the environment came back empty: another user's
    /// process), `Some(None)` unset or empty, `Some(Some(folder))`.
    pub folder_var: Option<Option<PathBuf>>,
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
    let mut found: Vec<Inspection> = terminals
        .iter()
        .map(|p| inspect_one(&system, p, known))
        .collect();
    // The found agents' variable, from their environments: one more read, of those processes only
    // (on Windows the first one covers every process).
    let wanted: Vec<(usize, Pid, &str)> = found
        .iter()
        .enumerate()
        .filter_map(|(i, seen)| {
            let var = known
                .iter()
                .find(|d| Some(&d.id) == seen.agent.as_ref())?
                .var
                .as_deref()?;
            Some((i, Pid::from_u32(seen.pid?), var))
        })
        .collect();
    if !wanted.is_empty() {
        let pids: Vec<Pid> = wanted.iter().map(|(_, pid, _)| *pid).collect();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pids),
            ProcessRefreshKind::new().with_environ(UpdateKind::Always),
        );
        for (i, pid, var) in wanted {
            found[i].folder_var = system
                .process(pid)
                .and_then(|process| folder_var(process, var));
        }
    }
    found
}

/// `var` in `process`'s environment: see [`Inspection::folder_var`].
fn folder_var(process: &sysinfo::Process, var: &str) -> Option<Option<PathBuf>> {
    let environ = process.environ();
    if environ.is_empty() {
        return None;
    }
    // Windows doesn't tell `Path` from `PATH`.
    let named = |entry: &str| {
        entry.len() > var.len()
            && entry.is_char_boundary(var.len())
            && entry.as_bytes()[var.len()] == b'='
            && if cfg!(windows) {
                entry[..var.len()].eq_ignore_ascii_case(var)
            } else {
                &entry[..var.len()] == var
            }
    };
    let value = environ
        .iter()
        .filter_map(|entry| entry.to_str())
        .find(|entry| named(entry))
        .map(|entry| &entry[var.len() + 1..])
        .filter(|value| !value.is_empty());
    Some(value.map(PathBuf::from))
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
    // The shell's folder: its own word, else the OS's.
    let shell = || {
        reported(p)
            .or_else(|| p.foreground.map(Pid::from_u32).and_then(cwd))
            .or_else(|| p.shell.map(Pid::from_u32).and_then(cwd))
    };
    let found = candidates(system, p)
        .into_iter()
        .find_map(|pid| Some((pid, agent_in(system.process(pid)?, known)?)));
    match found {
        // Its folder: under PowerShell, the shell's own doesn't follow `cd`, but programs it starts
        // do. One the OS won't tell is the shell's.
        Some((pid, agent)) => Inspection {
            folder: cwd(pid).or_else(shell),
            session: system.process(pid).and_then(|process| {
                let args = args(process);
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                crate::Agent::from_name(&agent)?.session_in(&args)
            }),
            agent: Some(agent),
            pid: Some(pid.as_u32()),
            started: system.process(pid).map(sysinfo::Process::start_time),
            folder_var: None,
        },
        None => Inspection {
            folder: shell(),
            ..Inspection::default()
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

fn args(process: &sysinfo::Process) -> Vec<String> {
    process
        .cmd()
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

fn agent_in(process: &sysinfo::Process, known: &[Detect]) -> Option<String> {
    let args = args(process);
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
    descendants(system, Pid::from_u32(shell))
}

/// A process of the OS's table, as [`below`] and [`front`] read it.
#[derive(Clone, Copy, Debug)]
struct Proc {
    pid: u32,
    parent: Option<u32>,
    started: u64,
}

/// The processes of `system`, threads left out.
fn table(system: &sysinfo::System) -> Vec<Proc> {
    system
        .processes()
        .values()
        .filter(|p| p.thread_kind().is_none())
        .map(|p| Proc {
            pid: p.pid().as_u32(),
            parent: p.parent().map(sysinfo::Pid::as_u32),
            started: p.start_time(),
        })
        .collect()
}

/// `root` and every process below it, nearest first.
fn descendants(system: &sysinfo::System, root: sysinfo::Pid) -> Vec<sysinfo::Pid> {
    below(&table(system), root.as_u32())
        .into_iter()
        .map(sysinfo::Pid::from_u32)
        .collect()
}

/// `root` and every process of `table` below it, nearest first. A child started before its
/// parent names a parent that died and whose pid was taken again (Windows keeps stale parent
/// ids): it isn't followed.
fn below(table: &[Proc], root: u32) -> Vec<u32> {
    let mut found = vec![root];
    let mut i = 0;
    while i < found.len() {
        let parent = found[i];
        let since = table
            .iter()
            .find(|p| p.pid == parent)
            .map_or(0, |p| p.started);
        let children: Vec<u32> = table
            .iter()
            .filter(|p| p.parent == Some(parent) && p.started >= since && !found.contains(&p.pid))
            .map(|p| p.pid)
            .collect();
        found.extend(children);
        i += 1;
    }
    found
}

/// The program in front of the innermost of `shells` where the OS can't tell (Windows): `agent`,
/// the agent's pid and start time a snapshot found, while that process is still below the shell
/// (its pid not taken again since), else the shell's oldest child. Never one of `shells`.
fn front(table: &[Proc], shells: &[u32], agent: Option<(u32, u64)>) -> Option<u32> {
    let shell = *shells.last()?;
    let under = below(table, shell);
    let ours = |pid: u32| !shells.contains(&pid) && under.contains(&pid);
    if let Some((pid, started)) = agent
        && ours(pid)
        && table.iter().any(|p| p.pid == pid && p.started == started)
    {
        return Some(pid);
    }
    table
        .iter()
        .filter(|p| p.parent == Some(shell) && ours(p.pid))
        .min_by_key(|p| (p.started, p.pid))
        .map(|p| p.pid)
}

/// A process to end later: its pid, and when it started, since a pid can be taken again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Running {
    pub pid: u32,
    pub started: u64,
}

/// Every process of these shells: each shell and what runs below it, and on Unix what runs in
/// its session (a daemon that forked twice still does). Read before the shells are hung up,
/// while the children still name their parents.
pub fn tree(shells: &[u32]) -> Vec<Running> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, ProcessRefreshKind::new());
    let mut pids: Vec<Pid> = Vec::new();
    for shell in shells.iter().map(|s| Pid::from_u32(*s)) {
        for pid in descendants(&system, shell) {
            if !pids.contains(&pid) {
                pids.push(pid);
            }
        }
        // The shell leads its session (`setsid`): what it started and detached stays in it.
        if cfg!(unix) {
            for (pid, process) in system.processes() {
                if process.thread_kind().is_none()
                    && process.session_id() == Some(shell)
                    && !pids.contains(pid)
                {
                    pids.push(*pid);
                }
            }
        }
    }
    pids.into_iter()
        .filter_map(|pid| {
            Some(Running {
                pid: pid.as_u32(),
                started: system.process(pid)?.start_time(),
            })
        })
        .collect()
}

/// Kills those of `procs` that still run, and are still the same process (SIGKILL on Unix,
/// TerminateProcess on Windows): what ignored the hangup a closing terminal sends.
pub fn kill_left(procs: &[Running]) {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System};
    let pids: Vec<Pid> = procs.iter().map(|p| Pid::from_u32(p.pid)).collect();
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), ProcessRefreshKind::new());
    for running in procs {
        if let Some(process) = system.process(Pid::from_u32(running.pid))
            && process.start_time() == running.started
            && process.status() != ProcessStatus::Zombie
        {
            process.kill();
        }
    }
}

/// The processes that are a terminal's shell, `process` the one Templum started, named `name`, run
/// from the file `exe`, with `children` (each one's pid and file, oldest first): it, and the shell
/// it runs where it only starts one: macOS's `login` (its first child), Git for Windows'
/// `bin\bash.exe` and `bin\sh.exe` (the `usr\bin\bash.exe` beside them). `stop_agent` never ends
/// them.
pub fn shells_of(process: u32, name: &str, exe: &str, children: &[(u32, String)]) -> Vec<u32> {
    let mut shells = vec![process];
    if name == "login" {
        shells.extend(children.first().map(|(pid, _)| *pid));
    } else if let Some(real) = git_bash(exe) {
        let shell = children.iter().find(|(_, e)| e.eq_ignore_ascii_case(&real));
        shells.extend(shell.map(|(pid, _)| *pid));
    }
    shells
}

/// The shell Git for Windows' `exe` starts when it is `<Git>\bin\bash.exe` or `<Git>\bin\sh.exe`,
/// a wrapper: `<Git>\usr\bin\bash.exe`.
fn git_bash(exe: &str) -> Option<String> {
    let lower = exe.to_ascii_lowercase();
    let end = ["\\bin\\bash.exe", "\\bin\\sh.exe"]
        .into_iter()
        .find(|end| lower.ends_with(end))?;
    Some(format!(
        "{}\\usr\\bin\\bash.exe",
        &exe[..exe.len() - end.len()]
    ))
}

/// What `stop_agent` ends in the terminal whose `TEMPLUM_TERMINAL` is `terminal`
/// (docs/specs/agent-tools.md#stop_agent): `program` and every process below it, and every process
/// carrying `terminal` in its environment whose parent is neither one of `shells` nor below them,
/// what a tool's shell left when it quit, even in a session of its own, started after the program
/// did (`since` without one: when its agent did; neither, none). Never one of `shells`, a job
/// started from them, nor a daemon started before the program (an ssh-agent or a tmux server from
/// the shell's startup files); a process whose environment can't be read is no orphan. Read before
/// any signal, while the children still name their parents.
pub fn strays(
    terminal: &str,
    shells: &[u32],
    program: Option<u32>,
    since: Option<u64>,
) -> Vec<Running> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        ProcessRefreshKind::new().with_environ(UpdateKind::Always),
    );
    let shells: Vec<Pid> = shells.iter().map(|s| Pid::from_u32(*s)).collect();
    let mut kept: Vec<Pid> = Vec::new();
    for shell in &shells {
        kept.extend(descendants(&system, *shell));
    }
    let mut found: Vec<Pid> = program
        .map(Pid::from_u32)
        .filter(|p| !shells.contains(p))
        .map(|p| descendants(&system, p))
        .unwrap_or_default();
    let variable = format!("TEMPLUM_TERMINAL={terminal}");
    // Start times are whole seconds: one in the program's own second is taken for older.
    let after = program
        .and_then(|p| system.process(Pid::from_u32(p)))
        .map(sysinfo::Process::start_time)
        .or(since);
    for (pid, process) in system.processes() {
        let orphan = process.thread_kind().is_none()
            && after.is_some_and(|after| process.start_time() > after)
            && !process.parent().is_some_and(|p| kept.contains(&p))
            && process.environ().iter().any(|v| *v == *variable);
        if orphan && !shells.contains(pid) && !found.contains(pid) {
            found.push(*pid);
        }
    }
    found
        .into_iter()
        .filter_map(|pid| {
            Some(Running {
                pid: pid.as_u32(),
                started: system.process(pid)?.start_time(),
            })
        })
        .collect()
}

/// Asks those of `procs` that still run, and are still the same process, to end: SIGTERM on Unix
/// (an agent saves its session on it), TerminateProcess on Windows. [`kill_left`] ends what is left.
pub fn terminate(procs: &[Running]) {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, Signal, System};
    let pids: Vec<Pid> = procs.iter().map(|p| Pid::from_u32(p.pid)).collect();
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::Some(&pids), ProcessRefreshKind::new());
    for running in procs {
        if let Some(process) = system.process(Pid::from_u32(running.pid))
            && process.start_time() == running.started
            && process.status() != ProcessStatus::Zombie
            && process.kill_with(Signal::Term).is_none()
        {
            process.kill();
        }
    }
}

/// The level Windows ends this process at when the session ends, before every program at the
/// default 0x280: the first of the range applications may take.
#[cfg(windows)]
const SHUTDOWN_LEVEL: u32 = 0x3FF;

/// Asks Windows to end this process before the programs in its terminals at sign-out, restart
/// and shutdown. Every process starts at the same level, and Windows ends those in any order: it
/// could end the shells and agents first, while Templum, still running, closes their blocks and
/// then saves the canvas without them.
#[cfg(windows)]
pub fn quit_before_terminals() {
    use windows_sys::Win32::System::Threading::SetProcessShutdownParameters;
    // SAFETY: two numbers in, no pointers; it only sets this process's place in the order.
    #[allow(unsafe_code)]
    let set = unsafe { SetProcessShutdownParameters(SHUTDOWN_LEVEL, 0) };
    if set == 0 {
        eprintln!(
            "templum: couldn't ask Windows to close Templum before its terminals: {}",
            io::Error::last_os_error()
        );
    }
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
    /// More variables for the shell and its programs (`TEMPLUM_BUS`, `PATH`), set after
    /// Templum's own.
    pub env: Vec<(String, String)>,
}

pub struct Terminal {
    term: SharedTerm,
    /// The shell's process ID, when the OS gave one.
    pid: Option<u32>,
    /// The PTY's master side, to ask which process is in the foreground.
    #[cfg(unix)]
    master: Option<std::fs::File>,
    shared: Arc<Shared>,
    /// Its PTY's input; None for a viewer, which runs nothing ([`Terminal::viewer`]).
    pty: Option<EventLoopSender>,
    cols: u16,
    rows: u16,
    shell_name: String,
    /// Unique among the terminals of every Templum running; its programs see it as
    /// `TEMPLUM_TERMINAL`.
    id: String,
    /// Someone typed into it ([`Terminal::had_input`]).
    input: AtomicBool,
    /// How many times input reached its program ([`Terminal::inputs`]).
    inputs: AtomicU64,
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
        let id = next_id();
        env.insert("TEMPLUM_TERMINAL".into(), id.clone());
        // Read again for each terminal: a program installed since Templum started is found.
        if let Some(path) = crate::path_var().and_then(|p| p.into_string().ok()) {
            env.insert("PATH".into(), path);
        }
        env.extend(opts.env.iter().cloned());
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
        #[cfg(target_os = "linux")]
        if let Some(pid) = pid {
            crate::scope::move_to_scope(pid, id.clone(), shell_name.clone());
        }
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
            pty: Some(pty),
            cols: opts.cols,
            rows: opts.rows,
            shell_name,
            id,
            input: AtomicBool::new(false),
            inputs: AtomicU64::new(0),
        })
    }

    /// A terminal that runs nothing: it shows what Templum feeds it ([`Terminal::feed`]), and
    /// keys, pastes and resizes reach no program. `name` is its title until what it shows sets
    /// one.
    pub fn viewer(
        name: &str,
        (cols, rows): (u16, u16),
        palette: Palette,
        options: TermOptions,
        notify: Notify,
    ) -> Self {
        let shared = Arc::new(Shared::new(
            notify,
            window_size(cols, rows, (0, 0)),
            palette,
        ));
        let term = Term::new(
            options.config(),
            &TermSize::new(cols as usize, rows as usize),
            Listener(shared.clone()),
        );
        Self {
            term: SharedTerm(Arc::new(FairMutex::new(term)), shared.clone()),
            pid: None,
            #[cfg(unix)]
            master: None,
            shared,
            pty: None,
            cols,
            rows,
            shell_name: name.to_string(),
            id: next_id(),
            input: AtomicBool::new(false),
            inputs: AtomicU64::new(0),
        }
    }

    /// Shows `bytes` in a viewer, as a program's output: whole lines, escape sequences and all.
    /// Its watches hear them.
    pub fn feed(&self, bytes: &[u8]) {
        self.feeder().feed(bytes);
    }

    /// A handle that feeds and resizes this viewer from any thread.
    pub fn feeder(&self) -> Feeder {
        Feeder(self.term.clone())
    }

    /// The terminal's `TEMPLUM_TERMINAL`.
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn shared(&self) -> &SharedTerm {
        &self.term
    }

    pub fn grid_size(&self) -> (u16, u16) {
        (self.cols, self.rows)
    }

    /// The shell's title (OSC 0/2), or the shell's name.
    pub fn title(&self) -> String {
        self.shared
            .title()
            .unwrap_or_else(|| self.shell_name.clone())
    }

    pub fn exited(&self) -> bool {
        self.shared.exited.get().is_some()
    }

    /// Its shell was killed (SIGKILL, Unix) rather than ending: what systemd-oomd does to the
    /// scope of a terminal that uses too much memory (on Linux each terminal has its own).
    /// Templum itself never kills a shell whose block is still open.
    pub fn killed(&self) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            self.shared.ended.get().and_then(|s| s.signal()) == Some(libc::SIGKILL)
        }
        #[cfg(not(unix))]
        false
    }

    /// How long its shell lived, once it ended.
    pub fn lived(&self) -> Option<Duration> {
        let ended = self.shared.exited.get()?;
        Some(ended.duration_since(self.shared.born))
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

    /// Its shell's processes ([`shells_of`]): empty once it exited.
    pub fn shells(&self) -> Vec<u32> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        let Some(pid) = self.pid.filter(|_| !self.exited()) else {
            return Vec::new();
        };
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            ProcessRefreshKind::new().with_exe(UpdateKind::Always),
        );
        let exe = |p: &sysinfo::Process| {
            p.exe()
                .map(|e| e.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        let (name, file) = system
            .process(Pid::from_u32(pid))
            .map(|p| (p.name().to_string_lossy().into_owned(), exe(p)))
            .unwrap_or_default();
        let mut children: Vec<&sysinfo::Process> = system
            .processes()
            .values()
            .filter(|p| p.parent() == Some(Pid::from_u32(pid)) && p.thread_kind().is_none())
            .collect();
        children.sort_by_key(|p| p.start_time());
        let children: Vec<(u32, String)> = children
            .iter()
            .map(|p| (p.pid().as_u32(), exe(p)))
            .collect();
        shells_of(pid, &name, &file, &children)
    }

    /// The program in front of its shell, by pid and name: the leader of the PTY's foreground
    /// process group when it isn't one of `shells` (Unix); on Windows, which can't tell who is in
    /// front, `agent` (the agent's pid and start time a snapshot found) while it still runs below
    /// the shell, else the shell's oldest child ([`front`]).
    pub fn program(&self, shells: &[u32], agent: Option<(u32, u64)>) -> Option<(u32, String)> {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
        let mut system = System::new();
        let pid = if cfg!(windows) {
            system.refresh_processes_specifics(ProcessesToUpdate::All, ProcessRefreshKind::new());
            front(&table(&system), shells, agent)?
        } else {
            let front = self.foreground().filter(|f| !shells.contains(f))?;
            let pids = [Pid::from_u32(front)];
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&pids),
                ProcessRefreshKind::new(),
            );
            front
        };
        let name = system.process(Pid::from_u32(pid))?.name();
        Some((pid, name.to_string_lossy().into_owned()))
    }

    /// Looks for `pattern` in each line it prints from now on, escape sequences dropped, until
    /// [`Terminal::unwatch`]; the watch's id, for [`Terminal::hit`].
    pub fn watch(&self, pattern: Pattern) -> u64 {
        self.shared.watch(pattern)
    }

    /// The first line that matched the watch `id`, once one did.
    pub fn hit(&self, id: u64) -> Option<String> {
        self.shared.hit(id)
    }

    pub fn unwatch(&self, id: u64) {
        self.shared.unwatch(id);
    }

    /// How many watches it has.
    pub fn watching(&self) -> usize {
        self.shared.watches.lock().unwrap().len()
    }

    /// What the shell (with shell integration) and its programs told about their commands, and
    /// how often they asked for attention.
    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }

    /// The PTY's foreground process group: its leader is the shell (or the program it runs).
    /// Not rustix's `tcgetpgrp`: macOS answers 0 once the shell has ended, which rustix makes a
    /// `Pid` that can't be 0 (a panic in debug builds, undefined behavior in release ones).
    #[cfg(unix)]
    fn foreground(&self) -> Option<u32> {
        use std::os::fd::AsRawFd as _;
        let master = self.master.as_ref()?.as_raw_fd();
        // SAFETY: a plain call on the PTY master `self.master` keeps open; no pointers.
        #[allow(unsafe_code)]
        let group = unsafe { libc::tcgetpgrp(master) };
        process_group(group)
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

    /// Sends raw input (scrolling the view back to the bottom, like any typing). A viewer takes
    /// none.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        if self.pty.is_none() {
            return;
        }
        self.term.0.lock_unfair().scroll_display(Scroll::Bottom);
        self.term.touched();
        self.send(bytes);
    }

    fn send(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let bytes = bytes.into();
        // alacritty's event loop never finishes an empty write: all input after it would wait
        // behind it, forever (an IME clearing its composition replaces it with "").
        if bytes.is_empty() {
            return;
        }
        if let Some(pty) = &self.pty {
            self.inputs.fetch_add(1, Ordering::Relaxed);
            let _ = pty.send(Msg::Input(bytes));
        }
    }

    /// How many times input reached its program: a key, text, a paste, what the app wrote, a
    /// click, a drag, the wheel (as arrows too). Not a mouse move with no button down, nor its
    /// answers to the program's queries: they answer no dialog.
    pub fn inputs(&self) -> u64 {
        self.inputs.load(Ordering::Relaxed)
    }

    /// `write`, for input someone gave: a key, a paste, typed text ([`Terminal::had_input`]).
    /// Once the shell ended, input reaches nothing and ends nothing: it doesn't count.
    fn input(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        if self.pty.is_none() {
            return;
        }
        if !self.exited() {
            self.input.store(true, Ordering::Relaxed);
        }
        self.write(bytes);
    }

    /// Someone, the person or an agent, pressed a key, pasted or typed text into it
    /// (`key`, `press`, `text`, `paste`, `type_in`): a shell that ends then was ended, it didn't
    /// fail to start. What the app itself writes (`write`) doesn't count.
    pub fn had_input(&self) -> bool {
        self.input.load(Ordering::Relaxed)
    }

    /// Sends a key press. Returns false for printable text, which must come through the
    /// text-input path instead (IME, dead keys).
    pub fn key(&self, input: &KeyInput) -> bool {
        let mode = *self.term.0.lock_unfair().mode();
        match encode_key(input, mode) {
            Some(bytes) => {
                self.input(bytes);
                true
            }
            None => false,
        }
    }

    /// Presses `key` as the keyboard would: its bytes in the program's mode (`key`), else the
    /// character it types, raw: never as a paste, which a dialog would ignore.
    pub fn press(&self, key: &KeyInput) {
        if !self.key(key)
            && let Some(text) = key.key_char
        {
            self.input(text.as_bytes().to_vec());
        }
    }

    /// Reports a mouse event to the program, at its cell clamped to the grid. False when the
    /// program didn't ask for this event.
    pub fn mouse(&self, input: &MouseInput) -> bool {
        let term = self.term.0.lock_unfair();
        let mode = *term.mode();
        let input = MouseInput {
            row: input.row.min(term.screen_lines() - 1),
            col: input.col.min(term.columns() - 1),
            ..*input
        };
        drop(term);
        match encode_mouse(&input, mode) {
            Some(bytes) if input.button.is_some() => {
                self.send(bytes);
                true
            }
            Some(bytes) => {
                if let Some(pty) = &self.pty {
                    let _ = pty.send(Msg::Input(bytes.into()));
                }
                true
            }
            None => false,
        }
    }

    /// Sends what the keyboard typed as text (letters, an IME's, a dead key's), raw.
    pub fn text(&self, text: &str) {
        self.input(text.as_bytes().to_vec());
    }

    pub fn paste(&self, text: &str) {
        self.input(paste_bytes(text, self.bracketed()));
    }

    /// Types `text` in for the person to send: never presses Enter (see [`typed_bytes`]).
    pub fn type_in(&self, text: &str) {
        self.input(typed_bytes(text, self.bracketed()));
    }

    /// The program asked for pastes to be bracketed.
    pub fn bracketed(&self) -> bool {
        self.term
            .0
            .lock_unfair()
            .mode()
            .contains(TermMode::BRACKETED_PASTE)
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
            .lock_unfair()
            .resize(TermSize::new(cols as usize, rows as usize));
        self.term.touched();
        if let Some(pty) = &self.pty {
            let _ = pty.send(Msg::Resize(size));
        }
    }

    /// Scrolls to the previous (`up`) or the next prompt (shell integration marks them): the one
    /// above or below the top of the view goes to the top, or as near as the history allows.
    /// False when the view didn't move: no prompt there, or a full-screen program runs.
    pub fn jump_to_prompt(&self, up: bool) -> bool {
        Self::jump_in(&self.term, up)
    }

    fn jump_in(term: &SharedTerm, up: bool) -> bool {
        let mut grid = term.0.lock_unfair();
        if grid.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        let top = -(grid.grid().display_offset() as i32);
        let starts = crate::tap::prompt_lines(grid.grid());
        let target = if up {
            starts.into_iter().rfind(|&l| l < top)
        } else {
            starts.into_iter().find(|&l| l > top)
        };
        let Some(target) = target else {
            return false;
        };
        grid.scroll_display(Scroll::Delta(top - target));
        let moved = -(grid.grid().display_offset() as i32) != top;
        drop(grid);
        term.touched();
        moved
    }

    /// Its view is scrolled back from the end of its output.
    pub fn scrolled_back(&self) -> bool {
        self.term.0.lock_unfair().grid().display_offset() > 0
    }

    /// Scrolls the history (positive = up). In full-screen apps that asked for it (less,
    /// vim: alternate screen + alternate scroll), sends arrow keys instead.
    pub fn scroll(&self, lines: i32) {
        if lines == 0 {
            return;
        }
        let mut term = self.term.0.lock_unfair();
        let mode = *term.mode();
        if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL) {
            drop(term);
            let key = KeyInput {
                key: if lines > 0 { "up" } else { "down" },
                ..KeyInput::default()
            };
            let arrow = encode_key(&key, mode).expect("arrow keys always encode");
            self.send(arrow.repeat(lines.unsigned_abs() as usize));
        } else {
            term.scroll_display(Scroll::Delta(lines));
            drop(term);
            self.term.touched();
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Ends the event loop, which drops the PTY and kills the shell.
        if let Some(pty) = &self.pty {
            let _ = pty.send(Msg::Shutdown);
        }
    }
}

/// A new terminal's id, unique among the terminals of every Templum running: `<pid>-<n>`.
fn next_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
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

/// The process group `tcgetpgrp` answered, when there is one: it says 0 (macOS, once the shell
/// has ended) or less when there is none.
#[cfg(any(unix, test))]
fn process_group(raw: i32) -> Option<u32> {
    u32::try_from(raw).ok().filter(|&group| group > 0)
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
        TermOptions::default().config(),
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
    term.touched();
}

/// Feeds `bytes` as the PTY thread would: through the tap, which marks the prompts.
#[cfg(test)]
pub(crate) fn feed_heard(term: &SharedTerm, bytes: &[u8]) {
    let mut scanner = crate::tap::Scanner::new();
    let inserts = scanner.scan(bytes, &term.1);
    feed(term, &crate::tap::rewrite(bytes, inserts));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn windows_ends_templum_before_the_programs_in_its_terminals() {
        use windows_sys::Win32::System::Threading::GetProcessShutdownParameters;
        quit_before_terminals();
        let (mut level, mut flags) = (0, 0);
        // SAFETY: two valid places for the two numbers it writes.
        #[allow(unsafe_code)]
        let read = unsafe { GetProcessShutdownParameters(&mut level, &mut flags) };
        assert_ne!(read, 0, "{}", io::Error::last_os_error());
        // Windows ends the higher levels first; a shell or an agent starts at 0x280.
        assert!(level > 0x280, "level {level:#x}");
    }

    /// A viewer shows what it is fed, as a program's output, and runs nothing: no input reaches
    /// anything, and it closes with nothing to end.
    #[test]
    fn a_viewer_shows_what_it_is_fed_and_takes_no_input() {
        let woke = Arc::new(AtomicBool::new(false));
        let wake = woke.clone();
        let notify: Notify = Arc::new(move || wake.store(true, Ordering::SeqCst));
        let options = TermOptions::default();
        let mut term = Terminal::viewer("Explore", (30, 4), Palette::default(), options, notify);
        assert_eq!(term.title(), "Explore");
        let watch = term.watch(Pattern::new("Done").unwrap());
        term.feed("\x1b]2;Explore · List\x07\x1b[1mTask\x1b[0m\r\nList the files.\r\n".as_bytes());
        term.feed(b"\x1b[2m- Done\x1b[0m\r\n");
        assert!(woke.load(Ordering::SeqCst), "the UI hears it");
        assert_eq!(term.title(), "Explore · List");
        assert_eq!(
            term.shared().lines(3),
            ["Task", "List the files.", "- Done"]
        );
        assert_eq!(term.hit(watch).as_deref(), Some("- Done"));
        term.text("x");
        term.paste("y");
        term.press(&KeyInput {
            key: "enter",
            ..KeyInput::default()
        });
        term.write(&b"z"[..]);
        term.resize(40, 5, (8, 16));
        assert_eq!(term.grid_size(), (40, 5));
        assert!(
            !term.had_input() && term.inputs() == 0,
            "nothing takes input"
        );
        assert!(!term.exited());
        assert_eq!(term.processes(), Processes::default());
        assert!(term.shells().is_empty());
        assert_ne!(
            term.id(),
            Terminal::viewer(
                "x",
                (1, 1),
                Palette::default(),
                TermOptions::default(),
                Arc::new(|| {})
            )
            .id()
        );
    }

    #[test]
    fn a_feeder_feeds_and_resizes_a_viewer_from_another_thread() {
        let v = Terminal::viewer(
            "v",
            (20, 4),
            Palette::default(),
            TermOptions::default(),
            Arc::new(|| {}),
        );
        let f = v.feeder();
        std::thread::spawn(move || f.feed(b"hi")).join().unwrap();
        assert_eq!(v.shared().lines(1), ["hi"]);
        assert_eq!(v.shared().grid_size(), (20, 4));
        assert!(v.feeder().same_viewer(&v.feeder()));
        let other = Terminal::viewer(
            "w",
            (20, 4),
            Palette::default(),
            TermOptions::default(),
            Arc::new(|| {}),
        );
        assert!(!v.feeder().same_viewer(&other.feeder()));
        v.feeder().resize(30, 6);
        assert_eq!(v.shared().grid_size(), (30, 6));
        assert_eq!(v.shared().lines(1), ["hi"], "a resize keeps what was shown");
    }

    #[test]
    fn no_foreground_group_is_no_process() {
        assert_eq!(process_group(0), None);
        assert_eq!(process_group(-1), None);
        assert_eq!(process_group(4242), Some(4242));
    }

    #[test]
    fn modes_say_what_the_program_asked_for() {
        let (term, _) = headless(10, 2);
        assert_eq!(term.modes(), Modes::default());
        feed(&term, b"\x1b[>1u");
        assert!(term.modes().kitty_keys, "the kitty keyboard is on offer");
        // Each screen has its own keyboard flags.
        feed(&term, b"\x1b[?1000h\x1b[?1049h");
        assert!(!term.modes().kitty_keys);
        feed(&term, b"\x1b[>1u");
        assert_eq!(
            term.modes(),
            Modes {
                mouse: true,
                kitty_keys: true,
                full_screen: true,
                ..Modes::default()
            }
        );
        // Bracketed pastes and a shown cursor: its input box. Claude Code and Codex hide the cursor
        // in their dialogs.
        let (term, _) = headless(10, 2);
        feed(&term, b"\x1b[?2004h");
        assert!(term.modes().typing);
        feed(&term, b"\x1b[?25l");
        assert!(!term.modes().typing, "a dialog");
        feed(&term, b"\x1b[?25h\x1b[?2004l");
        assert!(!term.modes().typing, "no bracketed pastes");
    }

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
        shared.pending.store(false, Ordering::Release); // what SharedTerm::rearm does
        shared.wake();
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn output_racing_a_rearm_is_either_read_or_wakes_the_ui() {
        // The UI re-arms, then `refresh` checks `dirty`; the PTY thread sets `dirty`, then
        // `pending`. However they interleave, one sees the other: the UI reads the output, or the
        // PTY thread wakes it again. The two threads go in lockstep, a random few spins apart.
        use std::sync::atomic::AtomicU64;
        fn spin(rng: &mut u64) {
            *rng ^= *rng << 13;
            *rng ^= *rng >> 7;
            *rng ^= *rng << 17;
            for _ in 0..*rng % 8 {
                std::hint::spin_loop();
            }
        }
        const ROUNDS: u64 = 200_000;
        let woken = Arc::new(AtomicU64::new(0));
        let w = woken.clone();
        let shared = Arc::new(Shared::new(
            Arc::new(move || {
                w.fetch_add(1, Ordering::SeqCst);
            }),
            window_size(10, 2, (8, 16)),
            Palette::default(),
        ));
        let term = SharedTerm(
            Arc::new(FairMutex::new(Term::new(
                Config::default(),
                &TermSize::new(10, 2),
                Listener(shared.clone()),
            ))),
            shared.clone(),
        );
        let (go, done) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let pty = {
            let (shared, go, done) = (shared.clone(), go.clone(), done.clone());
            std::thread::spawn(move || {
                let mut rng = 12345;
                for i in 1..=ROUNDS {
                    while go.load(Ordering::Acquire) != i {
                        std::hint::spin_loop();
                    }
                    spin(&mut rng);
                    shared.wake();
                    done.store(i, Ordering::Release);
                }
            })
        };
        let mut rng = 999;
        let mut lost = 0;
        for i in 1..=ROUNDS {
            // Everything was read, and a wake-up waits to be re-armed.
            shared.dirty.store(false, Ordering::Relaxed);
            shared.pending.store(true, Ordering::Relaxed);
            let before = woken.load(Ordering::SeqCst);
            go.store(i, Ordering::Release);
            spin(&mut rng);
            // What a frame does: re-arm, then `refresh` asks whether the grid changed.
            term.rearm();
            let read = term.dirty();
            while done.load(Ordering::Acquire) != i {
                std::hint::spin_loop();
            }
            if !read && woken.load(Ordering::SeqCst) == before {
                lost += 1;
            }
        }
        pty.join().unwrap();
        assert_eq!(
            lost, 0,
            "output neither read nor waking the UI, in {ROUNDS} rounds"
        );
    }

    #[test]
    fn exited_waits_for_the_last_output() {
        // alacritty sends ChildExit before it reads the child's last output, and Exit after.
        let (_, shared) = headless(10, 2);
        let listener = Listener(shared.clone());
        listener.send_event(Event::ChildExit(std::process::ExitStatus::default()));
        assert!(shared.exited.get().is_none(), "output may still be unread");
        listener.send_event(Event::Exit);
        assert!(shared.exited.get().is_some());
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
    fn an_exit_wakes_the_ui_even_before_the_terminal_is_rearmed() {
        // A block off screen is never re-armed: its program's exit must still reach the UI.
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
        Listener(shared.clone()).send_event(Event::Exit);
        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "not held back by the coalescing"
        );
        assert!(shared.exited.get().is_some());
    }

    /// Windows can't tell who is in front: under Git Bash's wrapper and its real shell, the
    /// program is the agent a snapshot found while it is still the same process below the shell,
    /// else the shell's oldest child; never the real shell, nor a process that took the agent's
    /// pid after it quit.
    #[test]
    fn on_windows_the_program_is_the_agent_still_below_the_shell_else_its_oldest_child() {
        let p = |pid, parent, started| Proc {
            pid,
            parent: Some(parent),
            started,
        };
        // Git's bin\bash.exe 10 starts the real bash 11; Claude Code 12 (and its node 13), then
        // a job the person started, 14; 20 is elsewhere.
        let tree = [
            p(10, 1, 100),
            p(11, 10, 100),
            p(14, 11, 300),
            p(12, 11, 200),
            p(13, 12, 201),
            p(20, 1, 50),
        ];
        let shells = [10, 11];
        assert_eq!(front(&tree, &shells, Some((14, 300))), Some(14));
        assert_eq!(front(&tree, &shells, None), Some(12), "the oldest child");
        assert_eq!(
            front(&tree, &shells, Some((20, 50))),
            Some(12),
            "not below the shell"
        );
        assert_eq!(front(&tree, &shells, Some((11, 100))), Some(12));
        // The agent 12 quit; its pid was taken again by a job started after the person's 14.
        let taken = [
            p(10, 1, 100),
            p(11, 10, 100),
            p(14, 11, 300),
            p(12, 11, 400),
        ];
        assert_eq!(front(&taken, &shells, Some((12, 200))), Some(14));
        let idle = [p(10, 1, 100), p(11, 10, 100)];
        assert_eq!(
            front(&idle, &shells, None),
            None,
            "the real shell isn't the program"
        );
        assert_eq!(front(&idle, &[], None), None);
    }

    /// An agent started with its folder variable set, and one without it, are found with each.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_found_agents_folder_variable_is_read() {
        use std::os::unix::fs::PermissionsExt as _;
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("claude");
        std::fs::write(&script, "#!/bin/sh\nsleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let known = [Detect {
            id: "claude".into(),
            names: vec!["claude".into()],
            packages: Vec::new(),
            var: Some("CLAUDE_CONFIG_DIR".into()),
        }];
        let start = |value: Option<&str>| {
            let mut command = std::process::Command::new(&script);
            match value {
                Some(v) => command.env("CLAUDE_CONFIG_DIR", v),
                None => command.env_remove("CLAUDE_CONFIG_DIR"),
            };
            Child(command.spawn().unwrap())
        };
        // The script is the agent once its shell has exec'd it: look until it is found.
        let read = |child: &Child| {
            let pid = child.0.id();
            let p = Processes {
                shell: Some(pid),
                foreground: Some(pid),
                reported: None,
            };
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let seen = inspect(std::slice::from_ref(&p), &known).pop().unwrap();
                if seen.agent.is_some() {
                    return seen;
                }
                assert!(std::time::Instant::now() < deadline, "{seen:?}");
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        };
        let (with, without) = (start(Some("/x/acc")), start(None));
        assert_eq!(read(&with).folder_var, Some(Some(PathBuf::from("/x/acc"))));
        assert_eq!(read(&without).folder_var, Some(None));
    }

    #[test]
    fn an_agent_whose_folder_the_os_wont_tell_is_in_its_shells() {
        use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
        // This test's process as the agent, read without folders: as one the OS won't tell of.
        let me = std::process::id();
        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[Pid::from_u32(me)]),
            ProcessRefreshKind::new().with_cmd(UpdateKind::Always),
        );
        let exe = std::env::current_exe().unwrap();
        let name = exe.file_stem().unwrap().to_string_lossy().to_lowercase();
        let known = [Detect {
            id: "agent".into(),
            names: vec![name],
            packages: Vec::new(),
            var: None,
        }];
        let reported = tempfile::tempdir().unwrap();
        let p = Processes {
            shell: Some(me),
            foreground: None,
            reported: Some(reported.path().to_path_buf()),
        };
        let found = inspect_one(&system, &p, &known);
        assert_eq!(found.agent.as_deref(), Some("agent"));
        assert_eq!(found.folder.as_deref(), Some(reported.path()));
    }

    #[test]
    fn the_reported_folder_counts_while_the_shell_is_in_front() {
        // Canonical: the OS may spell a folder another way (Windows).
        let folder = |p: Processes| {
            let folder = inspect(&[p], &[]).pop().unwrap().folder;
            folder.map(|f| f.canonicalize().unwrap())
        };
        let me = std::process::id();
        let mine = std::env::current_dir().unwrap().canonicalize().unwrap();
        let reported = tempfile::tempdir().unwrap();
        let canonical = reported.path().canonicalize().unwrap();
        let p = Processes {
            shell: Some(me),
            foreground: None,
            reported: Some(reported.path().to_path_buf()),
        };
        assert_eq!(
            folder(p.clone()).as_deref(),
            Some(canonical.as_path()),
            "no foreground (Windows)"
        );
        let front = Processes {
            foreground: Some(me),
            ..p.clone()
        };
        assert_eq!(
            folder(front).as_deref(),
            Some(canonical.as_path()),
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

    #[test]
    fn lines_reach_into_the_history_without_the_blank_screen_below() {
        let (term, _) = headless(20, 4);
        for n in 1..=10 {
            feed(&term, format!("{n}\r\n").as_bytes());
        }
        // 10 lines on a 4-row screen: 7 in the history, the cursor on a blank last row.
        assert_eq!(term.lines(3), ["8", "9", "10"]);
        let all = term.lines(100);
        assert_eq!((all.len(), all[0].as_str()), (10, "1"));
        assert!(term.lines(0).is_empty());
    }

    #[test]
    fn a_suggestion_where_the_cursor_waits_is_not_in_the_lines() {
        // Claude Code 2.1.309's empty input, as it draws it: its placeholder faint, the cursor
        // shown on the placeholder's first cell.
        let (term, _) = headless(40, 4);
        feed(
            &term,
            "\x1b[?25l\x1b[2;1H────\r\x1b[1B\x1b[39m❯\u{a0}\x1b[2mTry \"fix lint errors\"\r\x1b[1B\
             \x1b[22m────\x1b[3;3H\x1b[?25h"
                .as_bytes(),
        );
        assert_eq!(term.lines(4), ["", "────", "❯", "────"]);
        // Codex 0.154.0's; faint text the cursor isn't on is the screen's.
        feed(
            &term,
            b"\x1b[2J\x1b[1;1H\x1b[1m\xe2\x80\xba\x1b[22m \x1b[2mAsk Codex to do anything\
              \x1b[22m\r\n\x1b[2mtip\x1b[22m\r\n> /he\x1b[2mlp\x1b[22m done\x1b[1;3H",
        );
        assert_eq!(term.lines(3), ["›", "tip", "> /help done"]);
        // A completion after what was typed.
        feed(&term, b"\x1b[3;6H");
        assert_eq!(term.lines(1), ["> /he done"]);
        // Under a hidden cursor, nothing waits for typing.
        feed(&term, b"\x1b[?25l");
        assert_eq!(term.lines(1), ["> /help done"]);
    }

    #[test]
    fn the_last_command_is_its_prompt_and_output_up_to_the_next_prompt() {
        let (term, _) = headless(30, 8);
        feed_heard(
            &term,
            b"\x1b]133;A\x07$ ls\r\n\x1b]133;C\x07a b\r\n\x1b]133;D;0\x07\
              \x1b]133;A\x07$ make\r\n\x1b]133;C\x07error\r\n\x1b]133;D;2\x07\
              \x1b]133;A\x07$ ",
        );
        assert_eq!(term.last_command().as_deref(), Some("$ make\nerror"));
        let (fresh, _) = headless(30, 8);
        assert_eq!(
            fresh.last_command(),
            None,
            "no shell integration, no command"
        );
    }
    #[test]
    fn modes_say_every_motion_apart_from_the_mouse() {
        let v = Terminal::viewer(
            "v",
            (10, 2),
            Palette::default(),
            TermOptions::default(),
            Arc::new(|| {}),
        );
        let modes = |bytes: &[u8]| {
            v.feed(bytes);
            let m = v.shared().modes();
            (m.mouse, m.mouse_moves)
        };
        assert_eq!(modes(b"\x1b[?1000h"), (true, false), "clicks");
        assert_eq!(modes(b"\x1b[?1002h"), (true, false), "drags");
        assert_eq!(modes(b"\x1b[?1003h"), (true, true), "every motion");
        assert_eq!(modes(b"\x1b[?1003l"), (false, false));
    }

    #[test]
    fn a_mouse_event_is_wanted_as_the_modes_say() {
        use crate::input::Button;
        let at = |kind, button| MouseInput {
            button,
            kind,
            row: 0,
            col: 0,
            ctrl: false,
            alt: false,
            shift: false,
        };
        let clicks = Modes {
            mouse: true,
            ..Modes::default()
        };
        let every = Modes {
            mouse: true,
            mouse_moves: true,
            ..Modes::default()
        };
        for e in [
            at(MouseKind::Press, Some(Button::Left)),
            at(MouseKind::Release, Some(Button::Left)),
            at(MouseKind::Press, Some(Button::WheelUp)),
            at(MouseKind::Move, Some(Button::Left)),
        ] {
            assert!(
                clicks.wants(&e) && every.wants(&e) && !Modes::default().wants(&e),
                "{e:?}"
            );
        }
        let hover = at(MouseKind::Move, None);
        assert!(!clicks.wants(&hover) && every.wants(&hover));
    }

    /// A viewer as a mirror makes one: no scrollback of its own, home's rows written without
    /// autowrap.
    fn mirrored(cols: u16, rows: u16, screen: &[u8]) -> Terminal {
        let options = TermOptions {
            scrollback: 0,
            ..TermOptions::default()
        };
        let v = Terminal::viewer(
            "v",
            (cols, rows),
            Palette::default(),
            options,
            Arc::new(|| {}),
        );
        v.feed(b"\x1b[?7l");
        v.feed(screen);
        v
    }

    #[test]
    fn history_goes_above_the_live_screen_and_the_screen_stays() {
        let v = mirrored(10, 4, b"s1\r\ns2\r\ns3\r\ns4");
        let before = v.shared().screen().unwrap();
        assert!(v.feeder().fill_history("h1\nh2\nh3\ns1\ns2\ns3\ns4"));
        assert_eq!(
            v.shared().lines(100),
            ["h1", "h2", "h3", "s1", "s2", "s3", "s4"]
        );
        assert_eq!(
            v.shared().screen().unwrap(),
            before,
            "the live screen and its cursor stay"
        );
    }

    #[test]
    fn history_above_a_screen_that_isnt_full_loses_and_doubles_nothing() {
        let v = mirrored(10, 4, b"s1\r\ns2\r\n");
        let before = v.shared().screen().unwrap();
        // Home's text ends with its screen down to the last row written; a trailing blank line
        // is tolerated.
        assert!(v.feeder().fill_history("h1\nh2\nh3\ns1\ns2\n"));
        assert_eq!(v.shared().lines(100), ["h1", "h2", "h3", "s1", "s2"]);
        assert_eq!(v.shared().screen().unwrap(), before);
        let v = mirrored(10, 4, b"s1\r\ns2");
        assert!(v.feeder().fill_history("h1\ns1\ns2"));
        assert_eq!(v.shared().lines(100), ["h1", "s1", "s2"]);
    }

    #[test]
    fn long_lines_wrap_again_at_home_s_width() {
        let v = mirrored(10, 4, b"s1");
        assert!(v.feeder().fill_history("abcdefghijklmno\ns1"));
        assert_eq!(v.shared().lines(100), ["abcdefghijklmno", "s1"]);
    }

    #[test]
    fn a_fill_leaves_the_viewer_s_line_wrap_as_it_was() {
        // The painter turns the wrap off once, on its first frame: a row longer than the grid
        // after a fill must not wrap onto the next one.
        let v = mirrored(10, 4, b"s1");
        assert!(v.feeder().fill_history("h1\ns1"));
        v.feed(b"\x1b[2;1Hxy\x1b[1;1HABCDEFGHIJKLMNOP");
        assert_eq!(v.shared().lines(100), ["h1", "ABCDEFGHIP", "xy"]);
    }

    #[test]
    fn a_viewer_in_the_alternate_screen_takes_no_history() {
        let v = mirrored(10, 4, b"\x1b[?1049hvim");
        assert!(!v.feeder().fill_history("h1\nvim"));
        v.feed(b"\x1b[?1049l");
        assert!(!v.shared().lines(100).iter().any(|l| l == "h1"));
    }

    #[test]
    fn history_from_home_is_drawn_never_run() {
        let v = mirrored(40, 4, b"s1");
        assert!(
            v.feeder()
                .fill_history("\x1b]2;evil\x07a\x1b[?1049hb\x07\nh2\ns1")
        );
        let shared = v.shared();
        assert_eq!(
            shared.lines(100),
            ["]2;evila[?1049hb", "h2", "s1"],
            "shown as text, not run"
        );
        assert_eq!(shared.screen().unwrap().title, None);
        assert!(!shared.modes().full_screen);
    }

    #[test]
    fn the_scrollback_says_where_the_view_is_and_scroll_to_moves_it() {
        let (term, _) = headless(10, 4);
        assert_eq!(term.scrollback().unwrap().history, 0, "a fresh terminal");
        feed(&term, b"1\r\n2\r\n3\r\n4\r\n5\r\n6\r\n7\r\n8\r\n9");
        let back = term.scrollback().unwrap();
        assert_eq!((back.history, back.below, back.lines), (5, 0, 4));
        term.scroll_to(3);
        assert_eq!(term.scrollback().unwrap().below, 3);
        let mut snap = crate::Snapshot::default();
        snap.refresh(&term, &Palette::default());
        assert_eq!(snap.scrollback.below, 3, "the snapshot keeps it");
        assert_eq!(snap.rows[0].text, "3", "three lines over the screen's 6");
        term.scroll_to(100);
        assert_eq!(term.scrollback().unwrap().below, 5, "as far as it goes");
        term.scroll_to(0);
        assert_eq!(term.scrollback().unwrap().below, 0);
        let held = term.0.lock();
        assert_eq!(term.scrollback(), None, "painting doesn't wait");
        drop(held);
        feed(&term, b"\x1b[?1049h\x1b[?1000h");
        let full = term.scrollback().unwrap();
        assert_eq!(full.history, 0, "full screen has no history");
        assert!(full.mouse);
    }

    #[test]
    fn history_keeps_at_most_two_thousand_rows() {
        let v = mirrored(10, 4, b"");
        let text: Vec<String> = (0..3000).map(|n| n.to_string()).collect();
        assert!(v.feeder().fill_history(&text.join("\n")));
        let lines = v.shared().lines(10_000);
        assert!(lines.len() <= VIEWER_HISTORY, "{}", lines.len());
        assert_eq!(lines.last().map(String::as_str), Some("2999"));
    }

    #[test]
    fn a_fill_keeps_the_view_where_the_person_left_it() {
        let v = mirrored(10, 4, b"s1");
        let text: Vec<String> = (1..=30)
            .map(|n| format!("h{n}"))
            .chain(["s1".into()])
            .collect();
        assert!(v.feeder().fill_history(&text.join("\n")));
        v.scroll(5);
        v.shared()
            .select_start(0, 0, Side::Left, SelectionKind::Line);
        assert!(v.feeder().fill_history(&text.join("\n")));
        assert!(v.scrolled_back(), "still scrolled back");
        assert_eq!(
            v.shared().selection_text(),
            None,
            "a selection in the old history goes"
        );
        v.feeder().drop_history();
        assert!(!v.scrolled_back());
        assert_eq!(v.shared().lines(100), ["s1"]);
    }

    #[test]
    fn fed_counts_what_the_painter_feeds_never_a_fill() {
        let v = mirrored(10, 4, b"");
        let f = v.feeder();
        let n = f.fed();
        f.feed(b"x");
        assert_eq!(f.fed(), n + 1);
        assert!(f.fill_history("h1\nx"));
        assert_eq!(f.fed(), n + 1, "a history isn't a frame");
    }
}
