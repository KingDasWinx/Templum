//! Workspaces: switching, renaming, deleting and bringing them back.

use super::*;
use crate::space_dialog::Look;

#[test]
fn a_workspace_shows_an_agent_from_its_last_snapshot() {
    use crate::session::{Item, Saved, SavedCamera, SavedRect, TerminalItem, VERSION};
    let with = |agent: Option<&str>| crate::workspace::Space {
        saved: Some(Saved {
            version: VERSION,
            camera: SavedCamera {
                x: 0.0,
                y: 0.0,
                zoom: 1.0,
            },
            items: vec![Item::Terminal(TerminalItem {
                uid: String::new(),
                rect: SavedRect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
                folder: None,
                agent: agent.map(String::from),
                session: None,
                opened_by: None,
                model: None,
                effort: None,
                account: None,
                role: None,
                shell: None,
            })],
            focused: None,
            recent: Vec::new(),
            ink: Vec::new(),
            grid: None,
            wires: Vec::new(),
            git: None,
        }),
        ..crate::workspace::Space::default()
    };
    let known = crate::agents::effective(&Default::default(), &[], None);
    assert!(with(Some("claude")).has_agent(&known));
    assert!(
        !with(Some("vim")).has_agent(&known),
        "not an agent Templum knows"
    );
    assert!(!with(None).has_agent(&known));
    assert!(
        !crate::workspace::Space::default().has_agent(&known),
        "no snapshot yet"
    );
    let mut stopped = with(Some("claude"));
    stopped.stopped = true;
    assert!(!stopped.has_agent(&known), "a stopped workspace runs none");
}

#[test]
fn ctrl_digits_pick_a_workspace() {
    let digit = |k: &str, macos| {
        crate::workspace::workspace_digit(&gpui_kit::Keystroke::parse(k).unwrap(), macos)
    };
    assert_eq!(digit("ctrl-1", false), Some(0));
    assert_eq!(digit("ctrl-9", false), Some(8));
    assert_eq!(digit("cmd-2", true), Some(1));
    assert_eq!(digit("ctrl-2", true), None, "Cmd on macOS");
    assert_eq!(digit("ctrl-0", false), None, "zoom reset");
    assert_eq!(digit("ctrl-shift-1", false), None, "fit all");
    assert_eq!(digit("ctrl-alt-1", false), None);
    assert_eq!(digit("1", false), None);
}

#[cfg(target_os = "macos")]
const SECOND: &str = "cmd-2";
#[cfg(not(target_os = "macos"))]
const SECOND: &str = "ctrl-2";

#[gpui_kit::test]
fn a_new_workspace_is_empty_and_the_first_keeps_running(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!((ws.active, ws.others.len()), (1, 1));
        assert_eq!(ws.space.name, "Workspace 2");
        assert!(ws.space.canvas.blocks().is_empty(), "a new one is empty");
        assert_eq!(ws.others[0].name, "Main");
    });
    cx.update_window(handle, |_, window, cx| {
        window.find("workspace-name");
        window.click("workspace-default", cx);
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.active, 0);
        assert_eq!(ws.space.focused, Some(first));
        assert!(
            !ws.space.terminal(first).unwrap().term.exited(),
            "still running"
        );
    });
}

#[gpui_kit::test]
fn workspace_keys_cycle_and_jump(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            ws.new_space(window, cx);
        });
        window.render_frame(cx);
        assert_eq!(ws.read(cx).active, 2);
        window.press("ctrl-tab", cx);
        assert_eq!(ws.read(cx).active, 0, "wraps around");
        window.press("ctrl-shift-tab", cx);
        assert_eq!(ws.read(cx).active, 2);
        window.press(SECOND, cx);
        assert_eq!(ws.read(cx).active, 1);
    })
    .unwrap();
}

#[gpui_kit::test]
fn hidden_workspaces_get_theme_changes(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let first = first_id(&ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.new_space(window, cx));
    })
    .unwrap();
    ws.update(cx, |ws, cx| ws.set_dark(true, cx));
    ws.read_with(cx, |ws, _| {
        assert_eq!(
            ws.others[0].terminal(first).unwrap().term.palette(),
            ws.theme.palette
        );
    });
}

#[gpui_kit::test]
fn renaming_and_deleting_workspaces(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let edit =
                |ws: &mut Workspace, name, window: &mut Window, cx: &mut Context<Workspace>| {
                    ws.edit_space("default", name, Look::default(), None, window, cx)
                };
            assert!(edit(ws, "  ", window, cx).is_err());
            assert_eq!(ws.space.name, "Main", "an empty name is refused");
            edit(ws, " Work ", window, cx).unwrap();
            assert_eq!(ws.space.name, "Work");
            ws.delete_space("default", window, cx);
            assert_eq!(ws.others.len(), 0, "the last one stays");
            ws.new_space(window, cx);
            ws.open(Rect::new(0.0, 0.0, 600.0, 400.0), window, cx);
            ws.save_canvas_now(cx);
            ws.delete_space("w2", window, cx);
            assert_eq!((ws.active, ws.others.len()), (0, 0));
            assert_eq!(ws.space.name, "Work", "the one before it is shown");
        });
        window.render_frame(cx);
        let name = window.find("workspace-name");
        assert!(name.bounds().size.width > px(0.0));
    })
    .unwrap();
    let w2 = crate::session::path(data.path(), "w2");
    assert!(!w2.exists() && w2.with_extension("json.bak").exists());
    let index = crate::session::load_index(data.path());
    let crate::session::Loaded::Saved(index) = index else {
        panic!("no index: {index:?}");
    };
    assert_eq!(index.workspaces.len(), 1);
    assert_eq!(index.workspaces[0].name, "Work");
}

#[gpui_kit::test]
fn every_workspace_comes_back_in_order(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            ws.open(Rect::new(10.0, 20.0, 600.0, 400.0), window, cx);
            ws.open(Rect::new(700.0, 20.0, 600.0, 400.0), window, cx);
            ws.edit_space("w2", "Side project", Look::default(), None, window, cx)
                .unwrap();
            ws.new_space(window, cx);
            ws.switch_to(1, window, cx);
            ws.toggle_sidebar(window, cx);
            ws.save_canvas_now(cx);
        })
    })
    .unwrap();
    let (_, again) = open_in(cx, None, Some(data.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        let names: Vec<&str> = ws.order().iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Main", "Side project", "Workspace 3"]);
        assert_eq!(ws.active, 1);
        assert!(!ws.sidebar);
        assert_eq!(ws.space.canvas.blocks().len(), 2);
        assert_eq!(ws.others[0].canvas.blocks().len(), 1, "Main's terminal");
        assert!(ws.others[1].canvas.blocks().is_empty());
    });
}

#[gpui_kit::test]
fn enter_in_the_edit_dialog_applies_a_typed_name(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::space_dialog::open(ws, "default", window, cx)
        });
        window.render_frame(cx);
        // The name is selected: typing replaces it.
        window.input("Work", cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert_eq!(ws.read(cx).space.name, "Work");
    })
    .unwrap();
}

/// The index as written in `data`.
fn written_index(data: &std::path::Path) -> crate::session::Index {
    match crate::session::load_index(data) {
        crate::session::Loaded::Saved(index) => index,
        other => panic!("no index: {other:?}"),
    }
}

#[gpui_kit::test]
fn editing_a_workspace_sets_its_look_and_cancel_keeps_the_old_one(cx: &mut TestAppContext) {
    use gpui_kit::assets::IconName;
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.open_menu(
                crate::workspace::menu::Target::Space("default".into()),
                point(px(100.0), px(100.0)),
                window,
                cx,
            )
        });
        window.render_frame(cx);
        // Open, Edit….
        window.within("popup-menu").click(1usize, cx);
        window.render_frame(cx);
        window.within("dialog").click("space-color-blue", cx);
        window.within("dialog").click("space-icon-rocket", cx);
        window.render_frame(cx);
        window.within("dialog").click("cancel", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        assert_eq!(ws.read(cx).space.look, Look::default(), "Cancel keeps it");
        ws.update(cx, |ws, cx| {
            crate::space_dialog::open(ws, "default", window, cx)
        });
        window.render_frame(cx);
        window.within("dialog").click("space-color-blue", cx);
        window.within("dialog").click("space-icon-rocket", cx);
        window.render_frame(cx);
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        let look = ws.read(cx).space.look.clone();
        assert_eq!(look.color.as_deref(), Some("blue"));
        assert_eq!(look.icon, Some(IconName::Rocket));
        // The title bar shows it.
        window.render_frame(cx);
    })
    .unwrap();
    let entry = &written_index(data.path()).workspaces[0];
    assert_eq!(entry.icon.as_deref(), Some("rocket"));
    assert_eq!(entry.color.as_deref(), Some("blue"));
}

#[gpui_kit::test]
fn an_empty_name_is_refused_in_the_edit_dialog(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            crate::space_dialog::open(ws, "default", window, cx)
        });
        window.render_frame(cx);
        // The name is selected: this empties it.
        window.press("backspace", cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "it stays open");
        assert!(window.try_find("space-note").is_some(), "saying why");
        assert_eq!(ws.read(cx).space.name, "Main");
    })
    .unwrap();
}

#[cfg(target_os = "macos")]
const NEW_WORKSPACE: &str = "cmd-shift-n";
#[cfg(not(target_os = "macos"))]
const NEW_WORKSPACE: &str = "ctrl-shift-n";

/// The workspaces' names, in the sidebar's order.
fn names(ws: &Entity<Workspace>, cx: &gpui_kit::App) -> Vec<String> {
    ws.read(cx).order().iter().map(|s| s.name.clone()).collect()
}

#[gpui_kit::test]
fn every_new_workspace_button_opens_its_dialog_and_cancel_and_esc_make_nothing(
    cx: &mut TestAppContext,
) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let index = crate::session::index_path(data.path());
    // On the canvas, then on the Home: each of its ways in.
    for (home, ways) in [
        (false, &["new-workspace", NEW_WORKSPACE][..]),
        (true, &["home-new", "new-workspace", NEW_WORKSPACE]),
    ] {
        if home {
            cx.update_window(handle, |_, window, cx| {
                ws.update(cx, |ws, cx| ws.show_home(window, cx));
                window.render_frame(cx);
            })
            .unwrap();
        }
        let written = std::fs::read(&index).ok();
        for way in ways {
            for close in ["cancel", "escape"] {
                cx.update_window(handle, |_, window, cx| {
                    if way.contains('-') && !way.contains("shift") {
                        window.click(*way, cx);
                    } else {
                        window.press(way, cx);
                    }
                    window.render_frame(cx);
                    assert!(window.has_active_dialog(cx), "{way} opens it");
                    let ok = window.within("dialog").find("ok");
                    assert_eq!(ok.label(), Some("Create"), "{way}");
                    assert!(window.try_find("space-share").is_none(), "nothing to share");
                    assert_eq!(names(&ws, cx), ["Main"], "{way} makes nothing yet");
                    if close == "cancel" {
                        window.within("dialog").click("cancel", cx);
                    } else {
                        window.press("escape", cx);
                    }
                    window.render_frame(cx);
                    assert!(!window.has_active_dialog(cx), "{close} closes it");
                    assert_eq!(names(&ws, cx), ["Main"], "{close} makes nothing");
                    assert_eq!(ws.read(cx).home_up(), home);
                })
                .unwrap();
            }
        }
        assert_eq!(std::fs::read(&index).ok(), written, "nothing written");
    }
}

#[gpui_kit::test]
fn create_makes_the_new_workspace_with_its_defaults_and_shows_it(cx: &mut TestAppContext) {
    no_motion(cx);
    let (handle, ws) = open(cx);
    cx.update_window(handle, |_, window, cx| {
        window.click("new-workspace", cx);
        window.render_frame(cx);
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
        let ws = ws.read(cx);
        assert_eq!(ws.active, 1, "shown");
        assert_eq!(ws.space.name, "Workspace 2");
        assert_eq!(ws.space.look, Look::default());
        assert_eq!(ws.space.folder, None);
    })
    .unwrap();
}

#[gpui_kit::test]
fn create_makes_the_new_workspace_with_what_was_chosen(cx: &mut TestAppContext) {
    use gpui_kit::assets::IconName;
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    cx.update_window(handle, |_, window, cx| {
        window.press(NEW_WORKSPACE, cx);
        window.render_frame(cx);
        // The name is selected: emptied, it is refused, saying why.
        window.press("backspace", cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "it stays open");
        assert!(window.try_find("space-note").is_some());
        assert_eq!(names(&ws, cx), ["Main"]);
        window.input("Api", cx);
        window.within("dialog").click("space-color-blue", cx);
        window.within("dialog").click("space-icon-rocket", cx);
        window.render_frame(cx);
        window.press("enter", cx);
        window.render_frame(cx);
        assert!(!window.has_active_dialog(cx));
        assert_eq!(names(&ws, cx), ["Main", "Api"]);
        let ws = ws.read(cx);
        assert_eq!(ws.space.name, "Api", "shown");
        assert_eq!(ws.space.look.color.as_deref(), Some("blue"));
        assert_eq!(ws.space.look.icon, Some(IconName::Rocket));
    })
    .unwrap();
    let entry = &written_index(data.path()).workspaces[1];
    assert_eq!(entry.name, "Api");
    assert_eq!(entry.icon.as_deref(), Some("rocket"));
}

#[gpui_kit::test]
fn create_takes_a_folder_and_refuses_one_that_isnt(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let folder = tempfile::tempdir().unwrap();
    let gone = folder.path().join("gone");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let why = ws
                .create_space("Gone", Look::default(), Some(gone.clone()), window, cx)
                .unwrap_err();
            assert_eq!(why, format!("{} isn't a folder", gone.display()));
            assert_eq!(ws.others.len(), 0, "nothing made");
            let why = ws.create_space("  ", Look::default(), None, window, cx);
            assert!(why.is_err() && ws.others.is_empty(), "an empty name");
            let at = Some(folder.path().to_path_buf());
            ws.create_space("Api", Look::default(), at.clone(), window, cx)
                .unwrap();
            assert_eq!((ws.space.name.as_str(), &ws.space.folder), ("Api", &at));
        })
    })
    .unwrap();
}

#[gpui_kit::test]
fn deleting_a_workspace_removes_its_image(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let pictures = tempfile::tempdir().unwrap();
    let png = pictures.path().join("me.png");
    std::fs::write(&png, b"not really a png").unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let kept = crate::session::icons_dir(data.path()).join("w2.png");
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            let look = Look {
                image: Some(png.clone()),
                ..Look::default()
            };
            ws.edit_space("w2", "Me", look, None, window, cx).unwrap();
            assert_eq!(ws.space.look.image.as_ref(), Some(&kept), "a copy is shown");
        });
        // An image that can't be decoded shows the initials, and says nothing.
        window.render_frame(cx);
    })
    .unwrap();
    assert!(kept.exists());
    assert_eq!(
        written_index(data.path()).workspaces[1].image.as_deref(),
        Some("w2.png")
    );
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.delete_space("w2", window, cx))
    })
    .unwrap();
    assert!(!kept.exists());
    assert!(png.exists(), "the file chosen stays");
}

/// Waits for the process `pid` to end.
fn ended(pid: u32) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !templum_terminal::tree(&[pid]).is_empty() {
        assert!(std::time::Instant::now() < deadline, "{pid} still runs");
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// The canvas saved in `data` for workspace `id`: how many blocks.
fn saved_blocks(data: &std::path::Path, id: &str) -> usize {
    match crate::session::load(data, id) {
        crate::session::Loaded::Saved(saved) => saved.items.len(),
        other => panic!("{id}: {other:?}"),
    }
}

/// A second workspace, "w2", with a terminal; Main shown again. Gives the terminal's id and
/// its shell's pid.
fn second_with_a_terminal(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
) -> (BlockId, u32) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.new_space(window, cx);
            ws.open(Rect::new(0.0, 0.0, 600.0, 400.0), window, cx);
            let (id, pane) = ws.space.terminals().next().unwrap();
            let pid = pane.term.processes().shell.unwrap();
            ws.switch_to(0, window, cx);
            (id, pid)
        })
    })
    .unwrap()
}

#[gpui_kit::test]
fn stopping_a_workspace_ends_its_programs_and_keeps_its_canvas(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    let (_, pid) = second_with_a_terminal(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.stop_space("w2", window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        let w2 = &ws.others[0];
        assert!(w2.stopped, "nothing at work: stopped at once");
        assert!(w2.blocks.is_empty() && w2.canvas.blocks().is_empty());
        assert_eq!(ws.counts(w2).line(), "Stopped · 1 terminal");
    });
    ended(pid);
    assert_eq!(saved_blocks(data.path(), "w2"), 1);
    // The next snapshot doesn't write the stopped workspace's empty canvas over its file.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.snapshot(window, cx))
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(saved_blocks(data.path(), "w2"), 1);
    assert!(written_index(data.path()).workspaces[1].stopped);
}

#[gpui_kit::test]
fn a_snapshot_in_flight_during_stop_doesnt_empty_the_file(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    second_with_a_terminal(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            // The snapshot reads w2 with one terminal; it gets a second before Stop.
            ws.snapshot(window, cx);
            ws.switch_to(1, window, cx);
            ws.open(Rect::new(700.0, 0.0, 600.0, 400.0), window, cx);
            ws.switch_to(0, window, cx);
            ws.stop_now("w2", window, cx);
        })
    })
    .unwrap();
    cx.run_until_parked();
    assert_eq!(
        saved_blocks(data.path(), "w2"),
        2,
        "the older snapshot lost"
    );
    ws.read_with(cx, |ws, _| {
        let w2 = &ws.others[0];
        assert!(w2.stopped);
        assert_eq!(w2.saved.as_ref().map(|s| s.items.len()), Some(2));
    });
}

#[gpui_kit::test]
fn a_stopped_workspace_starts_when_opened(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    let (old, _) = second_with_a_terminal(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.stop_now("w2", window, cx);
            ws.switch_to(1, window, cx);
        });
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.space.stopped);
        let (id, pane) = ws.space.terminals().next().expect("its terminal, back");
        assert!(!pane.term.exited());
        assert!(id.0 > old.0, "blocks started again get new ids");
    });
}

#[gpui_kit::test]
fn a_stopped_workspace_stays_stopped_after_a_restart(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    second_with_a_terminal(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.stop_now("w2", window, cx);
            ws.save_canvas_now(cx);
        })
    })
    .unwrap();
    let (again_handle, again) = open_in(cx, None, Some(data.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        let w2 = &ws.others[0];
        assert!(w2.stopped && w2.blocks.is_empty(), "its shell didn't start");
        assert_eq!(w2.saved.as_ref().map(|s| s.items.len()), Some(1));
    });
    cx.update_window(again_handle, |_, window, cx| {
        again.update(cx, |ws, cx| ws.switch_to(1, window, cx))
    })
    .unwrap();
    again.read_with(cx, |ws, _| assert_eq!(ws.space.terminals().count(), 1));
}

#[gpui_kit::test]
fn stopping_the_active_workspace_shows_the_one_before(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    second_with_a_terminal(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.switch_to(1, window, cx);
            ws.stop_now("w2", window, cx);
            assert_eq!((ws.active, ws.space.name.as_str()), (0, "Main"));
            assert!(ws.others[0].stopped);
        })
    })
    .unwrap();
}

/// Four workspaces, Main, w2, w3 and w4, each with a terminal (Main its first run's); Main shown.
fn four(handle: AnyWindowHandle, ws: &Entity<Workspace>, cx: &mut TestAppContext) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            for _ in 0..3 {
                ws.new_space(window, cx);
                ws.open(Rect::new(0.0, 0.0, 600.0, 400.0), window, cx);
            }
            ws.switch_to(0, window, cx);
        })
    })
    .unwrap();
}

/// Stops `id` (nothing at work in it: at once); the workspace shown then, and whether the Home is.
fn stop(
    handle: AnyWindowHandle,
    ws: &Entity<Workspace>,
    cx: &mut TestAppContext,
    id: &str,
) -> (String, bool) {
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.stop_space(id, window, cx);
            assert!(ws.spaces().any(|s| s.id == id && s.stopped), "{id} stopped");
            (ws.space.id.clone(), ws.home_up())
        })
    })
    .unwrap()
}

#[gpui_kit::test]
fn stopping_the_shown_workspace_shows_the_nearest_running_one_above_else_below(
    cx: &mut TestAppContext,
) {
    let (handle, ws) = open(cx);
    four(handle, &ws, cx);
    // The first has none above: the one below.
    assert_eq!(stop(handle, &ws, cx, "default"), ("w2".into(), false));
    // w3 has w2 above it.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(2, window, cx))
    })
    .unwrap();
    assert_eq!(stop(handle, &ws, cx, "w3"), ("w2".into(), false));
    // w4: w3 above is stopped, skipped for w2.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(3, window, cx))
    })
    .unwrap();
    assert_eq!(stop(handle, &ws, cx, "w4"), ("w2".into(), false));
}

#[gpui_kit::test]
fn stopping_the_shown_workspace_skips_stopped_ones_below(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    four(handle, &ws, cx);
    stop(handle, &ws, cx, "w2");
    assert_eq!(stop(handle, &ws, cx, "default"), ("w3".into(), false));
}

#[gpui_kit::test]
fn stopping_the_last_running_workspace_shows_the_home(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    four(handle, &ws, cx);
    for id in ["w2", "w3", "w4"] {
        stop(handle, &ws, cx, id);
    }
    let (held, home) = stop(handle, &ws, cx, "default");
    assert!(home, "none runs: the Home, not a stopped one started");
    assert_eq!(held, "default", "the one stopped, held under it");
    ws.read_with(cx, |ws, cx| {
        assert!(ws.spaces().all(|s| s.stopped && s.blocks.is_empty()));
        assert!(
            ws.order()
                .iter()
                .all(|s| ws.counts(s).line() == "Stopped · 1 terminal")
        );
        let state = ws.api_state(cx);
        assert_eq!(state.app.shown, None);
        assert!(state.workspaces.iter().all(|w| w.stopped && !w.shown));
    });
    // Any card starts its workspace.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(2, window, cx));
        window.render_frame(cx);
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.home_up() && ws.space.id == "w3" && !ws.space.stopped);
        assert_eq!(ws.space.terminals().count(), 1, "its terminal, back");
    });
}

#[gpui_kit::test]
fn every_workspace_stopped_stays_stopped_after_a_restart(cx: &mut TestAppContext) {
    let data = tempfile::tempdir().unwrap();
    let (handle, ws) = open_in(cx, None, Some(data.path().to_path_buf()));
    second_with_a_terminal(handle, &ws, cx);
    for id in ["w2", "default"] {
        stop(handle, &ws, cx, id);
    }
    ws.update(cx, |ws, cx| ws.save_canvas_now(cx));
    let (_, again) = open_welcomed(cx, Some(quiet()), None, Some(data.path().to_path_buf()));
    again.read_with(cx, |ws, _| {
        assert!(ws.home_up());
        assert_eq!(
            ws.space.id, "default",
            "the one shown last, held under the Home"
        );
        assert!(
            ws.spaces().all(|s| s.stopped && s.blocks.is_empty()),
            "none started"
        );
    });
}

#[gpui_kit::test]
fn the_only_workspace_can_be_stopped_landing_on_the_home(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    ws.read_with(cx, |ws, _| {
        assert_eq!(ws.space.terminals().count(), 1, "the first run's");
        assert!(ws.can_stop(&ws.space), "the menu's Stop");
    });
    assert_eq!(stop(handle, &ws, cx, "default"), ("default".into(), true));
    // From the Home's card it starts again.
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.switch_to(0, window, cx))
    })
    .unwrap();
    ws.read_with(cx, |ws, _| {
        assert!(!ws.home_up() && !ws.space.stopped);
        assert_eq!(ws.space.terminals().count(), 1);
    });
}

#[gpui_kit::test]
fn stopping_the_workspace_held_under_the_home_keeps_the_home(cx: &mut TestAppContext) {
    let (handle, ws) = open(cx);
    four(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| ws.show_home(window, cx))
    })
    .unwrap();
    assert_eq!(stop(handle, &ws, cx, "default"), ("default".into(), true));
    assert_eq!(
        stop(handle, &ws, cx, "w3"),
        ("default".into(), true),
        "a hidden one"
    );
}

#[gpui_kit::test]
fn stopping_asks_first_when_a_terminal_is_at_work(cx: &mut TestAppContext) {
    use gpui_kit::component::WindowExt as _;
    no_motion(cx);
    let (handle, ws) = open(cx);
    let (id, _) = second_with_a_terminal(handle, &ws, cx);
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            let pane = ws.others[0].terminal_mut(id).unwrap();
            pane.seen.agent = Some("claude".into());
            ws.stop_space("w2", window, cx);
        });
        window.render_frame(cx);
        assert!(window.has_active_dialog(cx), "Claude Code is running in it");
        assert!(!ws.read(cx).others[0].stopped);
        window.within("dialog").click("ok", cx);
        window.render_frame(cx);
        assert!(ws.read(cx).others[0].stopped);
    })
    .unwrap();
}

#[gpui_kit::test]
fn a_new_terminal_starts_in_the_workspace_folder_when_nothing_focused_has_one(
    cx: &mut TestAppContext,
) {
    let project = tempfile::tempdir().unwrap();
    let (handle, ws) = open(cx);
    let id = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.new_space(window, cx);
                let folder = Some(project.path().to_path_buf());
                ws.edit_space("w2", "Project", Look::default(), folder, window, cx)
                    .unwrap();
                ws.new_terminal(window, cx);
                ws.space.focused.unwrap()
            })
        })
        .unwrap();
    assert_eq!(
        folder_of(&ws, id, cx),
        project.path().canonicalize().unwrap()
    );
}

/// Whether the process `pid` still runs.
#[cfg(unix)]
fn runs(pid: u32) -> bool {
    !templum_terminal::tree(&[pid]).is_empty()
}

/// Stop and Delete end what ignores the hangup: two seconds later, what is left is killed.
#[cfg(unix)]
#[gpui_kit::test]
fn stopping_and_deleting_end_programs_that_ignore_the_hangup(cx: &mut TestAppContext) {
    use std::time::{Duration, Instant};
    let stubborn = (
        "sh".to_string(),
        vec![
            "-c".to_string(),
            "trap '' HUP; sleep 1000 & sleep 1000".to_string(),
        ],
    );
    let (handle, ws) = open_as(cx, Some(stubborn), None, None);
    let main_shell = ws.read_with(cx, |ws, _| {
        ws.space
            .terminals()
            .next()
            .unwrap()
            .1
            .term
            .processes()
            .shell
            .unwrap()
    });
    let (_, w2) = second_with_a_terminal(handle, &ws, cx);
    let w3 = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.new_space(window, cx);
                ws.open(Rect::new(0.0, 0.0, 600.0, 400.0), window, cx);
                let shell = ws
                    .space
                    .terminals()
                    .next()
                    .unwrap()
                    .1
                    .term
                    .processes()
                    .shell;
                ws.switch_to(0, window, cx);
                shell.unwrap()
            })
        })
        .unwrap();
    // Each shell's two sleeps started: the 2 shells and their 4 sleeps. A sleep started after
    // Stop reads the tree isn't killed, and keeps its shell's session running.
    let deadline = Instant::now() + Duration::from_secs(10);
    let tree = loop {
        let tree = templum_terminal::tree(&[w2, w3]);
        if tree.len() >= 6 {
            break tree;
        }
        assert!(
            Instant::now() < deadline,
            "the sleeps never started: {tree:?}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    cx.update_window(handle, |_, window, cx| {
        ws.update(cx, |ws, cx| {
            ws.stop_now("w2", window, cx);
            ws.delete_space("w3", window, cx);
        })
    })
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(tree.iter().any(|r| runs(r.pid)), "the hangup was ignored");
    cx.executor().advance_clock(Duration::from_secs(3));
    cx.run_until_parked();
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(left) = tree.iter().find(|r| runs(r.pid)) {
        assert!(Instant::now() < deadline, "{left:?} still runs");
        std::thread::sleep(Duration::from_millis(50));
    }
    // Main's own stubborn terminal doesn't outlive the test.
    templum_terminal::kill_left(&templum_terminal::tree(&[main_shell]));
}

#[gpui_kit::test]
fn the_workspace_folder_comes_before_the_configured_one(cx: &mut TestAppContext) {
    let config = tempfile::tempdir().unwrap();
    let configured = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    // A TOML literal string: Windows backslashes need no escaping.
    write_config(
        config.path(),
        &format!(
            "[shell]\nworking_directory = '{}'\n",
            configured.path().display()
        ),
    );
    let (handle, ws) = open_with(cx, Some(config.path().to_path_buf()));
    let id = cx
        .update_window(handle, |_, window, cx| {
            ws.update(cx, |ws, cx| {
                ws.new_space(window, cx);
                let folder = Some(project.path().to_path_buf());
                ws.edit_space("w2", "Project", Look::default(), folder, window, cx)
                    .unwrap();
                ws.new_terminal(window, cx);
                ws.space.focused.unwrap()
            })
        })
        .unwrap();
    assert_eq!(
        folder_of(&ws, id, cx),
        project.path().canonicalize().unwrap()
    );
}
