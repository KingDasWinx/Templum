//! Tests of the workspace, and the helpers they share.

use gpui_kit::test::TestWindowExt;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Context, Entity, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, PlatformInput, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, Window, WindowBounds, WindowOptions, point, px, size,
};
use templum_canvas::{BlockId, Camera, Point, Rect, chrome};

use crate::config::Edit;

use super::{Workspace, bind_keys, told_so_far};

#[cfg(target_os = "macos")]
const NEW: &str = "cmd-t";
#[cfg(not(target_os = "macos"))]
const NEW: &str = "ctrl-shift-t";
#[cfg(target_os = "macos")]
const CLOSE: &str = "cmd-w";
#[cfg(not(target_os = "macos"))]
const CLOSE: &str = "ctrl-shift-w";

/// A long-lived command that prints nothing, so tests don't depend on shell output.
fn quiet() -> (String, Vec<String>) {
    #[cfg(unix)]
    return ("cat".into(), vec![]);
    #[cfg(windows)]
    return ("cmd".into(), vec!["/K".into()]);
}

/// `script` run by `sh` with job control: each program it runs is in front of it while it runs,
/// as an agent is in front of the shell it was typed into. `quiet`'s `cat` is the shell itself.
#[cfg(unix)]
fn jobs(script: &str) -> (String, Vec<String>) {
    ("sh".into(), vec!["-c".into(), format!("set -m; {script}")])
}

/// `quiet`, run as an agent is: in front of its shell (Unix; Windows can't tell who is in front).
fn agent_cat() -> (String, Vec<String>) {
    // `; true`: `sh` would run a last `cat` in its own place, as the shell.
    #[cfg(unix)]
    return jobs("cat; true");
    #[cfg(windows)]
    return quiet();
}

/// Waits until a program terminal `id`'s shell runs is in front of it (`job`), or the shell is.
#[cfg(unix)]
fn in_front(ws: &Entity<Workspace>, cx: &mut TestAppContext, id: BlockId, job: bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        let p = ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.processes());
        if (p.foreground != p.shell) == job {
            return;
        }
        assert!(std::time::Instant::now() < deadline, "{p:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn open(cx: &mut TestAppContext) -> (AnyWindowHandle, Entity<Workspace>) {
    open_with(cx, None)
}

/// Opens a workspace whose config folder is `config_dir` (None: no config, no user themes).
fn open_with(
    cx: &mut TestAppContext,
    config_dir: Option<std::path::PathBuf>,
) -> (AnyWindowHandle, Entity<Workspace>) {
    open_in(cx, config_dir, None)
}

/// Opens a workspace with a config folder and a data folder (None: nothing read or saved).
fn open_in(
    cx: &mut TestAppContext,
    config_dir: Option<std::path::PathBuf>,
    data_dir: Option<std::path::PathBuf>,
) -> (AnyWindowHandle, Entity<Workspace>) {
    open_as(cx, Some(quiet()), config_dir, data_dir)
}

/// `open_in` with the program new terminals run.
fn open_as(
    cx: &mut TestAppContext,
    command: Option<(String, Vec<String>)>,
    config_dir: Option<std::path::PathBuf>,
    data_dir: Option<std::path::PathBuf>,
) -> (AnyWindowHandle, Entity<Workspace>) {
    let (handle, ws) = open_welcomed(cx, command, config_dir, data_dir);
    // An empty data folder is a first run: the welcome, then the first terminal; a saved one
    // opens on the Home. The tests of everything else start on the canvas they leave.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.close_welcome(window, cx);
            ws.welcome = None;
            if ws.home_up() {
                let held = ws.active;
                ws.switch_to(held, window, cx);
            }
        })
    })
    .unwrap();
    ws.update(cx, |ws, _| ws.space.appearing.clear());
    (handle, ws)
}

/// `open_as`, with the welcome of a first run, or the Home of a later start, still open.
fn open_welcomed(
    cx: &mut TestAppContext,
    command: Option<(String, Vec<String>)>,
    config_dir: Option<std::path::PathBuf>,
    data_dir: Option<std::path::PathBuf>,
) -> (AnyWindowHandle, Entity<Workspace>) {
    // PTY threads wake the UI from outside the test scheduler.
    cx.executor().allow_parking();
    let opened = cx.update(|cx| {
        gpui_kit::init(cx);
        bind_keys(cx);
        let bounds = Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.)));
        gpui_kit::open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            cx,
            |window, cx| {
                cx.new(|cx| Workspace::with(command, None, config_dir, data_dir, window, cx))
            },
        )
        .expect("open window")
    });
    // The startup block at rest: tests that measure elements must not depend on how far its
    // pop-in got (new_terminals_pop_in_then_settle checks the animation itself).
    opened.1.update(cx, |ws, _| ws.space.appearing.clear());
    OPENED.with(|ws| *ws.borrow_mut() = Some(opened.1.downgrade()));
    opened
}

thread_local! {
    /// The workspace of the window this test opened last (each test runs on a thread of its own),
    /// for helpers that are given the window alone; weak, as the test's own handle keeps it alive.
    static OPENED: std::cell::RefCell<Option<gpui_kit::WeakEntity<Workspace>>> = const { std::cell::RefCell::new(None) };
}

/// The inbox, opened the way the person does (it never opens by itself), if it isn't.
fn inbox_open(window: &mut Window, cx: &mut gpui_kit::App) {
    let ws = OPENED
        .with(|ws| ws.borrow().as_ref().and_then(|ws| ws.upgrade()))
        .expect("a window was opened");
    ws.update(cx, |ws, cx| {
        if !ws.inbox_open {
            ws.open_inbox(None, window, cx)
        }
    });
}

fn click(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32), count: usize) {
    let position = point(px(at.0), px(at.1));
    let modifiers = Modifiers::default();
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: count,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: count,
        }),
        cx,
    );
}

fn right_click(window: &mut Window, cx: &mut gpui_kit::App, at: (f32, f32)) {
    let position = point(px(at.0), px(at.1));
    let modifiers = Modifiers::default();
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button: MouseButton::Right,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button: MouseButton::Right,
            position,
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

/// Clicks item `index` of the innermost open menu. A submenu inside a submenu has the ids of the
/// one above it (`submenu`, `popup-menu`), which `within` can't tell apart; the registered
/// elements can: the innermost menu is the one under the most `popup-menu`s.
fn click_innermost_menu_item(window: &mut Window, cx: &mut gpui_kit::App, index: usize) {
    use gpui_kit::ElementId;
    let menu = ElementId::from("popup-menu");
    let shots = gpui_kit::base::test_support::snapshots(window);
    let depth = |path: &[ElementId]| path.iter().filter(|id| **id == menu).count();
    let deepest = shots
        .iter()
        .map(|s| s.path())
        .max_by_key(|p| depth(p))
        .unwrap();
    let scope = &deepest[..=deepest.iter().rposition(|id| *id == menu).unwrap()];
    let item = shots
        .iter()
        .find(|s| s.path().starts_with(scope) && s.path().last() == Some(&ElementId::from(index)))
        .unwrap_or_else(|| panic!("no item {index} in the innermost menu {scope:?}"));
    let center = item.bounds().center();
    click(window, cx, (f32::from(center.x), f32::from(center.y)), 1);
}

/// Holds or lets go of the hand key (Alt).
fn hold_alt(window: &mut Window, cx: &mut gpui_kit::App, held: bool) {
    window.dispatch_event(
        PlatformInput::ModifiersChanged(gpui_kit::ModifiersChangedEvent {
            modifiers: Modifiers {
                alt: held,
                ..Default::default()
            },
            capslock: Default::default(),
        }),
        cx,
    );
    window.render_frame(cx);
}

fn press_with(
    window: &mut Window,
    cx: &mut gpui_kit::App,
    button: MouseButton,
    modifiers: Modifiers,
    from: (f32, f32),
    to: (f32, f32),
) {
    let at = |p: (f32, f32)| point(px(p.0), px(p.1));
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position: at(from),
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
            position: at(to),
            pressed_button: Some(button),
            modifiers,
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position: at(to),
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

fn wheel(
    window: &mut Window,
    cx: &mut gpui_kit::App,
    at: (f32, f32),
    delta: ScrollDelta,
    control: bool,
) {
    window.dispatch_event(
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position: point(px(at.0), px(at.1)),
            delta,
            modifiers: Modifiers {
                control,
                ..Default::default()
            },
            touch_phase: TouchPhase::Moved,
        }),
        cx,
    );
}

/// Restarts the camera's glide from now, so a test's next frame draws it on the way.
fn restart_glide(ws: &mut Workspace, _: &mut Context<Workspace>) {
    if let Some((_, since)) = &mut ws.space.flight {
        *since = std::time::Instant::now();
    }
}

/// The running glide as if it began 40 ms ago, so the next frame draws it mid-flight. Not a
/// sleep: a slow machine (macOS CI) can oversleep past the whole 150 ms glide.
fn glide_40ms_in(ws: &mut Workspace, _: &mut Context<Workspace>) {
    if let Some((_, since)) = &mut ws.space.flight {
        *since = std::time::Instant::now() - std::time::Duration::from_millis(40);
    }
}

fn first_id(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> templum_canvas::BlockId {
    ws.read_with(cx, |ws, _| ws.space.canvas.top().unwrap())
}

#[cfg(target_os = "macos")]
const ZOOM_IN: &str = "cmd-=";
#[cfg(not(target_os = "macos"))]
const ZOOM_IN: &str = "ctrl-=";

/// Presses `key`, then types "x" + Enter into `cat`, and returns the screen once "x" shows.
#[cfg(unix)]
fn echo_after(
    key: &str,
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> String {
    use std::time::{Duration, Instant};
    use templum_terminal::Snapshot;
    let id = first_id(ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.press(key, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        ws.space.terminal(id).unwrap().term.write(b"x\r".to_vec())
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let shown = ws.read_with(cx, |ws, _| {
            let mut snap = Snapshot::default();
            snap.refresh(
                ws.space.terminal(id).unwrap().term.shared(),
                &ws.theme.palette,
            );
            snap.text()
        });
        if shown.contains('x') {
            return shown;
        }
        assert!(Instant::now() < deadline, "no echo: {shown:?}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn builtin_index(name: &str) -> usize {
    crate::theme::BUILTIN
        .iter()
        .position(|b| b.name == name)
        .unwrap()
}

fn write_config(dir: &std::path::Path, text: &str) {
    std::fs::write(dir.join("config.toml"), text).unwrap();
}

fn reload(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.reload_config(window, cx))
    })
    .unwrap();
}

fn notification_count(handle: AnyWindowHandle, cx: &mut TestAppContext) -> usize {
    use gpui_kit::component::WindowExt as _;
    cx.update_window(handle, |_, window, cx| window.notifications(cx).len())
        .unwrap()
}

#[cfg(target_os = "macos")]
const PICKER: &str = "cmd-shift-p";
#[cfg(not(target_os = "macos"))]
const PICKER: &str = "ctrl-shift-p";

fn frame(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
}

#[cfg(target_os = "macos")]
const SETTINGS: &str = "cmd-,";
#[cfg(not(target_os = "macos"))]
const SETTINGS: &str = "ctrl-,";

fn panel_open(ws: &Entity<Workspace>, cx: &mut TestAppContext) -> bool {
    ws.read_with(cx, |ws, _| ws.settings.as_ref().is_some_and(|p| !p.closing))
}

/// Lets the panel's slide finish (it runs on the real clock, like the block animations).
fn settle_slide(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    std::thread::sleep(crate::motion::SLIDE + std::time::Duration::from_millis(50));
    frame(handle, cx);
}

/// Gives the workspace a start-at-login entry in `home`, a temporary folder standing for the
/// person's home: tests never reach the system's own.
fn with_autostart(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    home: &std::path::Path,
) -> crate::autostart::Autostart {
    let entry = crate::autostart::Autostart::in_dir("linux", home, "/opt/templum", false);
    ws.update(cx, |ws, _| ws.autostart.entry = Some(entry.clone()));
    entry
}

fn write_canvas(dir: &std::path::Path, text: &str) {
    std::fs::create_dir_all(dir.join("workspaces")).unwrap();
    std::fs::write(crate::session::path(dir, crate::session::DEFAULT_ID), text).unwrap();
}

/// Waits for a terminal's shell to report its folder (canonicalized).
fn folder_of(ws: &Entity<Workspace>, id: BlockId, cx: &mut TestAppContext) -> std::path::PathBuf {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(cwd) = ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.cwd()) {
            return cwd.canonicalize().unwrap();
        }
        assert!(Instant::now() < deadline, "no cwd");
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// An editor block on `folder`, near the canvas's top-left, read and at rest.
fn open_editor(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    folder: &std::path::Path,
) -> BlockId {
    cx.update_window(handle, |_, window, cx| {
        let id = ws.update(cx, |ws, cx| {
            let id = ws
                .add_editor_at(folder, Point::new(40.0, 40.0), window, cx)
                .unwrap();
            ws.space.appearing.clear();
            id
        });
        window.render_frame(cx);
        id
    })
    .unwrap()
}

/// Clicks the tree's row `ix` (a file opens, a folder expands).
fn click_file(handle: AnyWindowHandle, ix: usize, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        window.click(("entry", ix), cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Dialogs slide in over 250 ms of real time: a click mid-slide can miss its button.
fn no_motion(cx: &mut TestAppContext) {
    cx.update(|cx| cx.set_reduce_motion(true));
}

/// Runs `git args` in `dir`, panicking with git's stderr on failure; returns stdout.
fn git_run(dir: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// A repository on `main` in a temporary folder, with an author so commits work.
fn git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git_run(dir.path(), &["init", "-q"]);
    git_run(dir.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git_run(dir.path(), &["config", "user.name", "Ada"]);
    git_run(dir.path(), &["config", "user.email", "ada@example.com"]);
    git_run(dir.path(), &["config", "commit.gpgsign", "false"]);
    // Files as written: a runner's core.autocrlf would turn restored files into CRLF.
    git_run(dir.path(), &["config", "core.autocrlf", "false"]);
    dir
}

/// Lets the background git jobs finish, draws a frame, and lets what the frame started finish.
fn settle(handle: AnyWindowHandle, cx: &mut TestAppContext) {
    cx.run_until_parked();
    frame(handle, cx);
    cx.run_until_parked();
    frame(handle, cx);
}

/// Waits for the Git's next status read (a change made behind its back).
fn settle_read(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.refresh_git(window, cx))
    })
    .unwrap();
    settle(handle, cx);
}

/// The Git popup open on `folder`, looked up and read.
fn open_git(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    folder: &std::path::Path,
) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            assert!(ws.set_git_folder(folder, window, cx));
            ws.open_git(window, cx);
        })
    })
    .unwrap();
    settle(handle, cx);
}

fn git_of<T>(
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    read: impl FnOnce(&crate::git_block::Git) -> T,
) -> T {
    ws.read_with(cx, |ws, _| read(ws.space.git().unwrap()))
}

/// A repository with one commit holding a.txt ("one\n").
fn committed_repo() -> tempfile::TempDir {
    let repo = git_repo();
    std::fs::write(repo.path().join("a.txt"), "one\n").unwrap();
    git_run(repo.path(), &["add", "a.txt"]);
    git_run(repo.path(), &["commit", "-q", "-m", "First"]);
    repo
}

/// a.txt of 30 lines committed, then lines 2, 15 and 28 changed: three hunks.
fn three_hunk_repo() -> tempfile::TempDir {
    let repo = git_repo();
    let lines: Vec<String> = (1..=30).map(|i| format!("line {i}\n")).collect();
    std::fs::write(repo.path().join("a.txt"), lines.concat()).unwrap();
    git_run(repo.path(), &["add", "a.txt"]);
    git_run(repo.path(), &["commit", "-q", "-m", "a"]);
    let mut changed = lines;
    for i in [1, 14, 27] {
        changed[i] = format!("LINE {}\n", i + 1);
    }
    std::fs::write(repo.path().join("a.txt"), changed.concat()).unwrap();
    repo
}

/// Hovers a row (its buttons show) and clicks one of them.
fn click_row_button(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    row: gpui_kit::ElementId,
    button: gpui_kit::ElementId,
) {
    cx.update_window(handle, |_, window, cx| {
        window.hover(row, cx);
        window.render_frame(cx);
        window.click(button, cx);
    })
    .unwrap();
    settle(handle, cx);
}

fn git_button(what: &str, list: &str, i: usize) -> gpui_kit::ElementId {
    gpui_kit::ElementId::Name(format!("git-{what}-{list}-{i}").into())
}

/// A bare remote `origin` for `repo`, with its `main` pushed.
fn add_remote(repo: &std::path::Path) -> tempfile::TempDir {
    let remote = tempfile::tempdir().unwrap();
    git_run(remote.path(), &["init", "-q", "--bare"]);
    // Clones check out its HEAD: main, not git's default branch.
    git_run(remote.path(), &["symbolic-ref", "HEAD", "refs/heads/main"]);
    git_run(
        repo,
        &[
            "remote",
            "add",
            "origin",
            &remote.path().display().to_string(),
        ],
    );
    git_run(repo, &["push", "-q", "-u", "origin", "main"]);
    remote
}

#[cfg(target_os = "macos")]
pub(super) const COPY: &str = "cmd-c";
#[cfg(not(target_os = "macos"))]
pub(super) const COPY: &str = "ctrl-c";
#[cfg(target_os = "macos")]
pub(super) const PASTE: &str = "cmd-v";
#[cfg(not(target_os = "macos"))]
pub(super) const PASTE: &str = "ctrl-v";
#[cfg(target_os = "macos")]
pub(super) const UNDO: &str = "cmd-z";
#[cfg(not(target_os = "macos"))]
pub(super) const UNDO: &str = "ctrl-z";
#[cfg(target_os = "macos")]
pub(super) const REDO: &str = "cmd-shift-z";
#[cfg(not(target_os = "macos"))]
pub(super) const REDO: &str = "ctrl-shift-z";

pub(super) const SHIFT: Modifiers = Modifiers {
    shift: true,
    control: false,
    alt: false,
    platform: false,
    function: false,
};

/// The startup terminal moved down, a kanban block and a note ("hello") beside each other above
/// it, the camera at 100% on the world's origin: (kanban, note, terminal).
pub(super) fn three_blocks(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (BlockId, BlockId, BlockId) {
    let terminal = first_id(ws, cx);
    cx.update_window(handle, |_, window, cx| {
        let ids = ws.update(cx, |ws, cx| {
            // Touched: the startup terminal no longer pulls the camera to it.
            ws.space.pristine = false;
            ws.space.canvas.camera = Camera::at(Point::new(0.0, 0.0), 1.0);
            ws.space.canvas.get_mut(terminal).unwrap().rect = Rect::new(40.0, 400.0, 400.0, 300.0);
            let kanban = crate::session::Item::Kanban(crate::session::KanbanItem {
                uid: String::new(),
                rect: Rect::new(40.0, 40.0, 220.0, 200.0).into(),
                board: None,
            });
            let hello = ws.notes.add("hello".into(), None, 0);
            let note = crate::session::Item::Note(crate::session::NoteItem {
                uid: String::new(),
                rect: Rect::new(300.0, 40.0, 220.0, 200.0).into(),
                notes: vec![hello],
                active: 0,
                text: String::new(),
                color: None,
            });
            // Set up, not a change of the person's to take back.
            ws.quiet = true;
            let kanban = ws.add_saved(&kanban, window, cx).unwrap();
            let note = ws.add_saved(&note, window, cx).unwrap();
            ws.quiet = false;
            ws.space.appearing.clear();
            (kanban, note, terminal)
        });
        window.render_frame(cx);
        ids
    })
    .unwrap()
}

/// A checklist block "name" with `items` (a leading `[x] ` checks one) in the shown workspace:
/// (block, checklist).
pub(super) fn checklist_block(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    name: &str,
    items: &[&str],
) -> (BlockId, u64) {
    let items = crate::checklists::parse_items(&items.join("\n"));
    let block = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                let space = ws.space.id.clone();
                let new = super::ops::NewBlock::Checklist {
                    name: name.into(),
                    items,
                };
                ws.add_block_in(&space, new, super::ops::Place::Free, None, window, cx)
            })
        })
        .unwrap()
        .unwrap();
    let list = ws.read_with(cx, |ws, _| {
        ws.space.blocks[&block].checklist().unwrap().list.unwrap()
    });
    (block, list)
}

/// A kanban block on a new board with one card in its first column: (block, board, card).
pub(super) fn board_with_a_card(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (BlockId, u64, u64) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_kanban_at(Point::new(900.0, 0.0), window, cx);
            let block = ws.space.canvas.top().unwrap();
            ws.new_board(block, cx);
            let board = ws.space.kanban(block).unwrap().board.unwrap();
            let todo = ws.boards.board(board).unwrap().columns[0].id;
            let card = ws.edit_boards(|b| b.add_card(board, todo)).unwrap();
            ws.space.appearing.clear();
            (block, board, card)
        })
    })
    .unwrap()
}

/// Presses on the element `from`, drags it onto the element `to` and lets go: no sleeps, the
/// frames the drag needs are drawn by the test window.
pub(super) fn drop_on(
    handle: AnyWindowHandle,
    cx: &mut TestAppContext,
    from: impl Into<gpui_kit::ElementId>,
    to: impl Into<gpui_kit::ElementId>,
) {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.drag_to(from, to, cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// Moves the pointer to `to` (a window position), no button held, and draws.
pub(super) fn point_at(handle: AnyWindowHandle, cx: &mut TestAppContext, to: (f32, f32)) {
    cx.update_window(handle, |_, window, cx| {
        window.dispatch_event(
            PlatformInput::MouseMove(gpui_kit::MouseMoveEvent {
                position: point(px(to.0), px(to.1)),
                pressed_button: None,
                modifiers: Modifiers::default(),
            }),
            cx,
        );
        window.render_frame(cx);
        window.render_frame(cx);
    })
    .unwrap();
}

/// A local canvas point as a window position.
pub(super) fn at(ws: &Entity<Workspace>, cx: &mut TestAppContext, p: (f32, f32)) -> (f32, f32) {
    let o = ws.read_with(cx, |ws, _| ws.origin);
    (f32::from(o.x) + p.0, f32::from(o.y) + p.1)
}

mod accounts;
mod agents;
mod agents_view;
mod api;
mod api_git;
mod attachments;
mod automations;
mod block_ids;
mod blocks;
mod board_tools;
mod bus;
mod card_draft;
mod checklists;
mod config_reload;
mod content;
mod delegate;
mod drawing;
mod editor;
mod editor_tabs;
mod git_changes;
mod git_commit;
mod git_compare;
mod git_conflicts;
mod git_history;
mod git_hunks;
mod git_keys;
mod git_layout;
mod git_message;
mod git_mirror;
mod git_panel;
mod git_split;
mod git_status;
mod git_switch;
mod git_sync;
mod git_trash;
mod grid;
mod history;
mod home;
mod inbox;
mod instructions;
mod kanban;
mod lod;
mod menus;
mod mirror_end_to_end;
mod moving;
mod navigation;
mod notes;
#[cfg(unix)]
mod notices;
mod persistence;
mod popups;
mod remote;
mod remote_boards;
mod remote_content;
mod remote_notes;
mod remote_rules;
mod requests;
mod scrollback;
mod selection;
mod settings_edits;
mod settings_panel;
mod shortcuts;
// `cat` stands in for the agents.
#[cfg(unix)]
mod schedules;
#[cfg(unix)]
mod spawn;
mod subagents;
mod templum_agent;
mod terminals;
mod themes;
mod update;
mod usage;
mod welcome;
mod wires;
mod workspaces;
