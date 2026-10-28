//! Terminal streams (docs/specs/api.md, Batch 2): a terminal's live screen as `frame` events a
//! client can redraw. This file holds the frame (what a screen becomes as JSON, and what changed
//! between two), the terminals the UI thread lends and the reader that turns them into frames.

use crate::events::{self, Cell, Ids};
use crate::model::{Caller, Color, CursorView, FrameView, ModesView, RowView, RunView, SizeView};
use bytes::Bytes;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use templum_terminal::{CursorShape, Paint, Screen, SharedTerm, Styled};

/// Frames queued for one terminal stream (a second of them) before it is declared behind.
pub(crate) const FRAMES_QUEUED: usize = 20;
/// Bytes queued for one terminal stream: a big block's frames are megabytes each, and a sleeping
/// client must not hold twenty of them.
pub(crate) const BYTES_QUEUED: usize = 8 * 1024 * 1024;
/// How often the reader looks at the terminals that changed: 20 frames a second at most.
pub(crate) const FRAME_EVERY: Duration = Duration::from_millis(50);

/// A terminal the UI thread lends the server: its block, its workspace and its grid.
#[derive(Clone)]
pub struct TerminalRef {
    pub block: String,
    pub workspace: String,
    pub term: SharedTerm,
}

fn color(paint: Paint) -> Option<Color> {
    match paint {
        Paint::Default => None,
        Paint::Index(i) => Some(Color::Index(i)),
        Paint::Rgb(c) => Some(Color::Rgb(format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b))),
    }
}

fn run(styled: &Styled) -> RunView {
    RunView {
        text: styled.text.clone(),
        fg: color(styled.fg),
        bg: color(styled.bg),
        bold: styled.bold,
        dim: styled.dim,
        italic: styled.italic,
        underline: styled.underline,
        inverse: styled.inverse,
    }
}

fn row(y: usize, runs: &[Styled]) -> RowView {
    RowView {
        y,
        runs: runs.iter().map(run).collect(),
    }
}

/// What `now` shows that `last` (what the client has) doesn't: None when nothing; the whole screen when there is no
/// `last` or its size or screen (`modes.full_screen`) differs; else the rows that differ, by `y`. Size, cursor, title
/// and modes come in every frame.
pub(crate) fn frame(last: Option<&Screen>, now: &Screen) -> Option<FrameView> {
    if last == Some(now) {
        return None;
    }
    // Another screen (`?1049`) is a new picture: the rows that look alike are not the same ones.
    let last = last.filter(|l| {
        l.cols == now.cols
            && l.rows.len() == now.rows.len()
            && l.modes.full_screen == now.modes.full_screen
    });
    let rows = now
        .rows
        .iter()
        .enumerate()
        .filter(|(y, runs)| last.is_none_or(|l| l.rows[*y] != **runs))
        .map(|(y, runs)| row(y, runs))
        .collect();
    Some(FrameView {
        size: SizeView {
            cols: now.cols,
            rows: now.rows.len(),
        },
        cursor: now.cursor.map(|c| CursorView {
            row: c.row,
            col: c.col,
            shape: match c.shape {
                CursorShape::Underline => "underline",
                CursorShape::Beam => "beam",
                _ => "block",
            }
            .into(),
            blinking: c.blinking,
        }),
        title: now.title.clone(),
        whole: last.is_none(),
        rows,
        modes: ModesView {
            full_screen: now.modes.full_screen,
            mouse: now.modes.mouse,
            mouse_moves: now.modes.mouse_moves,
            kitty_keys: now.modes.kitty_keys,
        },
    })
}

/// The screen whole: what a client that has nothing (or fell behind) is sent.
pub(crate) fn whole(now: &Screen) -> FrameView {
    frame(None, now).expect("a frame with nothing before it is never empty")
}

struct Stream {
    caller: Caller,
    block: String,
    conn: u64,
    cell: Arc<Cell>,
}

/// What the reader last read of a terminal: the count of its changes before the read, and the
/// screen.
struct Read {
    changes: u64,
    screen: Arc<Screen>,
}

struct Lent {
    workspace: String,
    term: SharedTerm,
    read: Option<Read>,
}

/// The terminal streams and the terminals lent for them. The server keeps a terminal only while a
/// stream names its block, so a closed block's grid and scrollback are not held alive.
#[derive(Default)]
pub(crate) struct Screens {
    next: u64,
    streams: HashMap<u64, Stream>,
    lent: HashMap<String, Lent>,
}

impl Screens {
    /// A stream on `block`, pending until the block is lent. Its first frame is whole.
    pub fn open(&mut self, caller: Caller, block: &str, conn: u64) -> (u64, Arc<Cell>) {
        let cell = Arc::new(Cell::new(FRAMES_QUEUED, BYTES_QUEUED, true));
        let id = self.next;
        self.next += 1;
        self.streams.insert(
            id,
            Stream {
                caller,
                block: block.to_string(),
                conn,
                cell: cell.clone(),
            },
        );
        (id, cell)
    }

    /// The stream's block is lent and its caller sees the block's workspace.
    pub fn admits(&self, id: u64) -> bool {
        self.streams.get(&id).is_some_and(|stream| {
            self.lent
                .get(&stream.block)
                .is_some_and(|lent| stream.caller.sees(Some(&lent.workspace)))
        })
    }

    /// The stream is over, whoever ended it; a terminal no stream names is let go.
    pub fn close(&mut self, id: u64) {
        if let Some(stream) = self.streams.remove(&id) {
            stream.cell.end();
            self.release();
        }
    }

    fn release(&mut self) {
        let streams = &self.streams;
        self.lent
            .retain(|block, _| streams.values().any(|s| s.block == *block));
    }

    /// The terminals now. Ignored while no stream is open. A stream whose block is not among them,
    /// or whose caller doesn't see its workspace, ends. A block whose terminal is the same keeps
    /// what was read of it; another is read anew.
    pub fn set(&mut self, terminals: Vec<TerminalRef>) {
        if self.streams.is_empty() {
            return;
        }
        let gone: Vec<u64> = self
            .streams
            .iter()
            .filter(|(_, stream)| {
                !terminals
                    .iter()
                    .any(|t| t.block == stream.block && stream.caller.sees(Some(&t.workspace)))
            })
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.close(id);
        }
        let mut old = std::mem::take(&mut self.lent);
        for t in terminals {
            if !self.streams.values().any(|s| s.block == t.block) {
                continue;
            }
            let read = old
                .remove(&t.block)
                .filter(|lent| lent.term.same(&t.term))
                .and_then(|lent| lent.read);
            self.lent.insert(
                t.block,
                Lent {
                    workspace: t.workspace,
                    term: t.term,
                    read,
                },
            );
        }
    }

    /// Ends the streams whose caller `dead` says so of.
    pub fn cut(&mut self, dead: impl Fn(&Caller) -> bool) {
        let gone: Vec<u64> = self
            .streams
            .iter()
            .filter(|(_, s)| dead(&s.caller))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.close(id);
        }
    }

    /// Ends the streams of `token` that are on one of `conns`: its connections from another
    /// session are being cut.
    pub fn end_on(&mut self, token: u64, conns: &[u64]) {
        let gone: Vec<u64> = self
            .streams
            .iter()
            .filter(|(_, s)| s.caller.token == token && conns.contains(&s.conn))
            .map(|(id, _)| *id)
            .collect();
        for id in gone {
            self.close(id);
        }
    }

    pub fn ping(&self) {
        for stream in self.streams.values() {
            stream.cell.push(events::ping());
        }
    }

    /// How many streams are open (pending ones included).
    pub fn len(&self) -> usize {
        self.streams.len()
    }

    pub fn on_conn(&self, conn: u64) -> bool {
        self.streams.values().any(|s| s.conn == conn)
    }

    #[cfg(test)]
    pub fn lent(&self) -> usize {
        self.lent.len()
    }
}

fn bytes_of(ids: &Ids, frame: &FrameView) -> Bytes {
    events::frame(
        &ids.next().1,
        "frame",
        &serde_json::to_value(frame).unwrap_or(Value::Null),
    )
}

/// One round of the reader: each lent terminal that has a stream and changed is read, without
/// waiting, and its streams are sent what differs. Streams that are behind (a new one, or one that
/// let its queue fill) get the newest screen whole. The lock is never held while a terminal is read.
pub(crate) fn look(screens: &Mutex<Screens>, ids: &Ids) {
    let lock = || screens.lock().unwrap_or_else(|e| e.into_inner());
    struct Work {
        block: String,
        term: SharedTerm,
        read: Option<(u64, Arc<Screen>)>,
        cells: Vec<Arc<Cell>>,
    }
    let work: Vec<Work> = {
        let screens = lock();
        screens
            .lent
            .iter()
            .filter_map(|(block, lent)| {
                let cells: Vec<Arc<Cell>> = screens
                    .streams
                    .values()
                    .filter(|s| s.block == *block)
                    .map(|s| s.cell.clone())
                    .collect();
                (!cells.is_empty()).then(|| Work {
                    block: block.clone(),
                    term: lent.term.clone(),
                    read: lent.read.as_ref().map(|r| (r.changes, r.screen.clone())),
                    cells,
                })
            })
            .collect()
    };
    struct Done {
        block: String,
        term: SharedTerm,
        read: Option<Read>,
        delta: Option<Bytes>,
        whole: Option<Bytes>,
        cells: Vec<Arc<Cell>>,
    }
    let mut done: Vec<Done> = Vec::new();
    for w in work {
        let behind = w.cells.iter().any(|c| c.behind());
        // The count before the screen: a change during the read is read again next round.
        let changes = w.term.changes();
        let changed = w.read.as_ref().is_none_or(|(at, _)| *at != changes);
        if !changed && !behind {
            continue;
        }
        let (read, delta, screen) = if changed {
            // None: the lock is held; nothing is recorded and the next round looks again.
            let Some(now) = w.term.screen() else { continue };
            let delta = frame(w.read.as_ref().map(|(_, s)| &**s), &now);
            let now = Arc::new(now);
            (
                Some(Read {
                    changes,
                    screen: now.clone(),
                }),
                delta,
                now,
            )
        } else {
            let screen = w.read.as_ref().map(|(_, s)| s.clone()).unwrap();
            (None, None, screen)
        };
        let all_behind = w.cells.iter().all(|c| c.behind());
        done.push(Done {
            block: w.block,
            term: w.term,
            read,
            delta: delta.filter(|_| !all_behind).map(|f| bytes_of(ids, &f)),
            whole: behind.then(|| bytes_of(ids, &whole(&screen))),
            cells: w.cells,
        });
    }
    let mut screens = lock();
    for d in done {
        // Still the terminal that was read: it may have been replaced or let go since.
        let Some(lent) = screens
            .lent
            .get_mut(&d.block)
            .filter(|lent| lent.term.same(&d.term))
        else {
            continue;
        };
        if d.read.is_some() {
            lent.read = d.read;
        }
        for cell in d.cells {
            if cell.behind() {
                if let Some(whole) = &d.whole {
                    cell.resync(whole.clone());
                }
            } else if let Some(delta) = &d.delta {
                cell.push(delta.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Color;
    use serde_json::json;
    use std::sync::Arc;
    use templum_terminal::{Palette, TermOptions, Terminal};

    pub(crate) fn viewer(cols: u16, rows: u16) -> Terminal {
        Terminal::viewer(
            "v",
            (cols, rows),
            Palette::default(),
            TermOptions::default(),
            Arc::new(|| {}),
        )
    }

    fn screen(v: &Terminal) -> Screen {
        v.shared().screen().unwrap()
    }

    fn refer(block: &str, workspace: &str, v: &Terminal) -> TerminalRef {
        TerminalRef {
            block: block.into(),
            workspace: workspace.into(),
            term: v.shared().clone(),
        }
    }

    fn caller(token: u64, workspaces: Option<&[&str]>) -> Caller {
        Caller {
            token,
            name: "t".into(),
            scope: crate::store::Scope::Read,
            workspaces: workspaces.map(|w| w.iter().map(|s| s.to_string()).collect()),
        }
    }

    /// The frame inside an SSE event.
    fn decode(bytes: Bytes) -> FrameView {
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(text.contains("\nevent: frame\n"), "{text}");
        let data = text.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
        serde_json::from_str(data).unwrap()
    }

    fn texts(frame: &FrameView) -> Vec<String> {
        frame
            .rows
            .iter()
            .map(|r| r.runs.iter().map(|run| run.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn a_switch_of_screens_is_a_whole_frame() {
        // The same blank rows in the main screen and the alternate one: a delta would send
        // none of them, and the mirror would keep the old screen's picture under them.
        let home = viewer(10, 3);
        home.feed(b"keep\r\nme");
        let main = screen(&home);
        home.feed(b"\x1b[?1049h");
        let alternate = screen(&home);
        assert!(!main.modes.full_screen && alternate.modes.full_screen);
        let switched = frame(Some(&main), &alternate).unwrap();
        assert!(switched.whole, "{switched:?}");
        let back = frame(Some(&alternate), &main).unwrap();
        assert!(back.whole);
        home.feed(b"\x1b[?1049lx");
        let moved = frame(Some(&main), &screen(&home)).unwrap();
        assert!(!moved.whole, "the same screen stays a delta");
    }

    #[test]
    fn the_terminals_are_kept_only_while_a_stream_is_open() {
        let mut screens = Screens::default();
        let v = viewer(10, 2);
        screens.set(vec![refer("b1", "w3", &v)]);
        assert_eq!(screens.lent(), 0, "nobody streams: nothing kept");
        let (id, _) = screens.open(caller(1, None), "b1", 1);
        screens.set(vec![
            refer("b1", "w3", &v),
            refer("b2", "w3", &viewer(10, 2)),
        ]);
        assert!(
            screens.admits(id) && screens.lent() == 1,
            "only the streamed one"
        );
        screens.close(id);
        assert_eq!((screens.len(), screens.lent()), (0, 0));
    }

    #[test]
    fn set_ends_the_streams_whose_terminal_left_or_moved_out_of_sight() {
        let mut screens = Screens::default();
        let v = viewer(10, 2);
        let (limited, limited_cell) = screens.open(caller(1, Some(&["w3"])), "b1", 1);
        let (open, open_cell) = screens.open(caller(2, None), "b1", 2);
        let (pending, pending_cell) = screens.open(caller(2, None), "n0te", 3);
        screens.set(vec![refer("b1", "w3", &v), refer("n0te", "w3", &v)]);
        assert!(screens.admits(limited) && screens.admits(open) && screens.admits(pending));
        assert!(!limited_cell.ended() && !open_cell.ended() && !pending_cell.ended());
        // b1 moves to w4: the stream of the w3-limited caller ends, the other stays.
        screens.set(vec![refer("b1", "w4", &v), refer("n0te", "w3", &v)]);
        assert!(limited_cell.ended() && !screens.admits(limited));
        assert!(!open_cell.ended() && screens.admits(open));
        assert_eq!(screens.len(), 2);
        // n0te is not lent any more.
        screens.set(vec![refer("b1", "w4", &v)]);
        assert!(pending_cell.ended() && !open_cell.ended());
        screens.set(vec![]);
        assert!(open_cell.ended());
        assert_eq!((screens.len(), screens.lent()), (0, 0));
    }

    #[test]
    fn a_cut_ends_the_streams_of_the_callers_it_names() {
        let mut screens = Screens::default();
        let (a, a_cell) = screens.open(caller(1, None), "b1", 1);
        let (b, b_cell) = screens.open(caller(2, None), "b1", 2);
        screens.cut(|caller| caller.token == 1);
        assert!(a_cell.ended() && !b_cell.ended());
        assert_eq!(screens.len(), 1);
        let _ = (a, b);
        assert!(screens.on_conn(2) && !screens.on_conn(1));
    }

    #[test]
    fn a_stream_behind_gets_the_newest_whole_frame() {
        let (screens, ids) = (Mutex::new(Screens::default()), Ids::new());
        let v = viewer(20, 3);
        let (_, cell) = screens.lock().unwrap().open(caller(1, None), "b1", 1);
        screens.lock().unwrap().set(vec![refer("b1", "w3", &v)]);
        look(&screens, &ids);
        assert_eq!(cell.queued(), 1, "the first frame");
        assert!(!cell.behind());
        // The first frame and 19 more fill the queue; the next one empties it: behind.
        for i in 0..FRAMES_QUEUED {
            assert!(cell.queued() <= FRAMES_QUEUED);
            v.feed(format!("line {i}\r\n").as_bytes());
            look(&screens, &ids);
        }
        assert!(cell.behind() && cell.queued() == 0, "nobody took a frame");
        v.feed(b"last");
        look(&screens, &ids);
        assert_eq!(cell.queued(), 1, "one frame, not a backlog");
        let frame = decode(cell.take().unwrap());
        assert!(frame.whole);
        assert!(
            texts(&frame).iter().any(|row| row == "last"),
            "{:?}",
            texts(&frame)
        );
        assert!(!cell.behind());
    }

    #[test]
    fn an_unchanged_terminal_is_not_read_again() {
        let (screens, ids) = (Mutex::new(Screens::default()), Ids::new());
        let v = viewer(10, 2);
        v.feed(b"hi");
        let (_, cell) = screens.lock().unwrap().open(caller(1, None), "b1", 1);
        screens.lock().unwrap().set(vec![refer("b1", "w3", &v)]);
        look(&screens, &ids);
        assert!(decode(cell.take().unwrap()).whole);
        look(&screens, &ids);
        assert_eq!(cell.queued(), 0);
        v.feed(b"\x1b]2;t\x07");
        look(&screens, &ids);
        let frame = decode(cell.take().unwrap());
        assert!(!frame.whole && frame.rows.is_empty());
        assert_eq!(frame.title.as_deref(), Some("t"));
    }

    #[test]
    fn the_same_terminal_lent_again_keeps_what_was_read_of_it() {
        let (screens, ids) = (Mutex::new(Screens::default()), Ids::new());
        let v = viewer(10, 2);
        v.feed(b"hi");
        let (_, cell) = screens.lock().unwrap().open(caller(1, None), "b1", 1);
        screens.lock().unwrap().set(vec![refer("b1", "w3", &v)]);
        look(&screens, &ids);
        assert!(decode(cell.take().unwrap()).whole);
        // Each tick lends the terminals again: an unchanged one must not be sent whole again.
        screens.lock().unwrap().set(vec![refer("b1", "w3", &v)]);
        look(&screens, &ids);
        assert_eq!(cell.queued(), 0);
    }

    #[test]
    fn a_second_stream_on_a_quiet_terminal_still_gets_its_whole_frame() {
        let (screens, ids) = (Mutex::new(Screens::default()), Ids::new());
        let v = viewer(10, 2);
        v.feed(b"hi");
        let (_, first) = screens.lock().unwrap().open(caller(1, None), "b1", 1);
        screens.lock().unwrap().set(vec![refer("b1", "w3", &v)]);
        look(&screens, &ids);
        let (_, second) = screens.lock().unwrap().open(caller(1, None), "b1", 2);
        look(&screens, &ids);
        assert!(decode(first.take().unwrap()).whole);
        assert_eq!(first.queued(), 0, "the first stream heard nothing new");
        assert!(decode(second.take().unwrap()).whole);
    }

    #[test]
    fn a_replaced_terminal_is_read_anew() {
        let (screens, ids) = (Mutex::new(Screens::default()), Ids::new());
        let (a, b) = (viewer(10, 2), viewer(10, 2));
        a.feed(b"one");
        b.feed(b"two");
        let (_, cell) = screens.lock().unwrap().open(caller(1, None), "b1", 1);
        screens.lock().unwrap().set(vec![refer("b1", "w3", &a)]);
        look(&screens, &ids);
        let _ = cell.take();
        screens.lock().unwrap().set(vec![refer("b1", "w3", &b)]);
        look(&screens, &ids);
        let frame = decode(cell.take().unwrap());
        assert_eq!(texts(&frame)[0], "two");
    }

    fn plain(text: &str) -> RunView {
        RunView {
            text: text.into(),
            ..RunView::default()
        }
    }

    #[test]
    fn modes_ride_on_every_frame_and_a_change_alone_sends_one() {
        let v = viewer(10, 3);
        let a = screen(&v);
        v.feed(b"\x1b[?1000h");
        let b = screen(&v);
        let sent = frame(Some(&a), &b).expect("a mode alone is a change");
        assert!(!sent.whole && sent.rows.is_empty());
        assert!(sent.modes.mouse && !sent.modes.full_screen);
        assert_eq!(
            serde_json::to_value(&sent).unwrap()["modes"],
            json!({"mouse": true})
        );
        assert_eq!(serde_json::to_value(whole(&a)).unwrap()["modes"], json!({}));
    }

    #[test]
    fn every_motion_rides_on_frames_as_mouse_moves() {
        let v = viewer(10, 3);
        let a = screen(&v);
        v.feed(b"\x1b[?1003h");
        let sent = frame(Some(&a), &screen(&v)).expect("a mode alone is a change");
        assert_eq!(
            serde_json::to_value(&sent).unwrap()["modes"],
            json!({"mouse": true, "mouse_moves": true})
        );
    }

    #[test]
    fn a_first_frame_is_whole() {
        let v = viewer(10, 3);
        v.feed(b"hi");
        let a = screen(&v);
        assert_eq!(
            frame(None, &a),
            Some(FrameView {
                size: SizeView { cols: 10, rows: 3 },
                cursor: Some(CursorView {
                    row: 0,
                    col: 2,
                    shape: "block".into(),
                    blinking: false
                }),
                title: None,
                whole: true,
                rows: vec![
                    RowView {
                        y: 0,
                        runs: vec![plain("hi")]
                    },
                    RowView { y: 1, runs: vec![] },
                    RowView { y: 2, runs: vec![] },
                ],
                modes: ModesView::default(),
            })
        );
        assert_eq!(whole(&a), frame(None, &a).unwrap());
    }

    #[test]
    fn then_only_the_rows_that_changed() {
        let v = viewer(10, 3);
        v.feed(b"hi");
        let a = screen(&v);
        v.feed(b"\r\n\x1b[31mred");
        let b = screen(&v);
        let f = frame(Some(&a), &b).unwrap();
        assert!(!f.whole);
        assert_eq!(
            f.rows,
            vec![RowView {
                y: 1,
                runs: vec![RunView {
                    fg: Some(Color::Index(1)),
                    ..plain("red")
                }]
            }]
        );
        let cursor = f.cursor.unwrap();
        assert_eq!((cursor.row, cursor.col), (1, 3));
        assert_eq!(frame(Some(&b), &b), None);
    }

    #[test]
    fn a_resize_is_whole_and_a_cursor_move_alone_has_no_rows() {
        let (a, b) = (viewer(10, 3), viewer(12, 3));
        a.feed(b"hi");
        b.feed(b"hi");
        assert!(frame(Some(&screen(&a)), &screen(&b)).unwrap().whole);
        let before = screen(&a);
        a.feed(b"\x1b[1;1H");
        let f = frame(Some(&before), &screen(&a)).unwrap();
        assert!(!f.whole && f.rows.is_empty());
    }

    #[test]
    fn a_run_serializes_only_what_it_has() {
        let run = RunView {
            fg: Some(Color::Index(1)),
            bg: Some(Color::Rgb("#010203".into())),
            bold: true,
            ..plain("x")
        };
        let json = serde_json::to_value(&run).unwrap();
        assert_eq!(
            json,
            json!({"text": "x", "fg": 1, "bg": "#010203", "bold": true})
        );
        assert_eq!(serde_json::from_value::<RunView>(json).unwrap(), run);
        assert_eq!(
            serde_json::from_value::<RunView>(json!({"text": "y"})).unwrap(),
            plain("y")
        );
    }

    #[test]
    fn a_color_the_program_set_is_rgb_text() {
        let v = viewer(10, 1);
        v.feed(b"\x1b[38;2;255;0;16mX");
        let f = whole(&screen(&v));
        assert_eq!(
            f.rows[0].runs[0].fg,
            Some(Color::Rgb("#ff0010".to_string()))
        );
    }
}
