//! Editor tabs: editing, saving, reloading from disk and unsaved changes.

use super::*;

const CLOSE_TAB: &str = "ctrl-w";

#[gpui_kit::test]
fn clicking_a_file_opens_it_in_a_tab_with_its_text_and_language(cx: &mut TestAppContext) {
    use gpui_kit::ElementId;
    use gpui_kit::Focusable as _;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("main.rs"), "fn main() {}\r\n").unwrap();
    std::fs::write(dir.path().join("notes.md"), "# Hi\n").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    let n = id.0 as usize;
    click_file(handle, 0, cx);
    cx.update_window(handle, |_, window, cx| {
        let ws_now = ws.read(cx);
        let e = ws_now.space.editor(id).unwrap();
        assert_eq!(e.tabs.len(), 1);
        let tab = e.active_tab().unwrap();
        assert_eq!(
            tab.editor.read(cx).value().as_ref(),
            "fn main() {}\n",
            "CRLF normalized"
        );
        assert_eq!(tab.editor.read(cx).language_name().as_ref(), "rust");
        assert!(matches!(
            tab.content,
            crate::editor::Content::Text {
                crlf: true,
                newline: true,
                read_only: None
            }
        ));
        assert!(
            tab.editor.read(cx).focus_handle(cx).is_focused(window),
            "the file has the keyboard"
        );
        assert!(e.title().starts_with("main.rs — "), "{}", e.title());
        window.find(("code", n));
        window.find(ElementId::Name(format!("tab-{n}-0").into()));
    })
    .unwrap();
    // A second file: a second tab, active; the Markdown preview toggle shows for it.
    click_file(handle, 1, cx);
    cx.update_window(handle, |_, window, cx| {
        let e = ws.read(cx).space.editor(id).unwrap();
        assert_eq!((e.tabs.len(), e.active), (2, Some(1)));
        window.click(("md-preview", n), cx);
        window.render_frame(cx);
        window.find(("file-preview", n));
        assert!(window.try_find(("code", n)).is_none());
        // Clicking the first file's row again shows its tab, no new one.
        window.click(("entry", 0usize), cx);
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        assert_eq!((e.tabs.len(), e.active), (2, Some(0)));
    })
    .unwrap();
}

#[gpui_kit::test]
fn clicking_inside_an_open_file_keeps_its_editor_focused(cx: &mut TestAppContext) {
    use gpui_kit::Focusable as _;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "text").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| window.focus(&ws.focus_handle, cx));
        window.render_frame(cx);
        let body = window.find(("code", id.0 as usize)).bounds();
        click(
            window,
            cx,
            (
                f32::from(body.origin.x) + f32::from(body.size.width) / 2.0,
                f32::from(body.origin.y) + f32::from(body.size.height) / 2.0,
            ),
            1,
        );
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        assert!(
            e.active_tab()
                .unwrap()
                .editor
                .read(cx)
                .focus_handle(cx)
                .is_focused(window),
            "the block's mouse-down gave the keyboard back to the file"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_binary_file_and_a_big_one_are_not_edited(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.bin"), b"\x7fELF\0\x01").unwrap();
    let lines = crate::editor::READ_ONLY_OVER as usize / 2 + 1;
    std::fs::write(dir.path().join("big.txt"), "x\n".repeat(lines)).unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    let n = id.0 as usize;
    click_file(handle, 0, cx);
    cx.update_window(handle, |_, window, _| {
        window.find(("binary", n));
        window.find(("reveal", n));
        assert!(window.try_find(("code", n)).is_none());
    })
    .unwrap();
    click_file(handle, 1, cx);
    cx.update_window(handle, |_, window, cx| {
        window.find(("code", n));
        let e = ws.read(cx).space.editor(id).unwrap();
        assert!(!e.active_tab().unwrap().editor.read(cx).is_editable());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ctrl_w_closes_the_tab_and_the_x_closes_another(cx: &mut TestAppContext) {
    use gpui_kit::ElementId;
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(dir.path().join(name), name).unwrap();
    }
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    let n = id.0 as usize;
    click_file(handle, 0, cx);
    click_file(handle, 1, cx);
    click_file(handle, 2, cx);
    cx.update_window(handle, |_, window, cx| {
        // c.txt is active: Ctrl+W closes it, b.txt becomes active.
        window.press(CLOSE_TAB, cx);
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        assert_eq!((e.tabs.len(), e.active), (2, Some(1)));
        assert_eq!(e.active_tab().unwrap().name(), "b.txt");
        // The ✕ of a.txt: b.txt stays active, now at 0.
        window.click(ElementId::Name(format!("tab-close-{n}-0").into()), cx);
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        assert_eq!((e.tabs.len(), e.active), (1, Some(0)));
        assert_eq!(e.active_tab().unwrap().name(), "b.txt");
        window.press(CLOSE_TAB, cx);
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        assert_eq!((e.tabs.len(), e.active), (0, None));
        // With nothing open, a new terminal shortcut still works: the keyboard is back.
        window.press(NEW, cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| assert_eq!(ws.space.terminals().count(), 2));
}

#[gpui_kit::test]
fn a_saved_editor_comes_back_with_its_files_and_skips_paths_outside_the_folder(
    cx: &mut TestAppContext,
) {
    let data = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("proj");
    std::fs::create_dir_all(folder.join("src")).unwrap();
    std::fs::write(folder.join("src/a.rs"), "a").unwrap();
    std::fs::write(folder.join("b.txt"), "b").unwrap();
    std::fs::write(root.path().join("outside.txt"), "no").unwrap();
    let json = |p: &std::path::Path| p.display().to_string().replace('\\', "\\\\");
    write_canvas(
        data.path(),
        &format!(
            r#"{{"version":1,"camera":{{"x":0,"y":0,"zoom":1}},"items":[
                  {{"kind":"editor","rect":{{"x":0,"y":0,"w":900,"h":600}},"folder":"{}",
                    "open":["src/a.rs","../outside.txt","{}","gone.txt","b.txt"],"active":4}}]}}"#,
            json(&folder),
            json(&folder.join("b.txt"))
        ),
    );
    let (_, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let id = first_id(&ws, cx);
    ws.read_with(cx, |ws, _| {
        let e = ws.space.editor(id).unwrap();
        let names: Vec<_> = e.tabs.iter().map(|t| t.name()).collect();
        assert_eq!(
            names,
            ["a.rs", "b.txt"],
            "outside, absolute and gone: skipped"
        );
        assert_eq!(
            e.active,
            Some(1),
            "the saved index is beyond the kept tabs: the last one"
        );
    });
    // What is saved: relative paths, the active one's position among them.
    let item = ws.read_with(cx, |ws, _| {
        ws.space
            .editor(id)
            .unwrap()
            .item(ws.space.canvas.get(id).unwrap().rect)
    });
    assert_eq!(item.open, ["src/a.rs", "b.txt"]);
    assert_eq!(item.active, Some(1));
}

#[cfg(target_os = "macos")]
const SAVE: &str = "cmd-s";
#[cfg(not(target_os = "macos"))]
const SAVE: &str = "ctrl-s";

#[gpui_kit::test]
fn typing_marks_the_tab_changed_and_ctrl_s_writes_the_file(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "one\r\ntwo\r\n").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    cx.simulate_input(handle, "x");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        assert!(e.active_tab().unwrap().dirty);
        assert!(e.title().starts_with("● a.txt"), "{}", e.title());
        window.press(SAVE, cx);
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        assert!(!e.active_tab().unwrap().dirty, "saved");
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "xone\r\ntwo\r\n",
        "the file keeps its CRLF and its final newline"
    );
}

#[gpui_kit::test]
fn closing_with_unsaved_changes_asks_first(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    cx.simulate_input(handle, "x");
    click_file(handle, 1, cx);
    cx.simulate_input(handle, "y");
    // Ctrl+W on a changed tab: the question; Cancel keeps it.
    cx.update_window(handle, |_, window, cx| {
        window.press(CLOSE_TAB, cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        window.within("dialog").click("cancel", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        assert_eq!(ws.read(cx).space.editor(id).unwrap().tabs.len(), 2);
    })
    .unwrap();
    // Save writes b.txt and closes the tab.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.focus_block(id, window, cx));
        window.press(CLOSE_TAB, cx);
        window.render_frame(cx);
        window.within("dialog").click("save", cx);
        window.render_frame(cx);
        assert_eq!(ws.read(cx).space.editor(id).unwrap().tabs.len(), 1);
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "yb"
    );
    // Closing the block with a.txt changed: the question; Don't save closes anyway.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.close(id, window, cx));
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        assert!(ws.read(cx).space.editor(id).is_some(), "still there");
        window.within("dialog").click("discard", cx);
        window.render_frame(cx);
        assert!(ws.read(cx).space.editor(id).is_none());
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "a",
        "not written"
    );
}

#[gpui_kit::test]
fn a_close_tab_question_answered_in_another_workspace_does_nothing(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (one, two) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::fs::write(one.path().join("a.txt"), "a").unwrap();
    std::fs::write(two.path().join("c.txt"), "c").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, one.path());
    click_file(handle, 0, cx);
    cx.simulate_input(handle, "x");
    // The second workspace has an editor with the same id and a changed tab of its own.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx))
    })
    .unwrap();
    // A note takes the id the first workspace's terminal took.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.add_note_at(Point::new(900.0, 40.0), window, cx)
        })
    })
    .unwrap();
    assert_eq!(open_editor(handle, &ws, cx, two.path()), id);
    click_file(handle, 0, cx);
    cx.simulate_input(handle, "z");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.switch_to(0, window, cx);
            ws.focus_block(id, window, cx);
        });
        window.press(CLOSE_TAB, cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        ws.update(cx, |ws, cx| ws.switch_to(1, window, cx));
        window.within("dialog").click("discard", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.editor(id).unwrap().tabs.len(), 1, "c.txt stays");
        assert_eq!(
            ws.said.last().map(String::as_str),
            Some("Another workspace is shown now: nothing was done.")
        );
    });
}

#[gpui_kit::test]
fn a_failed_save_says_so_and_keeps_the_tab_changed(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "a").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    cx.simulate_input(handle, "x");
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&path, permissions).unwrap();
    let before = notification_count(handle, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press(SAVE, cx);
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        assert!(e.active_tab().unwrap().dirty);
    })
    .unwrap();
    assert_eq!(notification_count(handle, cx), before + 1);
}

/// Rewrites `path` as another program would, a modification time clearly later.
fn change_on_disk(path: &std::path::Path, text: &str) {
    use std::time::{Duration, SystemTime};
    std::fs::write(path, text).unwrap();
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() + Duration::from_secs(5))
        .unwrap();
}

#[gpui_kit::test]
fn a_file_changed_on_disk_is_reloaded_while_clean_and_flagged_while_edited(
    cx: &mut TestAppContext,
) {
    use gpui_kit::component::input::Position;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    let n = id.0 as usize;
    click_file(handle, 0, cx);
    let value = |ws: &Entity<Workspace>, cx: &mut TestAppContext| {
        ws.read_with(cx, |ws, cx| {
            let tab = ws.space.editor(id).unwrap().active_tab().unwrap();
            tab.editor.read(cx).value().to_string()
        })
    };
    // The cursor on line 2 (0-based 1); a clean tab is reloaded in place, the line kept.
    cx.update_window(handle, |_, window, cx| {
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        let editor = tab.editor.clone();
        editor.update(cx, |e, cx| {
            e.set_cursor_position(
                Position {
                    line: 1,
                    character: 0,
                },
                window,
                cx,
            )
        });
    })
    .unwrap();
    change_on_disk(&path, "uno\ndos\ntres\n");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.check_files(window, cx));
        window.render_frame(cx);
        let e = ws.read(cx).space.editor(id).unwrap();
        let tab = e.active_tab().unwrap();
        assert_eq!(tab.editor.read(cx).value().as_ref(), "uno\ndos\ntres\n");
        assert_eq!(tab.editor.read(cx).cursor_position().line, 1);
        assert!(!tab.dirty && !tab.conflict);
    })
    .unwrap();
    // Edited, then changed on disk: flagged, the bar shows; Keep mine clears the flag.
    cx.simulate_input(handle, "x");
    assert_eq!(
        value(&ws, cx),
        "uno\nxdos\ntres\n",
        "typed on the kept line"
    );
    change_on_disk(&path, "other\n");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.check_files(window, cx));
        window.render_frame(cx);
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        assert!(tab.conflict && tab.dirty);
        window.find(("conflict", n));
        window.click(("keep", n), cx);
        window.render_frame(cx);
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        assert!(!tab.conflict && tab.dirty);
        assert!(window.try_find(("conflict", n)).is_none());
    })
    .unwrap();
    assert_eq!(value(&ws, cx), "uno\nxdos\ntres\n", "kept");
    // Changed again: Reload takes the disk's text.
    change_on_disk(&path, "newest\n");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.check_files(window, cx));
        window.render_frame(cx);
        window.click(("reload", n), cx);
        window.render_frame(cx);
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        assert!(!tab.conflict && !tab.dirty);
    })
    .unwrap();
    assert_eq!(value(&ws, cx), "newest\n");
    // Templum's own save is not a change on disk: nothing is reloaded or flagged.
    cx.simulate_input(handle, "y");
    let typed = value(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press(SAVE, cx);
        ws.update(cx, |ws, cx| ws.check_files(window, cx));
        window.render_frame(cx);
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        assert!(
            !tab.conflict && !tab.dirty,
            "a save is not a change on disk"
        );
    })
    .unwrap();
    assert_eq!(value(&ws, cx), typed);
    // Gone: the tab stays, marked changed, and Save brings the file back.
    std::fs::remove_file(&path).unwrap();
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.check_files(window, cx));
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        assert!(tab.dirty && tab.mtime.is_none());
        window.press(SAVE, cx);
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        crate::editor::encode(&typed, false, true)
    );
}

#[cfg(target_os = "macos")]
const GO_TO_LINE: &str = "cmd-g";
#[cfg(not(target_os = "macos"))]
const GO_TO_LINE: &str = "ctrl-g";

#[gpui_kit::test]
fn ctrl_g_asks_for_a_line_and_goes_there(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "1\n2\n3\n4\n").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press(GO_TO_LINE, cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
    })
    .unwrap();
    cx.simulate_input(handle, "3");
    cx.update_window(handle, |_, window, cx| {
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        assert_eq!(tab.editor.read(cx).cursor_position().line, 2);
    })
    .unwrap();
}

// Linux only: macOS's zsh takes Ctrl+G as send-break and shows nothing, while bash
// in cooked mode echoes it as ^G.
#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn ctrl_g_reaches_the_terminal_without_an_editor(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let shown = echo_after(GO_TO_LINE, handle, &ws, cx);
    assert!(shown.contains("^G"), "{shown:?}");
}

/// An editor on `dir` with `name` open and typed into, so it has unsaved edits.
fn dirty_tab(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    dir: &std::path::Path,
) -> BlockId {
    let id = open_editor(handle, ws, cx, dir);
    click_file(handle, 0, cx);
    cx.simulate_input(handle, "x");
    id
}

#[gpui_kit::test]
fn closing_a_block_doesnt_ask_about_another_workspaces_files(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let (handle, ws) = open(cx);
    let id = dirty_tab(handle, &ws, cx, dir.path());
    // Another workspace, whose blocks count their ids from 0 again.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            while ws.space.terminal(id).is_none() {
                ws.new_terminal(window, cx);
            }
            ws.close(id, window, cx);
        });
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx), "not this workspace's file");
        assert!(ws.read(cx).space.terminal(id).is_none(), "closed");
    })
    .unwrap();
}

#[gpui_kit::test]
fn closing_the_window_with_unsaved_files_asks_first(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let (handle, ws) = open(cx);
    let mut window_cx = gpui_kit::VisualTestContext::from_window(handle, cx);
    assert!(window_cx.simulate_close(), "nothing unsaved: it closes");
    dirty_tab(handle, &ws, cx, dir.path());
    assert!(!window_cx.simulate_close(), "unsaved: it stays open");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "and asks");
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_file_that_isnt_utf8_opens_read_only_and_keeps_its_bytes(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("latin1.txt");
    std::fs::write(&path, b"caf\xe9\n").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    cx.simulate_input(handle, "x");
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        assert!(!tab.editor.read(cx).is_editable(), "read-only");
        assert!(!tab.dirty);
        window.press(SAVE, cx);
    })
    .unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"caf\xe9\n",
        "its bytes are kept"
    );
}

#[gpui_kit::test]
fn deleting_a_workspace_with_unsaved_files_asks_and_can_save_them(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let data = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    dirty_tab(handle, &ws, cx, dir.path());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            ws.open_menu(
                crate::workspace::menu::Target::Space("default".into()),
                point(px(100.0), px(100.0)),
                window,
                cx,
            );
        });
        window.render_frame(cx);
        // Open, Edit…, a separator, Stop, Delete….
        window.within("popup-menu").click(4usize, cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx));
        window.within("dialog").click("save", cx);
        window.render_frame(cx);
        assert_eq!(ws.read(cx).others.len(), 0, "deleted");
    })
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "xa",
        "saved first"
    );
}

#[gpui_kit::test]
fn a_file_that_grows_past_4_mb_on_disk_turns_read_only(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.txt");
    std::fs::write(&path, "start\n").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    let lines = crate::editor::READ_ONLY_OVER as usize / 2 + 1;
    change_on_disk(&path, &"x\n".repeat(lines));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.check_files(window, cx));
        window.render_frame(cx);
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        assert!(!tab.editor.read(cx).is_editable(), "over 4 MB: read-only");
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_makefile_indents_with_a_tab(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Makefile"), "all:\n").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    cx.update_window(handle, |_, window, cx| {
        window.press("tab", cx);
        window.render_frame(cx);
        let tab = ws.read(cx).space.editor(id).unwrap().active_tab().unwrap();
        let text = tab.editor.read(cx).value().to_string();
        assert!(text.starts_with('\t'), "{text:?}");
    })
    .unwrap();
}

#[gpui_kit::test]
fn choosing_another_folder_keeps_unsaved_tabs(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let (handle, ws) = open(cx);
    let id = dirty_tab(handle, &ws, cx, dir.path());
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.set_editor_folder(id, other.path(), window, cx);
        });
        let e = ws.read(cx).space.editor(id).unwrap();
        assert_eq!(e.tabs.len(), 1, "the changed file stays open");
        assert!(e.tabs[0].dirty);
        assert_eq!(e.active, Some(0));
    })
    .unwrap();
}

#[gpui_kit::test]
fn an_editor_sends_its_selection_or_its_file_and_line(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("a.rs");
    std::fs::write(&file, "fn a() {}\nfn b() {}\n").unwrap();
    let (handle, ws) = open(cx);
    let id = open_editor(handle, &ws, cx, dir.path());
    click_file(handle, 0, cx);
    cx.update_window(handle, |_, window, cx| {
        let tab = ws.read(cx).space.editor(id).unwrap().tabs[0].editor.clone();
        tab.update(cx, |e, cx| {
            e.set_cursor_position(
                gpui_kit::component::input::Position {
                    line: 1,
                    character: 3,
                },
                window,
                cx,
            )
        });
        let sent = ws.read(cx).space.editor(id).unwrap().to_send(cx);
        assert_eq!(sent, Some(format!("{}:2", file.display())));
        tab.update(cx, |e, cx| e.set_selected_range(3..4, cx));
        let sent = ws.read(cx).space.editor(id).unwrap().to_send(cx);
        assert_eq!(sent.as_deref(), Some("a"));
    })
    .unwrap();
}
