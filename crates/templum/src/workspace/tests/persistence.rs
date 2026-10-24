//! Saving the canvas and bringing it back.

use super::*;
use crate::workspace::{MAX_COLS, MAX_ROWS};

#[gpui_kit::test]
fn a_saved_canvas_comes_back(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    // JSON strings: Windows backslashes doubled.
    let json = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    write_canvas(
        data.path(),
        &format!(
            r#"{{"version":1,"camera":{{"x":-100,"y":50,"zoom":1.1}},"focused":1,"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{}"}},
                  {{"kind":"terminal","rect":{{"x":800,"y":20,"w":600,"h":400}},"folder":"{}"}}]}}"#,
            json(a.path()),
            json(b.path())
        ),
    );
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let (ids, rects, camera, focused) = ws.read_with(cx, |ws, _| {
        let blocks = ws.space.canvas.blocks();
        (
            blocks.iter().map(|b| b.id).collect::<Vec<_>>(),
            blocks.iter().map(|b| b.rect).collect::<Vec<_>>(),
            ws.space.canvas.camera,
            ws.space.focused,
        )
    });
    assert_eq!(
        rects,
        [
            Rect::new(0.0, 0.0, 700.0, 450.0),
            Rect::new(800.0, 20.0, 600.0, 400.0)
        ]
    );
    assert_eq!(camera.origin, Point::new(-100.0, 50.0));
    assert_eq!(camera.zoom(), 1.1);
    assert_eq!(focused, Some(ids[1]));
    assert_eq!(folder_of(&ws, ids[0], cx), a.path().canonicalize().unwrap());
    assert_eq!(folder_of(&ws, ids[1], cx), b.path().canonicalize().unwrap());
}

#[gpui_kit::test]
fn a_saved_canvas_with_no_blocks_opens_empty(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    write_canvas(
        data.path(),
        r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#,
    );
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.canvas.blocks().is_empty());
        assert_eq!(ws.space.focused, None);
    });
}

#[gpui_kit::test]
fn a_folder_thats_gone_starts_at_home_and_says_so(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let gone = data
        .path()
        .join("gone")
        .display()
        .to_string()
        .replace('\\', "\\\\");
    write_canvas(
        data.path(),
        &format!(
            r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{gone}"}}]}}"#
        ),
    );
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    assert_eq!(notification_count(handle, cx), 1);
    let id = first_id(&ws, cx);
    let home = std::env::home_dir().unwrap().canonicalize().unwrap();
    assert_eq!(folder_of(&ws, id, cx), home);
}

#[gpui_kit::test]
fn a_broken_canvas_file_is_set_aside(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    write_canvas(data.path(), "{nope");
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    assert_eq!(notification_count(handle, cx), 1);
    let bak =
        crate::session::path(data.path(), crate::session::DEFAULT_ID).with_extension("json.bak");
    assert_eq!(std::fs::read_to_string(bak).unwrap(), "{nope");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.canvas.blocks().len(), 1, "as on a first run")
    });
}

#[gpui_kit::test]
fn absurd_numbers_in_the_saved_canvas_are_made_safe(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    write_canvas(
        data.path(),
        r#"{"version":1,"camera":{"x":1e40,"y":0,"zoom":1e40},"focused":7,"items":[
              {"kind":"terminal","rect":{"x":1e40,"y":0,"w":700,"h":450}},
              {"kind":"terminal","rect":{"x":10,"y":10,"w":-5,"h":1e30}}]}"#,
    );
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.read_with(cx, |ws, _| {
        let blocks = ws.space.canvas.blocks();
        assert_eq!(blocks.len(), 1, "the infinite one is skipped");
        let min = chrome::min_block_size(ws.cell.size);
        let max = chrome::block_size(MAX_COLS, MAX_ROWS, ws.cell.size);
        assert_eq!(blocks[0].rect, Rect::new(10.0, 10.0, min.w, max.h));
        assert_eq!(ws.space.focused, None, "index 7 doesn't exist");
        assert_eq!(
            ws.space.canvas.camera,
            Camera::default(),
            "an infinite camera isn't used"
        );
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_unreadable_canvas_file_is_never_written_over(cx: &mut TestAppContext) {
    use std::os::unix::fs::PermissionsExt as _;
    let data = tempfile::tempdir().unwrap();
    write_canvas(
        data.path(),
        r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#,
    );
    let path = crate::session::path(data.path(), crate::session::DEFAULT_ID);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    assert_eq!(notification_count(handle, cx), 1);
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.frozen, "its canvas won't be saved");
        assert_eq!(ws.space.canvas.blocks().len(), 1, "as on a first run");
    });
    // The rest still saves: the boards, a new workspace.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.boards.add_board();
            ws.new_space(window, cx);
        })
    })
    .unwrap();
    tick(cx);
    assert!(crate::board::path(data.path()).exists(), "boards saved");
    assert!(crate::session::path(data.path(), "w2").exists(), "w2 saved");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#,
        "never written over"
    );
}

/// One snapshot period, with the background read-and-write finished.
fn tick(cx: &mut TestAppContext) {
    cx.executor().advance_clock(crate::session::EVERY);
    cx.run_until_parked();
}

#[gpui_kit::test]
fn the_canvas_comes_back_as_it_was_left(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open(Rect::new(900.0, 40.0, 600.0, 400.0), window, cx);
            let first = ws.space.canvas.blocks()[0].id;
            ws.space.canvas.get_mut(first).unwrap().rect = Rect::new(-50.0, 60.0, 700.0, 450.0);
            ws.focus_block(first, window, cx);
            ws.space.canvas.camera = Camera::at(Point::new(-40.0, 25.0), 1.25);
            ws.save_canvas_now(cx);
        })
    })
    .unwrap();
    let before = ws.read_with(cx, |ws, _| {
        let blocks = ws.space.canvas.blocks();
        let focused = blocks.iter().position(|b| Some(b.id) == ws.space.focused);
        (blocks.iter().map(|b| b.rect).collect::<Vec<_>>(), focused)
    });
    let (_, again) = open_in(cx, None, Some(data.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.name, "Main",
            "no index: the canvas is the first workspace"
        );
        let blocks = ws.space.canvas.blocks();
        assert_eq!(blocks.iter().map(|b| b.rect).collect::<Vec<_>>(), before.0);
        assert_eq!(
            blocks.iter().position(|b| Some(b.id) == ws.space.focused),
            before.1
        );
        assert_eq!(ws.space.canvas.camera.origin, Point::new(-40.0, 25.0));
        assert_eq!(ws.space.canvas.camera.zoom(), 1.25);
    });
}

#[gpui_kit::test]
fn snapshots_write_only_what_changed(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let path = crate::session::path(data.path(), crate::session::DEFAULT_ID);
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    tick(cx);
    assert!(path.exists(), "the first snapshot is written");
    std::fs::remove_file(&path).unwrap();
    tick(cx);
    assert!(!path.exists(), "nothing changed: nothing written");
    ws.update(cx, |ws, _| {
        ws.space.canvas.camera.pan_by(Point::new(30.0, 0.0))
    });
    tick(cx);
    assert!(path.exists(), "a pan is a change");
}

#[gpui_kit::test]
fn a_failing_canvas_write_says_so_once_and_recovers(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    // A file where the workspaces folder should be: every write fails.
    let blocker = data.path().join("workspaces");
    std::fs::write(&blocker, "").unwrap();
    let pan = |cx: &mut TestAppContext| {
        ws.update(cx, |ws, _| {
            ws.space.canvas.camera.pan_by(Point::new(10.0, 0.0))
        })
    };
    pan(cx);
    tick(cx);
    assert_eq!(notification_count(handle, cx), 1, "said once");
    // Still failing: retried every period without saying so again (the first hides after 5 s).
    for _ in 0..3 {
        pan(cx);
        tick(cx);
    }
    assert_eq!(notification_count(handle, cx), 0, "not again");
    std::fs::remove_file(&blocker).unwrap();
    tick(cx);
    assert!(
        crate::session::path(data.path(), crate::session::DEFAULT_ID).exists(),
        "retried"
    );
    assert_eq!(notification_count(handle, cx), 1, "and says it works again");
}

#[gpui_kit::test]
fn closing_the_window_saves_the_canvas(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.update(cx, |ws, _| {
        let first = ws.space.canvas.blocks()[0].id;
        ws.space.canvas.get_mut(first).unwrap().rect = Rect::new(-500.0, 0.0, 700.0, 450.0);
    });
    // The test's handle would keep the workspace alive.
    drop(ws);
    cx.update_window(handle, |_, window, _| window.remove_window())
        .unwrap();
    cx.run_until_parked();
    let text = std::fs::read_to_string(crate::session::path(
        data.path(),
        crate::session::DEFAULT_ID,
    ))
    .unwrap();
    assert!(text.contains("-500.0"), "{text}");
}

/// Quitting writes the canvas even when it looks unchanged: the file may hold an older one
/// written since (a snapshot still in flight).
#[gpui_kit::test]
fn quitting_writes_the_canvas_even_when_it_looks_unchanged(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let path = crate::session::path(data.path(), crate::session::DEFAULT_ID);
    let ours = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[]}"#,
    )
    .unwrap();
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), ours);
}

#[gpui_kit::test]
fn without_a_data_folder_nothing_is_kept(cx: &mut TestAppContext) {
    let (_, ws) = open(cx);
    tick(cx);
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.saved, None));
}

#[gpui_kit::test]
fn shells_that_cant_start_leave_the_saved_canvas_alone(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let text = r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[
              {"kind":"terminal","rect":{"x":0,"y":0,"w":700,"h":450}}]}"#;
    write_canvas(data.path(), text);
    // A shell setting with a typo: no block can start.
    let broken = Some(("/nonexistent/templum-shell".to_string(), Vec::new()));
    let (_, ws) = open_as(cx, broken, None, Some(data.path().to_path_buf()));
    tick(cx);
    ws.read_with(cx, |ws, _| assert!(ws.space.canvas.blocks().is_empty()));
    assert_eq!(
        std::fs::read_to_string(crate::session::path(
            data.path(),
            crate::session::DEFAULT_ID
        ))
        .unwrap(),
        text,
        "kept for the start after the setting is fixed"
    );
}

/// A restored shell that ends as it starts couldn't start, as one whose spawn fails: its block
/// stays, marked, the saved canvas is kept for the next start, and closing the block asks
/// nothing.
#[cfg(unix)]
#[gpui_kit::test]
fn a_restored_shell_that_ends_at_once_leaves_the_saved_canvas_alone(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let folder = data.path().display().to_string().replace('\\', "\\\\");
    let text = format!(
        r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
              {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{folder}",
                "agent":"claude","session":"abc-1"}}]}}"#
    );
    write_canvas(data.path(), &text);
    // A shell whose profile ends it.
    let ends = Some(("sh".to_string(), vec!["-c".into(), "exit 1".into()]));
    let (handle, ws) = open_as(cx, ends, None, Some(data.path().to_path_buf()));
    let id = first_id(&ws, cx);
    super::terminals::until("the shell to end", handle, &ws, cx, |ws| {
        ws.space.terminal(id).is_none_or(|p| p.term.exited())
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    ws.read_with(cx, |ws, cx| {
        assert!(ws.space.terminal(id).is_some(), "its block stays");
        assert_eq!(
            crate::block::title(ws, &ws.space.blocks[&id], cx),
            "The shell ended"
        );
        assert!(ws.space.frozen, "its canvas won't be saved");
    });
    assert_eq!(notification_count(handle, cx), 1);
    // Its agent, typed before the shell ended, isn't running: no question.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(id, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert!(ws.space.terminal(id).is_none()));
    tick(cx);
    let path = crate::session::path(data.path(), crate::session::DEFAULT_ID);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "kept");
}

/// A key pressed into the block of a restored shell that ended as it started (Enter, an arrow,
/// an agent's send_keys) didn't end it: the block keeps showing why, and the canvas stays kept.
#[cfg(unix)]
#[gpui_kit::test]
fn a_key_pressed_after_a_restored_shell_ended_at_once_keeps_its_block(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    write_canvas(
        data.path(),
        r#"{"version":1,"camera":{"x":0,"y":0,"zoom":1},"items":[
              {"kind":"terminal","rect":{"x":0,"y":0,"w":700,"h":450}}]}"#,
    );
    let ends = Some(("sh".to_string(), vec!["-c".into(), "exit 1".into()]));
    let (handle, ws) = open_as(cx, ends, None, Some(data.path().to_path_buf()));
    let id = first_id(&ws, cx);
    super::terminals::until("the shell to end", handle, &ws, cx, |ws| {
        ws.space.terminal(id).is_none_or(|p| p.term.exited())
    });
    ws.update(cx, |ws, cx| {
        ws.space
            .terminal(id)
            .unwrap()
            .term
            .press(&templum_terminal::KeyInput {
                key: "enter",
                key_char: None,
                ctrl: false,
                alt: false,
                shift: false,
            });
        cx.notify();
    });
    cx.update_window(handle, |_, window, cx| window.render_frame(cx))
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(ws.space.terminal(id).is_some(), "its block stays");
        assert!(ws.space.frozen, "its canvas won't be saved");
    });
}

/// A restored shell that ends as it starts keeps the saved canvas also before any frame (the
/// window minimized at start): neither a snapshot nor quitting writes over it.
#[cfg(unix)]
#[gpui_kit::test]
fn a_restored_shell_that_ends_before_any_frame_leaves_the_saved_canvas_alone(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let folder = data.path().display().to_string();
    let text = format!(
        r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
              {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{folder}",
                "agent":"claude","session":"abc-1"}}]}}"#
    );
    write_canvas(data.path(), &text);
    let ends = Some(("sh".to_string(), vec!["-c".into(), "exit 1".into()]));
    let (handle, ws) = open_as(cx, ends, None, Some(data.path().to_path_buf()));
    let id = first_id(&ws, cx);
    // No frame drawn meanwhile.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !ws.read_with(cx, |ws, _| ws.space.terminal(id).unwrap().term.exited()) {
        assert!(
            std::time::Instant::now() < deadline,
            "the shell never ended"
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.snapshot(window, cx);
            assert!(ws.space.frozen, "a snapshot keeps it");
            // As if no snapshot had come first.
            ws.space.frozen = false;
            ws.save_canvas_now(cx);
            assert!(ws.space.frozen, "quitting keeps it");
        })
    })
    .unwrap();
    cx.run_until_parked();
    let path = crate::session::path(data.path(), crate::session::DEFAULT_ID);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text, "kept");
}

/// A restored shell ended by the person, at once (Ctrl+D, or `exit` with no shell integration),
/// did start: its block closes as any other's, and the workspace is saved as usual.
#[cfg(unix)]
#[gpui_kit::test]
fn a_restored_shell_the_person_ends_at_once_closes_its_block(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let folder = data.path().display().to_string();
    write_canvas(
        data.path(),
        &format!(
            r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{folder}",
                    "agent":"claude","session":"abc-1"}}]}}"#
        ),
    );
    // `cat` is the shell: Ctrl+D, after the typed agent's line, ends it as it ends a prompt.
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let id = first_id(&ws, cx);
    ws.read_with(cx, |ws, _| {
        ws.space
            .terminal(id)
            .unwrap()
            .term
            .press(&templum_terminal::KeyInput {
                key: "d",
                key_char: Some("d"),
                ctrl: true,
                alt: false,
                shift: false,
            })
    });
    super::terminals::until("its block to close", handle, &ws, cx, |ws| {
        ws.space.terminal(id).is_none()
    });
    ws.read_with(cx, |ws, _| assert!(!ws.space.frozen, "its canvas is saved"));
    assert_eq!(notification_count(handle, cx), 0);
}

/// A snapshot taken before a restored agent is up (a slow shell, a question of its that took the
/// typed line) keeps its saved folder, agent and session: the next start resumes it still. Once
/// a snapshot finds an agent there, what it finds is saved.
#[gpui_kit::test]
fn a_snapshot_before_the_restored_agent_is_up_keeps_it(cx: &mut TestAppContext) {
    use crate::session::{Item, Saved};
    let data = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let folder = work.path().display().to_string();
    let json = folder.replace('\\', "\\\\");
    write_canvas(
        data.path(),
        &format!(
            r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"folder":"{json}",
                    "agent":"claude","session":"abc-1"}}]}}"#
        ),
    );
    // `cat` is the shell: the agent typed into it never starts.
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let id = first_id(&ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.canvas.camera.pan_by(Point::new(30.0, 0.0))
    });
    tick(cx);
    let path = crate::session::path(data.path(), crate::session::DEFAULT_ID);
    let saved: Saved = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let Item::Terminal(t) = &saved.items[0] else {
        panic!("{saved:?}")
    };
    assert_eq!(saved.camera.x, -30.0, "written");
    assert_eq!(
        (
            t.folder.as_deref(),
            t.agent.as_deref(),
            t.session.as_deref()
        ),
        (Some(folder.as_str()), Some("claude"), Some("abc-1"))
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let found = templum_terminal::Inspection {
                folder: Some(work.path().to_path_buf()),
                agent: Some("claude".into()),
                pid: Some(42),
                ..Default::default()
            };
            let space = ws.space.id.clone();
            ws.snapshot_done(
                vec![(space, vec![(id, found)], saved.clone(), None)],
                None,
                None,
                None,
                None,
                window,
                cx,
            );
            assert!(ws.space.terminal(id).unwrap().resuming.is_none());
        })
    })
    .unwrap();
}

/// A snapshot's read of the processes tells each terminal its folder and its agent, with the
/// agent's process: what the menus, the Git block and the usage read.
#[gpui_kit::test]
fn a_snapshot_tells_each_terminal_its_folder_and_agent(cx: &mut TestAppContext) {
    use crate::session::{Saved, SavedCamera};
    use templum_terminal::Inspection;
    let (handle, ws) = open(cx);
    let id = first_id(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let saved = Saved {
                version: 1,
                camera: SavedCamera {
                    x: 0.0,
                    y: 0.0,
                    zoom: 1.0,
                },
                items: Vec::new(),
                focused: None,
                recent: Vec::new(),
                ink: Vec::new(),
                grid: None,
                wires: Vec::new(),
                git: None,
            };
            let space = ws.space.id.clone();
            let found = Inspection {
                folder: Some("/srv/app".into()),
                agent: Some("claude".into()),
                pid: Some(42),
                started: Some(1_700_000_000),
                session: None,
                folder_var: None,
            };
            ws.snapshot_done(
                vec![(
                    space.clone(),
                    vec![(id, found.clone())],
                    saved.clone(),
                    None,
                )],
                None,
                None,
                None,
                None,
                window,
                cx,
            );
            assert_eq!(ws.space.terminal(id).unwrap().seen, found);
            // A folder the next one couldn't read keeps the one known; the agent left.
            ws.snapshot_done(
                vec![(space, vec![(id, Inspection::default())], saved, None)],
                None,
                None,
                None,
                None,
                window,
                cx,
            );
            let seen = &ws.space.terminal(id).unwrap().seen;
            assert_eq!(
                seen.folder.as_deref(),
                Some(std::path::Path::new("/srv/app"))
            );
            assert_eq!((&seen.agent, seen.pid, seen.started), (&None, None, None));
        })
    })
    .unwrap();
}

/// A workspace frozen (its file couldn't be read) is never written, but its terminals are still
/// read: their folder and agent stay current.
#[gpui_kit::test]
fn a_frozen_workspaces_terminals_are_still_read(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (_handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let id = first_id(&ws, cx);
    ws.update(cx, |ws, _| {
        ws.space.frozen = true;
        ws.space.terminal_mut(id).unwrap().seen.agent = Some("gone".into());
    });
    tick(cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.terminal(id).unwrap().seen.agent,
            None,
            "read again"
        );
    });
    let file = crate::session::path(data.path(), crate::session::DEFAULT_ID);
    assert!(!file.exists(), "not written");
}

/// A shell that tells its `CLAUDE_CONFIG_DIR`, then echoes what is typed.
#[cfg(unix)]
fn telling_its_folder() -> (String, Vec<String>) {
    let script = "echo \"dir=${CLAUDE_CONFIG_DIR-unset};\"; exec cat";
    ("sh".to_string(), vec!["-c".to_string(), script.to_string()])
}

/// A saved terminal in `folder`, with `agent`, `session` and `account` when given.
#[cfg(unix)]
fn saved_terminal(
    folder: &std::path::Path,
    agent: Option<&str>,
    session: Option<&str>,
    account: Option<&str>,
) -> crate::session::Item {
    serde_json::from_value(serde_json::json!({
        "kind": "terminal",
        "rect": { "x": 0.0, "y": 900.0, "w": 700.0, "h": 450.0 },
        "folder": folder,
        "agent": agent,
        "session": session,
        "account": account,
    }))
    .unwrap()
}

#[cfg(unix)]
fn restore(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    item: &crate::session::Item,
) -> BlockId {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.add_saved(item, window, cx).unwrap())
    })
    .unwrap()
}

#[cfg(unix)]
#[gpui_kit::test]
fn two_blocks_one_folder_two_accounts_resume_their_own(cx: &mut TestAppContext) {
    use super::bus::screen_with;
    let _own = super::agents::claude_keeping(&["a"]);
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    // Each keeps its session in its own folder: the account's is `b`.
    let account_folder = data.path().join("accounts").join("claude2");
    std::fs::create_dir_all(account_folder.join("projects/-app")).unwrap();
    std::fs::write(account_folder.join("projects/-app/b.jsonl"), "{}").unwrap();
    let (handle, ws) = open_as(
        cx,
        Some(telling_its_folder()),
        None,
        Some(data.path().to_path_buf()),
    );
    super::agents::with_agents(&ws, cx, super::agents::CLAUDE2);
    let own = restore(
        handle,
        &ws,
        cx,
        &saved_terminal(folder.path(), Some("claude"), Some("a"), None),
    );
    let item = saved_terminal(folder.path(), Some("claude2"), Some("b"), Some("claude2"));
    let account = restore(handle, &ws, cx, &item);
    screen_with(&ws, cx, account, "claude --resume b");
    screen_with(
        &ws,
        cx,
        account,
        &format!("dir={};", account_folder.display()),
    );
    let shown = screen_with(&ws, cx, own, "claude --resume a");
    assert!(
        !shown.contains("accounts"),
        "its agent's own folder: {shown}"
    );
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.terminal(own).unwrap().account, None);
        let pane = ws.space.terminal(account).unwrap();
        assert_eq!(pane.account.as_deref(), Some("claude2"));
        assert_eq!(pane.agent(), Some("claude2"));
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn an_account_block_keeps_its_folder_without_its_agent(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_as(
        cx,
        Some(telling_its_folder()),
        None,
        Some(data.path().to_path_buf()),
    );
    super::agents::with_agents(&ws, cx, super::agents::CLAUDE2);
    let gone = data.path().join("gone");
    let id = restore(
        handle,
        &ws,
        cx,
        &saved_terminal(&gone, None, None, Some("claude2")),
    );
    let account_folder = data.path().join("accounts").join("claude2");
    super::bus::screen_with(&ws, cx, id, &format!("dir={};", account_folder.display()));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.terminal(id).unwrap().account.as_deref(),
            Some("claude2")
        );
    });
}

#[cfg(unix)]
#[gpui_kit::test]
fn a_deleted_accounts_block_comes_back_plain(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let folder = tempfile::tempdir().unwrap();
    let (handle, ws) = open_as(
        cx,
        Some(telling_its_folder()),
        None,
        Some(data.path().to_path_buf()),
    );
    super::agents::with_agents(&ws, cx, super::agents::CLAUDE2);
    let item = saved_terminal(folder.path(), Some("gone"), None, Some("gone"));
    let id = restore(handle, &ws, cx, &item);
    let shown = super::bus::screen_with(&ws, cx, id, "dir=");
    assert!(!shown.contains("accounts"), "{shown}");
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.terminal(id).unwrap().account, None);
    });
}

/// A block opened as a role types the role's words again on restore, with its model and effort;
/// not once the role is gone from the config (docs/specs/agent-tools.md, Roles).
#[cfg(unix)]
#[gpui_kit::test]
fn a_roles_words_are_typed_again_on_restore_while_it_exists(cx: &mut TestAppContext) {
    use super::bus::screen_with;
    let folder = tempfile::tempdir().unwrap();
    let (handle, ws) = open_as(cx, Some(quiet()), None, None);
    ws.update(cx, |ws, _| {
        ws.config.roles.insert(
            "reviewer".into(),
            crate::config::Role {
                agent: "codex".into(),
                model: None,
                effort: None,
                args: Some("-s read-only".into()),
                instructions: Some("Never edit files.".into()),
            },
        )
    });
    let item = |id: &str| -> crate::session::Item {
        serde_json::from_value(serde_json::json!({
            "kind": "terminal", "uid": id,
            "rect": { "x": 0.0, "y": 900.0, "w": 700.0, "h": 450.0 },
            "folder": folder.path(), "agent": "codex", "role": "reviewer",
            "model": "gpt-5.6-luna",
        }))
        .unwrap()
    };
    let back = restore(handle, &ws, cx, &item("rol01"));
    let shown = screen_with(&ws, cx, back, "-m gpt-5.6-luna -s read-only");
    assert!(!shown.contains("Never edit"), "{shown}");
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.space.terminal(back).unwrap().role.as_deref(),
            Some("reviewer")
        )
    });
    ws.update(cx, |ws, _| ws.config.roles.clear());
    let plain = restore(handle, &ws, cx, &item("rol02"));
    let shown = screen_with(&ws, cx, plain, "-m gpt-5.6-luna");
    assert!(!shown.contains("read-only"), "{shown}");
}

/// A saved terminal comes back with the shell of its label among the shells found; one whose label
/// isn't found comes back with the default, its shell forgotten.
#[gpui_kit::test]
fn a_saved_shell_comes_back_and_one_not_found_opens_the_default(cx: &mut TestAppContext) {
    use templum_terminal::ShellOption;
    let (handle, ws) = open(cx);
    let (program, args) = quiet();
    let quiet_shell = ShellOption {
        label: "Quiet".into(),
        program,
        args,
    };
    ws.update(cx, |ws, _| ws.shells = vec![quiet_shell.clone()]);
    let mut add = |x: f32, label: &str| {
        let item: crate::session::Item = serde_json::from_value(serde_json::json!({
            "kind": "terminal",
            "rect": { "x": x, "y": 900.0, "w": 700.0, "h": 450.0 },
            "shell": label,
        }))
        .unwrap();
        cx.update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.add_saved(&item, window, cx))
        })
        .unwrap()
        .expect("the block exists either way")
    };
    let (found, lost) = (add(0.0, "Quiet"), add(800.0, "PowerShell 7"));
    ws.read_with(cx, |ws, _| {
        let rect = templum_canvas::Rect::new(0.0, 0.0, 700.0, 450.0);
        let found = ws.space.terminal(found).unwrap();
        assert_eq!(found.shell, Some(quiet_shell.clone()));
        assert_eq!(
            found.item(rect, String::new()).shell.as_deref(),
            Some("Quiet")
        );
        let lost = ws.space.terminal(lost).unwrap();
        assert_eq!(lost.shell, None, "another computer's shell: forgotten");
        assert_eq!(lost.item(rect, String::new()).shell, None);
    });
}

/// At start, a canvas file's terminal runs its shell: the startup restore reads the list first.
/// It runs the machine's own list, so the shell is one with no slow startup files (`sh`, never a login
/// bash or Git Bash); a machine whose list lacks it skips the check.
#[gpui_kit::test]
fn a_canvas_files_terminal_starts_its_shell(cx: &mut TestAppContext) {
    let label = if cfg!(windows) {
        "Command Prompt"
    } else {
        "sh"
    };
    let Some(first) = templum_terminal::available_shells()
        .into_iter()
        .find(|s| s.label == label)
    else {
        eprintln!("skipped: this computer lists no {label}");
        return;
    };
    let data = tempfile::tempdir().unwrap();
    write_canvas(
        data.path(),
        &format!(
            r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"terminal","rect":{{"x":0,"y":0,"w":700,"h":450}},"shell":{}}}]}}"#,
            serde_json::to_string(&first.label).unwrap()
        ),
    );
    let (_, ws) = open_as(cx, Some(quiet()), None, Some(data.path().to_path_buf()));
    tick(cx);
    ws.read_with(cx, |ws, _| {
        let blocks = ws.space.canvas.blocks();
        assert_eq!(blocks.len(), 1);
        assert_eq!(ws.space.terminal(blocks[0].id).unwrap().shell, Some(first));
    });
}

/// A restored agent's resume line is typed for its terminal's shell: none of Templum's words in WSL.
#[cfg(unix)]
#[gpui_kit::test]
fn a_restored_agents_line_follows_its_terminals_shell(cx: &mut TestAppContext) {
    use super::bus::screen_with;
    use templum_terminal::ShellOption;
    let (data, folder, bin) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    // A shell named `wsl` that only echoes what is typed.
    let wsl = bin.path().join("wsl");
    std::os::unix::fs::symlink(templum_terminal::locate("cat").unwrap(), &wsl).unwrap();
    let (handle, ws) = open_as(cx, Some(quiet()), None, Some(data.path().to_path_buf()));
    ws.update(cx, |ws, _| {
        ws.shells = vec![ShellOption {
            label: "WSL".into(),
            program: wsl.display().to_string(),
            args: vec![],
        }]
    });
    let mut item = saved_terminal(folder.path(), Some("claude"), None, None);
    if let crate::session::Item::Terminal(t) = &mut item {
        t.shell = Some("WSL".into());
    }
    let id = restore(handle, &ws, cx, &item);
    let shown = screen_with(&ws, cx, id, "claude");
    assert!(!shown.contains("--settings"), "{shown}");
}

/// A saved shell the list lacks opens the default at once, even while the login shell's PATH is
/// still to come: the window doesn't wait for it, and `shells_early` stays for the next menu to
/// read the list again.
#[gpui_kit::test]
fn a_saved_shell_missing_from_an_early_list_opens_the_default_without_waiting(
    cx: &mut TestAppContext,
) {
    use templum_terminal::ShellOption;
    let (handle, ws) = open(cx);
    let (program, args) = quiet();
    let quiet_shell = ShellOption {
        label: "Quiet".into(),
        program,
        args,
    };
    ws.update(cx, |ws, _| {
        ws.shells = vec![quiet_shell.clone()];
        ws.shells_early = true;
    });
    let item: crate::session::Item = serde_json::from_value(serde_json::json!({
        "kind": "terminal",
        "rect": { "x": 0.0, "y": 900.0, "w": 700.0, "h": 450.0 },
        "shell": "fish",
    }))
    .unwrap();
    let id = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| ws.add_saved(&item, window, cx))
        })
        .unwrap()
        .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.terminal(id).unwrap().shell, None);
        assert_eq!(ws.shells, vec![quiet_shell], "the list wasn't read again");
        assert!(ws.shells_early);
    });
}
